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
//! ([`with_default_destination`](MemoryService::with_default_destination), [`DOWNLOADS`] unless
//! told otherwise); without either they're refused, as by the real service.
//!
//! Remove, Delete and Move to Trash can be undone, last first, on the service's own machine
//! ([`lifecycle::undo`], #190, in `steps.rs`): every command is a step on its torrent's lane, those
//! three on the stack too, from the moment they come; ⌘Z takes the top one at once, aborting one
//! still running. The torrent comes back as it was before the command, under its id; refused while
//! one with its info hash is in the list.
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
use lifecycle::seeding::Meter;
use lifecycle::{Do, Event, Lib as Librqbit, Observation, Phase, State, Wants};
use proto::{
    AbsolutePath, AddSource, Added, Content, Details, File, Kept, KeptWhy, ListenPort, Needs, Priority, Refusal, Setting, Settings,
    Snapshot, Status, TorrentCommand, TorrentId, TorrentInfo, TorrentRefusal, Unavailable, UndoAction, Undoable, Volume,
};

use client::Failure;

use super::{AddError, AddRequest, Connection, Done, Incoming, Live, Service, answer};

mod steps;

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
    /// Changes in the torrents (the list and every torrent's details) and what Undo would undo this
    /// run, as the engine counts them: what's watched isn't one. Neither is it in the real service, whose snapshots
    /// have every watched torrent's details at once; the fake's slow details are its own, so their
    /// coming, or a new wait for some, is a change too. Otherwise one `(epoch, version, watching)`
    /// would be sent with and without them, against the protocol (#148).
    version: u64,
    /// The torrents as of `version`, the watched ones whose details are still on their way, and
    /// what Undo would undo.
    published: Option<(Vec<TorrentInfo>, Vec<Details>, BTreeSet<TorrentId>, Vec<Undoable>, Vec<proto::Left>, Option<Unavailable>, u64)>,
    /// Seconds a command's own snapshot and answer take, in turn: as with the real service, whose
    /// commands each run on a connection of their own, so they can land after newer snapshots
    /// from the stream, and out of order.
    stale_answers: f64,
    /// How long Delete's and Move to Trash's trashing takes; the torrents (by name) whose files the
    /// Trash refuses.
    trash_secs: f64,
    trash_refusing: BTreeSet<String>,
    /// The torrents whose Delete moves some of their files and not the rest.
    trash_partly: BTreeSet<String>,
    /// The torrents (by name) whose undo can't be carried out: emptied from the Trash, say.
    undo_refusing: BTreeSet<String>,
    /// The steps that waited behind an undo bringing their torrent back that failed: one that fails
    /// in turn is answered `Undone`, as the engine's.
    orphaned: BTreeSet<(u64, TorrentId)>,
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
    keep_awake: bool,
    speed_limits: proto::SpeedLimits,
    seeding: proto::SeedingLimits,
    /// Folders a default destination can't be set to, and why.
    unreachable: BTreeMap<PathBuf, Unavailable>,
    /// Ports another program listens on.
    taken: BTreeSet<NonZeroU16>,
    /// A new port or UPnP fails, as when the torrents don't start again with them.
    restarts_fail: bool,
    /// The settings changes an `UndoSetting` takes back, each with the settings before it, as the
    /// real service's (#221); and its count of changes of the settings ([`proto::Configured::at`]).
    setting_changes: lifecycle::settings::Changes<Settings>,
    settings_at: u64,
    /// Each torrent's destination, unless it's the default one.
    destinations: BTreeMap<TorrentId, PathBuf>,
    disk: Disk,
    /// What's free on the one disk every torrent downloads to.
    free_bytes: u64,
    /// The app clock's 0, as an instant, for the seeding meters.
    epoch_instant: std::time::Instant,
    /// Undo's machine, the engine's own: the undo stack and each torrent's lane (`steps.rs`); the
    /// Undos asked for, handled at the next poll.
    undo: steps::Machine,
    world: lifecycle::undo::World,
    undos: Vec<Done<Result<proto::Undone, Failure>>>,
    /// An undo takes this long on the app clock to put back (instant by default), as the real
    /// service's does while it moves files.
    undo_secs: f64,
    /// Each command's steps and its answer, and each step's command.
    batches: BTreeMap<usize, steps::Batch>,
    batch_of: BTreeMap<(u64, TorrentId), usize>,
    next_batch: usize,
    /// The client waiting for each Undo, by the action it takes back, with what that action was when
    /// the Undo came.
    undo_replies: BTreeMap<u64, (Done<Result<proto::Undone, Failure>>, Option<Undoable>)>,
    /// How each part of an undo went so far, by the action it takes back.
    tallies: BTreeMap<u64, steps::Tally>,
    /// Each step's part under way: when on the app clock it ends.
    due: BTreeMap<(u64, TorrentId), f64>,
    /// Answers to hand over at the next poll, each after its delay.
    ready: Vec<(Incoming, f64)>,
}

