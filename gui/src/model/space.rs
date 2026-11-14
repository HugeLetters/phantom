//! The disk warning's list (#127), as a machine: a click on a disk's warning in the status bar
//! opens the list of its torrents, with what each needs and Pause; it closes when asked, or by
//! itself once the latest snapshot has that disk no longer short. So it never shows a disk that
//! fits, nor opens again by itself.
//!
//! | State \ Event | Open(d) | Close | Snapshot(short) |
//! |---|---|---|---|
//! | Closed | Open(d) | Closed | Closed |
//! | Open(o) | Open(d) | Closed | Open(o) if o is short, else Closed |

use std::convert::Infallible;

use lifecycle::every;
use proto::{AbsolutePath, Volume};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Closed,
    /// The list of the disk mounted there.
    Open(AbsolutePath),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A click on that disk's warning.
    Open(AbsolutePath),
    /// A click outside the list, or Escape.
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// A snapshot came: the disks it has short, by mount.
    Snapshot { short: Vec<AbsolutePath> },
}

/// Nothing is refused: a click on a warning drawn is on a disk short in the snapshot drawn.
pub type Refusal = Infallible;

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    Ok(match (state, event) {
        (State::Closed | State::Open(_), Event::Command(Command::Open(disk))) => State::Open(disk.clone()),
        (State::Closed | State::Open(_), Event::Command(Command::Close)) => State::Closed,
        (State::Closed, Event::Observed(Observed::Snapshot { .. })) => State::Closed,
        (State::Open(open), Event::Observed(Observed::Snapshot { short })) if short.contains(open) => state.clone(),
        (State::Open(_), Event::Observed(Observed::Snapshot { .. })) => State::Closed,
    })
}

/// The disk warning's list as a [`Machine`](lifecycle::machine::Machine). Only ever named.
#[allow(dead_code)]
pub struct DiskWarning;

impl lifecycle::machine::Machine for DiskWarning {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = ();
    /// Nothing to carry out: drawing follows the state.
    type Do = Infallible;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(_: &State, _: &()) -> Option<Infallible> {
        None
    }
}

/// The mounts of the disks short in `volumes`.
pub fn short(volumes: &[Volume]) -> Vec<AbsolutePath> {
    volumes.iter().filter(|v| v.short()).map(|v| v.mount.clone()).collect()
}

fn disk(name: &str) -> AbsolutePath {
    AbsolutePath::new(format!("/Volumes/{name}")).expect("absolute")
}

every!(State {
    State::Closed => [State::Closed],
    State::Open(_) => [State::Open(disk("A")), State::Open(disk("B"))],
});

every!(Event {
    Event::Command(Command::Open(_)) => [Event::Command(Command::Open(disk("A"))), Event::Command(Command::Open(disk("B")))],
    Event::Command(Command::Close) => [Event::Command(Command::Close)],
    Event::Observed(Observed::Snapshot { .. }) => [vec![], vec![disk("A")], vec![disk("B")], vec![disk("A"), disk("B")]]
        .map(|short| Event::Observed(Observed::Snapshot { short })),
});

#[cfg(test)]
mod tests {
    use lifecycle::machine::{DecideRow, Every, Row, decide_table, sequences};

    use super::*;

    fn rows() -> Vec<DecideRow<DiskWarning>> {
        vec![
            Row {
                says: "a click opens that disk's list",
                when: |_, e| matches!(e, Event::Command(Command::Open(_))),
                then: |_, e, out| matches!((e, out), (Event::Command(Command::Open(d)), Ok(State::Open(o))) if d == o),
            },
            Row { says: "closing closes", when: |_, e| *e == Event::Command(Command::Close), then: |_, _, out| *out == Ok(State::Closed) },
            Row {
                says: "a snapshot opens nothing",
                when: |s, e| *s == State::Closed && matches!(e, Event::Observed(_)),
                then: |_, _, out| *out == Ok(State::Closed),
            },
            Row {
                says: "a snapshot keeps the list open exactly while its disk is short",
                when: |s, e| matches!(s, State::Open(_)) && matches!(e, Event::Observed(_)),
                then: |s, e, out| {
                    let (State::Open(open), Event::Observed(Observed::Snapshot { short })) = (s, e) else { return false };
                    *out == Ok(if short.contains(open) { s.clone() } else { State::Closed })
                },
            },
        ]
    }

    fn always() -> Vec<DecideRow<DiskWarning>> {
        vec![Row::always("nothing is refused", |_, _, out| out.is_ok())]
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<DiskWarning>(&rows(), &always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        sequences::<DiskWarning>(State::every(), 12, &rows(), &always());
    }
}
