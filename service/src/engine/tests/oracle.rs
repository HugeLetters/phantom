//! #374: the oracle every write to a torrent's files is held to. Sequences over the real engine, with
//! a real local peer, judged by two rules:
//!
//! - **Safe:** bytes a person put at a torrent's path never change, nor is their file cut, grown,
//!   removed or replaced, unless the person allowed it ([`Allow`] says where each allowance was decided).
//! - **Alive:** a torrent's own untouched files keep working: it ends complete, with the right bytes
//!   and no press. Whatever else happened, once the person took their own files away it's complete
//!   again with a few presses, never stuck.
//!
//! What's known to fail is [`KNOWN_OPEN`], each with a named, ignored case below. A gate PR that closes
//! one deletes its entry and moves its cases out of `open!`, to pass: those whose failure ends with
//! `open: [its why]` (run them with `--ignored`). A class holds from what led to it until the torrent
//! settles where nothing more comes by itself ([`World::settled`]).

use std::fmt::Debug;
use std::os::unix::fs::{FileExt, MetadataExt, PermissionsExt};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Weak};

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use super::*;

const NAMES: [&str; 3] = ["a.bin", "b.bin", "c.bin"];
/// Pieces 1 and 3 are shared by two files.
const SIZES: [usize; 3] = [24576, 36864, 28672];
/// How many sequences run at once.
const WIDTH: usize = 12;
/// How long what must happen may take, on a loaded machine too.
const PATIENCE: Duration = Duration::from_secs(60);

/// The torrent's own `file`: no zero in it.
fn own(file: usize) -> Vec<u8> {
    (0..SIZES[file]).map(|n| (((n * 7 + n / 251 + file * 31) % 255) + 1) as u8).collect()
}

/// Bytes of which no piece of the torrent is.
fn theirs(len: usize) -> Vec<u8> {
    (0..len).map(|n| 100 + (n % 91) as u8).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// A different file of the same size.
    Same,
    Longer,
    /// Its own bytes, with more after them.
    OwnAndMore,
    Shorter,
    Empty,
    /// The same size, one piece the torrent's.
    OnePiece,
    /// The torrent's own, cut off after a piece.
    Cut,
    /// A copy of the torrent's own.
    Equal,
    /// A symbolic link to a file of the same size.
    Link,
    /// Another name of a different file of the same size.
    HardLink,
    /// Another name of a file equal to the torrent's own.
    EqualHardLink,
    /// The torrent's own file, written into by hand.
    Edit,
    /// Another listed torrent's, complete.
    Another,
}

impl Kind {
    /// One that can't be the torrent's to write, whatever is pressed (#374, #361).
    fn never(self) -> bool {
        matches!(self, Kind::Longer | Kind::OwnAndMore | Kind::Link | Kind::HardLink | Kind::Another)
    }

    fn equal(self) -> bool {
        matches!(self, Kind::Equal | Kind::EqualHardLink)
    }
}

/// What may lead the engine to open a file that's at a path, since it was put there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Route {
    /// Nothing but running: put back where one was missing, say.
    Running,
    Add,
    /// Another torrent's add.
    AddOther,
    /// A file changed while one was known missing: the check of it is of every file.
    PutBack,
    Check,
    /// Not shown as Missing files with that file there, so not a choice of it.
    Resume,
    Priority,
    Move,
    Rename,
    Remove,
    Undo,
    DriveBack,
    Restart,
    Crash,
}

/// Whether the torrent's was known missing where it was put.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Missing {
    No,
    /// Nothing of it had been had.
    NoneHad,
    /// No more of it than `Cut` has.
    PieceHad,
    Had,
}

/// Where a violation belongs.
#[derive(Debug, Clone)]
struct Class {
    kind: Kind,
    routes: BTreeSet<Route>,
    /// The torrent had everything where it was put.
    whole: bool,
    missing: Missing,
    /// Its file was set Off at some time since: no entry is of it now (#374), but a failure shows it.
    #[allow(dead_code)]
    off: bool,
    /// Its folder or drive went while something was under way: as `off`.
    #[allow(dead_code)]
    gone_meanwhile: bool,
}

/// A class known to fail: its kinds, the routes to it (any of them taken is enough), and which
/// files of those.
struct Open {
    kinds: &'static [Kind],
    routes: &'static [Route],
    which: Which,
    /// The issue, or the situation in `coordination/file-trust.md` (a section and row).
    why: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
    Any,
    /// Put where the torrent's wasn't known missing.
    There,
    /// As `There`, and the torrent hadn't everything.
    Part,
    /// As `Missing`, nothing of it had.
    NoneHad,
}

use Kind::*;
/// What passes today, so is in no entry: Longer, OwnAndMore, Link, Equal, and HardLink (#374: an
/// incomplete one is Missing files, a whole one only read).
const OTHERS: &[Kind] = &[Same, Shorter, Empty, OnePiece, Cut, Edit];

const SAME_SIZE: &[Kind] = &[Same, OnePiece, Edit];
const SHORT: &[Kind] = &[Shorter, Empty, Cut];

const fn known(kinds: &'static [Kind], routes: &'static [Route], which: Which, why: &'static str) -> Open {
    Open { kinds, routes, which, why }
}

/// What fails today. Everything else must pass.
const KNOWN_OPEN: &[Open] = &[
    known(&[Another], &[Route::AddOther], Which::Any, "#374, #380's review: two torrents share files"),
    known(OTHERS, &[Route::Add], Which::Any, "file-trust A: first add, hard link"),
    known(OTHERS, &[Route::Check], Which::Any, "file-trust 1 row 9: Check"),
    known(OTHERS, &[Route::Resume], Which::Any, "file-trust 1 row 9: Resume of files not shown, or a hard link"),
    known(OTHERS, &[Route::Move, Route::Rename], Which::There, "file-trust B: Moving"),
    known(OTHERS, &[Route::Priority], Which::There, "file-trust 1 rows 17, 18: Off and Normal check the rest"),
    known(OTHERS, &[Route::PutBack], Which::There, "file-trust 1 row 6: the check of one put back is of every file"),
    known(OTHERS, &[Route::Undo], Which::Any, "file-trust B: undo of a Remove"),
    known(SAME_SIZE, &[Route::DriveBack], Which::There, "#373"),
    known(SAME_SIZE, &[Route::Restart, Route::Crash], Which::There, "file-trust A: replaced, same size, then a start"),
    known(SHORT, &[Route::DriveBack, Route::Restart, Route::Crash], Which::Part, "file-trust A: shorter, where less was had"),
    known(OTHERS, &[Route::Running], Which::NoneHad, "file-trust A: missing known, nothing had"),
];

impl Open {
    fn holds(&self, class: &Class) -> bool {
        self.kinds.contains(&class.kind)
            && self.routes.iter().any(|r| class.routes.contains(r))
            && match self.which {
                Which::There => class.missing == Missing::No,
                Which::Part => class.missing == Missing::No && !class.whole,
                Which::NoneHad => class.missing == Missing::NoneHad,
                Which::Any => true,
            }
    }
}

/// Whether the person allowed their file to be written over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Allow {
    No,
    /// - Resume ("Download over them") of a torrent shown Missing files with that file there, of a
    ///   regular file with one name, not longer (#361, "Download over them" and the hard-link rule).
    /// - A Move that uses it ("Use existing", #270), of such a file.
    /// - A matching piece where Phantom had none of the file (#374, "Enough of a match").
    Yes,
    /// The decisions don't say, or the test can't tell: not judged.
    Unsaid,
}

/// The bytes of a person's file, looked at all the time: damage later repaired is damage.
#[derive(Debug)]
struct Watched {
    /// Keeps its inode, wherever it goes. Of a link, what it leads to.
    handle: std::fs::File,
    bytes: Vec<u8>,
    /// Only this much of it is the person's.
    range: Option<std::ops::Range<usize>>,
    /// What was first seen wrong.
    damage: Mutex<Option<String>>,
    /// When it last changed, as last looked at.
    looked: Mutex<Option<(i64, i64)>>,
}

impl Watched {
    /// What's wrong with it, now or earlier.
    fn damage(&self) -> Option<String> {
        let mut damage = self.damage.lock().unwrap();
        if damage.is_none() {
            *damage = self.look();
        }
        damage.clone()
    }

    fn look(&self) -> Option<String> {
        let meta = self.handle.metadata().unwrap();
        let mut now = vec![0; meta.len() as usize];
        let read = self.handle.read_exact_at(&mut now, 0);
        let was = self.bytes.len();
        if now.len() != was {
            let how = if now.len() < was { "cut" } else { "grown" };
            return Some(format!("{how} from {was} to {} bytes", now.len()));
        }
        // Cut as it was read.
        let n = self.range.clone().unwrap_or(0..was).filter(|n| now[*n] != self.bytes[*n]).count();
        (read.is_err() || n > 0).then(|| format!("{n} of its bytes changed"))
    }

