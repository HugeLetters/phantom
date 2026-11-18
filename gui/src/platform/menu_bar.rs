//! The menu bar's own items, which macOS draws outside the window: Settings… in the app menu.
//! Edit ▸ Undo is Rune's standard item (`App::standard_menus`), which hands ⌘Z to the window as a
//! key, clicked or pressed: a focused field's own undo, else the shortcut table's Undo (#150), or in
//! Settings the settings' (#221). Its title names what it takes back there ([`name_undo`]).

use rune::MenuItem;

use crate::model::{self, Phantom};

pub fn items() -> Vec<MenuItem<Phantom>> {
    let settings = MenuItem::new("Settings…", |s: &mut Phantom, _| model::settings::open(s))
        .id("settings")
        .key_equivalent("cmd+comma".parse().expect("a shortcut"))
        .keys_first();
    vec![settings]
}

#[cfg(target_os = "macos")]
thread_local! {
    /// Whether the window is a real one, with a menu bar; and the title Edit ▸ Undo was last given.
    static NAMED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Edit ▸ Undo is named from now on: only a real window's app, never a headless one, has the menu.
#[cfg(target_os = "macos")]
pub fn install() {
    NAMED.set(Some(String::new()));
}

/// Gives Edit ▸ Undo (the standard item, whose action is `undo:`) `title`, when it has another.
/// Nothing in a headless run.
pub fn name_undo(title: &str) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;
        let changed = NAMED.with_borrow(|named| named.as_ref().is_some_and(|named| named != title));
        let Some(mtm) = MainThreadMarker::new().filter(|_| changed) else { return };
        let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
        let undo = objc2::sel!(undo:);
        for top in menu.itemArray() {
            let Some(submenu) = top.submenu() else { continue };
            for item in submenu.itemArray() {
                if item.action() == Some(undo) {
                    item.setTitle(&NSString::from_str(title));
                    NAMED.set(Some(title.to_owned()));
                    return;
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}
