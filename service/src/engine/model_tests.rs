//! The engine against a reference model: generated sequences of client commands and of what
//! happens around it (files deleted, put back or changed by hand, destinations gone and back,
//! restarts), each case on a real
//! librqbit session in its own temp dir, offline. After every step the engine must settle where
//! the model says: the same torrents with the same ids and statuses, files and progress, and the
//! same files on disk. librqbit is taken to be correct; the model is what Phantom promises.
//!
//! The torrents are two packs of three files, each already complete in its own destination, and a
//! magnet whose info never arrives. Nothing downloads (no peers), so a torrent's progress is what
//! its checks found on disk.
//!
//! 16 cases per `cargo test`; `PROPTEST_CASES=256 cargo test -p service model` runs more.
//! Failing sequences, shrunk, are saved to `service/proptest-regressions/` and run first after.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{AbsolutePath, AddSource, Added, Priority, Status, TorrentId, Unavailable};
use tempfile::TempDir;

use super::files::Seen;
use super::{Config as EngineConfig, Destination, Engine};

const PACKS: usize = 2;
const FILES: usize = 3;
const NAMES: [&str; FILES] = ["a.bin", "b.bin", "c.bin"];
const PIECE: usize = 16384;
/// A file's size: two pieces.
const FULL: u64 = 2 * PIECE as u64;
const MAGNET: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000aa&tr=http://127.0.0.1:1/announce";
/// How long a step may take to settle.
const SETTLE: Duration = Duration::from_secs(10);

/// What's at a file's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDisk {
    /// The file the torrent was made from.
    Valid,
    Absent,
    /// A file of its size with other bytes: one librqbit created, or one changed by hand.
    Other,
}

#[derive(Debug, Clone)]
enum Step {
    /// `pack`: one of the packs, else the magnet.
    Add { pack: Option<usize>, paused: bool },
    Pause(TorrentId),
    Resume(TorrentId),
    Check(TorrentId),
    Remove(TorrentId),
    Delete(TorrentId),
    SetPriority { id: TorrentId, file: usize, priority: Priority },
    /// By hand, outside Phantom.
    DeleteFile { pack: usize, file: usize },
    /// The file it was, written back by hand.
    PutBack { pack: usize, file: usize },
    /// Other bytes of the same size, written by hand.
    Overwrite { pack: usize, file: usize },
    /// The pack's destination folder is moved away, as a drive is disconnected.
    DestinationGone { pack: usize },
    /// And moved back.
    DestinationBack { pack: usize },
    /// The service stops and starts again.
    Restart,
}

#[derive(Debug, Clone)]
struct Model {
    disk: [[OnDisk; FILES]; PACKS],
    /// Each pack's destination is away.
    away: [bool; PACKS],
    torrents: BTreeMap<TorrentId, Torrent>,
    next_id: TorrentId,
    /// What the last step's command answers.
    answer: Answer,
}

#[derive(Debug, Clone, PartialEq)]
enum Answer {
    None,
    Ok,
    Err,
    Added(Added),
}

#[derive(Debug, Clone)]
enum Torrent {
    Magnet,
    Pack(Pack),
}

#[derive(Debug, Clone)]
struct Pack {
    pack: usize,
    /// Asked to run (Downloading or Seeding), rather than stopped.
    running: bool,
    off: [bool; FILES],
    /// Whether librqbit has the file: what its last check found.
    have: [bool; FILES],
    /// The files found missing; it's Missing files while there are any.
    missing: BTreeMap<usize, Gone>,
    /// Its destination is away: it's Destination unavailable, and `running` says whether it runs
    /// again once it's back.
    away: bool,
}

#[derive(Debug, Clone)]
struct Gone {
    /// librqbit had the file when it went: one put back must be the same to count.
    had: bool,
    /// A file was written at its path since it was found missing.
    back: bool,
}

impl Pack {
    fn complete(&self) -> bool {
        (0..FILES).all(|f| self.off[f] || self.have[f])
    }

    fn status(&self) -> Status {
        if self.away {
            return Status::DestinationUnavailable { reason: away(self.pack) };
        }
        match (self.missing.is_empty(), self.running, self.complete()) {
            (false, ..) => Status::MissingFiles,
            (true, true, true) => Status::Seeding,
            (true, true, false) => Status::Downloading,
            (true, false, true) => Status::Done,
            (true, false, false) => Status::Paused,
        }
    }

