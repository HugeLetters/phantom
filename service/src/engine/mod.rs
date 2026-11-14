//! The torrents, in Phantom's terms: [`Engine`] starts a session, adds (each into the destination
//! it's given), lists, pauses, resumes, checks, removes, deletes and stops them, and shows and sets
//! the priorities of their files (holding a change made during a check until the check is done).
//! Notices files deleted outside Phantom: the torrent stops and is Missing files; and destinations
//! that can't be reached: it stops and is Destination unavailable until they're back. Owns
//! torrent ids, de-duplicates adds by
//! info hash, and keeps each torrent's lifecycle ([ADR 0006]): what's wanted of it and what it's
//! doing, from which its Status and the actions it accepts follow. The only code that knows
//! librqbit; the server hands it requests and gets back `proto` types.
//!
//! [`Engine`] is a handle: every call is a message to one actor task (`actor.rs`) that owns all
//! the torrents and handles messages one at a time, but for `listing` and `details`, which read the
//! [`Snapshot`] it last published (`snapshot.rs`). `machine/` decides what each event does and how it's carried out on librqbit,
//! `store.rs` what's kept across restarts, `files.rs` how missing files are found.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

mod actor;
mod bitv;
mod files;
mod machine;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod race_tests;
mod snapshot;
mod space;
mod store;
mod trash;

pub use trash::{Trash, system as system_trash};

pub use files::Destination;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    AddTorrent, DhtSessionConfig, ListenerOptions, Magnet, Session, SessionOptions, SessionPersistenceConfig, torrent_from_bytes,
};
use proto::{AbsolutePath, AddSource, Added, TorrentCommand, TorrentId, Unavailable};
#[cfg(test)]
use proto::{Details, TorrentInfo};
use tokio::sync::{mpsc, oneshot, watch};

use actor::{Actor, Message};
pub use snapshot::Published;
use store::Store;

/// What the engine needs from the service's settings and environment.
pub struct Config {
    /// The data dir: the session's state, DHT state and the engine's own state go here.
    pub data_dir: PathBuf,
    /// librqbit's own folder. Every add names its destination, so nothing goes here: librqbit
    /// only uses it to name a torrent's own folder.
    pub download_dir: PathBuf,
    pub listen_port: u16,
    pub upnp: bool,
    /// No DHT, no local peer discovery and no listener: only trackers a torrent names find
    /// peers. For tests, which must not reach the internet.
    pub offline: bool,
    /// How often every torrent's files get a size check, besides when the watcher sees
    /// a path go.
    pub size_check_every: Duration,
    /// Where Delete moves a torrent's files: [`trash::system`], or a test's own.
    pub trash: Trash,
    /// For all torrents together; [`Engine::set_limits`] changes them while it runs.
    pub speed_limits: proto::SpeedLimits,
    /// When a finished torrent stops seeding; [`Engine::set_seeding`] changes them while it runs.
    pub seeding: machine::seeding::Limits,
}

// Why the engine owns torrent ids: in the user's terms a torrent exists from the moment it's
// added, even while a magnet's info is still being fetched, and it must keep one id throughout
// so clients can show, select and remove it. librqbit only numbers a torrent once its info
// arrives, and renumbers the torrents it restores, so its ids can't be ours.
//
// So every torrent gets a Phantom id when it's added. The store keeps each info hash's id, so
// ids survive restarts. Every add knows its info hash: a .torrent URL is downloaded first, outside
// the actor, so that wait is just a pending request.
//
// Torrents still fetching info don't survive a restart: librqbit only saves a torrent once its
// info arrives. Their ids are dropped when the service starts again.
pub struct Engine {
    actor: mpsc::Sender<Message>,
    /// What the actor last published.
    snapshot: watch::Receiver<Published>,
    /// To download .torrent URLs without the actor. Taken at stop, so the session can end.
    session: Mutex<Option<Arc<Session>>>,
    /// librqbit's session folder, where it saves the pieces of each torrent.
    session_folder: PathBuf,
}

/// An add, checked and ready to hand to librqbit.
struct Prepared {
    add: AddTorrent<'static>,
    info_hash: String,
    name: Option<String>,
    /// The trackers it names.
    trackers: Vec<String>,
    /// The folder its files go into: a torrent of several files gets its own folder inside it.
    destination: AbsolutePath,
}

