//! The actor: one task that owns every torrent, its lifecycle ([`State`]) and its librqbit side
//! ([`InLib`]), and librqbit's session, and handles one [`Message`] at a time: client commands
//! (each answered through its own reply channel), the end of an add, a regular poll of librqbit,
//! and a size check of the files on disk (every few seconds, at once when the watcher sees a path
//! go: `files.rs`, and before a command, so its answer doesn't depend on how soon the watcher
//! noticed). Each is an [`Event`] for [`decide`]; after it, [`machine::next`] says what makes
//! librqbit match and [`inlib::next`] how that's done on its librqbit side. The actor reports
//! facts (what librqbit is doing, what's on disk, how an add ended) and the machines (`machine/`)
//! decide what they mean. Slow work (fetching info, downloading a .torrent URL, moving files to
//! the Trash) runs in its own task and comes back as a message: the actor never waits on it.
//! After each, the actor publishes the [`Snapshot`] its state makes, if it changed, and only then
//! answers: an answer's effect is in what clients read next.
//!
//! librqbit only runs a torrent when the actor starts it: every add, and every add again, is
//! stopped, and a torrent wanted running is started once it's Ready.
//!
//! Missing files: a size check reports the files a torrent is getting and what's at each path; the
//! machine decides which are missing, set Off or back. One missing stops in librqbit (so nothing
//! more is written into the deleted file). Resume and Check check it (take it out of librqbit
//! and add it again, which opens the files afresh). A size check follows every change of a
//! torrent's phase, and a priority change, so what's on disk is judged in the phase it's now in:
//! e.g. the file a check just found isn't the one that went is recorded as it is.
//!
//! Destination unavailable: before its files, a torrent's destination is size-checked (every few
//! seconds, when the watcher sees a path go and before a command). One that can't be reached stops the torrent in
//! librqbit until it's back; then it's checked. librqbit creates a torrent's files whenever it
//! adds one: [`Guard`] knows every torrent's destination from its add, and gives one that can't be
//! reached then storage that touches nothing, so no add, add again or restore at start makes
//! folders where a drive isn't.
//!
//! What's wanted (run or stop) is saved in the store whenever it changes, so a start, after a stop
//! or a crash, makes librqbit match it. At stop, every torrent's missing files and the files it's
//! getting are recorded too, and running ones are paused so librqbit saves them paused. At start,
//! torrents whose destination can't be reached are Destination unavailable; the others' recorded
//! files are checked before librqbit restores its torrents (restoring creates missing files again,
//! empty), and torrents missing some are Missing files.
//!
//! Undo (`undo.rs` here, and the stack's machine in `../undo/`): a Remove, a Delete and a Move to
//! Trash are steps on the undo stack from the moment their command comes (`steps.rs`), each torrent's
//! work on a lane of its own, in order; an Undo takes the top step at once, and puts back what a
//! Delete moved, on a blocking task like the trashing, then adds the torrent again from what was
//! kept of it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentStats, TorrentStatsState};
use proto::{
    AbsolutePath, Added, Content, Details, File, Kept, KeptWhy, Priority, Status, TorrentCommand, TorrentId, TorrentInfo, Unavailable,
    Undoable,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::warn;

use super::bitv;
use super::files::{self, Expected, Gone, Guard, Seen, Watcher};
use super::machine::inlib;
use super::machine::{self, Broke, Event, Lib, Observation, Phase, Refusal, State, Wants, decide, seeding};
use super::snapshot::{Published, Snapshot};
use super::space;
use super::store::{self, Store};
use super::trash::{self, Trash, Trashed};
use super::undo;
use super::{Failure, Prepared};

mod filetrash;
mod steps;
mod undoing;

/// How an add reaches librqbit: [`add_into`], or in tests one whose waits the test controls.
pub(super) type AddInto = Arc<dyn Fn(Arc<Session>, TorrentId, AddTorrent<'static>, PathBuf, PathBuf, Guard) -> Adding + Send + Sync>;
pub(super) type Adding = Pin<Box<dyn Future<Output = Result<AddTorrentResponse>> + Send>>;

pub(super) enum Message {
    Add {
        prepared: Prepared,
        paused: bool,
        reply: oneshot::Sender<Result<Added, Failure>>,
    },
    Command {
        id: TorrentId,
        command: TorrentCommand,
        reply: oneshot::Sender<Result<(), Failure>>,
    },
    /// librqbit's add of torrent `id` ended.
    AddFinished {
        id: TorrentId,
        added: Result<AddTorrentResponse>,
    },
    /// The watcher saw these paths removed or renamed.
    FilesChanged(Vec<PathBuf>),
    /// What a look at a destination's disk found (`space.rs`).
    Space(space::Report),
    /// Record what's missing, pause what runs, stop the session, answer and end.
    Stop(Ending, oneshot::Sender<Vec<(String, u64)>>),
    /// New seeding limits, from the settings.
    Seeding(seeding::Limits),
    /// Undo the top of the undo stack.
    Undo(oneshot::Sender<Result<Undoable, Failure>>),
    /// Runs the closure on the actor, for tests to look inside.
    #[cfg(test)]
    Inspect(Box<dyn FnOnce(&mut Actor) + Send>),
    /// Ends at once, as a crash would: nothing saved, nothing waited for.
    #[cfg(test)]
    Kill(oneshot::Sender<()>),
}

/// What the actor works with, besides what's kept across restarts.
pub(super) struct Setup {
    pub session: Arc<Session>,
    pub data_dir: PathBuf,
    /// librqbit's own folder, from which it names a torrent's own folder.
    pub download_dir: PathBuf,
    pub guard: Guard,
    pub size_check_every: Duration,
    /// How often librqbit is polled, and the destinations' disks looked at.
    pub poll_every: Duration,
    pub trash: Trash,
    /// For add tasks and the watcher to report back.
    pub inbox: mpsc::WeakSender<Message>,
    pub add_into: AddInto,
    /// How a destination's disk is looked at.
    pub look: space::Look,
    /// When a finished torrent stops seeding.
    pub seeding: seeding::Limits,
    /// The undo stack, as kept.
    pub undo: undo::machine::State,
}

pub(super) struct Actor {
    pub(super) session: Arc<Session>,
    data_dir: PathBuf,
    pub(super) download_dir: PathBuf,
    /// Which torrents librqbit may create files for: not those whose destination is unavailable.
    guard: Guard,
    pub(super) store: Store,
    pub(super) torrents: BTreeMap<TorrentId, Torrent>,
    /// The snapshot clients read: the last one built, after the last event.
    pub(super) published: watch::Sender<Published>,
    /// For add tasks and the watcher to report back.
    inbox: mpsc::WeakSender<Message>,
    /// `None` if it couldn't start: then only the regular size check finds missing ones.
    watcher: Option<Watcher>,
    size_check_every: Duration,
    poll_every: Duration,
    /// Where Delete moves files.
    trash: Trash,
    /// Regular size checks of every torrent run so far, for tests to wait on one.
    pub(super) size_checks: u64,
    add_into: AddInto,
    /// The add of each torrent [`inlib::State::Adding`], which Remove stops and waits for.
    pub(super) adds: BTreeMap<TorrentId, JoinHandle<()>>,
    /// Delete's trashing, a blocking task per torrent [`Phase::Deleting`]: how it went, of how
    /// many items it planned.
    trashing: JoinSet<(TorrentId, trash::Carried, usize)>,
    /// Each item a trashing moves, as it goes, so a crash still knows where it went.
    went: mpsc::UnboundedReceiver<(TorrentId, Trashed)>,
    went_tx: mpsc::UnboundedSender<(TorrentId, Trashed)>,
    /// Each torrent being deleted: its step, the client waiting, and what aborts it.
    deletes: BTreeMap<TorrentId, Deleting>,
    /// Moving files to the Trash (`filetrash.rs`), a blocking task per torrent in `file_trashes`.
    file_trashing: JoinSet<filetrash::Ended>,
    /// Each torrent whose files are being trashed: its step, the client waiting, the priorities
    /// they had, and what aborts it.
    file_trashes: BTreeMap<TorrentId, filetrash::Pending>,
    /// Each item a Move to Trash moves, by step, as it goes: written ahead into the step.
    files_went: mpsc::UnboundedReceiver<(u64, Trashed)>,
    files_went_tx: mpsc::UnboundedSender<(u64, Trashed)>,
    /// The disk of each destination, as the last look found it; not those that can't be reached.
    pub(super) disks: BTreeMap<AbsolutePath, space::Known>,
    /// The bytes allocated to each file of a torrent that counts on its disk, as the last look
    /// found them; a file not looked at yet isn't here.
    allocated: BTreeMap<PathBuf, u64>,
    /// The destinations and files to look at, for `space::watch`.
    space: watch::Sender<space::Wanted>,
    /// When a finished torrent stops seeding.
    seeding: seeding::Limits,
    /// Undo's machine: the undo stack and each torrent's lane of steps, and the clients waiting on
    /// them (`steps.rs`, `undoing.rs`).
    pub(super) steps: steps::Steps,
    /// Each undo putting files back, by step: what's back, or the path that failed and why.
    putting_back: JoinSet<(u64, PutBack)>,
    /// The undos cut short by a crash, by step: what of them is no longer in the Trash is back.
    resumed: BTreeSet<u64>,
    /// A test's hook, run as a Move to Trash has polled its torrent, before it sets the files Off.
    #[cfg(test)]
    pub(super) before_off: Option<Box<dyn FnMut(&mut Actor) + Send + Sync>>,
    /// The Deletes a crash cut short, found at start: `run` ends them.
    interrupted_deletes: Vec<TorrentId>,
    /// Adds of a torrent something runs on (an undo bringing it back, its Delete), until it's over.
    adds_waiting: Vec<(Prepared, bool, oneshot::Sender<Result<Added, Failure>>)>,
    /// Where a test kills it: as torrent `.0` is taken off the list, before its record goes (after
    /// what undoes it is kept); `.1` says it did.
    #[cfg(test)]
    pub(super) crash_before_forgetting: Option<(TorrentId, Arc<std::sync::atomic::AtomicBool>)>,
    /// Killed by the test: it handles nothing more.
    #[cfg(test)]
    killed: bool,
}

/// Why the engine stops: the service stops, or the engine starts again with new settings (the
/// listening port, UPnP) while the service carries on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    Shutdown,
    Restart,
}

/// A torrent's librqbit side, with librqbit's own torrent.
pub(super) type InLib = inlib::State<Arc<ManagedTorrent>>;

/// How an undo's put-back went: what's back, or the path that failed and why, with what went back
/// and couldn't return.
type PutBack = Result<Vec<Trashed>, (trash::PutBackError, Vec<PathBuf>)>;

/// A Delete under way: its step, and what stops it after the item in flight.
struct Deleting {
    seq: u64,
    stop: Arc<AtomicBool>,
}

pub(super) struct Torrent {
    pub(super) lifecycle: State,
    /// Its librqbit side.
    pub(super) inlib: InLib,
    info_hash: String,
    /// The trackers it names, for its magnet link.
    trackers: Vec<String>,
    /// What librqbit said of it at the last poll, which clients see until the next.
    polled: Polled,
    /// Its seeding round, counted at each poll.
    pub(super) meter: seeding::Meter,
}

impl Actor {
    /// Takes over the torrents librqbit restored, wanted running or stopped as the store says:
    /// ones it doesn't know get fresh ids; ids of torrents that were fetching info when the service
    /// stopped go. Those in `unavailable` (by info hash, with why their destination can't be reached) are
    /// Destination unavailable; those in `missing` (the files missing at start, with what librqbit
    /// had of each) are Missing files.
    pub(super) fn new(
        setup: Setup,
        mut store: Store,
        mut missing: BTreeMap<String, BTreeMap<usize, u64>>,
        mut unavailable: BTreeMap<String, Unavailable>,
    ) -> Self {
        let Setup { session, data_dir, download_dir, guard, size_check_every, poll_every, trash, inbox, add_into, look, seeding, undo } =
            setup;
        let mut ended = Vec::new();
        let restored: Vec<Arc<ManagedTorrent>> = session.with_torrents(|ts| ts.map(|(_, t)| t.clone()).collect());
        let hashes: HashSet<String> = restored.iter().map(|t| t.info_hash().as_string()).collect();
        store.retain(|hash| hashes.contains(hash));
        // librqbit restores each torrent under the id it had there; a new one mustn't take it.
        let librqbit_ids = session.with_torrents(|ts| ts.map(|(id, _)| id as TorrentId).max());
        store.ids_above(librqbit_ids);
        let mut torrents = BTreeMap::new();
        for handle in restored {
            let hash = handle.info_hash().as_string();
            let id = store.id_or_assign(&hash);
            let saved = store.get_mut(&hash).expect("just assigned");
            saved.destination = destination(&handle);
            guard.set_destination(&hash, saved.destination.clone());
            let gone = gone_at_start(&handle, saved.held.as_ref(), missing.remove(&hash));
            let away = unavailable.remove(&hash);
            // A Delete the service stopped in the middle of (a crash: a stop waits for it): what
            // of the torrent's files is still on disk is what it left; nothing, and it's done.
            // With its drive away there's no telling: all of it.
            let interrupted = !saved.deleting.is_empty();
            if interrupted {
                let left = std::mem::take(&mut saved.deleting)
                    .into_iter()
                    .filter(|path| away.is_some() || std::fs::symlink_metadata(path).is_ok())
                    .map(|path| Kept { path: AbsolutePath::new(path).expect("saved absolute"), why: KeptWhy::Interrupted });
                saved.delete_failed = left.collect();
                // What it moved, as far as it was recorded, can be undone: `run` puts it on the stack.
                if !saved.trashed.is_empty() {
                    ended.push(id);
                }
            }
            let unchecked = saved.unchecked;
            // A Delete that ended with all of it gone but not off the list yet: a crash came between.
            let ended = !interrupted && !saved.trashed.is_empty() && saved.delete_failed.is_empty();
            let phase = match away {
                // `run` takes it off the list.
                _ if (interrupted || ended) && saved.delete_failed.is_empty() => Phase::Deleted,
                // Its files partly in the Trash on purpose, untouched by the restore, wherever its drive is.
                away if !saved.delete_failed.is_empty() => Phase::DeleteFailed { kept: saved.delete_failed.clone(), away, unchecked },
                Some(reason) => Phase::DestinationUnavailable { reason },
                // librqbit restored it checking; it's stopped, until a check is asked for.
                // Whatever files went meanwhile: only a check says.
                None if unchecked => Phase::Unchecked { gone: BTreeMap::new() },
                None if gone.is_empty() => restored_phase(&handle.stats()),
                None => Phase::MissingFiles { gone },
            };
            let torrent = Torrent {
                lifecycle: State::new(saved.wants, phase),
                info_hash: hash,
                trackers: sorted_trackers(&handle),
                polled: Polled::of(&handle.stats(), &Polled::default()),
                inlib: inlib::State::Has(handle),
                meter: seeding::Meter::new(saved.round),
            };
            torrents.insert(id, torrent);
        }
        let (went_tx, went) = mpsc::unbounded_channel();
        let (files_went_tx, files_went) = mpsc::unbounded_channel();
        let watcher_inbox = inbox.clone();
        let watcher = Watcher::start(move |paths| {
            // A full inbox drops the news; the regular size check catches it.
            if let Some(inbox) = watcher_inbox.upgrade() {
                let _ = inbox.try_send(Message::FilesChanged(paths));
            }
        });
        let watcher = watcher.inspect_err(|e| warn!(error = %e, "failed to start the file watcher")).ok();
        let (space, wanted) = watch::channel(space::Wanted::new());
        let space_inbox = inbox.clone();
        space::watch(look, wanted, poll_every, move |report| {
            let Some(inbox) = space_inbox.upgrade() else { return false };
            // A full inbox drops the reading; the next look is soon.
            let _ = inbox.try_send(Message::Space(report));
            true
        });
        // Each start of the engine is a new epoch: a later one's is larger.
        let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let published = watch::Sender::new(Published { epoch, ..Published::default() });
        let actor = Self {
            session,
            data_dir,
            download_dir,
            guard,
            store,
            torrents,
            published,
            inbox,
            watcher,
            size_check_every,
            poll_every,
            trash,
            size_checks: 0,
            add_into,
            adds: BTreeMap::new(),
            trashing: JoinSet::new(),
            went,
            went_tx,
            deletes: BTreeMap::new(),
            file_trashing: JoinSet::new(),
            file_trashes: BTreeMap::new(),
            files_went,
            files_went_tx,
            disks: BTreeMap::new(),
            allocated: BTreeMap::new(),
            space,
            seeding,
            steps: steps::Steps::new(undo),
            putting_back: JoinSet::new(),
            resumed: BTreeSet::new(),
            #[cfg(test)]
            before_off: None,
            interrupted_deletes: ended,
            adds_waiting: Vec::new(),
            #[cfg(test)]
            crash_before_forgetting: None,
            #[cfg(test)]
            killed: false,
        };
        actor.save();
        actor.save_undo();
        actor.publish();
        actor
    }

    pub(super) async fn run(mut self, mut inbox: mpsc::Receiver<Message>) {
        // What waited for a step is gone with its client; what ran is carried out again.
        self.report(steps::Fact::Restarted);
        // The undos a crash cut short, before ending what it cut short makes rollbacks: those start
        // afresh, checked and taken out as any undo is.
        self.resume_undos();
        // A Delete interrupted with nothing left to trash is done; one with something left waits.
        // Either way, what it moved can be undone.
        let deleted: Vec<TorrentId> =
            self.torrents.iter().filter(|(_, t)| t.lifecycle.phase == Phase::Deleted).map(|(id, _)| *id).collect();
        let interrupted = std::mem::take(&mut self.interrupted_deletes);
        for id in deleted.iter().chain(interrupted.iter().filter(|id| !deleted.contains(id))) {
            let seq = self.running_step(*id, proto::UndoAction::Delete);
            let (ended, finished) = self.delete_ended(*id, true, seq).await;
            if let Some(ended) = ended {
                self.steps.unanswered(ended);
            }
            if let Err(e) = finished {
                warn!(id, error = ?e, "failed to finish an interrupted Delete");
            }
        }
        // The other steps the crash cut short end with what they did; a Remove is finished; and
        // each undo under way resumes.
        self.end_cut_short();
        self.finish_removes().await;
        self.run_steps().await;
        // Those wanted running start.
        self.poll_librqbit().await;
        self.publish();
        let mut poll = tokio::time::interval(self.poll_every);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut size_checks = tokio::time::interval(self.size_check_every);
        size_checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                // What a Delete under way moved is recorded before anything else is handled.
                message = inbox.recv() => match self.went_so_far(message) {
                    Some(Message::Stop(ending, reply)) => {
                        let held = self.stop(ending).await;
                        // Its torrents and session go before the answer: see `Engine::stop`.
                        drop(self);
                        let _ = reply.send(held);
                        return;
                    }
                    // As if the process died: nothing more is saved, and a Delete under way is
                    // left where it is.
                    #[cfg(test)]
                    Some(Message::Kill(reply)) => {
                        self.session.stop().await;
                        let _ = reply.send(());
                        return;
                    }
                    // Looks without changing anything, so publishes nothing.
                    #[cfg(test)]
                    Some(Message::Inspect(f)) => {
                        f(&mut self);
                        continue;
                    }
                    Some(message) => self.handle(message).await,
                    // The engine is gone without stopping: as if the service crashed.
                    None => return,
                },
                Some(done) = self.trashing.join_next(), if !self.trashing.is_empty() => {
                    let (id, carried, planned) = done.expect("a trashing's panic is caught");
                    self.trashed(id, carried, planned).await;
                    self.run_steps().await;
                }
                Some(done) = self.file_trashing.join_next(), if !self.file_trashing.is_empty() => {
                    self.files_trashed(done.expect("a trashing's panic is caught")).await;
                    self.run_steps().await;
                }
                Some((id, moved)) = self.went.recv() => self.went(id, moved),
                Some((seq, moved)) = self.files_went.recv() => self.files_went(seq, moved),
                Some(done) = self.putting_back.join_next(), if !self.putting_back.is_empty() => {
                    let (seq, put) = done.expect("a put-back's panic is caught");
                    self.put_back_ended(seq, put).await;
                    self.run_steps().await;
                }
                _ = poll.tick() => self.poll_librqbit().await,
                _ = size_checks.tick() => {
                    self.watch_destinations();
                    self.size_check_all().await;
                    self.keep_rounds();
                    // A file emptied from the Trash since is in it no more, and what's kept of it goes.
                    self.guard.trashed().follow(self.steps.machine());
                }
            }
            #[cfg(test)]
            if self.killed {
                return;
            }
            self.publish();
        }
    }

    /// The snapshots this actor publishes, from the last one.
    pub(super) fn subscribe(&self) -> watch::Receiver<Published> {
        self.published.subscribe()
    }

    /// Publishes the snapshot the state makes now, if it isn't the last one, and tells
    /// `space::watch` what to look at now.
    fn publish(&self) {
        let wanted = self.wanted();
        self.space.send_if_modified(|last| {
            let changed = *last != wanted;
            *last = wanted;
            changed
        });
        let next = self.snapshot();
        self.published.send_if_modified(|last| {
            let changed = last.snapshot != next;
            if changed {
                *last = Published { version: last.version + 1, snapshot: next, ..*last };
            }
            changed
        });
    }

    /// Answers a client, once its answer's effect is published.
    fn answer<T>(&self, reply: oneshot::Sender<T>, answer: T) {
        self.publish();
        let _ = reply.send(answer);
    }

    /// What clients see of the torrents now.
    pub(super) fn snapshot(&self) -> Snapshot {
        let details = self.watched().filter_map(|id| Some((id, self.details(id).ok()?))).collect();
        let torrents = self.listing();
        let needs = self.torrents.keys().filter(|id| self.shown(**id).is_some()).filter_map(|&id| {
            let (destination, files) = self.counted(id)?;
            Some((id, destination, space::needs(files.into_iter().map(|(path, len)| (len, self.allocated.get(&path).copied())))?))
        });
        let volumes = space::volumes(needs, &self.disks);
        Snapshot { torrents, details, volumes, undo: undo::stacked(self.steps.machine()) }
    }

    /// A torrent that counts on its disk ([`machine::counts`]: wanted running, its destination
    /// not unavailable), with info: its destination, and each file it's getting with its length.
    /// What it needs comes from what's allocated to them, never from its progress, which is a
    /// check's while it's checked.
    fn counted(&self, id: TorrentId) -> Option<(&AbsolutePath, Vec<(PathBuf, u64)>)> {
        let torrent = &self.torrents[&id];
        if !machine::counts(&torrent.lifecycle) {
            return None;
        }
        let handle = torrent.inlib.info()?;
        let saved = self.store.get(&torrent.info_hash)?;
        let files = files(handle, &[], saved.held.as_ref(), &BTreeSet::new()).ok()?;
        let getting = files.into_iter().filter(|f| f.priority == Priority::Normal);
        Some((saved.destination.as_ref()?, getting.map(|f| (handle.output_folder().join(&f.path), f.size_bytes)).collect()))
    }

    /// The destinations of the torrents that count on their disks, with their files.
    fn wanted(&self) -> space::Wanted {
        let mut wanted = space::Wanted::new();
        for &id in self.torrents.keys() {
            if let Some((destination, files)) = self.counted(id) {
                wanted.entry(destination.clone()).or_default().extend(files.into_iter().map(|(path, _)| path));
            }
        }
        wanted
    }

    /// What a look at a destination found: its disk, or that it stalled (what's free there is
    /// unknown, where it's mounted still known), and what's allocated to its files.
    fn space(&mut self, report: space::Report) {
        match report.found {
            space::Found::Stalled => {
                if let Some(known) = self.disks.get_mut(&report.destination) {
                    known.free_bytes = None;
                }
            }
            space::Found::Read(reading) => {
                match reading.disk {
                    Some(disk) => {
                        let known = space::Known { mount: disk.mount, name: disk.name, free_bytes: Some(disk.free_bytes) };
                        self.disks.insert(report.destination, known);
                    }
                    None => {
                        self.disks.remove(&report.destination);
                    }
                }
                self.allocated.extend(reading.allocated);
            }
        }
        // Only what's still looked at: a torrent that counts again is unknown until looked at.
        let wanted = self.space.borrow();
        self.allocated.retain(|path, _| wanted.values().any(|files| files.contains(path)));
        self.disks.retain(|destination, _| wanted.contains_key(destination));
    }

    /// The torrents whose details the snapshot carries: every one clients see as it is, as no
    /// client says which it shows yet. Those without info have none, nor those an undo is bringing
    /// back, until they're back.
    fn watched(&self) -> impl Iterator<Item = TorrentId> + '_ {
        self.torrents.keys().copied().filter(|id| self.shown(*id).is_some_and(|s| s.was.is_none()))
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::Add { prepared, paused, reply } => self.add(prepared, paused, reply),
            Message::Command { id, command, reply } => self.arrive(id, command, reply).await,
            Message::AddFinished { id, added } => self.add_finished(id, added).await,
            Message::FilesChanged(paths) => self.files_changed(&paths).await,
            Message::Space(report) => self.space(report),
            // Applied at the next poll.
            Message::Seeding(limits) => self.seeding = limits,
            Message::Undo(reply) => self.undo(reply).await,
            Message::Stop(..) => unreachable!("handled by run"),
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => unreachable!("handled by run"),
        }
    }

    /// Stops everything; returns the pieces each torrent (by info hash) has, for `Engine::stop`.
    async fn stop(&mut self, ending: Ending) -> Vec<(String, u64)> {
        // At a shutdown, what waits on a lane doesn't start: its client hears the service is
        // stopping, and an action leaves the stack as one that never started. The undos, pressed,
        // still run: an Undo never waits. At a restart for new settings the service carries on, so
        // everything waiting runs first, as asked.
        if ending == Ending::Shutdown {
            self.report(steps::Fact::Stopping);
        }
        // What's under way ends first, so how it went is saved, and so do the undos after it.
        loop {
            tokio::select! {
                Some(done) = self.trashing.join_next(), if !self.trashing.is_empty() => {
                    let (id, carried, planned) = done.expect("a trashing's panic is caught");
                    self.trashed(id, carried, planned).await;
                    self.run_steps().await;
                }
                Some(done) = self.file_trashing.join_next(), if !self.file_trashing.is_empty() => {
                    self.files_trashed(done.expect("a trashing's panic is caught")).await;
                    self.run_steps().await;
                }
                Some(done) = self.putting_back.join_next(), if !self.putting_back.is_empty() => {
                    let (seq, put) = done.expect("a put-back's panic is caught");
                    self.put_back_ended(seq, put).await;
                    self.run_steps().await;
                }
                else => {
                    // A safety net: every arm above runs what its end let start already.
                    self.run_steps().await;
                    if self.trashing.is_empty() && self.file_trashing.is_empty() && self.putting_back.is_empty() {
                        break;
                    }
                }
            }
        }
        let mut running = Vec::new();
        for torrent in self.torrents.values_mut() {
            if let Some(saved) = self.store.get_mut(&torrent.info_hash) {
                saved.round = torrent.meter.round;
                saved.missing_at_shutdown = match &torrent.lifecycle.phase {
                    Phase::MissingFiles { gone } | Phase::Unchecked { gone } => {
                        gone.iter().map(|(&i, gone)| (i, gone.progress_bytes)).collect()
                    }
                    _ => BTreeMap::new(),
                };
                saved.files_at_shutdown = torrent.inlib.info().map(|h| expected(h, saved.held.as_ref())).unwrap_or_default();
            }
            if let inlib::State::Has(handle) = &torrent.inlib
                && matches!(handle.stats().state, TorrentStatsState::Live)
            {
                running.push(handle.clone());
            }
        }
        for (_, task) in std::mem::take(&mut self.adds) {
            task.abort();
        }
        self.save();
        for handle in running {
            if let Err(e) = self.session.pause(&handle).await {
                warn!(error = %e, "failed to pause a torrent while stopping");
            }
        }
        let held = self.pieces_kept();
        self.session.stop().await;
        held
    }

    /// The pieces librqbit has of each torrent (by info hash), which its save as it stops holds.
    pub(super) fn pieces_kept(&self) -> Vec<(String, u64)> {
        self.torrents
            .values()
            .filter_map(|t| {
                let handle = t.inlib.info()?;
                let piece_length = handle.with_metadata(|m| m.info.lengths().default_piece_length()).ok()?;
                // librqbit's progress is bytes read while it checks (one halted too): no piece is had.
                let stats = handle.stats();
                let had = if matches!(stats.state, TorrentStatsState::Initializing { .. }) { 0 } else { stats.progress_bytes };
                Some((t.info_hash.clone(), bitv::pieces_in(had, piece_length)))
            })
            .collect()
    }

    /// Decides `event` for torrent `id`, then makes librqbit match; once its phase changes, its files
    /// are size-checked, and that's decided too. A refused command fails with the reason.
    pub(super) async fn event(&mut self, id: TorrentId, mut event: Event) -> Result<(), Failure> {
        loop {
            let torrent = self.torrents.get_mut(&id).ok_or(Failure::torrent(id, Refusal::NotInList))?;
            let before = torrent.lifecycle.phase.clone();
            let next = decide(&torrent.lifecycle, &event).map_err(|refusal| Failure::torrent(id, refusal))?;
            let wants_changed = next.wants != torrent.lifecycle.wants;
            torrent.meter.wants_changed(torrent.lifecycle.wants, next.wants);
            torrent.lifecycle = next;
            // What Delete left is kept across restarts, and librqbit's storage for it touches
            // nothing meanwhile: restoring it would make the trashed files again, empty. What a
            // Delete under way is about to trash is saved by `delete`, and forgotten once it's over.
            // Not what's missing of one whose check was cancelled: it's Missing files at start, as any is.
            // Kept through a Delete, which Undo may take back (#150): it comes back Unchecked too.
            let unchecked = match &torrent.lifecycle.phase {
                Phase::Unchecked { gone } => gone.is_empty(),
                phase @ (Phase::Deleting { .. } | Phase::DeleteFailed { .. }) => phase.unchecked(),
                Phase::Deleted => self.store.get(&torrent.info_hash).is_some_and(|saved| saved.unchecked),
                Phase::FetchingInfo
                | Phase::FetchFailed { .. }
                | Phase::CheckDue { .. }
                | Phase::Checking { .. }
                | Phase::Ready
                | Phase::MissingFiles { .. }
                | Phase::DestinationUnavailable { .. }
                | Phase::Error { .. } => false,
            };
            let (kept, deleting) = match &torrent.lifecycle.phase {
                Phase::DeleteFailed { kept, .. } => (kept.clone(), false),
                Phase::Deleting { .. } => (Vec::new(), true),
                _ => (Vec::new(), false),
            };
            // What Undo needs of a Delete is kept until it's over: a Delete that moved nothing
            // leaves nothing to undo.
            let undoable = matches!(torrent.lifecycle.phase, Phase::Deleting { .. } | Phase::DeleteFailed { .. } | Phase::Deleted);
            let hash = torrent.info_hash.clone();
            if let Some(saved) = self.store.get_mut(&hash)
                && (wants_changed
                    || saved.unchecked != unchecked
                    || saved.delete_failed != kept
                    || (!deleting && !saved.deleting.is_empty())
                    || (!undoable && (saved.stash.is_some() || !saved.trashed.is_empty())))
            {
                saved.wants = self.torrents[&id].lifecycle.wants;
                saved.round = self.torrents[&id].meter.round;
                saved.unchecked = unchecked;
                // Only lifted here: kept untouched by the Delete's trashing, off the actor, as
                // that waits for a disk write (see `delete`).
                if kept.is_empty() && !deleting {
                    self.guard.keep_untouched(&hash, false);
                }
                saved.delete_failed = kept;
                if !deleting {
                    saved.deleting.clear();
                }
                if !undoable {
                    saved.stash = None;
                    saved.trashed.clear();
                }
                self.save();
            }
            self.carry_out(id).await?;
            let moved = self.torrents.get(&id).is_some_and(|t| t.lifecycle.phase != before);
            // Ends: a size check decided on the files as they are leaves the phase as it is.
            match self.size_checked(id) {
                Some(seen) if moved => event = Event::Observed(seen),
                _ => return Ok(()),
            }
        }
    }

    /// Makes librqbit match the torrent: what its lifecycle wants, carried out on its librqbit side.
    async fn carry_out(&mut self, id: TorrentId) -> Result<(), Failure> {
        let Some(torrent) = self.torrents.get(&id) else { return Ok(()) };
        let lib = match &torrent.inlib {
            inlib::State::Halted(_) => Lib::Halted,
            inlib::State::Has(handle) => match handle.stats().state {
                TorrentStatsState::Initializing { .. } => Lib::Checking,
                TorrentStatsState::Live => Lib::Running,
                TorrentStatsState::Paused => Lib::Stopped,
                TorrentStatsState::Error => Lib::Failed,
            },
            inlib::State::TakenOut(_) => Lib::Absent,
            inlib::State::Adding { .. } | inlib::State::NotAdded { .. } => return Ok(()),
        };
        let Some(wanted) = machine::next(&torrent.lifecycle, lib) else { return Ok(()) };
        let todo = inlib::next(&torrent.inlib, wanted);
        // librqbit's upload counter starts again with the torrent.
        if matches!(todo, Some(inlib::Do::Start(_) | inlib::Do::AddAgain { .. }))
            && let Some(torrent) = self.torrents.get_mut(&id)
        {
            torrent.meter.librqbit_started();
        }
        let paused = |actor: &mut Self, event| {
            if let Some(torrent) = actor.torrents.get_mut(&id) {
                torrent.inlib = inlib::decide(&torrent.inlib, &event);
            }
        };
        match todo {
            None => Ok(()),
            Some(inlib::Do::Start(handle)) => {
                self.session.unpause(&handle).await?;
                paused(self, inlib::Event::Started);
                Ok(())
            }
            // A pause ends a check, but librqbit says nothing of it: its side remembers.
            Some(inlib::Do::Stop(handle)) => {
                self.session.pause(&handle).await?;
                paused(self, inlib::Event::Paused { checking: lib == Lib::Checking });
                Ok(())
            }
            Some(inlib::Do::AddAgain { from, take_out }) => self.add_again(id, from, take_out).await,
        }
    }

    /// Counts the torrent's seeding round from librqbit's `stats`, and tells its lifecycle when
    /// that reached a seeding limit while it seeds.
    /// librqbit's figures read now, after the poll's event (which may have started it again).
    async fn meter(&mut self, id: TorrentId, handle: &Arc<ManagedTorrent>) {
        let Some(info) = self.listing_of(id, &BTreeMap::new(), None) else { return };
        let uploaded = handle.stats().uploaded_bytes;
        let torrent = self.torrents.get_mut(&id).expect("listed");
        let size = info.content.map_or(0, |c| c.size_bytes);
        let reached = torrent.meter.poll(uploaded, &info.status, size, &self.seeding, std::time::Instant::now());
        if let Some(reached) = reached
            && let Err(e) = self.event(id, Event::Observed(Observation::LimitReached(reached))).await
        {
            warn!(id, error = ?e, "failed to stop at a seeding limit");
        }
    }

    #[cfg(test)]
    pub(super) fn seeding(&self) -> seeding::Limits {
        self.seeding
    }

    /// Keeps each torrent's seeding round in the store.
    fn keep_rounds(&mut self) {
        let mut changed = false;
        for torrent in self.torrents.values() {
            if let Some(saved) = self.store.get_mut(&torrent.info_hash)
                && saved.round != torrent.meter.round
            {
                saved.round = torrent.meter.round;
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    /// Adds the torrent again from `handle`'s info, stopped, at the same place with the same
    /// priorities, so librqbit opens its files afresh and checks them, keeping what's valid:
    /// librqbit can't check a torrent it has (`take_out`: so it's taken out first), and forgets
    /// the pieces it had along with the torrent. On failure after taking it out, it's an Error,
    /// taken out of librqbit, until Resume or Check adds it again.
    async fn add_again(&mut self, id: TorrentId, handle: Arc<ManagedTorrent>, take_out: bool) -> Result<(), Failure> {
        let bytes = handle.with_metadata(|m| m.torrent_bytes.clone())?;
        // Priorities held during a check are applied by the add.
        let only_files = store::getting(self.held(&handle), handle.only_files().as_deref());
        let output_folder = handle.output_folder().to_str().context("the destination isn't valid UTF-8")?.to_owned();
        // Its folder and priorities known, the files set Off aren't made again (#67).
        let storage = self.guard.for_add(handle.output_folder().to_owned(), only_files.as_deref());
        if take_out {
            self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await?;
        }
        // Under the id it had, which the delete just freed.
        let preferred_id = Some(handle.id());
        let opts = AddTorrentOptions {
            paused: true,
            overwrite: true,
            only_files,
            output_folder: Some(output_folder),
            preferred_id,
            storage_factory: Some(storage.boxed()),
            ..Default::default()
        };
        let added = match self.session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => Ok(t),
            Ok(AddTorrentResponse::ListOnly(_)) => Err(anyhow!("unexpected list-only response")),
            Err(e) => Err(e),
        };
        let torrent = self.torrents.get_mut(&id).expect("being added again");
        match added {
            Ok(added) => {
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::Added(added));
                torrent.lifecycle = decide(&torrent.lifecycle, &Event::Observed(Observation::AddedAgain)).expect("never refused");
            }
            Err(e) => {
                let detail = format!("{e:#}");
                let failed = Event::Observed(Observation::Failed(Broke::AddAgainFailed { detail: detail.clone() }));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::TakenOut);
                return Err(proto::ServiceError::AddAgainFailed { id, detail }.into());
            }
        }
        if let Some(saved) = self.store.get_mut(&handle.info_hash().as_string()) {
            saved.held = None;
        }
        self.save();
        Ok(())
    }

    /// Watches the destinations of the torrents with info, for files going missing.
    pub(super) fn watch_destinations(&mut self) {
        let Some(watcher) = &mut self.watcher else { return };
        let folders =
            self.torrents.values().filter_map(|t| t.inlib.info().and_then(|h| destination(h))).map(AbsolutePath::into_path_buf).collect();
        watcher.watch(folders);
    }

    async fn size_check_all(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.size_check(id).await;
        }
        self.size_checks += 1;
    }

    /// Checks the files of the torrents whose files `changed` can touch.
    async fn files_changed(&mut self, changed: &[PathBuf]) {
        let ids: Vec<TorrentId> = self
            .torrents
            .iter()
            .filter(|(_, t)| {
                let location = t.inlib.info().and_then(|h| location(h).ok());
                location.is_some_and(|l| changed.iter().any(|c| files::touches(c, &l)))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.size_check(id).await;
        }
    }

    /// Size-checks the torrent, for its machine to decide what that means.
    pub(super) async fn size_check(&mut self, id: TorrentId) {
        let Some(seen) = self.size_checked(id) else { return };
        if let Err(e) = self.event(id, Event::Observed(seen)).await {
            warn!(id, error = ?e, "failed to follow the torrent's files");
        }
    }

    /// What a size check of the torrent finds: its destination out of reach, or the files it's
    /// getting as they are on disk. Not its files while librqbit checks them, which it's writing
    /// then (restoring a torrent makes its missing files again); nothing without info.
    fn size_checked(&self, id: TorrentId) -> Option<Observation> {
        let handle = self.torrents.get(&id)?.inlib.info()?;
        if let Some(reason) = destination(handle).and_then(|d| files::unavailable(&d)) {
            return Some(Observation::DestinationGone { reason });
        }
        let halted = matches!(self.torrents.get(&id)?.inlib, inlib::State::Halted(_));
        if matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) && !halted {
            return None;
        }
        Some(Observation::Files(files::disk_of(&expected(handle, self.held(handle)))))
    }

    /// A command that isn't a step, on its torrent's lane.
    async fn command(&mut self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        // Decided on what librqbit is doing now and on its files as they are now, not at the last
        // poll or once the watcher notices.
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(command.action())).await?;
        match command {
            TorrentCommand::Remove | TorrentCommand::Delete | TorrentCommand::TrashFiles { .. } => {
                unreachable!("a step is its lane's (`steps.rs`)")
            }
            TorrentCommand::SetPriority { files, priority } => {
                self.set_priority(id, &files, priority).await?;
                // Files set Off aren't missing any more, and those set Normal may be.
                self.size_check(id).await;
            }
            // Its lifecycle decided them, and `carry_out` stopped librqbit: a check under way ends.
            TorrentCommand::Pause | TorrentCommand::Resume | TorrentCommand::Check | TorrentCommand::CancelCheck => {}
        }
        // So the answer's effect shows in the next listing: e.g. every file Off is Seeding.
        if self.torrents.contains_key(&id) {
            self.poll_torrent(id).await;
        }
        Ok(())
    }

    async fn poll_librqbit(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.poll_torrent(id).await;
        }
    }

    /// Tells the torrent's lifecycle what librqbit is doing (which makes librqbit match it), and
    /// applies held priorities once librqbit can take them.
    pub(super) async fn poll_torrent(&mut self, id: TorrentId) {
        // Taken out, or not added: nothing to follow until librqbit has it.
        let (handle, halted) = match self.torrents.get(&id).map(|t| t.inlib.clone()) {
            Some(inlib::State::Has(handle)) => (handle, false),
            Some(inlib::State::Halted(handle)) => (handle, true),
            _ => return,
        };
        let stats = handle.stats();
        let torrent = self.torrents.get_mut(&id).expect("just found");
        torrent.polled = Polled::of(&stats, &torrent.polled);
        if let Err(e) = self.event(id, Event::Observed(librqbit_seen(&stats, halted))).await {
            warn!(id, error = ?e, "failed to follow librqbit");
        }
        self.meter(id, &handle).await;
        // Held until the next check, which applies them as it adds the torrent again.
        let unchecked = self.torrents.get(&id).is_some_and(|t| t.lifecycle.phase.unchecked());
        if !unchecked && !matches!(stats.state, TorrentStatsState::Initializing { .. }) {
            self.apply_held(id, &handle).await;
        }
    }

    async fn apply_held(&mut self, id: TorrentId, handle: &Arc<ManagedTorrent>) {
        let hash = handle.info_hash().as_string();
        let Some(held) = self.store.get_mut(&hash).and_then(|s| s.held.take()) else { return };
        let normal: HashSet<usize> = held.into_iter().collect();
        if let Err(e) = self.session.update_only_files(handle, &normal).await {
            warn!(id, error = %e, "failed to apply the held file priorities");
        }
        self.save();
    }

    /// Answers at once: with the torrent already in the list with its info hash, or with a new one
    /// fetching info.
    pub(super) fn add(&mut self, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added, Failure>>) {
        let Prepared { add, info_hash, name, trackers, destination } = prepared;
        // One something runs on (an undo bringing it back, its Delete) is added once that's over:
        // it's in the list then, or it's new.
        if self.hash_busy(&info_hash) {
            self.adds_waiting.push((Prepared { add, info_hash, name, trackers, destination }, paused, reply));
            return;
        }
        if let Some(id) = self.store.id(&info_hash) {
            let name = self.torrents.get(&id).and_then(|t| t.name());
            self.answer(reply, Ok(Added { id, name, already_existed: true }));
            return;
        }
        let id = self.store.next_id();
        let wants = if paused { Wants::Stop } else { Wants::Run };
        self.store.insert(info_hash.clone(), id, wants, Some(destination.clone()));
        self.save();
        let added = Added { id, name: name.clone(), already_existed: false };
        self.fetch(id, Prepared { add, info_hash, name, trackers, destination }, wants);
        self.answer(reply, Ok(added));
    }

    /// Starts torrent `id`'s add, in the store already, fetching its info: in the list at once.
    fn fetch(&mut self, id: TorrentId, prepared: Prepared, wants: Wants) {
        let Prepared { add, info_hash, name, trackers, destination } = prepared;
        self.guard.set_destination(&info_hash, Some(destination.clone()));
        // Its own task: fetching info takes as long as it takes, and Remove can stop it.
        let adding =
            (self.add_into)(self.session.clone(), id, add, destination.into_path_buf(), self.download_dir.clone(), self.guard.clone());
        let inbox = self.inbox.clone();
        let task = tokio::spawn(async move {
            let added = adding.await;
            if let Some(inbox) = inbox.upgrade() {
                let _ = inbox.send(Message::AddFinished { id, added }).await;
            }
        });
        self.adds.insert(id, task);
        let torrent = Torrent {
            lifecycle: State::new(wants, Phase::FetchingInfo),
            inlib: inlib::State::Adding { name },
            info_hash,
            trackers,
            polled: Polled::default(),
            meter: seeding::Meter::default(),
        };
        self.torrents.insert(id, torrent);
    }

    async fn add_finished(&mut self, id: TorrentId, added: Result<AddTorrentResponse>) {
        let added = match added {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => Ok(t),
            Ok(AddTorrentResponse::ListOnly(_)) => Err(anyhow!("unexpected list-only response")),
            Err(e) => Err(e),
        };
        self.adds.remove(&id);
        let Some(torrent) = self.torrents.get_mut(&id) else {
            // Removed while the add finished: take it back out, unless it's been added again.
            if let Ok(handle) = added
                && self.store.id(&handle.info_hash().as_string()).is_none()
                && let Err(e) = self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await
            {
                warn!(error = %e, "failed to take a removed torrent back out");
            }
            return;
        };
        let handle = match added {
            Ok(handle) => handle,
            Err(e) => {
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::FetchFailed);
                let _ = self.event(id, Event::Observed(Observation::FetchFailed(format!("{e:#}")))).await;
                return;
            }
        };
        let checking = matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
        torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::Added(handle));
        let _ = self.event(id, Event::Observed(Observation::InfoArrived { checking })).await;
        self.poll_torrent(id).await;
    }

    /// Delete, step `seq`: the torrent's files go to the Trash first, every one that can, and only
    /// once all went is it taken out of librqbit. It's [`Phase::Deleting`] meanwhile, stopped, and
    /// not listed; the trashing runs on a blocking task, which comes back to [`Actor::trashed`]; the
    /// client's answer waits for it, nothing else does. One without info has no files: it's removed.
    async fn delete(&mut self, id: TorrentId, seq: u64) {
        // As any command: decided on librqbit and the disk as they are now.
        self.poll_torrent(id).await;
        self.size_check(id).await;
        let goes = match self.torrents.get(&id).and_then(|t| t.inlib.info()).map(|h| to_trash(h)).transpose() {
            Ok(goes) => goes,
            Err(e) => return self.step_ended(seq, steps::Over::Failed, Err(e.into())),
        };
        if let Err(e) = self.event(id, Event::Command(proto::Action::Delete)).await {
            return self.step_ended(seq, steps::Over::Failed, Err(e));
        }
        let (Some(goes), Some(torrent)) = (goes, self.torrents.get(&id).filter(|t| matches!(t.lifecycle.phase, Phase::Deleting { .. })))
        else {
            let (ended, removed) = self.remove(id, proto::UndoAction::Delete, seq).await;
            return self.answer_step(ended, steps::Outcome::Done(removed.map_err(Failure::from)));
        };
        // Written ahead: a crash from here on comes back as what's left of it, never as a torrent
        // whose files librqbit makes again.
        let hash = torrent.info_hash.clone();
        // Kept aside for Undo while it's whole: once, before its first Delete.
        let stash = match self.store.get(&hash).map(|s| s.stash.is_none()) {
            Some(true) => self.stash_of(id),
            _ => None,
        };
        if let Some(saved) = self.store.get_mut(&hash) {
            saved.deleting = goes.files.clone();
            saved.stash = saved.stash.take().or(stash);
            self.save();
        }
        let trash = self.trash.clone();
        let went = self.went_tx.clone();
        let guard = self.guard.clone();
        let stop = Arc::new(AtomicBool::new(false));
        self.deletes.insert(id, Deleting { seq, stop: stop.clone() });
        // What goes is decided there too: it reads the folder, which may be slow to.
        self.trashing.spawn_blocking(move || {
            // First, nothing of librqbit's touches the files: this waits for a write under way,
            // which a stalled drive can make last, so here and not on the actor. Meanwhile the
            // torrent is Deleting, which no new storage of it is made in.
            guard.keep_untouched(&hash, true);
            let carried = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plan = trash::plan(&goes.location, &goes.files, goes.single);
                let carried = trash::carry_out_until(&plan, &trash, |moved| drop(went.send((id, moved.clone()))), &stop);
                (carried, plan.items.len())
            }));
            let failed = |path: &PathBuf| (path.clone(), KeptWhy::Failed { detail: "trashing stopped".into() });
            let (carried, planned) = carried.unwrap_or_else(|_| {
                (trash::Carried { moved: Vec::new(), kept: goes.files.iter().map(failed).collect() }, goes.files.len())
            });
            (id, carried, planned)
        });
    }

    /// How Delete's trashing of torrent `id` went, `kept` being what couldn't go of `planned`:
    /// decided by its lifecycle, which says it's deleted (it's taken off the list), back as it
    /// was, or waiting with what's left. Its step is done with what moved, or gone if nothing did.
    /// The client hears which; one an Undo aborted hears it was undone, and what moved is put back.
    async fn trashed(&mut self, id: TorrentId, carried: trash::Carried, planned: usize) {
        let Some(Deleting { seq, .. }) = self.deletes.remove(&id) else { return };
        let trash::Carried { moved: went, kept } = carried;
        for item in went {
            self.went(id, item);
        }
        let moved = kept.len() < planned;
        let kept: Vec<Kept> = kept
            .into_iter()
            .map(|(path, why)| Kept { path: AbsolutePath::new(path).expect("librqbit's paths are absolute"), why })
            .collect();
        let (ended, answer) = match self.event(id, Event::Observed(Observation::Trashed { kept: kept.clone(), moved })).await {
            Err(e) => (self.end(seq, steps::Over::Failed), Err(e)),
            // Its step ends there, written ahead of its record going.
            Ok(()) => {
                let (ended, taken_off) = self.delete_ended(id, moved, Some(seq)).await;
                let ended = ended.expect("a Delete with a step ends it");
                let answer = match (taken_off, self.torrents.get(&id)) {
                    (Err(e), _) => Err(e),
                    (Ok(()), None) => Ok(()),
                    (Ok(()), Some(t)) => {
                        let name = t.name().unwrap_or_else(|| format!("torrent {id}"));
                        Err(Failure::torrent(id, Refusal::NotTrashed { name, kept }))
                    }
                };
                (ended, answer)
            }
        };
        self.answer_step(ended, steps::Outcome::Done(answer));
    }

    /// Takes the torrent off the list and out of librqbit, keeping its files. One fetching info
    /// stops first: an add cut short may have left it in librqbit too.
    async fn take_off_list(&mut self, id: TorrentId) -> Result<()> {
        // As if the process died here: nothing more is saved.
        #[cfg(test)]
        if let Some((_, crashed)) = self.crash_before_forgetting.take_if(|(at, _)| *at == id) {
            self.session.stop().await;
            self.killed = true;
            crashed.store(true, std::sync::atomic::Ordering::SeqCst);
            bail!("killed");
        }
        let torrent = self.torrents.remove(&id).expect("decided on it");
        let hash = super::id20(&torrent.info_hash)?;
        self.store.forget(&torrent.info_hash);
        self.save();
        if let Some(task) = self.adds.remove(&id) {
            task.abort();
            let _ = task.await;
        }
        let taken_out = match self.session.get(TorrentIdOrHash::Hash(hash)) {
            Some(t) => self.session.delete(TorrentIdOrHash::Id(t.id()), false).await,
            None => Ok(()),
        };
        // Only now: until then, librqbit could still be adding it.
        self.guard.set_destination(&torrent.info_hash, None);
        // Touched again, the storage librqbit had would be too, but librqbit's delete took its
        // files out of it and dropped them: a check still ending finds no file to set the length
        // of. A librqbit that kept them would bring back #186.
        self.guard.keep_untouched(&torrent.info_hash, false);
        taken_out
    }

    /// librqbit's torrent, for one with info. Its lifecycle refuses the commands that need one
    /// before: one without is an internal error.
    pub(super) fn handle_of(&self, id: TorrentId) -> Result<&Arc<ManagedTorrent>> {
        self.torrents.get(&id).and_then(|t| t.inlib.info()).with_context(|| format!("torrent {id} has no info"))
    }

    pub(super) fn held(&self, handle: &ManagedTorrent) -> Option<&BTreeSet<usize>> {
        self.store.get(&handle.info_hash().as_string())?.held.as_ref()
    }

    /// Torrent `id`'s details, its files' priorities as clients see them.
    fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        let handle = self.handle_of(id)?;
        let torrent = &self.torrents[&id];
        // Nothing is known of what an unchecked torrent has.
        let progress = if torrent.lifecycle.phase.unchecked() { &[] } else { torrent.polled.file_progress.as_slice() };
        let mut files = files(handle, progress, self.held(handle), &missing(&torrent.lifecycle.phase))?;
        let priorities = self.shown(id).map(|s| s.priorities).unwrap_or_default();
        shown_as(&mut files, &priorities);
        let location = AbsolutePath::new(location(handle)?).map_err(|p| anyhow!("torrent {id} is at a relative path: {}", p.display()))?;
        Ok(Details { id, files, location })
    }

    /// Files set Off aren't downloaded and drop out of the torrent's Size, down to none at all:
    /// it's then finished. librqbit saves the choice with the torrent. While librqbit is
    /// checking the torrent the change is held in the store, shows at once, and is applied
    /// when the check is done.
    pub(super) async fn set_priority(&mut self, id: TorrentId, indices: &[usize], priority: Priority) -> Result<(), Failure> {
        let handle = self.handle_of(id)?.clone();
        let held = self.held(&handle).cloned();
        let files = files(&handle, &[], held.as_ref(), &BTreeSet::new())?;
        if let Some(bad) = indices.iter().find(|&&i| !files.iter().any(|f| f.index == i)) {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        let normal: BTreeSet<usize> = files
            .iter()
            .map(|f| (f.index, if indices.contains(&f.index) { priority } else { f.priority }))
            .filter(|(_, p)| *p == Priority::Normal)
            .map(|(i, _)| i)
            .collect();
        let checking = matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
        let unchecked = self.torrents.get(&id).is_some_and(|t| t.lifecycle.phase.unchecked());
        if !checking && !unchecked && held.is_none() {
            return Ok(self.session.update_only_files(&handle, &normal.into_iter().collect()).await?);
        }
        self.store.get_mut(&handle.info_hash().as_string()).context("torrent without an id")?.held = Some(normal);
        self.save();
        Ok(())
    }

    /// Torrent `id` as it is, its files' priorities as `priorities` says where it has them, its
    /// status and actions as `lifecycle` does (its own if none).
    fn listing_of(&self, id: TorrentId, priorities: &BTreeMap<usize, Priority>, lifecycle: Option<&State>) -> Option<TorrentInfo> {
        let t = self.torrents.get(&id)?;
        let lifecycle = lifecycle.unwrap_or(&t.lifecycle);
        match t.inlib.info() {
            Some(handle) => torrent_info(id, t, handle, self.held(handle), priorities, lifecycle),
            None => fetching_info(id, t, lifecycle),
        }
    }

    /// The store is a convenience to keep across restarts, so failing to save it is logged
    /// rather than failing the request.
    fn save(&self) {
        if let Err(e) = self.store.save(&self.data_dir) {
            warn!(error = %e, "failed to save the engine's state");
        }
    }
}

