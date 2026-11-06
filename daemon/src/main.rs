//! Torrent daemon: runs a librqbit session and serves `proto` requests on a Unix socket.
//! Exits `IDLE_GRACE` after the last client disconnects.

use std::fs::{self, File, Permissions};
use std::io::IsTerminal;
use std::net::Ipv6Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use proto::{AddSource, PROTOCOL_VERSION, Request, Response, TorrentId, TorrentInfo, TorrentState};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{Notify, watch};
use tracing::{info, warn};

mod pending;
mod settings;

use pending::PendingAdds;
use settings::Settings;

const IDLE_GRACE: Duration = Duration::from_secs(10);

struct Daemon {
    session: Arc<Session>,
    pending: Arc<PendingAdds>,
    clients: watch::Sender<usize>,
    shutdown: Notify,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,librqbit=warn,librqbit_dht=warn".into()),
        )
        // No color codes when logging to daemon.log.
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let dir = proto::data_dir();
    fs::create_dir_all(&dir)?;
    // Only our user may reach the socket inside.
    fs::set_permissions(&dir, Permissions::from_mode(0o700))?;

    // Held for the daemon's whole life: guarantees a single daemon per data dir.
    let Some(_lock) = acquire_lock(dir.join("daemon.lock")).await? else {
        info!("another daemon is running, exiting");
        return Ok(());
    };

    let mut settings = Settings::load(&dir)?;
    let port = settings.resolve_listen_port(&dir)?;
    info!(port, upnp = settings.upnp, "peer listener");

    let session = Session::new_with_opts(
        download_dir(),
        SessionOptions {
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json { folder: Some(dir.join("session")) }),
            // librqbit keeps DHT state (including its port) in a folder shared by every librqbit
            // program; keep ours with our data so two daemons, or rqbit, never fight over it.
            dht: Some(DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(dir.join("dht.json")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            listen: Some(ListenerOptions {
                listen_addr: (Ipv6Addr::UNSPECIFIED, port).into(),
                enable_upnp_port_forwarding: settings.upnp,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .context("failed to start torrent session")?;

    let socket = proto::socket_path();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), pid = std::process::id(), "daemon listening");

    let daemon = Arc::new(Daemon {
        session,
        pending: Arc::default(),
        clients: watch::Sender::new(0),
        shutdown: Notify::new(),
    });

    let idle = idle_timeout(daemon.clients.subscribe());
    tokio::pin!(idle);
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => { tokio::spawn(serve(stream, daemon.clone())); }
                Err(e) => warn!(error = %e, "accept failed"),
            },
            _ = &mut idle => { info!("no clients for {IDLE_GRACE:?}, shutting down"); break; }
            _ = daemon.shutdown.notified() => { info!("shutdown requested"); break; }
            _ = sigterm.recv() => { info!("SIGTERM, shutting down"); break; }
            _ = sigint.recv() => { info!("SIGINT, shutting down"); break; }
        }
    }

    // Remove the socket first so new clients start a fresh daemon instead of reaching this one.
    let _ = fs::remove_file(&socket);
    daemon.session.stop().await;
    info!("stopped");
    Ok(())
}

fn download_dir() -> PathBuf {
    std::env::var_os("PHANTOM_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME is not set")).join("Downloads"))
}

/// Waits briefly for the lock, since a previous daemon may still be shutting down.
async fn acquire_lock(path: PathBuf) -> Result<Option<File>> {
    let file = File::create(path)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if file.try_lock().is_ok() {
            return Ok(Some(file));
        }
        if Instant::now() > deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Resolves once the client count has stayed at zero for `IDLE_GRACE`.
async fn idle_timeout(mut clients: watch::Receiver<usize>) {
    loop {
        let _ = clients.wait_for(|n| *n == 0).await;
        if tokio::time::timeout(IDLE_GRACE, clients.wait_for(|n| *n > 0)).await.is_err() {
            return;
        }
    }
}

async fn serve(stream: UnixStream, daemon: Arc<Daemon>) {
    daemon.clients.send_modify(|n| *n += 1);
    if let Err(e) = serve_requests(stream, &daemon).await {
        warn!(error = %e, "client connection failed");
    }
    daemon.clients.send_modify(|n| *n -= 1);
}

async fn serve_requests(stream: UnixStream, daemon: &Daemon) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Shutdown) => {
                daemon.shutdown.notify_one();
                return Ok(());
            }
            Ok(request) => daemon
                .handle(request)
                .await
                .unwrap_or_else(|e| Response::Error { message: format!("{e:#}") }),
            Err(e) => Response::Error { message: format!("bad request: {e}") },
        };
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

