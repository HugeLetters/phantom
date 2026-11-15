//! The menu bar's own items, which macOS draws outside the window: Settings… in the app menu.
//! Edit ▸ Undo is Rune's standard item (`App::standard_menus`), which hands ⌘Z to the window as a
//! key, clicked or pressed: a focused field's own undo, else the shortcut table's Undo (#150).

use rune::MenuItem;

use crate::model::{self, Phantom};

pub fn items() -> Vec<MenuItem<Phantom>> {
    let settings = MenuItem::new("Settings…", |s: &mut Phantom, _| model::settings::open(s))
        .id("settings")
        .key_equivalent("cmd+comma".parse().expect("a shortcut"))
        .keys_first();
    vec![settings]
}
