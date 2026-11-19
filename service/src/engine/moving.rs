//! The file work of a move (#24, `lifecycle::moving` decides its stages): blocking, on a task of
//! its own, never on the actor.
//!
//! [`plan`] says what moves: each of the torrent's files, from its path in the old destination to
//! its path in the new one, refused when something is at one of those (a clash, unless the person
//! chose what to do with it) or a folder on the way there is a symbolic link or a file. [`stage`]
//! moves them into the move's own folder in the new destination (`.phantom-moving-<hash>`): renamed
//! on the same drive, copied in chunks to another, a chunk of zeros left a hole, with what's copied
//! told after each chunk and a stop heard between them. Then the actor writes the new destination
//! ahead, and [`finish`] puts each in its place (over what was there, for Replace) and removes what's
//! left in the old one; or, if it didn't get that far, [`restore`] renames back what was renamed and
//! removes what was copied. Either way the files are in exactly one destination once it's done.
//! Nothing is reached through a symbolic link, as in the Trash's code.

use std::io::{Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use proto::{AbsolutePath, Clash, NotMoved};
use serde::{Deserialize, Serialize};

use super::trash;

/// How much a copy reads at a time: what it says it copied, and stops, between.
const CHUNK: usize = 256 * 1024;

/// What a move moves, kept in the torrent's record while it moves.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Plan {
    /// The old destination and the new one.
    pub from: PathBuf,
    pub to: PathBuf,
    /// Where they go first, in the new destination.
    pub staging: PathBuf,
    /// On the same drive: renamed, else copied.
    pub renames: bool,
    pub items: Vec<Item>,
}

/// One of the torrent's files.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub index: usize,
    /// Inside the destination: the torrent's own folder, then the file's path in it.
    pub path: PathBuf,
    /// Something was at its path in the new destination, which the torrent takes as its file (Use
    /// existing): only what's in the old destination goes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
}

/// Why a move can't be planned.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    /// Something is at these paths its files would take, and nothing says what to do with them.
    Clash(Vec<PathBuf>),
    /// Something other than a real folder is on the way to one of them, or other than a file is at
    /// one: nothing goes through it, nor over it.
    NotAFolder(PathBuf),
}

/// How [`stage`] ended short of having every file staged.
#[derive(Debug, Clone, PartialEq)]
pub enum Stop {
    Failed(NotMoved),
    /// Told to stop.
    Stopped,
}

impl Plan {
    fn old(&self, item: &Item) -> PathBuf {
        self.from.join(&item.path)
    }

    fn new(&self, item: &Item) -> PathBuf {
        self.to.join(&item.path)
    }

    fn staged(&self, item: &Item) -> PathBuf {
        self.staging.join(&item.path)
    }

    /// It takes files that were in the new destination already: they're checked once it's moved.
    pub fn adopts(&self) -> bool {
        self.items.iter().any(|i| i.kept)
    }
}

fn meta(path: &Path) -> Option<std::fs::Metadata> {
    std::fs::symlink_metadata(path).ok()
}

/// Whether `a` and `b` are on the same volume, so one can be renamed into the other.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    matches!((meta(a), meta(b)), (Some(a), Some(b)) if a.dev() == b.dev())
}

/// The move of torrent `hash`'s `files` (each by index, with its path inside its destination) from
/// the destination `from` into `to`, `clash` saying what to do with what's at their paths there.
pub fn plan(from: &Path, to: &Path, hash: &str, files: &[(usize, PathBuf)], clash: Option<Clash>, renames: bool) -> Result<Plan, Refused> {
    let mut taken = Vec::new();
    for (_, path) in files {
        let new = to.join(path);
        let folders = path.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty());
        for folder in folders.map(|a| to.join(a)) {
            if meta(&folder).is_some_and(|m| !m.file_type().is_dir()) {
                return Err(Refused::NotAFolder(folder));
            }
        }
        match meta(&new) {
            None => {}
            Some(m) if m.file_type().is_file() => taken.push(new),
            Some(_) => return Err(Refused::NotAFolder(new)),
        }
    }
    if !taken.is_empty() && clash.is_none() {
        return Err(Refused::Clash(taken));
    }
    let items = files.iter().map(|(index, path)| Item {
        index: *index,
        path: path.clone(),
        kept: clash == Some(Clash::UseExisting) && taken.contains(&to.join(path)),
    });
    Ok(Plan {
        from: from.to_owned(),
        to: to.to_owned(),
        staging: to.join(format!(".phantom-moving-{hash}")),
        renames,
        items: items.collect(),
    })
}

