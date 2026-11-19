//! The file work of a move (#24, `lifecycle::moving` decides its stages): blocking, on a task of
//! its own, never on the actor.
//!
//! [`plan`] says what moves: each of the torrent's files, from its path in the old destination to
//! its path in the new one, as it finds each there (its volume and inode: what tells it from
//! another at the same path), refused when something is at one of those (a clash, unless the
//! person chose what to do with it) or a folder on the way there is a symbolic link or a file.
//! Folders are told apart by what they are on disk, not by how they're spelled ([`same_folder`],
//! [`inside`]). [`stage`] moves them into the move's own folder in the new destination
//! (`.phantom-moving-<hash>`), each renamed (on the same drive) or copied in chunks, a chunk of
//! zeros left a hole, with what's copied told after each chunk and a stop heard between them; one
//! gone from the old destination meanwhile stays out of the move. Then the actor writes ahead the
//! new destination and what's staged of each file, and [`finish`] puts each in its place, checked
//! again just before it takes it (nothing there that wasn't when the plan was made, no link on the
//! way), and only once all have landed removes what's left in the old one. One blocked there is put
//! back into staging with the rest, and the move restored. [`restore`] renames back what was
//! renamed and removes what was copied, while each original is still there as it was; one gone
//! (deleted by hand) leaves no way back: what was staged goes forward ([`forward`]), and one that
//! can't take its place then is left out of the move, back in the old destination, with what's at
//! its path named the person's ([`left_out`]), which librqbit never writes into. Either way the
//! files are in exactly one destination once it's done. Nothing is reached through a symbolic
//! link, as in the Trash's code, and nothing is erased that isn't the torrent's own copy: what
//! Replace and Use existing get rid of goes to the Trash.

use std::collections::BTreeSet;
use std::io::{Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use proto::{AbsolutePath, Clash, NotMoved, Unavailable};
use serde::{Deserialize, Serialize};

use super::trash::{self, Trash};

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
    /// On the same drive: each is renamed if it can be, else copied.
    pub renames: bool,
    pub items: Vec<Item>,
    /// There's no way back ([`Restored::NoWayBack`]): it ends moved, a file that can't take its
    /// place left out of it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forward: bool,
}

/// What tells one file from another at the same path: its volume and inode, which a rename keeps.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ino {
    pub dev: u64,
    pub ino: u64,
}

impl Ino {
    fn of(m: &std::fs::Metadata) -> Self {
        Ino { dev: m.dev(), ino: m.ino() }
    }
}

/// One of the torrent's files.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub index: usize,
    /// Inside the destination: the torrent's own folder, then the file's path in it.
    pub path: PathBuf,
    /// Something was at its path in the new destination, which the torrent takes as its file (Use
    /// existing): its own copy in the old destination goes to the Trash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
    /// Its file in the old destination as the plan found it, and its length: what moves. None: it
    /// wasn't there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<(Ino, u64)>,
    /// Where its bytes are in the old destination, inside it, while its path there holds the
    /// person's: its shadow (`trashed::shadow`). None: at its path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<PathBuf>,
    /// What was at its path in the new destination as the plan found it: Replace sends that to the
    /// Trash, and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theirs: Option<Ino>,
    /// What's staged of it, written ahead once all are: what lands at its new path. A staged file
    /// that is its source was renamed; any other was copied. None: nothing of it moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<Ino>,
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

/// How [`restore`] went.
#[derive(Debug, Clone, PartialEq)]
pub enum Restored {
    /// Every file is back where it was: what can't be done is told.
    Back(Vec<String>),
    /// Some file's original is gone from the old destination (deleted by hand): what was staged
    /// is all there is of it, so nothing went back, and the move goes forward ([`forward`]).
    NoWayBack,
}

impl Plan {
    /// Where its bytes are, inside the old destination: its path, or its shadow.
    fn old_inside<'a>(&self, item: &'a Item) -> &'a Path {
        item.shadow.as_deref().unwrap_or(&item.path)
    }

    fn old(&self, item: &Item) -> PathBuf {
        self.from.join(self.old_inside(item))
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

    /// The files that don't move, by index: not in the old destination (deleted by hand), nor
    /// kept from the new one.
    pub fn missing(&self) -> Vec<usize> {
        self.items.iter().filter(|i| !i.kept && i.staged.is_none()).map(|i| i.index).collect()
    }

    /// Records what [`stage`] staged of each file.
    pub fn staged_as(&mut self, staged: &[(usize, Ino)]) {
        for item in &mut self.items {
            item.staged = staged.iter().find(|(i, _)| *i == item.index).map(|(_, ino)| *ino);
        }
    }
}

