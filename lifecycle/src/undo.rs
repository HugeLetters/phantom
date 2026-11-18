//! Undo's machine (#150, #190, [ADR 0008]): one pure machine, as the lifecycle is, holding the undo
//! stack, which decides which step a ⌘Z takes, and a lane per torrent, which decides when steps run.
//!
//! A **step** is an action (a Remove, a Delete or a Move to Trash), an undo, or another command for
//! a torrent. It goes into the lane of every torrent it touches, all at once, as it arrives, so
//! lanes that share steps always agree on their order; it runs once it's first in all of them. An
//! action also goes on the stack as it arrives, in the order the commands came, so which step a ⌘Z
//! takes never depends on timing; only the work overlaps, and only across torrents. One press is
//! one step (#235): a Remove or a Delete of a selection is one action, with a **part** per torrent,
//! one entry on the stack, and one ⌘Z takes all of it back.
//!
//! A step is `Queued`, then `Running` once it's first in all its lanes (the actor starts each
//! part), then over once every part is: an action is `Committed` with what undoes each part that
//! changed something (it stays on the stack: one that failed in part holds what did happen), or
//! `Gone` if none did; an undo or a command just ends. ⌘Z on the action on top: one queued is gone
//! at once; one running is `Aborting` (each part ends after the item in flight, and what they did
//! is rolled back in its place in its lanes); one committed gets its undo, a new step, in the
//! lanes of the torrents it changed. An action rolled back, or whose undo couldn't be carried out,
//! is gone from the stack: no Undo ever waits for another action.
//!
//! The actor does the file moves and reports them, part by part: [`Observed::Moved`] (written
//! ahead), then [`Observed::Finished`] or [`Observed::Failed`]. [`next`] says which parts to start
//! now, and which steps to abort. A stop drops what waits but the undos ([`Observed::Stopping`]);
//! the next start drops it too, after a crash, and the steps that were running are carried out
//! again from their log ([`Observed::Restarted`]). (A crash itself is no event: the machine goes
//! with the process.)
//!
//! | Step | Event | Becomes |
//! |---|---|---|
//! | none (the stack empty) | Undo | refused: Nothing |
//! | an action queued, on top | Undo | gone |
//! | an action running, on top | Undo | aborting, off the stack |
//! | an action committed, on top | Undo | off the stack; its undo arrives, a part per torrent it changed |
//! | any | Arrived | queued in each of its lanes (an action on the stack too, past [`DEPTH`] the oldest off it), running if first in all; an action of no torrent is nothing |
//! | an action's part running or aborting | Moved(r) | that part holding `r` |
//! | an action's part running or aborting | Finished(e), Failed | that part over, with `e` (Failed: none) |
//! | an action running, every part over | | committed with what its parts hold, in its place on the stack, without the parts of older entries they absorb (an entry left with none: absorbed); none: gone (left: dropped) |
//! | an action aborting, every part over | | rolled back: its undo, of what its parts hold, running in its place, under its seq, without the parts of older entries they absorb; none: gone (left: undone) |
//! | an undo's part running | Finished, Refused(why), Failed | that part over: back, refused for `why`, failed |
//! | an undo, every part over | | over; its action left: undone if a part came back (with each part refused, and why: #235), else refused for the first part's `why`, else dropped |
//! | a command running | Finished, Failed | over |
//! | a part queued, over, or not the step's; an undo's or a command's (Moved), an action's or a command's (Refused) | Moved, Finished, Failed, Refused | as it was |
//! | any | Stopping | the actions and commands queued gone |
//! | any | Restarted | the actions and commands queued, and the commands running, gone |
//!
//! After each, every step first in all its lanes is running.
//!
//! **Why an entry left the stack** (#200) is kept as it happens, in `left`, the last
//! [`Left::KEPT`](proto::Left::KEPT), for clients to read: undone (its undo finished, or a ⌘Z took
//! it before it ran or while it ran), refused (its undo was), aged (past [`DEPTH`]), absorbed (a
//! newer entry took its place) or dropped (it changed nothing, or a stop dropped it).
//!
//! [ADR 0008]: ../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::marker::PhantomData;

