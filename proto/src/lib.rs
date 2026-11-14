//! The contract between the service and its clients: every request a client can make, every
//! answer, and the data they carry. No I/O; clients reach the service through the `client` crate.
//! Data, not sentences: a refusal is a [`Refusal`] with its facts, which `client` words.
//!
//! Wire format: one JSON object per line over a Unix socket at [`socket_path`]. Cap'n Proto and
//! QUIC were considered and rejected: the messages are tiny (a few KB per second) and both sides
//! are on the same machine, so JSON's simplicity and debuggability win.

pub mod tolerant;

use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};

/// Identifies the executable a service runs from: a hash of its bytes. The service and its
/// clients ship together, so a client replaces a service whose id isn't that of the
/// `phantom-service` it would start.
pub fn exe_id(exe: &Path) -> std::io::Result<String> {
    use std::hash::Hasher;
    use std::io::Read;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut file = std::fs::File::open(exe)?;
    let mut buf = vec![0; 1 << 16];
    loop {
        match file.read(&mut buf)? {
            0 => break,
            n => hasher.write(&buf[..n]),
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// Given by the service when a torrent is added, before its info arrives, and kept for as long
/// as the torrent is in the list, across restarts of the service.
pub type TorrentId = u64;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello,
    /// Turns the connection into a stream of [`Response::Snapshot`]s: one at once, then one after
    /// each change, at most about every 100 ms (a slow client skips to the latest, never backs
    /// up), with the details of the torrents `watching` that have info. Sent again, with the same
    /// torrents or others, it's answered with a snapshot at once, built for them. Nothing else is
    /// asked on it after; closing it ends the stream. Refused while the service is stopping, which
    /// also ends a stream.
    Subscribe {
        watching: Vec<TorrentId>,
    },
    /// Answered with `Added` as soon as the torrent is in the list, while its info is still being
    /// fetched; a .torrent URL is downloaded first (failing to is a failed add). Its files go into
    /// `destination`, or without one the default destination: refused with
    /// [`Refusal::NoDestination`] without either, and with [`Refusal::Destination`] when it can't
    /// be reached.
    Add {
        source: AddSource,
        paused: bool,
        destination: Option<AbsolutePath>,
    },
    /// Answered with `Ok`, or refused with a [`Refusal::Torrent`].
    Torrent {
        id: TorrentId,
        command: TorrentCommand,
    },
    /// The service's settings, answered with `Settings`.
    Settings,
    /// Change one setting, answered with `Settings`: all of them, as they are now, once the change
    /// applies. A default destination is refused as an add's is ([`Refusal::DefaultDestination`]).
    /// A new port or UPnP restarts the torrents, which come back as they were.
    SetSetting {
        setting: Setting,
    },
    Shutdown,
}

/// What a client can ask of one torrent. A torrent's [`TorrentInfo::actions`] are those that do
/// something to it now; the others are refused, or (Pause on a paused torrent) do nothing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TorrentCommand {
    /// Stop it. A check that's running ends first.
    Pause,
    /// Start it. One that's Missing files is checked first, then downloads them again. Refused
    /// while its destination is unavailable.
    Resume,
    /// Check its files on disk against its info, keeping what's valid; it then runs or stays
    /// stopped as it's wanted. Refused while it's fetching info or its destination is unavailable.
    Check,
    /// Take it off the list, keeping its files. A torrent fetching info stops.
    Remove,
    /// Remove it and move its files to the Trash; nothing else in its destination moves. Refused
    /// while its destination is unavailable.
    Delete,
    /// Set the priority of the files at `files` (their [`File::index`]). Every file Off is fine:
    /// the torrent is then finished. While it's being checked the change is held and applied when
    /// the check ends.
    SetPriority { files: Vec<usize>, priority: Priority },
}

impl TorrentCommand {
    pub fn action(&self) -> Action {
        match self {
            TorrentCommand::Pause => Action::Pause,
            TorrentCommand::Resume => Action::Resume,
            TorrentCommand::Check => Action::Check,
            TorrentCommand::Remove => Action::Remove,
            TorrentCommand::Delete => Action::Delete,
            TorrentCommand::SetPriority { .. } => Action::SetPriority,
        }
    }
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
    Hello {
        build_id: String,
        service_version: String,
    },
    /// Pushed to a subscribed connection.
    Snapshot(Snapshot),
    Added(Added),
    Settings(Settings),
    Ok,
    /// Refused, for a reason the client may act on.
    Refused(Refusal),
    /// Failed otherwise.
    Error {
        message: String,
    },
}

/// Why the service refused a request: the facts, which clients word.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum Refusal {
    /// An add names no destination and there's no default destination.
    NoDestination,
    /// An add's destination can't be reached.
    Destination { reason: Unavailable },
    /// The default destination being set can't be reached.
    DefaultDestination { reason: Unavailable },
    /// A command for torrent `id`, or its details.
    Torrent { id: TorrentId, why: TorrentRefusal },
    /// The service is stopping: every request is refused from then on. A client waits for it to
    /// go, then starts a new one.
    Stopping,
}

