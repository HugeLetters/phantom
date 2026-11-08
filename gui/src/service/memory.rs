//! An in-memory stand-in for the service, for headless runs and tests. Answers on the app clock:
//! a magnet or URL fetches info for `info_after` seconds after being added, then downloads at its
//! speed. Adds are de-duplicated by the magnet's info hash, as the real service does. Files
//! download in order, those set Off not at all, and leave the torrent's Size, as with the real one.
//! A torrent can be seeded with files missing ([`with_missing`](MemoryService::with_missing)): it's
//! Missing files until they're set Off, or until Resume or Check, which check for `CHECK_SECS`
//! and then download them again. A torrent's destination can be away
//! ([`with_destination_away`](MemoryService::with_destination_away)): it's Destination unavailable,
//! refusing Resume, Check and Delete, until it's back; then it's checked and runs again if it was
//! running. Adds go into the destination they name, or the default destination
//! ([`with_default_destination`](MemoryService::with_default_destination), `~/Downloads` unless
//! told otherwise); without either they're refused, as by the real service.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use proto::{AbsolutePath, Action, Added, Details, File, Priority, Refusal, Status, TorrentId, TorrentInfo, Unavailable};

use super::{AddRequest, Connection, Event, Notice, Service};

const MIB: u64 = 1024 * 1024;
const NEW_TORRENT_SIZE: u64 = 700 * MIB;
const DOWNLOAD_SPEED: u64 = 20 * MIB;
const SEED_SPEED: u64 = MIB;
/// How long a check takes.
const CHECK_SECS: f64 = 1.0;

pub struct MemoryService {
    torrents: Vec<TorrentInfo>,
    /// Each torrent's files, once its info is in.
    files: BTreeMap<TorrentId, Vec<File>>,
    watching: Option<TorrentId>,
    timings: Vec<Timing>,
    checks: Vec<Check>,
    info_after: f64,
    next_id: TorrentId,
    last_poll: Option<f64>,
    events: Vec<Event>,
    changed: bool,
    default_destination: Option<PathBuf>,
    /// Each torrent's destination, unless it's the default one.
    destinations: BTreeMap<TorrentId, PathBuf>,
    away: Vec<Away>,
}

/// A torrent whose destination is away. `since` is filled in by the first poll, as for [`Timing`].
struct Away {
    id: TorrentId,
    reason: Unavailable,
    /// It runs again once its destination is back.
    running: bool,
    back_after: f64,
    since: Option<f64>,
}

/// When a torrent fetching info gets it. Actions don't see the app clock, so `added_at` is
/// filled in by the first poll after the add.
struct Timing {
    id: TorrentId,
    added_at: Option<f64>,
    info_after: f64,
}

/// A torrent being checked. `started_at` is filled in by the first poll, as for [`Timing`].
struct Check {
    id: TorrentId,
    started_at: Option<f64>,
    /// It runs afterwards.
    run: bool,
}

impl Default for MemoryService {
    fn default() -> Self {
        Self {
            torrents: Vec::new(),
            files: BTreeMap::new(),
            watching: None,
            timings: Vec::new(),
            checks: Vec::new(),
            info_after: 2.0,
            next_id: 0,
            last_poll: None,
            events: vec![Event::Connection(Connection::Connected)],
            changed: true,
            default_destination: Some(downloads()),
            destinations: BTreeMap::new(),
            away: Vec::new(),
        }
    }
}