    /// Looks at each of `all` that changed, until the case is over.
    async fn watch(all: Weak<Mutex<Vec<Weak<Watched>>>>) {
        while let Some(all) = all.upgrade() {
            for one in all.lock().unwrap().iter().filter_map(Weak::upgrade) {
                let changed = one.handle.metadata().map(|m| (m.ctime(), m.ctime_nsec())).ok();
                if std::mem::replace(&mut *one.looked.lock().unwrap(), changed) != changed {
                    one.damage();
                }
            }
            drop(all);
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    }
}

/// How much a case can show.
#[derive(Debug, Default, Clone)]
struct Power {
    /// It had a person's file that must stay as it is.
    theirs: bool,
    /// At some look at one, a known class held: damage would be excused.
    excusable: bool,
    /// Looks at such a file, and how many of them a known class held at.
    looks: (usize, usize),
    /// The steps are over: what's left is only whether it ends complete.
    ending: bool,
    /// The peer sent pieces while such a file was there.
    sent: bool,
    /// The known classes whose damage was seen.
    known: BTreeSet<&'static str>,
}

/// A file a person put.
#[derive(Debug)]
struct Theirs {
    kind: Kind,
    file: usize,
    /// Where it was put.
    path: PathBuf,
    watched: Arc<Watched>,
    /// Of what's at `path` itself.
    ino: u64,
    /// Where a link leads, or a hard link's other name; and what's there.
    other: Option<(PathBuf, u64)>,
    /// What the peer had sent as it was put.
    sent: u64,
    /// Something that may take it elsewhere, whole, was asked since it last settled.
    relocated: bool,
    whole: bool,
    missing: Missing,
    off: bool,
    gone_meanwhile: bool,
    /// Shown Missing files with it there.
    seen: bool,
    routes: BTreeSet<Route>,
    allow: Allow,
}

impl Theirs {
    /// Where it is under `home`, but for its other name.
    fn places(&self, home: &Path) -> Vec<PathBuf> {
        let other = self.other.as_ref().map(|(path, _)| path.as_path());
        places(home, self.ino).into_iter().filter(|p| Some(p.as_path()) != other).collect()
    }

