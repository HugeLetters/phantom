//! Torrent ids, kept as `ids.json` in the data dir: each info hash's id and the next id to give.
//! librqbit numbers the torrents it restores its own way, so the ids clients see are ours and
//! survive restarts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::TorrentId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Ids {
    next: TorrentId,
    /// Info hash (hex) to id.
    ids: BTreeMap<String, TorrentId>,
}

impl Ids {
    pub fn load(dir: &Path) -> Result<Self> {
        match std::fs::read(path(dir)) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("invalid ids.json"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes a temporary file and renames it, so a crash never leaves half a file.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let tmp = dir.join("ids.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path(dir))?;
        Ok(())
    }

    /// A fresh id, for a torrent whose info hash isn't known yet.
    pub fn next(&mut self) -> TorrentId {
        let id = self.next;
        self.next += 1;
        id
    }

    pub fn get(&self, info_hash: &str) -> Option<TorrentId> {
        self.ids.get(info_hash).copied()
    }

    pub fn hash_of(&self, id: TorrentId) -> Option<&str> {
        self.ids.iter().find(|(_, i)| **i == id).map(|(hash, _)| hash.as_str())
    }

    pub fn insert(&mut self, info_hash: String, id: TorrentId) {
        self.ids.insert(info_hash, id);
    }

    /// The hash's id, giving it a fresh one if it has none.
    pub fn get_or_assign(&mut self, info_hash: &str) -> TorrentId {
        match self.get(info_hash) {
            Some(id) => id,
            None => {
                let id = self.next();
                self.insert(info_hash.to_owned(), id);
                id
            }
        }
    }

    pub fn forget(&mut self, info_hash: &str) {
        self.ids.remove(info_hash);
    }

    /// Forgets every hash `keep` says no to.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.ids.retain(|hash, _| keep(hash));
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("ids.json")
}