impl MemoryService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seconds from adding a magnet or URL until its info arrives; `f64::INFINITY` never.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn info_after(mut self, secs: f64) -> Self {
        self.info_after = secs;
        self
    }

    /// Seeded torrents fetching info keep fetching, as in a screenshot of a slow magnet. The
    /// others are one file, named after the torrent, until [`with_files`](Self::with_files).
    pub fn with_torrents(mut self, torrents: Vec<TorrentInfo>) -> Self {
        self.next_id = torrents.iter().map(|t| t.id + 1).max().unwrap_or(0).max(self.next_id);
        for mut t in torrents {
            if t.status != Status::FetchingInfo {
                let name = t.name.clone().unwrap_or_default();
                let file = File {
                    index: 0,
                    path: name.into(),
                    size_bytes: t.size_bytes,
                    progress_bytes: t.progress_bytes,
                    priority: Priority::Normal,
                    missing: false,
                };
                sync(&mut t, &[file.clone()]);
                self.files.insert(t.id, vec![file]);
            }
            self.torrents.push(t);
        }
        self
    }

    /// Gives torrent `id` these files; its Size and progress follow from them.
    pub fn with_files(mut self, id: TorrentId, files: Vec<File>) -> Self {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            sync(t, &files);
            self.files.insert(id, files);
        }
        self
    }

    /// The files of torrent `id` at `indices` went missing from disk: it stops and is Missing
    /// files, and they show as missing with nothing downloaded, as with the real service.
    pub fn with_missing(mut self, id: TorrentId, indices: &[usize]) -> Self {
        if let (Some(t), Some(files)) = (self.torrents.iter_mut().find(|t| t.id == id), self.files.get_mut(&id)) {
            for f in files.iter_mut().filter(|f| indices.contains(&f.index) && f.priority == Priority::Normal) {
                f.missing = true;
                f.progress_bytes = 0;
            }
            sync(t, files);
            stop(t, Status::MissingFiles);
        }
        self
    }

    /// Where adds that name no destination go; `None`: they're refused, so the app asks first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_default_destination(mut self, folder: Option<PathBuf>) -> Self {
        self.default_destination = folder;
        self
    }

    /// Torrent `id`'s destination can't be reached, for `reason`, until `back_after` seconds
    /// from now (`f64::INFINITY`: never): it stops and is Destination unavailable.
    pub fn with_destination_away(mut self, id: TorrentId, reason: Unavailable, back_after: f64) -> Self {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            let running = matches!(t.status, Status::Downloading | Status::Seeding | Status::Checking);
            stop(t, Status::DestinationUnavailable { reason: reason.clone() });
            self.checks.retain(|c| c.id != id);
            self.away.push(Away { id, reason, running, back_after, since: None });
        }
        self
    }

    fn away(&self, id: TorrentId) -> Option<&Away> {
        self.away.iter().find(|a| a.id == id)
    }

    /// Starts checking the torrent; it runs afterwards if `run`. Files that were missing are
    /// just to download now.
    fn start_check(&mut self, id: TorrentId, run: bool) {
        if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
            if let Some(files) = self.files.get_mut(&id) {
                files.iter_mut().for_each(|f| f.missing = false);
            }
            stop(t, Status::Checking);
            self.checks.retain(|c| c.id != id);
            self.checks.push(Check { id, started_at: None, run });
        }
    }

    fn details(&self, id: TorrentId) -> Option<Details> {
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let files = self.files.get(&id)?;
        let destination = self.destinations.get(&id).cloned().unwrap_or_else(downloads);
        let location = match &files[..] {
            [only] => destination.join(&only.path),
            _ => destination.join(t.name.as_deref().unwrap_or("Unnamed torrent")),
        };
        let location = AbsolutePath::new(location).ok()?;
        Some(Details { id, files: files.clone(), full_size_bytes: t.full_size_bytes, location })
    }

    fn torrent_mut(&mut self, id: TorrentId) -> Option<&mut TorrentInfo> {
        self.torrents.iter_mut().find(|t| t.id == id)
    }

    fn new_torrent(&mut self, name: Option<String>, info_hash: Option<String>) -> TorrentInfo {
        let id = self.next_id;
        self.next_id += 1;
        TorrentInfo {
            id,
            info_hash: info_hash.or_else(|| Some(format!("{id:040x}"))),
            name,
            status: Status::FetchingInfo,
            error: None,
            progress_bytes: 0,
            size_bytes: 0,
            full_size_bytes: 0,
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
        }
    }

    fn advance(&mut self, now: f64, dt: f64) {
        for t in &mut self.torrents {
            if !matches!(t.status, Status::Downloading | Status::Seeding) {
                continue;
            }
            if t.status == Status::Downloading {
                let files = self.files.entry(t.id).or_default();
                let mut got = (t.download_bytes_per_sec as f64 * dt) as u64;
                for f in files.iter_mut().filter(|f| f.priority == Priority::Normal) {
                    let more = got.min(f.size_bytes - f.progress_bytes);
                    f.progress_bytes += more;
                    got -= more;
                }
                sync(t, files);
                if t.progress_bytes == t.size_bytes {
                    t.status = Status::Seeding;
                    t.download_bytes_per_sec = 0;
                    t.upload_bytes_per_sec = SEED_SPEED;
                }
            }
            t.uploaded_bytes += (t.upload_bytes_per_sec as f64 * dt) as u64;
            self.changed = true;
        }

        for check in &mut self.checks {
            check.started_at.get_or_insert(now);
        }
        let (done, checking) = std::mem::take(&mut self.checks)
            .into_iter()
            .partition(|c| c.started_at.is_some_and(|at| now - at >= CHECK_SECS));
        self.checks = checking;
        for Check { id, run, .. } in done {
            if let (Some(t), Some(files)) = (self.torrents.iter_mut().find(|t| t.id == id), self.files.get_mut(&id)) {
                sync(t, files);
                let complete = t.progress_bytes == t.size_bytes;
                (t.status, t.download_bytes_per_sec, t.upload_bytes_per_sec) = match (run, complete) {
                    (true, false) => (Status::Downloading, DOWNLOAD_SPEED, 0),
                    (true, true) => (Status::Seeding, 0, SEED_SPEED),
                    (false, false) => (Status::Paused, 0, 0),
                    (false, true) => (Status::Done, 0, 0),
                };
                t.live_peers = if run { 12 } else { 0 };
                self.changed = true;
            }
        }

        for away in &mut self.away {
            away.since.get_or_insert(now);
        }
        let (back, away) = std::mem::take(&mut self.away)
            .into_iter()
            .partition(|a| a.since.is_some_and(|at| now - at >= a.back_after));
        self.away = away;
        for Away { id, running, .. } in back {
            self.start_check(id, running);
            self.changed = true;
        }

        for timing in &mut self.timings {
            timing.added_at.get_or_insert(now);
        }
        let due: Vec<TorrentId> = self
            .timings
            .iter()
            .filter(|t| t.added_at.is_some_and(|at| now - at >= t.info_after))
            .map(|t| t.id)
            .collect();
        self.timings.retain(|t| !due.contains(&t.id));
        for id in due {
            if let Some(t) = self.torrents.iter_mut().find(|t| t.id == id) {
                self.files.insert(id, start_downloading(t));
                self.changed = true;
            }
        }
    }

    /// Adds one torrent into `destination`, or the default destination.
    fn add_one(&mut self, source: AddRequest, destination: Option<AbsolutePath>) {
        let torrent = match source {
            AddRequest::File(path) => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                let mut torrent = self.new_torrent(name, None);
                let files = start_downloading(&mut torrent);
                self.files.insert(torrent.id, files);
                torrent
            }
            AddRequest::Url(url) => {
                let Some((name, info_hash)) = parse_url(&url) else {
                    self.events.push(Event::Notice(Notice::error("not a magnet link or .torrent URL")));
                    return;
                };
                if let Some(t) = self.torrents.iter().find(|t| info_hash.is_some() && t.info_hash == info_hash) {
                    let added = Added { id: t.id, name: t.name.clone(), already_existed: true };
                    self.events.push(Event::Added(added));
                    return;
                }
                let torrent = self.new_torrent(name, info_hash);
                self.timings.push(Timing { id: torrent.id, added_at: None, info_after: self.info_after });
                torrent
            }
        };
        self.events.push(Event::Added(Added { id: torrent.id, name: torrent.name.clone(), already_existed: false }));
        if let Some(folder) = destination.map(AbsolutePath::into_path_buf).or_else(|| self.default_destination.clone()) {
            self.destinations.insert(torrent.id, folder);
        }
        self.torrents.push(torrent);
        self.changed = true;
    }

    fn forget(&mut self, ids: &[TorrentId]) {
        self.away.retain(|a| !ids.contains(&a.id));
        self.destinations.retain(|id, _| !ids.contains(id));
        self.torrents.retain(|t| !ids.contains(&t.id));
        self.timings.retain(|t| !ids.contains(&t.id));
        self.checks.retain(|c| !ids.contains(&c.id));
        self.files.retain(|id, _| !ids.contains(id));
        self.changed = true;
    }
}

