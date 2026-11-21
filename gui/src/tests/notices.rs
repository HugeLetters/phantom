//! The status bar's notices (#120), generated: notices told (each severity, with more to read or
//! not), and a person clicking the bar, pressing Escape and Tab, clicking outside the popover and dismissing notices
//! in it; around it, rows selected and the row menu opened. A reference model knows the notices,
//! whether the popover is open, the selection and the menu. After
//! every step, through what's drawn:
//! - the bar shows the newest notice's short line in its own colour, then (elements of their own,
//!   which never shrink) a pill counting them, tinted by the worst severity among them, when there
//!   are several or the one has a list, and ›; nothing when there are none;
//! - the popover is drawn exactly while it's open, and shows one notice, the one the model says, under a head: ‹ n of m › when
//!   there are several (‹ and › disabled at the ends), with Dismiss all, which takes them all and
//!   closes it; and × (labelled Dismiss) on the notice's own line;
//! - ‹ and › step to a newer or an older notice, and so do ← and → (the focus is in the popover),
//!   stopping at the ends; what's shown
//!   stays while it's there, whatever is told around it; when it goes (its ×, the oldest dropped)
//!   the next older one is shown, else the nearest newer, else the popover closes;
//! - a click always opens it, at the newest, a lone notice with nothing more to read included: it
//!   never dismisses, only × does;
//! - Escape goes to the popover first (off the link field, which takes the first one): it closes
//!   it and keeps the notices and the selection, and only the next Escape clears the selection;
//!   a click outside closes it;
//! - no notice told opens the popover, an error included: it shows in the bar in its severity's
//!   colour with the count, and only a click opens it; one told while it's open leaves it on what
//!   it shows; a notice told never moves the focus, and typed text goes to the field. Errors have
//!   a cap of their own, apart from the others';
//! - one overlay at a time: the popover and the row menu, opening one closing the other;
//! - focus is in the popover while it's open, Tab too, and back on the bar's notice once it closes.
//!
//! 64 cases per `cargo test`; `PROPTEST_CASES` runs more.

use proptest::prelude::*;
use proptest::test_runner::Config;
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{app, settle, tap, torrent};
use crate::bar::machine::{MAX_TOLD, Way};
use crate::commands::{InApp, OnTorrents};
use crate::model::Phantom;
use crate::notice::{Notice, Severity};
use crate::service::MemoryService;
use crate::shortcuts;

#[derive(Debug, Clone)]
enum Step {
    Tell {
        severity: u8,
        more: bool,
    },
    Press,
    Escape,
    Outside,
    /// The × of the notice shown.
    Dismiss,
    /// Dismiss all.
    DismissAll,
    /// A click in the link field: it has the focus.
    Focus,
    /// "x" typed, or pasted: it goes where the focus is.
    Type,
    /// ‹ or › pressed.
    Button(Way),
    /// ← or →.
    Arrow(Way),
    /// A click on row `n`.
    Select(u64),
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
    more: bool,
    severity: u8,
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
    /// The row menu is open.
    menu: bool,
    /// The world has rows to select.
    rows: bool,
}

