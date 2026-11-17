//! Spotting a torrent's files deleted outside Phantom. librqbit opens each file once and keeps it
//! open, so it never notices one deleted, renamed or replaced: it goes on writing into the deleted
//! file and reports it complete. So the engine size-checks them itself, by name and size:
//!
//! - A size check reports a [`Disk`]: the files a torrent is getting (only those whose priority
//!   is Normal) and what's at each one's path; the machine (`machine/`) decides what it means.
//!   [`Found::is_missing`] says a file isn't on disk as it must be: nothing is at its path,
//!   something other than a file is, or the file's size is one it can't have: larger than its
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
//! - At stop the store records each torrent's files as [`Expected`]; the next start checks them
//!   before librqbit restores the torrents, because restoring creates a missing file again, empty.
//! - A [`Destination`] is parsed once where a client names it: an absolute path to a folder that
//!   can be reached then. Code past that never checks it again.
//! - Before its files, a torrent's destination: [`unavailable`] says why it can't be reached (a
//!   drive not connected, a folder gone, no permission). Its files are then not missing, just out of reach.
//!   [`Guard`] keeps librqbit from creating the files of a torrent whose destination is unavailable,
//!   even one that goes while librqbit creates them. It also keeps librqbit off a torrent's files
//!   once its Delete begins, through the storage librqbit already has too: a file in the Trash
//!   stays as it went, so Undo can tell it's the one.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
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

/// Size-checks `files` now: each, and what's at its path.
pub fn disk_of(files: &[Expected]) -> Disk {
    Disk { files: files.iter().map(|f| (f.index, Found { file: f.clone(), at: stamp(&f.path) })).collect() }
}

/// The regular file at `path`, if there is one.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(Stamp { inode: m.ino(), len: m.len(), modified: m.modified().ok() })
}

