//! The status bar's notices (#120), generated: notices told (each severity, with more to read or
//! not), conditions holding and ceasing (live notices, as a disk short of space), and a person
//! clicking the bar, pressing Escape and Tab, clicking outside the popover and dismissing notices
//! in it; around it, rows selected, errors reported and the row menu opened. A reference model
//! knows the notices, whether the popover is open, the selection, the errors and the menu. After
//! every step, through what's drawn:
//! - the bar shows the newest notice's short line, how many more there are, and › exactly when a
//!   click opens something (several notices, or one with more to read); nothing when there are none;
//! - the popover is drawn exactly while it's open, and lists every notice, newest first, with ×
//!   (labelled Dismiss) on those told once and not on live ones;
//! - with one notice and nothing more to read, a click dismisses it;
//! - Escape goes to the popover first: it closes it and keeps the notices, the selection and the
//!   errors, and only the next Escape clears those; a click outside closes it too;
//! - one overlay at a time: the popover, the row menu and the error list, opening one closing
//!   the others (the error list hidden, its errors kept);
//! - focus is in the popover while it's open, Tab too, and back where it was once it closes.
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
    /// A click on row `n`.
    Select(u64),
    /// Something fails, as a command can: the error list shows it.
    Fail,
    /// A right-click on row `n`: its menu opens.
    RowMenu(u64),
    Tab,
}

/// The rows the service lists.
const ROWS: u64 = 2;

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
    selected: Option<u64>,
    /// Errors in the error list.
    errors: usize,
    /// The row menu is open.
    menu: bool,
    /// The world has rows to select. Then no conditions are told by hand: a selection makes the
    /// service send a snapshot, whose disks are the live notices, which would undo them.
    rows: bool,
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
            // The menu, else the popover, else the page.
            Step::Escape if self.menu => self.menu = false,
            Step::Escape if self.open => self.open = false,
            Step::Escape => {
                self.selected = None;
                self.errors = 0;
            }
            Step::Outside => self.open = false,
            Step::Dismiss(n) => {
                let id = self.listed()[*n].id;
                self.shown.retain(|s| s.id != id);
            }
            Step::Select(n) => self.selected = Some(*n),
            Step::Fail => {
                self.errors += 1;
                self.open = false;
                self.menu = false;
            }
            Step::RowMenu(n) => {
                self.selected = Some(*n);
                self.menu = true;
            }
            Step::Tab => {}
        }
        self.open &= self.openable();
    }

    /// The overlay drawn: the menu or the popover over the error list.
    fn up(&self) -> Option<&'static str> {
        if self.menu {
            Some("row-menu-menu")
        } else if self.open {
            Some("notices")
        } else if self.errors > 0 {
            Some("error-list")
        } else {
            None
        }
    }

    /// What can be done now: the bar is clicked while it shows something, the popover's parts
    /// while it's open.
    fn can(&self, step: &Step) -> bool {
        match step {
            Step::Press => !self.shown.is_empty() && !self.open && !self.menu,
            Step::Outside | Step::Tab => self.open,
            Step::Dismiss(n) => self.open && self.listed().get(*n).is_some_and(|s| s.live.is_none()),
            // On the page, which the popover and the menu cover.
            Step::Select(_) | Step::RowMenu(_) => self.rows && !self.open && !self.menu,
            Step::Live(_) => !self.rows,
            Step::Tell { .. } | Step::Escape | Step::Fail => true,
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
        2 => (0..ROWS).prop_map(Step::Select),
        1 => Just(Step::Fail),
        1 => (0..ROWS).prop_map(Step::RowMenu),
        2 => Just(Step::Tab),
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
        Step::Select(n) => tap(rt, &format!("rows/{n}/name")),
        Step::Fail => rt.update(|s, _| s.errors.report(Notice::error("Something failed"))),
        Step::RowMenu(n) => {
            let (x, y) = rt.find(&format!("rows/{n}/name")).expect("the row");
            rt.secondary_down(x, y);
            rt.secondary_up(x, y);
        }
        Step::Tab => rt.key(KeyEvent { key: Key::Tab, mods: Mods::default() }),
    }
    settle(rt);
    if matches!(step, Step::Escape) {
        // The errors cleared fade before the list goes.
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
        settle(rt);
    }
}

/// The path of the element focused now, if any.
fn focused(rt: &Runtime<Phantom>) -> Option<String> {
    rt.elements().into_iter().find(|e| e.focused && !e.exiting).map(|e| e.path)
}