impl Model {
    /// The bar's pill: how many, and the worst severity among them; none below two.
    fn pill(&self) -> Option<(usize, u8)> {
        match self.shown.as_slice() {
            [] => None,
            [_] => None,
            all => Some((all.len(), all.iter().map(|s| s.severity).max().unwrap())),
        }
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
        if !now.contains(&shown) {
            let at = before.iter().position(|id| *id == shown).unwrap();
            self.open = before[at + 1..].iter().chain(before[..at].iter().rev()).find(|id| now.contains(id)).copied();
        }
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
            Step::Tell { more, severity } => {
                self.told += 1;
                self.shown.push(Shown { id: self.next_id, short: format!("Told {}", self.told), more: *more, severity: *severity });
                // No notice told, an error included, opens the popover or moves what it shows.
                self.next_id += 1;
                if self.shown.len() > MAX_TOLD {
                    self.shown.remove(0);
                }
            }
            // Always at the newest; never a dismissal.
            Step::Press => self.open = self.listed().first().copied(),
            // The menu, else the popover, else the page.
            Step::Escape if self.menu => self.menu = false,
            Step::Escape if self.open.is_some() => self.open = None,
            Step::Escape => self.selected = None,
            Step::Outside => self.open = None,
            Step::Dismiss => {
                let id = self.open.expect("open");
                self.shown.retain(|s| s.id != id);
            }
            // Closes the popover.
            Step::DismissAll => {
                self.shown.clear();
                self.open = None;
            }
            Step::Focus | Step::Type => {}
            Step::Button(way) => self.step(*way),
            // The popover is modal: the focus is in it, so the keys are its.
            Step::Arrow(way) => self.step(*way),
            Step::Select(n) => self.selected = Some(*n),
            Step::RowMenu(n) => {
                self.selected = Some(*n);
                self.menu = true;
            }
            Step::Tab => {}
        }
        self.settle(&before);
    }

    /// The overlay drawn: the menu, else the popover.
    fn up(&self) -> Option<&'static str> {
        if self.menu {
            Some("row-menu-menu")
        } else if self.open.is_some() {
            Some("notices-panel")
        } else {
            None
        }
    }

    /// What can be done now: the bar is clicked while it shows something, the popover's parts
    /// while it's open.
    fn can(&self, step: &Step) -> bool {
        match step {
            Step::Press => !self.shown.is_empty() && self.open.is_none() && !self.menu,
            Step::Outside | Step::Tab | Step::Arrow(_) => self.open.is_some(),
            Step::Dismiss => self.current().is_some(),
            Step::DismissAll => self.open.is_some() && self.shown.len() > 1,
            // A disabled button takes no press.
            Step::Button(way) => self.can_step(*way),
            // On the page, which the popover and the menu cover.
            Step::Select(_) | Step::RowMenu(_) => self.rows && self.open.is_none() && !self.menu,
            Step::Focus => self.open.is_none() && !self.menu,
            Step::Tell { .. } | Step::Escape | Step::Type => true,
        }
    }
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0..3u8, any::<bool>()).prop_map(|(severity, more)| Step::Tell { severity, more }),
        4 => Just(Step::Press),
        1 => Just(Step::Escape),
        1 => Just(Step::Outside),
        3 => Just(Step::Dismiss),
        2 => Just(Step::DismissAll),
        3 => Just(Step::Focus),
        3 => Just(Step::Type),
        4 => prop_oneof![Just(Way::Newer), Just(Way::Older)].prop_map(Step::Button),
        4 => prop_oneof![Just(Way::Newer), Just(Way::Older)].prop_map(Step::Arrow),
        2 => (0..ROWS).prop_map(Step::Select),
        1 => (0..ROWS).prop_map(Step::RowMenu),
        2 => Just(Step::Tab),
    ]
}

fn notice(short: String, severity: u8, more: bool) -> Notice {
    let severity = [Severity::Info, Severity::Warning, Severity::Error][severity as usize];
    let notice = Notice { severity, ..Notice::new(short) };
    if more { notice.with_text("More to read") } else { notice }
}

fn run(rt: &mut Runtime<Phantom>, m: &Model, step: &Step) {
    match step {
        Step::Tell { severity, more } => {
            let n = notice(format!("Told {}", m.told + 1), *severity, *more);
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        Step::Press => tap(rt, "notice-text"),
        // Off the link field, which takes Escape itself.
        Step::Escape => {
            rt.update(|_, tx| tx.blur());
            rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        }
        Step::DismissAll => tap(rt, "notice-dismiss-all"),
        Step::Focus => tap(rt, "magnet-field"),
        // Goes where the focus is: into the field if it has it, and nowhere else.
        // (With no field focused a paste is an add: that's the page's, not this test's.)
        Step::Type => {
            if focused(rt).is_some_and(|f| f.contains("magnet-field")) {
                // A paste may replace what the field had selected: the letter alternates, so what's
                // typed is told from what was there.
                let letter = if rt.state.magnet.text.ends_with('x') { "y" } else { "x" };
                rt.paste(letter);
                settle(rt);
                assert!(rt.state.magnet.text.ends_with(letter), "typed text goes to the field while it has the focus");
            }
        }
        Step::Outside => {
            // Far from the bar and its popover: the list's header.
            rt.pointer_down(480.0, 60.0);
            rt.pointer_up(480.0, 60.0);
        }
        Step::Dismiss => tap(rt, "notice-dismiss"),
        Step::Button(way) => tap(rt, if *way == Way::Newer { "notice-newer" } else { "notice-older" }),
        Step::Arrow(way) => {
            rt.key(KeyEvent { key: if *way == Way::Newer { Key::Left } else { Key::Right }, mods: Mods::default() });
        }
        Step::Select(n) => tap(rt, &format!("rows/{n}/name")),
        Step::RowMenu(n) => {
            let (x, y) = rt.find(&format!("rows/{n}/name")).expect("the row");
            rt.secondary_down(x, y);
            rt.secondary_up(x, y);
        }
        Step::Tab => rt.key(KeyEvent { key: Key::Tab, mods: Mods::default() }),
    }
    settle(rt);
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

fn drawn_color(rt: &Runtime<Phantom>, key: &str) -> Option<rune::Color> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).and_then(|e| e.text_color)
}