use proto::{AbsolutePath, CantUndo, Leave, Left};
use serde::{Deserialize, Serialize};

pub mod view;

/// How many actions can be undone: the oldest goes past it.
pub const DEPTH: usize = 20;

/// A torrent, by id: a lane's.
pub type Torrent = u64;

/// What the stack needs of an entry.
pub trait Undoes: Clone + PartialEq {
    /// Whether this entry, done now, takes the place of `older`: what undoing it undoes holds
    /// `older`'s too (the engine's Delete of a torrent whose partial Delete is on the stack).
    fn absorbs(&self, older: &Self) -> bool;
    /// Of this torrent's part, what it undoes and the torrent, as clients name it: what a refusal
    /// of that part says (#235).
    fn named(&self) -> (proto::UndoAction, proto::Named);
}

/// What a step is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum What<R, E, C> {
    /// A Remove, Delete or Move to Trash, a part per torrent it touches: holding `R` while that
    /// part runs (what it did so far), then what undoes it there (none: it changed nothing there).
    Action(BTreeMap<Torrent, Part<R, Option<E>>>),
    /// Putting back what action `of` did, a part per torrent it changed: `E` while that part runs,
    /// then how it went. An Undo of it once committed, or its rollback once aborted.
    Undo { of: u64, parts: BTreeMap<Torrent, Part<E, Undid>> },
    /// Any other command, for one torrent, which waits its turn there and is on no stack.
    Command(C),
}

/// A step's work on one of its torrents: under way, holding `T`, or over, as `O` says.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Part<T, O> {
    Doing(T),
    Over(O),
}

impl<T, O> Part<T, O> {
    /// What it holds while it's under way.
    pub fn doing(&self) -> Option<&T> {
        match self {
            Part::Doing(t) => Some(t),
            Part::Over(_) => None,
        }
    }
}

/// How an undo's part ended.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Undid {
    /// Its torrent, or its files, are back.
    Back,
    /// Refused: that torrent, what it undid of it, and why.
    Refused { not: proto::NotBack },
    /// It couldn't, for no reason of the undo's own.
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    /// An action running whose ⌘Z came: it ends after the item in flight, then is rolled back.
    Aborting,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Step<R, E, C> {
    /// The torrents it touches: it's in each one's lane. An action's or an undo's are its parts'.
    pub torrents: BTreeSet<Torrent>,
    pub what: What<R, E, C>,
    pub phase: Phase,
}

impl<R, E, C> Step<R, E, C> {
    /// Whether its part on torrent `t` is under way: a command's while it runs.
    fn doing(&self, t: Torrent) -> bool {
        let doing = match &self.what {
            What::Action(parts) => matches!(parts.get(&t), Some(Part::Doing(_))),
            What::Undo { parts, .. } => matches!(parts.get(&t), Some(Part::Doing(_))),
            What::Command(_) => self.torrents.contains(&t),
        };
        doing && self.phase != Phase::Queued
    }

    /// Its parts under way, by torrent: none while it's queued.
    pub fn underway(&self) -> impl Iterator<Item = Torrent> + '_ {
        self.torrents.iter().copied().filter(|t| self.doing(*t))
    }
}

