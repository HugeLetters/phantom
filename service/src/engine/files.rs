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
//!   even one that goes while librqbit creates them.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    /// Torrents whose storage touches nothing, wherever their destination: those a Delete left
    /// partly in the Trash.
    untouched: Arc<Mutex<HashSet<String>>>,
    init_in: InitIn,
    /// The one torrent's layout, for an add that knows it.
    layout: Option<Layout>,
    /// The layouts of the torrents librqbit restores at start, by info hash, until it's done.
    restoring: Arc<Mutex<HashMap<String, Layout>>>,
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
        Self { destinations: Arc::default(), untouched: Arc::default(), init_in, layout: None, restoring: Arc::default() }
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

    /// Whether librqbit's storage for the torrent touches nothing (`true`) or its files.
    pub fn keep_untouched(&self, info_hash: &str, untouched: bool) {
        let mut set = self.untouched.lock().unwrap();
        if untouched {
            set.insert(info_hash.to_owned());
        } else {
            set.remove(info_hash);
        }
    }
}

impl StorageFactory for Guard {
    type Storage = Box<dyn TorrentStorage>;

    fn create(&self, shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<Self::Storage> {
        let hash = shared.info_hash.as_string();
        if self.untouched.lock().unwrap().contains(&hash) {
            return Ok(Box::new(UnavailableStorage));
        }
        let destination = self.destinations.lock().unwrap().get(&hash).cloned();
        let layout = self.layout.clone().or_else(|| self.restoring.lock().unwrap().get(&hash).cloned());
        let files: Box<dyn TorrentStorage> = match (layout, &destination) {
            (Some(layout), Some(d)) => Box::new(Files::new(layout, d.to_path_buf())),
            _ => Box::new(FilesystemStorageFactory::default().create(shared, metadata)?),
        };
        Ok(match destination {
            Some(d) if unavailable(&d).is_some() => Box::new(UnavailableStorage),
            Some(d) => Box::new(Guarded { destination: d.to_path_buf(), storage: files, init_in: self.init_in.clone() }),
            None => files,
        })
    }

    // librqbit only saves torrents stored in files; these are, or will be once back.
    fn is_type_id(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<FilesystemStorageFactory>()
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
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
struct Files {
    layout: Layout,
    destination: PathBuf,
    /// Each file, by index, as opened; padding (BEP 47) is never opened.
    files: Vec<Slot>,
}

struct Slot {
    /// `None`: padding.
    path: Option<PathBuf>,
    file: RwLock<Opened>,
}

enum Opened {
    File(std::fs::File),
    /// Not on disk at `init`, or padding: made when written.
    NotThere,
    /// Handed on by `take`: this storage is done with it.
    Taken,
}

impl Files {
    fn new(layout: Layout, destination: PathBuf) -> Self {
        Self { layout, destination, files: Vec::new() }
    }

    fn slot(&self, index: usize) -> Result<&Slot> {
        self.files.get(index).with_context(|| format!("no file {index}"))
    }

    /// Runs `f` on the file, made first if it isn't there.
    fn made<R>(&self, index: usize, f: impl FnOnce(&std::fs::File) -> std::io::Result<R>) -> Result<R> {
        let slot = self.slot(index)?;
        if let Opened::File(file) = &*slot.file.read().unwrap() {
            return Ok(f(file)?);
        }
        let mut opened = slot.file.write().unwrap();
        if let Opened::NotThere = *opened {
            let path = slot.path.as_ref().context("a padding file is never written")?;
            make_folders(&self.destination, path.parent().context("a file without a folder")?)?;
            *opened = Opened::File(open(path, true)?);
        }
        match &*opened {
            Opened::File(file) => Ok(f(file)?),
            _ => bail!("file {index} was handed on"),
        }
    }
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
            if info.attrs.padding {
                files.push(Slot { path: None, file: RwLock::new(Opened::NotThere) });
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
            files.push(Slot { path: Some(path), file: RwLock::new(file) });
        }
        self.files = files;
        Ok(())
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        use std::os::unix::fs::FileExt;
        let slot = self.slot(file)?;
        match &*slot.file.read().unwrap() {
            Opened::File(f) => Ok(f.read_exact_at(buf, offset)?),
            Opened::NotThere => bail!("file {file} isn't on disk"),
            Opened::Taken => bail!("file {file} was handed on"),
        }
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        use std::os::unix::fs::FileExt;
        self.made(file, |f| f.write_all_at(buf, offset))
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
        self.made(file, |f| f.set_len(len))
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        let files = self
            .files
            .iter()
            .map(|s| Slot { path: s.path.clone(), file: RwLock::new(std::mem::replace(&mut *s.file.write().unwrap(), Opened::Taken)) })
            .collect();
        Ok(Box::new(Files { layout: self.layout.clone(), destination: self.destination.clone(), files }))
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
