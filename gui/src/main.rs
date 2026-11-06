//! Phantom: a GUI client for phantom-daemon.
//!
//! `cargo build && target/debug/phantom-gui` opens the window (build the whole workspace so
//! `phantom-daemon` is up to date next to it). `scripts/install-mac.sh` installs Phantom.app.
//!
//! Headless runs get an in-memory daemon: `target/debug/phantom-gui --snap out.png --scene demo`.
//! To snapshot a real daemon instead, opt in and point it at a throwaway data dir:
//! `PHANTOM_HEADLESS_DAEMON=1 PHANTOM_DATA_DIR=/tmp/x target/debug/phantom-gui --snap out.png`.

mod daemon;
#[cfg(test)]
mod tests;
mod view;

use std::path::PathBuf;

use proto::{Listing, PendingAdd, PendingId, TorrentId, TorrentInfo, TorrentState, format};
use rune::{App, AppSpec, Key, KeyEvent, TextEdit, Tx};
use rune_kit::{SortDir, TableState};

use daemon::{AddRequest, Connection, Daemon, Event, MemoryDaemon, Notice, SocketDaemon};

pub struct Phantom {
    daemon: Box<dyn Daemon>,
    torrents: Vec<TorrentInfo>,
    /// Adds the daemon is still resolving, shown above the torrents.
    pending: Vec<PendingAdd>,
    table: TableState,
    magnet: TextEdit,
    connection: Connection,
    notice: Option<Notice>,
}

impl Phantom {
    fn new(daemon: Box<dyn Daemon>) -> Self {
        Self {
            daemon,
            torrents: Vec::new(),
            pending: Vec::new(),
            table: TableState::new().multi_select(true),
            magnet: TextEdit::new(""),
            connection: Connection::Connecting,
            notice: None,
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Listing(Listing { torrents, pending }) => {
                self.pending = pending;
                self.torrents = torrents;
                if let Some((column, dir)) = self.table.sort() {
                    let column = column.to_owned();
                    sort(&mut self.torrents, &column, dir);
                }
            }
            Event::Connection(connection) => self.connection = connection,
            Event::Notice(notice) => self.notice = Some(notice),
        }
    }

    fn row_count(&self) -> usize {
        self.pending.len() + self.torrents.len()
    }
}

/// Rune calls this before every frame: drain what the daemon sent since the last one.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    let events = s.daemon.poll(now);
    let changed = !events.is_empty();
    events.into_iter().for_each(|e| s.apply(e));
    if let Some(after) = s.daemon.next_wake() {
        tx.wake_after(after);
    }
    changed
}

// Table rows are the pending adds (ids "p<n>") followed by the torrents (ids "<n>").

fn row_id(pending: &[PendingAdd], torrents: &[TorrentInfo], i: usize) -> String {
    match pending.get(i) {
        Some(p) => format!("p{}", p.id),
        None => torrents[i - pending.len()].id.to_string(),
    }
}

fn selected_torrents(table: &TableState) -> Vec<TorrentId> {
    table.selected().filter_map(|id| id.parse().ok()).collect()
}

fn selected_pending(table: &TableState) -> Vec<PendingId> {
    table.selected().filter_map(|id| id.strip_prefix('p')?.parse().ok()).collect()
}

/// Sends the field's URL to the daemon and empties the field; does nothing if it's blank.
fn submit_url(daemon: &mut dyn Daemon, field: &mut TextEdit) {
    let url = field.text.trim().to_owned();
    if !url.is_empty() {
        daemon.add(AddRequest::Url(url));
        field.clear();
    }
}

fn add_files(daemon: &mut dyn Daemon, files: &[PathBuf]) {
    files.iter().for_each(|f| daemon.add(AddRequest::File(f.clone())));
}

