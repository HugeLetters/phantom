//! Reaching the service from a client: [`Client`] connects (starting the service if needed),
//! checks the service is the build it would start and has one blocking method per request. Each answers
//! with a [`Failure`] when it fails: refused, with the reason, or an error. [`display`] renders what it returns for people, the service's typed reasons too.
//!
//! Connecting is a [`machine`], shared by every client: the CLI runs it until it's connected or
//! lost ([`Client::connect_or_spawn`]); the app's link thread runs it for as long as the app does,
//! through the same [`Socket`].

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use machine::{Do, Observed, State, World};
use proto::{
    AbsolutePath, AddSource, Added, Details, Refusal, Request, Response, Setting, Settings, TorrentCommand, TorrentId, TorrentInfo,
    data_dir, socket_path,
};

pub mod display;
pub mod machine;

/// Why the link field's text can't be added; [`display::link_problem`] words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkProblem {
    NotALink,
    /// A magnet link whose info hash is missing or not whole.
    NoInfoHash,
}

/// Why a request didn't do what it asks: the service refused it, for a reason a caller can act on
/// (e.g. ask for a destination), or it failed. Shown in [`display`]'s words.
#[derive(Debug)]
pub enum Failure {
    Refused(Refusal),
    Error(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Error(error)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Error(error.into())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self::Error(error.into())
    }
}

/// An error shows as the error itself, its causes too (`{:#}`, or through `source`).
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => f.write_str(&display::refusal(refusal)),
            Self::Error(e) => std::fmt::Display::fmt(e, f),
        }
    }
}

impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused(_) => None,
            Self::Error(e) => e.source(),
        }
    }
}

/// `path` from the root: a relative one against the current directory, as the person giving it
/// means. The protocol only carries absolute paths.
pub fn absolute(path: &Path) -> Result<AbsolutePath> {
    let path = std::path::absolute(path).with_context(|| format!("can't resolve {}", path.display()))?;
    Ok(AbsolutePath::new(path).expect("std::path::absolute"))
}

/// `phantom-service` next to the running executable, where every client expects it.
pub fn sibling_service_exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("phantom-service"))
}

/// Blocking client. Each open `Client` keeps the service alive.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    pub service_version: String,
}

impl Client {
    /// Connects to a running service. Fails if none is running or it isn't the sibling
    /// `phantom-service` build.
    pub fn connect() -> Result<Self> {
        Self::connect_to_build_of(&sibling_service_exe()?)
    }

    fn connect_to_build_of(service_exe: &Path) -> Result<Self> {
        let mut client = Self::connect_raw()?;
        let build_id = client.hello()?;
        let ours = our_build_id(service_exe)?;
        if build_id != ours {
            bail!("service is build {build_id}, we start {ours}");
        }
        Ok(client)
    }

    /// Connects to the service, starting `service_exe` if none is running. A service
    /// that isn't `service_exe`'s build, or is stopping, is waited for to go and replaced.
    pub fn connect_or_spawn(service_exe: &Path) -> Result<Self> {
        let mut socket = Socket::new(service_exe);
        match machine::settle(State::Connecting, &mut socket) {
            State::Lost { why, .. } => bail!("{}", display::lost(&why)),
            _ => Ok(socket.take().expect("connected through a hello")),
        }
    }

    /// Another connection to the service a link reached, as it is: no hello, no starting one.
    pub fn open() -> Result<Self> {
        Self::connect_raw()
    }

