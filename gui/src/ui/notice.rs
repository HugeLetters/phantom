//! The notices' blocks (#87): the status bar's notice, a button of the newest's line, a pill with
//! how many there are and ›; a notice's mark; what its list says; and the popover it opens in,
//! attached to the bar by a tail.

use rune::{El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Placement, popover, scrim};

use std::ops::Add;

use super::{Icon, Tone, Ui};
use crate::notice::{Line, Listing};

/// Of the popover's padding (the look's `s5`), what's inside the scrolling body, so a focused
/// control's ring isn't cut off at its edges; how far its edges fade while there's more under them;
/// what its pager takes of its height.
const RING_ROOM: f32 = 4.0;
const FADE: f32 = 16.0;
const HEAD_ROOM: f32 = 40.0;
/// The popover's width: as wide as the widest notice in the list needs, within these (the
/// minimum fits the pager and Dismiss all).
const MIN_WIDTH: f32 = 240.0;
const MAX_WIDTH: f32 = 480.0;
/// Where the focus rests in the popover while on none of its controls.
pub const NOTICES_REST: &str = "notices-rest";
/// The tail under the popover, pointing at the bar's line: how far in from its left edge, and how
/// far it reaches below it.
const TAIL_IN: f32 = 18.0;
const TAIL_DEPTH: f32 = 6.0;

impl Ui {
    /// The status bar's notice, keyed `notice`, one button: the newest's `line` (`notice-text`) in
    /// its colour, which truncates; the `undo` after it, a small button of its own; the `pill` (`notice-count`), a count tinted by the worst
    /// severity; and › (`notice-open`). It fills on hover. A control: Tab reaches it, with the
    /// look's focus ring, and the popover gives focus back to it.
    pub fn notice_button(&self, line: impl Into<String>, tone: Tone, undo: Option<El>, pill: Option<(String, Tone)>, open: &str) -> El {
        let k = &self.k;
        let button = row()
            .key("notice")
            .gap(self.spacing.s3)
            .center_y()
            .h(self.spacing.status_line)
            .px(self.spacing.s3)
            .radius(k.radius.sm)
            .role("button")
            .focusable(true)
            .focus_ring(2.0, 2.0, k.c.border_focus)
            .min_w(0.0)
            .shrink()
            .child(text(line.into()).key("notice-text").font(k.ty.text_small).color(self.tone_color(tone)).truncate().min_w(0.0).shrink())
            .children(undo.map(|undo| row().child(undo).child(self.focus_goes_nowhere())))
            .children(pill.map(|(count, tone)| self.notice_pill(count, tone)))
            .child(text(open.to_owned()).key("notice-open").font(k.ty.text_small).color(k.c.text_muted));
        k.interactive(button)
    }

    /// The bar's count of notices: a small oval, tinted by `tone`.
    fn notice_pill(&self, count: String, tone: Tone) -> El {
        let k = &self.k;
        let ink = self.tone_color(tone);
        el().key("notice-count")
            .h(16.0)
            .min_w(26.0)
            .px(9.0)
            .radius(8.0)
            .center()
            .fill(self.colours.count_fill(tone))
            .child(text(count).key("notice-count-text").font(k.ty.text_xsmall_medium.features("'tnum' 1")).color(ink))
    }

    /// A notice's mark, as the theme draws its icon: ✓ done, ⚠ a warning, ⛔ an error, in `tone`.
    pub fn notice_mark(&self, mark: Icon, tone: Tone) -> El {
        icon(self.icon(mark), 16.0).key("notice-mark").color(self.tone_color(tone))
    }

    /// A notice's line in the list: medium weight, in its colour, wrapping.
    pub fn notice_title(&self, line: impl Into<String>, tone: Tone) -> El {
        text(line.into()).font(self.k.ty.text_small_medium).color(self.tone_color(tone)).wrap()
    }

