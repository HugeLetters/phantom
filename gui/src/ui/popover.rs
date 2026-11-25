//! A small note anchored under an element: `ui.note("add-note", trigger, "Why", open, close, &mut layer)`.
//! It's as wide as its text, up to a maximum, where rune-kit's `Popover` is never narrower than a
//! dialog.

use rune::{El, col, text};
use rune_kit::{Placement, popover, scrim};

use super::Ui;

/// How long the pointer rests on an [`explained`](Ui::explained) control before its tooltip shows.
const EXPLAIN_AFTER: std::time::Duration = std::time::Duration::from_millis(500);
/// The widest a note gets before its text wraps.
const MAX_WIDTH: f32 = 320.0;
/// A note's space either side of its text.
const PAD_X: f32 = 10.0;

impl Ui {
    /// How Rune draws every `.help` tooltip: the look of this file's notes (the raised surface, the
    /// same edge, corners and type), so one window never shows two. Told to Rune once a view
    /// (`cx.help_look(ui.help_look())`); on macOS it replaces AppKit's own tooltip.
    pub fn help_look(&self) -> rune::HelpLook {
        let k = &self.k;
        rune::HelpLook::system(false)
            .fill(self.raised_fill())
            .ink(k.c.text_base)
            .ring(1.0, self.raised_edge())
            .radius(k.radius.md)
            .padding(PAD_X, 6.0)
            .font(k.ty.text_small)
            .max_width(Some(MAX_WIDTH - 2.0 * PAD_X))
            .gap(4.0)
            .delay(EXPLAIN_AFTER.as_secs_f64())
            .shadows(k.elevation.md.iter().copied())
    }

    /// `trigger`, with a note of `message` under it while `open`. The note goes into `layer`,
    /// which the page hands to [`Ui::root`]; pressing outside it calls `on_close`. What's known
    /// before anything is sent (why a button is off, why the text can't be added) says so here,
    /// on the control clicked: a neutral tooltip, the same in Light and Dark.
    pub fn note<S: 'static>(
        &self,
        key: &'static str,
        trigger: El,
        message: &str,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        let anchor = format!("{key}-trigger");
        let trigger = rune::row().key(anchor.clone()).child(trigger);
        if !open {
            return trigger;
        }
        layer.push(scrim(format!("{key}-scrim"), on_close));
        layer.push(popover::anchored(&self.k, self.tooltip(key, message), &anchor, Placement::Bottom, 4.0));
        trigger
    }

    /// A [`note`](Self::note)'s tooltip under the element keyed `anchor`, drawn earlier in
    /// `layer`, that takes no presses: it goes when its owner stops drawing it (a field's on the
    /// next keystroke).
    pub fn tooltip_under(&self, key: &str, anchor: &str, message: &str, layer: &mut Vec<El>) {
        let tip = self.tooltip(key, message).inert();
        layer.push(popover::anchored(&self.k, tip, anchor, Placement::BottomStart, 4.0));
    }

    /// `trigger`, which says what it does in a tooltip under it ([`tooltip_below`](Self::tooltip_below))
    /// once the pointer rests on it: `on_rest` hears it come to rest (`true`, after the same wait
    /// as macOS's) and leave (`false`). Screen readers hear `message` as its description.
    pub fn explained<S: 'static>(&self, trigger: El, message: &str, on_rest: impl Fn(&mut S, bool) + 'static) -> El {
        trigger
            .description(message.to_owned())
            .on_hover_change(move |s: &mut S, resting, _: &mut rune::Tx| on_rest(s, resting))
            .hover_delays(EXPLAIN_AFTER, std::time::Duration::ZERO)
    }

    /// A tooltip of `message` centred under the element keyed `anchor`, drawn earlier in `layer`,
    /// that takes no presses: an [`explained`](Self::explained) control's, while the pointer rests
    /// on it.
    pub fn tooltip_below(&self, key: &str, anchor: &str, message: &str, layer: &mut Vec<El>) {
        let tip = self.tooltip(key, message).inert();
        layer.push(popover::anchored(&self.k, tip, anchor, Placement::Bottom, 4.0));
    }

    /// The tooltip's surface: raised, in the text's own colour, as wide as its text up to
    /// [`MAX_WIDTH`]. No tint and no error colour: it says what's so, it isn't an alarm.
    fn tooltip(&self, key: &str, message: &str) -> El {
        let k = &self.k;
        let message = text(message.to_owned()).font(k.ty.text_small).color(k.c.text_base).max_lines(3);
        let body = col().key(format!("{key}-content")).role("tooltip").max_w(MAX_WIDTH).px(PAD_X).py(6.0).child(message);
        popover::raised(k, body, k.radius.md).border(1.0, super::opaque_over(k.c.border_weak, k.c.page))
    }
}
