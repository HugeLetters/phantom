//! The status bar's notices (#120), generated: notices told (each severity, with more to read or
//! not), conditions holding and ceasing (live notices, as a disk short of space), and a person
//! clicking the bar, pressing Escape and Tab, clicking outside the popover and dismissing notices
//! in it; around it, rows selected, errors reported and the row menu opened. A reference model
//! knows the notices, whether the popover is open, the selection, the errors and the menu. After
//! every step, through what's drawn:
//! - the bar shows the newest notice's short line, then (as an element of its own, which never
//!   shrinks) "· 1/n" when there are several, and › exactly
//!   when a click opens something (several notices, or one with more to read); nothing when there
//!   are none;
//! - the popover is drawn exactly while it's open, and shows one notice, the one the model says,
//!   under a head: ‹ n of m › when there are several (‹ and › disabled at the ends), and × (labelled
//!   Dismiss) on one told once and not on a live one;
//! - ‹ and › and the ← and → keys step to a newer or an older notice, and stop at the ends; what's
//!   shown stays while it's there, whatever is told or ceases around it; when it goes (its ×, its
//!   condition ceasing, the oldest told dropped) the next older one is shown, else the nearest
//!   newer, else the popover closes;
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
use crate::bar::machine::{MAX_TOLD, Way};
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
    /// The × of the notice shown.
    Dismiss,
    /// ‹ or › pressed.
    Button(Way),
    /// ← or →.
    Arrow(Way),
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
    /// The popover is open, showing this notice.
    open: Option<u64>,
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

    /// The notices' ids, newest first.
    fn listed(&self) -> Vec<u64> {
        self.shown.iter().rev().map(|s| s.id).collect()
    }

    /// The notice the popover shows.
    fn current(&self) -> Option<&Shown> {
        self.shown.iter().find(|s| Some(s.id) == self.open)
    }

    /// Where the popover's notice is, newest first.
    fn at(&self) -> Option<usize> {
        self.listed().iter().position(|id| Some(*id) == self.open)
    }

    /// The notices changed from `before` (ids, newest first): what's shown stays while it's there;
    /// else the next older one that is, else the nearest newer; else the popover closes.
    fn settle(&mut self, before: &[u64]) {
        let Some(shown) = self.open else { return };
        let now = self.listed();
        if now.contains(&shown) {
            return;
        }
        let at = before.iter().position(|id| *id == shown).unwrap();
        self.open = before[at + 1..].iter().chain(before[..at].iter().rev()).find(|id| now.contains(id)).copied();
    }

    /// One step toward `way`, if there's a notice there.
    fn step(&mut self, way: Way) {
        let (Some(at), list) = (self.at(), self.listed()) else { return };
        let to = match way {
            Way::Newer => at.checked_sub(1),
            Way::Older => Some(at + 1).filter(|to| *to < list.len()),
        };
        if let Some(to) = to {
            self.open = Some(list[to]);
        }
    }

    /// Whether `way` has a notice to go to.
    fn can_step(&self, way: Way) -> bool {
        match (self.at(), way) {
            (Some(at), Way::Newer) => at > 0,
            (Some(at), Way::Older) => at + 1 < self.shown.len(),
            (None, _) => false,
        }
    }

    fn apply(&mut self, step: &Step) {
        let before = self.listed();
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
            Step::Press if self.openable() => self.open = self.listed().first().copied(),
            Step::Press => self.shown.clear(),
            // The menu, else the popover, else the page.
            Step::Escape if self.menu => self.menu = false,
            Step::Escape if self.open.is_some() => self.open = None,
            Step::Escape => {
                self.selected = None;
                self.errors = 0;
            }
            Step::Outside => self.open = None,
            Step::Dismiss => {
                let id = self.open.expect("open");
                self.shown.retain(|s| s.id != id);
            }
            Step::Button(way) | Step::Arrow(way) => self.step(*way),
            Step::Select(n) => self.selected = Some(*n),
            Step::Fail => {
                self.errors += 1;
                self.open = None;
                self.menu = false;
            }
            Step::RowMenu(n) => {
                self.selected = Some(*n);
                self.menu = true;
            }
            Step::Tab => {}
        }
        self.settle(&before);
    }

    /// The overlay drawn: the menu or the popover over the error list.
    fn up(&self) -> Option<&'static str> {
        if self.menu {
            Some("row-menu-menu")
        } else if self.open.is_some() {
            Some("notices-panel")
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
            Step::Press => !self.shown.is_empty() && self.open.is_none() && !self.menu,
            Step::Outside | Step::Tab => self.open.is_some(),
            Step::Dismiss => self.current().is_some_and(|s| s.live.is_none()),
            // A disabled button takes no press.
            Step::Button(way) => self.can_step(*way),
            // The keys are the popover's while it's open, at the ends too.
            Step::Arrow(_) => self.open.is_some(),
            // On the page, which the popover and the menu cover.
            Step::Select(_) | Step::RowMenu(_) => self.rows && self.open.is_none() && !self.menu,
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
        3 => Just(Step::Dismiss),
        4 => prop_oneof![Just(Way::Newer), Just(Way::Older)].prop_map(Step::Button),
        4 => prop_oneof![Just(Way::Newer), Just(Way::Older)].prop_map(Step::Arrow),
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
        Step::Dismiss => tap(rt, &format!("notice-dismiss/{}", m.open.expect("open"))),
        Step::Button(way) => tap(rt, if *way == Way::Newer { "notice-newer" } else { "notice-older" }),
        Step::Arrow(way) => rt.key(KeyEvent { key: if *way == Way::Newer { Key::Left } else { Key::Right }, mods: Mods::default() }),
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
    // The line and the count are elements of their own (the count never shrinks), read apart.
    let newest = m.shown.last();
    assert_eq!(drawn_text(rt, "notice-text"), newest.map(|n| n.short.clone()), "the bar shows the newest");
    let tail = newest.map(|_| format!("{}{}", client::display::notices_count(m.shown.len()), if m.openable() { " ›" } else { "" }));
    assert_eq!(
        drawn_text(rt, "notice-count"),
        tail.filter(|t| !t.is_empty()),
        "the bar says which of how many, and › when a click opens something"
    );
    assert_eq!(rt.find("notices-panel").is_some(), m.open.is_some(), "the popover is drawn exactly while open");
    if let (Some(shown), Some(at)) = (m.current(), m.at()) {
        let total = m.shown.len();
        let listed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice/")))
            .map(|e| e.key.unwrap())
            .collect();
        assert_eq!(listed, vec![format!("notice/{}", shown.id)], "the one notice the model says, and no other");
        assert_eq!(rt.find(&format!("notice-dismiss/{}", shown.id)).is_some(), shown.live.is_none(), "× on a told one only: {shown:?}");
        let dismissals =
            rt.elements().iter().filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice-dismiss/"))).count();
        assert!(dismissals <= 1, "× for the shown one alone");
        assert_eq!(
            drawn_text(rt, "notice-place"),
            (total > 1).then(|| client::display::notice_place(at, total)),
            "its place among them, when there are several"
        );
        assert_eq!(rt.find("notice-newer").is_some() && rt.find("notice-older").is_some(), total > 1, "‹ and › when there are several");
        if total > 1 {
            let enabled = |key: &str| {
                let elements = rt.elements();
                let wrapper = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).expect("the button").path.clone();
                elements.iter().any(|e| !e.exiting && e.focusable && (e.path == wrapper || e.path.starts_with(&format!("{wrapper}/"))))
            };
            assert_eq!(enabled("notice-newer"), m.can_step(Way::Newer), "‹ is disabled at the newest");
            assert_eq!(enabled("notice-older"), m.can_step(Way::Older), "› is disabled at the oldest");
        }
        // What VoiceOver reads: the control focus lands on inside each button of the head.
        let elements = rt.elements();
        let wrappers = elements.iter().filter(|e| {
            !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice-dismiss/") || k == "notice-newer" || k == "notice-older")
        });
        for wrapper in wrappers {
            let control =
                elements.iter().find(|e| e.focusable && (e.path == wrapper.path || e.path.starts_with(&format!("{}/", wrapper.path))));
            let expected = match wrapper.key.as_deref() {
                Some("notice-newer") => client::display::newer(),
                Some("notice-older") => client::display::older(),
                _ => client::display::dismiss(),
            };
            if let Some(control) = control {
                assert_eq!(control.label.as_deref(), Some(expected), "a head button reads what it does");
            }
        }
        let focus = focused(rt);
        assert!(focus.as_deref().is_some_and(|f| in_popover(rt, f)), "focus is in the popover while it's open: {focus:?}");
    }
    let drawn: Vec<&str> = ["notices-panel", "row-menu-menu", "error-list"].into_iter().filter(|k| rt.find(k).is_some()).collect();
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
            let was_open = m.open.is_some();
            run(&mut rt, &m, step);
            m.apply(step);
            check(&rt, &m);
            if was_open && m.open.is_none() && matches!(step, Step::Escape | Step::Outside) {
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

    /// However many notices and whatever the window, down to the smallest (640×320): each notice,
    /// stepped to, shows with the popover above the status bar and inside the window, its head (the
    /// steps and ×) pinned in view, and its explanation scrolling, so its last button can be brought
    /// into view inside the panel, and pressed there.
    #[test]
    fn the_popover_fits_above_the_bar_and_scrolls_to_every_notices_buttons(
        notices in 2..=MAX_TOLD,
        lines in 1..30usize,
        size in prop_oneof![Just((640.0f32, 320.0f32)), Just((960.0, 600.0)), (640.0..1200.0f32, 320.0..800.0f32)],
    ) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport(size, 2.0);
        settle(&mut rt);
        for i in 0..notices {
            let text = vec!["A long explanation of what happened and what to do about it."; lines].join(" ");
            let action = crate::notice::NoticeAction { title: "Do it".into(), run: Rc::new(|_| {}) };
            let n = Notice { more: Some(crate::notice::explained(text, vec![action])), ..Notice::warning(format!("Told {i}")) };
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        settle(&mut rt);
        tap(&mut rt, "notice");
        // Twice: the room above the bar is measured from the frame before.
        settle(&mut rt);
        let rect = |rt: &Runtime<Phantom>, key: &str| super::generated::rect_of(rt, key).unwrap_or_else(|| panic!("no {key}"));
        for shown in 0..notices {
            let panel = rect(&rt, "notices-panel");
            let bar = rect(&rt, "status");
            prop_assert!(panel[1] >= 0.0 && panel[1] + panel[3] <= bar[1] + 0.5, "the panel {:?} is above the bar {:?}, in the window", panel, bar);
            for key in ["notices-head", "notice-newer", "notice-older"] {
                let at = rect(&rt, key);
                prop_assert!(inside(at, panel), "{} at {:?} isn't inside the panel {:?}", key, at, panel);
            }
            rt.update(|_, tx| tx.scroll_into_view("notice-action/0".to_owned()));
            settle(&mut rt);
            let (at, scroll, panel) = (rect(&rt, "notice-action/0"), rect(&rt, "notices-scroll"), rect(&rt, "notices-panel"));
            prop_assert!(inside(at, scroll) && inside(at, panel), "the action at {:?} isn't inside the panel {:?}", at, panel);
            prop_assert_eq!(rt.state.bar.shown().map(|(_, at, _)| at), Some(shown));
            if shown + 1 < notices {
                tap(&mut rt, "notice-older");
                settle(&mut rt);
            }
        }
    }
}