/// The indices of the files in `files` that aren't on disk as they must be.
pub fn missing(files: &[Expected]) -> BTreeSet<usize> {
    disk_of(files).files.into_iter().filter(|(_, f)| f.is_missing()).map(|(i, _)| i).collect()
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

/// librqbit's file storage, except for the torrents whose destination can't be reached when
/// librqbit creates their storage: those get storage that touches nothing on disk. librqbit
/// creates a torrent's files, and the folders up to them, whenever it adds one, restoring it at
/// start too; for a drive that isn't connected that would be a folder of its name on the internal
/// disk. Reading and writing fail, so a torrent added that way has nothing until it's checked
/// again, once its destination is back. The destination can still go between then and librqbit
/// creating the files: [`init_in`] takes out what librqbit made again, and the storage then touches
/// nothing either.
///
/// An add again ([`Guard::for_add`]) knows the torrent's folder and its priorities, and so does a
/// restore at start ([`Guard::restore`]), so its files are [`Files`]: those set Off aren't made
/// (#67, #158).
#[derive(Clone)]
pub struct Guard {
    /// Each torrent's destination, by info hash.
    destinations: Arc<Mutex<HashMap<String, AbsolutePath>>>,
    /// Whether each torrent's storage touches nothing, wherever its destination: one a Delete is
    /// trashing, or left partly in the Trash. Shared with the storage librqbit has already.
    untouched: Arc<Mutex<HashMap<String, Untouched>>>,
    init_in: InitIn,
    /// A test's hold on librqbit reading a torrent's files, as a check does: called before each read.
    reads: Option<ReadHook>,
    /// A test's hold on librqbit changing a torrent's files: called before each write, length
    /// set or removal.
    touches: Option<ReadHook>,
    /// A test's hold on librqbit setting a file's length, once `Files` has found the file in place.
    lengths: Option<ReadHook>,
    /// The one torrent's layout, for an add that knows it.
    layout: Option<Layout>,
    /// The layouts of the torrents librqbit restores at start, by info hash, until it's done.
    restoring: Arc<Mutex<HashMap<String, Layout>>>,
    /// How many storages it made that aren't Phantom's own (`Files`), for a test.
    #[cfg(test)]
    unguarded: Arc<std::sync::atomic::AtomicUsize>,
}

/// Where a torrent's files go and which it gets.
#[derive(Clone)]
struct Layout {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    folder: PathBuf,
    /// The files it gets (those Normal); `None`: all of them.
    normal: Option<Arc<BTreeSet<usize>>>,
}

impl Layout {
    fn new(folder: PathBuf, normal: Option<&[usize]>) -> Self {
        Self { folder, normal: normal.map(|n| Arc::new(n.iter().copied().collect())) }
    }

    fn is_off(&self, index: usize) -> bool {
        self.normal.as_ref().is_some_and(|normal| !normal.contains(&index))
    }
}

/// A test's hold on reads: it returns once a read may go on.
pub type ReadHook = Arc<dyn Fn() + Send + Sync>;

/// Where a test holds checks: each read of a file waits here while the gate is held, so a check
/// stays under way (Checking) until the test opens it.
#[cfg(test)]
#[derive(Default)]
pub struct ReadGate {
    held: Mutex<bool>,
    opened: std::sync::Condvar,
    /// How many wait there now.
    waiting: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl ReadGate {
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

    fn wait(&self) {
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
            destinations: Arc::default(),
            untouched: Arc::default(),
            init_in,
            reads: None,
            touches: None,
            lengths: None,
            layout: None,
            restoring: Arc::default(),
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

    /// With every read of a file, which a check does piece by piece, waiting at `gate` while it's
    /// held: a test holds a check there.
    #[cfg(test)]
    pub fn with_reads(self, gate: Arc<ReadGate>) -> Self {
        Self { reads: Some(Arc::new(move || gate.wait())), ..self }
    }

    /// With every length librqbit sets, once the file is found in place, waiting at `gate` while
    /// it's held: a test holds one there.
    #[cfg(test)]
    pub fn with_lengths(self, gate: Arc<ReadGate>) -> Self {
        Self { lengths: Some(Arc::new(move || gate.wait())), ..self }
    }

    /// With every change librqbit makes to a file (a write, its length set, a removal) waiting at
    /// `gate` while it's held: a test holds one under way there.
    #[cfg(test)]
    pub fn with_touches(self, gate: Arc<ReadGate>) -> Self {
        Self { touches: Some(Arc::new(move || gate.wait())), ..self }
    }

    /// For one add of a torrent into librqbit's output `folder`, getting the `normal` files
    /// (`None`: all of them), as librqbit's `only_files`.
    pub fn for_add(&self, folder: PathBuf, normal: Option<&[usize]>) -> Self {
        Self { layout: Some(Layout::new(folder, normal)), ..self.clone() }
    }

    /// The torrent librqbit is about to restore at start goes into `folder`, getting the `normal` files,
    /// as in [`Guard::for_add`].
    pub fn restore(&self, info_hash: &str, folder: PathBuf, normal: Option<&[usize]>) {
        self.restoring.lock().unwrap().insert(info_hash.to_owned(), Layout::new(folder, normal));
    }

    /// librqbit has restored the torrents: an add from now on says its own layout.
    pub fn restored(&self) {
        self.restoring.lock().unwrap().clear();
    }

    /// Where the torrent's files go; `None` once it's off the list.
    pub fn set_destination(&self, info_hash: &str, destination: Option<AbsolutePath>) {
        let mut destinations = self.destinations.lock().unwrap();
        match destination {
            Some(destination) => destinations.insert(info_hash.to_owned(), destination),
            None => destinations.remove(info_hash),
        };
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
            drop(flag.touches.write().unwrap_or_else(std::sync::PoisonError::into_inner));
        }
    }

    /// What a Move to Trash holds while it moves each file of the torrent: no write or length change
    /// of librqbit's is under way meanwhile, and once a file has moved none reaches it (`Files`
    /// tells it's gone from its path). On the blocking task that moves it, never the actor's.
    pub fn moving(&self, info_hash: &str) -> Moving {
        Moving(self.untouched_of(info_hash))
    }

    /// The files at `paths`, which a Move to Trash took, are the torrent's again: put back by its
    /// undo. librqbit may make them again from now on. (One asked for again, set Normal, is missing
    /// until a check, which adds the torrent again: that opens it at its path anyway.)
    pub fn back<'a>(&self, info_hash: &str, paths: impl IntoIterator<Item = &'a Path>) {
        self.untouched_of(info_hash).trashed(|trashed| {
            for path in paths {
                trashed.retain(|t| !path.starts_with(t));
            }
        });
    }

    /// The files at `paths` were taken by a Move to Trash its undo may put back: at a start, as the
    /// undo stack says.
    pub fn taken<'a>(&self, info_hash: &str, paths: impl IntoIterator<Item = &'a Path>) {
        self.untouched_of(info_hash).trashed(|t| t.extend(paths.into_iter().map(Path::to_owned)));
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
        let destination = self.destinations.lock().unwrap().get(hash).cloned();
        let layout = self.layout.clone().or_else(|| self.restoring.lock().unwrap().get(hash).cloned());
        let files: Box<dyn TorrentStorage> = match (layout, &destination) {
            (Some(layout), Some(d)) => {
                let mut files = Files::new(layout, d.to_path_buf(), self.untouched_of(hash));
                files.lengths = self.lengths.clone();
                Box::new(files)
            }
            _ => {
                #[cfg(test)]
                self.unguarded.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Box::new(FilesystemStorageFactory::default().create(shared, metadata)?)
            }
        };
        Ok(match destination {
            Some(d) if unavailable(&d).is_some() => Box::new(UnavailableStorage),
            Some(d) => {
                let (init_in, reads, touches) = (self.init_in.clone(), self.reads.clone(), self.touches.clone());
                Box::new(Guarded { destination: d.to_path_buf(), storage: files, init_in, reads, touches })
            }
            None => files,
        })
    }
}

