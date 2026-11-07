//! The torrents, in Phantom's terms: [`Engine`] starts a session, adds, lists, pauses, resumes,
//! removes, deletes and stops them. Owns torrent ids and de-duplicates adds by info hash. The
//! only code that knows librqbit; the server hands it requests and gets back `proto` types.

mod ids;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Magnet, ManagedTorrent,
    Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState, torrent_from_bytes,
};
use proto::{AddSource, Added, Status, TorrentId, TorrentInfo};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::warn;

use ids::Ids;

/// What the engine needs from the service's settings and environment.
pub struct Config {
    /// The data dir: the session's state, DHT state and torrent ids go here.
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    pub listen_port: u16,
    pub upnp: bool,
    /// No DHT, no local peer discovery and no listener: only trackers a torrent names find
    /// peers. For tests, which must not reach the internet.
    pub offline: bool,
}

// Why the engine owns torrent ids: in the user's terms a torrent exists from the moment it's
// added, even while a magnet's info is still being fetched, and it must keep one id throughout
// so clients can show, select and remove it. librqbit only numbers a torrent once its info
// arrives, and renumbers the torrents it restores, so its ids can't be ours.
//
// So every torrent gets a Phantom id when it's added. Until librqbit has it, the torrent is a
// `Fetching` entry here; after that it's librqbit's, found by info hash. `ids.json` keeps each
// info hash's id, so ids survive restarts. A .torrent URL has no info hash until the file is
// downloaded: its entry holds just the id, and the hash is recorded (or found to be a duplicate)
// once librqbit has the torrent.
//
// Torrents still fetching info don't survive a restart: librqbit only saves a torrent once its
// info arrives. Their ids are dropped when the service starts again.
pub struct Engine {
    session: Arc<Session>,
    shared: Arc<Shared>,
}

struct Shared {
    data_dir: PathBuf,
    state: Mutex<State>,
}

struct State {
    ids: Ids,
    /// Torrents librqbit doesn't have yet.
    fetching: BTreeMap<TorrentId, Fetching>,
}

struct Fetching {
    info_hash: Option<String>,
    name: Option<String>,
    /// Fetching failed. The torrent stays in the list with status Error until it's removed.
    error: Option<String>,
    /// The client got its answer when the torrent was added, so a failure only shows in the list.
    answered: bool,
    /// Adds the torrent to librqbit; `None` once it failed.
    task: Option<JoinHandle<()>>,
}

/// An add, checked and ready to hand to librqbit.
struct Prepared {
    add: AddTorrent<'static>,
    info_hash: Option<String>,
    name: Option<String>,
    /// A magnet answers at once; .torrent bytes are added quickly and a URL has to be
    /// downloaded before it can be checked for duplicates, so those answer when librqbit has them.
    answer_at_once: bool,
}