/// The colour of a notice of `severity` (0 info, 1 warning, 2 error) in the look the tests draw.
fn colour(severity: u8) -> rune::Color {
    let severity = [Severity::Info, Severity::Warning, Severity::Error][severity as usize];
    crate::ui::Ui::phantom(true).tone_color(crate::views::status_bar::tone_of(severity))
}

fn check(rt: &Runtime<Phantom>, m: &Model) {
    // The line, the pill and › are elements of their own (the pill and › never shrink), read apart.
    let newest = m.shown.last();
    assert_eq!(drawn_text(rt, "notice-text"), newest.map(|n| n.short.clone()), "the bar shows the newest");
    if let Some(newest) = newest {
        assert_eq!(drawn_color(rt, "notice-text"), Some(colour(newest.severity)), "in its own colour, a receipt never amber");
    }
    let pill = m.pill();
    assert_eq!(
        drawn_text(rt, "notice-count-text"),
        pill.map(|(n, _)| client::display::notices_count(n)),
        "the pill counts the notices, from two up"
    );
    if let Some((_, worst)) = pill {
        assert_eq!(drawn_color(rt, "notice-count-text"), Some(colour(worst)), "tinted by the worst of them");
    }
    assert_eq!(drawn_text(rt, "notice-open").is_some(), newest.is_some(), "› whenever there's a notice: a click opens it");
    assert_eq!(rt.find("notices-panel").is_some(), m.open.is_some(), "the popover is drawn exactly while open");
    assert_eq!(rt.find("notices-scrim").is_some(), m.open.is_some(), "with a scrim over the page");
    if m.open.is_some() {
        let told = m.shown.len();
        assert_eq!(rt.find("notice-dismiss-all").is_some(), told > 1, "Dismiss all while there are several notices");
        if told > 1 {
            let label = super::generated::texts_under(rt, "notice-dismiss-all");
            assert_eq!(label, [client::display::dismiss_all()], "Dismiss all");
        }
    }
    if let (Some(shown), Some(at)) = (m.current(), m.at()) {
        let total = m.shown.len();
        let listed: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("notice/")))
            .map(|e| e.key.unwrap())
            .collect();
        assert_eq!(listed, vec![format!("notice/{}", shown.id)], "the one notice the model says, and no other");
        assert!(rt.find("notice-dismiss").is_some(), "× on every notice: {shown:?}");
        let dismissals = rt.elements().iter().filter(|e| !e.exiting && e.key.as_deref() == Some("notice-dismiss")).count();
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
            !e.exiting && e.key.as_deref().is_some_and(|k| k == "notice-dismiss" || k == "notice-newer" || k == "notice-older")
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
    let drawn: Vec<&str> = ["notices-panel", "row-menu-menu"].into_iter().filter(|k| rt.find(k).is_some()).collect();
    assert_eq!(drawn, m.up().into_iter().collect::<Vec<_>>(), "one overlay at a time, the one the model says");
    let selected = crate::model::selected_torrents(&rt.state.table);
    assert_eq!(selected, m.selected.into_iter().collect::<Vec<_>>(), "the selection");
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
                // Paused: no snapshot changes by itself.
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
            // Focus goes back to where it was when a click opened the popover: the bar's notice.
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
        notices in 2..=8usize,
        lines in 1..30usize,
        size in prop_oneof![Just((640.0f32, 320.0f32)), Just((960.0, 600.0)), (640.0..1200.0f32, 320.0..800.0f32)],
    ) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport(size, 2.0);
        settle(&mut rt);
        for i in 0..notices {
            let text = vec!["A long explanation of what happened and what to do about it."; lines].join(" ");
            let action = crate::notice::NoticeAction::new("Do it", |_| {});
            let n = Notice::warning(format!("Told {i}")).with_text(text).with_actions(vec![action]);
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        settle(&mut rt);
        tap(&mut rt, "notice-text");
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
        notices in 2..=8usize,
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

proptest! {
    #![proptest_config(Config { cases: 16, ..config() })]

    /// However many errors, however long their lists and whatever the window (down to 640×320),
    /// with the message by Add shown or not: the popover a click opens lies under the toolbar, never over the message by Add, above the bar and
    /// inside the window; its list scrolls instead.
    #[test]
    fn the_error_popover_never_covers_the_toolbar_or_the_message_by_add(
        errors in 1..8usize,
        lines in 1..25usize,
        size in prop_oneof![Just((640.0f32, 320.0f32)), Just((960.0, 600.0)), (640.0..1200.0f32, 320.0..800.0f32)],
        note in any::<bool>(),
    ) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport(size, 2.0);
        settle(&mut rt);
        if note {
            // Add is off for an empty field: it says why.
            tap(&mut rt, "add");
        }
        for i in 0..errors {
            let text = vec!["A long explanation of what failed and what to do about it."; lines].join(" ");
            let n = Notice::error(format!("Failed {i}")).with_text(text);
            rt.update(move |s, _| s.bar.tell(n.clone()));
        }
        settle(&mut rt);
        // A click on the bar opens it (with the note by Add up, a tap would close the note).
        rt.update(|s, _| crate::overlay::press_notices(s));
        // Twice: the room above the bar is measured from the frame before.
        settle(&mut rt);
        settle(&mut rt);
        let rect = |rt: &Runtime<Phantom>, key: &str| super::generated::rect_of(rt, key).unwrap_or_else(|| panic!("no {key}"));
        let (panel, toolbar, bar) = (rect(&rt, "notices-panel"), rect(&rt, "toolbar"), rect(&rt, "status"));
        prop_assert!(panel[1] >= toolbar[1] + toolbar[3] - 0.5, "the popover {:?} is under the toolbar {:?}", panel, toolbar);
        prop_assert!(panel[1] + panel[3] <= bar[1] + 0.5 && panel[0] >= -0.5 && panel[0] + panel[2] <= size.0 + 0.5, "the popover {:?} is above the bar {:?}, in the window", panel, bar);
        if note {
            let add = rect(&rt, "add-note-content");
            prop_assert!(!super::generated::overlaps(panel, add), "the popover {:?} covers the message by Add {:?}", panel, add);
        }
    }
}