/// Whether a torrent's storage touches nothing on disk now ([`Guard::keep_untouched`]).
type Untouched = Arc<UntouchedFlag>;

/// A torrent's files, held still around each move ([`Guard::moving`]).
pub struct Moving(Untouched);

impl Moving {
    /// Nothing of librqbit's changes the torrent's files until this is dropped. Once it is, every
    /// storage of the torrent looks again whether its files are where it opened them.
    pub fn hold(&self) -> MoveHold<'_> {
        // Poisoned only by a panic while a move held it: it guards no data, so that means nothing.
        let held = self.0.touches.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        MoveHold { flag: &self.0, _held: held }
    }

    /// The file at `path` was taken to the Trash while held: never made again there by librqbit
    /// until it's back ([`Guard::back`]).
    pub fn took(&self, path: &Path) {
        self.0.trashed(|t| {
            t.insert(path.to_owned());
        });
    }
}

/// [`Moving::hold`]'s hold, until it's dropped.
pub struct MoveHold<'a> {
    flag: &'a UntouchedFlag,
    _held: std::sync::RwLockWriteGuard<'a, ()>,
}

impl Drop for MoveHold<'_> {
    fn drop(&mut self) {
        self.flag.moves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Default)]
struct UntouchedFlag {
    on: std::sync::atomic::AtomicBool,
    /// Held to read by each change to a file while it's made, to write once `on` is set, or while a
    /// Move to Trash moves a file: so that waits for the change under way.
    touches: RwLock<()>,
    /// How many files Moves to Trash have moved: a storage looks whether its files are still where
    /// it opened them only once this has changed, so a torrent nothing moved pays nothing.
    moves: std::sync::atomic::AtomicU64,
    /// The paths of the files a Move to Trash took that its undo may put back: none is made again
    /// there, even for a piece the file shares with one the torrent gets.
    trashed: Mutex<BTreeSet<PathBuf>>,
    /// How many are in `trashed`, so a torrent with none pays nothing to ask.
    trashed_count: std::sync::atomic::AtomicUsize,
}

