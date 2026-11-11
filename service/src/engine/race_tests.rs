//! Races between the actor and its add tasks, the one background task that touches librqbit (the
//! actor awaits every other librqbit call itself, and the watcher only sends it messages). Each
//! add waits at two gates the test opens: before librqbit has the torrent, and after it has it but
//! before the actor hears so. Generated sequences of adds, gate openings, commands, destinations
//! going and coming back (also just as librqbit creates a torrent's files), and restarts run on a
//! real librqbit session in a temp dir, offline. One pack's destination is on a drive: the internal
//! disk by its link under `/Volumes`.
//! After every step:
//! - every command was answered, and Delete refused for a torrent with info whose destination is
//!   unavailable, however soon the engine looked;
//! - librqbit holds nothing the actor doesn't know about;
//! - each torrent's librqbit side agrees with its phase: adding while fetching info, never added
//!   once that failed, taken out only while it needn't run (an Error, or missing files or its
//!   destination, until checked); one librqbit has is the one in the session; an add task
//!   exactly while adding; its content listed exactly once it has info;
//! - the snapshot clients read is the one the actor's state makes;
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

use librqbit::api::TorrentIdOrHash;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{AbsolutePath, AddSource, Status, TorrentCommand, TorrentId};
use tempfile::TempDir;
use tokio::sync::oneshot;

use super::actor::Actor;
use super::actor::{AddInto, add_into};
use super::files::{self, Guard, InitIn};
use super::machine::{Phase, inlib};
use super::{Config as EngineConfig, Destination, Engine};

const PACKS: usize = 2;
/// How long a call may take to be answered, and the engine to settle.
const SETTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
enum Step {
    Add {
        pack: usize,
        paused: bool,
    },
    /// Opens the next gate of the pack's add, if one is waiting.
    Open {
        pack: usize,
    },
    Pause {
        pack: usize,
    },
    Resume {
        pack: usize,
    },
    Remove {
        pack: usize,
        delete: bool,
    },
    /// The pack's destination folder is moved away, as a drive is disconnected.
    DestinationGone {
        pack: usize,
    },
    /// And moved back.
    DestinationBack {
        pack: usize,
    },
    /// The next time librqbit creates the pack's files, its destination is moved away first, or
    /// while librqbit creates them.
    GoesWhileCreated {
        pack: usize,
        during: bool,
    },
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

/// The packs' destinations, shared with librqbit creating the files.
#[derive(Default)]
struct Destinations {
    /// Each pack's destination is unavailable.
    unavailable: [bool; PACKS],
    /// The pack's destination goes the next time librqbit creates its files: during `init` or
    /// before.
    goes: [Option<bool>; PACKS],
}

type Disk = Arc<Mutex<Destinations>>;

/// [`files::init_in`], with each pack's destination going as [`Step::GoesWhileCreated`] said.
/// Never waits: while the test holds the destinations, librqbit just creates the files.
fn going(disk: Disk, dir: std::path::PathBuf) -> InitIn {
    Arc::new(move |path, init| {
        let Ok(mut disk) = disk.try_lock() else { return files::init_in(path, init) };
        let Some(pack) = (0..PACKS).find(|&k| destination(&dir, k) == path) else { return files::init_in(path, init) };
        let goes = disk.goes[pack].take().filter(|_| !disk.unavailable[pack]);
        let go = || std::fs::rename(destination(&dir, pack), moved_away(&dir, pack)).unwrap();
        disk.unavailable[pack] |= goes.is_some();
        match goes {
            Some(false) => {
                go();
                files::init_in(path, init)
            }
            Some(true) => files::init_in(path, &mut || {
                go();
                init()
            }),
            None => files::init_in(path, init),
        }
    })
}

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
    disk: Disk,
}