impl Torrent {
    fn name(&self) -> Option<String> {
        match &self.inlib {
            inlib::State::Adding { name } | inlib::State::NotAdded { name } => name.clone(),
            inlib::State::Has(h) | inlib::State::Halted(h) | inlib::State::TakenOut(h) => h.name(),
        }
    }
}

/// Adds the torrent to librqbit with its files in `destination`: a torrent of several files in its
/// own folder there, named as librqbit names it. librqbit only names that folder inside its own
/// (`download_dir`), so it first fetches the info without adding, then adds it with the peers it
/// found meanwhile. `overwrite` lets a re-added torrent resume into files that already exist.
/// Stopped: the actor starts it if it's wanted running. Under Phantom's id `id`: librqbit numbers
/// a torrent from the ones it has saved, so two adds at once would otherwise get the same number,
/// and the second the first's torrent.
pub(super) async fn add_into(
    session: Arc<Session>,
    id: TorrentId,
    add: AddTorrent<'static>,
    destination: PathBuf,
    download_dir: PathBuf,
    guard: Guard,
) -> Result<AddTorrentResponse> {
    let listed = session.add_torrent(add, Some(AddTorrentOptions { list_only: true, paused: true, ..Default::default() })).await?;
    let AddTorrentResponse::ListOnly(listed) = listed else { bail!("unexpected response to a list-only add") };
    let own_folder = listed.output_folder.strip_prefix(&download_dir).context("librqbit put the torrent elsewhere")?;
    let output_folder = destination.join(own_folder);
    let output_folder = output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned();
    let opts = AddTorrentOptions {
        paused: true,
        overwrite: true,
        initial_peers: Some(listed.seen_peers),
        preferred_id: Some(id.try_into()?),
        // Phantom's own storage, which knows its folder, as an add again's: it never writes to a file
        // moved from its path.
        storage_factory: Some(guard.for_add(PathBuf::from(&output_folder), None).boxed()),
        output_folder: Some(output_folder),
        ..Default::default()
    };
    session.add_torrent(AddTorrent::from_bytes(listed.torrent_bytes), Some(opts)).await
}