/// A path from the root, as every path on disk in the protocol is: clients resolve theirs (against
/// their own working directory) before sending, so the service never sees a relative one. One
/// that isn't absolute doesn't deserialize.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AbsolutePath(PathBuf);

impl AbsolutePath {
    /// `Err` hands the path back if it isn't absolute.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, PathBuf> {
        let path = path.into();
        if path.is_absolute() { Ok(Self(path)) } else { Err(path) }
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl std::ops::Deref for AbsolutePath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl PartialEq<Path> for AbsolutePath {
    fn eq(&self, other: &Path) -> bool {
        self.0 == other
    }
}

impl PartialEq<PathBuf> for AbsolutePath {
    fn eq(&self, other: &PathBuf) -> bool {
        &self.0 == other
    }
}

impl AsRef<Path> for AbsolutePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AbsolutePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = PathBuf::deserialize(deserializer)?;
        Self::new(path).map_err(|path| serde::de::Error::custom(format!("not an absolute path: {}", path.display())))
    }
}

/// Why a torrent can't do what it's asked.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorrentRefusal {
    /// No torrent in the list has the id.
    NotInList,
    NoInfoYet,
    /// Fetching its info failed, so it never will have it.
    FetchFailed,
    /// Delete is moving its files to the Trash: nothing else is done to it until that ends.
    Deleting,
    /// Delete moved some of its files to the Trash: resuming or checking it would make them
    /// again, so only Delete (again) and Remove apply.
    PartlyDeleted,
    /// Its destination can't be reached, so it can't do `action`.
    DestinationUnavailable {
        action: Action,
        reason: Unavailable,
    },
    /// It has no file at this index.
    NoFile {
        index: usize,
    },
    /// Delete couldn't move these to the Trash: they're still where they were. Nothing moved, so
    /// it's still in the list as it was; or some did, and it's stopped there until Delete is tried
    /// again.
    NotTrashed {
        /// The torrent's name, for the sentence that says it.
        name: String,
        kept: Vec<Kept>,
    },
}

/// A file or folder of a torrent's that Delete couldn't move to the Trash.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Kept {
    pub path: AbsolutePath,
    pub why: KeptWhy,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeptWhy {
    /// Its volume has no Trash, such as some external drives and network shares: Phantom won't
    /// erase it instead.
    NoTrash,
    /// Moving it failed otherwise, e.g. it's locked; `detail` is the system's text.
    Failed { detail: String },
    /// Phantom stopped (or crashed) before it was moved.
    Interrupted,
}

/// The answer to an add.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub id: TorrentId,
    pub name: Option<String>,
    /// A torrent with the same info hash was already in the list, and `id` is that torrent's.
    pub already_existed: bool,
}

