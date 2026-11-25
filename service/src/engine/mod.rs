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
//! `store.rs` each torrent's record, which librqbit gets it from (#45), `files.rs` how missing files
//! are found, `undo.rs` what takes back a Remove or a Delete.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

mod actor;
mod bitv;
mod default_folder;
mod files;
mod held;
mod looks;
mod machine;
#[cfg(test)]
mod model_tests;
mod moving;
mod places;
#[cfg(test)]
mod race_tests;
mod slots;
mod snapshot;
mod space;
mod store;
mod trash;
mod trashed;
mod undo;

pub use trash::{Trash, system as system_trash};

pub use files::Destination;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    AddTorrent, DhtSessionConfig, ListenerOptions, Magnet, Session, SessionOptions, SessionPersistenceConfig, torrent_from_bytes,
};
use proto::{AbsolutePath, AddSource, Added, ServiceError, TorrentCommand, TorrentId, Unavailable};
#[cfg(test)]
use proto::{Details, TorrentInfo};
use tokio::sync::{mpsc, oneshot, watch};

pub use actor::Carried;
use actor::{Actor, Message};
pub use snapshot::Published;
use store::Store;

/// How often the service's engine polls librqbit, between commands.
pub const POLL_EVERY: Duration = Duration::from_millis(500);

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
    /// How often the actor polls librqbit and looks at the destinations' disks, between
    /// commands: [`POLL_EVERY`], or shorter in tests.
    pub poll_every: Duration,
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
// Torrents still fetching info don't survive a restart: a record has a .torrent to add again only
// once its info arrives. Their ids are dropped when the service starts again.
pub struct Engine {
    actor: mpsc::Sender<Message>,
    /// What the actor last published.
    snapshot: watch::Receiver<Published>,
    /// To download .torrent URLs without the actor. Taken at stop, so the session can end.
    session: Mutex<Option<Arc<Session>>>,
    /// librqbit's session folder, where it saves the pieces of each torrent.
    session_folder: PathBuf,
    /// Where the torrents' database was kept aside at this start, if it was ([`store::Loaded::set_aside`]).
    records_set_aside: Option<PathBuf>,
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

/// How the engine looks at the disk and the folders it watches: the real ones, or a test's.
#[derive(Clone)]
struct Looks {
    /// Each destination's disk and its files ([`space`]).
    disks: space::Look,
    /// The default destination ([`default_folder`]).
    default_folder: default_folder::Look,
}

impl Looks {
    fn real() -> Self {
        Self { disks: space::look(), default_folder: default_folder::look() }
    }
}

impl Engine {
    /// Starts the session and the actor, resuming the torrents saved in the data dir with the
    /// ids they had; those wanted running run again.
    pub async fn start(config: Config) -> Result<Self> {
        Self::start_with(
            config,
            Arc::new(|session, id, add, destination, download_dir, guard| {
                Box::pin(actor::add_into(session, id, add, destination, download_dir, guard))
            }),
            files::Guard::default(),
            Looks::real(),
        )
        .await
    }

