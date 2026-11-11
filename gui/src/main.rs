//! Phantom: the GUI client for the Phantom service. This file only wires the app together: the
//! Rune spec and its scenes, and which service the app talks to.
//!
//! `cargo build && target/debug/phantom-gui` opens the window (build the whole workspace so
//! `phantom-service` is up to date next to it). `scripts/install-mac.sh` installs Phantom.app.
//!
//! Headless runs get an in-memory service: `target/debug/phantom-gui --snap out.png --scene demo`.
//! To snapshot a real service instead, opt in and point it at a throwaway data dir:
//! `PHANTOM_HEADLESS_SERVICE=1 PHANTOM_DATA_DIR=/tmp/x target/debug/phantom-gui --snap out.png`.

mod errors;
mod menu;
mod model;
mod notice;
mod platform;
mod service;
mod settings;
#[cfg(test)]
mod tests;
mod ui;
mod views;

use std::num::NonZeroU16;
use std::time::Instant;

use client::display;
use client::machine::{Step, Why};
use proto::{AbsolutePath, File, ListenPort, Priority, Settings, Status, TorrentInfo, Unavailable};
use rune::{App, AppSpec, ImageCache, Key, KeyEvent, MenuItem, Tx, Waker};

use model::settings::{AppIcon, SettingsSheet, Tab};
use model::{Phantom, add};
use notice::Notice;
use platform::app_icon::{Icons, Manifest};
use service::{AddRequest, Connection, DiskChange, Incoming, MemoryService, Service, SocketService};

/// Rune calls this before every frame: drain what the service sent since the last one, in order:
/// live data is applied, answers run.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    let incoming = s.service.poll(now);
    let changed = !incoming.is_empty() || s.add_note.as_ref().is_some_and(|n| n.alerting);
    for incoming in incoming {
        match incoming {
            Incoming::Live(live) => s.apply(live),
            Incoming::Answer(answer) => answer(s, tx),
        }
    }
    let listed = s.errors.pump(now, tx);
    // Also after selections the table doesn't report, such as Cmd-A or a removed row.
    model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
    model::close_empty_menu(s);
    let was_copied = s.copied.is_some();
    model::age_copy_feedback(&mut s.copied, now, tx);
    model::age_add_note(&mut s.add_note, now, tx);
    if let Some(after) = s.service.next_wake() {
        tx.wake_after(after);
    }
    changed || listed || was_copied
}

/// ⌘W closes the window and keeps downloading; ⌘Q quits (the service follows once no client is
/// left); ⌘, opens Settings, as the menu item does; Escape closes Settings, or else clears the
/// selection, which closes the details pane, and dismisses the notes about failed adds.
fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command(',') => model::settings::open(s),
        Key::Escape if s.settings.is_some() => model::settings::close(s),
        Key::Escape => {
            s.table.clear_selection();
            s.add_note = None;
            s.errors.escape_pressed();
        }
        Key::Command('w') => tx.close_window(),
        Key::Command('q') => tx.quit(),
        _ => return false,
    }
    true
}

const MIB: u64 = 1024 * 1024;

#[allow(clippy::too_many_arguments)]
fn t(id: u64, name: &str, status: Status, progress: u64, size: u64, down: u64, up: u64, peers: u32) -> TorrentInfo {
    TorrentInfo {
        id,
        info_hash: format!("{id:040x}"),
        name: Some(name.into()),
        status,
        progress_bytes: progress,
        size_bytes: size,
        full_size_bytes: size,
        uploaded_bytes: 0,
        download_bytes_per_sec: down,
        upload_bytes_per_sec: up,
        live_peers: peers,
        trackers: Vec::new(),
        location: None,
        actions: Vec::new(),
    }
}