/// A notice with a list to read, told as an error.
fn error_with_list(short: String) -> Notice {
    Notice::error(short).with_text("What failed.")
}

/// An example, not a property: where the focus is is a walk through the real elements. Typing in the
/// link field while an error arrives: the popover stays closed, the error shows in the bar in the
/// error colour, the field keeps the focus and every key after it, and a click on the bar opens it.
#[test]
fn an_error_arriving_while_typing_stays_in_the_bar_and_leaves_the_focus() {
    let mut rt = app(MemoryService::new());
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    tap(&mut rt, "magnet-field");
    rt.paste("abc");
    settle(&mut rt);
    let before = focused(&rt);
    assert!(before.as_deref().is_some_and(|f| f.contains("magnet-field")), "the field has the focus: {before:?}");
    rt.update(|s, _| s.bar.tell(error_with_list("Failed".into())));
    settle(&mut rt);
    assert_eq!(focused(&rt), before, "the error moved no focus");
    assert!(!rt.state.bar.is_open() && rt.find("notices-panel").is_none(), "and opened nothing");
    assert_eq!(drawn_text(&rt, "notice-text").as_deref(), Some("Failed"), "it shows in the bar");
    rt.paste("def");
    settle(&mut rt);
    assert_eq!(rt.state.magnet.text, "abcdef", "and every key after it goes to the field");
    tap(&mut rt, "notice-text");
    assert!(rt.state.bar.is_open(), "a click on the bar opens the popover");
}

/// An example, not a property, for the reason above: from the keyboard, with the focus in the popover,
/// ← and → step the notices; Tab reaches × and Space dismisses with the focus staying on × as the next
/// shows.
#[test]
fn from_the_keyboard_the_popover_steps_and_dismisses_with_the_focus_going_on() {
    let mut rt = app(MemoryService::new());
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    for i in 0..3 {
        rt.update(move |s, _| s.bar.tell(error_with_list(format!("Failed {i}"))));
    }
    settle(&mut rt);
    tap(&mut rt, "notice-text");
    let on_dismiss = |rt: &Runtime<Phantom>| focused(rt).is_some_and(|f| f.split('/').any(|p| p == "notice-dismiss"));
    let at = |rt: &Runtime<Phantom>| rt.state.bar.shown().map(|(_, at, _)| at);
    assert_eq!(at(&rt), Some(0), "a click opens it at the newest");
    rt.key(KeyEvent { key: Key::Right, mods: Mods::default() });
    settle(&mut rt);
    assert_eq!(at(&rt), Some(1), "→ steps to an older notice with the focus in the popover");
    rt.key(KeyEvent { key: Key::Left, mods: Mods::default() });
    settle(&mut rt);
    assert_eq!(at(&rt), Some(0), "and ← to a newer one");
    for _ in 0..40 {
        if on_dismiss(&rt) {
            break;
        }
        rt.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
        settle(&mut rt);
    }
    assert!(on_dismiss(&rt), "Tab reaches ×");
    for left in [2, 1] {
        rt.key(KeyEvent { key: Key::Text(" ".into()), mods: Mods::default() });
        settle(&mut rt);
        assert_eq!(rt.state.bar.entries().len(), left, "Space on × dismisses one");
        assert!(on_dismiss(&rt), "the focus stays on × as the next notice shows");
    }
    rt.key(KeyEvent { key: Key::Text(" ".into()), mods: Mods::default() });
    settle(&mut rt);
    assert!(rt.state.bar.entries().is_empty() && rt.find("notices-panel").is_none(), "the last one goes, and the popover");
}

