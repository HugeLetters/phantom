//! The service's lifetime (#116): starting, idle or serving clients, stopping, exiting. `main`'s
//! loop is the actor that owns it; `serve` tasks tell it when a connection opens and closes, and
//! read the state it publishes to answer or refuse each request.
//!
//! ```text
//! Starting --lock held--> Exiting      Starting --ready--> Idle
//! Idle <--first opened / last closed--> Serving
//! Idle --no client for IDLE_GRACE--> Stopping     Idle, Serving --Shutdown--> Stopping
//! Stopping --engine stopped--> Exiting
//! ```

use std::time::{Duration, Instant};

use proto::Refusal;

/// How long the service waits, with no client, before it stops.
pub const IDLE_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Taking the data dir's lock, starting the engine, binding the socket.
    Starting,
    /// No client since `since`.
    Idle {
        since: Instant,
    },
    /// `clients` connections open, at least one.
    Serving {
        clients: usize,
    },
    /// Stopping the engine; every request is refused.
    Stopping,
    Exiting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    /// What the actor sees happen, at `at`.
    Observed {
        what: Observed,
        at: Instant,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Any request a client sends: answered unless the service is stopping.
    Request,
    /// A client's `Shutdown`, SIGTERM or SIGINT.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// Another service holds the data dir's lock.
    LockHeld,
    /// The lock is taken, the engine started and the socket bound.
    Ready,
    Opened,
    Closed,
    /// Time passed: the clock reached `at`.
    Tick,
    EngineStopped,
}

/// What the actor carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Tick at this time, unless something happens first.
    WakeAt(Instant),
    StopEngine,
}

/// Only a request is ever refused: while stopping or once stopped. Observations never are.
pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    match event {
        Event::Command(command) => command_(state, command),
        Event::Observed { what, at } => Ok(observe(state, what, *at)),
    }
}

fn command_(state: &State, command: &Command) -> Result<State, Refusal> {
    use Command::*;
    use State::*;
    match (state, command) {
        (Starting | Idle { .. } | Serving { .. }, Request) => Ok(state.clone()),
        (Stopping | Exiting, Request) => Err(Refusal::Stopping),
        (Starting | Idle { .. } | Serving { .. }, Shutdown) => Ok(Stopping),
        (Stopping | Exiting, Shutdown) => Ok(state.clone()),
    }
}

fn observe(state: &State, what: &Observed, at: Instant) -> State {
    use Observed::*;
    use State::*;
    match (state, what) {
        (Starting, LockHeld) => Exiting,
        (Starting, Ready) => Idle { since: at },
        (Starting, Opened | Closed | Tick | EngineStopped) => Starting,
        (Idle { .. }, Opened) => Serving { clients: 1 },
        (Idle { since }, Tick) if at >= *since + IDLE_GRACE => Stopping,
        (Idle { .. }, Tick | Closed | EngineStopped) => state.clone(),
        (Serving { clients }, Opened) => Serving { clients: clients + 1 },
        (Serving { clients: 1 }, Closed) => Idle { since: at },
        (Serving { clients }, Closed) => Serving { clients: clients - 1 },
        (Serving { .. }, Tick | EngineStopped) => state.clone(),
        (Stopping, EngineStopped) => Exiting,
        (Stopping, Opened | Closed | Tick) => Stopping,
        (Idle { .. } | Serving { .. } | Stopping | Exiting, LockHeld | Ready) => state.clone(),
        (Exiting, Opened | Closed | Tick | EngineStopped) => Exiting,
    }
}

/// The service's lifetime as a [`Machine`](lifecycle::machine::Machine): [`decide`] and
/// [`next`], which looks at nothing but the state. Only ever named, as a type.
#[allow(dead_code)]
pub struct Lifetime;

impl lifecycle::machine::Machine for Lifetime {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = ();
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, _: &()) -> Option<Do> {
        next(state)
    }
}