/// What clients see of the torrents at one moment: the list, and the files of those a subscriber
/// watches. Everything a client shows comes from one, so it can't contradict itself.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// Which run of the engine it's from: a later one is larger. A client orders snapshots by
    /// `(epoch, version)` and keeps the latest, so one from another connection, or from before
    /// the service restarted, never replaces a newer one.
    pub epoch: u64,
    /// Counts up with every change in the torrents, within an epoch; not with what's watched.
    pub version: u64,
    /// The torrents it was built for (its `Subscribe`'s `watching`): it has the details of those
    /// that have info, and says nothing of others'.
    pub watching: Vec<TorrentId>,
    /// Every torrent, including those fetching info, by id.
    pub torrents: Vec<TorrentInfo>,
    /// The details of the watched torrents that have info, by id.
    pub details: Vec<Details>,
    /// The names, as `settings.json` has them, of the settings the service found invalid when it
    /// read the file and reset to their defaults. Empty when the file was fine.
    pub reset: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TorrentInfo {
    pub id: TorrentId,
    pub info_hash: String,
    /// While fetching info: from the magnet's `dn=`, if there is one.
    pub name: Option<String>,
    pub status: Status,
    /// Its files' sizes and progress: `None` until its info arrives.
    pub content: Option<Content>,
    pub uploaded_bytes: u64,
    pub download_bytes_per_sec: u64,
    pub upload_bytes_per_sec: u64,
    pub live_peers: u32,
    /// The trackers it names (a magnet's `tr=`, a .torrent's announce list), for its magnet link.
    /// Absent from a service that doesn't send them.
    #[serde(default)]
    pub trackers: Vec<String>,
    /// Where it is on disk: the file itself for a single-file torrent, the torrent's folder
    /// otherwise. It may not exist yet. `None` until its info arrives.
    #[serde(default)]
    pub location: Option<AbsolutePath>,
    /// What each action would do to it now, as the service decides: clients offer those that
    /// [`Answer::Does`] something, and say why the others don't.
    pub actions: Actions,
}

/// A torrent's files, summed: what it has of the files it's getting.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Content {
    /// The torrent's Size: the files you're getting.
    pub size_bytes: u64,
    /// The torrent's Full size: all its files, whatever their priority.
    pub full_size_bytes: u64,
    pub progress_bytes: u64,
}

impl Content {
    /// Downloaded fraction, 0.0 to 1.0. With every file Off nothing is left: 1.0.
    pub fn progress(&self) -> f32 {
        if self.size_bytes == 0 { 1.0 } else { (self.progress_bytes as f64 / self.size_bytes as f64) as f32 }
    }

    /// How long what's left takes at `bytes_per_sec`; `None` with nothing left or no speed.
    pub fn eta(&self, bytes_per_sec: u64) -> Option<Duration> {
        let left = self.size_bytes.saturating_sub(self.progress_bytes);
        if left == 0 || bytes_per_sec == 0 {
            return None;
        }
        Some(Duration::from_secs(left / bytes_per_sec))
    }
}

/// What an action would do to a torrent now (#89): something, nothing (with why), or it's
/// refused (with the same reason the command would get).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    Does,
    DoesNothing { why: Why },
    Refused { why: TorrentRefusal },
}

/// Why an action would do nothing to a torrent: answered Ok, but nothing changes.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Pause on one wanted stopped.
    AlreadyStopped,
    /// Resume on one wanted running.
    AlreadyRunning,
    /// Check on one being checked, or about to be.
    AlreadyChecking,
    /// Delete on one fetching info: it has no files to delete (it's removed).
    NoFilesYet,
    /// Open or Reveal on one whose files are gone from disk.
    FilesMissing,
}

/// Each [`Action`]'s [`Answer`] for one torrent: one field per action, so a new action needs its
/// answer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Actions {
    pub pause: Answer,
    pub resume: Answer,
    pub check: Answer,
    pub set_priority: Answer,
    pub remove: Answer,
    pub delete: Answer,
    pub open: Answer,
    pub reveal: Answer,
}

