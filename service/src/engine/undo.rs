//! Undo ([ADR 0008]): what a Remove or a Delete changed, kept as what it takes to reverse it, an
//! [`Entry`], on a stack of the last [`machine::DEPTH`] ([`machine`]). An undo is a command of its
//! own: it puts back from the Trash what Delete moved there ([`super::trash::put_back`]), then adds
//! the torrent again as it was, from its [`Stash`]: the .torrent and librqbit's `.bitv` (the pieces
//! it had), so it comes back with its progress and without a full check. librqbit deletes both
//! from its session folder when a torrent goes, so they're hard-linked aside first, while the
//! torrent is still whole: a link shares the file, so librqbit's last save, as it drops the
//! torrent, lands in the stash too.
//!
//! Kept in the data dir, next to the store: `undo.json`, the stack and the undo under way (written
//! before it starts, so one cut short by a crash is finished at the next start), and `undo/`, a
//! folder per stash. A stash goes once neither the stack nor a torrent's record names it.
//!
//! [ADR 0008]: ../../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

pub mod machine;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::{CantUndo, Priority, TorrentId, UndoAction, Undoable};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::store::Saved;
use super::trash::{Identity, Trashed, identity};

const FILE: &str = "undo.json";
const STASHES: &str = "undo";

/// What it takes to reverse one Remove or Delete.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its own, larger than any entry's before it: what clients tell entries apart by.
    #[serde(default)]
    pub seq: u64,
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
    /// `previous`, except a file that was moved and didn't come back, which stays Off.
    Files { previous: Vec<(usize, Priority, PathBuf)> },
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

/// A torrent kept aside: a folder in `undo/` with its .torrent and the pieces it had, and how
/// librqbit had it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stash {
    pub dir: PathBuf,
    /// librqbit's output folder: the destination, and its own folder in it for several files.
    pub output_folder: PathBuf,
    /// The files it gets (those Normal); `None`, every file.
    pub only_files: Option<Vec<usize>>,
    /// librqbit's id for it, which it gets again: no torrent added since takes one it had.
    pub librqbit_id: usize,
    /// Each of its files as it was when kept ([`super::trash::identity`]; `None`: not there). The
    /// pieces it had hold only while every one is still so.
    #[serde(default)]
    pub files: Vec<(PathBuf, Option<Identity>)>,
}

impl Entry {
    pub fn undoable(&self) -> Undoable {
        Undoable { seq: self.seq, action: self.action, id: self.id, name: self.name.clone() }
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

/// Whether the files a file trash moved can be put back into a torrent that's `torrent` now
/// (its lifecycle; `None`: not in the list): the torrent must be one trashing files would do
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
            | R::FilesNotTrashed { .. } => CantUndo::NotInList,
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

/// Keeps the torrent with `info_hash` aside: its .torrent (`torrent`'s bytes when librqbit has
/// none saved) and its `.bitv`, hard-linked from librqbit's `session` folder.
pub fn stash(
    data_dir: &Path,
    info_hash: &str,
    torrent: &[u8],
    output_folder: PathBuf,
    only_files: Option<Vec<usize>>,
    librqbit_id: usize,
    files: &[PathBuf],
) -> Result<Stash> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = data_dir.join(STASHES).join(format!("{info_hash}-{nanos}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to make {}", dir.display()))?;
    let session = data_dir.join("session");
    if std::fs::hard_link(session.join(format!("{info_hash}.torrent")), dir.join("torrent")).is_err() {
        std::fs::write(dir.join("torrent"), torrent).context("failed to keep the .torrent")?;
    }
    // No pieces saved yet: it comes back checked.
    let _ = std::fs::hard_link(super::bitv::path(&session, info_hash), dir.join("bitv"));
    let files = files.iter().map(|f| (f.clone(), identity(f))).collect();
    Ok(Stash { dir, output_folder, only_files, librqbit_id, files })
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

/// The stack in `bytes`, and how many parts of it were dropped.
fn decode(bytes: &[u8]) -> (machine::State, usize) {
    use serde_json::Value;
    let Ok(Value::Object(mut fields)) = serde_json::from_slice::<Value>(bytes) else { return (machine::State::default(), 1) };
    let values = match fields.remove("stack") {
        None => Vec::new(),
        Some(Value::Array(values)) => values,
        Some(other) => vec![other],
    };
    let undoing = fields.remove("undoing").filter(|v| !v.is_null());
    let read = |value: Value| serde_json::from_value::<Entry>(value).ok();
    let wanted = values.len() + usize::from(undoing.is_some());
    let stack: Vec<Entry> = values.into_iter().filter_map(read).collect();
    let undoing = undoing.and_then(read);
    let dropped = wanted - stack.len() - usize::from(undoing.is_some());
    let over = stack.len().saturating_sub(machine::DEPTH);
    (machine::State { stack: stack.into_iter().skip(over).collect(), undoing, queued: 0 }, dropped)
}

pub fn save(dir: &Path, state: &machine::State) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(state)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(FILE))?;
    Ok(())
}