impl Engine {
    /// Starts the session and the actor, resuming the torrents saved in the data dir with the
    /// ids they had; those wanted running run again.
    pub async fn start(config: Config) -> Result<Self> {
        Self::start_with(
            config,
            Arc::new(|session, id, add, destination, download_dir| Box::pin(actor::add_into(session, id, add, destination, download_dir))),
            files::Guard::default(),
            space::look(),
        )
        .await
    }

    /// [`Engine::start`], with adds reaching librqbit through `add_into`, librqbit's storage
    /// through `guard`, and disks looked at with `look`.
    async fn start_with(config: Config, add_into: actor::AddInto, guard: files::Guard, look: space::Look) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp, offline, size_check_every, trash, speed_limits, seeding } = config;
        let mut store = Store::load(&data_dir)?;
        // Before librqbit restores the torrents, which creates their files: not where a drive
        // isn't connected, and missing ones not again, empty.
        let unavailable = unavailable_at_start(&store);
        for (hash, saved) in store.iter() {
            guard.set_destination(hash, saved.destination.clone());
            // Partly in the Trash, or on its way there: its files aren't made again either.
            guard.keep_untouched(hash, !saved.delete_failed.is_empty() || !saved.deleting.is_empty());
        }
        let missing = missing_since_shutdown(&mut store, &unavailable);
        let session = Session::new_with_opts(
            download_dir.clone(),
            SessionOptions {
                default_storage_factory: Some(guard.clone().boxed()),
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json { folder: Some(data_dir.join("session")) }),
                // librqbit keeps DHT state (including its port) in a folder shared by every librqbit
                // program; keep ours with our data so two services, or rqbit, never fight over it.
                dht: (!offline).then(|| DhtSessionConfig {
                    persistence: Some(DhtPersistenceConfig { config_filename: Some(data_dir.join("dht.json")), ..Default::default() }),
                    ..Default::default()
                }),
                listen: (!offline).then(|| ListenerOptions {
                    listen_addr: (Ipv6Addr::UNSPECIFIED, listen_port).into(),
                    enable_upnp_port_forwarding: upnp,
                    ..Default::default()
                }),
                disable_local_service_discovery: offline,
                ratelimits: limits_config(speed_limits),
                ..Default::default()
            },
        )
        .await
        .context("failed to start torrent session")?;
        log_limits(speed_limits);

        let session_folder = data_dir.join("session");
        let (tx, inbox) = mpsc::channel(64);
        let actor = Actor::new(
            actor::Setup {
                session: session.clone(),
                data_dir,
                download_dir,
                guard,
                size_check_every,
                trash,
                inbox: tx.downgrade(),
                add_into,
                look,
                seeding,
            },
            store,
            missing,
            unavailable,
        );
        let snapshot = actor.subscribe();
        tokio::spawn(actor.run(inbox));
        Ok(Self { actor: tx, snapshot, session: Mutex::new(Some(session)), session_folder })
    }

    /// Every torrent, including those fetching info, by id.
    #[cfg(test)]
    pub async fn listing(&self) -> Vec<TorrentInfo> {
        self.snapshot.borrow().snapshot.torrents.clone()
    }

    /// The snapshots the actor publishes, from the latest: one per change.
    pub fn subscribe(&self) -> watch::Receiver<Published> {
        self.snapshot.clone()
    }

    /// What `proto::Request::Add` says it does, into `destination`, once a .torrent URL is
    /// [`fetched`]: the engine never waits on the network for an add.
    pub async fn add(&self, source: Fetched, paused: bool, destination: Destination) -> Result<Added> {
        let prepared = prepare(source, destination.into_path())?;
        self.ask(|reply| Message::Add { prepared, paused, reply }).await?
    }

    /// What `proto::TorrentCommand` says it does, or refused with a [`proto::Refusal::Torrent`].
    pub async fn command(&self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        self.ask(|reply| Message::Command { id, command, reply }).await?
    }

    /// [`Engine::command`], as a future of its own, for a test to wait on later.
    #[cfg(test)]
    pub fn command_later(&self, id: TorrentId, command: TorrentCommand) -> impl Future<Output = Result<(), Failure>> + Send + 'static {
        let actor = self.actor.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            actor.send(Message::Command { id, command, reply }).await.map_err(|_| anyhow!("the service is stopping"))?;
            answer.await.map_err(|_| anyhow!("the service is stopping"))?
        }
    }

    #[cfg(test)]
    pub async fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        self.snapshot.borrow().snapshot.details(id)
    }

    /// Records the missing files, pauses running torrents so librqbit saves them paused, and ends
    /// the session. Those wanted running resume on the next start.
    ///
    /// librqbit saves which pieces a torrent has every 16 MiB and when it drops the torrent, not
    /// when it pauses it, so a stop that left the torrents alive lost what came since the last
    /// save: the session is dropped here, and the saved pieces polled until they hold what each
    /// torrent had (see `bitv`).
    pub async fn stop(&self) {
        let held = self.ask(Message::Stop).await.unwrap_or_default();
        let Some(session) = self.session.lock().unwrap().take() else { return };
        let ended = Arc::downgrade(&session);
        drop(session);
        for _ in 0..40 {
            if ended.strong_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bitv::wait_for_saves(&self.session_folder, &held).await;
    }

    /// Every torrent's traffic goes through the session's limiter, which librqbit swaps at once:
    /// nothing restarts.
    pub fn set_limits(&self, limits: proto::SpeedLimits) {
        if let Some(session) = self.session.lock().unwrap().as_ref() {
            log_limits(limits);
            let config = limits_config(limits);
            session.ratelimits.set_download_bps(config.download_bps);
            session.ratelimits.set_upload_bps(config.upload_bps);
        }
    }

    /// When a finished torrent stops seeding, from now: its round so far counts.
    pub async fn set_seeding(&self, limits: machine::seeding::Limits) {
        let _ = self.actor.send(Message::Seeding(limits)).await;
    }

    /// The limits the session runs with now.
    #[cfg(test)]
    pub fn limits(&self) -> librqbit::limits::LimitsConfig {
        self.session.lock().unwrap().as_ref().expect("running").ratelimits.get_config()
    }

    /// Sends the actor a message and waits for its answer.
    async fn ask<T>(&self, message: impl FnOnce(oneshot::Sender<T>) -> Message) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        let stopped = || anyhow!("the service is stopping");
        self.actor.send(message(reply)).await.map_err(|_| stopped())?;
        answer.await.map_err(|_| stopped())
    }

    /// Ends the actor as a crash would; the engine is dropped next.
    #[cfg(test)]
    pub async fn kill(&self) {
        let _ = self.ask(Message::Kill).await;
    }

    /// Runs `f` on the actor and returns what it returns.
    #[cfg(test)]
    async fn inspect<T: Send + 'static>(&self, f: impl FnOnce(&mut Actor) -> T + Send + 'static) -> T {
        self.ask(|reply| Message::Inspect(Box::new(move |actor| drop(reply.send(f(actor)))))).await.unwrap()
    }
}

