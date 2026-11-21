//! A menu of choices: `ui.menu(...)` draws the kit's menu in the current look, for a context menu
//! at a point or a dropdown under its trigger. Screens say what the choices are and what a pick
//! or a dismissal does; the look, the keyboard focus and Escape and outside presses are the kit's.

use rune::{El, Tx, el};
use rune_kit::{MenuItem, Placement};

use super::{Icon, Ui};

/// One entry of a menu.
pub enum Entry {
    /// `id` names it to the pick's handler.
    Choice {
        id: &'static str,
        label: String,
        /// Its shortcut, shown right-aligned and dimmed, as in macOS menus.
        hint: Option<String>,
        icon: Option<Icon>,
        /// A choice among values, as the priorities: `Some(true)` for the current one. Screen readers
        /// hear it (`menuitemcheckbox`, checked); what's drawn is the entry's `icon` (a check in the
        /// gutter), never the kit's own mark at the right edge, which is blanked.
        checked: Option<bool>,
    },
    /// A section title: not clickable, not focusable.
    Title(String),
    Separator,
}

/// Draws nothing.
static BLANK: rune::Icon = rune::Icon { name: "phantom-blank", view: 16.0, body: "" };

/// The narrowest a menu is, however short its items.
const MIN_WIDTH: f32 = 280.0;
/// An item's icon, and the kit's gap between an item's parts.
const ICON: f32 = 14.0;
const GAP: f32 = 8.0;

/// Where an open menu is.
#[derive(Clone, Copy)]
pub enum At {
    /// A context menu's corner, in window points.
    Point(f32, f32),
}

impl Ui {
    /// `trigger`, with a menu of `entries` while `open` says where. A pick calls `on_pick` with
    /// the choice's id; Escape and a press outside call `on_close`.
    #[allow(clippy::too_many_arguments)]
    pub fn menu<S: 'static>(
        &self,
        key: &'static str,
        trigger: El,
        open: Option<At>,
        entries: Vec<Entry>,
        on_pick: impl Fn(&mut S, &'static str, &mut Tx) + Clone + 'static,
        on_close: impl Fn(&mut S) + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        // As wide as its widest item, so no label is cut ("Delete 3 torrents (files to the
        // Trash)"), and never narrower than a short menu has always been.
        let width = self.menu_width(&entries).max(MIN_WIDTH);
        let mut menu = rune_kit::Menu::new(key).trigger(trigger).open(open.is_some()).min_width(width);
        menu = match open {
            Some(At::Point(x, y)) => menu.at(x, y),
            None => menu.placement(Placement::BottomStart).gutter(4.0),
        };
        for entry in entries {
            menu = match entry {
                Entry::Choice { id, label, hint, icon, checked } => {
                    let pick = on_pick.clone();
                    let mut item = MenuItem::new(label).on_select_tx(move |s: &mut S, tx: &mut Tx| pick(s, id, tx));
                    item = match icon {
                        Some(icon) => item.icon(self.icon(icon)),
                        // An empty slot the icon's size, so every label starts at the same x.
                        None => item.lead(el().w(14.0).h(14.0)),
                    };
                    if let Some(on) = checked {
                        item = item.checked(on);
                    }
                    menu.item(match hint {
                        Some(hint) => item.shortcut(hint),
                        None => item,
                    })
                }
                Entry::Title(title) => menu.group_label(title),
                Entry::Separator => menu.separator(),
            };
        }
        // The kit draws a checked item's mark at the right edge; Phantom's is the icon in the gutter
        // (the theme's `Icon::Check`), so the kit's is blanked (its state stays, for screen readers).
        let mut kit = self.k.clone();
        kit.glyph.check_small = &BLANK;
        menu.on_close(on_close).build(&kit, layer)
    }

    /// How wide a menu of `entries` must be for its widest item to show whole, as the kit lays an
    /// item out: its padding, its icon, its label and its key, with the gaps between.
    pub fn menu_width(&self, entries: &[Entry]) -> f32 {
        let k = &self.k;
        let measure = |text: &str, style| rune::text::measure(text, &[], style, None).0;
        let hint_style = rune::TextStyle { line_height: 11.0, ..k.ty.small_medium };
        let item = |label: &str, hint: &Option<String>| {
            // The item's padding (12 and 6), the icon's slot and its gap, the frame's padding (2 a side).
            let mut w = 12.0 + 6.0 + ICON + GAP + 4.0 + self.label_width(label);
            if let Some(hint) = hint {
                w += GAP + measure(hint, hint_style);
            }
            w
        };
        let widest = entries.iter().map(|e| match e {
            Entry::Choice { label, hint, .. } => item(label, hint),
            Entry::Title(title) => 24.0 + measure(title, k.ty.small_medium),
            Entry::Separator => 0.0,
        });
        // A point to spare, as widths round.
        widest.fold(0.0, f32::max).ceil() + 2.0
    }

    /// How wide an item's `label` is, set as the kit sets it, whole.
    pub fn label_width(&self, label: &str) -> f32 {
        rune::text::measure(label, &[], self.k.ty.ui_compact, None).0
    }

    /// A context menu has no trigger on the page: its anchor is a point in the layer. It opens
    /// there when it fits below it in `window` (its size, when known); else it's moved up as far
    /// as it must, as macOS slides such a menu up, and one taller than the window starts at its
    /// top and scrolls.
    #[allow(clippy::too_many_arguments)]
    pub fn context_menu<S: 'static>(
        &self,
        key: &'static str,
        at: Option<(f32, f32)>,
        window: Option<(f32, f32)>,
        entries: Vec<Entry>,
        on_pick: impl Fn(&mut S, &'static str, &mut Tx) + Clone + 'static,
        on_close: impl Fn(&mut S) + 'static,
        layer: &mut Vec<El>,
    ) {
        let at = at.map(|(x, y)| match window {
            Some((_, h)) => (x, y.min(h - EDGE - menu_height(&entries)).max(EDGE)),
            None => (x, y),
        });
        let anchor = self.menu(key, el(), at.map(|(x, y)| At::Point(x, y)), entries, on_pick, on_close, layer);
        layer.push(anchor);
    }
}

/// How far from the window's edges a menu stays, as Rune keeps it.
const EDGE: f32 = 8.0;

/// How tall a menu of `entries` is, as the kit lays it out: a row each choice and title, a rule
/// with its margins each separator, the frame's padding.
fn menu_height(entries: &[Entry]) -> f32 {
    4.0 + entries.iter().map(|e| if matches!(e, Entry::Separator) { 5.0 } else { 28.0 }).sum::<f32>()
}