impl Actions {
    /// Each action's answer, as `answer` says.
    pub fn new(answer: impl Fn(Action) -> Answer) -> Self {
        Self {
            pause: answer(Action::Pause),
            resume: answer(Action::Resume),
            check: answer(Action::Check),
            set_priority: answer(Action::SetPriority),
            remove: answer(Action::Remove),
            delete: answer(Action::Delete),
            open: answer(Action::Open),
            reveal: answer(Action::Reveal),
        }
    }

    pub fn get(&self, action: Action) -> &Answer {
        match action {
            Action::Pause => &self.pause,
            Action::Resume => &self.resume,
            Action::Check => &self.check,
            Action::SetPriority => &self.set_priority,
            Action::Remove => &self.remove,
            Action::Delete => &self.delete,
            Action::Open => &self.open,
            Action::Reveal => &self.reveal,
        }
    }

    /// Whether `action` does something now: what clients offer.
    pub fn does(&self, action: Action) -> bool {
        *self.get(action) == Answer::Does
    }
}

/// Before the service has said: nothing is known to do anything.
impl Default for Actions {
    fn default() -> Self {
        Self::new(|_| Answer::Refused { why: TorrentRefusal::NoInfoYet })
    }
}

/// What can be done to a torrent: its [`TorrentCommand`]s without their arguments, and Open and
/// Reveal, which a client does itself with the torrent's [`TorrentInfo::location`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Pause,
    Resume,
    Check,
    SetPriority,
    Remove,
    /// Remove, moving its files to the Trash.
    Delete,
    /// Open its [`TorrentInfo::location`]: the file in its default app, or the folder in Finder.
    /// Offered while its files are on disk (even unfinished ones), not while it's fetching info,
    /// missing files, or its destination is unavailable.
    Open,
    /// Show its [`TorrentInfo::location`] in Finder, selected. Offered when Open is.
    Reveal,
}

/// The service's settings, the answer to `Settings`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where an add that names no destination goes. Without one, clients ask for a destination
    /// on every add.
    pub default_destination: Option<AbsolutePath>,
    pub listen_port: ListenPort,
    /// The port peers reach it on now: `listen_port`'s, or a temporary one while a pinned port is
    /// taken by another program.
    pub listening_on: NonZeroU16,
    /// Ask the router to forward the port (UPnP).
    pub upnp: bool,
    /// The names, as `settings.json` has them, of the settings the service found invalid when it
    /// read the file and reset to their defaults. Empty when the file was fine. Also in every
    /// [`Snapshot`].
    #[serde(default)]
    pub reset: Vec<String>,
}

/// The port for incoming peer connections: picked once at random and kept, so a forward set up by
/// hand on a router keeps working, or pinned by the user and never changed automatically.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListenPort {
    /// Picked by the service; replaced only when another program has taken it.
    Picked(NonZeroU16),
    /// Set by the user; while another program has it, the service listens on a temporary port.
    Pinned(NonZeroU16),
}

impl ListenPort {
    pub fn port(self) -> NonZeroU16 {
        match self {
            ListenPort::Picked(port) | ListenPort::Pinned(port) => port,
        }
    }
}

/// One setting with its new value; `None` unsets it, which is its default.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "name", content = "value", rename_all = "snake_case")]
pub enum Setting {
    /// A folder that exists; `None`: no default, so every add asks.
    DefaultDestination(Option<AbsolutePath>),
    /// Pin the port; `None`: the service picks it, keeping a pinned one (as picked) until another
    /// program takes it.
    ListenPort(Option<NonZeroU16>),
    /// `None`: on.
    Upnp(Option<bool>),
}

/// A torrent's files and where they are, in a [`Snapshot`] for a torrent watched.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub id: TorrentId,
    pub files: Vec<File>,
    /// Where the torrent is on disk: the file itself for a single-file torrent, the torrent's
    /// folder otherwise. It may not exist yet.
    pub location: AbsolutePath,
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
        reason: Unavailable,
    },
    /// Its files are being moved to a new destination.
    Moving,
    /// Delete is moving its files to the Trash; once they're all there it leaves the list.
    Deleting,
    Error {
        failure: TorrentFailure,
    },
}

