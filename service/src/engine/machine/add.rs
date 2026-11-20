//! An add's stages (#322): the task of adding one torrent, from the moment it's asked to the
//! moment librqbit has it, or it's given up. `lib` only says where the torrent is in librqbit
//! ([`super::lib::Lib::Adding`] is one of its stages); this is the add as a whole: the duplicate
//! check, the wait for a hash something runs on, fetching the info (the .torrent URL is fetched
//! before the engine, by the service), librqbit's add, and what a Remove during any of it means.
//!
//! ```text
//! Asked --Free--> Fetching --Added--> Done
//!   |  \             |  \--Failed--> Failed
//!   |   Busy--> Held  \--Remove--> Removed --Added (the task had finished)--> Orphan
//!   |           (asked again)
//!   \--Listed--> Existing
//! ```
//!
//! The stages are the ones the actor tells apart, no more: librqbit's add is one future (it lists
//! the files without adding, then adds under Phantom's id), whose halves nothing observes, so
//! there's no `Listing` between `Fetching` and `Adding`. `Existing` (an add of what the list has,
//! answered as such) and `Held` (its hash busy, asked again once nothing runs on it) are the add's
//! other ends and wait; `Orphan` is a task that ended with a handle after its torrent was taken
//! off the list: librqbit has a torrent nobody listed, which goes unless it was added again.
//!
//! The stage isn't stored: `Asked` and `Held` are the entry the add came by, `Fetching` is a torrent
//! whose librqbit side is `Lib::Adding`, and what ends its task is decided against `Fetching` if the
//! list still has the torrent so, else `Removed` (it was taken off, or its id is another add's), so
//! the task's end can't be taken for the end of one that isn't running. `Done`, `Failed`, `Existing`
//! and `Orphan` are where it ends, acted on at once.
//!
//! Nothing is refused: each event is a fact, and one that can't happen in a stage (a late `Free`
//! once fetching, a second end) leaves it as it is.

use std::convert::Infallible;

use ::machine::Machine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Asked, nothing decided: the first thing is whether its torrent is in the list already.
    Asked,
    /// Something runs on its hash (an undo bringing it back, its Delete): asked again once that's over.
    Held,
    /// In the list, fetching its info: the task runs, and a Remove stops it.
    Fetching,
    /// librqbit has it, with its info.
    Done,
    /// Fetching its info failed: in the list, an error until a Resume or a Check.
    Failed,
    /// The list had it: answered as that, nothing added.
    Existing,
    /// Taken off the list while it was fetching: its task stopped.
    Removed,
    /// Taken off the list, but its task had ended: librqbit has a torrent nobody lists, which
    /// `next` takes out unless it was added again meanwhile.
    Orphan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Its torrent is taken off the list (a Remove, a Delete, an undo of an add).
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// Its hash looked up against the list and what runs on torrents.
    Seen(Hash),
    /// Its task ended.
    Ended(Ended),
}

/// What the list and the lanes have of the add's info hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    /// Nothing: a new torrent.
    Free,
    /// Something runs on it, or waits to.
    Busy,
    /// A torrent in the list has it.
    Listed,
}

/// How its task ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// librqbit has the torrent.
    Added,
    /// It couldn't be fetched or added.
    Failed,
}

/// What `next` looks at besides the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct World {
    /// Its task is spawned and hasn't ended.
    pub running: bool,
    /// A torrent with its hash is in the list (again).
    pub listed: bool,
}

/// What the actor carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Start the task, the torrent in the list at once.
    Fetch,
    /// Stop the task.
    Abort,
    /// Take the torrent librqbit has out of it, for nobody listed it.
    TakeBackOut,
}

pub type Refusal = Infallible;

/// What `event` does to `s`.
pub fn decide(s: &State, event: &Event) -> Result<State, Refusal> {
    use Command as C;
    use Ended as X;
    use Event as E;
    use Hash as H;
    use Observed as O;
    use State as S;
    Ok(match (s, event) {
        (S::Asked | S::Held, E::Observed(O::Seen(H::Free))) => S::Fetching,
        (S::Asked | S::Held, E::Observed(O::Seen(H::Busy))) => S::Held,
        (S::Asked | S::Held, E::Observed(O::Seen(H::Listed))) => S::Existing,
        (S::Fetching, E::Observed(O::Ended(X::Added))) => S::Done,
        (S::Fetching, E::Observed(O::Ended(X::Failed))) => S::Failed,
        (S::Fetching, E::Command(C::Remove)) => S::Removed,
        // Its task ended with a handle before the Remove reached it: nobody has the torrent.
        (S::Removed, E::Observed(O::Ended(X::Added))) => S::Orphan,
        // What can't happen at this stage changes nothing.
        (S::Asked | S::Held, E::Command(C::Remove) | E::Observed(O::Ended(_))) => *s,
        (S::Fetching, E::Observed(O::Seen(_))) => *s,
        (S::Done | S::Failed | S::Existing, E::Command(C::Remove) | E::Observed(O::Seen(_) | O::Ended(_))) => *s,
        (S::Removed, E::Command(C::Remove) | E::Observed(O::Seen(_) | O::Ended(X::Failed))) => *s,
        (S::Orphan, E::Command(C::Remove) | E::Observed(O::Seen(_) | O::Ended(_))) => *s,
    })
}