/// A torrent that left the list, as it was before the command that took it off: what brings it
/// back.
#[derive(Clone)]
struct Left {
    action: UndoAction,
    info: TorrentInfo,
    files: Option<Vec<File>>,
    machine: Torrent,
    destination: Option<PathBuf>,
    timing: Option<Timing>,
    /// A Move to Trash (#151): the torrent never left the list, so `info` is only its id and name.
    trashed: Option<FilesTrashed>,
}

/// What a Move to Trash changed: each file asked about with the priority it had, and what was at the
/// path of each it moved.
#[derive(Clone)]
struct FilesTrashed {
    previous: Vec<(usize, Priority)>,
    moved: Vec<(usize, Stamp)>,
}

impl PartialEq for Left {
    fn eq(&self, other: &Self) -> bool {
        (self.action, self.info.id) == (other.action, other.info.id)
    }
}

/// As the engine's: a Delete of a torrent takes the place of its partial Delete before it (by id).
impl lifecycle::undo::Undoes for Left {
    fn absorbs(&self, older: &Self) -> bool {
        self.action == UndoAction::Delete && older.action == UndoAction::Delete && self.info.id == older.info.id
    }

    fn named(&self) -> (UndoAction, proto::Named) {
        (self.action, proto::Named { id: self.info.id, name: self.info.display_name().to_owned() })
    }
}

impl Left {
    fn undoable(&self, seq: u64) -> Undoable {
        Undoable { seq, action: self.action, torrents: vec![proto::Named { id: self.info.id, name: self.info.display_name().to_owned() }] }
    }
}

/// A command in the mailbox: for these torrents, answered through `done` with those that failed.
struct Message {
    ids: Vec<TorrentId>,
    command: TorrentCommand,
    done: Done<Vec<(TorrentId, Failure)>>,
}

/// A torrent as the fake runs it: the shared lifecycle, and what's around it.
#[derive(Clone)]
struct Torrent {
    state: State,
    librqbit: Pretend,
    /// What's at each file's path, by index: `None`, nothing.
    on_disk: BTreeMap<usize, Option<Stamp>>,
    /// Why its destination can't be reached, while it can't.
    unavailable: Option<Unavailable>,
    /// Its seeding round, and the pretend librqbit's upload counter, which starts again with it.
    meter: Meter,
    live_uploaded: u64,
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
    /// The default destination can't be reached, for `reason`, or can again (#209): the next
    /// snapshot says why, and setting it is refused for it while it can't, as any folder
    /// [`unreachable`](MemoryService::unreachable) is.
    #[cfg_attr(not(test), allow(dead_code))]
    DefaultGone { reason: Unavailable },
    #[cfg_attr(not(test), allow(dead_code))]
    DefaultBack,
    /// What's free on the disk the torrents download to is this now (space freed, or filled).
    #[cfg_attr(not(test), allow(dead_code))]
    FreeBytes(u64),
    /// Not the disk, but from outside too: the service restarts, with every torrent as it was.
    /// Its snapshots are a new epoch, counted afresh.
    #[cfg_attr(not(test), allow(dead_code))]
    Restarted,
    /// Another client (`phantom settings set …`) changes a setting, or undoes the newest settings
    /// change (`phantom settings undo`), as the real service has them (#221).
    #[cfg_attr(not(test), allow(dead_code))]
    SetByAnother(Setting),
    #[cfg_attr(not(test), allow(dead_code))]
    UndoneByAnother,
    /// From now, a new port or UPnP fails, or doesn't, as [`MemoryService::failing_restarts`].
    #[cfg_attr(not(test), allow(dead_code))]
    RestartsFailing(bool),
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
#[derive(Clone)]
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
            trash_partly: BTreeSet::new(),
            undo_refusing: BTreeSet::new(),
            orphaned: BTreeSet::new(),
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
            keep_awake: false,
            speed_limits: proto::SpeedLimits::default(),
            seeding: proto::SeedingLimits::default(),
            unreachable: BTreeMap::new(),
            taken: BTreeSet::new(),
            restarts_fail: false,
            setting_changes: lifecycle::settings::Changes::new(1),
            settings_at: 1,
            destinations: BTreeMap::new(),
            disk: Disk::default(),
            free_bytes: 500 << 30,
            epoch_instant: std::time::Instant::now(),
            undo: Default::default(),
            world: Default::default(),
            undos: Vec::new(),
            undo_secs: 0.0,
            batches: BTreeMap::new(),
            batch_of: BTreeMap::new(),
            next_batch: 0,
            undo_replies: BTreeMap::new(),
            tallies: BTreeMap::new(),
            due: BTreeMap::new(),
            ready: Vec::new(),
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

