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
//! the torrents and handles messages one at a time. `lifecycle.rs` decides what each event does,
//! `store.rs` what's kept across restarts, `files.rs` how missing files are found.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

mod actor;
mod files;
mod lifecycle;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod race_tests;
mod store;

pub use files::Destination;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Magnet, Session, SessionOptions,
    SessionPersistenceConfig, torrent_from_bytes,
};
use proto::{AbsolutePath, AddSource, Added, Details, TorrentCommand, TorrentId, TorrentInfo, Unavailable};
use tokio::sync::{mpsc, oneshot};

use actor::{Actor, Message};
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
    /// To download .torrent URLs without the actor.
    session: Arc<Session>,
}

/// An add, checked and ready to hand to librqbit.
struct Prepared {
    add: AddTorrent<'static>,
    info_hash: String,
    name: Option<String>,
    /// The folder its files go into: a torrent of several files gets its own folder inside it.
    destination: AbsolutePath,
}

impl Engine {
    /// Starts the session and the actor, resuming the torrents saved in the data dir with the
    /// ids they had; those wanted running run again.
    pub async fn start(config: Config) -> Result<Self> {
        Self::start_with(config, Arc::new(|session, id, add, destination, download_dir| {
            Box::pin(actor::add_into(session, id, add, destination, download_dir))
        }))
        .await
    }

    /// [`Engine::start`], with adds reaching librqbit through `add_into`.
    async fn start_with(config: Config, add_into: actor::AddInto) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp, offline, size_check_every } = config;
        let mut store = Store::load(&data_dir)?;
        // Before librqbit restores the torrents, which creates their files: not where a drive
        // isn't connected, and missing ones not again, empty.
        let unavailable = unavailable_at_start(&store);
        let guard = files::Guard::default();
        for (hash, saved) in store.iter() {
            guard.set_destination(hash, saved.destination.clone());
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

        let (tx, inbox) = mpsc::channel(64);
        let actor = Actor::new(
            actor::Setup { session: session.clone(), data_dir, download_dir, guard, size_check_every, inbox: tx.downgrade(), add_into },
            store,
            missing,
            unavailable,
        );
        tokio::spawn(actor.run(inbox));
        Ok(Self { actor: tx, session })
    }

    /// Every torrent, including those fetching info, by id.
    pub async fn listing(&self) -> Vec<TorrentInfo> {
        self.ask(Message::List).await.unwrap_or_default()
    }

    /// What `proto::Request::Add` says it does, into `destination`.
    pub async fn add(&self, source: AddSource, paused: bool, destination: Destination) -> Result<Added> {
        let destination = destination.into_path();
        let source = match source {
            AddSource::Url(url) if url.starts_with("http://") || url.starts_with("https://") => {
                AddSource::TorrentFile(download(&self.session, url).await?)
            }
            source => source,
        };
        let prepared = prepare(source, destination)?;
        self.ask(|reply| Message::Add { prepared, paused, reply }).await?
    }

    /// What `proto::TorrentCommand` says it does; refused with a [`Refused`].
    pub async fn command(&self, id: TorrentId, command: TorrentCommand) -> Result<()> {
        self.ask(|reply| Message::Command { id, command, reply }).await?
    }

    pub async fn details(&self, id: TorrentId) -> Result<Details> {
        self.ask(|reply| Message::Details { id, reply }).await?
    }

    /// Records the missing files, pauses running torrents so librqbit saves them paused, and stops
    /// the session. Those wanted running resume on the next start.
    pub async fn stop(&self) {
        let _ = self.ask(Message::Stop).await;
    }

    /// Sends the actor a message and waits for its answer.
    async fn ask<T>(&self, message: impl FnOnce(oneshot::Sender<T>) -> Message) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        let stopped = || anyhow!("the service is stopping");
        self.actor.send(message(reply)).await.map_err(|_| stopped())?;
        answer.await.map_err(|_| stopped())
    }

    /// Runs `f` on the actor and returns what it returns.
    #[cfg(test)]
    async fn inspect<T: Send + 'static>(&self, f: impl FnOnce(&mut Actor) -> T + Send + 'static) -> T {
        self.ask(|reply| Message::Inspect(Box::new(move |actor| drop(reply.send(f(actor)))))).await.unwrap()
    }
}

/// Why the destination of each torrent (by info hash) can't be reached now, for those whose can't.
fn unavailable_at_start(store: &Store) -> BTreeMap<String, Unavailable> {
    store
        .iter()
        .filter_map(|(hash, saved)| Some((hash.clone(), files::unavailable(saved.destination.as_ref()?)?)))
        .collect()
}

/// The files of each torrent (by info hash) missing since the service last stopped, with what
/// librqbit had of each: those that were, and those it was getting that aren't on disk now.
/// Takes what the store recorded at stop. Those whose destination is `unavailable` aren't missing.
fn missing_since_shutdown(store: &mut Store, unavailable: &BTreeMap<String, Unavailable>) -> BTreeMap<String, BTreeMap<usize, u64>> {
    store
        .iter_mut()
        .map(|(hash, saved)| {
            if unavailable.contains_key(hash) {
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

/// The .torrent file at `url`, downloaded and checked by librqbit's list-only add, which registers
/// nothing. A failure fails the add.
async fn download(session: &Arc<Session>, url: String) -> Result<Vec<u8>> {
    let options = AddTorrentOptions { list_only: true, ..Default::default() };
    match session.add_torrent(AddTorrent::from_url(url), Some(options)).await? {
        AddTorrentResponse::ListOnly(listed) => Ok(listed.torrent_bytes.to_vec()),
        _ => bail!("unexpected response to a list-only add"),
    }
}

/// A magnet link or a .torrent file's bytes, with its info hash.
fn prepare(source: AddSource, destination: AbsolutePath) -> Result<Prepared> {
    Ok(match source {
        AddSource::Url(url) => {
            let magnet = Magnet::parse(&url).map_err(|_| anyhow!("not a magnet link or .torrent URL"))?;
            let info_hash = magnet.as_id20().context("the magnet link has no BitTorrent v1 info hash")?.as_string();
            Prepared { info_hash, name: magnet.name, add: AddTorrent::from_url(url), destination }
        }
        AddSource::TorrentFile(bytes) => {
            let torrent = torrent_from_bytes(&bytes).context("not a valid .torrent file")?;
            let info_hash = torrent.info_hash.as_string();
            let name = torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
            Prepared { add: AddTorrent::from_bytes(bytes), info_hash, name, destination }
        }
    })
}

/// A refusal on its way to the client, which words it: the service only logs it, as `Debug`.
#[derive(Debug)]
pub struct Refused(pub proto::Refusal);

impl Refused {
    pub fn torrent(id: TorrentId, why: proto::TorrentRefusal) -> Self {
        Self(proto::Refusal::Torrent { id, why })
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::error::Error for Refused {}

fn id20(hash: &str) -> Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("bad info hash {hash}"))
}
