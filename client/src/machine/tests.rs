//! The link against a fake world on a virtual clock: whatever holds the data dir (nothing, our
//! build, another build, one starting or stopping, each taking its time), how a service started
//! does, and what happens to it while connected. Properties from #113.

use proptest::prelude::*;

use super::*;

/// A service in the fake world.
#[derive(Debug, Clone)]
struct Service {
    ours: bool,
    /// Answers hello from then on; before it's still starting.
    up_at: Instant,
    /// Asked to stop, or stopping on its own: gone then.
    gone_at: Option<Instant>,
    /// How long it takes to stop once asked.
    stops_in: Duration,
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
    /// A service started: fails to, or answers that long after.
    start: Result<Duration, ()>,
    troubles: Vec<(Duration, Trouble)>,
    /// The last observation, so `Shutdown` can tell whether it came after a hello.
    last: Option<Observed>,
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
        let stops = |s: &Service| s.ours && s.gone_at.is_none() || gone(s) + margin < self.now + REPLACE_WITHIN;
        let starts = self.start.is_ok_and(|takes| takes + margin < START_WITHIN);
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
                None => Observed::Gone,
                Some(s) if s.gone_at.is_some() || now < s.up_at => Observed::Going,
                Some(s) => Observed::Hello { ours: s.ours },
            },
            Do::Shutdown => {
                assert_eq!(self.last, Some(Observed::Hello { ours: false }), "Shutdown only after a hello from another build");
                if let Some(s) = self.service() {
                    s.gone_at.get_or_insert(now + s.stops_in);
                }
                Observed::Closed
            }
            Do::Spawn => match self.start {
                Err(()) => Observed::Failed("can't start".into()),
                // One already holding the data dir makes it exit.
                Ok(takes) => {
                    if self.service().is_none() {
                        self.service = Some(Service { ours: true, up_at: now + takes, gone_at: None, stops_in: Duration::ZERO });
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
    let service =
        prop_oneof![
            Just(None),
            (any::<bool>(), secs(15), prop::option::of(secs(40)), secs(40)).prop_map(move |(ours, up_in, gone_in, stops_in)| Some(
                Service { ours, up_at: now + up_in, gone_at: gone_in.map(|d| now + d), stops_in }
            )),
        ];
    let start = prop_oneof![1 => Just(Err(())), 9 => secs(15).prop_map(Ok)];
    let trouble = (secs(60), prop_oneof![Just(Trouble::Crash), secs(40).prop_map(Trouble::Stop)]);
    (service, start, prop::collection::vec(trouble, 0..3)).prop_map(move |(service, start, troubles)| Fake {
        now,
        service,
        start,
        troubles,
        last: None,
    })
}

/// Enough turns to replace a service and start one, each to its deadline, looking every tick.
const TURNS: usize =
    (2 * (REPLACE_WITHIN.as_millis() + START_WITHIN.as_millis() + RETRY_AFTER.as_millis()) / TICK.as_millis()) as usize + 10;

/// One turn: the state before, what was done (`None`: trouble while connected), what came of it,
/// and the state after.
type Turn = (State, Option<Do>, Observed, State);

/// The link in `world` from Connecting, until it's connected with no trouble left or has taken
/// `turns`.
fn run(world: &mut Fake, turns: usize) -> Vec<Turn> {
    let mut state = State::Connecting;
    let mut trace = Vec::new();
    for _ in 0..turns {
        let act = next(&state, world.now());
        let what = match &act {
            Some(act) => world.run(act),
            None => match world.trouble() {
                Some(what) => what,
                None => break,
            },
        };
        let after = decide(&state, &Event::Observed { what: what.clone(), at: world.now() }).unwrap();
        trace.push((state, act, what, after.clone()));
        state = after;
    }
    trace
}

/// Where a link settles: connected or lost, after the turns in between.
fn settles(state: &State) -> bool {
    matches!(state, State::Connected | State::Lost { .. })
}

proptest! {
    /// Connected only after a hello from our build; and `Shutdown` only after a hello from another
    /// (asserted by the fake).
    #[test]
    fn connected_only_after_a_hello_from_our_build(mut world in fake()) {
        for (before, _, what, after) in run(&mut world, 4 * TURNS) {
            if after == State::Connected && before != State::Connected {
                prop_assert_eq!(what, Observed::Hello { ours: true });
            }
        }
    }

    /// At most one service started on the way from Lost (or the start) to Connected or Lost.
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
    /// after each trouble, and ends connected; it's only lost through trouble while connected.
    #[test]
    fn with_a_working_service_it_connects(mut world in fake().prop_filter("works", Fake::works)) {
        let trace = run(&mut world, 10 * TURNS);
        let mut since = 0;
        for (before, _, _, after) in &trace {
            if matches!(after, State::Lost { .. }) && !matches!(before, State::Lost { .. }) {
                prop_assert_eq!(before, &State::Connected, "{:?}", after);
            }
            since = if *after == State::Connected { 0 } else { since + 1 };
            prop_assert!(since <= TURNS);
        }
        prop_assert_eq!(trace.last().map(|(_, _, _, s)| s), Some(&State::Connected));
    }
}
