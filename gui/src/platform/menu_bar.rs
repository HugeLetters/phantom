//! Shows the menu bar `menu_bar` builds. Phantom's own items are Rune's custom ones
//! (`AppHandle::set_menu`); what Rune's menus don't cover is AppKit's beside it ([`appkit`]): the
//! standard items' titles, greying and leaving out, keys shown but not matched, the Help menu.
//!
//! While a menu is open Phantom's own items stay as they were; the change shows once it's closed,
//! a frame later, so a pick made in it runs first.

#[cfg(target_os = "macos")]
pub mod appkit;

use rune::Tx;

use crate::menu_bar::{self, Around};
use crate::model::Phantom;

#[cfg(target_os = "macos")]
thread_local! {
    static BAR: std::cell::RefCell<Option<Bar>> = const { std::cell::RefCell::new(None) };
}

/// A real window's app and the menus it was last given; none in a headless run.
#[cfg(target_os = "macos")]
struct Bar {
    app: rune::AppHandle<Phantom>,
    set: Vec<menu_bar::Menu>,
    /// A menu was open at the last frame.
    was_open: bool,
}

/// The menu bar is `app`'s from now on: never called in a headless run, which has none.
#[cfg(target_os = "macos")]
pub fn install(app: rune::AppHandle<Phantom>) {
    appkit::watch_tracking();
    BAR.set(Some(Bar { app, set: Vec::new(), was_open: false }));
}

/// Whether there's a menu bar: a real window's app.
#[cfg(target_os = "macos")]
fn live() -> bool {
    BAR.with_borrow(|bar| bar.is_some())
}

/// Before every frame: the menu bar for the app as it is. Nothing in a headless run.
pub fn sync(s: &Phantom, tx: &mut Tx) {
    #[cfg(target_os = "macos")]
    if !live() {
        return;
    }
    // Before the bar is borrowed: what's around asks it too.
    #[cfg(target_os = "macos")]
    let menus = menu_bar::build(s, around(s));
    #[cfg(target_os = "macos")]
    BAR.with_borrow_mut(|bar| {
        let Some(bar) = bar else { return };
        appkit::arrange("Torrent", menus.iter().filter(|m| m.name == "Torrent").flat_map(shown_keys).collect());
        for item in menus.iter().flat_map(|m| &m.items) {
            if let menu_bar::Item::Standard { action, title, enabled, shown, .. } = item {
                appkit::standard(action, title, *enabled, *shown);
            }
        }
        let open = appkit::tracking();
        if open || std::mem::replace(&mut bar.was_open, open) {
            bar.was_open = open;
            return tx.wake_after(std::time::Duration::from_millis(16));
        }
        let own: Vec<_> = menus.into_iter().map(own_only).collect();
        if bar.set != own && bar.app.set_menu(own.iter().flat_map(items)).is_ok() {
            bar.set = own;
            // Rune titles its standard items anew as it takes the set: worded again next frame.
            tx.wake_after(std::time::Duration::from_millis(16));
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (s, tx);
}

/// The window in front and whether a panel of macOS's is up; in a headless run, as the windows said.
pub fn around(s: &Phantom) -> Around {
    #[cfg(target_os = "macos")]
    if live() {
        return Around { front: appkit::window_showing().then_some(s.front), panel: appkit::panel_up() };
    }
    Around { front: Some(s.front), panel: false }
}

/// ⌘W with a panel of macOS's in front (About): it closes.
pub fn close_panel() {
    #[cfg(target_os = "macos")]
    appkit::close_key_window();
}

/// `menu` without macOS's items, which [`appkit`] words: they don't make a new set.
#[cfg(target_os = "macos")]
fn own_only(mut menu: menu_bar::Menu) -> menu_bar::Menu {
    use menu_bar::Item;
    menu.items.retain(|i| matches!(i, Item::Own(_) | Item::Submenu { .. } | Item::Separator));
    // The app, Edit and Window menus are macOS's, with separators of their own.
    if matches!(menu.name, "Phantom" | "Edit") {
        menu.items.retain(|i| !matches!(i, Item::Separator));
    }
    menu
}

#[cfg(target_os = "macos")]
fn items(menu: &menu_bar::Menu) -> Vec<rune::MenuItem<Phantom>> {
    use menu_bar::Item;
    use rune::MenuItem;
    let place = |item: MenuItem<Phantom>| match menu.name {
        "Phantom" => item,
        "Window" => item.menu("Window").after("performZoom:"),
        name => item.menu(name),
    };
    let item = |item: &Item| match item {
        Item::Own(own) => Some(place(native(own))),
        Item::Separator => Some(place(MenuItem::separator())),
        Item::Submenu { title, enabled, items } => Some(place(MenuItem::submenu(*title, items.iter().map(native)).enabled(*enabled))),
        Item::Standard { .. } | Item::System { .. } => None,
    };
    menu.items.iter().filter_map(item).collect()
}

/// The keys `menu` only shows.
#[cfg(target_os = "macos")]
fn shown_keys(menu: &menu_bar::Menu) -> Vec<appkit::ShownKey> {
    use crate::shortcuts::Named;
    let key = |own: &menu_bar::Own| {
        let menu_bar::Keys::Shown(chord) = own.keys else { return None };
        let key = match chord.key {
            Named::Letter(c) => c,
            Named::Return => '\r',
            Named::Space => ' ',
            Named::Backspace => '\u{8}',
        };
        Some(appkit::ShownKey { title: own.title.clone(), key, cmd: chord.cmd, shift: chord.shift })
    };
    menu.own().filter_map(key).collect()
}

#[cfg(target_os = "macos")]
fn native(own: &menu_bar::Own) -> rune::MenuItem<Phantom> {
    use menu_bar::Keys;
    let pick = own.pick.clone();
    let item = rune::MenuItem::with_action(&own.title, move |s: &mut Phantom, tx| menu_bar::run(s, &pick, around(s), tx));
    let item = item.id(&own.id).enabled(own.enabled).checked(own.checked);
    match own.keys {
        // A shown key is the item's only while its menu is open ([`shown_keys`]).
        Keys::None | Keys::Shown(_) => item.keys_first(),
        Keys::Window(keys) => item.key_equivalent(keys.accelerator().parse().expect("a shortcut")).keys_first(),
    }
}