/// How librqbit's error for a torrent breaks it. librqbit hands over only the text, so a full disk
/// (ENOSPC) is told by its message: the OS's "No space left on device", or "(os error 28)" where
/// the text is wrapped.
fn broke(detail: &str) -> Broke {
    let lower = detail.to_lowercase();
    let enospc = lower.contains("no space left on device") || lower.contains("os error 28)");
    if enospc { Broke::DiskFull } else { Broke::Librqbit { detail: detail.to_owned() } }
}

/// What librqbit is doing with a torrent it has.
/// `halted`: its check was ended by a pause, which librqbit still calls Initializing.
fn librqbit_seen(stats: &TorrentStats, halted: bool) -> Observation {
    match stats.state {
        TorrentStatsState::Initializing { .. } if halted => Observation::Halted,
        TorrentStatsState::Initializing { .. } => Observation::Checking,
        TorrentStatsState::Live | TorrentStatsState::Paused => Observation::Idle { have: stats.file_progress.clone() },
        TorrentStatsState::Error => Observation::Failed(broke(stats.error.as_deref().unwrap_or("unknown error"))),
    }
}

/// The phase of a torrent librqbit restored, before anything happened to it.
fn restored_phase(stats: &TorrentStats) -> Phase {
    match &stats.state {
        TorrentStatsState::Initializing { .. } => Phase::Checking { returning: BTreeMap::new() },
        TorrentStatsState::Live | TorrentStatsState::Paused => Phase::Ready,
        TorrentStatsState::Error => Phase::Error { failure: broke(stats.error.as_deref().unwrap_or_default()) },
    }
}

