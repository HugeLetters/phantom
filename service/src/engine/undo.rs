//! Undo ([ADR 0008]): what a Remove or a Delete changed, kept as what it takes to reverse it, an
//! [`Entry`], on a stack of the last [`machine::DEPTH`] ([`machine`]). An undo is a command of its
//! own: it puts back from the Trash what Delete moved there ([`super::trash::put_back`]), then adds
//! the torrent again as it was, from its [`Stash`]: the .torrent and librqbit's `.bitv` (the pieces
//! it had), so it comes back with its progress and without a full check. librqbit deletes both
//! from its session folder when a torrent goes, so they're hard-linked aside first, while the
//! torrent is still whole: a link shares the file, so librqbit's last save, as it drops the
//! torrent, lands in the stash too.
//!
//! Every step is on the stack from the moment its command comes (#190), running ([`Running`]) until
//! it ends with what reverses it, an [`Entry`].
//!
//! Kept in the data dir, next to the store: `undo.json`, the stack and the undos under way (written
//! before they start, so one cut short by a crash is finished at the next start), and `undo/`, a
//! folder per stash. A stash goes once neither the stack nor a torrent's record names it.
//!
//! [ADR 0008]: ../../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

pub mod machine;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lifecycle::undo::Part;
use proto::{CantUndo, Named, Priority, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::machine::letgo;
use super::store::{Record, Saved};
use super::trash::{Identity, Trashed, identity};

const FILE: &str = "undo.json";
const STASHES: &str = "undo";
/// The shadows on their way to the Trash ([`Leaving`]).
const LEAVING: &str = "shadows-leaving.json";
/// What librqbit wrote into the files Moves to Trash took, while there (`trashed.rs`).
const PARTS: &str = "undo-parts";

/// Where what librqbit writes into a file a Move to Trash took is kept: by info hash, file index
/// and offset.
pub fn parts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PARTS)
}

/// What it takes to reverse one Remove, Delete or Move to Trash.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    pub action: UndoAction,
    /// The torrent's name, or its info hash.
    pub name: String,
    pub info_hash: String,
    /// The id it had, which it gets again.
    pub id: TorrentId,
    pub back: Back,
    /// What Delete moved to the Trash, which goes back first: none for a Remove.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub put_back: Vec<Trashed>,
    /// How its torrent was listed when the command came: what clients see of it while it's undone,
    /// until it's back (#190).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listed: Option<Seen>,
}

/// A torrent as clients saw it listed when a step's command came. Its lifecycle meanwhile isn't
/// kept: it's what the step's undo brings back, rebuilt from the undo's own record
/// (`Actor::coming_back`), so it's the same after a restart.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Seen {
    pub info: TorrentInfo,
}

/// A step running (#190): its command came and it hasn't ended.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Running {
    /// A Move to Trash's count is of the files asked about, until it ends.
    pub action: UndoAction,
    pub name: String,
    pub info_hash: String,
    pub id: TorrentId,
    /// How its torrent was listed when the command came, as [`Entry::listed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listed: Option<Seen>,
    /// A Move to Trash's, once it starts: each file asked about with the priority it had, and each
    /// item moved so far, written ahead as it goes, so a crash still knows what to put back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous: Vec<(usize, Priority, PathBuf)>,
    /// A Move to Trash's: what librqbit had of each file asked about, in bytes, as it started.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub had: Vec<(usize, u64)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<Trashed>,
    /// A Move to Trash's files, by index, as asked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<usize>,
}

impl Running {
    pub fn undoable(&self, seq: u64) -> Undoable {
        Undoable { seq, action: self.action, torrents: vec![Named { id: self.id, name: self.name.clone() }] }
    }
}

/// What clients see of the undo stack, oldest first: each action, running or done.
pub fn stacked(state: &machine::State) -> Vec<Undoable> {
    state.stack.iter().filter_map(|p| undoable(state, p.seq)).collect()
}