/// Why writing into the new destination failed, as a move says it.
fn not_moved(e: &std::io::Error, to: &Path, at: &Path) -> NotMoved {
    if let Ok(to) = AbsolutePath::new(to)
        && let Some(reason) = super::files::unavailable(&to)
    {
        return NotMoved::Unavailable { reason };
    }
    match (e.raw_os_error(), e.kind()) {
        (Some(libc::ENOSPC), _) => NotMoved::DiskFull,
        (_, std::io::ErrorKind::NotFound) => match AbsolutePath::new(at) {
            Ok(path) => NotMoved::CopyGone { path },
            Err(_) => NotMoved::Failed { detail: e.to_string() },
        },
        _ => NotMoved::Failed { detail: e.to_string() },
    }
}

/// Whether `file` is a real file inside `root`, reached through real folders only.
fn reached(root: &Path, inside: &Path) -> Option<std::fs::Metadata> {
    let folders = inside.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty());
    for folder in folders {
        if !meta(&root.join(folder)).is_some_and(|m| m.file_type().is_dir()) {
            return None;
        }
    }
    meta(&root.join(inside)).filter(|m| m.file_type().is_file())
}

/// Moves each file of `plan` not kept into its staging folder, telling `copied` how many bytes of
/// how many it copied so far after each chunk (none for a rename), and waiting in `between` (a
/// test's gate) after each. Once `stop` is set it stops after the chunk in flight. The files not in
/// the old destination stay out of it: their indices. Each staged file is then looked at again:
/// one gone from there (deleted by hand) fails it.
pub fn stage(plan: &Plan, mut copied: impl FnMut(u64, u64), stop: &AtomicBool, between: &dyn Fn() -> bool) -> Result<Vec<usize>, Stop> {
    let fail = |e: &std::io::Error, at: &Path| Stop::Failed(not_moved(e, &plan.to, at));
    match meta(&plan.staging) {
        Some(m) if m.file_type().is_dir() => {}
        Some(_) => return Err(Stop::Failed(not_a_folder(&plan.staging))),
        None => std::fs::create_dir(&plan.staging).map_err(|e| fail(&e, &plan.to))?,
    }
    let moving: Vec<(&Item, std::fs::Metadata)> =
        plan.items.iter().filter(|i| !i.kept).filter_map(|i| Some((i, reached(&plan.from, &i.path)?))).collect();
    let missing = plan.items.iter().filter(|i| !i.kept && !moving.iter().any(|(m, _)| m.index == i.index)).map(|i| i.index).collect();
    let total = if plan.renames { 0 } else { moving.iter().map(|(_, m)| m.len()).sum() };
    let mut done = 0;
    for (item, _) in &moving {
        if stop.load(Ordering::SeqCst) {
            return Err(Stop::Stopped);
        }
        let (old, staged) = (plan.old(item), plan.staged(item));
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent).map_err(|e| fail(&e, parent))?;
        }
        let renamed = plan.renames && std::fs::rename(&old, &staged).is_ok();
        if !renamed {
            copy(
                &old,
                &staged,
                &mut |n| {
                    done += n;
                    copied(done, total);
                },
                stop,
                between,
            )
            .map_err(|e| match e {
                Copy::Stopped => Stop::Stopped,
                Copy::Read(e) => Stop::Failed(NotMoved::Failed { detail: e.to_string() }),
                Copy::Write(e) => fail(&e, &plan.new(item)),
            })?;
        }
        if !between() {
            return Err(Stop::Stopped);
        }
    }
    for (item, was) in &moving {
        if !meta(&plan.staged(item)).is_some_and(|m| m.file_type().is_file() && m.len() == was.len()) {
            let path = AbsolutePath::new(plan.new(item)).map_err(|p| Stop::Failed(NotMoved::Failed { detail: p.display().to_string() }))?;
            return Err(Stop::Failed(NotMoved::CopyGone { path }));
        }
    }
    Ok(missing)
}

