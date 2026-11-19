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
//! each item: a Delete then waits, off the list and on the undo stack as it came (#190), having moved
//! its first, while the other steps run (other torrents' commands are answered, other torrents'
//! Deletes wait at the gate beside it, the snapshot follows), until a step opens it and the Delete
//! is answered, or the service stops (which waits for it) or is killed (which leaves the rest where
//! it is). ⌘Z on one waiting aborts it: its torrent is listed again at once, and once the gate opens
//! the Delete stops after the item in flight and what it moved goes back. The whole stack is compared. Throughout, no file a Delete moved comes back at its path, unless written
//! there by hand or its torrent is added again: the model asserts it of itself, and the engine
//! must agree with the model. Nothing downloads (no peers), so a torrent's progress is what
//! its checks found on disk.
//!
//! A step holds the checks at a gate of their own, which a check waits at before it reads its
//! first piece: a torrent added, checked or put back meanwhile is Checking until a step opens it, and
//! Cancel check ends one (#88): the torrent is Unchecked (or Missing files, if files were
//! returning), whatever the gate does then, and stays so across a restart. While a torrent is checked the files on disk and the destinations stay as they
//! are, and the service isn't stopped.
//!
//! Undo (#150): a Remove, and a Delete that moved something or took its torrent off the list, can be undone, last first, from a
//! stack the model keeps too. An undo puts back what Delete moved, the same bytes at the same paths,
//! and adds the torrent again under its id, with what librqbit had of it: as at a restart, librqbit
//! trusts what it saved while it can read each file it says it has, and checks them all when one
//! had to be made again, and only while every file is as it was when the torrent was kept aside:
//! one written or deleted by hand since makes it check them all. It's refused, and dropped, when something is
//! at a path it would put back, the trash was emptied, its destination is unavailable, or its
//! torrent is in the list again. What's on top of the stack is compared too. One removed or
//! deleted while checked is checked again in full; one Unchecked comes back so. No undo is asked while checks
//! are held (see `preconditions`).
//!
//! 16 cases per `cargo test`; `PROPTEST_CASES=256 cargo test -p service model` runs more.
//! Failing sequences, shrunk, are saved to `service/proptest-regressions/` and run first after.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{AbsolutePath, AddSource, Added, Priority, Status, TorrentCommand, TorrentId, Unavailable};
use tempfile::TempDir;

use super::machine::Phase;
use super::{Config as EngineConfig, Destination, Engine};
use lifecycle::disk::Seen;

const PACKS: usize = 2;
const FILES: usize = 3;
const NAMES: [&str; FILES] = ["a.bin", "b.bin", "c.bin"];
const PIECE: usize = 16384;
/// A file's size: two pieces.
const FULL: u64 = 2 * PIECE as u64;
const MAGNET: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000aa&tr=http://127.0.0.1:1/announce";
/// It has no name: it goes by its info hash.
const MAGNET_HASH: &str = "00000000000000000000000000000000000000aa";
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
    /// Ends a check under way.
    CancelCheck(TorrentId),
    Remove(TorrentId),
    Delete(TorrentId),
    /// A Remove (or, `delete`, a Delete) of several torrents in one press: one step, one entry on
    /// the stack, which one Undo takes back whole (#235).
    Selection {
        delete: bool,
        ids: Vec<TorrentId>,
    },
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
    /// Checks wait at a gate from now on (before they read), until it opens.
    HoldChecks,
    /// The checks' gate opens: those waiting go on and end.
    ReleaseChecks,
    /// The trash waits at a gate from now on: a Delete is answered once it's opened.
    HoldTrash,
    /// The gate opens: a Delete waiting goes on, and is answered.
    ReleaseTrash,
    /// These, one after another, each as on its own: ⌘Z on a Delete waiting at the gate and what's
    /// asked of its torrent next (#217's third review), a path too rare one step at a time.
    Burst(Vec<Step>),
    /// Set the files Off, then move those on disk to the Trash (#151).
    TrashFiles {
        id: TorrentId,
        files: Vec<usize>,
    },
    /// Undo the last Remove, Delete or Move to Trash.
    Undo,
    /// The trash is emptied: nothing Delete moved can be put back.
    EmptyTrash,
    /// One file the trash holds is taken out of it (by hand, in Finder): the rest of what an Undo of
    /// its Move to Trash moved still goes back.
    EmptyOne {
        pack: usize,
        file: usize,
    },
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
    /// What Delete moved to the trash and is still there, each with a number of its own.
    in_trash: Vec<Trashed>,
    /// The next item's number.
    next_item: usize,
    /// What Undo undoes, oldest first: an entry of the engine's stack is the parts of one press
    /// (`Undo::press`), side by side.
    undo: Vec<Undo>,
    /// The last press given.
    presses: u64,
    /// How many times each file was written or deleted by hand.
    writes: [[u32; FILES]; PACKS],
    /// What the trash refuses: a pack's file, or its folder (`None`).
    refusing: Option<(usize, Option<usize>)>,
    /// Checks are held at their gate.
    checks_held: bool,
    /// The trash is held at its gate; the Deletes waiting there, each of its own pack, in the order
    /// they came.
    held: bool,
    pending: Vec<Pending>,
    /// The torrents of the Deletes that waited, in the order they came, and of those an Undo
    /// aborted, in the order the Undos came: their answers come so.
    came: Vec<TorrentId>,
    aborts: Vec<TorrentId>,
    /// What's asked of a torrent whose aborted Delete waits at the gate: it's listed again, and each
    /// waits its turn behind the rollback (#217's re-review, N1), answered once the gate opens. An
    /// action of them is on the stack, queued; ⌘Z drops it.
    behind: Vec<Behind>,
    /// After a `Step::Burst`, the model after each of its steps, which the engine is held to.
    trail: Vec<Model>,
    /// The files Delete moved to the Trash, by pack: none may come back at its path until it's
    /// written there by hand, or its torrent is added again.
    moved: BTreeSet<(usize, usize)>,
    /// The files a Move to Trash took since the engine started, or that its undo stack names at a
    /// start, by pack: only these are looked for gone from disk while librqbit has them open (#234).
    taken: BTreeSet<(usize, usize)>,
    /// Each pack's folder is on disk; holds `mine.txt`; holds `.DS_Store`.
    folder: [bool; PACKS],
    mine: [bool; PACKS],
    litter: [bool; PACKS],
}

/// An item in the trash: the pack's folder whole, or one of its files, with what each of its files
/// was on disk, and whether Finder's litter went with the folder.
#[derive(Debug, Clone)]
struct Trashed {
    n: usize,
    pack: usize,
    item: Item,
    files: Vec<(usize, OnDisk)>,
    litter: bool,
}

/// What undoes a Remove or a Delete: the torrent `id` as it comes back, after the items numbered
/// `items` go back from the trash. `in_list`: a partial Delete, whose torrent is still listed.
/// `running`: the step of a Delete waiting at the gate, in its place on the stack since its command
/// came (#190), and only that: what undoes it is known once it ends.
#[derive(Debug, Clone)]
struct Undo {
    /// The press it's of: the parts of a selection's have the same one (none yet: 0).
    press: u64,
    running: bool,
    /// An action waiting behind the rollback of an aborted Delete (#217's re-review): which one of
    /// `Model::behind`.
    queued: Option<usize>,
    id: TorrentId,
    delete: bool,
    back: Torrent,
    items: Vec<usize>,
    in_list: bool,
    /// Its pack's files when it was kept aside; none for a magnet.
    kept: Option<Kept>,
    /// A Move to Trash: each file it was asked for, with whether it was Off and the number of the item
    /// that moved it (none: it wasn't moved). The torrent never left the list, so `back` isn't used.
    files: Option<Vec<(usize, bool, Option<usize>)>>,
}

/// Each file of a pack as it is: what's there, and how many times it was written or deleted by
/// hand, which changes it even when it's the same again.
type Kept = [(OnDisk, u32); FILES];

/// A Delete waiting at the trash's gate: torrent `id` of `pack`, what it plans, how far it got.
#[derive(Debug, Clone)]
struct Pending {
    id: TorrentId,
    pack: usize,
    items: Vec<Item>,
    done: usize,
    kept: Vec<Item>,
    /// It moved something.
    moved: bool,
    /// An Undo took its step while it waited (#190): once the gate opens it stops after the item in
    /// flight, and what it moved goes back.
    aborted: bool,
}

/// A step waiting behind a rollback: its number, its torrent, and whether ⌘Z dropped it.
#[derive(Debug, Clone)]
struct Behind {
    n: usize,
    id: TorrentId,
    step: Step,
    dropped: bool,
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
    /// Not answered yet: a Delete waiting at the trash's gate, or an Undo aborting one.
    Pending,
    /// A selection's: whether each torrent did as asked.
    Each(Vec<(TorrentId, bool)>),
    /// The gate opened: the answers of the Deletes that waited, and of the Undos that aborted them,
    /// each in the order they came.
    Released {
        deletes: Vec<Answer>,
        undos: Vec<Answer>,
        /// And of what waited behind their rollbacks.
        behind: Vec<Answer>,
    },
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
    /// A check is under way, held at its gate, with these files returning (missing ones put back).
    checking: Option<BTreeMap<usize, Gone>>,
    /// Its check was cancelled: it's Unchecked until it's checked again.
    unchecked: bool,
    /// What its first Delete kept for Undo.
    stash: Option<Stash>,
    /// The numbers of the items its Deletes moved to the trash.
    trashed: Vec<usize>,
}

/// A pack as its first Delete found it, which its Undo brings back.
#[derive(Debug, Clone, Copy)]
struct Stash {
    /// What librqbit had of each file.
    have: [bool; FILES],
    /// Its files as they were.
    kept: Kept,
    /// A check was under way: librqbit saved nothing of what it found, so the Undo checks it in
    /// full (#195).
    checking: bool,
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
    /// reached and no Delete under way or failed, the files it gets (not those Off) that take no space there yet, whatever it has
    /// of them; `None` when that's nothing. Checks don't change it.
    fn needs(&self, disk: &[OnDisk; FILES]) -> Option<u64> {
        let getting = (0..FILES).filter(|&f| !self.off[f]);
        let left = getting.filter(|&f| matches!(disk[f], OnDisk::Absent | OnDisk::Sparse)).count() as u64 * FULL;
        (self.running && !self.unavailable && !self.deleting && !self.delete_failed && left > 0).then_some(left)
    }

    /// Its status, but while its Delete waits, when it isn't listed (one aborted is, as it was).
    fn status(&self) -> Status {
        if self.unavailable {
            return Status::DestinationUnavailable { reason: folder_missing(self.pack) };
        }
        if self.delete_failed {
            return Status::Error { failure: proto::TorrentFailure::Librqbit { detail: String::new() } };
        }
        if self.checking.is_some() {
            return Status::Checking;
        }
        if self.unchecked && self.missing.is_empty() {
            return Status::Unchecked;
        }
        match (self.missing.is_empty(), self.running, self.complete()) {
            (false, ..) => Status::MissingFiles,
            (true, true, true) => Status::Seeding,
            (true, true, false) => Status::Downloading,
            (true, false, true) => Status::Done { reached: None },
            (true, false, false) => Status::Paused,
        }
    }

    /// Taken out of librqbit and added again: it creates every file it gets that isn't there and
    /// checks them all, once the checks' gate is open, `held` or not; `returning` are the missing
    /// files put back. Those in the Trash for an undo it reads as they went (`away`).
    fn check(&mut self, disk: &mut [OnDisk; FILES], folder: &mut bool, held: bool, returning: BTreeMap<usize, Gone>, away: Away) {
        self.make_absent(disk, folder);
        self.unchecked = false;
        self.missing.clear();
        if held {
            self.checking = Some(returning);
            return;
        }
        self.checked(disk, returning, away);
    }

    /// A check ends: what librqbit has is what's on disk, or in the Trash for an undo (`away`), and
    /// the files put back that it doesn't hold as they were are missing again.
    fn checked(&mut self, disk: &[OnDisk; FILES], returning: BTreeMap<usize, Gone>, away: Away) {
        self.checking = None;
        self.have = read(disk, away);
        self.missing =
            returning.into_iter().filter(|(f, g)| g.had && !self.have[*f]).map(|(f, g)| (f, Gone { back: false, ..g })).collect();
    }

    /// Cancel check: a check under way ends, its work lost. Unchecked, saying Missing files while
    /// the files that were returning are still gone as they were, until it's checked again.
    fn cancel(&mut self) {
        let Some(returning) = self.checking.take() else { return };
        self.unchecked = true;
        self.missing = returning.into_iter().map(|(f, g)| (f, Gone { back: false, ..g })).collect();
    }

    /// librqbit adding the torrent, or restoring it at start, creates every file it gets that
    /// isn't there, empty, never one set Off (#67, #158).
    fn make_absent(&self, disk: &mut [OnDisk; FILES], folder: &mut bool) {
        for (f, on_disk) in disk.iter_mut().enumerate() {
            if !self.off[f] && *on_disk == OnDisk::Absent {
                *on_disk = OnDisk::Sparse;
                // Its storage makes the folder with the file, if a Move to Trash took it (#212).
                *folder = true;
            }
        }
    }

    /// The engine's size check: a file it gets that's gone makes it Missing files; once
    /// every missing file is back it's checked, and those that aren't what they were are missing
    /// again.
    fn size_check(&mut self, disk: &mut [OnDisk; FILES], folder: &mut bool, held: bool, away: Away) {
        // Nothing is known of its files while they're checked, or until they are.
        if self.checking.is_some() || self.unchecked {
            return;
        }
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
        self.check(disk, folder, held, returned, away);
    }
}

impl Model {
    /// The pack's files as they are.
    fn kept(&self, pack: usize) -> Kept {
        std::array::from_fn(|f| (self.disk[pack][f], self.writes[pack][f]))
    }

