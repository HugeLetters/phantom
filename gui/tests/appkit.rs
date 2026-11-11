//! The App icon's AppKit side (`gui/src/platform/app_icon/appkit.rs`), on the main thread as
//! AppKit expects: `harness = false`, so `main` runs each test in order. Each touches only an
//! empty `.app` in a temp dir.

// Without a harness the module's own `#[test]`s (cfg(test) is still on) are left out, so their
// imports go unused.
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/platform/app_icon/mod.rs"]
mod app_icon;

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    assert!(objc2::MainThreadMarker::new().is_some(), "not on the main thread");
    let tests: [(&str, fn()); 2] = [
        ("phantom_writes_the_bundle_icon_once_and_resets_only_its_own", macos::phantom_writes_the_bundle_icon_once_and_resets_only_its_own),
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

    use rune::prefs::Prefs;

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
        let mut prefs = Prefs::memory();
        let dark_hour = icon("dark-hour");
        let image = image(&dark_hour.bytes).expect("the .icns reads");

        update_bundle(&mut prefs, &bundle.0, Some(&dark_hour), Some(&image));
        assert!(bundle.has_custom_icon());
        let written: Written = prefs.get(WRITTEN).expect("remembered");
        assert_eq!(written.id, "dark-hour");

        // The same icon again leaves the file alone.
        let before = std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap();
        update_bundle(&mut prefs, &bundle.0, Some(&dark_hour), Some(&image));
        assert_eq!(std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap(), before);

        update_bundle(&mut prefs, &bundle.0, None, None);
        assert!(!bundle.has_custom_icon());
        assert!(!bundle.0.join("Icon\r").exists());
        assert!(prefs.get::<Written>(WRITTEN).is_none());
    }

    pub fn the_users_own_finder_icon_survives_a_reset() {
        let bundle = Bundle::new("users");
        let mut prefs = Prefs::memory();
        let dark_hour = icon("dark-hour");
        update_bundle(&mut prefs, &bundle.0, Some(&dark_hour), image(&dark_hour.bytes).as_deref());
        // The user pastes their own icon in Finder.
        assert!(set_bundle_icon(&bundle.0, image(&icon("full-moon").bytes).as_deref()));
        update_bundle(&mut prefs, &bundle.0, None, None);
        assert!(bundle.has_custom_icon());
    }
}