    fn connect_raw() -> Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        Ok(Self { reader: BufReader::new(stream.try_clone()?), writer: stream, service_version: String::new() })
    }

    /// The service's build id.
    fn hello(&mut self) -> Result<String> {
        match self.call(&Request::Hello)? {
            Response::Hello { build_id, service_version } => {
                self.service_version = service_version;
                Ok(build_id)
            }
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn list(&mut self) -> Result<Vec<TorrentInfo>, Failure> {
        match self.call(&Request::List)? {
            Response::Torrents { torrents } => Ok(torrents),
            other => Err(unexpected(other)),
        }
    }

    pub fn add(&mut self, source: AddSource, paused: bool, destination: Option<AbsolutePath>) -> Result<Added, Failure> {
        match self.call(&Request::Add { source, paused, destination })? {
            Response::Added(added) => Ok(added),
            other => Err(unexpected(other)),
        }
    }

    pub fn command(&mut self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        self.call_ok(&Request::Torrent { id, command })
    }

    pub fn details(&mut self, id: TorrentId) -> Result<Details, Failure> {
        match self.call(&Request::Details { id })? {
            Response::Details(details) => Ok(details),
            other => Err(unexpected(other)),
        }
    }

    pub fn settings(&mut self) -> Result<Settings, Failure> {
        match self.call(&Request::Settings)? {
            Response::Settings(settings) => Ok(settings),
            other => Err(unexpected(other)),
        }
    }

    pub fn set_setting(&mut self, setting: Setting) -> Result<Settings, Failure> {
        match self.call(&Request::SetSetting { setting })? {
            Response::Settings(settings) => Ok(settings),
            other => Err(unexpected(other)),
        }
    }

    /// Stops the service regardless of other connected clients.
    pub fn shutdown(&mut self) -> Result<()> {
        self.send(&Request::Shutdown)
    }

    /// Blocks until the service closes the connection.
    pub fn wait_closed(&mut self) -> Result<()> {
        let mut buf = String::new();
        while self.reader.read_line(&mut buf)? > 0 {
            buf.clear();
        }
        Ok(())
    }

    fn call_ok(&mut self, req: &Request) -> Result<(), Failure> {
        match self.call(req)? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    fn call(&mut self, req: &Request) -> Result<Response, Failure> {
        self.send(req)?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(anyhow!("service closed the connection").into());
        }
        match serde_json::from_str(&line)? {
            Response::Refused(refusal) => Err(Failure::Refused(refusal)),
            Response::Error { message } => Err(anyhow!(message).into()),
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

/// The id of the service executable we'd start, hashed once per process (every client starts the
/// same one).
fn our_build_id(service_exe: &Path) -> Result<&'static str> {
    static ID: OnceLock<String> = OnceLock::new();
    if let Some(id) = ID.get() {
        return Ok(id);
    }
    let id = proto::exe_id(service_exe).with_context(|| format!("can't read {}", service_exe.display()))?;
    Ok(ID.get_or_init(|| id))
}

fn spawn_service(service_exe: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir())?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(data_dir().join("service.log"))?;
    Command::new(service_exe)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        // Own process group, so Ctrl+C in the client's terminal doesn't reach the service.
        .process_group(0)
        .spawn()
        .with_context(|| format!("failed to start {}", service_exe.display()))?;
    Ok(())
}

/// The socket and the real clock, which a link reaches the service through.
pub struct Socket {
    service_exe: PathBuf,
    /// The connection that answered the last hello.
    client: Option<Client>,
}

impl Socket {
    /// For a link that starts `service_exe`.
    pub fn new(service_exe: &Path) -> Self {
        Self { service_exe: service_exe.to_owned(), client: None }
    }

    /// The connection a hello from our build came through, once connected.
    pub fn take(&mut self) -> Option<Client> {
        self.client.take()
    }

    fn hello(&mut self) -> Observed {
        self.client = None;
        let ours = match our_build_id(&self.service_exe) {
            Ok(ours) => ours,
            Err(e) => return Observed::Failed(format!("{e:#}")),
        };
        let Ok(mut client) = Client::connect_raw() else {
            return if data_dir_held() { Observed::Going } else { Observed::Gone };
        };
        match client.hello() {
            Ok(build_id) => {
                self.client = Some(client);
                Observed::Hello { ours: build_id == ours }
            }
            // Refused as stopping, or closed unanswered: on its way out.
            Err(_) => Observed::Going,
        }
    }
}

impl World for Socket {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn run(&mut self, act: &Do) -> Observed {
        match act {
            Do::Hello => self.hello(),
            Do::Shutdown => {
                if let Some(mut client) = self.client.take() {
                    // However it ends, the link looks again.
                    let _ = client.reader.get_ref().set_read_timeout(Some(machine::REPLACE_WITHIN));
                    let _ = client.shutdown().and_then(|()| client.wait_closed());
                }
                Observed::Closed
            }
            Do::Spawn => match spawn_service(&self.service_exe) {
                Ok(()) => Observed::Spawned,
                Err(e) => Observed::Failed(format!("{e:#}")),
            },
            Do::Wait(until) => {
                std::thread::sleep(until.saturating_duration_since(Instant::now()));
                Observed::Tick
            }
        }
    }
}

/// Whether a service holds the data dir's lock: starting, serving or stopping.
fn data_dir_held() -> bool {
    File::open(proto::lock_path()).is_ok_and(|lock| lock.try_lock().is_err())
}

/// An answer that isn't the one the request has.
fn unexpected(response: Response) -> Failure {
    anyhow!("unexpected response: {response:?}").into()
}
