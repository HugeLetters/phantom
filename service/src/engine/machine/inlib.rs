//! A torrent's librqbit side (#115): whether librqbit is adding it, never did, has it, or had it
//! until adding it again failed. One state, so a torrent can't have librqbit's torrent and an add
//! under way at once, or neither once it has info. Generic over librqbit's torrent `H`, so it's
//! tested on plain values; the actor's is an `Arc<ManagedTorrent>`.
//!
//! [`decide`] says what an add's end does to it; [`next`] says how the lifecycle's [`super::Do`]
//! is carried out on it.
//!
//! ```text
//! Adding --Added--> Has      Adding --FetchFailed--> NotAdded
//! Has --Added (again)--> Has'      Has, TakenOut --TakenOut--> TakenOut
//! TakenOut --Added (again)--> Has'
//! ```
//!
//! Its add task is the actor's: an add can't be copied, and Remove waits for it to stop. The
//! actor keeps one exactly while a torrent is [`State::Adding`].

use std::convert::Infallible;
use std::marker::PhantomData;

use lifecycle::machine::Machine;

use super::Do as Wanted;

#[derive(Debug, Clone, PartialEq)]
pub enum State<H> {
    /// librqbit is adding it, fetching its info. `name`: from the magnet's `dn=` or the .torrent.
    Adding { name: Option<String> },
    /// Fetching its info failed: librqbit never had it.
    NotAdded { name: Option<String> },
    /// librqbit has it.
    Has(H),
    /// librqbit had it, until adding it again failed after taking it out. Its info is still there
    /// to add it again from.
    TakenOut(H),
}

/// How an add ended, the first or one again. Its events are all what the actor sees: nothing
/// here is a client's to ask.
#[derive(Debug, Clone, PartialEq)]
pub enum Event<H> {
    /// librqbit has it now, as this torrent.
    Added(H),
    /// Fetching its info failed.
    FetchFailed,
    /// Adding it again failed once it was taken out.
    TakenOut,
}

/// What the actor carries out on librqbit.
#[derive(Debug, Clone, PartialEq)]
pub enum Do<H> {
    Start(H),
    Stop(H),
    /// Add it again from `from`'s info, taking `from` out of librqbit first if `take_out`.
    AddAgain {
        from: H,
        take_out: bool,
    },
}

impl<H> State<H> {
    /// librqbit's torrent, whose info it has: one librqbit has or had.
    pub fn info(&self) -> Option<&H> {
        match self {
            State::Has(h) | State::TakenOut(h) => Some(h),
            State::Adding { .. } | State::NotAdded { .. } => None,
        }
    }
}

/// What `event` does to the librqbit side `s`. Never refused: an add's end is a fact.
pub fn decide<H: Clone>(s: &State<H>, event: &Event<H>) -> State<H> {
    use Event as E;
    use State as S;
    match (s, event) {
        (S::Adding { .. } | S::Has(_) | S::TakenOut(_), E::Added(h)) => S::Has(h.clone()),
        (S::Adding { name }, E::FetchFailed) => S::NotAdded { name: name.clone() },
        (S::Has(h) | S::TakenOut(h), E::TakenOut) => S::TakenOut(h.clone()),
        // An add that ended once doesn't end again; one adding never is added again.
        (S::NotAdded { .. }, E::Added(_) | E::FetchFailed | E::TakenOut) => s.clone(),
        (S::Has(_) | S::TakenOut(_), E::FetchFailed) => s.clone(),
        (S::Adding { .. }, E::TakenOut) => s.clone(),
    }
}

/// How `wanted` is carried out on the librqbit side `s`: only on a torrent librqbit has, but for
/// adding again one taken out, which is how it gets back in.
pub fn next<H: Clone>(s: &State<H>, wanted: Wanted) -> Option<Do<H>> {
    use State as S;
    match (s, wanted) {
        (S::Has(h), Wanted::Start) => Some(Do::Start(h.clone())),
        (S::Has(h), Wanted::Stop) => Some(Do::Stop(h.clone())),
        (S::Has(h), Wanted::AddAgain) => Some(Do::AddAgain { from: h.clone(), take_out: true }),
        (S::TakenOut(h), Wanted::AddAgain) => Some(Do::AddAgain { from: h.clone(), take_out: false }),
        (S::TakenOut(_), Wanted::Start | Wanted::Stop) => None,
        (S::Adding { .. } | S::NotAdded { .. }, Wanted::Start | Wanted::Stop | Wanted::AddAgain) => None,
    }
}