    /// Taken out of librqbit and added again: it creates every file that isn't there, Off ones
    /// too, and checks them all.
    fn recheck(&mut self, disk: &mut [OnDisk; FILES], run: bool) {
        librqbit_recreates_absent(disk);
        for f in 0..FILES {
            self.have[f] = disk[f] == OnDisk::Valid;
        }
        self.missing.clear();
        self.running = run;
    }

    /// The engine's look at the files: a file it gets that's gone makes it Missing files; once
    /// every missing file is back it's checked, staying stopped, and those that aren't what
    /// they were are missing again.
    fn look_at_files(&mut self, disk: &mut [OnDisk; FILES]) {
        let gone: Vec<usize> = (0..FILES).filter(|&f| !self.off[f] && disk[f] == OnDisk::Absent).collect();
        if self.missing.is_empty() {
            if !gone.is_empty() {
                self.running = false;
                self.missing = gone.into_iter().map(|f| (f, Gone { had: self.have[f], back: false })).collect();
            }
            return;
        }
        for f in gone {
            self.missing.entry(f).or_insert(Gone { had: self.have[f], back: false });
        }
        if !self.missing.values().all(|g| g.back) {
            return;
        }
        let returned = std::mem::take(&mut self.missing);
        self.recheck(disk, false);
        self.missing = returned.into_iter().filter(|(f, g)| g.had && !self.have[*f]).map(|(f, g)| (f, Gone { back: false, ..g })).collect();
    }
}

impl Model {
    fn pack_torrent(&mut self, pack: usize) -> Option<&mut Pack> {
        self.torrents.values_mut().find_map(|t| match t {
            Torrent::Pack(p) if p.pack == pack => Some(p),
            _ => None,
        })
    }

    fn add(&mut self, pack: Option<usize>, paused: bool) -> Answer {
        // Refused before it's looked up.
        if pack.is_some_and(|k| self.away[k]) {
            return Answer::Err;
        }
        let existing = self.torrents.iter().find(|(_, t)| match (t, pack) {
            (Torrent::Magnet, None) => true,
            (Torrent::Pack(p), Some(pack)) => p.pack == pack,
            _ => false,
        });
        if let Some((&id, _)) = existing {
            return Answer::Added(Added { id, name: name(pack), already_existed: true });
        }
        let id = self.next_id;
        self.next_id += 1;
        let torrent = match pack {
            None => Torrent::Magnet,
            Some(pack) => {
                let mut p = Pack { pack, running: !paused, off: [false; FILES], have: [false; FILES], missing: BTreeMap::new(), away: false };
                p.recheck(&mut self.disk[pack], !paused);
                Torrent::Pack(p)
            }
        };
        self.torrents.insert(id, torrent);
        Answer::Added(Added { id, name: name(pack), already_existed: false })
    }

    /// A command to torrent `id`: what it answers, and what it does.
    fn command(&mut self, id: TorrentId, step: &Step) -> Answer {
        let disk = &mut self.disk;
        let Some(torrent) = self.torrents.get_mut(&id) else { return Answer::Err };
        let p = match torrent {
            Torrent::Magnet if matches!(step, Step::Remove(_) | Step::Delete(_)) => {
                self.torrents.remove(&id);
                return Answer::Ok;
            }
            // No info yet.
            Torrent::Magnet => return Answer::Err,
            Torrent::Pack(p) => p,
        };
        let pack = p.pack;
        let missing = !p.missing.is_empty();
        match step {
            // Its files are out of reach.
            Step::Resume(_) | Step::Check(_) | Step::Delete(_) if p.away => return Answer::Err,
            Step::Pause(_) => p.running = false,
            Step::Resume(_) if missing => p.recheck(&mut disk[pack], true),
            Step::Resume(_) => p.running = true,
            Step::Check(_) => {
                let run = p.running && !missing;
                p.recheck(&mut disk[pack], run);
            }
            Step::SetPriority { file, priority, .. } => {
                p.off[*file] = *priority == Priority::Off;
                if *priority == Priority::Off && p.missing.remove(file).is_some() && p.missing.is_empty() {
                    p.running = false;
                }
            }
            Step::Remove(_) => {
                self.torrents.remove(&id);
            }
            Step::Delete(_) => {
                self.torrents.remove(&id);
                self.disk[pack] = [OnDisk::Absent; FILES];
            }
            _ => unreachable!("not a command: {step:?}"),
        }
        Answer::Ok
    }