/// An example, not a property, for the reason above: Dismiss all from the keyboard (Space) takes
/// every notice and closes the popover; with the bar empty, nothing is left to hold the focus.
#[test]
fn dismiss_all_from_the_keyboard_takes_every_notice_and_closes_the_popover() {
    let mut rt = app(MemoryService::new());
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    for i in 0..2 {
        rt.update(move |s, _| s.bar.tell(error_with_list(format!("Failed {i}"))));
    }
    settle(&mut rt);
    tap(&mut rt, "notice-text");
    for _ in 0..40 {
        if focused(&rt).is_some_and(|f| f.split('/').any(|p| p == "notice-dismiss-all")) {
            break;
        }
        rt.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
        settle(&mut rt);
    }
    assert!(focused(&rt).is_some_and(|f| f.split('/').any(|p| p == "notice-dismiss-all")), "Tab reaches Dismiss all");
    rt.key(KeyEvent { key: Key::Text(" ".into()), mods: Mods::default() });
    settle(&mut rt);
    assert!(rt.state.bar.entries().is_empty(), "every notice goes");
    assert!(rt.find("notices-panel").is_none(), "and the popover is closed");
    assert_eq!(focused(&rt), None, "no notice is left to hold the focus");
}

/// An example, not a property (the generated steps are too few to reach the cap; the machine's table
/// has it for every state): told notices, errors among them, are kept up to the most, the oldest
/// dropped first, whatever they are.
#[test]
fn told_notices_errors_included_are_kept_up_to_the_most_and_the_oldest_goes_first() {
    let mut rt = app(MemoryService::new());
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    for i in 0..MAX_TOLD + 3 {
        let n = if i % 2 == 0 { Notice::error(format!("Told {i}")) } else { Notice::new(format!("Told {i}")) };
        rt.update(move |s, _| s.bar.tell(n.clone()));
    }
    settle(&mut rt);
    let shorts: Vec<String> = rt.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect();
    let expected: Vec<String> = (3..MAX_TOLD + 3).map(|i| format!("Told {i}")).collect();
    assert_eq!(shorts, expected, "the last {MAX_TOLD} stay, the three oldest (two of them errors) went");
    assert!(!rt.state.bar.is_open(), "and none opened the popover");
}

const LONG: &str = "The Complete Collected Works of an Artist With a Remarkably Long Name (Remastered 2008, 24-bit, 96 kHz)";
const FOLDER: &str = "/Volumes/An External Drive With a Long Name/Downloads/A Folder Deep Down";

/// What a notice's list shows: its texts, once the popover is open on it.
fn list_of(rt: &mut Runtime<Phantom>, id: crate::bar::machine::NoticeId) -> String {
    super::generated::show_notice(rt, id);
    let list = super::generated::texts_under(rt, &format!("notice/{id}")).join("\n");
    super::generated::close_popover(rt);
    list
}

/// Checks the notices told since `from`: no bar line holds the torrent's name, a file name or a
/// path, the notice has a list, and the list holds what the line left out.
fn only_fixed_words(rt: &mut Runtime<Phantom>, from: usize, outside: &[&str], step: &str) {
    let told: Vec<_> = rt.state.bar.entries().iter().skip(from).map(|e| (e.id, e.notice.short.clone(), e.notice.has_list())).collect();
    assert!(!told.is_empty(), "{step}: nothing was told");
    for (id, short, has_more) in told {
        for text in outside {
            assert!(!short.contains(text), "{step}: the bar line {short:?} holds {text:?}");
        }
        assert!(short.chars().count() <= 40, "{step}: the bar line {short:?} is not short");
        assert!(!short.contains('/'), "{step}: the bar line {short:?} holds a path");
        assert!(has_more, "{step}: {short:?} left something out, so it has more to read");
        let list = list_of(rt, id);
        for text in outside {
            assert!(list.contains(text), "{step}: the list of {short:?} lacks {text:?}: {list:?}");
        }
    }
}