/// The librqbit side as a [`Machine`]: [`decide`], never refused, and [`next`]. Only ever named,
/// as a type.
#[allow(dead_code)]
pub struct InLib<H>(PhantomData<H>);

impl<H: Clone> Machine for InLib<H> {
    type State = State<H>;
    type Event = Event<H>;
    type Refusal = Infallible;
    type World = Wanted;
    type Do = Do<H>;

    fn decide(state: &State<H>, event: &Event<H>) -> Result<State<H>, Infallible> {
        Ok(decide(state, event))
    }

    fn next(state: &State<H>, wanted: &Wanted) -> Option<Do<H>> {
        next(state, *wanted)
    }
}

#[cfg(test)]
mod tests {
    use lifecycle::every;
    use lifecycle::machine::{Row, table};

    use super::*;

    /// librqbit's torrents, as numbers: 1 the one it has, 2 the one an add gives.
    type S = State<u8>;

    every!(S {
        S::Adding { .. } => [S::Adding { name: None }, S::Adding { name: Some("pack".into()) }],
        S::NotAdded { .. } => [S::NotAdded { name: Some("pack".into()) }],
        S::Has(_) => [S::Has(1)],
        S::TakenOut(_) => [S::TakenOut(1)],
    });

    every!(Event<u8> { Event::Added(_) => [Event::Added(2)], Event::FetchFailed => [Event::FetchFailed], Event::TakenOut => [Event::TakenOut] });

    fn name(s: &S) -> Option<&Option<String>> {
        match s {
            S::Adding { name } | S::NotAdded { name } => Some(name),
            S::Has(_) | S::TakenOut(_) => None,
        }
    }

    #[test]
    fn every_state_and_add_ending_is_decided_as_its_row_says() {
        let rows: [Row<S, Event<u8>, S>; 4] = [
            Row {
                says: "added, first or again, librqbit has the torrent the add gave",
                when: |s, e| matches!(e, Event::Added(_)) && !matches!(s, S::NotAdded { .. }),
                then: |_, _, n| *n == S::Has(2),
            },
            Row {
                says: "fetching info failing leaves it never added, with its name",
                when: |s, e| matches!(e, Event::FetchFailed) && matches!(s, S::Adding { .. }),
                then: |s, _, n| matches!(n, S::NotAdded { .. }) && name(n) == name(s),
            },
            Row {
                says: "adding again failing leaves it taken out, with the info it had",
                when: |s, e| matches!(e, Event::TakenOut) && s.info().is_some(),
                then: |s, _, n| *n == S::TakenOut(*s.info().unwrap()),
            },
            Row {
                says: "an add ends once: anything else changes nothing",
                when: |s, e| {
                    matches!(s, S::NotAdded { .. })
                        || matches!((s, e), (S::Has(_) | S::TakenOut(_), Event::FetchFailed) | (S::Adding { .. }, Event::TakenOut))
                },
                then: |s, _, n| n == s,
            },
        ];
        table(decide, &rows, &[]);
    }

    #[test]
    fn what_is_wanted_is_carried_out_only_on_a_torrent_librqbit_has_but_adding_one_taken_out_again() {
        let rows: [Row<S, Wanted, Option<Do<u8>>>; 3] = [
            Row {
                says: "on one it has, each is done to it, taking it out to add it again",
                when: |s, _| matches!(s, S::Has(_)),
                then: |_, w, todo| {
                    *todo
                        == Some(match w {
                            Wanted::Start => Do::Start(1),
                            Wanted::Stop => Do::Stop(1),
                            Wanted::AddAgain => Do::AddAgain { from: 1, take_out: true },
                        })
                },
            },
            Row {
                says: "one taken out is only added again, from the info it had, with nothing to take out",
                when: |s, _| matches!(s, S::TakenOut(_)),
                then: |_, w, todo| *todo == (*w == Wanted::AddAgain).then_some(Do::AddAgain { from: 1, take_out: false }),
            },
            Row { says: "without info nothing is done", when: |s, _| s.info().is_none(), then: |_, _, todo| todo.is_none() },
        ];
        table(|s, w| next(s, *w), &rows, &[]);
    }
}