    /// Delete and Move to Trash take `secs` on the app clock to move a torrent's files to the Trash;
    /// the list shows them done at once, and the command is answered after.
    pub fn trash_takes(mut self, secs: f64) -> Self {
        self.trash_secs = secs;
        self
    }

    /// Each undo takes `secs` on the app clock to put back; the list shows it back at once, and other
    /// torrents' undos go on beside it.
    pub fn undo_takes(mut self, secs: f64) -> Self {
        self.undo_secs = secs;
        self
    }

    /// The Trash refuses the files of the torrent named `name`, as a locked file: its Delete
    /// moves nothing, and fails.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn trash_refusing(mut self, name: &str) -> Self {
        self.trash_refusing.insert(name.to_owned());
        self
    }

    /// The Trash moves some of the files of the torrent named `name` and refuses the rest: its
    /// Delete fails, and it stays partly deleted, as does a Move to Trash that moved its first file.
    /// What moved is on the undo stack, as the engine's (ADR 0008).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn trash_partly(mut self, name: &str) -> Self {
        self.trash_partly.insert(name.to_owned());
        self
    }

    /// The undo of the torrent named `name` can't be carried out: what it moved was emptied from the
    /// Trash.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn undo_refusing(mut self, name: &str) -> Self {
        self.undo_refusing.insert(name.to_owned());
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
            self.machines.insert(t.id, Torrent { state, librqbit, on_disk, unavailable, meter: Meter::default(), live_uploaded: 0 });
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
            DiskChange::SetByAnother(setting) => {
                let _ = self.change_setting(setting, None);
                return;
            }
            DiskChange::UndoneByAnother => {
                let _ = self.undo_setting_now();
                return;
            }
            DiskChange::RestartsFailing(failing) => {
                self.restarts_fail = failing;
                return;
            }
            DiskChange::DefaultGone { reason } => {
                self.unreachable.extend(self.default_destination.clone().map(|folder| (folder, reason)));
                self.changed = true;
                return;
            }
            DiskChange::FreeBytes(bytes) => {
                self.free_bytes = bytes;
                self.changed = true;
                return;
            }
            DiskChange::DefaultBack => {
                if let Some(folder) = &self.default_destination {
                    self.unreachable.remove(folder);
                }
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

    /// `bytes` free on the disk every torrent downloads to (500 GiB unless said).
    pub fn with_free_space(mut self, bytes: u64) -> Self {
        self.free_bytes = bytes;
        self
    }

    /// The one disk, as the service sends it: each torrent that counts there
    /// ([`lifecycle::counts`]: wanted running, its destination not unavailable), with info,
    /// needing what's left of the files it gets. Its files are taken to be sparse, as on APFS:
    /// what's left isn't on disk yet. A check keeps what each file has, so never raises it.
    fn volumes(&self) -> Vec<Volume> {
        let needs = self.torrents.iter().filter_map(|t| {
            self.machines.get(&t.id).filter(|m| lifecycle::counts(&m.state))?;
            let content = t.content?;
            let bytes = content.size_bytes.saturating_sub(content.progress_bytes);
            (bytes > 0).then_some(Needs { id: t.id, bytes })
        });
        let needs: Vec<Needs> = needs.collect();
        if needs.is_empty() {
            return Vec::new();
        }
        vec![Volume {
            mount: AbsolutePath::new("/System/Volumes/Data").expect("absolute"),
            name: "Macintosh HD".into(),
            free_bytes: Some(self.free_bytes),
            needs,
        }]
    }

    /// When a finished torrent stops seeding, as if set.
    pub fn with_seeding(mut self, limits: proto::SeedingLimits) -> Self {
        self.seeding = limits;
        self
    }

    /// Setting `folder` as the default destination is refused, for `reason`; an undo back to it
    /// sets it all the same, and says why it can't be reached.
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
            default_destination_unavailable: self.default_unavailable(),
            listen_port: self.listen_port,
            listening_on,
            upnp: self.upnp,
            keep_awake: self.keep_awake,
            speed_limits: self.speed_limits,
            seeding: self.seeding,
            reset: Vec::new(),
        }
    }

    /// Why the default destination can't be reached, while there is one and it can't.
    fn default_unavailable(&self) -> Option<Unavailable> {
        self.default_destination.as_ref().and_then(|folder| self.unreachable.get(folder).cloned())
    }

    /// As the real service: a port set is pinned, unset keeps it as picked; a picked port another
    /// program has is replaced.
    fn set(&mut self, setting: Setting) -> Result<(), Refusal> {
        match setting {
            // A folder set is one that can be reached.
            Setting::DefaultDestination(Some(folder)) => match self.unreachable.get(&*folder) {
                Some(reason) => return Err(Refusal::DefaultDestination { reason: reason.clone() }),
                None => {
                    self.default_destination = Some(folder.into_path_buf());
                    self.changed = true;
                }
            },
            Setting::DefaultDestination(None) => {
                self.default_destination = None;
                self.changed = true;
            }
            Setting::ListenPort(Some(port)) => self.listen_port = ListenPort::Pinned(port),
            Setting::ListenPort(None) => {
                let port = self.listen_port.port();
                let free = (1..=u16::MAX).rev().filter_map(NonZeroU16::new).find(|p| !self.taken.contains(p));
                let port = if self.taken.contains(&port) { free.expect("a free port") } else { port };
                self.listen_port = ListenPort::Picked(port);
            }
            Setting::Upnp(upnp) => self.upnp = upnp.unwrap_or(true),
            Setting::KeepAwake(on) => self.keep_awake = on.unwrap_or(false),
            Setting::StopSeeding { at, value } => {
                let refused = |problem| Refusal::StopSeeding { at, problem };
                match at {
                    proto::SeedingLimit::Ratio => self.seeding.ratio = value.map(proto::Ratio::new).transpose().map_err(refused)?,
                    proto::SeedingLimit::Time => self.seeding.time = value.map(proto::SeedTime::new).transpose().map_err(refused)?,
                }
            }
            Setting::SpeedLimit { direction, bytes_per_sec } => {
                let limit = bytes_per_sec.map(proto::SpeedLimit::new).transpose();
                self.speed_limits.set(direction, limit.map_err(|problem| Refusal::SpeedLimit { direction, problem })?);
            }
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
            m.meter.wants_changed(m.state.wants, next.wants);
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
        let todo = lifecycle::next(&m.state, m.librqbit.lib());
        if matches!(todo, Some(Do::Start | Do::AddAgain)) {
            m.live_uploaded = 0;
            m.meter.librqbit_started();
        }
        match todo {
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
        let torrents = self.listing();
        let all = torrents.iter().filter_map(|t| self.details(t.id)).collect();
        let undo = self.stacked();
        let left: Vec<proto::Left> = self.undo.left.iter().cloned().collect();
        let unavailable = self.default_unavailable();
        let now = Some((
            torrents.clone(),
            all,
            self.details_asked.keys().copied().collect(),
            undo.clone(),
            left.clone(),
            unavailable.clone(),
            self.free_bytes,
        ));
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
            torrents,
            details,
            reset: self.reset.clone(),
            volumes: self.volumes(),
            undo,
            left,
            default_destination_unavailable: unavailable,
            settings: Some(proto::Configured {
                at: self.settings_at,
                settings: self.current_settings(),
                changes: self.setting_changes.listed(),
            }),
        };
        #[cfg(test)]
        self.keeps_the_protocol(&snapshot);
        snapshot
    }

    /// The protocol's promise: one snapshot per `(epoch, version, watching)`, but for its settings,
    /// which go by their own `at`. Two that differ under one key would let the app take an older
    /// one for the latest (#148).
    #[cfg(test)]
    fn keeps_the_protocol(&mut self, snapshot: &Snapshot) {
        let key = (snapshot.epoch, snapshot.version, snapshot.watching.clone());
        let torrents = Snapshot { settings: None, ..snapshot.clone() };
        let first = self.sent.entry(key).or_insert_with(|| torrents.clone());
        assert_eq!(*first, torrents, "two different snapshots for one (epoch, version, watching)");
    }

    /// `setting`, as `SetSetting` `by` the client's id: one that restarts fails while restarts do,
    /// changing nothing; one that changes the value is a settings change. The settings now.
    fn change_setting(&mut self, setting: Setting, by: Option<u64>) -> Result<proto::SettingSet, Failure> {
        let name = setting.name();
        let restarts = matches!(name, proto::SettingName::ListenPort | proto::SettingName::Upnp);
        if restarts && self.restarts_fail {
            return Err(Failure::Failed(proto::ServiceError::NotRestarted { detail: "no room".into() }));
        }
        let was = self.current_settings();
        self.set(setting).map_err(Failure::Refused)?;
        let now = self.current_settings();
        let change = (with_field(name, &now, &was) != now).then(|| self.setting_changes.push(name, by, was));
        self.settings_changed(restarts);
        Ok(proto::SettingSet { settings: now, change, at: self.settings_at })
    }

    /// Takes back the newest settings change, as `UndoSetting`: refused, as its value set by hand
    /// would be, it's dropped all the same; a default folder goes back reachable or not.
    fn undo_setting_now(&mut self) -> Result<proto::SettingUndone, Failure> {
        let (change, was) = self.setting_changes.take_newest().ok_or(Failure::Refused(Refusal::NothingToUndo))?;
        let restarts = matches!(change.name, proto::SettingName::ListenPort | proto::SettingName::Upnp);
        if restarts && self.restarts_fail {
            self.settings_changed(false);
            return Err(Failure::Failed(proto::ServiceError::NotRestarted { detail: "no room".into() }));
        }
        let back = with_field(change.name, &self.current_settings(), &was);
        self.default_destination = back.default_destination.map(AbsolutePath::into_path_buf);
        (self.listen_port, self.upnp, self.keep_awake) = (back.listen_port, back.upnp, back.keep_awake);
        (self.speed_limits, self.seeding) = (back.speed_limits, back.seeding);
        self.settings_changed(restarts);
        Ok(proto::SettingUndone { change, settings: self.current_settings() })
    }

    /// The settings changed: every client hears of it; a new port or UPnP started the engine again,
    /// so its snapshots are a new epoch.
    fn settings_changed(&mut self, restarted: bool) {
        self.settings_at += 1;
        self.changed = true;
        if restarted {
            self.change(DiskChange::Restarted);
        }
    }

    /// The files of torrent `id` it has found missing, by index.
    fn missing(&self, id: TorrentId) -> BTreeSet<usize> {
        match self.machines.get(&id).map(|m| &m.state.phase) {
            // As the engine's: one whose check was cancelled with files returning misses them too.
            Some(Phase::MissingFiles { gone } | Phase::Unchecked { gone }) => gone.keys().copied().collect(),
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
        // Not listed while it's leaving: as it was.
        let Some(status) = m.state.status(complete) else { return };
        // As the engine's: its check was cancelled, so nothing is known of its progress, a Delete
        // under way or partly done or not.
        if m.state.phase.unchecked()
            && let Some(content) = &mut t.content
        {
            content.progress_bytes = 0;
        }
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

    /// Torrent `id`'s details as clients see them: none while an Undo brings it back, its files'
    /// priorities as the steps on its lane make them.
    fn details(&self, id: TorrentId) -> Option<Details> {
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let lifecycle::undo::view::Shown { was: None, priorities, .. } = self.shown(id)? else { return None };
        let files: Vec<File> = self
            .files
            .get(&id)?
            .iter()
            .map(|f| File { priority: priorities.get(&f.index).copied().unwrap_or(f.priority), ..f.clone() })
            .collect();
        let files = &files;
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

        // Seeding is counted, and stops at a limit, as the engine does it.
        let limits = lifecycle::seeding::Limits::from(self.seeding);
        let mut reached = Vec::new();
        for t in &self.torrents {
            let Some(m) = self.machines.get_mut(&t.id) else { continue };
            m.live_uploaded += (t.upload_bytes_per_sec as f64 * dt) as u64;
            let size = t.content.map_or(0, |c| c.size_bytes);
            let at = self.epoch_instant + std::time::Duration::from_secs_f64(now);
            if let Some(limit) = m.meter.poll(m.live_uploaded, &t.status, size, &limits, at) {
                reached.push((t.id, limit));
            }
        }
        for (id, limit) in reached {
            self.event(id, Event::Observed(Observation::LimitReached(limit)));
            self.refresh(id);
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

    /// A command that's no step of the stack, for torrent `id`, carried out now; refused, as the
    /// engine's, for one not in the list or one its lifecycle refuses it for.
    fn command_now(&mut self, id: TorrentId, command: &TorrentCommand) -> Option<(TorrentId, Failure)> {
        let refused = |why| Some((id, Failure::Refused(Refusal::Torrent { id, why })));
        let Some(m) = self.machines.get(&id) else { return refused(TorrentRefusal::NotInList) };
        if let Err(why) = lifecycle::decide(&m.state, &Event::Command(command.action())) {
            return refused(why);
        }
        self.event(id, Event::Command(command.action()));
        match command {
            TorrentCommand::SetPriority { files, priority } => {
                for f in self.files.get_mut(&id).into_iter().flatten().filter(|f| files.contains(&f.index)) {
                    f.priority = *priority;
                }
                // Files set Off aren't missing any more, and those set Normal may be.
                self.size_check(id);
            }
            TorrentCommand::Remove | TorrentCommand::Delete | TorrentCommand::TrashFiles { .. } => {
                unreachable!("a step of the stack is `steps.rs`'s")
            }
            TorrentCommand::Pause | TorrentCommand::Resume | TorrentCommand::Check | TorrentCommand::CancelCheck => {}
        }
        self.refresh(id);
        self.changed = true;
        None
    }

    /// Move to Trash of torrent `id`, its files Off already: each asked for that's on disk is gone
    /// from it (unless the Trash refuses the torrent's files: then none moves and it fails). What
    /// moved undoes it, the priorities they had too.
    fn files_trashed(&mut self, id: TorrentId, run: &steps::Run) -> (Option<Left>, Option<(TorrentId, Failure)>) {
        let Some(name) = self.torrents.iter().find(|t| t.id == id).map(|t| t.display_name().to_owned()) else { return (None, None) };
        // Refused moves none; partly moves the first file and fails on the next, as a locked one would.
        let partly = self.trash_partly.contains(&name);
        let refused = partly || self.trash_refusing.contains(&name);
        let mut moved = Vec::new();
        if let (false, Some(machine)) = (self.trash_refusing.contains(&name), self.machines.get_mut(&id)) {
            for (index, _) in &run.previous {
                if let Some(stamp) = machine.on_disk.get_mut(index).and_then(Option::take) {
                    moved.push((*index, stamp));
                    if partly {
                        break;
                    }
                }
            }
        }
        self.size_check(id);
        self.refresh(id);
        self.changed = true;
        let left = (!moved.is_empty()).then(|| Left {
            action: UndoAction::TrashFiles { count: moved.len() },
            trashed: Some(FilesTrashed { previous: run.previous.clone(), moved }),
            ..run.left.clone()
        });
        if !refused {
            return (left, None);
        }
        let failed = AbsolutePath::new(downloads().join(&name)).ok().map(|path| {
            let kept = vec![Kept { path, why: KeptWhy::Failed { detail: "it's locked".into() } }];
            (id, Failure::Refused(Refusal::Torrent { id, why: TorrentRefusal::FilesNotTrashed { name, kept } }))
        });
        (left, failed)
    }

    /// The pretend trashing of torrent `id` ends: all of its files moved, and it's off the list;
    /// or, refused, none, and it's as it was, the Delete failing with why.
    fn trashed(&mut self, id: TorrentId) -> Option<(TorrentId, Failure)> {
        let torrent = self.torrents.iter().find(|t| t.id == id)?;
        let name = torrent.display_name().to_owned();
        let partly = self.trash_partly.contains(&name);
        let refused = partly || self.trash_refusing.contains(&name);
        let location = self.location(torrent).or_else(|| AbsolutePath::new(downloads().join(torrent.display_name())).ok());
        let kept: Vec<Kept> = match location {
            Some(path) if refused => vec![Kept { path, why: KeptWhy::Failed { detail: "it's locked".into() } }],
            _ => Vec::new(),
        };
        self.event(id, Event::Observed(Observation::Trashed { kept: kept.clone(), moved: partly }));
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
        let failed = |error: proto::ServiceError| AddError::Failed(Failure::Failed(error));
        // A .torrent file has its info at once.
        let (torrent, has_info) = match source {
            AddRequest::File(path) if self.rejecting.contains(&*path.to_string_lossy()) => {
                return Err(failed(proto::ServiceError::Internal { detail: "not a valid .torrent file".into() }));
            }
            AddRequest::File(path) => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                (self.new_torrent(name, None), true)
            }
            AddRequest::Link { text, .. } if self.rejecting_links || self.rejecting.contains(&text) => {
                return Err(failed(proto::ServiceError::Internal { detail: "no peers have this torrent".into() }));
            }
            AddRequest::Link { source, .. } => {
                let (name, info_hash, trackers) = implied(&source);
                if let Some(t) = self.torrents.iter().find(|t| t.info_hash == info_hash) {
                    return Ok(Added { id: t.id, name: t.name.clone(), already_existed: true });
                }
                let mut torrent = self.new_torrent(name, Some(info_hash));
                torrent.trackers = trackers;
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
            meter: Meter::default(),
            live_uploaded: 0,
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
            AddRequest::Link { source: AddSource::TorrentUrl(_), .. } => self.torrent_url_secs,
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

    /// What brings torrent `id` back as it is now, once `action` takes it off the list.
    fn keep(&self, id: TorrentId, action: UndoAction) -> Left {
        let info = self.torrents.iter().find(|t| t.id == id).cloned().unwrap_or_else(|| self.blank(id));
        let machine = self.machines.get(&id).cloned().unwrap_or_else(|| Torrent {
            state: State::new(Wants::Run, Phase::FetchingInfo),
            librqbit: Pretend::Stopped,
            on_disk: BTreeMap::new(),
            unavailable: None,
            meter: Meter::default(),
            live_uploaded: 0,
        });
        Left {
            action,
            info,
            files: self.files.get(&id).cloned(),
            machine,
            destination: self.destinations.get(&id).cloned(),
            timing: self.timings.iter().find(|t| t.id == id).cloned(),
            trashed: None,
        }
    }

    /// A torrent's info with nothing known of it but its id.
    fn blank(&self, id: TorrentId) -> TorrentInfo {
        TorrentInfo {
            id,
            info_hash: format!("{id:040x}"),
            name: None,
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

    /// Carries out the undo of action `of`, `left`: the torrent back as it was before it left, under its id.
    fn undone(&mut self, left: &Left, of: u64) -> Result<Undoable, Failure> {
        self.changed = true;
        if let (Some(trashed), UndoAction::Delete) = (&left.trashed, left.action) {
            return self.partial_delete_back(left, trashed, of);
        }
        if let Some(trashed) = &left.trashed {
            return self.files_back(left, trashed, of);
        }
        if self.torrents.iter().any(|t| t.info_hash == left.info.info_hash) {
            return Err(Failure::Refused(Refusal::Undo { undo: left.undoable(of), why: proto::CantUndo::InList, stranded: Vec::new() }));
        }
        if self.undo_refusing.contains(left.info.display_name()) {
            let path = AbsolutePath::new(downloads().join(left.info.display_name())).expect("absolute");
            let why = proto::CantUndo::NotInTrash { path };
            return Err(Failure::Refused(Refusal::Undo { undo: left.undoable(of), why, stranded: Vec::new() }));
        }
        let Left { info, files, machine, destination, timing, .. } = left.clone();
        let id = info.id;
        self.torrents.push(info);
        self.torrents.sort_by_key(|t| t.id);
        self.files.extend(files.map(|f| (id, f)));
        self.machines.insert(id, machine);
        self.destinations.extend(destination.map(|d| (id, d)));
        self.timings.extend(timing);
        self.size_check(id);
        self.refresh(id);
        self.changed = true;
        Ok(left.undoable(of))
    }

    /// Undo of a Move to Trash: only for a torrent still in the list that moving files to the Trash would do
    /// something to now; its files are back where they were and each has the priority it had.
    fn files_back(&mut self, left: &Left, trashed: &FilesTrashed, of: u64) -> Result<Undoable, Failure> {
        let id = left.info.id;
        let refuse = |why| Err(Failure::Refused(Refusal::Undo { undo: left.undoable(of), why, stranded: Vec::new() }));
        let same = self.torrents.iter().any(|t| t.id == id && t.info_hash == left.info.info_hash);
        let Some(machine) = self.machines.get(&id).filter(|_| same) else { return refuse(proto::CantUndo::NotInList) };
        if let proto::Answer::Refused { why } = lifecycle::answer(&machine.state, proto::Action::TrashFiles) {
            return refuse(match why {
                TorrentRefusal::DestinationUnavailable { reason, .. } => proto::CantUndo::Destination { reason },
                _ => proto::CantUndo::Deleting,
            });
        }
        if let Some(m) = self.machines.get_mut(&id) {
            for (index, stamp) in &trashed.moved {
                m.on_disk.insert(*index, Some(*stamp));
            }
        }
        for f in self.files.get_mut(&id).into_iter().flatten() {
            if let Some((_, priority)) = trashed.previous.iter().find(|(i, _)| *i == f.index) {
                f.priority = *priority;
            }
        }
        self.size_check(id);
        self.refresh(id);
        self.changed = true;
        Ok(left.undoable(of))
    }

    /// The undo of a partly done Delete: its torrent never left the list, and goes back to what it was
    /// before, with the file that moved where it was.
    fn partial_delete_back(&mut self, left: &Left, trashed: &FilesTrashed, of: u64) -> Result<Undoable, Failure> {
        let id = left.info.id;
        let refuse = |why| Err(Failure::Refused(Refusal::Undo { undo: left.undoable(of), why, stranded: Vec::new() }));
        if !self.torrents.iter().any(|t| t.id == id && t.info_hash == left.info.info_hash) {
            return refuse(proto::CantUndo::NotInList);
        }
        let mut machine = left.machine.clone();
        for (index, stamp) in &trashed.moved {
            machine.on_disk.insert(*index, Some(*stamp));
        }
        self.machines.insert(id, machine);
        self.size_check(id);
        self.refresh(id);
        self.changed = true;
        Ok(left.undoable(of))
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
        Status::Paused | Status::Done { reached: None } => (state(Wants::Stop, Phase::Ready), Pretend::Stopped, None),
        Status::Done { reached: Some(reached) } => (state(Wants::Seeded { reached: *reached }, Phase::Ready), Pretend::Stopped, None),
        Status::Checking => (state(Wants::Run, Phase::Checking { returning: BTreeMap::new() }), Pretend::Checking { since: None }, None),
        Status::Unchecked => (state(Wants::Stop, Phase::Unchecked { gone: BTreeMap::new() }), Pretend::Stopped, None),
        Status::DestinationUnavailable { reason } => {
            (state(Wants::Run, Phase::DestinationUnavailable { reason: reason.clone() }), Pretend::Stopped, Some(reason.clone()))
        }
        Status::Error { failure } => {
            let phase = match failure.clone() {
                proto::TorrentFailure::FetchFailed { detail } => Phase::FetchFailed { detail },
                proto::TorrentFailure::AddAgainFailed { detail } => Phase::Error { failure: lifecycle::Broke::AddAgainFailed { detail } },
                proto::TorrentFailure::Librqbit { detail } => Phase::Error { failure: lifecycle::Broke::Librqbit { detail } },
                proto::TorrentFailure::DiskFull => Phase::Error { failure: lifecycle::Broke::DiskFull },
                proto::TorrentFailure::NotTrashed { kept } => Phase::DeleteFailed { kept, away: None, unchecked: false },
            };
            (state(Wants::Run, phase), Pretend::Failed, None)
        }
        Status::MissingFiles => panic!("seed it running and give it missing files with `with_missing`"),
        Status::Moving => panic!("no torrent moves yet (#24)"),
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

/// The fake's default destination, unless told otherwise: the same on every Mac, so nothing a test
/// keeps (a shrunk failing case) names the person running it.
pub const DOWNLOADS: &str = "/Users/me/Downloads";

fn downloads() -> PathBuf {
    PathBuf::from(DOWNLOADS)
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

    /// Sent to the mailbox, handled at the next poll.
    fn undo(&mut self, done: Done<Result<proto::Undone, Failure>>) {
        self.undos.push(done);
    }

    fn settings(&mut self, done: Done<Result<Settings, Failure>>) {
        self.incoming.push(answer(done, Ok(self.current_settings())));
    }

    fn set_setting(&mut self, setting: Setting, by: Option<u64>, done: Done<Result<proto::SettingSet, Failure>>) {
        let result = self.change_setting(setting, by);
        self.incoming.push(answer(done, result));
    }

    fn undo_setting(&mut self, done: Done<Result<proto::SettingUndone, Failure>>) {
        let result = self.undo_setting_now();
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
        // Commands and Undos in the order they came: each a step at once (`steps.rs`).
        for Message { ids, command, done } in std::mem::take(&mut self.mailbox) {
            self.arrive_all(&ids, command, done);
            self.run_steps(now);
        }
        for done in std::mem::take(&mut self.undos) {
            self.undo(done);
            self.run_steps(now);
        }
        self.steps_due(now);
        let answered = self.take_ready();
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
            || !self.undos.is_empty()
            || !self.due.is_empty()
            || !self.late.is_empty()
            || !self.timings.is_empty()
            || self.machines.values().any(|m| matches!(m.librqbit, Pretend::Checking { .. }))
            || !self.disk.0.lock().unwrap().is_empty()
            || self.torrents.iter().any(|t| matches!(t.status, Status::Downloading | Status::Seeding));
        active.then_some(Duration::from_millis(500))
    }
}

/// `now` with the setting `name` as it is in `was`, as the real service's.
fn with_field(name: proto::SettingName, now: &Settings, was: &Settings) -> Settings {
    use proto::SettingName;
    let mut back = now.clone();
    match name {
        SettingName::DefaultDestination => back.default_destination = was.default_destination.clone(),
        SettingName::ListenPort => back.listen_port = was.listen_port,
        SettingName::Upnp => back.upnp = was.upnp,
        SettingName::KeepAwake => back.keep_awake = was.keep_awake,
        SettingName::DownloadLimit => back.speed_limits.download = was.speed_limits.download,
        SettingName::UploadLimit => back.speed_limits.upload = was.speed_limits.upload,
        SettingName::SeedRatio => back.seeding.ratio = was.seeding.ratio,
        SettingName::SeedTime => back.seeding.time = was.seeding.time,
    }
    back
}

/// The display name, info hash and trackers an add's source implies, as the real service reports
/// them: a magnet's; for a .torrent URL, whose file the fake doesn't download, its file name and a
/// hash made from the URL.
fn implied(source: &AddSource) -> (Option<String>, String, Vec<String>) {
    match source {
        AddSource::Magnet(magnet) => match client::parse_link(magnet) {
            Ok(client::Link::Magnet { info_hash, name, trackers }) => (name, info_hash.to_ascii_lowercase(), trackers),
            other => panic!("the client sends a magnet link as one, not {other:?}"),
        },
        AddSource::TorrentUrl(url) => {
            let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
            let hash = url.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
            (file.map(|f| f.trim_end_matches(".torrent").to_owned()), format!("{hash:040x}"), Vec::new())
        }
        AddSource::TorrentFile(_) => panic!("a file is added by its path here"),
    }
}
