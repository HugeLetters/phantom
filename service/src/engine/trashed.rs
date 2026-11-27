//! librqbit's storage of every torrent ([`Files`], #374), and a Move to Trash's hold on it (#151,
//! [ADR 0014]).
//!
//! - A Move to Trash [takes](Trashed::take) each file with librqbit's changes to the torrent's files
//!   held still around it, and closes it as it goes: librqbit's storage has nothing of it open once
//!   it's in the Trash, so nothing reads it, writes it or seeds it from there, and whatever was kept
//!   aside of it goes. The actor then lets go of its pieces (`machine::letgo`). Its undo puts it
//!   back and checks the torrent, which reads it at its path.
//! - A file deleted by hand while librqbit has it open is [closed](Trashed::look) off the actor, so
//!   its space is freed (#244).
//!
//! [ADR 0014]: ../../../docs/adr/0014-surprises-stop-check-decides.md

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use librqbit::storage::TorrentStorage;
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use tracing::warn;

use super::parts::Parts;
use super::slots::{Granting, Handle, Opened, Refused, Slots};
use super::trash::{self, Carried, Plan, Trash};
use super::undo::machine;

/// The open files of every torrent, shared by the actor and every storage librqbit gets.
#[derive(Clone, Default)]
pub struct Trashed {
    torrents: Arc<Mutex<HashMap<String, Arc<Torrent>>>>,
    /// Where what librqbit writes into a file it may not is kept (`parts::dir`), so it outlasts a
    /// restart: none in a test that doesn't say, which keeps nothing aside.
    parts: Option<PathBuf>,
    #[cfg(test)]
    hook: Option<Hook>,
    #[cfg(test)]
    reads_of: Option<ReadsOf>,
}

/// Where a test holds librqbit's storage: as it reads a file (a check does, piece by piece), or
/// with that file in hand, changes one (a write, its length set, a removal), or sets a length on a
/// file found in place.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Read,
    Change,
    Length,
    /// A read with its file in hand, as one from a disk that doesn't answer holds it.
    Reading,
    /// A look closing a file deleted by hand ([`Trashed::look`]), off the actor.
    Close,
}

/// A test's hold on librqbit's storage: it returns once what's at `At` may go on.
#[cfg(test)]
pub type Hook = Arc<dyn Fn(At) + Send + Sync>;

/// A test's hold on librqbit's reads of torrent `.0` (its info hash): it returns once the read may go on.
#[cfg(test)]
pub type ReadsOf = Arc<dyn Fn(&str) + Send + Sync>;

/// A torrent's open files, shared by every storage of it.
#[derive(Default)]
struct Torrent {
    /// Held to read by each change librqbit makes to a file while it makes it, to write while a
    /// file moves to the Trash: a move waits for the change under way, and none reaches the file
    /// meanwhile.
    changes: RwLock<()>,
    /// Its open files, which every storage of it has a handle of.
    slots: Arc<Slots>,
}

impl Trashed {
    /// Keeping what librqbit writes into a file it may not in `dir` (`parts::dir`).
    pub fn keeping_parts_in(self, dir: PathBuf) -> Self {
        Self { parts: Some(dir), ..self }
    }

    /// With librqbit's storage holding at `hook`.
    #[cfg(test)]
    pub fn with_hook(self, hook: Hook) -> Self {
        Self { hook: Some(hook), ..self }
    }

    /// With each read librqbit's storage makes held at `reads_of`, given its torrent's info hash.
    #[cfg(test)]
    pub fn with_reads_of(self, reads_of: ReadsOf) -> Self {
        Self { reads_of: Some(reads_of), ..self }
    }

    /// Where librqbit's storage holds now.
    #[cfg(test)]
    pub fn hook(&self) -> Option<Hook> {
        self.hook.clone()
    }

    /// Moves `plan`'s items of torrent `info_hash` to the Trash, its files `files` (by index and
    /// path), telling `went` of each that goes, until `stop`: [`trash::carry_out_until`], each item
    /// moving with librqbit's changes to the torrent's files held still, and closed as it goes, so
    /// nothing of librqbit's reaches it in the Trash. A read under way is waited for. On the blocking
    /// task that moves it, never the actor's.
    pub fn take(
        &self,
        info_hash: &str,
        plan: &Plan,
        files: &[(usize, PathBuf)],
        trash: &Trash,
        went: impl FnMut(&trash::Trashed),
        stop: &AtomicBool,
    ) -> Carried {
        let torrent = self.torrent(info_hash);
        trash::carry_out_holding(plan, trash, went, stop, |take| {
            // Poisoned only by a panic while a move held it: it guards no data, so that means nothing.
            let _held = torrent.changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            let taken = take();
            if let Ok(item) = &taken {
                let moved: BTreeSet<usize> = files.iter().filter(|(_, path)| *path == item.from).map(|(i, _)| *i).collect();
                torrent.close(&moved);
            }
            taken
        })
    }

