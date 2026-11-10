//! The error list's panel: under the toolbar, in the middle, up to about half the window's
//! height and scrolling past it. Only the panel takes presses; the app stays usable around it.

use rune::{Effect, El, ScrollSpec, Spring, SpringPair, Transition, col, el, row, text};
use rune_kit::popover;

use super::Ui;

/// The widest the panel gets.
const MAX_WIDTH: f32 = 360.0;
/// The shortest it gets, when the window leaves less: the header and one entry.
const MIN_HEIGHT: f32 = 96.0;

/// One failure of a batch: what failed, and why.
pub struct Failure<'a> {
    pub what: &'a str,
    pub why: &'a str,
}

impl Ui {
    /// The panel and the room it sits in, as the overlay to hand to [`Ui::root`]. `top` is how far
    /// down the window it starts; `header` and `entries` are its contents.
    pub fn error_layer(&self, top: f32, title: &str, clear_all: El, entries: Vec<El>) -> El {
        let k = &self.k;
        let ink = k.c.danger.fg;
        let header = row()
            .key("error-header")
            .w_full()
            .center_y()
            .child(text(title.to_owned()).font(k.ty.ui_medium).color(ink).grow(1.0))
            .child(clear_all);
        let body = col().w_full().gap(8.0).children(entries);
        let panel = col()
            .key("error-list")
            .role("region")
            .w_full()
            .max_w(MAX_WIDTH)
            .padding(10.0)
            .gap(8.0)
            .shrink()
            .min_h(MIN_HEIGHT)
            .child(header)
            .child(body.scroll(ScrollSpec { stick_bottom: false }).shrink().min_h(0.0));
        let panel = popover::raised(k, panel, k.radius.md).fill(k.c.danger.bg).border(1.0, k.c.danger.border);
        // The top half of the window holds the panel, so it takes up to about half the height.
        let upper = col().w_full().grow(1.0).min_h(0.0).center_y().child(el().h(top)).child(panel.transition(appear_at_once()));
        col()
            .key("error-layer")
            .role("none")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .child(upper)
            .child(el().grow(1.0))
            .instant_changes()
            .transition(appear_at_once())
    }

    /// One entry: its short line, and its view under it, if it has one. Pressing it calls
    /// `on_dismiss`.
    pub fn error_entry<S: 'static>(&self, key: String, short: &str, more: Option<El>, on_dismiss: impl Fn(&mut S) + 'static) -> El {
        let k = &self.k;
        col()
            .key(key)
            .w_full()
            .gap(4.0)
            .help("Click to dismiss")
            .on_press(on_dismiss)
            .child(text(short.to_owned()).font(k.ty.text_small).color(k.c.danger.fg).wrap())
            .children(more)
    }

    /// The failures of a batch, each the file or link and why, when there's a reason to give.
    pub fn failure_list(&self, failures: &[Failure<'_>]) -> El {
        let k = &self.k;
        let rows = failures.iter().map(|failure| {
            let what = text(failure.what.to_owned()).font(k.ty.text_small).color(k.c.text_base).truncate_middle();
            let why = (!failure.why.is_empty()).then(|| text(failure.why.to_owned()).font(k.ty.text_small).color(k.c.text_muted).wrap());
            col().child(what).children(why)
        });
        col().gap(4.0).children(rows)
    }
}

/// There at once, like the other notes, and only fading when dismissed.
fn appear_at_once() -> Transition {
    Transition {
        insertion: Effect::IDENTITY,
        removal: Effect::opacity(0.0),
        springs: SpringPair::new(Spring::new(0.1, 0.0), Spring::new(0.25, 0.0)),
        ..Transition::identity()
    }
}
