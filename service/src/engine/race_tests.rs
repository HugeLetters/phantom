//! Races between the actor and its add tasks, the one background task that touches librqbit (the
//! actor awaits every other librqbit call itself, and the watcher only sends it messages). Each
//! add waits at two gates the test opens: before librqbit has the torrent, and after it has it but
//! before the actor hears so. Generated sequences of adds, gate openings, commands, destinations
//! going and coming back, and restarts run on a real librqbit session in a temp dir, offline.
//! After every step:
//! - every command was answered;
//! - librqbit holds nothing the actor doesn't know about;
//! - nothing is created where a destination is unavailable;
//! - the list is what was added and not removed since: nothing removed comes back, and across a
//!   restart only torrents librqbit had come back (one fetching info doesn't survive one), with
//!   their ids.
//!
//! 16 cases per `cargo test`; `PROPTEST_CASES=256 cargo test -p service race` runs more.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{AbsolutePath, AddSource, Status, TorrentCommand, TorrentId};
use tempfile::TempDir;
use tokio::sync::oneshot;

use super::actor::{AddInto, add_into};
use super::{Config as EngineConfig, Destination, Engine};

const PACKS: usize = 2;
/// How long a call may take to be answered, and the engine to settle.
const SETTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
enum Step {
    Add { pack: usize, paused: bool },
    /// Opens the next gate of the pack's add, if one is waiting.
    Open { pack: usize },
    Pause { pack: usize },
    Resume { pack: usize },
    Remove { pack: usize, delete: bool },
    /// The pack's destination folder is moved away, as a drive is disconnected.
    DestinationGone { pack: usize },
    /// And moved back.
    DestinationBack { pack: usize },
    Restart,
}

/// Where the pack's add is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    /// librqbit doesn't have it.
    Waiting,
    /// librqbit is adding it.
    Adding,
    /// librqbit has it; the actor hasn't heard.
    InLibrqbit,
    /// The actor hears, or has heard.
    Told,
}

/// An add's gates, test side.
struct Gates {
    before: Option<oneshot::Sender<()>>,
    /// Answered once librqbit's add returns.
    in_librqbit: Option<oneshot::Receiver<()>>,
    after: Option<oneshot::Sender<()>>,
}

/// Each add's gates, in the order the actor started them.
type Adds = Arc<Mutex<Vec<Gates>>>;

/// [`add_into`] between two gates. A gate dropped unopened stays shut.
fn gated(adds: Adds) -> AddInto {
    Arc::new(move |session, id, add, destination, download_dir| {
        let (before, wait_before) = oneshot::channel();
        let (after, wait_after) = oneshot::channel();
        let (done, in_librqbit) = oneshot::channel();
        adds.lock().unwrap().push(Gates { before: Some(before), in_librqbit: Some(in_librqbit), after: Some(after) });
        Box::pin(async move {
            shut_unless_opened(wait_before).await;
            let added = add_into(session, id, add, destination, download_dir).await;
            let _ = done.send(());
            shut_unless_opened(wait_after).await;
            added
        })
    })
}

async fn shut_unless_opened(gate: oneshot::Receiver<()>) {
    if gate.await.is_err() {
        std::future::pending::<()>().await;
    }
}

struct Sut {
    rt: tokio::runtime::Runtime,
    dir: TempDir,
    engine: Option<Engine>,
    adds: Adds,
    /// Each pack's .torrent.
    sources: Vec<Vec<u8>>,
    /// What's in the list: by pack, its id, where its add is and which of `adds` it is.
    model: BTreeMap<usize, (TorrentId, Stage, usize)>,
    next_id: TorrentId,
    /// Each pack's destination is unavailable.
    unavailable: [bool; PACKS],
}