/// What to do in `state`; `None` is waiting for the next event.
pub fn next(state: &State) -> Option<Do> {
    match state {
        State::Idle { since } => Some(Do::WakeAt(*since + IDLE_GRACE)),
        State::Stopping => Some(Do::StopEngine),
        State::Starting | State::Serving { .. } | State::Exiting => None,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// What can happen once the service is up: connections opening and closing, time passing,
    /// a request, a shutdown, the engine stopping (only ever after it was asked to).
    #[derive(Debug, Clone)]
    enum Step {
        Open,
        /// Closes one of the open connections, if any.
        Close,
        Wait(Duration),
        Request,
        Shutdown,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            3 => Just(Step::Open),
            3 => Just(Step::Close),
            4 => (0..2 * IDLE_GRACE.as_millis() as u64).prop_map(|ms| Step::Wait(Duration::from_millis(ms))),
            2 => Just(Step::Request),
            1 => Just(Step::Shutdown),
        ]
    }

    /// Runs `steps` the way `main` does, carrying out each `Do`: the engine stops at once when told
    /// to, and a wake-up comes when it's due, before a later step.
    fn run(start: Instant, steps: &[Step]) -> Vec<(Event, Result<State, Refusal>)> {
        let mut at = start;
        let mut state = decide(&State::Starting, &Event::Observed { what: Observed::Ready, at }).unwrap();
        let mut trace = Vec::new();
        let mut open = 0;
        let apply = |state: &mut State, event: Event, trace: &mut Vec<_>| {
            let decided = decide(state, &event);
            if let Ok(next) = &decided {
                *state = next.clone();
            }
            trace.push((event, decided));
        };
        for step in steps {
            let event = match step {
                Step::Open => {
                    open += 1;
                    Event::Observed { what: Observed::Opened, at }
                }
                Step::Close if open > 0 => {
                    open -= 1;
                    Event::Observed { what: Observed::Closed, at }
                }
                Step::Close => continue,
                Step::Request => Event::Command(Command::Request),
                Step::Shutdown => Event::Command(Command::Shutdown),
                Step::Wait(d) => {
                    let until = at + *d;
                    while let Some(Do::WakeAt(due)) = next(&state)
                        && due <= until
                    {
                        apply(&mut state, Event::Observed { what: Observed::Tick, at: due }, &mut trace);
                    }
                    at = until;
                    Event::Observed { what: Observed::Tick, at }
                }
            };
            apply(&mut state, event, &mut trace);
            if next(&state) == Some(Do::StopEngine) {
                apply(&mut state, Event::Observed { what: Observed::EngineStopped, at }, &mut trace);
            }
        }
        trace
    }

    proptest! {
        /// It stops if and only if it had no client for `IDLE_GRACE` or was asked to; a client
        /// within the grace cancels the stop.
        #[test]
        fn it_stops_only_when_idle_for_the_grace_or_asked(steps in prop::collection::vec(step(), 0..40)) {
            let start = Instant::now();
            // The reference: how many connections are open, and since when none is.
            let (mut open, mut idle_since) = (0, Some(start));
            let mut before = State::Idle { since: start };
            for (event, decided) in run(start, &steps) {
                let after = decided.clone().unwrap_or(before.clone());
                let asked = event == Event::Command(Command::Shutdown);
                let idle_enough = match (&event, idle_since) {
                    (Event::Observed { what: Observed::Tick, at }, Some(since)) => *at >= since + IDLE_GRACE,
                    _ => false,
                };
                let running = matches!(before, State::Idle { .. } | State::Serving { .. });
                let starts_stopping = running && after == State::Stopping;
                prop_assert_eq!(starts_stopping, running && (asked || idle_enough), "{:?} in {:?}", event, before);
                match event {
                    Event::Observed { what: Observed::Opened, .. } => (open, idle_since) = (open + 1, None),
                    Event::Observed { what: Observed::Closed, at } if open == 1 => (open, idle_since) = (0, Some(at)),
                    Event::Observed { what: Observed::Closed, .. } => open -= 1,
                    _ => {}
                }
                before = after;
            }
        }

        /// Once stopping, every request is refused with `Stopping`, and it only goes on to exiting,
        /// once the engine has stopped.
        #[test]
        fn once_stopping_it_refuses_everything_and_exits_after_the_engine(steps in prop::collection::vec(step(), 0..40)) {
            let (mut stopping, mut engine_stopped) = (false, false);
            for (event, decided) in run(Instant::now(), &steps) {
                if stopping && event == Event::Command(Command::Request) {
                    prop_assert_eq!(&decided, &Err(Refusal::Stopping));
                }
                engine_stopped |= matches!(event, Event::Observed { what: Observed::EngineStopped, .. });
                match &decided {
                    Ok(State::Exiting) => prop_assert!(engine_stopped),
                    Ok(State::Starting | State::Idle { .. } | State::Serving { .. }) => prop_assert!(!stopping),
                    Ok(State::Stopping) => stopping = true,
                    Err(_) => prop_assert!(stopping),
                }
            }
        }
    }

    #[test]
    fn a_second_service_exits_without_serving() {
        let at = Instant::now();
        assert_eq!(decide(&State::Starting, &Event::Observed { what: Observed::LockHeld, at }), Ok(State::Exiting));
    }
}
