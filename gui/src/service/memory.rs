//! An in-memory stand-in for the service, for headless runs and tests. Answers on the app clock:
//! a magnet or URL fetches info for `info_after` seconds after being added, then downloads at its
//! speed. Adds are de-duplicated by the magnet's info hash, as the real service does. Files
//! download in order, those set Off not at all, and leave the torrent's Size, as with the real one.
//!
//! Each torrent runs the real service's own lifecycle (the shared `lifecycle` crate): what's
//! wanted and its phase, from which its Status and the [`Action`](proto::Action)s that do something to it now
//! follow. The fake only pretends to be what's around it: librqbit (running, stopped, or checking,
//! which takes `CHECK_SECS` and recreates the files that aren't there, empty) and the disk (each
//! file there or not, and its destination reachable or not), which it reports as the engine does,
//! as facts for the machine to decide on. Commands the machine says do nothing now are ignored.
//!
//! An actor without a thread: commands wait in its mailbox, with their [`Done`], until the next
//! [`poll`](Service::poll) on the app clock, which handles them in order and hands over what
//! came of them.
//!
//! What happens on disk outside Phantom is a [`DiskChange`], at a time on the app clock
//! ([`at`](MemoryService::at)) or at once through a [`Disk`] handle.
//!
//! Adds go into the destination they name, or the default destination
//! ([`with_default_destination`](MemoryService::with_default_destination), `~/Downloads` unless
//! told otherwise); without either they're refused, as by the real service.
//!
//! Its settings change as the real service's do, answered by the next poll: a default destination
//! [`unreachable`](MemoryService::unreachable) is refused, and a pinned port another program
//! [`has taken`](MemoryService::port_taken) is listened on through a temporary one.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lifecycle::disk::{Disk as Found, Expected, Stamp};
use lifecycle::{Do, Event, Lib as Librqbit, Observation, Phase, State, Wants};
use proto::{
    AbsolutePath, Added, Content, Details, File, Kept, KeptWhy, ListenPort, Priority, Refusal, Setting, Settings, Snapshot, Status,
    TorrentCommand, TorrentId, TorrentInfo, TorrentRefusal, Unavailable,
};

use anyhow::anyhow;
use client::Failure;

use super::{AddError, AddRequest, Connection, Done, Incoming, Live, Service, answer};

const MIB: u64 = 1024 * 1024;
const NEW_TORRENT_SIZE: u64 = 700 * MIB;
const DOWNLOAD_SPEED: u64 = 20 * MIB;
const SEED_SPEED: u64 = MIB;
/// How long a check takes.
const CHECK_SECS: f64 = 1.0;

pub struct MemoryService {
    torrents: Vec<TorrentInfo>,
    /// Each torrent's files, once its info is in, with what the pretend librqbit has of each.
    files: BTreeMap<TorrentId, Vec<File>>,
    /// Each torrent's lifecycle, pretend librqbit and disk.
    machines: BTreeMap<TorrentId, Torrent>,
    /// Commands sent and not handled yet, in the order they came.
    mailbox: Vec<Message>,
    /// The last file made on the pretend disk, so each one is another file.
    made: u64,
    /// The torrents whose details the snapshots carry.
    watching: Vec<TorrentId>,
    /// Seconds from [`Service::watch_details`] naming a torrent until its details come, as from a
    /// busy service.
    details_after: f64,
    /// When each watched torrent's details were asked for, until they come; filled in by the
    /// first poll after the ask, as for [`Timing`].
    details_asked: BTreeMap<TorrentId, Option<f64>>,
    /// The service's run: a restart is the next.
    epoch: u64,
    /// Changes in the torrents (the list and every torrent's details) this run, as the engine
    /// counts them: what's watched isn't one. Neither is it in the real service, whose snapshots
    /// have every watched torrent's details at once; the fake's slow details are its own, so their
    /// coming, or a new wait for some, is a change too. Otherwise one `(epoch, version, watching)`
    /// would be sent with and without them, against the protocol (#148).
    version: u64,
    /// The torrents as of `version`, and the watched ones whose details are still on their way.
    published: Option<(Vec<TorrentInfo>, Vec<Details>, BTreeSet<TorrentId>)>,
    /// Seconds a command's own snapshot and answer take, in turn: as with the real service, whose
    /// commands each run on a connection of their own, so they can land after newer snapshots
    /// from the stream, and out of order.
    stale_answers: f64,
    /// How long Delete's trashing takes; the torrents (by name) whose files the Trash refuses;
    /// and the trashing under way.
    trash_secs: f64,
    trash_refusing: BTreeSet<String>,
    trashing: Vec<Trashing>,
    /// Commands handled, to take the next of [`STALE`]'s delays.
    commands: usize,
    /// Commands' snapshots and answers on their way.
    late: Vec<Late>,
    /// What tests read of the latest snapshot streamed.
    probe: Probe,
    /// Every snapshot sent, by `(epoch, version, watching)`: the protocol says those fix it, as
    /// the real service's `for_client` does, and the app relies on it.
    #[cfg(test)]
    sent: BTreeMap<(u64, u64, Vec<TorrentId>), Snapshot>,
    /// The service's settings reset at its start, in every snapshot.
    reset: Vec<String>,
    timings: Vec<Timing>,
    info_after: f64,
    /// Every add of a link fails, as one of a magnet nobody has would.
    rejecting_links: bool,
    /// Adds of these links fail.
    rejecting: BTreeSet<String>,
    /// Seconds an add takes to be answered: of a magnet, info hash or file, and of a .torrent
    /// URL, which the real service downloads first.
    add_secs: f64,
    torrent_url_secs: f64,
    /// Adds sent and not done yet, in the order they came: each is done, and answered, once its
    /// time on the app clock has passed.
    adds: Vec<PendingAdd>,
    next_id: TorrentId,
    last_poll: Option<f64>,
    /// What the next poll hands over, after the listing: answers, in the order they were asked.
    incoming: Vec<Incoming>,
    changed: bool,
    default_destination: Option<PathBuf>,
    listen_port: ListenPort,
    upnp: bool,
    /// Folders a default destination can't be set to, and why.
    unreachable: BTreeMap<PathBuf, Unavailable>,
    /// Ports another program listens on.
    taken: BTreeSet<NonZeroU16>,
    /// A new port or UPnP fails, as when the torrents don't start again with them.
    restarts_fail: bool,
    /// Each torrent's destination, unless it's the default one.
    destinations: BTreeMap<TorrentId, PathBuf>,
    disk: Disk,
}

