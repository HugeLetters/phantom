//! Undo's machine (#150, #190, [ADR 0008]): one pure machine, as the lifecycle is, holding the undo
//! stack, which decides which step a ⌘Z takes, and a lane per torrent, which decides when steps run.
//!
//! A **step** is an action (a Remove, a Delete or a Move to Trash), an undo, or another command for
//! a torrent. It goes into the lane of every torrent it touches, all at once, as it arrives, so
//! lanes that share steps always agree on their order; it runs once it's first in all of them. An
//! action also goes on the stack as it arrives, in the order the commands came, so which step a ⌘Z
//! takes never depends on timing; only the work overlaps, and only across torrents.
//!
//! A step is `Queued`, then `Running` once it's first in all its lanes (the actor starts it), then
//! over: an action is `Committed` with what undoes it (it stays on the stack), or `Gone` if it
//! changed nothing; an undo or a command just ends. ⌘Z on the action on top: one queued is gone at
//! once; one running is `Aborting` (it ends after the item in flight, and what it did is rolled
//! back in its place in its lanes); one committed gets its undo, a new step, in the lanes of the
//! torrents it touched. An action rolled back, or whose undo couldn't be carried out, is gone from
//! the stack: no Undo ever waits for another action.
//!
//! The actor does the file moves and reports them: [`Observed::Moved`] (written ahead), then
//! [`Observed::Finished`] or [`Observed::Failed`]. [`next`] says which steps to start now, and
//! which to abort. A stop drops what waits but the undos ([`Observed::Stopping`]); the next start
//! drops it too, after a crash, and the steps that were running are carried out again from their
//! log ([`Observed::Restarted`]). (A crash itself is no event: the machine goes with the process.)
//!
//! | Step | Event | Becomes |
//! |---|---|---|
//! | none (the stack empty) | Undo | refused: Nothing |
//! | an action queued, on top | Undo | gone |
//! | an action running, on top | Undo | aborting, off the stack |
//! | an action committed, on top | Undo | off the stack; its undo arrives |
//! | any | Arrived | queued in each of its lanes (an action on the stack too, past [`DEPTH`] the oldest off it), running if first in all |
//! | an action running or aborting | Moved(r) | holding `r` |
//! | an action running | Finished(e) | committed with `e`, in its place on the stack, without the entries `e` absorbs |
//! | an action running | Finished(none), Failed | gone |
//! | an action aborting | Finished(e) | rolled back: its undo, with `e`, running in its place, under its seq |
//! | an action aborting | Finished(none), Failed | gone |
//! | an undo or a command running | Finished, Failed | over |
//! | a step queued, an undo or a command (Moved), or none | Moved, Finished, Failed | as it was |
//! | any | Stopping | the actions and commands queued gone |
//! | any | Restarted | the actions and commands queued, and the commands running, gone |
//!
//! After each, every step first in all its lanes is running.
//!
//! [ADR 0008]: ../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// How many actions can be undone: the oldest goes past it.
pub const DEPTH: usize = 20;

/// A torrent, by id: a lane's.
pub type Torrent = u64;

/// What the stack needs of an entry.
pub trait Undoes: Clone + PartialEq {
    /// Whether this entry, done now, takes the place of `older`: what undoing it undoes holds
    /// `older`'s too (the engine's Delete of a torrent whose partial Delete is on the stack).
    fn absorbs(&self, older: &Self) -> bool;
}

/// What a step is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum What<R, E, C> {
    /// A Remove, Delete or Move to Trash, holding `R` while it runs (what it did so far).
    Action(R),
    /// Putting back what action `of` did: `entry`. An Undo of it once committed, or its rollback
    /// once aborted.
    Undo { of: u64, entry: E },
    /// Any other command, which waits its turn on its torrents and is on no stack.
    Command(C),
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
    /// The torrents it touches: it's in each one's lane.
    pub torrents: BTreeSet<Torrent>,
    pub what: What<R, E, C>,
    pub phase: Phase,
}

/// An action on the undo stack: its step, the torrents it touches and, once committed, what undoes
/// it (until then its step is in `steps`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Pressed<E> {
    pub seq: u64,
    pub torrents: BTreeSet<Torrent>,
    pub done: Option<E>,
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
}

