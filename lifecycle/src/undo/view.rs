//! What clients see of the torrents as their steps make them (#190, ADR 0008's *Steps and lanes*):
//! one fold, the engine's and the GUI's fake's. Each torrent's row is folded from what it is now
//! through the steps on its lane, in turn:
//!
//! | Step | Phase | Its row |
//! |---|---|---|
//! | a Remove, a Delete | queued, running | not listed |
//! | a Remove, a Delete | aborting | listed as it was, while what it did is rolled back |
//! | a Move to Trash | queued | its files Off |
//! | a Move to Trash | running | as it is (its files are Off already) |
//! | a Move to Trash | aborting | its files back to the priorities they had |
//! | an undo bringing its torrent back | any | listed as it was, until it's back |
//! | an undo of a Move to Trash | any | its files back to the priorities they had |
//! | a command | queued | done already: its lifecycle as the command leaves it (a check due under way), a priority set |
//! | a command | running | as it is |
//!
//! The adapter ([`Torrents`]) says what each torrent is now and what each step kept; the fold
//! decides the rest. A command for a torrent is judged on its row: not listed, it's refused as not
//! in the list.

use std::collections::{BTreeMap, BTreeSet};

use proto::{Priority, TorrentCommand, TorrentInfo, UndoAction};

use super::{Phase, State as Machine, Torrent, What};
use crate::{Event, State};

/// What clients see of a torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown<'a> {
    /// Listed as it was when a step kept it (an undo bringing it back, an aborted Remove or Delete),
    /// until it's back; none: as it is.
    pub was: Option<&'a TorrentInfo>,
    /// Its files' priorities as the steps on its lane make them, where they change them.
    pub priorities: BTreeMap<usize, Priority>,
    /// Its lifecycle as the steps on its lane will leave it, the commands waiting there carried
    /// out: what its status and actions show. None: its row's own.
    pub state: Option<State>,
}

/// A torrent as a step kept it, and the lifecycle it shows meanwhile (none: its row's own).
#[derive(Debug, Clone, PartialEq)]
pub struct Was<'a> {
    pub info: &'a TorrentInfo,
    pub state: Option<State>,
}

/// What an action on a torrent's lane is to its row.
#[derive(Debug, Clone, PartialEq)]
pub enum Acting<'a> {
    /// A Remove or a Delete: off the list; aborted, listed as it `was` (if kept) while it's rolled back.
    Leaves { was: Option<Was<'a>> },
    /// A Move to Trash of the files `asked`, which had the priorities `previous` once it started.
    Trashes { asked: &'a [usize], previous: Vec<(usize, Priority)> },
}

/// What an undo on a torrent's lane is to its row.
#[derive(Debug, Clone, PartialEq)]
pub enum Undoing<'a> {
    /// It brings its torrent back: listed as it `was` (if kept) until it's back.
    BringsBack { was: Option<Was<'a>> },
    /// It puts back a Move to Trash: its files get the priorities `previous`.
    Restores { previous: Vec<(usize, Priority)> },
}

/// A torrent as it is, in the list (not one gone from it nor being deleted).
#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    /// With its lifecycle.
    With(State),
    /// Without one (the GUI's fake has torrents without): its row's own status and actions.
    Without,
}

/// The adapter's side: the torrents as they are, and what its steps (actions `R`, undos of `E`) kept.
pub trait Torrents<R, E> {
    /// The torrents in the list, by id.
    fn ids(&self) -> BTreeSet<Torrent>;
    /// Torrent `id` as it is, if it's listed.
    fn now(&self, id: Torrent) -> Option<Now>;
    /// Torrent `id`'s row as it is, with these `priorities` where they change its files', and the
    /// status and actions `state` gives it (none: its own).
    fn row(&self, id: Torrent, priorities: &BTreeMap<usize, Priority>, state: Option<&State>) -> Option<TorrentInfo>;
    fn acting<'a>(&'a self, action: &'a R) -> Acting<'a>;
    fn undoing<'a>(&'a self, entry: &'a E) -> Undoing<'a>;
}

