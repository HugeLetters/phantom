//! Delete moves a torrent's files to the Trash, so they can still be put back, and nothing else
//! in its destination moves. [`plan`] says what goes: the torrent's own folder whole when it holds
//! only the torrent's files (and Finder's litter, such as `.DS_Store`), else each of its files
//! that's on disk, so a file of the user's beside them stays. Nothing is reached through a
//! symbolic link: one the user made, inside the torrent's folder or in place of a file, is theirs.
//! [`carry_out`] trashes the plan, all of it, keeping what couldn't go and telling where each item
//! went ([`Trashed`]), and then removes the folders left with nothing of the user's ([`tidy`]).
//! [`put_back`] is Undo's way back: each item from where it went in the Trash to the path it had,
//! never over anything there, all of them or none. [`Trash`] is macOS's Trash ([`system`]), or in
//! tests a folder of their own, so a test never fills the user's Trash. Nothing is ever erased
//! instead.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proto::KeptWhy;
use serde::{Deserialize, Serialize};

/// Where Delete moves files, and Undo takes them back from.
pub trait Bin: Send + Sync {
    /// Moves one file or folder to the Trash: where it went there, if the system says.
    fn take(&self, path: &Path) -> Result<Option<PathBuf>, KeptWhy>;
    /// Moves `from`, in the Trash, back to `to`, which must be free: the system's text if it can't.
    fn give_back(&self, from: &Path, to: &Path) -> Result<(), String>;
    /// Moves `path`, which an undo put back, to `to`, where it was in the Trash, which must be
    /// free: the system's text if it can't. An undo that can't finish returns what it put back so.
    fn return_to(&self, path: &Path, to: &Path) -> Result<(), String> {
        rename_free(path, to)
    }
    /// After each item taken or given back (`item`, where it was or goes back), once it's told:
    /// where a test holds a Delete or an undo under way.
    fn after_each(&self, _item: &Path) {}
}

pub type Trash = Arc<dyn Bin>;

/// An item Delete moved to the Trash: the path it had, where it went, and what it was there
/// (either unknown leaves it out of reach of Undo).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    pub from: PathBuf,
    pub to: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<Identity>,
}

/// What tells one file or folder from another at the same path: its volume and inode, which a
/// rename keeps, and a file's length and modification time. A folder's own are left out: Finder
/// writes into folders it shows, the Trash's too.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Identity {
    pub dev: u64,
    pub ino: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Stamp>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub len: u64,
    pub secs: i64,
    pub nanos: i64,
}

/// What's at `path`, not following a symbolic link there.
pub fn identity(path: &Path) -> Option<Identity> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok()?;
    let file = m.file_type().is_file().then(|| Stamp { len: m.len(), secs: m.mtime(), nanos: m.mtime_nsec() });
    Some(Identity { dev: m.dev(), ino: m.ino(), file })
}

/// The Trash in Finder, through `NSFileManager trashItemAtURL`, in an autorelease pool of its own:
/// it runs on a blocking thread, which has none.
pub fn system() -> Trash {
    Arc::new(System)
}

struct System;

impl Bin for System {
    fn take(&self, path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
        objc2::rc::autoreleasepool(|_| to_trash(path))
    }

    fn give_back(&self, from: &Path, to: &Path) -> Result<(), String> {
        rename_free(from, to)
    }
}

/// Renames `from` to `to` only if nothing is at `to` (`renamex_np` with `RENAME_EXCL`): the check
/// and the move are one step, so nothing put there meanwhile is replaced.
pub fn rename_free(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string());
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: two NUL-terminated paths that live through the call.
    match unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error().to_string()),
    }
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
            items.extend(files.iter().filter(|f| reached(location, f)).cloned());
        }
    }
    Plan { location: location.to_owned(), items }
}

/// What to trash of the files `chosen` (a subset of a torrent's `files`) at `location`: each that's
/// a real file reached through real folders only, and, for a torrent of one file (`single`), the
/// file at `location` itself. Never the folder, which holds the torrent's other files, and never
/// anything through a symbolic link. Its folder is only tidied ([`carry_out`]) of what's left empty.
pub fn plan_files(location: &Path, chosen: &[PathBuf], single: bool) -> Plan {
    let meta = |p: &Path| std::fs::symlink_metadata(p).ok();
    let mut items = Vec::new();
    if single {
        items.extend((!chosen.is_empty() && meta(location).is_some_and(|m| real_file(&m))).then(|| location.to_owned()));
    } else if meta(location).is_some_and(|m| real_dir(&m)) {
        items.extend(chosen.iter().filter(|f| reached(location, f)).cloned());
    }
    Plan { location: location.to_owned(), items }
}