/// What the actor carries out in `s`, `world` being as it is.
pub fn next(s: &State, world: &World) -> Option<Do> {
    use State as S;
    match s {
        S::Fetching => (!world.running).then_some(Do::Fetch),
        S::Removed => world.running.then_some(Do::Abort),
        S::Orphan => (!world.listed).then_some(Do::TakeBackOut),
        S::Asked | S::Held | S::Done | S::Failed | S::Existing => None,
    }
}

/// The add as a [`Machine`], for the harness. Only ever named, as a type.
#[allow(dead_code)]
pub struct Add;

impl Machine for Add {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = World;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, world: &World) -> Option<Do> {
        next(state, world)
    }
}

#[cfg(test)]
mod tests {
    use ::machine::{DecideRow, Every, Row, decide_table, every, sequences, table};

    use super::*;

    every!(State {
        State::Asked => [State::Asked],
        State::Held => [State::Held],
        State::Fetching => [State::Fetching],
        State::Done => [State::Done],
        State::Failed => [State::Failed],
        State::Existing => [State::Existing],
        State::Removed => [State::Removed],
        State::Orphan => [State::Orphan],
    });

    every!(Event {
        Event::Command(Command::Remove) => [Event::Command(Command::Remove)],
        Event::Observed(Observed::Seen(_)) => [Hash::Free, Hash::Busy, Hash::Listed].map(|h| Event::Observed(Observed::Seen(h))),
        Event::Observed(Observed::Ended(_)) => [Ended::Added, Ended::Failed].map(|e| Event::Observed(Observed::Ended(e))),
    });

    every!(World {
        World { .. } => [true, false].into_iter().flat_map(|running| [true, false].map(|listed| World { running, listed })),
    });

    fn rows() -> Vec<DecideRow<Add>> {
        use Event as E;
        use Observed as O;
        use State as S;
        vec![
            Row {
                says: "asked, or held, its hash decides: a new one is fetched, a busy one held, a listed one is that torrent",
                when: |s, e| matches!(s, S::Asked | S::Held) && matches!(e, E::Observed(O::Seen(_))),
                then: |_, e, out| {
                    let E::Observed(O::Seen(h)) = e else { return false };
                    *out == Ok(match h {
                        Hash::Free => S::Fetching,
                        Hash::Busy => S::Held,
                        Hash::Listed => S::Existing,
                    })
                },
            },
            Row {
                says: "fetching, its task's end is the add's: done, or failed",
                when: |s, e| *s == S::Fetching && matches!(e, E::Observed(O::Ended(_))),
                then: |_, e, out| {
                    let E::Observed(O::Ended(x)) = e else { return false };
                    *out == Ok(match x {
                        Ended::Added => S::Done,
                        Ended::Failed => S::Failed,
                    })
                },
            },
            Row {
                says: "fetching, a Remove stops it",
                when: |s, e| *s == S::Fetching && *e == E::Command(Command::Remove),
                then: |_, _, out| *out == Ok(S::Removed),
            },
            Row {
                says: "removed, a task that ended with a torrent leaves an orphan to take out; one that failed leaves nothing",
                when: |s, e| *s == S::Removed && matches!(e, E::Observed(O::Ended(_))),
                then: |_, e, out| *out == Ok(if *e == E::Observed(O::Ended(Ended::Added)) { S::Orphan } else { S::Removed }),
            },
            Row {
                says: "every other pair is a fact that can't happen at this stage: it changes nothing",
                when: |s, e| {
                    !(matches!(s, S::Asked | S::Held) && matches!(e, E::Observed(O::Seen(_))))
                        && !(*s == S::Fetching && matches!(e, E::Observed(O::Ended(_)) | E::Command(Command::Remove)))
                        && !(*s == S::Removed && matches!(e, E::Observed(O::Ended(_))))
                },
                then: |s, _, out| *out == Ok(*s),
            },
        ]
    }

    fn always() -> Vec<DecideRow<Add>> {
        vec![Row::always("nothing is refused", |_, _, out| out.is_ok())]
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<Add>(&rows(), &always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        sequences::<Add>(vec![State::Asked], 12, &rows(), &always());
    }

    #[test]
    fn every_sequence_from_any_stage_follows_the_table() {
        sequences::<Add>(State::every(), 8, &rows(), &always());
    }

    #[test]
    fn the_actor_is_told_what_the_stage_needs() {
        let todo: [Row<State, World, Option<Do>>; 4] = [
            Row {
                says: "fetching, the task starts if it isn't running",
                when: |s, _| *s == State::Fetching,
                then: |_, w, d| *d == (!w.running).then_some(Do::Fetch),
            },
            Row {
                says: "removed, a task still running is stopped",
                when: |s, _| *s == State::Removed,
                then: |_, w, d| *d == w.running.then_some(Do::Abort),
            },
            Row {
                says: "an orphan goes unless the torrent was added again",
                when: |s, _| *s == State::Orphan,
                then: |_, w, d| *d == (!w.listed).then_some(Do::TakeBackOut),
            },
            Row {
                says: "no other stage does anything",
                when: |s, _| !matches!(s, State::Fetching | State::Removed | State::Orphan),
                then: |_, _, d| d.is_none(),
            },
        ];
        table(|s, w| next(s, w), &todo, &[]);
    }
}
