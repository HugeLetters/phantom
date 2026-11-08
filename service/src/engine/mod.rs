//! The torrents, in Phantom's terms: [`Engine`] starts a session, adds (each into the destination
//! it's given), lists, pauses, resumes, checks, removes, deletes and stops them, and shows and sets
//! the priorities of their files (holding a change made during a check until the check is done).
//! Notices files deleted outside Phantom: the torrent stops and is Missing files; and destinations
//! that can't be reached: it stops and is Destination unavailable until they're back. Owns
//! torrent ids, de-duplicates adds by
//! info hash, and keeps each torrent's Status as a state machine ([ADR 0006]). The only code that
//! knows librqbit; the server hands it requests and gets back `proto` types.
//!
//! [`Engine`] is a handle: every call is a message to one actor task (`actor.rs`) that owns all
//! the torrents and handles messages one at a time. `lifecycle.rs` is the state machine's table,
//! `store.rs` what's kept across restarts, `files.rs` how missing files are found.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

mod actor;
mod files;
mod lifecycle;
#[cfg(test)]
mod model_tests;
mod store;

pub use files::Destination;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, DhtSessionConfig, ListenerOptions, Magnet, Session, SessionOptions, SessionPersistenceConfig, torrent_from_bytes};
use proto::{AddSource, Added, Details, Priority, TorrentId, TorrentInfo, Unavailable};
use tokio::sync::{mpsc, oneshot};

use actor::{Actor, Answer, Command, Message};
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
    /// How often every torrent's files are looked for on disk, besides when the watcher sees
    /// a path go.
    pub check_files_every: Duration,
}

// Why the engine owns torrent ids: in the user's terms a torrent exists from the moment it's
// added, even while a magnet's info is still being fetched, and it must keep one id throughout
// so clients can show, select and remove it. librqbit only numbers a torrent once its info
// arrives, and renumbers the torrents it restores, so its ids can't be ours.
//
// So every torrent gets a Phantom id when it's added. The store keeps each info hash's id, so
// ids survive restarts. A .torrent URL has no info hash until the file is downloaded: until
// then it has just the id, and the hash is recorded (or found to be a duplicate) once librqbit
// has the torrent.
//
// Torrents still fetching info don't survive a restart: librqbit only saves a torrent once its
// info arrives. Their ids are dropped when the service starts again.
pub struct Engine {
    actor: mpsc::Sender<Message>,
}

/// An add, checked and ready to hand to librqbit.
struct Prepared {
    add: AddTorrent<'static>,
    info_hash: Option<String>,
    name: Option<String>,
    /// A magnet answers at once; .torrent bytes are added quickly and a URL has to be
    /// downloaded before it can be checked for duplicates, so those answer when librqbit has them.
    answer_at_once: bool,
    /// The folder its files go into: a torrent of several files gets its own folder inside it.
    destination: PathBuf,
}