    /// Runs `put_back` (an undo of a Move to Trash putting its files back, its own new copies `copies`
    /// (by index) going to the Trash first) with librqbit's changes to torrent `info_hash`'s files
    /// held still, then closes each of `copies` that's no longer at its path: nothing of librqbit's
    /// reaches a copy in the Trash. On a blocking task, never the actor's.
    pub fn putting_back<T>(&self, info_hash: &str, copies: &BTreeSet<usize>, put_back: impl FnOnce() -> T) -> T {
        let torrent = self.torrent(info_hash);
        let _held = torrent.changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let put = put_back();
        let mut gone = BTreeSet::new();
        torrent.slots.each(|index, path, file| {
            let opened = file.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            if copies.contains(&index)
                && let Opened::Open(f) | Opened::Closed(Some(f)) = &*opened
                && moved(path, f)
            {
                gone.insert(index);
            }
        });
        torrent.close(&gone);
        put
    }

    /// What's kept aside of each torrent neither in `listed` (by info hash) nor named by the undo
    /// stack `stack` goes: nothing can bring it back with its pieces. At a start, before any torrent
    /// is added to librqbit; then whenever the stack changes.
    pub fn keep_parts_of(&self, stack: &machine::State, listed: &BTreeSet<String>) {
        let kept: BTreeSet<String> = listed.iter().cloned().chain(machine::info_hashes(stack)).collect();
        let hashes: Vec<String> = self.torrents.lock().unwrap().keys().cloned().collect();
        for hash in hashes.iter().filter(|h| !kept.contains(*h)) {
            self.torrent(hash).slots.parts().forget_all();
        }
        let Some(dir) = &self.parts else { return };
        for torrent in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if !kept.contains(&*torrent.file_name().to_string_lossy()) {
                let _ = std::fs::remove_dir_all(torrent.path());
            }
        }
    }

    /// The files librqbit's storage of torrent `info_hash` has open that aren't the ones at their
    /// paths any more (replaced, moved or deleted by hand, or put there where none was), by index: librqbit's pieces of them are
    /// of the files it has open, not of what's there. One being read or written now counts, as
    /// nothing can tell. Each is looked at on disk (`fstat`, `stat`).
    pub fn replaced(&self, info_hash: &str) -> BTreeSet<usize> {
        let mut replaced = BTreeSet::new();
        self.torrent(info_hash).slots.each(|index, path, file| {
            let moved = match file.try_read() {
                Ok(opened) => match &*opened {
                    Opened::Open(file) | Opened::Closed(Some(file)) => moved(path, file),
                    // None was there as it was opened: one there now is another.
                    Opened::Closed(None) => path.is_some_and(|p| std::fs::symlink_metadata(p).is_ok()),
                    Opened::HandedOn => false,
                },
                Err(_) => true,
            };
            if moved {
                replaced.insert(index);
            }
        });
        replaced
    }

    /// The files librqbit's storage of torrent `info_hash` has open, by index, wherever they are now;
    /// one being opened or closed now counts. Only what's in memory, so the actor may ask.
    pub fn open(&self, info_hash: &str) -> BTreeSet<usize> {
        let mut open = BTreeSet::new();
        self.torrent(info_hash).slots.each(|index, _, file| {
            if file.try_read().map_or(true, |opened| matches!(&*opened, Opened::Open(_) | Opened::Closed(Some(_)))) {
                open.insert(index);
            }
        });
        open
    }

    /// Opens the files `files` (by index) of torrent `info_hash` for writing, each as `granting` says it
    /// may be ([`Slots::grant`]): as the actor lets the torrent run. Why each that wasn't. On a
    /// blocking task, never the actor's.
    pub fn grant(&self, info_hash: &str, granting: &Granting, files: &BTreeSet<usize>) -> Vec<Refused> {
        granting.grant(&self.torrent(info_hash).slots, files)
    }

    /// A look at torrent `info_hash` off the actor: closes the files `closable` (those missing, or
    /// let go of) that are gone for good (deleted by hand while librqbit has them open: no name left
    /// anywhere), so their space is freed at once; one moved elsewhere has its space still, and stays
    /// open. Looks at each (`fstat`), so never on the actor; waits for nothing, as [`Trashed::let_go`]:
    /// one librqbit is reading or opening now is left for the next look. Which of them it closed.
    pub fn look(&self, info_hash: &str, closable: &BTreeSet<usize>) -> BTreeSet<usize> {
        let Some(torrent) = self.torrents.lock().unwrap().get(info_hash).cloned() else { return BTreeSet::new() };
        let mut closed = BTreeSet::new();
        torrent.slots.each(|index, _, file| {
            if !closable.contains(&index) {
                return;
            }
            #[cfg(test)]
            if let Some(hook) = &self.hook {
                hook(At::Close);
            }
            if let Ok(mut opened) = file.try_write()
                && unlinked(&opened)
            {
                *opened = Opened::Closed(None);
                closed.insert(index);
            }
        });
        closed.iter().for_each(|index| torrent.slots.parts().forget(*index));
        closed
    }

    /// The files of torrent `info_hash` open for writing, by index: those granted, or made since. Only
    /// what's in memory, so the actor may ask; one being read or written now counts as it was.
    pub fn writable(&self, info_hash: &str) -> BTreeSet<usize> {
        let Some(torrent) = self.torrents.lock().unwrap().get(info_hash).cloned() else { return BTreeSet::new() };
        let mut open = BTreeSet::new();
        torrent.slots.each(|index, _, file| {
            if file.try_read().map_or(true, |opened| matches!(&*opened, Opened::Open(_))) {
                open.insert(index);
            }
        });
        open
    }

    /// Runs `f` while every file of torrent `info_hash` is in use, as by a read or a write of librqbit's.
    #[cfg(test)]
    pub fn in_use(&self, info_hash: &str, f: impl FnOnce()) {
        self.torrent(info_hash).slots.in_use(f);
    }

    /// Closes the files `files` (by index) of torrent `info_hash`, let go of (set Off and gone from
    /// their paths), so librqbit reads none of them any more, and the torrent is to be added back
    /// without their pieces. Touches no disk. Waits for nothing: one librqbit is reading or opening
    /// now is left open, for the next look to find again. Which of them it closed.
    pub fn let_go(&self, info_hash: &str, files: &BTreeSet<usize>) -> BTreeSet<usize> {
        let torrent = self.torrent(info_hash);
        let mut closed = BTreeSet::new();
        torrent.slots.each(|index, _, file| {
            if !files.contains(&index) {
                return;
            }
            // Swapped under the slot's own lock, which every read and write of it holds: the next
            // one finds it not there (and a write makes it again at its path, as for any file set
            // Off that isn't there).
            if let Ok(mut opened) = file.try_write()
                && matches!(*opened, Opened::Open(_) | Opened::Closed(Some(_)))
            {
                *opened = Opened::Closed(None);
                closed.insert(index);
            }
        });
        // Its pieces are let go of: nothing kept of it is read or written in any more.
        closed.iter().for_each(|index| torrent.slots.parts().forget(*index));
        closed
    }

    /// librqbit's storage of torrent `info_hash`'s files, laid out as `layout`, in `destination`.
    pub(super) fn storage(&self, info_hash: &str, layout: Layout, destination: PathBuf) -> Files {
        Files {
            layout,
            destination,
            files: Handle::default(),
            lens: Arc::default(),
            torrent: self.torrent(info_hash),
            #[cfg(test)]
            hook: self.hook.clone(),
            #[cfg(test)]
            reads_of: self.reads_of.clone().map(|r| (r, info_hash.to_owned())),
        }
    }

    /// The torrent's open files, made once, with what was kept of them before a restart.
    fn torrent(&self, info_hash: &str) -> Arc<Torrent> {
        let mut torrents = self.torrents.lock().unwrap();
        if let Some(torrent) = torrents.get(info_hash) {
            return torrent.clone();
        }
        let slots = Arc::new(Slots::new(Parts::new(self.parts_of(info_hash))));
        let torrent = Arc::new(Torrent { slots, ..Torrent::default() });
        torrents.insert(info_hash.to_owned(), torrent.clone());
        torrent
    }

    fn parts_of(&self, info_hash: &str) -> Option<PathBuf> {
        self.parts.as_ref().map(|p| p.join(info_hash))
    }
}

