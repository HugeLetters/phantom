//! Free space (#127): each disk's free space against what the torrents downloading to it still
//! need. A warning, never a block: nothing here refuses or stops anything.
//!
//! There's no machine: whether a disk or a torrent is short follows from the latest facts alone
//! ([`proto::Volume::short`], [`proto::Volume::too_big`]), with nothing remembered between them.
//! The actor records what `statfs` says of each destination when it polls ([`disk`]), and every
//! snapshot groups the torrents' needs by disk ([`volumes`]).

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use proto::{AbsolutePath, Needs, TorrentId, Volume};

/// What `statfs` says of a folder: the disk it's on and what's free there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Where its file system is mounted: folders with the same one are on the same disk.
    pub mount: AbsolutePath,
    pub free_bytes: u64,
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
/// needs nothing, or whose destination isn't on a known disk, counts nowhere.
pub fn volumes<'a>(
    needs: impl IntoIterator<Item = (TorrentId, &'a AbsolutePath, u64)>,
    disks: &BTreeMap<AbsolutePath, Disk>,
) -> Vec<Volume> {
    let mut by_disk: BTreeMap<&AbsolutePath, Volume> = BTreeMap::new();
    for (id, destination, bytes) in needs {
        let Some(disk) = disks.get(destination).filter(|_| bytes > 0) else { continue };
        let volume = by_disk.entry(&disk.mount).or_insert_with(|| Volume {
            mount: disk.mount.clone(),
            free_bytes: disk.free_bytes,
            needs: Vec::new(),
        });
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

    fn disks(free: [u64; 3], on: [usize; 5]) -> BTreeMap<AbsolutePath, Disk> {
        (0..5)
            .map(|i| {
                let d = on[i];
                (destination(i), Disk { mount: AbsolutePath::new(format!("/m{d}")).unwrap(), free_bytes: free[d] })
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
