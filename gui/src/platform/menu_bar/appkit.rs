//! What Rune's menus don't cover, on AppKit's own menu bar. Main thread only; each call finds the
//! menus anew, as Rune makes and drops them, and does nothing where there's none.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CString, c_char, c_void};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol, Sel};
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSString};

thread_local! {
    /// How many menus are open.
    static OPEN: Cell<u32> = const { Cell::new(0) };
    static HELPER: RefCell<Option<Retained<Helper>>> = const { RefCell::new(None) };
    /// Whether each standard item's action is offered.
    static OFFERED: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    /// The keys a menu shows while it's open ([`arrange`]).
    static SHOWN: RefCell<Vec<ShownKey>> = const { RefCell::new(Vec::new()) };
    static HOOKED: Cell<bool> = const { Cell::new(false) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PhantomMenuHelper"]
    struct Helper;

    impl Helper {
        #[unsafe(method(menuOpened:))]
        fn opened(&self, _: &NSNotification) {
            OPEN.set(OPEN.get() + 1);
        }

        #[unsafe(method(menuClosed:))]
        fn closed(&self, _: &NSNotification) {
            OPEN.set(OPEN.get().saturating_sub(1));
        }

        /// As a menu's delegate: it's about to open, with the keys its items show.
        #[unsafe(method(menuNeedsUpdate:))]
        fn needs_update(&self, menu: &NSMenu) {
            SHOWN.with_borrow(|shown| {
                for item in items(menu) {
                    let title = item.title().to_string();
                    let Some(key) = shown.iter().find(|k| k.title == title) else { continue };
                    let mut mask = NSEventModifierFlags::empty();
                    mask.set(NSEventModifierFlags::Command, key.cmd);
                    mask.set(NSEventModifierFlags::Shift, key.shift);
                    item.setKeyEquivalent(&NSString::from_str(&key.key.to_string()));
                    item.setKeyEquivalentModifierMask(mask);
                }
            });
        }

        #[unsafe(method(menuDidClose:))]
        fn did_close(&self, menu: &NSMenu) {
            drop_keys(menu);
        }

        /// So AppKit, looking for a key's item, doesn't have the menu show its keys.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn has_key(&self, _: &NSMenu, _: &NSEvent, _: *mut c_void, _: *mut c_void) -> bool {
            false
        }
    }
);

/// The key the item `title` shows: its character as AppKit has it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownKey {
    pub title: String,
    pub key: char,
    pub cmd: bool,
    pub shift: bool,
}

/// A closed menu's items have no keys: AppKit would match them before the window sees the press, and
/// swallow a greyed item's.
fn drop_keys(menu: &NSMenu) {
    for item in items(menu).filter(|i| !i.keyEquivalent().is_empty()) {
        item.setKeyEquivalent(&NSString::from_str(""));
    }
}

unsafe extern "C" {
    fn class_addMethod(class: *const AnyClass, selector: Sel, implementation: *const c_void, types: *const c_char) -> Bool;
}

fn helper() -> Retained<Helper> {
    HELPER.with_borrow_mut(|h| {
        // SAFETY: `init` on a fresh NSObject subclass with no ivars.
        h.get_or_insert_with(|| unsafe { msg_send![Helper::alloc(), init] }).clone()
    })
}

fn items(menu: &NSMenu) -> impl Iterator<Item = Retained<NSMenuItem>> {
    (0..menu.numberOfItems()).filter_map(|i| menu.itemAtIndex(i))
}

fn bar() -> Option<(MainThreadMarker, Retained<NSApplication>, Retained<NSMenu>)> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let root = app.mainMenu()?;
    Some((mtm, app, root))
}

/// Follows the menu bar's menus opening and closing, for [`tracking`].
pub fn watch_tracking() {
    let helper = helper();
    for (name, selector) in
        [("NSMenuDidBeginTrackingNotification", sel!(menuOpened:)), ("NSMenuDidEndTrackingNotification", sel!(menuClosed:))]
    {
        // SAFETY: the selectors are the helper's, taking the notification; it lives for the thread.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &helper,
                selector,
                Some(&NSString::from_str(name)),
                None,
            );
        }
    }
}

/// Whether a menu is open.
pub fn tracking() -> bool {
    OPEN.get() > 0
}

