//! librqbit's `session.json` in the session folder: each torrent it restores at start, with the
//! folder its files go into and the files it gets. The only code that knows that layout. Read
//! before librqbit restores the torrents, which creates their files (#158).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::warn;

const FILE: &str = "session.json";

/// Where a torrent librqbit restores puts its files, and which it gets.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Restored {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    pub output_folder: PathBuf,
    /// The files it gets; `None`: all of them.
    pub only_files: Option<Vec<usize>>,
}

/// The torrents librqbit will restore from `session_folder`, by info hash. Nothing if it has
/// saved none, or the file can't be read: librqbit then restores them with its own storage.
pub fn restored(session_folder: &Path) -> HashMap<String, Restored> {
    #[derive(Deserialize)]
    struct Db {
        torrents: HashMap<String, Torrent>,
    }
    #[derive(Deserialize)]
    struct Torrent {
        info_hash: String,
        #[serde(flatten)]
        restored: Restored,
    }
    let path = session_folder.join(FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            warn!(?path, error = %e, "can't read librqbit's session");
            return HashMap::new();
        }
    };
    match serde_json::from_slice::<Db>(&bytes) {
        Ok(db) => db.torrents.into_values().map(|t| (t.info_hash, t.restored)).collect(),
        Err(e) => {
            warn!(?path, error = %e, "librqbit's session isn't as expected");
            HashMap::new()
        }
    }
}