/// Whether `file` is a real file inside `location` reached through real folders only.
fn reached(location: &Path, file: &Path) -> bool {
    let meta = |p: &Path| std::fs::symlink_metadata(p).ok();
    let Ok(inside) = file.strip_prefix(location) else { return false };
    let folders = inside.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty());
    folders.into_iter().all(|a| meta(&location.join(a)).is_some_and(|m| real_dir(&m))) && meta(file).is_some_and(|m| real_file(&m))
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

/// How a [`carry_out`] went: what went and where, and what couldn't go, with why.
#[derive(Debug, Default, PartialEq)]
pub struct Carried {
    pub moved: Vec<Trashed>,
    pub kept: Vec<(PathBuf, KeptWhy)>,
}

/// Trashes every item of `plan`, carrying on past failures, telling `went` of each that goes as it
/// goes, then tidies the folder.
#[cfg(test)]
pub fn carry_out(plan: &Plan, trash: &Trash, went: impl FnMut(&Trashed)) -> Carried {
    carry_out_until(plan, trash, went, &std::sync::atomic::AtomicBool::new(false))
}

/// [`carry_out`], but once `stop` is set (an Undo aborting it, #190) the item in flight ends and
/// nothing more is tried, nor tidied: the rest are kept, interrupted, and the undo that follows
/// puts back what went.
pub fn carry_out_until(plan: &Plan, trash: &Trash, went: impl FnMut(&Trashed), stop: &std::sync::atomic::AtomicBool) -> Carried {
    carry_out_holding(plan, trash, went, stop, |take| take())
}

/// [`carry_out_until`], each item moving inside `around`, which takes it and may hold something
/// meanwhile (a Move to Trash holds the torrent's files still, `trashed::Trashed::take`).
pub fn carry_out_holding(
    plan: &Plan,
    trash: &Trash,
    mut went: impl FnMut(&Trashed),
    stop: &std::sync::atomic::AtomicBool,
    around: impl Fn(&mut dyn FnMut() -> Result<Trashed, KeptWhy>) -> Result<Trashed, KeptWhy>,
) -> Carried {
    let mut carried = Carried::default();
    for (n, item) in plan.items.iter().enumerate() {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            carried.kept.extend(plan.items[n..].iter().map(|item| (item.clone(), KeptWhy::Interrupted)));
            return carried;
        }
        let taken = around(&mut || {
            let to = trash.take(item)?;
            let seen = to.as_deref().and_then(identity);
            Ok(Trashed { from: item.clone(), to, seen })
        });
        match taken {
            Ok(moved) => {
                went(&moved);
                carried.moved.push(moved);
            }
            Err(why) => carried.kept.push((item.clone(), why)),
        }
        trash.after_each(item);
    }
    if std::fs::symlink_metadata(&plan.location).is_ok_and(|m| real_dir(&m)) {
        tidy(&plan.location);
    }
    carried
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

/// Why an item can't go back: found before anything moves, and again just before it does.
#[derive(Debug, Clone, PartialEq)]
pub enum Blocked {
    /// What's where it went in the Trash isn't what went there (or where is unknown).
    NotInTrash(PathBuf),
    /// Something is at the path it had.
    Taken(PathBuf),
    /// A folder on its way back, inside the destination, isn't a real folder: a symbolic link
    /// (which would take it elsewhere) or a file. Nothing is ever reached through a link.
    NotAFolder(PathBuf),
}

/// The items still in the Trash as they went: what an undo of a Move to Trash has left to put back
/// when some were emptied from it.
pub fn still_in_trash(items: &[Trashed]) -> Vec<Trashed> {
    items
        .iter()
        .filter(|item| matches!((&item.to, &item.seen), (Some(to), Some(seen)) if identity(to).as_ref() == Some(seen)))
        .cloned()
        .collect()
}

/// The torrent's own new copies of files a Move to Trash took (#276): set Normal since, made again
/// at their paths and downloaded there. Its undo sends them to the Trash (never erased), then puts
/// the old ones back: the old copy always wins. `location` is the torrent's folder (or its one file).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Copies {
    pub location: PathBuf,
    pub files: Vec<PathBuf>,
}

impl Copies {
    /// Whether what's at `path` is only these: one of them, or a real folder of only them, litter
    /// and real folders of only those.
    fn hold_only(&self, path: &Path) -> bool {
        match std::fs::symlink_metadata(path) {
            Ok(m) if real_file(&m) => self.files.iter().any(|f| f == path),
            Ok(m) if real_dir(&m) => only(path, &self.files),
            Ok(_) | Err(_) => false,
        }
    }
}

