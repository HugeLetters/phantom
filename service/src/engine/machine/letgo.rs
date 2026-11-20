//! Letting go of a file gone from the Trash (#234): a file a Move to Trash took, emptied from the
//! Trash (or deleted from it by hand) while librqbit still has it open. Phantom closes it, so its
//! space is freed at once, then takes the torrent out of librqbit and adds it back without that
//! file's pieces, so nothing offers them any more. librqbit can't withdraw a piece it has, and drops
//! a peer whose piece it can't read: its peers drop once, and connect again.
//!
//! One state per torrent, kept in its record (its row of `records.db`) and written ahead of each step, so a
//! crash between them is finished at the next start:
//!
//! ```text
//! Holding --Found--> Gone --(free) Stashed--> Stashed --Cleared | Failed--> Holding
//! Gone, free, none of its pieces had (a check dropped them) --Cleared--> Holding
//! at a start, before it's added:  Gone, Stashed + its pieces saved by librqbit  --> its pieces cleared there
//!                                 Stashed, none saved by librqbit              --> its pieces put back from the stash
//! ```
//!
//! The pieces go by file, each a range of pieces: those it shares with a neighbour too, which a
//! neighbour in the Trash reads from what's kept of it, and one the torrent gets downloads again.

use std::collections::BTreeMap;
use std::convert::Infallible;

use ::machine::Machine;
use serde::{Deserialize, Serialize};

use super::super::undo::Stash;

/// The pieces of each file let go of, by index: `[start, end)`.
pub type Pieces = BTreeMap<usize, (u32, u32)>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Nothing to let go of.
    #[default]
    Holding,
    /// These files are gone from the Trash, written ahead of closing them: the torrent is to be
    /// added back without their pieces, once it can be taken out of librqbit.
    Gone { pieces: Pieces },
    /// Kept aside in `stash` (its .torrent and its pieces), written ahead of taking it out of
    /// librqbit: added back from there, without `pieces`.
    Stashed { pieces: Pieces, stash: Stash },
}

/// What the actor finds, or how a step went.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A look found these files gone from the Trash, still open.
    Found(Pieces),
    /// The torrent is kept aside here.
    Stashed(Stash),
    /// librqbit has it without their pieces: added back, or cleared where librqbit saved it.
    Cleared,
    /// Adding it back failed once it was out: an error until a check adds it again, which finds
    /// what it has.
    Failed,
}

/// What `next` looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum World {
    /// The engine runs: `free`, whether the torrent can be taken out of librqbit now (librqbit has
    /// it, its lifecycle Ready, Unchecked or Missing files, and nothing runs on its lane);
    /// `offered`, whether librqbit has any piece of the files let go of (a check, or an add again,
    /// since may have dropped them all).
    Running { free: bool, offered: bool },
    /// The engine starts, before it adds the torrents to librqbit: `saved`, whether librqbit has its
    /// pieces saved.
    Starting { saved: bool },
}

/// What the actor carries out.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    /// Keep the torrent aside, as librqbit has it now, then [`Do::AddBack`].
    Stash,
    /// Nothing to take out: librqbit has none of those pieces any more. It's [`Event::Cleared`].
    End,
    /// Take it out of librqbit, clear `pieces` in what was kept, and add it back from there,
    /// stopped, without a full check.
    AddBack { pieces: Pieces, stash: Stash },
    /// Clear `pieces` where librqbit saved them, before it restores the torrent.
    ClearSaved { pieces: Pieces },
}

impl State {
    /// The files it lets go of, by index.
    pub fn files(&self) -> std::collections::BTreeSet<usize> {
        match self {
            State::Holding => Default::default(),
            State::Gone { pieces } | State::Stashed { pieces, .. } => pieces.keys().copied().collect(),
        }
    }
}

/// What `event` does to `s`. Never refused: each is a fact.
pub fn decide(s: &State, event: &Event) -> State {
    use Event as E;
    use State as S;
    match (s, event) {
        (S::Holding, E::Found(found)) => S::Gone { pieces: found.clone() },
        (S::Gone { pieces }, E::Found(found)) => S::Gone { pieces: union(pieces, found) },
        // Found while it's added back from the stash (it never is: that's one go on the actor), it's
        // left out of that add too.
        (S::Stashed { pieces, stash }, E::Found(found)) => S::Stashed { pieces: union(pieces, found), stash: stash.clone() },
        (S::Gone { pieces } | S::Stashed { pieces, .. }, E::Stashed(stash)) => S::Stashed { pieces: pieces.clone(), stash: stash.clone() },
        // Nothing to let go of: the stash isn't named, and goes.
        (S::Holding, E::Stashed(_)) => S::Holding,
        (S::Holding | S::Gone { .. } | S::Stashed { .. }, E::Cleared | E::Failed) => S::Holding,
    }
}

/// What makes librqbit match `s` in `world`.
pub fn next(s: &State, world: World) -> Option<Do> {
    use State as S;
    use World as W;
    match (s, world) {
        (S::Holding, W::Running { .. } | W::Starting { .. }) => None,
        (S::Gone { .. }, W::Running { free: true, offered: true }) => Some(Do::Stash),
        // A check dropped those pieces meanwhile: its peers aren't dropped for nothing.
        (S::Gone { .. }, W::Running { free: true, offered: false }) => Some(Do::End),
        // Its files are closed already; it's added back once it can be.
        (S::Gone { .. }, W::Running { free: false, .. }) => None,
        // Kept aside: it's being added back, which nothing waits for.
        (S::Stashed { pieces, stash }, W::Running { .. }) => Some(Do::AddBack { pieces: pieces.clone(), stash: stash.clone() }),
        // A crash before librqbit let go of it: it restores the torrent from what it saved.
        (S::Gone { pieces } | S::Stashed { pieces, .. }, W::Starting { saved: true }) => Some(Do::ClearSaved { pieces: pieces.clone() }),
        // A crash once librqbit let go of it, before it was added back.
        (S::Stashed { pieces, stash }, W::Starting { saved: false }) => Some(Do::AddBack { pieces: pieces.clone(), stash: stash.clone() }),
        // librqbit has nothing of it, and nothing was kept: the record goes, as any librqbit didn't
        // restore.
        (S::Gone { .. }, W::Starting { saved: false }) => None,
    }
}

