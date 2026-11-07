//! The torrents, in Phantom's terms: [`Engine`] starts a session, adds, lists, pauses, resumes,
//! removes and stops them. The only code that knows librqbit; the server hands it requests and
//! gets back `proto` types.

mod pending;

use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use proto::{AddSource, Listing, PendingId, TorrentId, TorrentInfo, TorrentState};

use pending::PendingAdds;

/// What the engine needs from the service's settings and environment.
pub struct Config {
    /// The data dir: the session's state and DHT state go here.
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    pub listen_port: u16,
    pub upnp: bool,
}

pub struct Engine {
    session: Arc<Session>,
    pending: Arc<PendingAdds>,
}

impl Engine {
    /// Starts the session, resuming the torrents saved in the data dir.
    pub async fn start(config: Config) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp } = config;
        let session = Session::new_with_opts(
            download_dir,
            SessionOptions {
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json { folder: Some(data_dir.join("session")) }),
                // librqbit keeps DHT state (including its port) in a folder shared by every librqbit
                // program; keep ours with our data so two services, or rqbit, never fight over it.
                dht: Some(DhtSessionConfig {
                    persistence: Some(DhtPersistenceConfig {
                        config_filename: Some(data_dir.join("dht.json")),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                listen: Some(ListenerOptions {
                    listen_addr: (Ipv6Addr::UNSPECIFIED, listen_port).into(),
                    enable_upnp_port_forwarding: upnp,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .context("failed to start torrent session")?;
        Ok(Self { session, pending: Arc::default() })
    }

    pub fn listing(&self) -> Listing {
        Listing {
            torrents: self
                .session
                .with_torrents(|torrents| torrents.map(|(id, t)| torrent_info(id as TorrentId, t)).collect()),
            pending: self.pending.list(),
        }
    }

    /// Returns the torrent's id, its name if known, and whether it was already in the list.
    /// For magnets this waits until metadata is resolved; [`cancel_add`](Self::cancel_add)
    /// makes it fail with "cancelled".
    pub async fn add(&self, source: AddSource, paused: bool) -> Result<(TorrentId, Option<String>, bool)> {
        let pending_id = self.pending.begin(&source);
        let add = match source {
            AddSource::Url(url) => AddTorrent::from_url(url),
            AddSource::TorrentFile(bytes) => AddTorrent::from_bytes(bytes),
        };
        // `overwrite` lets a re-added torrent resume into files that already exist.
        let opts = AddTorrentOptions { paused, overwrite: true, ..Default::default() };
        // Its own task, so `cancel_add` can abort it and so it finishes even if the client
        // that asked disconnects (the torrent then just shows up in the list).
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
                Ok((id as TorrentId, name, already_existed))
            }
            Err(e) if e.is_cancelled() => bail!("cancelled"),
            Err(e) => bail!("add failed: {e}"),
        }
    }

    pub fn cancel_add(&self, id: PendingId) -> Result<()> {
        if !self.pending.cancel(id) {
            bail!("no pending add with id {id}");
        }
        Ok(())
    }

    pub async fn pause(&self, id: TorrentId) -> Result<()> {
        self.session.pause(&self.torrent(id)?).await
    }

    pub async fn resume(&self, id: TorrentId) -> Result<()> {
        self.session.unpause(&self.torrent(id)?).await
    }

    pub async fn remove(&self, id: TorrentId, delete_files: bool) -> Result<()> {
        self.session.delete(TorrentIdOrHash::Id(id as usize), delete_files).await
    }

    /// Saves the torrents' state and stops them.
    pub async fn stop(&self) {
        self.session.stop().await;
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
