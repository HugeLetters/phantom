//! The connection to the service (#113): reaching it, replacing one from another build or one
//! that's stopping, starting it, and losing it. One link owns it (the CLI's connect, the app's link
//! thread); nothing else connects, starts or replaces a service. [`World`] is what the link carries
//! out a [`Do`] with: the socket, or a fake on a virtual clock.
//!
//! ```text
//! Connecting --hello, our build--> Connected --closed, stopping--> Lost --retry_at--> Connecting
//! Connecting --hello, another build--> Replacing (asks it to stop, looks until it's gone)
//! Connecting --stopping--> Replacing (looks until it's gone)
//! Connecting, Replacing --gone--> Spawning (starts it, looks until it answers) --hello, ours--> Connected
//! Replacing, Spawning --deadline--> Lost
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
    /// A service of our build answered.
    Connected,
    Lost {
        why: Why,
        retry_at: Instant,
    },
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
    /// The connection closed.
    Closed,
    /// The service is stopping.
    Stopping,
    /// The old service didn't go within [`REPLACE_WITHIN`].
    OldDidNotStop,
    /// The service started didn't answer within [`START_WITHIN`].
    DidNotStart,
    /// The service started is another build: the client's own `phantom-service` changed since.
    NotOurBuild,
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
    /// A service answered hello.
    Hello {
        ours: bool,
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
            State::Connecting | State::Replacing { .. } | State::Spawning { .. } | State::Connected => Ok(state.clone()),
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
        (Connecting | Replacing { .. }, Hello { ours: true }) => Connected,
        (Connecting, Hello { ours: false }) => replacing(Step::Act),
        (Connecting, Going) => replacing(look),
        (Connecting | Replacing { .. }, Gone) => spawning,
        (Connecting, Closed | Spawned | Tick) => Connecting,

        // Still answering: ask again.
        (Replacing { deadline, .. }, Hello { ours: false }) => Replacing { deadline: *deadline, step: Step::Act },
        (Replacing { deadline, .. }, Going | Closed) => Replacing { deadline: *deadline, step: look },
        (Replacing { deadline, .. }, Tick) if at >= *deadline => lost(Why::OldDidNotStop),
        (Replacing { .. }, Spawned | Tick) => state.clone(),

        (Spawning { .. }, Hello { ours: true }) => Connected,
        (Spawning { .. }, Hello { ours: false }) => lost(Why::NotOurBuild),
        // Not up yet, or not yet holding the data dir: it has until the deadline.
        (Spawning { deadline, .. }, Spawned | Going | Gone | Closed) => Spawning { deadline: *deadline, step: look },
        (Spawning { deadline, .. }, Tick) if at >= *deadline => lost(Why::DidNotStart),
        (Spawning { .. }, Tick) => state.clone(),

        (Connected, Closed | Gone) => lost(Why::Closed),
        (Connected, Going) => lost(Why::Stopping),
        (Connected, Hello { .. } | Spawned | Tick) => Connected,

        (Connecting | Replacing { .. } | Spawning { .. } | Connected, Failed(e)) => lost(Why::Failed(e.clone())),

        (Lost { retry_at, .. }, Tick) if at >= *retry_at => Connecting,
        (Lost { .. }, Hello { .. } | Going | Gone | Closed | Spawned | Failed(_) | Tick) => state.clone(),
    }
}

/// What to do in `state` at `now`; `None` once connected, where the link serves its client.
pub fn next(state: &State, now: Instant) -> Option<Do> {
    let look = |at: Instant| if now >= at { Do::Hello } else { Do::Wait(at) };
    match state {
        State::Connecting => Some(Do::Hello),
        State::Replacing { step: Step::Act, .. } => Some(Do::Shutdown),
        State::Spawning { step: Step::Act, .. } => Some(Do::Spawn),
        State::Replacing { step: Step::LookAt(at), .. } | State::Spawning { step: Step::LookAt(at), .. } => Some(look(*at)),
        State::Connected => None,
        State::Lost { retry_at, .. } => Some(Do::Wait(*retry_at)),
    }
}

/// One turn of a link: carries out what `state` asks for and decides on what came of it. `None`
/// once connected.
pub fn step(state: &State, world: &mut impl World) -> Option<State> {
    let act = next(state, world.now())?;
    let what = world.run(&act);
    Some(decide(state, &Event::Observed { what, at: world.now() }).expect("observations are never refused"))
}

/// Turns of a link from `state` until it's connected or lost.
pub fn settle(mut state: State, world: &mut impl World) -> State {
    while !matches!(state, State::Connected | State::Lost { .. }) {
        state = step(&state, world).expect("only connected has nothing to do");
    }
    state
}

#[cfg(test)]
mod tests;
