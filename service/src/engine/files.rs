//! Spotting a torrent's files deleted outside Phantom. librqbit opens each file once and keeps it
//! open, so it never notices one deleted, renamed or replaced: it goes on writing into the deleted
//! file and reports it complete. So the engine size-checks them itself, by name and size:
//!
//! - A size check reports a [`Disk`]: the files a torrent is getting (only those whose priority
//!   is Normal) and what's at each one's path; the machine (`machine/`) decides what it means.
//!   [`Found::is_missing`] says a file isn't on disk as it must be: nothing is at its path,
//!   something other than a file is (a symbolic link too, even to one: Phantom's storage never
//!   follows it, #267), or the file's size is one it can't have: larger than its
//!   full size, or smaller than the bytes librqbit has of it (pieces are written in place, so a file is at least as long as the end of
//!   the last piece it has; after a check librqbit sets each Normal file to its full size). A file
//!   replaced by another of a size that fits isn't caught: that's what Check is for.
//! - A missing file is remembered as [`Gone`]: what librqbit had of it, and what was at its path
//!   when it was last judged. [`Gone::is_back`] says it's back when a file of a size it can have is
//!   there and it isn't what was there then (librqbit creates a missing file again when it
//!   restores a torrent, or checks it: that one isn't the file put back). A check then tells
//!   whether it's the file it was.
//! - [`Watcher`] watches the torrents' destinations, recursively, so a torrent's own folder going
//!   is seen too, and reports the paths removed or renamed under them (and some more, see
//!   [`goes`]), so the actor size-checks at once.
//!   The actor also size-checks every torrent every few seconds, for what the watcher misses (a file
//!   cut short, or a destination that couldn't be watched).
//! - At start each torrent's files are size-checked from its record before it's added to librqbit,
//!   because an add creates a missing file again, empty: one missing isn't added.
//! - A [`Destination`] is parsed once where a client names it: an absolute path to a folder that
//!   can be reached then. Code past that never checks it again.
//! - Before its files, a torrent's destination: [`unavailable`] says why it can't be reached (a
//!   drive not connected, a folder gone, no permission). Its files are then not missing, just out of reach.
//!   [`Guard`] keeps librqbit off the files of a torrent whose destination is unavailable, even one
//!   that goes while its storage opens them (it makes none, #374). It also keeps librqbit off a torrent's files
//!   once its Delete begins, through the storage librqbit already has too: a file in the Trash
//!   stays as it went, so Undo can tell it's the one.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{Result, bail};
use librqbit::storage::filesystem::FilesystemStorageFactory;
use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, event::ModifyKind};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use proto::{AbsolutePath, Unavailable};
use tracing::warn;

/// How long the watcher gathers events before reporting them.
const DEBOUNCE: Duration = Duration::from_millis(250);

pub use lifecycle::disk::{Disk, Expected, Found, Gone, Seen, Stamp};

#[cfg(test)]
pub use super::trashed::At;
use super::trashed::{Layout, Trashed};

/// Size-checks `files` now: each, and what's at its path; `open`, those Phantom has open for writing;
/// `shared`, each one's bytes in pieces it shares with other files, by index.
pub fn disk_of(files: &[Expected], open: &BTreeSet<usize>, shared: &[u64]) -> Disk {
    let found = |f: &Expected| Found { shared: shared.get(f.index).copied().unwrap_or(0), ..found(f, open.contains(&f.index)) };
    Disk { files: files.iter().map(|f| (f.index, found(f))).collect() }
}

/// What's at `file`'s path: the file there, its names, or something else.
fn found(file: &Expected, open: bool) -> Found {
    let meta = std::fs::symlink_metadata(&file.path).ok();
    let at = meta.as_ref().filter(|m| m.is_file()).map(|m| Stamp { inode: m.ino(), len: m.len(), modified: m.modified().ok() });
    let links = meta.as_ref().map_or(1, |m| m.nlink());
    Found { file: file.clone(), at, links, taken: meta.is_some_and(|m| !m.is_file()), open, shared: 0 }
}

/// What `look` finds of the files, if the destination is still reachable after it: one that goes
/// while its files are looked at makes every one seem missing, so what was found isn't taken for
/// the files then (#366). Gone and back within one look isn't told; the next look recovers.
pub fn seen(destination: &AbsolutePath, look: impl FnOnce() -> Disk) -> Result<Disk, Unavailable> {
    let disk = look();
    match unavailable(destination) {
        Some(reason) => Err(reason),
        None => Ok(disk),
    }
}

/// The regular file at `path`, if there is one: a symbolic link there, even to one, isn't (#267).
pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::symlink_metadata(path).ok().filter(|m| m.is_file())?;
    Some(Stamp { inode: m.ino(), len: m.len(), modified: m.modified().ok() })
}

/// A folder torrents can go into: reachable when it was parsed (the protocol already makes it
/// absolute). Only [`Destination::parse`] makes one, so nothing that takes it checks again; its
/// destination going away later is what the actor watches for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination(AbsolutePath);