    /// The written file counts as put back for a torrent missing it.
    fn written(&mut self, pack: usize, file: usize) {
        if let Some(gone) = self.pack_torrent(pack).and_then(|p| p.missing.get_mut(&file)) {
            gone.back = true;
        }
    }

    /// Away, it's stopped in librqbit, remembering whether it was running; its missing files are
    /// forgotten, as they're out of reach.
    fn destination_gone(&mut self, pack: usize) {
        self.away[pack] = true;
        if let Some(p) = self.pack_torrent(pack) {
            p.away = true;
            p.missing.clear();
        }
    }

    /// Back, it's added again, so checked, and runs if it was running.
    fn destination_back(&mut self, pack: usize) {
        self.away[pack] = false;
        let disk = &mut self.disk[pack];
        if let Some(p) = self.torrents.values_mut().find_map(|t| match t {
            Torrent::Pack(p) if p.pack == pack => Some(p),
            _ => None,
        }) {
            p.away = false;
            let run = p.running;
            p.recheck(disk, run);
        }
    }

    fn restart(&mut self) {
        // Torrents fetching info don't survive it.
        self.torrents.retain(|_, t| matches!(t, Torrent::Pack(_)));
        for t in self.torrents.values_mut() {
            let Torrent::Pack(p) = t else { continue };
            // Restored without touching its files, so librqbit has nothing of them until they're
            // back and checked.
            if p.away {
                p.have = [false; FILES];
                continue;
            }
            // What's at a missing file's path at start isn't taken for the file put back. A file
            // made again doesn't fit its size, so librqbit checks them all again.
            librqbit_recreates_absent(&mut self.disk[p.pack]);
            p.have = self.disk[p.pack].map(|d| d == OnDisk::Valid);
            p.missing.values_mut().for_each(|g| g.back = false);
        }
    }

    /// What the engine must show once it has settled.
    fn view(&self) -> View {
        let torrents = self
            .torrents
            .iter()
            .map(|(&id, t)| match t {
                Torrent::Magnet => Listed { id, status: Status::FetchingInfo, progress_bytes: 0, files: Vec::new() },
                Torrent::Pack(p) => {
                    let counted = |f: usize| !p.off[f] && p.have[f] && !p.missing.contains_key(&f);
                    let progress_bytes = (0..FILES).filter(|&f| counted(f)).count() as u64 * FULL;
                    let files = (0..FILES)
                        .map(|f| (if p.off[f] { Priority::Off } else { Priority::Normal }, p.missing.contains_key(&f)))
                        .collect();
                    Listed { id, status: p.status(), progress_bytes, files }
                }
            })
            .collect();
        View { torrents, disk: self.disk }
    }
}

/// librqbit adding a torrent, or restoring it at start, creates every file of it that isn't on
/// disk, Off ones too, empty. Not a promise but a bug the model follows, so fixing it is a change
/// here (desk: "Check recreates a deleted file that's set Off").
fn librqbit_recreates_absent(disk: &mut [OnDisk; FILES]) {
    for f in disk.iter_mut().filter(|f| **f == OnDisk::Absent) {
        *f = OnDisk::Other;
    }
}

/// Why the pack's destination can't be reached while it's away, with the case's dir as `/`.
fn away(pack: usize) -> Unavailable {
    Unavailable::FolderMissing { path: AbsolutePath::new(destination(Path::new("/"), pack)).unwrap() }
}

fn name(pack: Option<usize>) -> Option<String> {
    pack.map(|k| format!("pack{k}"))
}

/// What's compared after every step.
#[derive(Debug, PartialEq)]
struct View {
    torrents: Vec<Listed>,
    disk: [[OnDisk; FILES]; PACKS],
}

#[derive(Debug, PartialEq)]
struct Listed {
    id: TorrentId,
    status: Status,
    progress_bytes: u64,
    /// Each file's priority and whether it's marked missing, by index.
    files: Vec<(Priority, bool)>,
}

struct EngineModel;

impl ReferenceStateMachine for EngineModel {
    type State = Model;
    type Transition = Step;

    fn init_state() -> BoxedStrategy<Model> {
        let disk = [[OnDisk::Valid; FILES]; PACKS];
        Just(Model { disk, away: [false; PACKS], torrents: BTreeMap::new(), next_id: 0, answer: Answer::None }).boxed()
    }