impl Torrent {
    /// Closes the files `files` (by index), just moved to the Trash: waits for a read under way
    /// (a change waits for the move), and what was kept aside of them goes.
    fn close(&self, files: &BTreeSet<usize>) {
        self.slots.each(|index, _, file| {
            if files.contains(&index) {
                *file.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Opened::Closed(None);
                self.slots.parts().forget(index);
            }
        });
    }
}

/// Whether `opened` is a file with no name left anywhere: gone from disk, still open.
fn unlinked(opened: &Opened) -> bool {
    use std::os::unix::fs::MetadataExt;
    match opened {
        Opened::Open(file) | Opened::Closed(Some(file)) => file.metadata().is_ok_and(|m| m.nlink() == 0),
        Opened::Closed(None) | Opened::HandedOn => false,
    }
}

/// Where a torrent's files go and which it gets.
#[derive(Clone)]
pub struct Layout {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    folder: PathBuf,
    /// The files it gets (those Normal); `None`: all of them.
    normal: Option<Arc<BTreeSet<usize>>>,
    /// The name of a torrent of one file's file, renamed (#331): librqbit names it as its info does.
    one_file: Option<PathBuf>,
}

impl Layout {
    pub fn new(folder: PathBuf, normal: Option<&[usize]>) -> Self {
        Self { folder, normal: normal.map(|n| Arc::new(n.iter().copied().collect())), one_file: None }
    }