    fn pack_torrent_ref(&self, pack: usize) -> Option<&Pack> {
        self.torrents.values().find_map(|t| match t {
            Torrent::Pack(p) if p.pack == pack => Some(p),
            _ => None,
        })
    }

    /// Which files of pack `pack` are in the Trash for an undo now: librqbit reads them there.
    fn in_trash_now(&self, pack: usize) -> [bool; FILES] {
        let mut there = [false; FILES];
        for t in self.in_trash.iter().filter(|t| t.pack == pack) {
            for &(f, _) in &t.files {
                there[f] = true;
            }
        }
        there
    }

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
                    checking: None,
                    unchecked: false,
                    stash: None,
                    trashed: Vec::new(),
                };
                p.check(&mut self.disk[pack], &mut self.folder[pack], self.checks_held, BTreeMap::new(), [None; FILES]);
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
        // Remove and Delete of a torrent without files can be undone; one with files, its Remove.
        let removed = self.torrents.get(&id).filter(|t| matches!(step, Step::Remove(_)) || matches!(t, Torrent::Magnet)).cloned();
        let kept_now = match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) => Some(p.stash.map_or_else(|| self.kept(p.pack), |stash| stash.kept)),
            _ => None,
        };
        let away = match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) => self.away(p.pack, &p.off),
            _ => [None; FILES],
        };
        let in_trash = match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) => self.in_trash_now(p.pack),
            _ => [false; FILES],
        };
        let (disk, folder, held) = (&mut self.disk, &mut self.folder, self.checks_held);
        let Some(torrent) = self.torrents.get_mut(&id) else { return Answer::Err };
        let p = match torrent {
            Torrent::Magnet if matches!(step, Step::Remove(_) | Step::Delete(_)) => {
                self.torrents.remove(&id);
                self.undo.push(Undo {
                    press: 0,
                    running: false,
                    queued: None,
                    id,
                    delete: matches!(step, Step::Delete(_)),
                    back: Torrent::Magnet,
                    items: Vec::new(),
                    in_list: false,
                    kept: None,
                    files: None,
                });
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
            // Only a check under way is cancelled.
            Step::CancelCheck(_) if p.checking.is_none() => return Answer::Err,
            Step::CancelCheck(_) => p.cancel(),
            Step::Pause(_) => p.running = false,
            Step::Resume(_) => {
                p.running = true;
                // Missing files, or Unchecked, are checked first.
                if missing || p.unchecked {
                    p.check(&mut disk[pack], &mut folder[pack], held, BTreeMap::new(), away);
                }
            }
            // One being checked already goes on.
            Step::Check(_) if p.checking.is_some() => {}
            Step::Check(_) => p.check(&mut disk[pack], &mut folder[pack], held, BTreeMap::new(), away),
            Step::SetPriority { file, priority, .. } => {
                p.off[*file] = *priority == Priority::Off;
                if *priority == Priority::Off {
                    // Missing, set Off: it's let go of (#244's re-check), librqbit offering none of its
                    // pieces once the torrent runs again; but for one in the Trash, read where it went.
                    if p.missing.remove(file).is_some() && !in_trash[*file] {
                        p.have[*file] = false;
                    }
                } else {
                    // Asked for again after a Move to Trash took it: a check makes it afresh, as for a
                    // file deleted by hand.
                    self.moved.remove(&(pack, *file));
                }
            }
            Step::Remove(_) => {
                self.torrents.remove(&id);
                let back = removed.expect("listed");
                self.undo.push(Undo {
                    press: 0,
                    running: false,
                    queued: None,
                    id,
                    delete: false,
                    back,
                    items: Vec::new(),
                    in_list: false,
                    kept: kept_now,
                    files: None,
                });
            }
            _ => unreachable!("not a command: {step:?}"),
        }
        Answer::Ok
    }

    /// A Remove (or, `delete`, a Delete) of each of `ids` in one press: each as on its own, every part
    /// on the stack one entry. Answered with how each went.
    fn selection(&mut self, delete: bool, ids: &[TorrentId]) -> Answer {
        let each = ids
            .iter()
            .map(|&id| {
                let step = if delete { Step::Delete(id) } else { Step::Remove(id) };
                (id, self.command(id, &step) == Answer::Ok)
            })
            .collect();
        Answer::Each(each)
    }

    /// Delete of pack `pack`'s torrent `id`: its folder whole to the Trash when nothing of the
    /// user's is in it, else each of its files on disk; then the folders left empty go. What the
    /// trash refuses stays: if nothing went, the torrent is as it was; if some did, it waits with
    /// an error. Either way it's refused. With the trash held, the first item goes, and the rest
    /// wait for the gate.
    fn delete(&mut self, id: TorrentId, pack: usize) -> Answer {
        // A folder that isn't there (every file moved to the Trash by a Move to Trash took it) has nothing to go.
        let items: Vec<Item> = if !self.folder[pack] {
            Vec::new()
        } else if !self.mine[pack] {
            vec![Item::Folder]
        } else {
            (0..FILES).filter(|&f| self.disk[pack][f] != OnDisk::Absent).map(Item::File).collect()
        };
        let kept = self.kept(pack);
        if let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) {
            p.stash = p.stash.or(Some(Stash { have: p.have, kept, checking: p.checking.is_some() }));
        }
        let mut pending = Pending { id, pack, items, done: 0, kept: Vec::new(), moved: false, aborted: false };
        if self.held && !pending.items.is_empty() {
            self.take(&mut pending);
            let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { unreachable!() };
            p.deleting = true;
            // On the stack as it came, running.
            let back = Torrent::Pack(p.clone());
            self.undo.push(Undo {
                press: 0,
                running: true,
                queued: None,
                id,
                delete: true,
                back,
                items: Vec::new(),
                in_list: false,
                kept: None,
                files: None,
            });
            self.pending.push(pending);
            self.came.push(id);
            return Answer::Pending;
        }
        while pending.done < pending.items.len() {
            self.take(&mut pending);
        }
        self.finish(pending).0
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
        let n = self.next_item;
        self.next_item += 1;
        let litter = item == Item::Folder && self.litter[pack];
        self.in_trash.push(Trashed { n, pack, item, files: files.iter().map(|&f| (f, self.disk[pack][f])).collect(), litter });
        if let Some(Torrent::Pack(p)) = self.torrents.get_mut(&pending.id) {
            p.trashed.push(n);
        }
        pending.moved = true;
        for f in files {
            self.moved.insert((pack, f));
            self.disk[pack][f] = OnDisk::Absent;
        }
        if item == Item::Folder {
            (self.folder[pack], self.litter[pack]) = (false, false);
        }
    }

    /// How the Delete went: all of it, and it's off the list; nothing, and it's as it was; some,
    /// and it waits with an error. Its step is done in its place (or gone, if nothing moved). One
    /// aborted is answered as undone instead, and what it moved goes back at once: the Undo's
    /// answer, second.
    fn finish(&mut self, pending: Pending) -> (Answer, Option<Answer>) {
        let Pending { id, items, kept, moved, aborted, .. } = pending;
        let held = self.checks_held;
        let away = match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) => self.away(p.pack, &p.off),
            _ => [None; FILES],
        };
        let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { unreachable!() };
        p.deleting = false;
        if kept.len() < items.len() {
            p.delete_failed = true;
            p.missing.clear();
            // Its check was halted with it, and nobody asked for a new one.
            p.checking = None;
        } else if let Some(returning) = p.checking.take() {
            // Nothing moved: its check, halted, goes again, added again, which applies the
            // priorities held (#185) and so makes a file set back to Normal meanwhile (#210).
            // With the gate open it ends; held, it waits there.
            p.check(&mut self.disk[p.pack], &mut self.folder[p.pack], held, returning, away);
        }
        if !p.delete_failed {
            p.stash = None;
        }
        let gone = kept.is_empty();
        let entry = if moved || gone { self.deleted(id, gone) } else { None };
        if gone {
            self.torrents.remove(&id);
        }
        if aborted {
            let undone = entry.map_or(Answer::Ok, |entry| self.undo_entry(entry));
            return (Answer::Err, Some(undone));
        }
        self.place(id, entry);
        (if gone { Answer::Ok } else { Answer::Err }, None)
    }

    /// Torrent `id`'s Delete ended: `entry` takes the place of its step, running since it came, or
    /// (none was) goes on top; nothing to undo, the step goes.
    fn place(&mut self, id: TorrentId, entry: Option<Undo>) {
        let at = self.undo.iter().position(|u| u.running && u.id == id);
        match (at, entry) {
            (Some(at), Some(entry)) => self.undo[at] = Undo { press: self.undo[at].press, ..entry },
            (Some(at), None) => drop(self.undo.remove(at)),
            (None, Some(entry)) => self.undo.push(entry),
            (None, None) => {}
        }
    }

    /// What undoes torrent `id`'s Delete, which moved something (or, `gone`, all of it): every item
    /// its Deletes moved goes back, and it comes back as it was before the first. It takes the
    /// place of the partial Delete before it.
    fn deleted(&mut self, id: TorrentId, gone: bool) -> Option<Undo> {
        let Some(Torrent::Pack(p)) = self.torrents.get(&id) else { unreachable!() };
        // Off the list it changed something, with nothing moved too (#190).
        if p.trashed.is_empty() && !gone {
            return None;
        }
        let kept = p.stash.map_or_else(|| self.kept(p.pack), |stash| stash.kept);
        let undo = Undo {
            press: 0,
            running: false,
            queued: None,
            id,
            delete: true,
            back: Torrent::Pack(before_delete(p)),
            items: p.trashed.clone(),
            in_list: !gone,
            kept: Some(kept),
            files: None,
        };
        self.undo.retain(|old| old.running || !(old.delete && old.id == id));
        Some(undo)
    }

    /// Move to Trash of `files` of torrent `id` (#151): they're set Off first, which is what keeps the
    /// torrent from Missing files, then each that's on disk goes to the trash, unless the trash
    /// refuses it (the torrent is Off all the same, and the others went); then its folder goes if
    /// nothing of the torrent's or the user's is left in it. Refused as a priority is, and while
    /// its destination is unavailable. What moved is one entry on the undo stack.
    fn trash_files(&mut self, id: TorrentId, files: &[usize]) -> Answer {
        let in_trash = match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) => self.in_trash_now(p.pack),
            _ => [false; FILES],
        };
        let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { return Answer::Err };
        if p.deleting || p.delete_failed || p.unavailable {
            return Answer::Err;
        }
        let pack = p.pack;
        let previous: Vec<(usize, bool)> = files.iter().map(|&f| (f, p.off[f])).collect();
        let mut moved_as: BTreeMap<usize, usize> = BTreeMap::new();
        for &f in files {
            p.off[f] = true;
            // Missing, set Off: let go of, as by a priority.
            if p.missing.remove(&f).is_some() && !in_trash[f] {
                p.have[f] = false;
            }
        }
        let mut items = Vec::new();
        let mut kept = 0;
        for &f in files {
            if self.disk[pack][f] == OnDisk::Absent {
                continue;
            }
            if self.refusing == Some((pack, Some(f))) {
                kept += 1;
                continue;
            }
            let n = self.next_item;
            self.next_item += 1;
            self.in_trash.push(Trashed { n, pack, item: Item::File(f), files: vec![(f, self.disk[pack][f])], litter: false });
            self.moved.insert((pack, f));
            self.taken.insert((pack, f));
            self.disk[pack][f] = OnDisk::Absent;
            items.push(n);
            moved_as.insert(f, n);
        }
        // Left with only what the torrent had and Finder's litter, the folder goes.
        if self.folder[pack] && !self.mine[pack] && self.disk[pack].iter().all(|d| *d == OnDisk::Absent) {
            (self.folder[pack], self.litter[pack]) = (false, false);
        }
        if !items.is_empty() {
            let Some(Torrent::Pack(p)) = self.torrents.get_mut(&id) else { unreachable!() };
            let back = Torrent::Pack(whole(p));
            let files = previous.into_iter().map(|(f, off)| (f, off, moved_as.get(&f).copied())).collect();
            self.undo.push(Undo {
                press: 0,
                running: false,
                queued: None,
                id,
                delete: false,
                back,
                items,
                in_list: true,
                kept: None,
                files: Some(files),
            });
        }
        if kept > 0 { Answer::Err } else { Answer::Ok }
    }

    /// Undo: the top step, at once (#190): a Delete waiting at the gate is aborted, and answered once
    /// it opens; else the last Remove or Delete reversed, or refused (and dropped) as nothing moves.
    /// A selection's comes back whole, each part on its own: done if any part came back (#235).
    fn undo(&mut self) -> Answer {
        let Some(undo) = self.undo.pop() else { return Answer::Err };
        let mut parts = vec![undo];
        while !parts[0].running && parts[0].queued.is_none() && self.undo.last().is_some_and(|u| u.press == parts[0].press) {
            parts.push(self.undo.pop().expect("just found"));
        }
        if parts.len() > 1 {
            let back: Vec<Answer> = parts.into_iter().map(|part| self.undo_entry(part)).collect();
            return if back.contains(&Answer::Ok) { Answer::Ok } else { Answer::Err };
        }
        let undo = parts.pop().expect("one");
        // An action waiting behind a rollback: dropped before it started.
        if let Some(n) = undo.queued {
            if let Some(b) = self.behind.iter_mut().find(|b| b.n == n) {
                b.dropped = true;
            }
            return Answer::Ok;
        }
        if undo.running {
            let pending = self.pending.iter_mut().find(|p| p.id == undo.id).expect("a running step's Delete waits");
            pending.aborted = true;
            self.aborts.push(undo.id);
            return Answer::Pending;
        }
        self.undo_entry(undo)
    }

    /// `undo` reversed, or refused as nothing moves.
    fn undo_entry(&mut self, undo: Undo) -> Answer {
        if let Some(previous) = &undo.files {
            return self.undo_files(&undo, previous);
        }
        let pack = match &undo.back {
            Torrent::Pack(p) => Some(p.pack),
            Torrent::Magnet => None,
        };
        let listed = self.torrents.iter().find(|(_, t)| match (t, pack) {
            (Torrent::Magnet, None) => true,
            (Torrent::Pack(p), Some(k)) => p.pack == k,
            _ => false,
        });
        // One still in the list comes back with its record as it is now: a Pause since its
        // Delete holds, as the newest thing asked of it.
        let (fits, now) = match (undo.in_list, listed) {
            (false, None) => (true, None),
            (true, Some((&id, Torrent::Pack(p)))) => (id == undo.id && p.delete_failed, Some(Torrent::Pack(before_delete(p)))),
            _ => (false, None),
        };
        let items: Option<Vec<Trashed>> = undo.items.iter().map(|n| self.in_trash.iter().find(|t| t.n == *n).cloned()).collect();
        let free = |t: &Trashed| match t.item {
            Item::Folder => !self.folder[t.pack],
            Item::File(f) => self.disk[t.pack][f] == OnDisk::Absent,
        };
        let Some(items) = items.filter(|items| fits && items.iter().all(free) && pack.is_none_or(|k| !self.unavailable[k])) else {
            return Answer::Err;
        };
        for t in items.iter().rev() {
            for &(f, on_disk) in &t.files {
                self.disk[t.pack][f] = on_disk;
                self.moved.remove(&(t.pack, f));
            }
            self.folder[t.pack] = true;
            if t.litter {
                self.litter[t.pack] = true;
            }
        }
        self.in_trash.retain(|t| !undo.items.contains(&t.n));
        let held = self.checks_held;
        let back = match now.unwrap_or(undo.back) {
            Torrent::Magnet => Torrent::Magnet,
            Torrent::Pack(mut p) => {
                let away = self.away(p.pack, &p.off);
                let writes = self.writes[p.pack];
                let disk = &mut self.disk[p.pack];
                // A check under way when it went is over: it's checked again in full.
                let checking = p.checking.take().is_some();
                // The destination is available now (else the Undo was refused above): one removed
                // while it was gone is added again with it back, and runs if it was running (#202).
                let was_unavailable = std::mem::take(&mut p.unavailable);
                if p.delete_failed {
                    // Its storage touches nothing: librqbit has nothing of it, as after a restart.
                    p.have = [false; FILES];
                } else {
                    // As at a restart: what librqbit saved holds unless a file it had is made again,
                    // and only while every file is as it was when it was kept aside.
                    // Its folder and priorities known, as an add again's, it makes only the files
                    // it gets that aren't there (#67, #158).
                    let made_again = (0..FILES).any(|f| p.have[f] && disk[f] == OnDisk::Absent && !read(disk, away)[f]);
                    let changed = undo.kept.is_some_and(|kept| kept != std::array::from_fn(|f| (disk[f], writes[f])));
                    // Its folder is made with the first file it gets: none, none made.
                    p.make_absent(disk, &mut self.folder[p.pack]);
                    self.moved.retain(|(k, _)| *k != p.pack);
                    // Unchecked, as at a restart: it comes back so, whatever librqbit samples; but
                    // one that said Missing files comes back as any torrent does.
                    p.unchecked &= p.missing.is_empty();
                    p.missing.clear();
                    if checking || was_unavailable || ((made_again || changed) && !p.unchecked) {
                        p.check(disk, &mut self.folder[p.pack], held, BTreeMap::new(), away);
                    }
                }
                Torrent::Pack(p)
            }
        };
        self.torrents.insert(undo.id, back);
        Answer::Ok
    }

    /// Undo of a Move to Trash: its torrent must still be the one listed, with its files as a priority
    /// can be set; each file goes back where it was, nothing is put over anything, and the files
    /// asked about have the priority they had.
    fn undo_files(&mut self, undo: &Undo, previous: &[(usize, bool, Option<usize>)]) -> Answer {
        let listed = matches!(self.torrents.get(&undo.id), Some(Torrent::Pack(p)) if !p.deleting && !p.delete_failed && !p.unavailable);
        // What's still in the trash goes back; refused only when none is.
        let items: Vec<Trashed> = undo.items.iter().filter_map(|n| self.in_trash.iter().find(|t| t.n == *n).cloned()).collect();
        let free = |t: &Trashed| match t.item {
            Item::Folder => !self.folder[t.pack],
            Item::File(f) => self.disk[t.pack][f] == OnDisk::Absent,
        };
        if !listed || items.is_empty() || !items.iter().all(free) {
            return Answer::Err;
        }
        for t in items.iter().rev() {
            for &(f, on_disk) in &t.files {
                self.disk[t.pack][f] = on_disk;
                self.moved.remove(&(t.pack, f));
                // A file back, as one put back by hand: a missing file it was learns it's there.
                self.written(t.pack, f);
            }
            self.folder[t.pack] = true;
        }
        self.in_trash.retain(|t| !undo.items.contains(&t.n));
        let Some(Torrent::Pack(p)) = self.torrents.get_mut(&undo.id) else { unreachable!() };
        // What the user gets: each file put back has what the torrent had of it at the move (#218's
        // S3). A Move to Trash changes nothing of what it has: librqbit reads a file in the Trash
        // as it went, a check too.
        let Torrent::Pack(before) = &undo.back else { unreachable!("a Move to Trash's torrent never leaves") };
        for f in items.iter().flat_map(|t| t.files.iter().map(|(f, _)| *f)) {
            p.have[f] = before.have[f];
        }
        for &(f, off, moved_as) in previous {
            // A file that was moved and didn't come back stays Off.
            if moved_as.is_some_and(|n| !undo.items.contains(&n) || !items.iter().any(|t| t.n == n)) {
                continue;
            }
            p.off[f] = off;
            if off {
                p.missing.remove(&f);
            }
        }
        Answer::Ok
    }

    /// The gate opens: each Delete waiting goes on to its end; one aborted stops after the item in
    /// flight (the rest is kept, interrupted), and what it moved goes back.
    fn release(&mut self) -> Answer {
        self.held = false;
        if self.pending.is_empty() {
            return Answer::None;
        }
        let (mut deletes, mut undos) = (Vec::new(), Vec::new());
        // The Undos are answered in the order they were asked.
        let mut pending = std::mem::take(&mut self.pending);
        let asked: Vec<TorrentId> = std::mem::take(&mut self.aborts);
        pending.sort_by_key(|p| asked.iter().position(|id| *id == p.id).unwrap_or(usize::MAX));
        let came = self.came.clone();
        for mut pending in pending {
            match pending.aborted {
                true => pending.kept.extend(pending.items[pending.done..].iter().copied()),
                false => {
                    while pending.done < pending.items.len() {
                        self.take(&mut pending);
                    }
                }
            }
            let id = pending.id;
            let (deleted, undone) = self.finish(pending);
            deletes.push((came.iter().position(|c| *c == id), deleted));
            undos.extend(undone);
        }
        // The Deletes, in the order they came.
        deletes.sort_by_key(|(at, _)| *at);
        self.came.clear();
        // Then what waited behind the rollbacks, in the order it came.
        let behind = std::mem::take(&mut self.behind).into_iter().map(|b| self.after_rollback(b)).collect();
        Answer::Released { deletes: deletes.into_iter().map(|(_, d)| d).collect(), undos, behind }
    }

    /// What waited behind a rollback runs: dropped by ⌘Z, it's answered as undone; an action's entry
    /// takes its place on the stack.
    fn after_rollback(&mut self, b: Behind) -> Answer {
        let at = self.undo.iter().position(|u| u.queued == Some(b.n));
        if let Some(at) = at {
            self.undo.remove(at);
        }
        if b.dropped {
            return Answer::Err;
        }
        let before = self.undo.len();
        let answer = match &b.step {
            Step::TrashFiles { id, files } => self.trash_files(*id, files),
            step => self.command(b.id, step),
        };
        if let (Some(at), true) = (at, self.undo.len() > before) {
            let entry = self.undo.pop().expect("just pushed");
            self.undo.insert(at.min(self.undo.len()), entry);
        }
        answer
    }

    /// Whether `step` for torrent `id` waits behind its aborted Delete's rollback: it's queued (an
    /// action on the stack too), or refused at once as the engine judges it on what clients see.
    fn queue_behind(&mut self, id: TorrentId, step: &Step) -> Option<Answer> {
        let Some(Torrent::Pack(p)) = self.torrents.get(&id) else { return None };
        if !p.deleting || !self.pending.iter().any(|w| w.id == id && w.aborted) {
            return None;
        }
        if !self.listed(id) {
            return Some(Answer::Err);
        }
        // Refused there as on any partly deleted torrent.
        if p.delete_failed && matches!(step, Step::Resume(_) | Step::SetPriority { .. } | Step::TrashFiles { .. }) {
            return Some(Answer::Err);
        }
        let n = self.behind.iter().map(|b| b.n + 1).max().unwrap_or(0);
        let action = match step {
            Step::Remove(_) => Some((false, Vec::new(), None)),
            Step::Delete(_) => Some((true, Vec::new(), None)),
            Step::TrashFiles { files, .. } => Some((false, vec![0; files.len()], Some(Vec::new()))),
            _ => None,
        };
        if let Some((delete, items, files)) = action {
            let back = Torrent::Pack(p.clone());
            self.undo.push(Undo { press: 0, running: true, queued: Some(n), id, delete, back, items, in_list: true, kept: None, files });
        }
        self.behind.push(Behind { n, id, step: step.clone(), dropped: false });
        Some(Answer::Pending)
    }

    /// Pack `p` (torrent `id`) as clients see it: the Pause or Resume waiting behind its rollback
    /// shown done, as the engine shows them (#190). A priority shows once it's back: until then it's
    /// listed as it was seen, whose Size has no file to take one from.
    fn shown(&self, id: TorrentId, p: &Pack) -> Pack {
        let mut p = p.clone();
        for b in self.behind.iter().filter(|b| b.id == id && !b.dropped) {
            match b.step {
                Step::Pause(_) => p.running = false,
                Step::Resume(_) => p.running = true,
                _ => {}
            }
        }
        p
    }

    /// A stop or a crash drops what waited behind a rollback, and its actions from the stack.
    fn drop_behind(&mut self) {
        self.behind.clear();
        self.undo.retain(|u| u.queued.is_none());
    }

    /// Killed with Deletes waiting: the rest of each stays. At the next start, what of the
    /// torrent's files is on disk is what it left; with none, it's deleted. What undoes one aborted
    /// is returned: its undo goes on at the start, once librqbit has restored its torrents (#190).
    fn kill(&mut self) -> Vec<Undo> {
        self.held = false;
        (self.came, self.aborts) = (Vec::new(), Vec::new());
        self.drop_behind();
        let mut aborted = Vec::new();
        for pending in std::mem::take(&mut self.pending) {
            let left = (0..FILES).any(|f| self.disk[pending.pack][f] != OnDisk::Absent);
            let Some(Torrent::Pack(p)) = self.torrents.get_mut(&pending.id) else { unreachable!() };
            (p.deleting, p.delete_failed) = (false, true);
            p.missing.clear();
            // What it moved can be undone, as recorded while it went.
            let entry = self.deleted(pending.id, !left);
            if !left {
                self.torrents.remove(&pending.id);
            }
            match (pending.aborted, entry) {
                (true, Some(entry)) => aborted.push(entry),
                (true, None) => {}
                (false, entry) => self.place(pending.id, entry),
            }
        }
        aborted
    }

    /// Whether torrent `id` is listed: not while its Delete waits, unless an Undo aborted it.
    fn listed(&self, id: TorrentId) -> bool {
        match self.torrents.get(&id) {
            Some(Torrent::Pack(p)) if p.deleting => {
                self.pending.iter().any(|w| w.id == id && w.aborted)
                    && !self.behind.iter().any(|b| b.id == id && !b.dropped && matches!(b.step, Step::Remove(_) | Step::Delete(_)))
            }
            Some(_) => true,
            None => false,
        }
    }

    /// The written file counts as put back for a torrent missing it.
    /// The file at pack `pack`'s file `file` loses its name, deleted or replaced by hand (a write
    /// lands in a new file, renamed over it). One a Move to Trash took, put back and Off, librqbit
    /// may have open: found gone with no name left, it's let go of as one emptied from the Trash is
    /// (#234). Not one with nothing at its path, nor one still in the Trash for an undo: librqbit
    /// reads that where it went, never what's at its path.
    fn unlinked(&mut self, pack: usize, file: usize) {
        if self.disk[pack][file] != OnDisk::Absent
            && !self.in_trash_now(pack)[file]
            && self.taken.contains(&(pack, file))
            && let Some(p) = self.pack_torrent(pack)
            && p.off[file]
        {
            p.have[file] = false;
        }
    }

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
            p.unchecked = false;
        }
    }

    /// The checks' gate opens: every check held goes on, and ends.
    fn release_checks(&mut self) {
        self.checks_held = false;
        let aways = self.aways();
        let Model { disk, torrents, .. } = self;
        for (id, t) in torrents.iter_mut() {
            // One being deleted has its check halted: it's due again once the Delete says how it went.
            if let Torrent::Pack(p) = t
                && !p.deleting
                && let Some(returning) = p.checking.take()
            {
                p.checked(&disk[p.pack], returning, aways[id]);
            }
        }
    }

    /// What librqbit reads where they went of pack `pack`'s files, `off` as they are: those a Move
    /// to Trash took that are still in the Trash for its undo, while they're Off (#218's S1). One set
    /// Normal again is made afresh at its path by the next check, as one deleted by hand; one with a
    /// file written at its path by hand is that file, which librqbit opens there once it's added
    /// again (checked, say).
    fn away(&self, pack: usize, off: &[bool; FILES]) -> Away {
        let mut away = [None; FILES];
        let empty = |f: usize| self.disk[pack][f] == OnDisk::Absent;
        for u in self.undo.iter().filter(|u| u.files.is_some()) {
            for t in self.in_trash.iter().filter(|t| t.pack == pack && u.items.contains(&t.n)) {
                for &(f, on_disk) in t.files.iter().filter(|(f, _)| off[*f] && empty(*f)) {
                    away[f] = Some(on_disk);
                }
            }
        }
        away
    }

    /// `gone`, emptied from the Trash (#234): a file of them a torrent has Off and isn't on disk is
    /// one librqbit doesn't have any more. Read where it went while it was there, it's let go of,
    /// and its torrent added back without its pieces; one it couldn't read already had none since
    /// its last check (but for one set Off unread, a move's whose entry left the stack before a
    /// restart: librqbit had it open, and lets go of it too). A torrent librqbit can't let go of now
    /// (checking, out of reach, partly deleted) finds it gone when it's checked next.
    fn let_go(&mut self, gone: &[Trashed]) {
        for t in gone {
            for &(f, _) in &t.files {
                let not_there = self.disk[t.pack][f] != OnDisk::Valid;
                for torrent in self.torrents.values_mut() {
                    if let Torrent::Pack(p) = torrent
                        && p.pack == t.pack
                        && p.off[f]
                        && not_there
                    {
                        p.have[f] = false;
                    }
                }
            }
        }
    }

    /// [`Model::away`] of each torrent with files, by id.
    fn aways(&self) -> BTreeMap<TorrentId, Away> {
        let packs = self.torrents.iter().filter_map(|(id, t)| match t {
            Torrent::Pack(p) => Some((*id, self.away(p.pack, &p.off))),
            Torrent::Magnet => None,
        });
        packs.collect()
    }

    /// Back, it's added again, so checked, and runs if it was running; one partly deleted only
    /// says so.
    fn destination_back(&mut self, pack: usize) {
        self.unavailable[pack] = false;
        let away = self.pack_torrent_ref(pack).map_or([None; FILES], |p| self.away(pack, &p.off));
        let (disk, folder, held) = (&mut self.disk[pack], &mut self.folder[pack], self.checks_held);
        if let Some(p) = self.torrents.values_mut().find_map(|t| match t {
            Torrent::Pack(p) if p.pack == pack => Some(p),
            _ => None,
        }) {
            p.unavailable = false;
            if !p.delete_failed {
                p.check(disk, folder, held, BTreeMap::new(), away);
            }
        }
    }

    fn restart(&mut self) {
        // Torrents fetching info don't survive it.
        self.torrents.retain(|_, t| matches!(t, Torrent::Pack(_)));
        // Only what the undo stack names is looked for after it.
        let named: BTreeSet<(usize, usize)> = self.in_trash.iter().flat_map(|t| t.files.iter().map(move |(f, _)| (t.pack, *f))).collect();
        self.taken.retain(|k| named.contains(k));
        let aways = self.aways();
        for (id, t) in self.torrents.iter_mut() {
            let Torrent::Pack(p) = t else { continue };
            // Away, or partly in the Trash (its error comes back with it): nothing is known of what
            // it has until it's back and checked.
            if p.unavailable || p.delete_failed {
                p.have = [false; FILES];
                continue;
            }
            // Missing files or not checked, it isn't given to librqbit at the start (#45) until it's
            // checked or Ready again: nothing makes a missing file again. One not checked that says
            // Missing files comes back so, as any such does, with nothing known of what it has.
            if p.unchecked && !p.missing.is_empty() {
                p.have = [false; FILES];
            } else {
                // Those in the Trash for an undo, where they went. librqbit trusts the pieces it saved
                // while it can read each file it says it has, and checks them all once it can't (a
                // missing file too, once it's Ready again): a file it didn't have, put back by hand
                // since, it still doesn't have (#242). Of the files at their paths, `restorable` lets
                // a restart come only when what it has is what's there, so this tells only for the
                // files in the Trash and those missing.
                let read = read(&self.disk[p.pack], aways[id]);
                let checked = (0..FILES).any(|f| p.have[f] && !read[f]);
                if checked {
                    p.have = read;
                }
            }
            // What's at a missing file's path at start isn't taken for the file put back.
            p.missing.values_mut().for_each(|g| g.back = false);
            p.unchecked &= p.missing.is_empty();
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
            .filter(|(id, _)| self.listed(**id))
            .map(|(&id, t)| match t {
                Torrent::Magnet => Listed {
                    id,
                    status: Status::FetchingInfo,
                    progress_bytes: 0,
                    files: Vec::new(),
                    unchecked_progress: Vec::new(),
                    off_had: Vec::new(),
                },
                Torrent::Pack(p) => {
                    let p = &self.shown(id, p);
                    let counted = |f: usize| !p.off[f] && p.have[f] && !p.missing.contains_key(&f);
                    // How far a check has got isn't the model's to say; nothing is known of one not checked.
                    let progress_bytes = match p.status() {
                        _ if p.unchecked => 0,
                        Status::Checking => 0,
                        _ => (0..FILES).filter(|&f| counted(f)).count() as u64 * FULL,
                    };
                    // Listed again as it was while its aborted Delete is undone: its details come once it's back.
                    let files = match p.deleting {
                        true => Vec::new(),
                        false => (0..FILES)
                            .map(|f| (if p.off[f] { Priority::Off } else { Priority::Normal }, p.missing.contains_key(&f)))
                            .collect(),
                    };
                    let off_had = match offers(&p.status()) && !files.is_empty() {
                        true => (0..FILES).filter(|&f| p.off[f]).map(|f| p.have[f]).collect(),
                        false => Vec::new(),
                    };
                    Listed {
                        id,
                        status: p.status(),
                        progress_bytes,
                        files,
                        unchecked_progress: if p.unchecked && !p.deleting { vec![0; FILES] } else { Vec::new() },
                        off_had,
                    }
                }
            })
            .collect();
        let mut trashed: Vec<(usize, usize)> = self.in_trash.iter().flat_map(|t| t.files.iter().map(|&(f, _)| (t.pack, f))).collect();
        trashed.sort();
        let parts = self.undo.iter().map(|u| {
            let id = u.id;
            let action = match (u.delete, &u.files) {
                (_, Some(_)) => proto::UndoAction::TrashFiles { count: u.items.len() },
                (true, None) => proto::UndoAction::Delete,
                (false, None) => proto::UndoAction::Remove,
            };
            let name = match &u.back {
                Torrent::Pack(p) => format!("pack{}", p.pack),
                Torrent::Magnet => MAGNET_HASH.to_owned(),
            };
            (u.press, proto::Undoable { seq: 0, action, torrents: vec![proto::Named { id, name }] })
        });
        // A press's parts are one entry, by torrent id; a Delete if any part is one.
        let mut undo: Vec<(u64, proto::Undoable)> = Vec::new();
        for (press, part) in parts {
            match undo.last_mut() {
                Some((last, entry)) if *last == press => {
                    if part.action == proto::UndoAction::Delete {
                        entry.action = part.action;
                    }
                    entry.torrents.extend(part.torrents);
                    entry.torrents.sort_by_key(|t| t.id);
                }
                Some(_) | None => undo.push((press, part)),
            }
        }
        let undo = undo.into_iter().map(|(_, u)| u).collect();
        View { torrents, needs, disk: self.disk, trashed, folder: self.folder, mine: self.mine, litter: self.litter, undo }
    }
}