    fn transitions(model: &Model) -> BoxedStrategy<Step> {
        // The torrents listed, and an id no torrent has.
        let ids: Vec<TorrentId> = model.torrents.keys().copied().chain([model.next_id]).collect();
        let id = proptest::sample::select(ids);
        let pack = 0..PACKS;
        let file = 0..FILES;
        let priority = prop_oneof![Just(Priority::Normal), Just(Priority::Off)];
        prop_oneof![
            3 => (proptest::option::weighted(0.85, pack.clone()), any::<bool>()).prop_map(|(pack, paused)| Step::Add { pack, paused }),
            2 => id.clone().prop_map(Step::Pause),
            2 => id.clone().prop_map(Step::Resume),
            2 => id.clone().prop_map(Step::Check),
            1 => id.clone().prop_map(Step::Remove),
            1 => id.clone().prop_map(Step::Delete),
            3 => (id, file.clone(), priority).prop_map(|(id, file, priority)| Step::SetPriority { id, file, priority }),
            3 => (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::DeleteFile { pack, file }),
            3 => (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::PutBack { pack, file }),
            1 => (pack.clone(), file).prop_map(|(pack, file)| Step::Overwrite { pack, file }),
            1 => pack.clone().prop_map(|pack| Step::DestinationGone { pack }),
            2 => pack.prop_map(|pack| Step::DestinationBack { pack }),
            1 => Just(Step::Restart),
        ]
        .boxed()
    }

    /// A restart only when what librqbit has is what's on disk (but for missing files, which
    /// are checked anyway): restoring a torrent, librqbit trusts what it saved while the files'
    /// sizes fit and checks them again otherwise, which is its business rather than Phantom's.
    /// Files are only changed by hand, and destinations only go and come back, when they can.
    fn preconditions(model: &Model, step: &Step) -> bool {
        match step {
            Step::DeleteFile { pack, .. } | Step::PutBack { pack, .. } | Step::Overwrite { pack, .. } => !model.away[*pack],
            Step::DestinationGone { pack } => !model.away[*pack],
            Step::DestinationBack { pack } => model.away[*pack],
            Step::Restart => model.torrents.values().all(|t| match t {
                Torrent::Magnet => true,
                Torrent::Pack(p) => (0..FILES)
                    .filter(|f| !p.missing.contains_key(f))
                    .all(|f| p.have[f] == (model.disk[p.pack][f] == OnDisk::Valid)),
            }),
            _ => true,
        }
    }

    fn apply(mut model: Model, step: &Step) -> Model {
        model.answer = match step {
            Step::Add { pack, paused } => model.add(*pack, *paused),
            Step::Pause(id) | Step::Resume(id) | Step::Check(id) | Step::Remove(id) | Step::Delete(id) => model.command(*id, step),
            Step::SetPriority { id, .. } => model.command(*id, step),
            Step::DeleteFile { pack, file } => {
                model.disk[*pack][*file] = OnDisk::Absent;
                if let Some(gone) = model.pack_torrent(*pack).and_then(|p| p.missing.get_mut(file)) {
                    gone.back = false;
                }
                Answer::None
            }
            Step::PutBack { pack, file } => {
                model.disk[*pack][*file] = OnDisk::Valid;
                model.written(*pack, *file);
                Answer::None
            }
            Step::Overwrite { pack, file } => {
                model.disk[*pack][*file] = OnDisk::Other;
                model.written(*pack, *file);
                Answer::None
            }
            Step::DestinationGone { pack } => {
                model.destination_gone(*pack);
                Answer::None
            }
            Step::DestinationBack { pack } => {
                model.destination_back(*pack);
                Answer::None
            }
            Step::Restart => {
                model.restart();
                Answer::None
            }
        };
        let Model { disk, torrents, .. } = &mut model;
        for t in torrents.values_mut() {
            if let Torrent::Pack(p) = t
                && !p.away
            {
                p.look_at_files(&mut disk[p.pack]);
            }
        }
        model
    }
}

/// A real engine in its own temp dir, driven one step at a time.
struct Sut {
    rt: tokio::runtime::Runtime,
    dir: TempDir,
    engine: Option<Engine>,
    /// Each pack's .torrent.
    sources: Vec<Vec<u8>>,
    /// Each pack's file names, by index: the order the .torrent lists them in.
    names: Vec<Vec<&'static str>>,
}

