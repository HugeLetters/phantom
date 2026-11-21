//! A small note anchored under an element: `ui.note("add-note", trigger, "Why", open, close, &mut layer)`.
//! It's as wide as its text, up to a maximum, where rune-kit's `Popover` is never narrower than a
//! dialog.

use rune::{El, col, text};
use rune_kit::{Placement, popover, scrim};

use super::{Tone, Ui};

/// The widest a note gets before its text wraps.
const MAX_WIDTH: f32 = 320.0;

impl Ui {
    /// `trigger`, with a note of `message` under it while `open`. The note goes into `layer`,
    /// which the page hands to [`Ui::root`]; pressing outside it calls `on_close`.
    pub fn note<S: 'static>(
        &self,
        key: &'static str,
        trigger: El,
        message: &str,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        self.note_in(Tone::Normal, key, trigger, message, open, on_close, layer)
    }

    /// [`note`](Self::note) in the error colour. Errors show next to the action that caused them.
    pub fn error_note<S: 'static>(
        &self,
        key: &'static str,
        trigger: El,
        message: &str,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        self.note_in(Tone::Error, key, trigger, message, open, on_close, layer)
    }

    #[allow(clippy::too_many_arguments)]
    fn note_in<S: 'static>(
        &self,
        tone: Tone,
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
        let k = &self.k;
        let ink = if tone == Tone::Error { k.c.danger.fg } else { self.tone_color(tone) };
        let message = text(message.to_owned()).font(k.ty.text_small).color(ink).max_lines(3);
        let mut surface = popover::raised(
            k,
            col().key(format!("{key}-content")).role("dialog").max_w(MAX_WIDTH).padding(8.0).child(message),
            k.radius.md,
        );
        if tone == Tone::Error {
            surface = surface.fill(k.c.danger.bg).border(1.0, k.c.danger.border);
        }
        layer.push(scrim(format!("{key}-scrim"), on_close));
        layer.push(popover::anchored(&self.k, surface, &anchor, Placement::Bottom, 4.0));
        trigger
    }
}