/// The undos under way: what each part puts back.
#[cfg(test)]
pub fn undoing(state: &machine::State) -> impl Iterator<Item = &Entry> {
    state.steps.values().flat_map(|s| match &s.what {
        machine::What::Undo { parts, .. } => parts.values().filter_map(Part::doing).collect(),
        machine::What::Action(_) | machine::What::Command(_) => Vec::new(),
    })
}

/// What clients see of action `seq` on the stack, running or done: its torrents, but those whose
/// part changed nothing.
pub fn undoable(state: &machine::State, seq: u64) -> Option<Undoable> {
    let done = state.stack.iter().find(|p| p.seq == seq).and_then(|p| p.done.as_ref());
    let parts: Vec<Undoable> = match (done, state.steps.get(&seq).map(|s| &s.what)) {
        (Some(done), _) => done.values().map(|e| e.undoable(seq)).collect(),
        (None, Some(machine::What::Action(parts))) => parts
            .values()
            .filter_map(|p| match p {
                Part::Doing(r) => Some(r.undoable(seq)),
                Part::Over(Some(e)) => Some(e.undoable(seq)),
                Part::Over(None) => None,
            })
            .collect(),
        (None, Some(machine::What::Undo { .. } | machine::What::Command(_)) | None) => Vec::new(),
    };
    merged(parts)
}

/// The parts of one entry as one: a Delete if any part is one (a Delete of a torrent without info
/// is a Remove), its torrents in order.
fn merged(parts: Vec<Undoable>) -> Option<Undoable> {
    let mut parts = parts.into_iter();
    let mut one = parts.next()?;
    for part in parts {
        if part.action == UndoAction::Delete {
            one.action = UndoAction::Delete;
        }
        one.torrents.extend(part.torrents);
    }
    Some(one)
}

/// How the torrent comes back.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Back {
    /// It left the list: it's added again with this record (its destination, what's wanted, its
    /// seeding round), from `source`.
    Gone { saved: Saved, source: Source },
    /// It's still in the list, stopped, its files partly in the Trash (a partial Delete): it's
    /// taken out and added again from its stash, with its record as it is then: a Pause since
    /// its Delete holds.
    InList { stash: Stash },
    /// Only files were moved (#151): the torrent never left the list. Undoing puts back from the
    /// Trash those of them still there, and each file asked about (every file the trash was asked
    /// for, by index and path, including those that weren't on disk) back to its priority
    /// `previous`, except a file that was moved and didn't come back, which stays Off. `had`: what
    /// librqbit had of each, in bytes, when it moved (none: a check was under way, so nothing was
    /// known); a file back that it has more of now, and what came couldn't go into, makes the undo
    /// check the torrent (`trashed::needs_check`). One an older build kept as `u64::MAX` never does.
    Files {
        previous: Vec<(usize, Priority, PathBuf)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        had: Vec<(usize, u64)>,
    },
}

/// What a torrent is added again from.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Stash(Stash),
    /// One without info has nothing in librqbit to keep: its magnet link, made from its info hash,
    /// name and trackers.
    Magnet(String),
}

/// A torrent kept aside: a folder in `undo/` with its .torrent and the pieces it had. Where its
/// files go and which it gets are its record's (#45).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stash {
    pub dir: PathBuf,
    /// Before #45: the files librqbit got (those Normal; `None`, every file), for a record that
    /// doesn't say. Read once.
    #[serde(default, skip_serializing)]
    pub only_files: Option<Vec<usize>>,
    /// Each of its files as it was when kept ([`super::trash::identity`]; `None`: not there). The
    /// pieces it had hold only while every one is still so.
    #[serde(default)]
    pub files: Vec<(PathBuf, Option<Identity>)>,
}

impl Entry {
    pub fn undoable(&self, seq: u64) -> Undoable {
        Undoable { seq, action: self.action, torrents: vec![Named { id: self.id, name: self.name.clone() }] }
    }

    /// A Delete takes the place of a Delete of the same torrent (by id: one added again since is
    /// another) before it: its partial Delete, whose items its own hold too, or itself, ended
    /// again at a start after a crash.
    pub fn absorbs(&self, older: &Entry) -> bool {
        self.action == UndoAction::Delete && older.action == UndoAction::Delete && self.id == older.id
    }

