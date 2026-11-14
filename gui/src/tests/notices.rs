//! The status bar's notices (#120), generated: notices told (each severity, with more to read or
//! not), conditions holding and ceasing (live notices, as a disk short of space), and a person
//! clicking the bar, pressing Escape, clicking outside the popover and dismissing notices in it.
//! A reference model knows the notices and whether the popover is open. After every step,
//! through what's drawn:
//! - the bar shows the newest notice's short line, how many more there are, and › exactly when a
//!   click opens something (several notices, or one with more to read); nothing when there are none;
//! - the popover is drawn exactly while it's open, and lists every notice, newest first, with ×
//!   on those told once and not on live ones;
//! - with one notice and nothing more to read, a click dismisses it;
//! - Escape and a click outside close the popover and keep the notices.
//!
//! 64 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::rc::Rc;

use proptest::prelude::*;
use proptest::test_runner::Config;
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{app, settle, tap};
use crate::bar::machine::MAX_TOLD;
use crate::model::Phantom;
use crate::notice::{Notice, Severity};
use crate::service::MemoryService;

const KEYS: [&str; 2] = ["disk-a", "disk-b"];

#[derive(Debug, Clone)]
enum Step {
    Tell {
        severity: u8,
        more: bool,
    },
    /// These conditions hold now.
    Live(Vec<bool>),
    Press,
    Escape,
    Outside,
    /// The ×, of the `n`th notice listed.
    Dismiss(usize),
}

#[derive(Debug, Clone)]
struct Shown {
    id: u64,
    short: String,
    live: Option<&'static str>,
    more: bool,
}

#[derive(Debug, Clone, Default)]
struct Model {
    /// Oldest first.
    shown: Vec<Shown>,
    next_id: u64,
    told: usize,
    open: bool,
}

impl Model {
    fn openable(&self) -> bool {
        self.shown.len() > 1 || self.shown.iter().any(|s| s.more)
    }

    /// The notices listed, newest first.
    fn listed(&self) -> Vec<&Shown> {
        self.shown.iter().rev().collect()
    }

    fn apply(&mut self, step: &Step) {
        match step {
            Step::Tell { more, .. } => {
                self.told += 1;
                self.shown.push(Shown { id: self.next_id, short: format!("Told {}", self.told), live: None, more: *more });
                self.next_id += 1;
                while self.shown.iter().filter(|s| s.live.is_none()).count() > MAX_TOLD {
                    let oldest = self.shown.iter().position(|s| s.live.is_none()).unwrap();
                    self.shown.remove(oldest);
                }
            }
            Step::Live(holds) => {
                let holding: Vec<&'static str> = KEYS.iter().zip(holds).filter(|(_, h)| **h).map(|(k, _)| *k).collect();
                self.shown.retain(|s| s.live.is_none_or(|k| holding.contains(&k)));
                for key in holding {
                    if !self.shown.iter().any(|s| s.live == Some(key)) {
                        self.shown.push(Shown { id: self.next_id, short: format!("Short on {key}"), live: Some(key), more: true });
                        self.next_id += 1;
                    }
                }
            }
            Step::Press if self.openable() => self.open = true,
            Step::Press => self.shown.clear(),
            Step::Escape | Step::Outside => self.open = false,
            Step::Dismiss(n) => {
                let id = self.listed()[*n].id;
                self.shown.retain(|s| s.id != id);
            }
        }
        self.open &= self.openable();
    }

    /// What can be done now: the bar is clicked while it shows something, the popover's parts
    /// while it's open.
    fn can(&self, step: &Step) -> bool {
        match step {
            Step::Press => !self.shown.is_empty() && !self.open,
            Step::Outside => self.open,
            Step::Dismiss(n) => self.open && self.listed().get(*n).is_some_and(|s| s.live.is_none()),
            Step::Tell { .. } | Step::Live(_) | Step::Escape => true,
        }
    }
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0..3u8, any::<bool>()).prop_map(|(severity, more)| Step::Tell { severity, more }),
        2 => proptest::collection::vec(any::<bool>(), 2).prop_map(Step::Live),
        4 => Just(Step::Press),
        1 => Just(Step::Escape),
        1 => Just(Step::Outside),
        3 => (0..7usize).prop_map(Step::Dismiss),
    ]
}

fn notice(short: String, severity: u8, more: bool) -> Notice {
    let severity = [Severity::Info, Severity::Warning, Severity::Error][severity as usize];
    let view: Option<crate::notice::View> =
        more.then(|| Rc::new(|_: &Phantom, ui: &crate::ui::Ui| ui.caption("More to read")) as crate::notice::View);
    Notice { short, severity, more: view }
}

fn run(rt: &mut Runtime<Phantom>, m: &Model, step: &Step) {
    match step {
        Step::Tell { severity, more } => {
            let n = notice(format!("Told {}", m.told + 1), *severity, *more);
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        Step::Live(holds) => {
            let live: Vec<(String, Notice)> = KEYS
                .iter()
                .zip(holds)
                .filter(|(_, h)| **h)
                .map(|(k, _)| (k.to_string(), notice(format!("Short on {k}"), 1, true)))
                .collect();
            rt.update(move |s, _| s.bar.live(live.clone()));
        }
        Step::Press => tap(rt, "notice"),
        Step::Escape => rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
        Step::Outside => {
            // Far from the bar and its popover: the list's header.
            rt.pointer_down(480.0, 60.0);
            rt.pointer_up(480.0, 60.0);
        }
        Step::Dismiss(n) => tap(rt, &format!("notice-dismiss/{}", m.listed()[*n].id)),
    }
    settle(rt);
}

fn drawn_text(rt: &Runtime<Phantom>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).and_then(|e| e.text)
}

fn check(rt: &Runtime<Phantom>, m: &Model) {
    let bar = drawn_text(rt, "notice");
    let expected = m.shown.last().map(|newest| {
        let more = client::display::more_notices(m.shown.len() - 1);
        format!("{}{more}{}", newest.short, if m.openable() { " ›" } else { "" })
    });
    assert_eq!(bar, expected, "the bar shows the newest, how many more, and › when a click opens something");
    assert_eq!(rt.find("notices").is_some(), m.open, "the popover is drawn exactly while open");
    if m.open {
        let listed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice/")))
            .map(|e| e.key.unwrap())
            .collect();
        let expected: Vec<String> = m.listed().iter().map(|s| format!("notice/{}", s.id)).collect();
        assert_eq!(listed, expected, "every notice, newest first");
        for s in m.listed() {
            assert_eq!(rt.find(&format!("notice-dismiss/{}", s.id)).is_some(), s.live.is_none(), "× on told ones only: {s:?}");
        }
    }
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64);
    Config { cases, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_status_bar_shows_its_notices_and_opens_them_as_the_model_says(steps in proptest::collection::vec(step(), 1..24)) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport((960.0, 600.0), 2.0);
        settle(&mut rt);
        let mut m = Model::default();
        check(&rt, &m);
        for step in &steps {
            if !m.can(step) {
                continue;
            }
            run(&mut rt, &m, step);
            m.apply(step);
            check(&rt, &m);
        }
    }
}
