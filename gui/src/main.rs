//! Phantom: the GUI client for the Phantom service. This file only wires the app together: the
//! Rune spec and its scenes, and which service the app talks to.
//!
//! `cargo build && target/debug/phantom-gui` opens the window (build the whole workspace so
//! `phantom-service` is up to date next to it). `scripts/install-mac.sh` installs Phantom.app.
//!
//! Headless runs get an in-memory service: `target/debug/phantom-gui --snap out.png --scene demo`.
//! To snapshot a real service instead, opt in and point it at a throwaway data dir:
//! `PHANTOM_HEADLESS_SERVICE=1 PHANTOM_DATA_DIR=/tmp/x target/debug/phantom-gui --snap out.png`.

mod model;
mod platform;
mod service;
#[cfg(test)]
mod tests;
mod ui;
mod views;

use proto::{File, Priority, Status, TorrentInfo};
use rune::{App, AppSpec, Key, KeyEvent, Tx};

use model::Phantom;
use service::{Connection, MemoryService, Notice, Service, SocketService};

/// Rune calls this before every frame: drain what the service sent since the last one.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    let events = s.service.poll(now);
    let changed = !events.is_empty();
    events.into_iter().for_each(|e| s.apply(e, tx));
    // Also after selections the table doesn't report, such as Cmd-A or a removed row.
    model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
    if let Some(after) = s.service.next_wake() {
        tx.wake_after(after);
    }
    changed
}

/// ⌘W closes the window and keeps downloading; ⌘Q quits (the service follows once no client is
/// left); Escape clears the selection, which closes the details pane.
fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Escape => s.table.clear_selection(),
        Key::Command('w') => tx.close_window(),
        Key::Command('q') => tx.quit(),
        _ => return false,
    }
    true
}

fn demo_service() -> MemoryService {
    let mib = 1024 * 1024;
    let t = |id, name: &str, status, progress: u64, size: u64, down: u64, up: u64, peers| TorrentInfo {
        id,
        info_hash: Some(format!("{id:040x}")),
        name: Some(name.into()),
        status,
        error: None,
        progress_bytes: progress,
        size_bytes: size,
        full_size_bytes: size,
        uploaded_bytes: 0,
        download_bytes_per_sec: down,
        upload_bytes_per_sec: up,
        live_peers: peers,
    };
    MemoryService::new().with_torrents(vec![
        t(0, "debian-13.7.0-amd64-netinst.iso", Status::Downloading, 412 * mib, 756 * mib, 68 * mib, 0, 32),
        t(1, "ubuntu-24.04.3-desktop-amd64.iso", Status::Seeding, 6100 * mib, 6100 * mib, 0, 3 * mib / 2, 7),
        t(2, "archlinux-2026.11.01-x86_64.iso", Status::Paused, 300 * mib, 1200 * mib, 0, 0, 0),
        t(3, "Sintel (4K)", Status::FetchingInfo, 0, 0, 0, 0, 0),
        t(4, "Big Buck Bunny (2008)", Status::Downloading, 0, 0, 24 * mib, 0, 18),
    ])
    .with_files(4, {
        let kib = 1024;
        let f = |index, path: &str, size, progress, priority| File { index, path: path.into(), size_bytes: size, progress_bytes: progress, priority, missing: false };
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

fn spec() -> AppSpec<Phantom> {
    AppSpec {
        on_key: Some(on_key),
        poll: Some(poll),
        min_size: Some((640.0, 320.0)),
        scenes: &[
            ("demo", |s| {
                s.service = Box::new(demo_service());
                s.table.select("0");
            }),
            ("details", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.pane.files.select("1");
            }),
            ("missing", |s| {
                s.service = Box::new(demo_service().with_missing(4, &[0]));
                s.table.select("4");
            }),
            ("notice", |s| {
                s.service = Box::new(demo_service());
                s.notice = Some(Notice::error("not a magnet link or .torrent URL"));
            }),
            ("lost", |s| s.connection = Connection::Lost("service did not start".into())),
        ],
        ..AppSpec::new("Phantom", (960.0, 540.0), || Phantom::new(Box::new(MemoryService::new())), views::view)
    }
}

fn main() {
    let app = App::new(spec()).keep_running().on_open_url(platform::open_url);
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
        Phantom { window: Some(window), ..Phantom::new(service) }
    })
    .run();
}
