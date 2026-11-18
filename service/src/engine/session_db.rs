//! librqbit's own record of its torrents, from before #45: `session.json` in the session folder,
//! with a `<hash>.torrent` copy of each. The only code that knows that layout. The first start of a
//! build with Phantom's own record takes it over ([`restored`]) and puts it aside as a marker
//! ([`taken_over`]): `session.migrated.json`, or `session.invalid.json` when some of it can't be
//! read, which is then kept as it was. From then on, librqbit's own (it writes one again as it
//! runs) is only deleted ([`forget`]), never read, so librqbit restores nothing and nothing it kept
//! comes back: the engine adds every torrent itself. What it saves of each torrent's pieces
//! (`<hash>.bitv`) stays.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::warn;

const FILE: &str = "session.json";
/// `session.json`, once taken over.
const MIGRATED: &str = "session.migrated.json";
/// `session.json`, some of which couldn't be read, kept as it was.
const INVALID: &str = "session.invalid.json";

/// A torrent librqbit had: where it put its files, and which it got.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Restored {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    pub output_folder: PathBuf,
    /// The files it gets; `None`: all of them.
    pub only_files: Option<Vec<usize>>,
    #[serde(default)]
    pub trackers: Vec<String>,
}

/// What librqbit kept in `session.json`.
#[derive(Debug, Default)]
pub struct Session {
    /// Its torrents, by info hash.
    pub torrents: HashMap<String, Restored>,
    /// Whether some of it couldn't be read: a torrent, or the whole file.
    pub invalid: bool,
}

/// Whether `session.json` was taken over already, by an earlier start.
pub fn taken_over(session_folder: &Path) -> bool {
    session_folder.join(MIGRATED).exists() || session_folder.join(INVALID).exists()
}

/// The torrents librqbit kept in `session_folder`, each read on its own: one that can't be read
/// doesn't keep the others out. None kept, nothing. Failing to read the file at all fails.
pub fn restored(session_folder: &Path) -> Result<Session> {
    #[derive(Deserialize)]
    struct Db {
        torrents: HashMap<String, serde_json::Value>,
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Session::default()),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
    };
    let Ok(db) = serde_json::from_slice::<Db>(&bytes) else {
        warn!(?path, "librqbit's session isn't as expected: kept aside, none of it taken over");
        return Ok(Session { torrents: HashMap::new(), invalid: true });
    };
    let mut session = Session::default();
    for (n, torrent) in db.torrents {
        match serde_json::from_value::<Torrent>(torrent) {
            Ok(t) => drop(session.torrents.insert(t.info_hash, t.restored)),
            Err(e) => {
                warn!(?path, torrent = n, error = %e, "a torrent of librqbit's session can't be read: kept aside, not taken over");
                session.invalid = true;
            }
        }
    }
    Ok(session)
}

/// Puts `session.json` aside once it's taken over: as the marker that it was, or, `invalid`, kept
/// as it was beside it.
pub fn put_aside(session_folder: &Path, invalid: bool) -> Result<()> {
    let path = session_folder.join(FILE);
    // None to take over: the marker alone, so librqbit's own, written as it runs, is never taken
    // for it.
    if !path.exists() {
        let marker = session_folder.join(MIGRATED);
        return std::fs::write(&marker, br#"{"torrents":{}}"#).with_context(|| format!("failed to write {}", marker.display()));
    }
    let to = session_folder.join(if invalid { INVALID } else { MIGRATED });
    std::fs::rename(&path, &to).with_context(|| format!("failed to put {} aside", path.display()))
}

/// Deletes librqbit's record of its torrents, which it writes again as it runs, and the .torrent
/// copies of those `kept` elsewhere (`kept` says which). What it saved of their pieces stays.
pub fn forget(session_folder: &Path, kept: impl Fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(session_folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let theirs = name == FILE || name.strip_suffix(".torrent").is_some_and(&kept);
        if theirs && let Err(e) = std::fs::remove_file(&path) {
            warn!(?path, error = %e, "can't delete librqbit's record of a torrent");
        }
    }
}

/// librqbit's .torrent copy of the torrent with `info_hash`.
pub fn torrent_path(session_folder: &Path, info_hash: &str) -> PathBuf {
    session_folder.join(format!("{info_hash}.torrent"))
}