impl Sut {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sources = (0..PACKS)
            .map(|k| {
                let folder = destination(dir.path(), k).join(format!("pack{k}"));
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(folder.join("a.bin"), vec![k as u8; 16384]).unwrap();
                let options = librqbit::CreateTorrentOptions {
                    name: None,
                    trackers: vec!["http://127.0.0.1:1/announce".into()],
                    piece_length: Some(16384),
                };
                let created = rt.block_on(async {
                    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
                    librqbit::create_torrent(&folder, options, &spawner).await.unwrap()
                });
                created.as_bytes().unwrap().to_vec()
            })
            .collect();
        let mut sut = Self { rt, dir, engine: None, adds: Adds::default(), sources, model: BTreeMap::new(), next_id: 0, unavailable: [false; PACKS] };
        sut.start();
        sut
    }

    fn start(&mut self) {
        self.adds = Adds::default();
        let config = EngineConfig {
            data_dir: self.dir.path().join("data"),
            download_dir: self.dir.path().join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: Duration::from_millis(200),
        };
        self.engine = Some(self.rt.block_on(Engine::start_with(config, gated(self.adds.clone()))).unwrap());
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    /// Runs a client call, which must be answered.
    fn call<T>(&self, call: impl Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
        self.rt.block_on(async { tokio::time::timeout(SETTLE, call).await.expect("every command is answered") })
    }

    fn command(&self, pack: usize, command: TorrentCommand) {
        if let Some(&(id, ..)) = self.model.get(&pack) {
            let _ = self.call(self.engine().command(id, command));
        }
    }

    fn apply(&mut self, step: &Step) {
        match *step {
            Step::Add { pack, paused } => {
                let source = AddSource::TorrentFile(self.sources[pack].clone());
                // Parsed at the edge, as the service does: one that can't be reached is refused there.
                let parsed = Destination::parse(AbsolutePath::new(destination(self.dir.path(), pack)).unwrap());
                if self.unavailable[pack] {
                    assert!(parsed.is_err(), "an add into a destination that can't be reached is refused");
                    return;
                }
                let added = self.call(self.engine().add(source, paused, parsed.unwrap())).unwrap();
                match self.model.get(&pack) {
                    Some(&(id, ..)) => assert_eq!((added.id, added.already_existed), (id, true)),
                    None => {
                        assert_eq!((added.id, added.already_existed), (self.next_id, false));
                        let gates = self.adds.lock().unwrap().len() - 1;
                        self.model.insert(pack, (added.id, Stage::Waiting, gates));
                        self.next_id += 1;
                    }
                }
            }
            Step::Open { pack } => self.open(pack),
            Step::Pause { pack } => self.command(pack, TorrentCommand::Pause),
            Step::Resume { pack } => self.command(pack, TorrentCommand::Resume),
            Step::Remove { pack, delete } => {
                self.command(pack, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove });
                self.model.remove(&pack);
            }
            Step::DestinationGone { pack } if !self.unavailable[pack] => {
                std::fs::rename(destination(self.dir.path(), pack), moved_away(self.dir.path(), pack)).unwrap();
                self.unavailable[pack] = true;
            }
            Step::DestinationBack { pack } if self.unavailable[pack] => {
                std::fs::rename(moved_away(self.dir.path(), pack), destination(self.dir.path(), pack)).unwrap();
                self.unavailable[pack] = false;
            }
            Step::DestinationGone { .. } | Step::DestinationBack { .. } => {}
            Step::Restart => {
                self.rt.block_on(self.engine().stop());
                self.start();
                // Fetching info doesn't survive a restart; the actor knows the rest once librqbit
                // restores them. One being added may or may not have made it.
                let listed: BTreeSet<TorrentId> = self.call(async { Ok(self.engine().listing().await) }).unwrap().iter().map(|t| t.id).collect();
                self.model.retain(|_, (id, stage, _)| match stage {
                    Stage::Waiting => false,
                    Stage::Adding => listed.contains(id),
                    Stage::InLibrqbit | Stage::Told => true,
                });
                for (_, stage, _) in self.model.values_mut() {
                    *stage = Stage::Told;
                }
            }
        }
    }

    /// Opens the next gate of the pack's add: librqbit adds it, or (once it has) the actor hears.
    fn open(&mut self, pack: usize) {
        let Some((_, stage, add)) = self.model.get_mut(&pack) else { return };
        if *stage == Stage::Told {
            return;
        }
        let mut adds = self.adds.lock().unwrap();
        let gates = &mut adds[*add];
        match stage {
            Stage::Waiting => {
                let _ = gates.before.take().unwrap().send(());
                *stage = Stage::Adding;
            }
            Stage::Adding => {
                let in_librqbit = gates.in_librqbit.take().unwrap();
                drop(adds);
                self.rt.block_on(async { tokio::time::timeout(SETTLE, in_librqbit).await }).unwrap().unwrap();
                *stage = Stage::InLibrqbit;
            }
            Stage::InLibrqbit => {
                let _ = gates.after.take().unwrap().send(());
                *stage = Stage::Told;
            }
            Stage::Told => unreachable!(),
        }
    }

    /// librqbit holds nothing the actor doesn't know about, and the list is the model's.
    fn check(&self) {
        let (unknown, ids) = self.rt.block_on(self.engine().inspect(|actor| {
            let hashes: Vec<String> = actor.session.with_torrents(|ts| ts.map(|(_, t)| t.info_hash().as_string()).collect());
            let unknown: Vec<String> =
                hashes.into_iter().filter(|h| !actor.store.id(h).is_some_and(|id| actor.torrents.contains_key(&id))).collect();
            (unknown, actor.torrents.keys().copied().collect::<BTreeSet<_>>())
        }));
        assert_eq!(unknown, Vec::<String>::new(), "torrents in librqbit the actor doesn't know about");
        let want: BTreeSet<TorrentId> = self.model.values().map(|(id, ..)| *id).collect();
        assert_eq!(ids, want, "the list");
        for pack in (0..PACKS).filter(|&k| self.unavailable[k]) {
            assert!(!destination(self.dir.path(), pack).exists(), "a destination that's unavailable was created");
        }
    }

    /// Opens every gate and waits until every torrent has its info.
    fn finish(&mut self) {
        for pack in 0..PACKS {
            for _ in 0..3 {
                self.open(pack);
            }
        }
        let deadline = Instant::now() + SETTLE;
        while self.rt.block_on(self.engine().listing()).iter().any(|t| t.status == Status::FetchingInfo) {
            assert!(Instant::now() < deadline, "an add never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        self.check();
    }
}

impl Drop for Sut {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            self.rt.block_on(engine.stop());
        }
    }
}

