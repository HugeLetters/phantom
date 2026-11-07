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

use proto::{Status, TorrentInfo};
use rune::{App, AppSpec, Key, KeyEvent, Tx};

use model::Phantom;
use service::{Connection, MemoryService, Notice, Service, SocketService};

/// Rune calls this before every frame: drain what the service sent since the last one.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    let events = s.service.poll(now);
    let changed = !events.is_empty();
    events.into_iter().for_each(|e| s.apply(e, tx));
    if let Some(after) = s.service.next_wake() {
        tx.wake_after(after);
    }
    changed
}

/// ⌘W closes the window and keeps downloading; ⌘Q quits (the service follows once no client is left).
fn on_key(_: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
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
    ])
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