impl UntouchedFlag {
    fn trashed(&self, change: impl FnOnce(&mut BTreeSet<PathBuf>)) {
        let mut trashed = self.trashed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut trashed);
        self.trashed_count.store(trashed.len(), std::sync::atomic::Ordering::SeqCst);
    }

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
    reads: Option<ReadHook>,
    touches: Option<ReadHook>,
}

impl Guarded {
    fn touch(&self) {
        if let Some(touches) = &self.touches {
            touches();
        }
    }
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
        if let Some(reads) = &self.reads {
            reads();
        }
        self.storage.pread_exact(file, offset, buf)
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        self.touch();
        self.storage.pwrite_all(file, offset, buf)
    }

    fn pwrite_all_vectored(&self, file: usize, offset: u64, bufs: [std::io::IoSlice<'_>; 2]) -> Result<usize> {
        self.touch();
        self.storage.pwrite_all_vectored(file, offset, bufs)
    }

    fn remove_file(&self, file: usize, path: &Path) -> Result<()> {
        self.touch();
        self.storage.remove_file(file, path)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        self.storage.remove_directory_if_empty(path)
    }

    fn ensure_file_length(&self, file: usize, len: u64) -> Result<()> {
        self.touch();
        self.storage.ensure_file_length(file, len)
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Guarded {
            destination: self.destination.clone(),
            storage: self.storage.take()?,
            init_in: self.init_in.clone(),
            reads: self.reads.clone(),
            touches: self.touches.clone(),
        }))
    }
}

/// Runs librqbit's `init`, which creates the torrent's files under `destination` and every folder
/// up to them, the destination too. If the destination isn't the folder it was just before, it
/// went, and `init` made it again ([`made_again`]): what's there is taken out (the files `init`
/// creates are empty, and only empty files and folders are removed). Whether the files are in the
/// destination.
pub fn init_in(destination: &Path, init: &mut dyn FnMut() -> Result<()>) -> Result<bool> {
    let Some(before) = folder(destination) else { return Ok(false) };
    let made = init();
    if made_again(before, folder(destination), || drive(destination).is_some_and(|(_, mounted)| mounted)) {
        take_out_empty(destination);
        return Ok(false);
    }
    made.map(|()| true)
}

/// Removes the empty files under `dir`, then the folders left empty, `dir` too.
fn take_out_empty(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        match entry.metadata() {
            Ok(m) if m.is_dir() => take_out_empty(&entry.path()),
            Ok(m) if m.is_file() && m.len() == 0 => {
                let _ = std::fs::remove_file(entry.path());
            }
            _ => {}
        }
    }
    let _ = std::fs::remove_dir(dir);
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

/// librqbit's file storage (`FilesystemStorage`), except for the files set Off: `init` opens one
/// only if it's there, and never makes it, so a check (which adds the torrent again) doesn't bring
/// back, empty, one deleted (#67). One that isn't there is made when librqbit writes into it, as
/// for a piece it shares with a file the torrent gets (a stub, of that file's length: librqbit
/// only sets the length of the files it gets), with the folders to it, but never the destination.
/// Nothing is ever removed but what librqbit asks.
///
/// A file moved from where it was opened is never changed where it went (#217's fourth review):
/// librqbit keeps it open, and a write or a length set through it would change it there, so a Move
/// to Trash's undo wouldn't find it as it went. Its length isn't set, and a read fails, as for a
/// file not there. One a Move to Trash took is never made again at its path either, until it's
/// back: what's written into it (a piece it shares with a file the torrent gets) is kept in memory
/// instead, so that piece checks, and is read from there (#217's fifth review).
struct Files {
    layout: Layout,
    destination: PathBuf,
    /// Each file, by index, as opened; padding (BEP 47) is never opened.
    files: Vec<Slot>,
    /// The torrent's moves, shared with every storage of it.
    flag: Untouched,
    lengths: Option<ReadHook>,
}

struct Slot {
    /// `None`: padding.
    path: Option<PathBuf>,
    file: RwLock<Opened>,
    /// The torrent's moves when the file was last found where it was opened.
    seen: std::sync::atomic::AtomicU64,
    /// What librqbit wrote into it while a Move to Trash had it, by offset.
    kept: Mutex<BTreeMap<u64, Vec<u8>>>,
}

