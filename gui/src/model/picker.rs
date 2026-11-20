//! The folder picker's queue as a machine (#321, from #110): adds the service refused for want of a
//! destination wait here, and the picker asks about them one at a time, first come first asked.
//! Pure; the model keeps what each ticket stands for (its requests and batch) and opens the
//! picker when [`next`] says so.
//!
//! | State \ Event | Refused(t)                                  | Chosen / Cancelled / Failed         |
//! |---------------|---------------------------------------------|-------------------------------------|
//! | Closed        | Open { asking: t, waiting: [] }             | refused: nothing was asked          |
//! | Open { a, q } | Open { a, q + t }; refused if t is a or in q | the first of q asked, else Closed  |
//!
//! Four properties hold on every row: at most one picker is open (one `asking`); every refused add
//! is answered exactly once (a ticket leaves the machine only as the one asked, by an answer);
//! the queue is first in, first out; and it is never `Closed` with something waiting (`Closed` has
//! no queue, so an answer that finds one opens the next).
//!
//! How an add was answered (a folder chosen, the picker cancelled, or failing to open) is three
//! events but one transition: what each does to the add is the model's, not the queue's.

use std::collections::VecDeque;

/// Stands for one refused add (its requests and its batch, which the model keeps) in the queue.
pub type Ticket = u64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    /// No picker is open, and none waits.
    #[default]
    Closed,
    /// The picker asks about `asking`; the others wait their turn, oldest first.
    Open { asking: Ticket, waiting: VecDeque<Ticket> },
}

impl State {
    /// The add the picker asks about.
    pub fn asking(&self) -> Option<Ticket> {
        match self {
            State::Closed => None,
            State::Open { asking, .. } => Some(*asking),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The service refused this add for want of a destination.
    Refused(Ticket),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The picker's answer to the add it asked about: a folder.
    Chosen,
    /// The user closed the picker.
    Cancelled,
    /// The picker couldn't be asked, or its answer couldn't be used.
    Failed,
}

/// What the model carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Open the picker for this add.
    Ask(Ticket),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// An answer with no picker open: it's not the queue's.
    NothingAsked,
    /// This add is in the queue already (or being asked): it's answered once.
    AlreadyHere(Ticket),
}

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    use Event::{Command as C, Observed as O};
    match (state, event) {
        (State::Closed, C(Command::Refused(t))) => Ok(State::Open { asking: *t, waiting: VecDeque::new() }),
        (State::Open { asking, waiting }, C(Command::Refused(t))) if asking == t || waiting.contains(t) => Err(Refusal::AlreadyHere(*t)),
        (State::Open { asking, waiting }, C(Command::Refused(t))) => {
            let mut waiting = waiting.clone();
            waiting.push_back(*t);
            Ok(State::Open { asking: *asking, waiting })
        }
        (State::Closed, O(Observed::Chosen | Observed::Cancelled | Observed::Failed)) => Err(Refusal::NothingAsked),
        (State::Open { waiting, .. }, O(Observed::Chosen | Observed::Cancelled | Observed::Failed)) => {
            let mut waiting = waiting.clone();
            Ok(match waiting.pop_front() {
                Some(asking) => State::Open { asking, waiting },
                None => State::Closed,
            })
        }
    }
}

/// What opens the picker: the add it should ask about, while it isn't asking about it (`open` is
/// the add its picker is open for now). Level-triggered: asking again is safe.
pub fn next(state: &State, open: &Option<Ticket>) -> Option<Do> {
    state.asking().filter(|asking| open.as_ref() != Some(asking)).map(Do::Ask)
}

/// The queue as a [`Machine`](machine::Machine), given the add its picker is open for. Only ever
/// named, as a type.
#[allow(dead_code)]
pub struct Picker;

impl machine::Machine for Picker {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = Option<Ticket>;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, open: &Option<Ticket>) -> Option<Do> {
        next(state, open)
    }
}

#[cfg(test)]
mod tests {
    use machine::every;
    use machine::{DecideRow, Row, decide_table, sequences};

    use super::*;

    impl State {
        /// Every add in the machine: the one asked, then those waiting, in order.
        fn tickets(&self) -> Vec<Ticket> {
            match self {
                State::Closed => Vec::new(),
                State::Open { asking, waiting } => std::iter::once(*asking).chain(waiting.iter().copied()).collect(),
            }
        }
    }

    fn open(asking: Ticket, waiting: &[Ticket]) -> State {
        State::Open { asking, waiting: waiting.iter().copied().collect() }
    }

    // Tickets 1 to 3 are in the machine in some states; 4 is never in one.
    every!(State {
        State::Closed => [State::Closed],
        State::Open { .. } => [open(1, &[]), open(1, &[2]), open(1, &[2, 3]), open(2, &[1, 3])],
    });

