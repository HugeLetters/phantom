//! The file work of a move (#24, `lifecycle::moving` decides its stages): blocking, on a task of
//! its own, never on the actor.
//!
//! [`plan`] says what moves: each of the torrent's files, from its path in the old destination to
//! its path in the new one (under a new name, its own folder's or its one file's, for a Rename:
//! #331, which is a move within its destination), as it finds each there (its volume and inode: what tells it from
//! another at the same path), refused when something is at one of those (a clash, unless the
//! person chose what to do with it, and it's what they were shown: #270; never another torrent's
//! file, [`anothers`]) or a folder on the way there is a symbolic link or a file.
//! Folders are told apart by what they are on disk, not by how they're spelled ([`same_folder`],
//! [`inside`]). [`stage`] moves them into the move's own folder in the new destination
//! (`.phantom-moving-<hash>`), each renamed (on the same drive) or copied in chunks, a chunk of
//! zeros left a hole, with what's copied told after each chunk and a stop heard between them; one
//! gone from the old destination meanwhile stays out of the move. Then the actor writes ahead the
//! new destination and what's staged of each file, and [`land`] puts each in its place, checked
//! again just before it takes it (nothing there that wasn't when the plan was made, no link on the
//! way). Once all have landed the actor writes that ahead too (the record's `landed`: from then on
//! it never goes back), and only then does [`remove_old`] remove what's left in the old one. One
//! blocked from its place is put back into staging with the rest ([`unland`], as when the landing
//! can't be written ahead), and the move restored: a move always goes back when something it didn't
//! expect is at a new path. [`restore`] renames back what was renamed and removes what was copied, while each
//! original is still there as it was; one whose original is gone (deleted by hand) stays gone, its
//! copy sent to the Trash, as one renamed that can't take its old path again. Either way the files
//! are in exactly one destination once it's done. Nothing is reached through a symbolic link, as in
//! the Trash's code, and nothing is erased that isn't the torrent's own copy: what Replace and Use
//! existing get rid of goes to the Trash.
//!
//! A torrent moved or renamed by hand is [`found_in`] the folder or under the name it has now
//! (#358): its plan keeps every file of it there, as Use existing keeps one, so nothing is staged,
//! landed or removed, and nothing is made in either place; [`pointing`] is the plan that moves
//! nothing back, a found Rename's undo.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use proto::{AbsolutePath, Answered, Clash, Found, NotMoved, Unavailable};
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
}

/// What tells one file from another at the same path: its volume and inode, which a rename keeps.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ino {
    pub dev: u64,
    pub ino: u64,
}

impl Ino {
    pub fn of(m: &std::fs::Metadata) -> Self {
        Ino { dev: m.dev(), ino: m.ino() }
    }
}

/// A file at a new path as the plan found it: which file it is, its length and when it was last
/// written, so one written in place since, or swapped for another, isn't taken for it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub ino: Ino,
    pub len: u64,
    pub secs: i64,
    pub nanos: i64,
}

impl Seen {
    fn of(m: &std::fs::Metadata) -> Self {
        Seen { ino: Ino::of(m), len: m.len(), secs: m.mtime(), nanos: m.mtime_nsec() }
    }

    /// Whether `m` is a file, this one as it was.
    fn is(&self, m: &std::fs::Metadata) -> bool {
        m.file_type().is_file() && Seen::of(m) == *self
    }
}

/// One of the torrent's files.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub index: usize,
    /// Inside the destination: the torrent's own folder, then the file's path in it.
    pub path: PathBuf,
    /// Inside the new destination, when the move gives the torrent a new name there (#331): else
    /// `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed: Option<PathBuf>,
    /// Something was at its path in the new destination, which the torrent takes as its file (Use
    /// existing): its own copy in the old destination goes to the Trash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
    /// Its file in the old destination as the plan found it, and its length: what moves. None: it
    /// wasn't there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<(Ino, u64)>,
    /// What was at its path in the new destination as the plan found it: Replace sends that to the
    /// Trash, and Use existing takes it, as it was then, and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<Seen>,
    /// What's staged of it, written ahead once all are: what lands at its new path. A staged file
    /// that is its source was renamed; any other was copied. None: nothing of it moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<Ino>,
}