impl Slot {
    fn new(path: Option<PathBuf>, file: Opened, moves: u64) -> Self {
        Self { path, file: RwLock::new(file), seen: moves.into(), kept: Mutex::default() }
    }
}

enum Opened {
    File(std::fs::File),
    /// Not on disk at `init`, or padding: made when written.
    NotThere,
    /// Handed on by `take`: this storage is done with it.
    Taken,
}

/// Where a file a slot has stands.
enum Place<'a> {
    /// Open, where it was opened.
    InPlace(&'a std::fs::File),
    /// Taken by a Move to Trash, which may put it back: never made again at its path.
    Trashed,
    /// Moved or gone some other way (by hand), or never there: made again where it's written.
    Elsewhere,
}

impl Files {
    fn new(layout: Layout, destination: PathBuf, flag: Untouched) -> Self {
        Self { layout, destination, files: Vec::new(), flag, lengths: None }
    }

    fn slot(&self, index: usize) -> Result<&Slot> {
        self.files.get(index).with_context(|| format!("no file {index}"))
    }

    fn moves(&self) -> u64 {
        self.flag.moves.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Where `slot`'s file, as `opened`, stands now. Looked at on disk only once a Move to Trash has
    /// moved something of the torrent since it was last found in place: one moved by hand is then
    /// noticed at the next size check, as Missing files.
    /// One a Move to Trash took is looked at every time until it's the torrent's again: its undo
    /// puts it back, and may take it back to the Trash, without a move.
    fn place<'a>(&self, slot: &Slot, opened: &'a Opened) -> Place<'a> {
        let moves = self.moves();
        let trashed = self.trashed(slot);
        if let Opened::File(file) = opened
            && ((!trashed && slot.seen.load(std::sync::atomic::Ordering::SeqCst) == moves) || !moved(slot, file))
        {
            slot.seen.store(moves, std::sync::atomic::Ordering::SeqCst);
            return Place::InPlace(file);
        }
        if trashed { Place::Trashed } else { Place::Elsewhere }
    }

    /// Whether `slot`'s file is one a Move to Trash took, or in a folder it took.
    fn trashed(&self, slot: &Slot) -> bool {
        if self.flag.trashed_count.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return false;
        }
        let trashed = self.flag.trashed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.path.as_ref().is_some_and(|p| p.ancestors().any(|a| trashed.contains(a)))
    }

    /// Writes `bufs`, one after another, at `offset` into file `index`, made first if it isn't
    /// there; into memory if a Move to Trash has it. Where it is is looked at once.
    fn write(&self, index: usize, offset: u64, bufs: &[&[u8]]) -> Result<()> {
        let slot = self.slot(index)?;
        {
            let opened = slot.file.read().unwrap();
            match self.place(slot, &opened) {
                Place::InPlace(file) => return Ok(write_at(file, offset, bufs)?),
                Place::Trashed => {
                    let mut kept = slot.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    kept.insert(offset, bufs.concat());
                    return Ok(());
                }
                Place::Elsewhere => {}
            }
        }
        self.made(slot, index)?;
        match &*slot.file.read().unwrap() {
            Opened::File(f) => Ok(write_at(f, offset, bufs)?),
            _ => bail!("file {index} was handed on"),
        }
    }

    /// Makes file `index` again at its path: one moved by hand, or never there.
    fn made(&self, slot: &Slot, index: usize) -> Result<()> {
        let mut opened = slot.file.write().unwrap();
        if let Place::InPlace(_) = self.place(slot, &opened) {
            return Ok(());
        }
        if let Opened::Taken = *opened {
            bail!("file {index} was handed on");
        }
        let path = slot.path.as_ref().context("a padding file is never written")?;
        make_folders(&self.destination, path.parent().context("a file without a folder")?)?;
        *opened = Opened::File(open(path, true)?);
        slot.seen.store(self.moves(), std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

/// Whether `file`, opened at `slot`'s path, is no longer what's there: moved (to the Trash, say), or
/// gone. A symbolic link at the path is followed, as librqbit's own storage does.
fn moved(slot: &Slot, file: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(path) = &slot.path else { return false };
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(open), Ok(there)) => (open.dev(), open.ino()) != (there.dev(), there.ino()),
        _ => true,
    }
}

