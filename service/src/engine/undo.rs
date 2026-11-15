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
use proto::{TorrentId, UndoAction, Undoable};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::store::Saved;
use super::trash::Trashed;

const FILE: &str = "undo.json";
const STASHES: &str = "undo";

/// What it takes to reverse one Remove or Delete.
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
}

/// How the torrent comes back.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Back {
    /// It left the list: it's added again with this record (its destination, what's wanted, its
    /// seeding round), from `source`.
    Gone { saved: Saved, source: Source },
    /// It's still in the list, stopped, its files partly in the Trash (a partial Delete): it's
    /// taken out and added again from its stash.
    InList { stash: Stash },
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
}

impl Entry {
    pub fn undoable(&self) -> Undoable {
        Undoable { action: self.action, id: self.id, name: self.name.clone() }
    }

    /// A Delete takes in the partial Delete of the same torrent (by id: one added again since is
    /// another) before it: its own items hold those too, and once it's undone the torrent is back
    /// whole.
    pub fn absorbs(&self, older: &Entry) -> bool {
        let deletes = self.action == UndoAction::Delete && older.action == UndoAction::Delete;
        deletes && self.id == older.id && matches!(older.back, Back::InList { .. })
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
        };
        named.into_iter().chain(saved).map(|s| s.dir.as_path())
    }
}

impl Stash {
    /// The .torrent's bytes.
    pub fn torrent(&self) -> Result<Vec<u8>> {
        std::fs::read(self.dir.join("torrent")).with_context(|| format!("the stash in {} has no .torrent", self.dir.display()))
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
    Ok(Stash { dir, output_folder, only_files, librqbit_id })
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

/// The stack as kept: loaded at start, written whole on every change.
pub fn load(dir: &Path) -> Result<machine::State> {
    match std::fs::read(dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("invalid {FILE}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(machine::State::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {FILE}")),
    }
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
        assert_eq!(load(dir.path()).unwrap(), machine::State::default(), "nothing saved yet");
        let state = machine::State { stack: machine::samples(3), undoing: machine::samples(1).pop() };
        save(dir.path(), &state).unwrap();
        assert_eq!(load(dir.path()).unwrap(), state);
    }

    #[test]
    fn a_stash_goes_once_nothing_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let kept = stash(dir.path(), "aa", b"t", "/x".into(), None, 0).unwrap();
        let named_by_a_record = stash(dir.path(), "bb", b"t", "/x".into(), None, 1).unwrap();
        let unnamed = stash(dir.path(), "cc", b"t", "/x".into(), None, 2).unwrap();
        let mut entry = machine::samples(1).pop().unwrap();
        entry.back = Back::InList { stash: kept.clone() };
        let state = machine::State { stack: vec![entry], undoing: None };
        let mut record = Saved::default();
        record.stash = Some(named_by_a_record.clone());
        tidy(dir.path(), &state, std::iter::once(&record));
        assert!(kept.dir.exists() && named_by_a_record.dir.exists());
        assert!(!unnamed.dir.exists());
        assert_eq!(std::fs::read(kept.dir.join("torrent")).unwrap(), b"t", "the .torrent, without one in the session folder");
    }
}