/// Whether every item can go back now: each still in the Trash as it went, nothing at the path it
/// had but the torrent's own new `copies` of it, and every folder on its way inside `destination` a
/// real one or not there yet.
pub fn can_put_back(items: &[Trashed], destination: &Path, copies: &Copies) -> Result<(), Blocked> {
    items.iter().try_for_each(|item| can_go_back(item, destination, copies))
}

fn can_go_back(item: &Trashed, destination: &Path, copies: &Copies) -> Result<(), Blocked> {
    let in_trash = matches!((&item.to, &item.seen), (Some(to), Some(seen)) if identity(to).as_ref() == Some(seen));
    if !in_trash {
        return Err(Blocked::NotInTrash(item.from.clone()));
    }
    if std::fs::symlink_metadata(&item.from).is_ok() && !copies.hold_only(&item.from) {
        return Err(Blocked::Taken(item.from.clone()));
    }
    on_the_way(destination, &item.from)
}

/// Every folder between `destination` and `path` (not `destination` itself, which the user chose)
/// that's there is a real folder; past the first that isn't there, none is.
fn on_the_way(destination: &Path, path: &Path) -> Result<(), Blocked> {
    let Some(inside) = path.parent().and_then(|p| p.strip_prefix(destination).ok()) else {
        return Err(Blocked::NotAFolder(path.to_owned()));
    };
    let mut at = destination.to_path_buf();
    for part in inside.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if real_dir(&m) => {}
            Ok(_) => return Err(Blocked::NotAFolder(at)),
            Err(_) => return Ok(()),
        }
    }
    Ok(())
}

/// Why [`put_back`] didn't.
#[derive(Debug, Clone, PartialEq)]
pub enum PutBackError {
    Blocked(Blocked),
    /// Making a folder or moving the item failed: the system's text.
    Failed {
        path: PathBuf,
        detail: String,
    },
}

/// Moves every item back from the Trash to the path it had, last moved first, making the folders on
/// its way inside `destination` (never `destination` itself: a drive that went stays gone), never
/// over anything there and never through a symbolic link, each checked again just before it
/// moves: all of them, or, at the first that can't go, none (those already back go to where they
/// were in the Trash, and the folders made go). `resumed`: an undo cut short by a crash, whose
/// items no longer in the Trash are the ones it put back before (those at their path as they
/// went), or ones emptied from the Trash meanwhile, which stay gone while their path is free
/// (#162); an item that can't go back, an emptied one whose path is taken too, returns those it
/// put back before the crash as well, as any undo that can't finish does, and the folders left
/// empty on their way go. A folder is told by its volume and inode alone, so what was put into one
/// put back before the crash goes to the Trash with it. What's back, in the order it went to the
/// Trash; or why not, with what couldn't return to the Trash.
pub fn put_back(
    items: &[Trashed],
    destination: &Path,
    trash: &Trash,
    resumed: bool,
    copies: &Copies,
) -> Result<Vec<Trashed>, (PutBackError, Vec<PathBuf>)> {
    // The torrent's own new copies go to the Trash first, as a Move to Trash sends them; given back
    // if the undo can't finish, so a refused one changes nothing.
    let sent = match copies.files.is_empty() {
        true => Carried::default(),
        false => {
            let plan = Plan { location: copies.location.clone(), items: copies.files.clone() };
            carry_out_until(&plan, trash, |_| {}, &std::sync::atomic::AtomicBool::new(false))
        }
    };
    if let Some((path, why)) = sent.kept.first() {
        let detail = format!("{why:?}");
        let failed = PutBackError::Failed { path: path.clone(), detail };
        return Err((failed, give_back(trash, &sent.moved)));
    }
    let put = put_back_items(items, destination, trash, resumed);
    match put {
        Ok(back) => Ok(back),
        Err((failed, mut stranded)) => {
            stranded.extend(give_back(trash, &sent.moved));
            Err((failed, stranded))
        }
    }
}

/// Gives `sent` back from the Trash, to their paths: those that couldn't go back.
fn give_back(trash: &Trash, sent: &[Trashed]) -> Vec<PathBuf> {
    let mut stuck = Vec::new();
    for item in sent.iter().rev() {
        let given = item.to.as_deref().map_or(Err("where it went is unknown".to_owned()), |to| {
            if let Some(parent) = item.from.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            trash.give_back(to, &item.from)
        });
        if let Err(e) = given {
            tracing::warn!(path = %item.from.display(), error = %e, "failed to give back a new copy after a failed undo");
            stuck.push(item.from.clone());
        }
    }
    stuck
}