impl Engine {
    /// Starts the session and the actor, resuming the torrents saved in the data dir with the
    /// ids they had; those running when the service stopped run again.
    pub async fn start(config: Config) -> Result<Self> {
        let Config { data_dir, download_dir, listen_port, upnp, offline, check_files_every } = config;
        let mut store = Store::load(&data_dir)?;
        // Before librqbit restores the torrents, which creates their files: not where a drive
        // isn't connected, and missing ones not again, empty.
        let away = away_at_start(&store);
        let guard = files::Guard::default();
        for hash in away.keys() {
            guard.set_away(id20(hash)?, true);
        }
        let missing = missing_since_shutdown(&mut store, &away);
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
            actor::Setup { session, data_dir, download_dir, guard, check_files_every, inbox: tx.downgrade() },
            store,
            missing,
            away,
        );
        tokio::spawn(actor.run(inbox));
        Ok(Self { actor: tx })
    }

    /// Every torrent, including those fetching info, by id.
    pub async fn listing(&self) -> Vec<TorrentInfo> {
        self.ask(Message::List).await.unwrap_or_default()
    }

    /// Adds the torrent into `destination`, or answers with the one already in the list with the
    /// same info hash.
    pub async fn add(&self, source: AddSource, paused: bool, destination: Destination) -> Result<Added> {
        let prepared = prepare(source, destination.into_path().into_path_buf())?;
        self.ask(|reply| Message::Add { prepared, paused, reply }).await?
    }

    /// The torrent's files with their progress and priority, and where it is on disk.
    pub async fn details(&self, id: TorrentId) -> Result<Details> {
        match self.command(id, Command::Details).await? {
            Answer::Details(details) => Ok(details),
            Answer::Done => unreachable!("details are answered with details"),
        }
    }

    /// Sets the priority of the files at `indices`. Every file Off is fine: the torrent is then
    /// finished. While the torrent is being checked the change is held (across restarts too)
    /// and shows at once; it's applied when the check is done.
    pub async fn set_priority(&self, id: TorrentId, indices: &[usize], priority: Priority) -> Result<()> {
        self.command(id, Command::SetPriority { files: indices.to_vec(), priority }).await.map(drop)
    }

    pub async fn pause(&self, id: TorrentId) -> Result<()> {
        self.command(id, Command::Pause).await.map(drop)
    }

    /// Starts the torrent. One that's Missing files is checked first, then downloads them again.
    pub async fn resume(&self, id: TorrentId) -> Result<()> {
        self.command(id, Command::Resume).await.map(drop)
    }

    /// Checks the torrent's files on disk, keeping what's valid; it's then downloaded or seeded as
    /// before, or stays stopped. A torrent that's Missing files stays stopped too.
    pub async fn check(&self, id: TorrentId) -> Result<()> {
        self.command(id, Command::Check).await.map(drop)
    }

    /// Takes the torrent off the list, keeping its files. A torrent fetching info stops.
    pub async fn remove(&self, id: TorrentId) -> Result<()> {
        self.command(id, Command::Remove).await.map(drop)
    }

    /// Removes the torrent and deletes its files. A torrent fetching info has none yet: it stops.
    pub async fn delete(&self, id: TorrentId) -> Result<()> {
        self.command(id, Command::Delete).await.map(drop)
    }

    /// Records which torrents are running, pauses them so librqbit saves them paused, and stops
    /// the session. They resume on the next start.
    pub async fn stop(&self) {
        let _ = self.ask(Message::Stop).await;
    }

    async fn command(&self, id: TorrentId, command: Command) -> Result<Answer> {
        self.ask(|reply| Message::Command { id, command, reply }).await?
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
fn away_at_start(store: &Store) -> BTreeMap<String, Unavailable> {
    store
        .iter()
        .filter_map(|(hash, saved)| Some((hash.clone(), files::unavailable(saved.destination.as_ref()?)?)))
        .collect()
}

/// The files of each torrent (by info hash) missing since the service last stopped, with what
/// librqbit had of each: those that were, and those it was getting that aren't on disk now.
/// Takes what the store recorded at stop. Those whose destination is `away` aren't missing.
fn missing_since_shutdown(store: &mut Store, away: &BTreeMap<String, Unavailable>) -> BTreeMap<String, BTreeMap<usize, u64>> {
    store
        .iter_mut()
        .map(|(hash, saved)| {
            if away.contains_key(hash) {
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

fn prepare(source: AddSource, destination: PathBuf) -> Result<Prepared> {
    Ok(match source {
        AddSource::Url(url) if url.starts_with("http://") || url.starts_with("https://") => {
            let name = url_file_name(&url);
            Prepared { add: AddTorrent::from_url(url), info_hash: None, name, answer_at_once: false, destination }
        }
        AddSource::Url(url) => {
            let magnet = Magnet::parse(&url).map_err(|_| anyhow!("not a magnet link or .torrent URL"))?;
            let info_hash = magnet.as_id20().context("the magnet link has no BitTorrent v1 info hash")?;
            Prepared {
                info_hash: Some(info_hash.as_string()),
                name: magnet.name,
                add: AddTorrent::from_url(url),
                answer_at_once: true,
                destination,
            }
        }
        AddSource::TorrentFile(bytes) => {
            let torrent = torrent_from_bytes(&bytes).context("not a valid .torrent file")?;
            let info_hash = Some(torrent.info_hash.as_string());
            let name = torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned());
            Prepared { add: AddTorrent::from_bytes(bytes), info_hash, name, answer_at_once: false, destination }
        }
    })
}

/// The name a .torrent URL implies: its file name without the extension.
fn url_file_name(url: &str) -> Option<String> {
    let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
    file.map(|f| f.trim_end_matches(".torrent").to_owned())
}

/// A refusal on its way to the client, which words it: the service only logs it, as `Debug`.
#[derive(Debug)]
pub struct Refused(pub proto::Refusal);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::error::Error for Refused {}

fn id20(hash: &str) -> Result<Id20> {
    Id20::from_str(hash).with_context(|| format!("bad info hash {hash}"))
}
