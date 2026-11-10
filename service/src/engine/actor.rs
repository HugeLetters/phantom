//! The actor: one task that owns every torrent, its lifecycle ([`State`]) and its librqbit side
//! ([`InLib`]), and librqbit's session, and handles one [`Message`] at a time: client commands
//! (each answered through its own reply channel), the end of an add, a regular poll of librqbit,
//! and a size check of the files on disk (every few seconds, at once when the watcher sees a path
//! go: `files.rs`, and before a command, so its answer doesn't depend on how soon the watcher
//! noticed). Each is an [`Event`] for [`decide`]; after it, [`machine::next`] says what makes
//! librqbit match and [`inlib::next`] how that's done on its librqbit side. The actor reports
//! facts (what librqbit is doing, what's on disk, how an add ended) and the machines (`machine/`)
//! decide what they mean. Slow work (fetching info, downloading a .torrent URL) runs in its own
//! task and comes back as a message.
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

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentStats, TorrentStatsState};
use proto::{AbsolutePath, Added, Details, File, Priority, TorrentCommand, TorrentId, TorrentInfo, Unavailable};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::warn;

use super::bitv;
use super::files::{self, Expected, Gone, Guard, Seen, Watcher};
use super::machine::inlib;
use super::machine::{self, Event, Lib, Observation, Phase, Refusal, State, Wants, decide};
use super::snapshot::Snapshot;
use super::store::Store;
use super::{Failure, Prepared};

/// How often the actor polls librqbit, between commands.
const POLL_EVERY: Duration = Duration::from_millis(500);

