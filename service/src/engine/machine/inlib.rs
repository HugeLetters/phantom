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
//! Has --Added (again)--> Has'      Has, Halted, TakenOut --TakenOut--> TakenOut
//! TakenOut, Halted --Added (again)--> Has'
//! Has --Paused while checking--> Halted     Halted --Started--> Has
//! Has --AddedBack--> Resuming --Initialized--> Has   (#234)
//! Kept --AddedBack--> Resuming   (a crash between taking it out and adding it back, #234)
//! ```
//!
//! [`State::Halted`] is a fact about librqbit that its own state hides: a pause ends a check
//! between pieces but leaves the torrent Initializing, the same as one checking, so only the
//! actor's own pause of a checking torrent tells them apart.
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
    /// librqbit has it, Initializing, its check ended by a pause: it checks nothing until it's
    /// started or added again.
    Halted(H),
    /// librqbit had it, until adding it again failed after taking it out. Its info is still there
    /// to add it again from.
    TakenOut(H),
    /// The actor added it back with the pieces it had (#234, letting go of a file gone from the
    /// Trash), and librqbit is reading a few of them to trust the rest: nothing is done to it, and
    /// its lifecycle hears nothing, until that's over. Its row stays as it was.
    Resuming(H),
    /// Out of librqbit, kept aside to let go of a file gone from the Trash (#234), a crash having
    /// cut that short before it was added back: its info is in the stash, which adds it back.
    Kept { name: Option<String> },
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
    /// The actor paused it; `checking`: librqbit was checking it, which that ends.
    Paused { checking: bool },
    /// The actor started it.
    Started,
    /// The actor added it back as this torrent, with the pieces it had.
    AddedBack(H),
    /// librqbit is done reading the pieces it was added back with: it trusts them, or checks it in
    /// full, which is its lifecycle's to hear.
    Initialized,
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
    /// Add it back from what was kept of it.
    AddBack,
}

impl<H> State<H> {
    /// librqbit's torrent, whose info it has: one librqbit has or had.
    pub fn info(&self) -> Option<&H> {
        match self {
            State::Has(h) | State::Halted(h) | State::TakenOut(h) | State::Resuming(h) => Some(h),
            State::Adding { .. } | State::NotAdded { .. } | State::Kept { .. } => None,
        }
    }
}

/// What `event` does to the librqbit side `s`. Never refused: an add's end is a fact.
pub fn decide<H: Clone>(s: &State<H>, event: &Event<H>) -> State<H> {
    use Event as E;
    use State as S;
    match (s, event) {
        (S::Adding { .. } | S::Has(_) | S::Halted(_) | S::TakenOut(_) | S::Resuming(_) | S::Kept { .. }, E::Added(h)) => S::Has(h.clone()),
        (S::Adding { name }, E::FetchFailed) => S::NotAdded { name: name.clone() },
        (S::Has(h) | S::Halted(h) | S::TakenOut(h) | S::Resuming(h), E::TakenOut) => S::TakenOut(h.clone()),
        // Pausing a check halts it; pausing anything else, or what's halted, changes nothing here.
        (S::Has(h), E::Paused { checking: true }) => S::Halted(h.clone()),
        (S::Has(_) | S::Halted(_), E::Paused { checking: false }) | (S::Halted(_), E::Paused { checking: true }) => s.clone(),
        // Started, it checks again if it was halted.
        (S::Halted(h), E::Started) => S::Has(h.clone()),
        (S::Has(_), E::Started) => s.clone(),
        // Nothing paused or started without it, or taken out, or being added.
        (S::Adding { .. } | S::NotAdded { .. } | S::TakenOut(_), E::Paused { .. } | E::Started) => s.clone(),
        // An add that ended once doesn't end again; one adding never is added again.
        (S::NotAdded { .. }, E::Added(_) | E::FetchFailed | E::TakenOut) => s.clone(),
        (S::Has(_) | S::Halted(_) | S::TakenOut(_) | S::Resuming(_), E::FetchFailed) => s.clone(),
        (S::Adding { .. }, E::TakenOut) => s.clone(),
        // Nothing pauses or starts it while it's resuming (the actor does nothing to it).
        (S::Resuming(_), E::Paused { .. } | E::Started) => s.clone(),
        // Added back with its pieces, librqbit having had it: resuming until it's initialized.
        (S::Has(_) | S::Halted(_) | S::TakenOut(_) | S::Resuming(_), E::AddedBack(h)) => S::Resuming(h.clone()),
        (S::Kept { .. }, E::AddedBack(h)) => S::Resuming(h.clone()),
        (S::Adding { .. } | S::NotAdded { .. }, E::AddedBack(_)) => s.clone(),
        // Kept, it's only added back: anything else changes nothing (adding it back failing too).
        (S::Kept { .. }, E::FetchFailed | E::TakenOut | E::Paused { .. } | E::Started | E::Initialized) => s.clone(),
        (S::Resuming(h), E::Initialized) => S::Has(h.clone()),
        (S::Adding { .. } | S::NotAdded { .. } | S::Has(_) | S::Halted(_) | S::TakenOut(_), E::Initialized) => s.clone(),
    }
}