    /// [`Engine::start`], with adds reaching librqbit through `add_into`, librqbit's storage
    /// through `guard`, and disks and folders looked at with `looks`.
    async fn start_with(config: Config, add_into: actor::AddInto, guard: files::Guard, looks: Looks) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp, offline, size_check_every, poll_every, trash, speed_limits, seeding } =
            config;
        let session_folder = session_folder(&data_dir);
        std::fs::create_dir_all(&session_folder).with_context(|| format!("failed to make {}", session_folder.display()))?;
        let undo = undo::load(&data_dir);
        let store::Loaded { mut store, mut records, unreadable, set_aside } = Store::load(&data_dir, &session_folder)?;
        // An Undo brings a torrent back under its id: no new torrent may have it, as when the records
        // were set aside as damaged and start again at 0.
        for id in undo::ids(&undo) {
            store.ids_above(id);
        }
        forget_librqbits_list(&session_folder);
        let guard = guard.keeping_parts_in(undo::parts_dir(&data_dir));
        // What Moves to Trash took that their undos may put back: librqbit reads it where it went,
        // and never makes it again.
        guard.trashed().follow(&undo);
        // A let-go of a file gone from the Trash a crash cut short: its pieces cleared in what
        // librqbit saved, before the torrent is added again with them (#234); so is a file set Off that
        // was deleted or moved away while the service was stopped (#276).
        actor::letgo::clear_at_start(&mut records, &session_folder, |hash| guard.trashed().in_trash(hash));
        let unavailable = unavailable_at_start(&records);
        let session = Session::new_with_opts(
            download_dir.clone(),
            SessionOptions {
                default_storage_factory: Some(guard.clone().boxed()),
                fastresume: true,
                // Only for the pieces each torrent has (`<hash>.bitv`): the torrents are Phantom's.
                persistence: Some(SessionPersistenceConfig::Json { folder: Some(session_folder.clone()) }),
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

        let (tx, inbox) = mpsc::channel(64);
        let actor = Actor::new(
            actor::Setup {
                session: session.clone(),
                data_dir,
                download_dir,
                guard,
                size_check_every,
                poll_every,
                trash,
                inbox: tx.downgrade(),
                add_into,
                looks,
                seeding,
                undo,
            },
            store,
            records,
            unavailable,
            unreadable,
        )
        .await;
        let snapshot = actor.subscribe();
        tokio::spawn(actor.run(inbox));
        Ok(Self { actor: tx, snapshot, session: Mutex::new(Some(session)), session_folder, records_set_aside: set_aside })
    }

    /// Every torrent, including those fetching info, by id.
    #[cfg(test)]
    pub async fn listing(&self) -> Vec<TorrentInfo> {
        self.snapshot.borrow().snapshot.torrents.clone()
    }

    /// Where the torrents' database was kept aside when this engine started, if it was.
    pub fn records_set_aside(&self) -> Option<&Path> {
        self.records_set_aside.as_deref()
    }

    /// The snapshots the actor publishes, from the latest: one per change.
    pub fn subscribe(&self) -> watch::Receiver<Published> {
        self.snapshot.clone()
    }

    /// What `proto::Request::Add` says it does, into `destination`, once a .torrent URL is
    /// [`fetched`]: the engine never waits on the network for an add.
    pub async fn add(&self, source: Fetched, paused: bool, destination: Destination) -> Result<Added, Failure> {
        let prepared = prepare(source, destination.into_path())?;
        self.ask(|reply| Message::Add { prepared, paused, reply }).await?
    }

    /// What `proto::Request::Torrents` says it does: those of `ids` that didn't do as asked, with
    /// why.
    pub async fn commands(&self, ids: Vec<TorrentId>, command: TorrentCommand) -> Result<Vec<(TorrentId, Failure)>, Failure> {
        self.ask(|reply| Message::Command { ids, command, reply }).await
    }

    /// What `proto::Request::Moves` says it does: those of `moves` that didn't move, with why.
    #[cfg(test)]
    pub async fn moves(&self, to: AbsolutePath, moves: Vec<proto::MoveOf>) -> Result<Vec<(TorrentId, Failure)>, Failure> {
        self.send_moves(to, moves).await?.await
    }

    /// [`Engine::moves`], sent: its answer, which holds nothing of this engine, comes once every
    /// Move is over, from the next engine for those a restart for new settings hands it. So a
    /// batch's caller needn't hold the engine as it runs, and the restart, which waits for what
    /// holds it, waits only for the Move under way.
    pub async fn send_moves(
        &self,
        to: AbsolutePath,
        moves: Vec<proto::MoveOf>,
    ) -> Result<impl Future<Output = Result<Vec<(TorrentId, Failure)>, Failure>> + Send + 'static, Failure> {
        let (reply, answer) = oneshot::channel();
        self.actor.send(Message::Moves { to, moves, reply }).await.map_err(|_| stopping())?;
        Ok(async move { answer.await.map_err(|_| stopping()) })
    }