/// Why a move can't be planned.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    /// Something is at these paths its files would take, and nothing says what to do with them.
    Clash(Vec<Found>),
    /// What's at this path isn't what the clash was answered for: it wasn't there then, or it's
    /// another file, or it was written since.
    Changed(PathBuf),
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

/// How [`restore`] went: every file is back where it was, but what the person deleted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Restored {
    /// What can't be done, told.
    pub warnings: Vec<String>,
    /// Something not the torrent's own file is at one of its paths in the old destination (the
    /// person put it there meanwhile): its pieces there aren't the ones librqbit had.
    pub replaced: bool,
}

impl Plan {
    fn old(&self, item: &Item) -> PathBuf {
        self.from.join(&item.path)
    }

    fn new(&self, item: &Item) -> PathBuf {
        self.to.join(item.at_new())
    }

    fn staged(&self, item: &Item) -> PathBuf {
        self.staging.join(item.at_new())
    }

    /// Every path its files are at while it moves: in the old destination, in staging and in the new
    /// one (under its new name, if it's given one).
    pub fn paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.items.iter().flat_map(|i| [self.old(i), self.staged(i), self.new(i)])
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

    /// The plan of a torrent [`found_in`] where it is now (#358) moves nothing and removes nothing:
    /// none of its files was at its old paths as it was found, and one put back there since (in the
    /// instant before the plan looked) stays where it is, the person's.
    pub fn moves_nothing(&mut self) {
        for item in &mut self.items {
            item.source = None;
        }
    }

    /// Records what [`stage`] staged of each file.
    pub fn staged_as(&mut self, staged: &[(usize, Ino)]) {
        for item in &mut self.items {
            item.staged = staged.iter().find(|(i, _)| *i == item.index).map(|(_, ino)| *ino);
        }
    }
}

impl Item {
    /// Its path inside the new destination.
    fn at_new(&self) -> &Path {
        self.renamed.as_deref().unwrap_or(&self.path)
    }
}

/// Where a file at `path` inside its destination goes under `name` (none: where it is).
pub fn new_path(path: &Path, name: Option<&str>) -> PathBuf {
    name.map_or_else(|| path.to_owned(), |name| named(path, name))
}

