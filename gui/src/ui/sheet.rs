//! A window's page of settings, and what goes on one: tabs, a card of rows, a row with its label, a
//! switch, and an icon to pick.
//! `ui.window_page("settings", "Settings", cx.titlebar_inset(), tabs, Some(body), close)`.

use rune::{El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{ControlSize, SegmentedControl, Select, Switch};

/// How tall the title bar's band is, the tabs in it level with the window's buttons, the body
/// under it.
const HEADER: f32 = 40.0;

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
    /// Drawn without its hairline edge: the Active look's, in the list's header.
    pub flat: bool,
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
    pub fn window_page<S: 'static>(
        &self,
        key: &str,
        title: &str,
        titlebar_inset: f32,
        top: El,
        body: Option<El>,
        on_close: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let page = col().w_full().h_full().fill(k.c.layer_01);
        let Some(body) = body else { return page };
        // Level with the traffic lights, which macOS centres 14 pt down a real title bar.
        let header = col().key("header").w_full().h(HEADER).pt(if titlebar_inset > 0.0 { 0.0 } else { 8.0 }).items_center().child(top);
        let body =
            col().key("sheet-body").w_full().grow(1.0).min_h(0.0).px(16.0).pb(16.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
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
            .radius(6.0)
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
            .radius(6.0)
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
                .px(12.0)
                .center()
                .radius(6.0)
                .grid_cell(i)
                .child(label)
                .on_press(move |s: &mut S| on_pick(s, i));
            if !on {
                segment = segment.hover(c.hover).press(pressed_over(c.hover, c.pressed));
            }
            group = group.child(if on && first_focus { segment.tab_index(1) } else { segment });
        }
        el().key(key).child(group)
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
        // A hairline over the look, so a chip the colour of the window doesn't vanish (none when
        // flat); in use, an outline 3 pt wide in the half's accent, 2 pt out from the chip.
        let edge = el()
            .key("edge")
            .absolute(0.0, 0.0)
            .size(CHIP, CHIP)
            .radius(k.radius.sm)
            .border(if chip.flat { 0.0 } else { 1.0 }, c.border_base)
            .a11y_hidden();
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
                chip_el = chip_el.hover(c.hover).press(pressed_over(c.hover, c.pressed)).on_press(move |s: &mut S| on_apply(s));
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
    /// last is as wide as a "Save as…" button, the widest it holds. `header`: the list's header,
    /// lightly tinted, its name in medium weight.
    #[allow(clippy::too_many_arguments)]
    pub fn preset_row(&self, key: &str, name: &str, header: bool, both: Option<El>, light: El, dark: El, edit: El, last: El) -> El {
        let k = &self.k;
        let font = if header { k.ty.ui_medium } else { k.ty.ui };
        let name = text(name.to_owned()).font(font).color(k.c.text_base).grow(1.0).min_w(96.0).wrap();
        let both = both.unwrap_or_else(|| el().key("no-both").size(CHIP, CHIP).a11y_hidden());
        // An unseen "Save as…" in the same cell sets its width; the real button fills it.
        let sizer = self.button("last-sizer", "Save as…").disabled(true).build().opacity(0.0).a11y_hidden().inert().grid_cell(0);
        let last = el().key("last").equal_columns(1).child(last.w_full().grid_cell(0)).child(sizer);
        let controls = row().gap(6.0).center_y().child(both).child(light).child(dark).child(edit).child(last);
        let tint = if header { k.c.layer_02 } else { rune::Color::TRANSPARENT };
        row()
            .key(key.to_owned())
            .fill(tint)
            .radius(k.radius.sm)
            .w_full()
            .min_h(36.0)
            .px(8.0)
            .py(4.0)
            .gap(12.0)
            .center_y()
            .flex_wrap()
            .justify_end()
            .child(name)
            .child(controls)
    }

    /// A sheet over the window (a Look Preset's editor, the App icon's tiles): `title` over
    /// `body`, and Done right under it; as tall as they are, up to the window less 20 pt above
    /// and below, past which the body scrolls. Modal, keyed `<key>-sheet`, over a scrim. Escape
    /// calls `on_escape`, Done `on_done`. The overlays to draw it with ([`Ui::root`]).
    pub fn over_sheet<S: 'static>(
        &self,
        key: &str,
        title: &str,
        body: El,
        on_escape: impl Fn(&mut S) + 'static,
        on_done: impl Fn(&mut S) + 'static,
    ) -> Vec<El> {
        let k = &self.k;
        let header =
            row().key("header").w_full().padding(16.0).child(text(title.to_owned()).font(k.ty.ui_medium.lh(16.0)).color(k.c.text_base));
        let body = col().key("body").w_full().shrink().min_h(0.0).px(16.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
        let done = row()
            .w_full()
            .padding(16.0)
            .justify_end()
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
            .px(16.0)
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
        let mut card = col().w_full().padding(4.0).radius(k.radius.md).fill(k.c.bg_base).border(1.0, k.c.border_weak);
        for (i, r) in rows.into_iter().enumerate() {
            if i > 0 {
                card = card.child(el().h(1.0).mx(8.0).fill(k.c.border_weak).a11y_hidden());
            }
            card = card.child(r);
        }
        card
    }

    /// A row of a Look Preset's editor: `label`, then its control for both halves across both
    /// columns, or one in each half's column (an empty one blank), then `toggle` (its ⟷, if any); under
    /// them, `notes` in their tones. The columns are the same width in every row, so they line up.
    pub fn halves_row(&self, key: &str, label: &str, cells: Halves, toggle: Option<El>, notes: Vec<(String, Tone)>) -> El {
        let k = &self.k;
        let cell = |content: Option<El>| row().grow(1.0).min_w(0.0).center_y().children(content);
        let cells = match cells {
            Halves::Joined(control) => row().grow(1.0).min_w(0.0).gap(HALF_GAP).child(cell(Some(control))),
            Halves::Split(light, dark) => row().grow(1.0).min_w(0.0).gap(HALF_GAP).child(cell(light)).child(cell(dark)),
        };
        let line = row()
            .w_full()
            .min_h(36.0)
            .gap(12.0)
            .center_y()
            .child(text(label.to_owned()).font(k.ty.ui).color(k.c.text_base).w(HALF_LABEL).wrap())
            .child(cells)
            .child(el().w(HALF_TOGGLE).center().children(toggle));
        let mut setting = col().key(key.to_owned()).w_full().px(8.0).py(4.0).gap(2.0).child(line);
        for (note, tone) in notes {
            let ink = if tone == Tone::Error { k.c.danger.fg } else { self.tone_color(tone) };
            setting = setting.child(text(note).key(format!("{key}-refused")).font(k.ty.text_small).color(ink).wrap());
        }
        setting
    }

    /// Over a Look Preset's editor's columns, while any row is split or each half has its own
    /// theme: ☀ Light and ☾ Dark; `rows`, over shared rows (past their labels, before their ⟷),
    /// else over two lists side by side.
    pub fn halves_header(&self, rows: bool) -> El {
        let k = &self.k;
        let head = |glyph: Icon, name: &str| {
            row()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .center_y()
                .child(icon(self.icon(glyph), 12.0).color(k.c.text_muted).a11y_hidden())
                .child(text(name.to_owned()).font(k.ty.text_small).color(k.c.text_muted))
        };
        let heads = row()
            .grow(1.0)
            .min_w(0.0)
            .gap(if rows { HALF_GAP } else { 8.0 + 16.0 })
            .child(head(Icon::Light, "Light"))
            .child(head(Icon::Dark, "Dark"));
        match rows {
            true => {
                row().key("halves-header").w_full().px(12.0).gap(12.0).child(el().w(HALF_LABEL)).child(heads).child(el().w(HALF_TOGGLE))
            }
            false => row().key("halves-header").w_full().px(12.0).child(heads),
        }
    }

    /// A row's ⟷, which joins or splits it.
    pub fn join_toggle<S: 'static>(&self, key: String, accessible: &str, on_press: impl Fn(&mut S) + 'static) -> El {
        self.button(key, "⟷").ghost().accessible(accessible).on_press(on_press).build()
    }

    /// One setting: `label` on the left, `control` on the right, and under them `notes`, each in
    /// its tone: what the value means, why a value was refused.
    pub fn setting_row(&self, key: &str, label: &str, control: El, notes: Vec<(String, Tone)>) -> El {
        let k = &self.k;
        let line = row()
            .w_full()
            .min_h(36.0)
            .gap(12.0)
            .center_y()
            .child(text(label.to_owned()).font(k.ty.ui).color(k.c.text_base).grow(1.0).min_w(0.0))
            .child(control);
        let mut setting = col().key(key.to_owned()).w_full().px(8.0).py(4.0).gap(2.0).child(line);
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
        let picture = el().size(48.0, 48.0).radius(48.0 * 0.22);
        let picture = match image {
            Some(image) => picture.pixels(image.pixels()),
            None => picture.fill(k.c.layer_04),
        };
        let remove = remove.map(|remove| el().absolute(TILE_W - 30.0, 2.0).group_opacity(0.0, 1.0).child(remove));
        col()
            .key(key)
            .role("group")
            .label(label.to_owned())
            .help(help.to_owned())
            .w(TILE_W)
            .h(TILE_H)
            .padding(6.0)
            .gap(4.0)
            .items_center()
            .radius(k.radius.md)
            .fill(k.alpha(k.c.text_base, 0.04))
            .hover_group()
            .child(picture)
            .child(text(label.to_owned()).font(k.ty.text_xsmall).color(k.c.text_muted).wrap().max_lines(2).a11y_hidden())
            .child(el().grow(1.0))
            .child(row().gap(4.0).children(chips))
            .children(remove)
    }

    /// A chip under an App icon's tile: ◐ both halves (`half` `None`), ☀ Light, ☾ Dark (`Some(true)`),
    /// drawn as the Look Presets' chips are, outlined while it's what shows (in use), when a press
    /// does nothing.
    pub fn icon_chip<S: 'static>(
        &self,
        key: String,
        half: Option<bool>,
        in_use: bool,
        help: String,
        on_press: impl Fn(&mut S) + 'static,
    ) -> El {
        let c = &self.k.c;
        let (glyph, pages, texts) = match half {
            None => (Icon::Both, vec![c.bg_base, c.text_base], vec![c.text_base, c.bg_base]),
            Some(dark) => (if dark { Icon::Dark } else { Icon::Light }, vec![c.bg_base], vec![c.text_base]),
        };
        let chip = LookChip { glyph, pages, texts, accent: c.border_focus, in_use, flat: false, help };
        self.look_chip(key, chip, false, Some(Box::new(on_press)))
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
            .padding(6.0)
            .gap(6.0)
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
        row().key(key.to_owned()).w_full().min_h(36.0).px(8.0).center_y().child(self.caption(text.to_owned()))
    }

    /// The App icon as it is now, small: one icon, or a pair, the light one first.
    pub fn icon_preview(&self, sides: &[Option<&rune::Image>]) -> El {
        let k = &self.k;
        let drawings = sides.iter().map(|image| {
            let tile = el().size(24.0, 24.0).radius(24.0 * 0.22);
            match image {
                Some(image) => tile.pixels(image.pixels()),
                None => tile.fill(k.c.layer_04),
            }
        });
        row().gap(4.0).center_y().a11y_hidden().children(drawings.collect::<Vec<_>>())
    }

    /// Tiles in rows, as many to a row as fit.
    pub fn tile_grid(&self, tiles: Vec<El>) -> El {
        row().w_full().flex_wrap().gap(4.0).children(tiles)
    }
}

/// What a Look Preset's editor row holds: one control for both halves, or one in each half's
/// column (`None`: that half's theme hasn't the setting).
pub enum Halves {
    Joined(El),
    Split(Option<El>, Option<El>),
}

/// How wide a Look Preset's editor row's label is, the gap between its columns, and its ⟷'s cell.
const HALF_LABEL: f32 = 80.0;
const HALF_GAP: f32 = 8.0;
const HALF_TOGGLE: f32 = 28.0;

/// How big an App icon's tile is, every one alike.
const TILE_W: f32 = 112.0;
const TILE_H: f32 = 132.0;

/// The kinds of image file the App icon's sheet takes as an icon.
pub const IMAGES: [&str; 6] = ["png", "jpg", "jpeg", "icns", "tiff", "heic"];

/// The press overlay that, over the hover one the runtime already shows, gives `pressed` over the
/// fill (as the kit's own controls).
fn pressed_over(hover: rune::Color, pressed: rune::Color) -> rune::Color {
    if hover.a >= 0.999 {
        return pressed;
    }
    pressed.alpha(((pressed.a - hover.a) / (1.0 - hover.a)).clamp(0.0, 1.0))
}