fn write_at(file: &std::fs::File, mut offset: u64, bufs: &[&[u8]]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    for buf in bufs {
        file.write_all_at(buf, offset)?;
        offset += buf.len() as u64;
    }
    Ok(())
}

/// `buf` from what `kept` holds from `offset`, if it holds all of it.
fn read_kept(kept: &BTreeMap<u64, Vec<u8>>, offset: u64, buf: &mut [u8]) -> bool {
    let mut at = 0usize;
    while at < buf.len() {
        let pos = offset + at as u64;
        let Some((start, bytes)) = kept.range(..=pos).next_back() else { return false };
        let from = (pos - start) as usize;
        if from >= bytes.len() {
            return false;
        }
        let n = (bytes.len() - from).min(buf.len() - at);
        buf[at..at + n].copy_from_slice(&bytes[from..from + n]);
        at += n;
    }
    true
}

/// The file at `path`, for reading and writing, `create`d if it isn't there (but not the folders
/// to it), never truncated.
fn open(path: &Path, create: bool) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).create(create).truncate(false).open(path)
}

/// Makes the folders from `destination` down to `dir`, one at a time, so a destination that's
/// gone is never made again.
fn make_folders(destination: &Path, dir: &Path) -> Result<()> {
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
        let mut files = Vec::with_capacity(metadata.file_infos.len());
        for (index, info) in metadata.file_infos.iter().enumerate() {
            let moves = self.moves();
            if info.attrs.padding {
                files.push(Slot::new(None, Opened::NotThere, moves));
                continue;
            }
            let path = self.layout.folder.join(&info.relative_filename);
            let file = if self.layout.is_off(index) {
                match open(&path, false) {
                    Ok(file) => Opened::File(file),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Opened::NotThere,
                    Err(e) => return Err(e).with_context(|| format!("opening {path:?}")),
                }
            } else {
                std::fs::create_dir_all(path.parent().context("a file without a folder")?)?;
                Opened::File(open(&path, true).with_context(|| format!("opening {path:?}"))?)
            };
            files.push(Slot::new(Some(path), file, moves));
        }
        self.files = files;
        Ok(())
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        use std::os::unix::fs::FileExt;
        let slot = self.slot(file)?;
        let opened = slot.file.read().unwrap();
        match (self.place(slot, &opened), &*opened) {
            (Place::InPlace(f), _) => Ok(f.read_exact_at(buf, offset)?),
            // Taken to the Trash: only what was written meanwhile. Anything else is gone for a check
            // too, which its undo then checks again.
            (Place::Trashed, _) => {
                if read_kept(&slot.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner), offset, buf) {
                    return Ok(());
                }
                bail!("file {file} is in the Trash")
            }
            (Place::Elsewhere, Opened::File(_)) => bail!("file {file} moved from its path"),
            (Place::Elsewhere, Opened::NotThere) => bail!("file {file} isn't on disk"),
            (Place::Elsewhere, Opened::Taken) => bail!("file {file} was handed on"),
        }
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
        Ok(std::fs::remove_file(self.layout.folder.join(path))?)
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
        // Nor one moved from its path: librqbit sets the length as its check ends, even a check
        // paused, and one moved to the Trash meanwhile stays as it went.
        let slot = self.slot(file)?;
        {
            let opened = slot.file.read().unwrap();
            match (self.place(slot, &opened), &*opened) {
                (Place::InPlace(f), _) => {
                    if let Some(lengths) = &self.lengths {
                        lengths();
                    }
                    return Ok(f.set_len(len)?);
                }
                (Place::Trashed, _) | (Place::Elsewhere, Opened::File(_)) => return Ok(()),
                (Place::Elsewhere, Opened::NotThere | Opened::Taken) => {}
            }
        }
        self.made(slot, file)?;
        match &*slot.file.read().unwrap() {
            Opened::File(f) => Ok(f.set_len(len)?),
            _ => bail!("file {file} was handed on"),
        }
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        let files = self
            .files
            .iter()
            .map(|s| {
                let taken = std::mem::replace(&mut *s.file.write().unwrap(), Opened::Taken);
                let slot = Slot::new(s.path.clone(), taken, s.seen.load(std::sync::atomic::Ordering::SeqCst));
                *slot.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                    s.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                slot
            })
            .collect();
        Ok(Box::new(Files {
            layout: self.layout.clone(),
            destination: self.destination.clone(),
            files,
            flag: self.flag.clone(),
            lengths: self.lengths.clone(),
        }))
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

    fn abs(path: &Path) -> AbsolutePath {
        AbsolutePath::new(path).unwrap()
    }

    fn expected(path: PathBuf, size_bytes: u64, progress_bytes: u64) -> Expected {
        Expected { index: 0, path, size_bytes, progress_bytes }
    }

    fn found(path: PathBuf, size_bytes: u64, progress_bytes: u64) -> Found {
        disk_of(&[expected(path, size_bytes, progress_bytes)]).files.remove(&0).unwrap()
    }

    /// #217's fourth and fifth reviews: a file a Move to Trash took while open is left as it went.
    /// librqbit's length set skips it, a read of it fails, and a write (a piece it shares with a
    /// file the torrent gets) makes nothing at its path: it's kept in memory, and read from there.
    /// Once it's back, it's the torrent's file again.
    #[test]
    fn a_file_a_move_to_trash_took_is_left_as_it_went() {
        let dir = tempfile::tempdir().unwrap();
        let (path, away) = (dir.path().join("a.bin"), dir.path().join("away.bin"));
        std::fs::write(&path, [1; 100]).unwrap();
        let guard = Guard::default();
        let slot = Slot::new(Some(path.clone()), Opened::File(open(&path, false).unwrap()), 0);
        let mut files = Files::new(Layout::new(dir.path().to_owned(), None), dir.path().to_owned(), guard.untouched_of("t"));
        files.files = vec![slot];
        let moving = guard.moving("t");
        {
            let _held = moving.hold();
            std::fs::rename(&path, &away).unwrap();
            moving.took(&path);
        }
        let went = std::fs::metadata(&away).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        files.ensure_file_length(0, 100).unwrap();
        assert!(!path.exists(), "its length isn't set at its path either");
        let mut buf = [0; 10];
        assert!(files.pread_exact(0, 0, &mut buf).is_err(), "nothing is read of it");
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        files.pwrite_all_vectored(0, 10, [std::io::IoSlice::new(&[3; 5]), std::io::IoSlice::new(&[4; 5])]).unwrap();
        assert!(!path.exists(), "a write makes nothing at its path");
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [2; 10], "what was written is read back");
        assert!(files.pread_exact(0, 15, &mut [0; 10]).is_err(), "and nothing more");
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "the one moved stays as it went");
        assert_eq!(std::fs::metadata(&away).unwrap().modified().unwrap(), went, "modified time too");
        // Put back, it's the torrent's again, as it went: what was written meanwhile isn't (its undo
        // checks the torrent, which gets that piece again).
        std::fs::rename(&away, &path).unwrap();
        guard.back("t", [path.as_path()]);
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [1; 10]);
        assert_eq!(std::fs::read(&path).unwrap(), [1; 100]);
        files.ensure_file_length(0, 50).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 50);
    }

    /// An undo that fails once a file is back takes it to the Trash again, with no move: until its
    /// undo is over, such a file is looked at every time, and so is never written where it went.
    #[test]
    fn a_file_taken_back_to_the_trash_by_a_failed_undo_is_left_as_it_went() {
        let dir = tempfile::tempdir().unwrap();
        let (path, away) = (dir.path().join("a.bin"), dir.path().join("away.bin"));
        std::fs::write(&path, [1; 100]).unwrap();
        let guard = Guard::default();
        let slot = Slot::new(Some(path.clone()), Opened::File(open(&path, false).unwrap()), 0);
        let mut files = Files::new(Layout::new(dir.path().to_owned(), None), dir.path().to_owned(), guard.untouched_of("t"));
        files.files = vec![slot];
        let moving = guard.moving("t");
        {
            let _held = moving.hold();
            std::fs::rename(&path, &away).unwrap();
            moving.took(&path);
        }
        // Back for a while (the undo), and read there.
        std::fs::rename(&away, &path).unwrap();
        files.pread_exact(0, 0, &mut [0; 10]).unwrap();
        // Taken back to the Trash (the undo failed).
        std::fs::rename(&path, &away).unwrap();
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "nothing written where it went");
        assert!(!path.exists());
    }

    /// A torrent's file that's a symbolic link is where it was opened, as librqbit's own storage
    /// follows it: never taken for one moved.
    #[test]
    fn a_file_that_is_a_link_is_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (path, real) = (dir.path().join("a.bin"), dir.path().join("real.bin"));
        std::fs::write(&real, [1; 100]).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        let guard = Guard::default();
        let slot = Slot::new(Some(path.clone()), Opened::File(open(&path, false).unwrap()), 0);
        let mut files = Files::new(Layout::new(dir.path().to_owned(), None), dir.path().to_owned(), guard.untouched_of("t"));
        files.files = vec![slot];
        drop(guard.moving("t").hold());
        let mut buf = [0; 10];
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [1; 10]);
    }

    /// S2 of #217's fifth review: a panic while a Move to Trash holds the torrent's files still (the
    /// Trash's own code can panic) leaves the torrent's storage working: the lock guards no data.
    #[test]
    fn a_panic_during_a_move_leaves_the_storage_working() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let guard = Guard::default();
        let slot = Slot::new(Some(path.clone()), Opened::File(open(&path, false).unwrap()), 0);
        let moving = guard.moving("t");
        let panicked = std::thread::scope(|s| {
            s.spawn(|| {
                let _held = moving.hold();
                panic!("the Trash failed");
            })
            .join()
        });
        assert!(panicked.is_err());
        // The storage, and the next move, go on.
        let files = Files {
            files: vec![slot],
            ..Files::new(Layout::new(dir.path().to_owned(), None), dir.path().to_owned(), guard.untouched_of("t"))
        };
        let storage = Untouchable { storage: Box::new(files), untouched: guard.untouched_of("t") };
        storage.pwrite_all(0, 0, &[2; 10]).unwrap();
        storage.ensure_file_length(0, 100).unwrap();
        drop(moving.hold());
        guard.keep_untouched("t", true);
        assert_eq!(std::fs::read(&path).unwrap()[..10], [2; 10]);
    }

    /// A file moved by hand is noticed once a Move to Trash moved something of the torrent (the size
    /// check notices it before): a write then makes a new one at its path, and leaves the moved one.
    #[test]
    fn a_file_moved_by_hand_is_made_again_where_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let (path, away) = (dir.path().join("a.bin"), dir.path().join("away.bin"));
        std::fs::write(&path, [1; 100]).unwrap();
        let guard = Guard::default();
        let slot = Slot::new(Some(path.clone()), Opened::File(open(&path, false).unwrap()), 0);
        let mut files = Files::new(Layout::new(dir.path().to_owned(), None), dir.path().to_owned(), guard.untouched_of("t"));
        files.files = vec![slot];
        std::fs::rename(&path, &away).unwrap();
        drop(guard.moving("t").hold());
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [2; 10]);
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100]);
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
        assert!(found(dir.path().join("b.bin"), 100, 0).is_missing(), "gone, though nothing was downloaded");
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
        recreated.seen = Seen::Was(stamp(&path));
        assert!(!recreated.is_back(stamp(&path)), "the file that was there when judged");
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&path, [1; 100]).unwrap();
        assert!(recreated.is_back(stamp(&path)), "another one since");
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
                std::thread::sleep(Duration::from_millis(30));
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
