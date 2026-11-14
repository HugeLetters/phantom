//! The status bar: the connection to the service, its notices ([`crate::bar`]), and the totals.
//! The bar shows the newest notice, with how many more there are; a click opens a popover above
//! it with every one, newest first, each with its explanation and actions, and × on those told
//! once. With one notice and nothing more to read, a click dismisses it (the bar's machine says
//! which, `Command::Press`). The popover opens over the other overlays ([`crate::overlay`]).

use rune::{El, col, el, row};

use super::speed;
use crate::bar::machine::{Entry, Source, Way};
use crate::model::Phantom;
use crate::notice::Severity;
use crate::service::Connection;
use crate::ui::{Tone, Ui};

/// `room`: how tall the popover may be, above the bar.
pub fn status_bar(s: &Phantom, ui: &Ui, room: f32, layer: &mut Vec<El>) -> El {
    let tone = match &s.connection {
        Connection::Connected { .. } => Tone::Good,
        Connection::Lost { .. } | Connection::Outdated => Tone::Error,
        Connection::Connecting | Connection::Replacing { .. } | Connection::Spawning { .. } => Tone::Muted,
    };
    let connection = client::display::connection(&s.connection);
    let down: u64 = s.torrents.iter().map(|t| t.download_bytes_per_sec).sum();
    let up: u64 = s.torrents.iter().map(|t| t.upload_bytes_per_sec).sum();
    let totals = match s.torrents.len() {
        0 => String::new(),
        1 => format!("1 torrent · ↓ {} · ↑ {}", speed(down), speed(up)),
        n => format!("{n} torrents · ↓ {} · ↑ {}", speed(down), speed(up)),
    };

    let mut bar = row().key("status").w_full().gap(8.0).center_y().child(ui.status_dot(tone)).child(ui.caption(connection));
    bar = match s.bar.newest() {
        Some(newest) => bar.child(notices(s, newest.short.clone(), ui, room, layer).grow(1.0).min_w(0.0)),
        None => bar.child(el().grow(1.0)),
    };
    bar.child(ui.figures(totals).key("totals"))
}

/// The bar's notice: the newest's short line, which of how many it is, and › when a click opens
/// something, in the tone of the most severe; and the popover, while it's open.
fn notices(s: &Phantom, newest: String, ui: &Ui, room: f32, layer: &mut Vec<El>) -> El {
    let entries = s.bar.entries();
    let count = client::display::notices_count(entries.len());
    let chevron = if s.bar.openable() { " ›" } else { "" };
    let severity = entries.iter().map(|e| e.notice.severity).max_by_key(|s| rank(*s)).unwrap_or(Severity::Info);
    let line = ui
        .notice(format!("{newest}{count}{chevron}"), tone_of(severity))
        .key("notice")
        // A control: Tab reaches it, and the popover gives focus back to it.
        .role("button")
        .focusable(true)
        .min_w(0.0)
        .help(client::display::notices_press(s.bar.openable()))
        .on_press(crate::overlay::press_notices);
    let (head, body) = match s.bar.shown() {
        Some((entry, at, total)) => popover_of(s, ui, entry, at, total),
        None => (el(), el()),
    };
    let open = crate::overlay::up(s) == Some(crate::overlay::Overlay::Notices);
    ui.panel_above(
        "notices",
        line,
        head,
        body,
        room,
        open,
        |s: &mut Phantom| {
            s.bar.close();
        },
        layer,
    )
}

/// The popover's head and body for the notice shown, `at` of `total` (newest first, from 0): the
/// head steps through them (‹ 1 of 3 ›, when there are several) with × on one told once; the body
/// is the notice's line and its explanation.
fn popover_of(s: &Phantom, ui: &Ui, entry: &Entry, at: usize, total: usize) -> (El, El) {
    let id = entry.id;
    let step = |key: &'static str, glyph: &str, label: &str, way: Way, there: bool| {
        ui.button(key, glyph)
            .accessible(label)
            .disabled(!there)
            .on_press(move |s: &mut Phantom| {
                s.bar.step(way);
            })
            .build()
    };
    let mut head = row().key("notices-head").w_full().gap(6.0).center_y();
    if total > 1 {
        head = head
            .child(step("notice-newer", client::display::newer_glyph(), client::display::newer(), Way::Newer, at > 0))
            .child(ui.caption(client::display::notice_place(at, total)).key("notice-place"))
            .child(step("notice-older", client::display::older_glyph(), client::display::older(), Way::Older, at + 1 < total));
    }
    head = head.child(el().grow(1.0));
    if entry.source == Source::Told {
        head = head.child(
            ui.button("notice-dismiss", client::display::dismiss_glyph())
                .accessible(client::display::dismiss())
                .on_press(move |s: &mut Phantom| s.bar.dismiss(id))
                .build()
                .key(format!("notice-dismiss/{id}")),
        );
    }
    let more = entry.notice.more.as_ref().map(|view| view(s, ui));
    let body = col()
        .key(format!("notice/{id}"))
        .w(400.0)
        .gap(6.0)
        .child(ui.notice(entry.notice.short.clone(), tone_of(entry.notice.severity)).wrap())
        .children(more);
    (head, body)
}

/// How severe, for the bar's tone: the worst of its notices.
fn rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

/// The colour a notice's severity reads in.
pub fn tone_of(severity: Severity) -> Tone {
    match severity {
        Severity::Info => Tone::Normal,
        Severity::Warning => Tone::Warning,
        Severity::Error => Tone::Error,
    }
}