/// What went wrong with a torrent: the case, which clients word, and librqbit's own text, kept as
/// it is (#100).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorrentFailure {
    /// Fetching its info failed, so it never had any.
    FetchFailed { detail: String },
    /// Adding it to librqbit again, as a check does, failed after taking it out.
    AddAgainFailed { detail: String },
    /// librqbit stopped it with this error.
    Librqbit { detail: String },
    /// Delete moved some of its files to the Trash, but not these: it's stopped until Delete is
    /// tried again.
    NotTrashed { kept: Vec<Kept> },
}

/// Why a destination can't be reached.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unavailable {
    /// A drive under `/Volumes` isn't mounted.
    DriveNotConnected {
        name: String,
    },
    FolderMissing {
        path: AbsolutePath,
    },
    NoPermission {
        path: AbsolutePath,
    },
    /// Something other than a folder is at `path`.
    NotAFolder {
        path: AbsolutePath,
    },
    /// It can't be opened for another reason, e.g. a disk error.
    CantOpen {
        path: AbsolutePath,
    },
}

impl TorrentInfo {
    /// What clients call it: its name, else its info hash.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.info_hash)
    }

    /// How long what's left takes at its speed now; `None` without info, with nothing left, or
    /// not downloading.
    pub fn eta(&self) -> Option<Duration> {
        self.content?.eta(self.download_bytes_per_sec)
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
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("phantom")
}

/// Where the service listens and clients find it: part of the contract, like the messages.
pub fn socket_path() -> PathBuf {
    data_dir().join("service.sock")
}

/// Held by the service for its whole life, from before it binds the socket until after it has
/// stopped: while it's held, a service is there, starting, serving or stopping.
pub fn lock_path() -> PathBuf {
    data_dir().join("service.lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_id_follows_the_bytes_not_the_path() {
        let dir = std::env::temp_dir().join(format!("phantom-exe-id-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(dir.join(name), bytes).unwrap();
            exe_id(&dir.join(name)).unwrap()
        };
        assert_eq!(write("a", b"same"), write("b", b"same"));
        assert_ne!(write("a", b"same"), write("c", b"other"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "timing of a release service, run by hand"]
    fn exe_id_of_the_release_service_is_quick() {
        let start = std::time::Instant::now();
        exe_id(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/release/phantom-service")).unwrap();
        println!("exe_id: {:?}", start.elapsed());
    }

    #[test]
    fn an_unavailable_destination_travels_as_its_case() {
        let status = Status::DestinationUnavailable { reason: Unavailable::DriveNotConnected { name: "MyDisk".into() } };
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"destination_unavailable":{"reason":{"kind":"drive_not_connected","name":"MyDisk"}}}"#);
        assert_eq!(serde_json::from_str::<Status>(&json).unwrap(), status);
    }

    #[test]
    fn a_relative_path_does_not_deserialize() {
        assert_eq!(serde_json::from_str::<AbsolutePath>(r#""/Users/me""#).unwrap(), AbsolutePath::new("/Users/me").unwrap());
        assert!(serde_json::from_str::<AbsolutePath>(r#""Films""#).is_err());
    }

    #[test]
    fn a_refusal_travels_as_its_facts() {
        let refused = Response::Refused(Refusal::Torrent { id: 3, why: TorrentRefusal::NoFile { index: 5 } });
        let json = serde_json::to_string(&refused).unwrap();
        assert_eq!(json, r#"{"type":"refused","refusal":"torrent","id":3,"why":{"kind":"no_file","index":5}}"#);
        assert!(matches!(serde_json::from_str(&json).unwrap(), Response::Refused(Refusal::Torrent { id: 3, .. })));
    }
}
