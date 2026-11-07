//! The contract between the service and its clients: every request a client can make, every
//! answer, and the data they carry. No I/O; clients reach the service through the `client` crate.
//!
//! Wire format: one JSON object per line over a Unix socket at [`socket_path`]. Cap'n Proto and
//! QUIC were considered and rejected: the messages are tiny (a few KB per second) and both sides
//! are on the same machine, so JSON's simplicity and debuggability win.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Bump on any incompatible change to [`Request`], [`Response`] or the types they carry.
pub const PROTOCOL_VERSION: u32 = 3;

pub type TorrentId = u64;
pub type PendingId = u64;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello { protocol: u32 },
    List,
    Add { source: AddSource, paused: bool },
    Pause { id: TorrentId },
    Resume { id: TorrentId },
    Remove { id: TorrentId, delete_files: bool },
    /// Abort an add that is still resolving; its `Add` request fails with "cancelled".
    CancelAdd { id: PendingId },
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AddSource {
    /// Magnet link, bare info hash, or http(s) URL of a .torrent file.
    Url(String),
    /// Contents of a .torrent file.
    TorrentFile(Vec<u8>),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello { protocol: u32, service_version: String },
    Torrents { torrents: Vec<TorrentInfo>, pending: Vec<PendingAdd> },
    Added { id: TorrentId, name: Option<String>, already_existed: bool },
    Ok,
    Error { message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TorrentInfo {
    pub id: TorrentId,
    pub info_hash: String,
    pub name: Option<String>,
    pub state: TorrentState,
    /// All wanted bytes are downloaded and verified.
    pub finished: bool,
    pub error: Option<String>,
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_bytes_per_sec: u64,
    pub upload_bytes_per_sec: u64,
    pub live_peers: u32,
}

/// An add the service is still working on, typically a magnet waiting for metadata from peers.
/// It becomes a torrent when that arrives; until then only `CancelAdd` can stop it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PendingAdd {
    pub id: PendingId,
    /// From the magnet's `dn=` or the URL's file name, when there is one.
    pub name: Option<String>,
    /// The magnet link or URL, or "torrent file".
    pub source: String,
    pub elapsed_secs: u64,
}

/// Everything `List` returns.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub torrents: Vec<TorrentInfo>,
    pub pending: Vec<PendingAdd>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentState {
    /// Checking existing files or resolving magnet metadata.
    Initializing,
    Live,
    Paused,
    Error,
}

impl TorrentInfo {
    /// Downloaded fraction, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        if self.total_bytes == 0 { 0.0 } else { (self.progress_bytes as f64 / self.total_bytes as f64) as f32 }
    }

    pub fn eta(&self) -> Option<Duration> {
        let left = self.total_bytes.saturating_sub(self.progress_bytes);
        if left == 0 || self.download_bytes_per_sec == 0 {
            return None;
        }
        Some(Duration::from_secs(left / self.download_bytes_per_sec))
    }
}

/// `~/Library/Application Support/Phantom` on macOS, `$XDG_DATA_HOME/phantom` elsewhere.
/// `PHANTOM_DATA_DIR` overrides it, e.g. to run an isolated service for testing.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PHANTOM_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"));
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/Phantom");
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("phantom")
}

/// Where the service listens and clients find it: part of the contract, like the messages.
pub fn socket_path() -> PathBuf {
    data_dir().join("service.sock")
}
