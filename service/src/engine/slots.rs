//! A torrent's open files ([`Slots`], #374): one object per torrent, outside librqbit. Every storage
//! librqbit gets of the torrent has a [`Handle`] of them, and only the last one it got has the files:
//!
//! - [`Slots::open`] is a storage's `init`: the files as just opened, each anew, whatever was open
//!   before, every one [closed](Opened::Closed): read only, so a check changes nothing on disk. What an
//!   older storage had open is closed, and its handle is over.
//! - [`Slots::grant`] opens one for writing, as the torrent is let run: if it's still the file the
//!   check read (or, not there then, it's made now, `O_EXCL`), no longer than the torrent's and, while
//!   incomplete, no other file's too (a hard link). What was kept aside of it goes into it first.
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
    /// May be written.
    Open(std::fs::File),
    /// Read only: what's written into it is kept aside (`parts.rs`). `None`: not on disk at `init`,
    /// let go of since, or padding.
    Closed(Option<std::fs::File>),
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

    /// Opens file `index` for writing, as `want` says it may be ([`Opened::Closed`] to
    /// [`Opened::Open`]); one open already is looked at again. Waits for the file's reads and writes
    /// under way: on a blocking task, never the actor's.
    pub fn grant(&self, index: usize, want: &Want) -> Result<(), Refused> {
        use std::os::unix::fs::MetadataExt;
        let Some(opening) = self.lock().clone() else { return Err(Refused::Changed) };
        let (Some(Some(path)), Some(slot)) = (opening.paths.get(index), opening.files.get(index)) else { return Ok(()) };
        // One open already is only looked at, beside the reads under way.
        if let Opened::Open(file) = &*waiting(|| slot.try_read())? {
            return if is_at(file, path) { Ok(()) } else { Err(there(path)) };
        }
        let mut slot = waiting(|| slot.try_write())?;
        let file = match std::mem::replace(&mut *slot, Opened::Closed(None)) {
            Opened::Closed(None) => match made(path, want) {
                Ok(file) => file,
                Err(e) if matches!(e.raw_os_error(), Some(libc::EEXIST | libc::ELOOP)) => return Err(there(path)),
                Err(e) => return Err(Refused::Failed(format!("making {path:?}: {e}"))),
            },
            Opened::Open(file) => {
                let at = is_at(&file, path);
                *slot = Opened::Open(file);
                return if at { Ok(()) } else { Err(there(path)) };
            }
            Opened::Closed(Some(file)) if !is_at(&file, path) => {
                *slot = Opened::Closed(Some(file));
                return Err(there(path));
            }
            Opened::Closed(Some(file)) => file,
            Opened::HandedOn => unreachable!("never in a slot"),
        };
        let fail = |e: std::io::Error| Refused::Failed(format!("{path:?}: {e}"));
        let meta = file.metadata().map_err(fail)?;
        let shared = meta.nlink() > 1;
        if meta.len() > want.size || (shared && !want.complete) {
            *slot = Opened::Closed(Some(file));
            return Err(Refused::NotItsOwn);
        }
        // Complete and another file's too: nothing is to be written, so it stays read only.
        if shared {
            *slot = Opened::Closed(Some(file));
            return Ok(());
        }
        let opened = self.parts.write_into(index, &file).and_then(|_| super::trashed::grow(&file, want.size));
        match opened {
            Ok(()) => {
                *slot = Opened::Open(file);
                Ok(())
            }
            Err(e) => {
                *slot = Opened::Closed(Some(file));
                Err(fail(e))
            }
        }
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

/// What a file must be to be [granted](Slots::grant).
pub(super) struct Want {
    /// Its length in the torrent.
    pub size: u64,
    /// Whether librqbit has all of it.
    pub complete: bool,
    /// The torrent's destination: a file made is made with its folders from there down, never it.
    pub destination: PathBuf,
}

/// Why a file isn't [granted](Slots::grant).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Refused {
    /// Another file is at its path than the one the check read (or none, or one where none was): the
    /// check is due again.
    Changed,
    /// What's at its path isn't the torrent's to write: longer than its, a hard link while incomplete,
    /// or something other than a file. A size check finds it Missing files.
    NotItsOwn,
    /// It couldn't be opened, made or written into.
    Failed(String),
    /// It's being read or written, as on a disk that doesn't answer: asked again later.
    Busy,
}

/// How long a grant waits for a file being read or written.
const BUSY_FOR: std::time::Duration = std::time::Duration::from_secs(2);

/// The lock `lock` takes, once it's free within [`BUSY_FOR`].
fn waiting<G>(lock: impl Fn() -> std::sync::TryLockResult<G>) -> Result<G, Refused> {
    let until = std::time::Instant::now() + BUSY_FOR;
    loop {
        match lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(p)) => return Ok(p.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(std::sync::TryLockError::WouldBlock) => return Err(Refused::Busy),
        }
    }
}

/// File `path`, not there when the check read the torrent, made now for writing: never over what's
/// there since, nor through a link. Its folders too, from `want.destination` down.
fn made(path: &Path, want: &Want) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    super::trashed::make_folders(&want.destination, path.parent().unwrap_or(path)).map_err(std::io::Error::other)?;
    std::fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// Whether `file` is what's at `path` now: the same file, not a link to it.
fn is_at(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::symlink_metadata(path)) {
        (Ok(open), Ok(there)) => there.is_file() && (open.dev(), open.ino()) == (there.dev(), there.ino()),
        _ => false,
    }
}

