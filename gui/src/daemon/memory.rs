//! An in-memory stand-in for the daemon, for headless runs and tests. Answers on the app clock:
//! magnets resolve `resolve_after` seconds after being added, downloads progress at their speed.

use std::time::Duration;

use proto::{Listing, PendingAdd, PendingId, TorrentId, TorrentInfo, TorrentState};

use super::{AddRequest, Connection, Daemon, Event, Notice};

const MIB: u64 = 1024 * 1024;
const NEW_TORRENT_SIZE: u64 = 700 * MIB;
const DOWNLOAD_SPEED: u64 = 20 * MIB;
const SEED_SPEED: u64 = MIB;

pub struct MemoryDaemon {
    listing: Listing,
    timings: Vec<Timing>,
    resolve_after: f64,
    next_torrent: TorrentId,
    next_pending: PendingId,
    last_poll: Option<f64>,
    events: Vec<Event>,
    changed: bool,
}

/// A pending add's clock. Actions don't see the app clock, so `added_at` is filled in by the
/// first poll after the add.
struct Timing {
    id: PendingId,
    added_at: Option<f64>,
    initial_elapsed: u64,
    resolve_after: f64,
}

impl Timing {
    fn new(id: PendingId, initial_elapsed: u64, resolve_after: f64) -> Self {
        Self { id, added_at: None, initial_elapsed, resolve_after }
    }

    fn elapsed(&self, now: f64) -> f64 {
        self.added_at.map_or(0.0, |at| now - at)
    }
}

impl Default for MemoryDaemon {
    fn default() -> Self {
        Self {
            listing: Listing::default(),
            timings: Vec::new(),
            resolve_after: 2.0,
            next_torrent: 0,
            next_pending: 0,
            last_poll: None,
            events: vec![Event::Connection(Connection::Connected)],
            changed: true,
        }
    }
}

impl MemoryDaemon {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seconds from adding a magnet or URL until it becomes a torrent; `f64::INFINITY` never.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn resolve_after(mut self, secs: f64) -> Self {
        self.resolve_after = secs;
        self
    }

    pub fn with_torrents(mut self, torrents: Vec<TorrentInfo>) -> Self {
        self.next_torrent = torrents.iter().map(|t| t.id + 1).max().unwrap_or(0).max(self.next_torrent);
        self.listing.torrents.extend(torrents);
        self
    }

    pub fn with_pending(mut self, pending: Vec<PendingAdd>) -> Self {
        self.next_pending = pending.iter().map(|p| p.id + 1).max().unwrap_or(0).max(self.next_pending);
        // Seeded adds keep waiting, as in a screenshot of a slow magnet.
        self.timings.extend(pending.iter().map(|p| Timing::new(p.id, p.elapsed_secs, f64::INFINITY)));
        self.listing.pending.extend(pending);
        self
    }

    fn torrent_mut(&mut self, id: TorrentId) -> Option<&mut TorrentInfo> {
        self.listing.torrents.iter_mut().find(|t| t.id == id)
    }

    fn new_torrent(&mut self, name: Option<String>) -> TorrentInfo {
        let id = self.next_torrent;
        self.next_torrent += 1;
        TorrentInfo {
            id,
            info_hash: format!("{id:040x}"),
            name,
            state: TorrentState::Live,
            finished: false,
            error: None,
            progress_bytes: 0,
            total_bytes: NEW_TORRENT_SIZE,
            uploaded_bytes: 0,
            download_bytes_per_sec: DOWNLOAD_SPEED,
            upload_bytes_per_sec: 0,
            live_peers: 12,
        }
    }

