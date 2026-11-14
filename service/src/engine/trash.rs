//! Delete moves a torrent's files to the Trash, so they can still be put back, and nothing else
//! in its destination moves. [`plan`] says what goes: the torrent's own folder whole when it holds
//! only the torrent's files (and Finder's litter, such as `.DS_Store`), else each of its files
//! that's on disk, so a file of the user's beside them stays. Nothing is reached through a
//! symbolic link: one the user made, inside the torrent's folder or in place of a file, is theirs.
//! [`carry_out`] trashes the plan, all of it, keeping what couldn't go, and then removes the
//! folders left with nothing of the user's ([`tidy`]). [`Trash`] is macOS's Trash
//! ([`system`]), or in tests a folder of their own, so a test never fills the user's Trash.
//! Nothing is ever erased instead.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proto::KeptWhy;

/// Moves one file or folder to the Trash, or says why it didn't.
pub type Trash = Arc<dyn Fn(&Path) -> Result<(), KeptWhy> + Send + Sync>;

/// The Trash in Finder, through `NSFileManager trashItemAtURL`, in an autorelease pool of its own:
/// it runs on a blocking thread, which has none.
pub fn system() -> Trash {
    Arc::new(|path| objc2::rc::autoreleasepool(|_| to_trash(path).map(drop)))
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

/// Files Finder leaves in folders it shows: a folder of only these counts as empty.
pub fn is_litter(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name == ".DS_Store" || name.starts_with("._")
}

/// What Delete trashes of a torrent at `location` (its own folder, or its one file).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub location: PathBuf,
    pub items: Vec<PathBuf>,
}

fn real_file(m: &Metadata) -> bool {
    m.file_type().is_file()
}

fn real_dir(m: &Metadata) -> bool {
    m.file_type().is_dir()
}

/// What to trash of a torrent at `location` whose files are at `files`: the one file if it's a
/// real file; else its folder if it holds nothing but them and litter (and real folders on their
/// way), else each of them that's a real file reached through real folders only.
pub fn plan(location: &Path, files: &[PathBuf], single: bool) -> Plan {
    let meta = |p: &Path| std::fs::symlink_metadata(p).ok();
    let mut items = Vec::new();
    if single {
        items.extend(meta(location).is_some_and(|m| real_file(&m)).then(|| location.to_owned()));
    } else if meta(location).is_some_and(|m| real_dir(&m)) {
        if only(location, files) {
            items.push(location.to_owned());
        } else {
            let reached = |f: &&PathBuf| {
                let Ok(inside) = f.strip_prefix(location) else { return false };
                let folders = inside.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty());
                folders.into_iter().all(|a| meta(&location.join(a)).is_some_and(|m| real_dir(&m))) && meta(f).is_some_and(|m| real_file(&m))
            };
            items.extend(files.iter().filter(reached).cloned());
        }
    }
    Plan { location: location.to_owned(), items }
}

/// Whether everything under `dir` is one of `files`, litter, or a real folder of only those.
fn only(dir: &Path, files: &[PathBuf]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().all(|e| {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => only(&path, files),
            Ok(t) if t.is_file() => files.contains(&path) || is_litter(&e.file_name()),
            _ => false,
        }
    })
}

/// Trashes every item of `plan`, carrying on past failures, then tidies the folder; what couldn't
/// go, with why.
pub fn carry_out(plan: &Plan, trash: &Trash) -> Vec<(PathBuf, KeptWhy)> {
    let kept: Vec<(PathBuf, KeptWhy)> = plan.items.iter().filter_map(|item| trash(item).err().map(|why| (item.clone(), why))).collect();
    if std::fs::symlink_metadata(&plan.location).is_ok_and(|m| real_dir(&m)) {
        tidy(&plan.location);
    }
    kept
}

/// Removes the folders under `dir` left with only litter in them, deepest first, and `dir` if it's
/// left so too: what's left of a torrent's folder once its files went one by one. Never goes
/// through a symbolic link.
pub fn tidy(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut litter = Vec::new();
    let mut other = false;
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                tidy(&e.path());
                other |= e.path().exists();
            }
            Ok(t) if t.is_file() && is_litter(&e.file_name()) => litter.push(e.path()),
            _ => other = true,
        }
    }
    if !other {
        litter.iter().for_each(|l| drop(std::fs::remove_file(l)));
        let _ = std::fs::remove_dir(dir);
    }
}