fn not_a_folder(path: &Path) -> NotMoved {
    match AbsolutePath::new(path) {
        Ok(path) => NotMoved::NotAFolder { path },
        Err(p) => NotMoved::Failed { detail: p.display().to_string() },
    }
}

enum Copy {
    Stopped,
    Read(std::io::Error),
    Write(std::io::Error),
}

/// Copies the file at `from` (never through a link) to a new file at `to`, a chunk at a time, each
/// told to `copied`, then `between` (false: stop); a chunk of zeros is left a hole, as librqbit
/// leaves what it hasn't downloaded. Once `stop` is set it stops after the chunk in flight.
fn copy(from: &Path, to: &Path, copied: &mut dyn FnMut(u64), stop: &AtomicBool, between: &dyn Fn() -> bool) -> Result<(), Copy> {
    let mut src = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(from).map_err(Copy::Read)?;
    let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(to).map_err(Copy::Write)?;
    let mut buf = vec![0; CHUNK];
    let mut at = 0;
    loop {
        let n = src.read(&mut buf).map_err(Copy::Read)?;
        if n == 0 {
            break;
        }
        if buf[..n].iter().all(|b| *b == 0) {
            dst.seek(std::io::SeekFrom::Current(n as i64)).map_err(Copy::Write)?;
        } else {
            dst.write_all(&buf[..n]).map_err(Copy::Write)?;
        }
        at += n as u64;
        copied(n as u64);
        if !between() || stop.load(Ordering::SeqCst) {
            return Err(Copy::Stopped);
        }
    }
    dst.set_len(at).map_err(Copy::Write)?;
    dst.sync_all().map_err(Copy::Write)
}

/// Puts each staged file in its place in the new destination, over what's there (Replace), then
/// removes what's left of the torrent's files in the old one, and the folders that leaves empty
/// there, and the staging folder. Past the point where the record names the new destination: what
/// can't be done is only told, as warnings.
pub fn finish(plan: &Plan) -> Vec<String> {
    let mut warnings = Vec::new();
    for item in plan.items.iter().filter(|i| !i.kept) {
        let (staged, new) = (plan.staged(item), plan.new(item));
        if !meta(&staged).is_some_and(|m| m.file_type().is_file()) {
            continue;
        }
        if let Some(parent) = new.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            warnings.push(format!("{}: {e}", parent.display()));
            continue;
        }
        if let Err(e) = std::fs::rename(&staged, &new) {
            warnings.push(format!("{}: {e}", new.display()));
        }
    }
    for item in &plan.items {
        if reached(&plan.from, &item.path).is_some()
            && let Err(e) = std::fs::remove_file(plan.old(item))
        {
            warnings.push(format!("{}: {e}", plan.old(item).display()));
        }
    }
    tidy_old(plan);
    remove_empty(&plan.staging);
    warnings
}