/// What librqbit reads of each of a pack's files where it went to the Trash ([`Model::away`]).
type Away = [Option<OnDisk>; FILES];

/// What librqbit reads of a pack's files: each at its path, as `disk` says, or where it went (`away`).
fn read(disk: &[OnDisk; FILES], away: Away) -> [bool; FILES] {
    std::array::from_fn(|f| disk[f] == OnDisk::Valid || away[f] == Some(OnDisk::Valid))
}

/// The pack as it was before its first Delete: nothing of it in the trash.
fn whole(p: &Pack) -> Pack {
    Pack { delete_failed: false, deleting: false, stash: None, trashed: Vec::new(), ..p.clone() }
}

/// What brings back the pack its Deletes moved files of: whole again, with what librqbit had when
/// the first started (checking, if it was then), and its record (what's wanted, its priorities) as
/// it is.
fn before_delete(p: &Pack) -> Pack {
    // Partly deleted or deleted, it's never being checked now: only the stash says it was.
    let have = p.stash.map_or(p.have, |stash| stash.have);
    let checking = p.stash.is_some_and(|stash| stash.checking).then(BTreeMap::new);
    Pack { have, checking, missing: BTreeMap::new(), unavailable: false, ..whole(p) }
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
    /// The undo stack, oldest first: running steps and done ones.
    undo: Vec<proto::Undoable>,
}

