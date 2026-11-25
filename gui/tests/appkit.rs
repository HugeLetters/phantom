//! The App icon's AppKit side (`gui/src/platform/app_icon/appkit.rs`), on the main thread as
//! AppKit expects: `harness = false`, so `main` runs each test in order. Each touches only an
//! empty `.app` in a temp dir.

// Without a harness the module's own `#[test]`s (cfg(test) is still on) are left out, so their
// imports go unused.
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/platform/app_icon/mod.rs"]
mod app_icon;

// What `appkit` keeps `gui.json` with, as the app has it.
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/settings"]
mod settings {
    pub mod file;
}

#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/platform/open_documents.rs"]
mod open_documents;

#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/platform/menu_bar/appkit.rs"]
mod menu_bar;

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    assert!(objc2::MainThreadMarker::new().is_some(), "not on the main thread");
    let tests: [(&str, fn()); 5] = [
        ("a_shown_key_is_no_key_of_the_menu", macos::a_shown_key_is_no_key_of_the_menu),
        ("the_standard_items_are_worded_greyed_and_left_out", macos::the_standard_items_are_worded_greyed_and_left_out),
        ("phantom_writes_the_bundle_icon_once_and_resets_only_its_own", macos::phantom_writes_the_bundle_icon_once_and_resets_only_its_own),
        (
            "phantom_alone_takes_open_documents_before_the_loop_and_after_launch",
            macos::phantom_alone_takes_open_documents_before_the_loop_and_after_launch,
        ),
        ("the_users_own_finder_icon_survives_a_reset", macos::the_users_own_finder_icon_survives_a_reset),
    ];
    for (name, test) in tests {
        test();
        println!("test {name} ... ok");
    }
    println!("test result: ok. {} passed", tests.len());
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use crate::app_icon::appkit::{image, set_bundle_icon, update_bundle};
    use crate::app_icon::{Icon, WRITTEN, Written, custom_icon_data};

    fn icon(id: &str) -> Icon {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../design/icons/app/{id}.icns"));
        Icon { id: id.into(), name: id.into(), bytes: std::fs::read(path).unwrap() }
    }

    /// An empty `.app` in a temp dir, removed on drop.
    struct Bundle(PathBuf);

    impl Bundle {
        fn new(name: &str) -> Bundle {
            let dir = std::env::temp_dir().join(format!("phantom-app-icon-{}-{name}", std::process::id()));
            let app = dir.join("Phantom.app");
            std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            Bundle(app)
        }

        fn has_custom_icon(&self) -> bool {
            custom_icon_data(&self.0).exists()
        }
    }

    impl Drop for Bundle {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    pub fn phantom_writes_the_bundle_icon_once_and_resets_only_its_own() {
        let bundle = Bundle::new("reset");
        let gui_json = bundle.0.parent().unwrap().join("gui.json");
        let dark_hour = icon("dark-hour");
        let image = image(&dark_hour.bytes).expect("the .icns reads");

        update_bundle(&gui_json, &bundle.0, Some(&dark_hour), Some(&image));
        assert!(bundle.has_custom_icon());
        let written: Written = crate::settings::file::read_at(&gui_json, WRITTEN).expect("remembered");
        assert_eq!(written.id, "dark-hour");

        // The same icon again leaves the file alone.
        let before = std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap();
        update_bundle(&gui_json, &bundle.0, Some(&dark_hour), Some(&image));
        assert_eq!(std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap(), before);

        update_bundle(&gui_json, &bundle.0, None, None);
        assert!(!bundle.has_custom_icon());
        assert!(!bundle.0.join("Icon\r").exists());
        assert!(crate::settings::file::read_at::<Written>(&gui_json, WRITTEN).is_none());
    }

    pub fn the_users_own_finder_icon_survives_a_reset() {
        let bundle = Bundle::new("users");
        let gui_json = bundle.0.parent().unwrap().join("gui.json");
        let dark_hour = icon("dark-hour");
        update_bundle(&gui_json, &bundle.0, Some(&dark_hour), image(&dark_hour.bytes).as_deref());
        // The user pastes their own icon in Finder.
        assert!(set_bundle_icon(&bundle.0, image(&icon("full-moon").bytes).as_deref()));
        update_bundle(&gui_json, &bundle.0, None, None);
        assert!(bundle.has_custom_icon());
    }

    /// Rune installs its own open-documents handler as its loop starts unless the app has one, and
    /// puts it back at launch: ours has to be there already, and still take the event, once, after
    /// AppKit's replaced it. The event is dispatched in this process only.
    pub fn phantom_alone_takes_open_documents_before_the_loop_and_after_launch() {
        use objc2::runtime::AnyObject;
        use objc2::{ClassType, msg_send};
        use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotificationCenter, NSString, NSURL};
        use std::ffi::c_void;
        use std::sync::{Arc, Mutex};

        #[link(name = "CoreServices", kind = "framework")]
        unsafe extern "C" {
            fn AEGetEventHandler(class: u32, id: u32, handler: *mut *const c_void, refcon: *mut *mut c_void, system: u8) -> i16;
        }
        /// Carbon's `AEDesc`, only ever behind a pointer here.
        #[repr(C)]
        struct AeDesc(u32, *mut c_void);
        // SAFETY: the encoding macOS's `AEDesc` has, for the layout above.
        unsafe impl objc2::RefEncode for AeDesc {
            const ENCODING_REF: objc2::Encoding = objc2::Encoding::Pointer(&objc2::Encoding::Struct(
                "AEDesc",
                &[
                    objc2::Encoding::UInt,
                    objc2::Encoding::Pointer(&objc2::Encoding::Pointer(&objc2::Encoding::Struct("OpaqueAEDataStorageType", &[]))),
                ],
            ));
        }
        const AEVT: u32 = u32::from_be_bytes(*b"aevt");
        const ODOC: u32 = u32::from_be_bytes(*b"odoc");
        // What Rune asks (`open_url/incoming.rs`, `files_handled_elsewhere`).
        let handled = || {
            let (mut handler, mut refcon) = (std::ptr::null(), std::ptr::null_mut());
            // SAFETY: reads the installed handler into the two out-pointers; 0 is noErr.
            unsafe { AEGetEventHandler(AEVT, ODOC, &mut handler, &mut refcon, 0) == 0 }
        };
        let manager = NSAppleEventManager::sharedAppleEventManager();
        assert!(!handled(), "nothing has it yet");

        let got: Arc<Mutex<Vec<Vec<PathBuf>>>> = Arc::default();
        let sink = got.clone();
        crate::open_documents::listen(move |paths| sink.lock().unwrap().push(paths));
        assert!(handled(), "Rune's loop finds it taken and leaves it alone");

        // AppKit's own handler takes its place as the app finishes launching, then it says so.
        // SAFETY: a Foundation call on the main thread.
        let _: () = unsafe { msg_send![&manager, removeEventHandlerForEventClass: AEVT, andEventID: ODOC] };
        assert!(!handled());
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        // SAFETY: posted on the main thread, with no object.
        unsafe { NSNotificationCenter::defaultCenter().postNotificationName_object(&name, None) };
        assert!(handled(), "back after launch");

        let list = NSAppleEventDescriptor::listDescriptor();
        list.insertDescriptor_atIndex(&NSAppleEventDescriptor::descriptorWithFileURL(&NSURL::from_file_path("/tmp/a.torrent").unwrap()), 1);
        let none = std::ptr::null::<AnyObject>();
        // SAFETY: Foundation constructors; nil is a valid target; both are descriptors; the raw
        // descriptors live as long as the events they come from.
        unsafe {
            let event: objc2::rc::Retained<NSAppleEventDescriptor> = msg_send![NSAppleEventDescriptor::class(),
                appleEventWithEventClass: AEVT, eventID: ODOC, targetDescriptor: none, returnID: -1i16, transactionID: 0i32];
            let _: () = msg_send![&event, setParamDescriptor: &*list, forKeyword: u32::from_be_bytes(*b"----")];
            let reply = NSAppleEventDescriptor::nullDescriptor();
            let raw_event: *const AeDesc = msg_send![&event, aeDesc];
            let raw_reply: *const AeDesc = msg_send![&reply, aeDesc];
            let _: i16 = msg_send![&manager, dispatchRawAppleEvent: raw_event, withRawReply: raw_reply, handlerRefCon: std::ptr::null_mut::<c_void>()];
        }
        assert_eq!(*got.lock().unwrap(), [vec![PathBuf::from("/tmp/a.torrent")]], "the files once");
    }

    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem};
    use objc2_foundation::{NSPoint, NSString};

    /// A menu bar of its own, never shown: an app menu and `menus`, each item a title, its action
    /// and its key with the modifiers.
    type Row<'a> = (&'a str, objc2::runtime::Sel, &'a str, NSEventModifierFlags);
    fn menu_bar(menus: &[(&str, &[Row])]) -> Retained<NSMenu> {
        let mtm = MainThreadMarker::new().unwrap();
        let root = NSMenu::new(mtm);
        for (name, rows) in std::iter::once(&("", &[] as &[Row])).chain(menus) {
            let (top, menu) = (NSMenuItem::new(mtm), NSMenu::new(mtm));
            top.setTitle(&NSString::from_str(name));
            menu.setTitle(&NSString::from_str(name));
            for (title, action, key, mask) in *rows {
                // SAFETY: AppKit's initializer, on the main thread.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        mtm.alloc(),
                        &NSString::from_str(title),
                        Some(*action),
                        &NSString::from_str(key),
                    )
                };
                item.setKeyEquivalentModifierMask(*mask);
                menu.addItem(&item);
            }
            top.setSubmenu(Some(&menu));
            root.addItem(&top);
        }
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&root));
        root
    }

    fn key(chars: &str, flags: NSEventModifierFlags, code: u16) -> Retained<NSEvent> {
        let chars = NSString::from_str(chars);
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown, NSPoint::new(0.0, 0.0), flags, 0.0, 0, None, &chars, &chars, false, code).unwrap()
    }

    /// A key the Torrent menu shows is its item's only while the menu is open: closed, a press is
    /// matched by nothing, whatever AppKit looks through.
    pub fn a_shown_key_is_no_key_of_the_menu() {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let none = NSEventModifierFlags::empty();
        let root = menu_bar(&[("Torrent", &[("Remove", sel!(hide:), "", none)])]);
        let shown = || vec![crate::menu_bar::ShownKey { title: "Remove".into(), key: '\u{8}', cmd: true, shift: false }];
        let menu = root.itemAtIndex(1).unwrap().submenu().unwrap();
        let item = menu.itemAtIndex(0).unwrap();
        let press = key("\u{8}", NSEventModifierFlags::Command, 51);
        crate::menu_bar::arrange("Torrent", shown());
        assert!(!root.performKeyEquivalent(&press), "closed");
        // SAFETY: the menu's delegate, sent what AppKit sends it.
        unsafe {
            let delegate: Retained<AnyObject> = msg_send![&menu, delegate];
            let _: () = msg_send![&delegate, menuNeedsUpdate: &*menu];
            assert_eq!(item.keyEquivalent().to_string(), "\u{8}", "open");
            assert_eq!(item.keyEquivalentModifierMask(), NSEventModifierFlags::Command);
            let _: () = msg_send![&delegate, menuDidClose: &*menu];
        }
        assert!(!root.performKeyEquivalent(&press), "closed again");
        assert_eq!(item.keyEquivalent().to_string(), "");

        // Help's search looks through a menu that never opens: its keys go as Help closes.
        crate::menu_bar::watch_tracking();
        let post = |name: &str| {
            let name = NSString::from_str(name);
            // SAFETY: what AppKit posts as the menu bar is tracked.
            unsafe { objc2_foundation::NSNotificationCenter::defaultCenter().postNotificationName_object(&name, Some(&root)) };
        };
        post("NSMenuDidBeginTrackingNotification");
        assert!(crate::menu_bar::tracking());
        // SAFETY: the menu's delegate, sent what AppKit sends it.
        unsafe {
            let delegate: Retained<AnyObject> = msg_send![&menu, delegate];
            let _: () = msg_send![&delegate, menuNeedsUpdate: &*menu];
        }
        crate::menu_bar::arrange("Torrent", shown());
        assert_eq!(item.keyEquivalent().to_string(), "\u{8}", "an open menu keeps its keys");
        post("NSMenuDidEndTrackingNotification");
        assert!(!crate::menu_bar::tracking());
        assert_eq!(item.keyEquivalent().to_string(), "", "searched");
    }

    pub fn the_standard_items_are_worded_greyed_and_left_out() {
        let cmd = NSEventModifierFlags::Command;
        let root = menu_bar(&[
            ("Edit", &[("Undo", sel!(undo:), "z", cmd), ("Redo", sel!(redo:), "", cmd)]),
            ("Window", &[("Close", sel!(performClose:), "", cmd)]),
        ]);
        crate::menu_bar::standard("undo:", "Undo Remove of Album", false, true);
        crate::menu_bar::standard("redo:", "Redo", true, false);
        crate::menu_bar::arrange("Torrent", Vec::new());
        crate::menu_bar::arrange("Torrent", Vec::new());
        let tops: Vec<String> = (0..root.numberOfItems()).map(|i| root.itemAtIndex(i).unwrap().title().to_string()).collect();
        assert_eq!(tops, ["", "Edit", "Window", "Help"], "Help once");
        let at = |menu: isize, i| root.itemAtIndex(menu).unwrap().submenu().unwrap().itemAtIndex(i).unwrap();
        assert_eq!((at(1, 0).title().to_string(), at(1, 0).isHidden()), ("Undo Remove of Album".into(), false));
        assert!(at(1, 1).isHidden(), "Redo is left out");
        assert!(at(1, 1).allowsKeyEquivalentWhenHidden(), "its key isn't");
        assert!(at(2, 0).isHidden(), "File has Close Window");
        assert!(!crate::menu_bar::offered(Some(sel!(undo:))));
        assert!(crate::menu_bar::offered(Some(sel!(redo:))));
        assert!(crate::menu_bar::offered(Some(sel!(performZoom:))), "others as ever");
    }
}
