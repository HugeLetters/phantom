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
    },
    Separator,
}

/// Where an open menu is.
#[derive(Clone, Copy)]
pub enum At {
    /// A context menu's corner, in window points.
    Point(f32, f32),
    /// Under its trigger.
    Under,
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
        // Wide enough for the longest label, which truncates otherwise.
        let mut menu = rune_kit::Menu::new(key).trigger(trigger).open(open.is_some()).min_width(280.0);
        menu = match open {
            Some(At::Point(x, y)) => menu.at(x, y),
            Some(At::Under) | None => menu.placement(Placement::BottomStart).gutter(4.0),
        };
        for entry in entries {
            menu = match entry {
                Entry::Choice { id, label, hint, icon } => {
                    let pick = on_pick.clone();
                    let mut item = MenuItem::new(label).on_select_tx(move |s: &mut S, tx: &mut Tx| pick(s, id, tx));
                    if let Some(icon) = icon {
                        item = item.icon(self.icon(icon));
                    }
                    menu.item(match hint {
                        Some(hint) => item.shortcut(hint),
                        None => item,
                    })
                }
                Entry::Separator => menu.separator(),
            };
        }
        menu.on_close(on_close).build(&self.k, layer)
    }

    /// A context menu has no trigger on the page: its anchor is a point in the layer.
    pub fn context_menu<S: 'static>(
        &self,
        key: &'static str,
        at: Option<(f32, f32)>,
        entries: Vec<Entry>,
        on_pick: impl Fn(&mut S, &'static str, &mut Tx) + Clone + 'static,
        on_close: impl Fn(&mut S) + 'static,
        layer: &mut Vec<El>,
    ) {
        let anchor = self.menu(key, el(), at.map(|(x, y)| At::Point(x, y)), entries, on_pick, on_close, layer);
        layer.push(anchor);
    }
}