    /// What a notice's list says under its line: each group (a name with its reason and paths) set
    /// apart from the next. A name is normal text, a reason muted; a path or a link has a line of
    /// its own, cut in the middle when it doesn't fit, whole on hover. A long name wraps, once the
    /// popover is as wide as it gets.
    pub fn notice_listing(&self, listing: &Listing) -> El {
        let k = &self.k;
        let line = |l: &Line| match l {
            Line::Name(t) => text(t.clone()).font(k.ty.text_small).color(k.c.text_base).wrap(),
            Line::Muted(t) => text(t.clone()).font(k.ty.text_small).color(k.c.text_muted).wrap(),
            Line::Path(t) => self.listing_path(t),
        };
        let groups = listing.groups.iter().map(|group| col().w_full().gap(self.spacing.s1).children(group.iter().map(line)));
        col().key("notice-listing").w_full().gap(if listing.is_apart() { 6.0 } else { 2.0 }).children(groups)
    }

    /// A path or a link in a list: one line, cut in the middle, whole on hover.
    fn listing_path(&self, path: &str) -> El {
        let k = &self.k;
        text(path.to_owned())
            .key("notice-path")
            .font(k.ty.text_small)
            .color(k.c.text_base)
            .truncate_middle()
            .w_full()
            .shrink()
            .min_w(0.0)
            .help_when_truncated(path.to_owned())
    }

    /// How wide the popover is for a list of notices: as the widest of them needs, within
    /// 240–480 points; the same on every page.
    pub fn popover_width(&self, needs: impl IntoIterator<Item = f32>) -> f32 {
        // At the widest nothing more is measured.
        let widest =
            needs.into_iter().try_fold(MIN_WIDTH, |widest, need| if need < MAX_WIDTH { Ok(need.max(widest)) } else { Err(MAX_WIDTH) });
        widest.unwrap_or_else(|widest| widest)
    }

    /// How wide one notice asks the popover to be: its widest line, unwrapped (a path at its real
    /// width), the mark, ×, and its `buttons` (each label with its hint) beside it, and the
    /// padding and border.
    pub fn notice_need(&self, line: &str, listing: Option<&Listing>, buttons: &[(String, Option<String>)]) -> f32 {
        use rune::text::advance;
        let (k, sp) = (&self.k, &self.spacing);
        // Mark and its gap; × and its gap; the panel's padding and border.
        let (mark, cross, frame) = (16.0 + sp.s4, 8.0 + 24.0, 2.0 * sp.s5 + 2.0);
        let title = mark + advance(line, k.ty.text_small_medium) + cross;
        // A path counts for its real width; the popover cuts it in the middle at its widest.
        let texts = listing.into_iter().flat_map(Listing::lines).map(|l| mark + advance(l.text(), k.ty.text_small));
        let row: f32 = buttons
            .iter()
            .map(|(label, hint)| {
                20.0 + advance(label, k.ty.small_medium) + hint.as_ref().map_or(0.0, |h| sp.s3 + advance(h, k.ty.text_xsmall)) + sp.s4
            })
            .sum();
        texts.chain([title, row]).fold(0.0, f32::max).add(frame)
    }

    /// A hairline across, `key`ed: under a popover's top row.
    pub fn rule(&self, key: &str) -> El {
        el().key(key.to_owned()).w_full().h(1.0).fill(self.k.c.border_weak).a11y_hidden()
    }

    /// A small borderless icon button: ×, ‹, ›. `label` is what VoiceOver reads.
    pub fn glyph_button(&self, glyph: Icon, label: &str) -> rune_kit::IconButton {
        rune_kit::IconButton::new(self.icon(glyph))
            .variant(rune_kit::IconButtonVariant::GhostMuted)
            .control_size(rune_kit::ControlSize::Small)
            .label(label)
    }

    /// Builds a [`glyph_button`](Self::glyph_button), keyed.
    pub fn glyph(&self, key: impl Into<String>, button: rune_kit::IconButton) -> El {
        super::keyed(key, self.glyph_el(button))
    }

    /// Builds a [`glyph_button`](Self::glyph_button) without the key's wrapper, for handlers
    /// (keys, hover) that must sit on the button itself, where the focus and the pointer are.
    pub fn glyph_el(&self, button: rune_kit::IconButton) -> El {
        super::ringed(button.build(&self.k), &self.k)
    }

