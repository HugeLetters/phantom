//! A window's page of settings, and what goes on one: tabs, a card of rows, a row with its label, a
//! switch, and an icon to pick.
//! `ui.window_page("settings", "Settings", tabs, Some(body), close)`.

use rune::{El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{ControlSize, SegmentedControl, Select, Switch};

/// How tall the tabs are.
const TABS: f32 = 28.0;

/// The room above and below the tabs in the title bar's band, so they never touch the window's edge.
pub const TABS_PAD: f32 = 10.0;

/// How tall the title bar's band is: the tabs, centred, with [`TABS_PAD`] above and below. The
/// body is under it.
pub const HEADER: f32 = TABS + 2.0 * TABS_PAD;

/// The traffic lights' top-left, so they sit level with the tabs' middle: the band's middle less
/// half a light.
pub const TRAFFIC_LIGHTS: (f32, f32) = (7.0, (HEADER - rune::Chrome::TRAFFIC_LIGHT) / 2.0);

/// How much of the window a sheet over it leaves free, above and below: it fits its content up
/// to the window less this, then scrolls.
const SHEET_MARGIN: f32 = 20.0;

use super::{Icon, Tone, Ui, keyed};

/// How big a chip is, square.
const CHIP: f32 = 22.0;

/// How big a chip's glyph is.
const GLYPH: f32 = 14.0;

/// How wide ◐'s ring is.
const DISC: f32 = 12.0;

/// How thick ◐'s outline is: the line weight of ☀ and ☾ (2 of their 24 units, at `GLYPH`).
const STROKE: f32 = GLYPH / 12.0;

/// How wide an in-use chip's outline is, and how far out from the chip.
const IN_USE: f32 = 3.0;
const IN_USE_GAP: f32 = 2.0;

/// What a chip of a Look Preset's half, or of both, shows ([`Ui::look_chip`]): the look itself,
/// no theme name.
pub struct LookChip {
    /// ☀, ☾ or ◐.
    pub glyph: Icon,
    /// The page of each half it stands for: one, or the Light's then the Dark's, split diagonally.
    pub pages: Vec<rune::Color>,
    /// The text of each half it stands for, in the same order: its glyph's colour on that page.
    pub texts: Vec<rune::Color>,
    /// The accent that outlines it while it's in use.
    pub accent: rune::Color,
    /// It's the Active look's half (◐: both), value for value.
    pub in_use: bool,
    /// What it's named, for a screen reader and the hover: what a press does, or, in use, why it
    /// does nothing.
    pub help: String,
}

impl Ui {
    /// A window's whole page (Settings, #257): `top` (its tabs) in the titlebar band, level with
    /// the traffic lights, over `body`, which scrolls when the window is short; nothing while
    /// `body` is `None` (the window is closed). `title` names the page for a screen reader; it
    /// isn't drawn. The page is modal, keyed `<key>-sheet`: once its body is new or nothing in it
    /// has the focus, the first control in its Tab order takes it, and Escape calls `on_close`.
    pub fn window_page<S: 'static>(&self, key: &str, title: &str, top: El, body: Option<El>, on_close: impl Fn(&mut S) + 'static) -> El {
        let k = &self.k;
        let page = col().w_full().h_full().fill(k.c.layer_01);
        let Some(body) = body else { return page };
        // The tabs centred in the band, level with the traffic lights ([`TRAFFIC_LIGHTS`]).
        let header = col().key("header").w_full().h(HEADER).center().child(top);
        let body = col()
            .key("sheet-body")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .px(self.spacing.s7)
            .pb(self.spacing.s7)
            .scroll(ScrollSpec { stick_bottom: false })
            .child(body);
        page.key(format!("{key}-sheet")).role("dialog").label(title.to_owned()).child(header).child(body).modal(false, on_close)
    }

    /// Tabs across the top of a page of settings: `labels`, `selected` showing; picking one calls `on_pick`
    /// with its index. Each tab is keyed `<key>-<index>`.
    pub fn tabs<S: 'static>(
        &self,
        key: &'static str,
        labels: &[&str],
        selected: usize,
        on_pick: impl Fn(&mut S, usize) + Clone + 'static,
    ) -> El {
        let control = SegmentedControl::new(labels, Some(selected)).label("Settings").fit().on_pick(move |s: &mut S, i, _| on_pick(s, i));
        row().center().child(keyed(key, control.build(&self.k)))
    }

    /// One of a few values, side by side, `selected` showing; picking one calls `on_pick` with its
    /// index. Each segment is keyed `<key>/#0/segment<index>`. Tab reaches the selected segment, the
    /// left and right arrows pick the previous and the next (wrapping), taking effect as a click
    /// does, and it shows the look's focus ring. `first_focus`: the selected segment is first in
    /// the Tab order, so the page focuses it as it opens. It's a radio group named `label`, its
    /// segments radios. The kit's `SegmentedControl` is drawn the same, but takes no Tab order.
    pub fn choice<S: 'static>(
        &self,
        key: String,
        label: &str,
        labels: &[&str],
        selected: usize,
        first_focus: bool,
        on_pick: impl Fn(&mut S, usize) + Clone + 'static,
    ) -> El {
        let k = &self.k;
        let c = &k.c;
        let n = labels.len().max(1);
        let font = rune::TextStyle { line_height: 13.0, ..k.ty.ui };
        let indicator = el()
            .key("indicator")
            .h(28.0)
            .radius(k.radius.md)
            .fill(c.bg_base)
            .box_shadow(rune::Shadow { spread: 0.5, color: c.border_strong, ..Default::default() })
            .token_motion(k.motion.control)
            .a11y_hidden()
            .grid_cell(selected);
        let mut group = row()
            .role("radiogroup")
            .focus_group()
            .label(label.to_owned())
            .h(28.0)
            .center_y()
            .radius(k.radius.md)
            .fill(c.layer_01)
            .box_shadow(rune::Shadow { spread: 0.5, color: c.border_base, ..Default::default() })
            .equal_columns(n)
            .child(indicator);
        for (i, name) in labels.iter().enumerate() {
            let on = i == selected;
            let ink = if on { c.text_base } else { c.text_muted };
            let mut label = text((*name).to_owned()).key("label").font(font).color(ink).truncate().a11y_hidden();
            if !on {
                label = label.group_color(c.text_base);
            }
            let on_pick = on_pick.clone();
            let mut segment = row()
                .key(format!("segment{i}"))
                .role("radio")
                .focusable(true)
                .focus_selected(on)
                .focus_ring(2.0, 1.0, c.border_focus)
                .label((*name).to_owned())
                .position_in_set(i + 1, n)
                .grow(1.0)
                .shrink()
                .h(28.0)
                .px(self.spacing.s4)
                .center()
                .radius(k.radius.md)
                .grid_cell(i)
                .child(label)
                .on_press(move |s: &mut S| on_pick(s, i));
            if !on {
                segment = segment.hover(c.hover).press(self.colours.pressed_over_hover);
            }
            group = group.child(if on && first_focus { segment.tab_index(1) } else { segment });
        }
        // As narrow as the cell it's in (an editor's half), its labels truncated, never past it.
        el().key(key).min_w(0.0).shrink().child(group.min_w(0.0).shrink())
    }

    /// A pop-up button (`Phantom ⌄`) named `label`, showing `current` of `options`; `open`, its
    /// list under it, drawn into `layer` (the overlays over everything else), whose option a press
    /// chooses (`on_choose` with its index). A press on the button calls `on_toggle`, a press
    /// outside or Escape `on_close`. Keyed `key`: its button `<key>-trigger`, its options
    /// `<key>-content/…/option<index>`. `first_focus`: first in the Tab order.
    #[allow(clippy::too_many_arguments)]
    pub fn popup_button<S: 'static>(
        &self,
        key: String,
        label: &str,
        options: &[&str],
        current: usize,
        open: bool,
        first_focus: bool,
        on_toggle: impl Fn(&mut S) + 'static,
        on_close: impl Fn(&mut S) + 'static,
        on_choose: impl Fn(&mut S, usize) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        let names: Vec<String> = options.iter().map(|o| (*o).to_owned()).collect();
        let select = Select::new(key.clone(), options)
            .label(label)
            .current(options.get(current).copied())
            .open(open)
            .on_toggle(on_toggle)
            .on_close(on_close)
            .on_choose(move |s: &mut S, chosen: &str, _| {
                if let Some(i) = names.iter().position(|n| n == chosen) {
                    on_choose(s, i)
                }
            })
            .build(&self.k, layer);
        let select = if first_focus { select.tab_index(1) } else { select };
        el().key(key).child(select)
    }

    /// A chip of a Look Preset's half (or both): its glyph over its look, 22 pt square, keyed `key`.
    /// `on_apply`, if there's one, makes it a button, a press calling it; in use it's outlined in
    /// the half's accent and a press does nothing. Without `on_apply` it only shows. Either way its
    /// hover reads `help`. `first_focus`: first in the Tab order.
    pub fn look_chip<S: 'static>(&self, key: String, chip: LookChip, first_focus: bool, on_apply: Option<Box<dyn Fn(&mut S)>>) -> El {
        let k = &self.k;
        let c = &k.c;
        let glyph =
            |ink: rune::Color, x: f32| icon(self.icon(chip.glyph), GLYPH).color(ink).absolute(x, (CHIP - GLYPH) / 2.0).a11y_hidden();
        let centre = (CHIP - GLYPH) / 2.0;
        // ◐ is two looks split along the diagonal from bottom-left to top-right: the Light's top-left,
        // the Dark's bottom-right. Each half is a square turned 45° (so its clip is a half-plane)
        // holding its half of a ring centred on the chip, in its own text colour.
        let half = |page: rune::Color, ink: rune::Color, light: bool| {
            let (size, turn) = (2.0 * CHIP, std::f32::consts::FRAC_PI_4);
            // The square's centre is `size / 2` from the chip's centre, perpendicular to the seam.
            // The Light's reaches past the seam, under the Dark's: no hairline of the page behind shows on it.
            const UNDER: f32 = 1.0;
            let away = if light { UNDER - size / 2.0 } else { size / 2.0 } / std::f32::consts::SQRT_2;
            let (x, y) = (CHIP / 2.0 + away - size / 2.0, CHIP / 2.0 + away - size / 2.0);
            // The chip's centre in the square's own frame: the middle of the edge facing the seam.
            let on_seam = if light { size - UNDER - GLYPH / 2.0 } else { -GLYPH / 2.0 };
            let at = (size / 2.0 - DISC / 2.0, on_seam + GLYPH / 2.0 - DISC / 2.0);
            let ring = el().absolute(at.0, at.1).size(DISC, DISC).radius(DISC / 2.0).a11y_hidden();
            let mark = ring.key("ring").border(STROKE, ink);
            el().key(if light { "face-light" } else { "face-dark" })
                .absolute(x, y)
                .size(size, size)
                .fill(page)
                .clip_rounded(0.0)
                .rotate(-turn)
                .child(mark)
        };
        let face = match (chip.pages.as_slice(), chip.texts.as_slice()) {
            ([light, dark], [light_text, dark_text]) => el()
                .key("face")
                .absolute(0.0, 0.0)
                .size(CHIP, CHIP)
                .child(half(*light, *light_text, true))
                .child(half(*dark, *dark_text, false)),
            ([page], [text]) => el().key("face").absolute(0.0, 0.0).size(CHIP, CHIP).fill(*page).child(glyph(*text, centre)),
            _ => el().key("face"),
        };
        let face = face.radius(k.radius.sm).clip_rounded(k.radius.sm);
        // A hairline over the look, so a chip the colour of the window doesn't vanish; in use, an
        // outline 3 pt wide in the half's accent, 2 pt out from the chip.
        let edge = el().key("edge").absolute(0.0, 0.0).size(CHIP, CHIP).radius(k.radius.sm).border(1.0, c.border_base).a11y_hidden();
        let ring = chip.in_use.then(|| {
            el().key("in-use")
                .absolute(0.0, 0.0)
                .size(CHIP, CHIP)
                .radius(k.radius.sm)
                .outline(IN_USE, IN_USE_GAP, chip.accent)
                .a11y_hidden()
        });
        let mut chip_el =
            el().size(CHIP, CHIP).radius(k.radius.sm).child(face).child(edge).children(ring).label(chip.help.clone()).help(chip.help);
        if let Some(on_apply) = on_apply {
            chip_el = chip_el.role("button").focusable(true).focus_ring(2.0, 1.0, c.border_focus);
            if !chip.in_use {
                chip_el = chip_el.hover(c.hover).press(self.colours.pressed_over_hover).on_press(move |s: &mut S| on_apply(s));
            }
            chip_el = if first_focus { chip_el.tab_index(1) } else { chip_el };
        } else {
            chip_el = chip_el.role("img");
        }
        el().key(key).child(chip_el)
    }

    /// A row of the Look Presets: `name`, then columns: `both` (◐; empty for the Active look),
    /// `light`, `dark`, `edit` and `last`, which go under the name, right-aligned, when the window is
    /// too narrow for one line. The columns are the same width in every row, so they line up; the
    /// last is as wide as a "Save as…" button, the widest it holds.
    #[allow(clippy::too_many_arguments)]
    pub fn preset_row(&self, key: &str, name: &str, both: Option<El>, light: El, dark: El, edit: El, last: El) -> El {
        let k = &self.k;
        let name = text(name.to_owned()).font(k.ty.ui).color(k.c.text_base).grow(1.0).min_w(96.0).wrap();
        let both = both.unwrap_or_else(|| el().key("no-both").size(CHIP, CHIP).a11y_hidden());
        // An unseen "Save as…" in the same cell sets its width; the real button fills it.
        let sizer = self.button("last-sizer", "Save as…").disabled(true).build().opacity(0.0).a11y_hidden().inert().grid_cell(0);
        let last = el().key("last").equal_columns(1).child(last.w_full().grid_cell(0)).child(sizer);
        let controls = row().gap(self.spacing.s3).center_y().child(both).child(light).child(dark).child(edit).child(last);
        row()
            .key(key.to_owned())
            .w_full()
            .min_h(36.0)
            .px(self.spacing.s4)
            .py(self.spacing.s2)
            .gap(self.spacing.s6)
            .center_y()
            .flex_wrap()
            .justify_end()
            .child(name)
            .child(controls)
    }

    /// A sheet over the window (a Look Preset's editor, the App icon's tiles): `title` over
    /// `body`, and Done right under it, `on_cancel`'s Cancel (`<key>-cancel`) left of it if there's
    /// one; as tall as they are, up to the window less 20 pt above and below, past which the body
    /// scrolls. Modal, keyed `<key>-sheet`, over a scrim. Escape calls `on_escape`, Done
    /// `on_done`. The overlays to draw it with ([`Ui::root`]).
    pub fn over_sheet<S: 'static>(
        &self,
        key: &str,
        title: &str,
        body: El,
        on_escape: impl Fn(&mut S) + 'static,
        on_cancel: Option<Box<dyn Fn(&mut S)>>,
        on_done: impl Fn(&mut S) + 'static,
    ) -> Vec<El> {
        let k = &self.k;
        let header = row()
            .key("header")
            .w_full()
            .padding(self.spacing.s7)
            .child(text(title.to_owned()).font(k.ty.ui_medium.lh(16.0)).color(k.c.text_base));
        let body =
            col().key("body").w_full().shrink().min_h(0.0).px(self.spacing.s7).scroll(ScrollSpec { stick_bottom: false }).child(body);
        let cancel = on_cancel.map(|cancel| self.button(format!("{key}-cancel"), "Cancel").on_press(move |s: &mut S| cancel(s)).build());
        let done = row()
            .w_full()
            .padding(self.spacing.s7)
            .gap(self.spacing.s4)
            .justify_end()
            .children(cancel)
            .child(self.button(format!("{key}-done"), "Done").primary().on_press(on_done).build());
        let panel = col()
            .key(format!("{key}-sheet"))
            .role("dialog")
            .label(title.to_owned())
            .w_full()
            .shrink()
            .min_h(0.0)
            .max_w(600.0)
            .radius(k.radius.md)
            .fill(k.c.layer_01)
            .box_shadow(rune::Shadow { spread: 0.5, color: k.c.border_base, ..Default::default() })
            .child(header)
            .child(body)
            .child(done)
            .token_motion(k.motion.overlay)
            .modal(true, on_escape)
            .modal_click_outside(false);
        let scrim = el()
            .key(format!("{key}-scrim"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim)
            .token_motion(k.motion.overlay)
            .a11y_hidden();
        let layer = col()
            .key(format!("{key}-layer"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .px(self.spacing.s7)
            .py(SHEET_MARGIN)
            .justify_center()
            .items_center()
            .child(panel);
        vec![scrim, layer]
    }

    /// A small square of `color`, beside the field that sets it.
    pub fn swatch(&self, color: rune::Color) -> El {
        el().size(20.0, 20.0).radius(self.k.radius.sm).fill(color).border(1.0, self.k.c.border_weak).a11y_hidden()
    }

    /// A raised card of rows, with a hairline between each.
    pub fn card(&self, rows: impl IntoIterator<Item = El>) -> El {
        let k = &self.k;
        let mut card =
            col().w_full().padding(self.spacing.s2).radius(k.radius.md).fill(k.c.bg_base).border(1.0, self.colours.border_on_page);
        for (i, r) in rows.into_iter().enumerate() {
            if i > 0 {
                card = card.child(el().h(1.0).mx(8.0).fill(k.c.border_weak).a11y_hidden());
            }
            card = card.child(r);
        }
        card
    }

    /// A row of a Look Preset's editor, the sheet's body split down the middle: the left half is
    /// drawn entirely in the Light look and the right half entirely in the Dark one (`looks`, Light's
    /// then Dark's; backgrounds too). The left holds `label`, then the ☀ control, or, joined, the one
    /// control for both halves, left-aligned where a split row's ☀ control sits; the right holds the
    /// ☾ control (blank while joined), then `toggle` (its Join or Split, if any). Under the left
    /// half's line, `notes` in their tones. The columns are the same width in every row, so they
    /// line up. The row is keyed `key`, its halves `<key>-left` and `<key>-right`, its controls'
    /// cells `<key>-both-cell` while joined, else `<key>-light-cell` and `<key>-dark-cell`.
    pub fn halves_row(&self, key: &str, label: &str, cells: Halves, looks: [&Ui; 2], toggle: Option<El>, notes: Vec<(String, Tone)>) -> El {
        let [light, dark] = looks;
        let cell = |content: Option<El>, key: String| row().key(key).grow(1.0).min_w(0.0).min_h(44.0).center_y().children(content);
        let (left_cell, right_cell) = match cells {
            Halves::Joined(control) => (cell(Some(control), format!("{key}-both-cell")), cell(None, format!("{key}-dark-cell"))),
            Halves::Split(a, b) => (cell(a, format!("{key}-light-cell")), cell(b, format!("{key}-dark-cell"))),
        };
        let line = row()
            .w_full()
            .gap(self.spacing.s6)
            .center_y()
            .child(text(label.to_owned()).font(light.tokens().ty.ui).color(light.tokens().c.text_base).w(HALF_LABEL).wrap())
            .child(left_cell);
        let mut left = col().w_full().gap(self.spacing.s1).child(line);
        for (note, tone) in notes {
            let ink = if tone == Tone::Error { light.tokens().c.danger.fg } else { light.tone_color(tone) };
            left = left
                .child(text(note).key(format!("{key}-refused")).font(light.tokens().ty.text_small).color(ink).wrap().pb(self.spacing.s2));
        }
        let right = row().w_full().gap(self.spacing.s6).center_y().child(right_cell).child(el().w(HALF_TOGGLE).center().children(toggle));
        self.halves_line(key, left, right, [light.tokens().c.bg_base, dark.tokens().c.bg_base])
    }

    /// A line of the editor in two halves, `left` on `pages[0]` and `right` on `pages[1]`, each
    /// grown to the line's height so the seam runs through unbroken; keyed `key`, its halves
    /// `<key>-left` and `<key>-right`.
    fn halves_line(&self, key: &str, left: El, right: El, pages: [rune::Color; 2]) -> El {
        let half = |content: El, page: rune::Color, which: &str, cell: usize| {
            col()
                .key(format!("{key}-{which}"))
                .grow(1.0)
                .min_w(0.0)
                .px(self.spacing.s4)
                .fill(page)
                .justify_start()
                .grid_cell(cell)
                .child(content)
        };
        row().key(key.to_owned()).w_full().equal_columns(2).child(half(left, pages[0], "left", 0)).child(half(right, pages[1], "right", 1))
    }

    /// The editor's rows in one card, split down the middle ([`Ui::halves_row`]): between each, one
    /// hairline across both halves (keyed `halves-rule-<n>`, its line `…-line`), the two looks'
    /// strong borders mixed half and half: a neutral mid tone, it reads on a light page and a dark
    /// one. The two halves' pages run under it, so the seam is unbroken.
    pub fn halves_card(&self, lines: Vec<El>, looks: [&Ui; 2]) -> El {
        let k = &self.k;
        let mut card = col().key("halves-card").w_full().radius(k.radius.md).clip_rounded(k.radius.md).border(1.0, k.c.border_weak);
        let [light, dark] = looks.map(|l| l.tokens().c.clone());
        let ink = light.border_base.mix(dark.border_base, 0.5);
        let page = |c: rune::Color, cell: usize| el().grow(1.0).h(1.0).fill(c).grid_cell(cell).a11y_hidden();
        for (i, line) in lines.into_iter().enumerate() {
            if i > 0 {
                let line_over = el().key(format!("halves-rule-{i}-line")).w_full().h(1.0).fill(ink).a11y_hidden();
                card = card.child(
                    row()
                        .key(format!("halves-rule-{i}"))
                        .w_full()
                        .h(1.0)
                        .equal_columns(2)
                        .child(page(light.bg_base, 0))
                        .child(page(dark.bg_base, 1))
                        .child(row().absolute(0.0, 0.0).w_full().h(1.0).px(self.spacing.s4).child(line_over)),
                );
            }
            card = card.child(line);
        }
        card
    }

    /// The heading of a Look Preset's editor's half, drawn in that half's look (`self`): ☀ Light
    /// or ☾ Dark on its card colour, keyed `halves-header-light` or `-dark`.
    pub fn half_heading(&self, appearance_dark: bool) -> El {
        let k = &self.k;
        let (glyph, name, which) = if appearance_dark { (Icon::Dark, "Dark", "dark") } else { (Icon::Light, "Light", "light") };
        row()
            .key(format!("halves-header-{which}"))
            .w_full()
            .h(28.0)
            .px(self.spacing.s4)
            .gap(self.spacing.s2)
            .center_y()
            .fill(k.c.bg_base)
            .child(icon(self.icon(glyph), 12.0).color(k.c.text_muted).a11y_hidden())
            .child(text(name.to_owned()).font(k.ty.text_small).color(k.c.text_muted))
    }

    /// Over a Look Preset's editor's shared rows, always: the two headings
    /// ([`Ui::half_heading`]) over their columns, the Light's in the left half, past the labels,
    /// the Dark's in the right.
    pub fn halves_header(&self, light: El, dark: El, looks: [&Ui; 2]) -> El {
        let left = row().w_full().gap(self.spacing.s6).child(el().w(HALF_LABEL)).child(light.grow(1.0).min_w(0.0));
        let right = row().w_full().gap(self.spacing.s6).child(dark.grow(1.0).min_w(0.0)).child(el().w(HALF_TOGGLE));
        self.halves_line("halves-header", left, right, looks.map(|l| l.tokens().c.bg_base))
    }

    /// A row's Join or Split: a small button that joins or splits it.
    pub fn join_toggle<S: 'static>(&self, key: String, label: &str, accessible: &str, on_press: impl Fn(&mut S) + 'static) -> El {
        self.button(key, label).small().accessible(accessible).on_press(on_press).build()
    }

    /// One setting: `label` on the left, `control` on the right, and under them `notes`, each in
    /// its tone: what the value means, why a value was refused.
    pub fn setting_row(&self, key: &str, label: &str, control: El, notes: Vec<(String, Tone)>) -> El {
        let k = &self.k;
        let line = row()
            .w_full()
            .min_h(36.0)
            .gap(self.spacing.s6)
            .center_y()
            .child(text(label.to_owned()).font(k.ty.ui).color(k.c.text_base).grow(1.0).min_w(0.0))
            .child(control);
        let mut setting = col().key(key.to_owned()).w_full().px(self.spacing.s4).py(self.spacing.s2).gap(self.spacing.s1).child(line);
        for (note, tone) in notes {
            let ink = if tone == Tone::Error { k.c.danger.fg } else { self.tone_color(tone) };
            let which = match tone {
                Tone::Error => "refused",
                // What's wrong now with what's set (a folder that can't be reached), apart from the note on the value.
                Tone::Warning => "problem",
                Tone::Normal | Tone::Muted | Tone::Good => "note",
            };
            setting = setting.child(text(note).key(format!("{key}-{which}")).font(k.ty.text_small).color(ink).wrap());
        }
        setting
    }

    /// On or off; a press calls `on_toggle`, which flips it in the app's state. `first_focus`: first
    /// in the Tab order, so the page focuses it as it opens. Off, its track is the look's strong
    /// border colour, so it shows on a light card too.
    pub fn switch<S: 'static>(
        &self,
        key: impl Into<String>,
        label: &str,
        on: bool,
        first_focus: bool,
        on_toggle: impl Fn(&mut S) + 'static,
    ) -> El {
        let mut k = self.k.clone();
        k.c.layer_04 = k.c.border_strong;
        let switch = Switch::new(on).label(label).hide_label().control_size(ControlSize::Small).on_toggle(on_toggle).build(&k);
        keyed(key, if first_focus { switch.tab_index(1) } else { switch })
    }

    /// An App icon in the App icon's sheet: its drawing, `label` under it, its hover `help`, and
    /// under them `chips` (◐ ☀ ☾, [`Ui::icon_chip`]). An icon a person added has `remove`, an ×
    /// in its corner that shows while the pointer is on the tile. A side not drawn yet shows as a
    /// blank. Every tile is as wide and as tall as every other, so the grid lines up in columns.
    pub fn icon_tile(&self, key: String, image: Option<&rune::Image>, label: &str, help: &str, chips: Vec<El>, remove: Option<El>) -> El {
        let k = &self.k;
        let picture = el().size(PICTURE, PICTURE).radius(PICTURE * 0.22);
        let picture = match image {
            Some(image) => picture.pixels(image.pixels()),
            None => picture.fill(k.c.layer_04),
        };
        let remove = remove.map(|remove| el().absolute(TILE_W - 30.0, 0.0).group_opacity(0.0, 1.0).child(remove));
        col()
            .key(key)
            .role("group")
            .label(label.to_owned())
            .help(help.to_owned())
            .w(TILE_W)
            .h(TILE_H)
            .padding(TILE_PAD)
            .gap(TILE_GAP)
            .items_center()
            .radius(k.radius.md)
            .fill(self.colours.tile_fill)
            .hover_group()
            .child(picture)
            .child(self.tile_name(label))
            .child(row().gap(CHIP_GAP).mt(CHIPS_MARGIN).children(chips))
            .children(remove)
    }

    /// A chip under an App icon's tile: ◐ both halves (`half` `None`), ☀ Light, ☾ Dark (`Some(true)`),
    /// drawn as the Look Presets' chips are, ☀ on `faces`' Light page and ☾ on its Dark one (each
    /// `(page, text)`), ◐ split between them; outlined while it's in use, when a press does
    /// nothing.
    pub fn icon_chip<S: 'static>(
        &self,
        key: String,
        half: Option<bool>,
        faces: [(rune::Color, rune::Color); 2],
        in_use: bool,
        help: String,
        on_press: impl Fn(&mut S) + 'static,
    ) -> El {
        let [(light_page, light_text), (dark_page, dark_text)] = faces;
        let (glyph, pages, texts) = match half {
            None => (Icon::Both, vec![light_page, dark_page], vec![light_text, dark_text]),
            Some(true) => (Icon::Dark, vec![dark_page], vec![dark_text]),
            Some(false) => (Icon::Light, vec![light_page], vec![light_text]),
        };
        let chip = LookChip { glyph, pages, texts, accent: self.k.c.border_focus, in_use, help };
        self.look_chip(key, chip, false, Some(Box::new(on_press)))
    }

    /// A tile's name: centred over up to two lines, cut with "…" only past the second (the whole of
    /// it is the tile's hover). Its box is two lines tall for any name, so what's under it stays put.
    fn tile_name(&self, label: &str) -> El {
        let k = &self.k;
        text(label.to_owned())
            .key("name")
            .font(k.ty.text_xsmall.lh(NAME_LINE))
            .color(k.c.text_muted)
            .align(rune::Align::Center)
            .max_lines(2)
            .w(TILE_W - 2.0 * TILE_PAD)
            .h(NAME_H)
            .a11y_hidden()
    }

    /// A designed pair's tile in the App icon's sheet: its two icons split along the diagonal from
    /// bottom-left to top-right (the light one top-left, the dark one bottom-right, as ◐ is), its
    /// name under them (no chips, so it's shorter than an icon's tile), its hover `help`; a press calls `on_press`, outlined while it's in use
    /// (keyed `in-use`), when a press does nothing.
    pub fn pair_tile<S: 'static>(
        &self,
        key: String,
        light: Option<&rune::Image>,
        dark: Option<&rune::Image>,
        label: &str,
        help: &str,
        in_use: bool,
        on_press: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let picture = |image: Option<&rune::Image>, blank: rune::Color| {
            let tile = el().size(PICTURE, PICTURE);
            match image {
                Some(image) => tile.pixels(image.pixels()),
                None => tile.fill(blank),
            }
        };
        // The dark icon in a square turned 45°, whose clip is the half-plane past the diagonal: the
        // icon turned back, its centre on the middle of the square's edge along the seam.
        let size = 2.0 * PICTURE;
        let away = size / 2.0 / std::f32::consts::SQRT_2;
        let at = PICTURE / 2.0 + away - size / 2.0;
        let dark_half =
            el().key("dark").absolute(at, at).size(size, size).clip_rounded(0.0).rotate(-std::f32::consts::FRAC_PI_4).child(
                picture(dark, k.c.layer_03).absolute(size / 2.0 - PICTURE / 2.0, -PICTURE / 2.0).rotate(std::f32::consts::FRAC_PI_4),
            );
        let both = el()
            .size(PICTURE, PICTURE)
            .radius(PICTURE * 0.22)
            .clip_rounded(PICTURE * 0.22)
            .child(picture(light, k.c.layer_04).key("light").absolute(0.0, 0.0))
            .child(dark_half)
            .a11y_hidden();
        let ring = in_use.then(|| {
            el().key("in-use")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(k.radius.md)
                .outline(IN_USE, IN_USE_GAP, k.c.border_focus)
                .a11y_hidden()
        });
        let mut tile = col()
            .key(key)
            .role("button")
            .label(label.to_owned())
            .help(help.to_owned())
            .w(TILE_W)
            .h(PAIR_H)
            .padding(TILE_PAD)
            .gap(TILE_GAP)
            .items_center()
            .radius(k.radius.md)
            .fill(self.colours.tile_fill)
            .focusable(true)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .child(both)
            .child(self.tile_name(label))
            .children(ring);
        if !in_use {
            tile = tile.hover(k.c.hover).press(self.colours.pressed_over_hover).on_press(on_press);
        }
        tile
    }

    /// The App icon sheet's last tile: "+ Add image…", which `on_press` follows with a file dialog.
    pub fn add_tile<S: 'static>(&self, key: &str, on_press: impl Fn(&mut S, &mut rune::Tx) + 'static) -> El {
        let k = &self.k;
        col()
            .key(key.to_owned())
            .role("button")
            .label("Add image…".to_owned())
            .w(TILE_W)
            .h(TILE_H)
            .padding(TILE_PAD)
            .gap(TILE_GAP)
            .center()
            .radius(k.radius.md)
            .border(1.0, k.c.border_strong)
            .hover(k.c.hover)
            .focusable(true)
            .focus_ring(2.0, 2.0, k.c.border_focus)
            .on_press_tx(on_press)
            .child(text("+".to_owned()).font(k.ty.ui_medium.lh(24.0)).color(k.c.text_muted).a11y_hidden())
            .child(text("Add image…".to_owned()).font(k.ty.text_xsmall).color(k.c.text_muted).a11y_hidden())
    }

    /// A card's row holding only `text`, muted: what a list without entries says in their place.
    pub fn muted_row(&self, key: &str, text: &str) -> El {
        row().key(key.to_owned()).w_full().min_h(36.0).px(self.spacing.s4).center_y().child(self.caption(text.to_owned()))
    }

    /// The App icon as it is now, small: one icon (`None` its half), or the Light one's and the
    /// Dark one's (`Some(false)`, `Some(true)`), each with a small ☀ or ☾ under it (keyed `mark`,
    /// its glyph `glyph`), drawn as the chips are: ☀ on `faces`' Light page and ☾ on its Dark one
    /// (each `(page, text)`). Each keyed `app-icon-preview-both`, `-light`, `-dark`.
    pub fn icon_preview(&self, sides: &[(Option<&rune::Image>, Option<bool>)], faces: [(rune::Color, rune::Color); 2]) -> El {
        let k = &self.k;
        let drawings = sides.iter().map(|(image, half)| {
            let tile = el().key("icon").absolute(0.0, 0.0).size(PREVIEW, PREVIEW).radius(PREVIEW * 0.22);
            let tile = match image {
                Some(image) => tile.pixels(image.pixels()),
                None => tile.fill(k.c.layer_04),
            };
            let which = match half {
                None => "both",
                Some(true) => "dark",
                Some(false) => "light",
            };
            // The mark overlaps the icon's bottom-right corner, so the preview is as tall as the icon.
            let mark = half.map(|dark| {
                let (page, ink) = faces[usize::from(dark)];
                let glyph = icon(self.icon(if dark { Icon::Dark } else { Icon::Light }), 9.0).key("glyph").color(ink);
                el().key("mark")
                    .absolute(PREVIEW - 8.0, PREVIEW - 8.0)
                    .size(14.0, 14.0)
                    .radius(k.radius.sm)
                    .fill(page)
                    .border(1.0, k.c.border_base)
                    .center()
                    .child(glyph)
            });
            let width = if half.is_some() { PREVIEW + 6.0 } else { PREVIEW };
            el().key(format!("app-icon-preview-{which}")).size(width, PREVIEW).child(tile).children(mark)
        });
        row().gap(self.spacing.s2).center_y().a11y_hidden().children(drawings.collect::<Vec<_>>())
    }

    /// Tiles in rows, as many to a row as fit.
    pub fn tile_grid(&self, tiles: Vec<El>) -> El {
        row().w_full().flex_wrap().gap(self.spacing.s2).children(tiles)
    }
}

