//! Seeding limits (#123): when a finished torrent stops seeding, at a share ratio or after a
//! time, whichever comes first. What a torrent did is counted in rounds: a round starts when it's
//! added, and again when it's resumed after stopping at a limit, so a Resume isn't undone by the
//! same limit straight away. A Resume after a Pause goes on with its round.
//!
//! [`Meter`] counts a round from librqbit's live upload counter (which starts again at 0 each time
//! librqbit starts the torrent, and isn't kept across restarts: the actor says when it starts one)
//! and the time it seeds; the engine keeps the round with the torrent. [`reached`] says whether a round reached a limit: the actor
//! then tells the lifecycle so ([`crate::Observation::LimitReached`]).

use std::time::{Duration, Instant};

use proto::{Ratio, Reached, SeedTime, SeedingLimits};
use serde::{Deserialize, Serialize};

use crate::Wants;

/// The limits, with the time as a [`Duration`] so tests can use short ones.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub ratio: Option<Ratio>,
    /// How long to seed, and the limit that says so.
    pub time: Option<(Duration, SeedTime)>,
}

impl From<SeedingLimits> for Limits {
    fn from(limits: SeedingLimits) -> Self {
        Self { ratio: limits.ratio, time: limits.time.map(|t| (t.duration(), t)) }
    }
}

/// What a torrent did in its round.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Round {
    /// Bytes uploaded.
    pub uploaded: u64,
    /// How long it was seeding.
    pub seeded: Duration,
}

/// Counts a round from what librqbit reports at each poll.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Meter {
    pub round: Round,
    /// librqbit's live upload counter at the last poll.
    last_live: u64,
    /// When the last poll found it seeding, if it did.
    seeding_at: Option<Instant>,
}

impl Meter {
    /// Goes on with `round`, kept from before.
    pub fn new(round: Round) -> Self {
        Self { round, last_live: 0, seeding_at: None }
    }

    /// A poll at `now`: librqbit's live counter is at `live_uploaded`, and the torrent is
    /// `seeding` or not. The time since the last poll counts as seeded only if it was seeding at
    /// both, so it never counts time it wasn't seeding (and may miss part of one poll's interval).
    pub fn observe(&mut self, live_uploaded: u64, seeding: bool, now: Instant) {
        self.round.uploaded += live_uploaded.saturating_sub(self.last_live);
        self.last_live = live_uploaded;
        if seeding && let Some(since) = self.seeding_at {
            self.round.seeded += now.saturating_duration_since(since);
        }
        self.seeding_at = seeding.then_some(now);
    }

    /// librqbit starts the torrent (or adds it again): its live counter starts again at 0.
    pub fn librqbit_started(&mut self) {
        self.last_live = 0;
    }

    /// What's wanted went from `before` to `after`: a Resume after a limit starts a new round.
    pub fn wants_changed(&mut self, before: Wants, after: Wants) {
        if starts_round(before, after) {
            self.round = Round::default();
        }
    }
}

/// Whether going from wanting `before` to `after` starts a new round: resumed after stopping at
/// a limit.
pub fn starts_round(before: Wants, after: Wants) -> bool {
    matches!((before, after), (Wants::Seeded { .. }, Wants::Run))
}

