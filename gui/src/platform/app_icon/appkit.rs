//! Shows the App icon ([`super`]) when Phantom runs as a bundle: in the Dock through
//! `NSApplication.applicationIconImage`, and on the bundle through `NSWorkspace.setIcon`, which
//! Finder, Spotlight, Launchpad and launchers read. Both are applied once the app finishes
//! launching (AppKit shows the bundle's icon over an earlier one) and when the choice changes.
//! When the effective Appearance changes only the Dock swaps a pair's sides: the bundle keeps the
//! side from launch, since each write is a main-thread stall of some 50-100 ms that rewrites the
//! signed bundle. Run outside a bundle (`cargo run`), nothing happens.
//!
//! The bundle icon is re-applied at every launch, as an install replaces the bundle. A custom
//! icon makes `codesign --verify --strict` refuse the bundle (FinderInfo detritus) while plain
//! `--verify` still passes; `scripts/install-mac.sh` clears it before signing.
//!
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationDidFinishLaunchingNotification, NSImage,
    NSWorkspace, NSWorkspaceIconCreationOptions,
};
use objc2_foundation::{
    NSArray, NSKeyValueObservingOptions, NSNotification, NSNotificationCenter, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString,
};
use rune::prefs::Prefs;

use super::{BundleStep, Icon, Manifest, SETTING, THEME, WRITTEN, Written, bundle_of, bundle_step, custom_icon_data, fingerprint, resolve, Icons};

struct State {
    prefs: Prefs,
    manifest: Manifest,
    resources: PathBuf,
    bundle: PathBuf,
    /// `None` follows the theme.
    choice: Option<Icons>,
    /// The icon the Dock shows; `None`, the bundle's own, at launch.
    dock: Option<String>,
}

thread_local! {
    /// Main thread only.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PhantomAppIcon"]
    struct Observer;

    impl Observer {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _: &NSNotification) {
            guarded(|| {
                apply(Bundle::Write);
                let Some(mtm) = MainThreadMarker::new() else { return };
                let key = NSString::from_str("effectiveAppearance");
                // SAFETY: the observer implements the KVO callback and lives for the process; no context.
                unsafe {
                    NSApplication::sharedApplication(mtm).addObserver_forKeyPath_options_context(
                        self,
                        &key,
                        NSKeyValueObservingOptions::empty(),
                        std::ptr::null_mut(),
                    );
                }
            });
        }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn appearance_changed(&self, _: Option<&NSString>, _: Option<&AnyObject>, _: Option<&AnyObject>, _: *mut c_void) {
            guarded(|| apply(Bundle::Keep));
        }
    }
);

/// Before Rune's event loop starts, on the main thread. Does nothing outside a bundle, without
/// its manifest, or if called again.
pub fn install() {
    if MainThreadMarker::new().is_none() || STATE.with_borrow(Option::is_some) {
        return;
    }
    let Some(state) = State::load() else { return };
    STATE.set(Some(state));
    // SAFETY: `init` on a fresh NSObject subclass with no ivars (objc2's way to make one).
    let observer: Retained<Observer> = unsafe { msg_send![Observer::alloc(), init] };
    // SAFETY: never released, as neither the notification center nor KVO retains it.
    let observer: &'static Observer = unsafe { &*Retained::into_raw(observer) };
    // SAFETY: the selector is the observer's, taking the notification; it's never freed.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            observer,
            sel!(applicationDidFinishLaunching:),
            Some(NSApplicationDidFinishLaunchingNotification),
            None,
        );
    }
}

/// The picker's way in (#42): stores the choice and shows it. Main thread, after [`install`].
#[allow(dead_code)]
pub fn choose(choice: Option<Icons>) {
    STATE.with_borrow_mut(|state| {
        let Some(state) = state else { return };
        let saved = match &choice {
            Some(icons) => state.prefs.set(SETTING, icons),
            None => state.prefs.remove(SETTING),
        };
        if let Err(e) = saved {
            eprintln!("phantom: {e}");
        }
        state.choice = choice;
    });
    apply(Bundle::Write);
}

impl State {
    fn load() -> Option<State> {
        let bundle = bundle_of(&std::env::current_exe().ok()?)?;
        let resources = bundle.join("Contents/Resources");
        let manifest = Manifest::parse(&std::fs::read_to_string(resources.join("app-icons.json")).ok()?)?;
        let prefs = Prefs::at(proto::data_dir().join("gui.json"));
        let choice = prefs.get(SETTING);
        Some(State { prefs, manifest, resources, bundle, choice, dock: None })
    }

    fn apply(&mut self, dark: bool, bundle: Bundle) {
        let target = resolve(self.choice.as_ref(), &self.manifest, THEME, dark);
        let id = target.map(|icon| icon.id.clone());
        // The bundle may lag the Dock (an Appearance change only swaps the Dock), so a write
        // goes ahead even when the Dock already shows the icon.
        if id == self.dock && bundle == Bundle::Keep {
            return;
        }
        let image = match target {
            None => None,
            Some(icon) => match image(&self.resources.join(&icon.file)) {
                Some(image) => Some(image),
                None => return eprintln!("phantom: can't read the app icon {}", icon.file.display()),
            },
        };
        if id != self.dock {
            set_dock_icon(image.as_deref());
            self.dock = id;
        }
        if bundle == Bundle::Write {
            update_bundle(&mut self.prefs, &self.bundle, target, image.as_deref());
        }
    }
}

