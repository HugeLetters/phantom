//! The engine against a reference model: generated sequences of client commands and of what
//! happens around it (files deleted, put back or changed by hand, destinations gone and back,
//! restarts), each case on a real
//! librqbit session in its own temp dir, offline. After every step the engine must settle where
//! the model says: the same torrents with the same ids and statuses, files and progress, what each
//! still needs on its disk (#127), and the same files on disk. And at once: the snapshot clients read is the one the actor's state makes,
//! and lists the model's torrents. librqbit is taken to be correct; the model is what Phantom
//! promises.
//!
//! The torrents are two packs of three files, each already complete in its own destination, and a
//! magnet whose info never arrives. A step writes a file of the user's, `mine.txt`, or Finder's
//! `.DS_Store` into a pack's folder: nothing may move the user's, and a folder of only the
//! torrent's files and litter goes whole. Delete moves files to a trash of the case's own, which a
//! step makes refuse one file or a pack's folder, as a locked one: what's in the trash, and which
//! folders are left, are compared too. A step holds the trash at a gate, which it waits at after
//! each item: a Delete then waits, Deleting, having moved its first, while the other steps run
//! (other torrents' commands are answered, the snapshot follows), until a step opens it and the
//! Delete is answered, or the service stops (which waits for it) or is killed (which leaves the
//! rest where it is). Throughout, no file a Delete moved comes back at its path, unless written
//! there by hand or its torrent is added again: the model asserts it of itself, and the engine
//! must agree with the model. Nothing downloads (no peers), so a torrent's progress is what
//! its checks found on disk.
//!
//! 16 cases per `cargo test`; `PROPTEST_CASES=256 cargo test -p service model` runs more.
//! Failing sequences, shrunk, are saved to `service/proptest-regressions/` and run first after.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{AbsolutePath, AddSource, Added, Priority, Status, TorrentCommand, TorrentId, Unavailable};
use tempfile::TempDir;

use super::files::Seen;
use super::machine::Phase;
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
    /// A file of its size with other bytes, changed by hand: it takes its space on disk.
    Other,
    /// One librqbit created: of its length, but sparse (the cases run on APFS), so it takes no
    /// space yet.
    Sparse,
}

#[derive(Debug, Clone)]
enum Step {
    /// `pack`: one of the packs, else the magnet.
    Add {
        pack: Option<usize>,
        paused: bool,
    },
    Pause(TorrentId),
    Resume(TorrentId),
    Check(TorrentId),
    Remove(TorrentId),
    Delete(TorrentId),
    SetPriority {
        id: TorrentId,
        file: usize,
        priority: Priority,
    },
    /// By hand, outside Phantom.
    DeleteFile {
        pack: usize,
        file: usize,
    },
    /// The file it was, written back by hand.
    PutBack {
        pack: usize,
        file: usize,
    },
    /// Other bytes of the same size, written by hand.
    Overwrite {
        pack: usize,
        file: usize,
    },
    /// The pack's destination folder is moved unavailable, as a drive is disconnected.
    DestinationGone {
        pack: usize,
    },
    /// And moved back.
    DestinationBack {
        pack: usize,
    },
    /// The service stops and starts again.
    Restart,
    /// The service is killed while a Delete waits at the gate, and starts again.
    Kill,
    /// The trash refuses the pack's file (`None`: its folder) from now on, or nothing (`None`).
    Refuse(Option<(usize, Option<usize>)>),
    /// The trash waits at a gate from now on: a Delete is answered once it's opened.
    HoldTrash,
    /// The gate opens: a Delete waiting goes on, and is answered.
    ReleaseTrash,
    /// A file of the user's, `mine.txt`, is written into the pack's folder.
    WriteMine {
        pack: usize,
    },
    /// Finder leaves a `.DS_Store` in the pack's folder.
    WriteLitter {
        pack: usize,
    },
}

#[derive(Debug, Clone)]
struct Model {
    disk: [[OnDisk; FILES]; PACKS],
    /// Each pack's destination is unavailable.
    unavailable: [bool; PACKS],
    torrents: BTreeMap<TorrentId, Torrent>,
    next_id: TorrentId,
    /// What the last step's command answers.
    answer: Answer,
    /// The files Delete moved to the Trash, by pack and file, in order.
    trashed: Vec<(usize, usize)>,
    /// What the trash refuses: a pack's file, or its folder (`None`).
    refusing: Option<(usize, Option<usize>)>,
    /// The trash is held at its gate; the Delete waiting there, if any.
    held: bool,
    pending: Option<Pending>,
    /// The files Delete moved to the Trash, by pack: none may come back at its path until it's
    /// written there by hand, or its torrent is added again.
    moved: BTreeSet<(usize, usize)>,
    /// Each pack's folder is on disk; holds `mine.txt`; holds `.DS_Store`.
    folder: [bool; PACKS],
    mine: [bool; PACKS],
    litter: [bool; PACKS],
}

/// A Delete waiting at the trash's gate: torrent `id` of `pack`, what it plans, how far it got.
#[derive(Debug, Clone)]
struct Pending {
    id: TorrentId,
    pack: usize,
    items: Vec<Item>,
    done: usize,
    kept: Vec<Item>,
}

