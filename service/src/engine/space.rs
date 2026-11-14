//! Free space (#127): each disk's free space against what the torrents downloading to it still
//! need. A warning, never a block: nothing here refuses or stops anything.
//!
//! There's no machine: whether a disk or a torrent is short follows from the latest facts alone
//! ([`proto::Volume::short`], [`proto::Volume::too_big`]), with nothing remembered between them.
//!
//! The disks are read off the actor: `statfs` and `stat` can hang on a network volume that stops
//! answering, and the actor must go on answering. [`watch`] looks at each destination the actor
//! asks about ([`Wanted`]) in a thread of its own, at most one at a time per destination, and its
//! readings come back to the actor as messages ([`Report`]). One that takes longer than
//! [`STALLED_AFTER`] is reported stalled: its disk's free space is unknown until it answers.
//! Every snapshot then groups the torrents' needs by disk ([`volumes`]).
//!
//! What a torrent needs is what its files still have to take on disk: each file it's getting,
//! less what's already allocated to it ([`allocated`]), whatever librqbit says it has. librqbit
//! sets each file's length when it initialises a torrent, which takes the whole of it on HFS+ and
//! exFAT and nothing on APFS (sparse), so counting what's left of the download would count a
//! preallocated file twice: once as taken, once as needed. And a check reads the files without
//! writing them, so it never raises what a torrent needs.
//!
//! On APFS, the volumes of one container share its free space, and statfs says all of it for
//! each: torrents on two volumes of one container are each held against the whole, not together.
//! Rare (the startup disk's data is one volume), so not added up.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proto::{AbsolutePath, Needs, TorrentId, Volume};
use tokio::sync::{oneshot, watch as channel};

/// How long a look at a destination may take before its disk is taken to be stalled.
pub const STALLED_AFTER: Duration = Duration::from_secs(2);

/// What `statfs` says of a folder: the disk it's on and what's free there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Where its file system is mounted: folders with the same one are on the same disk.
    pub mount: AbsolutePath,
    pub free_bytes: u64,
}

/// What the actor knows of a destination's disk: where it's mounted, and what's free there unless
/// the last look stalled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub mount: AbsolutePath,
    pub free_bytes: Option<u64>,
}

/// The destinations to look at, each with the files whose allocation counts there.
pub type Wanted = BTreeMap<AbsolutePath, BTreeSet<PathBuf>>;

/// What a look found at a destination: its disk (`None` when statfs can't reach it), and the bytes
/// allocated to each file asked about (0 for one that isn't there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub disk: Option<Disk>,
    pub allocated: BTreeMap<PathBuf, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Read(Reading),
    /// The look has taken longer than [`STALLED_AFTER`], and is still going.
    Stalled,
}

/// A reading of one destination, for the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub destination: AbsolutePath,
    pub found: Found,
}

/// How a destination is looked at: [`look`], or in tests one that stalls.
pub type Look = Arc<dyn Fn(&Path, &BTreeSet<PathBuf>) -> Reading + Send + Sync>;

/// statfs of the destination, and stat of each file.
pub fn look() -> Look {
    Arc::new(|destination, files| Reading { disk: disk(destination), allocated: files.iter().map(|f| (f.clone(), allocated(f))).collect() })
}

/// The bytes the file takes on disk: 0 when it isn't there, or a sparse file nothing was written
/// to yet.
pub fn allocated(file: &Path) -> u64 {
    std::fs::symlink_metadata(file).map_or(0, |m| m.blocks() * 512)
}

/// What a torrent still needs on its disk: each of its `files` (its length, what's allocated to
/// it) less what's taken, or `None` while one hasn't been looked at.
pub fn needs(files: impl IntoIterator<Item = (u64, Option<u64>)>) -> Option<u64> {
    files.into_iter().map(|(len, allocated)| Some(len.saturating_sub(allocated?))).sum()
}