impl Sut {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sources = (0..PACKS)
            .map(|k| {
                let folder = destination(dir.path(), k).join(format!("pack{k}"));
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::create_dir_all(folder.join("sub")).unwrap();
                std::fs::write(folder.join("a.bin"), vec![k as u8; 16384]).unwrap();
                std::fs::write(folder.join("sub/b.bin"), vec![k as u8; 16384]).unwrap();
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
        let mut sut =
            Self { rt, dir, engine: None, adds: Adds::default(), sources, model: BTreeMap::new(), next_id: 0, disk: Disk::default() };
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
        let guard = Guard::new(going(self.disk.clone(), self.dir.path().to_owned()));
        self.engine = Some(self.rt.block_on(Engine::start_with(config, gated(self.adds.clone()), guard)).unwrap());
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    /// Runs a client call, which must be answered.
    fn call<T, E>(&self, call: impl Future<Output = Result<T, E>>) -> Result<T, E> {
        self.rt.block_on(async { tokio::time::timeout(SETTLE, call).await.expect("every command is answered") })
    }

    fn command(&self, pack: usize, command: TorrentCommand) {
        if let Some(&(id, ..)) = self.model.get(&pack) {
            let _ = self.call(self.engine().command(id, command));
        }
    }

    fn apply(&mut self, step: &Step) {
        // Held through a step that depends on where the destinations are, so none goes meanwhile.
        let disk = self.disk.clone();
        let mut disk = disk.lock().unwrap();
        match *step {
            Step::Add { pack, paused } => {
                let source = AddSource::TorrentFile(self.sources[pack].clone());
                // Parsed at the edge, as the service does: one that can't be reached is refused there.
                let parsed = Destination::parse(AbsolutePath::new(destination(self.dir.path(), pack)).unwrap());
                if disk.unavailable[pack] {
                    assert!(parsed.is_err(), "an add into a destination that can't be reached is refused");
                    return;
                }
                let added = self.call(self.engine().add(super::Fetched::local(source), paused, parsed.unwrap())).unwrap();
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
            Step::Open { pack } => {
                drop(disk);
                self.open(pack)
            }
            Step::Pause { pack } => self.command(pack, TorrentCommand::Pause),
            Step::Resume { pack } => {
                drop(disk);
                self.command(pack, TorrentCommand::Resume)
            }
            Step::Remove { pack, delete } => {
                self.command(pack, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove });
                // Delete is refused for a torrent with info whose destination is unavailable.
                let refused = delete && disk.unavailable[pack] && self.model.get(&pack).is_some_and(|&(_, stage, _)| stage == Stage::Told);
                if !refused {
                    self.model.remove(&pack);
                }
            }
            Step::DestinationGone { pack } if !disk.unavailable[pack] => {
                std::fs::rename(destination(self.dir.path(), pack), moved_away(self.dir.path(), pack)).unwrap();
                disk.unavailable[pack] = true;
            }
            Step::DestinationBack { pack } if disk.unavailable[pack] => {
                std::fs::rename(moved_away(self.dir.path(), pack), destination(self.dir.path(), pack)).unwrap();
                disk.unavailable[pack] = false;
            }
            Step::DestinationGone { .. } | Step::DestinationBack { .. } => {}
            Step::GoesWhileCreated { pack, during } => disk.goes[pack] = Some(during),
            Step::Restart => {
                drop(disk);
                self.rt.block_on(self.engine().stop());
                self.start();
                // Fetching info doesn't survive a restart; the actor knows the rest once librqbit
                // restores them. One being added may or may not have made it.
                let listed: BTreeSet<TorrentId> =
                    self.call(async { anyhow::Ok(self.engine().listing().await) }).unwrap().iter().map(|t| t.id).collect();
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
                let id = self.model[&pack].0;
                drop(adds);
                // Until the actor hears, it's as if the gate were shut.
                let deadline = Instant::now() + SETTLE;
                while self.rt.block_on(self.engine().inspect(move |actor| actor.torrents[&id].lifecycle.phase == Phase::FetchingInfo)) {
                    assert!(Instant::now() < deadline, "the actor never heard");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Stage::Told => unreachable!(),
        }
    }

    /// librqbit holds nothing the actor doesn't know about, the list is the model's, and the
    /// snapshot clients read is the one the actor's state makes.
    fn check(&self) {
        let (unknown, ids, published, current) = self.rt.block_on(self.engine().inspect(|actor| {
            let hashes: Vec<String> = actor.session.with_torrents(|ts| ts.map(|(_, t)| t.info_hash().as_string()).collect());
            let unknown: Vec<String> =
                hashes.into_iter().filter(|h| !actor.store.id(h).is_some_and(|id| actor.torrents.contains_key(&id))).collect();
            (unknown, actor.torrents.keys().copied().collect::<BTreeSet<_>>(), actor.published.borrow().snapshot.clone(), actor.snapshot())
        }));
        assert_eq!(unknown, Vec::<String>::new(), "torrents in librqbit the actor doesn't know about");
        let disagree = self.rt.block_on(self.engine().inspect(librqbit_side_disagrees));
        assert_eq!(disagree, Vec::<String>::new(), "librqbit sides that disagree with their phase");
        assert_eq!(published, current, "the published snapshot isn't the actor's state");
        let want: BTreeSet<TorrentId> = self.model.values().map(|(id, ..)| *id).collect();
        assert_eq!(ids, want, "the list");
        let disk = self.disk.lock().unwrap();
        for pack in (0..PACKS).filter(|&k| disk.unavailable[k]) {
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

/// The torrents whose librqbit side and phase disagree, and why.
fn librqbit_side_disagrees(actor: &mut Actor) -> Vec<String> {
    let mut wrong = Vec::new();
    let listed = actor.snapshot().torrents;
    for (id, t) in &actor.torrents {
        let phase = &t.lifecycle.phase;
        let no_info = matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. });
        if listed.iter().find(|l| l.id == *id).is_none_or(|l| l.content.is_none() != no_info) {
            wrong.push(format!("{id}: {phase:?}, listed with content only once it has info"));
        }
        let agrees = match &t.inlib {
            inlib::State::Adding { .. } => *phase == Phase::FetchingInfo,
            inlib::State::NotAdded { .. } => matches!(phase, Phase::FetchFailed { .. }),
            inlib::State::Has(h) => {
                let in_session = actor.session.get(TorrentIdOrHash::Id(h.id())).is_some_and(|s| Arc::ptr_eq(&s, h));
                in_session && !matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. })
            }
            inlib::State::TakenOut(_) => {
                matches!(phase, Phase::Error { .. } | Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. })
            }
        };
        let adding = matches!(t.inlib, inlib::State::Adding { .. });
        if !agrees || adding != actor.adds.contains_key(id) {
            wrong.push(format!("{id}: {phase:?}, librqbit side {:?}, add task {}", kind(&t.inlib), actor.adds.contains_key(id)));
        }
    }
    wrong.extend(actor.adds.keys().filter(|id| !actor.torrents.contains_key(id)).map(|id| format!("{id}: an add task off the list")));
    wrong
}

fn kind<H>(s: &inlib::State<H>) -> &'static str {
    match s {
        inlib::State::Adding { .. } => "Adding",
        inlib::State::NotAdded { .. } => "NotAdded",
        inlib::State::Has(_) => "Has",
        inlib::State::TakenOut(_) => "TakenOut",
    }
}

impl Drop for Sut {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            self.rt.block_on(engine.stop());
        }
    }
}