/// What a Look Preset's editor row holds: one control for both halves, built in each half's
/// look, or one in each half's column (`None`: that half's theme hasn't the setting).
pub enum Halves {
    Joined(El),
    Split(Option<El>, Option<El>),
}

/// How big an App icon row's preview is.
const PREVIEW: f32 = 24.0;

/// How wide a Look Preset's editor row's label is, and its Join or Split's cell.
const HALF_LABEL: f32 = 64.0;
const HALF_TOGGLE: f32 = 46.0;

/// How big an App icon's tiles are: every one as wide, so the grid lines up in columns; an icon's
/// (and "+ Add image…") as tall as its drawing, name and chips need; a designed pair's, with no
/// chips, as tall as its drawing and name (two lines) need. The padding, and the gap between the parts.
const TILE_W: f32 = 96.0;
const PICTURE: f32 = 40.0;
const TILE_PAD: f32 = 4.0;
const TILE_GAP: f32 = 3.0;
/// A name's line height; its box holds two lines.
pub(crate) const NAME_LINE: f32 = 13.0;
const NAME_H: f32 = 2.0 * NAME_LINE;
/// Extra room over the chips, so an in-use outline (3 pt wide, 2 pt out) clears the name above.
const CHIPS_MARGIN: f32 = IN_USE + IN_USE_GAP - TILE_GAP;
const PAIR_H: f32 = 2.0 * TILE_PAD + PICTURE + TILE_GAP + NAME_H;
const TILE_H: f32 = PAIR_H + TILE_GAP + CHIPS_MARGIN + CHIP;

/// The gap between an App icon's chips: their in-use outlines, 3 pt wide and 2 pt out, never meet;
/// three chips and two gaps fill a tile's width less its padding.
const CHIP_GAP: f32 = (TILE_W - 2.0 * TILE_PAD - 3.0 * CHIP) / 2.0;

/// The kinds of image file the App icon's sheet takes as an icon.
pub const IMAGES: [&str; 6] = ["png", "jpg", "jpeg", "icns", "tiff", "heic"];