/// The files missing, by index: those of a torrent that's Missing files.
fn missing(phase: &Phase) -> BTreeSet<usize> {
    match phase {
        Phase::MissingFiles { gone } | Phase::Unchecked { gone } => gone.keys().copied().collect(),
        // As it was, until the trash says how it went.
        Phase::Deleting { was } => missing(was),
        _ => BTreeSet::new(),
    }
}

/// What [`trash::plan`] decides from, for the torrent: from its info alone, without the disk.
pub(super) struct ToTrash {
    pub(super) location: PathBuf,
    pub(super) files: Vec<PathBuf>,
    pub(super) single: bool,
}

pub(super) fn to_trash(handle: &ManagedTorrent) -> Result<ToTrash> {
    let location = location(handle)?;
    let (files, single) = handle.with_metadata(|m| {
        let files: Vec<PathBuf> = m.file_infos.iter().map(|f| handle.output_folder().join(&f.relative_filename)).collect();
        (files, m.file_infos.len() == 1)
    })?;
    Ok(ToTrash { location, files, single })
}

/// Where the torrent is on disk: the file itself for a torrent of one file, which librqbit puts
/// straight in the destination, and the torrent's own folder otherwise.
fn location(handle: &ManagedTorrent) -> Result<PathBuf> {
    handle.with_metadata(|m| match &m.file_infos[..] {
        [only] => handle.output_folder().join(&only.relative_filename),
        _ => handle.output_folder().to_owned(),
    })
}