#[derive(Debug, PartialEq)]
struct Listed {
    id: TorrentId,
    status: Status,
    progress_bytes: u64,
    /// Each file's priority and whether it's marked missing, by index.
    files: Vec<(Priority, bool)>,
    /// What the pane shows of each file's progress, of a torrent not checked: nothing is known of
    /// any (whether it says Missing files or not); not compared of the others.
    unchecked_progress: Vec<u64>,
    /// Whether librqbit has each file set Off, in order, of a torrent librqbit runs or holds stopped
    /// as it is (Seeding, Downloading, Done, Paused): it offers those pieces. One gone from the
    /// Trash isn't had any more (#234). Not compared of the others.
    off_had: Vec<bool>,
}

/// Whether a torrent showing `status` is one librqbit runs or holds stopped as it is: what it has
/// of a file set Off is compared.
fn offers(status: &Status) -> bool {
    matches!(status, Status::Seeding | Status::Downloading | Status::Done { .. } | Status::Paused)
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
            in_trash: Vec::new(),
            next_item: 0,
            undo: Vec::new(),
            presses: 0,
            writes: [[0; FILES]; PACKS],
            refusing: None,
            checks_held: false,
            held: false,
            pending: Vec::new(),
            came: Vec::new(),
            aborts: Vec::new(),
            behind: Vec::new(),
            trail: Vec::new(),
            moved: BTreeSet::new(),
            taken: BTreeSet::new(),
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
        // Two listed torrents make a selection possible: adds come more often until then.
        let few = model.torrents.keys().filter(|id| model.listed(**id)).count() < 2;
        let mut steps: Vec<(u32, BoxedStrategy<Step>)> = vec![
            (
                if few { 12 } else { 3 },
                (proptest::option::weighted(0.85, pack.clone()), any::<bool>())
                    .prop_map(|(pack, paused)| Step::Add { pack, paused })
                    .boxed(),
            ),
            (2, id.clone().prop_map(Step::Pause).boxed()),
            (2, id.clone().prop_map(Step::Resume).boxed()),
            (2, id.clone().prop_map(Step::Check).boxed()),
            (1, id.clone().prop_map(Step::CancelCheck).boxed()),
            (1, id.clone().prop_map(Step::Remove).boxed()),
            (2, id.clone().prop_map(Step::Delete).boxed()),
            (3, (id.clone(), file.clone(), priority).prop_map(|(id, file, priority)| Step::SetPriority { id, file, priority }).boxed()),
            (
                3,
                (id, proptest::sample::subsequence((0..FILES).collect::<Vec<_>>(), 1..=FILES))
                    .prop_map(|(id, files)| Step::TrashFiles { id, files })
                    .boxed(),
            ),
            (3, (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::DeleteFile { pack, file }).boxed()),
            (3, (pack.clone(), file.clone()).prop_map(|(pack, file)| Step::PutBack { pack, file }).boxed()),
            (1, (pack.clone(), file).prop_map(|(pack, file)| Step::Overwrite { pack, file }).boxed()),
            (1, pack.clone().prop_map(|pack| Step::DestinationGone { pack }).boxed()),
            (2, pack.clone().prop_map(|pack| Step::DestinationBack { pack }).boxed()),
            (1, Just(Step::Restart).boxed()),
            (2, proptest::option::of((pack.clone(), proptest::option::of(0..FILES))).prop_map(Step::Refuse).boxed()),
            (2, pack.clone().prop_map(|pack| Step::WriteMine { pack }).boxed()),
            (1, pack.clone().prop_map(|pack| Step::WriteLitter { pack }).boxed()),
            (1, Just(Step::HoldChecks).boxed()),
            (2, Just(Step::ReleaseChecks).boxed()),
            (1, Just(Step::HoldTrash).boxed()),
            (2, Just(Step::ReleaseTrash).boxed()),
            (1, Just(Step::EmptyTrash).boxed()),
        ];
        // A selection's Remove or Delete (#235): two torrents or more, one press.
        let listed: Vec<TorrentId> = model.torrents.keys().copied().filter(|id| model.listed(*id)).collect();
        if listed.len() >= 2 {
            let n = listed.len();
            steps.push((
                8,
                (proptest::sample::subsequence(listed, 2..=n), any::<bool>())
                    .prop_map(|(ids, delete)| Step::Selection { delete, ids })
                    .boxed(),
            ));
        }
        // A file taken out of the trash by hand, in the trash's turn.
        let in_trash: Vec<(usize, usize)> =
            model.in_trash.iter().filter_map(|t| if let Item::File(f) = t.item { Some((t.pack, f)) } else { None }).collect();
        if !in_trash.is_empty() {
            steps.push((2, proptest::sample::select(in_trash).prop_map(|(pack, file)| Step::EmptyOne { pack, file }).boxed()));
        }
        // Undo, more often once there's something to undo.
        steps.push((if model.undo.is_empty() { 1 } else { 5 }, Just(Step::Undo).boxed()));
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
        // The trash held, on the way to all of these.
        if !model.held && !packs.is_empty() {
            steps.push((6, Just(Step::HoldTrash).boxed()));
        }
        // A held Delete that moves nothing: the trash refusing a pack's folder whole.
        if model.held && model.pending.is_empty() {
            steps.push((2, pack.clone().prop_map(|pack| Step::Refuse(Some((pack, None)))).boxed()));
        }
        // A Delete into a held trash going file by file, so one aborted or cut short is partial.
        let whole: Vec<usize> = packs.iter().map(|(_, p)| p.pack).filter(|&k| !model.mine[k] && !model.unavailable[k]).collect();
        if model.held && !whole.is_empty() {
            steps.push((4, proptest::sample::select(whole).prop_map(|pack| Step::WriteMine { pack }).boxed()));
        }
        // Into a held trash, one Delete or several, each of its own torrent (#190).
        let listed_packs: Vec<TorrentId> = packs.iter().filter(|(_, p)| !p.deleting).map(|(id, _)| *id).collect();
        if model.held && !listed_packs.is_empty() {
            steps.push((10, proptest::sample::select(listed_packs.clone()).prop_map(Step::Delete).boxed()));
        }
        // Less often once one is aborted: what's asked of it next is the rarer case.
        if !model.pending.is_empty() {
            let often = if model.pending.iter().any(|p| p.aborted) { 2 } else { 4 };
            steps.push((often, Just(Step::Kill).boxed()));
            steps.push((often, Just(Step::Restart).boxed()));
        }
        // ⌘Z while the Delete on top waits: it's aborted (#190).
        // Or, an action waiting behind a rollback on top, now and then dropped.
        match model.undo.last() {
            Some(u) if u.running && u.queued.is_none() && !model.checks_held => steps.push((30, Just(Step::Undo).boxed())),
            Some(u) if u.queued.is_some() && !model.checks_held => steps.push((2, Just(Step::Undo).boxed())),
            _ => {}
        }
        // The way there: the trash held, and a Delete into it.
        if !model.held && model.pending.is_empty() && !listed_packs.is_empty() {
            let hold_and_delete =
                proptest::sample::select(listed_packs.clone()).prop_map(|id| Step::Burst(vec![Step::HoldTrash, Step::Delete(id)]));
            steps.push((6, hold_and_delete.boxed()));
        }
        // ⌘Z on it, and at once what's asked of its torrent next: mostly a Pause, a priority or a
        // Move to Trash, which show done at once and run once it's back.
        if let Some(u) = model.undo.last()
            && u.running
            && u.queued.is_none()
            && !model.checks_held
        {
            let id = u.id;
            let ask = (0..10usize, 0..FILES).prop_map(move |(n, file)| match n {
                0..=2 => Step::Pause(id),
                3..=5 => Step::SetPriority { id, file, priority: Priority::Off },
                6..=8 => Step::TrashFiles { id, files: vec![file] },
                _ => [Step::Resume(id), Step::Remove(id), Step::Delete(id)][file % 3].clone(),
            });
            let burst = proptest::collection::vec(ask, 1..=3).prop_map(|asked| Step::Burst([vec![Step::Undo], asked].concat()));
            steps.push((30, burst.boxed()));
        }
        // And what's asked of its torrent meanwhile, which waits behind the rollback.
        let aborted: Vec<TorrentId> = model.pending.iter().filter(|p| p.aborted).map(|p| p.id).collect();
        if !aborted.is_empty() {
            // Mostly a Pause, a priority or a Move to Trash: what shows done at once and runs later.
            let asked = (proptest::sample::select(aborted), 0..10usize, 0..FILES).prop_map(|(id, n, file)| match n {
                0..=2 => Step::Pause(id),
                3..=5 => Step::SetPriority { id, file, priority: Priority::Off },
                6..=8 => Step::TrashFiles { id, files: vec![file] },
                _ => [Step::Resume(id), Step::Remove(id), Step::Delete(id)][file % 3].clone(),
            });
            steps.push((60, asked.boxed()));
        }
        // And the gate opening on what waits there.
        if !model.behind.is_empty() {
            steps.push((15, Just(Step::ReleaseTrash).boxed()));
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
        if !beside.is_empty() && model.pending.is_empty() {
            let refuse = (proptest::sample::select(beside), 0..FILES).prop_map(|(pack, f)| Step::Refuse(Some((pack, Some(f)))));
            steps.push((3, refuse.boxed()));
        }
        // A destination that went comes back, which checks the torrent again, one not checked too.
        let away: Vec<usize> = (0..PACKS).filter(|&k| model.unavailable[k]).collect();
        if !away.is_empty() {
            steps.push((5, proptest::sample::select(away).prop_map(|pack| Step::DestinationBack { pack }).boxed()));
        }
        // Then its Delete.
        let primed: Vec<TorrentId> = packs
            .iter()
            .filter(|(_, p)| model.mine[p.pack] && !p.delete_failed && matches!(model.refusing, Some((r, Some(_))) if r == p.pack))
            .map(|(id, _)| *id)
            .collect();
        if !primed.is_empty() && model.pending.is_empty() {
            steps.push((6, proptest::sample::select(primed).prop_map(Step::Delete).boxed()));
        }
        // A check held, cancelled, or one cancelled checked again, and a restart that finds it
        // Unchecked.
        let checking: Vec<TorrentId> = packs.iter().filter(|(_, p)| p.checking.is_some()).map(|(id, _)| *id).collect();
        if !checking.is_empty() {
            steps.push((10, proptest::sample::select(checking.clone()).prop_map(Step::CancelCheck).boxed()));
            // A Delete halts the check, and moves nothing when the trash refuses a pack's folder
            // whole (held at its gate, or not): the check is due again, never Ready.
            let checking_packs: Vec<usize> = packs.iter().filter(|(_, p)| p.checking.is_some()).map(|(_, p)| p.pack).collect();
            steps.push((10, proptest::sample::select(checking_packs).prop_map(|pack| Step::Refuse(Some((pack, None)))).boxed()));
            let refused: Vec<TorrentId> = packs
                .iter()
                .filter(|(_, p)| p.checking.is_some() && !model.mine[p.pack] && model.refusing == Some((p.pack, None)))
                .map(|(id, _)| *id)
                .collect();
            if !refused.is_empty() {
                steps.push((24, proptest::sample::select(refused).prop_map(Step::Delete).boxed()));
            }
            steps.push((3, proptest::sample::select(checking).prop_map(Step::Delete).boxed()));
        }
        let unchecked: Vec<TorrentId> = packs.iter().filter(|(_, p)| p.unchecked).map(|(id, _)| *id).collect();
        if !unchecked.is_empty() {
            let one = proptest::sample::select(unchecked);
            steps.push((3, one.clone().prop_map(Step::Check).boxed()));
            steps.push((3, one.clone().prop_map(Step::Resume).boxed()));
            steps.push((3, one.clone().prop_map(Step::Pause).boxed()));
            steps.push((2, one.clone().prop_map(Step::Delete).boxed()));
            let priority = prop_oneof![Just(Priority::Normal), Just(Priority::Off)];
            steps.push((
                4,
                (one.clone(), 0..FILES, priority).prop_map(|(id, file, priority)| Step::SetPriority { id, file, priority }).boxed(),
            ));
            // Its destination going and coming back checks it again.
            let packs_unchecked: Vec<usize> = packs.iter().filter(|(_, p)| p.unchecked && !p.unavailable).map(|(_, p)| p.pack).collect();
            if !packs_unchecked.is_empty() {
                steps.push((3, proptest::sample::select(packs_unchecked).prop_map(|pack| Step::DestinationGone { pack }).boxed()));
            }
            steps.push((12, Just(Step::ReleaseChecks).boxed()));
            steps.push((12, Just(Step::Restart).boxed()));
        }
        if packs.is_empty() {
            steps.push((4, (0..PACKS, any::<bool>()).prop_map(|(pack, paused)| Step::Add { pack: Some(pack), paused }).boxed()));
        }
        // A file set Off, deleted by hand or not, its torrent checked, which mustn't make it again
        // nor lose it, and set Normal again (#67).
        let ids: Vec<TorrentId> = packs.iter().map(|(id, _)| *id).collect();
        if !ids.is_empty() {
            let off =
                (proptest::sample::select(ids), 0..FILES).prop_map(|(id, file)| Step::SetPriority { id, file, priority: Priority::Off });
            steps.push((3, off.boxed()));
        }
        let off: Vec<(TorrentId, usize)> =
            packs.iter().flat_map(|(id, p)| (0..FILES).filter(|&f| p.off[f]).map(move |f| (*id, f))).collect();
        if !off.is_empty() {
            let normal = proptest::sample::select(off).prop_map(|(id, file)| Step::SetPriority { id, file, priority: Priority::Normal });
            steps.push((3, normal.boxed()));
        }
        let off_on_disk: Vec<(usize, usize)> = packs
            .iter()
            .map(|(_, p)| *p)
            .filter(|p| !model.unavailable[p.pack])
            .flat_map(|p| (0..FILES).filter(|&f| p.off[f] && model.disk[p.pack][f] != OnDisk::Absent).map(move |f| (p.pack, f)))
            .collect();
        if !off_on_disk.is_empty() {
            steps.push((3, proptest::sample::select(off_on_disk).prop_map(|(pack, file)| Step::DeleteFile { pack, file }).boxed()));
        }
        let with_off: Vec<TorrentId> = packs.iter().filter(|(_, p)| !p.unavailable && p.off.contains(&true)).map(|(id, _)| *id).collect();
        if !with_off.is_empty() {
            steps.push((3, proptest::sample::select(with_off).prop_map(Step::Check).boxed()));
        }
        // Or a restart, which mustn't make it again either (#158).
        if !off_and_gone(model).is_empty() {
            steps.push((3, Just(Step::Restart).boxed()));
        }
        // Or a Remove of its torrent, to be undone: the undo mustn't make it again either.
        let with_off_gone: Vec<TorrentId> = packs
            .iter()
            .filter(|(_, p)| (0..FILES).any(|f| p.off[f] && model.disk[p.pack][f] == OnDisk::Absent))
            .map(|(id, _)| *id)
            .collect();
        if !with_off_gone.is_empty() {
            steps.push((8, proptest::sample::select(with_off_gone).prop_map(Step::Remove).boxed()));
        }
        if !coming_back_off_and_gone(model).is_empty() && !model.held && model.pending.is_empty() && !model.checks_held {
            steps.push((8, Just(Step::Undo).boxed()));
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
        let touches = |pack: usize| model.pending.iter().any(|p| p.pack == pack);
        // A check held reads the files when it goes on: they and the destinations stay as they
        // are meanwhile (but for what a Delete takes, which halts the check), and the service isn't
        // stopped.
        let checking = model.torrents.values().any(|t| matches!(t, Torrent::Pack(p) if p.checking.is_some()));
        if let Step::Burst(steps) = step {
            let mut model = model.clone();
            return steps.iter().all(|step| {
                let ok = <Self as ReferenceStateMachine>::preconditions(&model, step);
                if ok {
                    model = <Self as ReferenceStateMachine>::apply(model.clone(), step);
                }
                ok
            });
        }
        match step {
            Step::DeleteFile { .. }
            | Step::PutBack { .. }
            | Step::Overwrite { .. }
            | Step::DestinationGone { .. }
            | Step::DestinationBack { .. }
            | Step::Restart
            | Step::Kill
                if checking || (model.checks_held && matches!(step, Step::Restart | Step::Kill)) =>
            {
                false
            }
            // Nor while an aborted Delete waits to be rolled back: its rollback adds the torrent back
            // as an Undo does (see Undo below).
            Step::HoldChecks => !model.checks_held && !model.pending.iter().any(|p| p.aborted),
            Step::ReleaseChecks => model.checks_held,
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
            Step::Refuse(_) if !model.pending.is_empty() => false,
            // A torrent whose aborted Delete waits at the gate is listed again, and what's asked of it
            // waits behind the rollback (`queue_behind`), but a check: what it shows of a torrent
            // librqbit is still to restore is librqbit's (as an Undo isn't taken with checks held).
            Step::Check(id) | Step::CancelCheck(id) if model.pending.iter().any(|p| p.id == *id && p.aborted) => false,
            // A Resume that checks first shows a check of a torrent librqbit is still to restore,
            // which is librqbit's too.
            Step::Resume(id)
                if model.pending.iter().any(|p| p.id == *id && p.aborted)
                    && matches!(model.torrents.get(id), Some(Torrent::Pack(p)) if !p.missing.is_empty() || p.unchecked) =>
            {
                false
            }
            // Behind a rollback with the trash held, it moves once the gate opens.
            Step::TrashFiles { id, .. } if model.pending.iter().any(|p| p.id == *id && p.aborted) => true,
            // An add of a torrent whose Delete waits waits for it too.
            Step::Add { pack: Some(k), .. } if touches(*k) => false,
            // The trash's gate would hold it half done, with its torrent's other commands waiting (a
            // race test covers that).
            Step::TrashFiles { .. } => !model.held,
            // The put-back passes the trash's gate too: with it held, only the Delete waiting on top
            // is undone, aborted. And not while checks are held: librqbit reads a piece of each
            // file it says it has of the torrent it adds back, through their gate, and what that
            // finds once the gate opens isn't the model's to follow.
            Step::Undo if model.undo.last().is_some_and(|u| u.running) => !model.checks_held,
            // A selection when nothing waits (the trash's gate, a rollback): each part goes at once.
            Step::Selection { ids, .. } => {
                !model.held
                    && model.pending.is_empty()
                    && model.behind.is_empty()
                    && ids.iter().all(|id| model.listed(*id))
                    && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
            }
            Step::Undo => !model.held && model.pending.is_empty() && !model.checks_held,
            Step::EmptyTrash => !model.held && model.pending.is_empty(),
            Step::EmptyOne { pack, file } => {
                !model.held && model.pending.is_empty() && model.in_trash.iter().any(|t| t.pack == *pack && t.item == Item::File(*file))
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
                !model.pending.is_empty()
                    && restorable(model)
                    && model.torrents.values().all(|t| !matches!(t, Torrent::Pack(p) if !p.missing.is_empty()))
            }
            _ => true,
        }
    }

    fn apply(mut model: Model, step: &Step) -> Model {
        model.trail.clear();
        if let Step::Burst(steps) = step {
            let mut trail = Vec::new();
            for step in steps {
                model = <Self as ReferenceStateMachine>::apply(model, step);
                trail.push(Model { trail: Vec::new(), ..model.clone() });
            }
            model.trail = trail;
            return model;
        }
        // What an aborted Delete's rollback puts back from the Trash, at a stop or a crash.
        let rolled_back: Vec<(usize, usize)> = model
            .pending
            .iter()
            .filter(|p| p.aborted)
            .flat_map(|p| {
                p.items[..p.done].iter().flat_map(move |item| match item {
                    Item::Folder => (0..FILES).map(|f| (p.pack, f)).collect::<Vec<_>>(),
                    Item::File(f) => vec![(p.pack, *f)],
                })
            })
            .collect();
        let off_and_gone: Vec<(usize, usize)> = off_and_gone(&model).into_iter().filter(|g| !rolled_back.contains(g)).collect();
        let coming_back_off_and_gone = match step {
            Step::Undo => coming_back_off_and_gone(&model),
            _ => Vec::new(),
        };
        let behind = match step {
            Step::Pause(id)
            | Step::Resume(id)
            | Step::Remove(id)
            | Step::Delete(id)
            | Step::SetPriority { id, .. }
            | Step::TrashFiles { id, .. } => model.queue_behind(*id, step),
            _ => None,
        };
        model.answer = if let Some(answer) = behind {
            answer
        } else {
            match step {
                // A new torrent: its files may be made again.
                Step::Add { pack: Some(k), paused } => {
                    let answer = model.add(Some(*k), *paused);
                    if matches!(&answer, Answer::Added(a) if !a.already_existed) {
                        model.moved.retain(|(p, _)| p != k);
                    }
                    answer
                }
                Step::Add { pack, paused } => model.add(*pack, *paused),
                Step::Pause(id) | Step::Resume(id) | Step::Check(id) | Step::CancelCheck(id) | Step::Remove(id) | Step::Delete(id) => {
                    model.command(*id, step)
                }
                Step::HoldChecks => {
                    model.checks_held = true;
                    Answer::None
                }
                Step::ReleaseChecks => {
                    model.release_checks();
                    Answer::None
                }
                Step::SetPriority { id, .. } => model.command(*id, step),
                Step::Selection { delete, ids } => model.selection(*delete, ids),
                Step::TrashFiles { id, files } => model.trash_files(*id, files),
                Step::DeleteFile { pack, file } => {
                    let was = model.disk[*pack][*file] != OnDisk::Absent;
                    model.writes[*pack][*file] += u32::from(was);
                    model.unlinked(*pack, *file);
                    model.disk[*pack][*file] = OnDisk::Absent;
                    if let Some(gone) = model.pack_torrent(*pack).and_then(|p| p.missing.get_mut(file)) {
                        gone.back = false;
                    }
                    Answer::None
                }
                // Written by hand, into its folder, made again if it went.
                Step::PutBack { pack, file } => {
                    model.writes[*pack][*file] += 1;
                    model.unlinked(*pack, *file);
                    model.moved.remove(&(*pack, *file));
                    model.disk[*pack][*file] = OnDisk::Valid;
                    model.folder[*pack] = true;
                    model.written(*pack, *file);
                    Answer::None
                }
                Step::Overwrite { pack, file } => {
                    model.writes[*pack][*file] += 1;
                    model.unlinked(*pack, *file);
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
                    // The stop drops what waits behind a rollback first.
                    model.drop_behind();
                    if !model.pending.is_empty() {
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
                Step::Burst(_) => unreachable!("applied step by step above"),
                Step::Kill => {
                    let aborted = model.kill();
                    model.restart();
                    for entry in aborted {
                        model.undo_entry(entry);
                    }
                    Answer::None
                }
                Step::WriteLitter { pack } => {
                    (model.litter[*pack], model.folder[*pack]) = (true, true);
                    Answer::None
                }
                Step::Undo => model.undo(),
                Step::EmptyTrash => {
                    let gone = std::mem::take(&mut model.in_trash);
                    model.let_go(&gone);
                    Answer::None
                }
                Step::EmptyOne { pack, file } => {
                    let (gone, kept) =
                        std::mem::take(&mut model.in_trash).into_iter().partition(|t| t.pack == *pack && t.item == Item::File(*file));
                    model.in_trash = kept;
                    model.let_go(&gone);
                    Answer::None
                }
            }
        };
        // Each entry a step put on the stack is a press of its own; a selection's parts are one.
        let selection = matches!(step, Step::Selection { .. });
        let mut press = None;
        for i in 0..model.undo.len() {
            if model.undo[i].press == 0 {
                let given = match (selection, press) {
                    (true, Some(given)) => given,
                    (true, None) | (false, _) => {
                        model.presses += 1;
                        model.presses
                    }
                };
                press = Some(given);
                model.undo[i].press = given;
            }
        }
        // What Phantom promises of Delete: nothing it moved is made again.
        for &(k, f) in &model.moved {
            assert_eq!(model.disk[k][f], OnDisk::Absent, "file {f} of pack {k}, moved to the Trash, is back after {step:?}");
        }
        // And of a check, any add again, or a restart (#67, #158): a file set Off that isn't there
        // stays gone.
        if matches!(step, Step::Check(_) | Step::Resume(_) | Step::DestinationBack { .. } | Step::Restart | Step::Kill) {
            for (k, f) in off_and_gone {
                assert_eq!(model.disk[k][f], OnDisk::Absent, "file {f} of pack {k}, set Off and gone, is back after {step:?}");
            }
        }
        // Nor of an undo, for the torrent it brings back: but for what it puts back from the trash.
        for (k, f) in coming_back_off_and_gone {
            assert_eq!(model.disk[k][f], OnDisk::Absent, "file {f} of pack {k}, set Off and gone, is back after an undo");
        }
        let aways = model.aways();
        let Model { disk, folder, torrents, checks_held, .. } = &mut model;
        for (id, t) in torrents.iter_mut() {
            if let Torrent::Pack(p) = t
                && !p.unavailable
                && !p.delete_failed
                && !p.deleting
            {
                p.size_check(&mut disk[p.pack], &mut folder[p.pack], *checks_held, aways[id]);
            }
        }
        model
    }
}

/// The files set Off and not on disk of the torrent the next undo brings back, but those it puts
/// back from the trash.
fn coming_back_off_and_gone(model: &Model) -> Vec<(usize, usize)> {
    let Some(press) = model.undo.last().map(|u| u.press) else { return Vec::new() };
    let top = model.undo.iter().rev().take_while(|u| u.press == press);
    top.flat_map(|u| {
        // (A Move to Trash's undo brings back no torrent.)
        let Undo { back: Torrent::Pack(p), items, files: None, .. } = u else { return Vec::new() };
        let put_back: BTreeSet<usize> =
            items.iter().filter_map(|n| model.in_trash.iter().find(|t| t.n == *n)).flat_map(|t| t.files.iter().map(|(f, _)| *f)).collect();
        let disk = &model.disk[p.pack];
        (0..FILES).filter(|&f| p.off[f] && disk[f] == OnDisk::Absent && !put_back.contains(&f)).map(|f| (p.pack, f)).collect()
    })
    .collect()
}

/// The files of the torrents' packs that are set Off and not on disk.
fn off_and_gone(model: &Model) -> Vec<(usize, usize)> {
    let packs = model.torrents.values().filter_map(|t| match t {
        Torrent::Pack(p) => Some(p),
        Torrent::Magnet => None,
    });
    packs.flat_map(|p| (0..FILES).filter(|&f| p.off[f] && model.disk[p.pack][f] == OnDisk::Absent).map(move |f| (p.pack, f))).collect()
}

/// Whether a restart may come: what librqbit has of every torrent is what's on disk (see
/// `preconditions`), but for one whose files are partly in the Trash, which isn't checked, and
/// for a file set Off that's gone: librqbit reads a piece of every file it has some of before it
/// trusts what it saved, and one gone makes it check them all (#158).
fn restorable(model: &Model) -> bool {
    let aborted = |id: &TorrentId| model.pending.iter().any(|w| w.id == *id && w.aborted);
    model.torrents.iter().all(|(id, t)| match t {
        Torrent::Magnet => true,
        // Its Delete aborted, it comes back after the restart from what librqbit had of it then: as
        // for any, what it had must be what its files were (as they were kept aside, before any went),
        // a file Off too unless it was gone: librqbit's sampling reads none of a file it doesn't get,
        // so it never finds one Off changed by hand (#244's long run).
        Torrent::Pack(p) if p.deleting && aborted(id) => p.stash.is_none_or(|stash| {
            let off_and_gone = |f: usize| p.off[f] && stash.kept[f].0 == OnDisk::Absent;
            (0..FILES).filter(|&f| !p.missing.contains_key(&f) && !off_and_gone(f)).all(|f| p.have[f] == (stash.kept[f].0 == OnDisk::Valid))
        }),
        Torrent::Pack(p) if p.delete_failed || p.deleting || p.unchecked => true,
        Torrent::Pack(p) => {
            let disk = &model.disk[p.pack];
            let off_and_gone = |f: usize| p.off[f] && disk[f] == OnDisk::Absent;
            (0..FILES).filter(|&f| !p.missing.contains_key(&f) && !off_and_gone(f)).all(|f| p.have[f] == (disk[f] == OnDisk::Valid))
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
    /// The checks' gate: closed, a check waits before its next read.
    checks: Arc<super::files::Gate>,
    /// The Deletes waiting at the gate, and the Undos aborting them, in the order they came.
    pending: Vec<tokio::task::JoinHandle<Result<(), super::Failure>>>,
    undos: Vec<tokio::task::JoinHandle<Result<proto::Undone, super::Failure>>>,
    /// What waits behind a rollback, answered once the gate opens.
    behind: Vec<tokio::task::JoinHandle<Result<(), super::Failure>>>,
}

/// A step that fails with the gate held still ends: the runtime waits for the trash's thread.
impl Drop for Sut {
    fn drop(&mut self) {
        super::trash::hold(&self.gate, false);
        self.checks.hold(false);
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
            // Often, so a step settles as soon as the engine has looked: the service's are 5 s and
            // `POLL_EVERY`.
            size_check_every: Duration::from_millis(50),
            poll_every: Duration::from_millis(50),
            trash: super::trash::into(self.dir.path().join("trash"), self.refusing.clone(), self.gate.clone()),
            speed_limits: Default::default(),
            seeding: Default::default(),
        };
        let guard = super::files::Guard::default().with_gate(super::files::At::Read, self.checks.clone());
        let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
            Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
        });
        self.engine = Some(self.rt.block_on(Engine::start_with(config, add_into, guard, super::Looks::real())).unwrap());
    }

    /// Runs a client call, which must be answered.
    fn call<T, E>(&self, call: impl Future<Output = Result<T, E>>) -> Result<T, E> {
        self.rt.block_on(async { tokio::time::timeout(SETTLE, call).await.expect("every command is answered") })
    }

    fn view(&self) -> View {
        let engine = self.engine();
        let listing = self.rt.block_on(engine.listing());
        let unchecked: BTreeSet<TorrentId> = self.rt.block_on(
            engine.inspect(|actor| actor.torrents.iter().filter(|(_, t)| t.lifecycle.phase.unchecked()).map(|(id, _)| *id).collect()),
        );
        let torrents = listing
            .into_iter()
            .map(|t| {
                let details = self.rt.block_on(engine.details(t.id));
                let files = match &details {
                    Ok(details) => details.files.iter().map(|f| (f.priority, f.missing)).collect(),
                    Err(_) => Vec::new(),
                };
                let unchecked_progress = match &details {
                    Ok(details) if unchecked.contains(&t.id) => details.files.iter().map(|f| f.progress_bytes).collect(),
                    _ => Vec::new(),
                };
                // How far a check has got isn't the model's to say.
                let progress = match t.status {
                    Status::Checking => 0,
                    _ => t.content.map_or(0, |c| c.progress_bytes),
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
                let off_had = match (&details, offers(&status)) {
                    (Ok(details), true) => {
                        details.files.iter().filter(|f| f.priority == Priority::Off).map(|f| f.progress_bytes == f.size_bytes).collect()
                    }
                    _ => Vec::new(),
                };
                Listed { id: t.id, status, progress_bytes: progress, files, unchecked_progress, off_had }
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
            undo: engine.subscribe().borrow().snapshot.undo.iter().map(|u| proto::Undoable { seq: 0, ..u.clone() }).collect(),
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
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits, if a torrent has pieces to let go of, until it's let go of them, or two size checks
    /// (the looks with them) have passed, as it may not be able to now: what a let-go changes is
    /// then compared every time, not only when the look comes before the next step.
    fn let_go_settled(&self) {
        let state = || {
            self.rt.block_on(self.engine().inspect(|actor| {
                let letting =
                    actor.torrents.values().any(|t| matches!(t.record.letting_go, crate::engine::machine::letgo::State::Gone { .. }));
                (letting, actor.size_checks)
            }))
        };
        let (letting, start) = state();
        if !letting {
            return;
        }
        let deadline = Instant::now() + SETTLE;
        while Instant::now() < deadline {
            let (letting, checks) = state();
            if !letting || checks >= start + 2 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
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
            std::thread::sleep(Duration::from_millis(10));
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
        let mut sut = Sut {
            rt,
            dir,
            engine: None,
            sources,
            names,
            refusing: Arc::default(),
            gate: Arc::default(),
            checks: Arc::default(),
            pending: Vec::new(),
            undos: Vec::new(),
            behind: Vec::new(),
        };
        sut.start();
        sut
    }

    fn apply(mut sut: Sut, model: &Model, step: Step) -> Sut {
        if let Step::Burst(steps) = step {
            for (step, model) in steps.into_iter().zip(&model.trail) {
                sut = <Self as StateMachineTest>::apply(sut, model, step);
            }
            return sut;
        }
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
                        Err(e) => panic!("adding failed: {e:?}"),
                    },
                    Err(_) => Answer::Err,
                }
            }
            // Behind the rollback of an aborted Delete: answered once the gate opens.
            Step::Pause(id)
            | Step::Resume(id)
            | Step::Remove(id)
            | Step::Delete(id)
            | Step::SetPriority { id, .. }
            | Step::TrashFiles { id, .. }
                if model.answer == Answer::Pending && model.behind.iter().any(|b| b.id == *id) =>
            {
                let command = match step.clone() {
                    Step::Pause(_) => TorrentCommand::Pause,
                    Step::Resume(_) => TorrentCommand::Resume,
                    Step::Remove(_) => TorrentCommand::Remove,
                    Step::Delete(_) => TorrentCommand::Delete,
                    Step::SetPriority { file, priority, .. } => TorrentCommand::SetPriority { files: vec![file], priority },
                    Step::TrashFiles { files, .. } => TorrentCommand::TrashFiles { files },
                    _ => unreachable!("matched above"),
                };
                let later = sut.rt.block_on(engine.sent(*id, command));
                sut.behind.push(sut.rt.spawn(later));
                Answer::Pending
            }
            Step::Pause(id) => ok(sut.call(engine.command(*id, TorrentCommand::Pause))),
            Step::Resume(id) => ok(sut.call(engine.command(*id, TorrentCommand::Resume))),
            Step::Check(id) => ok(sut.call(engine.command(*id, TorrentCommand::Check))),
            Step::CancelCheck(id) => ok(sut.call(engine.command(*id, TorrentCommand::CancelCheck))),
            Step::HoldChecks => {
                sut.checks.hold(true);
                Answer::None
            }
            Step::ReleaseChecks => {
                sut.checks.hold(false);
                Answer::None
            }
            Step::Remove(id) => ok(sut.call(engine.command(*id, TorrentCommand::Remove))),
            Step::Selection { delete, ids } => {
                let command = if *delete { TorrentCommand::Delete } else { TorrentCommand::Remove };
                let failed = sut.call(engine.commands(ids.clone(), command)).expect("answered");
                Answer::Each(ids.iter().map(|id| (*id, failed.iter().all(|(f, _)| f != id))).collect())
            }
            // Waits at the gate, while the next steps run.
            Step::Delete(id) if model.answer == Answer::Pending => {
                let later = sut.rt.block_on(engine.sent(*id, TorrentCommand::Delete));
                sut.pending.push(sut.rt.spawn(later));
                Answer::Pending
            }
            Step::Delete(id) => ok(sut.call(engine.command(*id, TorrentCommand::Delete))),
            Step::SetPriority { id, file, priority } => {
                ok(sut.call(engine.command(*id, TorrentCommand::SetPriority { files: vec![*file], priority: *priority })))
            }
            Step::TrashFiles { id, files } => ok(sut.call(engine.command(*id, TorrentCommand::TrashFiles { files: files.clone() }))),
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
                // Refused wherever the destination is: a locked file stays locked as a drive is
                // disconnected and connected again (#207).
                let paths = refusing.iter().flat_map(|&(k, file)| {
                    let (here, away) = (destination(sut.dir.path(), k), moved_away(sut.dir.path(), k));
                    [here, away].map(|dir| match file {
                        Some(f) => dir.join(format!("pack{k}")).join(sut.names[k][f]),
                        None => dir.join(format!("pack{k}")),
                    })
                });
                *sut.refusing.lock().unwrap() = paths.collect();
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
            Step::Burst(_) => unreachable!("carried out step by step above"),
            Step::ReleaseTrash => {
                super::trash::hold(&sut.gate, false);
                if sut.pending.is_empty() {
                    Answer::None
                } else {
                    let deletes = std::mem::take(&mut sut.pending)
                        .into_iter()
                        .map(|waiting| ok(sut.call(async { waiting.await.expect("the Delete's task") })))
                        .collect();
                    let undos = std::mem::take(&mut sut.undos)
                        .into_iter()
                        .map(|waiting| ok(sut.call(async { waiting.await.expect("the Undo's task") }).map(drop)))
                        .collect();
                    let behind = std::mem::take(&mut sut.behind)
                        .into_iter()
                        .map(|waiting| ok(sut.call(async { waiting.await.expect("the command's task") })))
                        .collect();
                    Answer::Released { deletes, undos, behind }
                }
            }
            Step::WriteLitter { pack } => {
                write(sut.dir.path(), &sut.file(*pack, 0).with_file_name(".DS_Store"), b"finder");
                Answer::None
            }
            // Aborting a Delete at the gate: answered once it opens.
            Step::Undo if model.answer == Answer::Pending => {
                let later = sut.rt.block_on(engine.undo_sent());
                sut.undos.push(sut.rt.spawn(later));
                Answer::Pending
            }
            Step::Undo => ok(sut.call(engine.undo()).map(drop)),
            Step::EmptyTrash => {
                let _ = std::fs::remove_dir_all(sut.dir.path().join("trash"));
                Answer::None
            }
            Step::EmptyOne { pack, file } => {
                let (in_pack, name) = (format!("destination{pack}"), sut.names[*pack][*file]);
                for entry in std::fs::read_dir(sut.dir.path().join("trash")).into_iter().flatten().flatten() {
                    let shown = entry.file_name().to_string_lossy().into_owned();
                    let parts: Vec<&str> = shown.split('%').collect();
                    // A file's own item, not a folder with it inside: each, as the model takes every one
                    // (two Deletes of the same pack trash a file twice).
                    if parts.len() >= 3 && parts[parts.len() - 3] == in_pack && parts[parts.len() - 1] == name {
                        std::fs::remove_file(entry.path()).unwrap();
                    }
                }
                Answer::None
            }
            // With a Delete waiting, the stop is asked first, and the gate opened only then: the
            // stop must wait for the trashing, whose outcome is saved.
            Step::Restart => {
                let gate = sut.gate.clone();
                // Nothing waits at the gate: nothing for the stop to wait for.
                let waiting = if sut.pending.is_empty() { Duration::ZERO } else { Duration::from_millis(300) };
                sut.rt.block_on(async {
                    let stopping = engine.stop();
                    tokio::pin!(stopping);
                    let stopped = tokio::time::timeout(waiting, &mut stopping).await.is_ok();
                    super::trash::hold(&gate, false);
                    if !stopped {
                        stopping.await;
                    }
                });
                for waiting in std::mem::take(&mut sut.pending) {
                    let _ = sut.rt.block_on(waiting);
                }
                for waiting in std::mem::take(&mut sut.undos) {
                    let _ = sut.rt.block_on(waiting);
                }
                for waiting in std::mem::take(&mut sut.behind) {
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
                for waiting in std::mem::take(&mut sut.pending) {
                    let _ = sut.rt.block_on(waiting);
                }
                for waiting in std::mem::take(&mut sut.undos) {
                    let _ = sut.rt.block_on(waiting);
                }
                for waiting in std::mem::take(&mut sut.behind) {
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
        let want: Vec<TorrentId> = model.torrents.keys().copied().filter(|id| model.listed(*id)).collect();
        assert_eq!(listed, want, "the list right after {step:?}");
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
        sut.let_go_settled();
        sut.settle(model);
        // The record's priorities are the torrent's (#45): librqbit, whenever it has the torrent and
        // isn't checking it, gets the files the model says; a priority set while it couldn't take
        // one is applied, never dropped. One not checked or away isn't in librqbit: nothing applies
        // a priority to it, nor writes there, until it's checked.
        for (&id, t) in &model.torrents {
            if let Torrent::Pack(p) = t
                && !p.deleting
                && !p.delete_failed
            {
                let normal: BTreeSet<usize> = (0..FILES).filter(|&f| !p.off[f]).collect();
                let librqbit = sut.rt.block_on(sut.engine().inspect(move |actor| {
                    use super::machine::lib::Librqbit;
                    let handle = actor.torrents[&id].lib.torrent()?;
                    let checking = matches!(handle.stats().state, librqbit::TorrentStatsState::Initializing { .. });
                    Some((handle.gets(), checking))
                }));
                let out = matches!(p.status(), Status::Unchecked | Status::DestinationUnavailable { .. });
                // Out of librqbit only away or not checked, missing files since a start (it isn't added
                // until it's checked or Ready again), or as a check takes it out to add it again.
                let may_be_out = out || !p.missing.is_empty() || p.checking.is_some();
                match librqbit {
                    Some(_) if out => panic!("torrent {id} is {:?}, yet in librqbit after {step:?}", p.status()),
                    Some((gets, false)) => assert_eq!(gets, normal, "torrent {id}: the files librqbit gets after {step:?}"),
                    Some((_, true)) => {}
                    None if may_be_out => {}
                    None => panic!("torrent {id} is {:?}, yet out of librqbit after {step:?}", p.status()),
                }
            }
        }
        if let Some((id, before, (most, recording))) = checked {
            recording.abort();
            let most = *most.lock().unwrap();
            assert!(most <= before, "torrent {id} needed {before} before its check and {most} during it");
        }
        sut
    }

    /// Dropped, not stopped: the case is over, and a stop (librqbit's waits a second) is what
    /// Restart checks. A Delete waiting at the gate ends, and the checks held, so the runtime can
    /// end; it ends the engine's tasks with it.
    fn teardown(sut: Sut, _: Model) {
        super::trash::hold(&sut.gate, false);
        sut.checks.hold(false);
        drop(sut);
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

/// Runs `steps` against the model and the engine, as a generated case does, each step meeting
/// its preconditions.
fn replay(steps: &[Step]) {
    let mut model = EngineModel::init_state().new_tree(&mut proptest::test_runner::TestRunner::deterministic()).unwrap().current();
    let mut sut = <EngineModel as StateMachineTest>::init_test(&model);
    for step in steps {
        assert!(EngineModel::preconditions(&model, step), "{step:?} can't be taken here");
        model = <EngineModel as ReferenceStateMachine>::apply(model, step);
        sut = <EngineModel as StateMachineTest>::apply(sut, &model, step.clone());
    }
    <EngineModel as StateMachineTest>::teardown(sut, model);
}

/// N1 of #217's re-review: what's asked of a torrent whose aborted Delete waits at the gate waits
/// behind the rollback, and runs once it's back: a Move to Trash among them, undone in its turn.
#[test]
fn commands_behind_an_aborted_delete_run_once_it_is_back() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Undo,
        Step::Resume(0),
        Step::TrashFiles { id: 0, files: vec![1] },
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::ReleaseTrash,
        Step::Undo,
    ]);
}

/// B1 of #217's fourth review: a Move to Trash during a check held moves at once, and librqbit's
/// check, going on, never resizes the file it moved (it sets each file's length as it ends), so its
/// undo finds the file as it went.
#[test]
fn a_move_to_trash_during_a_check_is_undone_once_the_check_ends() {
    for restart in [false, true] {
        let mut steps = vec![
            Step::Add { pack: Some(0), paused: false },
            Step::HoldChecks,
            Step::Check(0),
            Step::TrashFiles { id: 0, files: vec![1] },
            Step::ReleaseChecks,
        ];
        // What the move recorded is in the undo log, so a restart changes nothing of it.
        if restart {
            steps.push(Step::Restart);
        }
        steps.push(Step::Undo);
        replay(&steps);
    }
}

/// And ⌘Z of an action still waiting there drops it: it never runs, and is answered undone.
#[test]
fn an_action_behind_an_aborted_delete_dropped_by_cmd_z_never_runs() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Undo,
        Step::TrashFiles { id: 0, files: vec![0] },
        Step::Remove(0),
        Step::Undo,
        Step::ReleaseTrash,
    ]);
}

/// A crash or a stop while commands wait behind a rollback drops them; the torrent comes back.
#[test]
fn commands_behind_an_aborted_delete_are_dropped_by_a_stop_or_a_crash() {
    for end in [Step::Kill, Step::Restart] {
        replay(&[
            Step::Add { pack: Some(0), paused: true },
            Step::WriteMine { pack: 0 },
            Step::HoldTrash,
            Step::Delete(0),
            Step::Undo,
            Step::Pause(0),
            Step::Delete(0),
            end,
        ]);
    }
}

/// Two Deletes of the same pack trash its file twice; emptying it from the Trash by hand takes both.
#[test]
fn a_file_trashed_twice_and_emptied_by_hand_is_gone_from_the_trash_both_times() {
    replay(&[
        Step::WriteMine { pack: 1 },
        Step::Add { pack: Some(1), paused: false },
        Step::Delete(0),
        Step::Add { pack: Some(1), paused: false },
        Step::Delete(1),
        Step::EmptyOne { pack: 1, file: 0 },
    ]);
}

/// A Delete aborted after it moved a file set Off, then a crash: the rollback puts the file back from
/// the Trash, as any undo does; nothing else of it is made again.
#[test]
fn a_file_set_off_that_an_aborted_delete_moved_is_put_back_after_a_crash() {
    replay(&[
        Step::Add { pack: Some(1), paused: true },
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::HoldTrash,
        Step::WriteMine { pack: 1 },
        Step::Delete(0),
        Step::Undo,
        Step::Kill,
    ]);
}

/// #185: a refused Delete halts the check under way, which goes again: added again, with the
/// priority held during it applied, so nothing is held once that check is cancelled.
#[test]
fn a_check_a_refused_delete_halted_goes_again_with_the_held_priority() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(1), paused: false },
        Step::Check(0),
        Step::Refuse(Some((1, None))),
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::Delete(0),
        Step::CancelCheck(0),
    ]);
}