impl Daemon {
    async fn handle(&self, request: Request) -> Result<Response> {
        Ok(match request {
            Request::Hello { .. } => Response::Hello {
                protocol: PROTOCOL_VERSION,
                daemon_version: env!("CARGO_PKG_VERSION").into(),
            },
            Request::List => Response::Torrents {
                torrents: self
                    .session
                    .with_torrents(|torrents| torrents.map(|(id, t)| torrent_info(id as TorrentId, t)).collect()),
                pending: self.pending.list(),
            },
            Request::Add { source, paused } => {
                let pending_id = self.pending.begin(&source);
                let add = match source {
                    AddSource::Url(url) => AddTorrent::from_url(url),
                    AddSource::TorrentFile(bytes) => AddTorrent::from_bytes(bytes),
                };
                // `overwrite` lets a re-added torrent resume into files that already exist.
                let opts = AddTorrentOptions { paused, overwrite: true, ..Default::default() };
                // Its own task, so `CancelAdd` can abort it and so it finishes even if this
                // client disconnects (the torrent then just shows up in the list).
                let session = self.session.clone();
                let pending = self.pending.clone();
                let task = tokio::spawn(async move {
                    let added = session.add_torrent(add, Some(opts)).await;
                    pending.finish(pending_id);
                    match added? {
                        AddTorrentResponse::Added(id, t) => Ok((id, t.name(), false)),
                        AddTorrentResponse::AlreadyManaged(id, t) => Ok((id, t.name(), true)),
                        AddTorrentResponse::ListOnly(_) => bail!("unexpected list-only response"),
                    }
                });
                self.pending.set_abort(pending_id, task.abort_handle());
                match task.await {
                    Ok(added) => {
                        let (id, name, already_existed) = added?;
                        Response::Added { id: id as TorrentId, name, already_existed }
                    }
                    Err(e) if e.is_cancelled() => bail!("cancelled"),
                    Err(e) => bail!("add failed: {e}"),
                }
            }
            Request::CancelAdd { id } => {
                if !self.pending.cancel(id) {
                    bail!("no pending add with id {id}");
                }
                Response::Ok
            }
            Request::Pause { id } => {
                self.session.pause(&self.torrent(id)?).await?;
                Response::Ok
            }
            Request::Resume { id } => {
                self.session.unpause(&self.torrent(id)?).await?;
                Response::Ok
            }
            Request::Remove { id, delete_files } => {
                self.session.delete(TorrentIdOrHash::Id(id as usize), delete_files).await?;
                Response::Ok
            }
            Request::Shutdown => unreachable!("handled by serve_requests"),
        })
    }

    fn torrent(&self, id: TorrentId) -> Result<Arc<ManagedTorrent>> {
        self.session
            .get(TorrentIdOrHash::Id(id as usize))
            .with_context(|| format!("no torrent with id {id}"))
    }
}

fn torrent_info(id: TorrentId, torrent: &ManagedTorrent) -> TorrentInfo {
    let stats = torrent.stats();
    let live = stats.live.as_ref();
    TorrentInfo {
        id,
        info_hash: torrent.info_hash().as_string(),
        name: torrent.name(),
        state: match stats.state {
            TorrentStatsState::Initializing { .. } => TorrentState::Initializing,
            TorrentStatsState::Live => TorrentState::Live,
            TorrentStatsState::Paused => TorrentState::Paused,
            TorrentStatsState::Error => TorrentState::Error,
        },
        finished: stats.finished,
        error: stats.error,
        progress_bytes: stats.progress_bytes,
        total_bytes: stats.total_bytes,
        uploaded_bytes: stats.uploaded_bytes,
        download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
        upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
        live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
    }
}
