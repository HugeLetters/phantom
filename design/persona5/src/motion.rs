//! Motion: easing curves, components' eased hovered/pressed/disabled/selected amounts, and the progress reports' smoothing.

use rune::Spring;

use crate::common::*;

/// The run of reports every row shares, as (time, percent): first the common case,
/// +4 every 0.5 s from 0 to 40; then once a second, uneven steps with a jump from 52 to 85, up
/// to 100, down with a drop from 79 to 25, to 0.
pub(crate) const TOGETHER: [(f32, f32); 29] = [
    (2.0, 0.0),
    (2.5, 4.0),
    (3.0, 8.0),
    (3.5, 12.0),
    (4.0, 16.0),
    (4.5, 20.0),
    (5.0, 24.0),
    (5.5, 28.0),
    (6.0, 32.0),
    (6.5, 36.0),
    (7.0, 40.0),
    (8.0, 43.0),
    (9.0, 45.0),
    (10.0, 48.0),
    (11.0, 52.0),
    (12.0, 85.0),
    (13.0, 88.0),
    (14.0, 92.0),
    (15.0, 96.0),
    (16.0, 100.0),
    (17.0, 96.0),
    (18.0, 91.0),
    (19.0, 85.0),
    (20.0, 79.0),
    (21.0, 25.0),
    (22.0, 20.0),
    (23.0, 14.0),
    (24.0, 9.0),
    (25.0, 0.0),
];

/// How a demo moves every row's progress.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgressDemo {
    /// The reports in [`TOGETHER`], smoothed as the bar would draw a client's reports.
    Reports,
    /// A steady sweep from 0 to 100 and back, so every bar is seen over its whole range.
    Sweep,
}

/// The reports, as (time, 0–1).
pub(crate) fn reports() -> Vec<(f32, f32)> {
    TOGETHER.iter().map(|&(t, v)| (t, v / 100.0)).collect()
}

/// The index of the last report at or before `t` (the first before any).
pub(crate) fn last_report(r: &[(f32, f32)], t: f32) -> usize {
    r.iter().rposition(|&(at, _)| at <= t).unwrap_or(0)
}

/// What the percentage pill shows: the latest report as it is. Only the bar is smoothed.
pub fn reported_progress(s: &S, t: &Torrent) -> f32 {
    if s.progress_demo != Some(ProgressDemo::Reports) {
        return t.progress;
    }
    let r = reports();
    r[last_report(&r, s.t)].1
}

/// What the bar shows at `t`. Reports are followed by a spring whose target is the last report
/// carried on at the recent speed (the median of the last three steps' speeds), at most one
/// report interval ahead. A big jump is far from the spring's target, so it moves fast there by
/// itself. The spring never moves against the direction of the latest report, so it can't
/// wobble back.
pub(crate) fn demo_progress(demo: ProgressDemo, t: f32) -> f32 {
    if demo == ProgressDemo::Sweep {
        let u = t - 2.0;
        return if u < 0.0 {
            0.0
        } else if u < 6.0 {
            u / 6.0
        } else {
            (1.0 - (u - 6.0) / 6.0).max(0.0)
        };
    }
    // Every row asks for the same frame: simulate it once.
    thread_local! {
        static CACHE: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((-1.0, 0.0)) };
    }
    let (at, x) = CACHE.get();
    if at == t {
        return x;
    }
    let r = reports();
    let dt = 1.0 / 120.0;
    let w = 6.0;
    let (mut x, mut v) = (r[0].1, 0.0f32);
    let mut tau = 0.0;
    while tau < t {
        let k = last_report(&r, tau);
        let last = r[k].1;
        let mut dir = 0.0;
        let target = if tau >= r[0].0 && k >= 1 {
            let mut speeds: Vec<f32> =
                (0..3).filter(|j| k >= j + 1).map(|j| (r[k - j].1 - r[k - j - 1].1) / (r[k - j].0 - r[k - j - 1].0)).collect();
            speeds.sort_by(f32::total_cmp);
            let speed = speeds[speeds.len() / 2];
            let every = r[k].0 - r[k - 1].0;
            dir = (r[k].1 - r[k - 1].1).signum();
            (last + speed * (tau - r[k].0).min(every)).clamp(0.0, 1.0)
        } else {
            last
        };
        let a = w * w * (target - x) - 2.0 * w * v;
        v += a * dt;
        if dir > 0.0 {
            v = v.max(0.0);
        } else if dir < 0.0 {
            v = v.min(0.0);
        }
        x = (x + v * dt).clamp(0.0, 1.0);
        tau += dt;
    }
    CACHE.set((t, x));
    x
}

