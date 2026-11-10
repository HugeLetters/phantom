//! The link against a fake world on a virtual clock: whatever holds the data dir (nothing, our
//! build, another build, one starting or stopping, each taking its time), how a service started
//! does, and what happens to it while connected. Properties from #113.

use proptest::prelude::*;

use super::*;

/// A service in the fake world.
#[derive(Debug, Clone)]
struct Service {
    /// Which one it is, so the link can be held to asking each to stop at most once.
    id: usize,
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
    /// It takes the lock `locks_in`, then answers `up_in` after: as our build, a newer one, or (an
    /// older one: someone else's service took the data dir first) another.
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
    /// The services the link asked to stop.
    asked: Vec<usize>,
    /// Services so far, for the next's id.
    services: usize,
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
            // Each lasts long enough to be a service stopping, not one crashing as it starts.
            && self.troubles.iter().all(|(after, t)| *after >= STABLE_AFTER && !matches!(t, Trouble::Stop(d) if *d + margin >= REPLACE_WITHIN))
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
                    let id = s.id;
                    s.gone_at.get_or_insert(now + s.stops_in);
                    assert!(!self.asked.contains(&id), "asks each service to stop at most once");
                    self.asked.push(id);
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
                        self.services += 1;
                        let id = self.services;
                        self.service = Some(Service { id, build, locks_at, up_at, gone_at: None, stops_in: Duration::ZERO });
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

/// Up to `max` seconds, but half the time under one: a start or a crash that comes at once.
fn soon_or_secs(max: u64) -> impl Strategy<Value = Duration> {
    prop_oneof![secs(1), secs(max)]
}

fn fake() -> impl Strategy<Value = Fake> {
    let now = Instant::now();
    let service = prop_oneof![
        Just(None),
        (prop_oneof![Just(Is::Ours), Just(Is::Older), Just(Is::Newer)], secs(15), prop::option::of(secs(40)), secs(40)).prop_map(
            move |(build, up_in, gone_in, stops_in)| Some(Service {
                id: 0,
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
        8 => (prop_oneof![6 => Just(Is::Ours), 1 => Just(Is::Newer), 1 => Just(Is::Older)], soon_or_secs(8), soon_or_secs(8))
            .prop_map(|(build, locks_in, up_in)| Start::Starts { build, locks_in, up_in }),
    ];
    let trouble = (soon_or_secs(60), prop_oneof![Just(Trouble::Crash), secs(40).prop_map(Trouble::Stop)]);
    (service, start, prop::collection::vec(trouble, 0..3)).prop_map(move |(service, start, troubles)| Fake {
        now,
        service,
        start,
        troubles,
        last: None,
        asked: Vec::new(),
        services: 0,
    })
}

/// Enough turns to replace a service and start one, each to its deadline, looking every tick.
const TURNS: usize =
    (2 * (REPLACE_WITHIN.as_millis() + START_WITHIN.as_millis() + RETRY_AFTER.as_millis()) / TICK.as_millis()) as usize + 10;

/// One turn: the state before, what was done at `at` (`None`: trouble while connected), what
/// came of it, and the state after.
#[derive(Debug)]
struct Turn {
    before: State,
    at: Instant,
    act: Option<Do>,
    what: Observed,
    after: State,
}

/// The link in `world` from Connecting, until it's connected with no trouble left, outdated, or
/// has taken `turns`.
fn run(world: &mut Fake, turns: usize) -> Vec<Turn> {
    let mut state = State::Connecting;
    let mut trace = Vec::new();
    for _ in 0..turns {
        let at = world.now();
        let act = next(&state, at);
        let what = match &act {
            Some(act) => world.run(act),
            None if matches!(state, State::Connected { .. }) => match world.trouble() {
                Some(what) => what,
                None => break,
            },
            None => break,
        };
        let after = decide(&state, &Event::Observed { what: what.clone(), at: world.now() }).unwrap();
        trace.push(Turn { before: state, at, act, what, after: after.clone() });
        state = after;
    }
    trace
}

fn connected(state: &State) -> bool {
    matches!(state, State::Connected { .. })
}

/// Where a link settles: connected, lost or outdated, after the turns in between.
fn settles(state: &State) -> bool {
    matches!(state, State::Connected { .. } | State::Lost { .. } | State::Outdated)
}

proptest! {
    /// What a hello decides: connected only after our build's; outdated only after, and always
    /// after, the build installed since; lost at once when a start of ours is answered by another
    /// build. The fake asserts the rest: `Shutdown` only after a hello from another build, and at
    /// most once per service.
    #[test]
    fn a_hello_decides_by_its_build(mut world in fake()) {
        for Turn { before, what, after, .. } in run(&mut world, 4 * TURNS) {
            if connected(&after) && !connected(&before) {
                prop_assert_eq!(&what, &Observed::Hello { build: Build::Ours });
            }
            if !matches!(before, State::Lost { .. } | State::Outdated | State::Connected { .. }) {
                prop_assert_eq!(after == State::Outdated, what == Observed::Hello { build: Build::Updated });
            }
            if matches!(before, State::Spawning { .. }) && what == (Observed::Hello { build: Build::Other }) {
                prop_assert!(matches!(after, State::Lost { why: Why::Taken, .. }), "{:?}", after);
            }
        }
    }

    /// At most one service started on each way to Connected, Lost or Outdated.
    #[test]
    fn at_most_one_spawn_per_way_to_connected(mut world in fake()) {
        let mut spawns = 0;
        for Turn { before, act, after, .. } in run(&mut world, 4 * TURNS) {
            spawns += usize::from(act == Some(Do::Spawn));
            prop_assert!(spawns <= 1, "a second spawn in {:?}", before);
            if settles(&after) && !settles(&before) {
                spawns = 0;
            }
        }
    }

    /// Services are started at least `RETRY_AFTER` apart, however soon each goes: no tight
    /// restart loop.
    #[test]
    fn spawns_are_retry_after_apart(mut world in fake()) {
        let spawns: Vec<_> = run(&mut world, 10 * TURNS).into_iter().filter(|t| t.act == Some(Do::Spawn)).map(|t| t.at).collect();
        for pair in spawns.windows(2) {
            prop_assert!(pair[1] - pair[0] >= RETRY_AFTER, "spawned {:?} apart", pair[1] - pair[0]);
        }
    }

    /// Wherever it is, a link is connected, lost or outdated within `TURNS`.
    #[test]
    fn it_settles_within_the_turns(mut world in fake()) {
        let mut since = 0;
        for Turn { after, .. } in run(&mut world, 4 * TURNS) {
            since = if settles(&after) { 0 } else { since + 1 };
            prop_assert!(since <= TURNS);
        }
    }

    /// With services that stop and start in time, the link connects within `TURNS`, at first and
    /// after each trouble (a service that stops or crashes after a while is no service: it starts
    /// a new one, and requests wait), and is never lost or outdated.
    #[test]
    fn with_a_working_service_it_connects(mut world in fake().prop_filter("works", Fake::works)) {
        let trace = run(&mut world, 10 * TURNS);
        let mut since = 0;
        for Turn { after, .. } in &trace {
            prop_assert!(!matches!(after, State::Lost { .. } | State::Outdated), "{:?}", after);
            since = if connected(after) { 0 } else { since + 1 };
            prop_assert!(since <= TURNS);
        }
        prop_assert!(trace.last().is_some_and(|t| connected(&t.after)));
    }

    /// A start of the build installed since ends it outdated, for good, whatever comes after.
    #[test]
    fn a_start_of_the_updated_build_ends_outdated(mut world in fake().prop_map(|mut w| {
        w.service = None;
        w.start = Start::Starts { build: Is::Newer, locks_in: Duration::ZERO, up_in: Duration::ZERO };
        w
    })) {
        let trace = run(&mut world, 4 * TURNS);
        prop_assert_eq!(trace.last().map(|t| &t.after), Some(&State::Outdated));
    }
}