    /// Its one file under `name` in its folder, whatever librqbit calls it: a torrent of one file
    /// renamed (#331).
    pub fn one_file_as(mut self, name: Option<PathBuf>) -> Self {
        self.one_file = name;
        self
    }

    /// Where librqbit's file at `relative` (in its output folder, as the info names it) is.
    fn at(&self, relative: &Path) -> PathBuf {
        self.folder.join(self.one_file.as_deref().unwrap_or(relative))
    }

    fn is_off(&self, index: usize) -> bool {
        self.normal.as_ref().is_some_and(|normal| !normal.contains(&index))
    }
}

/// librqbit's file storage (`FilesystemStorage`), but that `init` makes nothing and opens every
/// file there [closed](Opened::Closed) (#374): a check reads them and changes nothing, no file or
/// folder made, no length set. The actor [grants](Slots::grant) the files a torrent gets as it lets it
/// run. What's written into a closed file goes aside (`parts.rs`), read over it, and into it at its
/// grant. One that isn't there is made when librqbit writes into it (`O_EXCL`, so never over a file
/// put there since: that write goes aside too), as for a piece a file set Off shares with a file the
/// torrent gets (a stub), with the folders to it, but never the destination (#67). Nothing is ever
/// removed but what librqbit asks.
///
/// A file a Move to Trash took is closed as it goes ([`Trashed::take`]): from then on it's one that
/// isn't there. A write through a storage already taken is refused, as for every other file:
/// librqbit asks for the piece again.
pub(super) struct Files {
    layout: Layout,
    destination: PathBuf,
    /// Its files, as opened; padding (BEP 47) is never opened.
    files: Handle,
    /// Each file's length, by index.
    lens: Arc<Vec<u64>>,
    torrent: Arc<Torrent>,
    #[cfg(test)]
    hook: Option<Hook>,
    /// A test's hold on its reads, with its torrent's info hash.
    #[cfg(test)]
    reads_of: Option<(ReadsOf, String)>,
}

impl Files {
    #[cfg(test)]
    fn at(&self, at: At) {
        if let Some(hook) = &self.hook {
            hook(at);
        }
    }

    /// Writes `bufs`, one after another, at `offset` into file `index`, made first if it isn't
    /// there; aside if it's closed. Writing no bytes makes nothing (#376). Through a storage already
    /// taken, its peer late, it's refused ([`Handle::change`] refuses to make it).
    fn write(&self, index: usize, offset: u64, bufs: &[&[u8]]) -> Result<()> {
        // librqbit writes 0 bytes into the file before a chunk: that makes nothing (#376).
        if bufs.iter().all(|buf| buf.is_empty()) {
            return Ok(());
        }
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        let written = self.files.read(index, |_, opened| {
            match opened {
                Opened::Open(file) => write_at(file, offset, bufs)?,
                Opened::Closed(Some(_)) => {
                    if !self.layout.is_off(index) {
                        warn!("a write into file {index}, which the torrent gets, before it was granted: kept aside");
                    }
                    self.keep(index, offset, bufs)?
                }
                Opened::Closed(None) | Opened::HandedOn => return Ok(false),
            }
            Ok(true)
        })?;
        if written {
            return Ok(());
        }
        if !self.made(index)? {
            return self.files.read(index, |_, _| self.keep(index, offset, bufs));
        }
        self.files.read(index, |_, opened| match opened {
            Opened::Open(f) => Ok(write_at(f, offset, bufs)?),
            _ => bail!("file {index} was handed on"),
        })
    }

    /// Keeps `bufs`, written at `offset` into file `index`, aside, synced: one that can't be fails
    /// the write.
    fn keep(&self, index: usize, offset: u64, bufs: &[&[u8]]) -> Result<()> {
        // Nothing writes a file aside that much but a loop: its shared pieces, again for a few that fail.
        let cap = self.lens.get(index).map_or(u64::MAX / 2, |len| 4 * len + (64 << 10));
        self.torrent.slots.parts().keep(index, offset, bufs, cap).with_context(|| format!("keeping a part of file {index} aside"))
    }

    /// File `index`'s bytes from `offset` into `buf`: what's in `file` (if it's open), with what's kept
    /// of it over them. All of them, or it fails.
    fn read_into(&self, index: usize, file: Option<&std::fs::File>, offset: u64, buf: &mut [u8]) -> Result<()> {
        let read = match file {
            Some(file) => read_up_to(file, offset, buf)?,
            None => 0,
        };
        let parts = self.torrent.slots.parts();
        if (read == buf.len() && !parts.has(index)) || parts.read_over(index, offset, buf, read)? {
            return Ok(());
        }
        match file {
            Some(_) => bail!("file {index} ends before {}", offset + buf.len() as u64),
            None => bail!("file {index} isn't on disk"),
        }
    }