/// Removes the stashes in `undo/` that nothing names any more: not `state`, nor any of `records`.
pub fn tidy<'a>(dir: &Path, state: &machine::State, records: impl Iterator<Item = &'a Saved>) {
    let mut named: BTreeSet<&Path> = state.stack.iter().chain(&state.undoing).flat_map(Entry::stashes).collect();
    named.extend(records.filter_map(|s| s.stash.as_ref()).map(|s| s.dir.as_path()));
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
        let state = machine::State { stack: machine::samples(3), undoing: machine::samples(1).pop(), queued: 0 };
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
        let good = machine::samples(2);
        let good_json: Vec<serde_json::Value> = good.iter().map(|e| serde_json::to_value(e).unwrap()).collect();
        let mixed = serde_json::json!({"stack": [good_json[0], {"action": "move"}, good_json[1]], "undoing": {"action": "move"}});
        for (bytes, want) in [
            (b"not json".to_vec(), machine::State::default()),
            (b"[1, 2]".to_vec(), machine::State::default()),
            (serde_json::to_vec(&mixed).unwrap(), machine::State { stack: good.clone(), undoing: None, queued: 0 }),
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
        let kept = stash(dir.path(), "aa", b"t", "/x".into(), None, 0, &[]).unwrap();
        let named_by_a_record = stash(dir.path(), "bb", b"t", "/x".into(), None, 1, &[]).unwrap();
        let unnamed = stash(dir.path(), "cc", b"t", "/x".into(), None, 2, &[]).unwrap();
        let mut entry = machine::samples(1).pop().unwrap();
        entry.back = Back::InList { stash: kept.clone() };
        let state = machine::State { stack: vec![entry], undoing: None, queued: 0 };
        let mut record = Saved::default();
        record.stash = Some(named_by_a_record.clone());
        tidy(dir.path(), &state, std::iter::once(&record));
        assert!(kept.dir.exists() && named_by_a_record.dir.exists());
        assert!(!unnamed.dir.exists());
        assert_eq!(std::fs::read(kept.dir.join("torrent")).unwrap(), b"t", "the .torrent, without one in the session folder");
    }

    /// A file trash's undo is carried out only for a torrent that's in the list, the same one, and
    /// that trashing files would do something to now (every phase and what's wanted; each phase's
    /// reason by its own arm, no `_`): the lifecycle's own answer, in the words of Undo.
    #[test]
    fn a_file_trash_is_undone_only_where_trashing_files_would_do_something() {
        use lifecycle::Phase as P;
        use lifecycle::machine::Every;
        use proto::{Action, Answer};
        assert_eq!(files_back(None), Err(CantUndo::NotInList), "not in the list");
        for state in lifecycle::State::every() {
            let got = files_back(Some(&state));
            let does = lifecycle::answer(&state, Action::TrashFiles) == Answer::Does;
            let want = match &state.phase {
                P::DestinationUnavailable { reason } => Err(CantUndo::Destination { reason: reason.clone() }),
                P::Deleting { .. } | P::DeleteFailed { .. } => Err(CantUndo::Deleting),
                P::FetchingInfo | P::FetchFailed { .. } | P::Deleted => Err(CantUndo::NotInList),
                P::CheckDue { .. } | P::Checking { .. } | P::Ready | P::Unchecked { .. } | P::MissingFiles { .. } | P::Error { .. } => {
                    Ok(())
                }
            };
            assert_eq!(got, want, "{state:?}");
            assert_eq!(got.is_ok(), does, "it agrees with what trashing files does: {state:?}");
        }
    }

    /// A Trash files takes the place of no entry and no entry takes its: the torrent stays in the
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
