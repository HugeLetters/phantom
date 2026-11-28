//! The few things the service asks of the OS that differ between macOS, which the app is for, and
//! Linux, where the workspace builds and its tests run too. Each is a thin function; what's done
//! with its answer stays in the module that asks.
//!
//! | What | macOS | Linux |
//! |---|---|---|
//! | [`Assertion`]: keeping the machine awake (`awake.rs`) | IOKit's power assertion | `keepawake` (an inhibitor) |
//! | [`disk`]: a folder's file system (`engine/space.rs`) | `statfs` | `statvfs`, the mount found by device |
//! | [`finder`]: a volume's name and free space (`engine/space.rs`) | Finder's, through Foundation | none: `statvfs`'s free space, the mount's name |
//! | [`trash`]: moving to the Trash (`engine/trash.rs`) | `NSFileManager trashItemAtURL` | the FreeDesktop trash (`trash`) |
//! | [`untrashed`]: an item put back from the Trash | nothing to do | its `.trashinfo` deleted |
//! | [`rename_excl`]: a rename that never replaces | `renamex_np(RENAME_EXCL)` | `renameat2(RENAME_NOREPLACE)` |
//! | [`full_fsync`]: a folder flushed to the drive itself | `F_FULLFSYNC` | none: `fsync` is |
//! | [`normalized`]: a name as the drive compares it | Unicode composed (NFC) | as it is |

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;

/// What [`disk`] says of the file system a folder is on.
pub struct Fs {
    /// Where it's mounted.
    pub mount: std::path::PathBuf,
    /// The file system's id, which with the mount tells one disk from another.
    pub fsid: [i32; 2],
    /// What's free there for an unprivileged process, in bytes.
    pub free_bytes: u64,
}