/// Puts back what [`stage`] did: each staged file renamed back to its path in the old destination
/// (it was renamed), never over anything there nor making that destination again if it's gone; or
/// removed (it was copied: the original is still there, or out of reach with its drive). Then the
/// staging folder goes. What can't be done is told.
pub fn restore(plan: &Plan) -> Vec<String> {
    let mut warnings = Vec::new();
    let from_there = meta(&plan.from).is_some_and(|m| m.file_type().is_dir());
    for item in plan.items.iter().filter(|i| !i.kept) {
        let (staged, old) = (plan.staged(item), plan.old(item));
        if !meta(&staged).is_some_and(|m| m.file_type().is_file()) {
            continue;
        }
        let undone = match (plan.renames, from_there) {
            (false, _) => std::fs::remove_file(&staged).map_err(|e| e.to_string()),
            (true, true) => old
                .parent()
                .map_or(Ok(()), |p| std::fs::create_dir_all(p).map_err(|e| e.to_string()))
                .and_then(|()| trash::rename_free(&staged, &old)),
            (true, false) => Err(format!("{} is gone", plan.from.display())),
        };
        if let Err(e) = undone {
            warnings.push(format!("{}: {e}", staged.display()));
        }
    }
    remove_empty(&plan.staging);
    warnings
}

/// The folders of the torrent's own folder in the old destination left with nothing but litter go.
fn tidy_old(plan: &Plan) {
    let own: std::collections::BTreeSet<PathBuf> =
        plan.items.iter().filter_map(|i| i.path.components().next()).map(|c| plan.from.join(c)).collect();
    for folder in own.into_iter().filter(|f| meta(f).is_some_and(|m| m.file_type().is_dir())) {
        trash::tidy(&folder);
    }
}

