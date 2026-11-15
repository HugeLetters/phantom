//! The menu bar's own items, which macOS draws outside the window: Settings… in the app menu, and
//! Edit ▸ Undo <action> (#150), enabled while something can be undone, its title from the latest
//! snapshot ([`update`]). Its ⌘Z is the shortcut table's ([`crate::shortcuts`]): the window takes
//! the press first (`keys_first`: a focused field's own undo, then the table), and the item has it
//! only while enabled, as AppKit swallows the key equivalent of a disabled item.

use client::display;
use proto::Undoable;
use rune::MenuItem;

use crate::model::{self, Phantom};

/// Every item, with Edit ▸ Undo as `undo` says.
pub fn items(undo: Option<&Undoable>) -> Vec<MenuItem<Phantom>> {
    let settings = MenuItem::new("Settings…", |s: &mut Phantom, _| model::settings::open(s))
        .id("settings")
        .key_equivalent("cmd+comma".parse().expect("a shortcut"))
        .keys_first();
    let item = MenuItem::new(display::undo_item(undo), |s: &mut Phantom, _| model::undo(s)).id("undo").menu("Edit").enabled(undo.is_some());
    let item = match undo {
        Some(_) => item.key_equivalent("cmd+z".parse().expect("a shortcut")).keys_first(),
        None => item,
    };
    vec![settings, item]
}

/// The items as the app's state has them now, once the window is up; none in tests.
pub fn update(s: &Phantom) {
    if let Some(window) = &s.window
        && let Err(e) = window.set_menu(items(s.undo.as_ref()))
    {
        eprintln!("phantom: the menu bar: {e}");
    }
}