/// An action on the undo stack: its step, the torrents it touches and, once committed, what undoes
/// it on each torrent it changed (until then its step is in `steps`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Pressed<E> {
    pub seq: u64,
    pub torrents: BTreeSet<Torrent>,
    pub done: Option<BTreeMap<Torrent, E>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(
    default,
    bound(
        serialize = "R: Serialize, E: Serialize, C: Serialize",
        deserialize = "R: Deserialize<'de>, E: Deserialize<'de>, C: Deserialize<'de>"
    )
)]
pub struct State<R, E, C> {
    /// The actions, oldest first, in the order their commands came.
    pub stack: Vec<Pressed<E>>,
    /// Every step not over, by seq: an action's seq is its place on the stack.
    pub steps: BTreeMap<u64, Step<R, E, C>>,
    /// Each torrent's steps, in the order they came: the first runs.
    pub lanes: BTreeMap<Torrent, VecDeque<u64>>,
    /// The last seq given: none is given twice.
    pub last: u64,
    /// Why the entries that left the stack did, oldest first, the last [`Left::KEPT`].
    pub left: VecDeque<Left>,
}

impl<R, E, C> Default for State<R, E, C> {
    fn default() -> Self {
        Self { stack: Vec::new(), steps: BTreeMap::new(), lanes: BTreeMap::new(), last: 0, left: VecDeque::new() }
    }
}

impl<R, E, C> State<R, E, C> {
    /// What a ⌘Z would take now.
    pub fn top(&self) -> Option<&Pressed<E>> {
        self.stack.last()
    }

    /// The last seq given, or on it: a new step's is larger.
    pub fn last_seq(&self) -> u64 {
        self.stack.iter().map(|p| p.seq).chain(self.steps.keys().copied()).fold(self.last, u64::max)
    }

    /// Whether step `seq` is first in each of its lanes.
    fn first_everywhere(&self, seq: u64) -> bool {
        self.steps.get(&seq).is_some_and(|s| s.torrents.iter().all(|t| self.lanes.get(t).and_then(|l| l.front()) == Some(&seq)))
    }

    /// Step `seq`, in each of its lanes, in turn.
    fn enter(&mut self, seq: u64, step: Step<R, E, C>) {
        for t in &step.torrents {
            self.lanes.entry(*t).or_default().push_back(seq);
        }
        self.steps.insert(seq, step);
    }

    /// Step `seq` is out of the lanes of `torrents`.
    fn out_of(&mut self, seq: u64, torrents: impl IntoIterator<Item = Torrent>) {
        for t in torrents {
            if let Some(lane) = self.lanes.get_mut(&t) {
                lane.retain(|s| *s != seq);
                if lane.is_empty() {
                    self.lanes.remove(&t);
                }
            }
        }
    }

    /// Step `seq` is over: out of its lanes.
    fn leave(&mut self, seq: u64) {
        if let Some(step) = self.steps.remove(&seq) {
            self.out_of(seq, step.torrents);
        }
    }

    /// Every step queued and first in all its lanes runs.
    fn promote(&mut self) {
        let ready: Vec<u64> = self.steps.iter().filter(|(_, s)| s.phase == Phase::Queued).map(|(seq, _)| *seq).collect();
        for seq in ready {
            if self.first_everywhere(seq)
                && let Some(step) = self.steps.get_mut(&seq)
            {
                step.phase = Phase::Running;
            }
        }
    }

    /// Off the stack: action `seq`, which left for `why` (one not on it any more, aged or taken by
    /// a ⌘Z, has said why already).
    fn unstack(&mut self, seq: u64, why: Leave) {
        if self.stack.iter().any(|p| p.seq == seq) {
            self.stack.retain(|p| p.seq != seq);
            self.left(seq, why);
        }
    }

    /// Action `seq` left the stack for `why`.
    fn left(&mut self, seq: u64, why: Leave) {
        self.left.push_back(Left { seq, why });
        while self.left.len() > Left::KEPT {
            self.left.pop_front();
        }
    }
}