/// A command in the mailbox: for these torrents, answered through `done` with those that failed.
struct Message {
    ids: Vec<TorrentId>,
    command: TorrentCommand,
    done: Done<Vec<(TorrentId, Failure)>>,
}

/// A Delete's pretend trashing, due at a time on the app clock: the torrents it moves, the
/// command's failures so far, and its answer, which waits for it.
struct Trashing {
    due: f64,
    ids: Vec<TorrentId>,
    failed: Vec<(TorrentId, Failure)>,
    done: Done<Vec<(TorrentId, Failure)>>,
}

/// A torrent as the fake runs it: the shared lifecycle, and what's around it.
struct Torrent {
    state: State,
    librqbit: Pretend,
    /// What's at each file's path, by index: `None`, nothing.
    on_disk: BTreeMap<usize, Option<Stamp>>,
    /// Why its destination can't be reached, while it can't.
    unavailable: Option<Unavailable>,
}

/// What the pretend librqbit is doing with a torrent that has info.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pretend {
    Stopped,
    Running,
    /// Since when on the app clock, filled in by the first poll after it starts.
    Checking {
        since: Option<f64>,
    },
    Failed,
}

impl Pretend {
    fn lib(self) -> Librqbit {
        match self {
            Pretend::Stopped => Librqbit::Stopped,
            Pretend::Running => Librqbit::Running,
            Pretend::Checking { .. } => Librqbit::Checking,
            Pretend::Failed => Librqbit::Failed,
        }
    }
}

/// What happens to the fake's files outside Phantom. Files set Off and torrents fetching info are
/// left alone, as by the real service.
#[derive(Debug, Clone, PartialEq)]
pub enum DiskChange {
    /// These files of the torrent, by index, are deleted.
    FilesDeleted { id: TorrentId, files: Vec<usize> },
    /// These files deleted from the torrent are put back as they were.
    FilesPutBack { id: TorrentId, files: Vec<usize> },
    /// The torrent's destination can't be reached, for `reason`.
    DestinationGone { id: TorrentId, reason: Unavailable },
    /// The torrent's destination can be reached again.
    DestinationBack { id: TorrentId },
    /// Not the disk, but from outside too: the service restarts, with every torrent as it was.
    /// Its snapshots are a new epoch, counted afresh.
    #[cfg_attr(not(test), allow(dead_code))]
    Restarted,
}

/// How much of [`MemoryService::stale_answers`] each command takes, in turn.
const STALE: [f64; 3] = [2.0, 0.0, 1.0];

/// A command's snapshot, taken as it was handled, and its answer, due `after` seconds from the
/// poll that handled it.
struct Late {
    snapshot: Snapshot,
    answer: Incoming,
    since: f64,
    after: f64,
}

/// The latest snapshot the fake streamed, and the commands it was sent, for tests to compare the
/// app with.
#[derive(Clone, Default)]
pub struct Probe(Arc<Mutex<Option<Snapshot>>>, Arc<Mutex<Vec<(Vec<TorrentId>, TorrentCommand)>>>);

impl Probe {
    #[cfg(test)]
    pub fn latest(&self) -> Option<Snapshot> {
        self.0.lock().unwrap().clone()
    }

    /// Every command sent, with the torrents it was for, in order.
    #[cfg(test)]
    pub fn commands(&self) -> Vec<(Vec<TorrentId>, TorrentCommand)> {
        self.1.lock().unwrap().clone()
    }
}

/// The changes to the fake's disk, each due at a time on the app clock; a test can hold a handle
/// and change it while the app has the service. They apply at the first poll that's due.
#[derive(Clone, Default)]
pub struct Disk(Arc<Mutex<Vec<(f64, DiskChange)>>>);

impl Disk {
    /// `change` happens at the next poll.
    #[cfg(test)]
    pub fn change(&self, change: DiskChange) {
        self.0.lock().unwrap().push((0.0, change));
    }

    /// The changes due by `now`, in time order.
    fn due(&self, now: f64) -> Vec<DiskChange> {
        let mut changes = self.0.lock().unwrap();
        changes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let due = changes.iter().take_while(|(at, _)| *at <= now).count();
        changes.drain(..due).map(|(_, c)| c).collect()
    }
}

/// When a torrent fetching info gets it. Actions don't see the app clock, so `added_at` is
/// filled in by the first poll after the add.
struct Timing {
    id: TorrentId,
    added_at: Option<f64>,
    info_after: f64,
}

/// An add on its way. `sent_at` is filled in by the first poll after it, as for [`Timing`].
struct PendingAdd {
    sources: Vec<AddRequest>,
    destination: Option<AbsolutePath>,
    done: Done<Vec<Result<Added, AddError>>>,
    sent_at: Option<f64>,
    secs: f64,
}

