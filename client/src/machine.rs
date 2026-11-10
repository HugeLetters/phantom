//! The connection to the service (#113): reaching it, replacing one from another build or one
//! that's stopping, starting it, and losing it. One link owns it (the CLI's connect, the app's link
//! thread); nothing else connects, starts or replaces a service. [`World`] is what the link carries
//! out a [`Do`] with: the socket, or a fake on a virtual clock.
//!
//! ```text
//! Connecting --hello, our build--> Connected --closed, after STABLE_AFTER--> Connecting
//! Connecting, Connected --stopping--> Replacing (looks until it's gone)
//! Connecting --hello, another build--> Replacing (asks it to stop, looks until it's gone)
//! Connecting, Replacing --gone--> Spawning (starts it, looks until it answers) --hello, ours--> Connected
//! Connecting, Replacing, Spawning --hello, the build installed since--> Outdated (for good: restart)
//! Spawning --hello, another build--> Lost (someone else's service took the data dir)
//! Connected --closed or stopping, within STABLE_AFTER--> Lost (it crashed: no tight restart loop)
//! Replacing, Spawning --deadline--> Lost --retry_at--> Connecting
//! ```

use std::time::{Duration, Instant};

/// How often a link looks again while it waits for a service to go or to answer.
pub const TICK: Duration = Duration::from_millis(100);
/// How long a service has to go once it's asked to, or seen stopping.
pub const REPLACE_WITHIN: Duration = Duration::from_secs(30);
/// How long a service started has to answer.
pub const START_WITHIN: Duration = Duration::from_secs(10);
/// How long a lost link waits before connecting again.
pub const RETRY_AFTER: Duration = Duration::from_secs(2);
/// How long a connection lasts before losing it means "restart the service, requests wait"
/// rather than "it crashed: lost, requests fail, retry after [`RETRY_AFTER`]". At least
/// `RETRY_AFTER`, so services are never started closer together than that.
pub const STABLE_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Saying hello to whatever answers the socket.
    Connecting,
    /// A service that won't do (another build, or stopping) has until `deadline` to go.
    Replacing {
        deadline: Instant,
        step: Step,
    },
    /// A service started has until `deadline` to answer.
    Spawning {
        deadline: Instant,
        step: Step,
    },
    /// A service of our build answered, at `since`.
    Connected {
        since: Instant,
    },
    Lost {
        why: Why,
        retry_at: Instant,
    },
    /// A service answered as the build installed since this client hashed its own (Phantom
    /// updated). Final: replacing it would only start the same build again.
    Outdated,
}

/// Where a wait is: acting once (asking the old service to stop, starting a new one), then
/// looking at `TICK`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Act,
    LookAt(Instant),
}

/// Why the link is lost; [`crate::display::lost`] words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The old service didn't go within [`REPLACE_WITHIN`].
    OldDidNotStop,
    /// The service started didn't answer within [`START_WITHIN`].
    DidNotStart,
    /// The service went within [`STABLE_AFTER`] of connecting.
    StoppedEarly,
    /// A service of another build answered the one we started: someone else's took the data dir.
    Taken,
    /// The client is [`State::Outdated`]: what a request is refused with.
    Outdated,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    /// What the link saw, at `at`.
    Observed {
        what: Observed,
        at: Instant,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Any request a client makes: held until connected, refused while lost.
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// A service answered hello, as `build`.
    Hello {
        build: Build,
    },
    /// A service holds the data dir but doesn't answer: stopping (refused, closed the hello) or
    /// still starting.
    Going,
    /// Nothing holds the data dir: no service.
    Gone,
    /// The connection closed: after a `Shutdown`, or while connected.
    Closed,
    Spawned,
    Failed(String),
    /// Time passed: the clock reached `at`.
    Tick,
}

/// Which build a service that answers hello is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// The `phantom-service` this client hashed when it first looked.
    Ours,
    /// The `phantom-service` next to the client now, which isn't ours: Phantom was updated while
    /// the client ran.
    Updated,
    /// Any other: one installed before, or another copy's.
    Other,
}

/// What the link carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Connect and say hello: answered with Hello, Going, Gone or Failed.
    Hello,
    /// Ask the service that answered hello to stop: answered with Closed.
    Shutdown,
    /// Start `phantom-service`: answered with Spawned or Failed.
    Spawn,
    /// Wait until then: answered with Tick, the time it is.
    Wait(Instant),
}

/// The world a link connects through: the socket and the clock, or a fake of both.
pub trait World {
    fn now(&self) -> Instant;
    /// Carries out `act` and says what came of it.
    fn run(&mut self, act: &Do) -> Observed;
}

/// Only a request is ever refused, while lost, with why. Observations never are.
pub fn decide(state: &State, event: &Event) -> Result<State, Why> {
    match event {
        Event::Command(Command::Request) => match state {
            State::Lost { why, .. } => Err(why.clone()),
            State::Outdated => Err(Why::Outdated),
            State::Connecting | State::Replacing { .. } | State::Spawning { .. } | State::Connected { .. } => Ok(state.clone()),
        },
        Event::Observed { what, at } => Ok(observe(state, what, *at)),
    }
}

