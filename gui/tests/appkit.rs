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

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    assert!(objc2::MainThreadMarker::new().is_some(), "not on the main thread");
    let tests: [(&str, fn()); 3] = [
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
}