/// What Delete trashes of a pack: its folder whole, or a file of it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    Folder,
    File(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum Answer {
    None,
    Ok,
    Err,
    Added(Added),
    /// Not answered yet: a Delete waiting at the trash's gate.
    Pending,
}

#[derive(Debug, Clone)]
enum Torrent {
    Magnet,
    Pack(Pack),
}

#[derive(Debug, Clone)]
struct Pack {
    pack: usize,
    /// Wanted running, rather than stopped: set by the add, Pause and Resume only.
    running: bool,
    off: [bool; FILES],
    /// Whether librqbit has the file: what its last check found.
    have: [bool; FILES],
    /// The files found missing; it's Missing files while there are any.
    missing: BTreeMap<usize, Gone>,
    /// Its destination is unavailable: it's Destination unavailable, and `running` says whether it runs
    /// again once it's back.
    unavailable: bool,
    /// Delete moved some of its files to the Trash but not all: an error, stopped, until it's
    /// deleted again or removed; resuming, checking and priorities are refused. Kept across the
    /// drive going and coming back, and restarts.
    delete_failed: bool,
    /// Its Delete waits at the trash's gate: every command is refused.
    deleting: bool,
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

    /// What it still needs on its disk: while it's wanted running and its destination can be
    /// reached, the files it gets (not those Off) that take no space there yet, whatever it has
    /// of them; `None` when that's nothing. Checks don't change it.
    fn needs(&self, disk: &[OnDisk; FILES]) -> Option<u64> {
        let getting = (0..FILES).filter(|&f| !self.off[f]);
        let left = getting.filter(|&f| matches!(disk[f], OnDisk::Absent | OnDisk::Sparse)).count() as u64 * FULL;
        (self.running && !self.unavailable && left > 0).then_some(left)
    }

    fn status(&self) -> Status {
        if self.deleting {
            return Status::Deleting;
        }
        if self.unavailable {
            return Status::DestinationUnavailable { reason: folder_missing(self.pack) };
        }
        if self.delete_failed {
            return Status::Error { failure: proto::TorrentFailure::Librqbit { detail: String::new() } };
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
    fn check(&mut self, disk: &mut [OnDisk; FILES]) {
        librqbit_recreates_absent(disk);
        for f in 0..FILES {
            self.have[f] = disk[f] == OnDisk::Valid;
        }
        self.missing.clear();
    }

    /// The engine's size check: a file it gets that's gone makes it Missing files; once
    /// every missing file is back it's checked, and those that aren't what they were are missing
    /// again.
    fn size_check(&mut self, disk: &mut [OnDisk; FILES]) {
        let gone: Vec<usize> = (0..FILES).filter(|&f| !self.off[f] && disk[f] == OnDisk::Absent).collect();
        if self.missing.is_empty() {
            if !gone.is_empty() {
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
        self.check(disk);
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
        if pack.is_some_and(|k| self.unavailable[k]) {
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
                let mut p = Pack {
                    pack,
                    running: !paused,
                    off: [false; FILES],
                    have: [false; FILES],
                    missing: BTreeMap::new(),
                    unavailable: false,
                    delete_failed: false,
                    deleting: false,
                };
                p.check(&mut self.disk[pack]);
                // librqbit makes its folder for the files it makes.
                self.folder[pack] = true;
                Torrent::Pack(p)
            }
        };
        self.torrents.insert(id, torrent);
        Answer::Added(Added { id, name: name(pack), already_existed: false })
    }

    /// A command to torrent `id`: what it answers, and what it does.
    fn command(&mut self, id: TorrentId, step: &Step) -> Answer {
        if let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) {
            if p.deleting {
                return Answer::Err;
            }
            if matches!(step, Step::Delete(_)) && !p.unavailable {
                let pack = p.pack;
                return self.delete(id, pack);
            }
            // Each would have librqbit make the trashed files again.
            if p.delete_failed && matches!(step, Step::Resume(_) | Step::Check(_) | Step::SetPriority { .. }) {
                return Answer::Err;
            }
        }
        let disk = &mut self.disk;
        let Some(torrent) = self.torrents.get_mut(&id) else { return Answer::Err };
        let p = match torrent {
            Torrent::Magnet if matches!(step, Step::Remove(_) | Step::Delete(_)) => {
                self.torrents.remove(&id);
                return Answer::Ok;
            }
            // Pause and Resume are remembered for when its info arrives, which a magnet's never does.
            Torrent::Magnet if matches!(step, Step::Pause(_) | Step::Resume(_)) => return Answer::Ok,
            // No info yet.
            Torrent::Magnet => return Answer::Err,
            Torrent::Pack(p) => p,
        };
        let pack = p.pack;
        let missing = !p.missing.is_empty();
        match step {
            // Its files are out of reach.
            Step::Resume(_) | Step::Check(_) | Step::Delete(_) if p.unavailable => return Answer::Err,
            Step::Pause(_) => p.running = false,
            Step::Resume(_) => {
                p.running = true;
                if missing {
                    p.check(&mut disk[pack]);
                }
            }
            Step::Check(_) => p.check(&mut disk[pack]),
            Step::SetPriority { file, priority, .. } => {
                p.off[*file] = *priority == Priority::Off;
                if *priority == Priority::Off {
                    p.missing.remove(file);
                }
            }
            Step::Remove(_) => {
                self.torrents.remove(&id);
            }
            _ => unreachable!("not a command: {step:?}"),
        }
        Answer::Ok
    }

    /// Delete of pack `pack`'s torrent `id`: its folder whole to the Trash when nothing of the
    /// user's is in it, else each of its files on disk; then the folders left empty go. What the
    /// trash refuses stays: if nothing went, the torrent is as it was; if some did, it waits with
    /// an error. Either way it's refused. With the trash held, the first item goes, and the rest
    /// wait for the gate.
    fn delete(&mut self, id: TorrentId, pack: usize) -> Answer {
        let items: Vec<Item> = if !self.mine[pack] {
            vec![Item::Folder]
        } else {
            (0..FILES).filter(|&f| self.disk[pack][f] != OnDisk::Absent).map(Item::File).collect()
        };
        let mut pending = Pending { id, pack, items, done: 0, kept: Vec::new() };
        if self.held && !pending.items.is_empty() {
            self.take(&mut pending);
            let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { unreachable!() };
            p.deleting = true;
            self.pending = Some(pending);
            return Answer::Pending;
        }
        while pending.done < pending.items.len() {
            self.take(&mut pending);
        }
        self.finish(pending)
    }

    /// The trash takes the Delete's next item, or refuses it.
    fn take(&mut self, pending: &mut Pending) {
        let (pack, item) = (pending.pack, pending.items[pending.done]);
        pending.done += 1;
        let refused = match (item, self.refusing) {
            (Item::Folder, Some((k, None))) => k == pack,
            (Item::File(f), Some((k, Some(r)))) => k == pack && r == f,
            _ => false,
        };
        if refused {
            pending.kept.push(item);
            return;
        }
        let files: Vec<usize> = match item {
            Item::Folder => (0..FILES).filter(|&f| self.disk[pack][f] != OnDisk::Absent).collect(),
            Item::File(f) => vec![f],
        };
        for f in files {
            self.trashed.push((pack, f));
            self.moved.insert((pack, f));
            self.disk[pack][f] = OnDisk::Absent;
        }
        self.trashed.sort();
        if item == Item::Folder {
            (self.folder[pack], self.litter[pack]) = (false, false);
        }
    }

    /// How the Delete went: all of it, and it's off the list; nothing, and it's as it was; some,
    /// and it waits with an error.
    fn finish(&mut self, pending: Pending) -> Answer {
        let Pending { id, items, kept, .. } = pending;
        if kept.is_empty() {
            self.torrents.remove(&id);
            return Answer::Ok;
        }
        let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { unreachable!() };
        p.deleting = false;
        if kept.len() < items.len() {
            p.delete_failed = true;
            p.missing.clear();
        }
        Answer::Err
    }

    /// The gate opens: the Delete waiting goes on to its end.
    fn release(&mut self) -> Answer {
        self.held = false;
        let Some(mut pending) = self.pending.take() else { return Answer::None };
        while pending.done < pending.items.len() {
            self.take(&mut pending);
        }
        self.finish(pending)
    }

    /// Killed with a Delete waiting: the rest of it stays. At the next start, what of the
    /// torrent's files is on disk is what it left; with none, it's deleted.
    fn kill(&mut self) {
        self.held = false;
        let Some(pending) = self.pending.take() else { return };
        let left = (0..FILES).any(|f| self.disk[pending.pack][f] != OnDisk::Absent);
        if !left {
            self.torrents.remove(&pending.id);
            return;
        }
        let Some(Torrent::Pack(p)) = self.torrents.get_mut(&pending.id) else { unreachable!() };
        (p.deleting, p.delete_failed) = (false, true);
        p.missing.clear();
    }

    /// The written file counts as put back for a torrent missing it.
    fn written(&mut self, pack: usize, file: usize) {
        if let Some(gone) = self.pack_torrent(pack).and_then(|p| p.missing.get_mut(&file)) {
            gone.back = true;
        }
    }

    /// With its destination unavailable, it's stopped in librqbit, remembering whether it was running; its missing files are
    /// forgotten, as they're out of reach.
    fn destination_gone(&mut self, pack: usize) {
        self.unavailable[pack] = true;
        if let Some(p) = self.pack_torrent(pack) {
            p.unavailable = true;
            p.missing.clear();
        }
    }

    /// Back, it's added again, so checked, and runs if it was running; one partly deleted only
    /// says so.
    fn destination_back(&mut self, pack: usize) {
        self.unavailable[pack] = false;
        let disk = &mut self.disk[pack];
        if let Some(p) = self.torrents.values_mut().find_map(|t| match t {
            Torrent::Pack(p) if p.pack == pack => Some(p),
            _ => None,
        }) {
            p.unavailable = false;
            if !p.delete_failed {
                p.check(disk);
            }
        }
    }

    fn restart(&mut self) {
        // Torrents fetching info don't survive it.
        self.torrents.retain(|_, t| matches!(t, Torrent::Pack(_)));
        for t in self.torrents.values_mut() {
            let Torrent::Pack(p) = t else { continue };
            // Restored without touching its files, so librqbit has nothing of them until they're
            // back and checked.
            // Restored without touching its files either: its error comes back with it.
            if p.unavailable || p.delete_failed {
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
        let needs = self
            .torrents
            .iter()
            .filter_map(|(&id, t)| match t {
                Torrent::Pack(p) => p.needs(&self.disk[p.pack]).map(|bytes| (id, bytes)),
                Torrent::Magnet => None,
            })
            .collect();
        let torrents = self
            .torrents
            .iter()
            .map(|(&id, t)| match t {
                Torrent::Magnet => Listed { id, status: Status::FetchingInfo, progress_bytes: 0, files: Vec::new() },
                Torrent::Pack(p) => {
                    let counted = |f: usize| !p.off[f] && p.have[f] && !p.missing.contains_key(&f);
                    let progress_bytes = (0..FILES).filter(|&f| counted(f)).count() as u64 * FULL;
                    let files =
                        (0..FILES).map(|f| (if p.off[f] { Priority::Off } else { Priority::Normal }, p.missing.contains_key(&f))).collect();
                    Listed { id, status: p.status(), progress_bytes, files }
                }
            })
            .collect();
        View { torrents, needs, disk: self.disk, trashed: self.trashed.clone(), folder: self.folder, mine: self.mine, litter: self.litter }
    }
}

/// librqbit adding a torrent, or restoring it at start, creates every file of it that isn't on
/// disk, Off ones too, empty. Not a promise but a bug the model follows, so fixing it is a change
/// here (#67).
fn librqbit_recreates_absent(disk: &mut [OnDisk; FILES]) {
    for f in disk.iter_mut().filter(|f| **f == OnDisk::Absent) {
        *f = OnDisk::Sparse;
    }
}

/// Why the pack's destination can't be reached while it's unavailable, with the case's dir as `/`.
fn folder_missing(pack: usize) -> Unavailable {
    Unavailable::FolderMissing { path: AbsolutePath::new(destination(Path::new("/"), pack)).unwrap() }
}

fn name(pack: Option<usize>) -> Option<String> {
    pack.map(|k| format!("pack{k}"))
}

/// What's compared after every step.
#[derive(Debug, PartialEq)]
struct View {
    torrents: Vec<Listed>,
    /// What each torrent still needs on its disk, by id: those that need something.
    needs: BTreeMap<TorrentId, u64>,
    disk: [[OnDisk; FILES]; PACKS],
    trashed: Vec<(usize, usize)>,
    /// Each pack's folder, its `mine.txt` and its `.DS_Store` are there (wherever its destination
    /// is).
    folder: [bool; PACKS],
    mine: [bool; PACKS],
    litter: [bool; PACKS],
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
        Just(Model {
            disk,
            unavailable: [false; PACKS],
            torrents: BTreeMap::new(),
            next_id: 0,
            answer: Answer::None,
            trashed: Vec::new(),
            refusing: None,
            held: false,
            pending: None,
            moved: BTreeSet::new(),
            folder: [true; PACKS],
            mine: [false; PACKS],
            litter: [false; PACKS],
        })
        .boxed()
    }

    fn transitions(model: &Model) -> BoxedStrategy<Step> {
        // The torrents listed, and an id no torrent has.
        let ids: Vec<TorrentId> = model.torrents.keys().copied().chain([model.next_id]).collect();
        let id = proptest::sample::select(ids);
        let pack = 0..PACKS;
        let file = 0..FILES;
        let priority = prop_oneof![Just(Priority::Normal), Just(Priority::Off)];
        let mut steps: Vec<(u32, BoxedStrategy<Step>)> = vec![
            (
                3,
                (proptest::option::weighted(0.85, pack.clone()), any::<bool>())
                    .prop_map(|(pack, paused)| Step::Add { pack, paused })
                    .boxed(),
            ),
            (2, id.clone().prop_map(Step::Pause).boxed()),
            (2, id.clone().prop_map(Step::Resume).boxed()),
            (2, id.clone().prop_map(Step::Check).boxed()),
            (1, id.clone().prop_map(Step::Remove).boxed()),
            (2, id.clone().prop_map(Step::Delete).boxed()),
            (3, (id, file.clone(), priority).prop_map(|(id, file, priority)| Step::SetPriority { id, file, priority }).boxed()),
            (3, (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::DeleteFile { pack, file }).boxed()),
            (3, (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::PutBack { pack, file }).boxed()),
            (1, (pack.clone(), file).prop_map(|(pack, file)| Step::Overwrite { pack, file }).boxed()),
            (1, pack.clone().prop_map(|pack| Step::DestinationGone { pack }).boxed()),
            (2, pack.clone().prop_map(|pack| Step::DestinationBack { pack }).boxed()),
            (1, Just(Step::Restart).boxed()),
            (2, proptest::option::of((pack.clone(), proptest::option::of(0..FILES))).prop_map(Step::Refuse).boxed()),
            (2, pack.clone().prop_map(|pack| Step::WriteMine { pack }).boxed()),
            (1, pack.clone().prop_map(|pack| Step::WriteLitter { pack }).boxed()),
            (1, Just(Step::HoldTrash).boxed()),
            (2, Just(Step::ReleaseTrash).boxed()),
        ];
        // The rarer states, reached more often: a Delete into a held trash, a stop or a crash
        // while one waits, and a partial Delete's drive going and coming back.
        let packs: Vec<(TorrentId, &Pack)> = model
            .torrents
            .iter()
            .filter_map(|(id, t)| match t {
                Torrent::Pack(p) => Some((*id, p)),
                Torrent::Magnet => None,
            })
            .collect();
        // A held Delete that moves nothing: the trash refusing a pack's folder whole.
        if model.held && model.pending.is_none() {
            steps.push((2, pack.clone().prop_map(|pack| Step::Refuse(Some((pack, None)))).boxed()));
        }
        if model.held && model.pending.is_none() && !packs.is_empty() {
            steps.push((6, proptest::sample::select(packs.iter().map(|(id, _)| *id).collect::<Vec<_>>()).prop_map(Step::Delete).boxed()));
        }
        if model.pending.is_some() {
            steps.push((4, Just(Step::Kill).boxed()));
            steps.push((4, Just(Step::Restart).boxed()));
        }
        // On the way to a partial Delete: a file of the user's beside a torrent's (so its files go
        // one by one), and one of them refused.
        let listed: Vec<usize> = packs.iter().map(|(_, p)| p.pack).collect();
        let bare: Vec<usize> = listed.iter().copied().filter(|&k| !model.mine[k] && !model.unavailable[k]).collect();
        if !bare.is_empty() {
            steps.push((2, proptest::sample::select(bare).prop_map(|pack| Step::WriteMine { pack }).boxed()));
        }
        let beside: Vec<usize> =
            listed.iter().copied().filter(|&k| model.mine[k] && !matches!(model.refusing, Some((r, Some(_))) if r == k)).collect();
        if !beside.is_empty() && model.pending.is_none() {
            let refuse = (proptest::sample::select(beside), 0..FILES).prop_map(|(pack, f)| Step::Refuse(Some((pack, Some(f)))));
            steps.push((3, refuse.boxed()));
        }
        // Then its Delete.
        let primed: Vec<TorrentId> = packs
            .iter()
            .filter(|(_, p)| model.mine[p.pack] && !p.delete_failed && matches!(model.refusing, Some((r, Some(_))) if r == p.pack))
            .map(|(id, _)| *id)
            .collect();
        if !primed.is_empty() && model.pending.is_none() {
            steps.push((6, proptest::sample::select(primed).prop_map(Step::Delete).boxed()));
        }
        if packs.is_empty() {
            steps.push((4, (0..PACKS, any::<bool>()).prop_map(|(pack, paused)| Step::Add { pack: Some(pack), paused }).boxed()));
        }
        let partly: Vec<(TorrentId, usize)> = packs.iter().filter(|(_, p)| p.delete_failed).map(|(id, p)| (*id, p.pack)).collect();
        if !partly.is_empty() {
            let one = proptest::sample::select(partly);
            steps.push((2, one.clone().prop_map(|(id, _)| Step::Resume(id)).boxed()));
            steps.push((2, one.clone().prop_map(|(id, _)| Step::Check(id)).boxed()));
            let priority = prop_oneof![Just(Priority::Normal), Just(Priority::Off)];
            steps.push((
                1,
                (one.clone(), 0..FILES, priority).prop_map(|((id, _), file, priority)| Step::SetPriority { id, file, priority }).boxed(),
            ));
            steps.push((3, one.clone().prop_map(|(_, pack)| Step::DestinationGone { pack }).boxed()));
            steps.push((3, one.clone().prop_map(|(_, pack)| Step::DestinationBack { pack }).boxed()));
            steps.push((2, one.prop_map(|(id, _)| Step::Delete(id)).boxed()));
            steps.push((3, Just(Step::Restart).boxed()));
        }
        proptest::strategy::Union::new_weighted(steps).boxed()
    }

    /// A restart only when what librqbit has is what's on disk (but for missing files, which
    /// are checked anyway): restoring a torrent, librqbit trusts what it saved while the files'
    /// sizes fit and checks them again otherwise, which is its business rather than Phantom's.
    /// Files are only changed by hand, and destinations only go and come back, when they can.
    fn preconditions(model: &Model, step: &Step) -> bool {
        // The pack whose Delete waits at the gate: nothing changes its files meanwhile.
        let waiting = model.pending.as_ref().map(|p| p.pack);
        let touches = |pack: usize| waiting == Some(pack);
        match step {
            Step::DeleteFile { pack, .. }
            | Step::PutBack { pack, .. }
            | Step::Overwrite { pack, .. }
            | Step::DestinationGone { pack }
            | Step::DestinationBack { pack }
            | Step::WriteMine { pack }
            | Step::WriteLitter { pack }
                if touches(*pack) =>
            {
                false
            }
            Step::Refuse(_) if model.pending.is_some() => false,
            // One Delete waits at a time.
            Step::Delete(id) if model.pending.is_some() => {
                !matches!(model.torrents.get(id), Some(Torrent::Pack(p)) if !p.deleting && !p.unavailable)
            }
            Step::HoldTrash => !model.held,
            Step::ReleaseTrash => model.held,
            Step::DeleteFile { pack, .. } | Step::PutBack { pack, .. } | Step::Overwrite { pack, .. } => !model.unavailable[*pack],
            Step::DestinationGone { pack } => !model.unavailable[*pack],
            Step::WriteMine { pack } => !model.unavailable[*pack] && !model.mine[*pack],
            Step::WriteLitter { pack } => !model.unavailable[*pack] && !model.litter[*pack],
            Step::DestinationBack { pack } => model.unavailable[*pack],
            Step::Restart => restorable(model),
            // A crash records nothing at stop: no torrent misses files, which a stop records.
            Step::Kill => {
                model.pending.is_some()
                    && restorable(model)
                    && model.torrents.values().all(|t| !matches!(t, Torrent::Pack(p) if !p.missing.is_empty()))
            }
            _ => true,
        }
    }

    fn apply(mut model: Model, step: &Step) -> Model {
        model.answer = match step {
            // A new torrent: its files may be made again.
            Step::Add { pack: Some(k), paused } => {
                let answer = model.add(Some(*k), *paused);
                if matches!(&answer, Answer::Added(a) if !a.already_existed) {
                    model.moved.retain(|(p, _)| p != k);
                }
                answer
            }
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
            // Written by hand, into its folder, made again if it went.
            Step::PutBack { pack, file } => {
                model.moved.remove(&(*pack, *file));
                model.disk[*pack][*file] = OnDisk::Valid;
                model.folder[*pack] = true;
                model.written(*pack, *file);
                Answer::None
            }
            Step::Overwrite { pack, file } => {
                model.moved.remove(&(*pack, *file));
                model.disk[*pack][*file] = OnDisk::Other;
                model.folder[*pack] = true;
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
            // The stop waits for a Delete under way, the gate opened.
            Step::Restart => {
                if model.pending.is_some() {
                    model.release();
                }
                model.restart();
                Answer::None
            }
            Step::Refuse(refusing) => {
                model.refusing = *refusing;
                Answer::None
            }
            Step::WriteMine { pack } => {
                (model.mine[*pack], model.folder[*pack]) = (true, true);
                Answer::None
            }
            Step::HoldTrash => {
                model.held = true;
                Answer::None
            }
            Step::ReleaseTrash => model.release(),
            Step::Kill => {
                model.kill();
                model.restart();
                Answer::None
            }
            Step::WriteLitter { pack } => {
                (model.litter[*pack], model.folder[*pack]) = (true, true);
                Answer::None
            }
        };
        // What Phantom promises of Delete: nothing it moved is made again.
        for &(k, f) in &model.moved {
            assert_eq!(model.disk[k][f], OnDisk::Absent, "file {f} of pack {k}, moved to the Trash, is back after {step:?}");
        }
        let Model { disk, torrents, .. } = &mut model;
        for t in torrents.values_mut() {
            if let Torrent::Pack(p) = t
                && !p.unavailable
                && !p.delete_failed
                && !p.deleting
            {
                p.size_check(&mut disk[p.pack]);
            }
        }
        model
    }
}

/// Whether a restart may come: what librqbit has of every torrent is what's on disk (see
/// `preconditions`), but for one whose files are partly in the Trash, which isn't checked.
fn restorable(model: &Model) -> bool {
    model.torrents.values().all(|t| match t {
        Torrent::Magnet => true,
        Torrent::Pack(p) if p.delete_failed || p.deleting => true,
        Torrent::Pack(p) => {
            (0..FILES).filter(|f| !p.missing.contains_key(f)).all(|f| p.have[f] == (model.disk[p.pack][f] == OnDisk::Valid))
        }
    })
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
    /// The paths the trash refuses.
    refusing: Arc<Mutex<Vec<PathBuf>>>,
    /// The trash's gate, and the Delete waiting there.
    gate: super::trash::Gate,
    pending: Option<tokio::task::JoinHandle<Result<(), super::Failure>>>,
}

/// A step that fails with the gate held still ends: the runtime waits for the trash's thread.
impl Drop for Sut {
    fn drop(&mut self) {
        super::trash::hold(&self.gate, false);
    }
}

/// What torrent `id` needs in `snapshot`: nothing when no disk lists it.
fn needs_of(snapshot: &super::snapshot::Snapshot, id: TorrentId) -> u64 {
    snapshot.volumes.iter().flat_map(|v| &v.needs).find(|n| n.id == id).map_or(0, |n| n.bytes)
}

impl Sut {
    /// Records the most torrent `id` needs in every snapshot published from now, until aborted.
    fn most_needed(&self, id: TorrentId) -> (std::sync::Arc<std::sync::Mutex<u64>>, tokio::task::JoinHandle<()>) {
        let most = std::sync::Arc::new(std::sync::Mutex::new(0));
        let mut published = self.engine().subscribe();
        let recorded = most.clone();
        let recording = self.rt.spawn(async move {
            loop {
                let needs = needs_of(&published.borrow_and_update().snapshot, id);
                {
                    let mut most = recorded.lock().unwrap();
                    *most = (*most).max(needs);
                }
                if published.changed().await.is_err() {
                    return;
                }
            }
        });
        (most, recording)
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    /// Where the file is, its destination unavailable or not.
    fn file(&self, pack: usize, file: usize) -> PathBuf {
        let at = |dir: PathBuf| dir.join(format!("pack{pack}")).join(self.names[pack][file]);
        let moved = moved_away(self.dir.path(), pack);
        if moved.exists() { at(moved) } else { at(destination(self.dir.path(), pack)) }
    }

    /// What's in the case's trash, by pack and file: each item is named after the path it had, a
    /// file of a pack or a pack's folder whole.
    fn trashed(&self) -> Vec<(usize, usize)> {
        let mut trashed = Vec::new();
        for entry in std::fs::read_dir(self.dir.path().join("trash")).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let parts: Vec<&str> = name.split('%').collect();
            let Some(at) = parts.iter().position(|p| p.starts_with("destination")) else { panic!("{name} isn't a pack's") };
            let pack: usize = parts[at].trim_start_matches("destination").parse().unwrap();
            let file_of =
                |n: &str| self.names[pack].iter().position(|x| *x == n).unwrap_or_else(|| panic!("{n} isn't a file of pack {pack}"));
            match &parts[at + 1..] {
                // A folder whole, with Finder's litter in it, which isn't the torrent's to count.
                [_folder] => {
                    for f in std::fs::read_dir(entry.path()).unwrap().flatten() {
                        if f.file_name() != ".DS_Store" {
                            trashed.push((pack, file_of(&f.file_name().to_string_lossy())));
                        }
                    }
                }
                [_folder, file] => trashed.push((pack, file_of(file))),
                other => panic!("{other:?} in the trash"),
            }
        }
        trashed.sort();
        trashed
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
            size_check_every: Duration::from_millis(200),
            trash: super::trash::into(self.dir.path().join("trash"), self.refusing.clone(), self.gate.clone()),
        };
        self.engine = Some(self.rt.block_on(Engine::start(config)).unwrap());
    }

    /// Runs a client call, which must be answered.
    fn call<T, E>(&self, call: impl Future<Output = Result<T, E>>) -> Result<T, E> {
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
                    // What's wrong is the model's to say, not how it's worded.
                    Status::Error { .. } => Status::Error { failure: proto::TorrentFailure::Librqbit { detail: String::new() } },
                    status => status,
                };
                Listed { id: t.id, status, progress_bytes: t.content.map_or(0, |c| c.progress_bytes), files }
            })
            .collect();
        let disk = std::array::from_fn(|k| std::array::from_fn(|f| on_disk(&self.file(k, f), &self.content(k, f))));
        let volumes = self.rt.block_on(engine.inspect(|actor| actor.published.borrow().snapshot.volumes.clone()));
        let needs = volumes.iter().flat_map(|v| v.needs.iter().map(|n| (n.id, n.bytes))).collect();
        let in_folder = |k: usize, name: &str| self.file(k, 0).with_file_name(name).exists();
        let folder = std::array::from_fn(|k| self.file(k, 0).parent().unwrap().is_dir());
        View {
            torrents,
            needs,
            disk,
            trashed: self.trashed(),
            folder,
            mine: std::array::from_fn(|k| in_folder(k, "mine.txt")),
            litter: std::array::from_fn(|k| in_folder(k, ".DS_Store")),
        }
    }

    /// Whether the engine has size-checked since it started: until then, what's at a
    /// missing file's path isn't recorded, and a file put back would be taken for it.
    fn size_checked(&self) -> bool {
        let unjudged = self.rt.block_on(self.engine().inspect(|actor| {
            actor
                .torrents
                .values()
                .any(|t| matches!(&t.lifecycle.phase, Phase::MissingFiles { gone } if gone.values().any(|g| g.seen == Seen::NotYet)))
        }));
        !unjudged
    }

    /// The snapshot clients read is the one the actor's state makes now: nothing it changed went
    /// unpublished.
    fn published_is_current(&self) {
        let (published, current) =
            self.rt.block_on(self.engine().inspect(|actor| (actor.published.borrow().snapshot.clone(), actor.snapshot())));
        assert_eq!(published, current, "the published snapshot isn't the actor's state");
    }

    /// Waits for a regular size check that starts after now, so the engine has looked at the
    /// disk as it is: until then a file written at a missing file's path may look the same as
    /// before, and the next step could change the disk under the check it leads to. The actor
    /// answers between size checks, so the next one after the count is read starts after.
    fn size_check_from_now(&self) {
        let count = || self.rt.block_on(self.engine().inspect(|actor| actor.size_checks));
        let start = count();
        let deadline = Instant::now() + SETTLE;
        while count() == start {
            assert!(Instant::now() < deadline, "no size check in {SETTLE:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits for the engine to show what the model says, and fails with the difference if it
    /// doesn't in time.
    fn settle(&self, model: &Model) {
        let want = model.view();
        let deadline = Instant::now() + SETTLE;
        loop {
            let seen = self.view();
            if seen == want && self.size_checked() {
                self.published_is_current();
                return;
            }
            if Instant::now() > deadline {
                assert_eq!(seen, want, "the engine didn't settle where the model says in {SETTLE:?}");
                panic!("the engine never size-checked after starting");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The pack's destination: it's in its own folder there.
fn destination(dir: &Path, pack: usize) -> PathBuf {
    dir.join(format!("destination{pack}"))
}

/// Where the pack's destination is while it's unavailable.
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
        Ok(_) if super::space::allocated(path) < valid.len() as u64 => OnDisk::Sparse,
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
        let mut sut = Sut { rt, dir, engine: None, sources, names, refusing: Arc::default(), gate: Arc::default(), pending: None };
        sut.start();
        sut
    }

    fn apply(mut sut: Sut, model: &Model, step: Step) -> Sut {
        let engine = sut.engine();
        // A check never raises what a torrent needs (#127): no snapshot from its start until it's
        // settled has it needing more than before, however far the check has got.
        let checked = match &step {
            Step::Check(id) => Some((*id, needs_of(&engine.subscribe().borrow().snapshot, *id), sut.most_needed(*id))),
            _ => None,
        };
        let answer = match &step {
            Step::Add { pack, paused } => {
                let (source, destination) = match pack {
                    Some(k) => (AddSource::TorrentFile(sut.sources[*k].clone()), destination(sut.dir.path(), *k)),
                    None => (AddSource::Magnet(MAGNET.into()), sut.dir.path().join("downloads")),
                };
                // Parsed at the edge, as the service does: one unavailable is refused there.
                match Destination::parse(AbsolutePath::new(destination).unwrap()) {
                    Ok(destination) => match sut.call(engine.add(super::Fetched::local(source), *paused, destination)) {
                        Ok(added) => Answer::Added(added),
                        Err(e) => panic!("adding failed: {e:#}"),
                    },
                    Err(_) => Answer::Err,
                }
            }
            Step::Pause(id) => ok(sut.call(engine.command(*id, TorrentCommand::Pause))),
            Step::Resume(id) => ok(sut.call(engine.command(*id, TorrentCommand::Resume))),
            Step::Check(id) => ok(sut.call(engine.command(*id, TorrentCommand::Check))),
            Step::Remove(id) => ok(sut.call(engine.command(*id, TorrentCommand::Remove))),
            // Waits at the gate, while the next steps run.
            Step::Delete(id) if model.answer == Answer::Pending => {
                let later = engine.command_later(*id, TorrentCommand::Delete);
                sut.pending = Some(sut.rt.spawn(later));
                Answer::Pending
            }
            Step::Delete(id) => ok(sut.call(engine.command(*id, TorrentCommand::Delete))),
            Step::SetPriority { id, file, priority } => {
                ok(sut.call(engine.command(*id, TorrentCommand::SetPriority { files: vec![*file], priority: *priority })))
            }
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
            Step::Refuse(refusing) => {
                let path = refusing.map(|(k, file)| match file {
                    Some(f) => sut.file(k, f),
                    None => sut.file(k, 0).parent().unwrap().to_owned(),
                });
                *sut.refusing.lock().unwrap() = path.into_iter().collect();
                Answer::None
            }
            Step::WriteMine { pack } => {
                write(sut.dir.path(), &sut.file(*pack, 0).with_file_name("mine.txt"), b"mine");
                Answer::None
            }
            Step::HoldTrash => {
                super::trash::hold(&sut.gate, true);
                Answer::None
            }
            Step::ReleaseTrash => {
                super::trash::hold(&sut.gate, false);
                match sut.pending.take() {
                    Some(waiting) => ok(sut.call(async { waiting.await.expect("the Delete's task") })),
                    None => Answer::None,
                }
            }
            Step::WriteLitter { pack } => {
                write(sut.dir.path(), &sut.file(*pack, 0).with_file_name(".DS_Store"), b"finder");
                Answer::None
            }
            // With a Delete waiting, the stop is asked first, and the gate opened only then: the
            // stop must wait for the trashing, whose outcome is saved.
            Step::Restart => {
                let gate = sut.gate.clone();
                sut.rt.block_on(async {
                    let stopping = engine.stop();
                    tokio::pin!(stopping);
                    let stopped = tokio::time::timeout(Duration::from_millis(300), &mut stopping).await.is_ok();
                    super::trash::hold(&gate, false);
                    if !stopped {
                        stopping.await;
                    }
                });
                if let Some(waiting) = sut.pending.take() {
                    let _ = sut.rt.block_on(waiting);
                }
                if model.held {
                    super::trash::hold(&sut.gate, true);
                }
                sut.start();
                Answer::None
            }
            // The actor ends first, so the trashing's outcome is never heard; then what it hasn't
            // taken stays. A gate of its own for the next start.
            Step::Kill => {
                sut.rt.block_on(engine.kill());
                sut.engine = None;
                super::trash::abandon(&sut.gate);
                if let Some(waiting) = sut.pending.take() {
                    let _ = sut.rt.block_on(waiting);
                }
                sut.gate = Arc::default();
                sut.start();
                Answer::None
            }
        };
        assert_eq!(answer, model.answer, "the answer to {step:?}");
        sut.published_is_current();
        // An answer's effect is in what clients read next: which torrents there are, at once.
        let listed: Vec<TorrentId> = sut.rt.block_on(sut.engine().listing()).iter().map(|t| t.id).collect();
        assert_eq!(listed, model.torrents.keys().copied().collect::<Vec<_>>(), "the list right after {step:?}");
        // The model size-checks after every step; the engine on its own time.
        if matches!(
            step,
            Step::DeleteFile { .. }
                | Step::PutBack { .. }
                | Step::Overwrite { .. }
                | Step::DestinationGone { .. }
                | Step::DestinationBack { .. }
        ) {
            sut.size_check_from_now();
        }
        sut.settle(model);
        if let Some((id, before, (most, recording))) = checked {
            recording.abort();
            let most = *most.lock().unwrap();
            assert!(most <= before, "torrent {id} needed {before} before its check and {most} during it");
        }
        sut
    }

    fn teardown(mut sut: Sut, _: Model) {
        // A Delete waiting at the gate ends, so the engine can stop.
        super::trash::hold(&sut.gate, false);
        if let Some(engine) = sut.engine.take() {
            sut.rt.block_on(engine.stop());
        }
    }
}

fn ok<E>(result: Result<(), E>) -> Answer {
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