/// Whether [`apply`] also writes the bundle icon, or only swaps the Dock's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bundle {
    Write,
    Keep,
}

fn apply(bundle: Bundle) {
    let dark = is_dark();
    STATE.with_borrow_mut(|state| {
        if let Some(state) = state {
            state.apply(dark, bundle);
        }
    });
}

/// Callbacks from AppKit can't unwind.
fn guarded(f: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        eprintln!("phantom: app icon handler panicked");
    }
}

/// Sets, keeps or resets the bundle's custom icon, remembering in `prefs` the one Phantom wrote.
fn update_bundle(prefs: &mut Prefs, bundle: &Path, target: Option<&Icon>, image: Option<&NSImage>) {
    let data = custom_icon_data(bundle);
    let current = std::fs::read(&data).ok().map(|bytes| fingerprint(&bytes));
    let written: Option<Written> = prefs.get(WRITTEN);
    let saved = match bundle_step(target, current, written.as_ref()) {
        BundleStep::Leave => return,
        BundleStep::Set(icon) => set_bundle_icon(bundle, image).then(|| {
            let fingerprint = fingerprint(&std::fs::read(&data).unwrap_or_default());
            prefs.set(WRITTEN, &Written { id: icon.id.clone(), fingerprint })
        }),
        BundleStep::Reset => set_bundle_icon(bundle, None).then(|| prefs.remove(WRITTEN)),
    };
    match saved {
        None => eprintln!("phantom: macOS refused to change the icon of {}", bundle.display()),
        Some(Err(e)) => eprintln!("phantom: {e}"),
        Some(Ok(())) => {}
    }
}

fn is_dark() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    // SAFETY: AppKit's constant names, read-only.
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = NSApplication::sharedApplication(mtm).effectiveAppearance().bestMatchFromAppearancesWithNames(&names);
    // SAFETY: as above.
    best.is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua })
}

/// An `NSImage` of the file; `None` if AppKit can't read it.
fn image(path: &Path) -> Option<Retained<NSImage>> {
    NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path.to_str()?))
}

/// `None` gives the Dock back the bundle's icon.
// Not `rune::set_app_icon`, which can't take `None`; replace with Rune's own support (#70).
fn set_dock_icon(image: Option<&NSImage>) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    // SAFETY: `nil` is documented to restore the bundle's icon.
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(image) };
}

/// `None` removes the custom icon: both the `Icon\r` file and the custom-icon flag.
fn set_bundle_icon(bundle: &Path, image: Option<&NSImage>) -> bool {
    let path = NSString::from_str(&bundle.to_string_lossy());
    NSWorkspace::sharedWorkspace().setIcon_forFile_options(image, &path, NSWorkspaceIconCreationOptions::empty())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rune::prefs::Prefs;

    use super::super::{Icon, WRITTEN, Written, custom_icon_data};
    use super::{image, set_bundle_icon, update_bundle};

    fn icns(id: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../design/icons/app/{id}.icns"))
    }

    fn icon(id: &str) -> Icon {
        Icon { id: id.into(), file: format!("{id}.icns").into() }
    }

    /// The app changes icons on the main thread only; concurrent `setIcon`s can fail.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// An empty `.app` in a temp dir, removed on drop: the only bundle these tests touch.
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

    #[test]
    fn phantom_writes_the_bundle_icon_once_and_resets_only_its_own() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let bundle = Bundle::new("reset");
        let mut prefs = Prefs::memory();
        let dark_hour = image(&icns("dark-hour")).expect("the .icns reads");

        update_bundle(&mut prefs, &bundle.0, Some(&icon("dark-hour")), Some(&dark_hour));
        assert!(bundle.has_custom_icon());
        let written: Written = prefs.get(WRITTEN).expect("remembered");
        assert_eq!(written.id, "dark-hour");

        // The same icon again leaves the file alone.
        let before = std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap();
        update_bundle(&mut prefs, &bundle.0, Some(&icon("dark-hour")), Some(&dark_hour));
        assert_eq!(std::fs::metadata(custom_icon_data(&bundle.0)).unwrap().modified().unwrap(), before);

        update_bundle(&mut prefs, &bundle.0, None, None);
        assert!(!bundle.has_custom_icon());
        assert!(!bundle.0.join("Icon\r").exists());
        assert!(prefs.get::<Written>(WRITTEN).is_none());
    }

    #[test]
    fn the_users_own_finder_icon_survives_a_reset() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let bundle = Bundle::new("users");
        let mut prefs = Prefs::memory();
        update_bundle(&mut prefs, &bundle.0, Some(&icon("dark-hour")), image(&icns("dark-hour")).as_deref());
        // The user pastes their own icon in Finder.
        assert!(set_bundle_icon(&bundle.0, image(&icns("full-moon")).as_deref()));
        update_bundle(&mut prefs, &bundle.0, None, None);
        assert!(bundle.has_custom_icon());
    }
}