    /// The stashes it names.
    fn stashes(&self) -> impl Iterator<Item = &Path> {
        let (named, saved) = match &self.back {
            Back::Gone { saved, source } => (
                match source {
                    Source::Stash(s) => Some(s),
                    Source::Magnet(_) => None,
                },
                saved.stash.as_ref(),
            ),
            Back::InList { stash } => (Some(stash), None),
            Back::Files { .. } => (None, None),
        };
        named.into_iter().chain(saved).map(|s| s.dir.as_path())
    }
}

/// Whether the files a Move to Trash moved can be put back into a torrent that's `torrent` now
/// (its lifecycle; `None`: not in the list): the torrent must be one moving files to the Trash would do
/// something to now, and its destination reachable. Nothing is found by looking at the disk here;
/// [`super::trash::can_put_back`] does that.
pub fn files_back(torrent: Option<&lifecycle::State>) -> Result<(), CantUndo> {
    use proto::{Action, Answer, TorrentRefusal as R};
    let Some(state) = torrent else { return Err(CantUndo::NotInList) };
    match lifecycle::answer(state, Action::TrashFiles) {
        Answer::Does | Answer::DoesNothing { .. } => Ok(()),
        Answer::Refused { why } => Err(match why {
            R::DestinationUnavailable { reason, .. } => CantUndo::Destination { reason },
            R::Deleting | R::PartlyDeleted => CantUndo::Deleting,
            R::NotInList
            | R::NoInfoYet
            | R::FetchFailed
            | R::NotChecking
            | R::NoFile { .. }
            | R::NotTrashed { .. }
            | R::FilesNotTrashed { .. }
            | R::Clash { .. }
            | R::IntoItself
            | R::AlreadyThere
            | R::TrashedFiles
            | R::NotMoved { .. }
            // A Move to Trash's undo waits its turn on a lane while the torrent moves: never this.
            | R::Moving
            | R::MovedWithout { .. } => CantUndo::NotInList,
        }),
    }
}

impl Stash {
    /// The .torrent's bytes.
    pub fn torrent(&self) -> Result<Vec<u8>> {
        std::fs::read(self.dir.join("torrent")).with_context(|| format!("the stash in {} has no .torrent", self.dir.display()))
    }

    /// Whether every file is as it was when it was kept: a rename, as to the Trash and back,
    /// keeps a file so; anything written there, or put there instead, doesn't.
    pub fn unchanged(&self) -> bool {
        self.files.iter().all(|(path, seen)| identity(path) == *seen)
    }

    /// Whether its .torrent can be read and parsed: whether it can be added again.
    pub fn readable(&self) -> bool {
        self.torrent().is_ok_and(|bytes| librqbit::torrent_from_bytes(&bytes).is_ok())
    }

    /// Puts the pieces it had where librqbit looks for them when it adds the torrent: a copy, so
    /// librqbit's writes don't change the stash, which stays until the undo is over.
    pub fn restore_pieces(&self, bitv: &Path) -> Result<()> {
        let saved = self.dir.join("bitv");
        if saved.exists() {
            std::fs::copy(&saved, bitv).with_context(|| format!("failed to put back {}", bitv.display()))?;
        }
        Ok(())
    }

    /// The pieces it had, for waiting on librqbit's last save.
    pub fn bitv(&self) -> PathBuf {
        self.dir.join("bitv")
    }
}

/// Keeps the torrent with `info_hash` aside: its .torrent, `torrent`'s bytes, and its `.bitv`,
/// hard-linked from librqbit's `session` folder; with each of its `files` as it is now.
pub fn stash(data_dir: &Path, info_hash: &str, torrent: &[u8], files: &[PathBuf]) -> Result<Stash> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = data_dir.join(STASHES).join(format!("{info_hash}-{nanos}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to make {}", dir.display()))?;
    let session = super::session_folder(data_dir);
    std::fs::write(dir.join("torrent"), torrent).context("failed to keep the .torrent")?;
    // No pieces saved yet: it comes back checked.
    let _ = std::fs::hard_link(super::bitv::path(&session, info_hash), dir.join("bitv"));
    let files = files.iter().map(|f| (f.clone(), identity(f))).collect();
    Ok(Stash { dir, only_files: None, files })
}