    /// [`Engine::moves`], as a future of its own, for a test to wait on later.
    #[cfg(test)]
    pub fn moves_later(
        &self,
        to: AbsolutePath,
        moves: Vec<proto::MoveOf>,
    ) -> impl Future<Output = Result<Vec<(TorrentId, Failure)>, Failure>> + Send + 'static {
        let actor = self.actor.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            actor.send(Message::Moves { to, moves, reply }).await.map_err(|_| stopping())?;
            answer.await.map_err(|_| stopping())
        }
    }

    /// What `proto::Request::Clashes` says it does.
    pub async fn clashes(&self, ids: Vec<TorrentId>, to: AbsolutePath) -> Result<Vec<proto::Clashing>, Failure> {
        self.ask(|reply| Message::Clashes { ids, to, reply }).await
    }

    /// What `proto::Request::FreeName` says it does.
    pub async fn free_name(&self, id: TorrentId, to: AbsolutePath) -> Result<proto::FileName, Failure> {
        self.ask(|reply| Message::FreeName { id, to, reply }).await?
    }

    /// A Move of torrent `id` into `to` with `choice` for what a clash finds there now, as a client
    /// answers what it was told: none if nothing is there.
    #[cfg(test)]
    pub async fn move_answering(&self, id: TorrentId, to: AbsolutePath, choice: proto::Clash) -> TorrentCommand {
        let found = self.clashes(vec![id], to.clone()).await.unwrap_or_default();
        let clash = found.into_iter().next().map(|c| proto::Answered { choice, found: c.files });
        TorrentCommand::Move { to, clash, name: None }
    }

    /// What `proto::TorrentCommand` says it does to torrent `id`, or refused with a
    /// [`proto::Refusal::Torrent`].
    #[cfg(test)]
    pub async fn command(&self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        first_failure(self.commands(vec![id], command).await?)
    }

    /// [`Engine::add`], as a future of its own, for a test to wait on later.
    #[cfg(test)]
    pub fn add_later(
        &self,
        source: Fetched,
        paused: bool,
        destination: Destination,
    ) -> impl Future<Output = Result<Added, Failure>> + Send + 'static {
        let actor = self.actor.clone();
        async move {
            let prepared = prepare(source, destination.into_path())?;
            let (reply, answer) = oneshot::channel();
            actor.send(Message::Add { prepared, paused, reply }).await.map_err(|_| stopping())?;
            answer.await.map_err(|_| stopping())?
        }
    }

    /// What `proto::Request::Undo` says it does.
    pub async fn undo(&self) -> Result<proto::Undone, Failure> {
        self.ask(Message::Undo).await?
    }

    /// [`Engine::undo`], as a future of its own, for a test to wait on later.
    #[cfg(test)]
    pub fn undo_later(&self) -> impl Future<Output = Result<proto::Undone, Failure>> + Send + 'static {
        let actor = self.actor.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            actor.send(Message::Undo(reply)).await.map_err(|_| stopping())?;
            answer.await.map_err(|_| stopping())?
        }
    }

    /// [`Engine::command`], sent now, so it comes before anything sent after: its answer, to wait
    /// on later.
    #[cfg(test)]
    pub async fn sent(&self, id: TorrentId, command: TorrentCommand) -> impl Future<Output = Result<(), Failure>> + Send + 'static {
        let (reply, answer) = oneshot::channel();
        let sent = self.actor.send(Message::Command { ids: vec![id], command, reply }).await;
        async move {
            sent.map_err(|_| stopping())?;
            first_failure(answer.await.map_err(|_| stopping())?)
        }
    }

    /// [`Engine::commands`], sent now, so it comes before anything sent after: its answer, to wait
    /// on later.
    #[cfg(test)]
    pub async fn sent_all(
        &self,
        ids: Vec<TorrentId>,
        command: TorrentCommand,
    ) -> impl Future<Output = Result<Vec<(TorrentId, Failure)>, Failure>> + Send + 'static {
        let (reply, answer) = oneshot::channel();
        let sent = self.actor.send(Message::Command { ids, command, reply }).await;
        async move {
            sent.map_err(|_| stopping())?;
            answer.await.map_err(|_| stopping())
        }
    }

    /// [`Engine::undo`], sent now: its answer, to wait on later.
    #[cfg(test)]
    pub async fn undo_sent(&self) -> impl Future<Output = Result<proto::Undone, Failure>> + Send + 'static {
        let (reply, answer) = oneshot::channel();
        let sent = self.actor.send(Message::Undo(reply)).await;
        async move {
            sent.map_err(|_| stopping())?;
            answer.await.map_err(|_| stopping())?
        }
    }

    /// [`Engine::command`], as a future of its own, for a test to wait on later.
    #[cfg(test)]
    pub fn command_later(&self, id: TorrentId, command: TorrentCommand) -> impl Future<Output = Result<(), Failure>> + Send + 'static {
        let actor = self.actor.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            actor.send(Message::Command { ids: vec![id], command, reply }).await.map_err(|_| stopping())?;
            first_failure(answer.await.map_err(|_| stopping())?)
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
        self.stop_as(actor::Ending::Shutdown).await;
    }

    /// Stops it to start it again with new settings (the listening port, UPnP): what waits for its
    /// turn on a torrent runs first, as the service carries on, but the Moves waiting their turn:
    /// the restart waits only for the one under way (ADR 0008). Those it hands back, with their
    /// clients, for the next engine to [`resume`](Engine::resume). Cancel move meanwhile is this
    /// engine's to answer.
    pub async fn stop_to_restart(&self) -> Carried {
        self.stop_as(actor::Ending::Restart).await
    }

    /// The Moves the engine before this one didn't start as it restarted
    /// ([`Engine::stop_to_restart`]) wait their turn here, in their order: once they're in.
    pub async fn resume(&self, carried: Carried) {
        let _ = self.ask(|reply| Message::Resume(carried, reply)).await;
    }

    async fn stop_as(&self, ending: actor::Ending) -> Carried {
        let (held, carried) = self.ask(|reply| Message::Stop(ending, reply)).await.unwrap_or_default();
        let Some(session) = self.session.lock().unwrap().take() else { return carried };
        let ended = Arc::downgrade(&session);
        drop(session);
        for _ in 0..40 {
            if ended.strong_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bitv::wait_for_saves(&self.session_folder, &held).await;
        carried
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

    /// The default destination is `folder` (`None`: there's none): the engine watches whether it
    /// can be reached, and every snapshot says why not, if it can't. Answered once the last
    /// folder's reason is out of the snapshots, so an answer built on them is the new folder's.
    pub async fn set_default_destination(&self, folder: Option<AbsolutePath>) {
        let _ = self.ask(|reply| Message::DefaultFolder(folder, reply)).await;
    }

    /// Why the default destination can't be reached, as the last snapshot says.
    pub fn default_unavailable(&self) -> Option<Unavailable> {
        self.snapshot.borrow().snapshot.default_unavailable.clone()
    }

    /// When a finished torrent stops seeding, from now: its round so far counts.
    pub async fn set_seeding(&self, limits: machine::seeding::Limits) {
        let _ = self.actor.send(Message::Seeding(limits)).await;
    }

    /// The seeding limits the actor goes by now.
    #[cfg(test)]
    pub async fn seeding(&self) -> machine::seeding::Limits {
        self.inspect(|actor| actor.seeding()).await
    }

    /// The limits the session runs with now.
    #[cfg(test)]
    pub fn limits(&self) -> librqbit::limits::LimitsConfig {
        self.session.lock().unwrap().as_ref().expect("running").ratelimits.get_config()
    }

    /// Sends the actor a message and waits for its answer.
    async fn ask<T>(&self, message: impl FnOnce(oneshot::Sender<T>) -> Message) -> Result<T, Failure> {
        let (reply, answer) = oneshot::channel();
        self.actor.send(message(reply)).await.map_err(|_| stopping())?;
        answer.await.map_err(|_| stopping())
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
fn unavailable_at_start(records: &[store::Record]) -> BTreeMap<String, Unavailable> {
    records.iter().filter_map(|r| Some((r.info_hash.clone(), files::unavailable(&r.destination)?))).collect()
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
pub async fn fetched(source: AddSource) -> Result<Fetched, ServiceError> {
    Ok(match source {
        AddSource::TorrentUrl(url) => {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(ServiceError::NotHttpUrl { url });
            }
            let download = |e: reqwest::Error| ServiceError::TorrentDownload { detail: e.to_string() };
            let response = reqwest::get(&url).await.map_err(download)?;
            let status = response.status();
            if !status.is_success() {
                return Err(ServiceError::TorrentUrlStatus {
                    url,
                    status: status.as_u16(),
                    phrase: status.canonical_reason().map(str::to_owned),
                });
            }
            Fetched::TorrentFile(response.bytes().await.map_err(download)?.to_vec())
        }
        AddSource::Magnet(url) => Fetched::Magnet(url),
        AddSource::TorrentFile(bytes) => Fetched::TorrentFile(bytes),
    })
}

/// A magnet link or a .torrent file's bytes, with its info hash.
fn prepare(source: Fetched, destination: AbsolutePath) -> Result<Prepared, ServiceError> {
    Ok(match source {
        // Checked by the client; what librqbit can't read is refused here.
        Fetched::Magnet(url) => {
            let magnet = Magnet::parse(&url).map_err(|_| ServiceError::InvalidMagnet)?;
            let info_hash = magnet.as_id20().ok_or(ServiceError::NoV1InfoHash)?.as_string();
            Prepared { info_hash, name: magnet.name, trackers: magnet.trackers, add: AddTorrent::from_url(url), destination }
        }
        Fetched::TorrentFile(bytes) => {
            let torrent = torrent_from_bytes(&bytes).map_err(|e| ServiceError::InvalidTorrentFile { detail: format!("{e:#}") })?;
            if let Some(path) = store::Info::parse(&bytes).ok().and_then(|i| i.phantoms(None)) {
                let detail = format!("{} takes the name of a folder of Phantom's own", path.display());
                return Err(ServiceError::InvalidTorrentFile { detail });
            }
            let info_hash = torrent.info_hash.as_string();
            let name = torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
            let trackers = torrent.iter_announce().map(|t| String::from_utf8_lossy(t.as_ref()).into_owned()).collect();
            Prepared { add: AddTorrent::from_bytes(bytes), info_hash, name, trackers, destination }
        }
    })
}

/// The service going: what every request meets once the engine's actor has ended.
fn stopping() -> Failure {
    Failure::Refused(proto::Refusal::Stopping)
}

/// Why a request didn't do what it asks: refused, for a reason the client words and may act on,
/// or failed. The two stay apart up to the socket, where they become `Response::Refused` and
/// `Response::Error`; `?` on an `anyhow` error can only make the second.
#[derive(Debug)]
pub enum Failure {
    Refused(proto::Refusal),
    Error(proto::ServiceError),
}

impl Failure {
    pub fn torrent(id: TorrentId, why: proto::TorrentRefusal) -> Self {
        Self::Refused(proto::Refusal::Torrent { id, why })
    }

    /// Why an undo that ended so couldn't be done, as data clients word: its own refusal's reason
    /// and what it left out of the Trash, or else the failure itself.
    pub fn cant_undo(&self) -> (proto::CantUndo, Vec<proto::AbsolutePath>) {
        match self {
            Self::Refused(proto::Refusal::Undo { why, stranded, .. }) => (why.clone(), stranded.clone()),
            Self::Error(proto::ServiceError::Internal { detail }) => (proto::CantUndo::Failed { detail: detail.clone() }, Vec::new()),
            Self::Error(error) => (proto::CantUndo::Error { error: error.clone() }, Vec::new()),
            Self::Refused(refusal) => (proto::CantUndo::Refused { refusal: Box::new(refusal.clone()) }, Vec::new()),
        }
    }
}

impl From<Failure> for proto::Fault {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Refused(refusal) => proto::Fault::Refused { refusal },
            Failure::Error(error) => proto::Fault::Error { error },
        }
    }
}

impl From<proto::Refusal> for Failure {
    fn from(refusal: proto::Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<proto::ServiceError> for Failure {
    fn from(error: proto::ServiceError) -> Self {
        Self::Error(error)
    }
}

/// What has no case of its own: the error's text, as it is.
impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Error(proto::ServiceError::Internal { detail: format!("{error:#}") })
    }
}

/// A command for one torrent: its failure, if it failed.
#[cfg(test)]
fn first_failure(failed: Vec<(TorrentId, Failure)>) -> Result<(), Failure> {
    match failed.into_iter().next() {
        Some((_, failure)) => Err(failure),
        None => Ok(()),
    }
}

/// librqbit's session folder in the data dir, where it saves the pieces of each torrent.
pub(super) fn session_folder(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("session")
}

/// Deletes librqbit's list of its torrents (`session.json`), which it writes as it runs and would
/// restore from: the torrents are Phantom's, and the engine adds each one itself (#45). What it
/// saves of each torrent's pieces (`<hash>.bitv`) stays.
fn forget_librqbits_list(session_folder: &std::path::Path) {
    let path = session_folder.join("session.json");
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!(?path, error = %e, "can't delete librqbit's list of its torrents")
        }
        _ => {}
    }
}

fn id20(hash: &str) -> Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("bad info hash {hash}"))
}
