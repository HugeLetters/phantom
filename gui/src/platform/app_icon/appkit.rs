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
//! AppKit is reached by message, as in [`super::super::open_documents`]: Rune links it already.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, class, define_class, msg_send, sel};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSObject, NSString};
use rune::prefs::Prefs;
use serde_json::Value;

use super::{AppIcon, BundleStep, Icon, Manifest, SETTING, THEME, WRITTEN, Written, bundle_of, bundle_step, custom_icon_data, fingerprint, resolve};

struct State {
    prefs: Prefs,
    manifest: Manifest,
    resources: PathBuf,
    bundle: PathBuf,
    choice: AppIcon,
    /// The icon last applied (`None` inside: the bundle's own); `None` before the first.
    shown: Option<Option<String>>,
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
                let key = NSString::from_str("effectiveAppearance");
                // SAFETY: the observer implements the KVO callback and lives for the process.
                unsafe {
                    let _: () = msg_send![&*application(), addObserver: self, forKeyPath: &*key, options: 0usize,
                        context: std::ptr::null_mut::<c_void>()];
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
    // SAFETY: `init` on a fresh NSObject subclass with no ivars.
    let observer: Retained<Observer> = unsafe { msg_send![Observer::alloc(), init] };
    // SAFETY: never released, as neither the notification center nor KVO retains it.
    let observer: &'static AnyObject = unsafe { &*Retained::into_raw(observer).cast::<AnyObject>() };
    let name = NSString::from_str("NSApplicationDidFinishLaunchingNotification");
    // SAFETY: the selector is the observer's, taking the notification; it's never freed.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            observer,
            sel!(applicationDidFinishLaunching:),
            Some(&name),
            None,
        );
    }
}

/// The picker's way in (#42): stores the choice and shows it. Main thread, after [`install`].
#[allow(dead_code)]
pub fn choose(choice: AppIcon) {
    STATE.with_borrow_mut(|state| {
        let Some(state) = state else { return };
        if let Err(e) = state.prefs.set(SETTING, &choice.encode()) {
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
        let choice = match std::env::var("PHANTOM_APP_ICON") {
            Ok(s) => AppIcon::parse(&s),
            Err(_) => prefs.get::<Value>(SETTING).map(|v| AppIcon::decode(&v)).unwrap_or_default(),
        };
        Some(State { prefs, manifest, resources, bundle, choice, shown: None })
    }

    fn apply(&mut self, dark: bool, bundle: Bundle) {
        let target = resolve(&self.choice, &self.manifest, THEME, dark);
        let id = target.map(|icon| icon.id.clone());
        // The bundle may lag the Dock (an Appearance change only swaps the Dock), so a write
        // goes ahead even when the Dock already shows the icon.
        let swap = self.shown.as_ref() != Some(&id);
        if !swap && bundle == Bundle::Keep {
            return;
        }
        let image = match target {
            None => None,
            Some(icon) => match image(&self.resources.join(&icon.file)) {
                Some(image) => Some(image),
                None => return eprintln!("phantom: can't read the app icon {}", icon.file.display()),
            },
        };
        // The Dock already shows the bundle's icon until Phantom sets another.
        if swap && (image.is_some() || self.shown.as_ref().is_some_and(Option::is_some)) {
            set_dock_icon(image.as_deref());
        }
        if bundle == Bundle::Write {
            update_bundle(&mut self.prefs, &self.bundle, target, image.as_deref());
        }
        self.shown = Some(id);
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
fn update_bundle(prefs: &mut Prefs, bundle: &Path, target: Option<&Icon>, image: Option<&AnyObject>) {
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

fn application() -> Retained<AnyObject> {
    // SAFETY: the shared application, created on first use.
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

fn is_dark() -> bool {
    // SAFETY: plain getters; the names are AppKit's constants, whose values are their names.
    unsafe {
        let appearance: Option<Retained<AnyObject>> = msg_send![&*application(), effectiveAppearance];
        let Some(appearance) = appearance else { return false };
        let names = NSArray::from_retained_slice(&[NSString::from_str("NSAppearanceNameAqua"), NSString::from_str("NSAppearanceNameDarkAqua")]);
        let best: Option<Retained<NSString>> = msg_send![&appearance, bestMatchFromAppearancesWithNames: &*names];
        best.is_some_and(|name| name.to_string() == "NSAppearanceNameDarkAqua")
    }
}

/// An `NSImage` of the file; `None` if AppKit can't read it.
fn image(path: &Path) -> Option<Retained<AnyObject>> {
    let path = NSString::from_str(path.to_str()?);
    // SAFETY: NSImage's own initializer; nil for a file it can't read.
    unsafe {
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        msg_send![image, initWithContentsOfFile: &*path]
    }
}

/// `None` gives the Dock back the bundle's icon.
fn set_dock_icon(image: Option<&AnyObject>) {
    // SAFETY: on the main thread (AppKit's callbacks), with an NSImage or nil.
    unsafe {
        let _: () = msg_send![&*application(), setApplicationIconImage: image];
    }
}

/// `None` removes the custom icon: both the `Icon\r` file and the custom-icon flag.
fn set_bundle_icon(bundle: &Path, image: Option<&AnyObject>) -> bool {
    let path = NSString::from_str(&bundle.to_string_lossy());
    // SAFETY: NSWorkspace may be used from any thread; an NSImage or nil.
    unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![&workspace, setIcon: image, forFile: &*path, options: 0usize]
    }
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
