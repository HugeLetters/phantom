//! The service's lifetime (#116): starting, idle or serving clients, restarting the engine with
//! new settings, stopping, exiting. `main`'s loop is the actor that owns it; `serve` tasks tell it
//! when a connection opens and closes, ask it to restart, and read the state it publishes to
//! answer, hold or refuse each request ([`admit`]).
//!
//! ```text
//! Starting --lock held--> Exiting      Starting --ready--> Idle
//! Idle <--first opened / last closed--> Serving
//! Idle, Serving --Restart--> Restarting --engine restarted--> Idle, Serving (or Stopping, asked to)
//! Restarting --restart failed--> Stopping
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
    /// Stopping the engine and starting it again with new settings, `clients` connections open
    /// meanwhile. Requests that need the engine wait for it; the others are answered. `stop`: a
    /// shutdown came meanwhile, so the service stops once the engine is back.
    Restarting {
        clients: usize,
        stop: bool,
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
    /// Any request a client sends, by what it needs: answered, held or refused ([`admit`]).
    Request(Needs),
    /// A client's `Shutdown`, SIGTERM or SIGINT.
    Shutdown,
    /// The settings changed the listener or UPnP: start the engine again with them.
    Restart,
}

/// What a request needs to be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// The torrents or the settings the engine runs with: held while it restarts.
    Engine,
    /// Nothing that restarts: `Hello`, `Shutdown`.
    Nothing,
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
    /// The engine is back with the new settings, or failed to start with them.
    EngineRestarted {
        ok: bool,
    },
}

/// What the actor carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Tick at this time, unless something happens first.
    WakeAt(Instant),
    StopEngine,
    RestartEngine,
}

/// What happens to a request now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    Now,
    /// Held until the state changes: the engine is restarting.
    Wait,
    Refused(Refusal),
}

/// Whether a request that needs `needs` is answered now, waits, or is refused: refused once the
/// service is stopping, or will stop; held while the engine restarts, if it needs it.
pub fn admit(state: &State, needs: Needs) -> Admit {
    match (state, needs) {
        (State::Starting | State::Idle { .. } | State::Serving { .. }, Needs::Engine | Needs::Nothing) => Admit::Now,
        (State::Restarting { stop: false, .. }, Needs::Engine) => Admit::Wait,
        (State::Restarting { stop: false, .. }, Needs::Nothing) => Admit::Now,
        (State::Restarting { stop: true, .. } | State::Stopping | State::Exiting, Needs::Engine | Needs::Nothing) => {
            Admit::Refused(Refusal::Stopping)
        }
    }
}

/// Only a request is ever refused: while stopping or once stopped, or a restart then. Observations
/// never are.
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
        (_, Request(needs)) => match admit(state, *needs) {
            Admit::Now | Admit::Wait => Ok(state.clone()),
            Admit::Refused(refusal) => Err(refusal),
        },
        (Starting | Idle { .. } | Serving { .. }, Shutdown) => Ok(Stopping),
        (Restarting { clients, .. }, Shutdown) => Ok(Restarting { clients: *clients, stop: true }),
        (Stopping | Exiting, Shutdown) => Ok(state.clone()),
        (Starting | Idle { .. }, Restart) => Ok(Restarting { clients: 0, stop: false }),
        (Serving { clients }, Restart) => Ok(Restarting { clients: *clients, stop: false }),
        (Restarting { stop: false, .. }, Restart) => Ok(state.clone()),
        (Restarting { stop: true, .. } | Stopping | Exiting, Restart) => Err(Refusal::Stopping),
    }
}