/// The files the torrent is getting (those Normal), as they must be on disk.
fn expected(handle: &ManagedTorrent, held: Option<&BTreeSet<usize>>) -> Vec<Expected> {
    let files = files(handle, &handle.stats().file_progress, held, &BTreeSet::new()).unwrap_or_default();
    files
        .into_iter()
        .filter(|f| f.priority == Priority::Normal)
        .map(|f| Expected {
            index: f.index,
            path: handle.output_folder().join(&f.path),
            size_bytes: f.size_bytes,
            progress_bytes: f.progress_bytes,
        })
        .collect()
}

/// The files missing at start, from what librqbit had of each (by index), not judged yet:
/// librqbit restoring the torrent makes them again.
fn gone_at_start(handle: &ManagedTorrent, held: Option<&BTreeSet<usize>>, missing: Option<BTreeMap<usize, u64>>) -> BTreeMap<usize, Gone> {
    let Some(missing) = missing else { return BTreeMap::new() };
    let files = files(handle, &[], held, &BTreeSet::new()).unwrap_or_default();
    files
        .into_iter()
        .filter_map(|f| {
            let progress_bytes = *missing.get(&f.index)?;
            let path = handle.output_folder().join(&f.path);
            Some((f.index, Gone { path, size_bytes: f.size_bytes, progress_bytes, seen: Seen::NotYet }))
        })
        .collect()
}

