//! Shared contract between the daemon and its clients.
//!
//! Wire format: one JSON object per line over a Unix socket. Clients should use
//! [`Client`] rather than speaking the wire format directly.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub mod format;

/// Bump on any incompatible change to [`Request`], [`Response`] or the types they carry.
pub const PROTOCOL_VERSION: u32 = 2;

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
    Hello { protocol: u32, daemon_version: String },
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

/// An add the daemon is still working on, typically a magnet waiting for metadata from peers.
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
/// `PHANTOM_DATA_DIR` overrides it, e.g. to run an isolated daemon for testing.
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

pub fn socket_path() -> PathBuf {
    data_dir().join("daemon.sock")
}

/// `phantom-daemon` next to the running executable, where every client expects it.
pub fn sibling_daemon_exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("phantom-daemon"))
}

/// Blocking client. Each open `Client` keeps the daemon alive.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    pub daemon_version: String,
}

impl Client {
    /// Connects to a running daemon. Fails if none is running or its protocol differs.
    pub fn connect() -> Result<Self> {
        let mut client = Self::connect_raw()?;
        let (protocol, _) = client.hello()?;
        if protocol != PROTOCOL_VERSION {
            bail!("daemon speaks protocol {protocol}, we speak {PROTOCOL_VERSION}");
        }
        Ok(client)
    }

    /// Connects to the daemon, starting `daemon_exe` if none is running. A daemon
    /// with a different protocol version is shut down and replaced.
    pub fn connect_or_spawn(daemon_exe: &Path) -> Result<Self> {
        if let Ok(mut client) = Self::connect_raw() {
            let (protocol, _) = client.hello()?;
            if protocol == PROTOCOL_VERSION {
                return Ok(client);
            }
            client.send(&Request::Shutdown)?;
            drop(client);
            wait_until(Duration::from_secs(10), || Self::connect_raw().is_err())
                .context("old daemon did not shut down")?;
        }

        spawn_daemon(daemon_exe)?;
        let mut client = None;
        wait_until(Duration::from_secs(10), || {
            client = Self::connect().ok();
            client.is_some()
        })
        .context("daemon did not start; see daemon.log in the data dir")?;
        Ok(client.unwrap())
    }

    fn connect_raw() -> Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            daemon_version: String::new(),
        })
    }

    fn hello(&mut self) -> Result<(u32, String)> {
        match self.call(&Request::Hello { protocol: PROTOCOL_VERSION })? {
            Response::Hello { protocol, daemon_version } => {
                self.daemon_version = daemon_version.clone();
                Ok((protocol, daemon_version))
            }
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn list(&mut self) -> Result<Listing> {
        match self.call(&Request::List)? {
            Response::Torrents { torrents, pending } => Ok(Listing { torrents, pending }),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    /// For magnets this waits until metadata is resolved, which can take a while.
    pub fn add(&mut self, source: AddSource, paused: bool) -> Result<(TorrentId, Option<String>, bool)> {
        match self.call(&Request::Add { source, paused })? {
            Response::Added { id, name, already_existed } => Ok((id, name, already_existed)),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn pause(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Pause { id })
    }

    pub fn resume(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Resume { id })
    }

    pub fn remove(&mut self, id: TorrentId, delete_files: bool) -> Result<()> {
        self.call_ok(&Request::Remove { id, delete_files })
    }

    pub fn cancel_add(&mut self, id: PendingId) -> Result<()> {
        self.call_ok(&Request::CancelAdd { id })
    }

    /// Stops the daemon regardless of other connected clients.
    pub fn shutdown(&mut self) -> Result<()> {
        self.send(&Request::Shutdown)
    }

    /// Blocks until the daemon closes the connection.
    pub fn wait_closed(&mut self) -> Result<()> {
        let mut buf = String::new();
        while self.reader.read_line(&mut buf)? > 0 {
            buf.clear();
        }
        Ok(())
    }

    fn call_ok(&mut self, req: &Request) -> Result<()> {
        match self.call(req)? {
            Response::Ok => Ok(()),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    fn call(&mut self, req: &Request) -> Result<Response> {
        self.send(req)?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            bail!("daemon closed the connection");
        }
        match serde_json::from_str(&line)? {
            Response::Error { message } => bail!(message),
            resp => Ok(resp),
        }
    }

    fn send(&mut self, req: &Request) -> Result<()> {
        let mut line = serde_json::to_vec(req)?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        Ok(())
    }
}

fn spawn_daemon(daemon_exe: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir().join("daemon.log"))?;
    Command::new(daemon_exe)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        // Own process group, so Ctrl+C in the client's terminal doesn't reach the daemon.
        .process_group(0)
        .spawn()
        .with_context(|| format!("failed to start {}", daemon_exe.display()))?;
    Ok(())
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while !done() {
        if Instant::now() > deadline {
            bail!("timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
