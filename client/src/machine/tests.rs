//! The link against a fake world on a virtual clock: whatever holds the data dir (nothing, our
//! build, another build, one starting or stopping, each taking its time), how a service started
//! does, and what happens to it while connected. Properties from #113.

use proptest::prelude::*;

use super::*;

/// A service in the fake world.
#[derive(Debug, Clone)]
struct Service {
    build: Is,
    /// Holds the data dir from then on; before it's a process that hasn't taken the lock yet.
    locks_at: Instant,
    /// Answers hello from then on; before it's still starting.
    up_at: Instant,
    /// Asked to stop, or stopping on its own: gone then.
    gone_at: Option<Instant>,
    /// How long it takes to stop once asked.
    stops_in: Duration,
}

/// Which build a service is: ours, the one installed before ours, or one installed after we
/// hashed ours (Phantom updated while the client ran).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Is {
    Ours,
    Older,
    Newer,
}

impl Is {
    /// As the link's hello tells it.
    fn seen(self) -> Build {
        match self {
            Is::Ours => Build::Ours,
            Is::Older => Build::Other,
            Is::Newer => Build::Updated,
        }
    }
}

/// What a service the link starts does.
#[derive(Debug, Clone)]
enum Start {
    /// It can't be run.
    Fails,
    /// It runs, but exits before taking the lock.
    Exits,
    /// It takes the lock `locks_in`, then answers `up_in` after, as our build or a newer one.
    Starts { build: Is, locks_in: Duration, up_in: Duration },
}

/// Something that happens to the service while the link is connected, `after` the last.
#[derive(Debug, Clone)]
enum Trouble {
    Crash,
    Stop(Duration),
}

#[derive(Debug, Clone)]
struct Fake {
    now: Instant,
    service: Option<Service>,
    start: Start,
    troubles: Vec<(Duration, Trouble)>,
    /// The last observation, so `Shutdown` can tell whether it came after a hello.
    last: Option<Observed>,
    /// The builds the link asked to stop.
    asked: Vec<Is>,
}

impl Fake {
    fn service(&mut self) -> Option<&mut Service> {
        let now = self.now;
        if self.service.as_ref().is_some_and(|s| s.gone_at.is_some_and(|gone| gone <= now)) {
            self.service = None;
        }
        self.service.as_mut()
    }

    /// The next trouble, as the connected link sees it, if any is left.
    fn trouble(&mut self) -> Option<Observed> {
        let (after, trouble) = self.troubles.pop()?;
        self.now += after;
        let now = self.now;
        let service = self.service()?;
        Some(match trouble {
            Trouble::Crash => {
                self.service = None;
                Observed::Closed
            }
            Trouble::Stop(takes) => {
                service.gone_at = Some(now + takes);
                Observed::Going
            }
        })
    }

    /// Whether the services in this world do what's asked in time.
    fn works(&self) -> bool {
        let margin = 2 * TICK;
        // Gone within the deadline from when the link first sees it: once up and asked to stop.
        let gone = |s: &Service| s.gone_at.unwrap_or(s.up_at.max(self.now) + s.stops_in);
        let stops = |s: &Service| {
            s.build == Is::Ours && s.gone_at.is_none() || s.build != Is::Newer && gone(s) + margin < self.now + REPLACE_WITHIN
        };
        let starts = matches!(self.start, Start::Starts { build: Is::Ours, locks_in, up_in } if locks_in + up_in + margin < START_WITHIN);
        starts
            && self.service.as_ref().is_none_or(stops)
            && self.troubles.iter().all(|(_, t)| !matches!(t, Trouble::Stop(d) if *d + margin >= REPLACE_WITHIN))
    }
}

impl World for Fake {
    fn now(&self) -> Instant {
        self.now
    }

    fn run(&mut self, act: &Do) -> Observed {
        let now = self.now;
        let what = match act {
            Do::Hello => match self.service() {
                Some(s) if now < s.locks_at => Observed::Gone,
                None => Observed::Gone,
                Some(s) if s.gone_at.is_some() || now < s.up_at => Observed::Going,
                Some(s) => Observed::Hello { build: s.build.seen() },
            },
            Do::Shutdown => {
                assert_eq!(self.last, Some(Observed::Hello { build: Build::Other }), "Shutdown only after a hello from another build");
                if let Some(s) = self.service() {
                    let build = s.build;
                    s.gone_at.get_or_insert(now + s.stops_in);
                    assert!(!self.asked.contains(&build), "asks each build to stop at most once: {build:?}");
                    self.asked.push(build);
                }
                Observed::Closed
            }
            Do::Spawn => match self.start {
                Start::Fails => Observed::Failed("can't start".into()),
                Start::Exits => Observed::Spawned,
                Start::Starts { build, locks_in, up_in } => {
                    // One already holding the data dir makes it exit.
                    if self.service().is_none() {
                        let locks_at = now + locks_in;
                        let up_at = locks_at + up_in;
                        self.service = Some(Service { build, locks_at, up_at, gone_at: None, stops_in: Duration::ZERO });
                    }
                    Observed::Spawned
                }
            },
            Do::Wait(until) => {
                self.now = self.now.max(*until);
                Observed::Tick
            }
        };
        self.last = Some(what.clone());
        what
    }
}

