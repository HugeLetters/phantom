//! The actor: one task that owns every torrent (its record, its lifecycle ([`State`]) and its
//! librqbit side ([`Lib`])) and librqbit's session, and handles one [`Message`] at a time: client commands
//! (each answered through its own reply channel), the end of an add, a regular poll of librqbit,
//! and a size check of the files on disk (every few seconds, at once when the watcher sees a path
//! go: `files.rs`, and before a command, so its answer doesn't depend on how soon the watcher
//! noticed). Each is an [`Event`] for [`decide`]; after it, [`lib::reconcile`] says what makes
//! librqbit match it and its record, one step at a time. The actor reports
//! facts (what librqbit is doing, what's on disk, how an add ended) and the machines (`machine/`)
//! decide what they mean. Slow work (fetching info, downloading a .torrent URL, moving files to
//! the Trash) runs in its own task and comes back as a message: the actor never waits on it.
//! After each, the actor publishes the [`Snapshot`] its state makes, if it changed, and only then
//! answers: an answer's effect is in what clients read next.
//!
//! Phantom's record of a torrent is the torrent (#45, `store.rs`): its .torrent, destination,
//! priorities and what's wanted. librqbit only runs it: the actor adds every torrent itself, at a
//! start too, from its record, and librqbit's session only keeps the pieces each has. librqbit
//! only runs a torrent when the actor starts it: every add is stopped, and a torrent wanted running
//! is started once it's Ready.
//!
//! Missing files: a size check reports the files a torrent is getting and what's at each path; the
//! machine decides which are missing, set Off or back. One missing is taken out of librqbit (so
//! nothing more is written into the deleted file). Resume and Check check it (add it again, which
//! opens the files afresh). A size check follows every change of a
//! torrent's phase, and a priority change, so what's on disk is judged in the phase it's now in:
//! e.g. the file a check just found isn't the one that went is recorded as it is.
//!
//! Destination unavailable: before its files, a torrent's destination is size-checked (every few
//! seconds, when the watcher sees a path go and before a command). One that can't be reached is
//! taken out of librqbit until it's back; then it's checked. librqbit creates a torrent's files
//! whenever it adds one: [`Guard`] gets each add's destination, and gives one that can't be reached
//! then storage that touches nothing, so no add makes folders where a drive isn't.
//!
//! What's wanted (run or stop) is saved in the record whenever it changes, so a start, after a stop
//! or a crash, makes librqbit match it. At stop, every torrent's missing files and what librqbit has
//! of each file are recorded too, and running ones are paused. At start, torrents whose
//! destination can't be reached are Destination unavailable; the others' files are size-checked
//! from their records before any is added to librqbit (which would make missing files again,
//! empty), and torrents missing some are Missing files, out of librqbit.
//!
//! Undo (`undo.rs` here, and the stack's machine in `../undo/`): a Remove, a Delete and a Move to
//! Trash are steps on the undo stack from the moment their command comes (`steps.rs`), each torrent's
//! work on a lane of its own, in order; an Undo takes the top step at once, and puts back what a
//! Delete moved, on a blocking task like the trashing, then adds the torrent again from what was
//! kept of it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, TorrentStats, TorrentStatsState};
use proto::{
    AbsolutePath, Added, Content, Details, File, Kept, KeptWhy, Priority, Status, TorrentCommand, TorrentId, TorrentInfo, Unavailable,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::warn;

use super::bitv;
use super::files::{self, Expected, Guard, Watcher};
use super::held::Held;
use super::machine::add;
use super::machine::lib::{self, Librqbit};
use super::machine::{self, Broke, Event, Observation, Phase, Refusal, State, Wants, decide, seeding};
use super::slots::{Granting, Refused};
use super::snapshot::{Published, Snapshot};
use super::space;
use super::store::{Gets, Info, Record, Store};
use super::trash::{self, Trash, Trashed};
use super::undo;
use super::{Failure, Prepared, default_folder};

mod filetrash;
pub(super) mod letgo;
pub(super) mod moving;
mod steps;
pub use steps::Carried;
mod undoing;

/// How an add reaches librqbit: [`add_into`], or in tests one whose waits the test controls.
pub(super) type AddInto = Arc<dyn Fn(Arc<Session>, TorrentId, AddTorrent<'static>, AbsolutePath, PathBuf, Guard) -> Adding + Send + Sync>;
pub(super) type Adding = Pin<Box<dyn Future<Output = Result<AddTorrentResponse>> + Send>>;

pub(super) enum Message {
    Add {
        prepared: Prepared,
        paused: bool,
        reply: oneshot::Sender<Result<Added, Failure>>,
    },
    /// `command` for each of `ids`: answered with those that failed.
    Command {
        ids: Vec<TorrentId>,
        command: TorrentCommand,
        reply: oneshot::Sender<Vec<(TorrentId, Failure)>>,
    },
    /// `moves` into `to`, one step each, in order: answered with those that failed.
    Moves {
        to: AbsolutePath,
        moves: Vec<proto::MoveOf>,
        reply: oneshot::Sender<Vec<(TorrentId, Failure)>>,
    },
    /// Which of `ids` a Move into `to` would find files at (`proto::Request::Clashes`).
    Clashes {
        ids: Vec<TorrentId>,
        to: AbsolutePath,
        reply: oneshot::Sender<Vec<proto::Clashing>>,
    },
    /// A free name for torrent `id` in `to` (`proto::Request::FreeName`).
    FreeName {
        id: TorrentId,
        to: AbsolutePath,
        reply: oneshot::Sender<Result<proto::FileName, Failure>>,
    },
    /// librqbit's add of torrent `id` ended.
    AddFinished {
        id: TorrentId,
        /// The task that ended: an id's torrent can be fetched by a later task than this (a Remove,
        /// then its undo), whose end this isn't.
        task: tokio::task::Id,
        added: Result<AddTorrentResponse>,
    },
    /// The watcher saw these paths removed or renamed.
    FilesChanged(Vec<PathBuf>),
    /// librqbit is done reading the pieces it saved of torrent `id`, resuming: what was held for it
    /// goes on at once, not at the next poll.
    Sampled(TorrentId),
    /// What a look at a destination's disk found (`space.rs`).
    Space(space::Report),
    /// The default destination is this folder now, or there's none (the settings); answered once
    /// what was found of the last one is no longer said.
    DefaultFolder(Option<AbsolutePath>, oneshot::Sender<()>),
    /// What a look at the default folder found (`default_folder.rs`).
    DefaultFolderLook(default_folder::Report),
    /// Record what's missing, pause what runs, stop the session, answer and end: with the pieces
    /// each torrent has, and (a restart for new settings) the Moves it didn't start.
    Stop(Ending, oneshot::Sender<(Vec<(String, u64)>, steps::Carried)>),
    /// The Moves the engine before this one didn't start as it restarted for new settings: they
    /// arrive, in their order, each with its client. Answered once they're in.
    Resume(steps::Carried, oneshot::Sender<()>),
    /// New seeding limits, from the settings.
    Seeding(seeding::Limits),
    /// Undo the top of the undo stack.
    Undo(oneshot::Sender<Result<proto::Undone, Failure>>),
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
    /// How a destination's disk and the default folder are looked at.
    pub looks: super::Looks,
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
    pub(super) guard: Guard,
    /// The ids given so far, and where the records are kept.
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
    /// Polls of librqbit run so far, for tests to wait on one.
    pub(super) polls: u64,
    /// Reports of the watcher (or of a refused add) handled so far, for tests to wait on one.
    pub(super) changes_seen: u64,
    /// Reports of the looks at disks and at the default folder handled so far, for tests to wait on.
    pub(super) looks_reported: u64,
    /// Set as a stop begins, for tests to wait on: the actor answers nothing then.
    #[cfg(test)]
    pub(super) stopping: Arc<std::sync::atomic::AtomicBool>,
    add_into: AddInto,
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
    files_went: mpsc::UnboundedReceiver<(u64, TorrentId, Trashed)>,
    files_went_tx: mpsc::UnboundedSender<(u64, TorrentId, Trashed)>,
    /// The disk of each destination, as the last look found it; not those that can't be reached.
    pub(super) disks: BTreeMap<AbsolutePath, space::Known>,
    /// The bytes allocated to each file of a torrent that counts on its disk, as the last look
    /// found them; a file not looked at yet isn't here.
    allocated: BTreeMap<PathBuf, u64>,
    /// The torrents librqbit last said had run out of space.
    full: BTreeSet<TorrentId>,
    /// The destinations and files to look at, for `space::watch`.
    space: watch::Sender<space::Wanted>,
    /// The default destination, for `default_folder::watch`.
    default_folder: watch::Sender<default_folder::Folder>,
    /// The torrents whose open files are looked at, for `letgo::watch`.
    gone_looks: watch::Sender<letgo::Wanted>,
    /// Why it can't be reached, as the last look found it. Unknown (so none) until the first look
    /// answers or stalls, a few seconds after start for a folder on a disk that doesn't answer.
    default_unavailable: Option<Unavailable>,
    /// When a finished torrent stops seeding.
    seeding: seeding::Limits,
    /// Undo's machine: the undo stack and each torrent's lane of steps, and the clients waiting on
    /// them (`steps.rs`, `undoing.rs`).
    pub(super) steps: steps::Steps,
    /// Each undo putting files back, by step: what's back, or the path that failed and why.
    putting_back: JoinSet<(u64, TorrentId, PutBack)>,
    /// The undos cut short by a crash, by step: what of them is no longer in the Trash is back.
    resumed: BTreeSet<(u64, TorrentId)>,
    /// The steps held, by step and torrent, while librqbit reads the pieces it saved of their torrent
    /// ([`lib::Lib::Resuming`]): started once it's done (`steps.rs`).
    held: BTreeSet<(u64, TorrentId)>,
    /// The torrents an undo added back (its put-back, or an aborted Delete's rollback) that start
    /// only once their lane is idle: what waited behind it runs first, so a Pause among it means
    /// it never starts (#468). Started, if wanted, at the end of [`Actor::run_steps`].
    starts_once_idle: BTreeSet<TorrentId>,
    /// How many of the next grants find a file busy, as on a loaded machine, for tests.
    #[cfg(test)]
    pub(super) busy_grants: u32,
    /// A test's hook, run as a Move to Trash has polled its torrent, before it sets the files Off.
    #[cfg(test)]
    pub(super) before_off: Option<Box<dyn FnMut(&mut Actor) + Send + Sync>>,
    /// The Deletes a crash cut short, found at start: `run` ends them.
    interrupted_deletes: Vec<TorrentId>,
    /// Adds of a torrent something runs on (an undo bringing it back, its Delete), until it's over.
    pub(super) adds_waiting: Vec<(Prepared, bool, oneshot::Sender<Result<Added, Failure>>)>,
    /// Where a test kills it: as torrent `.0` is taken off the list, before its record goes (after
    /// what undoes it is kept); `.1` says it did.
    #[cfg(test)]
    pub(super) crash_before_forgetting: Option<(TorrentId, Arc<std::sync::atomic::AtomicBool>)>,
    /// Where a test kills it while it lets go of a file; `.1` says it did.
    #[cfg(test)]
    pub(super) crash_letting_go: Option<(letgo::CrashAt, Arc<std::sync::atomic::AtomicBool>)>,
    /// A test's gate on letting go: while it's held, what's to be let go of is written ahead and
    /// waits, the torrent left in librqbit as it is, until the test opens it.
    #[cfg(test)]
    pub(super) letting_go_held: bool,
    /// Killed by the test: it handles nothing more.
    #[cfg(test)]
    killed: bool,
    /// Each move under way (`moving.rs`), by torrent, and the tasks moving their files.
    moves: BTreeMap<TorrentId, moving::Mover>,
    move_tasks: JoinSet<(TorrentId, moving::Moved)>,
    /// What each copy copied so far, by torrent: done and total bytes.
    copied: mpsc::UnboundedReceiver<(TorrentId, u64, u64)>,
    copied_tx: mpsc::UnboundedSender<(TorrentId, u64, u64)>,
    /// A test's hold on moves: the gate their copies wait at, and copying on the same drive.
    #[cfg(test)]
    pub(super) move_hold: moving::Hold,
}

/// Why the engine stops: the service stops, or the engine starts again with new settings (the
/// listening port, UPnP) while the service carries on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    Shutdown,
    Restart,
}

/// A torrent's librqbit side, with librqbit's own torrent and the task of its first add.
pub(super) type Lib = lib::Lib<Held, JoinHandle<()>>;

/// How an undo's put-back went: what's back, or the path that failed and why, with what went back
/// and couldn't return.
type PutBack = Result<Vec<Trashed>, (trash::PutBackError, Vec<PathBuf>)>;

/// A Delete under way: its step, and what stops it after the item in flight.
struct Deleting {
    seq: u64,
    stop: Arc<AtomicBool>,
}

pub(super) struct Torrent {
    pub(super) id: TorrentId,
    /// What Phantom knows of it, kept across restarts: librqbit gets it from here.
    pub(super) record: Record,
    pub(super) lifecycle: State,
    /// Its librqbit side.
    pub(super) lib: Lib,
    /// What librqbit said of it at the last poll, which clients see until the next.
    polled: Polled,
    /// Its files at their paths (by index), as the last size check found them.
    pub(super) there: BTreeSet<usize>,
    /// Its seeding round, counted at each poll.
    pub(super) meter: seeding::Meter,
    /// How many times the actor added it to librqbit, for tests.
    #[cfg(test)]
    pub(super) added: u32,
    /// How many times it was started in librqbit, for tests.
    #[cfg(test)]
    pub(super) started: u32,
    /// How many times librqbit was told which files to get while it ran it, for tests.
    #[cfg(test)]
    pub(super) chosen_running: u32,
}

impl Actor {
    /// Takes over the torrents in `records`, wanted running or stopped as each says, and adds those
    /// that belong in librqbit to it: those in `unavailable` (by info hash, with why their destination
    /// can't be reached) are Destination unavailable; those missing files (as they went before the
    /// service stopped, or as a size check finds them now) are Missing files; neither is added. Those
    /// in `unreadable` (with why their .torrent can't be read) are an error, until a check reads it.
    pub(super) async fn new(
        setup: Setup,
        store: Store,
        records: Vec<Record>,
        mut unavailable: BTreeMap<String, Unavailable>,
        unreadable: BTreeMap<String, String>,
    ) -> Self {
        let Setup { session, data_dir, download_dir, guard, size_check_every, poll_every, trash, inbox, add_into, looks, seeding, undo } =
            setup;
        let mut ended = Vec::new();
        let mut torrents = BTreeMap::new();
        for mut record in records {
            let id = record.id;
            let away = unavailable.remove(&record.info_hash);
            // A Delete the service stopped in the middle of (a crash: a stop waits for it): what
            // of the torrent's files is still on disk is what it left; nothing, and it's done.
            // With its drive away there's no telling: all of it.
            let interrupted = !record.deleting.is_empty();
            if interrupted {
                let left = std::mem::take(&mut record.deleting)
                    .into_iter()
                    .filter(|path| away.is_some() || std::fs::symlink_metadata(path).is_ok())
                    .map(|path| Kept { path: AbsolutePath::new(path).expect("saved absolute"), why: KeptWhy::Interrupted });
                record.delete_failed = left.collect();
                // What it moved, as far as it was recorded, can be undone: `run` puts it on the stack.
                if !record.trashed.is_empty() {
                    ended.push(id);
                }
            }
            let unchecked = record.unchecked;
            // A Delete that ended with all of it gone but not off the list yet: a crash came between.
            let done = !interrupted && !record.trashed.is_empty() && record.delete_failed.is_empty();
            // Out of reach, or partly in the Trash on purpose: neither is missing files.
            let gone = if away.is_some() || !record.delete_failed.is_empty() { BTreeMap::new() } else { record.gone_at_start() };
            let phase = match away {
                // `run` takes it off the list.
                _ if (interrupted || done) && record.delete_failed.is_empty() => Phase::Deleted,
                // Its files partly in the Trash on purpose, wherever its drive is.
                away if !record.delete_failed.is_empty() => Phase::DeleteFailed { kept: record.delete_failed.clone(), away, unchecked },
                // What was known missing still is, once it's back (#464).
                Some(reason) => Phase::DestinationUnavailable { reason, gone: record.gone_at_start() },
                // Whatever files went meanwhile: only a check says.
                None if unchecked => Phase::Unchecked { gone: BTreeMap::new() },
                None if gone.is_empty() => Phase::Ready,
                None => Phase::MissingFiles { gone },
            };
            // Its .torrent can't be read: an error, for Check (or Resume) to read it again, or Remove.
            let phase = match unreadable.get(&record.info_hash) {
                Some(why) if phase != Phase::Deleted => {
                    Phase::Error { failure: Broke::AddAgainFailed { detail: format!("its .torrent can't be read: {why}") } }
                }
                Some(_) | None => phase,
            };
            // A move a crash cut short: finished past the point where the record names the new
            // destination, else its files go back where they were.
            let phase = match (&record.moving, phase) {
                (Some(m), phase) if phase != Phase::Deleted => match AbsolutePath::new(m.plan.to.clone()) {
                    Ok(to) => Phase::Moving { to, stage: moving::at_start(m), was: Box::new(phase) },
                    Err(_) => phase,
                },
                (_, phase) => phase,
            };
            // librqbit's saved pieces outlast a crash, and say more than the record of one downloading
            // since; they go as it's taken out, the record's doesn't (#370).
            let had = saved_progress(&super::session_folder(&data_dir), &record).unwrap_or_else(|| record.had.clone());
            // Away, or partly in the Trash: nothing is known of what it has until it's checked.
            let polled = match phase {
                Phase::DestinationUnavailable { .. } | Phase::DeleteFailed { .. } => Polled::default(),
                _ => Polled::kept(&record, had),
            };
            let mut torrent = Torrent {
                id,
                lifecycle: State::new(record.wants, phase),
                meter: seeding::Meter::new(record.round),
                polled,
                lib: lib::Lib::Out,
                there: BTreeSet::new(),
                record,
                #[cfg(test)]
                added: 0,
                #[cfg(test)]
                started: 0,
                #[cfg(test)]
                chosen_running: 0,
            };
            torrent.keep_known();
            torrents.insert(id, torrent);
        }
        let (went_tx, went) = mpsc::unbounded_channel();
        let (copied_tx, copied) = mpsc::unbounded_channel();
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
        space::watch(looks.disks, wanted, poll_every, move |report| {
            let Some(inbox) = space_inbox.upgrade() else { return false };
            // A full inbox drops the reading; the next look is soon.
            let _ = inbox.try_send(Message::Space(report));
            true
        });
        let (gone_looks, looked) = watch::channel(letgo::Wanted::new());
        let gone_inbox = inbox.clone();
        letgo::watch(guard.trashed().clone(), looked, size_check_every, move || gone_inbox.upgrade().is_some());
        let (default_folder, folder) = watch::channel(default_folder::Folder::new());
        let folder_inbox = inbox.clone();
        default_folder::watch(looks.default_folder, folder, poll_every, move |report| {
            let Some(inbox) = folder_inbox.upgrade() else { return false };
            let _ = inbox.try_send(Message::DefaultFolderLook(report));
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
            polls: 0,
            changes_seen: 0,
            looks_reported: 0,
            #[cfg(test)]
            stopping: Arc::default(),
            add_into,
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
            full: BTreeSet::new(),
            default_folder,
            gone_looks,
            default_unavailable: None,
            seeding,
            steps: steps::Steps::new(undo),
            putting_back: JoinSet::new(),
            resumed: BTreeSet::new(),
            held: BTreeSet::new(),
            starts_once_idle: BTreeSet::new(),
            #[cfg(test)]
            busy_grants: 0,
            #[cfg(test)]
            before_off: None,
            interrupted_deletes: ended,
            adds_waiting: Vec::new(),
            #[cfg(test)]
            crash_before_forgetting: None,
            #[cfg(test)]
            crash_letting_go: None,
            #[cfg(test)]
            letting_go_held: false,
            #[cfg(test)]
            killed: false,
            moves: BTreeMap::new(),
            move_tasks: JoinSet::new(),
            copied,
            copied_tx,
            #[cfg(test)]
            move_hold: Default::default(),
        };
        let mut actor = actor;
        // Before anything is added: an undo a crash cut short may have some of its files back.
        actor.reserve_resumed();
        actor.save();
        actor.save_undo();
        // librqbit restores nothing: each torrent that belongs there is added now, once its files
        // are size-checked as they are, so none missing is made again.
        let ids: Vec<TorrentId> = actor.torrents.keys().copied().collect();
        for &id in &ids {
            actor.size_check(id).await;
            if let Err(e) = actor.carry_out(id).await {
                warn!(id, error = ?e, "failed to add a torrent to librqbit at start");
            }
        }
        // Each resumes, listed as its record says: nothing waits for librqbit's sampling.
        for id in ids {
            actor.poll_torrent(id).await;
        }
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
                        let held = self.stop(ending, &mut inbox).await;
                        // Its torrents and session go before the answer: see `Engine::stop`.
                        drop(self);
                        let _ = reply.send(held);
                        return;
                    }
                    // As if the process died: nothing more is saved, and a Delete under way is
                    // left where it is.
                    #[cfg(test)]
                    Some(Message::Kill(reply)) => {
                        self.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
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
                    let Some((id, carried, planned)) = came_to(done) else { return };
                    self.trashed(id, carried, planned).await;
                    self.run_steps().await;
                }
                Some(done) = self.file_trashing.join_next(), if !self.file_trashing.is_empty() => {
                    let Some(ended) = came_to(done) else { return };
                    self.files_trashed(ended).await;
                    self.run_steps().await;
                }
                Some((id, moved)) = self.went.recv() => self.went(id, moved),
                Some((seq, id, moved)) = self.files_went.recv() => self.files_went(seq, id, moved),
                Some(done) = self.putting_back.join_next(), if !self.putting_back.is_empty() => {
                    let Some((seq, id, put)) = came_to(done) else { return };
                    self.put_back_ended(seq, id, put).await;
                    self.run_steps().await;
                }
                Some(done) = self.move_tasks.join_next(), if !self.move_tasks.is_empty() => {
                    let Some((id, moved)) = came_to(done) else { return };
                    self.move_task_ended(id, moved).await;
                    self.run_steps().await;
                }
                Some(copied) = self.copied.recv() => {
                    self.move_copied(copied).await;
                    self.run_steps().await;
                }
                _ = poll.tick() => {
                    self.poll_librqbit().await;
                    // A torrent librqbit is done sampling: what was held for it goes on.
                    self.run_steps().await;
                    self.polls += 1;
                }
                _ = size_checks.tick() => {
                    self.watch_destinations();
                    self.size_check_all().await;
                    self.keep_rounds();
                    // A file let go of is, once its torrent can be taken out.
                    self.let_go().await;
                    #[cfg(test)]
                    if self.killed {
                        return;
                    }
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
        // With the files each misses (deleted by hand while it's stopped in librqbit), and those it
        // lets go of, which the look closes if they're gone for good.
        let looked: letgo::Wanted = self
            .torrents
            .values()
            .filter(|t| t.lib.torrent().is_some())
            .map(|t| {
                let letting: Vec<usize> = match &t.record.letting_go {
                    machine::letgo::State::Gone { pieces } | machine::letgo::State::Stashed { pieces, .. } => {
                        pieces.keys().copied().collect()
                    }
                    machine::letgo::State::Holding => Vec::new(),
                };
                (t.record.info_hash.clone(), missing(&t.lifecycle.phase).into_iter().chain(letting).collect())
            })
            .collect();
        self.gone_looks.send_if_modified(|last| {
            let changed = *last != looked;
            *last = looked;
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
        Snapshot {
            torrents,
            details,
            volumes,
            undo: undo::stacked(self.steps.machine()),
            left: self.steps.machine().left.iter().cloned().collect(),
            default_unavailable: self.default_unavailable.clone(),
        }
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
        let record = &torrent.record;
        let folder = record.output_folder()?;
        let getting = files(record, &[], &BTreeSet::new()).into_iter().filter(|f| f.priority == Priority::Normal);
        Some((&record.destination, getting.map(|f| (folder.join(&f.path), f.size_bytes)).collect()))
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

    /// The default destination is `folder` now: what was found of the last one is no longer said,
    /// in the snapshot the answer waits for.
    fn default_folder(&mut self, folder: Option<AbsolutePath>, reply: oneshot::Sender<()>) {
        let now: default_folder::Folder = folder.into_iter().map(|f| (f, ())).collect();
        if *self.default_folder.borrow() != now {
            self.default_unavailable = None;
            self.default_folder.send_replace(now);
        }
        self.answer(reply, ());
    }

    /// What a look at the default folder found; one of a folder that's no longer it says nothing.
    fn default_folder_found(&mut self, report: default_folder::Report) {
        if self.default_folder.borrow().contains_key(&report.folder) {
            self.default_unavailable = report.unavailable;
        }
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
                        if !self.disks.contains_key(&report.destination) {
                            // Back (or new): a figure kept from before may be another drive's.
                            space::cache().refresh();
                        }
                        let known = space::Known { mount: disk.mount, name: disk.name, free_bytes: Some(disk.free_bytes) };
                        self.disks.insert(report.destination, known);
                    }
                    None => {
                        if self.disks.remove(&report.destination).is_some() {
                            space::cache().refresh();
                        }
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
            Message::Command { ids, command, reply } => self.arrive(ids, command, reply).await,
            Message::Moves { to, moves, reply } => self.arrive_moves(to, moves, reply).await,
            Message::Clashes { ids, to, reply } => drop(reply.send(self.clashes(ids, to).await)),
            Message::FreeName { id, to, reply } => drop(reply.send(self.free_name(id, &to))),
            Message::AddFinished { id, task, added } => self.add_finished(id, task, added).await,
            Message::FilesChanged(paths) => {
                self.files_changed(&paths).await;
                self.changes_seen += 1;
            }
            Message::Sampled(id) => {
                self.poll_torrent(id).await;
                self.run_steps().await;
            }
            Message::Space(report) => {
                self.space(report);
                self.looks_reported += 1;
            }
            Message::DefaultFolder(folder, reply) => self.default_folder(folder, reply),
            Message::DefaultFolderLook(report) => {
                self.default_folder_found(report);
                self.looks_reported += 1;
            }
            // Applied at the next poll.
            Message::Seeding(limits) => self.seeding = limits,
            Message::Undo(reply) => self.undo(reply).await,
            Message::Stop(..) => unreachable!("handled by run"),
            Message::Resume(carried, reply) => {
                let answers = self.steps.resume(carried);
                self.save_undo();
                self.deliver(answers);
                self.publish();
                let _ = reply.send(());
                self.run_steps().await;
            }
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => unreachable!("handled by run"),
        }
    }

    /// Stops everything; returns the pieces each torrent (by info hash) has, for `Engine::stop`,
    /// and the Moves a restart for new settings didn't start.
    async fn stop(&mut self, ending: Ending, inbox: &mut mpsc::Receiver<Message>) -> (Vec<(String, u64)>, steps::Carried) {
        // At a shutdown, what waits on a lane doesn't start: its client hears the service is
        // stopping, and an action leaves the stack as one that never started. The undos, pressed,
        // still run: an Undo never waits. At a restart for new settings the service carries on, so
        // everything waiting runs first, as asked, but a Move waiting its turn: the restart waits
        // only for the one under way, and those waiting go on once the engine is back (ADR 0008).
        #[cfg(test)]
        self.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
        match ending {
            Ending::Shutdown => self.report(steps::Fact::Stopping),
            Ending::Restart => self.report(steps::Fact::Restarting),
        }
        // At a shutdown, a move whose files aren't in place yet goes back, as a start after a crash
        // would have it. At a restart for new settings it's long work under way: the restart waits
        // for it, as for a Delete (ADR 0008).
        if ending == Ending::Shutdown {
            self.interrupt_moves().await;
            // What waited on a lane is dropped: a torrent it kept from starting stays stopped. One an
            // undo still under way adds back starts once it's back, as the undo was pressed.
            self.starts_once_idle.clear();
        }
        // What was held for librqbit's sampling runs now, and what's under way ends first, so how
        // it went is saved, and so do the undos after it.
        self.start_held().await;
        loop {
            tokio::select! {
                // The runtime ending cancelled a task: nothing more is saved, as at a crash.
                Some(done) = self.trashing.join_next(), if !self.trashing.is_empty() => {
                    let Some((id, carried, planned)) = came_to(done) else { return Default::default() };
                    self.trashed(id, carried, planned).await;
                    self.run_steps().await;
                }
                Some(done) = self.file_trashing.join_next(), if !self.file_trashing.is_empty() => {
                    let Some(ended) = came_to(done) else { return Default::default() };
                    self.files_trashed(ended).await;
                    self.run_steps().await;
                }
                Some(done) = self.putting_back.join_next(), if !self.putting_back.is_empty() => {
                    let Some((seq, id, put)) = came_to(done) else { return Default::default() };
                    self.put_back_ended(seq, id, put).await;
                    self.run_steps().await;
                }
                Some(done) = self.move_tasks.join_next(), if !self.move_tasks.is_empty() => {
                    let Some((id, moved)) = came_to(done) else { return Default::default() };
                    self.move_task_ended(id, moved).await;
                    self.run_steps().await;
                }
                // A restart waits for a move's copy: what it copied still shows.
                Some(copied) = self.copied.recv(), if !self.move_tasks.is_empty() => {
                    self.move_copied(copied).await;
                    self.publish();
                }
                // Cancel move takes a Move out of the queue meanwhile: the service sends it here
                // while it restarts. Nothing else is sent, but the actor's own news, which a stop
                // never needed. Only while a task runs, so the arm below still ends the stop.
                Some(message) = inbox.recv(), if ending == Ending::Restart && self.tasks_running() => {
                    if let Message::Command { ids, command: TorrentCommand::CancelMove, reply } = message {
                        self.arrive(ids, TorrentCommand::CancelMove, reply).await;
                    }
                }
                else => {
                    // A safety net: every arm above runs what its end let start already.
                    self.run_steps().await;
                    self.start_held().await;
                    if self.trashing.is_empty()
                        && self.file_trashing.is_empty()
                        && self.putting_back.is_empty()
                        && self.held.is_empty()
                        && !self.moving()
                    {
                        break;
                    }
                }
            }
        }
        let mut running = Vec::new();
        for torrent in self.torrents.values_mut() {
            torrent.keep_known();
            torrent.record.round = torrent.meter.round;
            // Stopped, what's at the missing files' paths at the next start is taken for what was there.
            if matches!(torrent.lifecycle.phase, Phase::MissingFiles { .. } | Phase::Unchecked { .. }) {
                torrent.record.missing.values_mut().for_each(|m| m.seen = files::Seen::NotYet);
            }
            match &torrent.lib {
                lib::Lib::In(handle) if matches!(handle.stats().state, TorrentStatsState::Live) => running.push(handle.clone()),
                lib::Lib::Adding(task) => task.abort(),
                lib::Lib::In(_) | lib::Lib::Resuming(_) | lib::Lib::Out => {}
            }
        }
        self.save();
        for handle in running {
            if let Err(e) = self.session.pause(&handle).await {
                warn!(error = %e, "failed to pause a torrent while stopping");
            }
        }
        let held = self.pieces_kept();
        self.session.stop().await;
        let carried = self.steps.carry();
        self.save_undo();
        (held, carried)
    }

    /// Whether a task of the stop's waits runs: a trashing, a put-back, a move's.
    fn tasks_running(&self) -> bool {
        !(self.trashing.is_empty() && self.file_trashing.is_empty() && self.putting_back.is_empty() && self.move_tasks.is_empty())
    }

    /// The pieces librqbit has of each torrent (by info hash), which its save as it stops holds.
    pub(super) fn pieces_kept(&self) -> Vec<(String, u64)> {
        self.torrents
            .values()
            .filter_map(|t| {
                let handle = t.lib.torrent()?;
                let piece_length = t.record.info.as_ref()?.piece_length;
                // librqbit's progress is bytes read while it checks: no piece is had.
                let stats = handle.stats();
                let had = if matches!(stats.state, TorrentStatsState::Initializing { .. }) { 0 } else { stats.progress_bytes };
                Some((t.record.info_hash.clone(), bitv::pieces_in(had, piece_length)))
            })
            .collect()
    }

    /// Decides `event` for torrent `id`, then makes librqbit match; once its phase changes, its files
    /// are size-checked, and that's decided too. A refused command fails with the reason.
    pub(super) async fn event(&mut self, id: TorrentId, mut event: Event) -> Result<(), Failure> {
        let session = self.session_folder();
        loop {
            let torrent = self.torrents.get_mut(&id).ok_or(Failure::torrent(id, Refusal::NotInList))?;
            let before = torrent.lifecycle.phase.clone();
            let next = decide(&torrent.lifecycle, &event).map_err(|refusal| Failure::torrent(id, refusal))?;
            let wants_changed = next.wants != torrent.lifecycle.wants;
            torrent.meter.wants_changed(torrent.lifecycle.wants, next.wants);
            torrent.lifecycle = next;
            // A file that was missing, or returning to a check, has its bytes unreadable: what
            // librqbit said of it last (kept through a check, or saved as the service stopped) isn't
            // counted while it's shown, until its check ends (#290, #366). Only shown: what librqbit
            // said stays as it was, as it's saved and what's decided from (is a file there still
            // the torrent's own).
            track_uncounted(&mut torrent.polled.uncounted, &before, &torrent.lifecycle.phase);
            // What Delete left is kept across restarts, and librqbit's storage for it touches
            // nothing meanwhile: restoring it would make the trashed files again, empty. What a
            // Delete under way is about to trash is saved by `delete`, and forgotten once it's over.
            // Not what's missing of one whose check was cancelled: it's Missing files at start, as any is.
            // Kept through a Delete, which Undo may take back (#150): it comes back Unchecked too.
            let unchecked = match &torrent.lifecycle.phase {
                Phase::Unchecked { gone } => gone.is_empty(),
                phase @ (Phase::Deleting { .. } | Phase::DeleteFailed { .. } | Phase::Moving { .. }) => phase.unchecked(),
                Phase::Deleted => torrent.record.unchecked,
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
            // Back with a file of a size it can't have: whatever makes it run again, the rest is checked
            // in full, as any that was away (one away at a start still has its pieces saved). Before
            // it's saved as missing: a crash between leaves one checked in full, not pieces trusted.
            if matches!((&before, &torrent.lifecycle.phase), (Phase::DestinationUnavailable { .. }, Phase::MissingFiles { .. })) {
                bitv::forget(&bitv::path(&session, &torrent.record.info_hash));
            }
            let (wants, round) = (torrent.lifecycle.wants, torrent.meter.round);
            // Before librqbit is made to match, which can drop the pieces it saved. Saved at every
            // change, not only when it learnt something: a save that failed then is made up for, as
            // the store writes what differs from its last successful one (#370).
            let phase_changed = torrent.lifecycle.phase != before;
            if phase_changed {
                torrent.keep_known();
            }
            let record = &mut torrent.record;
            if wants_changed
                || phase_changed
                || record.unchecked != unchecked
                || record.delete_failed != kept
                || (!deleting && !record.deleting.is_empty())
                || (!undoable && (record.stash.is_some() || !record.trashed.is_empty()))
            {
                record.wants = wants;
                record.round = round;
                record.unchecked = unchecked;
                // Only lifted here: kept untouched by the Delete's trashing, off the actor, as
                // that waits for a disk write (see `delete`).
                if kept.is_empty() && !deleting {
                    self.guard.keep_untouched(&record.info_hash, false);
                }
                record.delete_failed = kept;
                if !deleting {
                    record.deleting.clear();
                }
                if !undoable {
                    record.stash = None;
                    record.trashed.clear();
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

    /// Makes librqbit match the torrent, its lifecycle and its record ([`lib::reconcile`]), step by
    /// step: one in librqbit to be checked is taken out, then added again.
    pub(super) async fn carry_out(&mut self, id: TorrentId) -> Result<(), Failure> {
        // Out, then in: no more steps than that make it match.
        for _ in 0..3 {
            let Some(torrent) = self.torrents.get(&id) else { return Ok(()) };
            let Some(todo) = lib::reconcile(&torrent.lifecycle, &torrent.lib, &torrent.record.normal()) else { return Ok(()) };
            let handle = torrent.lib.torrent().cloned();
            match (todo, handle) {
                (lib::Do::Resume, _) => self.add_to_librqbit(id, false).await?,
                (lib::Do::Check, _) => self.add_to_librqbit(id, true).await?,
                (lib::Do::TakeOut, _) => self.take_out(id).await?,
                // Added back by an undo, not while steps wait on its lane: they come first (#468).
                (lib::Do::Start, Some(_)) if self.starts_once_idle.contains(&id) && self.steps.machine().lanes.contains_key(&id) => {
                    return Ok(());
                }
                // Not while it has pieces to let go of (#234): librqbit would offer some it can't read,
                // and drop each peer that asks. It starts once it's added back without them.
                (lib::Do::Start, Some(_)) if matches!(torrent.record.letting_go, machine::letgo::State::Gone { .. }) => return Ok(()),
                // Nor is a file to let go of, set Normal again, handed to it as it runs (#402): one in use
                // isn't closed yet, and would be downloaded into where it went. Added back, it gets it.
                (lib::Do::Priorities, Some(handle))
                    if handle.live().is_some()
                        && matches!(&torrent.record.letting_go, machine::letgo::State::Gone { pieces }
                            if pieces.keys().any(|i| torrent.record.gets(*i))) =>
                {
                    return Ok(());
                }
                // Nor with a file there of a size it can't have (#374), nor is such a file handed to it
                // as it runs: it would be downloaded into until the next size check, which judges the
                // files the same way and makes it Missing files, or, put right by then, carries this out.
                (lib::Do::Start, Some(_)) if self.any_impossible(id) => return Ok(()),
                (lib::Do::Priorities, Some(handle)) if handle.live().is_some() && self.any_impossible(id) => return Ok(()),
                (lib::Do::Start, Some(handle)) => {
                    let normal = self.torrents[&id].record.normal();
                    if !self.grant(id, &handle, &normal).await? {
                        return Ok(());
                    }
                    self.ask_for_all_not_had(&handle).await?;
                    self.session.unpause(&handle).await?;
                    // librqbit's upload counter starts again with the torrent.
                    if let Some(torrent) = self.torrents.get_mut(&id) {
                        torrent.meter.librqbit_started();
                        #[cfg(test)]
                        {
                            torrent.started += 1;
                        }
                    }
                }
                (lib::Do::Stop, Some(handle)) => self.session.pause(&handle).await?,
                (lib::Do::Priorities, Some(handle)) => {
                    // A file it's to get as it runs is let write first.
                    if handle.live().is_some() && !self.grant(id, &handle, &self.torrents[&id].record.normal()).await? {
                        return Ok(());
                    }
                    let normal = self.torrents[&id].record.normal().into_iter().collect();
                    #[cfg(test)]
                    if handle.live().is_some() {
                        self.torrents.get_mut(&id).expect("just found").chosen_running += 1;
                    }
                    self.session.update_only_files(&handle, &normal).await?;
                }
                (lib::Do::Start | lib::Do::Stop | lib::Do::Priorities, None) => unreachable!("only on a torrent librqbit has"),
            }
        }
        Ok(())
    }

    /// Lets librqbit write the files `files` of torrent `id` (those it's to get), each if it's still the
    /// file its check read ([`Trashed::grant`](super::trashed::Trashed::grant)), off the actor. Whether
    /// every one may be: if not, another file at a path makes the check due again (it's added again),
    /// and one that isn't the torrent's to write is size-checked, which finds it Missing files; one that
    /// can't be opened is an Error.
    async fn grant(&mut self, id: TorrentId, handle: &Held, files: &BTreeSet<usize>) -> Result<bool, Failure> {
        let Some(torrent) = self.torrents.get(&id) else { return Ok(false) };
        let Some(info) = &torrent.record.info else { return Ok(true) };
        let destination = torrent.record.destination.to_path_buf();
        let granting = Granting::of(info, handle.stats().file_progress, destination.clone());
        let (trashed, hash, files) = (self.guard.trashed().clone(), torrent.record.info_hash.clone(), files.clone());
        // Refused as busy, without a look, as a file held past `slots::BUSY_FOR` is (#468).
        #[cfg(test)]
        if self.busy_grants > 0 {
            self.busy_grants -= 1;
            return Ok(false);
        }
        let refused = tokio::task::spawn_blocking(move || trashed.grant(&hash, &granting, &files))
            .await
            .map_err(|e| anyhow!("granting its files: {e}"))?;
        if !refused.is_empty() {
            warn!(id, ?refused, "its files aren't the ones its check read, or not its own to write");
        }
        if let Some(detail) = refused.iter().find_map(|r| match r {
            Refused::Failed(detail) => Some(detail.clone()),
            Refused::Changed | Refused::NotItsOwn | Refused::Busy => None,
        }) {
            let torrent = self.torrents.get_mut(&id).expect("just found");
            let failed = Event::Observed(Observation::Failed(broke(&detail)));
            torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
            return Ok(false);
        }
        if refused.contains(&Refused::Changed) {
            self.take_out(id).await?;
            self.add_to_librqbit(id, true).await?;
            return Ok(false);
        }
        if refused.contains(&Refused::NotItsOwn) {
            if let Some(inbox) = self.inbox.upgrade() {
                let _ = inbox.try_send(Message::FilesChanged(vec![destination]));
            }
        }
        // One busy is granted at the next event: the size checks' come every few seconds.
        Ok(refused.is_empty())
    }

    /// Grants torrent `id`'s files it gets now, on the actor, as its start does: whether every one was.
    #[cfg(test)]
    pub(super) fn grant_now(&self, id: TorrentId) -> bool {
        let Some(torrent) = self.torrents.get(&id) else { return false };
        let (Some(info), Some(handle)) = (&torrent.record.info, torrent.lib.torrent()) else { return false };
        let granting = Granting::of(info, handle.stats().file_progress, torrent.record.destination.to_path_buf());
        self.guard.trashed().grant(&torrent.record.info_hash, &granting, &torrent.record.normal()).is_empty()
    }

    /// Has librqbit, stopped, ask its peers for every piece it doesn't have of the files it gets.
    /// One whose hash it was checking as it was paused it neither has nor asks for again (#375,
    /// ikatson/rqbit#773), but a file it's told to get has all its missing pieces asked for: so it's
    /// told to get none, then the same files. No piece is had or lost by it. Both run before the
    /// actor polls again, so no client sees it getting none. Once librqbit queues such a
    /// piece itself, this can go.
    async fn ask_for_all_not_had(&self, handle: &Held) -> Result<()> {
        let stats = handle.stats();
        if !matches!(stats.state, TorrentStatsState::Paused) || stats.finished {
            return Ok(());
        }
        let files = handle.gets().into_iter().collect();
        self.session.update_only_files(handle, &HashSet::new()).await?;
        self.session.update_only_files(handle, &files).await?;
        Ok(())
    }

    /// Adds torrent `id` to librqbit from its record, stopped, its files where the record says and
    /// getting those it says; under its own id, so no two of librqbit's torrents share one. `checks`:
    /// with none of the pieces librqbit saved, so it checks them all; else with them, resuming
    /// ([`lib::Lib::Resuming`]). On failure it's an Error, out of librqbit, until Resume or Check adds
    /// it again.
    async fn add_to_librqbit(&mut self, id: TorrentId, checks: bool) -> Result<(), Failure> {
        // One whose .torrent couldn't be read at the start: read again.
        let record = &self.torrents.get(&id).context("not in the list")?.record;
        if record.info.is_none() {
            let read = self.store.read_info(record).map_err(|why| anyhow!("its .torrent can't be read: {why}"));
            let torrent = self.torrents.get_mut(&id).expect("just found");
            match read {
                Ok(info) => torrent.record.info = Some(info),
                Err(e) => {
                    let detail = format!("{e:#}");
                    let failed = Event::Observed(Observation::Failed(Broke::AddAgainFailed { detail: detail.clone() }));
                    torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                    return Err(proto::ServiceError::AddAgainFailed { id, detail }.into());
                }
            }
            self.save();
        }
        let record = &self.torrents[&id].record;
        let saved = bitv::path(&self.session_folder(), &record.info_hash);
        if checks {
            bitv::forget(&saved);
        }
        // A check finds what's on disk: nothing is left to let go of (#234), a file gone for good
        // having none of its pieces once it's checked.
        if checks && matches!(record.letting_go, machine::letgo::State::Gone { .. }) {
            self.torrents.get_mut(&id).expect("just found").record.letting_go = machine::letgo::State::Holding;
            self.save();
        }
        let record = &self.torrents[&id].record;
        // Nothing saved, librqbit checks it all: a check like any.
        let resumes = !checks && saved.exists();
        let added = self.add_from_record(record).await;
        let torrent = self.torrents.get_mut(&id).expect("being added");
        match added {
            Ok(handle) => {
                #[cfg(test)]
                {
                    torrent.added += 1;
                }
                torrent.meter.librqbit_started();
                // Resuming, its row stays as it was until librqbit trusts its pieces.
                if resumes {
                    let (sampling, inbox) = (handle.clone(), self.inbox.clone());
                    tokio::spawn(async move {
                        let _ = sampling.wait_until_initialized().await;
                        if let Some(inbox) = inbox.upgrade() {
                            let _ = inbox.send(Message::Sampled(id)).await;
                        }
                    });
                    torrent.lib = lib::Lib::Resuming(handle);
                    return Ok(());
                }
                torrent.polled = Polled::of(&handle.stats(), &torrent.polled);
                torrent.lib = lib::Lib::In(handle);
                // Added again, it checks; with nothing saved, its lifecycle hears librqbit check it at
                // the next poll.
                if checks {
                    torrent.lifecycle = decide(&torrent.lifecycle, &Event::Observed(Observation::AddedAgain)).expect("never refused");
                }
                Ok(())
            }
            Err(e) => {
                let detail = format!("{e:#}");
                let failed = Event::Observed(Observation::Failed(Broke::AddAgainFailed { detail: detail.clone() }));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                torrent.lib = lib::Lib::Out;
                Err(proto::ServiceError::AddAgainFailed { id, detail }.into())
            }
        }
    }

    /// librqbit's torrent for `record`, added stopped.
    async fn add_from_record(&self, record: &Record) -> Result<Held> {
        let info = record.info.as_ref().context("a torrent without info isn't added")?;
        let folder = record.output_folder().context("a torrent with info has its folder")?;
        // Its folder and priorities known, the files set Off aren't made (#67).
        let normal: Vec<usize> = record.normal().into_iter().collect();
        let one_file = info.single().then(|| record.on_disk.as_ref().map(PathBuf::from)).flatten();
        let storage = self.guard.for_add(record.destination.clone(), folder.clone(), &normal, one_file);
        let opts = AddTorrentOptions {
            paused: true,
            overwrite: true,
            only_files: Some(normal),
            output_folder: Some(folder.to_str().context("the destination isn't valid UTF-8")?.to_owned()),
            preferred_id: Some(record.id.try_into()?),
            storage_factory: Some(storage.boxed()),
            ..Default::default()
        };
        match self.session.add_torrent(AddTorrent::from_bytes(info.bytes.to_vec()), Some(opts)).await? {
            AddTorrentResponse::Added(_, t) => Ok(Held::new(t, &self.session)),
            AddTorrentResponse::AlreadyManaged(_, t) if t.info_hash().as_string() == record.info_hash => Ok(Held::new(t, &self.session)),
            AddTorrentResponse::AlreadyManaged(..) | AddTorrentResponse::ListOnly(_) => {
                Err(anyhow!("librqbit has another torrent under its id"))
            }
        }
    }

    /// Takes torrent `id` out of librqbit, keeping its files; what librqbit saved of its pieces goes
    /// with it.
    pub(super) async fn take_out(&mut self, id: TorrentId) -> Result<()> {
        let Some(handle) = self.torrents.get(&id).and_then(|t| t.lib.torrent()).cloned() else { return Ok(()) };
        self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await?;
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.lib = lib::Lib::Out;
        }
        Ok(())
    }

    /// Counts the torrent's seeding round from librqbit's `stats`, and tells its lifecycle when
    /// that reached a seeding limit while it seeds.
    /// librqbit's figures read now, after the poll's event (which may have started it again).
    async fn meter(&mut self, id: TorrentId, handle: &Held) {
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
        for torrent in self.torrents.values_mut() {
            if torrent.record.round != torrent.meter.round {
                torrent.record.round = torrent.meter.round;
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    /// Watches the destinations of the torrents with info, for files going missing.
    pub(super) fn watch_destinations(&mut self) {
        let Some(watcher) = &mut self.watcher else { return };
        let folders = self.torrents.values().filter(|t| t.record.info.is_some()).map(|t| t.record.destination.to_path_buf()).collect();
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
            .filter(|(_, t)| t.record.location().is_some_and(|l| changed.iter().any(|c| files::touches(c, &l))))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.size_check(id).await;
        }
    }

    /// Size-checks the torrent, for its machine to decide what that means.
    pub(super) async fn size_check(&mut self, id: TorrentId) {
        let seen = self.size_checked(id);
        self.look_at_files(id, seen.as_ref());
        let Some(seen) = seen else { return };
        if let Err(e) = self.event(id, Event::Observed(seen)).await {
            warn!(id, error = ?e, "failed to follow the torrent's files");
        }
        self.off_gone(id);
    }

    /// Which of the torrent's files are at their paths now, every one, set Off or not: for what
    /// clients may do to each (`File::on_disk`) and for the files set Off gone by hand. Nothing of a
    /// torrent whose destination is out of reach. One stat per file: the files a size check `seen`
    /// are told by it, the rest (those set Off) are looked at here.
    fn look_at_files(&mut self, id: TorrentId, seen: Option<&Observation>) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        let Some(info) = &torrent.record.info else { return };
        let there = match files::unavailable(&torrent.record.destination) {
            Some(_) => BTreeSet::new(),
            None => {
                let paths = torrent.record.paths();
                let statted = match seen {
                    Some(Observation::Files(disk)) => Some(&disk.files),
                    Some(_) | None => None,
                };
                (0..info.files.len())
                    .filter(|&i| !info.files[i].padding)
                    .filter(|i| match statted.and_then(|files| files.get(i)) {
                        Some(found) => found.at.is_some(),
                        None => paths.get(*i).is_some_and(|p| files::stamp(p).is_some()),
                    })
                    .collect()
            }
        };
        torrent.there = there;
    }

    /// What a size check of the torrent finds: its destination out of reach, or the files it's
    /// getting as they are on disk. Not its files while librqbit checks them, which it's writing
    /// then (restoring a torrent makes its missing files again); nothing without info.
    fn size_checked(&self, id: TorrentId) -> Option<Observation> {
        let torrent = self.torrents.get(&id)?;
        torrent.record.info.as_ref()?;
        if let Some(reason) = files::unavailable(&torrent.record.destination) {
            return Some(Observation::DestinationGone { reason });
        }
        if torrent.lib.torrent().is_some_and(|h| matches!(h.stats().state, TorrentStatsState::Initializing { .. })) {
            return None;
        }
        // Gone while its files were looked at, every one seemed missing: it's the destination that is.
        // Open for writing, one gone from its path went; not one let go of, which is got from nothing.
        let letting_go: BTreeSet<usize> = match &torrent.record.letting_go {
            machine::letgo::State::Holding => BTreeSet::new(),
            machine::letgo::State::Gone { pieces } | machine::letgo::State::Stashed { pieces, .. } => pieces.keys().copied().collect(),
        };
        let open: BTreeSet<usize> = self.guard.trashed().writable(&torrent.record.info_hash).difference(&letting_go).copied().collect();
        let shared = torrent.record.info.as_ref().map(Info::shared_bytes).unwrap_or_default();
        Some(match files::seen(&torrent.record.destination, || files::disk_of(&expected(torrent), &open, &shared)) {
            Ok(disk) => Observation::Files(disk),
            Err(reason) => Observation::DestinationGone { reason },
        })
    }

    /// Whether a size check now finds a file [of a size it can't have](files::Disk::any_impossible).
    fn any_impossible(&self, id: TorrentId) -> bool {
        matches!(self.size_checked(id), Some(Observation::Files(disk)) if disk.any_impossible())
    }

    /// A command that isn't a step, on its torrent's lane.
    async fn command(&mut self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        // Decided on what librqbit is doing now and on its files as they are now, not at the last
        // poll or once the watcher notices.
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(command.action())).await?;
        if matches!(command, TorrentCommand::Check) {
            // The user asked: what's free is read afresh at the next look.
            space::cache().refresh();
        }
        match command {
            TorrentCommand::Remove
            | TorrentCommand::Delete
            | TorrentCommand::TrashFiles { .. }
            | TorrentCommand::Move { .. }
            | TorrentCommand::Rename { .. } => {
                unreachable!("a step is its lane's (`steps.rs`)")
            }
            TorrentCommand::CancelMove => unreachable!("answered as it comes, on no lane (`steps.rs`)"),
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

    /// Tells the torrent's lifecycle what librqbit is doing, which makes librqbit match it.
    pub(super) async fn poll_torrent(&mut self, id: TorrentId) {
        // Out of librqbit, or being added: nothing to follow until librqbit has it.
        let handle = match self.torrents.get(&id).map(|t| &t.lib) {
            Some(lib::Lib::In(handle)) => handle.clone(),
            // Added with its pieces: its row stays as it was while librqbit samples them. One it
            // checks in full, having found one it can't trust (it drops what it saved then), is a
            // check like any.
            Some(lib::Lib::Resuming(handle)) => {
                let handle = handle.clone();
                let saved = bitv::path(&self.session_folder(), &handle.info_hash().as_string()).exists();
                if saved && matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
                    return;
                }
                self.torrents.get_mut(&id).expect("just found").lib = lib::Lib::In(handle.clone());
                handle
            }
            Some(lib::Lib::Adding(_) | lib::Lib::Out) | None => return,
        };
        let stats = handle.stats();
        let torrent = self.torrents.get_mut(&id).expect("just found");
        let partly_deleted = matches!(torrent.lifecycle.phase, Phase::DeleteFailed { .. });
        let polled = Polled::of(&stats, &torrent.polled);
        // Partly deleted, librqbit still holds the stopped torrent and says it has a file that was
        // missing as the Delete began: what it said of each file stands as `event` left it, none
        // of a file whose bytes can't be read (#290, #297).
        torrent.polled = match partly_deleted {
            true => Polled { file_progress: std::mem::take(&mut torrent.polled.file_progress), ..polled },
            false => polled,
        };
        let seen = librqbit_seen(&stats);
        // A torrent that has just run out of space: its disk's free space is read afresh (once, not
        // at every poll of a torrent that stays failed).
        let full = matches!(seen, Observation::Failed(Broke::DiskFull));
        if full && self.full.insert(id) {
            space::cache().refresh();
        } else if !full {
            self.full.remove(&id);
        }
        if let Err(e) = self.event(id, Event::Observed(seen)).await {
            warn!(id, error = ?e, "failed to follow librqbit");
        }
        self.meter(id, &handle).await;
    }

    /// Answers at once: with the torrent already in the list with its info hash, or with a new one
    /// fetching info.
    pub(super) fn add(&mut self, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added, Failure>>) {
        self.ask(add::State::Asked, prepared, paused, reply);
    }

    /// An add that waited for its hash, asked again once nothing runs on it.
    pub(super) fn add_again(&mut self, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added, Failure>>) {
        self.ask(add::State::Held, prepared, paused, reply);
    }

    /// An add at `stage` (`Asked`, or `Held` asked again), its hash looked up: [`add::decide`] says
    /// what follows.
    fn ask(&mut self, stage: add::State, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added, Failure>>) {
        // One something runs on (an undo bringing it back, its Delete) is added once that's over:
        // it's in the list then, or it's new.
        let hash = match (self.hash_busy(&prepared.info_hash), self.by_hash(&prepared.info_hash)) {
            (true, _) => add::Hash::Busy,
            (false, Some(_)) => add::Hash::Listed,
            (false, None) => add::Hash::Free,
        };
        let Ok(stage) = add::decide(&stage, &add::Event::Observed(add::Observed::Seen(hash)));
        match stage {
            add::State::Held => self.adds_waiting.push((prepared, paused, reply)),
            add::State::Existing => {
                let torrent = self.by_hash(&prepared.info_hash).expect("it was listed");
                let answer = Added { id: torrent.id, name: torrent.name(), already_existed: true };
                self.answer(reply, Ok(answer));
            }
            add::State::Fetching => {
                let id = self.store.next_id();
                let wants = if paused { Wants::Stop } else { Wants::Run };
                let Prepared { add, info_hash, name, trackers, destination } = prepared;
                let added = Added { id, name: name.clone(), already_existed: false };
                self.fetch(Record::new(info_hash, id, name, destination, trackers, wants), add);
                self.save();
                self.answer(reply, Ok(added));
            }
            add::State::Asked | add::State::Done | add::State::Failed | add::State::Removed | add::State::Orphan => {
                unreachable!("a hash seen leaves {stage:?}")
            }
        }
    }

    /// The torrent in the list with `info_hash`, if one is.
    pub(super) fn by_hash(&self, info_hash: &str) -> Option<&Torrent> {
        self.torrents.values().find(|t| t.record.info_hash == info_hash)
    }

    /// Starts the add of `record`'s torrent, fetching its info: in the list at once.
    pub(super) fn fetch(&mut self, record: Record, add: AddTorrent<'static>) {
        let id = record.id;
        // Its own task: fetching info takes as long as it takes, and Remove can stop it.
        let adding =
            (self.add_into)(self.session.clone(), id, add, record.destination.clone(), self.download_dir.clone(), self.guard.clone());
        let inbox = self.inbox.clone();
        let task = tokio::spawn(async move {
            let added = adding.await;
            if let Some(inbox) = inbox.upgrade() {
                let _ = inbox.send(Message::AddFinished { id, task: tokio::task::id(), added }).await;
            }
        });
        let torrent = Torrent {
            id,
            lifecycle: State::new(record.wants, Phase::FetchingInfo),
            lib: lib::Lib::Adding(task),
            polled: Polled::default(),
            there: BTreeSet::new(),
            meter: seeding::Meter::new(record.round),
            record,
            #[cfg(test)]
            added: 0,
            #[cfg(test)]
            started: 0,
            #[cfg(test)]
            chosen_running: 0,
        };
        self.torrents.insert(id, torrent);
    }

    /// An add of torrent `id` that isn't the one under way ends, as one taken off the list does.
    #[cfg(test)]
    pub(super) fn old_add_ends(&self, id: TorrentId) {
        let (task, added) = (tokio::spawn(async {}).id(), Err(anyhow!("cut short")));
        let inbox = self.inbox.upgrade().expect("it runs");
        // In the inbox as this returns: what's sent after comes after it.
        inbox.try_send(Message::AddFinished { id, task, added }).expect("room in the inbox");
    }

    /// Torrent `id`'s add task ended with `added`: [`add::decide`], from what the add was when it
    /// ended, says what it came to.
    async fn add_finished(&mut self, id: TorrentId, task: tokio::task::Id, added: Result<AddTorrentResponse>) {
        let added = match added {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => Ok(t),
            Ok(AddTorrentResponse::ListOnly(_)) => Err(anyhow!("unexpected list-only response")),
            Err(e) => Err(e),
        };
        // Fetching while its torrent is in the list with this task's add under way; else it was
        // taken off, and perhaps brought back by an undo, whose add is another task's.
        let stage = match self.torrents.get(&id).is_some_and(|t| matches!(&t.lib, lib::Lib::Adding(own) if own.id() == task)) {
            true => add::State::Fetching,
            false => add::State::Removed,
        };
        // Taken off, it said so then (`take_off_list`): what's said since is another task's.
        if stage == add::State::Fetching {
            self.guard.places().added(id);
        }
        let ended = if added.is_ok() { add::Ended::Added } else { add::Ended::Failed };
        let Ok(stage) = add::decide(&stage, &add::Event::Observed(add::Observed::Ended(ended)));
        let handle = match (stage, added) {
            (add::State::Done, Ok(handle)) => handle,
            (add::State::Failed, Err(e)) => {
                let torrent = self.torrents.get_mut(&id).expect("it was fetching");
                torrent.lib = lib::Lib::Out;
                let _ = self.event(id, Event::Observed(Observation::FetchFailed(format!("{e:#}")))).await;
                return;
            }
            // Removed while the add finished: take it back out, if librqbit still has this very torrent
            // and none is listed under its id (a Remove's own delete is usually there first).
            (add::State::Orphan, Ok(handle)) => {
                let held = self.session.get(TorrentIdOrHash::Id(handle.id())).is_some_and(|t| Arc::ptr_eq(&t, &handle));
                let world = add::World { running: false, listed: self.torrents.contains_key(&id), held };
                if add::next(&stage, &world) == Some(add::Do::TakeBackOut)
                    && let Err(e) = self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await
                {
                    warn!(error = %e, "failed to take a removed torrent back out");
                }
                return;
            }
            // Removed, and its add failed: nothing's left of it.
            (add::State::Removed, _) => return,
            // `decide` of an end gives Done with a torrent, Failed with an error, Orphan with a
            // torrent or Removed: no other pair.
            (add::State::Done, Err(_)) | (add::State::Failed, Ok(_)) | (add::State::Orphan, Err(_)) => {
                unreachable!("{stage:?} doesn't follow an end as it was")
            }
            (add::State::Asked | add::State::Held | add::State::Fetching | add::State::Existing, _) => {
                unreachable!("an add that ended is never {stage:?}")
            }
        };
        let torrent = self.torrents.get_mut(&id).expect("it was fetching");
        // Its info is the record's from now on: its .torrent is kept with it, in the same save.
        let info = handle.with_metadata(|m| Info::parse(&m.torrent_bytes)).map_err(anyhow::Error::from).and_then(|info| info);
        match info {
            Ok(info) => {
                torrent.record.name = handle.name().or(torrent.record.name.take());
                torrent.record.gets = Gets::Every;
                torrent.record.info = Some(info);
            }
            Err(e) => warn!(id, error = %e, "can't read the torrent's info"),
        }
        let checking = matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
        torrent.lib = lib::Lib::In(Held::new(handle, &self.session));
        self.save();
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
        // Where its files are, from its record, librqbit having it or not.
        let goes = self.torrents.get(&id).and_then(|t| to_trash(&t.record));
        if let Err(e) = self.event(id, Event::Command(proto::Action::Delete)).await {
            return self.step_ended(seq, id, steps::Over::Failed, Err(e));
        }
        let (Some(goes), Some(torrent)) = (goes, self.torrents.get(&id).filter(|t| matches!(t.lifecycle.phase, Phase::Deleting { .. })))
        else {
            let (ended, removed) = self.remove(id, proto::UndoAction::Delete, seq).await;
            return self.answer_step(ended, steps::Outcome::Done(removed.map_err(Failure::from)));
        };
        // Written ahead: a crash from here on comes back as what's left of it, never as a torrent
        // whose files librqbit makes again.
        let (hash, kept) = (torrent.record.info_hash.clone(), torrent.record.stash.is_some());
        // Kept aside for Undo while it's whole: once, before its first Delete, with the pieces it has.
        self.finish_letting_go(id).await;
        let stash = if kept { None } else { self.stash_of(id) };
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.record.deleting = goes.files.clone();
            torrent.record.stash = torrent.record.stash.take().or(stash);
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
            Err(e) => (self.end(seq, id, steps::Over::Failed), Err(e)),
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
        let hash = super::id20(&torrent.record.info_hash)?;
        self.save();
        if let lib::Lib::Adding(task) = torrent.lib {
            let Ok(stage) = add::decide(&add::State::Fetching, &add::Event::Command(add::Command::Remove));
            let world = add::World { running: !task.is_finished(), listed: false, held: false };
            if add::next(&stage, &world) == Some(add::Do::Abort) {
                task.abort();
            }
            let _ = task.await;
            self.guard.places().added(id);
        }
        let taken_out = match self.session.get(TorrentIdOrHash::Hash(hash)) {
            Some(t) => self.session.delete(TorrentIdOrHash::Id(t.id()), false).await,
            None => Ok(()),
        };
        // What librqbit saved of its pieces goes with it, whether librqbit had it or not: an add of
        // it again is a new one, checked in full. What undoes this kept its own. So does its
        // .torrent, which a start would take for librqbit having that new one (#334).
        bitv::forget(&bitv::path(&self.session_folder(), &torrent.record.info_hash));
        super::store::forget_librqbits(&self.session_folder(), &torrent.record.info_hash);
        // Touched again, the storage librqbit had would be too, but librqbit's delete took its
        // files out of it and dropped them: a check still ending finds no file to set the length
        // of. A librqbit that kept them would bring back #186.
        self.guard.keep_untouched(&torrent.record.info_hash, false);
        taken_out
    }

    /// Torrent `id`'s record, for one with info. Its lifecycle refuses the commands that need one
    /// before: one without is an internal error.
    pub(super) fn with_info(&self, id: TorrentId) -> Result<&Record> {
        self.torrents.get(&id).map(|t| &t.record).filter(|r| r.info.is_some()).with_context(|| format!("torrent {id} has no info"))
    }

    /// Torrent `id`'s details, its files' priorities as clients see them.
    fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        let record = self.with_info(id)?;
        let torrent = &self.torrents[&id];
        // Nothing is known of what an unchecked torrent has.
        let progress = if torrent.lifecycle.phase.unchecked() { &[] } else { torrent.polled.file_progress.as_slice() };
        let mut files = files(record, progress, &missing(&torrent.lifecycle.phase));
        uncount(&mut files, torrent);
        // One let go of, nothing at its path (moved to the Trash, say), shows none of it (#276): its
        // pieces go once its torrent is added back, and it would download it all set Normal.
        let letting = record.letting_go.files();
        for f in &mut files {
            f.on_disk = torrent.there.contains(&f.index);
            if letting.contains(&f.index) && !f.on_disk {
                f.progress_bytes = 0;
            }
        }
        let priorities = self.shown(id).map(|s| s.priorities).unwrap_or_default();
        shown_as(&mut files, &priorities);
        let location = record.location().context("a torrent with info has a location")?;
        let location = AbsolutePath::new(location).map_err(|p| anyhow!("torrent {id} is at a relative path: {}", p.display()))?;
        Ok(Details { id, files, location })
    }

    /// Files set Off aren't downloaded and drop out of the torrent's Size, down to none at all:
    /// it's then finished. The record says which it gets, from the moment it's asked; librqbit is
    /// told once it can take it (not during a check, nor before one of a torrent not checked).
    pub(super) async fn set_priority(&mut self, id: TorrentId, indices: &[usize], priority: Priority) -> Result<(), Failure> {
        self.set_priority_back(id, indices, priority, &BTreeSet::new()).await
    }

    /// [`Self::set_priority`], for files of which those in `back` were just put back at their paths:
    /// missing still to the torrent's phase (till it's checked or resumed), they aren't let go of.
    pub(super) async fn set_priority_back(
        &mut self,
        id: TorrentId,
        indices: &[usize],
        priority: Priority,
        back: &BTreeSet<usize>,
    ) -> Result<(), Failure> {
        let record = self.with_info(id)?;
        let files = files(record, &[], &BTreeSet::new());
        if let Some(bad) = indices.iter().find(|&&i| !files.iter().any(|f| f.index == i)) {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        let normal: BTreeSet<usize> = files
            .iter()
            .map(|f| (f.index, if indices.contains(&f.index) { priority } else { f.priority }))
            .filter(|(_, p)| *p == Priority::Normal)
            .map(|(i, _)| i)
            .collect();
        let torrent = self.torrents.get_mut(&id).expect("just found");
        // A file missing, set Off: librqbit would offer its pieces once the torrent runs, and can't
        // read them once the file is closed. It's let go of, written ahead, before it starts again.
        let set_off = |i: &usize| priority == Priority::Off && indices.contains(i) && !back.contains(i);
        let gone: machine::letgo::Pieces = match &torrent.record.info {
            Some(info) => missing(&torrent.lifecycle.phase)
                .into_iter()
                .filter(set_off)
                .filter_map(|i| Some((i, info.files.get(i)?.pieces.clone())))
                .map(|(i, r)| (i, (r.start, r.end)))
                .collect(),
            None => Default::default(),
        };
        torrent.record.gets = Gets::Only(normal);
        if !gone.is_empty() {
            torrent.record.letting_go = machine::letgo::decide(&torrent.record.letting_go, &machine::letgo::Event::Found(gone));
        }
        if priority == Priority::Normal {
            self.normal_again(id, indices, back);
        }
        self.save();
        self.carry_out(id).await
    }

    /// Torrent `id`'s files `indices`, set Normal, that aren't at their paths (#276): each is
    /// downloaded from nothing, not Missing files, which is for a file it gets that vanished.
    /// librqbit's pieces of each are let go of, as they're of a file that's elsewhere or gone. So is
    /// one it has open with none had
    /// (#402), or it would be downloaded into where it went, not at its path. Each is made at its path
    /// as the torrent is let run (its grant, #374), and the size check doesn't find it missing
    /// meanwhile, none of it had. Not one just put `back` by an undo, nor one missing already: that
    /// one vanished while it was Normal.
    fn normal_again(&mut self, id: TorrentId, indices: &[usize], back: &BTreeSet<usize>) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(info) = &torrent.record.info else { return };
        if files::unavailable(&torrent.record.destination).is_some() {
            return;
        }
        let (paths, have, missing) = (torrent.record.paths(), progress(torrent), missing(&torrent.lifecycle.phase));
        let again: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|i| !back.contains(i) && !missing.contains(i) && info.files.get(*i).is_some_and(|f| !f.padding))
            .filter(|i| paths.get(*i).is_some_and(|p| files::stamp(p).is_none()))
            .collect();
        if again.is_empty() {
            return;
        }
        let hash = torrent.record.info_hash.clone();
        let open = self.guard.trashed().open(&hash);
        let gone: machine::letgo::Pieces = again
            .iter()
            .filter(|i| open.contains(i) || have.get(**i).is_some_and(|had| *had > 0))
            .map(|&i| (i, (info.files[i].pieces.start, info.files[i].pieces.end)))
            .collect();
        if !gone.is_empty() {
            let closing = gone.keys().copied().collect();
            let torrent = self.torrents.get_mut(&id).expect("just found");
            torrent.record.letting_go = machine::letgo::decide(&torrent.record.letting_go, &machine::letgo::Event::Found(gone));
            self.guard.trashed().let_go(&hash, &closing);
        }
        self.look_at_files(id, None);
    }

    /// Torrent `id` as it is, its files' priorities as `priorities` says where it has them, its
    /// status and actions as `lifecycle` does (its own if none).
    fn listing_of(&self, id: TorrentId, priorities: &BTreeMap<usize, Priority>, lifecycle: Option<&State>) -> Option<TorrentInfo> {
        let t = self.torrents.get(&id)?;
        let lifecycle = lifecycle.unwrap_or(&t.lifecycle);
        match t.record.info {
            Some(_) => torrent_info(id, t, priorities, lifecycle),
            None => fetching_info(id, t, lifecycle),
        }
    }

    /// What torrent `id` shows it has of each file is what librqbit saved of it, as it's about to be
    /// added back with that: nothing, with nothing saved (it's checked in full then).
    pub(super) fn polled_as_saved(&mut self, id: TorrentId) {
        let session = self.session_folder();
        let Some(torrent) = self.torrents.get_mut(&id).filter(|t| t.record.info.is_some()) else { return };
        let had = saved_progress(&session, &torrent.record).unwrap_or_default();
        let kept = Polled::kept(&torrent.record, had);
        torrent.polled = Polled { finished: kept.finished, file_progress: kept.file_progress, ..std::mem::take(&mut torrent.polled) };
    }

    /// librqbit's session folder, where it saves the pieces of each torrent.
    pub(super) fn session_folder(&self) -> PathBuf {
        super::session_folder(&self.data_dir)
    }

    /// Keeps every torrent's record. Failing to is logged rather than failing the request: the
    /// torrents run on as they are.
    pub(super) fn save(&self) {
        if let Err(e) = self.try_save() {
            warn!(error = %e, "failed to save the engine's state");
        }
    }

    /// Saves the records, as [`Actor::save`]: an error if it can't, for a save something relies on.
    pub(super) fn try_save(&self) -> Result<()> {
        self.store.save(self.torrents.values().map(|t| &t.record))
    }
}

impl Torrent {
    /// Its name as clients see it: the one it has on disk (#331).
    pub(super) fn name(&self) -> Option<String> {
        self.record.shown_name()
    }

    /// Its record as it's kept, with what librqbit has of each file now: what brings it back
    /// shows that until librqbit has it again.
    pub(super) fn kept(&self) -> super::store::Saved {
        let mut saved = self.record.saved();
        // Brought back, its files are size-checked as they are then.
        saved.missing.clear();
        saved.had = if self.lifecycle.phase.unchecked() { Vec::new() } else { progress(self) };
        saved
    }

    /// Puts what's known of its files in its record: those missing, and what librqbit has of each.
    /// Saved as it's learnt, not only at a stop, as taking it out of librqbit
    /// drops the pieces it saved, and a start after a crash would then take any file at a missing
    /// one's path for the torrent's own (#370).
    pub(super) fn keep_known(&mut self) {
        // Away from an error, which keeps what was known missing in its record alone: that goes
        // with it, to be judged once it's back, not checked as the torrent's own (#464).
        if let Phase::DestinationUnavailable { gone, .. } = &mut self.lifecycle.phase
            && gone.is_empty()
        {
            *gone = self.record.gone_at_start();
        }
        let phase = &self.lifecycle.phase;
        let missing = match phase {
            Phase::MissingFiles { gone: files }
            | Phase::Unchecked { gone: files }
            | Phase::CheckDue { returning: files }
            | Phase::Checking { returning: files }
            | Phase::DestinationUnavailable { gone: files, .. } => files.iter().map(|(&i, gone)| (i, gone.into())).collect(),
            // Its files as they were: what was missing still is, for all that's known.
            Phase::Error { .. } | Phase::DeleteFailed { .. } | Phase::Deleting { .. } | Phase::Moving { .. } => self.record.missing.clone(),
            Phase::FetchingInfo | Phase::FetchFailed { .. } | Phase::Ready | Phase::Deleted => BTreeMap::new(),
        };
        let had = match phase {
            // Whatever is on disk is what a check of no file put back takes.
            Phase::CheckDue { returning } | Phase::Checking { returning } if returning.is_empty() => Vec::new(),
            phase if phase.unchecked() => Vec::new(),
            // librqbit says nothing while it checks, nor the last poll of one away at a start.
            _ => Some(progress(self)).filter(|p| !p.is_empty()).unwrap_or_else(|| self.record.had.clone()),
        };
        (self.record.missing, self.record.had) = (missing, had);
    }
}

/// What librqbit's saved pieces, in its `session` folder, say it has of each of `record`'s files.
fn saved_progress(session: &Path, record: &Record) -> Option<Vec<u64>> {
    let info = record.info.as_ref()?;
    let saved = std::fs::read(bitv::path(session, &record.info_hash)).ok()?;
    let lens: Vec<u64> = info.files.iter().map(|f| f.len).collect();
    Some(bitv::file_progress(&saved, &lens, info.piece_length))
}

impl Librqbit for Held {
    fn doing(&self) -> machine::Lib {
        // librqbit's own: no piece is counted for it.
        match (**self).stats().state {
            TorrentStatsState::Initializing { .. } => machine::Lib::Checking,
            TorrentStatsState::Live => machine::Lib::Running,
            TorrentStatsState::Paused => machine::Lib::Stopped,
            TorrentStatsState::Error => machine::Lib::Failed,
        }
    }

    fn gets(&self) -> BTreeSet<usize> {
        match self.only_files() {
            Some(files) => files.into_iter().collect(),
            None => self.with_metadata(|m| (0..m.file_infos.len()).collect()).unwrap_or_default(),
        }
    }
}

/// Adds a new torrent to librqbit with its files in `destination`: a torrent of several files in
/// its own folder there, named as librqbit names it. librqbit only names that folder inside its
/// own (`download_dir`), so it first fetches the info without adding, then adds it with the peers
/// it found meanwhile. `overwrite` lets a torrent resume into files that already exist. Stopped:
/// the actor starts it if it's wanted running. Under Phantom's id `id`: librqbit numbers a torrent
/// from the ones it has saved, so two adds at once would otherwise get the same number, and the
/// second the first's torrent. Every later add is the actor's, from the torrent's record.
pub(super) async fn add_into(
    session: Arc<Session>,
    id: TorrentId,
    add: AddTorrent<'static>,
    destination: AbsolutePath,
    download_dir: PathBuf,
    guard: Guard,
) -> Result<AddTorrentResponse> {
    add_from(session, id, add, destination, download_dir, guard, Vec::new()).await
}

/// [`add_into`], fetching the info from `peers` too, as a test's local seeder.
pub(super) async fn add_from(
    session: Arc<Session>,
    id: TorrentId,
    add: AddTorrent<'static>,
    destination: AbsolutePath,
    download_dir: PathBuf,
    guard: Guard,
    peers: Vec<std::net::SocketAddr>,
) -> Result<AddTorrentResponse> {
    let list =
        AddTorrentOptions { list_only: true, paused: true, initial_peers: Some(peers).filter(|p| !p.is_empty()), ..Default::default() };
    let listed = session.add_torrent(add, Some(list)).await?;
    let AddTorrentResponse::ListOnly(listed) = listed else { bail!("unexpected response to a list-only add") };
    let own_folder = listed.output_folder.strip_prefix(&download_dir).context("librqbit put the torrent elsewhere")?;
    // A magnet's info is only in now: one taking a name of Phantom's own is refused before any of
    // its files is made, as a .torrent file is at its add.
    for file in listed.info.iter_file_details() {
        let path = own_folder.join(file.filename.to_pathbuf());
        if path.components().any(|c| super::trashed::phantoms(c.as_os_str())) {
            bail!("{} takes the name of a folder of Phantom's own", path.display());
        }
    }
    let output_folder = destination.join(own_folder);
    // Never into a place an undo is putting files back to (#371): it waits till that's over.
    let paths = listed.info.iter_file_details().map(|f| output_folder.join(f.filename.to_pathbuf())).collect();
    guard.places().claim(id, paths).await;
    let all: Vec<usize> = (0..listed.info.iter_file_lengths().count()).collect();
    // Phantom's own storage, which knows its folder, as every add's: it never writes to a file moved
    // from its path.
    let storage = guard.for_add(destination, output_folder.clone(), &all, None);
    let output_folder = output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned();
    let opts = AddTorrentOptions {
        paused: true,
        overwrite: true,
        initial_peers: Some(listed.seen_peers),
        preferred_id: Some(id.try_into()?),
        storage_factory: Some(storage.boxed()),
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
fn librqbit_seen(stats: &TorrentStats) -> Observation {
    match stats.state {
        TorrentStatsState::Initializing { .. } => Observation::Checking,
        TorrentStatsState::Live | TorrentStatsState::Paused => Observation::Idle { have: stats.file_progress.clone() },
        TorrentStatsState::Error => Observation::Failed(broke(stats.error.as_deref().unwrap_or("unknown error"))),
    }
}

/// What a task of the actor's (a trashing, a put-back, a move's) came to, or `None` once its runtime
/// ends: tokio then cancels a blocking task not started yet, and one spawned after (#442). Each task
/// catches its own panic, so that's the only error. The actor ends then too, as at a crash, and the
/// next start finishes what the task was doing: each step is written ahead.
fn came_to<T>(done: Result<T, tokio::task::JoinError>) -> Option<T> {
    match done {
        Ok(came) => Some(came),
        Err(e) if e.is_cancelled() => None,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}

/// The files missing, by index: those of a torrent that's Missing files.
fn missing(phase: &Phase) -> BTreeSet<usize> {
    match phase {
        Phase::MissingFiles { gone } | Phase::Unchecked { gone } => gone.keys().copied().collect(),
        // As it was, until the trash says how it went, or the move ends.
        Phase::Deleting { was } | Phase::Moving { was, .. } => missing(was),
        _ => BTreeSet::new(),
    }
}

/// The files of `phase`'s torrent whose bytes can't be read: those missing, and those it had
/// missing that a check was to look at again.
fn unreadable(phase: &Phase) -> BTreeSet<usize> {
    match phase {
        Phase::CheckDue { returning } | Phase::Checking { returning } => returning.keys().copied().collect(),
        phase => missing(phase),
    }
}

/// Updates `uncounted` as a torrent's phase goes from `before` to `next`: files that can't be read
/// are added as it's set aside, kept while a check is yet to end, forgotten once none is.
pub(super) fn track_uncounted(uncounted: &mut BTreeSet<usize>, before: &Phase, next: &Phase) {
    match next {
        Phase::CheckDue { .. } | Phase::DeleteFailed { .. } | Phase::DestinationUnavailable { .. } => uncounted.extend(unreadable(before)),
        // An Error is no check's end: a file's bytes are as unreadable as they were.
        Phase::Checking { .. } | Phase::Unchecked { .. } | Phase::Deleting { .. } | Phase::Moving { .. } | Phase::Error { .. } => {}
        _ => uncounted.clear(),
    }
}

/// `files` as shown: none counted of those whose bytes can't be read now.
fn uncount(files: &mut [File], torrent: &Torrent) {
    for f in files.iter_mut().filter(|f| torrent.polled.uncounted.contains(&f.index)) {
        f.progress_bytes = 0;
    }
}

/// What [`trash::plan`] decides from, for the torrent: from its record alone, without the disk.
pub(super) struct ToTrash {
    pub(super) location: PathBuf,
    pub(super) files: Vec<PathBuf>,
    pub(super) single: bool,
}

/// Where the torrent's files are, once it has info.
pub(super) fn to_trash(record: &Record) -> Option<ToTrash> {
    Some(ToTrash { location: record.location()?, files: record.paths(), single: record.info.as_ref()?.single() })
}

/// What librqbit has of each of the torrent's files, by index: what it says now if it has the
/// torrent (nothing while it checks), else what it said last.
pub(super) fn progress(torrent: &Torrent) -> Vec<u64> {
    match torrent.lib.torrent() {
        Some(handle) => handle.stats().file_progress,
        None => torrent.polled.file_progress.clone(),
    }
}

/// The files the torrent is getting (those Normal), as they must be on disk: of one that was away,
/// by its record, which says what was had as it went, or before a start with it away, and nothing
/// while a check was to say.
fn expected(torrent: &Torrent) -> Vec<Expected> {
    let paths = torrent.record.paths();
    let had = match torrent.lifecycle.phase {
        Phase::DestinationUnavailable { .. } => torrent.record.had.clone(),
        _ => progress(torrent),
    };
    files(&torrent.record, &had, &BTreeSet::new())
        .into_iter()
        .filter(|f| f.priority == Priority::Normal)
        .map(|f| Expected { index: f.index, path: paths[f.index].clone(), size_bytes: f.size_bytes, progress_bytes: f.progress_bytes })
        .collect()
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
        info_hash: t.record.info_hash.clone(),
        name: t.name(),
        status: lifecycle.status(false)?,
        content: None,
        uploaded_bytes: 0,
        download_bytes_per_sec: 0,
        upload_bytes_per_sec: 0,
        live_peers: 0,
        original_name: None,
        trackers: t.record.trackers.clone(),
        location: None,
        actions: lifecycle.actions(),
    })
}

/// The torrent's files, without BEP 47 padding (which only aligns the others and is never written),
/// with the priorities its record says. `progress` is librqbit's bytes per file, empty while
/// checking. `missing` files are gone from disk, so none of them is downloaded, whatever librqbit
/// thinks.
pub(super) fn files(record: &Record, progress: &[u64], missing: &BTreeSet<usize>) -> Vec<File> {
    let Some(info) = &record.info else { return Vec::new() };
    let letting = record.letting_go.files();
    info.files
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.padding)
        .map(|(index, f)| File {
            index,
            path: record.file_path(f),
            size_bytes: f.len,
            // Let go of, it has none of it, whatever librqbit says until it's added back without it.
            progress_bytes: if missing.contains(&index) || letting.contains(&index) {
                0
            } else {
                progress.get(index).copied().unwrap_or(0)
            },
            priority: if record.gets(index) { Priority::Normal } else { Priority::Off },
            missing: missing.contains(&index),
            on_disk: false,
        })
        .collect()
}

/// Torrent `t` as listed, its files' priorities as `priorities` says where it has them; none while
/// it's leaving the list.
fn torrent_info(id: TorrentId, t: &Torrent, priorities: &BTreeMap<usize, Priority>, lifecycle: &State) -> Option<TorrentInfo> {
    let stats = &t.polled;
    // Size and progress from the files you're getting: librqbit counts whole pieces, which
    // spill into neighbouring files that may be Off.
    let mut files = files(&t.record, &stats.file_progress, &missing(&t.lifecycle.phase));
    uncount(&mut files, t);
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
        info_hash: t.record.info_hash.clone(),
        name: t.name(),
        status,
        content: Some(Content { size_bytes, full_size_bytes: files.iter().map(|f| f.size_bytes).sum(), progress_bytes }),
        uploaded_bytes: stats.uploaded_bytes,
        // Never "Paused · 2 MB/s": a status that isn't librqbit's yet has no rates.
        download_bytes_per_sec: if same { stats.download_bytes_per_sec } else { 0 },
        upload_bytes_per_sec: if same { stats.upload_bytes_per_sec } else { 0 },
        live_peers: if same { stats.live_peers } else { 0 },
        original_name: t.record.on_disk.as_ref().and(t.record.name.clone()),
        trackers: t.record.trackers.clone(),
        location: t.record.location().and_then(|path| AbsolutePath::new(path).ok()),
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
    /// Files whose bytes can't be read now (missing as their destination went, or as a delete
    /// failed): shown with none counted, whatever `file_progress` says of them. Never saved as what
    /// librqbit has (`undo.json` holds the row as shown, which only draws the step), nor decided
    /// from: `file_progress` is what librqbit said (#366).
    uncounted: BTreeSet<usize>,
    uploaded_bytes: u64,
    download_bytes_per_sec: u64,
    upload_bytes_per_sec: u64,
    live_peers: u32,
}

impl Polled {
    /// What librqbit last said of `record`'s torrent, before it has it again: it `had` this of each
    /// file (as the service stopped), and is finished once it has every file it gets.
    fn kept(record: &Record, had: Vec<u64>) -> Self {
        let finished = !had.is_empty()
            && files(record, &had, &BTreeSet::new()).iter().all(|f| f.priority == Priority::Off || f.progress_bytes >= f.size_bytes);
        Self { finished, file_progress: had, ..Self::default() }
    }

    /// The stats, keeping what `last` said of each file while librqbit says nothing of them.
    fn of(stats: &TorrentStats, last: &Polled) -> Self {
        let live = stats.live.as_ref();
        let file_progress = if stats.file_progress.is_empty() { &last.file_progress } else { &stats.file_progress };
        Self {
            finished: stats.finished,
            checked_bytes: stats.progress_bytes,
            file_progress: file_progress.clone(),
            uncounted: last.uncounted.clone(),
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

    /// Torrent `id` as if librqbit had a peer sending and taking `rate` bytes a second, till the next
    /// poll: the rates and peers it's listed with then.
    pub(super) fn fake_rates(&mut self, id: TorrentId, rate: u64) -> Option<(u64, u64, u32)> {
        let t = self.torrents.get_mut(&id)?;
        (t.polled.download_bytes_per_sec, t.polled.upload_bytes_per_sec, t.polled.live_peers) = (rate, rate, 1);
        self.listed(id).map(|t| (t.download_bytes_per_sec, t.upload_bytes_per_sec, t.live_peers))
    }
}

#[cfg(test)]
mod uncounted_tests {
    use super::*;

    /// A missing file whose check fails (librqbit erroring as the drive goes) and whose destination
    /// then goes: it stays uncounted, as no check ended (#366's review).
    #[test]
    fn an_error_without_a_check_ending_keeps_the_files_uncounted() {
        let expected = files::Expected { index: 0, path: "/gone/b.bin".into(), size_bytes: 10, progress_bytes: 10 };
        let gone = BTreeMap::from([(1, files::Gone::found(&files::disk_of(&[expected], &BTreeSet::new(), &[]).files[&0]))]);
        let error = Phase::Error { failure: Broke::Librqbit { detail: "drive gone".into() } };
        let away = Phase::DestinationUnavailable {
            reason: proto::Unavailable::FolderMissing { path: AbsolutePath::new("/away").unwrap() },
            gone: Default::default(),
        };
        let mut uncounted = BTreeSet::new();
        let mut steps = vec![(Phase::MissingFiles { gone: gone.clone() }, Phase::CheckDue { returning: gone.clone() })];
        steps.push((steps[0].1.clone(), Phase::Checking { returning: gone }));
        steps.push((steps[1].1.clone(), error.clone()));
        steps.push((error, away));
        for (before, next) in &steps {
            track_uncounted(&mut uncounted, before, next);
        }
        assert_eq!(uncounted, BTreeSet::from([1]));
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

#[cfg(test)]
mod keep_known_tests {
    use super::super::store::{FileOf, Missing};
    use super::*;

    #[test]
    fn a_drive_gone_after_a_check_failed_keeps_what_was_missing() {
        let destination = AbsolutePath::new("/away").unwrap();
        let mut record = Record::new("00".repeat(20), 1, Some("t".into()), destination, Vec::new(), Wants::Run);
        let file = |name: &str, pieces| FileOf { path: name.into(), len: 10, padding: false, pieces };
        record.info = Some(Info {
            bytes: Arc::from(Vec::new()),
            files: vec![file("a.bin", 0..1), file("b.bin", 1..2)],
            piece_length: 10,
            name: Some("t".into()),
        });
        record.missing = [(0, Missing { had: 10, seen: files::Seen::Was(None) })].into();
        record.had = vec![10, 10];
        let returning = record.gone_at_start();
        let mut torrent = Torrent {
            id: 1,
            lifecycle: State::new(Wants::Run, Phase::Checking { returning }),
            meter: seeding::Meter::new(record.round),
            polled: Polled::default(),
            lib: lib::Lib::Out,
            there: BTreeSet::new(),
            record,
            added: 0,
            started: 0,
            chosen_running: 0,
        };
        let reason = proto::Unavailable::FolderMissing { path: AbsolutePath::new("/away").unwrap() };
        let failed = Event::Observed(Observation::Failed(Broke::Librqbit { detail: "Input/output error (os error 5)".into() }));
        for event in [failed, Event::Observed(Observation::DestinationGone { reason })] {
            torrent.lifecycle = decide(&torrent.lifecycle, &event).unwrap();
            torrent.keep_known();
        }
        assert!(torrent.record.missing.contains_key(&0), "the record forgot a.bin was missing: {:?}", torrent.record.missing);
        let paths = torrent.record.paths();
        let found = |i: usize, inode| {
            let file = Expected { index: i, path: paths[i].clone(), size_bytes: 10, progress_bytes: 10 };
            (i, files::Found::of(file, Some(files::Stamp { inode, len: 10, modified: None })))
        };
        let disk = files::Disk { files: [found(0, 7), found(1, 8)].into() };
        let back = decide(&torrent.lifecycle, &Event::Observed(Observation::Files(disk))).unwrap();
        assert_ne!(back.phase, Phase::CheckDue { returning: BTreeMap::new() }, "checked in full: a.bin is taken for the torrent's");
    }
}

#[cfg(test)]
mod runtime_end_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::super::{Config, Engine, POLL_EVERY, trash};
    use super::{Actor, Message};

    /// #442: as its runtime ends, tokio cancels what's spawned on its blocking pool from then on:
    /// here a put-back, spawned by the actor still running then, as when an undo a crash cut short
    /// resumes at a start and the service's `main` fails and returns. The actor ends with the
    /// runtime, as at a crash, without a panic.
    #[test]
    fn a_put_back_the_ending_runtime_cancels_ends_the_actor_without_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let engine = rt
            .block_on(Engine::start(Config {
                data_dir: dir.path().join("data"),
                download_dir: dir.path().join("downloads"),
                listen_port: 0,
                upnp: false,
                offline: true,
                size_check_every: POLL_EVERY,
                poll_every: POLL_EVERY,
                trash: trash::into(dir.path().join("trash"), Default::default(), Default::default()),
                speed_limits: Default::default(),
                seeding: Default::default(),
            }))
            .unwrap();
        let (inside, entered) = std::sync::mpsc::channel();
        let put_back_as_it_ends = move |actor: &mut Actor| {
            inside.send(()).unwrap();
            // Till the runtime's blocking pool is shut down: a probe spawned from then on never runs.
            loop {
                let ran = Arc::new(AtomicBool::new(false));
                let probe = tokio::task::spawn_blocking({
                    let ran = ran.clone();
                    move || ran.store(true, Ordering::SeqCst)
                });
                while !probe.is_finished() {
                    std::hint::spin_loop();
                }
                if !ran.load(Ordering::SeqCst) {
                    break;
                }
            }
            actor.putting_back.spawn_blocking(|| unreachable!("cancelled before it starts"));
        };
        engine.actor.try_send(Message::Inspect(Box::new(put_back_as_it_ends))).unwrap();
        entered.recv().unwrap();
        drop(rt);
        let ended = tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(engine.ran);
        assert!(!ended.as_ref().is_err_and(|e| e.is_panic()), "the actor panicked: {ended:?}");
    }
}
