//! A torrent's open files ([`Slots`], #374): one object per torrent, outside librqbit. Every storage
//! librqbit gets of the torrent has a [`Handle`] of them, and only the last one it got has the files:
//!
//! - [`Slots::open`] is a storage's `init`: the files as just opened, each anew, whatever was open
//!   before. What an older storage had open is closed, and its handle is over.
//! - [`Handle::hand_on`] is librqbit's `take` (as a check ends, at a pause): the files stay open, as
//!   they are, for the new handle. It waits for each read and write under way; none begins after it
//!   through the old handle, which finds every file [`HandedOn`](Opened::HandedOn).
//! - The handle that has the files closes them as it drops: librqbit let the torrent go (a Remove, a
//!   Move, a drive that went, an Error), and nothing of Phantom's holds a drive that's ejected. A
//!   handle that was handed on closes nothing, and keeps nothing open however long librqbit keeps it.
//!
//! Locks: a file's own is held by each read and write of it, a disk that doesn't answer too, and so
//! is [waited for](Handle::hand_on) only where librqbit's `take` always did. [`Slots::now`] is never
//! held while a file is read, written, opened or closed, so neither [`Slots::each`] nor a handle's
//! drop waits for a disk that another thread waits for. No lock is taken under it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

use anyhow::{Context, Result, bail};

use super::parts::Parts;

/// A torrent's open files, outliving the storages librqbit gets of it.
#[derive(Default)]
pub(super) struct Slots {
    /// What's open now; `None` while librqbit has no storage of the torrent.
    now: Mutex<Option<Arc<Opening>>>,
    /// What's kept aside of its files: outlives every storage, as it must a re-add.
    parts: Parts,
}

/// What a storage's `init` opened.
struct Opening {
    paths: Paths,
    /// Each file, by index.
    files: Vec<RwLock<Opened>>,
    /// Which handle has them ([`Handle::number`]).
    holder: AtomicU64,
}

/// Each file's path, by index; `None`: padding, never opened.
type Paths = Arc<Vec<Option<PathBuf>>>;

pub(super) enum Opened {
    File(std::fs::File),
    /// Not on disk at `init`, let go of since, or padding: made when written.
    NotThere,
    /// What a handle finds that no longer has the files; never in a slot.
    HandedOn,
}

impl Slots {
    pub fn new(parts: Parts) -> Self {
        Self { now: Mutex::default(), parts }
    }

    /// What's kept aside of its files.
    pub fn parts(&self) -> &Parts {
        &self.parts
    }

    /// The files just opened, for the storage that opened them.
    pub fn open(self: &Arc<Self>, files: Vec<(Option<PathBuf>, Opened)>) -> Handle {
        let (paths, files): (Vec<_>, Vec<_>) = files.into_iter().map(|(path, file)| (path, RwLock::new(file))).unzip();
        let paths = Arc::new(paths);
        let opening = Arc::new(Opening { paths: paths.clone(), files, holder: AtomicU64::new(0) });
        let handle = Handle { slots: self.clone(), paths, opening: Arc::downgrade(&opening), number: 0 };
        let before = self.lock().replace(opening);
        // Closed with `now` free.
        drop(before);
        handle
    }

    /// `f` of each file, by index, with its path: one being read or written now is locked.
    pub fn each(&self, mut f: impl FnMut(usize, Option<&Path>, &RwLock<Opened>)) {
        let Some(opening) = self.lock().clone() else { return };
        for (index, (path, file)) in opening.paths.iter().zip(&opening.files).enumerate() {
            f(index, path.as_deref(), file);
        }
    }