/// The folder the torrent's files go into: librqbit's output folder, less the torrent's own
/// folder if it has several files.
fn destination(handle: &ManagedTorrent) -> Option<AbsolutePath> {
    let single = handle.with_metadata(|m| m.file_infos.len() == 1).ok()?;
    // librqbit joins an empty folder name for a single file, which leaves a trailing slash.
    let folder: PathBuf = handle.output_folder().components().collect();
    let folder = if single { folder } else { folder.parent()?.to_owned() };
    AbsolutePath::new(folder).ok()
}

/// The trackers librqbit has for a restored torrent, in a fixed order.
fn sorted_trackers(handle: &ManagedTorrent) -> Vec<String> {
    let mut trackers: Vec<String> = handle.shared().trackers.iter().map(|t| t.to_string()).collect();
    trackers.sort();
    trackers
}

/// Its files with the priorities clients see: `priorities` where it says.
fn shown_as(files: &mut [File], priorities: &BTreeMap<usize, Priority>) {
    for f in files {
        if let Some(p) = priorities.get(&f.index) {
            f.priority = *p;
        }
    }
}

fn fetching_info(id: TorrentId, t: &Torrent, lifecycle: &State) -> Option<TorrentInfo> {
    Some(TorrentInfo {
        id,
        info_hash: t.info_hash.clone(),
        name: t.name(),
        status: lifecycle.status(false)?,
        content: None,
        uploaded_bytes: 0,
        download_bytes_per_sec: 0,
        upload_bytes_per_sec: 0,
        live_peers: 0,
        trackers: t.trackers.clone(),
        location: None,
        actions: lifecycle.actions(),
    })
}