/// The magnet link of a torrent without info.
pub fn magnet(info_hash: &str, name: Option<&str>, trackers: &[String]) -> String {
    let mut link = format!("magnet:?xt=urn:btih:{info_hash}");
    let mut push = |key: &str, value: &str| link += &format!("&{key}={}", urlencode(value));
    if let Some(name) = name {
        push("dn", name);
    }
    for tracker in trackers {
        push("tr", tracker);
    }
    link
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The stack as kept, loaded at start: never failing, as Undo is a convenience and the service
/// must start. A file that isn't JSON is no stack; an entry that doesn't read (a newer build's, say)
/// is dropped alone. Whenever something is dropped, the file is kept beside it as
/// `undo.invalid.json`, as a bad `settings.json` is, before the next save writes over it.
pub fn load(dir: &Path) -> machine::State {
    let path = dir.join(FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return machine::State::default(),
        Err(e) => {
            warn!(error = %e, "can't read {FILE}: nothing to undo");
            return machine::State::default();
        }
    };
    let (state, dropped) = decode(&bytes);
    if dropped > 0 {
        warn!(dropped, "{FILE} had what this build can't read: dropped, the file kept as undo.invalid.json");
        if let Err(e) = std::fs::write(proto::tolerant::invalid_copy(&path), &bytes) {
            warn!(error = %e, "failed to keep the invalid {FILE}");
        }
    }
    state
}

/// The stack in `bytes`, and how many parts of it were dropped: each action on the stack and each
/// step on its own. What a build before #190 kept (entries with their seq, one undo under way)
/// reads as done actions, and the undo as a step running on its torrent's lane; what one before
/// #235 kept (each step of one torrent), as steps with one part.
fn decode(bytes: &[u8]) -> (machine::State, usize) {
    use serde_json::Value;
    let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(bytes) else { return (machine::State::default(), 1) };
    let mut dropped = 0;
    let mut state = machine::State::default();
    let list = |value: Option<Value>| match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values,
        Some(other) => vec![other],
    };
    // An entry as kept before #190, with its seq: done.
    let legacy = |value: &Value| Some((value.get("seq").and_then(Value::as_u64)?, serde_json::from_value::<Entry>(value.clone()).ok()?));
    let one = |id: TorrentId| std::iter::once(id).collect::<BTreeSet<_>>();
    for value in list(fields.remove("stack")) {
        let read = serde_json::from_value::<machine::Pressed>(value.clone())
            .ok()
            .or_else(|| serde_json::from_value::<OnePressed>(value.clone()).ok().map(OnePressed::into_pressed))
            .or_else(|| {
                legacy(&value).map(|(seq, e)| machine::Pressed { seq, torrents: one(e.id), done: Some(BTreeMap::from([(e.id, e)])) })
            });
        match read {
            Some(pressed) => state.stack.push(pressed),
            None => dropped += 1,
        }
    }
    if let Some(Value::Object(steps)) = fields.remove("steps") {
        for (seq, value) in steps {
            let step = serde_json::from_value::<machine::Step>(value.clone())
                .ok()
                .or_else(|| serde_json::from_value::<OneStep>(value).ok().and_then(OneStep::into_step));
            match (seq.parse::<u64>(), step) {
                (Ok(seq), Some(step)) => drop(state.steps.insert(seq, step)),
                _ => dropped += 1,
            }
        }
    }
    for value in list(fields.remove("undoing")) {
        match legacy(&value) {
            Some((seq, entry)) => {
                let what = machine::What::Undo { of: seq, parts: BTreeMap::from([(entry.id, Part::Doing(entry.clone()))]) };
                state.steps.insert(seq, machine::Step { torrents: one(entry.id), what, phase: lifecycle::undo::Phase::Running });
            }
            None => dropped += 1,
        }
    }
    // Each step in the lanes of its torrents, in the order they came.
    for (seq, step) in &state.steps {
        for t in &step.torrents {
            state.lanes.entry(*t).or_default().push_back(*seq);
        }
    }
    // Why entries left: each on its own, like the rest; the newest kept.
    for value in list(fields.remove("left")) {
        match serde_json::from_value::<proto::Left>(value) {
            Ok(left) => state.left.push_back(left),
            Err(_) => dropped += 1,
        }
    }
    while state.left.len() > proto::Left::KEPT {
        state.left.pop_front();
    }
    state.last = fields.remove("last").and_then(|v| v.as_u64()).unwrap_or(0).max(state.last_seq());
    let over = state.stack.len().saturating_sub(machine::DEPTH);
    state.stack.drain(..over);
    (state, dropped)
}