/// Whether `path` is the popover's panel or inside it.
fn in_popover(rt: &Runtime<Phantom>, path: &str) -> bool {
    let panel = rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some("notices-panel")).expect("the panel");
    path == panel.path || path.starts_with(&format!("{}/", panel.path))
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
        // What VoiceOver reads: the control focus lands on inside each ×.
        let elements = rt.elements();
        for wrapper in elements.iter().filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice-dismiss/"))) {
            let control =
                elements.iter().find(|e| e.focusable && (e.path == wrapper.path || e.path.starts_with(&format!("{}/", wrapper.path))));
            assert_eq!(control.and_then(|c| c.label.as_deref()), Some(client::display::dismiss()), "× reads Dismiss");
        }
        let focus = focused(rt);
        assert!(focus.as_deref().is_some_and(|f| in_popover(rt, f)), "focus is in the popover while it's open: {focus:?}");
    }
    let drawn: Vec<&str> = ["notices", "row-menu-menu", "error-list"].into_iter().filter(|k| rt.find(k).is_some()).collect();
    assert_eq!(drawn, m.up().into_iter().collect::<Vec<_>>(), "one overlay at a time, the one the model says");
    let selected = crate::model::selected_torrents(&rt.state.table);
    assert_eq!(selected, m.selected.into_iter().collect::<Vec<_>>(), "the selection");
    let errors = rt.state.errors.entries().iter().filter(|e| !e.leaving).count();
    assert_eq!(errors, m.errors, "the errors in the error list");
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64);
    Config { cases, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_status_bar_shows_its_notices_and_opens_them_as_the_model_says(steps in proptest::collection::vec(step(), 1..24), with_rows in any::<bool>()) {
        let rows = (0..ROWS * u64::from(with_rows)).map(|id| proto::TorrentInfo {
                // Paused: no snapshot changes by itself, so the disks' live notices stay as told.
                status: proto::Status::Paused,
                download_bytes_per_sec: 0,
                ..super::torrent(id, &format!("Torrent {id}"))
            }).collect();
        let mut rt = app(MemoryService::new().with_torrents(rows));
        rt.set_viewport((960.0, 600.0), 2.0);
        settle(&mut rt);
        let mut m = Model { rows: with_rows, ..Model::default() };
        check(&rt, &m);
        for step in &steps {
            if !m.can(step) {
                continue;
            }
            let was_open = m.open;
            run(&mut rt, &m, step);
            m.apply(step);
            check(&rt, &m);
            if was_open && !m.open && matches!(step, Step::Escape | Step::Outside) {
                let trigger = rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some("notice")).map(|e| e.path);
                prop_assert_eq!(focused(&rt), trigger, "focus goes back to the bar's notice once the popover closes");
            }
        }
    }
}

/// `inner` lies inside `outer`, give or take half a point of rounding.
fn inside(inner: [f32; 4], outer: [f32; 4]) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[0] + inner[2] <= outer[0] + outer[2] + 0.5
        && inner[1] + inner[3] <= outer[1] + outer[3] + 0.5
}

proptest! {
    #![proptest_config(Config { cases: 16, ..config() })]

    /// However many notices and whatever the window, down to the smallest (640×320): the popover
    /// stays above the status bar and inside the window, and scrolls, so every notice's × can be
    /// brought into view inside it, and pressed there.
    #[test]
    fn the_popover_fits_above_the_bar_and_scrolls_to_every_notice(
        more in proptest::collection::vec(any::<bool>(), 4..=MAX_TOLD),
        size in prop_oneof![Just((640.0f32, 320.0f32)), Just((960.0, 600.0)), (640.0..1200.0f32, 320.0..800.0f32)],
    ) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport(size, 2.0);
        settle(&mut rt);
        for (i, more) in more.iter().enumerate() {
            let n = notice(format!("Told {i}"), 1, *more);
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        settle(&mut rt);
        tap(&mut rt, "notice");
        // Twice: the room above the bar is measured from the frame before.
        settle(&mut rt);
        let rect = |rt: &Runtime<Phantom>, key: &str| super::generated::rect_of(rt, key).unwrap_or_else(|| panic!("no {key}"));
        let panel = rect(&rt, "notices-panel");
        let bar = rect(&rt, "status");
        prop_assert!(panel[1] >= 0.0 && panel[1] + panel[3] <= bar[1] + 0.5, "the panel {:?} is above the bar {:?}, in the window", panel, bar);
        let ids: Vec<String> = rt.elements().into_iter().filter_map(|e| e.key.filter(|k| k.starts_with("notice-dismiss/"))).collect();
        prop_assert_eq!(ids.len(), more.len());
        for key in ids {
            let target = key.clone();
            rt.update(move |_, tx| tx.scroll_into_view(target.clone()));
            settle(&mut rt);
            let at = rect(&rt, &key);
            prop_assert!(inside(at, rect(&rt, "notices-scroll")) && inside(at, panel), "{} at {:?} isn't inside the panel {:?}", key, at, panel);
        }
    }
}
