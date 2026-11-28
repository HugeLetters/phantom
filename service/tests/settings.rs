//! Every setting through a real service, end to end through `client`, over generated sequences of
//! adds, pauses and resumes, setting changes, restarts of the service and clients connecting while
//! the engine restarts, against a model. After every step:
//! - what's set reads back, and a refused change (a folder that doesn't exist, a speed limit out of
//!   range) stored nothing;
//! - every torrent is still there and as wanted (running or stopped), and its progress never
//!   went down, across engine restarts (a new port or UPnP) and service restarts;
//! - exactly one librqbit session runs: the service listens for peers on one port, the one it
//!   says, which is the port set unless another program has it;
//! - a client that connects while the engine restarts is answered, never refused as stopping, and
//!   meets the same service;
//! - the service keeps the Mac awake (macOS lists its power assertion) exactly while the setting is
//!   on and a torrent is downloading, fetching info or checking, and no longer once it's stopping, before it exits.
//!
//! Every case ends the same way: a download limit set and an upload limit refused, kept across a
//! restart; then a torrent downloading, the setting turned off and on again, and a stop. So whatever
//! was generated, each case shows a limit applied and kept, the actor hearing of the setting, and
//! releasing at a stop.

use std::collections::BTreeMap;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use client::setting::{self, ValueProblem};
use client::{Client, Dirs, Failure};
use proptest::prelude::*;
use proto::{
    AbsolutePath, AddSource, Direction, ListenPort, Ratio, Refusal, SeedTime, SeedingLimit, SeedingLimits, Setting, Settings, SpeedLimit,
    SpeedLimits, Status, TorrentCommand, TorrentId, Unavailable,
};

/// Torrents to add, each a folder of two files; the odd ones miss a file on disk, so they download.
const PACKS: usize = 4;
const PIECE: usize = 16 * 1024;

#[derive(Debug, Clone)]
enum Step {
    Add {
        pack: usize,
        paused: bool,
    },
    /// Pause or resume the torrent added `n`th.
    Pause(usize),
    Resume(usize),
    /// One of the folders, which exist: never restarts the engine.
    Destination(Option<usize>),
    /// A folder that doesn't exist: refused.
    MissingDestination,
    Port(Option<NonZeroU16>),
    /// Text that isn't a port, refused before it's sent.
    BadPort(String),
    Upnp(Option<bool>),
    KeepAwake(Option<bool>),
    /// A ratio in hundredths or a time in minutes, in range or not; `None`: never.
    Seeding {
        at: SeedingLimit,
        value: Option<u64>,
    },
    /// Bytes per second, in range or not; `None`: unlimited.
    Limit {
        direction: Direction,
        bytes_per_sec: Option<u64>,
    },
    /// A new port, with another client connecting again and again until it's answered.
    ConnectDuringRestart(NonZeroU16),
    /// Pin a port another program has, so the service listens on a temporary one; change the
    /// folder, which mustn't restart the engine; then let the port go and the service pick it.
    FolderWhilePinnedTaken {
        port: NonZeroU16,
        folder: usize,
    },
    /// Stop the service and start a new one.
    Restart,
}

