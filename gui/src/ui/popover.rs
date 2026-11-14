//! A small note anchored under an element: `ui.note("add-note", trigger, "Why", open, close, &mut layer)`.
//! It's as wide as its text, up to a maximum, where rune-kit's `Popover` is never narrower than a
//! dialog.

use rune::{El, ScrollSpec, col, text};
use rune_kit::{Placement, popover, scrim};

use super::{Tone, Ui};

/// The widest a note gets before its text wraps.
const MAX_WIDTH: f32 = 320.0;
/// A panel above the status bar: its padding, part of it inside the scrolling body so a focused
/// control's ring isn't cut off at its edges, and how far its edges fade while there's more under
/// them, as the error list's.
const PANEL_PADDING: f32 = 6.0;
const RING_ROOM: f32 = 4.0;
const FADE: f32 = 16.0;
/// What a panel's head takes of its height, with the gap under it.
const HEAD_ROOM: f32 = 40.0;

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

impl Ui {
    /// `trigger`, with `content` in a raised panel above it while `open`, for what opens from the
    /// status bar: at most `room` tall, `content` scrolling past it, its edges fading while
    /// there's more, under a `head` that stays. The panel goes into `layer`. It's modal while open: focus goes into it as it opens and stays
    /// there for Tab, and back where it was once it closes (`trigger`, when it's focusable and
    /// pressed), as a menu's does. Escape and pressing outside it call `on_close`.
    #[allow(clippy::too_many_arguments)]
    pub fn panel_above<S: 'static>(
        &self,
        key: &str,
        trigger: El,
        head: El,
        content: El,
        room: f32,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        let anchor = format!("{key}-trigger");
        let trigger = rune::row().key(anchor.clone()).child(trigger);
        if !open {
            return trigger;
        }
        let tallest = room.max(80.0);
        // The body itself is bounded: a cap on the panel alone doesn't reach the scroll viewport.
        let body = col()
            .key(format!("{key}-scroll"))
            .shrink()
            .min_h(0.0)
            .max_h(tallest - 2.0 * PANEL_PADDING - HEAD_ROOM)
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(FADE)
            .scroll_margin(FADE)
            .padding(RING_ROOM)
            .child(content);
        let panel = col()
            .key(format!("{key}-panel"))
            .role("dialog")
            .max_w(420.0)
            .max_h(tallest)
            .padding(PANEL_PADDING)
            .gap(4.0)
            .child(head)
            .child(body)
            .modal(true, on_close.clone());
        let surface = popover::raised(&self.k, panel, self.k.radius.md);
        layer.push(scrim(format!("{key}-scrim"), on_close));
        layer.push(popover::anchored(&self.k, surface, &anchor, Placement::TopStart, 4.0));
        trigger
    }
}