/// The rule (#281): whatever happens to a torrent called something long, or a file somewhere deep,
/// the bar line says it in Phantom's own few words and the list behind it holds the name or path.
#[test]
fn a_bar_line_holds_only_fixed_words_and_the_names_are_in_the_list() {
    let torrents = vec![torrent(0, LONG), torrent(1, &format!("{LONG} II"))];
    let mut rt = app(MemoryService::new().with_torrents(torrents));
    let at = |rt: &Runtime<Phantom>| rt.state.bar.entries().len();
    let dismiss = |rt: &mut Runtime<Phantom>| rt.update(|s, _| s.bar.dismiss_all());

    tap(&mut rt, "rows/0");
    let from = at(&rt);
    super::row_pick(&mut rt, 0, "Copy magnet link");
    only_fixed_words(&mut rt, from, &[LONG], "Copy magnet link");
    dismiss(&mut rt);

    let from = at(&rt);
    rt.paste(&format!("magnet:?xt=urn:btih:{:040x}&dn=Another+name", 0));
    settle(&mut rt);
    only_fixed_words(&mut rt, from, &[LONG], "an add of one already in the list");
    dismiss(&mut rt);

    for (command, step) in [(OnTorrents::Remove, "Remove"), (OnTorrents::Delete, "Delete")] {
        tap(&mut rt, "rows/0");
        let from = at(&rt);
        rt.key(shortcuts::keys(command)[0].event());
        for _ in 0..30 {
            settle(&mut rt);
        }
        only_fixed_words(&mut rt, from, &[LONG], step);
        dismiss(&mut rt);
        let from = at(&rt);
        rt.update(|_, tx| tx.blur());
        rt.key(shortcuts::keys(InApp::Undo)[0].event());
        for _ in 0..30 {
            settle(&mut rt);
        }
        only_fixed_words(&mut rt, from, &[LONG], &format!("the undo of {step}"));
        dismiss(&mut rt);
    }

    // A failed Open or Reveal: the path and macOS's reason are the list's.
    struct Refusing;
    impl crate::platform::Finder for Refusing {
        fn show(&mut self, _: &std::path::Path, _: crate::platform::Show) -> Result<(), crate::platform::ShowError> {
            Err(crate::platform::ShowError::NotOnDisk)
        }
        fn exists(&self, _: &std::path::Path) -> bool {
            true
        }
    }
    for how in [crate::platform::Show::Open, crate::platform::Show::Reveal] {
        rt.state.finder = Box::new(Refusing);
        rt.update(|s, _| s.torrents[0].location = Some(proto::AbsolutePath::new(FOLDER).unwrap()));
        let from = at(&rt);
        rt.update(|s, _| crate::platform::show_torrent(s, 0, how));
        settle(&mut rt);
        only_fixed_words(&mut rt, from, &[FOLDER], &format!("{how:?} refused"));
        dismiss(&mut rt);
    }
}

const KEPT: &str = "/Users/me/Library/Application Support/Phantom/records.invalid.db";

/// A torrent list set aside as damaged (#308): a warning that stays until dismissed, the line in
/// Phantom's own words, the list with where it was kept and that the list started empty; told once
/// per set-aside, and not at all when the service started normally.
#[test]
fn a_torrent_list_set_aside_is_told_in_a_warning_with_its_path_in_the_list() {
    let mut rt = app(MemoryService::new().with_records_set_aside(KEPT));
    settle(&mut rt);
    settle(&mut rt);
    assert_eq!(rt.state.bar.entries().len(), 1, "told once");
    let notice = rt.state.bar.newest().cloned().expect("a notice");
    assert_eq!(notice.short, "Couldn't read the torrent list");
    assert_eq!(notice.severity, crate::notice::Severity::Warning);
    assert_eq!(notice.lasts, crate::notice::Lasts::UntilDismissed);
    let id = rt.state.bar.entries()[0].id;
    super::generated::show_notice(&mut rt, id);
    let list = super::generated::texts_under(&rt, &format!("notice/{id}")).join("\n");
    assert!(list.contains(KEPT), "{list}");
    assert!(list.contains("kept aside") && list.contains("empty list"), "{list}");

    let mut rt = app(MemoryService::new());
    settle(&mut rt);
    assert!(rt.state.bar.newest().is_none(), "a normal start tells nothing");
}

const PART: &str = "/Users/me/Downloads/Pack/b.bin";