impl Default for MemoryService {
    fn default() -> Self {
        Self {
            torrents: Vec::new(),
            files: BTreeMap::new(),
            machines: BTreeMap::new(),
            mailbox: Vec::new(),
            made: 0,
            watching: Vec::new(),
            details_after: 0.0,
            details_asked: BTreeMap::new(),
            epoch: 1,
            version: 0,
            published: None,
            stale_answers: 0.0,
            trash_secs: 0.0,
            trash_refusing: BTreeSet::new(),
            trashing: Vec::new(),
            commands: 0,
            late: Vec::new(),
            probe: Probe::default(),
            #[cfg(test)]
            sent: BTreeMap::new(),
            reset: Vec::new(),
            timings: Vec::new(),
            info_after: 2.0,
            rejecting_links: false,
            rejecting: BTreeSet::new(),
            add_secs: 0.0,
            torrent_url_secs: 0.0,
            adds: Vec::new(),
            next_id: 0,
            last_poll: None,
            incoming: vec![Incoming::Live(Live::Connection(Connection::Connected { since: std::time::Instant::now() }))],
            changed: true,
            default_destination: Some(downloads()),
            listen_port: ListenPort::Picked(NonZeroU16::new(51413).unwrap()),
            upnp: true,
            unreachable: BTreeMap::new(),
            taken: BTreeSet::new(),
            restarts_fail: false,
            destinations: BTreeMap::new(),
            disk: Disk::default(),
        }
    }
}