fn observe(state: &State, what: &Observed, at: Instant) -> State {
    use Observed::*;
    use State::*;
    match (state, what) {
        (Starting, LockHeld) => Exiting,
        (Starting, Ready) => Idle { since: at },
        (Starting, Opened | Closed | Tick | EngineStopped | EngineRestarted { .. }) => Starting,
        (Idle { .. }, Opened) => Serving { clients: 1 },
        (Idle { since }, Tick) if at >= *since + IDLE_GRACE => Stopping,
        (Idle { .. }, Tick | Closed | EngineStopped | EngineRestarted { .. }) => state.clone(),
        (Serving { clients }, Opened) => Serving { clients: clients + 1 },
        (Serving { clients: 1 }, Closed) => Idle { since: at },
        (Serving { clients }, Closed) => Serving { clients: clients - 1 },
        (Serving { .. }, Tick | EngineStopped | EngineRestarted { .. }) => state.clone(),
        (Restarting { clients, stop }, Opened) => Restarting { clients: clients + 1, stop: *stop },
        (Restarting { clients, stop }, Closed) => Restarting { clients: clients.saturating_sub(1), stop: *stop },
        (Restarting { stop: true, .. }, EngineRestarted { .. }) => Stopping,
        (Restarting { stop: false, .. }, EngineRestarted { ok: false }) => Stopping,
        (Restarting { clients: 0, stop: false }, EngineRestarted { ok: true }) => Idle { since: at },
        (Restarting { clients, stop: false }, EngineRestarted { ok: true }) => Serving { clients: *clients },
        (Restarting { .. }, Tick | EngineStopped) => state.clone(),
        (Stopping, EngineStopped) => Exiting,
        (Stopping, Opened | Closed | Tick | EngineRestarted { .. }) => Stopping,
        (Idle { .. } | Serving { .. } | Restarting { .. } | Stopping | Exiting, LockHeld | Ready) => state.clone(),
        (Exiting, Opened | Closed | Tick | EngineStopped | EngineRestarted { .. }) => Exiting,
    }
}

/// The service's lifetime as a [`Machine`](::machine::Machine): [`decide`] and
/// [`next`], which looks at nothing but the state. Only ever named, as a type.
#[allow(dead_code)]
pub struct Lifetime;

impl ::machine::Machine for Lifetime {
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
        State::Restarting { .. } => Some(Do::RestartEngine),
        State::Stopping => Some(Do::StopEngine),
        State::Starting | State::Serving { .. } | State::Exiting => None,
    }
}

#[cfg(test)]
mod tests {
    use ::machine::every;
    use ::machine::{DecideRow, Row, decide_table};
    use proptest::prelude::*;

    use super::*;

    // The table: every state × event is in exactly one row. A new state, event or `Needs` needs
    // samples (`every!`) and a row before this passes.

    fn base() -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    every!(State {
        State::Starting => [State::Starting],
        State::Idle { .. } => [State::Idle { since: base() }],
        State::Serving { .. } => [State::Serving { clients: 1 }, State::Serving { clients: 2 }],
        State::Restarting { .. } => [(0, false), (0, true), (2, false), (2, true)].map(|(clients, stop)| State::Restarting { clients, stop }),
        State::Stopping => [State::Stopping],
        State::Exiting => [State::Exiting],
    });

    every!(Needs { Needs::Engine => [Needs::Engine], Needs::Nothing => [Needs::Nothing] });

    every!(Command {
        Command::Request(_) => <Needs as ::machine::Every>::every().into_iter().map(Command::Request),
        Command::Shutdown => [Command::Shutdown],
        Command::Restart => [Command::Restart],
    });

    every!(Observed {
        Observed::LockHeld => [Observed::LockHeld],
        Observed::Ready => [Observed::Ready],
        Observed::Opened => [Observed::Opened],
        Observed::Closed => [Observed::Closed],
        Observed::Tick => [Observed::Tick],
        Observed::EngineStopped => [Observed::EngineStopped],
        Observed::EngineRestarted { .. } => [true, false].map(|ok| Observed::EngineRestarted { ok }),
    });

    every!(Event {
        Event::Command(_) => <Command as ::machine::Every>::every().into_iter().map(Event::Command),
        Event::Observed { .. } => <Observed as ::machine::Every>::every()
            .into_iter()
            .flat_map(|what| [Duration::ZERO, IDLE_GRACE].map(move |d| Event::Observed { what: what.clone(), at: base() + d })),
    });

    type R = DecideRow<Lifetime>;

    fn clients(s: &State) -> Option<usize> {
        match s {
            State::Serving { clients } | State::Restarting { clients, .. } => Some(*clients),
            State::Idle { .. } => Some(0),
            State::Starting | State::Stopping | State::Exiting => None,
        }
    }