/// [`put_back`] of the items alone.
fn put_back_items(items: &[Trashed], destination: &Path, trash: &Trash, resumed: bool) -> Result<Vec<Trashed>, (PutBackError, Vec<PathBuf>)> {
    let mut back: Vec<&Trashed> = Vec::new();
    // Put back before the crash: the folders on their way, which that run made, aren't in `made`.
    let mut before: Vec<&Trashed> = Vec::new();
    let mut made: Vec<PathBuf> = Vec::new();
    let mut failed = None;
    for item in items.iter().rev() {
        if resumed && item.to.as_deref().is_none_or(|to| std::fs::symlink_metadata(to).is_err()) {
            if item.seen.is_some() && identity(&item.from) == item.seen {
                back.push(item);
                before.push(item);
            } else if std::fs::symlink_metadata(&item.from).is_ok() {
                failed = Some(PutBackError::Blocked(Blocked::Taken(item.from.clone())));
                break;
            }
            continue;
        }
        if let Err(blocked) = can_go_back(item, destination, &Copies::default()) {
            failed = Some(PutBackError::Blocked(blocked));
            break;
        }
        let to = item.to.as_deref().expect("checked");
        if let Some(parent) = item.from.parent() {
            let missing: Vec<PathBuf> =
                parent.ancestors().take_while(|a| std::fs::symlink_metadata(a).is_err()).map(Path::to_owned).collect();
            let made_here = match missing.last() {
                Some(top) if !top.starts_with(destination) || top == destination => Err(format!("{} isn't there", destination.display())),
                _ => std::fs::create_dir_all(parent).map_err(|e| e.to_string()),
            };
            if let Err(detail) = made_here {
                failed = Some(PutBackError::Failed { path: item.from.clone(), detail });
                break;
            }
            made.extend(missing.into_iter().rev());
        }
        let given = trash.give_back(to, &item.from);
        trash.after_each(&item.from);
        match given {
            Ok(()) => back.push(item),
            Err(detail) => {
                failed = Some(PutBackError::Failed { path: item.from.clone(), detail });
                break;
            }
        }
    }
    let Some(failed) = failed else { return Ok(back.into_iter().rev().cloned().collect()) };
    let stranded = take_back(trash, back.into_iter().rev());
    for dir in made.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
    // Only an empty folder goes: nothing of the user's.
    for item in before {
        for dir in item.from.ancestors().skip(1).take_while(|d| d.starts_with(destination) && *d != destination) {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Err((failed, stranded))
}

/// Returns items put back to where they were in the Trash, in turn: those that couldn't go.
pub fn take_back<'a>(trash: &Trash, items: impl IntoIterator<Item = &'a Trashed>) -> Vec<PathBuf> {
    let mut stranded = Vec::new();
    for item in items {
        let returned = item.to.as_deref().map_or(Err("where it went is unknown".to_owned()), |to| trash.return_to(&item.from, to));
        if let Err(e) = returned {
            tracing::warn!(path = %item.from.display(), error = %e, "failed to return an item to the Trash after a failed undo");
            stranded.push(item.from.clone());
        }
    }
    stranded
}

/// A test trash's gate: open, held (it waits after each item it takes, so a Delete under way
/// has moved its first), held for the items under one folder only (one torrent's, while the
/// others go on), or abandoned (a killed service's: it takes nothing more).
#[cfg(test)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Hold {
    #[default]
    Open,
    Held,
    HeldUnder(PathBuf),
    Abandoned,
}

#[cfg(test)]
pub type Gate = Arc<(std::sync::Mutex<Hold>, std::sync::Condvar, std::sync::atomic::AtomicUsize)>;

/// Whether something waits at `gate` now: a Delete or an undo, held after an item.
#[cfg(test)]
pub fn waiting(gate: &Gate) -> bool {
    waiters(gate) > 0
}

/// How many trashings or put-backs wait at `gate` now, side by side.
#[cfg(test)]
pub fn waiters(gate: &Gate) -> usize {
    gate.2.load(std::sync::atomic::Ordering::SeqCst)
}

/// Holds `gate` shut for the items under `folder` alone.
#[cfg(test)]
pub fn hold_under(gate: &Gate, folder: &Path) {
    set(gate, Hold::HeldUnder(folder.to_owned()));
}

/// Holds `gate` shut, or opens it.
#[cfg(test)]
pub fn hold(gate: &Gate, held: bool) {
    set(gate, if held { Hold::Held } else { Hold::Open });
}

/// What a crash does to the trashing under way: what it hasn't taken stays.
#[cfg(test)]
pub fn abandon(gate: &Gate) {
    set(gate, Hold::Abandoned);
}

#[cfg(test)]
fn set(gate: &Gate, hold: Hold) {
    *gate.0.lock().unwrap() = hold;
    gate.1.notify_all();
}