impl Engine {
    /// Starts the session, resuming the torrents saved in the data dir with the ids they had.
    pub async fn start(config: Config) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp, offline } = config;
        let mut ids = Ids::load(&data_dir)?;
        let session = Session::new_with_opts(
            download_dir,
            SessionOptions {
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json { folder: Some(data_dir.join("session")) }),
                // librqbit keeps DHT state (including its port) in a folder shared by every librqbit
                // program; keep ours with our data so two services, or rqbit, never fight over it.
                dht: (!offline).then(|| DhtSessionConfig {
                    persistence: Some(DhtPersistenceConfig {
                        config_filename: Some(data_dir.join("dht.json")),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                listen: (!offline).then(|| ListenerOptions {
                    listen_addr: (Ipv6Addr::UNSPECIFIED, listen_port).into(),
                    enable_upnp_port_forwarding: upnp,
                    ..Default::default()
                }),
                disable_local_service_discovery: offline,
                ..Default::default()
            },
        )
        .await
        .context("failed to start torrent session")?;

        // The session has restored its torrents. Ones we don't know get fresh ids; ids of
        // torrents that were fetching info when the service stopped go.
        let restored: Vec<String> = session.with_torrents(|ts| ts.map(|(_, t)| t.info_hash().as_string()).collect());
        ids.retain(|hash| restored.iter().any(|h| h == hash));
        restored.iter().for_each(|hash| {
            ids.get_or_assign(hash);
        });
        ids.save(&data_dir)?;

        let state = Mutex::new(State { ids, fetching: BTreeMap::new() });
        Ok(Self { session, shared: Arc::new(Shared { data_dir, state }) })
    }

    /// Every torrent, including those fetching info, by id.
    pub fn listing(&self) -> Vec<TorrentInfo> {
        let state = self.shared.state.lock().unwrap();
        let mut torrents: Vec<TorrentInfo> = self.session.with_torrents(|ts| {
            // One without an id is a .torrent URL's that librqbit has just added; its
            // `Fetching` entry still stands for it.
            ts.filter_map(|(_, t)| Some(torrent_info(state.ids.get(&t.info_hash().as_string())?, t))).collect()
        });
        let fetching: Vec<TorrentInfo> = state
            .fetching
            .iter()
            // librqbit may have it already, a moment before its add finishes.
            .filter(|(id, _)| !torrents.iter().any(|t| t.id == **id))
            .map(|(&id, f)| fetching_info(id, f))
            .collect();
        torrents.extend(fetching);
        torrents.sort_by_key(|t| t.id);
        torrents
    }

    /// Adds the torrent, or answers with the one already in the list with the same info hash.
    pub async fn add(&self, source: AddSource, paused: bool) -> Result<Added> {
        let Prepared { add, info_hash, name, answer_at_once } = prepare(source)?;
        let (id, answer) = {
            let mut state = self.shared.state.lock().unwrap();
            if let Some(hash) = &info_hash
                && let Some(id) = state.ids.get(hash)
            {
                let name = self.name(&state, id, hash);
                return Ok(Added { id, name, already_existed: true });
            }
            let id = state.ids.next();
            if let Some(hash) = &info_hash {
                state.ids.insert(hash.clone(), id);
            }
            self.shared.save(&state);
            // `overwrite` lets a re-added torrent resume into files that already exist.
            let opts = AddTorrentOptions { paused, overwrite: true, ..Default::default() };
            let (tx, answer) = oneshot::channel();
            // Its own task, so Remove can stop it and so it finishes even if the client that
            // asked disconnects. It can't finish before its entry is in: it needs the lock.
            let (session, shared) = (self.session.clone(), self.shared.clone());
            let task = tokio::spawn(async move {
                let added = session.add_torrent(add, Some(opts)).await;
                let _ = tx.send(shared.finish_add(&session, id, added).await);
            });
            let entry = Fetching { info_hash, name: name.clone(), error: None, answered: answer_at_once, task: Some(task) };
            state.fetching.insert(id, entry);
            (id, answer)
        };
        if answer_at_once {
            return Ok(Added { id, name, already_existed: false });
        }
        answer.await.unwrap_or_else(|_| bail!("removed while fetching info"))
    }

    pub async fn pause(&self, id: TorrentId) -> Result<()> {
        self.session.pause(&self.torrent(id)?).await
    }

    pub async fn resume(&self, id: TorrentId) -> Result<()> {
        self.session.unpause(&self.torrent(id)?).await
    }

    /// Takes the torrent off the list, keeping its files. A torrent fetching info stops.
    pub async fn remove(&self, id: TorrentId) -> Result<()> {
        self.take_off_list(id, false).await
    }

    /// Removes the torrent and deletes its files. A torrent fetching info has none yet: it stops.
    pub async fn delete(&self, id: TorrentId) -> Result<()> {
        self.take_off_list(id, true).await
    }

    async fn take_off_list(&self, id: TorrentId, delete_files: bool) -> Result<()> {
        let fetching = {
            let mut state = self.shared.state.lock().unwrap();
            let entry = state.fetching.remove(&id);
            if let Some(hash) = entry.as_ref().and_then(|e| e.info_hash.as_deref()) {
                state.ids.forget(hash);
                self.shared.save(&state);
            }
            entry
        };
        if let Some(entry) = fetching {
            if let Some(task) = entry.task {
                task.abort();
                let _ = task.await;
            }
            // An add cut short may have left the torrent in librqbit, with no id now.
            if let Some(hash) = entry.info_hash
                && let Some(t) = self.session.get(TorrentIdOrHash::Hash(id20(&hash)?))
                && self.shared.state.lock().unwrap().ids.get(&hash).is_none()
            {
                self.session.delete(TorrentIdOrHash::Id(t.id()), false).await?;
            }
            return Ok(());
        }

        let hash = self.hash(id)?;
        self.session.delete(TorrentIdOrHash::Hash(id20(&hash)?), delete_files).await?;
        let mut state = self.shared.state.lock().unwrap();
        state.ids.forget(&hash);
        self.shared.save(&state);
        Ok(())
    }

    /// Saves the torrents' state and stops them.
    pub async fn stop(&self) {
        self.session.stop().await;
    }

    fn torrent(&self, id: TorrentId) -> Result<Arc<ManagedTorrent>> {
        let hash = self.hash(id)?;
        self.session
            .get(TorrentIdOrHash::Hash(id20(&hash)?))
            .with_context(|| format!("no torrent with id {id}"))
    }

    /// The info hash of a torrent librqbit has.
    fn hash(&self, id: TorrentId) -> Result<String> {
        let state = self.shared.state.lock().unwrap();
        if state.fetching.contains_key(&id) {
            bail!("torrent {id} has no info yet");
        }
        state.ids.hash_of(id).map(str::to_owned).with_context(|| format!("no torrent with id {id}"))
    }

    fn name(&self, state: &State, id: TorrentId, hash: &str) -> Option<String> {
        match state.fetching.get(&id) {
            Some(f) => f.name.clone(),
            None => self.session.get(TorrentIdOrHash::Hash(id20(hash).ok()?))?.name(),
        }
    }
}

impl Shared {
    /// The add of torrent `id` ended: record it and make the answer.
    async fn finish_add(&self, session: &Session, id: TorrentId, added: Result<AddTorrentResponse>) -> Result<Added> {
        if let Some(answer) = self.record_add(id, &added) {
            return answer;
        }
        // Removed while librqbit was adding it: take it back out, unless librqbit had it
        // already (then it's another torrent's).
        if let Ok(AddTorrentResponse::Added(lib_id, _)) = added {
            session.delete(TorrentIdOrHash::Id(lib_id), false).await?;
        }
        bail!("removed while fetching info")
    }

    /// `None` if the torrent was removed meanwhile.
    fn record_add(&self, id: TorrentId, added: &Result<AddTorrentResponse>) -> Option<Result<Added>> {
        let mut state = self.state.lock().unwrap();
        let entry = state.fetching.get_mut(&id)?;
        let torrent = match added {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => t,
            Ok(AddTorrentResponse::ListOnly(_)) => return Some(Err(anyhow!("unexpected list-only response"))),
            Err(e) if entry.answered => {
                entry.error = Some(format!("{e:#}"));
                entry.task = None;
                return Some(Err(anyhow!("{e:#}")));
            }
            // The client that asked gets the error instead.
            Err(e) => {
                if let Some(hash) = state.fetching.remove(&id).and_then(|e| e.info_hash) {
                    state.ids.forget(&hash);
                    self.save(&state);
                }
                return Some(Err(anyhow!("{e:#}")));
            }
        };
        state.fetching.remove(&id);
        let hash = torrent.info_hash().as_string();
        Some(Ok(match state.ids.get(&hash) {
            // A .torrent URL's torrent turned out to be in the list already, maybe still
            // fetching info from a magnet.
            Some(existing) if existing != id => Added { id: existing, name: torrent.name(), already_existed: true },
            _ => {
                state.ids.insert(hash, id);
                self.save(&state);
                Added { id, name: torrent.name(), already_existed: false }
            }
        }))
    }

    /// The ids are only a convenience to keep across restarts, so failing to save them is
    /// logged rather than failing the request.
    fn save(&self, state: &State) {
        if let Err(e) = state.ids.save(&self.data_dir) {
            warn!(error = %e, "failed to save torrent ids");
        }
    }
}

fn prepare(source: AddSource) -> Result<Prepared> {
    Ok(match source {
        AddSource::Url(url) if url.starts_with("http://") || url.starts_with("https://") => {
            let name = url_file_name(&url);
            Prepared { add: AddTorrent::from_url(url), info_hash: None, name, answer_at_once: false }
        }
        AddSource::Url(url) => {
            let magnet = Magnet::parse(&url).map_err(|_| anyhow!("not a magnet link or .torrent URL"))?;
            let info_hash = magnet.as_id20().context("the magnet link has no BitTorrent v1 info hash")?;
            Prepared {
                info_hash: Some(info_hash.as_string()),
                name: magnet.name,
                add: AddTorrent::from_url(url),
                answer_at_once: true,
            }
        }
        AddSource::TorrentFile(bytes) => {
            let torrent = torrent_from_bytes(&bytes).context("not a valid .torrent file")?;
            let info_hash = Some(torrent.info_hash.as_string());
            let name = torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
            Prepared { add: AddTorrent::from_bytes(bytes), info_hash, name, answer_at_once: false }
        }
    })
}

/// The name a .torrent URL implies: its file name without the extension.
fn url_file_name(url: &str) -> Option<String> {
    let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
    file.map(|f| f.trim_end_matches(".torrent").to_owned())
}

fn id20(hash: &str) -> Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("bad info hash {hash}"))
}

