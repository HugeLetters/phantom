//! The undo stack as a machine (#150, [ADR 0008]): what a Remove or Delete that can be undone, an
//! Undo, and an undo's end do to it. An Undo takes the top entry off the stack at once, and it's
//! under way until it ends, whether it was carried out or not: a refused undo is dropped.
//!
//! Generic over what an entry holds, the engine's own ([`Undoes`]), so the GUI's fake service runs
//! the same stack.
//!
//! | State | Event | Becomes |
//! |---|---|---|
//! | none under way, an entry on the stack | Undo | the top entry under way |
//! | none under way, the stack empty | Undo | refused: Nothing |
//! | one under way | Undo | refused: UnderWay |
//! | any | Done(e) | `e` on top, without the entries it absorbs; past [`DEPTH`] the oldest goes |
//! | any | Ended | none under way |
//!
//! | Under way | The world | Do |
//! |---|---|---|
//! | an entry | no undo running | PutBack(entry) |
//! | an entry | one running | nothing |
//! | none | any | nothing |
//!
//! [ADR 0008]: ../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// How many actions can be undone: the oldest goes past it.
pub const DEPTH: usize = 20;

/// What the stack needs of an entry.
pub trait Undoes: Clone + PartialEq {
    /// Whether this entry, done now, takes the place of `older`: what undoing it undoes holds
    /// `older`'s too (the engine's Delete of a torrent whose partial Delete is on the stack).
    fn absorbs(&self, older: &Self) -> bool;
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, bound(serialize = "E: Serialize", deserialize = "E: Deserialize<'de>"))]
pub struct State<E> {
    /// Oldest first.
    pub stack: Vec<E>,
    /// The undo under way, taken off the stack.
    pub undoing: Option<E>,
}

impl<E> Default for State<E> {
    fn default() -> Self {
        Self { stack: Vec::new(), undoing: None }
    }
}