    fn stopping(s: &State) -> bool {
        matches!(s, State::Restarting { stop: true, .. } | State::Stopping | State::Exiting)
    }

    fn what(e: &Event) -> Option<(&Observed, Instant)> {
        match e {
            Event::Observed { what, at } => Some((what, *at)),
            Event::Command(_) => None,
        }
    }

    fn observed(e: &Event, is: fn(&Observed) -> bool) -> bool {
        what(e).is_some_and(|(o, _)| is(o))
    }

    fn rows() -> Vec<R> {
        vec![
            Row {
                says: "a request or a restart once stopping, or bound to stop, is refused: Stopping",
                when: |s, e| stopping(s) && matches!(e, Event::Command(Command::Request(_) | Command::Restart)),
                then: |_, _, out| *out == Err(Refusal::Stopping),
            },
            Row {
                says: "a request not refused changes nothing",
                when: |s, e| !stopping(s) && matches!(e, Event::Command(Command::Request(_))),
                then: |s, _, out| out.as_ref() == Ok(s),
            },
            Row {
                says: "a restart not refused: Restarting with the clients it had, none from Starting; one running goes on",
                when: |s, e| !stopping(s) && matches!(e, Event::Command(Command::Restart)),
                then: |s, _, out| match s {
                    State::Restarting { .. } => out.as_ref() == Ok(s),
                    _ => out.as_ref() == Ok(&State::Restarting { clients: clients(s).unwrap_or(0), stop: false }),
                },
            },
            Row {
                says: "a shutdown is never refused: it stops, or, during a restart, binds it to stop, or goes on stopping",
                when: |_, e| matches!(e, Event::Command(Command::Shutdown)),
                then: |s, _, out| match s {
                    State::Starting | State::Idle { .. } | State::Serving { .. } => out.as_ref() == Ok(&State::Stopping),
                    State::Restarting { clients, .. } => out.as_ref() == Ok(&State::Restarting { clients: *clients, stop: true }),
                    State::Stopping | State::Exiting => out.as_ref() == Ok(s),
                },
            },
            Row {
                says: "another service holding the lock ends a starting one; nothing else notices",
                when: |_, e| observed(e, |o| matches!(o, Observed::LockHeld)),
                then: |s, _, out| out.as_ref() == Ok(&if *s == State::Starting { State::Exiting } else { s.clone() }),
            },
            Row {
                says: "ready ends starting, idle since then; nothing else notices",
                when: |_, e| observed(e, |o| matches!(o, Observed::Ready)),
                then: |s, e, out| {
                    let at = what(e).unwrap().1;
                    out.as_ref() == Ok(&if *s == State::Starting { State::Idle { since: at } } else { s.clone() })
                },
            },
            Row {
                says: "a connection opening is counted, by what's up; stopping or starting doesn't count",
                when: |_, e| observed(e, |o| matches!(o, Observed::Opened)),
                then: |s, _, out| match (s, out) {
                    (State::Starting | State::Stopping | State::Exiting, out) => out.as_ref() == Ok(s),
                    (_, Ok(after)) => {
                        matches!(after, State::Serving { .. } | State::Restarting { .. }) && clients(after) == clients(s).map(|n| n + 1)
                    }
                    (_, Err(_)) => false,
                },
            },
            Row {
                says: "a connection closing is counted, the last one idles it (not during a restart)",
                when: |_, e| observed(e, |o| matches!(o, Observed::Closed)),
                then: |s, e, out| match (s, out) {
                    (State::Starting | State::Stopping | State::Exiting | State::Idle { .. }, out) => out.as_ref() == Ok(s),
                    (State::Serving { clients: 1 }, out) => out.as_ref() == Ok(&State::Idle { since: what(e).unwrap().1 }),
                    (State::Serving { .. }, Ok(after)) => {
                        clients(after) == clients(s).map(|n| n - 1) && matches!(after, State::Serving { .. })
                    }
                    (State::Restarting { stop, .. }, Ok(after)) => {
                        matches!(after, State::Restarting { stop: s2, .. } if s2 == stop)
                            && clients(after) == clients(s).map(|n| n.saturating_sub(1))
                    }
                    (_, Err(_)) => false,
                },
            },
            Row {
                says: "time passing stops an idle service once the grace is over, and changes nothing else",
                when: |_, e| observed(e, |o| matches!(o, Observed::Tick)),
                then: |s, e, out| {
                    let at = what(e).unwrap().1;
                    match s {
                        State::Idle { since } if at >= *since + IDLE_GRACE => out.as_ref() == Ok(&State::Stopping),
                        _ => out.as_ref() == Ok(s),
                    }
                },
            },
            Row {
                says: "the engine stopping ends a stopping service; nothing else notices",
                when: |_, e| observed(e, |o| matches!(o, Observed::EngineStopped)),
                then: |s, _, out| out.as_ref() == Ok(&if *s == State::Stopping { State::Exiting } else { s.clone() }),
            },
            Row {
                says: "a restart ending: stopping if it failed or was bound to stop, else back to idle or serving, with the clients it has; nothing else notices",
                when: |_, e| observed(e, |o| matches!(o, Observed::EngineRestarted { .. })),
                then: |s, e, out| {
                    let (Observed::EngineRestarted { ok }, at) = what(e).map(|(o, at)| (o.clone(), at)).unwrap() else { return false };
                    match s {
                        State::Restarting { stop, .. } if *stop || !ok => out.as_ref() == Ok(&State::Stopping),
                        State::Restarting { clients: 0, .. } => out.as_ref() == Ok(&State::Idle { since: at }),
                        State::Restarting { clients, .. } => out.as_ref() == Ok(&State::Serving { clients: *clients }),
                        _ => out.as_ref() == Ok(s),
                    }
                },
            },
        ]
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        decide_table::<Lifetime>(
            &rows(),
            &[Row::always("only a command is refused, and only once stopping", |s, e, out| {
                out.is_ok() || (stopping(s) && matches!(e, Event::Command(_)))
            })],
        );
    }