/// #195: a Delete halts the check under way, before librqbit saved any pieces; its Undo checks it
/// in full, as one removed while checked.
#[test]
fn the_undo_of_a_delete_made_while_checking_checks_it_in_full() {
    replay(&[Step::HoldChecks, Step::Add { pack: Some(0), paused: false }, Step::Delete(0), Step::ReleaseChecks, Step::Undo]);
}

/// #195: partly deleted while Unchecked, nothing is known of its progress still.
#[test]
fn a_torrent_partly_deleted_while_unchecked_shows_no_progress() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(0), paused: false },
        Step::CancelCheck(0),
        Step::WriteMine { pack: 0 },
        Step::Refuse(Some((0, Some(0)))),
        Step::Delete(0),
    ]);
}

/// #195: the same after a restart, which librqbit checks it at: what it found isn't shown either.
#[test]
fn a_torrent_partly_deleted_while_unchecked_after_a_restart_shows_no_progress() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(0), paused: false },
        Step::CancelCheck(0),
        Step::ReleaseChecks,
        Step::Restart,
        Step::WriteMine { pack: 0 },
        Step::Refuse(Some((0, Some(0)))),
        Step::Delete(0),
    ]);
}

/// #195: and it stays so across a restart, and comes back Unchecked by Undo.
#[test]
fn a_torrent_partly_deleted_while_unchecked_stays_so_across_a_restart() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(0), paused: false },
        Step::CancelCheck(0),
        Step::ReleaseChecks,
        Step::WriteMine { pack: 0 },
        Step::Refuse(Some((0, Some(0)))),
        Step::Delete(0),
        Step::Restart,
        Step::Undo,
    ]);
}

