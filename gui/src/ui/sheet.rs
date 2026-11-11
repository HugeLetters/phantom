//! A sheet over the window, and what goes on one: tabs, a card of rows, a row with its label, a
//! switch, and an icon to pick.
//! `ui.sheet("settings", "Settings", body, close, &mut overlays)`.

use rune::{El, ScrollSpec, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, SegmentedControl, Switch};

use super::{Tone, Ui, keyed};

/// The widest a sheet gets, and the tallest, leaving the window's edge around it otherwise.
const SHEET_WIDTH: f32 = 600.0;
const SHEET_HEIGHT: f32 = 460.0;

impl Ui {
    /// `body` on a panel over the window, under `title` and a close button; the window behind is
    /// dimmed and inert. Escape, the close button and a click outside call `on_close`. The panel goes
    /// into `layer`, which the page hands to [`Ui::root`]; the body scrolls when the window is short.
    pub fn sheet<S: 'static>(
        &self,
        key: &'static str,
        title: &str,
        body: El,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) {
        let k = &self.k;
        let close = IconButton::new(k.glyph.close)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Regular)
            .label("Close")
            .build(k)
            .key("close")
            .on_press(on_close.clone());
        let header = row()
            .key("header")
            .w_full()
            .padding(16.0)
            .pb(8.0)
            .center_y()
            .child(text(title.to_owned()).font(k.ty.ui_medium.lh(16.0)).color(k.c.text_base).grow(1.0))
            .child(close);
        let body =
            col().key("sheet-body").w_full().grow(1.0).min_h(0.0).px(16.0).pb(16.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
        let mut panel = col()
            .key(format!("{key}-sheet"))
            .role("dialog")
            .label(title.to_owned())
            .w_full()
            .max_w(SHEET_WIDTH)
            .h_full()
            .max_h(SHEET_HEIGHT)
            .radius(k.radius.md)
            .fill(k.c.layer_01)
            .child(header)
            .child(body)
            .modal(true, on_close);
        for shadow in k.shadow_layers(&k.elevation.overlay) {
            panel = panel.box_shadow(shadow);
        }
        layer.push(el().key(format!("{key}-scrim")).absolute(0.0, 0.0).w_full().h_full().fill(k.c.scrim).a11y_hidden());
        layer.push(el().key(format!("{key}-layer")).absolute(0.0, 0.0).w_full().h_full().padding(16.0).center().child(panel));
    }

    /// Tabs across the top of a sheet: `labels`, `selected` showing; picking one calls `on_pick`
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
            let which = if tone == Tone::Error { "refused" } else { "note" };
            setting = setting.child(text(note).key(format!("{key}-{which}")).font(k.ty.text_small).color(ink).wrap());
        }
        setting
    }

    /// On or off; a press calls `on_toggle`, which flips it in the app's state. `first_focus`: first
    /// in the Tab order, so the sheet focuses it as it opens.
    pub fn switch<S: 'static>(
        &self,
        key: &'static str,
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
