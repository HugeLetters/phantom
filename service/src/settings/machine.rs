//! The listening port (#65): a port picked once and kept, or one the user pinned. The service
//! decides it at every start of the engine, from whether the port is free, and on the user's set
//! and unset.
//!
//! | State | At start | Becomes | Listens on |
//! |---|---|---|---|
//! | Unpicked | | Picked(fresh), saved | fresh |
//! | Picked(p) | p free | unchanged | p |
//! | Picked(p) | p taken | Picked(fresh), saved | fresh |
//! | Pinned(p) | p free | unchanged | p |
//! | Pinned(p) | p taken | unchanged | a temporary port, with a warning |
//!
//! Set pins a port; Unset keeps the port, picked rather than pinned, so it changes only once
//! another program takes it.

use std::convert::Infallible;
use std::num::NonZeroU16;

/// The port as `settings.json` keeps it. Clients only ever see it picked or pinned ([`State::sent`]):
/// the engine has started, which picks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not picked yet: a fresh data dir, until the first start.
    Unpicked,
    /// Picked by the service; replaced only when another program has taken it.
    Picked(NonZeroU16),
    /// Set by the user; while another program has it, a temporary port is used.
    Pinned(NonZeroU16),
}

impl State {
    pub fn port(self) -> Option<NonZeroU16> {
        match self {
            State::Unpicked => None,
            State::Picked(port) | State::Pinned(port) => Some(port),
        }
    }

    /// As clients see it; `None` before a start picked one.
    pub fn sent(self) -> Option<proto::ListenPort> {
        match self {
            State::Unpicked => None,
            State::Picked(port) => Some(proto::ListenPort::Picked(port)),
            State::Pinned(port) => Some(proto::ListenPort::Pinned(port)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// The user's port.
    Set(NonZeroU16),
    /// Let the service pick.
    Unset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// The engine is starting: whether the state's port is free (an unpicked one never is), and
    /// a port that's free now, from the OS.
    Starting { free: bool, fresh: NonZeroU16 },
}

/// Nothing is refused: a port is parsed, nonzero, before it gets here.
pub type Refusal = Infallible;

/// What the service carries out once it has decided a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Listen(NonZeroU16),
    /// The pinned port is taken: listen here this session, and say so.
    ListenTemporarily(NonZeroU16),
}

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    use Command::*;
    use State::*;
    Ok(match (*state, *event) {
        (Unpicked | Picked(_) | Pinned(_), Event::Command(Set(port))) => Pinned(port),
        (Unpicked, Event::Command(Unset)) => Unpicked,
        (Picked(port) | Pinned(port), Event::Command(Unset)) => Picked(port),
        (Unpicked, Event::Observed(Observed::Starting { fresh, .. })) => Picked(fresh),
        (Picked(port), Event::Observed(Observed::Starting { free: true, .. })) => Picked(port),
        (Picked(_), Event::Observed(Observed::Starting { free: false, fresh })) => Picked(fresh),
        (Pinned(port), Event::Observed(Observed::Starting { .. })) => Pinned(port),
    })
}

/// Where to listen, in the state a start was decided into: nowhere before one.
pub fn next(state: &State, observed: &Observed) -> Option<Do> {
    let Observed::Starting { free, fresh } = *observed;
    match *state {
        State::Unpicked => None,
        State::Picked(port) => Some(Do::Listen(port)),
        State::Pinned(port) if free => Some(Do::Listen(port)),
        State::Pinned(_) => Some(Do::ListenTemporarily(fresh)),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn port_() -> impl Strategy<Value = NonZeroU16> {
        (1..=u16::MAX).prop_map(|p| NonZeroU16::new(p).unwrap())
    }

    fn state() -> impl Strategy<Value = State> {
        prop_oneof![Just(State::Unpicked), port_().prop_map(State::Picked), port_().prop_map(State::Pinned)]
    }

    fn observed() -> impl Strategy<Value = Observed> {
        (any::<bool>(), port_()).prop_map(|(free, fresh)| Observed::Starting { free, fresh })
    }

    fn event() -> impl Strategy<Value = Event> {
        prop_oneof![
            port_().prop_map(|p| Event::Command(Command::Set(p))),
            Just(Event::Command(Command::Unset)),
            observed().prop_map(Event::Observed),
        ]
    }

    /// Where a start listens: decided, then carried out.
    fn start(state: &State, observed: Observed) -> (State, Do) {
        let after = decide(state, &Event::Observed(observed)).unwrap();
        (after, next(&after, &observed).expect("a start always listens"))
    }

    proptest! {
        /// Every row of the table in the module's docs, for every state × start.
        #[test]
        fn a_start_follows_the_table(state in state(), observed in observed()) {
            let Observed::Starting { free, fresh } = observed;
            let (after, listen) = start(&state, observed);
            let expected = match state {
                State::Unpicked => (State::Picked(fresh), Do::Listen(fresh)),
                State::Picked(p) if free => (State::Picked(p), Do::Listen(p)),
                State::Picked(_) => (State::Picked(fresh), Do::Listen(fresh)),
                State::Pinned(p) if free => (State::Pinned(p), Do::Listen(p)),
                State::Pinned(p) => (State::Pinned(p), Do::ListenTemporarily(fresh)),
            };
            prop_assert_eq!((after, listen), expected);
        }

        #[test]
        fn a_pinned_port_never_changes_but_by_the_user(port in port_(), events in proptest::collection::vec(event(), 0..12)) {
            let mut state = State::Pinned(port);
            for event in events {
                state = decide(&state, &event).unwrap();
                match event {
                    Event::Command(_) => return Ok(()),
                    Event::Observed(_) => prop_assert_eq!(state, State::Pinned(port)),
                }
            }
        }

        #[test]
        fn a_picked_port_changes_only_when_its_taken(state in state(), event in event()) {
            let after = decide(&state, &event).unwrap();
            if let (State::Picked(p), State::Picked(q)) = (state, after) && p != q {
                let taken = matches!(event, Event::Observed(Observed::Starting { free: false, .. }));
                prop_assert!(taken, "{:?} changed {:?} to {:?}", event, state, after);
            }
        }

        #[test]
        fn the_user_sets_and_unsets(state in state(), port in port_()) {
            prop_assert_eq!(decide(&state, &Event::Command(Command::Set(port))).unwrap(), State::Pinned(port));
            let unset = decide(&state, &Event::Command(Command::Unset)).unwrap();
            prop_assert_eq!(unset.port(), state.port(), "unset keeps the port");
            prop_assert!(!matches!(unset, State::Pinned(_)));
        }

        /// After any history, a start listens on a port: never 0, never nowhere.
        #[test]
        fn every_start_listens(state in state(), events in proptest::collection::vec(event(), 0..12), observed in observed()) {
            let state = events.iter().fold(state, |s, e| decide(&s, e).unwrap());
            let (_, listen) = start(&state, observed);
            let (Do::Listen(port) | Do::ListenTemporarily(port)) = listen;
            prop_assert_ne!(port.get(), 0);
        }
    }
}