    fn advance(&mut self, now: f64, dt: f64) {
        for t in &mut self.listing.torrents {
            if t.state != TorrentState::Live {
                continue;
            }
            if !t.finished {
                t.progress_bytes = (t.progress_bytes + (t.download_bytes_per_sec as f64 * dt) as u64).min(t.total_bytes);
                if t.progress_bytes == t.total_bytes {
                    t.finished = true;
                    t.download_bytes_per_sec = 0;
                    t.upload_bytes_per_sec = SEED_SPEED;
                }
            }
            t.uploaded_bytes += (t.upload_bytes_per_sec as f64 * dt) as u64;
            self.changed = true;
        }

        for timing in &mut self.timings {
            timing.added_at.get_or_insert(now - timing.initial_elapsed as f64);
        }
        let due: Vec<PendingId> = self.timings.iter().filter(|t| t.elapsed(now) >= t.resolve_after).map(|t| t.id).collect();
        for id in due {
            self.timings.retain(|t| t.id != id);
            let Some(i) = self.listing.pending.iter().position(|p| p.id == id) else { continue };
            let pending = self.listing.pending.remove(i);
            let torrent = self.new_torrent(pending.name);
            self.listing.torrents.push(torrent);
            self.changed = true;
        }
        for p in &mut self.listing.pending {
            let Some(timing) = self.timings.iter().find(|t| t.id == p.id) else { continue };
            let elapsed = timing.elapsed(now) as u64;
            self.changed |= elapsed != p.elapsed_secs;
            p.elapsed_secs = elapsed;
        }
    }
}

impl Daemon for MemoryDaemon {
    fn add(&mut self, source: AddRequest) {
        match source {
            AddRequest::File(path) => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                let torrent = self.new_torrent(name);
                self.events.push(Event::Notice(Notice::info(format!("Added {}", torrent.name.as_deref().unwrap_or("torrent")))));
                self.listing.torrents.push(torrent);
            }
            AddRequest::Url(url) => {
                let Some(name) = url_name(&url) else {
                    self.events.push(Event::Notice(Notice::error("provided path is not a valid magnet URL")));
                    return;
                };
                let id = self.next_pending;
                self.next_pending += 1;
                self.listing.pending.push(PendingAdd { id, name, source: url, elapsed_secs: 0 });
                self.timings.push(Timing::new(id, 0, self.resolve_after));
            }
        }
        self.changed = true;
    }

    fn pause(&mut self, ids: &[TorrentId]) {
        for &id in ids {
            if let Some(t) = self.torrent_mut(id) {
                t.state = TorrentState::Paused;
                t.download_bytes_per_sec = 0;
                t.upload_bytes_per_sec = 0;
                t.live_peers = 0;
            }
        }
        self.changed = true;
    }

    fn resume(&mut self, ids: &[TorrentId]) {
        for &id in ids {
            if let Some(t) = self.torrent_mut(id) {
                t.state = TorrentState::Live;
                (t.download_bytes_per_sec, t.upload_bytes_per_sec) =
                    if t.finished { (0, SEED_SPEED) } else { (DOWNLOAD_SPEED, 0) };
                t.live_peers = 12;
            }
        }
        self.changed = true;
    }

    fn remove(&mut self, ids: &[TorrentId], _delete_files: bool) {
        self.listing.torrents.retain(|t| !ids.contains(&t.id));
        self.changed = true;
    }

    fn cancel_adds(&mut self, ids: &[PendingId]) {
        self.listing.pending.retain(|p| !ids.contains(&p.id));
        self.timings.retain(|t| !ids.contains(&t.id));
        self.events.push(Event::Notice(Notice::info("Stopped adding")));
        self.changed = true;
    }

    fn poll(&mut self, now: f64) -> Vec<Event> {
        let dt = self.last_poll.map_or(0.0, |last| (now - last).max(0.0));
        self.last_poll = Some(now);
        self.advance(now, dt);
        if std::mem::take(&mut self.changed) {
            self.events.push(Event::Listing(self.listing.clone()));
        }
        std::mem::take(&mut self.events)
    }

    fn in_flight(&self) -> usize {
        0
    }

    fn next_wake(&self) -> Option<Duration> {
        let active = !self.listing.pending.is_empty() || self.listing.torrents.iter().any(|t| t.state == TorrentState::Live);
        active.then_some(Duration::from_millis(500))
    }
}

/// The display name a URL implies, as the real daemon reports it: a magnet's `dn=`, else a
/// URL's file name. `None` for something that is neither (the real daemon rejects those too);
/// `Some(None)` for a valid source with no name.
fn url_name(url: &str) -> Option<Option<String>> {
    if let Some(query) = url.strip_prefix("magnet:?") {
        let dn = query.split('&').find_map(|kv| kv.strip_prefix("dn="));
        return Some(dn.map(percent_decode));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
        return Some(file.map(|f| f.trim_end_matches(".torrent").to_owned()));
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |i: usize| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], hex(i)) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