/// The torrent's files, without BEP 47 padding (which only aligns the others and is never written).
/// `progress` is librqbit's bytes per file, empty while checking.
/// `held` is the priorities held until a check ends, if any, which take the place of librqbit's.
/// `missing` files are gone from disk, so none of them is downloaded, whatever librqbit thinks.
pub(super) fn files(
    torrent: &ManagedTorrent,
    progress: &[u64],
    held: Option<&BTreeSet<usize>>,
    missing: &BTreeSet<usize>,
) -> Result<Vec<File>> {
    // librqbit gets every file until it's told otherwise.
    let normal: Option<BTreeSet<usize>> = held.cloned().or_else(|| torrent.only_files().map(|files| files.into_iter().collect()));
    torrent.with_metadata(|m| {
        m.file_infos
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.attrs.padding)
            .map(|(index, f)| File {
                index,
                path: f.relative_filename.clone(),
                size_bytes: f.len,
                progress_bytes: if missing.contains(&index) { 0 } else { progress.get(index).copied().unwrap_or(0) },
                priority: match &normal {
                    Some(normal) if !normal.contains(&index) => Priority::Off,
                    _ => Priority::Normal,
                },
                missing: missing.contains(&index),
            })
            .collect()
    })
}

/// Torrent `t` as listed, its files' priorities as `priorities` says where it has them; none while
/// it's leaving the list.
fn torrent_info(
    id: TorrentId,
    t: &Torrent,
    torrent: &ManagedTorrent,
    held: Option<&BTreeSet<usize>>,
    priorities: &BTreeMap<usize, Priority>,
    lifecycle: &State,
) -> Option<TorrentInfo> {
    let stats = &t.polled;
    // Size and progress from the files you're getting: librqbit counts whole pieces, which
    // spill into neighbouring files that may be Off.
    let mut files = files(torrent, &stats.file_progress, held, &missing(&t.lifecycle.phase)).unwrap_or_default();
    shown_as(&mut files, priorities);
    let normal = || files.iter().filter(|f| f.priority == Priority::Normal);
    let size_bytes = normal().map(|f| f.size_bytes).sum();
    let status = lifecycle.status(stats.finished)?;
    // Shown as commands waiting for it will leave it (#190): its rates and progress are of that too.
    let checking = t.lifecycle.status(stats.finished) == Some(Status::Checking);
    let progress_bytes = match status {
        // How much has been checked so far; none yet of a check that waits to start.
        Status::Checking if checking => stats.checked_bytes.min(size_bytes),
        Status::Checking => 0,
        // Its check was cancelled: nothing is known until it's checked, what was returning
        // still gone or not.
        _ if lifecycle.phase.unchecked() => 0,
        // Any other status: what librqbit last said it has, file by file. A check under way (in
        // one taken out of librqbit mid-check, say) isn't progress (#141).
        _ => normal().map(|f| f.progress_bytes).sum(),
    };
    let same = t.lifecycle.status(stats.finished).as_ref() == Some(&status);
    Some(TorrentInfo {
        id,
        info_hash: torrent.info_hash().as_string(),
        name: torrent.name(),
        status,
        content: Some(Content { size_bytes, full_size_bytes: files.iter().map(|f| f.size_bytes).sum(), progress_bytes }),
        uploaded_bytes: stats.uploaded_bytes,
        // Never "Paused · 2 MB/s": a status that isn't librqbit's yet has no rates.
        download_bytes_per_sec: if same { stats.download_bytes_per_sec } else { 0 },
        upload_bytes_per_sec: if same { stats.upload_bytes_per_sec } else { 0 },
        live_peers: if same { stats.live_peers } else { 0 },
        trackers: t.trackers.clone(),
        location: location(torrent).ok().and_then(|path| AbsolutePath::new(path).ok()),
        actions: lifecycle.actions(),
    })
}