fn destination(dir: &Path, pack: usize) -> std::path::PathBuf {
    dir.join(format!("destination{pack}"))
}

/// Where the pack's destination is while it's unavailable.
fn moved_away(dir: &Path, pack: usize) -> std::path::PathBuf {
    dir.join(format!("away{pack}"))
}

fn step() -> impl Strategy<Value = Step> {
    let pack = 0..PACKS;
    prop_oneof![
        3 => (pack.clone(), any::<bool>()).prop_map(|(pack, paused)| Step::Add { pack, paused }),
        4 => pack.clone().prop_map(|pack| Step::Open { pack }),
        1 => pack.clone().prop_map(|pack| Step::Pause { pack }),
        1 => pack.clone().prop_map(|pack| Step::Resume { pack }),
        2 => (pack.clone(), any::<bool>()).prop_map(|(pack, delete)| Step::Remove { pack, delete }),
        1 => pack.clone().prop_map(|pack| Step::DestinationGone { pack }),
        1 => pack.prop_map(|pack| Step::DestinationBack { pack }),
        1 => Just(Step::Restart),
    ]
}

/// 16 cases, or `PROPTEST_CASES`; shrinking is bounded, since every case runs a real engine.
fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(16);
    Config { cases, max_shrink_iters: 256, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn adds_racing_commands_and_restarts_leave_nothing_unknown_or_lost(steps in prop::collection::vec(step(), 1..25)) {
        run(&steps);
    }
}

fn run(steps: &[Step]) {
    let mut sut = Sut::new();
    for step in steps {
        sut.apply(step);
        sut.check();
    }
    sut.finish();
}

// The races suspected in #32, and the one these tests found.

use Step::*;

#[test]
fn a_torrent_removed_once_librqbit_has_it_but_before_the_actor_hears_is_taken_out() {
    run(&[Add { pack: 0, paused: false }, Open { pack: 0 }, Open { pack: 0 }, Remove { pack: 0, delete: false }]);
    run(&[Add { pack: 0, paused: false }, Open { pack: 0 }, Open { pack: 0 }, Open { pack: 0 }, Remove { pack: 0, delete: false }]);
}

#[test]
fn a_stop_once_librqbit_has_the_torrent_but_before_the_actor_hears_keeps_it() {
    run(&[Add { pack: 0, paused: false }, Open { pack: 0 }, Open { pack: 0 }, Restart]);
    run(&[Add { pack: 0, paused: true }, Open { pack: 0 }, Open { pack: 0 }, Open { pack: 0 }, Restart]);
}

#[test]
fn a_destination_gone_while_its_torrent_is_added_is_not_created() {
    run(&[Add { pack: 0, paused: false }, DestinationGone { pack: 0 }, Open { pack: 0 }, Open { pack: 0 }]);
}

#[test]
fn a_torrent_removed_while_librqbit_adds_it_into_a_destination_gone_creates_nothing() {
    run(&[Add { pack: 0, paused: false }, DestinationGone { pack: 0 }, Open { pack: 0 }, Remove { pack: 0, delete: false }]);
}

#[test]
fn a_destination_gone_before_a_stop_while_its_torrent_is_added_is_not_created_at_start() {
    run(&[Add { pack: 0, paused: false }, Open { pack: 0 }, Open { pack: 0 }, DestinationGone { pack: 0 }, Restart]);
}

#[test]
fn torrents_added_at_the_same_time_each_get_their_own_id_in_librqbit() {
    run(&[Add { pack: 0, paused: false }, Add { pack: 1, paused: false }, Open { pack: 0 }, Open { pack: 1 }, Open { pack: 0 }, Open { pack: 1 }, Restart]);
    run(&[Add { pack: 0, paused: true }, Add { pack: 1, paused: true }, Open { pack: 0 }, Open { pack: 1 }, Open { pack: 0 }, Open { pack: 1 }, Restart]);
}