    /// Makes file `index` again at its path: one moved by hand, or never there. Whether it's open for
    /// writing now: not if something is at its path (made since by someone else, or a link).
    fn made(&self, index: usize) -> Result<bool> {
        self.files.change(index, |path, opened| {
            match opened {
                Opened::Open(_) => return Ok(true),
                Opened::Closed(Some(_)) => return Ok(false),
                Opened::Closed(None) | Opened::HandedOn => {}
            }
            let path = path.context("a padding file is never written")?;
            make_folders(&self.destination, path.parent().context("a file without a folder")?)?;
            let file = match new_file(path) {
                Ok(file) => file,
                Err(e) if matches!(e.raw_os_error(), Some(libc::EEXIST | libc::ELOOP)) => return Ok(false),
                Err(e) => return Err(e).with_context(|| format!("making {path:?}")),
            };
            // What was kept of it, from before it went: in it before anything else.
            self.torrent.slots.parts().write_into(index, &file)?;
            *opened = Opened::Open(file);
            Ok(true)
        })
    }

    /// File `index` at `path` if it's there, closed.
    fn open_closed(&self, path: &Path) -> Result<Opened> {
        match open(path, false) {
            Ok(file) => Ok(Opened::Closed(Some(file))),
            // A link there isn't it (#267): as if nothing were.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound || is_link(&e) => Ok(Opened::Closed(None)),
            Err(e) => Err(e).with_context(|| format!("opening {path:?}")),
        }
    }
}

/// Whether `file`, opened at `path`, is no longer what's there: moved (to the Trash, say), or
/// gone. A symbolic link at the path isn't followed (#267): it's never the torrent's file, even one
/// to the file it has open. librqbit's storage doesn't look: the open handle serves its own file
/// until the size check finds it gone, and `init` opens what's there again.
fn moved(path: Option<&Path>, file: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(path) = path else { return false };
    match (file.metadata(), std::fs::symlink_metadata(path)) {
        (Ok(open), Ok(there)) => (open.dev(), open.ino()) != (there.dev(), there.ino()),
        _ => true,
    }
}

/// Makes `file` `len` bytes long if it's shorter: the one place a torrent's file gets its length, and
/// it's never cut (#374). A longer one isn't the torrent's, and the size check finds it missing. Its
/// length is asked of the open file; there's no call that only lengthens one, so a program making
/// it longer than `len` between that and the next line has what it just added cut off.
pub(super) fn grow(file: &std::fs::File, len: u64) -> std::io::Result<()> {
    if file.metadata()?.len() < len {
        file.set_len(len)?;
    }
    Ok(())
}

fn write_at(file: &std::fs::File, mut offset: u64, bufs: &[&[u8]]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    for buf in bufs {
        file.write_all_at(buf, offset)?;
        offset += buf.len() as u64;
    }
    Ok(())
}

/// `buf`'s bytes from `offset` in `file`, up to its end: how many there were.
fn read_up_to(file: &std::fs::File, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    let mut read = 0;
    while read < buf.len() {
        match file.read_at(&mut buf[read..], offset + read as u64) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(read)
}

/// The file at `path`, for reading and writing, `create`d if it isn't there (but not the folders
/// to it), never truncated. Never through a symbolic link at `path` (#267): one there, dangling or
/// not, isn't the torrent's file, and opening it fails ([`is_link`]); a link to a folder on the way
/// is the person's choice of where the files go, and is followed.
fn open(path: &Path, create: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).create(create).truncate(false).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// The file at `path`, made now for reading and writing: never over anything there, a symbolic link
/// too.
fn new_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// Whether [`open`] failed for a symbolic link at the path.
fn is_link(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ELOOP)
}

/// Whether `name` takes the name of a folder of Phantom's own in a destination: a move's staging
/// (`.phantom-moving-<hash>`). No torrent's file or folder may. In any case: APFS folds it, so
/// `.PHANTOM-Moving-…` (or `.phantom-movinſ-…`, were there an `s`) is the same folder.
pub(super) fn phantoms(name: &std::ffi::OsStr) -> bool {
    // Upper, then lower: folds what lowering alone keeps apart, as `ſ` from `s`.
    let name: String = name.to_string_lossy().chars().flat_map(char::to_uppercase).flat_map(char::to_lowercase).collect();
    name.starts_with(".phantom-moving-")
}

/// Makes the folders from `destination` down to `dir`, one at a time, so a destination that's
/// gone is never made again.
pub(super) fn make_folders(destination: &Path, dir: &Path) -> Result<()> {
    let inside = dir.strip_prefix(destination).with_context(|| format!("{dir:?} isn't in {destination:?}"))?;
    let mut at = destination.to_owned();
    for part in inside.components() {
        at.push(part);
        match std::fs::create_dir(&at) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e).with_context(|| format!("making {at:?}")),
            _ => {}
        }
    }
    Ok(())
}