/// A part of a file in the Trash the service couldn't keep on disk (#304): a warning that stays
/// until dismissed, the line in Phantom's own words, the list with the file, the reason and that
/// its Undo may need a re-download; told once while the service keeps saying it, and not at all
/// when it says nothing (a retry that kept it before it told).
#[test]
fn a_part_not_saved_is_told_in_a_warning_with_its_file_and_reason_in_the_list() {
    let mut rt = app(MemoryService::new().with_part_not_saved(PART, "No space left on device (os error 28)"));
    settle(&mut rt);
    settle(&mut rt);
    assert_eq!(rt.state.bar.entries().len(), 1, "told once");
    let notice = rt.state.bar.newest().cloned().expect("a notice");
    assert_eq!(notice.short, "Couldn't save part of a file in the Trash");
    assert_eq!(notice.severity, crate::notice::Severity::Warning);
    assert_eq!(notice.lasts, crate::notice::Lasts::UntilDismissed);
    let id = rt.state.bar.entries()[0].id;
    super::generated::show_notice(&mut rt, id);
    let list = super::generated::texts_under(&rt, &format!("notice/{id}")).join("\n");
    assert!(list.contains(PART) && list.contains("No space left on device"), "{list}");
    assert!(list.contains("re-download"), "{list}");

    let mut rt = app(MemoryService::new());
    settle(&mut rt);
    assert!(rt.state.bar.newest().is_none(), "nothing failing, nothing told");
}

/// A Move of a selection (#270) tells how many moved in its few words and, in its list, each
/// torrent that didn't, by name, with why: one skipped because it can't move, one cancelled in its
/// clash dialog.
#[test]
fn a_moves_bar_line_holds_only_fixed_words_and_the_torrents_that_didnt_move_are_in_its_list() {
    let mut fetching = torrent(2, LONG);
    fetching.status = proto::Status::FetchingInfo;
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, &format!("{LONG} II")), fetching]).holding("/Films");
    let mut rt = app(service);
    super::wait(&mut rt, 0.5);
    let from = rt.state.bar.entries().len();
    rt.update(|s, _| crate::model::moving::start(s, vec![0, 1, 2], proto::AbsolutePath::new("/Films").unwrap()));
    settle(&mut rt);
    tap(&mut rt, "clash-use-existing");
    tap(&mut rt, "clash-cancel");
    super::wait(&mut rt, 1.0);
    only_fixed_words(&mut rt, from, &[LONG], "a Move of a selection that moved one of three");
    assert_eq!(super::notice(&rt), Some("Moved 1 of 3"));
    let id = rt.state.bar.entries().last().expect("told").id;
    let list = list_of(&mut rt, id);
    assert!(list.contains("Cancelled") && list.contains(&format!("{LONG} II")), "{list:?}");
}

/// A clash answered for what the dialog showed, which changed by that torrent's turn (#270's
/// review, 1): its Move is refused then, and the notice's list says so in fixed words, with the
/// path; the bar line counts only.
#[test]
fn a_move_whose_new_folder_changed_while_it_waited_is_listed_with_why() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, LONG)]).holding("/Films").moves_take(2.0);
    let disk = service.disk();
    let mut rt = app(service);
    super::wait(&mut rt, 0.5);
    let from = rt.state.bar.entries().len();
    rt.update(|s, _| crate::model::moving::start(s, vec![0, 1], proto::AbsolutePath::new("/Films").unwrap()));
    settle(&mut rt);
    tap(&mut rt, "clash-apply-to-rest");
    tap(&mut rt, "clash-replace");
    super::wait(&mut rt, 0.5);
    disk.change(crate::service::DiskChange::Rewritten("/Films".into()));
    super::wait(&mut rt, 3.0);
    only_fixed_words(&mut rt, from, &[LONG], "a Move of a selection one of which was refused at its turn");
    assert_eq!(super::notice(&rt), Some("Moved 1 of 2"));
    let id = rt.state.bar.entries().last().expect("told").id;
    let list = list_of(&mut rt, id);
    assert!(list.contains(LONG) && list.contains("Its new folder changed while it waited: /Films/"), "{list:?}");
}

