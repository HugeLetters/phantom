//! macOS: IOKit, Foundation and Darwin's own calls.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use proto::KeptWhy;

use super::Fs;

/// A held power assertion: its id. macOS releases it when the process ends.
pub struct Assertion(u32);

/// `kIOReturnSuccess`.
const SUCCESS: i32 = 0;

/// Holds `kIOPMAssertionTypePreventUserIdleSystemSleep` under `name`, through IOKit's
/// `IOPMAssertionCreateWithName` (`IOKit/pwr_mgt/IOPMLib.h`, from `objc2-io-kit`).
pub fn prevent_idle_sleep(name: &str) -> Result<Assertion> {
    use objc2_core_foundation::CFString;
    use objc2_io_kit::{IOPMAssertionCreateWithName, IOPMAssertionID, kIOPMAssertionLevelOn};
    let kind = CFString::from_static_str("PreventUserIdleSystemSleep");
    let name = CFString::from_str(name);
    let mut id: IOPMAssertionID = 0;
    // SAFETY: both strings live through the call, and `id` is a valid place for the id.
    let returned = unsafe { IOPMAssertionCreateWithName(Some(&kind), kIOPMAssertionLevelOn, Some(&name), &mut id) };
    if returned != SUCCESS {
        bail!("IOPMAssertionCreateWithName returned {returned:#x}");
    }
    Ok(Assertion(id))
}

/// Releases `assertion` (`IOPMAssertionRelease`).
pub fn release(assertion: Assertion) -> Result<()> {
    let returned = objc2_io_kit::IOPMAssertionRelease(assertion.0);
    if returned != SUCCESS {
        bail!("IOPMAssertionRelease returned {returned:#x}");
    }
    Ok(())
}

/// The file system `folder` is on, through `statfs`, or `None` when it can't be reached.
pub fn disk(folder: &Path) -> Option<Fs> {
    let path = CString::new(folder.as_os_str().as_bytes()).ok()?;
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a C string that lives through the call, and `fs` is a place for the answer.
    if unsafe { libc::statfs(path.as_ptr(), &mut fs) } != 0 {
        return None;
    }
    // SAFETY: statfs fills `f_mntonname` with a NUL-terminated path.
    let mount = unsafe { CStr::from_ptr(fs.f_mntonname.as_ptr()) };
    let mount = PathBuf::from(std::ffi::OsStr::from_bytes(mount.to_bytes()));
    // SAFETY: `fsid_t` is two `i32`s.
    let fsid = unsafe { std::mem::transmute::<libc::fsid_t, [i32; 2]>(fs.f_fsid) };
    Some(Fs { mount, fsid, free_bytes: fs.f_bavail * u64::from(fs.f_bsize) })
}

/// What Finder says of the volume `folder` is on: its free space with purgeable space counted
/// (`NSURLVolumeAvailableCapacityForImportantUsageKey`) and its name (`NSURLVolumeLocalizedNameKey`),
/// each `None` when the volume gives none. A call into Foundation, which may block like `statfs`.
pub fn finder(folder: &Path) -> (Option<u64>, Option<String>) {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{
        NSArray, NSNumber, NSString, NSURL, NSURLVolumeAvailableCapacityForImportantUsageKey, NSURLVolumeLocalizedNameKey,
    };
    // Not the main thread: its own pool, so what Foundation autoreleases is freed here.
    autoreleasepool(|_| {
        let Some(path) = folder.to_str() else { return (None, None) };
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        // SAFETY: the keys are Foundation's own constants.
        let (important, localized) = unsafe { (NSURLVolumeAvailableCapacityForImportantUsageKey, NSURLVolumeLocalizedNameKey) };
        let keys = NSArray::from_slice(&[important, localized]);
        let Ok(values) = url.resourceValuesForKeys_error(&keys) else { return (None, None) };
        let free_bytes = values
            .objectForKey(important)
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map(|n| n.as_i64())
            .and_then(|n| u64::try_from(n).ok());
        let name = values.objectForKey(localized).and_then(|v| v.downcast::<NSString>().ok()).map(|s| s.to_string());
        (free_bytes, name)
    })
}

/// Moves `path` to the Trash in Finder (`NSFileManager trashItemAtURL`), in an autorelease pool
/// of its own: it runs on a blocking thread, which has none. Where it went.
pub fn trash(path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
    objc2::rc::autoreleasepool(|_| to_trash(path))
}

/// Moves `path` to the Trash in Finder; where it went. A volume with no Trash says so
/// (`NSFeatureUnsupportedError`).
fn to_trash(path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
    use objc2_foundation::{NSFeatureUnsupportedError, NSFileManager, NSURL};
    let failed = |detail: String| KeptWhy::Failed { detail };
    let url = NSURL::from_file_path(path).ok_or_else(|| failed(format!("not a file path: {}", path.display())))?;
    let mut moved = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut moved))
        .map_err(|e| if e.code() == NSFeatureUnsupportedError { KeptWhy::NoTrash } else { failed(e.localizedDescription().to_string()) })?;
    Ok(moved.and_then(|u| u.to_file_path()))
}

/// An item at `_in_trash` was put back out of the Trash: Finder keeps nothing else of it.
pub fn untrashed(_in_trash: &Path) {}

/// Renames `from` to `to` only if nothing is at `to` (`renamex_np` with `RENAME_EXCL`): the check
/// and the move are one step.
pub fn rename_excl(from: &CStr, to: &CStr) -> std::io::Result<()> {
    // SAFETY: two NUL-terminated paths that live through the call.
    match unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Flushes the open folder `dir` to the drive itself (`F_FULLFSYNC`): whether that worked.
pub fn full_fsync(dir: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: a descriptor open through the call.
    unsafe { libc::fcntl(dir.as_raw_fd(), libc::F_FULLFSYNC) == 0 }
}

/// `name` in Unicode's composed form (NFC), as Foundation has it: a drive that doesn't tell the forms
/// apart (APFS) keeps one name for `é` as one character and as `e` and an accent.
pub fn normalized(name: &str) -> String {
    use objc2_foundation::NSString;
    NSString::from_str(name).precomposedStringWithCanonicalMapping().to_string()
}