    /// Runs `f` while every file is in use, as by a read or a write of librqbit's.
    #[cfg(test)]
    pub fn in_use(&self, f: impl FnOnce()) {
        let opening = self.lock().clone();
        let held: Vec<_> = opening.iter().flat_map(|o| &o.files).map(|file| file.read().unwrap()).collect();
        f();
        drop(held);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<Opening>>> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A storage's handle of its torrent's files; the default one is of none.
#[derive(Default)]
pub(super) struct Handle {
    slots: Arc<Slots>,
    paths: Paths,
    /// Weak: a handle that was handed on keeps no file open.
    opening: Weak<Opening>,
    number: u64,
}

impl Handle {
    /// `f` of file `index`, which nothing closes, makes or hands on meanwhile: its path, and how it's
    /// open.
    pub fn read<T>(&self, index: usize, f: impl FnOnce(Option<&Path>, &Opened) -> Result<T>) -> Result<T> {
        let path = self.path(index)?;
        if let Some(opening) = self.opening() {
            let file = opening.files[index].read().unwrap();
            // Asked again with the file locked: a hand-on since waits for this one, or is seen here.
            if opening.is_held_by(self) {
                return f(path, &file);
            }
        }
        f(path, &Opened::HandedOn)
    }

    /// `f` of file `index`, which nothing else reads or writes meanwhile.
    pub fn change<T>(&self, index: usize, f: impl FnOnce(Option<&Path>, &mut Opened) -> Result<T>) -> Result<T> {
        let path = self.path(index)?;
        if let Some(opening) = self.opening() {
            let mut file = opening.files[index].write().unwrap();
            if opening.is_held_by(self) {
                return f(path, &mut file);
            }
        }
        bail!("file {index} was handed on")
    }

    /// A new handle, which has the files from now on. One of a handle that no longer has them has none
    /// either: a check that ends after its torrent was taken out takes nothing from the storage after.
    pub fn hand_on(&self) -> Handle {
        let next = |opening, number| Handle { slots: self.slots.clone(), paths: self.paths.clone(), opening, number };
        let Some(opening) = self.opening() else { return next(Weak::new(), 0) };
        let number = self.number + 1;
        if opening.holder.compare_exchange(self.number, number, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return next(Weak::new(), 0);
        }
        let next = next(Arc::downgrade(&opening), number);
        // Each read and write that found this handle having the files holds its file's lock.
        for file in &opening.files {
            drop(file.write().unwrap());
        }
        next
    }

    fn path(&self, index: usize) -> Result<Option<&Path>> {
        Ok(self.paths.get(index).with_context(|| format!("no file {index}"))?.as_deref())
    }

    /// What's open, while this handle has it.
    fn opening(&self) -> Option<Arc<Opening>> {
        self.opening.upgrade().filter(|opening| opening.is_held_by(self))
    }
}

impl Opening {
    fn is_held_by(&self, handle: &Handle) -> bool {
        self.holder.load(Ordering::SeqCst) == handle.number
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let Some(opening) = self.opening() else { return };
        let mut now = self.slots.lock();
        let closed = now.take_if(|now| Arc::ptr_eq(now, &opening));
        drop(now);
        // Closed with `now` free: `opening` is the last of it, but for a look under way, which closes
        // it as it ends.
        drop((closed, opening));
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::FileExt;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::super::trashed::open_on;
    use super::*;

    struct Dir(tempfile::TempDir);

    impl Dir {
        fn new() -> Self {
            let dir = Self(tempfile::tempdir().unwrap());
            for name in ["a", "b"] {
                std::fs::write(dir.path(name), [0; 8]).unwrap();
            }
            dir
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.path().join(name)
        }

        /// Opened as a storage's `init` does: files 0 and 1.
        fn open(&self, slots: &Arc<Slots>) -> Handle {
            let opened = |path: &Path| Opened::File(std::fs::File::options().read(true).write(true).open(path).unwrap());
            slots.open(["a", "b"].map(|name| (Some(self.path(name)), opened(&self.path(name)))).into())
        }
    }

    /// Writes `byte` at the start of file `index`: whether the handle has the file.
    fn write(handle: &Handle, index: usize, byte: u8) -> bool {
        let wrote = handle.read(index, |_, opened| match opened {
            Opened::File(file) => Ok(file.write_all_at(&[byte], 0).map(|()| true)?),
            Opened::NotThere | Opened::HandedOn => Ok(false),
        });
        let changes = handle.change(index, |_, _| Ok(())).is_ok();
        let wrote = wrote.unwrap();
        assert_eq!(wrote, changes, "had to change as it is to read");
        wrote
    }

    /// librqbit's `take` as a check ends or at a pause: a write under way, in whichever file, ends
    /// first, as it would have; nothing is written through the old handle from then on.
    #[test]
    fn handing_on_waits_for_the_write_under_way_and_ends_the_old_handle() {
        for index in [0, 1] {
            let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
            let path = dir.path(["a", "b"][index]);
            let old = dir.open(&slots);
            let (begun_tx, begun) = mpsc::channel();
            let (go, go_rx) = mpsc::channel::<()>();
            let (next_tx, next) = mpsc::channel();
            std::thread::scope(|s| {
                let old = &old;
                s.spawn(move || {
                    old.read(index, |_, opened| {
                        begun_tx.send(()).unwrap();
                        go_rx.recv().unwrap();
                        let Opened::File(file) = opened else { panic!("handed on under a write") };
                        Ok(file.write_all_at(&[7], 0)?)
                    })
                    .unwrap();
                });
                begun.recv().unwrap();
                s.spawn(move || next_tx.send(old.hand_on()).unwrap());
                let early = next.recv_timeout(Duration::from_millis(100));
                // A look waits for neither.
                let (looked_tx, looked) = mpsc::channel();
                s.spawn(move || {
                    let mut busy = Vec::new();
                    slots.each(|i, _, file| busy.extend(file.try_read().is_err().then_some(i)));
                    looked_tx.send(busy).unwrap();
                });
                let looked = looked.recv_timeout(Duration::from_secs(10));
                go.send(()).unwrap();
                assert!(early.is_err(), "handed on with a write of file {index} under way");
                assert_eq!(looked.expect("a look waits for nothing"), [index], "waited for");
            });
            let new = next.recv().unwrap();
            assert_eq!(std::fs::read(&path).unwrap()[0], 7, "the write under way ended as it would have");
            assert!(!write(&old, index, 8) && write(&new, index, 9), "only the new handle has the files");
            assert_eq!(std::fs::read(&path).unwrap()[0], 9);
        }
    }

    /// One that waits for its file as its handle is handed on, behind another under way: it's late too.
    #[test]
    fn a_write_still_waiting_for_its_file_at_a_hand_on_is_refused() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let old = dir.open(&slots);
        let (begun_tx, begun) = mpsc::channel();
        let (go, go_rx) = mpsc::channel::<()>();
        std::thread::scope(|s| {
            let old = &old;
            s.spawn(move || {
                old.change(0, |_, _| {
                    begun_tx.send(()).unwrap();
                    Ok(go_rx.recv().unwrap())
                })
                .unwrap()
            });
            begun.recv().unwrap();
            let reading = s.spawn(|| old.read(0, |_, opened| Ok(matches!(opened, Opened::HandedOn))).unwrap());
            let changing = s.spawn(|| old.change(0, |_, _| Ok(())).is_err());
            // Both wait for the file.
            std::thread::sleep(Duration::from_millis(200));
            let next = s.spawn(|| old.hand_on());
            std::thread::sleep(Duration::from_millis(100));
            go.send(()).unwrap();
            assert!(reading.join().unwrap() && changing.join().unwrap(), "late");
            assert!(write(&next.join().unwrap(), 0, 1));
        });
    }

    /// The same files stay open across a hand-on, and the handle that has them closes them, however
    /// long librqbit keeps one handed on: a torrent taken out leaves nothing open on its drive.
    #[test]
    fn files_stay_open_across_a_hand_on_and_close_with_the_handle_that_has_them() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let open = open_on(&dir.path("a"));
        let first = dir.open(&slots);
        let second = first.hand_on();
        assert_eq!(open(), 1, "neither closed nor opened again");
        let third = second.hand_on();
        drop(second);
        assert_eq!(open(), 1);
        assert!(write(&third, 0, 1));
        drop(third);
        assert_eq!(open(), 0, "closed, with the first handle still there");
        assert!(!write(&first, 0, 2));
        let mut files = 0;
        slots.each(|_, _, _| files += 1);
        assert_eq!(files, 0);
    }

    /// The storage a check hands on as it ends after its torrent was taken out and added again (both
    /// `take` it): it takes nothing from the new one, nor closes anything of it as it drops.
    #[test]
    fn an_old_handle_takes_nothing_from_a_newer_one() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let check = dir.open(&slots);
        let taken_out = check.hand_on();
        let late = check.hand_on();
        assert!(write(&taken_out, 0, 1) && !write(&late, 0, 2), "handed on once");
        drop(late);
        assert!(write(&taken_out, 0, 1), "nothing closed");
        drop(taken_out);

        let again = dir.open(&slots);
        let later = check.hand_on();
        assert!(!write(&check, 0, 3) && !write(&later, 0, 4) && write(&again, 0, 5));
        drop((check, later));
        assert!(write(&again, 0, 6), "nothing closed");
    }