impl Sut {
    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    /// Where the file is, its destination away or not.
    fn file(&self, pack: usize, file: usize) -> PathBuf {
        let at = |dir: PathBuf| dir.join(format!("pack{pack}")).join(self.names[pack][file]);
        let away = moved_away(self.dir.path(), pack);
        if away.exists() { at(away) } else { at(destination(self.dir.path(), pack)) }
    }

    fn content(&self, pack: usize, file: usize) -> Vec<u8> {
        content(pack, self.names[pack][file])
    }

    fn start(&mut self) {
        let config = EngineConfig {
            data_dir: self.dir.path().join("data"),
            download_dir: self.dir.path().join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            check_files_every: Duration::from_millis(200),
        };
        self.engine = Some(self.rt.block_on(Engine::start(config)).unwrap());
    }

    /// Runs a client call, which must be answered.
    fn call<T>(&self, call: impl Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
        self.rt.block_on(async { tokio::time::timeout(SETTLE, call).await.expect("every command is answered") })
    }

    fn view(&self) -> View {
        let engine = self.engine();
        let listing = self.rt.block_on(engine.listing());
        let torrents = listing
            .into_iter()
            .map(|t| {
                let files = match self.rt.block_on(engine.details(t.id)) {
                    Ok(details) => details.files.iter().map(|f| (f.priority, f.missing)).collect(),
                    Err(_) => Vec::new(),
                };
                // The reason's path with the case's dir as `/`, as in the model.
                let status = match t.status {
                    Status::DestinationUnavailable { reason: Unavailable::FolderMissing { path } } => {
                        let path = Path::new("/").join(path.strip_prefix(self.dir.path()).unwrap_or(&path));
                        let path = AbsolutePath::new(path).unwrap();
                        Status::DestinationUnavailable { reason: Unavailable::FolderMissing { path } }
                    }
                    status => status,
                };
                Listed { id: t.id, status, progress_bytes: t.progress_bytes, files }
            })
            .collect();
        let disk = std::array::from_fn(|k| std::array::from_fn(|f| on_disk(&self.file(k, f), &self.content(k, f))));
        View { torrents, disk }
    }

    /// Whether the engine has looked at the files since it started: until then, what's at a
    /// missing file's path isn't recorded, and a file put back would be taken for it.
    fn looked_at_files(&self) -> bool {
        let unjudged = self.rt.block_on(self.engine().inspect(|actor| {
            actor.torrents.values().any(|t| t.missing.values().any(|g| g.seen == Seen::NotYet))
        }));
        !unjudged
    }