/// The connection's words get a button while it's wrong (#87): Show log, which reveals the
/// service's log in Finder, while the service is unavailable; Restart while a newer build replaced
/// it; none while it's fine. The words are short and fixed, and the bar's click is the notices'.
#[test]
fn a_wrong_connection_offers_its_button_after_its_words() {
    use crate::platform::{Show, Shown};
    use crate::service::Connection;
    use client::machine::Why;

    let mut rt = app(MemoryService::new());
    settle(&mut rt);
    assert_eq!(drawn_text(&rt, "connection").as_deref(), Some("Connected"));
    assert!(rt.find("show-log").is_none() && rt.find("restart").is_none(), "nothing to do while it's fine");

    rt.update(|s, _| s.connection = Connection::Lost { why: Why::DidNotStart, retry_at: std::time::Instant::now() });
    settle(&mut rt);
    assert_eq!(drawn_text(&rt, "connection").as_deref(), Some("Service unavailable"), "why is the hover's, not the line's");
    let shown = Shown::default();
    rt.state.finder = Box::new(shown.clone());
    tap(&mut rt, "show-log");
    assert_eq!(shown.all(), vec![(proto::data_dir().join("service.log"), Show::Reveal)], "Show log reveals the service's log");

    rt.update(|s, _| s.connection = Connection::Outdated);
    settle(&mut rt);
    assert_eq!(drawn_text(&rt, "connection").as_deref(), Some("Phantom was updated"));
    assert!(rt.find("restart").is_some() && rt.find("show-log").is_none(), "Restart, after its words");
}

/// Both drawings of × and Dismiss all (#87, for the user to choose between): where they sit, and
/// that each dismisses what it says, the one notice or all of them.
#[test]
fn both_cross_layouts_dismiss_the_right_thing() {
    use crate::bar::DismissLayout;
    for layout in [DismissLayout::OneRow, DismissLayout::TwoRows] {
        let mut rt = app(MemoryService::new());
        rt.set_viewport((960.0, 600.0), 2.0);
        rt.state.bar.dismiss_layout = layout;
        settle(&mut rt);
        let rect = |rt: &Runtime<Phantom>, key: &str| super::generated::rect_of(rt, key).unwrap_or_else(|| panic!("{layout:?}: no {key}"));
        // One notice: × alone; no pager row.
        rt.update(|s, _| s.bar.tell(error_with_list("Failed 0".into())));
        settle(&mut rt);
        tap(&mut rt, "notice-text");
        assert!(rt.find("notices-head").is_none() && rt.find("notice-dismiss-all").is_none(), "{layout:?}: no top row for one");
        let (panel, line, cross) = (rect(&rt, "notices-panel"), rect(&rt, "notice-line"), rect(&rt, "notice-dismiss"));
        assert!(inside(cross, panel) && cross[1] < line[1] + line[3], "{layout:?}: × beside the line, in the panel");
        // Several.
        for i in 1..4 {
            rt.update(move |s, _| s.bar.tell(error_with_list(format!("Failed {i}"))));
        }
        settle(&mut rt);
        let (head, all, cross, line) =
            (rect(&rt, "notices-head"), rect(&rt, "notice-dismiss-all"), rect(&rt, "notice-dismiss"), rect(&rt, "notice-line"));
        assert!(inside(all, head), "{layout:?}: Dismiss all on the top row");
        match layout {
            DismissLayout::OneRow => {
                assert!(inside(cross, head), "a: × on the top row");
                assert!(cross[0] >= all[0] + all[2], "a: × after Dismiss all, last");
                let divider = rect(&rt, "notices-head-divider");
                assert!(
                    inside(divider, head) && divider[0] >= all[0] + all[2] - 0.5 && divider[0] + divider[2] <= cross[0] + 0.5,
                    "a: a divider between them"
                );
                assert!(divider[2] <= 2.0 && divider[3] > 8.0, "a: a short vertical one: {divider:?}");
            }
            DismissLayout::TwoRows => {
                assert!(cross[1] >= head[1] + head[3] - 0.5, "b: × below the top row, with the notice");
                assert!(cross[1] < line[1] + line[3], "b: on the notice's own line");
                let line_rule = rect(&rt, "notices-head-rule");
                assert!(line_rule[1] >= head[1] + head[3] - 2.0 && line_rule[1] <= cross[1], "b: a hairline between: {line_rule:?}");
                assert!(line_rule[2] > 100.0 && line_rule[3] <= 1.5, "b: across the panel");
            }
        }
        // × takes the newest only; the others stay.
        let shown = rt.state.bar.shown().map(|(e, _, _)| e.notice.short.clone()).expect("one is shown");
        tap(&mut rt, "notice-dismiss");
        settle(&mut rt);
        assert_eq!(rt.state.bar.entries().len(), 3, "{layout:?}: × dismisses the shown notice alone");
        assert!(rt.state.bar.entries().iter().all(|e| e.notice.short != shown), "{layout:?}: the shown one went");
        // Dismiss all takes every one.
        tap(&mut rt, "notice-dismiss-all");
        settle(&mut rt);
        assert_eq!(rt.state.bar.entries().len(), 0, "{layout:?}: Dismiss all clears them");
    }
}