/// The limits the session runs with from now, in bytes per second, 0 unlimited: what a test of the
/// whole service reads back.
fn log_limits(limits: proto::SpeedLimits) {
    let bps = |limit: Option<proto::SpeedLimit>| limit.map_or(0, u64::from);
    tracing::info!(download_bps = bps(limits.download), upload_bps = bps(limits.upload), "speed limits");
}

fn limits_config(limits: proto::SpeedLimits) -> librqbit::limits::LimitsConfig {
    librqbit::limits::LimitsConfig {
        download_bps: limits.download.map(proto::SpeedLimit::bytes_per_sec),
        upload_bps: limits.upload.map(proto::SpeedLimit::bytes_per_sec),
    }
}

/// Why the destination of each torrent (by info hash) can't be reached now, for those whose can't.
fn unavailable_at_start(store: &Store) -> BTreeMap<String, Unavailable> {
    store.iter().filter_map(|(hash, saved)| Some((hash.clone(), files::unavailable(saved.destination.as_ref()?)?))).collect()
}

/// The files of each torrent (by info hash) missing since the service last stopped, with what
/// librqbit had of each: those that were, and those it was getting that aren't on disk now.
/// Takes what the store recorded at stop. Those whose destination is `unavailable` aren't missing.
fn missing_since_shutdown(store: &mut Store, unavailable: &BTreeMap<String, Unavailable>) -> BTreeMap<String, BTreeMap<usize, u64>> {
    store
        .iter_mut()
        .map(|(hash, saved)| {
            // Out of reach, or partly in the Trash on purpose: neither is missing files.
            if unavailable.contains_key(hash) || !saved.delete_failed.is_empty() || !saved.deleting.is_empty() {
                saved.missing_at_shutdown.clear();
                saved.files_at_shutdown.clear();
            }
            let mut missing = std::mem::take(&mut saved.missing_at_shutdown);
            let files = std::mem::take(&mut saved.files_at_shutdown);
            let gone = files::missing(&files);
            missing.extend(files.iter().filter(|f| gone.contains(&f.index)).map(|f| (f.index, f.progress_bytes)));
            (hash.clone(), missing)
        })
        .filter(|(_, missing)| !missing.is_empty())
        .collect()
}

