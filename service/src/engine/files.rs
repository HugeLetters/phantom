//! Spotting a torrent's files deleted outside Phantom. librqbit opens each file once and keeps it
//! open, so it never notices one deleted, renamed or replaced: it goes on writing into the deleted
//! file and reports it complete. So the engine looks for itself, by name and size:
//!
//! - [`missing`] says which files aren't on disk as they must be. Only files whose priority is
//!   Normal count. One is missing when nothing is at its path, something other than a file is, or
//!   the file's size is one it can't have: larger than its full size, or smaller than the bytes
//!   librqbit has of it (pieces are written in place, so a file is at least as long as the end of
//!   the last piece it has; after a check librqbit sets each Normal file to its full size). A file
//!   replaced by another of a size that fits isn't caught: that's what Check is for.
//! - A missing file is remembered as [`Gone`]: what librqbit had of it, and what was at its path
//!   when it was last judged. [`is_back`] says it's back when a file of a size it can have is
//!   there and it isn't what was there then (librqbit creates a missing file again when it
//!   restores a torrent, or checks it: that one isn't the file put back). A check then tells
//!   whether it's the file it was.
//! - [`Watcher`] watches the torrents' destinations, recursively, so a torrent's own folder going
//!   is seen too, and reports the paths removed or renamed under them (and some more, see
//!   [`goes`]), so the actor looks at once.
//!   The actor also looks at every torrent every few seconds, for what the watcher misses (a file
//!   cut short, or a destination that couldn't be watched).
//! - At stop the store records each torrent's files as [`Expected`]; the next start checks them
//!   before librqbit restores the torrents, because restoring creates a missing file again, empty.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, event::ModifyKind};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use serde::{Deserialize, Serialize};
use tracing::warn;

/// How long the watcher gathers events before reporting them.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// A file a torrent is getting, and what must be on disk for it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Expected {
    pub index: usize,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// What librqbit has of it.
    pub progress_bytes: u64,
}

/// A missing file, by what librqbit had of it before it went and what was at its path since.
#[derive(Debug, Clone, PartialEq)]
pub struct Gone {
    pub path: PathBuf,
    pub size_bytes: u64,
    /// What librqbit had of it before it went: a file put back must hold at least as much.
    pub progress_bytes: u64,
    pub seen: Seen,
}

/// What was at a missing file's path when it was last judged.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    /// Not looked yet: the next look records what's there (the file librqbit made again when it
    /// restored the torrent) without taking it for the file put back.
    NotYet,
    /// This file (`None`: none).
    Was(Option<Stamp>),
}

/// Which file is at a path, and its size and when it was last changed: another file, or the same
/// one changed, differs in one of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    inode: u64,
    len: u64,
    modified: Option<SystemTime>,
}

impl Gone {
    /// Gone now, with whatever is at its path.
    pub fn now(file: &Expected) -> Self {
        let seen = Seen::Was(stamp(&file.path));
        Self { path: file.path.clone(), size_bytes: file.size_bytes, progress_bytes: file.progress_bytes, seen }
    }
}

/// The regular file at `path`, if there is one.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(Stamp { inode: m.ino(), len: m.len(), modified: m.modified().ok() })
}

/// Whether a file that can be the missing one is at its path: of a size it can have, and not
/// what was there when it was last judged.
pub fn is_back(gone: &Gone) -> bool {
    let Seen::Was(seen) = gone.seen else { return false };
    let Some(now) = stamp(&gone.path) else { return false };
    now.len <= gone.size_bytes && now.len >= gone.progress_bytes && Some(now) != seen
}

/// The indices of the files in `files` that aren't on disk as they must be.
pub fn missing(files: &[Expected]) -> BTreeSet<usize> {
    files.iter().filter(|f| is_missing(f)).map(|f| f.index).collect()
}

pub fn is_missing(file: &Expected) -> bool {
    match std::fs::metadata(&file.path) {
        Ok(m) if m.is_file() => m.len() > file.size_bytes || m.len() < file.progress_bytes,
        _ => true,
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
                let paths: Vec<PathBuf> = events
                    .into_iter()
                    .filter(|e| goes(&e.kind))
                    .flat_map(|e| e.event.paths)
                    .map(as_watched)
                    .collect();
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

    fn expected(path: PathBuf, size_bytes: u64, progress_bytes: u64) -> Expected {
        Expected { index: 0, path, size_bytes, progress_bytes }
    }

    #[test]
    fn a_file_is_missing_when_gone_or_of_a_size_it_cannot_have() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [0; 100]).unwrap();
        assert!(!is_missing(&expected(path.clone(), 100, 100)), "complete");
        assert!(!is_missing(&expected(path.clone(), 200, 50)), "partly downloaded, written up to 100");
        assert!(is_missing(&expected(path.clone(), 50, 0)), "larger than it can be");
        assert!(is_missing(&expected(path.clone(), 200, 150)), "shorter than what librqbit has of it");
        assert!(is_missing(&expected(dir.path().join("b.bin"), 100, 0)), "gone, though nothing was downloaded");
        assert!(is_missing(&expected(dir.path().to_owned(), 100, 0)), "a folder in its place");
    }

    #[test]
    fn a_missing_file_is_back_when_a_file_that_can_be_it_appears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let gone = Gone::now(&expected(path.clone(), 100, 60));
        assert!(!is_back(&gone), "still gone");
        std::fs::write(&path, [0; 50]).unwrap();
        assert!(!is_back(&gone), "too short to hold what librqbit had");
        std::fs::write(&path, [0; 100]).unwrap();
        assert!(is_back(&gone));

        let mut recreated = gone.clone();
        recreated.seen = Seen::NotYet;
        assert!(!is_back(&recreated), "not judged yet");
        recreated.seen = Seen::Was(stamp(&path));
        assert!(!is_back(&recreated), "the file that was there when judged");
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&path, [1; 100]).unwrap();
        assert!(is_back(&recreated), "another one since");
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