fn meta(path: &Path) -> Option<std::fs::Metadata> {
    std::fs::symlink_metadata(path).ok()
}

fn ino(path: &Path) -> Option<Ino> {
    meta(path).map(|m| Ino::of(&m))
}

/// The folder at `path` on disk, following links: what tells it from another, however it's spelled.
fn folder(path: &Path) -> Option<Ino> {
    std::fs::metadata(path).ok().filter(|m| m.is_dir()).map(|m| Ino::of(&m))
}

/// Whether `a` and `b` are the same folder on disk: through a link, in another case on a drive
/// that ignores case, with `..`, or as written.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((folder(a), folder(b)), (Some(a), Some(b)) if a == b)
}

/// Whether the folder `to` is `location` (the torrent's own folder) or inside it, on disk: the
/// folders on its real path, each told by what it is.
pub fn inside(to: &Path, location: &Path) -> bool {
    let Some(own) = folder(location) else { return false };
    let Ok(real) = std::fs::canonicalize(to) else { return false };
    real.ancestors().any(|a| folder(a) == Some(own))
}

/// Whether `a` and `b` are on the same volume, so one can be renamed into the other.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    matches!((std::fs::metadata(a), std::fs::metadata(b)), (Ok(a), Ok(b)) if a.dev() == b.dev())
}

/// Whether the new destination has room for what `plan` copies (the blocks its files take: a
/// file not downloaded in full has holes, which stay holes). One renamed needs none: on the same
/// drive, but not one on another volume mounted inside the torrent's folder, which is copied.
pub fn room(plan: &Plan) -> bool {
    let Ok(to_dev) = std::fs::metadata(&plan.to).map(|m| m.dev()) else { return true };
    let copied = |m: &std::fs::Metadata| !plan.renames || m.dev() != to_dev;
    let sources = plan.items.iter().filter(|i| !i.kept && i.source.is_some()).filter_map(|i| meta(&plan.old(i)));
    let needed: u64 = sources.filter(copied).map(|m| m.blocks() * 512).sum();
    if needed == 0 {
        return true;
    }
    use std::os::unix::ffi::OsStrExt;
    let Ok(to) = std::ffi::CString::new(plan.to.as_os_str().as_bytes()) else { return true };
    // SAFETY: a NUL-terminated path that lives through the call, and a struct it fills.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(to.as_ptr(), &mut stats) } != 0 {
        return true;
    }
    (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64) >= needed
}