/// How `wanted` is carried out on the librqbit side `s`: only on a torrent librqbit has, but for
/// adding again one taken out, which is how it gets back in.
pub fn next<H: Clone>(s: &State<H>, wanted: Wanted) -> Option<Do<H>> {
    use State as S;
    match (s, wanted) {
        (S::Has(h), Wanted::Start) => Some(Do::Start(h.clone())),
        (S::Has(h), Wanted::Stop) => Some(Do::Stop(h.clone())),
        // Halted, it's stopped already: starting it would only check it again from the start.
        (S::Halted(_), Wanted::Stop) => None,
        (S::Halted(h), Wanted::Start) => Some(Do::Start(h.clone())),
        (S::Has(h) | S::Halted(h), Wanted::AddAgain) => Some(Do::AddAgain { from: h.clone(), take_out: true }),
        (S::TakenOut(h), Wanted::AddAgain) => Some(Do::AddAgain { from: h.clone(), take_out: false }),
        (S::TakenOut(_), Wanted::Start | Wanted::Stop) => None,
        (S::Adding { .. } | S::NotAdded { .. }, Wanted::Start | Wanted::Stop | Wanted::AddAgain) => None,
        // What's wanted is done once it's initialized.
        (S::Resuming(_), Wanted::Start | Wanted::Stop | Wanted::AddAgain) => None,
        // Kept aside: added back as an add again (Resume, Check), nothing else.
        (S::Kept { .. }, Wanted::AddAgain) => Some(Do::AddBack),
        (S::Kept { .. }, Wanted::Start | Wanted::Stop) => None,
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
        S::Halted(_) => [S::Halted(1)],
        S::TakenOut(_) => [S::TakenOut(1)],
        S::Resuming(_) => [S::Resuming(1)],
        S::Kept { .. } => [S::Kept { name: Some("pack".into()) }],
    });

    every!(Event<u8> {
        Event::Added(_) => [Event::Added(2)],
        Event::FetchFailed => [Event::FetchFailed],
        Event::TakenOut => [Event::TakenOut],
        Event::Paused { .. } => [Event::Paused { checking: true }, Event::Paused { checking: false }],
        Event::Started => [Event::Started],
        Event::AddedBack(_) => [Event::AddedBack(2)],
        Event::Initialized => [Event::Initialized],
    });

    fn name(s: &S) -> Option<&Option<String>> {
        match s {
            S::Adding { name } | S::NotAdded { name } | S::Kept { name } => Some(name),
            S::Has(_) | S::Halted(_) | S::TakenOut(_) | S::Resuming(_) => None,
        }
    }

    #[test]
    fn every_state_and_add_ending_is_decided_as_its_row_says() {
        let rows: [Row<S, Event<u8>, S>; 8] = [
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
                says: "pausing a check that librqbit does halts it, and nothing else does; started again, it's librqbit's to check",
                when: |s, e| matches!(e, Event::Paused { .. } | Event::Started) && matches!(s, S::Has(_) | S::Halted(_)),
                then: |s, e, n| match (s, e) {
                    (S::Has(h), Event::Paused { checking: true }) => *n == S::Halted(*h),
                    (S::Halted(h), Event::Started) => *n == S::Has(*h),
                    _ => n == s,
                },
            },
            Row {
                says: "one taken out, resuming or not here isn't paused or started",
                when: |s, e| matches!(e, Event::Paused { .. } | Event::Started) && !matches!(s, S::Has(_) | S::Halted(_)),
                then: |s, _, n| n == s,
            },
            Row {
                says: "added back with its pieces, one librqbit had or kept aside resumes as the torrent the add gave; one without info isn't added back",
                when: |_, e| matches!(e, Event::AddedBack(_)),
                then: |s, _, n| if s.info().is_some() || matches!(s, S::Kept { .. }) { *n == S::Resuming(2) } else { n == s },
            },
            Row {
                says: "initialized, one resuming is librqbit's again, as it is; nothing else changes",
                when: |_, e| matches!(e, Event::Initialized),
                then: |s, _, n| match s {
                    S::Resuming(h) => *n == S::Has(*h),
                    _ => n == s,
                },
            },
            Row {
                says: "an add ends once: anything else changes nothing",
                when: |s, e| {
                    matches!(s, S::NotAdded { .. }) && matches!(e, Event::Added(_) | Event::FetchFailed | Event::TakenOut)
                        || matches!(
                            (s, e),
                            (S::Has(_) | S::Halted(_) | S::TakenOut(_) | S::Resuming(_), Event::FetchFailed)
                                | (S::Adding { .. }, Event::TakenOut)
                                | (S::Kept { .. }, Event::FetchFailed | Event::TakenOut)
                        )
                },
                then: |s, _, n| n == s,
            },
        ];
        table(decide, &rows, &[]);
    }

    #[test]
    fn what_is_wanted_is_carried_out_only_on_a_torrent_librqbit_has_but_adding_one_taken_out_again() {
        let rows: [Row<S, Wanted, Option<Do<u8>>>; 6] = [
            Row {
                says: "on one halted, it's started, or taken out to add it again, and never stopped again",
                when: |s, _| matches!(s, S::Halted(_)),
                then: |_, w, todo| {
                    *todo
                        == match w {
                            Wanted::Start => Some(Do::Start(1)),
                            Wanted::Stop => None,
                            Wanted::AddAgain => Some(Do::AddAgain { from: 1, take_out: true }),
                        }
                },
            },
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
            Row {
                says: "without info nothing is done",
                when: |s, _| s.info().is_none() && !matches!(s, S::Kept { .. }),
                then: |_, _, todo| todo.is_none(),
            },
            Row {
                says: "kept aside, it's added back as an add again, and nothing else is done",
                when: |s, _| matches!(s, S::Kept { .. }),
                then: |_, w, todo| *todo == (*w == Wanted::AddAgain).then_some(Do::AddBack),
            },
            Row {
                says: "resuming, nothing is done until it's initialized",
                when: |s, _| matches!(s, S::Resuming(_)),
                then: |_, _, todo| todo.is_none(),
            },
        ];
        table(|s, w| next(s, *w), &rows, &[]);
    }
}