impl<E> State<E> {
    /// What an Undo would undo now.
    pub fn top(&self) -> Option<&E> {
        self.stack.last()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event<E> {
    Command(Command),
    Observed(Observed<E>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Undo,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Observed<E> {
    /// A Remove or a Delete that moved something ended: this undoes it.
    Done(E),
    /// The undo under way ended, carried out or not.
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The stack is empty.
    Nothing,
    /// Another undo is under way.
    UnderWay,
}

/// Whether the actor is carrying out an undo now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum World {
    Idle,
    Running,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Do<E> {
    /// Put the entry's files back, then add its torrent again.
    PutBack(E),
}

pub fn decide<E: Undoes>(state: &State<E>, event: &Event<E>) -> Result<State<E>, Refusal> {
    match (&state.undoing, event, state.top()) {
        (Some(_), Event::Command(Command::Undo), _) => Err(Refusal::UnderWay),
        (None, Event::Command(Command::Undo), None) => Err(Refusal::Nothing),
        (None, Event::Command(Command::Undo), Some(_)) => {
            let mut stack = state.stack.clone();
            let undoing = stack.pop();
            Ok(State { stack, undoing })
        }
        (_, Event::Observed(Observed::Done(entry)), _) => {
            let mut stack: Vec<E> = state.stack.iter().filter(|old| !entry.absorbs(old)).cloned().collect();
            stack.push(entry.clone());
            let over = stack.len().saturating_sub(DEPTH);
            stack.drain(..over);
            Ok(State { stack, undoing: state.undoing.clone() })
        }
        (_, Event::Observed(Observed::Ended), _) => Ok(State { stack: state.stack.clone(), undoing: None }),
    }
}

pub fn next<E: Clone>(state: &State<E>, world: &World) -> Option<Do<E>> {
    match (&state.undoing, world) {
        (Some(entry), World::Idle) => Some(Do::PutBack(entry.clone())),
        (Some(_), World::Running) | (None, _) => None,
    }
}

/// The undo stack as a [`Machine`](crate::machine::Machine), for the harness.
pub struct UndoStack<E>(PhantomData<E>);

impl<E: Undoes> crate::machine::Machine for UndoStack<E> {
    type State = State<E>;
    type Event = Event<E>;
    type Refusal = Refusal;
    type World = World;
    type Do = Do<E>;

    fn decide(state: &State<E>, event: &Event<E>) -> Result<State<E>, Refusal> {
        decide(state, event)
    }

    fn next(state: &State<E>, world: &World) -> Option<Do<E>> {
        next(state, world)
    }
}

#[cfg(test)]
mod tests {
    use crate::every;
    use crate::machine::{DecideRow, Every, Row, decide_table, sequences, table};

    use super::*;

    /// An entry for the tables: torrent `id`, and whether it's a partial Delete, which a later
    /// entry of the same torrent absorbs.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Entry {
        id: u64,
        partial: bool,
    }

    impl Undoes for Entry {
        fn absorbs(&self, older: &Self) -> bool {
            self.id == older.id && older.partial
        }
    }

    /// `n` entries, each of its own torrent, every third a partial Delete.
    fn samples(n: usize) -> Vec<Entry> {
        (0..n as u64).map(|id| Entry { id, partial: id % 3 == 2 }).collect()
    }

    every!(State<Entry> {
        State { undoing: None, .. } => [0, 1, 3, DEPTH - 1, DEPTH].map(|n| State { stack: samples(n), undoing: None }),
        State { undoing: Some(_), .. } => [0, 2, DEPTH].map(|n| State { stack: samples(n), undoing: Some(Entry { id: 50, partial: false }) }),
    });

    every!(Event<Entry> {
        Event::Command(Command::Undo) => [Event::Command(Command::Undo)],
        // Of a partial Delete on the stack, of one that isn't, of a torrent not on it, and partial.
        Event::Observed(Observed::Done(_)) => [2, 3, 60].map(|id| Event::Observed(Observed::Done(Entry { id, partial: false })))
            .into_iter().chain([Event::Observed(Observed::Done(Entry { id: 61, partial: true }))]),
        Event::Observed(Observed::Ended) => [Event::Observed(Observed::Ended)],
    });

    every!(World {
        World::Idle => [World::Idle],
        World::Running => [World::Running],
    });

    type Stack = UndoStack<Entry>;

    fn decide_rows() -> Vec<DecideRow<Stack>> {
        vec![
            Row {
                says: "an Undo takes the top entry off the stack, under way",
                when: |s, e| s.undoing.is_none() && !s.stack.is_empty() && *e == Event::Command(Command::Undo),
                then: |s, _, out| {
                    let Ok(out) = out else { return false };
                    out.undoing.as_ref() == s.stack.last() && out.stack[..] == s.stack[..s.stack.len() - 1]
                },
            },
            Row {
                says: "an Undo with nothing on the stack is refused",
                when: |s, e| s.undoing.is_none() && s.stack.is_empty() && *e == Event::Command(Command::Undo),
                then: |_, _, out| *out == Err(Refusal::Nothing),
            },
            Row {
                says: "an Undo while one is under way is refused, and takes nothing off",
                when: |s, e| s.undoing.is_some() && *e == Event::Command(Command::Undo),
                then: |_, _, out| *out == Err(Refusal::UnderWay),
            },
            Row {
                says: "what's done goes on top, without what it absorbs, the oldest gone past the depth",
                when: |_, e| matches!(e, Event::Observed(Observed::Done(_))),
                then: |s, e, out| {
                    let (Event::Observed(Observed::Done(entry)), Ok(out)) = (e, out) else { return false };
                    let kept: Vec<&Entry> = s.stack.iter().filter(|old| !entry.absorbs(old)).collect();
                    let from = (kept.len() + 1).saturating_sub(DEPTH);
                    out.top() == Some(entry)
                        && out.stack[..out.stack.len() - 1].iter().eq(kept[from..].iter().copied())
                        && out.undoing == s.undoing
                },
            },
            Row {
                says: "an undo's end leaves none under way, and the stack as it is",
                when: |_, e| *e == Event::Observed(Observed::Ended),
                then: |s, _, out| *out == Ok(State { stack: s.stack.clone(), undoing: None }),
            },
        ]
    }

    fn decide_always() -> Vec<DecideRow<Stack>> {
        vec![
            Row::always("never deeper than DEPTH", |_, _, out| out.as_ref().is_ok_and(|s| s.stack.len() <= DEPTH) || out.is_err()),
            Row::always("only an Undo is refused", |_, e, out| out.is_ok() || matches!(e, Event::Command(_))),
            Row::always("an observation never starts an undo", |s, e, out| {
                matches!(e, Event::Command(_)) || out.as_ref().is_ok_and(|o| o.undoing.is_none() || o.undoing == s.undoing)
            }),
        ]
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<Stack>(&decide_rows(), &decide_always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        sequences::<Stack>(State::every(), 60, &decide_rows(), &decide_always());
    }

    /// Past 20 actions the oldest is gone, and those left come off newest first.
    #[test]
    fn past_twenty_the_oldest_is_gone() {
        let mut state = State::default();
        for id in 0..DEPTH as u64 + 5 {
            state = decide(&state, &Event::Observed(Observed::Done(Entry { id, partial: false }))).unwrap();
        }
        let mut undone = Vec::new();
        for _ in 0..2 * DEPTH {
            let Ok(next) = decide(&state, &Event::Command(Command::Undo)) else { break };
            undone.push(next.undoing.unwrap().id);
            state = decide(&next, &Event::Observed(Observed::Ended)).unwrap();
        }
        assert_eq!(undone, (5..DEPTH as u64 + 5).rev().collect::<Vec<_>>());
    }

    #[test]
    fn next_puts_back_the_undo_under_way_once() {
        let rows: Vec<Row<State<Entry>, World, Option<Do<Entry>>>> = vec![
            Row {
                says: "one under way and nothing running: put it back",
                when: |s, w| s.undoing.is_some() && *w == World::Idle,
                then: |s, _, d| *d == Some(Do::PutBack(s.undoing.unwrap())),
            },
            Row {
                says: "one running: nothing more",
                when: |s, w| s.undoing.is_some() && *w == World::Running,
                then: |_, _, d| d.is_none(),
            },
            Row { says: "none under way: nothing", when: |s, _| s.undoing.is_none(), then: |_, _, d| d.is_none() },
        ];
        table(next, &rows, &[]);
    }
}
