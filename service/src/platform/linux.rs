//! Linux: an inhibitor for keeping awake, the FreeDesktop trash, and plain POSIX calls. Linux isn't
//! a platform Phantom ships for: this is what lets the workspace build and its tests run there.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::KeptWhy;

use super::Fs;

/// A held inhibitor (systemd's, or the desktop's screen saver's): released when dropped, and when
/// the process ends.
pub struct Assertion(#[allow(dead_code)] keepawake::KeepAwake);

/// Holds off idle sleep under `name`, through `keepawake`.
pub fn prevent_idle_sleep(name: &str) -> Result<Assertion> {
    let held = keepawake::Builder::default().idle(true).reason(name).app_name("Phantom").create().context("keepawake")?;
    Ok(Assertion(held))
}

/// Releases `assertion`: dropping it does.
pub fn release(assertion: Assertion) -> Result<()> {
    drop(assertion);
    Ok(())
}

/// The file system `folder` is on, through `statvfs`, or `None` when it can't be reached. Linux's
/// has no mount point: it's the highest folder above `folder`'s real path on the same device.
pub fn disk(folder: &Path) -> Option<Fs> {
    let path = CString::new(folder.as_os_str().as_bytes()).ok()?;
    let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a C string that lives through the call, and `fs` is a place for the answer.
    if unsafe { libc::statvfs(path.as_ptr(), &mut fs) } != 0 {
        return None;
    }
    let real = std::fs::canonicalize(folder).ok()?;
    let dev = std::fs::metadata(&real).ok()?.dev();
    let mount = real.ancestors().take_while(|a| std::fs::metadata(a).is_ok_and(|m| m.dev() == dev)).last()?.to_path_buf();
    let fsid = fs.f_fsid as u64;
    Some(Fs { mount, fsid: [fsid as i32, (fsid >> 32) as i32], free_bytes: (fs.f_bavail as u64).saturating_mul(fs.f_frsize as u64) })
}

/// No Finder: no figures of its own, so a disk's free space is `statvfs`'s and its name its mount's.
pub fn finder(_folder: &Path) -> (Option<u64>, Option<String>) {
    (None, None)
}

/// Moves `path` to the FreeDesktop trash (`trash`: the home trash, or the `.Trash-<uid>` at the top
/// of the drive it's on). Where it went: the item listed with its original path whose file is the
/// one that was at `path` (by device and inode, which the trash's rename keeps), else the newest of
/// them; `None` if the trash lists none.
pub fn trash(path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
    let failed = |e: trash::Error| KeptWhy::Failed { detail: e.to_string() };
    // The trash records the path with its folders' links resolved.
    let original = match (path.parent().and_then(|p| std::fs::canonicalize(p).ok()), path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_owned(),
    };
    let was = std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()));
    trash::delete(path).map_err(failed)?;
    let Ok(items) = trash::os_limited::list() else { return Ok(None) };
    let mut candidates: Vec<(i64, PathBuf)> = items
        .into_iter()
        .filter(|i| i.original_path() == original)
        .filter_map(|i| Some((i.time_deleted, in_trash(Path::new(&i.id))?)))
        .collect();
    candidates.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let same = |p: &Path| std::fs::symlink_metadata(p).ok().map(|m| (m.dev(), m.ino())) == was;
    let found = candidates.iter().find(|(_, p)| same(p)).or(candidates.first());
    Ok(found.map(|(_, p)| p.clone()))
}

/// The item a `.trashinfo` file at `info` (`<trash>/info/<name>.trashinfo`) is about:
/// `<trash>/files/<name>`.
fn in_trash(info: &Path) -> Option<PathBuf> {
    let name = info.file_name()?.as_bytes().strip_suffix(b".trashinfo")?;
    Some(info.parent()?.parent()?.join("files").join(std::ffi::OsStr::from_bytes(name)))
}

/// An item at `in_trash` (`<trash>/files/<name>`) was put back out of the trash: its
/// `<trash>/info/<name>.trashinfo` goes, so the trash doesn't list it any longer.
pub fn untrashed(in_trash: &Path) {
    let (Some(files), Some(name)) = (in_trash.parent(), in_trash.file_name()) else { return };
    if files.file_name() != Some(std::ffi::OsStr::new("files")) {
        return;
    }
    let Some(trash) = files.parent() else { return };
    let mut info = name.to_owned();
    info.push(".trashinfo");
    let _ = std::fs::remove_file(trash.join("info").join(info));
}

/// Renames `from` to `to` only if nothing is at `to` (`renameat2` with `RENAME_NOREPLACE`): the
/// check and the move are one step.
pub fn rename_excl(from: &CStr, to: &CStr) -> std::io::Result<()> {
    // SAFETY: two NUL-terminated paths that live through the call, relative to the working folder.
    match unsafe { libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// No `F_FULLFSYNC` on Linux: its `fsync`, which the caller falls back to, flushes to the drive.
pub fn full_fsync(_dir: &std::fs::File) -> bool {
    false
}

/// `name` as it is: Linux's drives tell Unicode's forms apart, and the service has no table of them
/// without Foundation.
pub fn normalized(name: &str) -> String {
    name.to_owned()
}