/// Removes the selected torrents and stops the selected pending adds, which have no files yet.
fn remove_selected(daemon: &mut dyn Daemon, table: &mut TableState, delete_files: bool) {
    let (torrents, pending) = (selected_torrents(table), selected_pending(table));
    if !torrents.is_empty() {
        daemon.remove(&torrents, delete_files);
    }
    if !pending.is_empty() {
        daemon.cancel_adds(&pending);
    }
    table.clear_selection();
}

fn sort(torrents: &mut [TorrentInfo], column: &str, dir: SortDir) {
    torrents.sort_by(|a, b| {
        dir.apply(match column {
            "name" => rune_kit::natural_cmp(a.name.as_deref().unwrap_or(""), b.name.as_deref().unwrap_or("")),
            "progress" => a.progress().total_cmp(&b.progress()),
            "status" => format::status(a).cmp(format::status(b)),
            "size" => a.total_bytes.cmp(&b.total_bytes),
            "down" => a.download_bytes_per_sec.cmp(&b.download_bytes_per_sec),
            "up" => a.upload_bytes_per_sec.cmp(&b.upload_bytes_per_sec),
            "peers" => a.live_peers.cmp(&b.live_peers),
            "eta" => a.eta().cmp(&b.eta()),
            _ => a.id.cmp(&b.id),
        })
    });
}

/// ⌘W closes the window and keeps downloading; ⌘Q quits (the daemon follows once no client is left).
fn on_key(_: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command('w') => tx.close_window(),
        Key::Command('q') => tx.quit(),
        _ => return false,
    }
    true
}

fn demo_daemon() -> MemoryDaemon {
    let mib = 1024 * 1024;
    let t = |id, name: &str, state, finished, progress: u64, total: u64, down: u64, up: u64, peers| TorrentInfo {
        id,
        info_hash: format!("{id:040x}"),
        name: Some(name.into()),
        state,
        finished,
        error: None,
        progress_bytes: progress,
        total_bytes: total,
        uploaded_bytes: 0,
        download_bytes_per_sec: down,
        upload_bytes_per_sec: up,
        live_peers: peers,
    };
    MemoryDaemon::new()
        .with_torrents(vec![
            t(0, "debian-13.7.0-amd64-netinst.iso", TorrentState::Live, false, 412 * mib, 756 * mib, 68 * mib, 0, 32),
            t(1, "ubuntu-24.04.3-desktop-amd64.iso", TorrentState::Live, true, 6100 * mib, 6100 * mib, 0, 3 * mib / 2, 7),
            t(2, "archlinux-2026.11.01-x86_64.iso", TorrentState::Paused, false, 300 * mib, 1200 * mib, 0, 0, 0),
        ])
        .with_pending(vec![PendingAdd {
            id: 0,
            name: Some("Sintel (4K)".into()),
            source: "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel+(4K)".into(),
            elapsed_secs: 42,
        }])
}

fn spec() -> AppSpec<Phantom> {
    AppSpec {
        on_key: Some(on_key),
        poll: Some(poll),
        min_size: Some((640.0, 320.0)),
        scenes: &[
            ("demo", |s| {
                s.daemon = Box::new(demo_daemon());
                s.table.select("0");
            }),
            ("notice", |s| {
                s.daemon = Box::new(demo_daemon());
                s.notice = Some(Notice::error("provided path is not a valid magnet URL"));
            }),
            ("lost", |s| s.connection = Connection::Lost("daemon did not start".into())),
        ],
        ..AppSpec::new("Phantom", (960.0, 540.0), || Phantom::new(Box::new(MemoryDaemon::new())), view::view)
    }
}

fn main() {
    let app = App::new(spec()).keep_running();
    let handle = app.handle();
    app.init(move || {
        let wake = move || handle.wake();
        let daemon: Box<dyn Daemon> = if rune::tooling::headless_run().is_none() {
            Box::new(SocketDaemon::start(wake))
        } else if std::env::var_os("PHANTOM_HEADLESS_DAEMON").is_some() {
            Box::new(SocketDaemon::start_loaded(wake))
        } else {
            Box::new(MemoryDaemon::new())
        };
        Phantom::new(daemon)
    })
    .run();
}