/// The move of torrent `hash`'s `files` (each by index, with its path inside its destination) from
/// the destination `from` into `to`, `clash` saying what to do with what's at their paths there.
/// What's at the path of one of `theirs` (by index) in `from` is the person's, not the torrent's:
/// it doesn't move. The torrent's own is its shadow there, which moves to the file's path in `to`;
/// with no shadow the file is missing, and its path in `to` is no clash, as nothing goes there.
pub fn plan(
    from: &Path,
    to: &Path,
    hash: &str,
    files: &[(usize, PathBuf)],
    theirs: &BTreeSet<usize>,
    clash: Option<Clash>,
    renames: bool,
) -> Result<Plan, Refused> {
    let shadow = |index: &usize| theirs.contains(index).then(|| super::trashed::shadows_inside(hash).join(index.to_string()));
    let source = |index: &usize, path: &Path| {
        let inside = shadow(index).unwrap_or_else(|| path.to_owned());
        reached(from, &inside).map(|m| (Ino::of(&m), m.len()))
    };
    let mut taken = Vec::new();
    for (_, path) in files.iter().filter(|(index, path)| !theirs.contains(index) || source(index, path).is_some()) {
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
    let items = files.iter().map(|(index, path)| {
        let at_new = taken.contains(&to.join(path)).then(|| ino(&to.join(path))).flatten();
        Item {
            index: *index,
            path: path.clone(),
            kept: clash == Some(Clash::UseExisting) && at_new.is_some(),
            source: source(index, path),
            shadow: shadow(index),
            theirs: at_new,
            staged: None,
        }
    });
    Ok(Plan {
        from: from.to_owned(),
        to: to.to_owned(),
        staging: to.join(format!(".phantom-moving-{hash}")),
        renames,
        items: items.collect(),
        forward: false,
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
        (Some(libc::EACCES | libc::EPERM | libc::EROFS), _) => match AbsolutePath::new(to) {
            Ok(path) => NotMoved::Unavailable { reason: Unavailable::NoPermission { path } },
            Err(_) => NotMoved::Failed { detail: e.to_string() },
        },
        (Some(libc::EEXIST), _) => at_path(at, |path| NotMoved::Taken { path }),
        (_, std::io::ErrorKind::NotFound) => at_path(at, |path| NotMoved::CopyGone { path }),
        _ => NotMoved::Failed { detail: e.to_string() },
    }
}

/// `why` at `path`, which a move only ever names absolute.
fn at_path(path: &Path, why: impl FnOnce(AbsolutePath) -> NotMoved) -> NotMoved {
    match AbsolutePath::new(path) {
        Ok(path) => why(path),
        Err(p) => NotMoved::Failed { detail: p.display().to_string() },
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

/// Makes the folders on the way to `inside` under `root` that aren't there, one at a time, never
/// through a link: each that's there must be a real folder. Those it made, in `made`.
pub(super) fn make_folders(root: &Path, inside: &Path, made: &mut Vec<PathBuf>) -> Result<(), NotMoved> {
    let mut at = root.to_path_buf();
    for part in inside.parent().into_iter().flat_map(|p| p.components()) {
        at.push(part);
        match meta(&at) {
            Some(m) if m.file_type().is_dir() => {}
            Some(_) => return Err(at_path(&at, |path| NotMoved::NotAFolder { path })),
            None => match std::fs::create_dir(&at) {
                Ok(()) => made.push(at.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && meta(&at).is_some_and(|m| m.file_type().is_dir()) => {}
                Err(e) => return Err(not_moved(&e, root, &at)),
            },
        }
    }
    Ok(())
}

/// Flushes `dir`'s entries to the drive itself (`F_FULLFSYNC`; `fsync` where that isn't had), so
/// what was renamed or made in it is there before anything relies on it.
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let d = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(dir)?;
    // SAFETY: a descriptor open through the call.
    if unsafe { libc::fcntl(d.as_raw_fd(), libc::F_FULLFSYNC) } == 0 {
        return Ok(());
    }
    d.sync_all()
}

/// Moves each file of `plan` not kept into its staging folder, telling `copied` how many bytes of
/// how many it copied so far after each chunk (none for a rename), and waiting in `between` (a
/// test's gate) after each. Once `stop` is set it stops after the chunk in flight. Each is renamed
/// if the plan renames and that works, else copied. A file not in the old destination as the plan
/// found it (gone, or replaced, by hand) stays out of it, as one gone as its copy starts. Each
/// staged file is then looked at again (one gone from there, deleted by hand, fails it) and its
/// folders flushed to the drive. What's staged of each, by index.
pub fn stage(
    plan: &Plan,
    mut copied: impl FnMut(u64, u64),
    stop: &AtomicBool,
    between: &dyn Fn() -> bool,
) -> Result<Vec<(usize, Ino)>, Stop> {
    let fail = |e: &std::io::Error, at: &Path| Stop::Failed(not_moved(e, &plan.to, at));
    match meta(&plan.staging) {
        Some(m) if m.file_type().is_dir() => {}
        Some(_) => return Err(Stop::Failed(at_path(&plan.staging, |path| NotMoved::NotAFolder { path }))),
        None => std::fs::create_dir(&plan.staging).map_err(|e| fail(&e, &plan.to))?,
    }
    let moving: Vec<(&Item, Ino, u64)> =
        plan.items.iter().filter(|i| !i.kept).filter_map(|i| i.source.map(|(ino, len)| (i, ino, len))).collect();
    let total = if plan.renames { 0 } else { moving.iter().map(|(_, _, len)| len).sum() };
    let mut done = 0;
    let mut staged = Vec::new();
    for &(item, source, _) in &moving {
        if stop.load(Ordering::SeqCst) {
            return Err(Stop::Stopped);
        }
        let (old, at) = (plan.old(item), plan.staged(item));
        if reached(&plan.from, plan.old_inside(item)).is_none_or(|m| Ino::of(&m) != source) {
            continue;
        }
        make_folders(&plan.staging, &item.path, &mut Vec::new()).map_err(Stop::Failed)?;
        let renamed = plan.renames && trash::rename_free(&old, &at).is_ok();
        if !renamed {
            let mut count = |n| {
                done += n;
                copied(done, total);
            };
            match copy(&old, &at, &mut count, stop, between) {
                Ok(()) => {}
                // Deleted by hand as its copy starts: it stays out of the move.
                Err(Copy::Gone) => continue,
                Err(Copy::Stopped) => return Err(Stop::Stopped),
                Err(Copy::Read(e)) => return Err(Stop::Failed(NotMoved::Failed { detail: e.to_string() })),
                Err(Copy::Create(e) | Copy::Write(e)) => return Err(fail(&e, &plan.new(item))),
            }
        }
        staged.push(item);
        if !between() {
            return Err(Stop::Stopped);
        }
    }
    let mut found = Vec::new();
    for item in staged {
        let len = item.source.map(|(_, len)| len);
        match meta(&plan.staged(item)).filter(|m| m.file_type().is_file() && Some(m.len()) == len) {
            Some(m) => found.push((item.index, Ino::of(&m))),
            None => return Err(Stop::Failed(at_path(&plan.new(item), |path| NotMoved::CopyGone { path }))),
        }
    }
    let mut folders: BTreeSet<PathBuf> = plan
        .items
        .iter()
        .filter(|i| found.iter().any(|(f, _)| *f == i.index))
        .filter_map(|i| plan.staged(i).parent().map(Path::to_owned))
        .collect();
    folders.insert(plan.staging.clone());
    for dir in folders.iter().rev() {
        sync_dir(dir).map_err(|e| fail(&e, dir))?;
    }
    Ok(found)
}

enum Copy {
    Stopped,
    /// Its source isn't there to read.
    Gone,
    Read(std::io::Error),
    /// The new file couldn't be made: nothing was.
    Create(std::io::Error),
    Write(std::io::Error),
}

/// Copies the file at `from` (never through a link) to a new file at `to`, a chunk at a time, each
/// told to `copied`, then `between` (false: stop); a chunk of zeros is left a hole, as librqbit
/// leaves what it hasn't downloaded. Once `stop` is set it stops after the chunk in flight.
fn copy(from: &Path, to: &Path, copied: &mut dyn FnMut(u64), stop: &AtomicBool, between: &dyn Fn() -> bool) -> Result<(), Copy> {
    let mut src = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(from) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Copy::Gone),
        Err(e) => return Err(Copy::Read(e)),
    };
    let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).custom_flags(libc::O_NOFOLLOW).open(to).map_err(Copy::Create)?;
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

/// Puts each staged file in its place in the new destination, each checked just before it moves:
/// the folders on its way real ones (made where they aren't there), and nothing at its path but
/// what the plan found there and the person chose to Replace, which goes to the Trash first. It
/// takes its place only if that's free (`RENAME_EXCL`), so nothing put there meanwhile is ever
/// replaced. One already there (a start finishing a move a crash cut short) has landed. Once every
/// one has, their folders are flushed to the drive, and what's left of the torrent's files in the
/// old one goes: a copy's original, and the torrent's own copy of one kept (Use existing), which
/// goes to the Trash; then the folders that leaves empty there, and the staging folder. What can't
/// be done of that is told, as warnings. One that can't take its place stops it: those that had go
/// back into staging, what went to the Trash for them comes back (but what a run a crash cut short
/// sent there, which stays in the Trash), and it's why, the old destination as it was. With no way
/// back (`forward`), one that can't take its place goes back to the old destination instead, its
/// original kept there, and the rest go on; [`left_out`] then takes it out of the plan. Use existing
/// whose file at the path is gone by then: the torrent's own copy takes the free path. `between` (a
/// test's gate) after each: false stops it there, as a crash.
pub fn finish(plan: &Plan, trash: &Trash, between: &dyn Fn(&Path) -> bool) -> Result<Vec<String>, NotMoved> {
    let mut landed: Vec<&Item> = Vec::new();
    let mut trashed: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut made: Vec<PathBuf> = Vec::new();
    let mut warnings = Vec::new();
    let mut blocked = None;
    for item in plan.items.iter().filter(|i| !i.kept) {
        let Some(staged) = item.staged else { continue };
        match place(plan, item, staged, trash, &mut made, &mut trashed) {
            Ok(()) => landed.push(item),
            // With no way back, one that can't take its place is left out of the move (see
            // [`left_out`]): its own copy goes back where it was.
            Err(why) if plan.forward => {
                let copy = put_back(plan, item);
                warnings.push(format!("{} left out of the move ({why:?}): its copy is at {}", plan.new(item).display(), copy.display()));
            }
            Err(why) => {
                blocked = Some(why);
                break;
            }
        }
        if !between(&plan.new(item)) {
            return Err(NotMoved::Interrupted);
        }
    }
    if let Some(why) = blocked {
        for item in landed.iter().rev() {
            let (at, new) = (plan.staged(item), plan.new(item));
            if ino(&new) != item.staged || trash::rename_free(&new, &at).is_err() {
                tracing::warn!(path = %new.display(), "a move that couldn't finish left a file in its new destination");
            }
        }
        for (path, to) in trashed.iter().rev() {
            if let Some(to) = to
                && trash.give_back(to, path).is_err()
            {
                tracing::warn!(path = %path.display(), to = %to.display(), "a move that couldn't finish left a file in the Trash");
            }
        }
        // What a run a crash cut short sent to the Trash for Replace stays there: that run logged
        // where it went.
        for item in plan.items.iter().filter(|i| !i.kept && i.theirs.is_some()) {
            let new = plan.new(item);
            if !trashed.iter().any(|(p, _)| *p == new) && ino(&new) != item.theirs && ino(&new) != item.staged {
                tracing::warn!(
                    path = %new.display(),
                    "a move that couldn't finish: the file Replace chose here went to the Trash in a run a crash cut short, and stays there (where: that run's log line)"
                );
            }
        }
        for dir in made.iter().rev() {
            let _ = std::fs::remove_dir(dir);
        }
        return Err(why);
    }
    let parents: BTreeSet<PathBuf> = landed.iter().filter_map(|i| plan.new(i).parent().map(Path::to_owned)).collect();
    for dir in &parents {
        if let Err(e) = sync_dir(dir) {
            warnings.push(format!("{}: {e}", dir.display()));
        }
    }
    for item in &plan.items {
        let old = plan.old(item);
        let Some(m) = reached(&plan.from, plan.old_inside(item)) else { continue };
        // The same file: the old destination is the new one (never so, but never removed).
        if ino(&plan.new(item)) == Some(Ino::of(&m)) {
            continue;
        }
        let ours = item.source.is_some_and(|(source, _)| source == Ino::of(&m));
        if item.kept && ours {
            let new = plan.new(item);
            match meta(&new) {
                // The person's is gone by now: the torrent's own copy takes the free path.
                None => {
                    let placed = make_folders(&plan.to, &item.path, &mut Vec::new())
                        .map_err(|why| format!("{why:?}"))
                        .and_then(|()| take_place(&old, &new).map_err(|e| e.to_string()));
                    if let Err(e) = placed {
                        warnings.push(format!("{}, kept where it is: {e}", old.display()));
                    }
                }
                Some(m) if m.file_type().is_file() => {
                    if let Err(why) = trash.take(&old) {
                        warnings.push(format!("{}: {why:?}", old.display()));
                    }
                }
                Some(_) => warnings.push(format!("{}, kept where it is: no file at {}", old.display(), new.display())),
            }
        } else if ours
            && landed.iter().any(|l| l.index == item.index)
            && let Err(e) = std::fs::remove_file(&old)
        {
            warnings.push(format!("{}: {e}", old.display()));
        }
    }
    tidy_old(plan);
    // The shadows' folder there, once its shadows moved.
    for folder in plan.items.iter().filter_map(|i| i.shadow.as_ref()?.parent()).collect::<BTreeSet<_>>() {
        let _ = std::fs::remove_dir(plan.from.join(folder));
    }
    remove_empty(&plan.staging);
    Ok(warnings)
}

/// Puts `item`'s staged file (`staged`) at its new path, checked just before: see [`finish`].
fn place(
    plan: &Plan,
    item: &Item,
    staged: Ino,
    trash: &Trash,
    made: &mut Vec<PathBuf>,
    trashed: &mut Vec<(PathBuf, Option<PathBuf>)>,
) -> Result<(), NotMoved> {
    let (at, new) = (plan.staged(item), plan.new(item));
    // Landed already: a start finishing a move a crash cut short. Or Replace onto a hard link of
    // its own file, renamed: the staged name is one more name of the file there, and goes.
    if ino(&new) == Some(staged) {
        if ino(&at) == Some(staged) {
            let _ = std::fs::remove_file(&at);
        }
        return Ok(());
    }
    if ino(&at) != Some(staged) {
        return Err(at_path(&new, |path| NotMoved::CopyGone { path }));
    }
    make_folders(&plan.to, &item.path, made)?;
    match meta(&new) {
        None => {}
        Some(m) if m.file_type().is_file() && item.theirs == Some(Ino::of(&m)) => match trash.take(&new) {
            Ok(to) => {
                tracing::info!(path = %new.display(), to = ?to, "Replace sent the file at a new path to the Trash");
                trashed.push((new.clone(), to));
            }
            Err(why) => return Err(NotMoved::Failed { detail: format!("{}: {why:?}", new.display()) }),
        },
        Some(_) => return Err(at_path(&new, |path| NotMoved::Taken { path })),
    }
    rename_excl(&at, &new).map_err(|e| not_moved(&e, &plan.to, &new))
}

/// Renames `from` to `to` only if nothing is at `to`, as an error the system gives.
pub(super) fn rename_excl(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: two NUL-terminated paths that live through the call.
    match unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Moves the file at `from` to `to` only if nothing is at `to`: renamed, or across drives copied
/// (holes stay holes) and then removed.
fn take_place(from: &Path, to: &Path) -> std::io::Result<()> {
    match rename_excl(from, to) {
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
        done => return done,
    }
    match copy(from, to, &mut |_| {}, &AtomicBool::new(false), &|| true) {
        Ok(()) => std::fs::remove_file(from),
        Err(Copy::Create(e)) => Err(e),
        Err(failed) => {
            let _ = std::fs::remove_file(to);
            Err(match failed {
                Copy::Read(e) | Copy::Write(e) | Copy::Create(e) => e,
                Copy::Gone => std::io::ErrorKind::NotFound.into(),
                Copy::Stopped => std::io::Error::other("stopped"),
            })
        }
    }
}

/// `item`, left out of a move with no way back, goes back to where it was in the old destination,
/// where the person can find it: a copy whose original is there as the plan found it is removed,
/// the original left as it is; any other staged file (renamed, or a copy that's all there is of it)
/// takes its old path if that's free, the folders on the way made again. Where the torrent's own
/// copy is now: one that can't go back stays staged, and is said where.
fn put_back(plan: &Plan, item: &Item) -> PathBuf {
    let (at, old) = (plan.staged(item), plan.old(item));
    // Nothing staged of it (gone, or back already: a start running the end again).
    if item.staged.is_none() || ino(&at) != item.staged {
        return old;
    }
    let original = item
        .source
        .is_some_and(|(source, len)| reached(&plan.from, plan.old_inside(item)).is_some_and(|m| Ino::of(&m) == source && m.len() == len));
    if original {
        return match std::fs::remove_file(&at) {
            Ok(()) => old,
            Err(_) => at,
        };
    }
    let back = make_folders(&plan.from, plan.old_inside(item), &mut Vec::new()).ok().and_then(|()| take_place(&at, &old).ok());
    match back {
        Some(()) => old,
        None => {
            tracing::warn!(path = %at.display(), "a file left out of a move couldn't go back where it was: it stays staged");
            at
        }
    }
}

/// Once a move ends moved ([`finish`]): each file that isn't at its new path as the move's is out of
/// it, so it's [missing](Plan::missing): one whose staged file didn't take its place (one with no
/// way back left out), and one kept (Use existing) with no file there by now, whose own copy
/// couldn't take the free path. The person's paths in the new destination: by index, each file
/// missing there (those, one gone by hand, or one whose path in the old destination was the
/// person's) with something at its path, a file or a link, which is the person's, not the torrent's.
pub fn left_out(plan: &mut Plan) -> BTreeSet<usize> {
    let mut theirs = BTreeSet::new();
    for i in 0..plan.items.len() {
        let item = &plan.items[i];
        let new = plan.new(item);
        let landed = match item.kept {
            true => meta(&new).is_some_and(|m| m.file_type().is_file()),
            false => item.staged.is_some_and(|staged| ino(&new) == Some(staged)),
        };
        if landed {
            continue;
        }
        if meta(&new).is_some() {
            theirs.insert(item.index);
        }
        (plan.items[i].staged, plan.items[i].kept) = (None, false);
    }
    theirs
}

/// Puts back what [`stage`] did, when it can: each staged file that is its source (renamed) back to
/// its path in the old destination, never over anything there, nor making a folder there again
/// (the stage left them all); each other (copied) removed, while its original is there as the plan
/// found it. One whose original is gone (deleted by hand meanwhile), or renamed with its folder in
/// the old destination gone, is all there is of it: nothing is touched, and it's
/// [`Restored::NoWayBack`]. Then the staging folder goes. A start after a crash runs it again:
/// what's back already is.
pub fn restore(plan: &Plan) -> Restored {
    let mut undo = Vec::new();
    for item in plan.items.iter().filter(|i| !i.kept) {
        let (Some((source, len)), Some(at)) = (item.source, meta(&plan.staged(item)).filter(|m| m.file_type().is_file())) else {
            continue;
        };
        let renamed = Ino::of(&at) == source;
        let original = reached(&plan.from, plan.old_inside(item)).is_some_and(|m| Ino::of(&m) == source && m.len() == len);
        // Its folders there, the old destination's own too: real ones, as the stage left them.
        let folders = plan.old_inside(item).ancestors().skip(1).all(|a| meta(&plan.from.join(a)).is_some_and(|m| m.file_type().is_dir()));
        match (renamed, original) {
            (true, _) if folders && meta(&plan.old(item)).is_none() => undo.push((item, true)),
            (false, true) => undo.push((item, false)),
            (true, _) | (false, false) => return Restored::NoWayBack,
        }
    }
    let mut warnings = Vec::new();
    for (item, renamed) in undo {
        let (at, old) = (plan.staged(item), plan.old(item));
        let undone = match renamed {
            true => trash::rename_free(&at, &old),
            false => std::fs::remove_file(&at).map_err(|e| e.to_string()),
        };
        if let Err(e) = undone {
            warnings.push(format!("{}: {e}", at.display()));
        }
    }
    remove_empty(&plan.staging);
    Restored::Back(warnings)
}

/// A move with no way back ([`Restored::NoWayBack`]) goes forward: what's staged of each file is
/// what lands, written in `plan`. A copy cut short whose original is still there is removed (the
/// original stays where it is, out of the move); one whose original is gone is kept, as all there
/// is of it.
pub fn forward(plan: &mut Plan) {
    plan.forward = true;
    for i in 0..plan.items.len() {
        let item = &plan.items[i];
        let Some((source, len)) = item.source.filter(|_| !item.kept) else { continue };
        let at = plan.staged(item);
        let Some(m) = meta(&at).filter(|m| m.file_type().is_file()) else { continue };
        let original = reached(&plan.from, plan.old_inside(item)).is_some_and(|o| Ino::of(&o) == source);
        let staged = match (m.len() == len || Ino::of(&m) == source, original) {
            (false, true) => {
                let _ = std::fs::remove_file(&at);
                None
            }
            _ => Some(Ino::of(&m)),
        };
        plan.items[i].staged = staged;
    }
}

/// The folders of the torrent's own folder in the old destination left with nothing but litter go.
fn tidy_old(plan: &Plan) {
    let own: BTreeSet<PathBuf> = plan.items.iter().filter_map(|i| i.path.components().next()).map(|c| plan.from.join(c)).collect();
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

#[cfg(test)]
mod tests;