fn port() -> impl Strategy<Value = NonZeroU16> {
    // A range nothing on a test machine listens on, as a rule.
    (40_000..41_000u16).prop_map(|p| NonZeroU16::new(p).unwrap())
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0..PACKS, any::<bool>()).prop_map(|(pack, paused)| Step::Add { pack, paused }),
        1 => (0..PACKS).prop_map(Step::Pause),
        1 => (0..PACKS).prop_map(Step::Resume),
        1 => proptest::option::of(0..2usize).prop_map(Step::Destination),
        1 => Just(Step::MissingDestination),
        2 => proptest::option::of(port()).prop_map(Step::Port),
        1 => prop_oneof!["[a-z]{1,5}", "[7-9][0-9]{4}", Just("0".to_owned())].prop_map(Step::BadPort),
        2 => proptest::option::of(any::<bool>()).prop_map(Step::Upnp),
        2 => proptest::option::of(any::<bool>()).prop_map(Step::KeepAwake),
        3 => (
            prop_oneof![Just(Direction::Download), Just(Direction::Upload)],
            proptest::option::of(prop_oneof![
                3 => SpeedLimit::MIN..=SpeedLimit::MAX,
                1 => 0..SpeedLimit::MIN,
                1 => SpeedLimit::MAX + 1..=u64::MAX,
            ]),
        )
            .prop_map(|(direction, bytes_per_sec)| Step::Limit { direction, bytes_per_sec }),
        2 => (
            prop_oneof![Just(SeedingLimit::Ratio), Just(SeedingLimit::Time)],
            proptest::option::of(prop_oneof![3 => 1..=1_000_000u64, 1 => Just(0u64), 1 => 6_000_000..=u64::MAX]),
        )
            .prop_map(|(at, value)| Step::Seeding { at, value }),
        1 => port().prop_map(Step::ConnectDuringRestart),
        1 => (port(), 0..2usize).prop_map(|(port, folder)| Step::FolderWhilePinnedTaken { port, folder }),
        1 => Just(Step::Restart),
    ]
}

/// What the settings should be, as far as the steps decide them.
#[derive(Debug, Clone, PartialEq)]
struct Stored {
    default_destination: Option<AbsolutePath>,
    listen_port: ListenPort,
    upnp: bool,
    keep_awake: bool,
    speed_limits: SpeedLimits,
    seeding: SeedingLimits,
}

impl Stored {
    fn of(settings: &Settings) -> Self {
        Self {
            default_destination: settings.default_destination.clone(),
            listen_port: settings.listen_port,
            upnp: settings.upnp,
            keep_awake: settings.keep_awake,
            speed_limits: settings.speed_limits,
            seeding: settings.seeding,
        }
    }
}

/// A torrent added: whether it's wanted running, and the most progress seen.
#[derive(Debug)]
struct Added {
    id: TorrentId,
    running: bool,
    progress: u64,
}

/// How many cases run at once, at most (the runners of `cases::side_by_side`), and the ports each
/// of them takes its generated ports from: its own window, as no two services of one machine can
/// listen on the same port.
const SLOTS: usize = 8;
const WINDOW: u16 = 1000 / SLOTS as u16;

/// Which of the windows each running case holds.
static SLOTS_TAKEN: std::sync::Mutex<[bool; SLOTS]> = std::sync::Mutex::new([false; SLOTS]);

struct Case {
    /// The window of ports this case's generated ports fall in.
    slot: usize,
    root: PathBuf,
    dirs: Dirs,
    client: Client,
    stored: Stored,
    /// By pack.
    added: BTreeMap<usize, Added>,
    /// In the order they were added.
    order: Vec<usize>,
    torrents: Vec<Vec<u8>>,
}

/// A client of the service of `dirs`, started if none runs. The case's dirs are named outright, not
/// through the environment, which cases running side by side in one process can't each have.
fn connect(dirs: &Dirs) -> Client {
    Client::connect_or_spawn_in(env!("CARGO_BIN_EXE_phantom-service").as_ref(), dirs).unwrap()
}

fn folder(root: &Path, i: usize) -> AbsolutePath {
    AbsolutePath::new(root.join(format!("folder-{i}"))).unwrap()
}

/// Two files of two pieces each in `content/pack-<i>`, and a .torrent of them whose tracker is a
/// closed local port; the odd packs then lose their second file, so they have something to get.
fn packs(root: &Path) -> Vec<Vec<u8>> {
    let runtime = client::with_fds_not_inherited(|| tokio::runtime::Runtime::new().unwrap());
    (0..PACKS)
        .map(|i| {
            let pack = root.join(format!("content/pack-{i}"));
            std::fs::create_dir_all(&pack).unwrap();
            for f in 0..2 {
                let bytes: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + i * 31 + f * 13) as u8).collect();
                std::fs::write(pack.join(format!("{f}.bin")), bytes).unwrap();
            }
            let options = librqbit::CreateTorrentOptions {
                name: None,
                trackers: vec!["http://127.0.0.1:1/announce".into()],
                piece_length: Some(PIECE as u32),
            };
            let created = runtime.block_on(async {
                let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
                librqbit::create_torrent(&pack, options, &spawner).await.unwrap()
            });
            if i % 2 == 1 {
                std::fs::remove_file(pack.join("1.bin")).unwrap();
            }
            created.as_bytes().unwrap().to_vec()
        })
        .collect()
}