    every!(Command { Command::Refused(_) => [1, 2, 4].map(Command::Refused) });

    every!(Observed {
        Observed::Chosen => [Observed::Chosen],
        Observed::Cancelled => [Observed::Cancelled],
        Observed::Failed => [Observed::Failed],
    });

    every!(Event {
        Event::Command(_) => <Command as machine::Every>::every().into_iter().map(Event::Command),
        Event::Observed(_) => <Observed as machine::Every>::every().into_iter().map(Event::Observed),
    });

    type R = DecideRow<Picker>;

    fn refused(e: &Event) -> Option<Ticket> {
        match e {
            Event::Command(Command::Refused(t)) => Some(*t),
            Event::Observed(_) => None,
        }
    }

    fn answered(e: &Event) -> bool {
        matches!(e, Event::Observed(_))
    }

    fn rows() -> Vec<R> {
        vec![
            Row {
                says: "an add refused with no picker open is asked about at once, with nothing waiting",
                when: |s, e| refused(e).is_some() && *s == State::Closed,
                then: |_, e, r| *r == Ok(open(refused(e).unwrap(), &[])),
            },
            Row {
                says: "an add refused while the picker asks waits last, and the one asked stays",
                when: |s, e| refused(e).is_some_and(|t| *s != State::Closed && !s.tickets().contains(&t)),
                then: |s, e, r| {
                    let t = refused(e).unwrap();
                    let State::Open { asking, waiting } = s else { return false };
                    let mut want = waiting.clone();
                    want.push_back(t);
                    *r == Ok(State::Open { asking: *asking, waiting: want })
                },
            },
            Row {
                says: "an add that's in the queue already, or asked about, is refused: it's answered once",
                when: |s, e| refused(e).is_some_and(|t| s.tickets().contains(&t)),
                then: |_, e, r| *r == Err(Refusal::AlreadyHere(refused(e).unwrap())),
            },
            Row {
                says: "an answer closes the picker, or opens it on the one that waited longest; the answered add is gone",
                when: |s, e| answered(e) && *s != State::Closed,
                then: |s, _, r| {
                    let State::Open { waiting, .. } = s else { return false };
                    *r == Ok(match waiting.front() {
                        Some(first) => open(*first, &waiting.iter().skip(1).copied().collect::<Vec<_>>()),
                        None => State::Closed,
                    })
                },
            },
            Row {
                says: "an answer with no picker open is refused",
                when: |s, e| answered(e) && *s == State::Closed,
                then: |_, _, r| *r == Err(Refusal::NothingAsked),
            },
        ]
    }

    fn always() -> Vec<R> {
        vec![
            Row::always("no add is in the machine twice", |_, _, r| {
                r.as_ref().map_or(true, |n| {
                    let mut all = n.tickets();
                    all.sort();
                    all.windows(2).all(|w| w[0] != w[1])
                })
            }),
            Row::always("an add leaves only as the one asked, by an answer, and none is lost or made up", |s, e, r| {
                let Ok(n) = r else { return true };
                let (before, after) = (s.tickets(), n.tickets());
                match e {
                    Event::Command(Command::Refused(t)) => {
                        after.len() == before.len() + 1 && after.last() == Some(t) && after[..before.len()] == before[..]
                    }
                    Event::Observed(_) => before.split_first().is_some_and(|(_, rest)| after == rest),
                }
            }),
            Row::always("the queue is first in, first out: those waiting keep their order", |s, _, r| {
                let Ok(n) = r else { return true };
                let (before, after) = (s.tickets(), n.tickets());
                after.iter().filter(|t| before.contains(t)).copied().eq(before.iter().filter(|t| after.contains(t)).copied())
            }),
            Row::always("the picker is never closed while an add waits: one's asked about when any are in the machine", |_, _, r| {
                r.as_ref().map_or(true, |n| n.asking() == n.tickets().first().copied())
            }),
        ]
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        decide_table::<Picker>(&rows(), &always());
    }

    #[test]
    fn every_state_reached_is_decided_as_its_row_says() {
        sequences::<Picker>(<State as machine::Every>::every(), 16, &rows(), &always());
    }

    #[test]
    fn it_opens_the_picker_for_the_one_asked_until_it_is_open_for_it() {
        for s in <State as machine::Every>::every() {
            let asking = s.asking();
            assert_eq!(next(&s, &asking), None, "{s:?}");
            assert_eq!(next(&s, &None), asking.map(Do::Ask), "{s:?}");
            assert_eq!(next(&s, &Some(9)), asking.map(Do::Ask), "{s:?}");
        }
    }
}