impl<R: Clone, E: Undoes, C: Clone> State<R, E, C> {
    /// Action `seq` once each of its parts is over: committed with what undoes them, gone if none
    /// changed anything; aborted, rolled back in its place on the lanes of the torrents it changed.
    fn conclude_action(&mut self, seq: u64) {
        let Some(step) = self.steps.get(&seq) else { return };
        let What::Action(parts) = &step.what else { return };
        let mut entries = BTreeMap::new();
        for (t, part) in parts {
            match part {
                Part::Doing(_) => return,
                Part::Over(Some(entry)) => drop(entries.insert(*t, entry.clone())),
                Part::Over(None) => {}
            }
        }
        let unchanged: Vec<Torrent> = step.torrents.iter().copied().filter(|t| !entries.contains_key(t)).collect();
        match (step.phase, entries.is_empty()) {
            // One a ⌘Z took off the stack is undone; one that changed nothing, there was none to undo.
            (Phase::Aborting, true) => {
                self.leave(seq);
                self.left(seq, Leave::Undone { refused: Vec::new() });
            }
            (Phase::Queued | Phase::Running, true) => {
                self.leave(seq);
                self.unstack(seq, Leave::Dropped);
            }
            // What it did is rolled back whole, what older entries' parts it absorbs too: they leave.
            (Phase::Aborting, false) => {
                self.absorb(seq, &entries);
                self.out_of(seq, unchanged);
                if let Some(step) = self.steps.get_mut(&seq) {
                    step.torrents = entries.keys().copied().collect();
                    let parts = entries.into_iter().map(|(t, e)| (t, Part::Doing(e))).collect();
                    (step.what, step.phase) = (What::Undo { of: seq, parts }, Phase::Running);
                }
            }
            (Phase::Queued | Phase::Running, false) => {
                self.leave(seq);
                self.commit(seq, entries);
            }
        }
    }

    /// Action `seq` is committed with `entries`, in its place on the stack, without the parts of
    /// older entries they absorb.
    fn commit(&mut self, seq: u64, entries: BTreeMap<Torrent, E>) {
        let Some(at) = self.stack.iter().position(|p| p.seq == seq) else { return };
        self.stack[at].torrents = entries.keys().copied().collect();
        self.stack[at].done = Some(entries.clone());
        self.absorb(seq, &entries);
    }

    /// The parts of entries older than action `seq` that its `entries` absorb leave them; an entry
    /// left with none leaves the stack, absorbed by `seq`.
    fn absorb(&mut self, seq: u64, entries: &BTreeMap<Torrent, E>) {
        let mut taken = Vec::new();
        for p in self.stack.iter_mut().filter(|p| p.seq < seq) {
            if let Some(done) = &mut p.done {
                done.retain(|_, old| !entries.values().any(|e| e.absorbs(old)));
                p.torrents = done.keys().copied().collect();
                if done.is_empty() {
                    taken.push(p.seq);
                }
            }
        }
        self.stack.retain(|p| !taken.contains(&p.seq));
        for old in taken {
            self.left(old, Leave::Absorbed { by: seq });
        }
    }

    /// Action `seq`'s part on torrent `t` is over, with what undoes it (none: it changed nothing).
    fn action_part_over(&mut self, seq: u64, t: Torrent, entry: Option<E>) {
        if let Some(Step { what: What::Action(parts), .. }) = self.steps.get_mut(&seq) {
            parts.insert(t, Part::Over(entry));
        }
        self.conclude_action(seq);
    }

    /// Undo step `seq`'s part on torrent `t` is over as `undid` says.
    fn undo_part_over(&mut self, seq: u64, t: Torrent, undid: Undid) {
        if let Some(Step { what: What::Undo { parts, .. }, .. }) = self.steps.get_mut(&seq) {
            parts.insert(t, Part::Over(undid));
        }
        self.conclude_undo(seq);
    }