/// #195: removed while partly deleted and Unchecked, its Undo brings it back so.
#[test]
fn the_undo_of_a_remove_of_a_torrent_partly_deleted_while_unchecked_keeps_it_unchecked() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(0), paused: false },
        Step::CancelCheck(0),
        Step::ReleaseChecks,
        Step::WriteMine { pack: 0 },
        Step::Refuse(Some((0, Some(0)))),
        Step::Delete(0),
        Step::Remove(0),
        Step::Undo,
    ]);
}

/// #202: removed while its destination was gone, and undone after it came back: the destination
/// is available when the Undo runs, so it comes back as one added again, not Destination unavailable.
#[test]
fn the_undo_of_a_remove_made_while_the_destination_was_gone_after_it_came_back_is_not_unavailable() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::DestinationGone { pack: 0 },
        Step::Remove(0),
        Step::DestinationBack { pack: 0 },
        Step::Undo,
    ]);
}

/// #199: a file set Off while Unchecked (the priority held) and deleted by hand stays gone when
/// the torrent's Delete is undone (#67, #158). Stands for the issue's sequence, without the
/// fetching-info torrent and `WriteMine` (neither affects the bug).
#[test]
fn the_undo_of_a_delete_never_makes_a_file_set_off_while_unchecked() {
    replay(&[
        Step::HoldChecks,
        Step::Add { pack: Some(0), paused: false },
        Step::CancelCheck(0),
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::DeleteFile { pack: 0, file: 0 },
        Step::ReleaseChecks,
        Step::Delete(0),
        Step::Undo,
    ]);
}

