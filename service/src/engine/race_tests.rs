//! Races between the actor and its add tasks, the one background task that touches librqbit (the
//! actor awaits every other librqbit call itself, and the watcher only sends it messages). Each
//! add waits at two gates the test opens: before librqbit has the torrent, and after it has it but
//! before the actor hears so. Generated sequences of adds, gate openings, commands, destinations
//! going and coming back (also just as librqbit creates a torrent's files), and restarts run on a
//! real librqbit session in a temp dir, offline. One pack's destination is on a drive: the internal
//! disk by its link under `/Volumes`. A local seeder has the first of each pack's two files and
//! not the second, so a running torrent downloads exactly half and stalls there: no rate limit,
//! no timing.
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
//! Checks wait at a gate too, before they read: a torrent checked meanwhile is Checking until the gate
//! opens, and Cancel check (#88) then ends it, whatever the add's gates or the restart do: the
//! torrent is Unchecked, librqbit never runs it, and across a restart it's still Unchecked.
//!
//! Undo (#150) races them too: an undo puts back what Delete moved on a blocking task, which a step
//! holds at the trash's gate after each item of its own torrent's only, while adds (of the same
//! torrent too, which wait for the undo), removes and Deletes of the other (never delayed by it,
//! #190), destinations and restarts go on. An undo answered comes back under the id it
//! had; one refused leaves the list as it was; an add of the torrent it brings back is answered with
//! that id once it's back, or as a new torrent if it didn't come back. A crash cuts an undo short
//! (and something of the user's may be put where the rest goes back meanwhile), or comes as a
//! torrent is removed or deleted, between keeping what undoes it and its record going (#162).
//!
//! And across a restart, once the torrents it restored are checked: each has at least the progress
//! it had before the stop, but where its destination can't be reached and one stopped while being
//! checked (its progress is how much is checked). The seeder is out of reach meanwhile, so what's
//! lost can't be downloaded again before it's seen.
//!
//! Moves (#24) race them too: a pack's torrent moves between its destination and `elsewhere<pack>`,
//! renamed or copied, its copy held at a gate of its own after each chunk while the other steps go
//! on. Commands for it meanwhile wait their turn, but Check and another Move, refused at once; a
//! Remove or a Delete cancels it first; a stop puts back what it moved, and so does a start after a
//! crash cut it short. However it ends, nothing it staged is left, and its files are in exactly one
//! of the two destinations: the new one if it moved, else the old, as they were.
//!
//! 16 cases per `cargo test`; `PROPTEST_CASES=256 cargo test -p service race` runs more.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use librqbit::api::TorrentIdOrHash;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{AbsolutePath, AddSource, Status, TorrentCommand, TorrentId};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch};

use super::actor::Actor;
use super::actor::{AddInto, add_into};
use super::files::{self, Guard, InitIn};
use super::machine::Phase;
use super::{Config as EngineConfig, Destination, Engine};

const PACKS: usize = 2;
/// How long a call may take to be answered, and the engine to settle.
const SETTLE: Duration = Duration::from_secs(10);
const PIECE: u32 = 64 * 1024;
/// Each of a pack's two files: 16 pieces. Below librqbit's 16 MiB save interval, so nothing is
/// saved of a torrent but at completion and when it's dropped.
const FILE: usize = 1 << 20;
/// What the seeder has of each pack: its first file.
const SEEDED: u64 = FILE as u64;

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
    /// Opens the pack's add's gates, and waits for its torrent to download what the seeder has,
    /// if it's downloading.
    Arrive {
        pack: usize,
    },
    Check {
        pack: usize,
    },
    CancelCheck {
        pack: usize,
    },
    /// Checks wait at their gate from now on, until it opens.
    HoldChecks,
    ReleaseChecks,
    Restart,
    /// Undo the last Remove or Delete: its put-back held at the gate after each item, or not.
    Undo {
        held: bool,
    },
    /// Opens the gate: the undo under way ends.
    UndoEnds,
    /// The engine is killed while an undo's put-back waits at the gate, as a crash, and starts
    /// again: it finishes the undo (the torrent back under its id, its files as they were), or
    /// refuses it with everything still in the trash. Never half of it. Meanwhile (#162), `empty`:
    /// the first item still in the trash to go back is emptied from it, and stays gone; `block`: a
    /// file of the user's is put where the one at `at` of those still in the trash goes back, so
    /// it's refused, and what went back before the crash returns to the trash (an emptied item's
    /// path taken blocks it too); `lose_stash`: its stash's .torrent goes, so adding it again
    /// fails, and it's refused so too. Only with every add told.
    KillMidUndo {
        block: bool,
        empty: bool,
        lose_stash: bool,
        at: prop::sample::Index,
    },
    /// The .torrent of the stash of the undo held at the gate goes: adding the torrent again fails
    /// once its files are back, and they go to the trash again, none stranded.
    LoseStash,
    /// A file of the user's, `mine.txt`, is written into the pack's folder: a Delete then moves
    /// the torrent's files one by one, so an undo has several to put back.
    WriteMine {
        pack: usize,
    },
    /// The engine is killed as the pack's torrent is removed or deleted, after what undoes it is
    /// kept and before its record goes (#162), and starts again: the torrent is off the list, and
    /// what undoes it is on top of the stack, once. Only with every add told and no undo under
    /// way; else it's a Remove. So never a Delete of a torrent without info, which goes through the
    /// same rule but can't be cut short so: a start lists no torrent without info. `both`: the
    /// other pack's torrent too, in one press (#235), its part done before the crash comes as the
    /// last part's record goes: the press is one entry holding both, once.
    CrashAsRemoved {
        pack: usize,
        delete: bool,
        both: bool,
    },
    /// The pack's torrent moves to its other destination, renamed or (`copies`) copied: held at the
    /// move's gate after each chunk, or not. One at a time.
    Move {
        pack: usize,
        held: bool,
        copies: bool,
    },
    /// Opens the move's gate: the move under way ends.
    MoveEnds,
    /// The engine is killed while a move waits at its gate, as a crash, and starts again: the move
    /// is put back, nothing of it left in the new destination.
    KillMidMove,
    /// The pack's torrent moves (renamed or copied), and the engine is killed once the record names
    /// the new destination and its first file has taken its place there, as a crash: the start
    /// finishes the move.
    KillAfterPlaced {
        pack: usize,
        copies: bool,
    },
}

