//! The system's open events reach Rune's hooks (`App::on_open_files`, `App::on_open_url`), at both
//! moments: the batch the app was launched with, dispatched before the event loop runs, and one
//! after launch. Real Apple Events (`'aevt'`/`'odoc'` for files, `'GURL'` for a `magnet:` link)
//! go through the real `NSAppleEventManager` of a real Rune event loop, with a hidden window of an
//! accessory app: no Dock icon, nothing shown. `harness = false`: AppKit wants the main thread.
//! That the app's own hooks are `platform::open_files` and `platform::open_url` is `main`'s
//! wiring, tried by `open_files.rs`.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
    println!("test result: ok. open events reach Rune's hooks at launch and after it");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{class, msg_send};
    use objc2_foundation::{NSString, NSURL};
    use rune::{App, AppSpec, Cx, El, Tx, WindowOptions, el};

    #[derive(Default)]
    struct Seen {
        files: Vec<Vec<PathBuf>>,
        urls: Vec<String>,
        /// The after-launch events are sent once the launch ones have arrived.
        sent_later: bool,
    }

    /// Where the hooks (plain `fn`s) put what they got, for `main` to judge after the loop.
    static LOG: Mutex<(Vec<Vec<PathBuf>>, Vec<String>)> = Mutex::new((Vec::new(), Vec::new()));
    const MAGNET_LAUNCH: &str = "magnet:?xt=urn:btih:0000000000000000000000000000000000000001";
    const MAGNET_LATER: &str = "magnet:?xt=urn:btih:0000000000000000000000000000000000000002";

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

    const fn code(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    /// Dispatches an event to the handler `NSAppleEventManager` has for it, in this process only.
    fn dispatch(class_code: u32, id: u32, direct_object: &AnyObject) {
        // SAFETY: Foundation calls; nil is a valid target; the raw descriptors live as long as the
        // events they come from.
        unsafe {
            let event: Retained<NSObject> = msg_send![class!(NSAppleEventDescriptor),
                appleEventWithEventClass: class_code, eventID: id, targetDescriptor: std::ptr::null::<AnyObject>(),
                returnID: -1i16, transactionID: 0i32];
            let _: () = msg_send![&event, setParamDescriptor: direct_object, forKeyword: code(b"----")];
            let reply: Retained<NSObject> = msg_send![class!(NSAppleEventDescriptor), nullDescriptor];
            let manager: Retained<NSObject> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
            let raw_event: *const AeDesc = msg_send![&event, aeDesc];
            let raw_reply: *const AeDesc = msg_send![&reply, aeDesc];
            let _: i16 = msg_send![&manager, dispatchRawAppleEvent: raw_event, withRawReply: raw_reply,
                handlerRefCon: std::ptr::null_mut::<c_void>()];
        }
    }

    fn open_documents(paths: &[&str]) {
        // SAFETY: Foundation constructors.
        unsafe {
            let list: Retained<NSObject> = msg_send![class!(NSAppleEventDescriptor), listDescriptor];
            for (i, path) in paths.iter().enumerate() {
                let url = NSURL::from_file_path(path).unwrap();
                let item: Retained<NSObject> = msg_send![class!(NSAppleEventDescriptor), descriptorWithFileURL: &*url];
                let _: () = msg_send![&list, insertDescriptor: &*item, atIndex: i as isize + 1];
            }
            dispatch(code(b"aevt"), code(b"odoc"), &list);
        }
    }

    fn open_link(link: &str) {
        // SAFETY: a Foundation constructor.
        unsafe {
            let string = NSString::from_str(link);
            let desc: Retained<NSObject> = msg_send![class!(NSAppleEventDescriptor), descriptorWithString: &*string];
            dispatch(code(b"GURL"), code(b"GURL"), &desc);
        }
    }

    /// Before the event loop runs the handlers are Rune's, put in by `run`: `init` is the first
    /// code that runs after `prepare`, before the window listens.
    fn init() -> Seen {
        open_documents(&["/tmp/launch.torrent"]);
        open_link(MAGNET_LAUNCH);
        Seen::default()
    }

    fn view(_: &Seen, _: &mut Cx<Seen>) -> El {
        el()
    }

    /// Every frame: once the launch events have come, send the later ones; once those too, quit.
    fn poll(s: &mut Seen, _: f64, tx: &mut Tx) -> bool {
        let (files, urls) = {
            let log = LOG.lock().unwrap();
            (log.0.len(), log.1.len())
        };
        if !s.sent_later && files >= 1 && urls >= 1 {
            s.sent_later = true;
            open_documents(&["/tmp/later.torrent"]);
            open_link(MAGNET_LATER);
        } else if s.sent_later && files >= 2 && urls >= 2 {
            tx.quit();
        }
        false
    }

    pub fn run() {
        // A loop that never gets its events must fail, not hang.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(20));
            let log = LOG.lock().unwrap();
            eprintln!("open events never all arrived: files {:?}, links {:?}", log.0, log.1);
            std::process::exit(1);
        });
        let options = WindowOptions { visible: false, ..WindowOptions::default() };
        let app = App::new(AppSpec { poll: Some(poll), ..AppSpec::new("open-events", (320.0, 200.0), init, view) })
            .window_options(options)
            .accessory()
            .on_open_files(|_: &mut Seen, paths, _| LOG.lock().unwrap().0.push(paths.to_vec()))
            .on_open_url(|_: &mut Seen, url, _| LOG.lock().unwrap().1.push(url.to_owned()));
        app.try_run().expect("the loop ran");
        let log = LOG.lock().unwrap();
        assert_eq!(log.0, [vec![PathBuf::from("/tmp/launch.torrent")], vec![PathBuf::from("/tmp/later.torrent")]], "files, in order");
        assert_eq!(log.1, [MAGNET_LAUNCH, MAGNET_LATER], "magnet links, in order");
    }
}