/// An entry on the stack as a build before #235 kept it: of one torrent.
#[derive(Deserialize)]
struct OnePressed {
    seq: u64,
    torrents: BTreeSet<TorrentId>,
    done: Option<Entry>,
}

impl OnePressed {
    fn into_pressed(self) -> machine::Pressed {
        let done = self.done.map(|e| BTreeMap::from([(e.id, e)]));
        machine::Pressed { seq: self.seq, torrents: self.torrents, done }
    }
}

/// A step as a build before #235 kept it: of one torrent.
#[derive(Deserialize)]
struct OneStep {
    torrents: BTreeSet<TorrentId>,
    what: OneWhat,
    phase: machine::Phase,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OneWhat {
    Action(Running),
    Undo { of: u64, entry: Entry },
    Command(TorrentCommand),
}

impl OneStep {
    fn into_step(self) -> Option<machine::Step> {
        let id = *self.torrents.first()?;
        let what = match self.what {
            OneWhat::Action(r) => machine::What::Action(BTreeMap::from([(id, Part::Doing(r))])),
            OneWhat::Undo { of, entry } => machine::What::Undo { of, parts: BTreeMap::from([(id, Part::Doing(entry))]) },
            OneWhat::Command(c) => machine::What::Command(c),
        };
        Some(machine::Step { torrents: self.torrents, what, phase: self.phase })
    }
}

pub fn save(dir: &Path, state: &machine::State) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(state)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(FILE))?;
    Ok(())
}

/// Every entry on the stack, and of the steps under way, an undo's or an action's part over.
fn entries(state: &machine::State) -> impl Iterator<Item = &Entry> {
    let done = state.stack.iter().filter_map(|p| p.done.as_ref()).flat_map(|d| d.values());
    // An action's parts over hold what undoes them until the action is committed.
    let undoing = state.steps.values().flat_map(|s| match &s.what {
        machine::What::Undo { parts, .. } => parts.values().filter_map(Part::doing).collect(),
        machine::What::Action(parts) => parts
            .values()
            .filter_map(|p| match p {
                Part::Over(Some(e)) => Some(e),
                Part::Over(None) | Part::Doing(_) => None,
            })
            .collect(),
        machine::What::Command(_) => Vec::<&Entry>::new(),
    });
    done.chain(undoing)
}

/// The info hashes of the torrents that left the list, which the stack brings back.
pub fn gone(state: &machine::State) -> BTreeSet<String> {
    entries(state).filter(|e| matches!(e.back, Back::Gone { .. })).map(|e| e.info_hash.clone()).collect()
}

/// A torrent off the list whose files at the person's paths (#24) have their shadows in its
/// destination, which its undo would find there: by info hash, id and destination.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Shadowed {
    pub info_hash: String,
    pub id: TorrentId,
    pub destination: PathBuf,
}