proptest! {
    #![proptest_config(Config { cases: 16, ..config() })]

    /// However long the newest notice's line and however wide the window (from the smallest, 640),
    /// with several notices the count and › are drawn whole inside the bar, clear of the line and
    /// the totals: only the line truncates.
    #[test]
    fn the_count_stays_whole_in_the_bar_whatever_the_line_and_the_width(
        notices in 2..=MAX_TOLD,
        words in 1..60usize,
        width in prop_oneof![Just(640.0f32), 640.0..1600.0f32],
        with_rows in any::<bool>(),
    ) {
        // The count's width with a short line, where nothing is cut: what it must have with a long one.
        let drawn = |words: usize| {
            let rows = (0..ROWS * u64::from(with_rows)).map(|id| super::torrent(id, &format!("Torrent {id}"))).collect();
            let mut rt = app(MemoryService::new().with_torrents(rows));
            rt.set_viewport((width, 320.0), 2.0);
            settle(&mut rt);
            for i in 0..notices {
                let n = Notice::warning(format!("Told {i}{}", " a long line".repeat(words)));
                rt.update(move |s, _| s.bar.tell(n.clone()));
            }
            settle(&mut rt);
            rt
        };
        let whole = super::generated::rect_of(&drawn(0), "notice-count").expect("the count")[2];
        let rt = drawn(words);
        // Other elements are keyed "status" too (a cell): the bar is the one at the bottom.
        let bar = rt.elements().into_iter().filter(|e| e.key.as_deref() == Some("status") && !e.exiting).map(|e| e.rect).fold([0.0; 4], |a, r| if r[1] >= a[1] { r } else { a });
        let rect = |key: &str| super::generated::rect_of(&rt, key).unwrap_or_else(|| panic!("no {key}"));
        let (text, count) = (rect("notice-text"), rect("notice-count"));
        prop_assert!(count[2] >= whole - 0.5, "the count {:?} is cut from {} wide", count, whole);
        prop_assert!(inside(count, bar), "the count {:?} isn't inside the bar {:?}", count, bar);
        prop_assert!(text[0] + text[2] <= count[0] + 0.5, "the line {:?} runs over the count {:?}", text, count);
        if let Some(totals) = super::generated::rect_of(&rt, "totals") {
            prop_assert!(count[0] + count[2] <= totals[0] + 0.5, "the count {:?} runs over the totals {:?}", count, totals);
        }
    }
}