/// A trash of its own in `dir`, for tests: each item moves there, named after its path (after a
/// count, as the Trash does to two of one name), but for the paths in `refusing`, which it keeps,
/// as a locked file; after each item it waits while `gate` is held.
#[cfg(test)]
pub fn into(dir: PathBuf, refusing: Arc<std::sync::Mutex<Vec<PathBuf>>>, gate: Gate) -> Trash {
    Arc::new(Folder { dir, refusing, gate })
}

#[cfg(test)]
pub struct Folder {
    dir: PathBuf,
    refusing: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    gate: Gate,
}

#[cfg(test)]
impl Folder {
    /// Whether the gate was abandoned, as a crash leaves it.
    fn abandoned(&self) -> bool {
        *self.gate.0.lock().unwrap() == Hold::Abandoned
    }
}

#[cfg(test)]
impl Bin for Folder {
    fn take(&self, path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
        // One count for every trash: a service started again in the same folder goes on from it.
        static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if self.abandoned() {
            return Err(KeptWhy::Interrupted);
        }
        let moved = (|| {
            if self.refusing.lock().unwrap().iter().any(|r| r == path) {
                return Err(KeptWhy::Failed { detail: "locked".into() });
            }
            std::fs::create_dir_all(&self.dir).map_err(|e| KeptWhy::Failed { detail: e.to_string() })?;
            let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let name = format!("{n}%{}", path.to_string_lossy().trim_start_matches('/').replace('/', "%"));
            let to = self.dir.join(name);
            std::fs::rename(path, &to).map_err(|e| KeptWhy::Failed { detail: e.to_string() })?;
            Ok(Some(to))
        })();
        moved
    }

    fn give_back(&self, from: &Path, to: &Path) -> Result<(), String> {
        if self.abandoned() {
            return Err("interrupted".into());
        }
        rename_free(from, to)
    }

    /// A killed service's returns nothing either: a crash leaves what was put back where it is.
    fn return_to(&self, path: &Path, to: &Path) -> Result<(), String> {
        if self.abandoned() {
            return Err("interrupted".into());
        }
        rename_free(path, to)
    }

    fn after_each(&self, item: &Path) {
        wait_at(&self.gate, item);
    }
}

