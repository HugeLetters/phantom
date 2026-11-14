//! Whether the Mac is kept awake (#122): held exactly while the setting is on and some torrent is
//! downloading, and never once the service stops.
//!
//! | State | Event | Becomes |
//! |---|---|---|
//! | Running | Set(on) | Running, the setting `on` |
//! | Running | Downloading(d) | Running, downloading `d` |
//! | any | Stop | Stopped |
//! | Stopped | Set, Downloading | Stopped |
//!
//! | Wanted held | The assertion | Do |
//! |---|---|---|
//! | yes | released | Hold |
//! | no | held | Release |
//! | as it is | | nothing |

use std::convert::Infallible;

use lifecycle::every;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The service runs: whether the setting is on, and whether some torrent is downloading.
    Running { on: bool, downloading: bool },
    /// The service is stopping: nothing is held from now on.
    Stopped,
}

impl State {
    /// Whether the Mac is to be kept awake.
    pub fn held(self) -> bool {
        matches!(self, State::Running { on: true, downloading: true })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// The setting, turned on or off.
    Set(bool),
    /// The service stops: an idle shutdown, or asked to.
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// The engine's latest snapshot: whether some torrent in it is Downloading.
    Downloading(bool),
}

/// Whether the actor holds the power assertion now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assertion {
    Released,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Hold,
    Release,
}

/// Nothing is refused: a setting is a bool, and a stop always stops.
pub type Refusal = Infallible;

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    use State::*;
    Ok(match (*state, *event) {
        (Running { downloading, .. }, Event::Command(Command::Set(on))) => Running { on, downloading },
        (Running { on, .. }, Event::Observed(Observed::Downloading(downloading))) => Running { on, downloading },
        (Running { .. } | Stopped, Event::Command(Command::Stop)) => Stopped,
        (Stopped, Event::Command(Command::Set(_)) | Event::Observed(Observed::Downloading(_))) => Stopped,
    })
}

/// What makes the assertion match the state.
pub fn next(state: &State, assertion: &Assertion) -> Option<Do> {
    match (state.held(), *assertion) {
        (true, Assertion::Released) => Some(Do::Hold),
        (false, Assertion::Held) => Some(Do::Release),
        (true, Assertion::Held) | (false, Assertion::Released) => None,
    }
}

/// Keeping the Mac awake as a [`Machine`](lifecycle::machine::Machine), for the harness. Only ever
/// named, as a type.
#[allow(dead_code)]
pub struct KeepAwake;

impl lifecycle::machine::Machine for KeepAwake {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = Assertion;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, assertion: &Assertion) -> Option<Do> {
        next(state, assertion)
    }
}

const BOOLS: [bool; 2] = [false, true];

every!(State {
    State::Running { .. } => BOOLS.into_iter().flat_map(|on| BOOLS.map(|downloading| State::Running { on, downloading })),
    State::Stopped => [State::Stopped],
});

every!(Event {
    Event::Command(Command::Set(_)) => BOOLS.map(|on| Event::Command(Command::Set(on))),
    Event::Command(Command::Stop) => [Event::Command(Command::Stop)],
    Event::Observed(Observed::Downloading(_)) => BOOLS.map(|d| Event::Observed(Observed::Downloading(d))),
});

every!(Assertion {
    Assertion::Released => [Assertion::Released],
    Assertion::Held => [Assertion::Held],
});

#[cfg(test)]
mod tests {
    use lifecycle::machine::{DecideRow, Every, Row, decide_table, sequences, table};

    use super::*;

    fn decide_rows() -> Vec<DecideRow<KeepAwake>> {
        vec![
            Row {
                says: "a setting changes only the setting",
                when: |s, e| matches!((s, e), (State::Running { .. }, Event::Command(Command::Set(_)))),
                then: |s, e, out| {
                    let (State::Running { downloading, .. }, Event::Command(Command::Set(on))) = (s, e) else { return false };
                    *out == Ok(State::Running { on: *on, downloading: *downloading })
                },
            },
            Row {
                says: "a snapshot changes only whether something downloads",
                when: |s, e| matches!((s, e), (State::Running { .. }, Event::Observed(_))),
                then: |s, e, out| {
                    let (State::Running { on, .. }, Event::Observed(Observed::Downloading(downloading))) = (s, e) else { return false };
                    *out == Ok(State::Running { on: *on, downloading: *downloading })
                },
            },
            Row { says: "a stop stops", when: |_, e| *e == Event::Command(Command::Stop), then: |_, _, out| *out == Ok(State::Stopped) },
            Row {
                says: "nothing starts it again once stopped",
                when: |s, e| *s == State::Stopped && *e != Event::Command(Command::Stop),
                then: |_, _, out| *out == Ok(State::Stopped),
            },
        ]
    }

    fn decide_always() -> Vec<DecideRow<KeepAwake>> {
        vec![Row::always("nothing is refused", |_, _, out| out.is_ok())]
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<KeepAwake>(&decide_rows(), &decide_always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        let starts = State::every();
        sequences::<KeepAwake>(starts, 12, &decide_rows(), &decide_always());
    }

    /// Every state × assertion: `next` takes it to held exactly when the state wants it, in one
    /// step, and then says nothing more.
    #[test]
    fn next_makes_the_assertion_what_the_state_wants() {
        let rows: Vec<Row<State, Assertion, Option<Do>>> = vec![
            Row { says: "held when wanted", when: |s, a| s.held() && *a == Assertion::Released, then: |_, _, d| *d == Some(Do::Hold) },
            Row {
                says: "released when not wanted, a stopped service's too",
                when: |s, a| !s.held() && *a == Assertion::Held,
                then: |_, _, d| *d == Some(Do::Release),
            },
            Row {
                says: "nothing to do when it's as wanted",
                when: |s, a| s.held() == (*a == Assertion::Held),
                then: |_, _, d| d.is_none(),
            },
        ];
        let always: Vec<Row<State, Assertion, Option<Do>>> = vec![Row::always("one step settles it", |s, a, d| {
            let after = match d {
                Some(Do::Hold) => Assertion::Held,
                Some(Do::Release) => Assertion::Released,
                None => *a,
            };
            next(s, &after).is_none() && (after == Assertion::Held) == s.held()
        })];
        table(next, &rows, &always);
    }
}