    fn class(&self) -> Class {
        Class {
            kind: self.kind,
            routes: self.routes.clone(),
            whole: self.whole,
            missing: self.missing,
            off: self.off,
            gone_meanwhile: self.gone_meanwhile,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum By {
    Put {
        file: usize,
        kind: Kind,
    },
    /// Put where a Move would take the file.
    PutAtTarget {
        file: usize,
        kind: Kind,
    },
    /// The file taken away, kept for `PutBack`.
    TakeAway {
        file: usize,
    },
    PutBack {
        file: usize,
    },
    /// New times, the same bytes.
    Touch {
        file: usize,
    },
    RenameFolder,
    RenameFolderBack,
    DriveGone,
    DriveBack,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Do {
    Check,
    CancelCheck,
    Resume,
    Pause,
    Off { file: usize },
    Normal { file: usize },
    Move { replace: bool },
    Rename,
    Remove,
    Delete,
    TrashFile { file: usize },
    Undo,
    Add,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    By(By),
    Do(Do),
    /// `1`, while it's under way.
    During(Do, By),
    /// Another torrent, one file of it at b.bin's path.
    AddOther,
    Restart,
    Crash,
    /// `0`, while the service isn't running.
    Between {
        by: By,
        crash: bool,
    },
    PeerOpens,
    Settle,
}

/// What's there as the torrent is added.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Start {
    Nothing,
    /// The first file.
    Half,
    Whole,
}

#[derive(Debug, Clone)]
struct Case {
    start: Start,
    peer: bool,
    paused: bool,
    before: Option<(usize, Kind)>,
    steps: Vec<Step>,
}

/// Peers with everything, for every case.
struct Peers {
    _dir: TempDir,
    _sessions: Vec<Arc<librqbit::Session>>,
    /// Each torrent's info (the pack, then the other), and its peer's port.
    infos: Vec<(Vec<u8>, u16)>,
}

impl Peers {
    async fn start() -> Self {
        use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, SessionOptions};
        let dir = temp();
        let (mut sessions, mut infos) = (Vec::new(), Vec::new());
        for other in [false, true] {
            let base = dir.path().join(format!("seed{other}"));
            let pack = base.join("pack");
            std::fs::create_dir_all(&pack).unwrap();
            let files: Vec<(String, Vec<u8>)> = (NAMES.iter().enumerate())
                .map(|(file, name)| match (other, file) {
                    (false, _) => ((*name).into(), own(file)),
                    (true, 1) => ((*name).into(), theirs(SIZES[1])),
                    (true, _) => (format!("other-{name}"), theirs(SIZES[file])),
                })
                .collect();
            for (name, bytes) in &files {
                std::fs::write(pack.join(name), bytes).unwrap();
            }
            // The pieces of all the files as one: a folder's files get no order of the test's.
            let one = base.join("one");
            std::fs::write(&one, files.iter().flat_map(|(_, bytes)| bytes.clone()).collect::<Vec<_>>()).unwrap();
            let options = CreateTorrentOptions { piece_length: Some(PIECE as u32), ..Default::default() };
            let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
            let one = librqbit::create_torrent(&one, options, &spawner).await.unwrap().as_bytes().unwrap().to_vec();
            let key = b"6:pieces";
            let at = one.windows(key.len()).position(|w| w == key).unwrap() + key.len();
            let digits = one[at..].iter().position(|b| *b == b':').unwrap();
            let len: usize = std::str::from_utf8(&one[at..at + digits]).unwrap().parse().unwrap();
            let pieces = &one[at..at + digits + 1 + len];
            let list: String = files.iter().map(|(name, b)| format!("d6:lengthi{}e4:pathl{}:{name}ee", b.len(), name.len())).collect();
            let head = format!("d5:filesl{list}e4:name4:pack12:piece lengthi{PIECE}e6:pieces");
            let info = [head.as_bytes(), pieces, b"e"].concat();
            let options = SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
                ..Default::default()
            };
            let session = librqbit::Session::new_with_opts(base.join("session"), options).await.unwrap();
            let torrent = [b"d4:info", &info[..], b"e"].concat();
            let options = AddTorrentOptions { overwrite: true, output_folder: Some(pack.to_str().unwrap().into()), ..Default::default() };
            let added = session.add_torrent(AddTorrent::from_bytes(torrent), Some(options)).await.unwrap().into_handle().unwrap();
            // A peer not up yet would be tried again only after librqbit's 10 s.
            by(|| async { added.stats().finished && added.live().is_some() }).await.expect("the peer has everything");
            infos.push((info, session.listen_addr().expect("it listens").port()));
            sessions.push(session);
        }
        Self { _dir: dir, _sessions: sessions, infos }
    }

    /// What the peer sends is added to `sent`. A .torrent whose tracker, asked each second, names a peer once `open`: one at a new address
    /// whenever none is connected, as one that went is tried again only after 10 s, then 60 s.
    async fn torrent(&self, which: usize, open: Arc<AtomicBool>, sent: Arc<AtomicU64>) -> AddSource {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};
        let (info, port) = self.infos[which].clone();
        let tracker = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
        tokio::spawn(async move {
            // Held by the connection under way.
            let connected = Arc::new(());
            loop {
                let Ok((mut stream, _)) = tracker.accept().await else { continue };
                let mut peers = Vec::new();
                if open.load(Ordering::SeqCst) && Arc::strong_count(&connected) == 1 {
                    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    peers.extend([127, 0, 0, 1]);
                    peers.extend(relay.local_addr().unwrap().port().to_be_bytes());
                    let (connected, sent) = (connected.clone(), sent.clone());
                    tokio::spawn(async move {
                        let accepted = tokio::time::timeout(Duration::from_secs(5), relay.accept()).await;
                        if let Ok(Ok((mut from, _))) = accepted
                            && let Ok(mut to) = TcpStream::connect(("127.0.0.1", port)).await
                        {
                            let ((mut asks, mut gets), (mut gives, mut asked)) = (from.split(), to.split());
                            let give = async {
                                let mut buf = [0; 16384];
                                while let Ok(n @ 1..) = gives.read(&mut buf).await {
                                    sent.fetch_add(n as u64, Ordering::SeqCst);
                                    if gets.write_all(&buf[..n]).await.is_err() {
                                        break;
                                    }
                                }
                            };
                            tokio::select! {
                                () = give => {}
                                _ = tokio::io::copy(&mut asks, &mut asked) => {}
                            }
                        }
                        drop(connected);
                    });
                }
                let body = [format!("d8:intervali1e5:peers{}:", peers.len()).as_bytes(), &peers, b"e"].concat();
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                tokio::spawn(async move {
                    let _ = stream.read(&mut [0; 2048]).await;
                    let _ = stream.write_all(&[head.as_bytes(), &body].concat()).await;
                });
            }
        });
        let head = format!("d8:announce{}:{announce}4:info", announce.len());
        AddSource::TorrentFile([head.as_bytes(), &info, b"e"].concat())
    }
}

/// Waits for `met`.
async fn by<F: Future<Output = bool>>(mut met: impl FnMut() -> F) -> Result<(), ()> {
    let deadline = std::time::Instant::now() + PATIENCE;
    while !met().await {
        if std::time::Instant::now() > deadline {
            return Err(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

/// What's at `path` itself.
fn inode(path: &Path) -> Option<u64> {
    std::fs::symlink_metadata(path).ok().map(|m| m.ino())
}

/// Every entry under `dir` whose inode is `ino`.
fn places(dir: &Path, ino: u64) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if inode(&path) == Some(ino) {
            found.push(path.clone());
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            found.extend(places(&path, ino));
        }
    }
    found
}

/// What `record` says of `file`.
fn missing_in(record: &super::super::store::Record, file: usize) -> Option<Missing> {
    let missing = record.missing.get(&file)?;
    Some(match missing.had {
        0 => Missing::NoneHad,
        had if had <= [16384, 24576, 20480][file] => Missing::PieceHad,
        _ => Missing::Had,
    })
}

struct World {
    temp: TempDir,
    gate: Arc<super::super::files::Gate>,
    trash: super::super::trash::Gate,
    engine: Option<Engine>,
    source: AddSource,
    other: AddSource,
    open: Arc<AtomicBool>,
    id: Option<u64>,
    /// Where the torrent's folder is, or last was: in `downloads` as it's there.
    folder: PathBuf,
    away: bool,
    theirs: Vec<Theirs>,
    /// The files taken away, put aside.
    absent: BTreeSet<usize>,
    renamed: bool,
    off: BTreeSet<usize>,
    /// Something was done after which the torrent may need a press, or other bytes.
    disturbed: Option<String>,
    watched: Arc<Mutex<Vec<Weak<Watched>>>>,
    sent: Arc<AtomicU64>,
    power: Power,
    strict: bool,
    made: usize,
    paused: bool,
    renames: usize,
    /// What may still be under way: since it last settled.
    recent: BTreeSet<Route>,
    missing: std::collections::BTreeMap<usize, Missing>,
    /// The engine had every piece, when last asked.
    had_all: bool,
}

impl World {
    fn dir(&self) -> &Path {
        self.temp.path()
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    async fn run(peers: &Peers, case: &Case, strict: bool) -> Power {
        let temp = temp();
        // As the engine names it: `/var` is a link.
        let dir = temp.path().canonicalize().unwrap();
        for folder in ["downloads", "elsewhere", "theirs", "aside"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let open = Arc::new(AtomicBool::new(case.peer));
        let sent: Arc<AtomicU64> = Arc::default();
        let watched = Arc::default();
        tokio::spawn(Watched::watch(Arc::downgrade(&watched)));
        let mut world = World {
            gate: Arc::default(),
            trash: Default::default(),
            engine: None,
            source: peers.torrent(0, open.clone(), sent.clone()).await,
            other: peers.torrent(1, open.clone(), sent.clone()).await,
            open,
            id: None,
            folder: dir.join("downloads/pack"),
            away: false,
            theirs: Vec::new(),
            absent: BTreeSet::new(),
            renamed: false,
            off: BTreeSet::new(),
            disturbed: None,
            watched,
            sent: sent.clone(),
            power: Power::default(),
            strict,
            made: 0,
            paused: case.paused,
            renames: 0,
            recent: BTreeSet::new(),
            missing: Default::default(),
            had_all: false,
            temp,
        };
        let _opens = (Opens(world.gate.clone()), OpensTrash(world.trash.clone()));
        world.start().await;
        let there = match case.start {
            Start::Nothing => 0,
            Start::Half => 1,
            Start::Whole => 3,
        };
        if there > 0 {
            std::fs::create_dir_all(&world.folder).unwrap();
        }
        for file in 0..there {
            std::fs::write(world.folder.join(NAMES[file]), own(file)).unwrap();
        }
        if let Some((file, kind)) = case.before {
            world.by(By::Put { file, kind }).await;
            // Nothing of it was had: a matching piece is enough.
            for t in world.theirs.iter_mut().filter(|t| matches!(t.kind, Kind::OnePiece | Kind::Cut)) {
                t.allow = Allow::Yes;
            }
        }
        world.did(Do::Add, None).await;
        if case.paused {
            world.paused = true;
            let _ = world.command(TorrentCommand::Pause).await;
        }
        world.settle().await;
        world.judge("the add");
        for step in &case.steps {
            world.step(*step).await;
            world.judge(&format!("{step:?}"));
        }
        world.end().await;
        world.engine().stop().await;
        world.power
    }

    async fn start(&mut self) {
        let dir = self.dir();
        let config = Config {
            data_dir: dir.join("data"),
            download_dir: dir.join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: SIZE_CHECK_EVERY,
            poll_every: Duration::from_millis(50),
            trash: super::super::trash::into(dir.join("trash"), Default::default(), self.trash.clone()),
            speed_limits: Default::default(),
            seeding: Default::default(),
        };
        let add_into: super::super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
            Box::pin(super::super::actor::add_into(session, id, add, destination, download_dir, guard))
        });
        let guard = super::super::files::Guard::default().with_gate(super::super::files::At::Read, self.gate.clone());
        self.engine = Some(Engine::start_with(config, add_into, guard, super::super::Looks::real()).await.unwrap());
    }

    async fn stop(&mut self, crash: bool) {
        self.know_missing().await;
        let engine = self.engine.take().expect("running");
        let stopping = async {
            match crash {
                true => engine.kill().await,
                false => engine.stop().await,
            }
        };
        tokio::time::timeout(PATIENCE, stopping).await.expect("the service stops");
        // Shown to the person in that run only.
        self.theirs.iter_mut().for_each(|t| t.seen = false);
    }

    async fn listed(&self) -> Option<TorrentInfo> {
        let id = self.id?;
        self.engine().listing().await.into_iter().find(|t| t.id == id)
    }

    async fn status(&self) -> Option<Status> {
        self.listed().await.map(|t| t.status)
    }

    async fn command(&self, command: TorrentCommand) -> Result<(), Failure> {
        let Some(id) = self.id else { return Ok(()) };
        let what = format!("{command:?}");
        tokio::time::timeout(PATIENCE, self.engine().command(id, command)).await.unwrap_or_else(|_| panic!("{what} is never answered"))
    }

    /// Waits until nothing seems to happen: the status the same over three whole size checks (and
    /// the polls between them). Where that's too short, a later look still sees what was done.
    async fn settle(&mut self) {
        let checks = async |engine: &Engine| engine.inspect(|a| a.size_checks).await;
        let mut same = (None, checks(self.engine()).await);
        let deadline = std::time::Instant::now() + PATIENCE;
        let stable = loop {
            let now = self.listed().await.map(|t| (t.status, t.content));
            let busy = now.as_ref().is_some_and(|(s, _)| {
                let busy = matches!(s, Status::Checking | Status::FetchingInfo | Status::Moving { .. } | Status::WaitingToMove);
                // Its peer is on its way.
                busy || (*s == Status::Downloading && self.open.load(Ordering::SeqCst))
            });
            // Counted from the first check that starts after it's seen: one under way may have
            // begun before.
            let checked = checks(self.engine()).await;
            if Some(&now) != same.0.as_ref() || busy {
                same = (Some(now), checked + 1);
            }
            if checked >= same.1 + 3 {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        if stable {
            self.settled(same.0.flatten().map(|(status, _)| status));
        }
        if self.status().await == Some(Status::MissingFiles) {
            let here: Vec<PathBuf> = NAMES.iter().map(|n| self.folder.join(n)).collect();
            self.theirs.iter_mut().filter(|t| here.iter().any(|p| inode(p) == Some(t.ino))).for_each(|t| t.seen = true);
        }
    }

    /// Nothing is under way. Where nothing more comes by itself, what was done before is over: damage
    /// from here on needs a new route.
    fn settled(&mut self, status: Option<Status>) {
        self.judge("it settled");
        self.recent.clear();
        if !matches!(status, None | Some(Status::Seeding | Status::Done { .. } | Status::Paused | Status::MissingFiles)) {
            return;
        }
        let home = self.dir().canonicalize().unwrap();
        for t in &mut self.theirs {
            t.routes = BTreeSet::from([Route::Running]);
            t.off = self.off.contains(&t.file);
            t.gone_meanwhile = false;
            if t.relocated
                && let [place] = &t.places(&home)[..]
            {
                t.path =
                    place.strip_prefix(home.join("away")).map(|rest| home.join("downloads").join(rest)).unwrap_or_else(|_| place.clone());
                t.relocated = false;
            }
        }
    }

    /// `path`, where it is while the drive is away.
    fn now(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(self.dir().canonicalize().unwrap().join("downloads")) {
            Ok(rest) if self.away => self.dir().canonicalize().unwrap().join("away").join(rest),
            _ => path.to_owned(),
        }
    }

    fn at(&self, file: usize) -> PathBuf {
        self.now(&self.folder).join(NAMES[file])
    }

    fn whole(&self, file: usize) -> bool {
        std::fs::symlink_metadata(self.at(file)).is_ok_and(|m| m.is_file()) && std::fs::read(self.at(file)).is_ok_and(|b| b == own(file))
    }

    fn disturb(&mut self, why: impl Debug) {
        if self.disturbed.is_none() {
            self.disturbed = Some(format!("{why:?}"));
        }
    }

    /// What the person has at `path` is theirs to take from there.
    fn unregister(&mut self, path: &Path) {
        let ino = inode(path);
        self.theirs.retain(|t| Some(t.ino) != ino);
    }

    fn register(&mut self, file: usize, kind: Kind, path: &Path, range: Option<std::ops::Range<usize>>, missing: Missing) {
        let handle = std::fs::File::open(path).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let watched = Arc::new(Watched { handle, bytes, range, damage: Mutex::default(), looked: Mutex::default() });
        self.watched.lock().unwrap().push(Arc::downgrade(&watched));
        let home = self.dir().canonicalize().unwrap();
        let path = path.strip_prefix(home.join("away")).map(|rest| home.join("downloads").join(rest)).unwrap_or_else(|_| path.to_owned());
        self.theirs.push(Theirs {
            ino: inode(&self.now(&path)).unwrap(),
            kind,
            file,
            path,
            watched,
            other: None,
            sent: self.sent.load(Ordering::SeqCst),
            relocated: self.renamed,
            whole: false,
            missing,
            off: self.off.contains(&file),
            gone_meanwhile: false,
            seen: false,
            routes: self.recent.iter().copied().chain([Route::Running]).collect(),
            allow: Allow::No,
        });
    }

    /// What the engine knows missing, for a file put there while it isn't running too.
    async fn know_missing(&mut self) {
        let Some(id) = self.id else { return };
        let known = |record: &super::super::store::Record| (0..3).filter_map(|f| Some((f, missing_in(record, f)?))).collect();
        let looked = self.engine().inspect(move |a| {
            let t = a.torrents.get(&id)?;
            Some((known(&t.record), super::super::actor::progress(t).iter().map(|b| *b as usize).eq(SIZES)))
        });
        (self.missing, self.had_all) = looked.await.unwrap_or_default();
    }

    async fn put(&mut self, file: usize, kind: Kind, path: PathBuf) {
        let missing = self.missing.get(&file).copied().unwrap_or(Missing::No);
        let had_all = self.whole(file);
        // What the engine has, not what's on disk: one it hasn't checked since is had by nobody's word.
        let complete = self.had_all && (0..3).all(|f| self.whole(f));
        self.made += 1;
        let dir = self.dir().canonicalize().unwrap();
        let mine = dir.join("theirs").join(self.made.to_string());
        let size = SIZES[file];
        let bytes = match kind {
            Kind::Same | Kind::HardLink | Kind::Link => theirs(size),
            Kind::Longer => theirs(size + 777),
            Kind::OwnAndMore => [own(file), theirs(777)].concat(),
            Kind::Shorter => theirs(size / 2),
            Kind::Empty => Vec::new(),
            // Pieces 0, 2 and 4 are whole in the files.
            Kind::OnePiece => {
                let mut bytes = theirs(size);
                let from = [0, 8192, 4096][file];
                bytes[from..from + PIECE].copy_from_slice(&own(file)[from..from + PIECE]);
                bytes
            }
            Kind::Cut => own(file)[..[PIECE, 8192 + PIECE, 4096 + PIECE][file] + 10].to_vec(),
            Kind::Equal | Kind::EqualHardLink => own(file),
            Kind::Edit | Kind::Another => {
                // Only a whole file's bytes are all settled: later ones would still be downloaded.
                if !had_all || self.theirs.iter().any(|t| Some(t.ino) == inode(&path)) {
                    return;
                }
                let f = std::fs::File::options().write(true).open(&path).unwrap();
                f.write_all_at(&theirs(500), 100).unwrap();
                self.register(file, kind, &path, Some(100..600), Missing::No);
                return self.disturb(kind);
            }
        };
        if std::fs::create_dir_all(path.parent().unwrap()).is_err() {
            return;
        }
        self.unregister(&path);
        std::fs::write(&mine, bytes).unwrap();
        let staged = dir.join("theirs").join(format!("{}-staged", self.made));
        match kind {
            Kind::Link => std::os::unix::fs::symlink(&mine, &staged).unwrap(),
            Kind::HardLink | Kind::EqualHardLink => std::fs::hard_link(&mine, &staged).unwrap(),
            _ => std::fs::rename(&mine, &staged).unwrap(),
        }
        std::fs::rename(&staged, &path).unwrap();
        self.register(file, kind, &path, None, missing);
        let put = self.theirs.last_mut().unwrap();
        put.whole = complete;
        put.other = inode(&mine).map(|ino| (mine, ino));
        // A hard link, even to its own bytes, is Missing files until a check finds it whole (#374).
        if !kind.equal() || kind == Kind::EqualHardLink {
            self.disturb(kind);
        }
        if matches!(kind, Kind::OnePiece | Kind::Cut) && !complete && missing != Missing::Had {
            // How much of it was had isn't the test's to know.
            self.theirs.last_mut().unwrap().allow = Allow::Unsaid;
        }
    }

    async fn by(&mut self, by: By) {
        if self.engine.is_some() {
            self.know_missing().await;
        }
        // Of the missing ones' paths only: nothing else has one looked at.
        let back = match by {
            By::Put { file, .. } | By::PutBack { file } | By::Touch { file } => self.missing.contains_key(&file),
            By::RenameFolderBack => !self.missing.is_empty(),
            _ => false,
        };
        if back {
            self.route(Route::PutBack);
        }
        match by {
            By::Put { file, kind } => {
                let path = self.at(file);
                if self.renamed && !path.parent().unwrap().exists() {
                    return;
                }
                self.put(file, kind, path).await;
                // What went before it isn't missing any more, but may be had in part.
                if self.absent.remove(&file) {
                    self.disturb("put where one was taken away");
                }
            }
            By::PutAtTarget { file, kind } => {
                if kind == Kind::Edit {
                    return;
                }
                let path = self.now(&self.target().join("pack").join(NAMES[file]));
                self.put(file, kind, path).await;
            }
            By::TakeAway { file } => {
                let (path, aside) = (self.at(file), self.dir().join("aside").join(NAMES[file]));
                let ours = !self.theirs.iter().any(|t| Some(t.ino) == inode(&path));
                if !ours || aside.exists() {
                    return;
                }
                if !self.whole(file) {
                    self.disturb("a file taken away in part");
                }
                if std::fs::rename(path, aside).is_ok() {
                    self.absent.insert(file);
                }
            }
            By::PutBack { file } => {
                let (path, aside) = (self.at(file), self.dir().join("aside").join(NAMES[file]));
                if !self.theirs.iter().any(|t| Some(t.ino) == inode(&path)) && std::fs::rename(aside, path).is_ok() {
                    self.absent.remove(&file);
                }
            }
            By::Touch { file } => {
                let path = self.at(file);
                if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
                    && let Ok(f) = std::fs::File::options().write(true).open(&path)
                {
                    let mode = f.metadata().unwrap().permissions().mode();
                    f.set_permissions(std::fs::Permissions::from_mode(mode)).unwrap();
                    f.set_modified(std::time::SystemTime::now()).unwrap();
                }
            }
            By::RenameFolder => {
                let (from, to) = (self.now(&self.folder), self.dir().join("aside/folder"));
                if !to.exists() && std::fs::rename(from, to).is_ok() {
                    self.renamed = true;
                    self.theirs.iter_mut().for_each(|t| t.relocated = true);
                }
            }
            By::RenameFolderBack => {
                let (to, from) = (self.now(&self.folder), self.dir().join("aside/folder"));
                if self.renamed && !to.exists() && std::fs::rename(from, to).is_ok() {
                    self.renamed = false;
                    self.theirs.iter_mut().for_each(|t| t.relocated = true);
                }
            }
            By::DriveGone => {
                if !self.away {
                    std::fs::rename(self.dir().join("downloads"), self.dir().join("away")).unwrap();
                    self.away = true;
                }
            }
            By::DriveBack => {
                let (here, away) = (self.dir().join("downloads"), self.dir().join("away"));
                // Nothing makes a destination again.
                assert!(!self.away || !here.exists(), "the destination was made again while its drive was away");
                if self.away {
                    self.route(Route::DriveBack);
                    std::fs::rename(away, here).unwrap();
                    self.away = false;
                }
            }
        }
    }

    fn route(&mut self, route: Route) {
        self.recent.insert(route);
        self.theirs.iter_mut().for_each(|t| t.routes.extend([route]));
    }

    /// Where a Move would take it.
    fn target(&self) -> PathBuf {
        let home = self.dir().canonicalize().unwrap();
        match self.folder.starts_with(home.join("downloads")) {
            true => home.join("elsewhere"),
            false => home.join("downloads"),
        }
    }

    /// `what` pressed, `meanwhile` done while it's under way.
    async fn did(&mut self, what: Do, meanwhile: Option<By>) {
        let status = self.status().await;
        self.judge("what came before");
        let before: Vec<_> = self.theirs.iter().map(|t| (t.ino, t.allow, t.routes.clone())).collect();
        let recent = self.recent.clone();
        if (!self.absent.is_empty() || self.renamed) && !matches!(what, Do::Pause) {
            self.disturb(format!("{what:?} with files taken away"));
        }
        let (route, moves) = match what {
            Do::Check => (Some(Route::Check), false),
            Do::Resume => (Some(Route::Resume), false),
            Do::CancelCheck | Do::Pause => (None, false),
            Do::Off { .. } | Do::Normal { .. } => (Some(Route::Priority), false),
            Do::TrashFile { .. } => (Some(Route::Priority), true),
            Do::Move { .. } => (Some(Route::Move), true),
            Do::Rename => (Some(Route::Rename), true),
            Do::Remove => (Some(Route::Remove), false),
            Do::Delete => (Some(Route::Remove), true),
            Do::Undo => (Some(Route::Undo), true),
            Do::Add => (Some(Route::Add), false),
        };
        if let Some(route) = route {
            // Unless it's the choice of them, below.
            self.route(route);
        }
        if moves {
            self.theirs.iter_mut().for_each(|t| t.relocated = true);
        }
        if moves && meanwhile.is_some() {
            self.disturb("a file changed as it's moved");
        }
        // Held by what it's done through, to be caught under way.
        let held = meanwhile.is_some();
        match what {
            Do::Delete | Do::TrashFile { .. } | Do::Undo => super::super::trash::hold(&self.trash, held),
            Do::Move { .. } | Do::Rename => {
                super::super::trash::hold(&self.trash, held);
                let gate = held.then(|| self.trash.clone());
                let hold = super::super::actor::moving::Hold { gate: gate.clone(), finish_gate: gate, ..Default::default() };
                self.engine().inspect(move |a| a.move_hold = hold).await;
            }
            _ => self.gate.hold(held),
        }
        let id = self.id.unwrap_or(u64::MAX);
        let command = match what {
            Do::Check => TorrentCommand::Check,
            Do::CancelCheck => {
                self.disturb("a check cancelled");
                TorrentCommand::CancelCheck
            }
            Do::Resume => {
                let shown = status == Some(Status::MissingFiles);
                for t in self.theirs.iter_mut().filter(|t| shown && t.seen && t.allow == Allow::No && !t.kind.never()) {
                    // The file that isn't the one it's missing may have matched when it was last looked
                    // at: whether the press takes it too isn't said (question 1).
                    t.allow = if t.missing != Missing::No { Allow::Yes } else { Allow::Unsaid };
                }
                TorrentCommand::Resume
            }
            Do::Pause => TorrentCommand::Pause,
            Do::Off { file } | Do::TrashFile { file } => {
                self.off.insert(file);
                self.theirs.iter_mut().filter(|t| t.file == file).for_each(|t| t.off = true);
                match what {
                    Do::Off { .. } => TorrentCommand::SetPriority { files: vec![file], priority: Priority::Off },
                    _ => {
                        self.disturb(what);
                        TorrentCommand::TrashFiles { files: vec![file] }
                    }
                }
            }
            Do::Normal { file } => {
                self.off.remove(&file);
                TorrentCommand::SetPriority { files: vec![file], priority: Priority::Normal }
            }
            Do::Move { replace } => {
                let to = self.target();
                let choice = if replace { proto::Clash::Replace } else { proto::Clash::UseExisting };
                let target: Vec<PathBuf> = NAMES.iter().map(|n| to.join("pack").join(n)).collect();
                if self.id.is_some() && target.iter().any(|p| std::fs::symlink_metadata(p).is_ok()) {
                    self.disturb("a Move onto files");
                }
                for t in self.theirs.iter_mut().filter(|t| !replace && target.iter().any(|p| inode(p) == Some(t.ino))) {
                    t.allow = match t.kind {
                        // Whether the choice covers a file with another name isn't said (question 2).
                        Kind::HardLink | Kind::EqualHardLink => Allow::Unsaid,
                        kind if kind.never() => continue,
                        _ => Allow::Yes,
                    };
                }
                let to = AbsolutePath::new(&to).unwrap();
                self.engine().move_answering(id, to, choice).await
            }
            Do::Rename => {
                self.renames += 1;
                TorrentCommand::Rename { name: proto::FileName::new(&format!("pack{}", self.renames)).unwrap() }
            }
            Do::Remove => TorrentCommand::Remove,
            Do::Delete => TorrentCommand::Delete,
            Do::Undo | Do::Add => TorrentCommand::Pause,
        };
        if what == Do::Add && status.is_none() {
            // Nothing is had of a torrent not in the list: a matching piece is enough.
            let here: Vec<PathBuf> = NAMES.iter().map(|n| self.now(&self.folder).join(n)).collect();
            let enough = |t: &&mut Theirs| matches!(t.kind, OnePiece | Cut | Edit) && here.iter().any(|p| inode(p) == Some(t.ino));
            self.theirs.iter_mut().filter(enough).for_each(|t| t.allow = Allow::Yes);
        }
        let engine = self.engine();
        let mut answer: std::pin::Pin<Box<dyn Future<Output = Result<Option<u64>, Failure>> + Send>> = match what {
            Do::Undo => {
                let undone = engine.undo_later();
                Box::pin(async move { undone.await.map(|_| None) })
            }
            Do::Add => {
                let source = super::super::Fetched::local(self.source.clone());
                // Where it last was, if it can be had.
                let to = self.folder.parent().unwrap().to_owned();
                if std::fs::symlink_metadata(&to).is_err() {
                    return self.release();
                }
                let added = engine.add_later(source, self.paused, parsed(&to));
                Box::pin(async move { added.await.map(|a| Some(a.id)) })
            }
            _ => {
                let answer = engine.command_later(id, command);
                Box::pin(async move { answer.await.map(|()| None) })
            }
        };
        let (gate, trash) = (self.gate.clone(), self.trash.clone());
        let task = tokio::spawn(async move { tokio::time::timeout(PATIENCE, &mut answer).await });
        if let Some(meanwhile) = meanwhile {
            // Under way, or over without meeting what holds it: some do nothing here.
            let held = std::time::Instant::now();
            while !gate.waiting() && !super::super::trash::waiting(&trash) && held.elapsed() < SIZE_CHECK_EVERY * 5 {
                if task.is_finished() && !matches!(what, Do::Check | Do::Resume | Do::Add | Do::Normal { .. } | Do::Undo) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            self.by(meanwhile).await;
            if matches!(meanwhile, By::RenameFolder | By::DriveGone) {
                self.theirs.iter_mut().for_each(|t| t.gone_meanwhile = true);
            }
        }
        self.release();
        let Ok(answer) = task.await.unwrap() else { panic!("{what:?} is never answered") };
        match (what, answer) {
            (Do::Add, Ok(Some(id))) => self.id = Some(id),
            (Do::Resume, Ok(_)) => self.paused = false,
            (Do::Pause, Ok(_)) => self.paused = true,
            // Moved, without the files that weren't there: the choice it was sent with was taken.
            (Do::Move { .. }, Err(Failure::Refused(proto::Refusal::Torrent { why: proto::TorrentRefusal::MovedWithout { .. }, .. }))) => {}
            // Refused: nothing is asked of it, so nothing allowed or excused by it.
            (_, Err(_)) if meanwhile.is_none() => {
                self.recent = recent;
                for (ino, allow, routes) in before {
                    self.theirs.iter_mut().filter(|t| t.ino == ino).for_each(|t| (t.allow, t.routes) = (allow, routes.clone()));
                }
            }
            _ => {}
        }
        let _ = by(|| async { self.status().await != Some(Status::FetchingInfo) }).await;
        self.refresh().await;
        if moves && meanwhile.is_some() {
            self.theirs.iter_mut().for_each(|t| t.relocated = true);
        }
    }

    fn release(&self) {
        self.gate.hold(false);
        super::super::trash::hold(&self.trash, false);
    }

    /// Where the torrent is now, and what it gets.
    async fn refresh(&mut self) {
        let Some(listed) = self.listed().await else { return };
        if matches!(listed.status, Status::Moving { .. } | Status::WaitingToMove) {
            return;
        }
        if let Some(location) = listed.location {
            self.folder = location.to_path_buf();
        }
        if let Some(id) = self.id
            && let Ok(details) = self.engine().details(id).await
        {
            let off: BTreeSet<usize> = details.files.iter().filter(|f| f.priority == Priority::Off).map(|f| f.index).collect();
            self.theirs.iter_mut().filter(|t| off.contains(&t.file)).for_each(|t| t.off = true);
            self.off = off;
        }
    }

    async fn step(&mut self, step: Step) {
        match step {
            Step::By(by) => self.by(by).await,
            Step::Do(what) => self.did(what, None).await,
            Step::During(what, by) => self.did(what, Some(by)).await,
            Step::AddOther => {
                // Its files are the first torrent's: only while it's whole, and it alone has them.
                if self.away
                    || !self.whole(1)
                    || self.listed().await.is_none()
                    || self.theirs.iter().any(|t| Some(t.ino) == inode(&self.at(1)))
                {
                    return;
                }
                let path = self.at(1);
                self.register(1, Kind::Another, &path, None, Missing::No);
                self.route(Route::AddOther);
                self.disturb("another torrent's add");
                let Some(to) = self.folder.parent().map(Path::to_owned) else { return };
                let added = self.engine().add(super::super::Fetched::local(self.other.clone()), false, parsed(&to)).await;
                let Ok(added) = added else { return };
                let done = |s: &Status| !matches!(s, Status::FetchingInfo | Status::Checking);
                let _ = by(|| async { done(&find(self.engine(), added.id).await.status) }).await;
                self.settle().await;
                let _ = self.engine().command(added.id, TorrentCommand::Remove).await;
                // The first Undo would bring that one back.
                let _ = by(|| async { self.engine().listing().await.iter().all(|t| t.id != added.id) }).await;
            }
            Step::Restart | Step::Crash => {
                let crash = matches!(step, Step::Crash);
                self.stop(crash).await;
                self.judge("the stop");
                self.route(if crash { Route::Crash } else { Route::Restart });
                self.start().await;
            }
            Step::Between { by, crash } => {
                self.stop(crash).await;
                self.judge("the stop");
                self.by(by).await;
                self.route(if crash { Route::Crash } else { Route::Restart });
                self.start().await;
            }
            Step::PeerOpens => self.open.store(true, Ordering::SeqCst),
            Step::Settle => self.settle().await,
        }
        self.refresh().await;
    }

    /// Whether each of the person's files is as it was put.
    fn judge(&mut self, after: &str) {
        let home = self.dir().canonicalize().unwrap();
        let mut broken = Vec::new();
        for (i, t) in self.theirs.iter().enumerate().filter(|(_, t)| t.allow == Allow::No) {
            self.power.theirs = true;
            if !self.power.ending {
                let excusable = KNOWN_OPEN.iter().any(|open| open.holds(&t.class()));
                self.power.excusable |= excusable;
                self.power.looks.0 += 1;
                self.power.looks.1 += usize::from(excusable);
            }
            self.power.sent |= !self.power.ending && self.sent.load(Ordering::SeqCst) > t.sent + PIECE as u64;
            let places = t.places(&home);
            let expected = self.now(&t.path);
            let what = if let Some(damage) = t.watched.damage() {
                damage
            } else if places.is_empty() {
                "removed".into()
            } else if !t.relocated && !places.contains(&expected) {
                format!("moved from {expected:?} to {places:?}")
            } else if let Some((path, ino)) = &t.other
                && inode(path) != Some(*ino)
            {
                format!("other name, or what it leads to, removed or replaced at {path:?}")
            } else {
                continue;
            };
            broken.push((i, what));
        }
        for (i, what) in broken {
            let t = &mut self.theirs[i];
            t.allow = Allow::Unsaid;
            let class = t.class();
            let open: Vec<&str> = KNOWN_OPEN.iter().filter(|open| open.holds(&class)).map(|open| open.why).collect();
            match open.first() {
                Some(why) if !self.strict => drop(self.power.known.insert(why)),
                _ => panic!("after {after}: the person's {} {what}; {class:?}; open: {open:?}", NAMES[t.file]),
            }
        }
    }

    async fn end(&mut self) {
        self.open.store(true, Ordering::SeqCst);
        self.by(By::DriveBack).await;
        self.power.ending = true;
        self.settle().await;
        self.refresh().await;
        self.judge("the end");
        let safe = self.disturbed.is_none() && self.absent.is_empty() && !self.renamed;
        if safe && let Some(listed) = self.listed().await {
            // No press.
            let get: Vec<usize> = (0..3).filter(|f| !self.off.contains(f)).collect();
            let done = by(|| async {
                let status = self.status().await;
                match self.paused {
                    // What it has stays; it isn't started.
                    true => matches!(status, Some(Status::Paused | Status::Done { .. })),
                    false => status == Some(Status::Seeding) && get.iter().all(|f| self.whole(*f)),
                }
            });
            if done.await.is_err() {
                let now = self.status().await;
                panic!(
                    "its own untouched files aren't complete with no press (stuck): {:?}, then {now:?}, {:?}",
                    listed.status,
                    self.state()
                );
            }
        }
        // The person takes what's theirs, and all is asked for: it's complete, with a few presses.
        let home = self.dir().canonicalize().unwrap();
        for t in &mut self.theirs {
            t.relocated = true;
            for place in places(&home, t.ino) {
                if place.starts_with(self.folder.parent().unwrap()) {
                    self.made += 1;
                    let _ = std::fs::rename(&place, self.temp.path().join("theirs").join(format!("{}-taken", self.made)));
                }
            }
        }
        for other in self.engine().listing().await.into_iter().filter(|t| Some(t.id) != self.id) {
            let _ = self.engine().command(other.id, TorrentCommand::Remove).await;
        }
        // One partly in the Trash: only a Delete (again) or a Remove applies.
        if matches!(self.status().await, Some(Status::Error { failure: proto::TorrentFailure::NotTrashed { .. } })) {
            let _ = self.command(TorrentCommand::Remove).await;
        }
        if self.listed().await.is_none() {
            self.id = None;
            self.paused = false;
            self.did(Do::Add, None).await;
        }
        self.settle().await;
        self.refresh().await;
        let files: Vec<usize> = self.off.iter().copied().collect();
        if !files.is_empty() {
            let _ = self.command(TorrentCommand::SetPriority { files, priority: Priority::Normal }).await;
        }
        let mut presses = 0;
        let deadline = std::time::Instant::now() + PATIENCE;
        let mut seeding = None;
        loop {
            self.settle().await;
            self.refresh().await;
            self.judge("what's theirs is taken");
            let status = self.status().await;
            if status == Some(Status::Seeding) && (0..3).all(|f| self.whole(f)) {
                break;
            }
            // Nothing more comes of one that has everything, to it.
            seeding = (status == Some(Status::Seeding)).then(|| seeding.unwrap_or_else(std::time::Instant::now));
            let stuck = seeding.is_some_and(|since| since.elapsed() > PATIENCE / 4);
            let stopped = !matches!(status, Some(Status::Downloading | Status::Checking | Status::Seeding));
            assert!(
                !stuck && std::time::Instant::now() < deadline && presses < 5,
                "stuck, once the person took what's theirs: {status:?} after {presses} presses, {:?}, disturbed by {:?}",
                self.state(),
                self.disturbed
            );
            if stopped {
                presses += 1;
                let _ = self.command(TorrentCommand::Resume).await;
            }
        }
        self.judge("it's complete again");
    }

    /// Each file's length, and whether it's whole.
    fn state(&self) -> Vec<(Option<u64>, bool)> {
        (0..3).map(|f| (std::fs::symlink_metadata(self.at(f)).ok().map(|m| m.len()), self.whole(f))).collect()
    }
}

fn file_at() -> impl Strategy<Value = usize> {
    0..3usize
}

fn kind() -> impl Strategy<Value = Kind> {
    use Kind::*;
    // The ones nothing excuses come more often.
    prop::sample::select(vec![
        Longer,
        Link,
        Same,
        Longer,
        OwnAndMore,
        Shorter,
        Empty,
        OnePiece,
        Cut,
        Equal,
        Link,
        HardLink,
        EqualHardLink,
        Edit,
    ])
}

fn person() -> impl Strategy<Value = By> {
    prop_oneof![
        8 => (file_at(), kind()).prop_map(|(file, kind)| By::Put { file, kind }),
        1 => (file_at(), kind()).prop_map(|(file, kind)| By::PutAtTarget { file, kind }),
        2 => file_at().prop_map(|file| By::TakeAway { file }),
        2 => file_at().prop_map(|file| By::PutBack { file }),
        1 => file_at().prop_map(|file| By::Touch { file }),
        1 => Just(By::RenameFolder),
        1 => Just(By::RenameFolderBack),
        2 => Just(By::DriveGone),
        3 => Just(By::DriveBack),
    ]
}

fn action() -> impl Strategy<Value = Do> {
    prop_oneof![
        3 => Just(Do::Check),
        1 => Just(Do::CancelCheck),
        3 => Just(Do::Resume),
        2 => Just(Do::Pause),
        2 => file_at().prop_map(|file| Do::Off { file }),
        2 => file_at().prop_map(|file| Do::Normal { file }),
        2 => any::<bool>().prop_map(|replace| Do::Move { replace }),
        1 => Just(Do::Rename),
        2 => Just(Do::Remove),
        1 => Just(Do::Delete),
        1 => file_at().prop_map(|file| Do::TrashFile { file }),
        3 => Just(Do::Undo),
        3 => Just(Do::Add),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        8 => person().prop_map(Step::By),
        8 => action().prop_map(Step::Do),
        3 => (action(), person()).prop_map(|(what, by)| Step::During(what, by)),
        1 => Just(Step::AddOther),
        1 => Just(Step::Restart),
        1 => Just(Step::Crash),
        2 => (person(), any::<bool>()).prop_map(|(by, crash)| Step::Between { by, crash }),
        2 => Just(Step::PeerOpens),
        4 => Just(Step::Settle),
    ]
}

/// What may have the engine take what's there for the torrent's.
fn trigger() -> impl Strategy<Value = Vec<Step>> {
    prop_oneof![
        action().prop_map(|what| vec![Step::Do(what)]),
        Just(vec![Step::Restart]),
        Just(vec![Step::Crash]),
        Just(vec![Step::By(By::DriveGone), Step::Settle, Step::By(By::DriveBack)]),
        Just(vec![Step::Do(Do::Remove), Step::Do(Do::Undo)]),
        Just(vec![]),
    ]
}

/// A file put where the torrent's is, or was known missing, then what may have it taken.
fn taken() -> impl Strategy<Value = Vec<Step>> {
    let closed = prop::option::of(any::<bool>());
    (file_at(), kind(), any::<bool>(), closed, trigger()).prop_map(|(file, kind, missing, closed, trigger)| {
        let mut steps = if missing { vec![Step::By(By::TakeAway { file }), Step::Settle] } else { vec![] };
        let by = By::Put { file, kind };
        steps.push(closed.map_or(Step::By(by), |crash| Step::Between { by, crash }));
        steps.extend(trigger);
        steps.extend([Step::Settle, Step::PeerOpens, Step::Settle]);
        steps
    })
}

/// A file put just where the engine is to put one: at a Move's target, or where an undo brings one back.
fn in_the_way() -> impl Strategy<Value = Vec<Step>> {
    prop_oneof![
        (any::<bool>(), file_at(), kind())
            .prop_map(|(replace, file, kind)| vec![Step::During(Do::Move { replace }, By::PutAtTarget { file, kind }), Step::Settle]),
        (any::<bool>(), file_at(), kind()).prop_map(|(all, file, kind)| {
            let taken = if all { Do::Delete } else { Do::TrashFile { file } };
            vec![Step::Do(taken), Step::During(Do::Undo, By::Put { file, kind }), Step::Settle]
        }),
    ]
}

/// One partly had and known missing, another file there as the service starts, then a peer sending.
fn partly() -> impl Strategy<Value = Case> {
    (0..2usize, kind(), any::<bool>(), prop::collection::vec(step(), 0..2)).prop_map(|(file, kind, crash, then)| {
        let mut steps =
            vec![Step::By(By::TakeAway { file }), Step::Settle, Step::Between { by: By::Put { file, kind }, crash }, Step::Settle];
        steps.extend(then);
        steps.extend([Step::PeerOpens, Step::Settle]);
        Case { start: Start::Half, peer: false, paused: false, before: None, steps }
    })
}

fn case() -> impl Strategy<Value = Case> {
    prop_oneof![4 => mixed(), 1 => partly()]
}

fn mixed() -> impl Strategy<Value = Case> {
    let start = prop_oneof![2 => Just(Start::Nothing), 2 => Just(Start::Half), 3 => Just(Start::Whole)];
    let before = prop::option::weighted(0.15, (file_at(), kind()));
    let steps = prop_oneof![3 => step().prop_map(|s| vec![s]), 2 => taken(), 1 => in_the_way()];
    (start, prop::bool::weighted(0.4), prop::bool::weighted(0.15), before, prop::collection::vec(steps, 1..5))
        .prop_map(|(start, peer, paused, before, steps)| Case { start, peer, paused, before, steps: steps.concat() })
}

/// Why `case` fails, if it does.
async fn fails(peers: &Arc<Peers>, case: &Case, strict: bool) -> Result<Power, String> {
    let (peers, case) = (peers.clone(), case.clone());
    let failed = tokio::spawn(async move { World::run(&peers, &case, strict).await }).await;
    failed.map_err(|e| match e.into_panic().downcast::<String>() {
        Ok(text) => *text,
        Err(other) => other.downcast_ref::<&str>().map_or_else(|| "it panicked".into(), |s| (*s).into()),
    })
}

/// How many tests the generated cases are split over, each in a process of its own.
const SHARDS: usize = 4;

/// Generated cases, some at a time: each takes seconds, most of them waiting. A failing one is shrunk.
/// `PROPTEST_CASES` (32 by default) are split over [`SHARDS`] tests, which nextest runs in parallel.
async fn generated_shard() {
    let total: usize = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32);
    let cases = total.div_ceil(SHARDS);
    let peers = Arc::new(Peers::start().await);
    let width = Arc::new(tokio::sync::Semaphore::new(WIDTH));
    let mut running = tokio::task::JoinSet::new();
    let mut trees = std::collections::HashMap::new();
    {
        let mut runner = TestRunner::default();
        for n in 0..cases {
            let tree = case().new_tree(&mut runner).unwrap();
            let (peers, width, case) = (peers.clone(), width.clone(), tree.current());
            running.spawn(async move {
                let _turn = width.acquire().await.unwrap();
                (n, fails(&peers, &case, false).await)
            });
            trees.insert(n, tree);
        }
    }
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let (mut failed, mut theirs, mut excusable, mut sent, mut looks) = (None, 0, 0, 0, (0, 0));
    while let Some(done) = running.join_next().await {
        match done.unwrap() {
            (_, Ok(power)) => {
                power.known.into_iter().for_each(|k| *seen.entry(k).or_default() += 1);
                theirs += usize::from(power.theirs);
                excusable += usize::from(power.excusable);
                sent += usize::from(power.sent);
                looks = (looks.0 + power.looks.0, looks.1 + power.looks.1);
            }
            (n, Err(why)) => {
                failed = Some((n, why));
                break;
            }
        }
    }
    println!(
        "{cases} cases, {theirs} with a person's file: {sent} with a peer sending then, {excusable} excusable at some look, {} of {} looks",
        looks.1, looks.0
    );
    println!("known open, seen: {seen:#?}");
    let Some((n, mut why)) = failed else {
        assert!(total < 256 || sent * 16 >= theirs, "the peer hardly sends: nothing is tested");
        return;
    };
    // The rest is over before it's shrunk: they'd load the machine.
    running.shutdown().await;
    let tree = trees.get_mut(&n).unwrap();
    let mut least = tree.current();
    println!("failing, to be shrunk: {least:?}\n{why}");
    let until = std::time::Instant::now() + Duration::from_secs(300);
    let mut more = tree.simplify();
    while more && std::time::Instant::now() < until {
        let case = tree.current();
        more = match fails(&peers, &case, false).await {
            Err(now) => {
                (least, why) = (case, now);
                tree.simplify()
            }
            Ok(_) => tree.complicate(),
        };
    }
    panic!("{why}\nminimal: {least:?}");
}

/// The generated cases, `SHARDS` tests of an equal share.
mod a_persons_bytes_stay_and_a_torrents_own_files_work {
    macro_rules! shards {
        ($($name:ident),*) => {$(
            #[tokio::test(flavor = "multi_thread")]
            async fn $name() {
                super::generated_shard().await;
            }
        )*};
    }
    shards!(shard_1, shard_2, shard_3, shard_4);
}

/// Each class of [`KNOWN_OPEN`] on its own: fails while that's open.
macro_rules! open {
    ($($name:ident: $why:literal, $start:ident, $peer:literal, [$($step:expr),*];)*) => { $(
        #[tokio::test(flavor = "multi_thread")]
        #[ignore = $why]
        async fn $name() {
            let steps = vec![$($step),*];
            World::run(&Peers::start().await, &Case { start: Start::$start, peer: $peer, paused: false, before: None, steps }, true).await;
        }
    )* };
}

/// What a gate PR closed of [`KNOWN_OPEN`], each class as `open!` had it: it passes now (#374).
macro_rules! closed {
    ($($name:ident: $why:literal, $start:ident, $peer:literal, [$($step:expr),*];)*) => { $(
        #[tokio::test(flavor = "multi_thread")]
        async fn $name() {
            let _was_open = $why;
            let steps = vec![$($step),*];
            World::run(&Peers::start().await, &Case { start: Start::$start, peer: $peer, paused: false, before: None, steps }, true).await;
        }
    )* };
}

const B: usize = 1;

fn put(kind: Kind) -> Step {
    Step::By(By::Put { file: B, kind })
}

open! {
    open_another_torrents_file_is_downloaded_over_by_an_add: "#374, #380's review", Whole, true, [Step::AddOther];
    open_an_add_over_another_file: "file-trust A: first add", Nothing, true, [Step::Do(Do::Remove), put(Same), Step::Do(Do::Add)];
    open_an_add_grows_a_shorter_file: "file-trust A: first add", Nothing, true, [Step::Do(Do::Remove), put(Empty), Step::Do(Do::Add)];
    open_a_check_downloads_over_another_file: "file-trust 1, row 9", Whole, true, [put(Same), Step::Do(Do::Check)];
    open_a_check_grows_a_shorter_file: "file-trust A: shorter", Whole, true, [put(Shorter), Step::Do(Do::Check)];
    open_a_check_downloads_over_an_edited_file: "file-trust A: edited in place", Whole, true, [put(Edit), Step::Do(Do::Check)];
    open_a_move_takes_another_file_for_its_own: "file-trust B: Moving", Whole, true, [put(Same), Step::Do(Do::Move { replace: false })];
    open_a_rename_takes_another_file_for_its_own: "file-trust B: Moving", Whole, true, [put(Same), Step::Do(Do::Rename)];
    open_resume_grows_a_shorter_file_before_its_shown: "#361: Download over them is of the files a check read", Whole, true,
        [Step::By(By::TakeAway { file: B }), Step::Settle, put(Shorter), Step::Do(Do::Resume)];
    open_resume_after_a_start_of_files_not_checked_since: "#361: Download over them is of the files a check read", Whole, true,
        [put(Cut), Step::Settle, Step::Crash, Step::Do(Do::Resume)];
    open_an_undone_remove_downloads_over_another_file: "file-trust B: undo of Remove", Whole, true,
        [Step::Do(Do::Remove), put(Same), Step::Do(Do::Undo)];
    open_a_file_set_off_has_an_edited_one_checked_and_downloaded_over: "file-trust 1, rows 17 and 18", Whole, false,
        [
            Step::By(By::TakeAway { file: 0 }), Step::Settle, Step::By(By::Put { file: 0, kind: Same }), Step::Do(Do::Check), Step::Settle,
            Step::During(Do::TrashFile { file: 0 }, By::Put { file: 1, kind: Edit })
        ];
    open_a_check_of_one_put_back_takes_another: "file-trust 1, row 6", Whole, true,
        [Step::By(By::TakeAway { file: 0 }), Step::Settle, Step::By(By::Put { file: 2, kind: Same }), Step::By(By::PutBack { file: 0 })];
    open_an_undone_remove_forgets_what_was_missing: "file-trust B: undo of Remove", Whole, true,
        [Step::By(By::TakeAway { file: B }), Step::Settle, put(Shorter), Step::Settle, Step::Do(Do::Remove), Step::Do(Do::Undo)];
    open_another_file_as_the_drive_is_back: "#373", Whole, true, [Step::By(By::DriveGone), Step::Settle, put(Same), Step::By(By::DriveBack)];
    open_another_file_before_a_start: "file-trust A: replaced, same size", Nothing, false, [put(Same), Step::Restart];
    open_another_file_before_a_start_after_a_crash: "file-trust A: replaced, same size", Nothing, false, [put(Same), Step::Crash];
    open_a_shorter_file_where_less_was_had: "file-trust A: shorter", Nothing, false, [put(Empty), Step::Restart];
    open_another_file_where_nothing_was_had: "file-trust A: missing known, nothing had", Half, false,
        [Step::By(By::TakeAway { file: B }), Step::Settle, put(Same)];
}

closed! {
    a_drive_back_keeps_what_was_missing: "#373: what was known missing is forgotten (#464)", Whole, true,
        [Step::By(By::TakeAway { file: B }), Step::Settle, put(Same), Step::Settle, Step::By(By::DriveGone), Step::Settle, Step::By(By::DriveBack)];
    an_add_over_a_hard_link_writes_nothing: "file-trust A: hard link", Nothing, true, [Step::Do(Do::Remove), put(HardLink), Step::Do(Do::Add)];
    a_longer_file_isnt_written_as_its_folder_is_renamed_during_a_check: "#374: found by this test", Whole, true,
        [put(Longer), Step::Settle, Step::During(Do::Check, By::RenameFolder), Step::Settle];
    resume_over_a_hard_link_writes_nothing: "#361: the hard-link rule", Whole, true,
        [Step::By(By::TakeAway { file: B }), Step::Settle, put(HardLink), Step::Settle, Step::Do(Do::Resume)];
    a_shared_piece_goes_aside_from_a_longer_file_at_an_off_ones: "file-trust A: Off, another file there", Nothing, false,
        [Step::Do(Do::Off { file: B }), put(Longer), Step::Restart];
    a_shared_piece_goes_aside_from_a_file_put_where_an_off_one_was_gone: "file-trust A: Off, another file there", Nothing, false,
        [Step::Do(Do::Off { file: B }), Step::By(By::TakeAway { file: B }), Step::Restart, Step::Settle, put(Same)];
    a_shorter_file_where_a_piece_was_had_isnt_grown: "file-trust A: missing known, then a shorter one", Nothing, true,
        [
            Step::Do(Do::Pause), Step::By(By::Put { file: 2, kind: OnePiece }), Step::By(By::Put { file: 0, kind: OnePiece }),
            Step::By(By::DriveGone), Step::Settle, Step::By(By::DriveBack), Step::Settle,
            Step::By(By::TakeAway { file: B }), Step::Settle, put(Shorter), Step::Settle
        ];
    a_shared_piece_goes_aside_from_another_file_at_an_off_ones: "file-trust A: Off, another file there", Nothing, false,
        [Step::Do(Do::Off { file: B }), put(Same), Step::Restart];
}

/// ADR 0014: a file a Move to Trash takes is closed as it goes, so nothing writes into it in the
/// Trash, a person's file put at the torrent's path too, while the pieces it shares with its
/// neighbours are downloaded, and once the torrent lets go of its pieces.
mod a_file_moved_to_the_trash_is_never_written_there {
    use super::*;

    async fn of(kind: Kind) {
        let peers = Peers::start().await;
        let steps = vec![put(kind), Step::Do(Do::TrashFile { file: B }), Step::Settle, Step::PeerOpens, Step::Settle];
        World::run(&peers, &Case { start: Start::Nothing, peer: true, paused: false, before: None, steps }, true).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn same() {
        of(Same).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn edit() {
        of(Edit).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_piece() {
        of(OnePiece).await;
    }
}

/// Found by this test at 256 cases (#374): a hard link to the torrent's own bytes, put while none of
/// them was had, its neighbours not there, is Missing files until a check reads it; then it's whole but
/// for the piece it shares with them, which only they complete, so it's read only, not Missing files,
/// and the torrent completes.
#[tokio::test(flavor = "multi_thread")]
async fn a_hard_link_to_its_own_bytes_completes_once_checked() {
    let steps = vec![
        Step::By(By::Put { file: 0, kind: EqualHardLink }),
        Step::Do(Do::Remove),
        Step::Do(Do::Undo),
        Step::Settle,
        Step::PeerOpens,
        Step::Settle,
    ];
    World::run(&Peers::start().await, &Case { start: Start::Nothing, peer: false, paused: false, before: None, steps }, true).await;
}

/// #402: gone by hand, or to the Trash: it's downloaded again, not zeros shown as had.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_off_and_gone_set_normal_while_stopped_is_downloaded() {
    let gone = [vec![Step::Do(Do::Off { file: B }), Step::By(By::TakeAway { file: B })], vec![Step::Do(Do::TrashFile { file: B })]];
    strictly(gone.map(|gone| {
        let steps = [vec![Step::Do(Do::Pause)], gone, vec![Step::Settle, Step::Do(Do::Normal { file: B })]].concat();
        Case { start: Start::Half, peer: false, paused: false, before: None, steps }
    }))
    .await;
}

/// Moved without a file that was gone, using another that was there: the answer is no refusal, and
/// what the choice allowed stays allowed.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_without_a_missing_file_takes_the_choice_it_was_sent_with() {
    let put = Step::By(By::PutAtTarget { file: B, kind: Shorter });
    let gone = [Step::By(By::Put { file: 0, kind: Link }), Step::Do(Do::TrashFile { file: 2 }), Step::Settle];
    let steps = [gone.to_vec(), vec![Step::PeerOpens, Step::Settle, put, Step::Do(Do::Move { replace: false })]].concat();
    World::run(&Peers::start().await, &Case { start: Start::Nothing, peer: false, paused: false, before: None, steps }, false).await;
}

/// #406: a Move that uses one file there leaves the person's where another was missing as it is.
mod a_move_that_uses_a_file_there_leaves_one_where_a_file_was_missing {
    use super::*;

    async fn of(kind: Kind) {
        let cases = [false, true].map(|peer| {
            let gone = [Step::Settle, Step::By(By::TakeAway { file: B }), Step::Settle, put(kind)];
            let used = Step::By(By::PutAtTarget { file: 0, kind: Same });
            let steps = [gone.to_vec(), vec![used, Step::Do(Do::Move { replace: false }), Step::Settle]].concat();
            Case { start: Start::Whole, peer, paused: false, before: None, steps }
        });
        strictly(cases).await;
    }

    macro_rules! kinds {
        ($($name:ident: $kind:expr),*) => {$(
            #[tokio::test(flavor = "multi_thread")]
            async fn $name() {
                of($kind).await;
            }
        )*};
    }
    kinds!(same: Same, shorter: Shorter, empty: Empty, one_piece: OnePiece, cut: Cut, hard_link: HardLink, edit: Edit, longer: Longer);
}

/// Runs each of `cases` strictly, `WIDTH` at a time as the generated ones are: each takes seconds,
/// most of them waiting. A failing one fails the test, named.
async fn strictly(cases: impl IntoIterator<Item = Case>) {
    let peers = Arc::new(Peers::start().await);
    super::side_by_side(WIDTH, cases, move |case| {
        let peers = peers.clone();
        async move {
            if let Err(why) = fails(&peers, &case, true).await {
                panic!("{why}\ncase: {case:?}");
            }
        }
    })
    .await;
}

/// One that depends on when its steps come: some runs of it pass.
async fn often(steps: Vec<Step>) {
    strictly(vec![Case { start: Start::Whole, peer: true, paused: false, before: None, steps }; 20]).await;
}

/// #375: a piece being put back as the torrent is paused isn't lost.
#[tokio::test(flavor = "multi_thread")]
async fn a_piece_put_back_as_the_torrent_is_paused_isnt_lost() {
    let back = Step::During(Do::Resume, By::PutBack { file: 0 });
    often(vec![Step::By(By::TakeAway { file: 0 }), Step::Settle, back, Step::Do(Do::Pause)]).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "file-trust A: shorter than had, a crash"]
async fn open_a_crash_during_a_check_lets_a_shorter_file_grow() {
    let put = Step::Between { by: By::Put { file: 0, kind: Shorter }, crash: true };
    often(vec![Step::Settle, Step::Do(Do::Check), put]).await;
}

/// Found by this test (#464): a file put where one known missing was, the drive gone and back, then a
/// crash once the other missing file is back. The check of the returning drive took the person's file
/// for the torrent's own, so the next start downloaded into it.
#[tokio::test(flavor = "multi_thread")]
async fn a_drive_back_keeps_a_file_put_where_one_was_missing() {
    let steps = vec![
        Step::By(By::TakeAway { file: 0 }),
        Step::Settle,
        Step::By(By::Put { file: 0, kind: OnePiece }),
        put(Link),
        Step::By(By::DriveGone),
        Step::Settle,
        Step::By(By::DriveBack),
        Step::Settle,
        Step::Between { by: By::Put { file: B, kind: OnePiece }, crash: true },
    ];
    World::run(&Peers::start().await, &Case { start: Start::Whole, peer: false, paused: false, before: None, steps }, true).await;
}