/// The torrent's info is in: one file, named after it. Returns its files.
fn start_downloading(t: &mut TorrentInfo) -> Vec<File> {
    let name = t.name.clone().unwrap_or_else(|| "Unnamed torrent".into());
    let files = vec![File {
        index: 0,
        path: name.into(),
        size_bytes: NEW_TORRENT_SIZE,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
    }];
    sync(t, &files);
    t.status = Status::Downloading;
    t.download_bytes_per_sec = DOWNLOAD_SPEED;
    t.live_peers = 12;
    files
}

/// Stopped, as `status`: nothing moves.
fn stop(t: &mut TorrentInfo, status: Status) {
    t.status = status;
    t.download_bytes_per_sec = 0;
    t.upload_bytes_per_sec = 0;
    t.live_peers = 0;
}

/// The torrent's Size, Full size and progress, from its files.
fn sync(t: &mut TorrentInfo, files: &[File]) {
    let normal = || files.iter().filter(|f| f.priority == Priority::Normal);
    t.size_bytes = normal().map(|f| f.size_bytes).sum();
    t.progress_bytes = normal().map(|f| f.progress_bytes).sum();
    t.full_size_bytes = files.iter().map(|f| f.size_bytes).sum();
}

/// The fake's default destination, unless told otherwise.
fn downloads() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from).join("Downloads")
}