/// The limit `round` reached, of a torrent of `size` bytes (its Size), if any: the ratio first
/// when both are. Size 0 has no ratio.
pub fn reached(round: &Round, size: u64, limits: &Limits) -> Option<Reached> {
    let ratio = limits.ratio.filter(|r| size > 0 && u128::from(round.uploaded) * 100 >= u128::from(r.hundredths()) * u128::from(size));
    let time = limits.time.filter(|(duration, _)| round.seeded >= *duration);
    ratio.map(Reached::Ratio).or(time.map(|(_, t)| Reached::Time(t)))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn limits() -> impl Strategy<Value = Limits> {
        let ratio = proptest::option::of((1..=500u64).prop_map(|h| Ratio::new(h).unwrap()));
        let time = proptest::option::of((1..=120u64).prop_map(|m| (Duration::from_secs(m), SeedTime::new(m).unwrap())));
        (ratio, time).prop_map(|(ratio, time)| Limits { ratio, time })
    }

    /// Polls over a round: more uploaded, librqbit starting the torrent again or not, the seconds
    /// since the last poll, and whether it's seeding.
    fn polls() -> impl Strategy<Value = Vec<(u64, bool, u64, bool)>> {
        proptest::collection::vec((0..2000u64, any::<bool>(), 0..3u64, any::<bool>()), 0..40)
    }

    proptest! {
        /// Every byte librqbit reports is counted once, across its counter starting again when it
        /// starts the torrent; the time counted is that between polls that both found it seeding,
        /// so never time it wasn't.
        #[test]
        fn a_round_counts_every_byte_once_and_only_time_seeded(steps in polls()) {
            let mut meter = Meter::default();
            let start = Instant::now();
            let (mut now, mut live, mut total, mut seeded, mut was_seeding) = (start, 0u64, 0u64, Duration::ZERO, false);
            for (more, restarted, secs, seeding) in steps {
                if restarted {
                    meter.librqbit_started();
                    live = 0;
                }
                live += more;
                total += more;
                now += Duration::from_secs(secs);
                if seeding && was_seeding {
                    seeded += Duration::from_secs(secs);
                }
                was_seeding = seeding;
                meter.observe(live, seeding, now);
                prop_assert_eq!(meter.round, Round { uploaded: total, seeded });
            }
        }

        /// Never before a limit, always at it: a round reaches the ratio exactly when uploaded ÷
        /// size ≥ the ratio, and the time exactly when it seeded that long; the ratio said first.
        #[test]
        fn a_limit_is_reached_exactly_at_it(uploaded in 0..10_000u64, size in 0..2_000u64, secs in 0..200u64, limits in limits()) {
            let round = Round { uploaded, seeded: Duration::from_secs(secs) };
            let at_ratio = limits.ratio.filter(|r| size > 0 && uploaded * 100 >= r.hundredths() * size).map(Reached::Ratio);
            let at_time = limits.time.filter(|(d, _)| secs >= d.as_secs()).map(|(_, t)| Reached::Time(t));
            prop_assert_eq!(reached(&round, size, &limits), at_ratio.or(at_time));
        }

        /// Right at the edge: exactly the ratio's bytes reach it, a byte fewer don't.
        #[test]
        fn exactly_at_the_ratio_it_is_reached_a_byte_short_it_is_not(h in 1..=Ratio::MAX_HUNDREDTHS, size in 1..1_000_000u64) {
            let ratio = Ratio::new(h).unwrap();
            let limits = Limits { ratio: Some(ratio), time: None };
            let at = (h * size).div_ceil(100);
            let round = |uploaded| Round { uploaded, seeded: Duration::ZERO };
            prop_assert_eq!(reached(&round(at), size, &limits), Some(Reached::Ratio(ratio)));
            prop_assert_eq!(reached(&round(at - 1), size, &limits), None);
        }

        /// A Resume after a limit starts a new round, which reached nothing yet: it isn't stopped
        /// straight away. Any other change of what's wanted keeps the round.
        #[test]
        fn a_resume_after_a_limit_is_not_undone_straight_away(uploaded in 0..10_000u64, secs in 0..200u64, limits in limits(), size in 1..2_000u64) {
            let reached_now = reached(&Round { uploaded, seeded: Duration::from_secs(secs) }, size, &limits);
            let seeded = Wants::Seeded { reached: reached_now.unwrap_or(Reached::Ratio(Ratio::new(100).unwrap())) };
            for (before, after) in [(seeded, Wants::Run), (Wants::Stop, Wants::Run), (Wants::Run, Wants::Stop), (Wants::Run, seeded)] {
                let mut meter = Meter::new(Round { uploaded, seeded: Duration::from_secs(secs) });
                meter.wants_changed(before, after);
                if starts_round(before, after) {
                    prop_assert_eq!(meter.round, Round::default());
                    prop_assert_eq!(reached(&meter.round, size, &limits), None);
                } else {
                    prop_assert_eq!(meter.round, Round { uploaded, seeded: Duration::from_secs(secs) });
                }
            }
        }
    }
}