impl TorrentStorage for Files {
    fn init(&mut self, _: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<()> {
        // A backstop to the add's check: never a file in a folder of Phantom's own, every path
        // looked at before any file is made.
        for info in &metadata.file_infos {
            let path = self.layout.at(&info.relative_filename);
            if let Ok(inside) = path.strip_prefix(&self.destination)
                && inside.components().any(|c| phantoms(c.as_os_str()))
            {
                bail!("{inside:?} takes the name of a folder of Phantom's own");
            }
        }
        self.lens = Arc::new(metadata.file_infos.iter().map(|info| info.len).collect());
        let mut files = Vec::with_capacity(metadata.file_infos.len());
        for info in &metadata.file_infos {
            if info.attrs.padding {
                files.push((None, Opened::Closed(None)));
                continue;
            }
            let path = self.layout.at(&info.relative_filename);
            let file = self.open_closed(&path)?;
            files.push((Some(path), file));
        }
        self.files = self.torrent.slots.open(files);
        Ok(())
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        #[cfg(test)]
        self.at(At::Read);
        #[cfg(test)]
        if let Some((reads_of, hash)) = &self.reads_of {
            reads_of(hash);
        }
        self.files.read(file, |_, opened| {
            #[cfg(test)]
            self.at(At::Reading);
            // Nothing to read: librqbit asks so of the file a piece starts right after, whether it's
            // there or not (one let go of, say), and a failed read would drop the peer that asked.
            if buf.is_empty() {
                return Ok(());
            }
            // What's kept of it, over the file: one let go of, or moved by hand, too.
            match opened {
                Opened::Open(f) | Opened::Closed(Some(f)) => self.read_into(file, Some(f), offset, buf),
                Opened::Closed(None) => self.read_into(file, None, offset, buf),
                Opened::HandedOn => bail!("file {file} was handed on"),
            }
        })
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        self.write(file, offset, &[buf])
    }

    fn pwrite_all_vectored(&self, file: usize, offset: u64, bufs: [std::io::IoSlice<'_>; 2]) -> Result<usize> {
        let [a, b] = bufs;
        self.write(file, offset, &[&a, &b])?;
        Ok(a.len() + b.len())
    }

    fn remove_file(&self, _: usize, path: &Path) -> Result<()> {
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        Ok(std::fs::remove_file(self.layout.at(path))?)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        let path = self.layout.folder.join(path);
        if !path.is_dir() {
            bail!("cannot remove dir: {path:?} is not a directory")
        }
        if std::fs::read_dir(&path)?.next().is_none() {
            std::fs::remove_dir(&path).with_context(|| format!("error removing {path:?}"))?;
        }
        Ok(())
    }

    fn ensure_file_length(&self, file: usize, len: u64) -> Result<()> {
        // librqbit asks for the files it gets only. A file set Off but held, until a check applies it,
        // is one it still gets: neither made nor resized.
        if self.layout.is_off(file) {
            return Ok(());
        }
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        // Only one open for writing: librqbit sets the length as its check ends, when every file is
        // closed (it gets its length at its grant).
        self.files.read(file, |_, opened| match opened {
            Opened::Open(f) => {
                #[cfg(test)]
                self.at(At::Length);
                Ok(grow(f, len)?)
            }
            Opened::HandedOn => bail!("file {file} was handed on"),
            Opened::Closed(_) => Ok(()),
        })
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Files {
            layout: self.layout.clone(),
            destination: self.destination.clone(),
            files: self.files.hand_on(),
            lens: self.lens.clone(),
            torrent: self.torrent.clone(),
            #[cfg(test)]
            hook: self.hook.clone(),
            #[cfg(test)]
            reads_of: self.reads_of.clone(),
        }))
    }
}