/// How an add reaches librqbit: [`add_into`], or in tests one whose waits the test controls.
pub(super) type AddInto = Arc<dyn Fn(Arc<Session>, TorrentId, AddTorrent<'static>, PathBuf, PathBuf) -> Adding + Send + Sync>;
pub(super) type Adding = Pin<Box<dyn Future<Output = Result<AddTorrentResponse>> + Send>>;

pub(super) enum Message {
    Add {
        prepared: Prepared,
        paused: bool,
        reply: oneshot::Sender<Result<Added>>,
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
    /// Record what's missing, pause what runs, stop the session, answer and end.
    Stop(oneshot::Sender<Vec<(String, u64)>>),
    /// Runs the closure on the actor, for tests to look inside.
    #[cfg(test)]
    Inspect(Box<dyn FnOnce(&mut Actor) + Send>),
}

/// What the actor works with, besides what's kept across restarts.
pub(super) struct Setup {
    pub session: Arc<Session>,
    pub data_dir: PathBuf,
    /// librqbit's own folder, from which it names a torrent's own folder.
    pub download_dir: PathBuf,
    pub guard: Guard,
    pub size_check_every: Duration,
    /// For add tasks and the watcher to report back.
    pub inbox: mpsc::WeakSender<Message>,
    pub add_into: AddInto,
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
    pub(super) published: watch::Sender<Snapshot>,
    /// For add tasks and the watcher to report back.
    inbox: mpsc::WeakSender<Message>,
    /// `None` if it couldn't start: then only the regular size check finds missing ones.
    watcher: Option<Watcher>,
    size_check_every: Duration,
    /// Regular size checks of every torrent run so far, for tests to wait on one.
    pub(super) size_checks: u64,
    add_into: AddInto,
    /// The add of each torrent [`inlib::State::Adding`], which Remove stops and waits for.
    pub(super) adds: BTreeMap<TorrentId, JoinHandle<()>>,
}

/// A torrent's librqbit side, with librqbit's own torrent.
pub(super) type InLib = inlib::State<Arc<ManagedTorrent>>;

pub(super) struct Torrent {
    pub(super) lifecycle: State,
    /// Its librqbit side.
    pub(super) inlib: InLib,
    info_hash: String,
    /// What librqbit said of it at the last poll, which clients see until the next.
    polled: Polled,
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
        let Setup { session, data_dir, download_dir, guard, size_check_every, inbox, add_into } = setup;
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
            let phase = match unavailable.remove(&hash) {
                Some(reason) => Phase::DestinationUnavailable { reason },
                None if gone.is_empty() => restored_phase(&handle.stats()),
                None => Phase::MissingFiles { gone },
            };
            let torrent = Torrent {
                lifecycle: State::new(saved.wants, phase),
                info_hash: hash,
                polled: Polled::of(&handle.stats()),
                inlib: inlib::State::Has(handle),
            };
            torrents.insert(id, torrent);
        }
        let watcher_inbox = inbox.clone();
        let watcher = Watcher::start(move |paths| {
            // A full inbox drops the news; the regular size check catches it.
            if let Some(inbox) = watcher_inbox.upgrade() {
                let _ = inbox.try_send(Message::FilesChanged(paths));
            }
        });
        let watcher = watcher.inspect_err(|e| warn!(error = %e, "failed to start the file watcher")).ok();
        let published = watch::Sender::new(Snapshot::default());
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
            size_checks: 0,
            add_into,
            adds: BTreeMap::new(),
        };
        actor.save();
        actor.publish();
        actor
    }

    pub(super) async fn run(mut self, mut inbox: mpsc::Receiver<Message>) {
        // Those wanted running start.
        self.poll_librqbit().await;
        self.publish();
        let mut poll = tokio::time::interval(POLL_EVERY);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut size_checks = tokio::time::interval(self.size_check_every);
        size_checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                message = inbox.recv() => match message {
                    Some(Message::Stop(reply)) => {
                        let held = self.stop().await;
                        // Its torrents and session go before the answer: see `Engine::stop`.
                        drop(self);
                        let _ = reply.send(held);
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
                _ = poll.tick() => self.poll_librqbit().await,
                _ = size_checks.tick() => {
                    self.watch_destinations();
                    self.size_check_all().await;
                }
            }
            self.publish();
        }
    }

    /// The snapshots this actor publishes, from the last one.
    pub(super) fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.published.subscribe()
    }

    /// Publishes the snapshot the state makes now, if it isn't the last one.
    fn publish(&self) {
        let next = self.snapshot();
        self.published.send_if_modified(|last| {
            let changed = *last != next;
            if changed {
                *last = next;
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
        Snapshot { torrents: self.listing(), details }
    }

    /// The torrents whose details the snapshot carries: every one, as no client says which it
    /// shows yet. Those without info have none.
    fn watched(&self) -> impl Iterator<Item = TorrentId> + '_ {
        self.torrents.keys().copied()
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::Add { prepared, paused, reply } => self.add(prepared, paused, reply),
            Message::Command { id, command, reply } => {
                let answer = self.command(id, command).await;
                self.answer(reply, answer);
            }
            Message::AddFinished { id, added } => self.add_finished(id, added).await,
            Message::FilesChanged(paths) => self.files_changed(&paths).await,
            Message::Stop(_) => unreachable!("handled by run"),
            #[cfg(test)]
            Message::Inspect(_) => unreachable!("handled by run"),
        }
    }

    /// Stops everything; returns the pieces each torrent (by info hash) has, for `Engine::stop`.
    async fn stop(&mut self) -> Vec<(String, u64)> {
        let mut running = Vec::new();
        for torrent in self.torrents.values_mut() {
            if let Some(saved) = self.store.get_mut(&torrent.info_hash) {
                saved.missing_at_shutdown = match &torrent.lifecycle.phase {
                    Phase::MissingFiles { gone } => gone.iter().map(|(&i, gone)| (i, gone.progress_bytes)).collect(),
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
        let held = self
            .torrents
            .values()
            .filter_map(|t| {
                let handle = t.inlib.info()?;
                let piece_length = handle.with_metadata(|m| m.info.lengths().default_piece_length()).ok()?;
                Some((t.info_hash.clone(), bitv::pieces_in(handle.stats().progress_bytes, piece_length)))
            })
            .collect();
        self.session.stop().await;
        held
    }

    /// Decides `event` for torrent `id`, then makes librqbit match; once its phase changes, its files
    /// are size-checked, and that's decided too. A refused command fails with the reason.
    async fn event(&mut self, id: TorrentId, mut event: Event) -> Result<(), Failure> {
        loop {
            let torrent = self.torrents.get_mut(&id).ok_or(Failure::torrent(id, Refusal::NotInList))?;
            let before = torrent.lifecycle.phase.clone();
            let next = decide(&torrent.lifecycle, &event).map_err(|refusal| Failure::torrent(id, refusal))?;
            let wants_changed = next.wants != torrent.lifecycle.wants;
            torrent.lifecycle = next;
            if wants_changed && let Some(saved) = self.store.get_mut(&torrent.info_hash) {
                saved.wants = torrent.lifecycle.wants;
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
        match inlib::next(&torrent.inlib, wanted) {
            None => Ok(()),
            Some(inlib::Do::Start(handle)) => Ok(self.session.unpause(&handle).await?),
            Some(inlib::Do::Stop(handle)) => Ok(self.session.pause(&handle).await?),
            Some(inlib::Do::AddAgain { from, take_out }) => self.add_again(id, from, take_out).await,
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
        let only_files = match self.held(&handle) {
            Some(held) => Some(held.iter().copied().collect()),
            None => handle.only_files(),
        };
        let output_folder = handle.output_folder().to_str().context("the destination isn't valid UTF-8")?.to_owned();
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
                let message = format!("failed to add it again: {e:#}");
                let failed = Event::Observed(Observation::Failed(message.clone()));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::TakenOut);
                return Err(anyhow!("torrent {id} {message}").into());
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
    async fn size_check(&mut self, id: TorrentId) {
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
        if matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
            return None;
        }
        Some(Observation::Files(files::disk_of(&expected(handle, self.held(handle)))))
    }

    async fn command(&mut self, id: TorrentId, command: TorrentCommand) -> Result<(), Failure> {
        // Decided on what librqbit is doing now and on its files as they are now, not at the last
        // poll or once the watcher notices.
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(command.action())).await?;
        match command {
            TorrentCommand::Remove => self.take_off_list(id, false).await?,
            TorrentCommand::Delete => self.take_off_list(id, true).await?,
            TorrentCommand::SetPriority { files, priority } => {
                self.set_priority(id, &files, priority).await?;
                // Files set Off aren't missing any more, and those set Normal may be.
                self.size_check(id).await;
            }
            TorrentCommand::Pause | TorrentCommand::Resume | TorrentCommand::Check => {}
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
    async fn poll_torrent(&mut self, id: TorrentId) {
        // Taken out, or not added: nothing to follow until librqbit has it.
        let Some(inlib::State::Has(handle)) = self.torrents.get(&id).map(|t| t.inlib.clone()) else { return };
        let stats = handle.stats();
        let torrent = self.torrents.get_mut(&id).expect("just found");
        torrent.polled = Polled::of(&stats);
        if let Err(e) = self.event(id, Event::Observed(librqbit_seen(&stats))).await {
            warn!(id, error = ?e, "failed to follow librqbit");
        }
        if !matches!(stats.state, TorrentStatsState::Initializing { .. }) {
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
    fn add(&mut self, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added>>) {
        let Prepared { add, info_hash, name, destination } = prepared;
        if let Some(id) = self.store.id(&info_hash) {
            let name = self.torrents.get(&id).and_then(|t| t.name());
            self.answer(reply, Ok(Added { id, name, already_existed: true }));
            return;
        }
        let id = self.store.next_id();
        let wants = if paused { Wants::Stop } else { Wants::Run };
        self.store.insert(info_hash.clone(), id, wants, Some(destination.clone()));
        self.save();
        self.guard.set_destination(&info_hash, Some(destination.clone()));
        // Its own task: fetching info takes as long as it takes, and Remove can stop it.
        let adding = (self.add_into)(self.session.clone(), id, add, destination.into_path_buf(), self.download_dir.clone());
        let inbox = self.inbox.clone();
        let task = tokio::spawn(async move {
            let added = adding.await;
            if let Some(inbox) = inbox.upgrade() {
                let _ = inbox.send(Message::AddFinished { id, added }).await;
            }
        });
        self.adds.insert(id, task);
        let added = Added { id, name: name.clone(), already_existed: false };
        let torrent = Torrent {
            lifecycle: State::new(wants, Phase::FetchingInfo),
            inlib: inlib::State::Adding { name },
            info_hash,
            polled: Polled::default(),
        };
        self.torrents.insert(id, torrent);
        self.answer(reply, Ok(added));
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

    /// Takes the torrent off the list and out of librqbit. One fetching info stops first: an add
    /// cut short may have left it in librqbit too.
    async fn take_off_list(&mut self, id: TorrentId, delete_files: bool) -> Result<()> {
        let torrent = self.torrents.remove(&id).expect("decided on it");
        let hash = super::id20(&torrent.info_hash)?;
        self.store.forget(&torrent.info_hash);
        self.save();
        if let Some(task) = self.adds.remove(&id) {
            task.abort();
            let _ = task.await;
        }
        let taken_out = match self.session.get(TorrentIdOrHash::Hash(hash)) {
            Some(t) => self.session.delete(TorrentIdOrHash::Id(t.id()), delete_files).await,
            None => Ok(()),
        };
        // Only now: until then, librqbit could still be adding it.
        self.guard.set_destination(&torrent.info_hash, None);
        taken_out
    }

    /// librqbit's torrent, for one with info. Its lifecycle refuses the commands that need one
    /// before: one without is an internal error.
    fn handle_of(&self, id: TorrentId) -> Result<&Arc<ManagedTorrent>> {
        self.torrents.get(&id).and_then(|t| t.inlib.info()).with_context(|| format!("torrent {id} has no info"))
    }

    fn held(&self, handle: &ManagedTorrent) -> Option<&BTreeSet<usize>> {
        self.store.get(&handle.info_hash().as_string())?.held.as_ref()
    }

    fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        let handle = self.handle_of(id)?;
        let torrent = &self.torrents[&id];
        let files = files(handle, &torrent.polled.file_progress, self.held(handle), &missing(&torrent.lifecycle.phase))?;
        let location = AbsolutePath::new(location(handle)?).map_err(|p| anyhow!("torrent {id} is at a relative path: {}", p.display()))?;
        Ok(Details { id, files, location })
    }

    /// Files set Off aren't downloaded and drop out of the torrent's Size, down to none at all:
    /// it's then finished. librqbit saves the choice with the torrent. While librqbit is
    /// checking the torrent the change is held in the store, shows at once, and is applied
    /// when the check is done.
    async fn set_priority(&mut self, id: TorrentId, indices: &[usize], priority: Priority) -> Result<(), Failure> {
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
        if !checking && held.is_none() {
            return Ok(self.session.update_only_files(&handle, &normal.into_iter().collect()).await?);
        }
        self.store.get_mut(&handle.info_hash().as_string()).context("torrent without an id")?.held = Some(normal);
        self.save();
        Ok(())
    }

    /// Every torrent, including those fetching info, by id.
    fn listing(&self) -> Vec<TorrentInfo> {
        self.torrents
            .iter()
            .map(|(&id, t)| match t.inlib.info() {
                Some(handle) => torrent_info(id, t, handle, self.held(handle)),
                None => fetching_info(id, t),
            })
            .collect()
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
            inlib::State::Has(h) | inlib::State::TakenOut(h) => h.name(),
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
) -> Result<AddTorrentResponse> {
    let listed = session.add_torrent(add, Some(AddTorrentOptions { list_only: true, paused: true, ..Default::default() })).await?;
    let AddTorrentResponse::ListOnly(listed) = listed else { bail!("unexpected response to a list-only add") };
    let own_folder = listed.output_folder.strip_prefix(&download_dir).context("librqbit put the torrent elsewhere")?;
    let output_folder = destination.join(own_folder);
    let output_folder = output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned();
    let opts = AddTorrentOptions {
        paused: true,
        overwrite: true,
        output_folder: Some(output_folder),
        initial_peers: Some(listed.seen_peers),
        preferred_id: Some(id.try_into()?),
        ..Default::default()
    };
    session.add_torrent(AddTorrent::from_bytes(listed.torrent_bytes), Some(opts)).await
}

/// What librqbit is doing with a torrent it has.
fn librqbit_seen(stats: &TorrentStats) -> Observation {
    match stats.state {
        TorrentStatsState::Initializing { .. } => Observation::Checking,
        TorrentStatsState::Live | TorrentStatsState::Paused => Observation::Idle { have: stats.file_progress.clone() },
        TorrentStatsState::Error => Observation::Failed(stats.error.clone().unwrap_or_else(|| "unknown error".into())),
    }
}

/// The phase of a torrent librqbit restored, before anything happened to it.
fn restored_phase(stats: &TorrentStats) -> Phase {
    match &stats.state {
        TorrentStatsState::Initializing { .. } => Phase::Checking { returning: BTreeMap::new() },
        TorrentStatsState::Live | TorrentStatsState::Paused => Phase::Ready,
        TorrentStatsState::Error => Phase::Error { message: stats.error.clone().unwrap_or_default() },
    }
}

/// The files missing, by index: those of a torrent that's Missing files.
fn missing(phase: &Phase) -> BTreeSet<usize> {
    match phase {
        Phase::MissingFiles { gone } => gone.keys().copied().collect(),
        _ => BTreeSet::new(),
    }
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

fn fetching_info(id: TorrentId, t: &Torrent) -> TorrentInfo {
    TorrentInfo {
        id,
        info_hash: t.info_hash.clone(),
        name: t.name(),
        status: t.lifecycle.status(false),
        progress_bytes: 0,
        size_bytes: 0,
        full_size_bytes: 0,
        uploaded_bytes: 0,
        download_bytes_per_sec: 0,
        upload_bytes_per_sec: 0,
        live_peers: 0,
        actions: t.lifecycle.actions(),
    }
}

/// The torrent's files, without BEP 47 padding (which only aligns the others and is never written).
/// `progress` is librqbit's bytes per file, empty while checking.
/// `held` is the priorities held until a check ends, if any, which take the place of librqbit's.
/// `missing` files are gone from disk, so none of them is downloaded, whatever librqbit thinks.
fn files(torrent: &ManagedTorrent, progress: &[u64], held: Option<&BTreeSet<usize>>, missing: &BTreeSet<usize>) -> Result<Vec<File>> {
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

fn torrent_info(id: TorrentId, t: &Torrent, torrent: &ManagedTorrent, held: Option<&BTreeSet<usize>>) -> TorrentInfo {
    let stats = &t.polled;
    // Size and progress from the files you're getting: librqbit counts whole pieces, which
    // spill into neighbouring files that may be Off.
    let files = files(torrent, &stats.file_progress, held, &missing(&t.lifecycle.phase)).unwrap_or_default();
    let normal = || files.iter().filter(|f| f.priority == Priority::Normal);
    let size_bytes = normal().map(|f| f.size_bytes).sum();
    let progress_bytes = match stats.file_progress.is_empty() {
        // Checking: how much has been checked so far.
        true => stats.progress_bytes.min(size_bytes),
        false => normal().map(|f| f.progress_bytes).sum(),
    };
    TorrentInfo {
        id,
        info_hash: torrent.info_hash().as_string(),
        name: torrent.name(),
        status: t.lifecycle.status(stats.finished),
        progress_bytes,
        size_bytes,
        full_size_bytes: files.iter().map(|f| f.size_bytes).sum(),
        uploaded_bytes: stats.uploaded_bytes,
        download_bytes_per_sec: stats.download_bytes_per_sec,
        upload_bytes_per_sec: stats.upload_bytes_per_sec,
        live_peers: stats.live_peers,
        actions: t.lifecycle.actions(),
    }
}

/// What clients see of librqbit's stats of a torrent.
#[derive(Default)]
struct Polled {
    finished: bool,
    progress_bytes: u64,
    /// By file index; empty while checking.
    file_progress: Vec<u64>,
    uploaded_bytes: u64,
    download_bytes_per_sec: u64,
    upload_bytes_per_sec: u64,
    live_peers: u32,
}

impl Polled {
    fn of(stats: &TorrentStats) -> Self {
        let live = stats.live.as_ref();
        Self {
            finished: stats.finished,
            progress_bytes: stats.progress_bytes,
            file_progress: stats.file_progress.clone(),
            uploaded_bytes: stats.uploaded_bytes,
            download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
            upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
            live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
        }
    }
}