/// What clients see of torrent `id`, the steps of `machine` on its lane folded in: none while it
/// isn't listed.
pub fn shown<'a, R, E, T: Torrents<R, E>>(torrents: &'a T, machine: &'a Machine<R, E, TorrentCommand>, id: Torrent) -> Option<Shown<'a>> {
    let state = |now| match now {
        Now::With(state) => Some(state),
        Now::Without => None,
    };
    let mut shown = torrents.now(id).map(|now| Shown { was: None, priorities: BTreeMap::new(), state: state(now) });
    let back = |was: Option<Was<'a>>| was.map(|w| Shown { was: Some(w.info), priorities: BTreeMap::new(), state: w.state });
    for step in machine.lanes.get(&id).into_iter().flatten().filter_map(|seq| machine.steps.get(seq)) {
        match (&step.what, step.phase) {
            (What::Action(action), phase) => match (torrents.acting(action), phase) {
                (Acting::Leaves { .. }, Phase::Queued | Phase::Running) => shown = None,
                (Acting::Leaves { was }, Phase::Aborting) => shown = back(was).or(shown),
                (Acting::Trashes { asked, .. }, Phase::Queued) => set(&mut shown, asked.iter().map(|i| (*i, Priority::Off))),
                // Under way: its files are Off already.
                (Acting::Trashes { .. }, Phase::Running) => {}
                (Acting::Trashes { previous, .. }, Phase::Aborting) => set(&mut shown, previous),
            },
            (What::Undo { entry, .. }, _) => match torrents.undoing(entry) {
                // Once it's back the undo is over, and it's shown as it is.
                Undoing::BringsBack { was } => shown = back(was).or(shown),
                Undoing::Restores { previous } => set(&mut shown, previous),
            },
            // Shown done already: its outcome is known. If it fails once its turn comes, the row
            // follows, and its client hears why.
            (What::Command(command), Phase::Queued) => {
                if let Some(shown) = &mut shown {
                    if let Some(mut next) = shown.state.as_ref().and_then(|s| crate::decide(s, &Event::Command(command.action())).ok()) {
                        // A check due is under way as soon as it runs: the adapter hands it to
                        // librqbit then. So it can be cancelled, as it shows.
                        if let crate::Phase::CheckDue { returning } = &next.phase {
                            next.phase = crate::Phase::Checking { returning: returning.clone() };
                        }
                        shown.state = Some(next);
                    }
                    if let TorrentCommand::SetPriority { files, priority } = command {
                        shown.priorities.extend(files.iter().map(|i| (*i, *priority)));
                    }
                }
            }
            (What::Command(_), Phase::Running | Phase::Aborting) => {}
        }
    }
    shown
}

/// Torrent `id`'s row as clients see it, if they do.
pub fn listed<R, E, T: Torrents<R, E>>(torrents: &T, machine: &Machine<R, E, TorrentCommand>, id: Torrent) -> Option<TorrentInfo> {
    let shown = shown(torrents, machine, id)?;
    match shown.was {
        Some(was) => Some(seen_as(was.clone(), shown.state.as_ref())),
        None => torrents.row(id, &shown.priorities, shown.state.as_ref()),
    }
}

/// Every row clients see, by id: the torrents in the list and those an undo is bringing back.
pub fn listing<R, E, T: Torrents<R, E>>(torrents: &T, machine: &Machine<R, E, TorrentCommand>) -> Vec<TorrentInfo> {
    let mut ids = torrents.ids();
    ids.extend(machine.lanes.keys());
    ids.into_iter().filter_map(|id| listed(torrents, machine, id)).collect()
}

/// `info` with the status and actions `state` gives it, if known: a status librqbit isn't in yet
/// has no rates or peers.
pub fn seen_as(mut info: TorrentInfo, state: Option<&State>) -> TorrentInfo {
    if let Some(state) = state {
        let complete = info.content.is_some_and(|c| c.progress_bytes >= c.size_bytes);
        if let Some(status) = state.status(complete).filter(|s| *s != info.status) {
            (info.download_bytes_per_sec, info.upload_bytes_per_sec, info.live_peers) = (0, 0, 0);
            info.status = status;
        }
        info.actions = state.actions();
    }
    info
}

/// Whether `action` takes its torrent off the list (a Remove, a Delete) rather than moving files.
pub fn leaves(action: UndoAction) -> bool {
    match action {
        UndoAction::Remove | UndoAction::Delete => true,
        UndoAction::TrashFiles { .. } => false,
    }
}

fn set(shown: &mut Option<Shown<'_>>, priorities: impl IntoIterator<Item = (usize, Priority)>) {
    if let Some(shown) = shown {
        shown.priorities.extend(priorities);
    }
}
