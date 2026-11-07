//! Files the system opens with Phantom: Finder's double-click and Open With, files dropped on the
//! Dock icon, `open -a Phantom x.torrent`. macOS sends them as the open-documents Apple event
//! ('aevt'/'odoc'); the bundle declares the .torrent type (`scripts/install-mac.sh`).
//!
//! Rune doesn't forward that event yet, so Phantom installs its own handler, the way Rune does
//! for links (`open_url/incoming.rs`). Once Rune has a hook for it (anomalyco/rune#157), this
//! module goes and `main` passes [`super::open_files`] to the hook instead.
//!
//! AppKit puts its own 'odoc' handler in place as the app finishes launching, replacing one
//! installed earlier, and then dispatches the event that launched the app. So [`install`], called
//! before Rune's event loop starts, only listens for `NSApplicationWillFinishLaunching`, and the
//! handler goes in then: after AppKit's, before the launch event. Only 'odoc' is taken; get-URL
//! stays Rune's, and reopen and quit stay AppKit's.
//!
//! An event's files become one `AppHandle::update` running [`super::open_files`]. Rune queues
//! updates until the app's state exists, so the files that launched the app arrive too.

use std::path::PathBuf;
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObject, NSString};
use rune::AppHandle;

use crate::model::Phantom;

/// `'aevt'`: `kCoreEventClass`.
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
/// `'odoc'`: `kAEOpenDocuments`.
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
/// `'----'`: `keyDirectObject`, the event's list of files.
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

/// Where the handler sends each event's files.
static SINK: OnceLock<Box<dyn Fn(Vec<PathBuf>) + Send + Sync>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PhantomOpenDocuments"]
    struct Handler;

    impl Handler {
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn will_finish_launching(&self, _: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            // SAFETY: the selector is one of this object's methods, taking the event and the
            // reply; the object lives for the process (see `install`).
            unsafe {
                let _: () = msg_send![&manager, setEventHandler: self, andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: CORE_EVENT_CLASS, andEventID: OPEN_DOCUMENTS];
            }
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn open_documents(&self, event: Option<&NSAppleEventDescriptor>, _reply: Option<&NSAppleEventDescriptor>) {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let paths = event.map(paths_of).unwrap_or_default();
                if let (false, Some(sink)) = (paths.is_empty(), SINK.get()) {
                    sink(paths);
                }
            }));
            if caught.is_err() {
                eprintln!("phantom: open-documents handler panicked");
            }
        }
    }
);

/// Before Rune's event loop starts, on the main thread: files opened with Phantom go to its state
/// through `handle`. Does nothing elsewhere or if called again.
pub fn install(handle: AppHandle<Phantom>) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    if SINK.set(Box::new(move |paths| handle.update(move |s, tx| super::open_files(s, &paths, tx)))).is_err() {
        return;
    }
    // SAFETY: `init` on a fresh NSObject subclass with no ivars.
    let handler: Retained<Handler> = unsafe { msg_send![Handler::alloc(), init] };
    // SAFETY: never released, as neither the notification center nor the event manager retains
    // it: it lives for the process.
    let handler: &'static AnyObject = unsafe { &*Retained::into_raw(handler).cast::<AnyObject>() };
    // The constant's value is its name; it's AppKit's, which Phantom doesn't link for this alone.
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    // SAFETY: the selector is the handler's, taking the notification; the handler is never freed.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            handler,
            sel!(applicationWillFinishLaunching:),
            Some(&name),
            None,
        );
    }
}

/// The files of an open-documents event: its direct object, a list of file URLs or just one.
fn paths_of(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: returns the parameter or nil. (Called by message: objc2-foundation has it only with
    // its objc2-core-services feature, a whole framework's bindings for this one call.)
    let files: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
    let Some(files) = files else { return Vec::new() };
    let path = |d: &NSAppleEventDescriptor| d.fileURLValue().and_then(|url| url.to_file_path());
    match files.numberOfItems() {
        n if n >= 1 => (1..=n).filter_map(|i| files.descriptorAtIndex(i).as_deref().and_then(path)).collect(),
        _ => path(&files).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use objc2::{ClassType, msg_send};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSAppleEventDescriptor, NSURL};

    use super::{CORE_EVENT_CLASS, DIRECT_OBJECT, OPEN_DOCUMENTS, paths_of};

    /// An open-documents event, built but never sent.
    fn event() -> Retained<NSAppleEventDescriptor> {
        let none = std::ptr::null::<AnyObject>();
        // SAFETY: a Foundation constructor; nil is a valid target.
        unsafe {
            msg_send![NSAppleEventDescriptor::class(), appleEventWithEventClass: CORE_EVENT_CLASS, eventID: OPEN_DOCUMENTS,
                targetDescriptor: none, returnID: -1i16, transactionID: 0i32]
        }
    }

    fn set_files(event: &NSAppleEventDescriptor, files: &NSAppleEventDescriptor) {
        // SAFETY: both are descriptors.
        unsafe { msg_send![event, setParamDescriptor: files, forKeyword: DIRECT_OBJECT] }
    }

    fn file(path: &str) -> Retained<NSAppleEventDescriptor> {
        NSAppleEventDescriptor::descriptorWithFileURL(&NSURL::from_file_path(path).unwrap())
    }

    #[test]
    fn the_files_are_the_events_direct_object() {
        let event = event();
        assert!(paths_of(&event).is_empty(), "no direct object");
        let list = NSAppleEventDescriptor::listDescriptor();
        list.insertDescriptor_atIndex(&file("/tmp/a.torrent"), 1);
        list.insertDescriptor_atIndex(&file("/tmp/b c.torrent"), 2);
        set_files(&event, &list);
        assert_eq!(paths_of(&event), [PathBuf::from("/tmp/a.torrent"), PathBuf::from("/tmp/b c.torrent")]);
    }

    #[test]
    fn a_single_file_needs_no_list() {
        let event = event();
        set_files(&event, &file("/tmp/a.torrent"));
        assert_eq!(paths_of(&event), [PathBuf::from("/tmp/a.torrent")]);
    }
}
