//! Reaching the service from a client: [`Client`] connects (starting the service if needed),
//! checks the protocol version and has one blocking method per request. [`format`] renders what
//! it returns for people.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use proto::{AddSource, Added, PROTOCOL_VERSION, Request, Response, TorrentId, TorrentInfo, data_dir, socket_path};

pub mod format;

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
    /// Connects to a running service. Fails if none is running or its protocol differs.
    pub fn connect() -> Result<Self> {
        let mut client = Self::connect_raw()?;
        let (protocol, _) = client.hello()?;
        if protocol != PROTOCOL_VERSION {
            bail!("service speaks protocol {protocol}, we speak {PROTOCOL_VERSION}");
        }
        Ok(client)
    }

    /// Connects to the service, starting `service_exe` if none is running. A service
    /// with a different protocol version is shut down and replaced.
    pub fn connect_or_spawn(service_exe: &Path) -> Result<Self> {
        if let Ok(mut client) = Self::connect_raw() {
            let (protocol, _) = client.hello()?;
            if protocol == PROTOCOL_VERSION {
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
            client = Self::connect().ok();
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

    fn hello(&mut self) -> Result<(u32, String)> {
        match self.call(&Request::Hello { protocol: PROTOCOL_VERSION })? {
            Response::Hello { protocol, service_version } => {
                self.service_version = service_version.clone();
                Ok((protocol, service_version))
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

    /// A magnet answers at once; a .torrent URL once it's downloaded.
    pub fn add(&mut self, source: AddSource, paused: bool) -> Result<Added> {
        match self.call(&Request::Add { source, paused })? {
            Response::Added(added) => Ok(added),
            other => bail!("unexpected response: {other:?}"),
        }
    }

    pub fn pause(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Pause { id })
    }

    pub fn resume(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Resume { id })
    }

    /// Takes the torrent off the list, keeping its files.
    pub fn remove(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Remove { id })
    }

    /// Removes the torrent and deletes its files.
    pub fn delete(&mut self, id: TorrentId) -> Result<()> {
        self.call_ok(&Request::Delete { id })
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