impl Destination {
    /// Never creates the folder.
    pub fn parse(path: AbsolutePath) -> Result<Self, Unavailable> {
        match unavailable(&path) {
            Some(reason) => Err(reason),
            None => Ok(Self(path)),
        }
    }

    pub fn into_path(self) -> AbsolutePath {
        self.0
    }
}

/// Why a torrent's destination can't be reached, or `None` if it can: a drive that isn't
/// connected, a folder that's gone, or one Phantom may not open. Only reads: never creates
/// anything, so a drive that isn't connected never gets a folder of its name on the internal disk.
pub fn unavailable(destination: &AbsolutePath) -> Option<Unavailable> {
    if let Some(name) = disconnected_drive(destination) {
        return Some(Unavailable::DriveNotConnected { name });
    }
    let path = destination.to_owned();
    match std::fs::read_dir(destination) {
        Ok(_) => None,
        Err(e) => Some(match e.kind() {
            std::io::ErrorKind::NotFound => Unavailable::FolderMissing { path },
            std::io::ErrorKind::PermissionDenied => Unavailable::NoPermission { path },
            std::io::ErrorKind::NotADirectory => Unavailable::NotAFolder { path },
            _ => Unavailable::CantOpen { path },
        }),
    }
}

/// The drive's name, if `path` is on a drive under `/Volumes` that isn't mounted.
fn disconnected_drive(path: &Path) -> Option<String> {
    drive(path).filter(|(_, mounted)| !mounted).map(|(name, _)| name)
}

/// The drive's name, if `path` is on a drive under `/Volumes`, and whether it's mounted: it isn't
/// when nothing is at `/Volumes/<name>`, or only a folder of the internal disk (a mounted drive is
/// another device). `/Volumes/Macintosh HD` is a link to `/`, so it's always there.
fn drive(path: &Path) -> Option<(String, bool)> {
    use std::path::Component::{Normal, RootDir};
    let mut parts = path.components();
    let (Some(RootDir), Some(Normal(volumes)), Some(Normal(name))) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if volumes != "Volumes" {
        return None;
    }
    let root = Path::new("/Volumes");
    let mounted = match (std::fs::metadata(root), std::fs::symlink_metadata(root.join(name))) {
        (_, Ok(m)) if m.file_type().is_symlink() => true,
        // A plain folder left at /Volumes/<name> would shadow the mount: only another device is it.
        (Ok(volumes), Ok(drive)) => drive.dev() != volumes.dev(),
        _ => false,
    };
    Some((name.to_string_lossy().into_owned(), mounted))
}

/// Phantom's storage for librqbit ([`Files`]), each add's ([`Guard::for_add`]), which knows the
/// torrent's destination, its folder and its priorities: those set Off aren't made (#67, #158).
/// A torrent whose destination can't be reached when librqbit creates its storage gets storage that
/// touches nothing on disk: for a drive that isn't connected a file made would be a folder of its
/// name on the internal disk. Reading and writing fail, so a torrent added that way has nothing
/// until it's checked again, once its destination is back. The destination can still go while the
/// storage opens the files ([`init_in`]): the storage then touches nothing either.
#[derive(Clone)]
pub struct Guard {
    /// Whether each torrent's storage touches nothing, wherever its destination: one a Delete is
    /// trashing, or left partly in the Trash. Shared with the storage librqbit has already.
    untouched: Arc<Mutex<HashMap<String, Untouched>>>,
    init_in: InitIn,
    /// The files in the Trash, which every storage of Phantom's own reads where they went and keeps
    /// librqbit off (`trashed.rs`).
    trashed: Trashed,
    /// The places undos are putting files back to, which an add waits for.
    places: super::places::Places,
    /// The one torrent's destination and layout, for its add.
    layout: Option<(AbsolutePath, Layout)>,
    /// How many storages it made that aren't Phantom's own (`Files`), for a test.
    #[cfg(test)]
    unguarded: Arc<std::sync::atomic::AtomicUsize>,
}

