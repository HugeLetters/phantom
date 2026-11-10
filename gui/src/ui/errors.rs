//! The error list's panel: under the toolbar, in the middle, up to about half the window's
//! height and scrolling past it. Only the panel takes presses; the app stays usable around it.

use rune::{Effect, El, ScrollSpec, Spring, SpringPair, Transition, col, el, row, text};
use rune_kit::popover;

use super::{Tone, Ui};

/// The widest the panel gets.
const MAX_WIDTH: f32 = 360.0;
/// The tallest it gets on a large window, so it never covers the whole list.
pub const MAX_HEIGHT: f32 = 480.0;
/// What it leaves below it: the status bar, the page's padding and a margin.
pub const BOTTOM_ROOM: f32 = 52.0;
/// The label of an entry that was dismissed and is fading out.
pub const LEAVING: &str = "Dismissed";
/// How long a leaving entry takes to fade.
const LEAVE_FADE: f32 = 0.15;
/// How long the panel takes to settle to a new size.
const SETTLE: f32 = 0.25;
/// The shortest it gets, when the window leaves less: the header and one entry.
const MIN_HEIGHT: f32 = 96.0;

/// How far the scrolling edges of the list fade while entries are hidden under them.
const FADE: f32 = 16.0;

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
        // A line between entries, so a batch's failures read as its own.
        let divider = || el().w_full().h(1.0).fill(k.alpha(k.c.danger.border, 0.6));
        let mut separated = Vec::new();
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                separated.push(divider());
            }
            separated.push(entry);
        }
        let body = col().w_full().gap(8.0).children(separated);
        let panel = col()
            .key("error-list")
            .role("region")
            .w_full()
            .max_w(MAX_WIDTH)
            .padding(10.0)
            .gap(8.0)
            .shrink()
            .min_h(MIN_HEIGHT)
            .max_h(MAX_HEIGHT)
            .motion(Spring::new(SETTLE, 0.0))
            .child(header)
            .child(
                body.key("error-scroll")
                    .scroll(ScrollSpec { stick_bottom: false })
                    .scroll_edge_fade(FADE)
                    .scroll_margin(FADE)
                    .shrink()
                    .min_h(0.0),
            );
        let panel = popover::raised(k, panel, k.radius.md).fill(k.c.danger.bg).border(1.0, k.c.danger.border);
        // Room above (the toolbar) and below (the status bar), and the panel between, as tall as
        // its entries until it meets either the bottom room or its cap.
        col()
            .key("error-layer")
            .role("none")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .center_y()
            .child(el().h(top))
            .child(panel.transition(appear_at_once()))
            .child(el().h(BOTTOM_ROOM))
            .transition(appear_at_once())
    }

    /// One entry: its short line, and its view under it, if it has one. Pressing it calls
    /// `on_dismiss`.
    pub fn error_entry<S: 'static>(
        &self,
        key: String,
        short: &str,
        tone: Tone,
        more: Option<El>,
        leaving: bool,
        on_dismiss: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let entry = col().w_full().gap(4.0);
        // A leaving entry fades where it is and takes no presses; the ones below it move up when
        // it's gone, so nothing overlaps on any frame.
        let entry = if leaving {
            entry.key(key).label(LEAVING).opacity(0.0).opacity_motion(Spring::new(LEAVE_FADE, 0.0))
        } else {
            entry.key(key).help("Click to dismiss").on_press(on_dismiss)
        };
        entry.child(text(short.to_owned()).font(k.ty.text_small).color(self.tone_color(tone)).wrap()).children(more)
    }

    /// The failures of a batch, each the file or link and why, when there's a reason to give.
    pub fn failure_list(&self, failures: &[Failure<'_>]) -> El {
        let k = &self.k;
        let rows = failures.iter().map(|failure| {
            let what = text(failure.what.to_owned()).font(k.ty.text_small).color(k.c.text_base).truncate_middle();
            let why = (!failure.why.is_empty()).then(|| text(failure.why.to_owned()).font(k.ty.text_small).color(k.c.text_muted).wrap());
            col().child(what).children(why)
        });
        // Indented under the entry's title.
        col().gap(6.0).pl(12.0).children(rows)
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