    /// Undo step `seq` once each of its parts is over: its action left the stack undone if
    /// something came back, with the parts that were refused (#235); else refused, if a part was
    /// (for the first one's reason), or dropped.
    fn conclude_undo(&mut self, seq: u64) {
        let Some(What::Undo { of, parts }) = self.steps.get(&seq).map(|s| &s.what) else { return };
        let (mut back, mut refused) = (false, Vec::new());
        for part in parts.values() {
            match part {
                Part::Doing(_) => return,
                Part::Over(Undid::Back) => back = true,
                Part::Over(Undid::Refused { not }) => refused.push(not.clone()),
                Part::Over(Undid::Failed) => {}
            }
        }
        let why = match (back, refused.first()) {
            (true, _) => Leave::Undone { refused },
            (false, Some(first)) => {
                Leave::Refused { why: first.why.clone(), stranded: refused.iter().flat_map(|r| r.stranded.iter().cloned()).collect() }
            }
            (false, None) => Leave::Dropped,
        };
        let of = *of;
        self.leave(seq);
        self.left(of, why);
    }
}

/// What arrives: an action, on the stack, or another command.
#[derive(Debug, Clone, PartialEq)]
pub enum Arrival<R, C> {
    /// A Remove, Delete or Move to Trash of each of these torrents, with what it holds there as it
    /// comes: one step, one entry on the stack (one press, one step: #235). Of no torrent, it's
    /// nothing.
    Action(BTreeMap<Torrent, R>),
    Command(Torrent, C),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event<R, E, C> {
    Command(Command),
    Observed(Observed<R, E, C>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// ⌘Z, Edit ▸ Undo, `phantom undo`, a notice's Undo.
    Undo,
}

/// What the actor found. A part is step `seq`'s on one torrent; news of one that isn't under way
/// changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Observed<R, E, C> {
    /// A step came, numbered `seq` or, if that isn't past every step's, one past the last (the
    /// newest step says).
    Arrived { seq: u64, what: Arrival<R, C> },
    /// Running action `seq` moved an item of torrent `t`: that part holds `R` now, written ahead.
    Moved(u64, Torrent, R),
    /// Step `seq`'s part on torrent `t` ended: an action's that changed something, with what undoes
    /// it (none: it changed nothing), an undo's carried out, a command done.
    Finished(u64, Torrent, Option<E>),
    /// Step `seq`'s part on torrent `t` couldn't: an action's that changed nothing, an undo's (its
    /// action is gone from the stack all the same), a command refused.
    Failed(u64, Torrent),
    /// Undo step `seq`'s part on `torrent` was refused, for `why`.
    Refused { seq: u64, torrent: Torrent, why: CantUndo, stranded: Vec<AbsolutePath> },
    /// The service is stopping: what waits doesn't start, but the undos, which were pressed.
    Stopping,
    /// The engine starts again, after a stop or a crash: what waited is gone but the undos (nobody
    /// waits for it any more), and the steps that were running are carried out again.
    Restarted,
}

impl<R, E, C> Observed<R, E, C> {
    /// The part it's news of, and whether it's that part's end.
    fn part(&self) -> Option<(u64, Torrent, bool)> {
        match self {
            Observed::Moved(seq, t, _) => Some((*seq, *t, false)),
            Observed::Finished(seq, t, _) | Observed::Failed(seq, t) | Observed::Refused { seq, torrent: t, .. } => Some((*seq, *t, true)),
            Observed::Arrived { .. } | Observed::Stopping | Observed::Restarted => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The stack is empty.
    Nothing,
}

/// What the actor carries out now: the parts it started, and the steps it told to stop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub started: BTreeSet<(u64, Torrent)>,
    pub aborted: BTreeSet<u64>,
}

impl World {
    /// What the actor carries out once `event` made `state`: not a part it reported the end of (so
    /// an aborted action's rollback, under the same seq, starts), nor the stop of a step that's no
    /// longer aborting.
    pub fn follow<R, E, C>(&mut self, event: &Event<R, E, C>, state: &State<R, E, C>) {
        if let Event::Observed(observed) = event
            && let Some((seq, t, true)) = observed.part()
        {
            self.started.remove(&(seq, t));
        }
        self.aborted.retain(|seq| state.steps.get(seq).is_some_and(|s| s.phase == Phase::Aborting));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Start step `seq`'s part on torrent `t`: the step is first in all its lanes.
    Start(u64, Torrent),
    /// Stop action `seq`, each part once the item in flight is done.
    Abort(u64),
}

pub fn decide<R: Clone, E: Undoes, C: Clone>(state: &State<R, E, C>, event: &Event<R, E, C>) -> Result<State<R, E, C>, Refusal> {
    // The step whose part under way an event is news of.
    let target = match event {
        Event::Observed(observed) => observed.part().and_then(|(seq, t, _)| state.steps.get(&seq).filter(|s| s.doing(t))).map(|s| &s.what),
        Event::Command(_) => None,
    };
    let mut next = state.clone();
    match (event, target) {
        (Event::Command(Command::Undo), _) => {
            let top = next.stack.pop().ok_or(Refusal::Nothing)?;
            match (top.done, next.steps.get(&top.seq).map(|s| s.phase)) {
                (Some(entries), _) => {
                    let seq = next.last_seq().max(top.seq) + 1;
                    next.last = seq;
                    let torrents = entries.keys().copied().collect();
                    let parts = entries.into_iter().map(|(t, e)| (t, Part::Doing(e))).collect();
                    next.enter(seq, Step { torrents, what: What::Undo { of: top.seq, parts }, phase: Phase::Queued });
                }
                (None, Some(Phase::Queued)) => {
                    next.leave(top.seq);
                    next.left(top.seq, Leave::Undone { refused: Vec::new() });
                }
                (None, Some(Phase::Running | Phase::Aborting)) => {
                    if let Some(step) = next.steps.get_mut(&top.seq) {
                        step.phase = Phase::Aborting;
                    }
                }
                (None, None) => next.left(top.seq, Leave::Dropped),
            }
        }
        (Event::Observed(Observed::Arrived { seq, what }), _) => {
            let (torrents, what): (BTreeSet<Torrent>, _) = match what {
                Arrival::Action(parts) if parts.is_empty() => return Ok(next),
                Arrival::Action(parts) => {
                    (parts.keys().copied().collect(), What::Action(parts.iter().map(|(t, r)| (*t, Part::Doing(r.clone()))).collect()))
                }
                Arrival::Command(t, c) => (BTreeSet::from([*t]), What::Command(c.clone())),
            };
            let seq = (*seq).max(state.last_seq() + 1);
            next.last = seq;
            if matches!(what, What::Action(_)) {
                next.stack.push(Pressed { seq, torrents: torrents.clone(), done: None });
                let over = next.stack.len().saturating_sub(DEPTH);
                let aged: Vec<u64> = next.stack.drain(..over).map(|p| p.seq).collect();
                for seq in aged {
                    next.left(seq, Leave::Aged);
                }
            }
            next.enter(seq, Step { torrents, what, phase: Phase::Queued });
        }
        (Event::Observed(Observed::Moved(seq, t, r)), Some(What::Action(_))) => {
            if let Some(Step { what: What::Action(parts), .. }) = next.steps.get_mut(seq) {
                parts.insert(*t, Part::Doing(r.clone()));
            }
        }
        (Event::Observed(Observed::Finished(seq, t, entry)), Some(What::Action(_))) => next.action_part_over(*seq, *t, entry.clone()),
        (Event::Observed(Observed::Failed(seq, t)), Some(What::Action(_))) => next.action_part_over(*seq, *t, None),
        (Event::Observed(Observed::Finished(seq, t, _)), Some(What::Undo { .. })) => next.undo_part_over(*seq, *t, Undid::Back),
        (Event::Observed(Observed::Failed(seq, t)), Some(What::Undo { .. })) => next.undo_part_over(*seq, *t, Undid::Failed),
        (Event::Observed(Observed::Refused { seq, torrent, why, stranded }), Some(What::Undo { parts, .. })) => {
            let undid = match parts.get(torrent).and_then(Part::doing) {
                Some(entry) => {
                    let (action, named) = entry.named();
                    Undid::Refused { not: proto::NotBack { torrent: named, action, why: why.clone(), stranded: stranded.clone() } }
                }
                None => Undid::Failed,
            };
            next.undo_part_over(*seq, *torrent, undid);
        }
        (Event::Observed(Observed::Finished(seq, ..) | Observed::Failed(seq, _)), Some(What::Command(_))) => next.leave(*seq),
        (Event::Observed(Observed::Moved(..)), Some(What::Undo { .. } | What::Command(_)) | None)
        | (Event::Observed(Observed::Finished(..) | Observed::Failed(..) | Observed::Refused { .. }), None)
        | (Event::Observed(Observed::Refused { .. }), Some(What::Action(_) | What::Command(_))) => {}
        (Event::Observed(observed @ (Observed::Stopping | Observed::Restarted)), _) => {
            let restart = matches!(observed, Observed::Restarted);
            let gone: Vec<u64> = state
                .steps
                .iter()
                .filter(|(_, s)| match (&s.what, s.phase) {
                    (What::Action(_), Phase::Queued) | (What::Command(_), Phase::Queued) => true,
                    (What::Command(_), Phase::Running | Phase::Aborting) => restart,
                    (What::Undo { .. }, _) | (What::Action(_), Phase::Running | Phase::Aborting) => false,
                })
                .map(|(seq, _)| *seq)
                .collect();
            for seq in gone {
                next.leave(seq);
                next.unstack(seq, Leave::Dropped);
            }
        }
    }
    next.promote();
    Ok(next)
}

/// What the actor starts or aborts now, of what it doesn't carry out already: each part of a step
/// running, then the stop of one aborting.
pub fn next<R, E, C>(state: &State<R, E, C>, world: &World) -> Option<Do> {
    let start =
        state.steps.iter().find_map(|(seq, s)| s.underway().find(|t| !world.started.contains(&(*seq, *t))).map(|t| Do::Start(*seq, t)));
    start.or_else(|| {
        state.steps.iter().find_map(|(seq, s)| (s.phase == Phase::Aborting && !world.aborted.contains(seq)).then_some(Do::Abort(*seq)))
    })
}

/// What an Undo's client hears once each part of the undo of action `seq` is over: the torrents
/// that came back, and those that didn't, each with why; none back, it's refused, for the first
/// one's reason, with what each left out of the Trash (one press, one step: #235). The engine's and
/// the fake's.
pub fn undone(
    seq: u64,
    action: proto::UndoAction,
    back: Vec<proto::Named>,
    refused: Vec<proto::NotBack>,
) -> Result<proto::Undone, proto::Refusal> {
    if !back.is_empty() {
        return Ok(proto::Undone { undo: proto::Undoable { seq, action, torrents: back }, refused });
    }
    let why = refused.first().map_or(CantUndo::Failed { detail: "nothing came back".into() }, |r| r.why.clone());
    let stranded = refused.iter().flat_map(|r| r.stranded.iter().cloned()).collect();
    let undo = proto::Undoable { seq, action, torrents: refused.into_iter().map(|r| r.torrent).collect() };
    Err(proto::Refusal::Undo { undo, why, stranded })
}

/// Undo's machine as a [`Machine`](crate::machine::Machine), for the harness.
pub struct Steps<R, E, C>(PhantomData<(R, E, C)>);

impl<R: Clone, E: Undoes, C: Clone> crate::machine::Machine for Steps<R, E, C> {
    type State = State<R, E, C>;
    type Event = Event<R, E, C>;
    type Refusal = Refusal;
    type World = World;
    type Do = Do;

    fn decide(state: &State<R, E, C>, event: &Event<R, E, C>) -> Result<State<R, E, C>, Refusal> {
        decide(state, event)
    }

    fn next(state: &State<R, E, C>, world: &World) -> Option<Do> {
        next(state, world)
    }
}

#[cfg(test)]
mod tests;