fn secs(max: u64) -> impl Strategy<Value = Duration> {
    (0..max * 1000).prop_map(Duration::from_millis)
}

fn fake() -> impl Strategy<Value = Fake> {
    let now = Instant::now();
    let service = prop_oneof![
        Just(None),
        (prop_oneof![Just(Is::Ours), Just(Is::Older), Just(Is::Newer)], secs(15), prop::option::of(secs(40)), secs(40)).prop_map(
            move |(build, up_in, gone_in, stops_in)| Some(Service {
                build,
                locks_at: now,
                up_at: now + up_in,
                gone_at: gone_in.map(|d| now + d),
                stops_in
            })
        ),
    ];
    let start = prop_oneof![
        1 => Just(Start::Fails),
        1 => Just(Start::Exits),
        8 => (prop::bool::weighted(0.8), secs(8), secs(8)).prop_map(|(ours, locks_in, up_in)| Start::Starts {
            build: if ours { Is::Ours } else { Is::Newer },
            locks_in,
            up_in
        }),
    ];
    let trouble = (secs(60), prop_oneof![Just(Trouble::Crash), secs(40).prop_map(Trouble::Stop)]);
    (service, start, prop::collection::vec(trouble, 0..3)).prop_map(move |(service, start, troubles)| Fake {
        now,
        service,
        start,
        troubles,
        last: None,
        asked: Vec::new(),
    })
}

/// Enough turns to replace a service and start one, each to its deadline, looking every tick.
const TURNS: usize =
    (2 * (REPLACE_WITHIN.as_millis() + START_WITHIN.as_millis() + RETRY_AFTER.as_millis()) / TICK.as_millis()) as usize + 10;

/// One turn: the state before, what was done (`None`: trouble while connected), what came of it,
/// and the state after.
type Turn = (State, Option<Do>, Observed, State);

/// The link in `world` from Connecting, until it's connected with no trouble left, outdated, or
/// has taken `turns`.
fn run(world: &mut Fake, turns: usize) -> Vec<Turn> {
    let mut state = State::Connecting;
    let mut trace = Vec::new();
    for _ in 0..turns {
        let act = next(&state, world.now());
        let what = match &act {
            Some(act) => world.run(act),
            None if state == State::Connected => match world.trouble() {
                Some(what) => what,
                None => break,
            },
            None => break,
        };
        let after = decide(&state, &Event::Observed { what: what.clone(), at: world.now() }).unwrap();
        trace.push((state, act, what, after.clone()));
        state = after;
    }
    trace
}

/// Where a link settles: connected, lost or outdated, after the turns in between.
fn settles(state: &State) -> bool {
    matches!(state, State::Connected | State::Lost { .. } | State::Outdated)
}

proptest! {
    /// Connected only after a hello from our build. The fake asserts the rest: `Shutdown` only
    /// after a hello from another build, and at most once per build.
    #[test]
    fn connected_only_after_a_hello_from_our_build(mut world in fake()) {
        for (before, _, what, after) in run(&mut world, 4 * TURNS) {
            if after == State::Connected && before != State::Connected {
                prop_assert_eq!(what, Observed::Hello { build: Build::Ours });
            }
        }
    }

    /// At most one service started on each way to Connected, Lost or Outdated.
    #[test]
    fn at_most_one_spawn_per_way_to_connected(mut world in fake()) {
        let mut spawns = 0;
        for (before, act, _, after) in run(&mut world, 4 * TURNS) {
            spawns += usize::from(act == Some(Do::Spawn));
            prop_assert!(spawns <= 1, "a second spawn in {:?}", before);
            if settles(&after) && !settles(&before) {
                spawns = 0;
            }
        }
    }

    /// Wherever it is, a link is connected or lost within `TURNS`.
    #[test]
    fn it_settles_within_the_turns(mut world in fake()) {
        let mut since = 0;
        for (_, _, _, after) in run(&mut world, 4 * TURNS) {
            since = if settles(&after) { 0 } else { since + 1 };
            prop_assert!(since <= TURNS);
        }
    }

    /// With services that stop and start in time, the link connects within `TURNS`, at first and
    /// after each trouble (a crash or a stop is no service: it starts a new one), and is never
    /// lost or outdated.
    #[test]
    fn with_a_working_service_it_connects(mut world in fake().prop_filter("works", Fake::works)) {
        let trace = run(&mut world, 10 * TURNS);
        let mut since = 0;
        for (_, _, _, after) in &trace {
            prop_assert!(!matches!(after, State::Lost { .. } | State::Outdated), "{:?}", after);
            since = if *after == State::Connected { 0 } else { since + 1 };
            prop_assert!(since <= TURNS);
        }
        prop_assert_eq!(trace.last().map(|(_, _, _, s)| s), Some(&State::Connected));
    }

    /// A service it started that's another build ends it outdated, for good: it doesn't ask that
    /// service to stop (the fake asserts at most once per build, which a loop would break).
    #[test]
    fn a_start_of_another_build_ends_outdated(mut world in fake().prop_map(|mut w| {
        w.service = None;
        w.start = Start::Starts { build: Is::Newer, locks_in: Duration::ZERO, up_in: Duration::ZERO };
        w
    })) {
        let trace = run(&mut world, 4 * TURNS);
        prop_assert_eq!(trace.last().map(|(_, _, _, s)| s), Some(&State::Outdated));
    }
}
