//! Reaching the service from a client: [`Client`] connects (starting the service if needed),
//! checks the service is the build it would start and has one blocking method per request. A refusal comes back as
//! [`Refused`]. [`display`] renders what it returns for people, the service's typed reasons too.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use proto::{
    AbsolutePath, AddSource, Added, Details, Refusal, Request, Response, Setting, Settings, TorrentCommand, TorrentId, TorrentInfo, data_dir, socket_path,
};

pub mod display;

/// Why the link field's text can't be added; [`display::link_problem`] words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkProblem {
    NotALink,
    /// A magnet link whose info hash is missing or not whole.
    NoInfoHash,
}

/// The service refused a request; shown in [`display`]'s words. Callers that can act on the
/// reason (e.g. ask for a destination) downcast to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub Refusal);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&display::refusal(&self.0))
    }
}

impl std::error::Error for Refused {}

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
    /// that isn't `service_exe`'s build is shut down and replaced.
    pub fn connect_or_spawn(service_exe: &Path) -> Result<Self> {
        if let Ok(mut client) = Self::connect_raw() {
            if client.hello()? == our_build_id(service_exe)? {
                return Ok(client);
            }
            client.send(&Request::Shutdown)?;
            drop(client);
            wait_until(Duration::from_secs(10), || Self::connect_raw().is_err())
                .context("old service did not shut down")?;
        }

        spawn_service(service_exe)?;
        let mut client = None;
        wait_until(Duration::from_secs(10), || {
            client = Self::connect_to_build_of(service_exe).ok();
            client.is_some()
        })
        .context("service did not start; see service.log in the data dir")?;
        Ok(client.unwrap())
    }

    fn connect_raw() -> Result<Self> {
        let stream = UnixStream::connect(socket_path())?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            service_version: String::new(),
        })
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

    pub fn list(&mut self) -> Result<Vec<TorrentInfo>> {
        match self.call(&Request::List)? {
            Response::Torrents { torrents } => Ok(torrents),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn add(&mut self, source: AddSource, paused: bool, destination: Option<AbsolutePath>) -> Result<Added> {
        match self.call(&Request::Add { source, paused, destination })? {
            Response::Added(added) => Ok(added),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn command(&mut self, id: TorrentId, command: TorrentCommand) -> Result<()> {
        self.call_ok(&Request::Torrent { id, command })
    }

    pub fn details(&mut self, id: TorrentId) -> Result<Details> {
        match self.call(&Request::Details { id })? {
            Response::Details(details) => Ok(details),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn settings(&mut self) -> Result<Settings> {
        match self.call(&Request::Settings)? {
            Response::Settings(settings) => Ok(settings),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn set_setting(&mut self, setting: Setting) -> Result<Settings> {
        match self.call(&Request::SetSetting { setting })? {
            Response::Settings(settings) => Ok(settings),
            other => bail!("unexpected response: {other:?}"),
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
            bail!("service closed the connection");
        }
        match serde_json::from_str(&line)? {
            Response::Refused(refusal) => Err(Refused(refusal).into()),
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
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir().join("service.log"))?;
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