/// `path` inside a destination with its first part, the torrent's own folder or its one file,
/// named `name` instead.
fn named(path: &Path, name: &str) -> PathBuf {
    std::iter::once(std::ffi::OsStr::new(name)).chain(path.components().skip(1).map(|c| c.as_os_str())).collect()
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

/// `name` in Unicode's composed form (NFC), as Foundation has it: a drive that doesn't tell the forms
/// apart (APFS) keeps one name for `é` as one character and as `e` and an accent.
pub fn normalized(name: &str) -> String {
    use objc2_foundation::NSString;
    NSString::from_str(name).precomposedStringWithCanonicalMapping().to_string()
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

/// The files at `files`' paths in `to` (each with its path inside the destination), as a clash
/// tells them.
pub fn found_at(to: &Path, files: &[(usize, PathBuf)]) -> Vec<Found> {
    let at = files.iter().map(|(_, path)| to.join(path));
    at.filter_map(|new| meta(&new).filter(|m| m.file_type().is_file()).and_then(|m| found(&new, &m))).collect()
}

/// What's at `path`, a file, as a clash tells it: what tells it from another put there since.
fn found(path: &Path, m: &std::fs::Metadata) -> Option<Found> {
    let path = AbsolutePath::new(path).ok()?;
    Some(Found { path, dev: m.dev(), ino: m.ino(), len: m.len(), secs: m.mtime(), nanos: m.mtime_nsec() })
}

/// The move of torrent `hash`'s `files` (each by index, with its path inside its destination) from
/// the destination `from` into `to`, under `name` there if it's given one (its own folder's, or its
/// one file's: a Rename, #331, moves it into its own destination so), `clash` saying what to do
/// with what's at their paths there: only with the files it was answered for, as they were then
/// (#270). Anything else there, or one of them changed, refuses it, whatever the choice: nothing
/// goes to the Trash, or is taken as the torrent's, that the person wasn't asked about.
pub fn plan(
    from: &Path,
    to: &Path,
    name: Option<&str>,
    hash: &str,
    files: &[(usize, PathBuf)],
    clash: Option<&Answered>,
    renames: bool,
) -> Result<Plan, Refused> {
    let source = |path: &Path| reached(from, path).map(|m| (Ino::of(&m), m.len()));
    let new_path = |path: &Path| new_path(path, name);
    let mut taken = Vec::new();
    for (_, path) in files {
        let path = &new_path(path);
        let new = to.join(path);
        let folders = path.ancestors().skip(1).filter(|a| !a.as_os_str().is_empty());
        for folder in folders.map(|a| to.join(a)) {
            if meta(&folder).is_some_and(|m| !m.file_type().is_dir()) {
                return Err(Refused::NotAFolder(folder));
            }
        }
        match meta(&new) {
            None => {}
            Some(m) if m.file_type().is_file() => taken.extend(found(&new, &m)),
            Some(_) => return Err(Refused::NotAFolder(new)),
        }
    }
    let choice = match clash {
        None if taken.is_empty() => None,
        None => return Err(Refused::Clash(taken)),
        Some(answered) => {
            if let Some(changed) = taken.iter().find(|f| !answered.found.contains(f)) {
                return Err(Refused::Changed(changed.path.to_path_buf()));
            }
            Some(answered.choice)
        }
    };
    let items = files.iter().map(|(index, path)| {
        let renamed = Some(new_path(path)).filter(|new| new != path);
        let new = to.join(renamed.as_deref().unwrap_or(path));
        let at_new = taken.iter().find(|f| *f.path == new).map(|f| Seen {
            ino: Ino { dev: f.dev, ino: f.ino },
            len: f.len,
            secs: f.secs,
            nanos: f.nanos,
        });
        Item {
            index: *index,
            path: path.clone(),
            renamed,
            kept: choice == Some(Clash::UseExisting) && at_new.is_some(),
            source: source(path),
            replaces: at_new,
            staged: None,
        }
    });
    Ok(Plan {
        from: from.to_owned(),
        to: to.to_owned(),
        staging: to.join(format!(".phantom-moving-{hash}")),
        renames,
        items: items.collect(),
    })
}

/// Whether the torrent whose `files` (each by index, with its path inside its destination and its
/// full length) is found in `to` under `name` (none: its own), its files moved or renamed there by
/// hand (#358): nothing at any of their paths in `from`, its own folder (or its one file) in `to`,
/// each of its files there a real file reached through real folders, no longer than its own (a
/// check sets each file's length, which would cut a longer one short), and one at least. What's
/// there, as a clash finds it: the files a Move takes as they are, as Use existing does.
pub fn found_in(from: &Path, to: &Path, name: Option<&str>, files: &[(usize, PathBuf, u64)]) -> Option<Vec<Found>> {
    if files.is_empty() || files.iter().any(|(_, path, _)| meta(&from.join(path)).is_some()) {
        return None;
    }
    let news: Vec<(PathBuf, u64)> = files.iter().map(|(_, path, len)| (new_path(path, name), *len)).collect();
    let top = meta(&to.join(news[0].0.components().next()?))?;
    let own_folder = news.iter().any(|(path, _)| path.components().count() > 1);
    if !(if own_folder { top.file_type().is_dir() } else { top.file_type().is_file() }) {
        return None;
    }
    let mut there = Vec::new();
    for (path, len) in &news {
        let new = to.join(path);
        match (meta(&new), reached(to, path)) {
            (None, _) => {}
            (Some(_), Some(m)) if m.len() <= *len => there.extend(found(&new, &m)),
            (Some(_), _) => return None,
        }
    }
    (!there.is_empty()).then_some(there)
}

/// A move of the torrent `hash`'s `files` from `from` into `to`, under `name` there, that moves
/// nothing (#358): the undo of a Rename that found its torrent points it back at the name it had,
/// its files staying where the person put them.
pub fn pointing(from: &Path, to: &Path, name: Option<&str>, hash: &str, files: &[(usize, PathBuf)]) -> Plan {
    let items = files.iter().map(|(index, path)| Item {
        index: *index,
        path: path.clone(),
        renamed: Some(new_path(path, name)).filter(|new| new != path),
        kept: false,
        source: None,
        replaces: None,
        staged: None,
    });
    Plan {
        from: from.to_owned(),
        to: to.to_owned(),
        staging: to.join(format!(".phantom-moving-{hash}")),
        renames: true,
        items: items.collect(),
    }
}

/// Where the folder `path` is: its inode if it's on disk, else its nearest ancestor on disk's and the
/// rest folded as `name_free` folds a name (Unicode composed, lower case), so the same place in
/// another spelling is the same.
fn folder_place(path: &Path) -> (Option<Ino>, String) {
    let folded = |p: &Path| normalized(&p.to_string_lossy().to_lowercase());
    match path.ancestors().find_map(|a| folder(a).map(|i| (a, i))) {
        Some((near, ino)) => (Some(ino), folded(path.strip_prefix(near).unwrap_or(path))),
        None => (None, folded(path)),
    }
}

/// The first of `files`' paths in `to` (each with its path inside the destination) that is one of
/// `others`' (other torrents' files, wherever they are, each with whose it is, which it's returned
/// with), or whose file is one of theirs: the same
/// file on disk however it's reached (a hard link of it, another case), and the same path however
/// its folders are reached (a link, another case), though nothing is at it now (its files deleted
/// by hand, turned back on they'd be made there). A link at their path isn't followed (#306). A
/// torrent's Move never replaces, nor takes as its own, a file another torrent has (#270): the
/// clash's answer can't tell, as one taken over with Use existing stays as it was.
pub fn anothers<T: std::marker::Copy>(
    to: &Path,
    files: &[(usize, PathBuf)],
    others: impl IntoIterator<Item = (T, PathBuf)>,
) -> Option<(PathBuf, T)> {
    let tagged: Vec<(T, PathBuf)> = others.into_iter().collect();
    let others: Vec<PathBuf> = tagged.iter().map(|(_, p)| p.clone()).collect();
    let mut parents: BTreeMap<PathBuf, (Option<Ino>, String)> = BTreeMap::new();
    let mut spelled = |path: &Path| {
        let parent = path.parent().unwrap_or(path).to_owned();
        let folder = parents.entry(parent.clone()).or_insert_with(|| folder_place(&parent)).clone();
        (folder, path.file_name().map(|n| normalized(&n.to_string_lossy().to_lowercase())))
    };
    let theirs: Vec<_> = others.iter().map(|p| spelled(p)).collect();
    let news: Vec<PathBuf> = files.iter().map(|(_, path)| to.join(path)).collect();
    if let Some((new, at)) = news.iter().find_map(|new| {
        let new_spelled = spelled(new);
        theirs.iter().position(|t| *t == new_spelled).map(|at| (new, at))
    }) {
        return Some((new.clone(), tagged[at].0));
    }
    let here: Vec<(&PathBuf, Ino)> =
        news.iter().filter_map(|new| meta(new).filter(|m| m.file_type().is_file()).map(|m| (new, Ino::of(&m)))).collect();
    if here.is_empty() {
        return None;
    }
    let inos: Vec<(T, Ino)> = tagged.iter().filter_map(|(t, p)| ino(p).map(|i| (*t, i))).collect();
    here.into_iter().find_map(|(new, found)| inos.iter().find(|(_, i)| *i == found).map(|(t, _)| (new.clone(), *t)))
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
fn make_folders(root: &Path, inside: &Path, made: &mut Vec<PathBuf>) -> Result<(), NotMoved> {
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
    let moving: Vec<(&Item, Ino, u64)> =
        plan.items.iter().filter(|i| !i.kept).filter_map(|i| i.source.map(|(ino, len)| (i, ino, len))).collect();
    // Nothing to move (a torrent found where it is now, #358): nothing is made there either.
    if moving.is_empty() {
        return Ok(Vec::new());
    }
    match meta(&plan.staging) {
        Some(m) if m.file_type().is_dir() => {}
        Some(_) => return Err(Stop::Failed(at_path(&plan.staging, |path| NotMoved::NotAFolder { path }))),
        None => std::fs::create_dir(&plan.staging).map_err(|e| fail(&e, &plan.to))?,
    }
    let total = if plan.renames { 0 } else { moving.iter().map(|(_, _, len)| len).sum() };
    let mut done = 0;
    let mut staged = Vec::new();
    for &(item, source, _) in &moving {
        if stop.load(Ordering::SeqCst) {
            return Err(Stop::Stopped);
        }
        let (old, at) = (plan.old(item), plan.staged(item));
        if reached(&plan.from, &item.path).is_none_or(|m| Ino::of(&m) != source) {
            continue;
        }
        make_folders(&plan.staging, item.at_new(), &mut Vec::new()).map_err(Stop::Failed)?;
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

/// What [`land`] did that only it can take back, should its landing not be written ahead: the
/// files it put in place (by index), what Replace sent to the Trash for them, and the folders it
/// made. [`unland`] takes it back.
#[derive(Debug, Default)]
pub struct Landed {
    items: Vec<usize>,
    trashed: Vec<(PathBuf, Option<PathBuf>)>,
    made: Vec<PathBuf>,
}

/// Puts each staged file in its place in the new destination, each checked just before it moves:
/// the folders on its way real ones (made where they aren't there), and nothing at its path but
/// what the plan found there, as it was, and the person chose to Replace, which goes to the Trash
/// first. It takes its place only if that's free (`RENAME_EXCL`), so nothing put there meanwhile
/// is ever replaced. One already there (a start landing a move a crash cut short) has landed. Then
/// each kept one (Use existing) must still be the file the plan found, as it was. Once every one
/// has landed, their folders are flushed to the drive. Nothing in the old destination is touched: the
/// actor writes ahead that they landed (the record's `landed`), and only then does [`remove_old`]
/// run, so a move never goes back once an original is gone. One that can't take its place, or
/// folders that can't be flushed, stop it: those that had go back into staging ([`unland`]), and
/// it's why, the old destination as it was. `between` (a test's gate) after each: false stops it
/// there, as a crash.
pub fn land(plan: &Plan, trash: &Trash, between: &dyn Fn(&Path) -> bool) -> Result<Landed, NotMoved> {
    let mut landed = Landed::default();
    let mut blocked = make_top_folder(plan, &mut landed.made).err();
    for item in plan.items.iter().filter(|i| !i.kept && blocked.is_none()) {
        let Some(staged) = item.staged else { continue };
        match place(plan, item, staged, trash, &mut landed.made, &mut landed.trashed) {
            Ok(()) => landed.items.push(item.index),
            Err(why) => {
                blocked = Some(why);
                break;
            }
        }
        if !between(&plan.new(item)) {
            return Err(NotMoved::Interrupted);
        }
    }
    // What Use existing takes is the file the plan found, as it was: one swapped or written since
    // (the person's, as the others copied) isn't taken, and the move goes back. One gone since
    // leaves its path free for the torrent's own copy ([`remove_old`]).
    if blocked.is_none() {
        blocked = plan.items.iter().filter(|i| i.kept).find_map(|item| {
            let new = plan.new(item);
            let changed = meta(&new).is_some_and(|m| !item.replaces.is_some_and(|s| s.is(&m)));
            changed.then(|| at_path(&new, |path| NotMoved::Taken { path }))
        });
    }
    if blocked.is_none() {
        let moved = plan.items.iter().filter(|i| !i.kept && i.staged.is_some());
        let mut parents: BTreeSet<PathBuf> = moved.filter_map(|i| plan.new(i).parent().map(Path::to_owned)).collect();
        // None moved, none staged: no staging folder was made (#358).
        if !parents.is_empty() {
            parents.insert(plan.staging.clone());
        }
        blocked = parents.iter().find_map(|dir| sync_dir(dir).err().map(|e| not_moved(&e, &plan.to, dir)));
    }
    let Some(why) = blocked else { return Ok(landed) };
    unland(plan, trash, &landed);
    Err(why)
}

/// A move under a new name (#331) makes the torrent's new top folder itself, unless the person chose
/// what to do with files already at their paths (a clash): one a person made there since the plan
/// found nothing at it is never merged into (`Taken`). One already there is the
/// run's own only if it's empty (a crash cut short just after it was made) or holds a file this plan
/// already landed (a start landing a move a crash cut short).
fn make_top_folder(plan: &Plan, made: &mut Vec<PathBuf>) -> Result<(), NotMoved> {
    let renamed = |i: &&Item| !i.kept && i.staged.is_some() && i.renamed.is_some();
    if plan.items.iter().any(|i| i.kept || i.replaces.is_some()) {
        return Ok(());
    }
    let Some(top) =
        plan.items.iter().find(renamed).and_then(|i| i.at_new().components().next().filter(|_| i.at_new().components().count() > 1))
    else {
        return Ok(());
    };
    let top = plan.to.join(top);
    match std::fs::create_dir(&top) {
        Ok(()) => {
            made.push(top);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let ours = meta(&top).is_some_and(|m| m.file_type().is_dir())
                && (plan.items.iter().filter(renamed).any(|i| i.staged.is_some() && ino(&plan.new(i)) == i.staged)
                    || std::fs::read_dir(&top).is_ok_and(|mut d| d.next().is_none()));
            match ours {
                true => Ok(()),
                false => Err(at_path(&top, |path| NotMoved::Taken { path })),
            }
        }
        Err(e) => Err(not_moved(&e, &plan.to, &top)),
    }
}

/// Takes back what [`land`] did (`landed`): the files it put in place go back into staging, last
/// first, and what Replace sent to the Trash for them comes back (but what a run a crash cut short
/// sent there, which stays in the Trash); then the folders it made go. [`restore`] then puts the
/// move back.
pub fn unland(plan: &Plan, trash: &Trash, landed: &Landed) {
    for item in plan.items.iter().rev().filter(|i| landed.items.contains(&i.index)) {
        let (at, new) = (plan.staged(item), plan.new(item));
        if ino(&new) != item.staged || trash::rename_free(&new, &at).is_err() {
            tracing::warn!(path = %new.display(), "a move that couldn't finish left a file in its new destination");
        }
    }
    for (path, to) in landed.trashed.iter().rev() {
        if let Some(to) = to
            && trash.give_back(to, path).is_err()
        {
            tracing::warn!(path = %path.display(), to = %to.display(), "a move that couldn't finish left a file in the Trash");
        }
    }
    // What a run a crash cut short sent to the Trash for Replace stays there: that run logged
    // where it went.
    for item in plan.items.iter().filter(|i| !i.kept && i.replaces.is_some()) {
        let new = plan.new(item);
        if !landed.trashed.iter().any(|(p, _)| *p == new) && ino(&new) != item.replaces.map(|s| s.ino) && ino(&new) != item.staged {
            tracing::warn!(
                path = %new.display(),
                "a move that couldn't finish: the file Replace chose here went to the Trash in a run a crash cut short, and stays there (where: that run's log line)"
            );
        }
    }
    for dir in landed.made.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Once every file has landed ([`land`]), and that's written ahead: what's left of the torrent's
/// files in the old destination goes (a copy's original, and the torrent's own copy of one kept,
/// Use existing, which goes to the Trash), then the folders that leaves empty there, and the
/// staging folder. Run again after a crash, it goes on from where it was, whatever is at the new
/// paths by then: it never goes back. What can't be done is told, as warnings. Use existing whose
/// file at the path is gone by then: the torrent's own copy takes the free path.
pub fn remove_old(plan: &Plan, trash: &Trash) -> Vec<String> {
    let mut warnings = Vec::new();
    for item in &plan.items {
        let old = plan.old(item);
        let Some(m) = reached(&plan.from, &item.path) else { continue };
        let new = plan.new(item);
        // The same file: the old destination is the new one (never so, but never removed).
        if ino(&new) == Some(Ino::of(&m)) {
            continue;
        }
        let ours = item.source.is_some_and(|(source, _)| source == Ino::of(&m));
        if item.kept && ours {
            match meta(&new) {
                // The person's is gone by now: the torrent's own copy takes the free path.
                None => {
                    let placed = make_folders(&plan.to, item.at_new(), &mut Vec::new())
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
        // Every staged file landed (one gone from its new path since was the person's doing).
        } else if ours
            && item.staged.is_some()
            && let Err(e) = std::fs::remove_file(&old)
        {
            warnings.push(format!("{}: {e}", old.display()));
        }
    }
    tidy_old(plan);
    remove_empty(&plan.staging);
    warnings
}

/// Puts `item`'s staged file (`staged`) at its new path, checked just before: see [`land`].
fn place(
    plan: &Plan,
    item: &Item,
    staged: Ino,
    trash: &Trash,
    made: &mut Vec<PathBuf>,
    trashed: &mut Vec<(PathBuf, Option<PathBuf>)>,
) -> Result<(), NotMoved> {
    let (at, new) = (plan.staged(item), plan.new(item));
    // Landed already: a start landing a move a crash cut short. Or Replace onto a hard link of
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
    make_folders(&plan.to, item.at_new(), made)?;
    match meta(&new) {
        None => {}
        Some(m) if item.replaces.is_some_and(|s| s.is(&m)) => match trash.take(&new) {
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
fn rename_excl(from: &Path, to: &Path) -> std::io::Result<()> {
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

/// Once a move ends moved ([`remove_old`]): each file that isn't at its new path as the move's is out of
/// it, so it's [missing](Plan::missing): one kept (Use existing) with no file there by now, whose
/// own copy couldn't take the free path, and one whose staged file is gone from there (the person's
/// doing, after a crash once every file had landed).
pub fn left_out(plan: &mut Plan) {
    for i in 0..plan.items.len() {
        let item = &plan.items[i];
        let new = plan.new(item);
        let landed = match item.kept {
            true => meta(&new).is_some_and(|m| m.file_type().is_file()),
            false => item.staged.is_some_and(|staged| ino(&new) == Some(staged)),
        };
        if !landed {
            (plan.items[i].staged, plan.items[i].kept) = (None, false);
        }
    }
}

/// Puts back what [`stage`] did: each staged file that is its source (renamed) back to its path in
/// the old destination, the folders on its way made again where they're gone (never through a
/// link), never over anything there; each other (copied) removed, while its original is there as
/// the plan found it. One whose original is gone (deleted by hand meanwhile) stays gone: its copy
/// goes to the Trash, the Trash's own way, and so does one renamed whose old path can't be had
/// (something of the person's there now, or where a folder goes). Then the staging folder goes. A
/// start after a crash runs it again: what's back already is.
pub fn restore(plan: &Plan, trash: &Trash) -> Restored {
    let mut restored = Restored::default();
    for item in plan.items.iter().filter(|i| !i.kept) {
        let (Some((source, len)), Some(at)) = (item.source, meta(&plan.staged(item)).filter(|m| m.file_type().is_file())) else {
            continue;
        };
        let (staged, old) = (plan.staged(item), plan.old(item));
        let undone = match Ino::of(&at) == source {
            true => match meta(&old) {
                None => make_folders(&plan.from, &item.path, &mut Vec::new())
                    .map_err(|why| format!("{why:?}"))
                    .and_then(|()| rename_excl(&staged, &old).map_err(|e| e.to_string())),
                Some(_) => Err("something else is at its path".into()),
            },
            false => match reached(&plan.from, &item.path).is_some_and(|m| Ino::of(&m) == source && m.len() == len) {
                true => std::fs::remove_file(&staged).map_err(|e| e.to_string()),
                false => Err("its original is gone".into()),
            },
        };
        if let Err(why) = undone {
            match trash.take(&staged) {
                Ok(to) => {
                    tracing::info!(path = %old.display(), to = ?to, why, "a move going back sent the torrent's copy of a file to the Trash")
                }
                Err(e) => restored.warnings.push(format!("{}: {why}, and it couldn't go to the Trash: {e:?}", staged.display())),
            }
        }
    }
    restored.replaced = plan
        .items
        .iter()
        .filter(|i| !i.kept)
        .any(|i| i.source.is_some_and(|(source, _)| meta(&plan.old(i)).is_some_and(|m| Ino::of(&m) != source)));
    remove_empty(&plan.staging);
    restored
}

/// The folders of the torrent's own folder in the old destination left with nothing but litter go:
/// of the files that moved, so one that moves nothing (#358) leaves the old destination as it is.
fn tidy_old(plan: &Plan) {
    let moved = plan.items.iter().filter(|i| i.source.is_some());
    let own: BTreeSet<PathBuf> = moved.filter_map(|i| i.path.components().next()).map(|c| plan.from.join(c)).collect();
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