/// Pack 1's is on a drive.
fn destination(dir: &Path, pack: usize) -> std::path::PathBuf {
    let on = if pack == 1 { Path::new("/Volumes/Macintosh HD").join(dir.strip_prefix("/").unwrap()) } else { dir.to_owned() };
    on.join(format!("destination{pack}"))
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
        1 => pack.clone().prop_map(|pack| Step::DestinationBack { pack }),
        1 => (pack, any::<bool>()).prop_map(|(pack, during)| Step::GoesWhileCreated { pack, during }),
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
    run(&[
        Add { pack: 0, paused: false },
        Add { pack: 1, paused: false },
        Open { pack: 0 },
        Open { pack: 1 },
        Open { pack: 0 },
        Open { pack: 1 },
        Restart,
    ]);
    run(&[
        Add { pack: 0, paused: true },
        Add { pack: 1, paused: true },
        Open { pack: 0 },
        Open { pack: 1 },
        Open { pack: 0 },
        Open { pack: 1 },
        Restart,
    ]);
}

#[test]
fn a_torrent_deleted_while_its_destination_is_gone_stays_however_soon_the_engine_looked() {
    run(&[
        Add { pack: 0, paused: false },
        Open { pack: 0 },
        Open { pack: 0 },
        Open { pack: 0 },
        DestinationGone { pack: 0 },
        Remove { pack: 0, delete: true },
    ]);
    run(&[
        Add { pack: 0, paused: false },
        DestinationGone { pack: 0 },
        Open { pack: 0 },
        Open { pack: 0 },
        Restart,
        Remove { pack: 0, delete: true },
    ]);
}
