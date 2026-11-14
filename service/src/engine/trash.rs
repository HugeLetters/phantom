//! Delete moves a torrent's files to the Trash, so they can still be put back, and nothing else
//! in its destination moves. [`plan`] says what goes: the torrent's own folder whole when it holds
//! only the torrent's files, else each of its files that's on disk, so a file of the user's that
//! sits beside them stays. [`Trash`] carries it out: macOS's Trash ([`system`]), or in tests a
//! folder of their own, so a test never fills the user's Trash.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

/// Moves one file or folder to the Trash.
pub type Trash = Arc<dyn Fn(&Path) -> Result<()> + Send + Sync>;

/// The Trash in Finder.
pub fn system() -> Trash {
    Arc::new(|path| to_trash(path).map(drop))
}

/// Moves `path` to the Trash in Finder, through `NSFileManager trashItemAtURL`; where it went.
fn to_trash(path: &Path) -> Result<Option<PathBuf>> {
    use objc2_foundation::{NSFileManager, NSURL};
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow::anyhow!("not a file path: {}", path.display()))?;
    let mut moved = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut moved))
        .map_err(|e| anyhow::anyhow!("can't move {} to the Trash: {}", path.display(), e.localizedDescription()))?;
    Ok(moved.and_then(|u| u.to_file_path()))
}

/// What to trash of a torrent at `location` (its own folder, or its one file) whose files are at
/// `files`: the folder if it holds nothing but them (and the folders on their way), else each of
/// them that's on disk. Afterwards, the folders left empty inside it go too ([`tidy`]).
pub fn plan(location: &Path, files: &[PathBuf], single: bool) -> Vec<PathBuf> {
    if single {
        return location.exists().then(|| location.to_owned()).into_iter().collect();
    }
    if !location.is_dir() {
        return Vec::new();
    }
    if only(location, files) {
        return vec![location.to_owned()];
    }
    files.iter().filter(|f| std::fs::symlink_metadata(f).is_ok()).cloned().collect()
}

/// Whether everything under `dir` is one of `files` or a folder.
fn only(dir: &Path, files: &[PathBuf]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().all(|e| {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => only(&path, files),
            _ => files.contains(&path),
        }
    })
}

/// Removes the empty folders under `dir`, deepest first, and `dir` if it's empty then: what's
/// left of a torrent's folder once its files went one by one.
pub fn tidy(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            tidy(&entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// A trash of its own in `dir`, for tests: each item moves there, named after its path.
#[cfg(test)]
pub fn into(dir: PathBuf) -> Trash {
    Arc::new(move |path| {
        std::fs::create_dir_all(&dir)?;
        let name = path.to_string_lossy().trim_start_matches('/').replace('/', "%");
        Ok(std::fs::rename(path, dir.join(name))?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn a_folder_of_only_its_files_goes_whole_and_one_with_others_goes_file_by_file() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        let files = vec![pack.join("a.bin"), pack.join("sub/b.bin"), pack.join("gone.bin")];
        files[..2].iter().for_each(|f| write(f));
        assert_eq!(plan(&pack, &files, false), std::slice::from_ref(&pack), "only its files: the folder");

        write(&pack.join("mine.txt"));
        assert_eq!(plan(&pack, &files, false), files[..2], "another file there: its files on disk, one by one");
        assert_eq!(plan(&dir.path().join("none"), &files, false), Vec::<PathBuf>::new(), "no folder, nothing");
    }

    #[test]
    fn a_single_file_goes_if_it_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.iso");
        assert!(plan(&file, std::slice::from_ref(&file), true).is_empty());
        write(&file);
        assert_eq!(plan(&file, std::slice::from_ref(&file), true), [file]);
    }

    #[test]
    fn tidying_leaves_folders_with_something_in_them() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack");
        std::fs::create_dir_all(pack.join("empty/deeper")).unwrap();
        write(&pack.join("kept/mine.txt"));
        tidy(&pack);
        assert!(!pack.join("empty").exists() && pack.join("kept/mine.txt").exists());
    }

    /// An example, not generated: the real Trash, which only macOS has. The item it makes is
    /// deleted from the Trash right after, so nothing is left there.
    #[test]
    fn the_system_trash_takes_a_file_away() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("phantom-trash-test-{}.txt", std::process::id()));
        write(&file);
        let moved = to_trash(&file).unwrap().expect("it says where it went");
        assert!(!file.exists(), "gone from where it was");
        assert!(moved.exists(), "and it's in the Trash");
        std::fs::remove_file(moved).unwrap();
    }
}