/// `menu` shows `keys` while it's open and has none else; the Help menu is there, for macOS's search; Window's Close leaves
/// its place to File's Close Window.
pub fn arrange(menu: &str, keys: Vec<ShownKey>) {
    SHOWN.set(keys);
    let Some((mtm, app, root)) = bar() else { return };
    let helper = helper();
    let mut help = false;
    for top in items(&root) {
        let Some(submenu) = top.submenu() else { continue };
        let title = top.title().to_string();
        help |= title == "Help";
        if title == menu {
            // SAFETY: the delegate isn't retained; the helper lives for the thread.
            unsafe {
                let _: () = msg_send![&submenu, setDelegate: &*helper];
            }
            if !tracking() {
                drop_keys(&submenu);
            }
        }
        if title == "Window" {
            items(&submenu).filter(|i| i.action() == Some(sel!(performClose:))).for_each(|i| i.setHidden(true));
        }
    }
    // The bar has only the app menu until Rune's first items are in.
    if !help && root.numberOfItems() > 1 {
        let title = NSString::from_str("Help");
        let (top, submenu) = (NSMenuItem::new(mtm), NSMenu::new(mtm));
        top.setTitle(&title);
        submenu.setTitle(&title);
        top.setSubmenu(Some(&submenu));
        root.addItem(&top);
        // SAFETY: AppKit's own setter, on the main thread.
        unsafe {
            let _: () = msg_send![&app, setHelpMenu: &*submenu];
        }
    }
}

/// The standard item whose action is `action`: its title, whether it's greyed, whether it's there.
/// Compared with the item's own, which AppKit and Rune retitle meanwhile.
pub fn standard(action: &str, title: &str, enabled: bool, shown: bool) {
    OFFERED.with_borrow_mut(|o| o.insert(action.to_owned(), enabled));
    hook();
    let Some((_, _, root)) = bar() else { return };
    let selector = Sel::register(&CString::new(action).expect("a selector"));
    for top in items(&root) {
        let Some(submenu) = top.submenu() else { continue };
        for item in items(&submenu).filter(|i| i.action() == Some(selector)) {
            if item.title().to_string() != title {
                item.setTitle(&NSString::from_str(title));
            }
            if item.isHidden() == shown {
                item.setHidden(!shown);
            }
        }
    }
}

/// Rune's view answers the standard actions, so AppKit asks it whether each is offered.
fn hook() {
    if HOOKED.get() {
        return;
    }
    let Some(class) = AnyClass::get(c"WinitView") else { return };
    // SAFETY: main thread; the function has the ABI of `-(BOOL)validateMenuItem:(NSMenuItem *)item`.
    // Adding never replaces a method the class has.
    unsafe { class_addMethod(class, sel!(validateMenuItem:), validate as *const c_void, c"B@:@".as_ptr()) };
    HOOKED.set(true);
}

extern "C" fn validate(_: *mut AnyObject, _: Sel, item: *mut AnyObject) -> Bool {
    // SAFETY: AppKit validates on the main thread, with a live menu item.
    let action = unsafe { item.cast::<NSMenuItem>().as_ref() }.and_then(|i| i.action());
    Bool::new(offered(action))
}

/// Whether the item of `action` is offered now. Always for a ⌘ key: AppKit swallows a greyed item's
/// key, and the window is the one to say what it does.
pub fn offered(action: Option<Sel>) -> bool {
    let pressed = MainThreadMarker::new()
        .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent())
        .is_some_and(|e| e.r#type() == NSEventType::KeyDown && e.modifierFlags().contains(NSEventModifierFlags::Command));
    let name = action.and_then(|a| a.name().to_str().ok().map(str::to_owned));
    pressed || name.is_none_or(|n| OFFERED.with_borrow(|o| o.get(&n).copied().unwrap_or(true)))
}

/// Whether a window shows: not closed, not minimized.
pub fn window_showing() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    NSApplication::sharedApplication(mtm).windows().iter().any(|w| w.isVisible() && !is_panel(&w) && w.canBecomeMainWindow())
}

/// Whether a panel of macOS's is up: a file picker, or About with the keyboard.
pub fn panel_up() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    app.modalWindow().is_some()
        || app.keyWindow().is_some_and(|w| is_panel(&w))
        || app.windows().iter().any(|w| w.attachedSheet().is_some())
}

pub fn close_key_window() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() {
        window.performClose(None);
    }
}

fn is_panel(window: &objc2_app_kit::NSWindow) -> bool {
    AnyClass::get(c"NSPanel").is_some_and(|panel| window.isKindOfClass(panel))
}