/// How many times the services of this data dir restarted their engine, from their log.
fn engine_restarts(data: &Path) -> usize {
    let log = std::fs::read_to_string(data.join("service.log")).unwrap_or_default();
    log.matches("restarting the engine").count()
}

/// The limits the data dir's services last ran their sessions with, from their log.
fn limits_applied(data: &Path) -> Option<SpeedLimits> {
    let log = std::fs::read_to_string(data.join("service.log")).unwrap_or_default();
    let line = log.lines().rev().find(|l| l.contains("speed limits"))?;
    let bps = |name: &str| -> Option<SpeedLimit> {
        let value = line.split(&format!("{name}=")).nth(1)?.split_whitespace().next()?;
        SpeedLimit::new(value.parse().ok()?).ok()
    };
    Some(SpeedLimits { download: bps("download_bps"), upload: bps("upload_bps") })
}

/// The pid of the service holding the data dir's lock.
fn service_pid(data: &Path) -> String {
    let out = client::with_process_started(|| Command::new("lsof").args(["-t"]).arg(proto::lock_path_in(data)).output()).unwrap();
    String::from_utf8(out.stdout).unwrap().lines().next().expect("a service holds the lock").to_owned()
}

/// The TCP ports the service listens on: one librqbit session's peer listener.
fn listening(pid: &str) -> Vec<u16> {
    let out = client::with_process_started(|| Command::new("lsof").args(["-nP", "-a", "-p", pid, "-iTCP", "-sTCP:LISTEN", "-Fn"]).output())
        .unwrap();
    let mut ports: Vec<u16> =
        String::from_utf8(out.stdout).unwrap().lines().filter_map(|l| l.strip_prefix('n')?.rsplit(':').next()?.parse().ok()).collect();
    ports.sort();
    ports.dedup();
    ports
}

/// Whether macOS lists the service's assertion that keeps the Mac awake.
fn keeps_awake(pid: &str) -> bool {
    let out = client::with_process_started(|| Command::new("pmset").args(["-g", "assertions"]).output()).unwrap();
    let owner = format!("pid {pid}(");
    String::from_utf8(out.stdout).unwrap().lines().any(|l| l.contains(&owner) && l.contains("PreventUserIdleSystemSleep"))
}

impl Drop for Case {
    fn drop(&mut self) {
        SLOTS_TAKEN.lock().unwrap_or_else(|e| e.into_inner())[self.slot] = false;
    }
}

impl Case {
    fn new(root: &Path) -> Self {
        let slot = loop {
            let mut taken = SLOTS_TAKEN.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(free) = taken.iter().position(|t| !t) {
                taken[free] = true;
                break free;
            }
            drop(taken);
            std::thread::sleep(Duration::from_millis(20));
        };
        let dirs = Dirs { data: root.join("data"), download: root.join("downloads") };
        std::fs::create_dir_all(&dirs.data).unwrap();
        for i in 0..2 {
            std::fs::create_dir_all(folder(root, i)).unwrap();
        }
        let torrents = packs(root);
        let mut client = connect(&dirs);
        let stored = Stored::of(&client.settings().unwrap());
        Self { slot, root: root.to_owned(), dirs, client, stored, added: BTreeMap::new(), order: Vec::new(), torrents }
    }

    fn pid(&self) -> String {
        service_pid(&self.dirs.data)
    }