/// Waits at `gate` while it's held for `item`: whether to go on (not once it's abandoned, as a
/// crash leaves it). A move's copy waits here too, after each chunk.
#[cfg(test)]
pub fn wait_at(gate: &Gate, item: &Path) -> bool {
    let (hold, changed, waiting) = &**gate;
    let shut = |hold: &mut Hold| match hold {
        Hold::Held => true,
        Hold::HeldUnder(folder) => item.starts_with(folder),
        Hold::Open | Hold::Abandoned => false,
    };
    let mut held = hold.lock().unwrap();
    if shut(&mut held) {
        waiting.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        held = changed.wait_while(held, |hold| shut(hold)).unwrap();
        waiting.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    *held != Hold::Abandoned
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

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
            let carried = carry_out(&plan(&location, &files, t.single), &into(trash_dir.clone(), Arc::default(), Arc::default()), |_| {});
            prop_assert!(carried.kept.is_empty());

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

    /// Every file under `dir` with its bytes, and every symbolic link with its target.
    fn contents(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        tree(dir)
            .into_iter()
            .filter_map(|p| {
                let full = dir.join(&p);
                let m = std::fs::symlink_metadata(&full).ok()?;
                if m.file_type().is_symlink() {
                    return Some((p, std::fs::read_link(&full).unwrap().into_os_string().into_encoded_bytes()));
                }
                m.is_file().then(|| (p, std::fs::read(&full).unwrap()))
            })
            .filter(|(p, _)| !is_litter(p.file_name().unwrap()))
            .collect()
    }

    proptest! {
        /// Stopped after its `k`th item (an Undo aborting it): what went is the plan's first `k`, in
        /// order, and the rest stay where they were, kept as interrupted.
        #[test]
        fn a_trashing_stopped_moves_what_came_before_and_keeps_the_rest(t in trees(), k in 0usize..6) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let plan = plan(&location, &files, t.single);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let stop = std::sync::atomic::AtomicBool::new(k == 0);
            let there: Vec<bool> = plan.items.iter().map(|i| std::fs::symlink_metadata(i).is_ok()).collect();
            let mut went = 0;
            let carried = carry_out_until(&plan, &trash, |_| {
                went += 1;
                if went == k {
                    stop.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }, &stop);
            let moved: Vec<&PathBuf> = carried.moved.iter().map(|m| &m.from).collect();
            let first: Vec<&PathBuf> = plan.items.iter().take(k.min(plan.items.len())).collect();
            prop_assert_eq!(moved, first);
            let rest: Vec<(PathBuf, KeptWhy)> = plan.items.iter().skip(k).map(|i| (i.clone(), KeptWhy::Interrupted)).collect();
            prop_assert_eq!(&carried.kept, &rest);
            for (n, item) in plan.items.iter().enumerate().skip(k) {
                prop_assert_eq!(std::fs::symlink_metadata(item).is_ok(), there[n], "{} stays as it was", item.display());
            }
        }
    }

    proptest! {
        /// Delete, then putting back what it moved: every file is back at its path with its bytes,
        /// and the trash is empty again. Only Finder's litter, which tidying a folder removes, and
        /// empty folders may be gone.
        #[test]
        fn putting_back_what_delete_moved_leaves_the_same_bytes_at_the_same_paths(t in trees()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let before = contents(root.path());
            let trash_dir = root.path().join("trash");
            let trash = into(trash_dir.clone(), Arc::default(), Arc::default());
            let mut told = Vec::new();
            let carried = carry_out(&plan(&location, &files, t.single), &trash, |m| told.push(m.clone()));
            prop_assert_eq!(&told, &carried.moved, "told of each item as it went");
            prop_assert_eq!(can_put_back(&carried.moved, root.path(), &Copies::default()), Ok(()));
            prop_assert_eq!(put_back(&carried.moved, root.path(), &trash, false, &Copies::default()), Ok(carried.moved.clone()));
            prop_assert_eq!(contents(root.path()), before);
            prop_assert!(tree(&trash_dir).is_empty(), "nothing left in the trash");
        }

        /// One item that can't go back, as a locked one: none goes back, everything is where Delete
        /// left it, and no folder was made for it.
        #[test]
        fn a_put_back_that_fails_puts_back_nothing(t in trees(), fail in any::<prop::sample::Index>()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let carried = carry_out(&plan(&location, &files, t.single), &trash, |_| {});
            prop_assume!(!carried.moved.is_empty());
            let after_delete = contents(root.path());
            let dirs_after = tree(root.path());
            let failing = carried.moved[fail.index(carried.moved.len())].from.clone();
            struct Fails(Trash, PathBuf);
            impl Bin for Fails {
                fn take(&self, path: &Path) -> Result<Option<PathBuf>, KeptWhy> { self.0.take(path) }
                fn give_back(&self, from: &Path, to: &Path) -> Result<(), String> {
                    if to == self.1 { Err("locked".into()) } else { self.0.give_back(from, to) }
                }
            }
            let failing_trash: Trash = Arc::new(Fails(trash, failing.clone()));
            let failed = PutBackError::Failed { path: failing, detail: "locked".to_owned() };
            prop_assert_eq!(put_back(&carried.moved, root.path(), &failing_trash, false, &Copies::default()), Err((failed, Vec::new())));
            prop_assert_eq!(contents(root.path()), after_delete);
            prop_assert_eq!(tree(root.path()), dirs_after);
        }
    }

    /// A torrent folder whose files go one by one, at least two: the user's file is in it.
    fn split_trees() -> impl Strategy<Value = Tree> {
        trees().prop_map(|mut t| {
            t.single = false;
            t.mine = Some(t.mine.unwrap_or(0));
            t.on_disk[0] = t.on_disk[0].or(Some(true));
            t.on_disk[1] = t.on_disk[1].or(Some(true));
            t
        })
    }

    /// As [`split_trees`], with three files on disk, each its own item.
    fn split_trees3() -> impl Strategy<Value = Tree> {
        split_trees().prop_map(|mut t| {
            t.sub_is_link = false;
            t.on_disk[2] = t.on_disk[2].or(Some(true));
            t
        })
    }

    /// Puts back the last `n` items Delete moved, last first, as an undo does before a crash
    /// cuts it short.
    fn put_back_first(moved: &[Trashed], n: usize) {
        for item in moved.iter().rev().take(n) {
            std::fs::create_dir_all(item.from.parent().unwrap()).unwrap();
            rename_free(item.to.as_deref().unwrap(), &item.from).unwrap();
        }
    }

    proptest! {
        /// An undo cut short by a crash once it put back some items, resumed with one of the rest
        /// blocked (a file of the user's put at its path meanwhile): nothing is back, what the
        /// first run put back is in the Trash again too, and the user's file stays (#162).
        #[test]
        fn a_resumed_put_back_that_is_blocked_puts_back_nothing(t in split_trees(), cut in any::<prop::sample::Index>(), block in any::<prop::sample::Index>()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let moved = carry_out(&plan(&location, &files, t.single), &trash, |_| {}).moved;
            prop_assume!(moved.len() >= 2);
            let mut want = contents(root.path());
            let first = 1 + cut.index(moved.len() - 1);
            put_back_first(&moved, first);
            let blocked = &moved[block.index(moved.len() - first)];
            write(&blocked.from);
            want.insert(blocked.from.strip_prefix(root.path()).unwrap().to_owned(), b"x".to_vec());
            let refused = put_back(&moved, root.path(), &trash, true, &Copies::default());
            prop_assert_eq!(refused, Err((PutBackError::Blocked(Blocked::Taken(blocked.from.clone())), Vec::new())));
            prop_assert_eq!(contents(root.path()), want);
        }

        /// An undo cut short by a crash, resumed after one item still in the Trash was emptied from
        /// it and a file of the user's put at its path, and another blocked so too: nothing is
        /// back, and both of the user's files stay at their paths, none in the Trash (#162).
        #[test]
        fn a_resumed_put_back_never_takes_the_users_file_at_an_emptied_items_path(t in split_trees3(), cut in any::<prop::sample::Index>(), pick in any::<(prop::sample::Index, prop::sample::Index)>()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let moved = carry_out(&plan(&location, &files, t.single), &trash, |_| {}).moved;
            prop_assume!(moved.len() >= 3);
            let first = 1 + cut.index(moved.len() - 2);
            let left = moved.len() - first;
            let emptied = pick.0.index(left);
            let blocked = (emptied + 1 + pick.1.index(left - 1)) % left;
            let mut want = contents(root.path());
            put_back_first(&moved, first);
            let to = moved[emptied].to.as_deref().unwrap();
            want.remove(to.strip_prefix(root.path()).unwrap());
            std::fs::remove_file(to).unwrap();
            for item in [&moved[emptied], &moved[blocked]] {
                write(&item.from);
                want.insert(item.from.strip_prefix(root.path()).unwrap().to_owned(), b"x".to_vec());
            }
            // The first of the two in the order they go back.
            let taken = moved[emptied.max(blocked)].from.clone();
            prop_assert_eq!(put_back(&moved, root.path(), &trash, true, &Copies::default()), Err((PutBackError::Blocked(Blocked::Taken(taken)), Vec::new())));
            prop_assert_eq!(contents(root.path()), want);
        }

        /// An undo cut short by a crash, resumed after some of what's still in the Trash was
        /// emptied from it: what's left goes back, the emptied items stay gone, and what's back is
        /// both runs' (#162).
        #[test]
        fn a_resumed_put_back_puts_back_what_is_left_in_the_trash(t in split_trees(), cut in any::<prop::sample::Index>(), emptied in any::<prop::sample::Index>()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let moved = carry_out(&plan(&location, &files, t.single), &trash, |_| {}).moved;
            prop_assume!(moved.len() >= 2);
            let first = 1 + cut.index(moved.len() - 1);
            put_back_first(&moved, first);
            let gone = &moved[emptied.index(moved.len() - first)];
            let to = gone.to.as_deref().unwrap();
            if to.is_dir() { std::fs::remove_dir_all(to).unwrap() } else { std::fs::remove_file(to).unwrap() }
            let back: Vec<Trashed> = moved.iter().filter(|m| *m != gone).cloned().collect();
            prop_assert_eq!(put_back(&moved, root.path(), &trash, true, &Copies::default()), Ok(back.clone()));
            for item in &back {
                prop_assert_eq!(identity(&item.from), item.seen, "{} is back as it went", item.from.display());
            }
            prop_assert!(std::fs::symlink_metadata(&gone.from).is_err(), "what was emptied stays gone");
            prop_assert!(tree(&root.path().join("trash")).is_empty());
        }
    }

    proptest! {
        /// An item in the trash swapped for another at the same path, as when the Trash was
        /// emptied and something else of that name went there since: nothing goes back, and
        /// everything stays where it is.
        #[test]
        fn another_item_at_the_trash_path_is_never_put_back(t in trees(), swap in any::<prop::sample::Index>()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let carried = carry_out(&plan(&location, &files, t.single), &trash, |_| {});
            prop_assume!(!carried.moved.is_empty());
            let swapped = &carried.moved[swap.index(carried.moved.len())];
            let to = swapped.to.clone().unwrap();
            std::fs::rename(&to, root.path().join("emptied")).unwrap();
            if root.path().join("emptied").is_dir() {
                std::fs::create_dir(&to).unwrap();
            } else {
                let len = std::fs::metadata(root.path().join("emptied")).unwrap().len() as usize;
                std::fs::write(&to, vec![0xee; len]).unwrap();
            }
            let before = contents(root.path());
            prop_assert_eq!(can_put_back(&carried.moved, root.path(), &Copies::default()), Err(Blocked::NotInTrash(swapped.from.clone())));
            prop_assert!(put_back(&carried.moved, root.path(), &trash, false, &Copies::default()).is_err());
            prop_assert_eq!(contents(root.path()), before);
        }

        /// A folder on the way back replaced by a symbolic link to a folder elsewhere: nothing goes
        /// back, and nothing lands behind the link.
        #[test]
        fn nothing_goes_back_through_a_link(t in trees()) {
            let root = tempfile::tempdir().unwrap();
            let (location, files) = build(root.path(), &t);
            let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
            let carried = carry_out(&plan(&location, &files, t.single), &trash, |_| {});
            // Files went one by one, so their folder stayed: it becomes a link.
            prop_assume!(!t.single && location.is_dir() && !carried.moved.is_empty());
            let elsewhere = root.path().join("elsewhere");
            std::fs::create_dir(&elsewhere).unwrap();
            std::fs::rename(&location, root.path().join("was-pack")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &location).unwrap();
            prop_assert_eq!(can_put_back(&carried.moved, root.path(), &Copies::default()), Err(Blocked::NotAFolder(location.clone())));
            prop_assert!(put_back(&carried.moved, root.path(), &trash, false, &Copies::default()).is_err());
            prop_assert!(tree(&elsewhere).is_empty(), "nothing behind the link: {:?}", tree(&elsewhere));
        }
    }

    #[test]
    fn nothing_goes_back_over_what_is_there_or_from_an_emptied_trash() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("pack/a");
        write(&file);
        let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
        let moved = carry_out(&plan(&file, std::slice::from_ref(&file), true), &trash, |_| {}).moved;
        std::fs::write(&file, b"mine").unwrap();
        assert_eq!(can_put_back(&moved, root.path(), &Copies::default()), Err(Blocked::Taken(file.clone())));
        assert!(put_back(&moved, root.path(), &trash, false, &Copies::default()).is_err(), "and the move itself never replaces it");
        assert_eq!(std::fs::read(&file).unwrap(), b"mine");
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir_all(root.path().join("trash")).unwrap();
        assert_eq!(can_put_back(&moved, root.path(), &Copies::default()), Err(Blocked::NotInTrash(file)));
    }

    /// Something put at the path just before the move, after the check: the move itself never
    /// replaces it (`RENAME_EXCL`), and nothing goes back.
    #[test]
    fn a_file_put_there_between_the_check_and_the_move_stays() {
        struct Races(Trash);
        impl Bin for Races {
            fn take(&self, path: &Path) -> Result<Option<PathBuf>, KeptWhy> {
                self.0.take(path)
            }
            fn give_back(&self, from: &Path, to: &Path) -> Result<(), String> {
                std::fs::write(to, b"mine").unwrap();
                self.0.give_back(from, to)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("pack.iso");
        write(&file);
        let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
        let moved = carry_out(&plan(&file, std::slice::from_ref(&file), true), &trash, |_| {}).moved;
        let racing: Trash = Arc::new(Races(trash));
        assert!(put_back(&moved, root.path(), &racing, false, &Copies::default()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"mine", "what was put there stays");
        assert!(moved[0].to.as_ref().unwrap().exists(), "and the item stays in the trash");
    }

    /// A drive that went while its files go back stays gone: nothing makes the destination again.
    #[test]
    fn a_put_back_never_makes_the_destination() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("drive");
        let files = vec![destination.join("pack/a"), destination.join("pack/b")];
        files.iter().for_each(|f| write(f));
        write(&destination.join("pack/mine.txt"));
        let trash = into(root.path().join("trash"), Arc::default(), Arc::default());
        let moved = carry_out(&plan(&destination.join("pack"), &files, false), &trash, |_| {}).moved;
        std::fs::remove_dir_all(&destination).unwrap();
        assert!(put_back(&moved, &destination, &trash, false, &Copies::default()).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn a_failure_keeps_that_item_and_the_rest_still_go() {
        let root = tempfile::tempdir().unwrap();
        let pack = root.path().join("pack");
        let files = vec![pack.join("a"), pack.join("sub/b")];
        files.iter().for_each(|f| write(f));
        write(&pack.join("mine.txt"));
        let refusing = Arc::new(std::sync::Mutex::new(vec![files[0].clone()]));
        let kept = carry_out(&plan(&pack, &files, false), &into(root.path().join("trash"), refusing, Arc::default()), |_| {}).kept;
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
        let mut moved = Remove(system().take(&file).unwrap());
        assert!(!file.exists(), "gone from where it was");
        let to = moved.0.clone().expect("the Trash says where it went");
        assert!(to.exists(), "and it's in the Trash");
        system().give_back(&to, &file).unwrap();
        assert!(file.exists() && !to.exists(), "put back where it was");
        moved.0 = Some(file);
    }
}