/// #199: the held priorities win over librqbit's own once it has some file set Off. Stands for the
/// review's replay of #201: librqbit already has `b.bin` Off, `a.bin` is only held Off, and Undo
/// must not make `a.bin` (taking librqbit's over the record's would).
#[test]
fn the_undo_of_a_delete_takes_the_held_priorities_over_librqbits() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::SetPriority { id: 0, file: 1, priority: Priority::Off },
        Step::HoldChecks,
        Step::Check(0),
        Step::CancelCheck(0),
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::DeleteFile { pack: 0, file: 0 },
        Step::ReleaseChecks,
        Step::Delete(0),
        Step::Undo,
    ]);
}

/// #207: not an engine bug: the harness armed the refusal by the file's path while the destination
/// was gone (moved away), so once it was back the trash no longer refused it, as the model
/// expected. The refusal now holds at both paths.
#[test]
fn a_delete_after_the_destination_came_back_is_answered_as_the_model_says() {
    replay(&[
        Step::WriteMine { pack: 1 },
        Step::Add { pack: Some(1), paused: false },
        Step::DestinationGone { pack: 1 },
        Step::HoldTrash,
        Step::ReleaseTrash,
        Step::Refuse(Some((1, Some(0)))),
        Step::DestinationBack { pack: 1 },
        Step::Delete(0),
    ]);
}