/// Why a file that isn't the one at `path` is refused: another file there is checked; nothing, or
/// something else, is the size check's.
fn there(path: &Path) -> Refused {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Refused::Changed,
        Ok(_) => Refused::NotItsOwn,
        Err(_) => Refused::Changed,
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
            let opened = |path: &Path| Opened::Open(std::fs::File::options().read(true).write(true).open(path).unwrap());
            slots.open(["a", "b"].map(|name| (Some(self.path(name)), opened(&self.path(name)))).into())
        }
    }

    /// Writes `byte` at the start of file `index`: whether the handle has the file.
    fn write(handle: &Handle, index: usize, byte: u8) -> bool {
        let wrote = handle.read(index, |_, opened| match opened {
            Opened::Open(file) => Ok(Some(file.write_all_at(&[byte], 0).map(|()| true)?)),
            Opened::Closed(_) => Ok(Some(false)),
            Opened::HandedOn => Ok(None),
        });
        let changes = handle.change(index, |_, _| Ok(())).is_ok();
        let wrote = wrote.unwrap();
        assert_eq!(wrote.is_some(), changes, "had to change as it is to read");
        wrote == Some(true)
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
                        let Opened::Open(file) = opened else { panic!("handed on under a write") };
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

    /// Files 0 and 1 as `init` opens them now: closed, `a` there and `b` not (both of 8 bytes).
    fn closed(dir: &Dir, slots: &Arc<Slots>) -> Handle {
        std::fs::remove_file(dir.path("b")).unwrap();
        let a = std::fs::File::options().read(true).write(true).open(dir.path("a")).unwrap();
        slots.open(vec![(Some(dir.path("a")), Opened::Closed(Some(a))), (Some(dir.path("b")), Opened::Closed(None))])
    }

    fn want(dir: &Dir, size: u64, complete: bool) -> Want {
        Want { size, complete, destination: dir.0.path().to_owned() }
    }

    /// Granted, the file the check read may be written, what was kept aside of it in it first; one not
    /// there then is made now, of its full length. Nothing is written before.
    #[test]
    fn a_grant_opens_the_file_the_check_read_and_makes_one_that_was_not_there() {
        let dir = Dir::new();
        let slots = Arc::new(Slots::new(Parts::new(Some(dir.path("parts")))));
        let handle = closed(&dir, &slots);
        assert!(!write(&handle, 0, 1) && !write(&handle, 1, 1), "closed");
        slots.parts().keep(0, 2, &[&[3; 2]], 100).unwrap();
        for index in [0, 1] {
            slots.grant(index, &want(&dir, 8, false)).unwrap();
        }
        assert!(write(&handle, 0, 1) && write(&handle, 1, 1));
        assert_eq!(std::fs::read(dir.path("a")).unwrap(), [1, 0, 3, 3, 0, 0, 0, 0], "with what was kept");
        assert_eq!(std::fs::read(dir.path("b")).unwrap(), [1, 0, 0, 0, 0, 0, 0, 0], "made, of its length");
        assert!(!slots.parts().has(0));
        slots.grant(0, &want(&dir, 8, false)).unwrap();
    }

    /// The grant refuses what isn't the file the check read, or isn't the torrent's to write.
    #[test]
    fn a_grant_refuses_another_file_a_longer_one_a_hard_link_and_a_link() {
        let refused = |change: &dyn Fn(&Dir), index: usize, complete: bool| {
            let dir = Dir::new();
            let slots = Arc::new(Slots::new(Parts::new(Some(dir.path("parts")))));
            let handle = closed(&dir, &slots);
            change(&dir);
            let granted = slots.grant(index, &want(&dir, 8, complete));
            assert!(!write(&handle, index, 1) || granted.is_ok(), "written though refused");
            (granted, std::fs::read(dir.path("a")).ok())
        };
        let replaced = |d: &Dir| {
            std::fs::write(d.path("new"), [5; 8]).unwrap();
            std::fs::rename(d.path("new"), d.path("a")).unwrap();
        };
        assert_eq!(refused(&replaced, 0, false), (Err(Refused::Changed), Some(vec![5; 8])), "another file at its path");
        let gone = |d: &Dir| std::fs::rename(d.path("a"), d.path("moved")).unwrap();
        assert_eq!(refused(&gone, 0, false).0, Err(Refused::Changed), "renamed away, a folder above it say");
        let put = |d: &Dir| std::fs::write(d.path("b"), [5; 8]).unwrap();
        assert_eq!(refused(&put, 1, false).0, Err(Refused::Changed), "one put where none was");
        let longer = |d: &Dir| std::fs::OpenOptions::new().append(true).open(d.path("a")).unwrap().write_all_at(&[9], 8).unwrap();
        assert_eq!(refused(&longer, 0, false).0, Err(Refused::NotItsOwn), "longer");
        let linked = |d: &Dir| std::fs::hard_link(d.path("a"), d.path("other")).unwrap();
        assert_eq!(refused(&linked, 0, false), (Err(Refused::NotItsOwn), Some(vec![0; 8])), "a hard link, incomplete");
        assert_eq!(refused(&linked, 0, true), (Ok(()), Some(vec![0; 8])), "complete: read only, not refused");
        let link = |d: &Dir| std::os::unix::fs::symlink(d.path("a"), d.path("b")).unwrap();
        assert_eq!(refused(&link, 1, false).0, Err(Refused::NotItsOwn), "a link where none was");
    }

    #[test]
    fn a_handle_of_nothing_has_no_files() {
        let handle = Handle::default();
        assert!(handle.read(0, |_, _| Ok(())).is_err());
        assert!(handle.hand_on().change(0, |_, _| Ok(())).is_err());
    }
}