fn demo_service() -> MemoryService {
    let mib = MIB;
    MemoryService::new()
        .with_torrents(vec![
            t(0, "debian-13.7.0-amd64-netinst.iso", Status::Downloading, 412 * mib, 756 * mib, 68 * mib, 0, 32),
            t(1, "ubuntu-24.04.3-desktop-amd64.iso", Status::Seeding, 6100 * mib, 6100 * mib, 0, 3 * mib / 2, 7),
            t(2, "archlinux-2026.11.01-x86_64.iso", Status::Paused, 300 * mib, 1200 * mib, 0, 0, 0),
            t(3, "Sintel (4K)", Status::FetchingInfo, 0, 0, 0, 0, 0),
            t(4, "Big Buck Bunny (2008)", Status::Downloading, 0, 0, 24 * mib, 0, 18),
        ])
        .with_files(4, {
            let kib = 1024;
            let f = |index, path: &str, size, progress, priority| File {
                index,
                path: path.into(),
                size_bytes: size,
                progress_bytes: progress,
                priority,
                missing: false,
            };
            vec![
                f(0, "Big Buck Bunny (2008) 1080p.mkv", 1430 * mib, 862 * mib, Priority::Normal),
                f(1, "Big Buck Bunny (2008) 2160p.mkv", 4210 * mib, 0, Priority::Off),
                f(2, "Making of/Interview with the director.mp4", 312 * mib, 0, Priority::Normal),
                f(3, "Making of/Storyboards.pdf", 38 * mib, 0, Priority::Normal),
                f(4, "Subtitles/Deutsch.srt", 51 * kib, 51 * kib, Priority::Normal),
                f(5, "Subtitles/English.srt", 48 * kib, 48 * kib, Priority::Normal),
                f(6, "Subtitles/Français.srt", 50 * kib, 50 * kib, Priority::Normal),
                f(7, "poster.jpg", 820 * kib, 820 * kib, Priority::Normal),
            ]
        })
}

fn slow_adds() -> MemoryService {
    let nobody = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";
    MemoryService::new()
        .with_torrents(vec![t(0, "debian-13.7.0-amd64-netinst.iso", Status::Seeding, 756 * MIB, 756 * MIB, 0, MIB, 4)])
        .adds_after(1.0, 3.0)
        .rejecting(nobody)
}

/// An app whose link is in `connection`: after what the fake service sent first, which says
/// it's connected.
fn connection(s: &mut Phantom, connection: Connection) {
    for incoming in s.service.poll(0.0) {
        if let Incoming::Live(live) = incoming {
            s.apply(live);
        }
    }
    s.connection = connection;
}

const NOBODY: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";

/// Files named `names` in Downloads, as dropped on the window; those starting with `bad` fail.
fn files(names: &[&str]) -> Vec<AddRequest> {
    names.iter().map(|name| AddRequest::File(format!("/Users/me/Downloads/{name}").into())).collect()
}

/// The demo list, adds answered after half a second, the magnet named Nobody and the files
/// named `bad…` failing.
fn failing_adds() -> MemoryService {
    let mut service = demo_service().adds_after(0.5, 0.5).rejecting(NOBODY).with_default_destination(Some("/srv/torrents".into()));
    for name in ["bad-1.torrent", "bad-2.torrent", "bad-3.torrent"] {
        service = service.rejecting(&format!("/Users/me/Downloads/{name}"));
    }
    service
}

/// The settings the fake service answers with, its port pinned and taken if `taken`.
fn demo_settings(taken: bool) -> Settings {
    let port = NonZeroU16::new(51413).unwrap();
    Settings {
        default_destination: Some(AbsolutePath::new("/Users/me/Downloads").unwrap()),
        listen_port: if taken { ListenPort::Pinned(port) } else { ListenPort::Picked(port) },
        listening_on: if taken { NonZeroU16::new(54321).unwrap() } else { port },
        upnp: true,
        reset: Vec::new(),
    }
}

/// The Settings sheet open on `tab`, as already answered.
fn settings_scene(s: &mut Phantom, tab: Tab, settings: Settings) {
    s.service = Box::new(demo_service());
    s.settings = Some(SettingsSheet::showing(tab, settings));
}

/// Decodes the App icons to draw; a headless run waits for them, so its frames show them.
fn setup(s: &mut Phantom, waker: Waker) {
    s.app_icon.load_images(&ImageCache::new(waker));
    if rune::tooling::headless_run().is_some() {
        for (_, image) in &s.app_icon.images {
            let _ = image.wait();
        }
    }
}

