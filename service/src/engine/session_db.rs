//! librqbit's own record of its torrents, from before #45: `session.json` in the session folder,
//! with a `<hash>.torrent` copy of each. The only code that knows that layout. [`restored`] reads
//! it, so the first start of a build with Phantom's own record takes it over; [`forget`] deletes
//! it, so librqbit restores nothing: the engine adds every torrent itself. librqbit writes it again
//! as it runs, which nothing reads; what it saves of each torrent's pieces (`<hash>.bitv`) stays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::warn;

const FILE: &str = "session.json";

/// A torrent librqbit had: where it put its files, which it got, and its .torrent.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Restored {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    pub output_folder: PathBuf,
    /// The files it gets; `None`: all of them.
    pub only_files: Option<Vec<usize>>,
    #[serde(default)]
    pub trackers: Vec<String>,
    /// Its .torrent, if librqbit kept a copy.
    #[serde(skip)]
    pub torrent: Option<Vec<u8>>,
}

/// The torrents librqbit kept in `session_folder`, by info hash. Nothing if it kept none, or the
/// file can't be read.
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
        Ok(db) => db
            .torrents
            .into_values()
            .map(|t| {
                let torrent = std::fs::read(torrent_path(session_folder, &t.info_hash)).ok();
                (t.info_hash, Restored { torrent, ..t.restored })
            })
            .collect(),
        Err(e) => {
            warn!(?path, error = %e, "librqbit's session isn't as expected");
            HashMap::new()
        }
    }
}

/// Deletes librqbit's record of its torrents and their .torrent copies, keeping what it saved of
/// their pieces.
pub fn forget(session_folder: &Path) {
    let Ok(entries) = std::fs::read_dir(session_folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let theirs = path.file_name().is_some_and(|n| n == FILE) || path.extension().is_some_and(|e| e == "torrent");
        if theirs && let Err(e) = std::fs::remove_file(&path) {
            warn!(?path, error = %e, "can't delete librqbit's record of a torrent");
        }
    }
}

fn torrent_path(session_folder: &Path, info_hash: &str) -> PathBuf {
    session_folder.join(format!("{info_hash}.torrent"))
}