    /// `trigger` (the bar's notice), with `content` above it while `open`: a raised panel attached
    /// to it by a tail, the theme's radius and a soft shadow, `width` wide (as
    /// [`popover_width`](Self::popover_width) gives it), at most `room` tall, `content` scrolling past it under a `head` that stays
    /// (`None`: no head, and no room kept for one). The panel goes into `layer`. It's modal: focus
    /// goes into it as it opens and stays there for Tab, and back to `trigger` once it closes.
    /// Escape and pressing outside it call `on_close`.
    #[allow(clippy::too_many_arguments)]
    pub fn notice_panel<S: 'static>(
        &self,
        key: &str,
        trigger: El,
        width: f32,
        head: Option<El>,
        content: El,
        room: f32,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        on_key: impl Fn(&mut S, &rune::KeyEvent, &mut rune::Tx) -> bool + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        let anchor = format!("{key}-trigger");
        let trigger = row().key(anchor.clone()).min_w(0.0).shrink().child(trigger).child(self.focus_goes_nowhere());
        if !open {
            return trigger;
        }
        let k = &self.k;
        let tallest = room.max(80.0);
        let head_room = if head.is_some() { HEAD_ROOM } else { 0.0 };
        // The body itself is bounded: a cap on the panel alone doesn't reach the scroll viewport.
        let body = col()
            .key(format!("{key}-scroll"))
            .shrink()
            .min_h(0.0)
            .max_h(tallest - 2.0 * self.spacing.s5 - head_room)
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_edge_fade(FADE)
            .scroll_margin(FADE)
            .padding(RING_ROOM)
            .child(content);
        let fill = self.raised_fill();
        let border = self.raised_edge();
        // Takes the focus of a button that left with its notice: in the panel, so its keys still work,
        // and pressing nothing.
        let rest = el()
            .key(NOTICES_REST)
            .absolute(0.0, 0.0)
            .size(1.0, 1.0)
            .focusable(true)
            .tab_index(-1)
            .focus_ring(0.0, 0.0, rune::Color::TRANSPARENT)
            .a11y_hidden();
        let panel = col()
            .key(format!("{key}-panel"))
            .role("dialog")
            .w(width)
            .max_h(tallest)
            .padding(self.spacing.s5 - RING_ROOM)
            .gap(self.spacing.s1)
            .children(head.map(|head| head.px(RING_ROOM)))
            .child(body)
            .child(self.tail(fill, border))
            .child(rest)
            // Keys while the focus is anywhere in it, on its head, its body or itself.
            .on_key(on_key);
        let panel = panel.modal(true, on_close.clone());
        let surface = k.elevate(panel.radius(k.radius.lg).fill(fill).border(1.0, border), &k.elevation.overlay);
        layer.push(scrim(format!("{key}-scrim"), on_close));
        layer.push(popover::anchored(k, surface, &anchor, Placement::TopStart, TAIL_DEPTH + 4.0));
        trigger
    }

    /// The edge of the notices popover's panel: `std_border` over the page, a surface raised over
    /// it that is to show. (A note's own is fainter: `k.c.border_weak`.)
    pub(super) fn raised_edge(&self) -> rune::Color {
        self.colours.raised_edge
    }

    /// A tooltip's edge: the panel's in Light, about half as strong in Dark, where the card is the
    /// page's own colour and a strong edge would outweigh the small text in it.
    pub(super) fn tooltip_edge(&self) -> rune::Color {
        self.colours.tooltip_edge
    }

    /// The fill of a surface raised over the page: the card's in Light; in Dark the window's own
    /// surface, set apart from the list by its border and shadow.
    pub(super) fn raised_fill(&self) -> rune::Color {
        self.colours.raised_fill
    }

    /// The tail under a panel: a turned square in its fill and border, clipped to its lower half,
    /// which covers the panel's own border where they meet.
    fn tail(&self, fill: rune::Color, border: rune::Color) -> El {
        let side = TAIL_DEPTH * std::f32::consts::SQRT_2;
        let square = el()
            .size(side, side)
            .fill(fill)
            .border(1.0, border)
            .rotate(std::f32::consts::FRAC_PI_4)
            .absolute(TAIL_DEPTH - side / 2.0, -side / 2.0);
        el().key("notice-tail")
            .size(2.0 * TAIL_DEPTH, TAIL_DEPTH + 1.0)
            .pin(Some(TAIL_IN), None, None, Some(-TAIL_DEPTH - 1.0))
            .clip_rounded(0.0)
            .child(square)
    }
}
