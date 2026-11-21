//! The status bar: the connection to the service, its notices ([`crate::bar`]), and the totals.
//! The bar shows the newest notice's line in its own colour, its Undo while there is one, a pill
//! (from two notices up) with how many there are tinted by the worst of them, and ›; a click always opens a popover above it, attached by a tail,
//! showing one notice at a time, newest first: its mark, its line and ×, the names, why, and its
//! buttons. Only × dismisses (#87). The popover opens over the other overlays ([`crate::overlay`]).
//!
//! While the connection is wrong, its few words get a button: Show log, Restart.

use rune::{El, col, el, row};

use super::speed;
use crate::bar::machine::{Entry, Way};
use crate::model::Phantom;
use crate::notice::{NoticeAction, Severity};
use crate::service::Connection;
use crate::ui::{Icon, Tone, Ui};

/// `room`: how tall the popover may be, above the bar.
pub fn status_bar(s: &Phantom, ui: &Ui, room: f32, layer: &mut Vec<El>) -> El {
    let tone = match &s.connection {
        Connection::Connected { .. } => Tone::Good,
        Connection::Lost { .. } | Connection::Outdated => Tone::Error,
        Connection::Connecting | Connection::Replacing { .. } | Connection::Spawning { .. } => Tone::Muted,
    };
    let mut words = ui.caption(client::display::connection(&s.connection)).key("connection");
    if let Some(why) = client::display::connection_why(&s.connection) {
        words = words.help(why);
    }
    let down: u64 = s.torrents.iter().map(|t| t.download_bytes_per_sec).sum();
    let up: u64 = s.torrents.iter().map(|t| t.upload_bytes_per_sec).sum();
    let totals = match s.torrents.len() {
        0 => String::new(),
        1 => format!("1 torrent · ↓ {} · ↑ {}", speed(down), speed(up)),
        n => format!("{n} torrents · ↓ {} · ↑ {}", speed(down), speed(up)),
    };

    let mut bar =
        row().key("status").w_full().gap(8.0).center_y().child(ui.status_dot(tone)).child(words).children(connection_button(s, ui));
    if let Some(newest) = s.bar.newest() {
        bar = bar.child(notices(s, newest.short.clone(), tone_of(newest.severity), ui, room, layer));
    }
    bar.child(el().grow(1.0)).child(ui.figures(totals).key("totals"))
}

/// The button after the connection's words while it's wrong: Show log reveals the service's log;
/// Restart restarts the app, which a newer build replaced.
fn connection_button(s: &Phantom, ui: &Ui) -> Option<El> {
    let button = match &s.connection {
        Connection::Lost { .. } => ui.button("show-log", client::display::show_log()).on_press(|s: &mut Phantom| {
            crate::platform::show(s, &proto::data_dir().join("service.log"), crate::platform::Show::Reveal);
        }),
        Connection::Outdated => ui.button("restart", client::display::restart_app()).on_press_tx(crate::platform::restart),
        _ => return None,
    };
    Some(button.small().build())
}

/// The bar's Undo, right after the newest notice's words: the newest action's notice's Undo, in the
/// small button the connection's are, for as long as that notice lasts and its Undo can be pressed;
/// its hover tip ⌘Z.
fn bar_undo(s: &Phantom, ui: &Ui) -> Option<El> {
    let undo = s.bar.newest_undo()?;
    if undo.off.as_ref().and_then(|off| off(s)).is_some() {
        return None;
    }
    let run = undo.run.clone();
    let hint = undo.hint.as_ref().and_then(|hint| hint(s));
    let tip = hint.map(|hint| format!("{}  {hint}", undo.title));
    let button = ui.button("bar-undo", &undo.title).small().on_press(move |s: &mut Phantom| run(s)).build();
    Some(match tip {
        Some(tip) => button.help(tip),
        None => button,
    })
}

/// The bar's notice, a button: the newest's line in its colour, its Undo, the pill (how many, in the colour
/// of the worst; none for one) and ›; and the popover, while open.
fn notices(s: &Phantom, newest: String, tone: Tone, ui: &Ui, room: f32, layer: &mut Vec<El>) -> El {
    let pill = s.bar.pill().map(|(count, worst)| (client::display::notices_count(count), tone_of(worst)));
    let line = ui
        .notice_button(newest, tone, bar_undo(s, ui), pill, client::display::notices_open_glyph())
        .on_press(crate::overlay::press_notices);
    let (head, body, width) = match s.bar.shown() {
        Some((entry, at, total)) => {
            let n = &entry.notice;
            let buttons: Vec<_> = n.actions.iter().map(|a| (a.title.clone(), a.hint.as_ref().and_then(|hint| hint(s)))).collect();
            let width = ui.notice_width(&n.short, n.more.as_ref(), &buttons);
            let (head, body) = popover_of(s, ui, entry, at, total);
            (head, body, width)
        }
        None => (None, el(), 0.0),
    };
    let open = crate::overlay::up(s) == Some(crate::overlay::Overlay::Notices);
    ui.notice_panel(
        "notices",
        line,
        width,
        head,
        body,
        room,
        open,
        |s: &mut Phantom| {
            s.bar.close();
        },
        step_keys,
        layer,
    )
}

