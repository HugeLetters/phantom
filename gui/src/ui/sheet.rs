//! A window's page of settings, and what goes on one: tabs, a card of rows, a row with its label, a
//! switch, and an icon to pick.
//! `ui.window_page("settings", "Settings", cx.titlebar_inset(), Some(body), close)`.

use rune::{El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{ControlSize, SegmentedControl, Switch};

/// How tall the title's band is, the body under it.
const HEADER: f32 = 48.0;

use super::{Icon, Tone, Ui, keyed};

/// How big a chip is, square.
const CHIP: f32 = 22.0;

/// How big a chip's glyph is.
const GLYPH: f32 = 14.0;

/// What a chip of a Look Preset's half, or of both, shows ([`Ui::look_chip`]): the look itself,
/// no theme name.
pub struct LookChip {
    /// ☀, ☾ or ◐.
    pub glyph: Icon,
    /// The page of each half it stands for: one, or the Light's then the Dark's, side by side.
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
    /// A window's whole page (Settings, #257): `title` in the titlebar band, beside the traffic
    /// lights, over `body`, which scrolls when the window is short; nothing while `body` is `None`
    /// (the window is closed). The page is modal, keyed `<key>-sheet`: once its body is new or
    /// nothing in it has the focus, the first control in its Tab order takes it, and Escape calls
    /// `on_close`.
    pub fn window_page<S: 'static>(
        &self,
        key: &str,
        title: &str,
        titlebar_inset: f32,
        body: Option<El>,
        on_close: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let page = col().w_full().h_full().fill(k.c.layer_01);
        let Some(body) = body else { return page };
        // The title level with the traffic lights.
        let header = col()
            .key("header")
            .w_full()
            .h(HEADER)
            .pt(if titlebar_inset > 0.0 { 5.0 } else { 12.0 })
            .items_center()
            .child(text(title.to_owned()).font(k.ty.ui_medium.lh(16.0)).color(k.c.text_base));
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
        row().w_full().center().child(keyed(key, control.build(&self.k)))
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
        // ◐ is two looks side by side, split where its glyph is: the Light's on the left, the Dark's
        // on the right, each half of the glyph in its own text colour.
        let face = match (chip.pages.as_slice(), chip.texts.as_slice()) {
            ([light, dark], [light_text, dark_text]) => row()
                .key("face")
                .absolute(0.0, 0.0)
                .size(CHIP, CHIP)
                .child(el().key("face-light").size(CHIP / 2.0, CHIP).fill(*light).clip_rounded(0.0).child(glyph(*light_text, centre)))
                .child(
                    el().key("face-dark")
                        .size(CHIP / 2.0, CHIP)
                        .fill(*dark)
                        .clip_rounded(0.0)
                        .child(glyph(*dark_text, centre - CHIP / 2.0)),
                ),
            ([page], [text]) => el().key("face").absolute(0.0, 0.0).size(CHIP, CHIP).fill(*page).child(glyph(*text, centre)),
            _ => el().key("face"),
        };
        // The outline over the look: 2 pt in the half's accent while it's in use, else a hairline, so
        // a chip the colour of the window doesn't vanish.
        let (width, edge) = if chip.in_use { (2.0, chip.accent) } else { (1.0, c.border_base) };
        let ring = el()
            .key(if chip.in_use { "in-use" } else { "edge" })
            .absolute(0.0, 0.0)
            .size(CHIP, CHIP)
            .radius(k.radius.sm)
            .border(width, edge)
            .a11y_hidden();
        let mut chip_el = el()
            .size(CHIP, CHIP)
            .radius(k.radius.sm)
            .clip_rounded(k.radius.sm)
            .child(face)
            .child(ring)
            .label(chip.help.clone())
            .help(chip.help);
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

    /// A row of the Look Presets: `name`, then `chips` and `buttons`, which go under the name when
    /// the window is too narrow for one line.
    pub fn preset_row(&self, key: &str, name: &str, chips: Vec<El>, buttons: Vec<El>) -> El {
        let k = &self.k;
        let name = text(name.to_owned()).font(k.ty.ui).color(k.c.text_base).grow(1.0).min_w(96.0).wrap();
        let controls = row().gap(6.0).center_y().children(chips).child(row().gap(2.0).center_y().children(buttons));
        row().key(key.to_owned()).w_full().min_h(36.0).px(8.0).py(4.0).gap(12.0).center_y().flex_wrap().child(name).child(controls)
    }

    /// A sheet over the window (a saved Look Preset's editor): `title` over `body`, which scrolls,
    /// and Done; modal, keyed `<key>-sheet`, over a scrim. Escape calls `on_escape`, Done `on_done`.
    /// The overlays to draw it with ([`Ui::root`]).
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
        let body = col().key("body").w_full().grow(1.0).shrink().min_h(0.0).px(16.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
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
            .h_full()
            .max_w(520.0)
            .max_h(420.0)
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
        let layer = el().key(format!("{key}-layer")).absolute(0.0, 0.0).w_full().h_full().padding(16.0).pt(HEADER).center().child(panel);
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
    /// in the Tab order, so the page focuses it as it opens.
    pub fn switch<S: 'static>(
        &self,
        key: impl Into<String>,
        label: &str,
        on: bool,
        first_focus: bool,
        on_toggle: impl Fn(&mut S) + 'static,
    ) -> El {
        let switch = Switch::new(on).label(label).hide_label().control_size(ControlSize::Small).on_toggle(on_toggle).build(&self.k);
        keyed(key, if first_focus { switch.tab_index(1) } else { switch })
    }

    /// An App icon to pick: its drawing (two side by side for a pair, dark first), `label` under it,
    /// ringed while `selected`; the selected one is first in the Tab order while `first_focus`. A
    /// side not drawn yet shows as a blank tile.
    pub fn icon_tile<S: 'static>(
        &self,
        key: String,
        sides: &[Option<&rune::Image>],
        label: &str,
        selected: bool,
        first_focus: bool,
        on_press: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let size = if sides.len() > 1 { 36.0 } else { 48.0 };
        let drawings = sides.iter().map(|image| {
            let tile = el().size(size, size).radius(size * 0.22);
            match image {
                Some(image) => tile.pixels(image.pixels()),
                None => tile.fill(k.c.layer_04),
            }
        });
        let ring = if selected { k.c.border_focus } else { rune::Color::TRANSPARENT };
        let tile = col()
            .key(key)
            .role("radio")
            .checked(selected)
            .label(label.to_owned())
            .w(104.0)
            .padding(6.0)
            .gap(4.0)
            .items_center()
            .radius(k.radius.md)
            .border(2.0, ring)
            .hover(k.c.hover)
            .focusable(true)
            .focus_ring(2.0, 2.0, k.c.border_focus)
            .on_press(on_press)
            .child(row().h(48.0).gap(2.0).center_y().children(drawings.collect::<Vec<_>>()))
            .child(text(label.to_owned()).font(k.ty.text_xsmall).color(k.c.text_muted).wrap().max_lines(2).a11y_hidden());
        if selected && first_focus { tile.tab_index(1) } else { tile }
    }

    /// Tiles in rows, as many to a row as fit.
    pub fn tile_grid(&self, tiles: Vec<El>) -> El {
        row().w_full().flex_wrap().gap(4.0).children(tiles)
    }
}

/// The press overlay that, over the hover one the runtime already shows, gives `pressed` over the
/// fill (as the kit's own controls).
fn pressed_over(hover: rune::Color, pressed: rune::Color) -> rune::Color {
    if hover.a >= 0.999 {
        return pressed;
    }
    pressed.alpha(((pressed.a - hover.a) / (1.0 - hover.a)).clamp(0.0, 1.0))
}
