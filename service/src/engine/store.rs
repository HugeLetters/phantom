//! What the engine keeps across restarts beyond librqbit's own session state: `engine.json` in
//! the data dir. Per torrent, by info hash: its id, its destination, the priorities held while
//! it was being checked, and, from when the service last stopped, whether it was running, which
//! files were missing and the files it was getting (checked on the next start). Plus the next id
//! to give. Written whole on every change, through a temporary file and a rename, so a crash
//! never leaves half a file.
//!
//! librqbit numbers the torrents it restores its own way, so the ids clients see are ours.
//! Before `engine.json` the ids were in `ids.json`; [`Store::load`] takes them from there once.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::TorrentId;
use serde::{Deserialize, Serialize};

use super::files::Expected;

const FILE: &str = "engine.json";
/// The store before `engine.json`: just `next` and `ids` (info hash to id).
const OLD_IDS_FILE: &str = "ids.json";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Store {
    next_id: TorrentId,
    /// By info hash (hex).
    torrents: BTreeMap<String, Saved>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Saved {
    pub id: TorrentId,
    /// The folder the torrent's files go into; a torrent of several files has its own folder
    /// inside it. Unknown until the torrent's info arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<PathBuf>,
    /// Priorities asked for while the torrent was being checked: the files to get (those
    /// Normal). librqbit can't change files during a check, so they're applied once it's done,
    /// after a restart if need be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<BTreeSet<usize>>,
    /// It was running (Downloading, Seeding or Checking to run) when the service stopped, so it
    /// resumes when the service starts again. Cleared once it has.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running_at_shutdown: bool,
    /// The files that were missing (in Missing files) when the service stopped, by index, with
    /// what librqbit had of each before it went. Taken on start.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missing_at_shutdown: BTreeMap<usize, u64>,
    /// The files it was getting when the service stopped, checked on the next start before
    /// librqbit restores the torrent (which creates missing files again). Taken on start, so a
    /// crash, which skips the stop, checks nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_at_shutdown: Vec<Expected>,
}

impl Store {
    pub fn load(dir: &Path) -> Result<Self> {
        if let Some(bytes) = read(&dir.join(FILE))? {
            return serde_json::from_slice(&bytes).with_context(|| format!("invalid {FILE}"));
        }
        let Some(bytes) = read(&dir.join(OLD_IDS_FILE))? else { return Ok(Self::default()) };
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct OldIds {
            next: TorrentId,
            ids: BTreeMap<String, TorrentId>,
        }
        let old: OldIds = serde_json::from_slice(&bytes).with_context(|| format!("invalid {OLD_IDS_FILE}"))?;
        let torrents = old
            .ids
            .into_iter()
            .map(|(hash, id)| (hash, Saved { id, ..Default::default() }))
            .collect();
        Ok(Self { next_id: old.next, torrents })
    }

    /// Writes `engine.json` (and drops `ids.json`, which it replaces).
    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(&serde_json::to_vec_pretty(self)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(dir.join(FILE))?;
        match std::fs::remove_file(dir.join(OLD_IDS_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// A fresh id, for a torrent whose info hash isn't known yet.
    pub fn next_id(&mut self) -> TorrentId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn id(&self, info_hash: &str) -> Option<TorrentId> {
        self.torrents.get(info_hash).map(|s| s.id)
    }

    pub fn get(&self, info_hash: &str) -> Option<&Saved> {
        self.torrents.get(info_hash)
    }

    pub fn get_mut(&mut self, info_hash: &str) -> Option<&mut Saved> {
        self.torrents.get_mut(info_hash)
    }

    pub fn insert(&mut self, info_hash: String, id: TorrentId) {
        self.torrents.insert(info_hash, Saved { id, ..Default::default() });
    }

    /// The hash's id, giving it a fresh one if it has none.
    pub fn id_or_assign(&mut self, info_hash: &str) -> TorrentId {
        match self.id(info_hash) {
            Some(id) => id,
            None => {
                let id = self.next_id();
                self.insert(info_hash.to_owned(), id);
                id
            }
        }
    }

    pub fn forget(&mut self, info_hash: &str) {
        self.torrents.remove(info_hash);
    }

    /// Every torrent, by info hash.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Saved)> {
        self.torrents.iter_mut()
    }

    /// Forgets every hash `keep` says no to.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.torrents.retain(|hash, _| keep(hash));
    }
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_json_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ids.json"), r#"{"next": 5, "ids": {"aa": 3}}"#).unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        assert_eq!(store.id("aa"), Some(3));
        assert_eq!(store.next_id(), 5);
        store.save(dir.path()).unwrap();
        assert!(!dir.path().join("ids.json").exists(), "replaced");
        assert_eq!(Store::load(dir.path()).unwrap(), store);
    }

    #[test]
    fn a_saved_store_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::default();
        let id = store.id_or_assign("aa");
        let saved = store.get_mut("aa").unwrap();
        saved.destination = Some("/tmp/x".into());
        saved.held = Some([0, 2].into());
        saved.running_at_shutdown = true;
        saved.missing_at_shutdown = [(1, 5)].into();
        saved.files_at_shutdown = vec![Expected { index: 0, path: "/tmp/x/a".into(), size_bytes: 9, progress_bytes: 3 }];
        store.id_or_assign("bb");
        store.save(dir.path()).unwrap();
        let loaded = Store::load(dir.path()).unwrap();
        assert_eq!(loaded, store);
        assert_eq!(loaded.id("aa"), Some(id));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary file left");
    }
}