/// The ease-out-back of a snap: overshoots a touch, then settles, over `dur` seconds.
pub fn snap(t: f32, dur: f32) -> f32 {
    let x = (t / dur).clamp(0.0, 1.0);
    let c = 1.9;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

pub(crate) fn kick() -> Spring {
    Spring::new(0.28, 0.45)
}

/// A button's state as three amounts, 0–1, each a spring easing toward what the current look
/// wants: hovered, pressed, disabled. The Phantom Thieves button is drawn from these alone, so
/// any change of state (pressed to idle, hovered to disabled, a press let go halfway) is the
/// amounts easing from wherever they are, with no transition written per pair of states.
pub(crate) fn button_amounts(s: &S, key: &str, look: Look) -> [f32; 3] {
    thread_local! {
        static STATE: std::cell::RefCell<std::collections::HashMap<String, ([f32; 6], f32)>> = Default::default();
    }
    let target = match look {
        Look::Idle => [0.0, 0.0, 0.0],
        Look::Hover => [1.0, 0.0, 0.0],
        Look::Press => [1.0, 1.0, 0.0],
        Look::Disabled => [0.0, 0.0, 1.0],
    };
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let (x, at) = st.entry(key.to_owned()).or_insert(([target[0], 0.0, target[1], 0.0, target[2], 0.0], s.t));
        let mut left = (s.t - *at).clamp(0.0, 1.0);
        *at = s.t;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            left -= dt;
            for (k, &goal) in target.iter().enumerate() {
                // Hover comes in with a touch of overshoot, as the snap did, and leaves without;
                // the press is quicker; disabled the calmest.
                let (w, z) = match k {
                    0 if goal > x[0] => (17.0, 0.62),
                    0 => (16.0, 1.0),
                    1 => (24.0, 1.0),
                    _ => (13.0, 1.0),
                };
                let (pos, vel) = (x[2 * k], x[2 * k + 1]);
                let a = w * w * (goal - pos) - 2.0 * z * w * vel;
                x[2 * k + 1] = vel + a * dt;
                x[2 * k] = pos + x[2 * k + 1] * dt;
            }
        }
        [x[0], x[2], x[4]]
    })
}

/// A row's state as eased amounts (see `button_amounts`): how hovered (pressed counts) and how
/// selected it is, 0–1, and the seed its selection band was cut with when it was selected, so a
/// band fading out keeps its shape after another row is picked.
pub(crate) fn row_amounts(s: &S, key: &str, hovered: bool, selected: bool, seed_now: u32) -> (f32, f32, u32) {
    thread_local! {
        static STATE: std::cell::RefCell<std::collections::HashMap<String, ([f32; 4], f32, u32, bool)>> = Default::default();
    }
    let target = [if hovered { 1.0 } else { 0.0 }, if selected { 1.0 } else { 0.0 }];
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let (x, at, seed, was) = st.entry(key.to_owned()).or_insert(([target[0], 0.0, target[1], 0.0], s.t, seed_now, selected));
        if selected && (!*was || *seed != seed_now) {
            *seed = seed_now;
        }
        *was = selected;
        let mut left = (s.t - *at).clamp(0.0, 1.0);
        *at = s.t;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            left -= dt;
            for (k, &goal) in target.iter().enumerate() {
                // Selecting comes in with an overshoot (the Phantom Thieves' slam); everything
                // else settles.
                let (w, z) = match k {
                    1 if goal > x[2] => (20.0, 0.55),
                    // Deselecting is quick, so a name already back in its pale ink spends as
                    // little time as possible over a fading flag.
                    1 => (30.0, 1.0),
                    _ => (22.0, 1.0),
                };
                let (pos, vel) = (x[2 * k], x[2 * k + 1]);
                let a = w * w * (goal - pos) - 2.0 * z * w * vel;
                x[2 * k + 1] = vel + a * dt;
                x[2 * k] = pos + x[2 * k + 1] * dt;
            }
        }
        (x[0].max(0.0), x[2].max(0.0), *seed)
    })
}

/// 0 to 1 over the first fifth of `u`, for fading something in.
pub(crate) fn smooth_in(u: f32) -> f32 {
    (u / 0.2).clamp(0.0, 1.0)
}
