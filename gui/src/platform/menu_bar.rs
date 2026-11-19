//! The menu bar's own items, which macOS draws outside the window: Settings… in the app menu, and
//! Window ▸ Phantom, which brings the main window back after ⌘W closed it (#257).
//! Edit ▸ Undo is Rune's standard item (`App::standard_menus`), which hands ⌘Z to the window in
//! front as a key, clicked or pressed: a focused field's own undo, else the shortcut table's Undo
//! (#150), or in Settings the main window's, which it's handed to (#221, #257). Its title names what
//! it takes back there ([`name_undo`]), whichever window is in front.

use rune::MenuItem;

use crate::model::{self, Phantom};

pub fn items() -> Vec<MenuItem<Phantom>> {
    let settings = MenuItem::new("Settings…", |s: &mut Phantom, _| s.settings.post(|w, _| model::settings::open(w)))
        .id("settings")
        .key_equivalent("cmd+comma".parse().expect("a shortcut"))
        .keys_first();
    let main = MenuItem::new("Phantom", |s: &mut Phantom, _| s.window.raise()).id("main-window").menu("Window");
    vec![settings, main]
}

#[cfg(target_os = "macos")]
thread_local! {
    /// Whether the window is a real one, with a menu bar.
    static LIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Edit ▸ Undo is named from now on: only a real window's app, never a headless one, has the menu.
#[cfg(target_os = "macos")]
pub fn install() {
    LIVE.set(true);
}

/// Gives Edit ▸ Undo (the standard item, whose action is `undo:`) `title`, when it has another:
/// compared with the item's own, which AppKit may have retitled meanwhile ("Undo Typing").
/// Nothing in a headless run.
pub fn name_undo(title: &str) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;
        let Some(mtm) = MainThreadMarker::new().filter(|_| LIVE.get()) else { return };
        let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() else { return };
        let undo = objc2::sel!(undo:);
        for top in menu.itemArray() {
            let Some(submenu) = top.submenu() else { continue };
            for item in submenu.itemArray() {
                if item.action() == Some(undo) {
                    if item.title().to_string() != title {
                        item.setTitle(&NSString::from_str(title));
                    }
                    return;
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}