/// The App icons and the choice kept in `gui.json`; headless, nothing is kept.
fn app_icon() -> AppIcon {
    let manifest = Manifest::load(&platform::app_icon::manifest_dir());
    if rune::tooling::headless_run().is_some() {
        return AppIcon::new(manifest, None, |_| {});
    }
    let choice = rune::prefs::Prefs::at(proto::data_dir().join("gui.json")).get(platform::app_icon::SETTING);
    #[cfg(target_os = "macos")]
    return AppIcon::new(manifest, choice, platform::app_icon::appkit::choose);
    #[cfg(not(target_os = "macos"))]
    AppIcon::new(manifest, choice, |_| {})
}

fn spec() -> AppSpec<Phantom> {
    AppSpec {
        setup: Some(setup),
        on_key: Some(on_key),
        poll: Some(poll),
        min_size: Some((640.0, 320.0)),
        scenes: &[
            ("demo", |s| {
                s.service = Box::new(demo_service());
                s.table.select("0");
            }),
            // Nothing selected; a selected torrent's details come 2 s after it's selected.
            ("slow-details", |s| s.service = Box::new(demo_service().details_after(2.0))),
            ("details", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.pane.files.select("1");
            }),
            ("fetching", |s| {
                s.service = Box::new(demo_service());
                s.table.select("3");
            }),
            ("missing", |s| {
                s.service = Box::new(demo_service().with_missing(4, &[0]));
                s.table.select("4");
            }),
            ("unavailable", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                s.service = Box::new(demo_service().at(0.0, DiskChange::DestinationGone { id: 4, reason }));
                s.table.select("4");
            }),
            ("destination-back", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                let back = DiskChange::DestinationBack { id: 4 };
                s.service = Box::new(demo_service().at(0.0, DiskChange::DestinationGone { id: 4, reason }).at(2.0, back));
                s.table.select("4");
            }),
            // The missing file is put back 2 s in: checked, then running again, as with the real service.
            ("put-back", |s| {
                let back = DiskChange::FilesPutBack { id: 4, files: vec![0] };
                s.service = Box::new(demo_service().with_missing(4, &[0]).at(2.0, back));
                s.table.select("4");
            }),
            ("error", |s| {
                let message = "No space left on device (os error 28)".into();
                let failed = t(5, "Tears of Steel (2012)", Status::Error { message }, 210 * MIB, 3400 * MIB, 0, 0, 0);
                s.service = Box::new(demo_service().with_torrents(vec![failed]));
                s.table.select("5");
            }),
            ("notice", |s| {
                s.service = Box::new(demo_service());
                s.errors.report(Notice::error("Not a magnet link or .torrent URL"));
            }),
            ("settings-reset", |s| {
                s.service = Box::new(demo_service());
                s.notice = model::settings::reset_notice(&["app_icon".into()]);
            }),
            ("rejecting", |s| s.service = Box::new(MemoryService::new().rejecting_links())),
            // Adds take 1 s, a .torrent URL 3 s, as from a busy service; the magnet named Nobody fails.
            ("adds", |s| s.service = Box::new(slow_adds())),
            ("no-default", |s| s.service = Box::new(slow_adds().with_default_destination(None))),
            // The connection's states, as the status bar words them.
            ("connecting", |s| connection(s, Connection::Connecting)),
            ("replacing", |s| connection(s, Connection::Replacing { deadline: Instant::now(), step: Step::Act })),
            ("spawning", |s| connection(s, Connection::Spawning { deadline: Instant::now(), step: Step::Act })),
            ("lost", |s| connection(s, Connection::Lost { why: Why::DidNotStart, retry_at: Instant::now() })),
            ("outdated", |s| connection(s, Connection::Outdated)),
            // The list, ready for files to be dropped (scripts); `bad…` ones fail.
            ("outside-adds", |s| s.service = Box::new(failing_adds())),
            // Adds made outside the window, failing: one link, 2 of 5 files, two batches together.
            ("add-failed", |s| {
                s.service = Box::new(failing_adds());
                add(s, vec![AddRequest::Url(NOBODY.into())], Vec::new());
            }),
            ("add-failed-some", |s| {
                s.service = Box::new(failing_adds());
                add(
                    s,
                    files(&["Sintel.torrent", "bad-1.torrent", "Tears of Steel.torrent", "bad-2.torrent", "Elephants Dream.torrent"]),
                    Vec::new(),
                );
            }),
            ("add-failed-two", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                add(s, vec![AddRequest::Url(NOBODY.into())], Vec::new());
            }),
            // The error list with the message by Add open, and with more than fits.
            ("add-failed-and-add-note", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                s.add_note = Some(model::AddNote::about(&s.magnet, display::link_problem(client::LinkProblem::NotALink)));
            }),
            // Several entries of different shapes, enough to scroll.
            ("several-entries", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                add(s, vec![AddRequest::Url(NOBODY.into())], Vec::new());
                s.errors.report(Notice::error("Can't open Finder: no such application"));
                add(s, files(&["bad-1.torrent", "bad-2.torrent", "bad-3.torrent", "Tears of Steel.torrent"]), Vec::new());
                add(s, vec![AddRequest::Url(NOBODY.into())], Vec::new());
            }),
            ("many-errors", |s| {
                s.service = Box::new(failing_adds());
                for n in 0..12 {
                    add(s, files(&["bad-1.torrent", "bad-2.torrent"]), Vec::new());
                    s.errors.report(Notice::error(format!("Can't open Finder: attempt {n}")));
                }
            }),
            // The list, for scripts that open Settings; a folder on a drive that isn't connected
            // can't be the default.
            ("settings-script", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                s.service = Box::new(
                    demo_service()
                        .with_default_destination(Some("/Users/me/Downloads".into()))
                        .unreachable("/Volumes/MyDisk/Films", reason),
                );
            }),
            ("settings", |s| settings_scene(s, Tab::General, demo_settings(false))),
            ("settings-network", |s| settings_scene(s, Tab::Network, demo_settings(false))),
            // A pinned port another program has: a temporary one for now.
            ("settings-taken", |s| settings_scene(s, Tab::Network, demo_settings(true))),
            ("settings-refused", |s| {
                settings_scene(s, Tab::Network, demo_settings(false));
                s.settings.as_mut().unwrap().port = rune::TextEdit::new("70000");
                model::settings::submit_port(s);
            }),
            ("settings-no-default", |s| {
                settings_scene(s, Tab::General, Settings { default_destination: None, ..demo_settings(false) });
            }),
            ("settings-icons", |s| settings_scene(s, Tab::AppIcon, demo_settings(false))),
            ("settings-custom", |s| {
                settings_scene(s, Tab::AppIcon, demo_settings(false));
                s.settings.as_mut().unwrap().custom_pair = true;
                s.app_icon.choice = Some(Icons::Pair { dark: "calling-card".into(), light: "full-moon".into() });
            }),
        ],
        ..AppSpec::new("Phantom", (960.0, 540.0), || Phantom::new(Box::new(MemoryService::new())), views::view)
    }
}