    /// A check's end and a Remove at once, each taking the same storage: one of them has the files.
    #[test]
    fn of_two_hand_ons_at_once_one_has_the_files() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        for _ in 0..2000 {
            let old = dir.open(&slots);
            let at_once = std::sync::Barrier::new(2);
            let hand_on = || {
                at_once.wait();
                old.hand_on()
            };
            let both = std::thread::scope(|s| [s.spawn(hand_on), s.spawn(hand_on)].map(|t| t.join().unwrap()));
            let has = both.each_ref().map(|handle| [0, 1].map(|index| write(handle, index, 1)));
            assert!(has == [[true; 2], [false; 2]] || has == [[false; 2], [true; 2]], "{has:?}");
        }
    }

    /// A storage's `init` opens each file anew: what's at its path then, not what was open before.
    #[test]
    fn an_init_opens_the_files_anew() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let before = dir.open(&slots);
        let was_open = open_on(&dir.path("a"));
        std::fs::write(dir.path("new"), [5; 8]).unwrap();
        std::fs::rename(dir.path("new"), dir.path("a")).unwrap();
        // Handed on, it's still the file it opened: one put at its path since isn't written.
        let paused = before.hand_on();
        assert!(write(&paused, 0, 9));
        assert_eq!(std::fs::read(dir.path("a")).unwrap(), [5; 8], "the person's file, as it was");
        let after = dir.open(&slots);
        assert_eq!(was_open(), 0, "the one before is closed");
        assert!(!write(&paused, 0, 9) && write(&after, 0, 6));
        assert_eq!(std::fs::read(dir.path("a")).unwrap()[0], 6);
    }

    /// A handle drops while a look has a file locked, on a disk that doesn't answer, say: it doesn't
    /// wait for it, and the look closes the files as it ends.
    #[test]
    fn a_handle_drops_without_waiting_for_a_look() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let open = open_on(&dir.path("a"));
        let handle = dir.open(&slots);
        let (looking_tx, looking) = mpsc::channel();
        let (go, go_rx) = mpsc::channel::<()>();
        let (dropped_tx, dropped) = mpsc::channel();
        std::thread::scope(|s| {
            let looked = &slots;
            s.spawn(move || {
                looked.each(|index, _, file| {
                    let _looked_at = file.try_read().unwrap();
                    if index == 0 {
                        looking_tx.send(()).unwrap();
                        go_rx.recv().unwrap();
                    }
                })
            });
            looking.recv().unwrap();
            s.spawn(move || {
                drop(handle);
                dropped_tx.send(()).unwrap();
            });
            let dropped = dropped.recv_timeout(Duration::from_secs(10));
            let meanwhile = open();
            // Nor does the next add.
            let again = dir.open(&slots);
            go.send(()).unwrap();
            assert!(dropped.is_ok(), "waited for the look");
            assert_eq!(meanwhile, 1, "the look has it");
            assert!(write(&again, 0, 1));
        });
        assert_eq!(open(), 0, "closed by then");
    }

    /// A handle from before an `init` drops while a look still keeps what it had open: it closes nothing
    /// of the newer one.
    #[test]
    fn an_old_handle_that_drops_under_a_look_closes_nothing_of_a_newer_one() {
        let (dir, slots) = (Dir::new(), Arc::new(Slots::default()));
        let mut old = Some(dir.open(&slots));
        let mut again = None;
        slots.each(|_, _, _| {
            again.get_or_insert_with(|| dir.open(&slots));
            drop(old.take());
        });
        assert!(write(&again.unwrap(), 0, 1), "still open");
    }

    #[test]
    fn a_handle_of_nothing_has_no_files() {
        let handle = Handle::default();
        assert!(handle.read(0, |_, _| Ok(())).is_err());
        assert!(handle.hand_on().change(0, |_, _| Ok(())).is_err());
    }
}