fn fetching_info(id: TorrentId, f: &Fetching) -> TorrentInfo {
    TorrentInfo {
        id,
        info_hash: f.info_hash.clone(),
        name: f.name.clone(),
        status: if f.error.is_some() { Status::Error } else { Status::FetchingInfo },
        error: f.error.clone(),
        progress_bytes: 0,
        size_bytes: 0,
        uploaded_bytes: 0,
        download_bytes_per_sec: 0,
        upload_bytes_per_sec: 0,
        live_peers: 0,
    }
}

fn torrent_info(id: TorrentId, torrent: &ManagedTorrent) -> TorrentInfo {
    let stats = torrent.stats();
    let live = stats.live.as_ref();
    TorrentInfo {
        id,
        info_hash: Some(torrent.info_hash().as_string()),
        name: torrent.name(),
        status: match stats.state {
            TorrentStatsState::Initializing { .. } => Status::Checking,
            TorrentStatsState::Live if stats.finished => Status::Seeding,
            TorrentStatsState::Live => Status::Downloading,
            TorrentStatsState::Paused if stats.finished => Status::Done,
            TorrentStatsState::Paused => Status::Paused,
            TorrentStatsState::Error => Status::Error,
        },
        error: stats.error,
        progress_bytes: stats.progress_bytes,
        size_bytes: stats.total_bytes,
        uploaded_bytes: stats.uploaded_bytes,
        download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
        upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
        live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
    }
}