/// The popover's head and body for the notice shown, `at` of `total` (newest first, from 0). The
/// head, only while there are several: ‹ 2 of 3 › and Dismiss all, then a hairline; × is on the
/// notice's own line, with one notice too. The body: the notice's mark, its line, what its list says, and
/// its buttons, right-aligned.
fn popover_of(s: &Phantom, ui: &Ui, entry: &Entry, at: usize, total: usize) -> (Option<El>, El) {
    let id = entry.id;
    let dismiss = || {
        ui.glyph(
            // The same key for every notice: the focus on × stays there as the next one shows.
            "notice-dismiss",
            ui.glyph_button(Icon::Close, client::display::dismiss()).on_press(move |s: &mut Phantom| s.bar.dismiss(id)),
        )
    };
    let head = (total > 1).then(|| {
        let step = |key: &'static str, glyph: Icon, label: &str, way: Way, there: bool| {
            ui.glyph(
                key,
                ui.glyph_button(glyph, label).disabled(!there).on_press(move |s: &mut Phantom| {
                    s.bar.step(way);
                }),
            )
        };
        let all = ui
            .button("notice-dismiss-all", client::display::dismiss_all())
            .small()
            .borderless()
            .on_press(|s: &mut Phantom| s.bar.dismiss_all());
        let pager = row()
            .key("notices-head")
            .w_full()
            .gap(2.0)
            .center_y()
            .child(step("notice-newer", Icon::Previous, client::display::newer(), Way::Newer, at > 0))
            .child(ui.figures(client::display::notice_place(at, total)).key("notice-place"))
            .child(step("notice-older", Icon::Next, client::display::older(), Way::Older, at + 1 < total))
            .child(el().grow(1.0));
        col()
            .key("notices-head")
            .w_full()
            .gap(4.0)
            .child(pager.key("notices-pager").child(all.muted().build()))
            .child(ui.rule("notices-head-rule"))
    });
    let notice = &entry.notice;
    let tone = tone_of(notice.severity);
    // × beside the notice's line, pinned outside the notice's own subtree: the focus on it stays
    // as the next notice shows.
    let title = ui.notice_title(notice.short.clone(), tone).key("notice-line").pr(CROSS_ROOM);
    let text = col().gap(4.0).grow(1.0).min_w(0.0).child(title).children(notice.more.as_ref().map(|listing| ui.notice_listing(listing)));
    let (mark, mark_tone) = mark_of(notice.severity);
    let card = row().gap(8.0).items_start().w_full().child(ui.notice_mark(mark, mark_tone).mt(2.0)).child(text);
    let (buttons, why_off) = buttons(s, ui, &notice.actions);
    let shown = col().key(format!("notice/{id}")).w_full().gap(10.0).child(card).children(buttons).children(why_off);
    let body = col().key("notice-shown").w_full().child(shown).child(dismiss().pin(None, Some(-2.0), Some(-2.0), None));
    (head, body)
}

/// How much of the line's row × takes, pinned at its end.
const CROSS_ROOM: f32 = 24.0;

/// A notice's buttons, right-aligned, small, the first the default, each with its shortcut inside
/// it; and, once a disabled one is pressed, why it's off.
fn buttons(s: &Phantom, ui: &Ui, actions: &[NoticeAction]) -> (Option<El>, Option<El>) {
    if actions.is_empty() {
        return (None, None);
    }
    let mut why_shown = None;
    let buttons: Vec<El> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let run = action.run.clone();
            let off = action.off.as_ref().and_then(|off| off(s));
            if off.is_some() && s.why_off_note == off {
                why_shown = off.clone();
            }
            let note = off.clone();
            let hint = action.hint.as_ref().and_then(|hint| hint(s));
            let button = ui.button("notice-action", &action.title).small().hint(hint);
            let button = if i == 0 { button.primary() } else { button };
            let button = button
                .disabled(off.is_some())
                .on_press(move |s: &mut Phantom| run(s))
                .on_press_disabled(move |s: &mut Phantom| s.why_off_note = note.clone())
                .build();
            row().key(format!("notice-action/{i}")).child(button)
        })
        .collect();
    let row = row().key("notice-actions").w_full().gap(8.0).justify_end().children(buttons);
    (Some(row), why_shown.map(|why| ui.caption(why).wrap().key("notice-action-why")))
}

/// The colour a notice's severity reads in: a receipt or an action's notice in the text's own,
/// never amber.
pub fn tone_of(severity: Severity) -> Tone {
    match severity {
        Severity::Info => Tone::Normal,
        Severity::Warning => Tone::Warning,
        Severity::Error => Tone::Error,
    }
}

/// A notice's mark and its colour: ✓ done, ⚠ a warning, ⛔ an error.
fn mark_of(severity: Severity) -> (Icon, Tone) {
    match severity {
        Severity::Info => (Icon::Done, Tone::Good),
        Severity::Warning => (Icon::Warning, Tone::Warning),
        Severity::Error => (Icon::Error, Tone::Error),
    }
}

/// ← and → with the focus inside the popover, step to a newer or
/// an older notice, and stop at the ends. With the focus elsewhere they're the page's.
fn step_keys(s: &mut Phantom, ev: &rune::KeyEvent, _: &mut rune::Tx) -> bool {
    let way = match ev.key {
        rune::Key::Left => Way::Newer,
        rune::Key::Right => Way::Older,
        _ => return false,
    };
    s.bar.step(way);
    true
}