/// What the user's files hold.
const MINE: &[u8] = b"mine";

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
    /// The torrent it adds.
    id: TorrentId,
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
    /// Where the pack's destination that went was: its own, or the one its torrent moved to.
    gone_from: [Option<std::path::PathBuf>; PACKS],
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
        if goes.is_some() {
            disk.gone_from[pack] = Some(destination(&dir, pack));
        }
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
    Arc::new(move |session, id, add, destination, download_dir, guard| {
        let (before, wait_before) = oneshot::channel();
        let (after, wait_after) = oneshot::channel();
        let (done, in_librqbit) = oneshot::channel();
        adds.lock().unwrap().push(Gates { id, before: Some(before), in_librqbit: Some(in_librqbit), after: Some(after) });
        Box::pin(async move {
            shut_unless_opened(wait_before).await;
            let added = add_into(session, id, add, destination, download_dir, guard).await;
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
    seeder: Seeder,
    /// What's in the list: by pack, its id, where its add is and which of `adds` it is.
    model: BTreeMap<usize, (TorrentId, Stage, usize)>,
    next_id: TorrentId,
    disk: Disk,
    /// Where checks wait before their next read.
    checks: Arc<files::Gate>,
    /// The ids each pack's torrents had when they left the list, and whether they had their info.
    left: BTreeMap<usize, Vec<(TorrentId, bool)>>,
    /// Each pack's info hash.
    hashes: Vec<String>,
    /// The trash's gate, which holds an undo's put-back.
    gate: super::trash::Gate,
    /// The bytes of each file of the pack's folder, by path, as its Delete found them, by torrent.
    deleted: BTreeMap<TorrentId, BTreeMap<std::path::PathBuf, Vec<u8>>>,
    /// The undo under way, and the adds waiting for it, by pack.
    undoing: Option<tokio::task::JoinHandle<Result<proto::Undone, super::Failure>>>,
    waiting: Vec<(usize, tokio::task::JoinHandle<Result<proto::Added, super::Failure>>)>,
    /// The undo held at the gate lost its stash's .torrent: its pack, its items, and whether its
    /// destination went since.
    stash_lost: Option<(usize, Vec<super::trash::Trashed>, bool)>,
    /// The running engine's trash: once a killed engine's is gone, so is its put-back's thread.
    trash: std::sync::Weak<dyn super::trash::Bin>,
    /// The torrents (by id) whose destination is their pack's other one, `elsewhere<pack>`.
    elsewhere: BTreeSet<TorrentId>,
    /// The move's gate, which holds its copy after each chunk; the move under way.
    move_gate: super::trash::Gate,
    moving: Option<Moving>,
}

/// A move under way: the pack's torrent `id`, going `elsewhere` or back, its answer, what its old
/// folder and the new held of the torrent's before, and the commands that wait behind it.
struct Moving {
    pack: usize,
    id: TorrentId,
    elsewhere: bool,
    answer: tokio::task::JoinHandle<Result<(), super::Failure>>,
    old: Files,
    new: Files,
    behind: Vec<tokio::task::JoinHandle<Result<(), super::Failure>>>,
}

/// A librqbit session with the first file of each pack, which its own tracker names to whoever
/// asks, behind a gate: a connection made while it's shut waits there until it opens.
struct Seeder {
    /// Kept alive for the case: dropped with the runtime at its end, not stopped.
    _session: Arc<librqbit::Session>,
    open: watch::Sender<bool>,
}

impl Seeder {
    /// Seeds the packs from `dir`, made there with this content and their trackers: their
    /// .torrents.
    async fn start(dir: &Path) -> (Self, Vec<Vec<u8>>) {
        let tracker = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
        let gate = TcpListener::bind("127.0.0.1:0").await.unwrap();
        // librqbit picks the port itself: one picked here and let go can be held a moment longer by a
        // child another test spawns meanwhile, which got the socket too (#166).
        let session = librqbit::Session::new_with_opts(
            dir.join("seeder"),
            librqbit::SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                listen: Some(librqbit::ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listen = session.listen_addr().expect("the seeder listens");
        let mut sources = Vec::new();
        for k in 0..PACKS {
            let folder = dir.join(format!("seed{k}/pack{k}"));
            std::fs::create_dir_all(folder.join("sub")).unwrap();
            std::fs::write(folder.join("a.bin"), content(k, 0)).unwrap();
            std::fs::write(folder.join("sub/b.bin"), content(k, 1)).unwrap();
            let options = librqbit::CreateTorrentOptions { name: None, trackers: vec![announce.clone()], piece_length: Some(PIECE) };
            let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
            let source = librqbit::create_torrent(&folder, options, &spawner).await.unwrap().as_bytes().unwrap().to_vec();
            // The seeder doesn't have the second file.
            std::fs::write(folder.join("sub/b.bin"), vec![0xee; FILE]).unwrap();
            let opts =
                librqbit::AddTorrentOptions { overwrite: true, output_folder: Some(folder.to_str().unwrap().into()), ..Default::default() };
            let added = session.add_torrent(librqbit::AddTorrent::from_bytes(source.clone()), Some(opts)).await.unwrap();
            let handle = added.into_handle().unwrap();
            let deadline = Instant::now() + SETTLE;
            while !(matches!(handle.stats().state, librqbit::TorrentStatsState::Live) && handle.stats().progress_bytes == SEEDED) {
                assert!(Instant::now() < deadline, "the seeder never had its half: {:?}", handle.stats());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            sources.push(source);
        }
        let (open, reachable) = watch::channel(true);
        tokio::spawn(name_the_gate(tracker, gate.local_addr().unwrap().port()));
        tokio::spawn(pass_while_open(gate, listen, reachable));
        (Self { _session: session, open }, sources)
    }
}

/// A tracker that names one peer, on 127.0.0.1, to every announce.
async fn name_the_gate(tracker: TcpListener, port: u16) {
    let mut body = b"d8:intervali60e5:peers6:\x7f\x00\x00\x01".to_vec();
    body.extend(port.to_be_bytes());
    body.push(b'e');
    loop {
        let Ok((mut stream, _)) = tracker.accept().await else { continue };
        let body = body.clone();
        tokio::spawn(async move {
            let _ = stream.read(&mut [0; 2048]).await;
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&body).await;
        });
    }
}

/// Passes each connection on to the seeder once the gate is open. One held stays connected (a
/// refused one would wait librqbit's 10 s backoff before it tries again), for less than librqbit's
/// 10 s handshake timeout.
async fn pass_while_open(gate: TcpListener, seeder: SocketAddr, open: watch::Receiver<bool>) {
    loop {
        let Ok((mut inbound, _)) = gate.accept().await else { continue };
        let mut open = open.clone();
        tokio::spawn(async move {
            if open.wait_for(|open| *open).await.is_err() {
                return;
            }
            let Ok(mut outbound) = TcpStream::connect(seeder).await else { return };
            let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
        });
    }
}

/// The bytes of the pack's files on disk, in its `folder`, in whole valid pieces: what a check
/// would find.
fn valid_on_disk(folder: &Path, pack: usize) -> u64 {
    let root = folder;
    let valid = |file: usize, path: &str| {
        let bytes = std::fs::read(root.join(path)).unwrap_or_default();
        let wanted = content(pack, file);
        bytes.chunks(PIECE as usize).zip(wanted.chunks(PIECE as usize)).filter(|(had, want)| had == want).count() as u64 * u64::from(PIECE)
    };
    valid(0, "a.bin") + valid(1, "sub/b.bin")
}

/// The bytes of the pack's file, different for every file.
fn content(pack: usize, file: usize) -> Vec<u8> {
    (0..FILE).map(|n| (n * 7 + file * 31 + pack * 101) as u8).collect()
}

impl Sut {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        for k in 0..PACKS {
            std::fs::create_dir_all(destination(dir.path(), k)).unwrap();
            std::fs::create_dir_all(elsewhere(dir.path(), k)).unwrap();
        }
        let (seeder, sources) = rt.block_on(Seeder::start(dir.path()));
        let hashes = sources.iter().map(|s| librqbit::torrent_from_bytes(s).unwrap().info_hash.as_string()).collect();
        let mut sut = Self {
            rt,
            dir,
            engine: None,
            adds: Adds::default(),
            sources,
            seeder,
            model: BTreeMap::new(),
            next_id: 0,
            disk: Disk::default(),
            checks: Arc::default(),
            left: BTreeMap::new(),
            deleted: BTreeMap::new(),
            hashes,
            gate: Default::default(),
            undoing: None,
            waiting: Vec::new(),
            stash_lost: None,
            trash: std::sync::Weak::<super::trash::Folder>::new(),
            elsewhere: BTreeSet::new(),
            move_gate: Default::default(),
            moving: None,
        };
        sut.start();
        sut
    }

    /// What each torrent has, by id: but where its destination can't be reached, and of one being
    /// checked, whose progress is how much is checked.
    fn progress(&self, disk: &Destinations) -> BTreeMap<TorrentId, u64> {
        let reachable: BTreeSet<TorrentId> =
            self.model.iter().filter(|(pack, _)| !disk.unavailable[**pack]).map(|(_, (id, ..))| *id).collect();
        let listed = self.rt.block_on(self.engine().listing());
        listed
            .into_iter()
            // Nothing is known of one whose check was cancelled; one moving is put back by the stop.
            .filter(|t| reachable.contains(&t.id) && !matches!(t.status, Status::Checking | Status::Unchecked | Status::Moving { .. }))
            .map(|t| (t.id, t.content.map_or(0, |c| c.progress_bytes)))
            .collect()
    }

    /// Waits until no torrent is checked, then: each torrent of `before` still listed, whose
    /// destination can be reached, has at least the progress it had.
    fn kept(&self, before: &BTreeMap<TorrentId, u64>) {
        let deadline = Instant::now() + SETTLE;
        while self.rt.block_on(self.engine().listing()).iter().any(|t| t.status == Status::Checking) {
            assert!(Instant::now() < deadline, "the torrents restored were never checked");
            std::thread::sleep(Duration::from_millis(10));
        }
        let after = self.progress(&self.disk.lock().unwrap());
        let lost: Vec<_> = before
            .iter()
            .filter_map(|(id, &had)| Some((*id, had, *after.get(id)?)))
            .filter(|&(_, had, has)| has < had)
            .map(|(id, had, has)| format!("{id}: {has} bytes, {had} before"))
            .collect();
        assert_eq!(lost, Vec::<String>::new(), "progress lost across a restart");
    }

    /// Opens the pack's add's gates, then waits until its torrent has what the seeder has, unless
    /// it isn't downloading.
    fn arrive(&mut self, pack: usize) {
        for _ in 0..3 {
            self.open(pack);
        }
        let Some(&(id, Stage::Told, _)) = self.model.get(&pack) else { return };
        let deadline = Instant::now() + SETTLE;
        loop {
            let listed = self.rt.block_on(self.engine().listing());
            let Some(t) = listed.iter().find(|t| t.id == id) else { return };
            let progress = t.content.as_ref().map_or(0, |c| c.progress_bytes);
            match t.status {
                // Held at the gate: nothing arrives until it opens (a piece is read to be verified, as a
                // check reads them).
                Status::Checking | Status::Downloading if self.checks.is_held() => return,
                Status::Checking => {}
                Status::Downloading if progress < SEEDED => {}
                _ => return,
            }
            assert!(Instant::now() < deadline, "the seeder's pieces never arrived: {t:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn start(&mut self) {
        self.adds = Adds::default();
        let config = EngineConfig {
            data_dir: self.dir.path().join("data"),
            download_dir: self.dir.path().join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            // Often, so a step settles as soon as the engine has looked: the service's are 5 s and
            // `POLL_EVERY`.
            size_check_every: Duration::from_millis(50),
            poll_every: Duration::from_millis(50),
            trash: {
                let trash = super::trash::into(self.dir.path().join("trash"), Default::default(), self.gate.clone());
                self.trash = Arc::downgrade(&trash);
                trash
            },
            speed_limits: Default::default(),
            seeding: Default::default(),
        };
        let guard = Guard::new(going(self.disk.clone(), self.dir.path().to_owned())).with_gate(super::files::At::Read, self.checks.clone());
        self.engine = Some(self.rt.block_on(Engine::start_with(config, gated(self.adds.clone()), guard, super::Looks::real())).unwrap());
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    /// Runs a client call, which must be answered.
    fn call<T, E>(&self, call: impl Future<Output = Result<T, E>>) -> Result<T, E> {
        self.rt.block_on(async { tokio::time::timeout(SETTLE, call).await.expect("every command is answered") })
    }

    fn command(&mut self, pack: usize, command: TorrentCommand) {
        let Some(&(id, ..)) = self.model.get(&pack) else { return };
        // Behind a move held at its gate, it waits its turn there: answered once the move ends.
        match &mut self.moving {
            Some(m) if m.pack == pack => {
                let waiting = self.rt.spawn(self.engine.as_ref().expect("running").command_later(id, command));
                m.behind.push(waiting);
            }
            Some(_) | None => drop(self.call(self.engine().command(id, command))),
        }
    }

    /// The pack's destination now: its own, or the other one its torrent moved to.
    fn here(&self, pack: usize) -> std::path::PathBuf {
        match self.model.get(&pack) {
            Some((id, ..)) => self.spot(pack, *id),
            None => destination(self.dir.path(), pack),
        }
    }

    /// Torrent `id`'s destination, of the pack's two.
    fn spot(&self, pack: usize, id: TorrentId) -> std::path::PathBuf {
        if self.elsewhere.contains(&id) { elsewhere(self.dir.path(), pack) } else { destination(self.dir.path(), pack) }
    }

    /// The pack's own folder, where its destination is now, or where it went.
    fn folder_of(&self, pack: usize, disk: &Destinations) -> std::path::PathBuf {
        let root = if disk.unavailable[pack] { moved_away(self.dir.path(), pack) } else { self.here(pack) };
        root.join(format!("pack{pack}"))
    }

    /// [`Step::Move`].
    fn start_move(&mut self, pack: usize, held: bool, copies: bool) {
        let Some(&(id, Stage::Told, _)) = self.model.get(&pack) else { return };
        if self.moving.is_some() || self.checks.is_held() {
            return;
        }
        let goes_elsewhere = !self.elsewhere.contains(&id);
        let to = if goes_elsewhere { elsewhere(self.dir.path(), pack) } else { destination(self.dir.path(), pack) };
        let old = bytes_under(&self.here(pack).join(format!("pack{pack}")));
        let new = bytes_under(&to.join(format!("pack{pack}")));
        let gate = self.move_gate.clone();
        self.rt.block_on(
            self.engine().inspect(move |a| a.move_hold = super::actor::moving::Hold { gate: Some(gate), copies, ..Default::default() }),
        );
        match held {
            true => super::trash::hold_under(&self.move_gate, &to),
            false => super::trash::hold(&self.move_gate, false),
        }
        // Whatever's left of an earlier torrent of the pack's there is replaced.
        let command = TorrentCommand::Move { to: AbsolutePath::new(&to).unwrap(), clash: Some(proto::Clash::Replace) };
        let answer = self.rt.spawn(self.engine().command_later(id, command));
        let deadline = Instant::now() + SETTLE;
        while !answer.is_finished() && !super::trash::waiting(&self.move_gate) {
            assert!(Instant::now() < deadline, "the move neither ended nor waited at its gate");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Refused (its destination out of reach, say), or nothing to move: as any move that ended,
        // never one held at its gate.
        let ended = answer.is_finished();
        self.moving = Some(Moving { pack, id, elsewhere: goes_elsewhere, answer, old, new, behind: Vec::new() });
        if !held || ended {
            self.move_ends();
        }
    }

    /// Opens the move's gate and waits for the move under way, if any, and what waited behind it.
    fn move_ends(&mut self) {
        super::trash::hold(&self.move_gate, false);
        let Some(m) = self.moving.take() else { return };
        let answer = self.rt.block_on(async { tokio::time::timeout(SETTLE, m.answer).await }).expect("every move is answered").unwrap();
        self.moved(m.pack, m.id, m.elsewhere, &answer, &m.old, &m.new);
        for waiting in m.behind {
            let _ = self.rt.block_on(async { tokio::time::timeout(SETTLE, waiting).await }).expect("what waited behind a move is answered");
        }
    }

    /// A move of the pack's torrent `id` ended as `answer` says: moved, its files are in the new
    /// destination and none in the old; else none of them is in the new one but what was there,
    /// and the old has each it had. Nothing is staged in either.
    fn moved(&mut self, pack: usize, id: TorrentId, elsewhere_now: bool, answer: &Result<(), super::Failure>, old: &Files, new: &Files) {
        let name = format!("pack{pack}");
        let (home, other) = (destination(self.dir.path(), pack), elsewhere(self.dir.path(), pack));
        let (old_root, new_root) = if elsewhere_now { (home, other) } else { (other, home) };
        // It ends once the torrent is past moving.
        let deadline = Instant::now() + SETTLE;
        let moving = move |a: &mut Actor| a.torrents.get(&id).is_some_and(|t| matches!(t.lifecycle.phase, Phase::Moving { .. }));
        while self.rt.block_on(self.engine().inspect(moving)) {
            assert!(Instant::now() < deadline, "the move never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        // And once librqbit has sampled it where it is now: checks held from here on don't hold that.
        let resuming = move |a: &mut Actor| a.torrents.get(&id).is_some_and(|t| matches!(t.lib, super::machine::lib::Lib::Resuming(_)));
        while self.rt.block_on(self.engine().inspect(resuming)) {
            assert!(Instant::now() < deadline, "librqbit never sampled the moved torrent");
            std::thread::sleep(Duration::from_millis(5));
        }
        let names = |files: &Files, root: &Path| -> BTreeSet<std::path::PathBuf> {
            files.keys().map(|p| p.strip_prefix(root).unwrap().to_owned()).collect()
        };
        let unavailable = self.disk.lock().unwrap().unavailable[pack];
        let moved = matches!(
            answer,
            Ok(()) | Err(super::Failure::Refused(proto::Refusal::Torrent { why: proto::TorrentRefusal::MovedWithout { .. }, .. }))
        );
        let (old_folder, new_folder) = (old_root.join(&name), new_root.join(&name));
        if moved {
            self.elsewhere.retain(|e| *e != id);
            if elsewhere_now {
                self.elsewhere.insert(id);
            }
            assert!(bytes_under(&old_folder).is_empty(), "moved, and something of it is left in the old destination");
            if !unavailable {
                assert!(names(old, &old_folder).is_subset(&names(&bytes_under(&new_folder), &new_folder)), "moved, without all it had");
            }
        } else if !unavailable {
            assert_eq!(bytes_under(&new_folder), *new, "not moved ({answer:?}), and the new destination changed");
            assert!(
                names(old, &old_folder).is_subset(&names(&bytes_under(&old_folder), &old_folder)),
                "not moved ({answer:?}), and something of it went"
            );
        }
        for root in [&old_root, &new_root] {
            assert!(!root.join(format!(".phantom-moving-{}", self.hashes[pack])).exists(), "a move left its staging in {}", root.display());
        }
    }

    /// [`Step::KillAfterPlaced`].
    fn kill_after_placed(&mut self, pack: usize, copies: bool) {
        let Some(&(id, Stage::Told, _)) = self.model.get(&pack) else { return };
        let told = self.model.values().all(|(_, stage, _)| *stage == Stage::Told);
        if self.moving.is_some() || self.checks.is_held() || !told || self.undoing.is_some() || !self.waiting.is_empty() {
            return;
        }
        let goes_elsewhere = !self.elsewhere.contains(&id);
        let to = if goes_elsewhere { elsewhere(self.dir.path(), pack) } else { destination(self.dir.path(), pack) };
        let old = bytes_under(&self.here(pack).join(format!("pack{pack}")));
        let new = bytes_under(&to.join(format!("pack{pack}")));
        let finish_gate = super::trash::Gate::default();
        super::trash::hold_under(&finish_gate, &to);
        let gate = finish_gate.clone();
        let hold = move |a: &mut Actor| a.move_hold = super::actor::moving::Hold { finish_gate: Some(gate), copies, ..Default::default() };
        self.rt.block_on(self.engine().inspect(hold));
        let command = TorrentCommand::Move { to: AbsolutePath::new(&to).unwrap(), clash: Some(proto::Clash::Replace) };
        let answer = self.rt.spawn(self.engine().command_later(id, command));
        let deadline = Instant::now() + SETTLE;
        while !answer.is_finished() && !super::trash::waiting(&finish_gate) {
            assert!(Instant::now() < deadline, "the move neither ended nor placed a file");
            std::thread::sleep(Duration::from_millis(5));
        }
        if answer.is_finished() {
            // Refused, or nothing to place: as any move that ended.
            let answer = self.rt.block_on(answer).unwrap();
            return self.moved(pack, id, goes_elsewhere, &answer, &old, &new);
        }
        self.seeder.open.send_replace(false);
        self.rt.block_on(self.engine().kill());
        self.engine = None;
        super::trash::abandon(&finish_gate);
        let _ = self.rt.block_on(answer);
        self.start();
        // The start finishes it: the record names the new destination.
        self.moved(pack, id, goes_elsewhere, &Ok(()), &old, &new);
        self.seeder.open.send_replace(true);
    }

    /// [`Step::KillMidMove`].
    fn kill_mid_move(&mut self) {
        let Some(m) = self.moving.take() else { return };
        if !super::trash::waiting(&self.move_gate) || !self.model.values().all(|(_, stage, _)| *stage == Stage::Told) {
            self.moving = Some(m);
            return self.move_ends();
        }
        self.seeder.open.send_replace(false);
        self.rt.block_on(self.engine().kill());
        self.engine = None;
        super::trash::abandon(&self.move_gate);
        let _ = self.rt.block_on(m.answer);
        for waiting in m.behind {
            let _ = self.rt.block_on(waiting);
        }
        // Its copy, past the gate, stops there: it moves nothing more.
        let deadline = Instant::now() + SETTLE;
        while super::trash::waiting(&self.move_gate) {
            assert!(Instant::now() < deadline, "the killed engine's move never stopped");
            std::thread::sleep(Duration::from_millis(5));
        }
        self.move_gate = Default::default();
        self.start();
        // The start puts it back.
        self.moved(m.pack, m.id, m.elsewhere, &Err(proto::Refusal::Stopping.into()), &m.old, &m.new);
        self.seeder.open.send_replace(true);
    }

    fn apply(&mut self, step: &Step) {
        // Held through a step that depends on where the destinations are, so none goes meanwhile.
        let disk = self.disk.clone();
        let mut disk = disk.lock().unwrap();
        match *step {
            Step::Add { pack, paused } => {
                let source = AddSource::TorrentFile(self.sources[pack].clone());
                // Parsed at the edge, as the service does: one that can't be reached is refused there.
                let into = destination(self.dir.path(), pack);
                let parsed = Destination::parse(AbsolutePath::new(&into).unwrap());
                // The add goes into the pack's original folder: it can't be reached only if that is the one gone,
                // not when a moved torrent's new folder is.
                if disk.unavailable[pack] && disk.gone_from[pack].as_deref() == Some(into.as_path()) {
                    assert!(parsed.is_err(), "an add into a destination that can't be reached is refused");
                    return;
                }
                let adding = self.engine().add_later(super::Fetched::local(source), paused, parsed.unwrap());
                // One an undo is bringing back waits for it.
                let hash = self.hashes[pack].clone();
                let coming_back = self.rt.block_on(self.engine().inspect(move |actor| actor.hash_busy(&hash)));
                if coming_back {
                    let engine_add = self.rt.spawn(adding);
                    self.waiting.push((pack, engine_add));
                    return;
                }
                let added = self.call(adding).unwrap();
                self.added(pack, added);
            }
            Step::Open { pack } => {
                drop(disk);
                self.open(pack)
            }
            Step::Pause { pack } => self.command(pack, TorrentCommand::Pause),
            Step::Check { pack } => {
                drop(disk);
                self.command(pack, TorrentCommand::Check)
            }
            Step::CancelCheck { pack } => self.command(pack, TorrentCommand::CancelCheck),
            // Not while a move is under way, as no move starts while checks are held: once it ends,
            // librqbit samples the torrent's pieces through the gate, and what's asked of it waits
            // behind that sampling (its lane), on a gate only the test opens.
            Step::HoldChecks if self.moving.is_some() => {}
            Step::HoldChecks => self.checks.hold(true),
            Step::ReleaseChecks => self.checks.hold(false),
            Step::Resume { pack } => {
                drop(disk);
                self.command(pack, TorrentCommand::Resume)
            }
            Step::Remove { pack, delete } if self.moving.as_ref().is_some_and(|m| m.pack == pack) => {
                drop(disk);
                self.remove_moving(pack, delete);
            }
            Step::Remove { pack, delete } => {
                self.deleting(pack, delete);
                self.command(pack, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove });
                self.removed(pack, delete, &disk);
            }
            Step::CrashAsRemoved { .. } if self.moving.is_some() => {
                drop(disk);
                self.move_ends();
                self.apply(step);
            }
            Step::CrashAsRemoved { pack, delete, both } => {
                let told = self.model.values().all(|(_, stage, _)| *stage == Stage::Told);
                let Some(&(id, ..)) = self.model.get(&pack).filter(|_| told && self.undoing.is_none() && self.waiting.is_empty()) else {
                    drop(disk);
                    return self.apply(&Step::Remove { pack, delete });
                };
                // Of both, in one press: the crash comes at the last part, the other's done before it.
                let other = 1 - pack;
                let pair = both
                    .then(|| self.model.get(&other).map(|&(o, ..)| o))
                    .flatten()
                    .filter(|_| !(delete && (disk.unavailable[pack] || disk.unavailable[other])));
                let packs: Vec<(usize, TorrentId)> = match pair {
                    Some(o) => {
                        let mut packs = vec![(pack, id), (other, o)];
                        packs.sort_by_key(|(_, id)| *id);
                        packs
                    }
                    None => vec![(pack, id)],
                };
                let ids: Vec<TorrentId> = packs.iter().map(|(_, id)| *id).collect();
                let last = *ids.last().expect("one at least");
                for (pack, _) in &packs {
                    self.deleting(*pack, delete);
                }
                let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let armed = crashed.clone();
                self.rt.block_on(self.engine().inspect(move |actor| actor.crash_before_forgetting = Some((last, armed))));
                let _ =
                    self.call(self.engine().commands(ids.clone(), if delete { TorrentCommand::Delete } else { TorrentCommand::Remove }));
                if !crashed.load(std::sync::atomic::Ordering::SeqCst) {
                    // Refused: nothing was taken off the list.
                    self.rt.block_on(self.engine().inspect(|actor| actor.crash_before_forgetting = None));
                    for (pack, _) in &packs {
                        self.removed(*pack, delete, &disk);
                    }
                    return;
                }
                drop(disk);
                self.seeder.open.send_replace(false);
                self.rt.block_on(self.engine().kill());
                self.engine = None;
                self.start();
                // The start finishes what the crash cut short before it handles anything.
                let (top, entries) = self.rt.block_on(self.engine().inspect(move |actor| {
                    let ids_of = |p: &super::undo::machine::Pressed| {
                        super::undo::undoable(actor.steps.machine(), p.seq).map(|u| u.torrents.iter().map(|t| t.id).collect::<Vec<_>>())
                    };
                    (
                        actor.steps.machine().top().and_then(ids_of),
                        actor.steps.machine().stack.iter().filter(|p| ids_of(p).is_some_and(|ids| ids.contains(&last))).count(),
                    )
                }));
                assert_eq!(
                    (top, entries),
                    (Some(ids.clone()), 1),
                    "what undoes the crashed press of {ids:?} is on top of the stack, once, as one entry"
                );
                for (pack, id) in packs {
                    self.model.remove(&pack);
                    self.left.entry(pack).or_default().push((id, true));
                }
                self.seeder.open.send_replace(true);
            }
            Step::WriteMine { pack } if !disk.unavailable[pack] => {
                let folder = self.here(pack).join(format!("pack{pack}"));
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(folder.join("mine.txt"), MINE).unwrap();
            }
            Step::WriteMine { .. } => {}
            // One asked while another is under way waits its turn: the model has no queue, so the
            // steps don't ask (`a_second_undo_while_the_first_is_held_waits_its_turn` does).
            Step::Undo { .. } if self.undoing.as_ref().is_some_and(|u| !u.is_finished()) => {}
            Step::Undo { held } => {
                drop(disk);
                // One answered already ends first.
                self.undo_ends();
                // Only its own torrent's put-back is held (#190): every other pack's Delete, and
                // whatever else the steps do meanwhile, goes on and is answered, never delayed by it.
                let top = self.rt.block_on(self.engine().inspect(|actor| {
                    actor
                        .steps
                        .machine()
                        .top()
                        .and_then(|p| super::undo::undoable(actor.steps.machine(), p.seq))
                        .map(|u| u.torrents[0].name.clone())
                }));
                let pack = top.and_then(|name| name.trim_start_matches("pack").parse::<usize>().ok());
                // The torrent it brings back is the pack's that left last.
                let at = pack.and_then(|pack| Some((pack, self.left.get(&pack)?.last()?.0)));
                match (held, at) {
                    (true, Some((pack, id))) => super::trash::hold_under(&self.gate, &self.spot(pack, id)),
                    (true, None) | (false, _) => super::trash::hold(&self.gate, held),
                }
                let undoing = self.rt.spawn(self.engine().undo_later());
                // Until it's under way or answered (refused at once, or back with nothing to put back).
                let deadline = Instant::now() + SETTLE;
                while !undoing.is_finished()
                    && !self.rt.block_on(self.engine().inspect(|actor| super::undo::undoing(actor.steps.machine()).next().is_some()))
                {
                    assert!(Instant::now() < deadline, "the undo never started");
                    std::thread::sleep(Duration::from_millis(5));
                }
                self.undoing = Some(undoing);
                if !held {
                    self.undo_ends();
                }
            }
            Step::UndoEnds => {
                drop(disk);
                self.undo_ends();
            }
            Step::KillMidUndo { block, empty, lose_stash, at } => {
                drop(disk);
                self.kill_mid_undo(block, empty, lose_stash, at);
            }
            Step::LoseStash => {
                drop(disk);
                let under_way = self.rt.block_on(
                    self.engine().inspect(|actor| under_way(actor).map(|e| (e.info_hash.clone(), e.put_back.clone(), stash_of(&e)))),
                );
                let held = self.undoing.is_some() && super::trash::waiting(&self.gate);
                if let (Some((hash, items, Some(stash))), true) = (under_way, held) {
                    std::fs::remove_file(stash.join("torrent")).unwrap();
                    let pack = self.hashes.iter().position(|h| *h == hash).expect("a pack's");
                    self.stash_lost = Some((pack, items, false));
                }
            }
            Step::DestinationGone { pack } if !disk.unavailable[pack] => {
                if let Some((lost, _, went)) = &mut self.stash_lost {
                    *went |= *lost == pack;
                }
                let here = self.here(pack);
                std::fs::rename(&here, moved_away(self.dir.path(), pack)).unwrap();
                disk.unavailable[pack] = true;
                disk.gone_from[pack] = Some(here);
            }
            Step::DestinationBack { pack } if disk.unavailable[pack] => {
                let back = disk.gone_from[pack].take().unwrap_or_else(|| destination(self.dir.path(), pack));
                std::fs::rename(moved_away(self.dir.path(), pack), back).unwrap();
                disk.unavailable[pack] = false;
            }
            Step::DestinationGone { .. } | Step::DestinationBack { .. } => {}
            Step::GoesWhileCreated { pack, during } => disk.goes[pack] = Some(during),
            Step::Move { pack, held, copies } => {
                drop(disk);
                self.start_move(pack, held, copies);
            }
            Step::MoveEnds => {
                drop(disk);
                self.move_ends();
            }
            Step::KillMidMove => {
                drop(disk);
                self.kill_mid_move();
            }
            Step::KillAfterPlaced { pack, copies } => {
                drop(disk);
                self.kill_after_placed(pack, copies);
            }
            Step::Arrive { pack } => {
                drop(disk);
                self.arrive(pack)
            }
            Step::Restart => {
                let before = self.progress(&disk);
                // Those not checked, whose destination can be reached: still so after the restart.
                let unchecked: BTreeSet<TorrentId> = self
                    .rt
                    .block_on(self.engine().listing())
                    .into_iter()
                    .filter(|t| {
                        t.status == Status::Unchecked && self.model.iter().any(|(k, (id, ..))| *id == t.id && !disk.unavailable[*k])
                    })
                    .map(|t| t.id)
                    .collect();
                drop(disk);
                self.seeder.open.send_replace(false);
                // A check held would keep the session from stopping.
                self.checks.hold(false);
                // The stop waits for an undo or a move under way: the gates open only once it's asked.
                let (gate, move_gate) = (self.gate.clone(), self.move_gate.clone());
                self.rt.block_on(async {
                    let stopping = self.engine().stop();
                    tokio::pin!(stopping);
                    let stopped = tokio::time::timeout(Duration::from_millis(300), &mut stopping).await.is_ok();
                    super::trash::hold(&gate, false);
                    super::trash::hold(&move_gate, false);
                    if !stopped {
                        stopping.await;
                    }
                });
                self.undo_ends();
                // A move under way went back, as the stop has it, or ended first.
                let ended = self.moving.take().map(|m| {
                    let answer = self.rt.block_on(m.answer).unwrap();
                    for waiting in m.behind {
                        let _ = self.rt.block_on(waiting);
                    }
                    (m.pack, m.id, m.elsewhere, answer, m.old, m.new)
                });
                self.start();
                if let Some((pack, id, elsewhere, answer, old, new)) = ended {
                    self.moved(pack, id, elsewhere, &answer, &old, &new);
                }
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
                self.kept(&before);
                let now = self.rt.block_on(self.engine().listing());
                for id in unchecked {
                    let status = now.iter().find(|t| t.id == id).map(|t| &t.status);
                    assert!(
                        status.is_none_or(|s| *s == Status::Unchecked),
                        "torrent {id} was not checked before the restart, and is {status:?}"
                    );
                }
                self.seeder.open.send_replace(true);
            }
        }
    }

    /// A Remove or a Delete of the pack whose move waits at its gate: it cancels the move first,
    /// which puts back what it moved (nothing of it in the new destination but what was there,
    /// nothing staged), then it's carried out where the torrent was.
    fn remove_moving(&mut self, pack: usize, delete: bool) {
        let Some(m) = self.moving.take() else { return };
        let id = m.id;
        let command = if delete { TorrentCommand::Delete } else { TorrentCommand::Remove };
        let removing = self.rt.spawn(self.engine().command_later(id, command));
        let deadline = Instant::now() + SETTLE;
        let cancelled = move |a: &mut Actor| {
            use lifecycle::moving::Stage;
            a.torrents
                .get(&id)
                .is_none_or(|t| !matches!(t.lifecycle.phase, Phase::Moving { stage: Stage::TakingOut | Stage::Copying { .. }, .. }))
        };
        while !self.rt.block_on(self.engine().inspect(cancelled)) {
            assert!(Instant::now() < deadline, "the Remove never cancelled the move");
            std::thread::sleep(Duration::from_millis(5));
        }
        super::trash::hold(&self.move_gate, false);
        let answer = self.rt.block_on(async { tokio::time::timeout(SETTLE, m.answer).await }).expect("every move is answered").unwrap();
        assert!(
            matches!(
                &answer,
                Err(super::Failure::Refused(proto::Refusal::Torrent {
                    why: proto::TorrentRefusal::NotMoved { why: proto::NotMoved::Cancelled, .. },
                    ..
                }))
            ),
            "a Remove or a Delete cancels the move: {answer:?}"
        );
        for waiting in m.behind {
            let _ = self.rt.block_on(async { tokio::time::timeout(SETTLE, waiting).await }).expect("what waited behind a move is answered");
        }
        let _ = self.rt.block_on(async { tokio::time::timeout(SETTLE, removing).await }).expect("every command is answered");
        let (home, other) = (destination(self.dir.path(), pack), elsewhere(self.dir.path(), pack));
        let (old_root, new_root) = if m.elsewhere { (home, other) } else { (other, home) };
        let name = format!("pack{pack}");
        assert_eq!(bytes_under(&new_root.join(&name)), m.new, "cancelled, and the new destination changed");
        for root in [&old_root, &new_root] {
            assert!(!root.join(format!(".phantom-moving-{}", self.hashes[pack])).exists(), "a move left its staging in {}", root.display());
        }
        // Removed, its files where they were; deleted, from there.
        if delete {
            self.deleted.insert(id, m.old);
        } else {
            assert_eq!(
                bytes_under(&old_root.join(&name)).keys().collect::<Vec<_>>(),
                m.old.keys().collect::<Vec<_>>(),
                "removed, its files kept"
            );
        }
        let disk = self.disk.clone();
        let disk = disk.lock().unwrap();
        self.removed(pack, delete, &disk);
    }

    /// Before the pack's Delete: the bytes of its torrent's files, for its undo.
    fn deleting(&mut self, pack: usize, delete: bool) {
        if delete && let Some(&(id, ..)) = self.model.get(&pack) {
            let folder = self.here(pack).join(format!("pack{pack}"));
            self.deleted.insert(id, bytes_under(&folder));
        }
    }

    /// The pack's torrent was removed or deleted: it left the list, but for a Delete refused, as
    /// for a torrent with info whose destination is unavailable.
    fn removed(&mut self, pack: usize, delete: bool, disk: &Destinations) {
        let refused = delete && disk.unavailable[pack] && self.model.get(&pack).is_some_and(|&(_, stage, _)| stage == Stage::Told);
        if !refused && let Some((id, stage, _)) = self.model.remove(&pack) {
            self.left.entry(pack).or_default().push((id, stage == Stage::Told));
        }
    }

    /// [`Step::KillMidUndo`].
    fn kill_mid_undo(&mut self, block: bool, empty: bool, lose_stash: bool, at: prop::sample::Index) {
        let under_way = self.rt.block_on(
            self.engine().inspect(|actor| under_way(actor).map(|e| (e.info_hash.clone(), e.id, e.put_back.clone(), stash_of(&e)))),
        );
        let told = self.model.values().all(|(_, stage, _)| *stage == Stage::Told);
        let (Some((hash, id, items, stash)), Some(undoing), true) = (under_way, self.undoing.take(), told) else { return };
        // Lost while it was held, or now.
        let lose_stash = lose_stash && stash.is_some() || self.stash_lost.take().is_some();
        let pack = self.hashes.iter().position(|h| *h == hash).expect("a pack's");
        self.seeder.open.send_replace(false);
        self.rt.block_on(self.engine().kill());
        self.engine = None;
        super::trash::abandon(&self.gate);
        let _ = self.rt.block_on(undoing);
        for (_, waiting) in std::mem::take(&mut self.waiting) {
            let _ = self.rt.block_on(waiting);
        }
        self.gate = Default::default();
        // Its put-back's thread, past the gate, ends: it doesn't make or remove folders meanwhile.
        let deadline = Instant::now() + SETTLE;
        while self.trash.strong_count() > 0 {
            assert!(Instant::now() < deadline, "the killed engine's put-back never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        // While it's down: what's emptied from the trash, what's put where an item still there
        // goes back, and the stash.
        let reachable = !self.disk.lock().unwrap().unavailable[pack];
        let still: Vec<&super::trash::Trashed> = items.iter().rev().filter(|i| i.to.as_ref().is_some_and(|to| to.exists())).collect();
        let emptied = still.first().filter(|_| empty).map(|i| i.from.clone());
        if let Some(item) = still.first().filter(|_| empty) {
            let to = item.to.as_deref().unwrap();
            if to.is_dir() { std::fs::remove_dir_all(to) } else { std::fs::remove_file(to) }.unwrap();
        }
        let blocked = (!still.is_empty() && block && reachable).then(|| still[at.index(still.len())].from.clone());
        if let Some(path) = &blocked {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, MINE).unwrap();
        }
        if let Some(stash) = stash.filter(|_| lose_stash) {
            let _ = std::fs::remove_file(stash.join("torrent"));
        }
        self.start();
        // The undo cut short is finished at start.
        let deadline = Instant::now() + SETTLE;
        while self.rt.block_on(self.engine().inspect(|actor| super::undo::undoing(actor.steps.machine()).next().is_some())) {
            assert!(Instant::now() < deadline, "the undo cut short never ended");
            std::thread::sleep(Duration::from_millis(10));
        }
        let listed = self.rt.block_on(self.engine().listing()).iter().any(|t| t.id == id);
        // Where its destination is now: it may have gone as the torrent was added again.
        let unavailable = self.disk.lock().unwrap().unavailable[pack];
        let home = self.spot(pack, id);
        let root = if unavailable { moved_away(self.dir.path(), pack) } else { home.clone() };
        let folder = root.join(format!("pack{pack}"));
        let moved = |bytes: &BTreeMap<std::path::PathBuf, Vec<u8>>| -> BTreeMap<std::path::PathBuf, Vec<u8>> {
            let from = home.join(format!("pack{pack}"));
            bytes.iter().map(|(p, b)| (folder.join(p.strip_prefix(&from).unwrap()), b.clone())).collect()
        };
        // An emptied item's files are gone, made again empty once it's back.
        let kept = |bytes: BTreeMap<std::path::PathBuf, Vec<u8>>, under: &Path| -> BTreeMap<std::path::PathBuf, Vec<u8>> {
            let gone = emptied.as_ref().map(|e| under.join(e.strip_prefix(&home).unwrap()));
            bytes.into_iter().filter(|(p, _)| gone.as_ref().is_none_or(|g| !p.starts_with(g))).collect()
        };
        if listed {
            assert_eq!((&blocked, lose_stash), (&None, false), "an undo that can't finish at start is refused");
            assert_eq!(kept(bytes_under(&folder), &root), kept(moved(&self.deleted[&id]), &root), "back with the bytes it had");
            self.left.entry(pack).or_default().retain(|(left, _)| *left != id);
            self.model.insert(pack, (id, Stage::Told, 0));
        } else {
            for item in &items {
                let at_path = match &blocked {
                    Some(path) if *path == item.from => std::fs::read(path).unwrap() != MINE,
                    Some(_) | None => item.from.exists(),
                };
                let in_trash = item.to.as_ref().is_some_and(|to| to.exists()) != (emptied.as_ref() == Some(&item.from));
                assert!(in_trash && !at_path, "half an undo: {item:?}");
            }
        }
        self.seeder.open.send_replace(true);
    }

    /// An add of the pack answered: the torrent there, or a new one fetching info.
    fn added(&mut self, pack: usize, added: proto::Added) {
        match self.model.get(&pack) {
            Some(&(id, ..)) => assert_eq!((added.id, added.already_existed), (id, true)),
            None => {
                assert!(!added.already_existed && added.id >= self.next_id, "a new torrent gets a new id: {added:?}");
                self.next_id = added.id + 1;
                let gates = self.gates_of(added.id);
                self.model.insert(pack, (added.id, Stage::Waiting, gates));
            }
        }
    }

    /// An undo the actor isn't carrying out any more was answered: one with nothing to put back
    /// ends at once, held or not.
    /// One under way is either held at the gate or about to end: this waits for one of the two, so
    /// the list isn't read between the undo's end and its answer being taken.
    fn ended(&mut self) {
        let deadline = Instant::now() + SETTLE;
        while self.undoing.is_some() {
            if !self.rt.block_on(self.engine().inspect(|actor| super::undo::undoing(actor.steps.machine()).next().is_some())) {
                return self.undo_ends();
            }
            if super::trash::waiting(&self.gate) {
                return;
            }
            assert!(Instant::now() < deadline, "an undo neither ended nor waited at the gate");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Which of `adds` is torrent `id`'s latest.
    fn gates_of(&self, id: TorrentId) -> usize {
        self.adds.lock().unwrap().iter().rposition(|g| g.id == id).expect("its add started")
    }

    /// Opens the gate and waits for the undo under way, if any: one answered is back under the id it
    /// had, fetching info if it had none; one refused changed nothing. Then the adds that waited for
    /// it are answered.
    fn undo_ends(&mut self) {
        super::trash::hold(&self.gate, false);
        let Some(undoing) = self.undoing.take() else { return };
        let answer = self.rt.block_on(async { tokio::time::timeout(SETTLE, undoing).await }).expect("every undo is answered").unwrap();
        if let Some((pack, items, went)) = self.stash_lost.take() {
            assert!(answer.is_err(), "an undo whose stash is lost is refused");
            let unavailable = self.disk.lock().unwrap().unavailable[pack];
            if let Err(super::Failure::Refused(proto::Refusal::Undo { why: proto::CantUndo::AddFailed { .. }, stranded, .. })) = &answer
                && !went
                && !unavailable
            {
                assert!(stranded.is_empty(), "nothing stranded once adding it again failed: {stranded:?}");
                for item in &items {
                    assert!(item.to.as_ref().is_some_and(|to| to.exists()) && !item.from.exists(), "back in the trash: {item:?}");
                }
            }
        }
        // Each torrent of the entry that came back, a press of both too (#235).
        if let Ok(undone) = answer {
            let listed = self.rt.block_on(self.engine().listing());
            for torrent in &undone.undo.torrents {
                let name = torrent.name.clone();
                let pack: usize = name.trim_start_matches("pack").parse().expect("a pack's");
                assert!(!self.model.contains_key(&pack), "an undo brings back only one that isn't there");
                let back = listed.iter().find(|t| t.name.as_deref() == Some(name.as_str())).expect("it's listed");
                let left = self.left.entry(pack).or_default();
                let at = left
                    .iter()
                    .position(|(id, _)| *id == back.id)
                    .unwrap_or_else(|| panic!("pack {pack} is back as {}, not an id it had", back.id));
                let (id, had_info) = left.remove(at);
                let stage = if had_info { (Stage::Told, 0) } else { (Stage::Waiting, self.gates_of(id)) };
                self.model.insert(pack, (id, stage.0, stage.1));
            }
        }
        for (pack, waiting) in std::mem::take(&mut self.waiting) {
            let added = self.rt.block_on(async { tokio::time::timeout(SETTLE, waiting).await }).expect("every add is answered");
            self.added(pack, added.unwrap().unwrap());
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
            let unknown: Vec<String> = hashes.into_iter().filter(|h| actor.by_hash(h).is_none()).collect();
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
            let gone = disk.gone_from[pack].clone().unwrap_or_else(|| destination(self.dir.path(), pack));
            assert!(!gone.exists(), "a destination that's unavailable was created");
        }
        // Check progress shows only while Checking: otherwise progress is pieces had (#141).
        for (&pack, &(id, ..)) in &self.model {
            let Some(t) = published.torrents.iter().find(|t| t.id == id) else { continue };
            let shown = t.content.map_or(0, |c| c.progress_bytes);
            let valid = valid_on_disk(&self.folder_of(pack, &disk), pack);
            // Moving, its files are on their way: what it had is what it shows.
            assert!(
                matches!(t.status, Status::Checking | Status::Moving { .. }) || shown <= valid,
                "torrent {id} shows {shown} bytes {:?}, {valid} valid on disk",
                t.status
            );
        }
    }

    /// Opens every gate and waits until every torrent has its info.
    fn finish(&mut self) {
        self.undo_ends();
        self.move_ends();
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

/// The entry of the undo under way, if one is: the steps never ask a second while one is.
fn under_way(actor: &mut Actor) -> Option<super::undo::Entry> {
    super::undo::undoing(actor.steps.machine()).next().cloned()
}

/// The torrents whose librqbit side and phase disagree, and why.
fn librqbit_side_disagrees(actor: &mut Actor) -> Vec<String> {
    use super::machine::lib::Lib;
    let mut wrong = Vec::new();
    let listed = actor.snapshot().torrents;
    for (id, t) in &actor.torrents {
        let phase = &t.lifecycle.phase;
        let no_info = matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. });
        if listed.iter().find(|l| l.id == *id).is_none_or(|l| l.content.is_none() != no_info) {
            wrong.push(format!("{id}: {phase:?}, listed with content only once it has info"));
        }
        let agrees = match &t.lib {
            Lib::Adding(_) => *phase == Phase::FetchingInfo,
            // Never away or not checked in librqbit: out until checked.
            Lib::In(h) | Lib::Resuming(h) => {
                let in_session = actor.session.get(TorrentIdOrHash::Id(h.id())).is_some_and(|s| Arc::ptr_eq(&s, h));
                let out = matches!(
                    phase,
                    Phase::FetchingInfo
                        | Phase::FetchFailed { .. }
                        | Phase::Unchecked { .. }
                        | Phase::DestinationUnavailable { .. }
                        | Phase::Moving { .. }
                );
                in_session && !out
            }
            // Out only while it needn't run: never added, an error, away, missing files, not checked,
            // or being deleted.
            Lib::Out => matches!(
                phase,
                Phase::FetchFailed { .. }
                    | Phase::Error { .. }
                    | Phase::Unchecked { .. }
                    | Phase::MissingFiles { .. }
                    | Phase::DestinationUnavailable { .. }
                    | Phase::DeleteFailed { .. }
                    | Phase::Deleting { .. }
                    | Phase::Deleted
                    | Phase::Moving { .. }
            ),
        };
        if !agrees {
            wrong.push(format!("{id}: {phase:?}, librqbit side {}", kind(&t.lib)));
        }
    }
    wrong
}

fn kind<H, T>(s: &super::machine::lib::Lib<H, T>) -> &'static str {
    use super::machine::lib::Lib;
    match s {
        Lib::Adding(_) => "Adding",
        Lib::In(_) => "In",
        Lib::Resuming(_) => "Resuming",
        Lib::Out => "Out",
    }
}

impl Drop for Sut {
    /// Dropped, not stopped: the case is over, and a stop (librqbit's waits a second, for the
    /// engine and the seeder alike) is what Restart checks. A check held and a Delete or undo
    /// waiting at the gate end, so the runtime can end; it ends the tasks of both sessions.
    fn drop(&mut self) {
        self.checks.hold(false);
        super::trash::hold(&self.gate, false);
        super::trash::hold(&self.move_gate, false);
    }
}

/// Every file of the torrent's under `folder`, by path, with its bytes: not the user's
/// `mine.txt`.
fn bytes_under(folder: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut all = BTreeMap::new();
    let mut stack = vec![folder.to_owned()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            } else if e.file_name() != "mine.txt" {
                all.insert(e.path(), std::fs::read(e.path()).unwrap_or_default());
            }
        }
    }
    all
}

/// The stash an undo adds its torrent again from, if any.
fn stash_of(entry: &super::undo::Entry) -> Option<std::path::PathBuf> {
    use super::undo::{Back, Source};
    match &entry.back {
        Back::Gone { source: Source::Stash(stash), .. } | Back::InList { stash } => Some(stash.dir.clone()),
        // A Move to Trash's undo adds nothing again: no stash.
        Back::Gone { source: Source::Magnet(_), .. } | Back::Files { .. } | Back::Renamed { .. } => None,
    }
}

/// Pack 1's is on a drive.
fn destination(dir: &Path, pack: usize) -> std::path::PathBuf {
    let on = if pack == 1 { Path::new("/Volumes/Macintosh HD").join(dir.strip_prefix("/").unwrap()) } else { dir.to_owned() };
    on.join(format!("destination{pack}"))
}

/// The pack's other destination, which its torrent moves to and back from.
fn elsewhere(dir: &Path, pack: usize) -> std::path::PathBuf {
    dir.join(format!("elsewhere{pack}"))
}

/// The bytes of each file, by path.
type Files = BTreeMap<std::path::PathBuf, Vec<u8>>;

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
        3 => (pack.clone(), any::<bool>()).prop_map(|(pack, delete)| Step::Remove { pack, delete }),
        1 => pack.clone().prop_map(|pack| Step::DestinationGone { pack }),
        1 => pack.clone().prop_map(|pack| Step::DestinationBack { pack }),
        1 => (pack.clone(), any::<bool>()).prop_map(|(pack, during)| Step::GoesWhileCreated { pack, during }),
        2 => pack.clone().prop_map(|pack| Step::Arrive { pack }),
        2 => pack.clone().prop_map(|pack| Step::Check { pack }),
        3 => pack.clone().prop_map(|pack| Step::CancelCheck { pack }),
        2 => Just(Step::HoldChecks),
        2 => Just(Step::ReleaseChecks),
        2 => Just(Step::Restart),
        3 => any::<bool>().prop_map(|held| Step::Undo { held }),
        2 => Just(Step::UndoEnds),
        2 => (any::<bool>(), any::<bool>(), any::<bool>(), any::<prop::sample::Index>())
            .prop_map(|(block, empty, lose_stash, at)| Step::KillMidUndo { block, empty, lose_stash, at }),
        1 => Just(Step::LoseStash),
        2 => pack.clone().prop_map(|pack| Step::WriteMine { pack }),
        2 => (pack.clone(), any::<bool>(), any::<bool>()).prop_map(|(pack, delete, both)| Step::CrashAsRemoved { pack, delete, both }),
        3 => (pack.clone(), any::<bool>(), any::<bool>()).prop_map(|(pack, held, copies)| Step::Move { pack, held, copies }),
        2 => Just(Step::MoveEnds),
        1 => Just(Step::KillMidMove),
        1 => (pack.clone(), any::<bool>()).prop_map(|(pack, copies)| Step::KillAfterPlaced { pack, copies }),
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
        sut.ended();
        sut.check();
    }
    sut.finish();
}

// The races suspected in #32, and the one these tests found.

use Step::*;

/// #88: a check cancelled while the torrent is being added, or after, ends Unchecked, and a
/// restart keeps it so.
#[test]
fn a_cancelled_check_stays_not_checked_whatever_follows() {
    run(&[HoldChecks, Add { pack: 0, paused: false }, Arrive { pack: 0 }, CancelCheck { pack: 0 }, ReleaseChecks, Restart]);
    run(&[Add { pack: 0, paused: false }, Arrive { pack: 0 }, HoldChecks, Check { pack: 0 }, CancelCheck { pack: 0 }, Resume { pack: 0 }]);
    run(&[
        HoldChecks,
        Add { pack: 1, paused: true },
        Arrive { pack: 1 },
        CancelCheck { pack: 1 },
        DestinationGone { pack: 1 },
        DestinationBack { pack: 1 },
    ]);
}

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

/// #26 (rqbit#735), which the generated sequences shrink to once `Engine::stop` doesn't drop the
/// session and wait for the saves: kept so it runs every time.
#[test]
fn what_a_torrent_downloaded_is_kept_across_a_restart() {
    run(&[Add { pack: 0, paused: false }, Arrive { pack: 0 }, Restart]);
    run(&[Add { pack: 0, paused: false }, Arrive { pack: 0 }, Pause { pack: 0 }, Restart]);
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

/// #141: a torrent taken out of librqbit while librqbit checks it as it's added (its destination
/// went as its files were made) showed the check's progress as what it has, under Destination
/// unavailable. The minimal case the property after every step found, red before the
/// fix in most runs, kept so it runs every time.
#[test]
fn a_torrent_taken_out_mid_check_shows_no_check_progress() {
    run(&[GoesWhileCreated { pack: 1, during: true }, Add { pack: 1, paused: true }, Arrive { pack: 1 }]);
}

/// The races of an undo (#150), as the generated sequences reach them less often: kept so they run
/// every time.
#[test]
fn an_add_of_the_torrent_an_undo_brings_back_waits_for_it() {
    let added = [Add { pack: 0, paused: false }, Arrive { pack: 0 }];
    run(&[&added[..], &[Remove { pack: 0, delete: true }, Undo { held: true }, Add { pack: 0, paused: false }, UndoEnds]].concat());
    // Refused, as something is at the path: the add that waited is a new torrent.
    run(&[&added[..], &[Remove { pack: 0, delete: false }, Add { pack: 0, paused: false }, Undo { held: true }, UndoEnds]].concat());
}

#[test]
fn a_stop_waits_for_an_undo_under_way() {
    run(&[Add { pack: 0, paused: false }, Arrive { pack: 0 }, Remove { pack: 0, delete: true }, Undo { held: true }, Restart]);
    run(&[
        Add { pack: 1, paused: true },
        Arrive { pack: 1 },
        Remove { pack: 1, delete: true },
        Undo { held: true },
        Add { pack: 1, paused: true },
        Restart,
    ]);
}

#[test]
fn a_destination_gone_while_its_files_go_back_is_not_made_again() {
    run(&[
        Add { pack: 0, paused: false },
        Arrive { pack: 0 },
        Remove { pack: 0, delete: true },
        Undo { held: true },
        DestinationGone { pack: 0 },
        UndoEnds,
    ]);
}

#[test]
fn a_torrent_removed_while_fetching_info_comes_back_fetching_it() {
    run(&[Add { pack: 0, paused: false }, Remove { pack: 0, delete: false }, Undo { held: false }, Arrive { pack: 0 }]);
    run(&[
        Add { pack: 0, paused: false },
        Open { pack: 0 },
        Open { pack: 0 },
        Remove { pack: 0, delete: true },
        Undo { held: false },
        Arrive { pack: 0 },
    ]);
}

/// An undo cut short by a crash as its first item went back is finished at the next start.
#[test]
fn an_undo_cut_short_by_a_crash_is_finished_at_start() {
    run(&[
        Add { pack: 0, paused: false },
        Arrive { pack: 0 },
        Remove { pack: 0, delete: true },
        Undo { held: true },
        KillMidUndo { block: false, empty: false, lose_stash: false, at: first() },
    ]);
    run(&[
        Add { pack: 1, paused: true },
        Arrive { pack: 1 },
        Remove { pack: 1, delete: true },
        Undo { held: true },
        KillMidUndo { block: false, empty: false, lose_stash: false, at: first() },
        Restart,
    ]);
}

/// The drive goes as an undo cut short by a crash adds its torrent again at the next start: the
/// files are back, in the destination wherever it went.
#[test]
fn an_undo_finished_at_start_as_its_drive_goes_is_back_there() {
    run(&[
        Add { pack: 1, paused: false },
        Arrive { pack: 1 },
        GoesWhileCreated { pack: 1, during: false },
        Remove { pack: 1, delete: true },
        Undo { held: true },
        KillMidUndo { block: false, empty: false, lose_stash: false, at: first() },
    ]);
}

/// #162: an undo cut short by a crash once it put back the first of a Delete's files, resumed at
/// start with something of the user's where the next goes back: refused, and the first goes back
/// to the trash too, as an undo that can't finish does.
#[test]
fn an_undo_finished_at_start_that_can_not_finish_puts_back_nothing() {
    run(&[
        Add { pack: 0, paused: false },
        Arrive { pack: 0 },
        WriteMine { pack: 0 },
        Remove { pack: 0, delete: true },
        Undo { held: true },
        KillMidUndo { block: true, empty: false, lose_stash: false, at: first() },
    ]);
}

/// #162: a crash between keeping what undoes a Remove or a Delete and its record going: the torrent
/// is off the list at the next start, and the undo brings it back.
#[test]
fn a_remove_cut_short_by_a_crash_is_finished_at_start() {
    for delete in [false, true] {
        run(&[Add { pack: 0, paused: false }, Arrive { pack: 0 }, CrashAsRemoved { pack: 0, delete, both: false }, Undo { held: false }]);
    }
    run(&[
        Add { pack: 1, paused: true },
        Arrive { pack: 1 },
        WriteMine { pack: 1 },
        CrashAsRemoved { pack: 1, delete: true, both: false },
        Undo { held: false },
    ]);
}

/// The same, of a Remove or a Delete of both torrents in one press (#235): the first part done, the
/// crash at the last's: one entry holding both at the next start, and one undo brings both back.
#[test]
fn a_selection_cut_short_by_a_crash_is_one_entry_at_start() {
    for delete in [false, true] {
        run(&[
            Add { pack: 0, paused: false },
            Arrive { pack: 0 },
            Add { pack: 1, paused: true },
            Arrive { pack: 1 },
            CrashAsRemoved { pack: 0, delete, both: true },
            Undo { held: false },
        ]);
    }
}

/// The first of any list, for a step's `at`.
fn first() -> prop::sample::Index {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let mut tree = any::<prop::sample::Index>().new_tree(&mut runner).unwrap();
    while tree.simplify() {}
    tree.current()
}

/// Review of #181: adding the torrent again fails once its files are back (its stash's .torrent
/// lost), held or after a crash cut the undo short, with an item emptied from the trash meanwhile:
/// it's refused, what's back goes to the trash again, and nothing is stranded.
#[test]
fn an_undo_that_can_not_add_its_torrent_again_puts_its_files_back_in_the_trash() {
    let deleted = [Add { pack: 0, paused: false }, Arrive { pack: 0 }, WriteMine { pack: 0 }, Remove { pack: 0, delete: true }];
    run(&[&deleted[..], &[Undo { held: true }, LoseStash, UndoEnds]].concat());
    run(&[&deleted[..], &[Undo { held: true }, KillMidUndo { block: false, empty: true, lose_stash: true, at: first() }]].concat());
}

/// #24: a move held mid-copy, then each way it can end: its gate opens; a Remove or a Delete
/// cancels it; a stop puts it back; a crash cuts it short and the start puts it back. Each, renamed
/// and copied, kept so they run every time.
#[test]
fn an_add_into_the_old_folder_of_a_moved_torrent_goes_through_while_its_new_folder_is_gone() {
    run(&[
        Add { pack: 0, paused: false },
        Arrive { pack: 0 },
        Move { pack: 0, held: false, copies: false },
        DestinationGone { pack: 0 },
        Add { pack: 0, paused: false },
    ]);
}

#[test]
fn a_move_held_mid_copy_ends_in_exactly_one_destination_however_it_ends() {
    let arrived = [Add { pack: 0, paused: false }, Arrive { pack: 0 }];
    for copies in [false, true] {
        let held = Move { pack: 0, held: true, copies };
        for then in [&[MoveEnds][..], &[Remove { pack: 0, delete: false }], &[Remove { pack: 0, delete: true }], &[Restart], &[KillMidMove]]
        {
            run(&[&arrived[..], &[held.clone()], then].concat());
        }
        // A crash once its first file took its place: the start finishes it; and back again.
        run(&[&arrived[..], &[KillAfterPlaced { pack: 0, copies }, KillAfterPlaced { pack: 0, copies }]].concat());
        // There and back, and a Pause behind it, answered once it's moved.
        run(&[&arrived[..], &[held.clone(), Pause { pack: 0 }, MoveEnds, Move { pack: 0, held: false, copies }]].concat());
    }
    // The old destination goes as the copy waits: it's put back, once its drive is.
    run(&[&arrived[..], &[Move { pack: 0, held: true, copies: true }, DestinationGone { pack: 0 }, MoveEnds, DestinationBack { pack: 0 }]]
        .concat());
}

/// Found by #264's 256-case run: a move held at its gate, checks held, then a Remove that crashes
/// as it takes the torrent off the list. Checks held while the move was under way held librqbit's
/// sampling once it ended, and the Remove behind it: the test's own gate, never opened.
#[test]
fn a_remove_crashing_after_a_held_move_with_checks_held_is_answered() {
    run(&[
        Add { pack: 0, paused: false },
        Open { pack: 0 },
        Restart,
        Move { pack: 0, held: true, copies: false },
        HoldChecks,
        CrashAsRemoved { pack: 0, delete: false, both: false },
    ]);
}

/// #328, found by a 256-case run: a pack's destination goes as librqbit creates its files, so the
/// torrent is Destination unavailable, and a Move of it is refused at once; the test took that for
/// a move held at its gate, which the Remove after it would cancel.
#[test]
fn a_move_of_a_torrent_whose_destination_went_as_it_was_created_then_a_remove() {
    run(&[
        Add { pack: 1, paused: false },
        GoesWhileCreated { pack: 1, during: false },
        Arrive { pack: 1 },
        Move { pack: 1, held: true, copies: false },
        Remove { pack: 1, delete: false },
    ]);
}
