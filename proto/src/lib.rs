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
pub const PROTOCOL_VERSION: u32 = 7;

/// Given by the service when a torrent is added, before its info arrives, and kept for as long
/// as the torrent is in the list, across restarts of the service.
pub type TorrentId = u64;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello { protocol: u32 },
    List,
    /// Answered with `Added` as soon as the torrent is in the list, for a magnet while its info
    /// is still being fetched. A .torrent URL is downloaded first, so its answer can say whether
    /// the torrent was already in the list.
    Add { source: AddSource, paused: bool },
    Pause { id: TorrentId },
    /// Start the torrent. One that's Missing files is checked first, then downloads them again.
    Resume { id: TorrentId },
    /// Check the torrent's files on disk against its info, keeping what's valid; it then runs or
    /// stays stopped as before. Refused while it's fetching info.
    Check { id: TorrentId },
    /// Take the torrent off the list, keeping its files. A torrent fetching info stops.
    Remove { id: TorrentId },
    /// Remove the torrent and delete its files.
    Delete { id: TorrentId },
    /// The torrent's files, answered with `Details`. Refused while it's fetching info.
    Details { id: TorrentId },
    /// Set the priority of the files at `files` (their [`File::index`]). Every file Off is fine:
    /// the torrent is then finished. While the torrent is being checked the change is held and
    /// applied when the check ends.
    SetPriority { id: TorrentId, files: Vec<usize>, priority: Priority },
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
    Torrents { torrents: Vec<TorrentInfo> },
    Added(Added),
    Details(Details),
    Ok,
    Error { message: String },
}

/// The answer to an add.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub id: TorrentId,
    pub name: Option<String>,
    /// A torrent with the same info hash was already in the list, and `id` is that torrent's.
    pub already_existed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TorrentInfo {
    pub id: TorrentId,
    /// Unknown only while a .torrent URL is downloading.
    pub info_hash: Option<String>,
    /// While fetching info: from the magnet's `dn=` or the URL's file name, if there is one.
    pub name: Option<String>,
    pub status: Status,
    pub error: Option<String>,
    pub progress_bytes: u64,
    /// The torrent's Size: the files you're getting. 0 until its info arrives.
    pub size_bytes: u64,
    /// The torrent's Full size: all its files, whatever their priority. 0 until its info arrives.
    pub full_size_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_bytes_per_sec: u64,
    pub upload_bytes_per_sec: u64,
    pub live_peers: u32,
}

/// A torrent's files and where they are, the answer to `Details`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub id: TorrentId,
    pub files: Vec<File>,
    pub full_size_bytes: u64,
    /// Where the torrent is on disk: the file itself for a single-file torrent, the torrent's
    /// folder otherwise. It may not exist yet.
    pub location: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Names the file in `SetPriority`; stays the same for the torrent's whole life.
    pub index: usize,
    /// Inside the torrent, e.g. `disc 1/track 01.flac`.
    pub path: PathBuf,
    pub size_bytes: u64,
    pub progress_bytes: u64,
    pub priority: Priority,
    /// Gone from disk (or of a size it can't have) since it was last checked; its progress is 0.
    /// Only a file whose priority is Normal is missing.
    pub missing: bool,
}

impl File {
    /// Downloaded fraction, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        if self.size_bytes == 0 { 1.0 } else { (self.progress_bytes as f64 / self.size_bytes as f64) as f32 }
    }
}

/// Whether a file is downloaded.
// Only Normal and Off: librqbit can include or skip a file, nothing more, and downloads the
// files it includes in name order (its "TODO: make it configurable" in
// `TorrentStateLive::new`). High and Low wait for that: issue #10.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Normal,
    /// Not downloaded.
    Off,
}

impl Priority {
    /// How clients show it.
    pub fn label(self) -> &'static str {
        match self {
            Priority::Normal => "Normal",
            Priority::Off => "Off",
        }
    }
}

/// What a torrent is doing right now, in the glossary's words. Ordered as a list sorted by
/// status reads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    FetchingInfo,
    /// Verifying files already on disk.
    Checking,
    Downloading,
    Seeding,
    /// Paused before it was complete.
    Paused,
    /// Complete and paused.
    Done,
    /// Files it had downloaded are gone from disk; resuming downloads them again.
    MissingFiles,
    /// Its destination can't be reached, e.g. the drive isn't connected.
    DestinationUnavailable {
        /// Why, for people: e.g. "drive not connected".
        reason: String,
    },
    /// Its files are being moved to a new destination.
    Moving,
    Error,
}

impl Status {
    /// How clients show it.
    pub fn label(&self) -> &'static str {
        match self {
            Status::FetchingInfo => "Fetching info",
            Status::Checking => "Checking",
            Status::Downloading => "Downloading",
            Status::Seeding => "Seeding",
            Status::Paused => "Paused",
            Status::Done => "Done",
            Status::MissingFiles => "Missing files",
            Status::DestinationUnavailable { .. } => "Destination unavailable",
            Status::Moving => "Moving",
            Status::Error => "Error",
        }
    }
}

impl TorrentInfo {
    /// Downloaded fraction, 0.0 to 1.0. A torrent with every file Off has nothing left: 1.0.
    pub fn progress(&self) -> f32 {
        if self.size_bytes == 0 {
            if self.full_size_bytes > 0 { 1.0 } else { 0.0 }
        } else {
            (self.progress_bytes as f64 / self.size_bytes as f64) as f32
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let left = self.size_bytes.saturating_sub(self.progress_bytes);
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
