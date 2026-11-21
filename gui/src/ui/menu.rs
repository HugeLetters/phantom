//! A menu of choices: `ui.menu(...)` draws the kit's menu in the current look, for a context menu
//! at a point or a dropdown under its trigger. Screens say what the choices are and what a pick
//! or a dismissal does; the look, the keyboard focus and Escape and outside presses are the kit's.

use rune::{El, Tx, el};
use rune_kit::{MenuEntry, MenuItem, Placement};

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
        /// A choice among values, as the priorities: `Some(true)` for the current one, checked at
        /// the item's right edge (its icon is its own, as every item's). Screen readers hear it
        /// (`menuitemcheckbox`, checked).
        checked: Option<bool>,
    },
    /// A choice that opens more to its right while `open` (Move to ▸): picking it calls the pick's
    /// handler with `id`, to open or close it; its entries are picked as the menu's own.
    Sub {
        id: &'static str,
        label: String,
        icon: Option<Icon>,
        entries: Vec<Entry>,
        open: bool,
    },
    /// A section title: not clickable, not focusable.
    Title(String),
    Separator,
}

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
                Entry::Title(title) => menu.group_label(title),
                Entry::Separator => menu.separator(),
                item => menu.item(self.menu_item(item, &on_pick)),
            };
        }
        // The kit's check, at a checked item's right edge, in the theme's own drawing.
        let mut kit = self.k.clone();
        kit.glyph.check_small = self.icon(Icon::Check);
        menu.on_close(on_close).build(&kit, layer)
    }

    /// A choice, or a submenu with its own, as the kit draws it.
    fn menu_item<S: 'static>(&self, entry: Entry, on_pick: &(impl Fn(&mut S, &'static str, &mut Tx) + Clone + 'static)) -> MenuItem {
        let (id, label, icon) = match &entry {
            Entry::Choice { id, label, icon, .. } | Entry::Sub { id, label, icon, .. } => (*id, label.clone(), *icon),
            Entry::Title(_) | Entry::Separator => unreachable!("not an item"),
        };
        let pick = on_pick.clone();
        let mut item = MenuItem::new(label).on_select_tx(move |s: &mut S, tx: &mut Tx| pick(s, id, tx));
        item = match icon {
            Some(icon) => item.icon(self.icon(icon)),
            // An empty slot the icon's size, so every label starts at the same x.
            None => item.lead(el().w(14.0).h(14.0)),
        };
        match entry {
            Entry::Choice { hint, checked, .. } => {
                if let Some(on) = checked {
                    item = item.checked(on);
                }
                match hint {
                    Some(hint) => item.shortcut(hint),
                    None => item,
                }
            }
            Entry::Sub { entries, open, .. } => {
                let sub = entries
                    .into_iter()
                    .map(|e| match e {
                        Entry::Title(title) => MenuEntry::Label(title),
                        Entry::Separator => MenuEntry::Separator,
                        item => MenuEntry::item(self.menu_item(item, on_pick)),
                    })
                    .collect();
                item.submenu(sub, open)
            }
            Entry::Title(_) | Entry::Separator => unreachable!("not an item"),
        }
    }

    /// How wide a menu of `entries` must be for its widest item to show whole, as the kit lays an
    /// item out: its padding, its icon, its label, its key and its check, with the gaps between.
    pub fn menu_width(&self, entries: &[Entry]) -> f32 {
        let k = &self.k;
        let measure = |text: &str, style| rune::text::measure(text, &[], style, None).0;
        let hint_style = rune::TextStyle { line_height: 11.0, ..k.ty.small_medium };
        let item = |label: &str, hint: &Option<String>, checked: &Option<bool>| {
            // The item's padding (12 and 6), the icon's slot and its gap, the frame's padding (2 a side).
            let mut w = 12.0 + 6.0 + ICON + GAP + 4.0 + self.label_width(label);
            if let Some(hint) = hint {
                w += GAP + measure(hint, hint_style);
            }
            if checked.is_some() {
                w += GAP + 16.0;
            }
            w
        };
        let widest = entries.iter().map(|e| match e {
            Entry::Choice { label, hint, checked, .. } => item(label, hint, checked),
            // Its chevron where a check would be.
            Entry::Sub { label, .. } => item(label, &None, &Some(false)),
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