    /// The list once no torrent is checking: what a restart's done with them.
    fn settled(&mut self) -> Vec<proto::TorrentInfo> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let list = self.client.list().unwrap();
            let busy = list.iter().any(|t| matches!(t.status, Status::Checking | Status::FetchingInfo));
            if !busy || Instant::now() > deadline {
                return list;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn check(&mut self, settings: &Settings) -> Result<(), TestCaseError> {
        prop_assert_eq!(&Stored::of(settings), &self.stored, "what's stored");
        prop_assert_eq!(limits_applied(&self.dirs.data), Some(self.stored.speed_limits), "the limits the torrents run with");
        prop_assert_eq!(settings.listening_on, settings.listen_port.port(), "nothing else has the port");
        let pid = self.pid();
        prop_assert_eq!(listening(&pid), vec![settings.listening_on.get()], "one session, listening where it says");
        let list = self.settled();
        let mut ids: Vec<TorrentId> = list.iter().map(|t| t.id).collect();
        let mut expected: Vec<TorrentId> = self.added.values().map(|a| a.id).collect();
        ids.sort();
        expected.sort();
        prop_assert_eq!(ids, expected, "every torrent added, and no other");
        for added in self.added.values_mut() {
            let t = list.iter().find(|t| t.id == added.id).unwrap();
            let running = matches!(t.status, Status::Downloading | Status::Seeding);
            let stopped = matches!(t.status, Status::Paused | Status::Done { .. });
            prop_assert!(running || stopped, "torrent {} is {:?}", t.id, t.status);
            prop_assert_eq!(running, added.running, "torrent {} as wanted: {:?}", t.id, t.status);
            let progress = t.content.map_or(0, |c| c.progress_bytes);
            prop_assert!(progress >= added.progress, "torrent {}'s progress went down", t.id);
            added.progress = progress;
        }
        // The service hears of each snapshot on its own time.
        let downloading = list.iter().any(|t| matches!(t.status, Status::Downloading | Status::FetchingInfo | Status::Checking));
        let deadline = Instant::now() + Duration::from_secs(5);
        while keeps_awake(&pid) != (self.stored.keep_awake && downloading) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        prop_assert_eq!(keeps_awake(&pid), self.stored.keep_awake && downloading, "kept awake, downloading: {}", downloading);
        Ok(())
    }

    /// Stops the service. While it stops (its socket is still there, which it removes last), the
    /// Mac is no longer kept awake: the last look before the socket goes finds no assertion.
    fn stop(&mut self) -> Result<(), TestCaseError> {
        let pid = self.pid();
        self.client.shutdown().unwrap();
        let mut last = None;
        loop {
            let held = keeps_awake(&pid);
            if !proto::socket_in(&self.dirs.data).exists() {
                break;
            }
            last = Some(held);
            // Each look starts a process: not a busy loop, with the others running side by side.
            std::thread::sleep(Duration::from_millis(20));
        }
        prop_assert_ne!(last, Some(true), "still kept awake while the service stops");
        self.client.wait_closed().unwrap();
        Ok(())
    }

    /// A download limit set and an upload limit refused, then a restart; a torrent downloading (an
    /// odd pack, added or resumed), the setting off and on again, then a stop.
    fn end(&mut self) -> Result<(), TestCaseError> {
        self.run(Step::Limit { direction: Direction::Download, bytes_per_sec: Some(2 << 20) })?;
        self.run(Step::Limit { direction: Direction::Upload, bytes_per_sec: Some(8 << 10) })?;
        self.run(Step::Restart)?;
        self.run(Step::Add { pack: 1, paused: false })?;
        if !self.added[&1].running {
            let n = self.order.iter().position(|&p| p == 1).unwrap();
            self.run(Step::Resume(n))?;
        }
        self.run(Step::KeepAwake(Some(false)))?;
        self.run(Step::KeepAwake(Some(true)))?;
        prop_assert!(keeps_awake(&self.pid()), "kept awake before the stop");
        self.stop()
    }

    /// A generated port, folded into this case's window of ports.
    fn windowed(&self, port: NonZeroU16) -> NonZeroU16 {
        let offset = (port.get() - 40_000) % WINDOW;
        NonZeroU16::new(40_000 + self.slot as u16 * WINDOW + offset).unwrap()
    }

    fn run(&mut self, step: Step) -> Result<(), TestCaseError> {
        let step = match step {
            Step::Port(port) => Step::Port(port.map(|p| self.windowed(p))),
            Step::ConnectDuringRestart(port) => Step::ConnectDuringRestart(self.windowed(port)),
            Step::FolderWhilePinnedTaken { port, folder } => Step::FolderWhilePinnedTaken { port: self.windowed(port), folder },
            other => other,
        };
        let answer = match step {
            Step::Add { pack, paused } => {
                if !self.added.contains_key(&pack) {
                    let destination = AbsolutePath::new(self.root.join("content")).unwrap();
                    let source = AddSource::TorrentFile(self.torrents[pack].clone());
                    let added = self.client.add(source, paused, Some(destination)).unwrap();
                    self.added.insert(pack, Added { id: added.id, running: !paused, progress: 0 });
                    self.order.push(pack);
                }
                self.client.settings().unwrap()
            }
            Step::Pause(n) | Step::Resume(n) => {
                if let Some(&pack) = self.order.get(n) {
                    let resume = matches!(step, Step::Resume(_));
                    let added = self.added.get_mut(&pack).unwrap();
                    // Settled first: a check that's running ends at a pause.
                    let command = if resume { TorrentCommand::Resume } else { TorrentCommand::Pause };
                    self.client.command(added.id, command).unwrap();
                    added.running = resume;
                }
                self.client.settings().unwrap()
            }
            Step::Destination(i) => {
                let folder = i.map(|i| folder(&self.root, i));
                self.stored.default_destination = folder.clone();
                let pid = self.pid();
                let answer = self.client.set_setting(Setting::DefaultDestination(folder)).unwrap();
                prop_assert_eq!(self.pid(), pid);
                answer
            }
            Step::MissingDestination => {
                let path = AbsolutePath::new(self.root.join("missing")).unwrap();
                let refused = self.client.set_setting(Setting::DefaultDestination(Some(path.clone())));
                let expected = Refusal::DefaultDestination { reason: Unavailable::FolderMissing { path } };
                prop_assert!(matches!(&refused, Err(Failure::Refused(r)) if *r == expected), "{:?}", refused);
                self.client.settings().unwrap()
            }
            Step::Port(port) => {
                self.stored.listen_port = match (port, self.stored.listen_port) {
                    (Some(port), _) => ListenPort::Pinned(port),
                    (None, kept) => ListenPort::Picked(kept.port()),
                };
                self.client.set_setting(Setting::ListenPort(port)).unwrap()
            }
            Step::BadPort(text) => {
                prop_assert!(matches!(setting::port(&text), Err(ValueProblem::NotAPort | ValueProblem::PortOutOfRange)));
                self.client.settings().unwrap()
            }
            Step::Upnp(upnp) => {
                self.stored.upnp = upnp.unwrap_or(true);
                self.client.set_setting(Setting::Upnp(upnp)).unwrap()
            }
            Step::KeepAwake(on) => {
                self.stored.keep_awake = on.unwrap_or(false);
                self.client.set_setting(Setting::KeepAwake(on)).unwrap()
            }
            Step::Seeding { at, value } => {
                let set = self.client.set_setting(Setting::StopSeeding { at, value });
                let parsed = match at {
                    SeedingLimit::Ratio => value.map(Ratio::new).transpose().map(|r| self.stored.seeding.ratio = r),
                    SeedingLimit::Time => value.map(SeedTime::new).transpose().map(|t| self.stored.seeding.time = t),
                };
                if let Err(problem) = parsed {
                    let expected = Refusal::StopSeeding { at, problem };
                    prop_assert!(matches!(&set, Err(Failure::Refused(r)) if *r == expected), "{:?}", set);
                }
                self.client.settings().unwrap()
            }
            Step::Limit { direction, bytes_per_sec } => {
                let set = self.client.set_setting(Setting::SpeedLimit { direction, bytes_per_sec });
                match bytes_per_sec.map(SpeedLimit::new).transpose() {
                    Ok(limit) => self.stored.speed_limits.set(direction, limit),
                    Err(problem) => {
                        let expected = Refusal::SpeedLimit { direction, problem };
                        prop_assert!(matches!(&set, Err(Failure::Refused(r)) if *r == expected), "{:?}", set);
                    }
                }
                self.client.settings().unwrap()
            }
            Step::ConnectDuringRestart(port) => {
                self.stored.listen_port = ListenPort::Pinned(port);
                let pid = self.pid();
                let mut changer = connect(&self.dirs);
                let change = std::thread::spawn(move || changer.set_setting(Setting::ListenPort(Some(port))));
                let mut met = 0;
                while !change.is_finished() || met == 0 {
                    let started = Instant::now();
                    let mut other = connect(&self.dirs);
                    let listed = other.list();
                    prop_assert!(listed.is_ok(), "a client meeting the restart was answered: {:?}", listed.err());
                    prop_assert!(started.elapsed() < Duration::from_secs(30), "and not timed out");
                    met += 1;
                }
                prop_assert_eq!(self.pid(), pid, "the same service");
                change.join().unwrap().unwrap()
            }
            Step::FolderWhilePinnedTaken { port, folder: i } => {
                let held =
                    client::with_fds_not_inherited(|| std::net::TcpListener::bind((std::net::Ipv6Addr::UNSPECIFIED, port.get()))).unwrap();
                let pinned = self.client.set_setting(Setting::ListenPort(Some(port))).unwrap();
                prop_assert_ne!(pinned.listening_on, port, "a temporary port while it's taken");
                let restarts = engine_restarts(&self.dirs.data);
                let folder = folder(&self.root, i);
                let changed = self.client.set_setting(Setting::DefaultDestination(Some(folder.clone()))).unwrap();
                prop_assert_eq!(engine_restarts(&self.dirs.data), restarts, "a new folder doesn't restart the engine");
                prop_assert_eq!(changed.listening_on, pinned.listening_on, "and it listens where it did");
                prop_assert_eq!(listening(&self.pid()), vec![pinned.listening_on.get()]);
                drop(held);
                self.stored.default_destination = Some(folder);
                self.stored.listen_port = ListenPort::Picked(port);
                self.client.set_setting(Setting::ListenPort(None)).unwrap()
            }
            Step::Restart => {
                self.stop()?;
                self.client = connect(&self.dirs);
                self.client.settings().unwrap()
            }
        };
        self.check(&answer)?;
        let now = self.client.settings().unwrap();
        self.check(&now)
    }
}

#[test]
fn settings_and_torrents_survive_changes_and_restarts() {
    // Each case starts a service, and many steps restart its engine: a few cases go a long way.
    // Each case has its own dirs, so they run side by side.
    let config = ProptestConfig { cases: 6, failure_persistence: None, ..ProptestConfig::default() };
    let config = proptest::test_runner::contextualize_config(ProptestConfig {
        source_file: Some(file!()),
        test_name: Some(concat!(module_path!(), "::settings_and_torrents_survive_changes_and_restarts")),
        ..config
    });
    cases::side_by_side(
        config,
        || proptest::collection::vec(step(), 1..10),
        |steps| {
            let dir = tempfile::tempdir().unwrap();
            let mut case = Case::new(dir.path());
            let defaults = Stored {
                default_destination: None,
                listen_port: case.stored.listen_port,
                upnp: true,
                keep_awake: false,
                speed_limits: SpeedLimits::default(),
                seeding: SeedingLimits::default(),
            };
            prop_assert_eq!(&case.stored, &defaults, "defaults");
            prop_assert!(matches!(case.stored.listen_port, ListenPort::Picked(_)), "picked at the first start");
            let first = case.client.settings().unwrap();
            case.check(&first)?;
            for step in steps {
                case.run(step)?;
            }
            case.end()
        },
    );
}