/// How many of this process's open files are the file at `path` now: for tests, which look for a
/// handle still open on it once it's gone from disk.
#[cfg(test)]
pub(super) fn open_on(path: &Path) -> impl Fn() -> usize + use<> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).expect("there to look at");
    let (dev, ino) = (meta.dev(), meta.ino());
    move || {
        // SAFETY: `fstat` only writes into `stat`, and fails on a number that isn't open.
        let max = unsafe { libc::getdtablesize() };
        (0..max)
            .filter(|&fd| unsafe {
                let mut stat: libc::stat = std::mem::zeroed();
                libc::fstat(fd, &mut stat) == 0 && stat.st_dev as u64 == dev as u64 && stat.st_ino == ino
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use proto::KeptWhy;

    use super::*;

    /// Keeping parts in `dir`, as the engine does in its data dir.
    fn kept(dir: &Path) -> Trashed {
        Trashed::default().keeping_parts_in(dir.join("parts"))
    }

    /// A trash into a folder of its own, as the engine's tests have.
    fn bin(dir: &Path) -> Trash {
        trash::into(dir.join("trash"), Default::default(), Default::default())
    }

    /// Storage of one file at `path`, opened, of torrent "t".
    fn storage(trashed: &Trashed, dir: &Path, path: &Path) -> Files {
        let mut files = trashed.storage("t", Layout::new(dir.to_owned(), None), dir.to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.to_owned()), Opened::Open(open(path, false).unwrap()))]);
        files
    }

    /// A Move to Trash of the file at `path`, of torrent "t", as file 0: where it went.
    fn take(trashed: &Trashed, trash: &Trash, dir: &Path, path: &Path) -> trash::Trashed {
        let plan = Plan { location: dir.to_owned(), items: vec![path.to_owned()] };
        let carried = trashed.take("t", &plan, &[(0, path.to_owned())], trash, |_| {}, &AtomicBool::new(false));
        carried.moved.into_iter().next().expect("moved")
    }

    /// #376: storage of one file at `path`, which isn't there, set Off.
    fn absent(trashed: &Trashed, dir: &Path, path: &Path) -> Files {
        let mut files = trashed.storage("t", Layout::new(dir.to_owned(), None), dir.to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.to_owned()), Opened::Closed(None))]);
        files
    }

    /// ADR 0014: a file a Move to Trash takes is closed as it goes, and what was kept aside of it
    /// goes too. Nothing of librqbit's reads it, writes it or sets its length in the Trash: it's one
    /// that isn't there, so a piece it shares with a file the torrent gets makes it anew at its path.
    #[test]
    fn a_file_a_move_to_trash_takes_is_closed_as_it_goes() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (kept(dir.path()), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), Some(&[])), dir.path().to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.clone()), Opened::Closed(Some(open(&path, false).unwrap())))]);
        files.pwrite_all(0, 0, &[7; 10]).unwrap();
        assert!(trashed.torrent("t").slots.parts().has(0), "kept aside while it's closed");
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        let open = open_on(&away);
        assert_eq!(open(), 0, "closed as it went");
        assert!(!trashed.torrent("t").slots.parts().has(0), "and what was kept of it goes");
        assert!(files.pread_exact(0, 0, &mut [0; 10]).is_err(), "nothing to read of it");
        files.ensure_file_length(0, 150).unwrap();
        files.pwrite_all(0, 20, &[2; 10]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [[0u8; 20].as_slice(), &[2; 10]].concat(), "made anew at its path");
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "the one in the Trash as it went");
        assert_eq!(trashed.writable("t"), BTreeSet::from([0]), "Phantom's own, open for writing");
    }

    /// a granted file (open for writing) a Move to Trash takes is closed too: a late
    /// write through a storage already taken never lands in the Trash.
    #[test]
    fn a_granted_file_a_move_to_trash_takes_is_never_written_in_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (kept(dir.path()), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), None), dir.path().to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.clone()), Opened::Open(open(&path, false).unwrap()))]);
        let late = files.take().unwrap();
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        assert_eq!(open_on(&away)(), 0, "closed as it went");
        let _ = late.pwrite_all(0, 0, &[9; 10]);
        let _ = files.pwrite_all(0, 10, &[9; 10]);
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "the one in the Trash as it went");
    }

    /// A part of a closed file that can't be kept aside (the data dir can't be written into; a full
    /// disk alike) fails librqbit's write, so the torrent goes to Error, and nothing is read of it.
    #[test]
    fn a_part_that_cannot_be_kept_fails_the_write() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let trashed = kept(&data);
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), Some(&[])), dir.path().to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.clone()), Opened::Closed(Some(open(&path, false).unwrap())))]);
        let mode = std::fs::metadata(&data).unwrap().permissions();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
        let wrote = files.pwrite_all(0, 4, &[2; 10]);
        std::fs::set_permissions(&data, mode).unwrap();
        assert!(wrote.is_err(), "kept nowhere");
        let mut buf = [0; 10];
        files.pread_exact(0, 4, &mut buf).unwrap();
        assert_eq!(buf, [1; 10], "the file as it is, nothing over it");
    }

    /// #376: librqbit writes 0 bytes into the file before a chunk. Not a byte, not a file, nor its
    /// folders: whether it's one write or two empty slices, as vectored.
    #[test]
    fn a_write_of_nothing_makes_no_file_nor_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder/deeper/off.bin");
        let files = absent(&Trashed::default(), dir.path(), &path);
        files.pwrite_all(0, 0, &[]).unwrap();
        files.pwrite_all(0, 7, &[]).unwrap();
        assert_eq!(files.pwrite_all_vectored(0, 0, [std::io::IoSlice::new(&[]), std::io::IoSlice::new(&[])]).unwrap(), 0);
        assert!(!dir.path().join("folder").exists(), "no folder");
    }

    /// An empty slice beside one with bytes still makes the file, and writes the bytes.
    #[test]
    fn a_vectored_write_of_one_empty_slice_and_one_not_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder/off.bin");
        let files = absent(&Trashed::default(), dir.path(), &path);
        let n = files.pwrite_all_vectored(0, 2, [std::io::IoSlice::new(&[]), std::io::IoSlice::new(b"ab")]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"\0\0ab");
        let n = files.pwrite_all_vectored(0, 0, [std::io::IoSlice::new(b"x"), std::io::IoSlice::new(&[])]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"x\0ab");
    }

    /// librqbit's `take` as it sets a file's length (a check's end), or writes: it waits for that one.
    /// The old storage then changes and makes nothing, a file not there too, and the new one goes on
    /// with what was open, a file replaced by hand at its path meanwhile left as it is.
    #[test]
    fn a_storage_taken_ends_its_length_first_and_then_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (path, absent) = (dir.path().join("a.bin"), dir.path().join("folder/b.bin"));
        std::fs::write(&path, [1; 10]).unwrap();
        let gate = Arc::new(super::super::files::Gate::default());
        let trashed = Trashed::default().with_hook(Arc::new({
            let gate = gate.clone();
            move |at| (at == At::Length).then(|| gate.wait()).unwrap_or_default()
        }));
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), None), dir.path().to_owned());
        files.files = trashed
            .torrent("t")
            .slots
            .open(vec![(Some(path.clone()), Opened::Open(open(&path, false).unwrap())), (Some(absent.clone()), Opened::Closed(None))]);
        gate.hold(true);
        let (tx, rx) = std::sync::mpsc::channel();
        let taken = std::thread::scope(|s| {
            s.spawn(|| files.ensure_file_length(0, 20).unwrap());
            while !gate.waiting() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            s.spawn(|| tx.send(files.take().unwrap()).unwrap());
            let early = rx.recv_timeout(std::time::Duration::from_millis(100));
            let length = std::fs::metadata(&path).unwrap().len();
            gate.hold(false);
            assert!(early.is_err() && length == 10, "taken with a length under way");
            rx.recv().unwrap()
        });
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 20, "set as it would have been");

        std::fs::rename(&path, dir.path().join("moved.bin")).unwrap();
        std::fs::write(&path, [5; 20]).unwrap();
        for index in [0, 1] {
            assert!(files.pwrite_all(index, 0, &[2; 10]).is_err(), "written through the old storage");
            assert!(files.ensure_file_length(index, 30).is_err(), "sized through the old storage");
            assert!(files.pread_exact(index, 0, &mut [0; 10]).is_err(), "read through the old storage");
        }
        assert!(!dir.path().join("folder").exists(), "nothing made by the old storage");
        taken.pwrite_all(0, 0, &[3; 10]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [5; 20], "the one put at its path, as it was");
        assert_eq!(std::fs::read(dir.path().join("moved.bin")).unwrap()[..10], [3; 10], "the one open");
        taken.pwrite_all(1, 0, &[4; 10]).unwrap();
        drop(files);
        taken.pread_exact(1, 0, &mut [0; 10]).unwrap();
    }

    /// A symbolic link at a torrent file's path is never the file (#267): opening it fails, to make
    /// it as well, so nothing is made where a dangling one points; and it isn't where the file was
    /// opened, even a link to that file, so a write fails rather than go through it.
    #[test]
    fn a_link_at_a_files_path_is_never_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (path, real, nowhere) = (dir.path().join("a.bin"), dir.path().join("real.bin"), dir.path().join("nowhere.bin"));
        std::fs::write(&real, [1; 100]).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        let dangling = dir.path().join("b.bin");
        std::os::unix::fs::symlink(&nowhere, &dangling).unwrap();
        for create in [false, true] {
            assert!(open(&path, create).is_err_and(|e| is_link(&e)), "create {create}: a link to a file");
            assert!(open(&dangling, create).is_err_and(|e| is_link(&e)), "create {create}: a dangling link");
        }
        assert!(std::fs::symlink_metadata(&nowhere).is_err(), "nothing made where the dangling one points");

        // librqbit's storage, the file it has open being the one the link points to.
        let trashed = kept(dir.path());
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), None), dir.path().to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.clone()), Opened::Open(open(&real, false).unwrap()))]);
        assert_eq!(trashed.replaced("t"), BTreeSet::from([0]), "not the file at its path");
    }

    /// S2 of #217's fifth review: a panic while a Move to Trash holds the torrent's files still (the
    /// Trash's own code can panic) leaves the torrent's storage working: the lock guards no data.
    #[test]
    fn a_panic_during_a_move_leaves_the_storage_working() {
        struct Panics;
        impl trash::Bin for Panics {
            fn take(&self, _: &Path) -> Result<Option<PathBuf>, KeptWhy> {
                panic!("the Trash failed")
            }
            fn give_back(&self, _: &Path, _: &Path) -> Result<(), String> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let trashed = kept(dir.path());
        let files = storage(&trashed, dir.path(), &path);
        let panics: Trash = Arc::new(Panics);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| take(&trashed, &panics, dir.path(), &path)));
        assert!(panicked.is_err());
        // The storage, and the next move, go on.
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        files.ensure_file_length(0, 100).unwrap();
        assert_eq!(std::fs::read(&path).unwrap()[..10], [2; 10]);
        take(&trashed, &bin(dir.path()), dir.path(), &path);
        assert!(!path.exists());
    }
}