/// Removes `dir` and the folders in it, deepest first, while they're empty; never through a link.
fn remove_empty(dir: &Path) {
    if !meta(dir).is_some_and(|m| m.file_type().is_dir()) {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            remove_empty(&e.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// Where each of a plan's files is now, by index: the old destination, the new one, staged, or
/// nowhere; for tests' properties.
#[cfg(test)]
pub fn where_is(plan: &Plan, item: &Item) -> [bool; 3] {
    [plan.old(item), plan.new(item), plan.staged(item)].map(|p| meta(&p).is_some())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Three files of a torrent's own folder, `t/`, each of a size of its own, in `from`; and what
    /// a step does around the move: a file at a path one takes in `to`, one deleted from the old
    /// destination before it moves.
    #[derive(Debug, Clone)]
    struct Case {
        renames: bool,
        clash: Option<Clash>,
        /// A file of the person's at the new path of file `n`.
        theirs: Option<usize>,
        /// File `n` is gone from the old destination before it moves.
        gone: Option<usize>,
        /// It's stopped after this many chunks (then restored), or finished.
        stop_after: Option<usize>,
    }

    fn case() -> impl Strategy<Value = Case> {
        (
            any::<bool>(),
            prop_oneof![Just(None), Just(Some(Clash::UseExisting)), Just(Some(Clash::Replace))],
            proptest::option::of(0..3usize),
            proptest::option::of(0..3usize),
            proptest::option::of(0..6usize),
        )
            .prop_map(|(renames, clash, theirs, gone, stop_after)| Case { renames, clash, theirs, gone, stop_after })
    }

    fn files() -> Vec<(usize, PathBuf)> {
        vec![(0, "t/a.bin".into()), (1, "t/sub/b.bin".into()), (2, "t/c.bin".into())]
    }

    fn bytes(n: usize) -> Vec<u8> {
        // A chunk and a half, and a chunk of zeros in the middle of the second.
        let mut b: Vec<u8> = (0..CHUNK + CHUNK / 2 + n * 7).map(|i| (i * 13 + n) as u8 | 1).collect();
        if n == 1 {
            b[CHUNK / 4..CHUNK / 4 + CHUNK].fill(0);
        }
        b
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64),
            ..ProptestConfig::default()
        })]

        /// Whatever the case: refused with a clash exactly when something is at a new path and no
        /// choice says what to do; else once it's finished every file is at its new path with its
        /// bytes (or, kept, the person's), none in the old destination nor staged, and the person's
        /// file kept or replaced as chosen; once it's restored, every file is where it was with its
        /// bytes, nothing is staged and nothing of the torrent's is in the new destination. Outside
        /// a move, each file is in exactly one destination.
        #[test]
        fn a_move_ends_in_exactly_one_destination(c in case()) {
            let dir = tempfile::tempdir().unwrap();
            let (from, to) = (dir.path().join("from"), dir.path().join("to"));
            std::fs::create_dir_all(&to).unwrap();
            for (n, path) in files() {
                std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
                std::fs::write(from.join(path), bytes(n)).unwrap();
            }
            if let Some(n) = c.theirs {
                let at = to.join(&files()[n].1);
                std::fs::create_dir_all(at.parent().unwrap()).unwrap();
                std::fs::write(&at, b"theirs").unwrap();
            }
            let planned = plan(&from, &to, "h", &files(), c.clash, c.renames);
            let plan = match (planned, c.theirs, c.clash) {
                (Err(Refused::Clash(taken)), Some(n), None) => {
                    prop_assert_eq!(taken, vec![to.join(&files()[n].1)]);
                    return Ok(());
                }
                (Ok(plan), _, Some(_)) | (Ok(plan), None, None) => plan,
                (other, ..) => panic!("{other:?} for {c:?}"),
            };
            if let Some(n) = c.gone {
                std::fs::remove_file(from.join(&files()[n].1)).unwrap();
            }
            let stop = AtomicBool::new(false);
            let chunks = std::cell::Cell::new(0);
            let between = || {
                chunks.set(chunks.get() + 1);
                if c.stop_after == Some(chunks.get()) {
                    stop.store(true, Ordering::SeqCst);
                }
                true
            };
            let staged = stage(&plan, |_, _| {}, &stop, &between);
            let kept = |n: usize| c.theirs == Some(n) && c.clash == Some(Clash::UseExisting);
            match staged {
                Ok(missing) if c.stop_after.is_none_or(|s| s > chunks.get()) => {
                    prop_assert_eq!(missing, c.gone.filter(|n| !kept(*n)).into_iter().collect::<Vec<_>>());
                    prop_assert_eq!(finish(&plan), Vec::<String>::new());
                    for (item, (n, path)) in plan.items.iter().zip(files()) {
                        let [old, new, staged] = where_is(&plan, item);
                        prop_assert!(!old && !staged, "{path:?} left behind");
                        let want = if kept(n) { b"theirs".to_vec() } else { bytes(n) };
                        match (c.gone == Some(n) && !kept(n), new) {
                            (true, present) => prop_assert!(!present || c.theirs == Some(n), "{path:?} gone, made again"),
                            (false, _) => prop_assert_eq!(std::fs::read(to.join(&path)).unwrap(), want),
                        }
                    }
                    prop_assert!(!plan.staging.exists() && !from.join("t").exists());
                }
                _ => {
                    prop_assert_eq!(restore(&plan), Vec::<String>::new());
                    for (item, (n, path)) in plan.items.iter().zip(files()) {
                        let [old, new, staged] = where_is(&plan, item);
                        prop_assert!(!staged, "{path:?} still staged");
                        prop_assert_eq!(new, c.theirs == Some(n), "{:?} in the new destination", path);
                        match c.gone == Some(n) {
                            true => prop_assert!(!old),
                            false => prop_assert_eq!(std::fs::read(from.join(&path)).unwrap(), bytes(n)),
                        }
                    }
                    prop_assert!(!plan.staging.exists());
                }
            }
        }
    }

    #[test]
    fn nothing_moves_through_a_link_on_the_way() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to, elsewhere) = (dir.path().join("from"), dir.path().join("to"), dir.path().join("elsewhere"));
        for d in [&from, &to, &elsewhere] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink(&elsewhere, to.join("t")).unwrap();
        assert_eq!(plan(&from, &to, "h", &files(), None, true), Err(Refused::NotAFolder(to.join("t"))));
    }
}