    /// What can happen once the service is up: connections opening and closing, time passing, a
    /// request, a shutdown, a restart (the engine coming back or failing to), the engine stopping
    /// (only ever after it was asked to).
    #[derive(Debug, Clone)]
    enum Step {
        Open,
        /// Closes one of the open connections, if any.
        Close,
        Wait(Duration),
        Request(Needs),
        Shutdown,
        /// Asks for a restart; while one runs, the next steps happen during it.
        Restart,
        /// The restarting engine is back, or failed to start.
        Restarted {
            ok: bool,
        },
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            3 => Just(Step::Open),
            3 => Just(Step::Close),
            4 => (0..2 * IDLE_GRACE.as_millis() as u64).prop_map(|ms| Step::Wait(Duration::from_millis(ms))),
            2 => prop_oneof![Just(Needs::Engine), Just(Needs::Nothing)].prop_map(Step::Request),
            1 => Just(Step::Shutdown),
            2 => Just(Step::Restart),
            2 => (0..6u8).prop_map(|n| Step::Restarted { ok: n > 0 }),
        ]
    }

    /// Runs `steps` the way `main` does: the engine stops at once when told to, a restart's end
    /// comes when a step says, and a wake-up comes when it's due, before a later step.
    fn run(start: Instant, steps: &[Step]) -> Vec<(State, Event, Result<State, Refusal>)> {
        let mut at = start;
        let mut state = decide(&State::Starting, &Event::Observed { what: Observed::Ready, at }).unwrap();
        let mut trace = Vec::new();
        let mut open = 0;
        let apply = |state: &mut State, event: Event, trace: &mut Vec<_>| {
            let decided = decide(state, &event);
            trace.push((state.clone(), event, decided.clone()));
            if let Ok(next) = decided {
                *state = next;
            }
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
                Step::Request(needs) => Event::Command(Command::Request(*needs)),
                Step::Shutdown => Event::Command(Command::Shutdown),
                Step::Restart => Event::Command(Command::Restart),
                // Only a restart that runs ends.
                Step::Restarted { ok } if next(&state) == Some(Do::RestartEngine) => {
                    Event::Observed { what: Observed::EngineRestarted { ok: *ok }, at }
                }
                Step::Restarted { .. } => continue,
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
        /// It starts stopping if and only if it had no client for `IDLE_GRACE`, was asked to, or a
        /// restart failed or ended after it was asked to; a client within the grace cancels the stop.
        #[test]
        fn it_stops_only_when_idle_for_the_grace_asked_or_a_restart_failed(steps in prop::collection::vec(step(), 0..40)) {
            let start = Instant::now();
            // The reference: how many connections are open, since when none is, and whether a
            // shutdown came during a restart.
            let (mut open, mut idle_since, mut asked_meanwhile) = (0usize, Some(start), false);
            for (before, event, decided) in run(start, &steps) {
                let after = decided.clone().unwrap_or(before.clone());
                let restarting = matches!(before, State::Restarting { .. });
                let cause = match (&event, &before) {
                    (Event::Command(Command::Shutdown), State::Idle { .. } | State::Serving { .. }) => true,
                    (Event::Observed { what: Observed::Tick, at }, State::Idle { .. }) => idle_since.is_some_and(|since| *at >= since + IDLE_GRACE),
                    (Event::Observed { what: Observed::EngineRestarted { ok }, .. }, State::Restarting { .. }) => !ok || asked_meanwhile,
                    _ => false,
                };
                let up = matches!(before, State::Idle { .. } | State::Serving { .. } | State::Restarting { .. });
                prop_assert_eq!(up && after == State::Stopping, up && cause, "{:?} in {:?}", event, before);
                match &event {
                    Event::Observed { what: Observed::Opened, .. } => (open, idle_since) = (open + 1, None),
                    Event::Observed { what: Observed::Closed, at } if open == 1 => (open, idle_since) = (0, (!restarting).then_some(*at)),
                    Event::Observed { what: Observed::Closed, .. } => open -= 1,
                    Event::Observed { what: Observed::EngineRestarted { .. }, at } if restarting && open == 0 => idle_since = Some(*at),
                    Event::Command(Command::Shutdown) if restarting => asked_meanwhile = true,
                    _ => {}
                }
                if !matches!(after, State::Restarting { .. }) {
                    asked_meanwhile = false;
                }
                // The connections it counts are those open.
                match &after {
                    State::Serving { clients } | State::Restarting { clients, .. } => prop_assert_eq!(*clients, open),
                    State::Idle { .. } => prop_assert_eq!(open, 0),
                    _ => {}
                }
            }
        }

        /// Once stopping, or bound to stop after a restart, every request is refused with
        /// `Stopping`, and it only goes on to exiting once the engine has stopped.
        #[test]
        fn once_stopping_it_refuses_everything_and_exits_after_the_engine(steps in prop::collection::vec(step(), 0..40)) {
            let mut engine_stopped = false;
            for (before, event, decided) in run(Instant::now(), &steps) {
                let stopping = matches!(before, State::Stopping | State::Exiting | State::Restarting { stop: true, .. });
                if stopping && matches!(event, Event::Command(Command::Request(_) | Command::Restart)) {
                    prop_assert_eq!(&decided, &Err(Refusal::Stopping));
                }
                engine_stopped |= matches!(event, Event::Observed { what: Observed::EngineStopped, .. });
                if decided == Ok(State::Exiting) && before != State::Starting {
                    prop_assert!(engine_stopped);
                }
                if decided.is_err() {
                    prop_assert!(stopping, "{:?} refused in {:?}", event, before);
                }
            }
        }

        /// A client meeting a restart that isn't bound to stop is never refused: what needs the
        /// engine waits for it, the rest is answered at once.
        #[test]
        fn during_a_restart_requests_wait_or_are_answered_never_refused(steps in prop::collection::vec(step(), 0..40)) {
            for (before, _, _) in run(Instant::now(), &steps) {
                if let State::Restarting { stop: false, .. } = before {
                    prop_assert_eq!(admit(&before, Needs::Engine), Admit::Wait);
                    prop_assert_eq!(admit(&before, Needs::Nothing), Admit::Now);
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