impl MemoryService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every add of a link fails, as one of a magnet nobody has would.
    pub fn rejecting_links(mut self) -> Self {
        self.rejecting_links = true;
        self
    }

    /// Delete takes `secs` on the app clock to move a torrent's files to the Trash; it's
    /// Deleting meanwhile, and the command is answered after.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn trash_takes(mut self, secs: f64) -> Self {
        self.trash_secs = secs;
        self
    }

    /// The Trash refuses the files of the torrent named `name`, as a locked file: its Delete
    /// moves nothing, and fails.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn trash_refusing(mut self, name: &str) -> Self {
        self.trash_refusing.insert(name.to_owned());
        self
    }

    /// Adds of `link`, or of the file at that path, fail, as one of a magnet nobody has would.
    pub fn rejecting(mut self, link: &str) -> Self {
        self.rejecting.insert(link.to_owned());
        self
    }

    /// Seconds an add takes to be answered (and to show in the list), as from a busy service:
    /// `secs` for a magnet, info hash or file, `torrent_url_secs` for a .torrent URL. 0: by the
    /// next poll.
    pub fn adds_after(mut self, secs: f64, torrent_url_secs: f64) -> Self {
        (self.add_secs, self.torrent_url_secs) = (secs, torrent_url_secs);
        self
    }

    /// Seconds from adding a magnet or URL until its info arrives; `f64::INFINITY` never.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn info_after(mut self, secs: f64) -> Self {
        self.info_after = secs;
        self
    }

    /// Seconds from watching a torrent until its details come; 0 by the next poll.
    pub fn details_after(mut self, secs: f64) -> Self {
        self.details_after = secs;
        self
    }

    /// A command's own snapshot and answer take `secs` times 2, 0 and 1 in turn, so they land late
    /// and out of order; 0: by the next poll.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn stale_answers(mut self, secs: f64) -> Self {
        self.stale_answers = secs;
        self
    }

    /// Every torrent seeded so far is wanted stopped: those running stop, so nothing changes until
    /// something is done to them.
    #[cfg(test)]
    pub fn paused(mut self) -> Self {
        let ids: Vec<TorrentId> = self.machines.keys().copied().collect();
        for id in ids {
            self.event(id, Event::Command(proto::Action::Pause));
            self.refresh(id);
        }
        self
    }

    /// A handle to what the fake streamed last, for tests.
    #[cfg(test)]
    pub fn probe(&self) -> Probe {
        self.probe.clone()
    }

    /// The service reset these settings at its start, as `settings.json` had them invalid.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_reset(mut self, names: &[&str]) -> Self {
        self.reset = names.iter().map(|n| n.to_string()).collect();
        self
    }

    /// Seeded torrents fetching info keep fetching, as in a screenshot of a slow magnet. The
    /// others are one file, named after the torrent, until [`with_files`](Self::with_files), and
    /// are in the phase their status says: Downloading, Seeding, Paused, Done, Checking,
    /// Destination unavailable or Error. Missing files come from [`with_missing`](Self::with_missing).
    pub fn with_torrents(mut self, torrents: Vec<TorrentInfo>) -> Self {
        self.next_id = torrents.iter().map(|t| t.id + 1).max().unwrap_or(0).max(self.next_id);
        for t in torrents {
            let (state, librqbit, unavailable) = seeded(&t.status);
            if let Some(content) = t.content {
                let name = t.name.clone().unwrap_or_default();
                let file = File {
                    index: 0,
                    path: name.into(),
                    size_bytes: content.size_bytes,
                    progress_bytes: content.progress_bytes,
                    priority: Priority::Normal,
                    missing: false,
                };
                self.files.insert(t.id, vec![file]);
            }
            let on_disk = BTreeMap::new();
            self.machines.insert(t.id, Torrent { state, librqbit, on_disk, unavailable });
            let id = t.id;
            self.torrents.push(t);
            self.make_files(id);
            self.refresh(id);
        }
        self
    }

    /// Gives torrent `id` these files, all on disk; its Size and progress follow from them.
    pub fn with_files(mut self, id: TorrentId, files: Vec<File>) -> Self {
        if self.torrents.iter().any(|t| t.id == id) {
            self.files.insert(id, files);
            if let Some(m) = self.machines.get_mut(&id) {
                m.on_disk.clear();
            }
            self.make_files(id);
            self.refresh(id);
        }
        self
    }

    /// The files of torrent `id` at `indices` went missing from disk: it stops and is Missing
    /// files, and they show as missing with nothing downloaded, as with the real service.
    pub fn with_missing(mut self, id: TorrentId, indices: &[usize]) -> Self {
        self.change(DiskChange::FilesDeleted { id, files: indices.to_vec() });
        self
    }

    /// `change` happens at `secs` on the app clock (0: at the first poll), e.g. files put back
    /// during a scene.
    pub fn at(self, secs: f64, change: DiskChange) -> Self {
        self.disk.0.lock().unwrap().push((secs, change));
        self
    }

    /// A handle to change the fake's disk once the app has it.
    #[cfg(test)]
    pub fn disk(&self) -> Disk {
        self.disk.clone()
    }

    fn change(&mut self, change: DiskChange) {
        let id = match change {
            DiskChange::Restarted => {
                self.epoch += 1;
                (self.version, self.published) = (0, None);
                self.changed = true;
                return;
            }
            DiskChange::FilesDeleted { id, files } => {
                self.on_disk(id, &files, |_| None);
                id
            }
            DiskChange::FilesPutBack { id, files } => {
                let mut made = self.made;
                self.on_disk(id, &files, |at| {
                    at.or_else(|| {
                        made += 1;
                        Some(made)
                    })
                });
                self.made = made;
                id
            }
            DiskChange::DestinationGone { id, reason } => {
                self.reach(id, Some(reason));
                id
            }
            DiskChange::DestinationBack { id } => {
                self.reach(id, None);
                id
            }
        };
        self.size_check(id);
        self.changed = true;
    }

    /// What's at the paths of torrent `id`'s files at `indices`: the file made (by number) that's
    /// there, from what was. A file put back is as it was: librqbit had what it holds.
    fn on_disk(&mut self, id: TorrentId, indices: &[usize], at: impl FnMut(Option<u64>) -> Option<u64>) {
        let (Some(m), Some(files)) = (self.machines.get_mut(&id), self.files.get(&id)) else { return };
        let mut at = at;
        for f in files.iter().filter(|f| indices.contains(&f.index)) {
            let now = at(m.on_disk.get(&f.index).copied().flatten().map(|s| s.inode));
            m.on_disk.insert(f.index, now.map(|n| file_stamp(n, f)));
        }
    }

    /// Torrent `id`'s destination can't be reached, for `reason`, or can again.
    fn reach(&mut self, id: TorrentId, reason: Option<Unavailable>) {
        if self.files.contains_key(&id)
            && let Some(m) = self.machines.get_mut(&id)
        {
            m.unavailable = reason;
        }
    }

    /// Every file of torrent `id` that isn't on disk is made, as librqbit does when it adds one.
    fn make_files(&mut self, id: TorrentId) {
        let (Some(m), Some(files)) = (self.machines.get_mut(&id), self.files.get(&id)) else { return };
        for f in files {
            if m.on_disk.get(&f.index).is_none_or(Option::is_none) {
                self.made += 1;
                m.on_disk.insert(f.index, Some(file_stamp(self.made, f)));
            }
        }
    }

    /// Where adds that name no destination go; `None`: they're refused, so the app asks first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_default_destination(mut self, folder: Option<PathBuf>) -> Self {
        self.default_destination = folder;
        self
    }

    /// Setting `folder` as the default destination is refused, for `reason`.
    pub fn unreachable(mut self, folder: impl Into<PathBuf>, reason: Unavailable) -> Self {
        self.unreachable.insert(folder.into(), reason);
        self
    }

    /// Another program listens on `port`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn port_taken(mut self, port: u16) -> Self {
        self.taken.extend(NonZeroU16::new(port));
        self
    }

    /// A new port or UPnP fails, changing nothing, as when the torrents don't start again with them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn failing_restarts(mut self) -> Self {
        self.restarts_fail = true;
        self
    }

    /// Its settings as the real service answers them. It listens on the port, or, while another
    /// program has a pinned one, on a temporary port.
    fn current_settings(&self) -> Settings {
        let port = self.listen_port.port();
        let listening_on = if self.taken.contains(&port) { NonZeroU16::new(54321).unwrap() } else { port };
        Settings {
            default_destination: self.default_destination.clone().map(|f| AbsolutePath::new(f).expect("absolute")),
            listen_port: self.listen_port,
            listening_on,
            upnp: self.upnp,
            reset: Vec::new(),
        }
    }

    /// As the real service: a port set is pinned, unset keeps it as picked; a picked port another
    /// program has is replaced.
    fn set(&mut self, setting: Setting) -> Result<(), Refusal> {
        match setting {
            Setting::DefaultDestination(Some(folder)) => match self.unreachable.get(&*folder) {
                Some(reason) => return Err(Refusal::DefaultDestination { reason: reason.clone() }),
                None => self.default_destination = Some(folder.into_path_buf()),
            },
            Setting::DefaultDestination(None) => self.default_destination = None,
            Setting::ListenPort(Some(port)) => self.listen_port = ListenPort::Pinned(port),
            Setting::ListenPort(None) => {
                let port = self.listen_port.port();
                let free = (1..=u16::MAX).rev().filter_map(NonZeroU16::new).find(|p| !self.taken.contains(p));
                let port = if self.taken.contains(&port) { free.expect("a free port") } else { port };
                self.listen_port = ListenPort::Picked(port);
            }
            Setting::Upnp(upnp) => self.upnp = upnp.unwrap_or(true),
        }
        Ok(())
    }

    /// Decides `event` for torrent `id`, then makes the pretend librqbit match; once its phase
    /// changes, it's size-checked, and that's decided too, as by the engine's actor.
    fn event(&mut self, id: TorrentId, event: Event) {
        let mut event = event;
        loop {
            let Some(m) = self.machines.get_mut(&id) else { return };
            let before = m.state.phase.clone();
            let Ok(next) = lifecycle::decide(&m.state, &event) else { return };
            m.state = next;
            self.carry_out(id);
            self.changed = true;
            let moved = self.machines.get(&id).is_some_and(|m| m.state.phase != before);
            match self.size_checked(id) {
                Some(seen) if moved => event = Event::Observed(seen),
                _ => return,
            }
        }
    }

    /// Makes the pretend librqbit match the torrent, as the engine makes librqbit.
    fn carry_out(&mut self, id: TorrentId) {
        if !self.files.contains_key(&id) {
            return;
        }
        let Some(m) = self.machines.get_mut(&id) else { return };
        match lifecycle::next(&m.state, m.librqbit.lib()) {
            None => {}
            Some(Do::Start) => m.librqbit = Pretend::Running,
            Some(Do::Stop) => m.librqbit = Pretend::Stopped,
            Some(Do::AddAgain) => {
                // Added again it makes the files that aren't there, empty, and checks them.
                for f in self.files.get_mut(&id).into_iter().flatten() {
                    if m.on_disk.get(&f.index).is_none_or(Option::is_none) {
                        f.progress_bytes = 0;
                    }
                }
                self.make_files(id);
                let m = self.machines.get_mut(&id).expect("just found");
                m.librqbit = Pretend::Checking { since: None };
                m.state = lifecycle::decide(&m.state, &Event::Observed(Observation::AddedAgain)).expect("never refused");
            }
        }
    }

    /// Size-checks torrent `id` and decides what that finds.
    fn size_check(&mut self, id: TorrentId) {
        if let Some(seen) = self.size_checked(id) {
            self.event(id, Event::Observed(seen));
        }
    }

    /// What a size check finds, as the engine's: its destination out of reach, or the files it's
    /// getting and what's at each path; nothing while it's checked, or without info.
    fn size_checked(&self, id: TorrentId) -> Option<Observation> {
        let (m, files) = (self.machines.get(&id)?, self.files.get(&id)?);
        if let Some(reason) = &m.unavailable {
            return Some(Observation::DestinationGone { reason: reason.clone() });
        }
        if matches!(m.librqbit, Pretend::Checking { .. }) {
            return None;
        }
        let found = files.iter().filter(|f| f.priority == Priority::Normal).map(|f| {
            let file = Expected { index: f.index, path: f.path.clone(), size_bytes: f.size_bytes, progress_bytes: f.progress_bytes };
            (f.index, lifecycle::disk::Found { file, at: m.on_disk.get(&f.index).copied().flatten() })
        });
        Some(Observation::Files(Found { files: found.collect() }))
    }

    /// The snapshot of now, built for what's watched: the details of those watched that came
    /// (slow ones haven't). A change in the torrents, or in which details are on their way,
    /// counts up the version.
    fn snapshot(&mut self) -> Snapshot {
        let all = self.torrents.iter().filter_map(|t| self.details(t.id)).collect();
        let now = Some((self.torrents.clone(), all, self.details_asked.keys().copied().collect()));
        if self.published != now {
            self.version += 1;
            self.published = now;
        }
        let came = self.watching.iter().filter(|id| !self.details_asked.contains_key(id));
        let details = came.filter_map(|&id| self.details(id)).collect();
        let snapshot = Snapshot {
            epoch: self.epoch,
            version: self.version,
            watching: self.watching.clone(),
            torrents: self.torrents.clone(),
            details,
            reset: self.reset.clone(),
        };
        #[cfg(test)]
        self.keeps_the_protocol(&snapshot);
        snapshot
    }

    /// The protocol's promise: one snapshot per `(epoch, version, watching)`. Two that differ
    /// under one key would let the app take an older one for the latest (#148).
    #[cfg(test)]
    fn keeps_the_protocol(&mut self, snapshot: &Snapshot) {
        let key = (snapshot.epoch, snapshot.version, snapshot.watching.clone());
        let first = self.sent.entry(key).or_insert_with(|| snapshot.clone());
        assert_eq!(first, snapshot, "two different snapshots for one (epoch, version, watching)");
    }

    /// The files of torrent `id` it has found missing, by index.
    fn missing(&self, id: TorrentId) -> BTreeSet<usize> {
        match self.machines.get(&id).map(|m| &m.state.phase) {
            Some(Phase::MissingFiles { gone }) => gone.keys().copied().collect(),
            _ => BTreeSet::new(),
        }
    }

    /// Torrent `id`'s Status and actions, from its lifecycle, and its sizes and progress, from its
    /// files; its speeds follow its Status when that changes.
    fn refresh(&mut self, id: TorrentId) {
        let missing = self.missing(id);
        let Some(idx) = self.torrents.iter().position(|t| t.id == id) else { return };
        let (t, Some(m)) = (&mut self.torrents[idx], self.machines.get(&id)) else { return };
        let files = self.files.get(&id).map(Vec::as_slice).unwrap_or_default();
        if !files.is_empty() {
            sync(t, files, &missing);
        }
        let normal = || files.iter().filter(|f| f.priority == Priority::Normal);
        let complete = normal().all(|f| f.progress_bytes == f.size_bytes);
        let status = m.state.status(complete);
        if status != t.status {
            (t.download_bytes_per_sec, t.upload_bytes_per_sec, t.live_peers) = match status {
                Status::Downloading => (DOWNLOAD_SPEED, 0, 12),
                Status::Seeding => (0, SEED_SPEED, 12),
                _ => (0, 0, 0),
            };
            t.status = status;
        }
        t.actions = m.state.actions();
        let location = self.location(&self.torrents[idx]);
        self.torrents[idx].location = location;
    }

    /// Where torrent `t` is on disk: the file itself for one file, its own folder otherwise.
    fn location(&self, t: &TorrentInfo) -> Option<AbsolutePath> {
        let files = self.files.get(&t.id)?;
        let destination = self.destinations.get(&t.id).cloned().unwrap_or_else(downloads);
        let location = match &files[..] {
            [only] => destination.join(&only.path),
            _ => destination.join(t.display_name()),
        };
        AbsolutePath::new(location).ok()
    }

    fn details(&self, id: TorrentId) -> Option<Details> {
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let files = self.files.get(&id)?;
        // As the engine shows them: one missing has nothing, whatever librqbit thinks.
        let missing = self.missing(id);
        let files = files
            .iter()
            .map(|f| {
                let gone = missing.contains(&f.index);
                File { missing: gone, progress_bytes: if gone { 0 } else { f.progress_bytes }, ..f.clone() }
            })
            .collect();
        Some(Details { id, files, location: self.location(t)? })
    }

    fn torrent_mut(&mut self, id: TorrentId) -> Option<&mut TorrentInfo> {
        self.torrents.iter_mut().find(|t| t.id == id)
    }

    fn new_torrent(&mut self, name: Option<String>, info_hash: Option<String>) -> TorrentInfo {
        let id = self.next_id;
        self.next_id += 1;
        TorrentInfo {
            id,
            info_hash: info_hash.unwrap_or_else(|| format!("{id:040x}")),
            name,
            status: Status::FetchingInfo,
            actions: proto::Actions::default(),
            content: None,
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
            trackers: Vec::new(),
            location: None,
        }
    }

    fn advance(&mut self, now: f64, dt: f64) {
        let ids: Vec<TorrentId> = self.torrents.iter().map(|t| t.id).collect();
        for &id in &ids {
            self.refresh(id);
        }
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
            }
            t.uploaded_bytes += (t.upload_bytes_per_sec as f64 * dt) as u64;
            self.changed = true;
        }

        // Checks that took long enough end: librqbit has what the files hold.
        let mut ended = Vec::new();
        for (&id, m) in &mut self.machines {
            if let Pretend::Checking { since } = &mut m.librqbit
                && now - *since.get_or_insert(now) >= CHECK_SECS
            {
                m.librqbit = Pretend::Stopped;
                ended.push(id);
            }
        }
        for id in ended {
            let have = self.files.get(&id).map(|files| files.iter().map(|f| f.progress_bytes).collect()).unwrap_or_default();
            self.event(id, Event::Observed(Observation::Idle { have }));
        }

        for timing in &mut self.timings {
            timing.added_at.get_or_insert(now);
        }
        let due: Vec<TorrentId> =
            self.timings.iter().filter(|t| t.added_at.is_some_and(|at| now - at >= t.info_after)).map(|t| t.id).collect();
        self.timings.retain(|t| !due.contains(&t.id));
        for id in due {
            self.info_arrived(id);
        }
        for &id in &ids {
            self.refresh(id);
        }
    }

    /// Torrent `id`'s info is in: one file, named after it, on disk; stopped until its lifecycle
    /// starts it, as wanted.
    fn info_arrived(&mut self, id: TorrentId) {
        let Some(t) = self.torrent_mut(id) else { return };
        let files = start_downloading(t);
        self.files.insert(id, files);
        if let Some(m) = self.machines.get_mut(&id) {
            m.librqbit = Pretend::Stopped;
        }
        self.make_files(id);
        self.event(id, Event::Observed(Observation::InfoArrived { checking: false }));
        self.refresh(id);
    }

    /// A command for each of `ids` its lifecycle says does something now; the others are ignored.
    /// Each of `ids` its lifecycle says the command does something to; the others are ignored.
    /// Deletes being trashed are returned, for their answer to wait; failures with them.
    fn handle(&mut self, ids: &[TorrentId], command: TorrentCommand) -> (Vec<TorrentId>, Vec<(TorrentId, Failure)>) {
        self.probe.1.lock().unwrap().push((ids.to_vec(), command.clone()));
        let action = command.action();
        let accepting: Vec<TorrentId> =
            ids.iter().copied().filter(|id| self.machines.get(id).is_some_and(|m| m.state.actions().does(action))).collect();
        let (mut trashing, mut failed) = (Vec::new(), Vec::new());
        for id in accepting {
            self.event(id, Event::Command(action));
            match &command {
                TorrentCommand::Remove => self.forget(&[id]),
                TorrentCommand::Delete if !self.machines.get(&id).is_some_and(|m| matches!(m.state.phase, Phase::Deleting { .. })) => {
                    self.forget(&[id])
                }
                // Its pretend files go to the Trash at once, or once the trash took its time.
                TorrentCommand::Delete if self.trash_secs > 0.0 => trashing.push(id),
                TorrentCommand::Delete => failed.extend(self.trashed(id)),
                TorrentCommand::SetPriority { files, priority } => {
                    for f in self.files.get_mut(&id).into_iter().flatten().filter(|f| files.contains(&f.index)) {
                        f.priority = *priority;
                    }
                    // Files set Off aren't missing any more, and those set Normal may be.
                    self.size_check(id);
                }
                TorrentCommand::Pause | TorrentCommand::Resume | TorrentCommand::Check => {}
            }
            self.refresh(id);
        }
        self.changed = true;
        (trashing, failed)
    }

    /// The pretend trashing of torrent `id` ends: all of its files moved, and it's off the list;
    /// or, refused, none, and it's as it was, the Delete failing with why.
    fn trashed(&mut self, id: TorrentId) -> Option<(TorrentId, Failure)> {
        let torrent = self.torrents.iter().find(|t| t.id == id)?;
        let name = torrent.display_name().to_owned();
        let refused = self.trash_refusing.contains(&name);
        let location = self.location(torrent).or_else(|| AbsolutePath::new(downloads().join(torrent.display_name())).ok());
        let kept: Vec<Kept> = match location {
            Some(path) if refused => vec![Kept { path, why: KeptWhy::Failed { detail: "it's locked".into() } }],
            _ => Vec::new(),
        };
        self.event(id, Event::Observed(Observation::Trashed { kept: kept.clone(), moved: false }));
        self.changed = true;
        if kept.is_empty() {
            self.forget(&[id]);
            return None;
        }
        self.refresh(id);
        Some((id, Failure::Refused(Refusal::Torrent { id, why: TorrentRefusal::NotTrashed { name, kept } })))
    }

    /// Adds one torrent into `destination`, or the default destination.
    fn add_one(&mut self, source: AddRequest, destination: Option<AbsolutePath>) -> Result<Added, AddError> {
        let failed = |error: &str| AddError::Failed(Failure::Error(anyhow!("{error}")));
        // A .torrent file has its info at once.
        let (torrent, has_info) = match source {
            AddRequest::File(path) if self.rejecting.contains(&*path.to_string_lossy()) => {
                return Err(failed("not a valid .torrent file"));
            }
            AddRequest::File(path) => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                (self.new_torrent(name, None), true)
            }
            AddRequest::Url(url) if self.rejecting_links || self.rejecting.contains(&url) => {
                return Err(failed("no peers have this torrent"));
            }
            AddRequest::Url(url) => {
                let Some((name, info_hash)) = parse_url(&url) else { return Err(failed("not a magnet link or .torrent URL")) };
                if let Some(t) = self.torrents.iter().find(|t| t.info_hash == info_hash) {
                    return Ok(Added { id: t.id, name: t.name.clone(), already_existed: true });
                }
                let mut torrent = self.new_torrent(name, Some(info_hash));
                if let Ok(client::Link::Magnet { trackers, .. }) = client::parse_link(&url) {
                    torrent.trackers = trackers;
                }
                self.timings.push(Timing { id: torrent.id, added_at: None, info_after: self.info_after });
                (torrent, false)
            }
        };
        let (id, added) = (torrent.id, Added { id: torrent.id, name: torrent.name.clone(), already_existed: false });
        if let Some(folder) = destination.map(AbsolutePath::into_path_buf).or_else(|| self.default_destination.clone()) {
            self.destinations.insert(id, folder);
        }
        self.torrents.push(torrent);
        let fetching = Torrent {
            state: State::new(Wants::Run, Phase::FetchingInfo),
            librqbit: Pretend::Stopped,
            on_disk: BTreeMap::new(),
            unavailable: None,
        };
        self.machines.insert(id, fetching);
        if has_info {
            self.info_arrived(id);
        }
        self.refresh(id);
        self.changed = true;
        Ok(added)
    }

    /// How long the add of `source` takes, as with the real service.
    fn add_takes(&self, source: &AddRequest) -> f64 {
        match source {
            AddRequest::Url(url) if url.starts_with("http://") || url.starts_with("https://") => self.torrent_url_secs,
            _ => self.add_secs,
        }
    }

    /// Does the adds whose time has come, in the order they came, and queues their answers.
    fn finish_adds(&mut self, now: f64) {
        for add in &mut self.adds {
            add.sent_at.get_or_insert(now);
        }
        let (due, waiting) = std::mem::take(&mut self.adds).into_iter().partition(|a| a.sent_at.is_some_and(|at| now - at >= a.secs));
        self.adds = waiting;
        for PendingAdd { sources, destination, done, .. } in due {
            let results = if destination.is_none() && self.default_destination.is_none() {
                sources.into_iter().map(|source| Err(AddError::NoDestination(source))).collect()
            } else {
                sources.into_iter().map(|source| self.add_one(source, destination.clone())).collect()
            };
            self.incoming.push(answer(done, results));
        }
    }

    fn forget(&mut self, ids: &[TorrentId]) {
        self.destinations.retain(|id, _| !ids.contains(id));
        self.torrents.retain(|t| !ids.contains(&t.id));
        self.timings.retain(|t| !ids.contains(&t.id));
        self.machines.retain(|id, _| !ids.contains(id));
        self.files.retain(|id, _| !ids.contains(id));
        self.changed = true;
    }
}