    /// Waits for the engine to show what the model says, and fails with the difference if it
    /// doesn't in time.
    fn settle(&self, model: &Model) {
        let want = model.view();
        let deadline = Instant::now() + SETTLE;
        loop {
            let seen = self.view();
            if seen == want && self.looked_at_files() {
                return;
            }
            if Instant::now() > deadline {
                assert_eq!(seen, want, "the engine didn't settle where the model says in {SETTLE:?}");
                panic!("the engine never looked at the files after starting");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The pack's destination: it's in its own folder there.
fn destination(dir: &Path, pack: usize) -> PathBuf {
    dir.join(format!("destination{pack}"))
}

/// Where the pack's destination is while it's away.
fn moved_away(dir: &Path, pack: usize) -> PathBuf {
    dir.join(format!("away{pack}"))
}

fn pack_dir(dir: &Path, pack: usize) -> PathBuf {
    destination(dir, pack).join(format!("pack{pack}"))
}

/// The bytes of the pack's file, different for every file.
fn content(pack: usize, name: &str) -> Vec<u8> {
    let file = NAMES.iter().position(|n| *n == name).unwrap();
    (0..FULL as usize).map(|n| (n * 7 + file * 31 + pack * 101) as u8).collect()
}

fn on_disk(path: &Path, valid: &[u8]) -> OnDisk {
    match std::fs::read(path) {
        Ok(bytes) if bytes == valid => OnDisk::Valid,
        Ok(_) => OnDisk::Other,
        Err(_) => OnDisk::Absent,
    }
}

impl StateMachineTest for EngineModel {
    type SystemUnderTest = Sut;
    type Reference = EngineModel;

    fn init_test(_: &Model) -> Sut {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::create_dir_all(dir.path().join("downloads")).unwrap();
        let sources: Vec<Vec<u8>> = (0..PACKS)
            .map(|k| {
                let folder = pack_dir(dir.path(), k);
                std::fs::create_dir_all(&folder).unwrap();
                for name in NAMES {
                    std::fs::write(folder.join(name), content(k, name)).unwrap();
                }
                let options = librqbit::CreateTorrentOptions {
                    name: None,
                    trackers: vec!["http://127.0.0.1:1/announce".into()],
                    piece_length: Some(PIECE as u32),
                };
                let created = rt.block_on(async {
                    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
                    librqbit::create_torrent(&folder, options, &spawner).await.unwrap()
                });
                created.as_bytes().unwrap().to_vec()
            })
            .collect();
        let names = sources
            .iter()
            .map(|bytes| {
                let files = librqbit::torrent_from_bytes(bytes).unwrap().info.data.files.unwrap();
                let name = |path: &[_]| NAMES.into_iter().find(|n| n.as_bytes() == path).unwrap();
                files.iter().map(|f| name(f.path[0].as_ref())).collect()
            })
            .collect();
        let mut sut = Sut { rt, dir, engine: None, sources, names };
        sut.start();
        sut
    }

    fn apply(mut sut: Sut, model: &Model, step: Step) -> Sut {
        let engine = sut.engine();
        let answer = match &step {
            Step::Add { pack, paused } => {
                let (source, destination) = match pack {
                    Some(k) => (AddSource::TorrentFile(sut.sources[*k].clone()), destination(sut.dir.path(), *k)),
                    None => (AddSource::Url(MAGNET.into()), sut.dir.path().join("downloads")),
                };
                // Parsed at the edge, as the service does: one unavailable is refused there.
                match Destination::parse(AbsolutePath::new(destination).unwrap()) {
                    Ok(destination) => match sut.call(engine.add(source, *paused, destination)) {
                        Ok(added) => Answer::Added(added),
                        Err(e) => panic!("adding failed: {e:#}"),
                    },
                    Err(_) => Answer::Err,
                }
            }
            Step::Pause(id) => ok(sut.call(engine.pause(*id))),
            Step::Resume(id) => ok(sut.call(engine.resume(*id))),
            Step::Check(id) => ok(sut.call(engine.check(*id))),
            Step::Remove(id) => ok(sut.call(engine.remove(*id))),
            Step::Delete(id) => ok(sut.call(engine.delete(*id))),
            Step::SetPriority { id, file, priority } => ok(sut.call(engine.set_priority(*id, &[*file], *priority))),
            Step::DeleteFile { pack, file } => {
                let _ = std::fs::remove_file(sut.file(*pack, *file));
                Answer::None
            }
            Step::PutBack { pack, file } => {
                write(sut.dir.path(), &sut.file(*pack, *file), &sut.content(*pack, *file));
                Answer::None
            }
            Step::Overwrite { pack, file } => {
                write(sut.dir.path(), &sut.file(*pack, *file), &vec![0xee; FULL as usize]);
                Answer::None
            }
            Step::DestinationGone { pack } => {
                std::fs::rename(destination(sut.dir.path(), *pack), moved_away(sut.dir.path(), *pack)).unwrap();
                Answer::None
            }
            Step::DestinationBack { pack } => {
                std::fs::rename(moved_away(sut.dir.path(), *pack), destination(sut.dir.path(), *pack)).unwrap();
                Answer::None
            }
            Step::Restart => {
                sut.rt.block_on(engine.stop());
                sut.start();
                Answer::None
            }
        };
        assert_eq!(answer, model.answer, "the answer to {step:?}");
        sut.settle(model);
        sut
    }

    fn teardown(mut sut: Sut, _: Model) {
        if let Some(engine) = sut.engine.take() {
            sut.rt.block_on(engine.stop());
        }
    }
}

fn ok(result: anyhow::Result<()>) -> Answer {
    if result.is_ok() { Answer::Ok } else { Answer::Err }
}

/// Writes a file by hand, as a person putting one back would: a new file moved into place in one
/// go, so the engine never sees the path empty in between.
fn write(dir: &Path, path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let new = dir.join("new.bin");
    std::fs::write(&new, bytes).unwrap();
    std::fs::rename(&new, path).unwrap();
}

/// 16 cases, or `PROPTEST_CASES`; shrinking is bounded, since every step runs a real engine.
fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(16);
    Config { cases, max_shrink_iters: 256, ..Config::default() }
}

prop_state_machine! {
    #![proptest_config(config())]

    #[test]
    fn the_engine_follows_the_model(sequential 1..20 => EngineModel);
}