fn union(a: &Pieces, b: &Pieces) -> Pieces {
    a.iter().chain(b).map(|(i, r)| (*i, *r)).collect()
}

/// The let-go as a [`Machine`]. Only ever named, as a type.
#[allow(dead_code)]
pub struct LetGo;

impl Machine for LetGo {
    type State = State;
    type Event = Event;
    type Refusal = Infallible;
    type World = World;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Infallible> {
        Ok(decide(state, event))
    }

    fn next(state: &State, world: &World) -> Option<Do> {
        next(state, *world)
    }
}

#[cfg(test)]
mod tests {
    use ::machine::every;
    use ::machine::{Row, table};

    use super::*;

    fn stash(n: usize) -> Stash {
        Stash { dir: format!("/undo/{n}").into(), files: Vec::new() }
    }

    fn pieces(files: &[usize]) -> Pieces {
        files.iter().map(|&f| (f, (f as u32 * 2, f as u32 * 2 + 2))).collect()
    }

    every!(State {
        State::Holding => [State::Holding],
        State::Gone { .. } => [State::Gone { pieces: pieces(&[0]) }],
        State::Stashed { .. } => [State::Stashed { pieces: pieces(&[0]), stash: stash(1) }],
    });

    every!(Event {
        Event::Found(_) => [Event::Found(pieces(&[1]))],
        Event::Stashed(_) => [Event::Stashed(stash(2))],
        Event::Cleared => [Event::Cleared],
        Event::Failed => [Event::Failed],
    });

    every!(World {
        World::Running { .. } => [true, false].into_iter().flat_map(|free| [true, false].map(|offered| World::Running { free, offered })),
        World::Starting { .. } => [World::Starting { saved: true }, World::Starting { saved: false }],
    });

    fn let_go(s: &State) -> Pieces {
        match s {
            State::Holding => Pieces::new(),
            State::Gone { pieces } | State::Stashed { pieces, .. } => pieces.clone(),
        }
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        let rows: [Row<State, Event, State>; 4] = [
            Row {
                says: "files found gone join those let go of, whatever step it's at",
                when: |_, e| matches!(e, Event::Found(_)),
                then: |s, e, n| {
                    let Event::Found(found) = e else { unreachable!() };
                    let same_step = matches!((s, n), (State::Holding | State::Gone { .. }, State::Gone { .. }))
                        || matches!((s, n), (State::Stashed { stash: a, .. }, State::Stashed { stash: b, .. }) if a == b);
                    same_step && let_go(n) == union(&let_go(s), found)
                },
            },
            Row {
                says: "kept aside, it's added back from there, with what it lets go of",
                when: |s, e| matches!(e, Event::Stashed(_)) && !matches!(s, State::Holding),
                then: |s, _, n| *n == State::Stashed { pieces: let_go(s), stash: stash(2) },
            },
            Row {
                says: "kept aside with nothing to let go of, nothing changes",
                when: |s, e| matches!(e, Event::Stashed(_)) && matches!(s, State::Holding),
                then: |_, _, n| *n == State::Holding,
            },
            Row {
                says: "cleared, or failed to add back, there's nothing left to let go of",
                when: |_, e| matches!(e, Event::Cleared | Event::Failed),
                then: |_, _, n| *n == State::Holding,
            },
        ];
        table(decide, &rows, &[]);
    }

    #[test]
    fn what_is_let_go_of_is_added_back_without_it_once_the_torrent_is_free_or_cleared_where_librqbit_saved_it() {
        let rows: [Row<State, World, Option<Do>>; 5] = [
            Row { says: "nothing to let go of, nothing's done", when: |s, _| *s == State::Holding, then: |_, _, d| d.is_none() },
            Row {
                says: "gone, it's kept aside once the torrent is free, ends there if librqbit has none of it, and waits while it isn't",
                when: |s, w| matches!(s, State::Gone { .. }) && matches!(w, World::Running { .. }),
                then: |_, w, d| {
                    *d == match w {
                        World::Running { free: true, offered: true } => Some(Do::Stash),
                        World::Running { free: true, offered: false } => Some(Do::End),
                        World::Running { free: false, .. } | World::Starting { .. } => None,
                    }
                },
            },
            Row {
                says: "kept aside, it's added back from there, running or at a start librqbit has nothing of it",
                when: |s, w| matches!(s, State::Stashed { .. }) && matches!(w, World::Running { .. } | World::Starting { saved: false }),
                then: |s, _, d| *d == Some(Do::AddBack { pieces: let_go(s), stash: stash(1) }),
            },
            Row {
                says: "at a start librqbit has it saved: its pieces are cleared there",
                when: |s, w| *s != State::Holding && *w == World::Starting { saved: true },
                then: |s, _, d| *d == Some(Do::ClearSaved { pieces: let_go(s) }),
            },
            Row {
                says: "at a start librqbit has nothing of it, and nothing was kept: nothing's done",
                when: |s, w| matches!(s, State::Gone { .. }) && *w == World::Starting { saved: false },
                then: |_, _, d| d.is_none(),
            },
        ];
        table(|s, w| next(s, *w), &rows, &[]);
    }
}