/// The torrent's info is in: one file, named after it. Returns its files.
fn start_downloading(t: &mut TorrentInfo) -> Vec<File> {
    let name = t.display_name().to_owned();
    let files = vec![File {
        index: 0,
        path: name.into(),
        size_bytes: NEW_TORRENT_SIZE,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
    }];
    sync(t, &files, &BTreeSet::new());
    files
}

/// The lifecycle, pretend librqbit and destination of a torrent seeded with `status`.
fn seeded(status: &Status) -> (State, Pretend, Option<Unavailable>) {
    let state = |wants, phase| State::new(wants, phase);
    match status {
        Status::FetchingInfo => (state(Wants::Run, Phase::FetchingInfo), Pretend::Stopped, None),
        Status::Downloading | Status::Seeding => (state(Wants::Run, Phase::Ready), Pretend::Running, None),
        Status::Paused | Status::Done => (state(Wants::Stop, Phase::Ready), Pretend::Stopped, None),
        Status::Checking => (state(Wants::Run, Phase::Checking { returning: BTreeMap::new() }), Pretend::Checking { since: None }, None),
        Status::DestinationUnavailable { reason } => {
            (state(Wants::Run, Phase::DestinationUnavailable { reason: reason.clone() }), Pretend::Stopped, Some(reason.clone()))
        }
        Status::Error { failure } => {
            let phase = match failure.clone() {
                proto::TorrentFailure::FetchFailed { detail } => Phase::FetchFailed { detail },
                proto::TorrentFailure::AddAgainFailed { detail } => Phase::Error { failure: lifecycle::Broke::AddAgainFailed { detail } },
                proto::TorrentFailure::Librqbit { detail } => Phase::Error { failure: lifecycle::Broke::Librqbit { detail } },
                proto::TorrentFailure::NotTrashed { kept } => Phase::DeleteFailed { kept, away: None },
            };
            (state(Wants::Run, phase), Pretend::Failed, None)
        }
        Status::MissingFiles => panic!("seed it running and give it missing files with `with_missing`"),
        Status::Moving => panic!("no torrent moves yet (#24)"),
        Status::Deleting => panic!("Delete one to see it Deleting: `trash_takes`"),
    }
}