/// Looks at each destination `wanted` says every `every`, and at once when it changes, each look
/// in a thread of its own with `look`, and tells `report` what it found: at most one look of a
/// destination at a time, and while one has taken longer than [`STALLED_AFTER`], that it's
/// stalled. Ends when `wanted`'s sender goes, or `report` says nobody's listening.
pub fn watch(look: Look, mut wanted: channel::Receiver<Wanted>, every: Duration, report: impl Fn(Report) -> bool + Send + Sync + 'static) {
    let report = Arc::new(report);
    // The destinations being looked at, since when.
    let looking: Arc<Mutex<BTreeMap<AbsolutePath, Instant>>> = Arc::default();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                changed = wanted.changed() => if changed.is_err() { return },
            }
            let now = wanted.borrow_and_update().clone();
            for (destination, files) in now {
                if let Some(since) = looking.lock().unwrap().get(&destination).copied() {
                    if since.elapsed() >= STALLED_AFTER && !report(Report { destination, found: Found::Stalled }) {
                        return;
                    }
                    continue;
                }
                looking.lock().unwrap().insert(destination.clone(), Instant::now());
                let (found, reading) = oneshot::channel();
                let (look, at) = (look.clone(), destination.clone());
                // Not tokio's blocking pool: a look that never returns mustn't hold up its shutdown.
                std::thread::spawn(move || {
                    let _ = found.send(look(&at, &files));
                });
                let (report, looking) = (report.clone(), looking.clone());
                tokio::spawn(async move {
                    let reading = reading.await;
                    looking.lock().unwrap().remove(&destination);
                    if let Ok(reading) = reading {
                        report(Report { destination, found: Found::Read(reading) });
                    }
                });
            }
        }
    });
}

/// The disk `folder` is on, or `None` when it can't be reached.
pub fn disk(folder: &Path) -> Option<Disk> {
    let path = std::ffi::CString::new(folder.as_os_str().as_bytes()).ok()?;
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a C string that lives through the call, and `fs` is a place for the answer.
    if unsafe { libc::statfs(path.as_ptr(), &mut fs) } != 0 {
        return None;
    }
    // SAFETY: statfs fills `f_mntonname` with a NUL-terminated path.
    let mount = unsafe { CStr::from_ptr(fs.f_mntonname.as_ptr()) };
    let mount = AbsolutePath::new(PathBuf::from(std::ffi::OsStr::from_bytes(mount.to_bytes()))).ok()?;
    Some(Disk { mount, free_bytes: fs.f_bavail * u64::from(fs.f_bsize) })
}