/// What clients see of librqbit's stats of a torrent.
#[derive(Default)]
struct Polled {
    finished: bool,
    /// While librqbit checks it, how much it has checked; otherwise what it has.
    checked_bytes: u64,
    /// What librqbit last said it has of each file, by index: kept through a check, during which
    /// it says nothing per file.
    file_progress: Vec<u64>,
    uploaded_bytes: u64,
    download_bytes_per_sec: u64,
    upload_bytes_per_sec: u64,
    live_peers: u32,
}

impl Polled {
    /// The stats, keeping what `last` said of each file while librqbit says nothing of them.
    fn of(stats: &TorrentStats, last: &Polled) -> Self {
        let live = stats.live.as_ref();
        let file_progress = if stats.file_progress.is_empty() { &last.file_progress } else { &stats.file_progress };
        Self {
            finished: stats.finished,
            checked_bytes: stats.progress_bytes,
            file_progress: file_progress.clone(),
            uploaded_bytes: stats.uploaded_bytes,
            download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
            upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
            live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
        }
    }
}

#[cfg(test)]
impl Actor {
    /// Whether torrent `id`'s lifecycle has a check under way, whatever clients are shown.
    pub(super) fn checking(&self, id: TorrentId) -> bool {
        self.torrents.get(&id).is_some_and(|t| t.lifecycle.status(false) == Some(Status::Checking))
    }

    /// Torrent `id` as if librqbit had said, at its last poll, it has `bytes` of file `file`.
    pub(super) fn fake_progress(&mut self, id: TorrentId, file: usize, bytes: u64) {
        if let Some(p) = self.torrents.get_mut(&id).and_then(|t| t.polled.file_progress.get_mut(file)) {
            *p = bytes;
        }
    }

    /// Torrent `id` as if librqbit had a peer sending and taking `rate` bytes a second, till the next
    /// poll: the rates and peers it's listed with then.
    pub(super) fn fake_rates(&mut self, id: TorrentId, rate: u64) -> Option<(u64, u64, u32)> {
        let t = self.torrents.get_mut(&id)?;
        (t.polled.download_bytes_per_sec, t.polled.upload_bytes_per_sec, t.polled.live_peers) = (rate, rate, 1);
        self.listed(id).map(|t| (t.download_bytes_per_sec, t.upload_bytes_per_sec, t.live_peers))
    }
}

#[cfg(test)]
mod classify_tests {
    use proptest::prelude::*;

    use super::*;

    const ENOSPC: &str = "No space left on device (os error 28)";

    proptest! {
        /// ENOSPC in any shape librqbit words it (bare, after a path or a context, in a chain,
        /// the OS's text alone or its code alone) is a full disk.
        #[test]
        fn enospc_in_any_shape_is_disk_full(
            before in "[a-zA-Z0-9 /._-]{0,30}",
            after in "[a-zA-Z0-9 /._-]{0,30}",
            shape in prop::sample::select(vec![ENOSPC, "no space left on device", "NO SPACE LEFT ON DEVICE", "(os error 28)"]),
            glue in prop::sample::select(vec![": ", " - ", "\ncaused by: "]),
        ) {
            prop_assert_eq!(broke(&format!("{before}{glue}{shape}{glue}{after}")), Broke::DiskFull);
            prop_assert_eq!(broke(shape), Broke::DiskFull);
        }

        /// Any other error, including other OS errors, stays librqbit's text.
        #[test]
        fn any_other_error_is_librqbit_s(text in "[a-zA-Z0-9 /._:()-]{0,60}", code in (0u32..400).prop_filter("not ENOSPC", |c| *c != 28)) {
            for detail in [text.clone(), format!("{text}: Permission denied (os error {code})")] {
                let lower = detail.to_lowercase();
                prop_assume!(!lower.contains("no space left") && !lower.contains("os error 28)"));
                prop_assert_eq!(broke(&detail), Broke::Librqbit { detail });
            }
        }
    }
}