/// The file made `n`-th on the pretend disk, at `f`'s path: its full size, as librqbit makes it.
fn file_stamp(n: u64, f: &File) -> Stamp {
    Stamp { inode: n, len: f.size_bytes, modified: None }
}

/// The torrent's Size, Full size and progress, from its files; those `missing` have nothing.
fn sync(t: &mut TorrentInfo, files: &[File], missing: &BTreeSet<usize>) {
    let normal = || files.iter().filter(|f| f.priority == Priority::Normal);
    t.content = Some(Content {
        size_bytes: normal().map(|f| f.size_bytes).sum(),
        progress_bytes: normal().filter(|f| !missing.contains(&f.index)).map(|f| f.progress_bytes).sum(),
        full_size_bytes: files.iter().map(|f| f.size_bytes).sum(),
    });
}

/// The fake's default destination, unless told otherwise.
fn downloads() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from).join("Downloads")
}

impl Service for MemoryService {
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>, done: Done<Vec<Result<Added, AddError>>>) {
        // Together, one batch is answered when its slowest add is.
        let secs = sources.iter().map(|source| self.add_takes(source)).fold(0.0, f64::max);
        self.adds.push(PendingAdd { sources, destination, done, sent_at: None, secs });
    }

    /// Sent to the mailbox, handled at the next poll.
    fn command(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Vec<(TorrentId, Failure)>>) {
        self.mailbox.push(Message { ids: ids.to_vec(), command, done });
    }

    fn settings(&mut self, done: Done<Result<Settings, Failure>>) {
        self.incoming.push(answer(done, Ok(self.current_settings())));
    }

    fn set_setting(&mut self, setting: Setting, done: Done<Result<Settings, Failure>>) {
        let restarts = matches!(setting, Setting::ListenPort(_) | Setting::Upnp(_));
        let result = if restarts && self.restarts_fail {
            Err(Failure::Error(anyhow!("the torrents didn't start again with the new settings: no room")))
        } else {
            self.set(setting).map(|()| self.current_settings()).map_err(Failure::Refused)
        };
        // The engine started again: its snapshots are a new epoch.
        if restarts && result.is_ok() {
            self.change(DiskChange::Restarted);
        }
        self.incoming.push(answer(done, result));
    }

    /// Those newly watched come after `details_after`; the others go on coming.
    fn watch_details(&mut self, ids: Vec<TorrentId>) {
        for &id in ids.iter().filter(|id| !self.watching.contains(id)) {
            self.details_asked.insert(id, None);
        }
        self.details_asked.retain(|id, _| ids.contains(id));
        self.watching = ids;
        self.changed = true;
    }

    fn poll(&mut self, now: f64) -> Vec<Incoming> {
        // Each command's own snapshot is taken once the poll has done all it does, as the real
        // service's command takes one once the engine has published the command's effect.
        let mut answered = Vec::new();
        for Message { ids, command, done } in std::mem::take(&mut self.mailbox) {
            let (trashing, failed) = self.handle(&ids, command);
            if !trashing.is_empty() {
                self.trashing.push(Trashing { due: now + self.trash_secs, ids: trashing, failed, done });
                continue;
            }
            let after = self.stale_answers * STALE[self.commands % STALE.len()];
            self.commands += 1;
            answered.push((answer(done, failed), after));
        }
        let (due, waiting): (Vec<Trashing>, Vec<Trashing>) = std::mem::take(&mut self.trashing).into_iter().partition(|t| now >= t.due);
        self.trashing = waiting;
        for Trashing { ids, mut failed, done, .. } in due {
            for id in ids {
                failed.extend(self.trashed(id));
            }
            answered.push((answer(done, failed), 0.0));
        }
        self.disk.due(now).into_iter().for_each(|c| self.change(c));
        let dt = self.last_poll.map_or(0.0, |last| (now - last).max(0.0));
        self.last_poll = Some(now);
        self.advance(now, dt);
        self.finish_adds(now);
        let details_after = self.details_after;
        let before = self.details_asked.len();
        self.details_asked.retain(|_, asked| now - *asked.get_or_insert(now) < details_after);
        self.changed |= self.details_asked.len() != before;
        for (answer, after) in answered {
            let snapshot = self.snapshot();
            self.late.push(Late { snapshot, answer, since: now, after });
        }
        if std::mem::take(&mut self.changed) {
            // Before the answers, so the torrents they name are in the list.
            let snapshot = self.snapshot();
            *self.probe.0.lock().unwrap() = Some(snapshot.clone());
            self.incoming.insert(0, Incoming::Live(Live::Snapshot(snapshot)));
        }
        // Commands' own, after the stream's: each its snapshot, then its answer.
        let (due, waiting) = std::mem::take(&mut self.late).into_iter().partition(|l: &Late| now - l.since >= l.after);
        self.late = waiting;
        for Late { snapshot, answer, .. } in due {
            self.incoming.extend([Incoming::Live(Live::Snapshot(snapshot)), answer]);
        }
        std::mem::take(&mut self.incoming)
    }

    fn next_wake(&self) -> Option<Duration> {
        let active = !self.details_asked.is_empty()
            || !self.adds.is_empty()
            || !self.mailbox.is_empty()
            || !self.trashing.is_empty()
            || !self.late.is_empty()
            || !self.timings.is_empty()
            || self.machines.values().any(|m| matches!(m.librqbit, Pretend::Checking { .. }))
            || !self.disk.0.lock().unwrap().is_empty()
            || self.torrents.iter().any(|t| matches!(t.status, Status::Downloading | Status::Seeding));
        active.then_some(Duration::from_millis(500))
    }
}

/// The display name and info hash a URL implies, as the real service reports them: a magnet's
/// `dn=` and `xt=`, or a bare info hash; for a .torrent URL, whose file the fake doesn't download,
/// its file name and a hash made from the URL. `None` for something that is neither (the real
/// service rejects those too).
fn parse_url(url: &str) -> Option<(Option<String>, String)> {
    if let Some(query) = url.strip_prefix("magnet:?") {
        let param = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key));
        let info_hash = param("xt=urn:btih:")?.to_ascii_lowercase();
        return Some((param("dn=").map(percent_decode), info_hash));
    }
    if url.len() == 40 && url.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some((None, url.to_ascii_lowercase()));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
        let hash = url.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
        return Some((file.map(|f| f.trim_end_matches(".torrent").to_owned()), format!("{hash:040x}")));
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