/// Each disk with the torrents that still need space on it, by mount: `needs` is each torrent's
/// destination and what it still needs, `disks` what's known of each destination. A torrent that
/// needs nothing, or whose destination isn't on a known disk, counts nowhere. A disk's free space
/// is unknown only when no destination on it says it.
pub fn volumes<'a>(
    needs: impl IntoIterator<Item = (TorrentId, &'a AbsolutePath, u64)>,
    disks: &BTreeMap<AbsolutePath, Known>,
) -> Vec<Volume> {
    let mut by_disk: BTreeMap<&AbsolutePath, Volume> = BTreeMap::new();
    for (id, destination, bytes) in needs {
        let Some(disk) = disks.get(destination).filter(|_| bytes > 0) else { continue };
        let volume =
            by_disk.entry(&disk.mount).or_insert_with(|| Volume { mount: disk.mount.clone(), free_bytes: None, needs: Vec::new() });
        volume.free_bytes = volume.free_bytes.or(disk.free_bytes);
        volume.needs.push(Needs { id, bytes });
    }
    let mut volumes: Vec<Volume> = by_disk.into_values().collect();
    for volume in &mut volumes {
        volume.needs.sort_by_key(|n| n.id);
    }
    volumes
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Destinations, spread over three disks.
    fn destination(i: usize) -> AbsolutePath {
        AbsolutePath::new(format!("/d{i}")).unwrap()
    }

    fn disks(free: [u64; 3], on: [usize; 5]) -> BTreeMap<AbsolutePath, Known> {
        (0..5)
            .map(|i| {
                let d = on[i];
                (destination(i), Known { mount: AbsolutePath::new(format!("/m{d}")).unwrap(), free_bytes: Some(free[d]) })
            })
            .collect()
    }

    proptest! {
        /// Every torrent that needs something is on its disk's volume with what it needs, and on no
        /// other; a disk is short exactly when its torrents' needs add up to more than is free, and
        /// a torrent too big exactly when it alone needs more.
        #[test]
        fn each_disk_has_its_torrents_needs_against_its_free_space(
            free in proptest::array::uniform3(0..100u64),
            on in proptest::array::uniform5(0..3usize),
            needs in proptest::collection::vec((0..5usize, 0..60u64), 0..12),
        ) {
            let disks = disks(free, on);
            let destinations: Vec<AbsolutePath> = (0..5).map(destination).collect();
            let volumes = volumes(needs.iter().enumerate().map(|(id, &(d, b))| (id as TorrentId, &destinations[d], b)), &disks);
            for (id, &(d, bytes)) in needs.iter().enumerate() {
                let holding: Vec<&Volume> = volumes.iter().filter(|v| v.needs.iter().any(|n| n.id == id as TorrentId)).collect();
                if bytes == 0 {
                    prop_assert!(holding.is_empty(), "torrent {} needs nothing", id);
                    continue;
                }
                prop_assert_eq!(holding.len(), 1);
                prop_assert_eq!(&holding[0].mount, &disks[&destinations[d]].mount);
                prop_assert_eq!(holding[0].too_big(id as TorrentId), (bytes > free[on[d]]).then_some(bytes));
            }
            for disk in 0..3 {
                let total: u64 = needs.iter().filter(|(d, _)| on[*d] == disk).map(|(_, b)| b).sum();
                let mount = AbsolutePath::new(format!("/m{disk}")).unwrap();
                let volume = volumes.iter().find(|v| v.mount == mount);
                prop_assert_eq!(volume.is_some(), total > 0, "a volume exactly for a disk that's needed");
                if let Some(volume) = volume {
                    prop_assert_eq!(volume.needs_bytes(), total);
                    prop_assert_eq!(volume.short(), total > free[disk]);
                }
            }
        }
    }

    proptest! {
        /// A torrent needs what its files still have to take: each one's length less what's
        /// allocated to it, never below nothing (a preallocated file needs nothing more, one with
        /// more blocks than bytes no less); unknown while one of them hasn't been looked at.
        #[test]
        fn a_torrent_needs_its_files_less_what_they_take(files in proptest::collection::vec((0..1000u64, proptest::option::of(0..1200u64)), 0..6)) {
            let want = files.iter().all(|(_, a)| a.is_some()).then(|| files.iter().map(|(len, a)| len.saturating_sub(a.unwrap())).sum::<u64>());
            prop_assert_eq!(needs(files.iter().copied()), want);
        }

        /// A destination whose look stalled leaves its disk's free space unknown, unless another
        /// destination on the same disk says it: then nothing there is short.
        #[test]
        fn a_disk_nobody_could_read_has_its_free_space_unknown(stalled in proptest::array::uniform5(any::<bool>()), on in proptest::array::uniform5(0..3usize)) {
            let mut disks = disks([1, 1, 1], on);
            for (i, &s) in stalled.iter().enumerate() {
                if s {
                    disks.get_mut(&destination(i)).unwrap().free_bytes = None;
                }
            }
            let destinations: Vec<AbsolutePath> = (0..5).map(destination).collect();
            let volumes = volumes((0..5).map(|i| (i as TorrentId, &destinations[i], 10)), &disks);
            for volume in volumes {
                let known = (0..5).any(|i| !stalled[i] && disks[&destinations[i]].mount == volume.mount);
                prop_assert_eq!(volume.free_bytes.is_some(), known);
                prop_assert_eq!(volume.short(), known);
            }
        }
    }

    /// The issue's table, pinned as it's written: 5 free; 6 then 2 warns the 6 and the disk, 4 then 2 the disk only.
    #[test]
    fn the_issues_examples() {
        let disks = disks([5, 0, 0], [0; 5]);
        let at = destination(0);
        let big = volumes([(0, &at, 6), (1, &at, 2)], &disks).remove(0);
        assert_eq!((big.short(), big.too_big(0), big.too_big(1)), (true, Some(6), None));
        let small = volumes([(0, &at, 4), (1, &at, 2)], &disks).remove(0);
        assert_eq!((small.short(), small.too_big(0), small.too_big(1)), (true, None, None));
    }

    /// statfs itself, which can't be generated: a temp folder is on a disk whose mount is on that
    /// same disk (not always above it as a path: macOS links `/private/var` into the data volume),
    /// and a missing folder on none.
    #[test]
    fn statfs_names_the_disk_a_folder_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let disk = disk(dir.path()).expect("a disk");
        assert_eq!(super::disk(&disk.mount).map(|d| d.mount), Some(disk.mount.clone()));
        assert!(disk.free_bytes > 0);
        assert_eq!(super::disk(&dir.path().join("missing")), None);
    }
}