fn observe(state: &State, what: &Observed, at: Instant) -> State {
    use Observed::*;
    use State::*;
    let look = Step::LookAt(at + TICK);
    let lost = |why| Lost { why, retry_at: at + RETRY_AFTER };
    let replacing = |step| Replacing { deadline: at + REPLACE_WITHIN, step };
    let spawning = Spawning { deadline: at + START_WITHIN, step: Step::Act };
    match (state, what) {
        (Connecting | Replacing { .. } | Spawning { .. }, Hello { build: Build::Ours }) => Connected { since: at },
        // Replacing it would only start the same build again.
        (Connecting | Replacing { .. } | Spawning { .. }, Hello { build: Build::Updated }) => Outdated,
        (Connecting, Hello { build: Build::Other }) => replacing(Step::Act),
        (Connecting, Going) => replacing(look),
        (Connecting | Replacing { .. }, Gone) => spawning,
        (Connecting, Closed | Spawned | Tick) => Connecting,

        // Still answering: ask again.
        (Replacing { deadline, .. }, Hello { build: Build::Other }) => Replacing { deadline: *deadline, step: Step::Act },
        (Replacing { deadline, .. }, Going | Closed) => Replacing { deadline: *deadline, step: look },
        (Replacing { deadline, .. }, Tick) if at >= *deadline => lost(Why::OldDidNotStop),
        (Replacing { .. }, Spawned | Tick) => state.clone(),

        // Replacing it would start a second service on the way to one connection.
        (Spawning { .. }, Hello { build: Build::Other }) => lost(Why::Taken),
        // Not up yet, or not yet holding the data dir: it has until the deadline.
        (Spawning { deadline, .. }, Spawned | Going | Gone | Closed) => Spawning { deadline: *deadline, step: look },
        (Spawning { deadline, .. }, Tick) if at >= *deadline => lost(Why::DidNotStart),
        (Spawning { .. }, Tick) => state.clone(),

        // Gone so soon it crashed: lost, so it's started again only after `RETRY_AFTER`.
        (Connected { since }, Closed | Gone | Going) if at < *since + STABLE_AFTER => lost(Why::StoppedEarly),
        // No service, as anywhere else: a new one is started, and requests wait for it.
        (Connected { .. }, Closed | Gone) => Connecting,
        (Connected { .. }, Going) => replacing(look),
        (Connected { .. }, Hello { .. } | Spawned | Tick) => state.clone(),

        (Connecting | Replacing { .. } | Spawning { .. } | Connected { .. }, Failed(e)) => lost(Why::Failed(e.clone())),

        (Lost { retry_at, .. }, Tick) if at >= *retry_at => Connecting,
        (Lost { .. }, Hello { .. } | Going | Gone | Closed | Spawned | Failed(_) | Tick) => state.clone(),
        (Outdated, Hello { .. } | Going | Gone | Closed | Spawned | Failed(_) | Tick) => Outdated,
    }
}

/// The connection as a [`Machine`](lifecycle::machine::Machine): [`decide`] and [`next`], at
/// the clock's `now`.
pub struct Link;

impl lifecycle::machine::Machine for Link {
    type State = State;
    type Event = Event;
    type Refusal = Why;
    type World = Instant;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Why> {
        decide(state, event)
    }

    fn next(state: &State, now: &Instant) -> Option<Do> {
        next(state, *now)
    }
}

/// What to do in `state` at `now`; `None` once connected, where the link serves its client, or
/// outdated, where nothing more is done.
pub fn next(state: &State, now: Instant) -> Option<Do> {
    let look = |at: Instant| if now >= at { Do::Hello } else { Do::Wait(at) };
    match state {
        State::Connecting => Some(Do::Hello),
        State::Replacing { step: Step::Act, .. } => Some(Do::Shutdown),
        State::Spawning { step: Step::Act, .. } => Some(Do::Spawn),
        State::Replacing { step: Step::LookAt(at), .. } | State::Spawning { step: Step::LookAt(at), .. } => Some(look(*at)),
        State::Connected { .. } | State::Outdated => None,
        State::Lost { retry_at, .. } => Some(Do::Wait(*retry_at)),
    }
}

/// One turn of a link: carries out what `state` asks for and decides on what came of it. `None`
/// once connected or outdated.
pub fn step(state: &State, world: &mut impl World) -> Option<State> {
    let act = next(state, world.now())?;
    let what = world.run(&act);
    Some(decide(state, &Event::Observed { what, at: world.now() }).expect("observations are never refused"))
}

/// Turns of a link from `state` until it's connected, lost or outdated.
pub fn settle(mut state: State, world: &mut impl World) -> State {
    while !matches!(state, State::Lost { .. }) {
        match step(&state, world) {
            Some(next) => state = next,
            None => break,
        }
    }
    state
}

#[cfg(test)]
mod tests;