/// Where a test holds librqbit's storage ([`Guard::with_gate`]): a check's reads, say, wait here
/// while the gate is held, so the check stays under way (Checking) until the test opens it.
#[cfg(test)]
#[derive(Default)]
pub struct Gate {
    held: Mutex<bool>,
    opened: std::sync::Condvar,
    /// How many wait there now.
    waiting: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl Gate {
    pub fn hold(&self, held: bool) {
        *self.held.lock().unwrap() = held;
        self.opened.notify_all();
    }

    pub fn is_held(&self) -> bool {
        *self.held.lock().unwrap()
    }

    /// Whether something waits there now.
    pub fn waiting(&self) -> bool {
        self.waiting.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// Returns once the gate is open.
    pub fn wait(&self) {
        let held = self.held.lock().unwrap();
        self.waiting.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        drop(self.opened.wait_while(held, |held| *held).unwrap());
        self.waiting.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// [`init_in`], or a test's hold on it.
pub type InitIn = Arc<dyn Fn(&Path, &mut dyn FnMut() -> Result<()>) -> Result<bool> + Send + Sync>;

impl Default for Guard {
    fn default() -> Self {
        Self::new(Arc::new(|destination, init| init_in(destination, init)))
    }
}

impl Guard {
    /// With librqbit creating the files through `init_in`.
    pub fn new(init_in: InitIn) -> Self {
        Self {
            untouched: Arc::default(),
            init_in,
            trashed: Trashed::default(),
            places: Default::default(),
            layout: None,
            #[cfg(test)]
            unguarded: Arc::default(),
        }
    }

    /// How many storages it made that aren't Phantom's own: librqbit's, which knows nothing of a
    /// file moved from its path.
    #[cfg(test)]
    pub fn unguarded(&self) -> usize {
        self.unguarded.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// With librqbit's storage waiting at `gate` while it's held, at `at`: every read of a file (a
    /// check reads piece by piece), every change (a write, a length set, a removal), or every length
    /// set on a file found in place. Gates add up.
    /// Running `hook` wherever librqbit's storage can be held ([`At`]), after the gates before it.
    #[cfg(test)]
    pub fn with_hook(self, hook: super::trashed::Hook) -> Self {
        let before = self.trashed.hook();
        let both: super::trashed::Hook = Arc::new(move |now| {
            if let Some(before) = &before {
                before(now);
            }
            hook(now);
        });
        Self { trashed: self.trashed.clone().with_hook(both), ..self }
    }

    /// Counting into `reads` every read librqbit makes of a torrent's files (a check reads piece by
    /// piece; a start that trusts its saved pieces, a few of them).
    #[cfg(test)]
    pub fn counting_reads(self, reads: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        self.with_hook(Arc::new(move |now| {
            if now == At::Read {
                reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }))
    }

    #[cfg(test)]
    pub fn with_gate(self, at: At, gate: Arc<Gate>) -> Self {
        let before = self.trashed.hook();
        let hook: super::trashed::Hook = Arc::new(move |now| {
            if let Some(before) = &before {
                before(now);
            }
            if now == at {
                gate.wait();
            }
        });
        Self { trashed: self.trashed.clone().with_hook(hook), ..self }
    }

    /// With every read of a check in full waiting at `gate` while it's held: one of a torrent
    /// librqbit has saved no pieces of in `session` (it drops them before such a check). A sampling
    /// of what it saved, as it's added back with it, never waits.
    #[cfg(test)]
    pub fn holding_full_checks(self, gate: Arc<Gate>, session: PathBuf) -> Self {
        let reads_of: super::trashed::ReadsOf = Arc::new(move |hash| {
            if !super::bitv::path(&session, hash).exists() {
                gate.wait();
            }
        });
        Self { trashed: self.trashed.clone().with_reads_of(reads_of), ..self }
    }

    /// Keeping what librqbit writes into a file in the Trash in `dir`.
    pub fn keeping_parts_in(self, dir: PathBuf) -> Self {
        Self { trashed: self.trashed.clone().keeping_parts_in(dir), ..self }
    }

    /// The places undos are putting files back to.
    pub fn places(&self) -> &super::places::Places {
        &self.places
    }

    /// The files in the Trash ([`Trashed`]).
    pub fn trashed(&self) -> &Trashed {
        &self.trashed
    }

    /// For one add of a torrent into `destination`, its files in librqbit's output `folder`, getting
    /// the `normal` files, as librqbit's `only_files`; a torrent of one file's file under the name
    /// `one_file`, if it was renamed (#331).
    pub fn for_add(&self, destination: AbsolutePath, folder: PathBuf, normal: &[usize], one_file: Option<PathBuf>) -> Self {
        Self { layout: Some((destination, Layout::new(folder, Some(normal)).one_file_as(one_file))), ..self.clone() }
    }

    /// Whether librqbit's storage for the torrent touches nothing (`true`) or its files. The
    /// storage it has already too: kept untouched, what it's doing to them ends first, so nothing
    /// touches them once this returns (#186). That waits for a disk write, which a stalled drive
    /// can make last: never on the actor while librqbit may be writing. Touched again, nothing
    /// waits. librqbit sets a file's length as its check ends, through the file it opened, which
    /// moves the file's modification time: a file already in the Trash would no longer be the one
    /// that went, and Undo would refuse it. That length isn't set again once it's touched again:
    /// the file stays short until written or checked, which a write extends.
    pub fn keep_untouched(&self, info_hash: &str, untouched: bool) {
        let flag = self.untouched_of(info_hash);
        flag.on.store(untouched, std::sync::atomic::Ordering::SeqCst);
        if untouched {
            #[cfg(test)]
            flag.keeping.store(true, std::sync::atomic::Ordering::SeqCst);
            drop(flag.touches.write().unwrap_or_else(std::sync::PoisonError::into_inner));
            #[cfg(test)]
            flag.keeping.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// The torrent's flag, made once: every storage of it shares it.
    fn untouched_of(&self, info_hash: &str) -> Untouched {
        self.untouched.lock().unwrap().entry(info_hash.to_owned()).or_default().clone()
    }
}

impl StorageFactory for Guard {
    type Storage = Box<dyn TorrentStorage>;

    fn create(&self, shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<Self::Storage> {
        let hash = shared.info_hash.as_string();
        let untouched = self.untouched_of(&hash);
        if untouched.is_on() {
            return Ok(Box::new(UnavailableStorage));
        }
        let storage = self.create_touching(&hash, shared, metadata)?;
        Ok(Box::new(Untouchable { storage, untouched }))
    }

    // librqbit only saves torrents stored in files; these are, or will be once back.
    fn is_type_id(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<FilesystemStorageFactory>()
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

impl Guard {
    /// The storage of torrent `hash` while it may touch its files.
    fn create_touching(&self, hash: &str, shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<Box<dyn TorrentStorage>> {
        let Some((destination, layout)) = self.layout.clone() else {
            // librqbit's own, for an add that isn't Phantom's: there's none.
            #[cfg(test)]
            self.unguarded.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Ok(Box::new(FilesystemStorageFactory::default().create(shared, metadata)?));
        };
        if unavailable(&destination).is_some() {
            return Ok(Box::new(UnavailableStorage));
        }
        let files = Box::new(self.trashed.storage(hash, layout, destination.to_path_buf()));
        Ok(Box::new(Guarded { destination: destination.to_path_buf(), storage: files, init_in: self.init_in.clone() }))
    }
}

/// Whether a torrent's storage touches nothing on disk now ([`Guard::keep_untouched`]).
type Untouched = Arc<UntouchedFlag>;

#[derive(Default)]
struct UntouchedFlag {
    on: std::sync::atomic::AtomicBool,
    /// Held to read by each change to a file while it's made, to write once `on` is set: so that
    /// waits for the change under way.
    touches: RwLock<()>,
    /// A [`Guard::keep_untouched`] waits for the change under way, for tests to wait on.
    #[cfg(test)]
    keeping: std::sync::atomic::AtomicBool,
}

impl UntouchedFlag {
    fn is_on(&self) -> bool {
        self.on.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// A torrent's storage, which does nothing to its files from the moment it's kept untouched:
/// one librqbit already has when a Delete begins. Reads go on: they change nothing of a file.
struct Untouchable {
    storage: Box<dyn TorrentStorage>,
    untouched: Untouched,
}

impl Untouchable {
    /// Does `touch` unless the torrent is kept untouched, which waits meanwhile.
    fn touching<T>(&self, touch: impl FnOnce() -> Result<T>) -> Result<T> {
        let _touching = self.untouched.touches.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.untouched.is_on() {
            bail!("its files are being deleted, or partly in the Trash");
        }
        touch()
    }

    /// `init`s the storage, which creates its files, unless it was kept untouched since it was
    /// made: then it touches nothing, as if it had been made then.
    fn init_with(&mut self, init: impl FnOnce(&mut dyn TorrentStorage) -> Result<()>) -> Result<()> {
        let untouched = self.untouched.clone();
        let _touching = untouched.touches.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        if untouched.is_on() {
            self.storage = Box::new(UnavailableStorage);
            return Ok(());
        }
        init(&mut *self.storage)
    }
}

impl TorrentStorage for Untouchable {
    fn init(&mut self, shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<()> {
        self.init_with(|storage| storage.init(shared, metadata))
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.storage.pread_exact(file, offset, buf)
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        self.touching(|| self.storage.pwrite_all(file, offset, buf))
    }

    fn pwrite_all_vectored(&self, file: usize, offset: u64, bufs: [std::io::IoSlice<'_>; 2]) -> Result<usize> {
        self.touching(|| self.storage.pwrite_all_vectored(file, offset, bufs))
    }

    fn remove_file(&self, file: usize, path: &Path) -> Result<()> {
        self.touching(|| self.storage.remove_file(file, path))
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        self.touching(|| self.storage.remove_directory_if_empty(path))
    }

    fn ensure_file_length(&self, file: usize, len: u64) -> Result<()> {
        self.touching(|| self.storage.ensure_file_length(file, len))
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Untouchable { storage: self.storage.take()?, untouched: self.untouched.clone() }))
    }
}

/// librqbit's file storage for a torrent whose destination was there when its storage was created,
/// until it turns out to have gone before librqbit created the files.
struct Guarded {
    destination: PathBuf,
    storage: Box<dyn TorrentStorage>,
    init_in: InitIn,
}

impl TorrentStorage for Guarded {
    fn init(&mut self, shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<()> {
        let storage = &mut self.storage;
        if !(self.init_in)(&self.destination, &mut || storage.init(shared, metadata))? {
            self.storage = Box::new(UnavailableStorage);
        }
        Ok(())
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.storage.pread_exact(file, offset, buf)
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        self.storage.pwrite_all(file, offset, buf)
    }

    fn pwrite_all_vectored(&self, file: usize, offset: u64, bufs: [std::io::IoSlice<'_>; 2]) -> Result<usize> {
        self.storage.pwrite_all_vectored(file, offset, bufs)
    }

    fn remove_file(&self, file: usize, path: &Path) -> Result<()> {
        self.storage.remove_file(file, path)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        self.storage.remove_directory_if_empty(path)
    }

    fn ensure_file_length(&self, file: usize, len: u64) -> Result<()> {
        self.storage.ensure_file_length(file, len)
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Guarded { destination: self.destination.clone(), storage: self.storage.take()?, init_in: self.init_in.clone() }))
    }
}

/// Runs the storage's `init`, which opens the torrent's files under `destination` (and makes
/// nothing, #374). If the destination isn't the folder it was just before ([`made_again`]), it went
/// meanwhile: the files opened aren't in it, and the storage touches nothing. Whether the files are
/// in the destination.
pub fn init_in(destination: &Path, init: &mut dyn FnMut() -> Result<()>) -> Result<bool> {
    let Some(before) = folder(destination) else { return Ok(false) };
    let made = init();
    if made_again(before, folder(destination), || drive(destination).is_some_and(|(_, mounted)| mounted)) {
        return Ok(false);
    }
    made.map(|()| true)
}

/// A folder, by its device and inode.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Folder {
    dev: u64,
    ino: u64,
}

/// Which folder is at `path`, if one is.
fn folder(path: &Path) -> Option<Folder> {
    std::fs::metadata(path).ok().filter(|m| m.is_dir()).map(|m| Folder { dev: m.dev(), ino: m.ino() })
}

/// Whether the folder at a destination's path after librqbit's `init` was made by it, by the one
/// there `before`: another on the same device is one made again where the destination was. On
/// another device it's the drive mounted again, if it's `mounted`; if not, a folder of the internal
/// disk made in the drive's place.
fn made_again(before: Folder, after: Option<Folder>, mounted: impl FnOnce() -> bool) -> bool {
    match after {
        Some(after) if after == before => false,
        Some(after) if after.dev != before.dev => !mounted(),
        _ => true,
    }
}

/// The storage of a torrent whose destination is unavailable.
struct UnavailableStorage;

impl TorrentStorage for UnavailableStorage {
    fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> Result<()> {
        Ok(())
    }

    fn pread_exact(&self, _: usize, _: u64, _: &mut [u8]) -> Result<()> {
        bail!("its destination is unavailable")
    }

    fn pwrite_all(&self, _: usize, _: u64, _: &[u8]) -> Result<()> {
        bail!("its destination is unavailable")
    }

    fn remove_file(&self, _: usize, _: &Path) -> Result<()> {
        Ok(())
    }

    fn remove_directory_if_empty(&self, _: &Path) -> Result<()> {
        Ok(())
    }

    fn ensure_file_length(&self, _: usize, _: u64) -> Result<()> {
        bail!("its destination is unavailable")
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(UnavailableStorage))
    }
}

/// Whether an event can mean a path went: removed, renamed, or its metadata changed. The last
/// because the debouncer drops a path created and removed while it gathers events, and macOS
/// reports a recently created file as created again with its removal; what's left is a metadata
/// change of its folder. Writes don't count: they come all the time while downloading.
fn goes(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_) | ModifyKind::Metadata(_) | ModifyKind::Any))
}

/// Whether a change at `changed` can make a file at or under `location` go missing: it's the
/// location, inside it, or a folder it's in.
pub fn touches(changed: &Path, location: &Path) -> bool {
    changed.starts_with(location) || location.starts_with(changed)
}

/// Watches folders for removed and renamed paths.
pub struct Watcher {
    debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    /// Each watched folder by its real path, which events name: e.g. `/private/var/…` for a
    /// folder watched as `/var/…`, a symbolic link.
    watching: Arc<Mutex<BTreeMap<PathBuf, PathBuf>>>,
}

impl Watcher {
    /// `changed` gets the paths removed or renamed under the watched folders, on the watcher's
    /// own thread.
    pub fn start(changed: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Self> {
        let watching = Arc::new(Mutex::new(BTreeMap::<PathBuf, PathBuf>::new()));
        let roots = watching.clone();
        let debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| match result {
            Ok(events) => {
                let roots = roots.lock().unwrap();
                // Back to the paths as the engine knows them.
                let as_watched = |path: PathBuf| match roots.iter().find_map(|(real, w)| Some(w.join(path.strip_prefix(real).ok()?))) {
                    Some(watched) => watched,
                    None => path,
                };
                let paths: Vec<PathBuf> =
                    events.into_iter().filter(|e| goes(&e.kind)).flat_map(|e| e.event.paths).map(as_watched).collect();
                if !paths.is_empty() {
                    changed(paths);
                }
            }
            Err(errors) => errors.iter().for_each(|e| warn!(error = %e, "file watcher")),
        })?;
        Ok(Self { debouncer, watching })
    }

    /// Watches exactly `folders`. One that can't be watched, e.g. because it isn't there, is
    /// tried again on the next call.
    pub fn watch(&mut self, folders: BTreeSet<PathBuf>) {
        let mut watching = self.watching.lock().unwrap();
        for (_, gone) in watching.extract_if(.., |_, watched| !folders.contains(watched)) {
            let _ = self.debouncer.unwatch(&gone);
        }
        let watched: BTreeSet<PathBuf> = watching.values().cloned().collect();
        for folder in folders.difference(&watched) {
            let Ok(real) = folder.canonicalize() else { continue };
            if self.debouncer.watch(folder, RecursiveMode::Recursive).is_ok() {
                watching.insert(real, folder.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifecycle::disk::Seen;

    fn abs(path: &Path) -> AbsolutePath {
        AbsolutePath::new(path).unwrap()
    }

    fn expected(path: PathBuf, size_bytes: u64, progress_bytes: u64) -> Expected {
        Expected { index: 0, path, size_bytes, progress_bytes }
    }

    fn found(path: PathBuf, size_bytes: u64, progress_bytes: u64) -> Found {
        disk_of(&[expected(path, size_bytes, progress_bytes)], &BTreeSet::new(), &[]).files.remove(&0).unwrap()
    }

    #[test]
    fn a_file_is_missing_when_gone_or_of_a_size_it_cannot_have() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [0; 100]).unwrap();
        assert!(!found(path.clone(), 100, 100).is_missing(), "complete");
        assert!(!found(path.clone(), 200, 50).is_missing(), "partly downloaded, written up to 100");
        assert!(found(path.clone(), 50, 0).is_missing(), "larger than it can be");
        assert!(found(path.clone(), 200, 150).is_missing(), "shorter than what librqbit has of it");
        assert!(!found(dir.path().join("b.bin"), 100, 0).is_missing(), "gone, but nothing was downloaded: made as it starts");
        assert!(found(dir.path().join("b.bin"), 100, 1).is_missing(), "gone");
        assert!(found(dir.path().to_owned(), 100, 0).is_missing(), "a folder in its place");
    }

    #[test]
    fn a_missing_file_is_back_when_a_file_that_can_be_it_appears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let gone = Gone::found(&found(path.clone(), 100, 60));
        assert!(!gone.is_back(stamp(&path)), "still gone");
        std::fs::write(&path, [0; 50]).unwrap();
        assert!(!gone.is_back(stamp(&path)), "too short to hold what librqbit had");
        std::fs::write(&path, [0; 100]).unwrap();
        assert!(gone.is_back(stamp(&path)));

        let mut recreated = gone.clone();
        recreated.seen = Seen::NotYet;
        assert!(!recreated.is_back(stamp(&path)), "not judged yet");
        let judged = stamp(&path);
        recreated.seen = Seen::Was(judged);
        assert!(!recreated.is_back(stamp(&path)), "the file that was there when judged");
        // Written again until its modification time moves on: the clock's tick can be coarse.
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while stamp(&path) == judged {
            assert!(std::time::Instant::now() < until, "its modification time never moved on");
            std::fs::write(&path, [1; 100]).unwrap();
        }
        assert!(recreated.is_back(stamp(&path)), "another one since");
    }

    #[test]
    fn files_looked_at_while_their_destination_goes_arent_taken_for_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("downloads");
        std::fs::create_dir(&destination).unwrap();
        let path = destination.join("a.bin");
        std::fs::write(&path, [0; 100]).unwrap();
        let (dest, files) = (abs(&destination), [expected(path, 100, 100)]);

        let disk = seen(&dest, || disk_of(&files, &BTreeSet::new(), &[])).unwrap();
        assert!(!disk.files[&0].is_missing(), "there all along");

        let away = dir.path().join("away");
        let gone = seen(&dest, || {
            let disk = disk_of(&files, &BTreeSet::new(), &[]);
            std::fs::rename(&destination, &away).unwrap();
            disk
        });
        assert_eq!(gone.err(), Some(Unavailable::FolderMissing { path: dest }), "went after the look began");
    }

    #[test]
    fn a_destination_is_unavailable_when_its_drive_or_folder_is_gone_or_cannot_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unavailable(&abs(dir.path())), None);
        let gone = abs(&dir.path().join("gone"));
        assert_eq!(unavailable(&gone), Some(Unavailable::FolderMissing { path: gone.clone() }));
        let file = abs(&dir.path().join("a.bin"));
        std::fs::write(&file, b"").unwrap();
        assert_eq!(unavailable(&file), Some(Unavailable::NotAFolder { path: file.clone() }));

        let drive = &abs(Path::new("/Volumes/Phantom test drive that isn't there/Torrents"));
        let name = "Phantom test drive that isn't there".to_owned();
        assert_eq!(unavailable(drive), Some(Unavailable::DriveNotConnected { name }));
        assert!(!drive.parent().unwrap().exists(), "and nothing was made");
        assert_eq!(unavailable(&abs(Path::new("/Volumes/Macintosh HD/tmp"))), None, "the internal disk, by its link");
        assert_eq!(disconnected_drive(Path::new("/Users/x/Volumes/y")), None);
    }

    #[test]
    fn a_destination_phantom_may_not_open_is_unavailable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = abs(&dir.path().join("locked"));
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let reason = unavailable(&locked);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(reason, Some(Unavailable::NoPermission { path: locked }));
    }

    /// What can't be generated: a drive mounted again is another device.
    #[test]
    fn the_folder_after_librqbit_creates_the_files_is_made_again_unless_it_is_the_one_before_or_a_drive_back() {
        let before = Folder { dev: 1, ino: 10 };
        for mounted in [false, true] {
            assert!(!made_again(before, Some(before), || mounted), "the same folder");
            assert!(made_again(before, Some(Folder { dev: 1, ino: 11 }), || mounted), "made again where it was");
            assert!(made_again(before, None, || mounted), "gone");
            let other_device = made_again(before, Some(Folder { dev: 2, ino: 10 }), || mounted);
            assert_eq!(other_device, !mounted, "the drive back, or the internal disk in its place");
        }
    }

    /// What librqbit's storage does to a file: through the file it opened, as librqbit does, but
    /// a removal, by its path.
    #[derive(Debug, Clone, Copy)]
    enum Touch {
        Write,
        WriteVectored,
        SetLength,
        Remove,
    }

    /// A file's storage, that can hold a touch once it has begun: the file is librqbit's, opened
    /// before it goes to the Trash.
    struct OnDisk {
        file: std::fs::File,
        path: PathBuf,
        hold: Arc<Hold>,
    }

    /// Where a touch waits once it has begun, while held.
    #[derive(Default)]
    struct Hold {
        state: Mutex<HoldState>,
        changed: std::sync::Condvar,
    }

    #[derive(Default)]
    struct HoldState {
        held: bool,
        /// A touch waits there, or did.
        waiting: bool,
    }

    impl Hold {
        fn new(held: bool) -> Arc<Self> {
            Arc::new(Self { state: Mutex::new(HoldState { held, waiting: false }), changed: Default::default() })
        }

        /// Waits while held.
        fn wait(&self) {
            let mut state = self.state.lock().unwrap();
            state.waiting = true;
            self.changed.notify_all();
            drop(self.changed.wait_while(state, |s| s.held).unwrap());
        }

        fn wait_for_a_touch(&self) {
            drop(self.changed.wait_while(self.state.lock().unwrap(), |s| !s.waiting).unwrap());
        }

        fn open(&self) {
            self.state.lock().unwrap().held = false;
            self.changed.notify_all();
        }
    }

    impl OnDisk {
        fn held(&self) {
            self.hold.wait();
        }
    }

    impl TorrentStorage for OnDisk {
        fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> Result<()> {
            Ok(())
        }

        fn pread_exact(&self, _: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
            Ok(std::os::unix::fs::FileExt::read_exact_at(&self.file, buf, offset)?)
        }

        fn pwrite_all(&self, _: usize, offset: u64, buf: &[u8]) -> Result<()> {
            self.held();
            Ok(std::os::unix::fs::FileExt::write_all_at(&self.file, buf, offset)?)
        }

        fn remove_file(&self, _: usize, _: &Path) -> Result<()> {
            self.held();
            Ok(std::fs::remove_file(&self.path)?)
        }

        fn remove_directory_if_empty(&self, _: &Path) -> Result<()> {
            Ok(())
        }

        fn ensure_file_length(&self, _: usize, len: u64) -> Result<()> {
            self.held();
            Ok(self.file.set_len(len)?)
        }

        fn take(&self) -> Result<Box<dyn TorrentStorage>> {
            bail!("not taken in these tests")
        }
    }

    fn touch(storage: &dyn TorrentStorage, touch: Touch, path: &Path) -> Result<()> {
        match touch {
            Touch::Write => storage.pwrite_all(0, 0, &[1; 16]),
            Touch::WriteVectored => {
                storage.pwrite_all_vectored(0, 0, [std::io::IoSlice::new(&[2; 8]), std::io::IoSlice::new(&[3; 8])]).map(drop)
            }
            // librqbit's, as its check ends: the length it has, which still moves its modification time.
            Touch::SetLength => storage.ensure_file_length(0, 4096),
            Touch::Remove => storage.remove_file(0, path),
        }
    }

    /// One that leaves the file where it is.
    fn a_change() -> impl proptest::strategy::Strategy<Value = Touch> {
        proptest::prop_oneof![
            proptest::strategy::Just(Touch::Write),
            proptest::strategy::Just(Touch::WriteVectored),
            proptest::strategy::Just(Touch::SetLength),
        ]
    }

    fn any_touch() -> impl proptest::strategy::Strategy<Value = Touch> {
        proptest::prop_oneof![
            proptest::strategy::Just(Touch::Write),
            proptest::strategy::Just(Touch::WriteVectored),
            proptest::strategy::Just(Touch::SetLength),
            proptest::strategy::Just(Touch::Remove),
        ]
    }

    proptest::proptest! {
        /// A torrent's storage kept untouched, as its Delete begins (#186): a touch under way ends
        /// first, and from then on nothing changes its file, gone to the Trash, whatever librqbit
        /// does; once it's no longer kept so, it's touched again.
        #[test]
        fn nothing_touches_a_torrent_s_files_once_it_is_kept_untouched(
            under_way in proptest::option::of(a_change()),
            after in proptest::collection::vec(any_touch(), 1..5),
            again in any_touch(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.bin");
            std::fs::write(&path, [0; 4096]).unwrap();
            let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
            let hold = Hold::new(under_way.is_some());
            let guard = Guard::default();
            let storage = Arc::new(Untouchable {
                storage: Box::new(OnDisk { file, path: path.clone(), hold: hold.clone() }),
                untouched: guard.untouched_of("hash"),
            });

            if let Some(under_way) = under_way {
                let touching = std::thread::spawn({
                    let (storage, path) = (storage.clone(), path.clone());
                    move || touch(&*storage, under_way, &path)
                });
                hold.wait_for_a_touch();
                let returned = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let keeping = std::thread::spawn({
                    let (guard, returned) = (guard.clone(), returned.clone());
                    move || {
                        guard.keep_untouched("hash", true);
                        returned.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                });
                // At the touch's lock: it waits there, or (the bug) is past it.
                let until = std::time::Instant::now() + Duration::from_secs(10);
                while !guard.untouched_of("hash").keeping.load(std::sync::atomic::Ordering::SeqCst)
                    && !returned.load(std::sync::atomic::Ordering::SeqCst)
                {
                    proptest::prop_assert!(std::time::Instant::now() < until, "never at the touch's lock");
                    std::thread::yield_now();
                }
                proptest::prop_assert!(!returned.load(std::sync::atomic::Ordering::SeqCst), "it waits for the touch under way");
                hold.open();
                proptest::prop_assert!(touching.join().unwrap().is_ok(), "the touch under way ends as it would have");
                keeping.join().unwrap();
            } else {
                guard.keep_untouched("hash", true);
            }

            // To the Trash, as it is now.
            let trashed = dir.path().join("trashed");
            std::fs::rename(&path, &trashed).unwrap();
            let went = crate::engine::trash::identity(&trashed);
            for t in after {
                proptest::prop_assert!(touch(&*storage, t, &path).is_err(), "{:?} refused", t);
            }
            proptest::prop_assert_eq!(crate::engine::trash::identity(&trashed), went, "in the Trash as it went");
            proptest::prop_assert!(!path.exists(), "nothing made at its path");

            std::fs::rename(&trashed, &path).unwrap();
            guard.keep_untouched("hash", false);
            proptest::prop_assert!(touch(&*storage, again, &path).is_ok(), "{:?} once back", again);
        }
    }

    /// A Delete begun between librqbit making a torrent's storage and creating its files: none is
    /// created, and the storage touches nothing, once touched again too.
    #[test]
    fn a_storage_kept_untouched_before_its_files_are_created_creates_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [0; 4096]).unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let guard = Guard::default();
        let on_disk = || Box::new(OnDisk { file: file.try_clone().unwrap(), path: path.clone(), hold: Hold::new(false) });
        let mut storage = Untouchable { storage: on_disk(), untouched: guard.untouched_of("hash") };
        guard.keep_untouched("hash", true);
        let mut created = false;
        storage.init_with(|_| Ok(created = true)).unwrap();
        assert!(!created, "no file created");
        guard.keep_untouched("hash", false);
        let before = crate::engine::trash::identity(&path);
        assert!(touch(&storage, Touch::SetLength, &path).is_err(), "nothing touched once touched again");
        assert_eq!(crate::engine::trash::identity(&path), before);

        let mut storage = Untouchable { storage: on_disk(), untouched: guard.untouched_of("hash") };
        storage.init_with(|_| Ok(created = true)).unwrap();
        assert!(created, "while touched, its files are created");
    }

    #[test]
    fn a_change_touches_the_location_its_insides_and_the_folders_it_is_in() {
        let location = Path::new("/d/pack");
        assert!(touches(Path::new("/d/pack"), location));
        assert!(touches(Path::new("/d/pack/a.bin"), location));
        assert!(touches(Path::new("/d"), location));
        assert!(!touches(Path::new("/d/other"), location));
        assert!(!touches(Path::new("/d/pack2"), location));
    }
}