/// The model's sequence found by review (#179): a file moved to the Trash, set Normal again (it's
/// missing now), then the Move to Trash undone: the file is back, so the torrent isn't missing it.
#[test]
fn a_file_moved_to_the_trash_then_set_normal_is_found_again_by_its_undo() {
    replay(&[
        Step::Add { pack: None, paused: false },
        Step::Add { pack: Some(0), paused: false },
        Step::TrashFiles { id: 1, files: vec![0] },
        Step::SetPriority { id: 1, file: 0, priority: Priority::Normal },
        Step::Undo,
    ]);
}

/// Found by a longer run: every file moved to the Trash, the torrent removed and brought back: it
/// gets no file, so its folder isn't made again.
#[test]
fn a_torrent_with_every_file_in_the_trash_comes_back_without_its_folder() {
    replay(&[Step::Add { pack: Some(0), paused: false }, Step::TrashFiles { id: 0, files: vec![0, 1, 2] }, Step::Remove(0), Step::Undo]);
}

/// Found by a longer run: every file moved to the Trash (which takes the folder), then the torrent
/// deleted: nothing is left to move, but it's off the list, so its undo brings it back (#190, which
/// the engine's restore property found).
#[test]
fn a_delete_after_every_file_went_to_the_trash_has_nothing_to_move() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::TrashFiles { id: 0, files: vec![0, 1, 2] },
        Step::Delete(0),
        Step::Undo,
        Step::Undo,
    ]);
}

/// #212: every file moved to the Trash (which takes the folder), then file 0 set Normal again and
/// the torrent resumed: the engine makes file 0, empty, and its folder with it.
#[test]
fn a_file_set_normal_after_every_file_went_to_the_trash_brings_the_folder_back() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::TrashFiles { id: 0, files: vec![0, 1, 2] },
        Step::SetPriority { id: 0, file: 0, priority: Priority::Normal },
        Step::Resume(0),
    ]);
}

/// #210: file 0, Off and deleted, is set Normal again while a check waits at the gate, so that
/// priority is held; a refused Delete halts the check, which goes again: added again, with the
/// held priorities applied, so file 0 is made, empty. The last Check, nothing to the model, has
/// the engine hear librqbit first, so its halt is heard before the views are compared.
#[test]
fn a_check_a_refused_delete_halted_makes_a_file_set_normal_while_it_was_held() {
    replay(&[
        Step::Add { pack: Some(1), paused: true },
        Step::SetPriority { id: 0, file: 0, priority: Priority::Off },
        Step::HoldChecks,
        Step::DeleteFile { pack: 1, file: 0 },
        Step::Check(0),
        Step::SetPriority { id: 0, file: 0, priority: Priority::Normal },
        Step::Refuse(Some((1, None))),
        Step::Delete(0),
        Step::Check(0),
    ]);
}

/// The user's sequence pressed quickly (#190): Delete A, Delete B, ⌘Z, ⌘Z while both wait at the
/// trash's gate. Each leaves the list as it comes and is back as its ⌘Z comes; once the gate opens
/// both Deletes are answered as undone, and what each moved is back.
#[test]
fn delete_a_delete_b_undo_undo_while_they_wait_brings_both_back() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::Add { pack: Some(1), paused: true },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Delete(1),
        Step::Undo,
        Step::Undo,
        Step::ReleaseTrash,
    ]);
}

/// A Delete going file by file (a file of the user's beside them), aborted after its first: that
/// one is back, the rest never moved, and the torrent is as it was.
#[test]
fn an_undo_of_a_delete_going_file_by_file_puts_back_the_one_that_went() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::WriteMine { pack: 0 },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Undo,
        Step::ReleaseTrash,
    ]);
}

/// A Delete aborted while it waits, then the service stops (which lets the undo finish) or is
/// killed (the undo goes on at the next start).
#[test]
fn a_delete_aborted_while_it_waits_is_undone_across_a_stop_or_a_crash() {
    for end in [Step::Restart, Step::Kill] {
        replay(&[
            Step::Add { pack: Some(0), paused: true },
            Step::Add { pack: Some(1), paused: true },
            Step::WriteMine { pack: 0 },
            Step::HoldTrash,
            Step::Delete(0),
            Step::Delete(1),
            Step::Undo,
            end,
        ]);
    }
}

/// B1 of #217's review: a Delete going file by file, aborted after its first file, then a crash:
/// at the start its rollback is a new undo, checked and taken out as any, not one resumed, and the
/// torrent comes back whole.
#[test]
fn a_partial_delete_aborted_then_killed_comes_back() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::WriteMine { pack: 0 },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Undo,
        Step::Kill,
    ]);
}

/// Found by the 256-case run (#242): a Delete killed at the trash's gate, the pack added again, its
/// file 2 put back by hand while the torrent doesn't have it (nothing checks it), then moved to the
/// Trash, then a restart. librqbit trusts the pieces it saved, as it can read each file it says it
/// has: it still doesn't have file 2, whole as it is where it went. A model that read every file
/// again at a restart says it does.
#[test]
fn a_file_put_back_by_hand_then_moved_to_the_trash_isnt_had_after_a_restart() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::HoldTrash,
        Step::Delete(0),
        Step::Kill,
        Step::Add { pack: Some(0), paused: false },
        Step::PutBack { pack: 0, file: 2 },
        Step::TrashFiles { id: 1, files: vec![2] },
        Step::Restart,
    ]);
}

/// #45's long run: a Delete aborted by Undo, its torrent missing a file, put back and checked, a
/// restart cutting that check short: nothing is recorded of what librqbit had while it checked, so
/// the file librqbit made again isn't judged missing at the next start.
#[test]
fn an_undo_whose_check_a_restart_cuts_short_is_checked_after_it() {
    replay(&[
        Step::Add { pack: None, paused: false },
        Step::HoldTrash,
        Step::Add { pack: Some(1), paused: false },
        Step::DeleteFile { pack: 1, file: 0 },
        Step::Delete(1),
        Step::Undo,
        Step::Restart,
    ]);
}

/// #45's long run: a missing file moved to the Trash (set Off) while the service runs, or after a
/// restart: the torrent runs again with the pieces librqbit has of the others, and those of the file
/// that went as librqbit holds them.
#[test]
fn a_missing_file_moved_to_the_trash_runs_the_torrent_again() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::DeleteFile { pack: 0, file: 2 },
        Step::TrashFiles { id: 0, files: vec![2] },
    ]);
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::DeleteFile { pack: 0, file: 2 },
        Step::Add { pack: None, paused: false },
        Step::Check(1),
        Step::Restart,
        Step::TrashFiles { id: 0, files: vec![2] },
    ]);
}

/// #244's long run: a file a Move to Trash took, set Normal (so missing) and then Off while it's in
/// the Trash, keeps its pieces: librqbit reads it where it went, so nothing is let go of, and the
/// undo puts it back with them.
#[test]
fn a_file_in_the_trash_set_normal_then_off_keeps_its_pieces_for_the_undo() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::Check(0),
        Step::TrashFiles { id: 0, files: vec![2] },
        Step::Remove(0),
        Step::Undo,
        Step::SetPriority { id: 0, file: 2, priority: Priority::Normal },
        Step::SetPriority { id: 0, file: 2, priority: Priority::Off },
        Step::Undo,
    ]);
}

/// #244's long run: the undo of a Move to Trash sets a file it put back Off while the torrent is
/// still Missing files from when it was asked for in the Trash. It's back, so it's not let go of.
#[test]
fn a_file_an_undo_puts_back_and_sets_off_keeps_its_pieces() {
    replay(&[
        Step::Add { pack: Some(0), paused: true },
        Step::Undo,
        Step::SetPriority { id: 0, file: 2, priority: Priority::Off },
        Step::TrashFiles { id: 1, files: vec![1] },
        Step::TrashFiles { id: 1, files: vec![0, 1] },
        Step::EmptyTrash,
        Step::DeleteFile { pack: 0, file: 1 },
        Step::Add { pack: Some(1), paused: false },
        Step::Check(0),
        Step::TrashFiles { id: 0, files: vec![1, 2] },
        Step::Remove(0),
        Step::Undo,
        Step::SetPriority { id: 0, file: 2, priority: Priority::Normal },
        Step::Undo,
    ]);
}

/// #244's long run (as on master): a file a Move to Trash took, with another written at its path by
/// hand, is that one to a check: librqbit, added again, opens it there.
#[test]
fn a_file_in_the_trash_with_another_at_its_path_is_checked_as_that_one() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::TrashFiles { id: 0, files: vec![2] },
        Step::Overwrite { pack: 0, file: 2 },
        Step::Check(0),
    ]);
}

/// #244's long run: a file in the Trash for an undo, with another written at its path and deleted
/// by hand, keeps its pieces: librqbit reads it where it went, so nothing it has open went.
#[test]
fn a_file_written_and_deleted_at_the_path_of_one_in_the_trash_lets_go_of_nothing() {
    replay(&[
        Step::Add { pack: Some(0), paused: false },
        Step::Burst(vec![Step::HoldTrash, Step::Delete(0)]),
        Step::Undo,
        Step::TrashFiles { id: 0, files: vec![2] },
        Step::Remove(0),
        Step::ReleaseTrash,
        Step::Undo,
        Step::Overwrite { pack: 0, file: 2 },
        Step::DeleteFile { pack: 0, file: 2 },
    ]);
}