/// The real service's refusal of `action` on a torrent whose destination is away, worded as
/// clients word it.
fn refused(id: TorrentId, action: Action, away: &Away) -> Event {
    let refusal = Refusal::TorrentDestinationUnavailable { id, action, reason: away.reason.clone() };
    Event::Notice(Notice::error(client::display::refusal(&refusal)))
}

impl Service for MemoryService {
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>) {
        if destination.is_none() && self.default_destination.is_none() {
            self.events.push(Event::NoDestination(sources));
            return;
        }
        for source in sources {
            self.add_one(source, destination.clone());
        }
    }

    fn pause(&mut self, ids: &[TorrentId]) {
        for &id in ids {
            // Stays stopped once its destination is back.
            if let Some(away) = self.away.iter_mut().find(|a| a.id == id) {
                away.running = false;
            }
            if let Some(t) = self.torrent_mut(id) {
                match t.status {
                    Status::Downloading => t.status = Status::Paused,
                    Status::Seeding => t.status = Status::Done,
                    _ => {}
                }
                t.download_bytes_per_sec = 0;
                t.upload_bytes_per_sec = 0;
                t.live_peers = 0;
            }
        }
        self.changed = true;
    }

    fn resume(&mut self, ids: &[TorrentId]) {
        for &id in ids {
            if let Some(away) = self.away(id) {
                self.events.push(refused(id, Action::Resume, away));
                break;
            }
            if self.torrent_mut(id).is_some_and(|t| t.status == Status::MissingFiles) {
                self.start_check(id, true);
                continue;
            }
            if let Some(t) = self.torrent_mut(id) {
                (t.status, t.download_bytes_per_sec, t.upload_bytes_per_sec) = match t.status {
                    Status::Paused => (Status::Downloading, DOWNLOAD_SPEED, 0),
                    Status::Done => (Status::Seeding, 0, SEED_SPEED),
                    _ => continue,
                };
                t.live_peers = 12;
            }
        }
        self.changed = true;
    }

    fn check(&mut self, ids: &[TorrentId]) {
        for &id in ids {
            if let Some(away) = self.away(id) {
                self.events.push(refused(id, Action::Check, away));
                break;
            }
            let Some(t) = self.torrent_mut(id) else { continue };
            let run = matches!(t.status, Status::Downloading | Status::Seeding);
            if matches!(t.status, Status::Downloading | Status::Seeding | Status::Paused | Status::Done | Status::MissingFiles) {
                self.start_check(id, run);
            }
        }
        self.changed = true;
    }

    fn remove(&mut self, ids: &[TorrentId]) {
        self.forget(ids);
    }

    /// As the real service's client, stops at the first refusal.
    fn delete(&mut self, ids: &[TorrentId]) {
        let refusal = ids.iter().position(|&id| self.away(id).is_some());
        let deleted = &ids[..refusal.unwrap_or(ids.len())];
        self.forget(deleted);
        if let Some(i) = refusal {
            let away = self.away(ids[i]).expect("found");
            self.events.push(refused(ids[i], Action::Delete, away));
        }
    }

    fn watch_details(&mut self, id: Option<TorrentId>) {
        self.watching = id;
        self.changed = true;
    }

    fn set_priority(&mut self, id: TorrentId, indices: &[usize], priority: Priority) {
        let (Some(t), Some(files)) = (self.torrents.iter_mut().find(|t| t.id == id), self.files.get_mut(&id)) else {
            self.events.push(Event::Notice(Notice::error(format!("torrent {id} has no info yet"))));
            return;
        };
        for f in files.iter_mut().filter(|f| indices.contains(&f.index)) {
            f.priority = priority;
            // Off clears a missing file.
            f.missing &= priority == Priority::Normal;
        }
        sync(t, files);
        if t.status == Status::MissingFiles && !files.iter().any(|f| f.missing) {
            t.status = if t.progress_bytes == t.size_bytes { Status::Done } else { Status::Paused };
        }
        // A file set Normal again has more to download; one set Off may leave nothing.
        (t.status, t.download_bytes_per_sec, t.upload_bytes_per_sec) = match t.status {
            Status::Seeding if t.progress_bytes < t.size_bytes => (Status::Downloading, DOWNLOAD_SPEED, 0),
            Status::Downloading if t.progress_bytes == t.size_bytes => (Status::Seeding, 0, SEED_SPEED),
            Status::Done if t.progress_bytes < t.size_bytes => (Status::Paused, 0, 0),
            Status::Paused if t.progress_bytes == t.size_bytes => (Status::Done, 0, 0),
            _ => (t.status.clone(), t.download_bytes_per_sec, t.upload_bytes_per_sec),
        };
        self.changed = true;
    }

    fn poll(&mut self, now: f64) -> Vec<Event> {
        let dt = self.last_poll.map_or(0.0, |last| (now - last).max(0.0));
        self.last_poll = Some(now);
        self.advance(now, dt);
        if std::mem::take(&mut self.changed) {
            // Before the add outcomes, so the torrents they name are in the list.
            let details = self.watching.and_then(|id| self.details(id)).map(Event::Details);
            self.events.splice(0..0, [Event::Listing(self.torrents.clone())].into_iter().chain(details));
        }
        std::mem::take(&mut self.events)
    }

    fn next_wake(&self) -> Option<Duration> {
        let active = !self.timings.is_empty()
            || !self.checks.is_empty()
            || self.away.iter().any(|a| a.back_after.is_finite())
            || self.torrents.iter().any(|t| matches!(t.status, Status::Downloading | Status::Seeding));
        active.then_some(Duration::from_millis(500))
    }
}

/// The display name and info hash a URL implies, as the real service reports them: a magnet's
/// `dn=` and `xt=`, a bare info hash, else a URL's file name and no hash. `None` for something that is neither (the
/// real service rejects those too).
fn parse_url(url: &str) -> Option<(Option<String>, Option<String>)> {
    if let Some(query) = url.strip_prefix("magnet:?") {
        let param = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key));
        let info_hash = param("xt=urn:btih:")?.to_ascii_lowercase();
        return Some((param("dn=").map(percent_decode), Some(info_hash)));
    }
    if url.len() == 40 && url.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some((None, Some(url.to_ascii_lowercase())));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
        return Some((file.map(|f| f.trim_end_matches(".torrent").to_owned()), None));
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