/// An add's source with a .torrent URL already downloaded: a magnet link or a file's bytes.
pub enum Fetched {
    Magnet(String),
    TorrentFile(Vec<u8>),
}

#[cfg(test)]
impl Fetched {
    /// A magnet or a file, which need no fetching.
    fn local(source: AddSource) -> Self {
        match source {
            AddSource::Magnet(url) => Self::Magnet(url),
            AddSource::TorrentFile(bytes) => Self::TorrentFile(bytes),
            AddSource::TorrentUrl(_) => panic!("a URL is fetched"),
        }
    }
}

/// Downloads a .torrent URL, outside the engine, so nothing waits on it but the add; a magnet or
/// a file is as it is.
pub async fn fetched(source: AddSource) -> Result<Fetched> {
    Ok(match source {
        AddSource::TorrentUrl(url) => {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                bail!("{url} isn't an http(s) URL");
            }
            let response = reqwest::get(&url).await.context("can't download the .torrent")?;
            if !response.status().is_success() {
                bail!("downloading the .torrent: {url} answered {}", response.status());
            }
            Fetched::TorrentFile(response.bytes().await.context("can't download the .torrent")?.to_vec())
        }
        AddSource::Magnet(url) => Fetched::Magnet(url),
        AddSource::TorrentFile(bytes) => Fetched::TorrentFile(bytes),
    })
}

/// A magnet link or a .torrent file's bytes, with its info hash.
fn prepare(source: Fetched, destination: AbsolutePath) -> Result<Prepared> {
    Ok(match source {
        // Checked by the client; what librqbit can't read is refused here.
        Fetched::Magnet(url) => {
            let magnet = Magnet::parse(&url).map_err(|_| anyhow!("not a valid magnet link"))?;
            let info_hash = magnet.as_id20().context("the magnet link has no BitTorrent v1 info hash")?.as_string();
            Prepared { info_hash, name: magnet.name, trackers: magnet.trackers, add: AddTorrent::from_url(url), destination }
        }
        Fetched::TorrentFile(bytes) => {
            let torrent = torrent_from_bytes(&bytes).context("not a valid .torrent file")?;
            let info_hash = torrent.info_hash.as_string();
            let name = torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
            let trackers = torrent.iter_announce().map(|t| String::from_utf8_lossy(t.as_ref()).into_owned()).collect();
            Prepared { add: AddTorrent::from_bytes(bytes), info_hash, name, trackers, destination }
        }
    })
}

/// Why a request didn't do what it asks: refused, for a reason the client words and may act on,
/// or failed. The two stay apart up to the socket, where they become `Response::Refused` and
/// `Response::Error`; `?` on an `anyhow` error can only make the second.
#[derive(Debug)]
pub enum Failure {
    Refused(proto::Refusal),
    Error(anyhow::Error),
}

impl Failure {
    pub fn torrent(id: TorrentId, why: proto::TorrentRefusal) -> Self {
        Self::Refused(proto::Refusal::Torrent { id, why })
    }
}

impl From<proto::Refusal> for Failure {
    fn from(refusal: proto::Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Error(error)
    }
}

fn id20(hash: &str) -> Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("bad info hash {hash}"))
}