/// Each torrent the stack would bring back with shadows (a Remove's: a Delete takes them to the
/// Trash with its files), with those files (by index) and the stash its names are in.
pub fn shadowed(state: &machine::State) -> BTreeMap<Shadowed, (BTreeSet<usize>, Option<Stash>)> {
    entries(state)
        .filter_map(|e| match &e.back {
            Back::Gone { saved, source } if !saved.theirs.is_empty() => {
                let destination = saved.destination.as_ref()?.to_path_buf();
                let stash = match source {
                    Source::Stash(s) => Some(s.clone()),
                    Source::Magnet(_) => None,
                };
                Some((Shadowed { info_hash: e.info_hash.clone(), id: e.id, destination }, (saved.theirs.clone(), stash)))
            }
            Back::Gone { .. } | Back::InList { .. } | Back::Files { .. } => None,
        })
        .collect()
}

/// The shadows of a torrent whose entry left the stack, on their way to the Trash
/// (`trashed::trash_shadows`): each file by index, with the name it goes there under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leaving {
    pub info_hash: String,
    pub destination: PathBuf,
    pub files: Vec<(usize, std::ffi::OsString)>,
}

/// The shadows on their way to the Trash, as written ahead; none if it can't be read (reset, with
/// a warning).
pub fn load_leaving(dir: &Path) -> Vec<Leaving> {
    let bytes = match std::fs::read(dir.join(LEAVING)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(error = %e, "can't read {LEAVING}: no shadows to send to the Trash");
            return Vec::new();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        warn!(error = %e, "{LEAVING} is damaged: no shadows to send to the Trash");
        Vec::new()
    })
}

/// Writes ahead the shadows on their way to the Trash, as a Delete's `deleting` is: before the
/// stack is saved without their entry, so a crash between them still sends them there.
pub fn save_leaving(dir: &Path, leaving: &[Leaving]) -> Result<()> {
    if leaving.is_empty() {
        return match std::fs::remove_file(dir.join(LEAVING)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    }
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(leaving)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(LEAVING))?;
    Ok(())
}