impl<R, E, C> Default for State<R, E, C> {
    fn default() -> Self {
        Self { stack: Vec::new(), steps: BTreeMap::new(), lanes: BTreeMap::new(), last: 0 }
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

    /// Step `seq` is over: out of its lanes.
    fn leave(&mut self, seq: u64) {
        if let Some(step) = self.steps.remove(&seq) {
            for t in &step.torrents {
                if let Some(lane) = self.lanes.get_mut(t) {
                    lane.retain(|s| *s != seq);
                    if lane.is_empty() {
                        self.lanes.remove(t);
                    }
                }
            }
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

    /// Off the stack: action `seq`.
    fn unstack(&mut self, seq: u64) {
        self.stack.retain(|p| p.seq != seq);
    }
}

/// What arrives: an action, on the stack, or another command.
#[derive(Debug, Clone, PartialEq)]
pub enum Arrival<R, C> {
    Action(R),
    Command(C),
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

#[derive(Debug, Clone, PartialEq)]
pub enum Observed<R, E, C> {
    /// A step for `torrents` came, numbered `seq` or, if that isn't past every step's, one past the
    /// last (the newest step says).
    Arrived { seq: u64, torrents: BTreeSet<Torrent>, what: Arrival<R, C> },
    /// Running action `seq` moved an item: it holds `R` now, written ahead.
    Moved(u64, R),
    /// Running step `seq` ended: an action that changed something, with what undoes it (none: it
    /// changed nothing), an undo carried out, a command done.
    Finished(u64, Option<E>),
    /// Running step `seq` couldn't: an action that changed nothing, an undo refused (its action is
    /// gone from the stack all the same), a command refused.
    Failed(u64),
    /// The service is stopping: what waits doesn't start, but the undos, which were pressed.
    Stopping,
    /// The engine starts again, after a stop or a crash: what waited is gone but the undos (nobody
    /// waits for it any more), and the steps that were running are carried out again.
    Restarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The stack is empty.
    Nothing,
}

/// What the actor carries out now: the steps it started, and those it told to stop. It takes a step
/// out as it reports the step's end, so an aborted action's rollback, under the same seq, starts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct World {
    pub started: BTreeSet<u64>,
    pub aborted: BTreeSet<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Start step `seq`: it's first in all its lanes.
    Start(u64),
    /// Stop action `seq` once the item in flight is done.
    Abort(u64),
}

pub fn decide<R: Clone, E: Undoes, C: Clone>(state: &State<R, E, C>, event: &Event<R, E, C>) -> Result<State<R, E, C>, Refusal> {
    let target = match event {
        Event::Observed(Observed::Moved(seq, _) | Observed::Finished(seq, _) | Observed::Failed(seq)) => state.steps.get(seq),
        Event::Command(_) | Event::Observed(Observed::Arrived { .. } | Observed::Stopping | Observed::Restarted) => None,
    };
    let target = target.map(|s| (&s.what, s.phase));
    let mut next = state.clone();
    match (event, target) {
        (Event::Command(Command::Undo), _) => {
            let top = next.stack.pop().ok_or(Refusal::Nothing)?;
            match (top.done, next.steps.get(&top.seq).map(|s| s.phase)) {
                (Some(entry), _) => {
                    let seq = next.last_seq().max(top.seq) + 1;
                    next.last = seq;
                    next.enter(seq, Step { torrents: top.torrents, what: What::Undo { of: top.seq, entry }, phase: Phase::Queued });
                }
                (None, Some(Phase::Queued)) => next.leave(top.seq),
                (None, Some(Phase::Running | Phase::Aborting)) => {
                    if let Some(step) = next.steps.get_mut(&top.seq) {
                        step.phase = Phase::Aborting;
                    }
                }
                (None, None) => {}
            }
        }
        (Event::Observed(Observed::Arrived { seq, torrents, what }), _) => {
            let seq = (*seq).max(state.last_seq() + 1);
            next.last = seq;
            let what = match what {
                Arrival::Action(r) => {
                    next.stack.push(Pressed { seq, torrents: torrents.clone(), done: None });
                    let over = next.stack.len().saturating_sub(DEPTH);
                    next.stack.drain(..over);
                    What::Action(r.clone())
                }
                Arrival::Command(c) => What::Command(c.clone()),
            };
            next.enter(seq, Step { torrents: torrents.clone(), what, phase: Phase::Queued });
        }
        (Event::Observed(Observed::Moved(seq, r)), Some((What::Action(_), Phase::Running | Phase::Aborting))) => {
            if let Some(step) = next.steps.get_mut(seq) {
                step.what = What::Action(r.clone());
            }
        }
        (Event::Observed(Observed::Finished(seq, Some(entry))), Some((What::Action(_), Phase::Running))) => {
            next.leave(*seq);
            if let Some(at) = next.stack.iter().position(|p| p.seq == *seq) {
                next.stack[at].done = Some(entry.clone());
                let absorbed = |p: &Pressed<E>| p.seq != *seq && p.done.as_ref().is_some_and(|old| entry.absorbs(old));
                next.stack.retain(|p| !absorbed(p));
            }
        }
        (
            Event::Observed(Observed::Finished(seq, None) | Observed::Failed(seq)),
            Some((What::Action(_), Phase::Running | Phase::Aborting)),
        ) => {
            next.leave(*seq);
            next.unstack(*seq);
        }
        (Event::Observed(Observed::Finished(seq, Some(entry))), Some((What::Action(_), Phase::Aborting))) => {
            if let Some(step) = next.steps.get_mut(seq) {
                (step.what, step.phase) = (What::Undo { of: *seq, entry: entry.clone() }, Phase::Running);
            }
        }
        (
            Event::Observed(Observed::Finished(seq, _) | Observed::Failed(seq)),
            Some((What::Undo { .. } | What::Command(_), Phase::Running | Phase::Aborting)),
        ) => next.leave(*seq),
        (
            Event::Observed(Observed::Moved(..)),
            Some((What::Undo { .. } | What::Command(_), _) | (What::Action(_), Phase::Queued)) | None,
        )
        | (Event::Observed(Observed::Finished(..) | Observed::Failed(_)), Some((_, Phase::Queued)) | None) => {}
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
                next.unstack(seq);
            }
        }
    }
    next.promote();
    Ok(next)
}

/// What the actor starts or aborts now, of what it doesn't carry out already.
pub fn next<R, E, C>(state: &State<R, E, C>, world: &World) -> Option<Do> {
    state.steps.iter().find_map(|(seq, s)| match s.phase {
        Phase::Running | Phase::Aborting if !world.started.contains(seq) => Some(Do::Start(*seq)),
        Phase::Aborting if !world.aborted.contains(seq) => Some(Do::Abort(*seq)),
        Phase::Queued | Phase::Running | Phase::Aborting => None,
    })
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