/// A trash of its own in `dir`, for tests: each item moves there, named after its path (after a
/// count, as the Trash does to two of one name), but for the paths in `refusing`, which it keeps,
/// as a locked file.
#[cfg(test)]
pub fn into(dir: PathBuf, refusing: Arc<std::sync::Mutex<Vec<PathBuf>>>) -> Trash {
    let count = std::sync::atomic::AtomicUsize::new(0);
    Arc::new(move |path| {
        if refusing.lock().unwrap().iter().any(|r| r == path) {
            return Err(KeptWhy::Failed { detail: "locked".into() });
        }
        std::fs::create_dir_all(&dir).map_err(|e| KeptWhy::Failed { detail: e.to_string() })?;
        let n = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("{n}%{}", path.to_string_lossy().trim_start_matches('/').replace('/', "%"));
        std::fs::rename(path, dir.join(name)).map_err(|e| KeptWhy::Failed { detail: e.to_string() })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    /// What's under `dir`, as relative paths: files, symlinks and folders, without following links.
    fn tree(dir: &Path) -> BTreeSet<PathBuf> {
        let mut all = BTreeSet::new();
        let mut stack = vec![dir.to_owned()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                all.insert(e.path().strip_prefix(dir).unwrap().to_owned());
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(e.path());
                }
            }
        }
        all
    }

    /// A generated torrent folder: files 0-3 of the torrent, at `a`, `b`, `sub/c`, `sub/deep/d`,
    /// each on disk (whole or partly written) or not; the user's own file in some folder; litter in
    /// some; empty folders; and `sub` maybe a symbolic link the user made to a folder outside, holding
    /// files of the same names.
    #[derive(Debug, Clone)]
    struct Tree {
        on_disk: [Option<bool>; 4],
        mine: Option<usize>,
        litter: Vec<usize>,
        empty: Vec<usize>,
        sub_is_link: bool,
        single: bool,
    }

    const FILES: [&str; 4] = ["a", "b", "sub/c", "sub/deep/d"];
    const FOLDERS: [&str; 3] = ["", "sub", "sub/deep"];

    fn trees() -> impl Strategy<Value = Tree> {
        let on_disk = proptest::array::uniform4(proptest::option::of(any::<bool>()));
        let folder = || 0..FOLDERS.len();
        (
            on_disk,
            proptest::option::of(folder()),
            proptest::collection::vec(folder(), 0..3),
            proptest::collection::vec(folder(), 0..2),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(|(on_disk, mine, litter, empty, sub_is_link, single)| Tree {
                on_disk,
                mine,
                litter,
                empty,
                sub_is_link,
                single,
            })
    }

    /// Builds `t` in `root`: the torrent at `root/pack` (or the file `root/pack.iso`), and the
    /// folder outside at `root/outside`.
    fn build(root: &Path, t: &Tree) -> (PathBuf, Vec<PathBuf>) {
        if t.single {
            let file = root.join("pack.iso");
            if t.on_disk[0].is_some() {
                write(&file);
            }
            return (file.clone(), vec![file]);
        }
        let pack = root.join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        let outside = root.join("outside");
        if t.sub_is_link {
            for f in FILES.iter().filter(|f| f.starts_with("sub/")) {
                write(&outside.join(f.trim_start_matches("sub/")));
            }
            std::os::unix::fs::symlink(&outside, pack.join("sub")).unwrap();
        }
        let files: Vec<PathBuf> = FILES.iter().map(|f| pack.join(f)).collect();
        for (f, present) in files.iter().zip(t.on_disk) {
            let behind_link = t.sub_is_link && f.starts_with(pack.join("sub"));
            match present {
                Some(whole) if !behind_link => {
                    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                    std::fs::write(f, if whole { &b"whole"[..] } else { &b"pa"[..] }).unwrap();
                }
                _ => {}
            }
        }
        let folder = |i: usize| pack.join(FOLDERS[i]);
        let reachable = |i: usize| !(t.sub_is_link && i > 0);
        for &i in t.empty.iter().filter(|&&i| reachable(i)) {
            std::fs::create_dir_all(folder(i).join("empty")).unwrap();
        }
        for &i in t.litter.iter().filter(|&&i| reachable(i)) {
            write(&folder(i).join(".DS_Store"));
        }
        if let Some(i) = t.mine.filter(|&i| reachable(i)) {
            write(&folder(i).join("mine.txt"));
        }
        (pack, files)
    }

    proptest! {
        /// After a Delete: every file of the torrent that was a real file reached through real
        /// folders is in the trash, and gone from where it was; everything of the user's (their
        /// file, their link, and what's behind it) is still at its path; and what's left of the
        /// folder is exactly what holds something of the user's. Nothing fails, so nothing is kept.
        #[test]
        fn delete_moves_exactly_the_torrents_files_and_leaves_the_users(t in trees()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let outside_before = tree(&root.path().join("outside"));
            let mine_before: Vec<PathBuf> =
                tree(root.path()).into_iter().filter(|p| p.ends_with("mine.txt") || t.sub_is_link && p.ends_with("pack/sub")).collect();
            let torrents: Vec<PathBuf> = files
                .iter()
                .filter(|f| std::fs::symlink_metadata(f).is_ok_and(|m| m.is_file()))
                .filter(|f| !(t.sub_is_link && f.starts_with(location.join("sub"))))
                .cloned()
                .collect();

            let trash_dir = root.path().join("trash");
            let kept = carry_out(&plan(&location, &files, t.single), &into(trash_dir.clone(), Arc::default()));
            prop_assert!(kept.is_empty());

            for f in &torrents {
                prop_assert!(std::fs::symlink_metadata(f).is_err(), "{} is still there", f.display());
            }
            let trashed_files = tree(&trash_dir).into_iter().filter(|p| {
                let name = p.file_name().unwrap();
                !is_litter(name) && std::fs::symlink_metadata(trash_dir.join(p)).is_ok_and(|m| m.is_file())
            }).count();
            prop_assert_eq!(trashed_files, torrents.len(), "exactly the torrent's files went to the trash");
            for p in &mine_before {
                prop_assert!(std::fs::symlink_metadata(root.path().join(p)).is_ok(), "the user's {} stays", p.display());
            }
            prop_assert_eq!(tree(&root.path().join("outside")), outside_before, "nothing behind the link moves");
            if !t.single {
                // Left: only folders on the way to something of the user's, and litter in those.
                let left = tree(&location);
                let users = |p: &PathBuf| p.ends_with("mine.txt") || p == Path::new("sub") && t.sub_is_link;
                for p in &left {
                    let holds = left.iter().any(|q| users(q) && q.starts_with(p));
                    // Litter stays where its folder stays, for something of the user's in it.
                    let beside = is_litter(p.file_name().unwrap()) && left.iter().any(|q| users(q) && q.starts_with(p.parent().unwrap()));
                    prop_assert!(holds || beside, "{} is left with nothing of the user's: {left:?}", p.display());
                }
                prop_assert_eq!(location.exists(), left.iter().any(users), "the folder goes once nothing of the user's is in it");
            }
        }
    }

    #[test]
    fn a_failure_keeps_that_item_and_the_rest_still_go() {
        let root = tempfile::tempdir().unwrap();
        let pack = root.path().join("pack");
        let files = vec![pack.join("a"), pack.join("sub/b")];
        files.iter().for_each(|f| write(f));
        write(&pack.join("mine.txt"));
        let refusing = Arc::new(std::sync::Mutex::new(vec![files[0].clone()]));
        let kept = carry_out(&plan(&pack, &files, false), &into(root.path().join("trash"), refusing));
        assert_eq!(kept, [(files[0].clone(), KeptWhy::Failed { detail: "locked".into() })]);
        assert!(files[0].exists() && !files[1].exists() && !pack.join("sub").exists(), "the rest went, and their folder");
    }

    #[test]
    fn a_folder_with_the_name_of_a_single_files_torrent_is_the_users() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.iso");
        std::fs::create_dir_all(&path).unwrap();
        assert!(plan(&path, std::slice::from_ref(&path), true).items.is_empty());
    }

    /// An example, not generated: the real Trash, which only macOS has. It touches the user's
    /// Trash, so it runs on request: `cargo test -p service -- --ignored system_trash`.
    #[test]
    #[ignore = "uses the user's Trash; run by hand"]
    fn the_system_trash_takes_a_file_away() {
        struct Remove(Option<PathBuf>);
        impl Drop for Remove {
            fn drop(&mut self) {
                if let Some(p) = &self.0 {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("phantom-trash-test-{}.txt", std::process::id()));
        write(&file);
        let moved = Remove(objc2::rc::autoreleasepool(|_| to_trash(&file)).unwrap());
        assert!(!file.exists(), "gone from where it was");
        assert!(moved.0.as_ref().is_some_and(|p| p.exists()), "and it's in the Trash");
    }
}