fn main() {
    // Before anything reads `gui.json`; the notice shows once the window does.
    let reset = if rune::tooling::headless_run().is_none() { settings::repair(&proto::data_dir().join("gui.json")) } else { Ok(vec![]) };
    let settings = MenuItem::new("Settings…", |s: &mut Phantom, _| model::settings::open(s))
        .id("settings")
        .key_equivalent("cmd+comma".parse().expect("a shortcut"))
        .keys_first();
    let app = App::new(spec()).keep_running().on_open_url(platform::open_url).menu_item(settings);
    let handle = app.handle();
    #[cfg(target_os = "macos")]
    if rune::tooling::headless_run().is_none() {
        platform::open_documents::install(handle.clone());
        platform::app_icon::appkit::install();
    }
    app.init(move || {
        let window = handle.clone();
        let wake = move || handle.wake();
        let service: Box<dyn Service> = if rune::tooling::headless_run().is_none() {
            Box::new(SocketService::start(wake))
        } else if std::env::var_os("PHANTOM_HEADLESS_SERVICE").is_some() {
            Box::new(SocketService::start_loaded(wake))
        } else {
            Box::new(MemoryService::new())
        };
        let mut app = Phantom { window: Some(window), app_icon: app_icon(), ..Phantom::new(service) };
        // Headless runs keep the fake finder: nothing opens.
        if rune::tooling::headless_run().is_none() {
            app.finder = Box::new(platform::SystemFinder);
        }
        match &reset {
            Ok(reset) => app.notice = model::settings::reset_notice(reset),
            Err(e) => app.errors.report(Notice::error(format!("Can't read gui.json: {e}"))),
        }
        app
    })
    .run();
}