/// Removes the stashes in `undo/` that nothing names any more: not `state`, nor any of `records`
/// (an undo's, or a let-go's, #234).
pub fn tidy<'a>(dir: &Path, state: &machine::State, records: impl Iterator<Item = &'a Record>) {
    let mut named: BTreeSet<&Path> = entries(state).flat_map(Entry::stashes).collect();
    for saved in records {
        let letting_go = match &saved.letting_go {
            letgo::State::Stashed { stash, .. } => Some(stash),
            letgo::State::Holding | letgo::State::Gone { .. } => None,
        };
        let moving = saved.moving.as_ref().and_then(|m| m.stash.as_ref());
        named.extend(saved.stash.iter().chain(letting_go).chain(moving).map(|s| s.dir.as_path()));
    }
    for entry in std::fs::read_dir(dir.join(STASHES)).into_iter().flatten().flatten() {
        if !named.contains(entry.path().as_path())
            && let Err(e) = std::fs::remove_dir_all(entry.path())
        {
            warn!(path = %entry.path().display(), error = %e, "failed to remove a stash");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_magnet_link_names_the_hash_name_and_trackers() {
        let hash = "00000000000000000000000000000000000000ab";
        let link = magnet(hash, Some("Big Buck"), &["http://t/announce".into()]);
        assert_eq!(link, format!("magnet:?xt=urn:btih:{hash}&dn=Big%20Buck&tr=http%3A%2F%2Ft%2Fannounce"));
        let parsed = librqbit::Magnet::parse(&link).unwrap();
        assert_eq!((parsed.name.as_deref(), parsed.trackers), (Some("Big Buck"), vec!["http://t/announce".to_owned()]));
    }

    #[test]
    fn the_stack_loads_back_as_saved() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), machine::State::default(), "nothing saved yet");
        let state = machine::sample_state();
        save(dir.path(), &state).unwrap();
        assert_eq!(load(dir.path()), state);
        assert!(!dir.path().join("undo.invalid.json").exists(), "nothing dropped, nothing kept aside");
    }

    /// Whatever is in the file, the stack loads: what doesn't read goes, each entry on its own, and
    /// the file is kept aside when anything went.
    #[test]
    fn a_bad_stack_loads_as_what_can_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("undo.json");
        let good = machine::sample_state();
        let good_json: Vec<serde_json::Value> = good.stack.iter().map(|e| serde_json::to_value(e).unwrap()).collect();
        let mixed = serde_json::json!({"stack": [good_json[0], {"action": "move"}, good_json[1]], "undoing": {"action": "move"}});
        let want = machine::State { stack: good.stack[..2].to_vec(), last: 2, ..Default::default() };
        for (bytes, want) in [
            (b"not json".to_vec(), machine::State::default()),
            (b"[1, 2]".to_vec(), machine::State::default()),
            (serde_json::to_vec(&mixed).unwrap(), want),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            let _ = std::fs::remove_file(dir.path().join("undo.invalid.json"));
            assert_eq!(load(dir.path()), want);
            assert_eq!(std::fs::read(dir.path().join("undo.invalid.json")).unwrap(), bytes, "kept aside");
        }
    }

    #[test]
    fn a_stash_goes_once_nothing_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let kept = stash(dir.path(), "aa", b"t", &[]).unwrap();
        let named_by_a_record = stash(dir.path(), "bb", b"t", &[]).unwrap();
        let unnamed = stash(dir.path(), "cc", b"t", &[]).unwrap();
        let mut entry = machine::samples(1).pop().unwrap();
        entry.back = Back::InList { stash: kept.clone() };
        let one = std::iter::once(entry.id).collect();
        let done = Some(BTreeMap::from([(entry.id, entry)]));
        let state = machine::State { stack: vec![machine::Pressed { seq: 1, torrents: one, done }], ..Default::default() };
        let destination = proto::AbsolutePath::new("/x").unwrap();
        let mut record = Record::new("bb".into(), 1, None, destination, Vec::new(), Default::default());
        record.stash = Some(named_by_a_record.clone());
        tidy(dir.path(), &state, std::iter::once(&record));
        assert!(kept.dir.exists() && named_by_a_record.dir.exists());
        assert!(!unnamed.dir.exists());
        assert_eq!(std::fs::read(kept.dir.join("torrent")).unwrap(), b"t", "the .torrent, without one in the session folder");
    }

    /// A Move to Trash's undo is carried out only for a torrent that's in the list, the same one, and
    /// that moving files to the Trash would do something to now (every phase and what's wanted; each phase's
    /// reason by its own arm, no `_`): the lifecycle's own answer, in the words of Undo.
    #[test]
    fn a_file_trash_is_undone_only_where_trashing_files_would_do_something() {
        use ::machine::Every;
        use lifecycle::Phase as P;
        use proto::{Action, Answer};
        assert_eq!(files_back(None), Err(CantUndo::NotInList), "not in the list");
        for state in lifecycle::State::every() {
            let got = files_back(Some(&state));
            let does = lifecycle::answer(&state, Action::TrashFiles) == Answer::Does;
            let want = match &state.phase {
                P::DestinationUnavailable { reason } => Err(CantUndo::Destination { reason: reason.clone() }),
                P::Deleting { .. } | P::DeleteFailed { .. } => Err(CantUndo::Deleting),
                P::FetchingInfo | P::FetchFailed { .. } | P::Deleted => Err(CantUndo::NotInList),
                // Moving, it waits its turn on its lane, then is carried out where the files are.
                P::CheckDue { .. }
                | P::Checking { .. }
                | P::Ready
                | P::Unchecked { .. }
                | P::MissingFiles { .. }
                | P::Error { .. }
                | P::Moving { .. } => Ok(()),
            };
            assert_eq!(got, want, "{state:?}");
            assert_eq!(got.is_ok(), does, "it agrees with what moving files to the Trash does: {state:?}");
        }
    }

    /// A Move to Trash takes the place of no entry and no entry takes its: the torrent stays in the
    /// list, so it's a step of its own on the stack.
    #[test]
    fn a_file_trash_absorbs_nothing_and_is_absorbed_by_nothing() {
        let entries = machine::samples(12);
        let files: Vec<&Entry> = entries.iter().filter(|e| matches!(e.back, Back::Files { .. })).collect();
        assert!(!files.is_empty());
        for f in &files {
            let same_torrent = Entry { id: f.id, action: UndoAction::Delete, ..entries[1].clone() };
            assert!(entries.iter().all(|e| !f.absorbs(e)), "it absorbs none");
            assert!(!same_torrent.absorbs(f), "a Delete of its torrent doesn't take its place");
        }
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    /// What a build before #235 kept (each entry and step of one torrent) loads as steps of one part.
    #[test]
    fn a_stack_kept_before_selections_loads_as_one_part_each() {
        let mut new = machine::sample_state();
        let pause = machine::What::Command(TorrentCommand::Pause);
        new.steps.insert(6, machine::Step { torrents: [7].into(), what: pause, phase: lifecycle::undo::Phase::Queued });
        new.lanes.insert(7, [6].into());
        new.last = 6;
        let mut old = serde_json::to_value(&new).unwrap();
        for p in old["stack"].as_array_mut().unwrap() {
            if let Some(done) = p["done"].as_object().cloned() {
                p["done"] = done.values().next().unwrap().clone();
            }
        }
        for (_, step) in old["steps"].as_object_mut().unwrap() {
            let what = step["what"].clone();
            if let Some(parts) = what.get("action") {
                step["what"] = serde_json::json!({"action": parts.as_object().unwrap().values().next().unwrap()["doing"]});
            } else if let Some(undo) = what.get("undo") {
                let entry = undo["parts"].as_object().unwrap().values().next().unwrap()["doing"].clone();
                step["what"] = serde_json::json!({"undo": {"of": undo["of"], "entry": entry}});
            }
        }
        let (state, dropped) = decode(&serde_json::to_vec(&old).unwrap());
        assert_eq!(dropped, 0);
        assert_eq!((state.stack, state.steps, state.lanes), (new.stack, new.steps, new.lanes));
    }

    /// Why entries left, as a build before #235 kept it (an undone one with nothing more), loads.
    #[test]
    fn why_an_entry_left_kept_before_selections_loads() {
        let old = serde_json::json!({"left": [{"seq": 3, "why": {"kind": "undone"}}]});
        let (state, dropped) = decode(&serde_json::to_vec(&old).unwrap());
        assert_eq!(dropped, 0);
        assert_eq!(state.left.into_iter().collect::<Vec<_>>(), [proto::Left { seq: 3, why: proto::Leave::Undone { refused: Vec::new() } }]);
    }

    /// A stack a build before #190 kept (entries with their seq, one undo under way) loads as done
    /// actions, in their places, and the undo as a step running on its torrent's lane.
    #[test]
    fn a_stack_kept_before_steps_ran_loads_as_done() {
        let entries = machine::samples(3);
        let with_seq = |e: &Entry, seq: u64| {
            let mut v = serde_json::to_value(e).unwrap();
            v["seq"] = seq.into();
            v
        };
        let old = serde_json::json!({"stack": [with_seq(&entries[0], 5), with_seq(&entries[1], 9)], "undoing": with_seq(&entries[2], 11)});
        let (state, dropped) = decode(&serde_json::to_vec(&old).unwrap());
        let one = |e: &Entry| std::iter::once(e.id).collect::<BTreeSet<_>>();
        let done = |seq, e: &Entry| machine::Pressed { seq, torrents: one(e), done: Some(BTreeMap::from([(e.id, e.clone())])) };
        assert_eq!(dropped, 0);
        assert_eq!(state.stack, [done(5, &entries[0]), done(9, &entries[1])]);
        let undoing = machine::What::Undo { of: 11, parts: BTreeMap::from([(entries[2].id, Part::Doing(entries[2].clone()))]) };
        assert_eq!(state.steps[&11], machine::Step { torrents: one(&entries[2]), what: undoing, phase: lifecycle::undo::Phase::Running });
        assert_eq!(state.lanes[&entries[2].id], [11]);
    }
}
