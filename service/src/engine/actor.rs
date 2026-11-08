//! The actor: one task that owns every torrent's [`Lifecycle`] and librqbit's session, and
//! handles one [`Message`] at a time: client commands (each answered through its own reply
//! channel), the end of an add, a regular poll of librqbit, and a size check of the files
//! on disk (every few seconds, and at once when the watcher sees a path go: `files.rs`). Each is an
//! [`Event`] for [`decide`]; after it, [`reconcile`] says what makes librqbit match. Slow work
//! (fetching info, downloading a .torrent URL) runs in its own task and comes back as a message.
//!
//! librqbit only runs a torrent when the actor starts it: every add, and every add again, is
//! stopped, and a torrent wanted running is started once it's Ready.
//!
//! Missing files: a torrent with a file it's getting gone from disk stops in librqbit (so nothing
//! more is written into the deleted file) and is Missing files, with the files that are missing in
//! its phase. Setting them Off makes it Ready; Resume and Check check it (take it out of librqbit
//! and add it again, which opens the files afresh). Once every missing file is back on disk it's
//! checked too; a file put back that holds less than it did before it went makes it Missing files
//! again (until that file changes once more).
//!
//! Destination unavailable: before its files, a torrent's destination is size-checked (every few
//! seconds and when the watcher sees a path go). One that can't be reached stops the torrent in
//! librqbit until it's back; then it's checked. librqbit creates a torrent's files whenever it
//! adds one: [`Guard`] gives the torrents whose destination is unavailable storage that touches nothing,
//! so restoring them at start makes no folders where a drive isn't.
//!
//! What's wanted (run or stop) is saved in the store whenever it changes, so a start, after a stop
//! or a crash, makes librqbit match it. At stop, every torrent's missing files and the files it's
//! getting are recorded too, and running ones are paused so librqbit saves them paused. At start,
//! torrents whose destination can't be reached are Destination unavailable; the others' recorded
//! files are checked before librqbit restores its torrents (restoring creates missing files again,
//! empty), and torrents missing some are Missing files.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentStats, TorrentStatsState};
use proto::{AbsolutePath, Added, Details, File, Priority, TorrentId, TorrentInfo, Unavailable};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::warn;

use super::{Prepared, Refused};
use super::files::{self, Expected, Gone, Guard, Seen, Watcher};
use super::lifecycle::{self, Do, Event, Lib, Lifecycle, Observation, Phase, Refusal, Wants, decide, reconcile};
use super::store::Store;

/// How often the actor polls librqbit, between commands.
const POLL_EVERY: Duration = Duration::from_millis(500);

pub(super) enum Message {
    List(oneshot::Sender<Vec<TorrentInfo>>),
    Add { prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added>> },
    Command { id: TorrentId, command: Command, reply: oneshot::Sender<Result<Answer>> },
    /// librqbit's add of torrent `id` ended.
    AddFinished { id: TorrentId, added: Result<AddTorrentResponse> },
    /// The watcher saw these paths removed or renamed.
    FilesChanged(Vec<PathBuf>),
    /// Record what's missing, pause what runs, stop the session, answer and end.
    Stop(oneshot::Sender<()>),
    /// Runs the closure on the actor, for tests to look inside.
    #[cfg(test)]
    Inspect(Box<dyn FnOnce(&mut Actor) + Send>),
}

pub(super) enum Command {
    Pause,
    Resume,
    Remove,
    Delete,
    Details,
    SetPriority { files: Vec<usize>, priority: Priority },
    Check,
}

pub(super) enum Answer {
    Done,
    Details(Details),
}

impl Command {
    /// The lifecycle's command; Details only reads.
    fn lifecycle(&self) -> Option<lifecycle::Command> {
        Some(match self {
            Command::Pause => lifecycle::Command::Pause,
            Command::Resume => lifecycle::Command::Resume,
            Command::Remove => lifecycle::Command::Remove,
            Command::Delete => lifecycle::Command::Delete,
            Command::Details => return None,
            Command::SetPriority { .. } => lifecycle::Command::SetPriority,
            Command::Check => lifecycle::Command::Check,
        })
    }
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
}

pub(super) struct Actor {
    pub(super) session: Arc<Session>,
    data_dir: PathBuf,
    pub(super) download_dir: PathBuf,
    /// Which torrents librqbit may create files for: not those whose destination is unavailable.
    guard: Guard,
    pub(super) store: Store,
    pub(super) torrents: BTreeMap<TorrentId, Torrent>,
    /// For add tasks and the watcher to report back.
    inbox: mpsc::WeakSender<Message>,
    /// `None` if it couldn't start: then only the regular size check finds missing ones.
    watcher: Option<Watcher>,
    size_check_every: Duration,
}

pub(super) struct Torrent {
    pub(super) lifecycle: Lifecycle,
    /// Unknown only while a .torrent URL is downloading.
    info_hash: Option<String>,
    /// While fetching info: from the magnet's `dn=` or the URL's file name, if there is one.
    name: Option<String>,
    /// librqbit's torrent, once its info arrived.
    pub(super) handle: Option<Arc<ManagedTorrent>>,
    /// Adds the torrent to librqbit, while fetching info.
    add_task: Option<JoinHandle<()>>,
    /// The client waiting for the add's answer, if it wasn't answered at once.
    waiting: Option<oneshot::Sender<Result<Added>>>,
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
        let Setup { session, data_dir, download_dir, guard, size_check_every, inbox } = setup;
        let restored: Vec<Arc<ManagedTorrent>> = session.with_torrents(|ts| ts.map(|(_, t)| t.clone()).collect());
        let hashes: HashSet<String> = restored.iter().map(|t| t.info_hash().as_string()).collect();
        store.retain(|hash| hashes.contains(hash));
        let mut torrents = BTreeMap::new();
        for handle in restored {
            let hash = handle.info_hash().as_string();
            let id = store.id_or_assign(&hash);
            let saved = store.get_mut(&hash).expect("just assigned");
            saved.destination = destination(&handle);
            let gone = gone_at_start(&handle, saved.held.as_ref(), missing.remove(&hash));
            let phase = match unavailable.remove(&hash) {
                Some(reason) => Phase::DestinationUnavailable { reason },
                None if gone.is_empty() => restored_phase(&handle.stats()),
                None => Phase::MissingFiles { gone },
            };
            let torrent = Torrent {
                lifecycle: Lifecycle::new(saved.wants, phase),
                info_hash: Some(hash),
                name: None,
                handle: Some(handle),
                add_task: None,
                waiting: None,
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
        let actor = Self { session, data_dir, download_dir, guard, store, torrents, inbox, watcher, size_check_every };
        actor.save();
        actor
    }

    pub(super) async fn run(mut self, mut inbox: mpsc::Receiver<Message>) {
        // Those wanted running start.
        self.poll_librqbit().await;
        let mut poll = tokio::time::interval(POLL_EVERY);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut size_checks = tokio::time::interval(self.size_check_every);
        size_checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                message = inbox.recv() => match message {
                    Some(Message::Stop(reply)) => {
                        self.stop().await;
                        let _ = reply.send(());
                        return;
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
        }
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::List(reply) => {
                self.poll_librqbit().await;
                let _ = reply.send(self.listing());
            }
            Message::Add { prepared, paused, reply } => self.add(prepared, paused, reply),
            Message::Command { id, command, reply } => {
                let answer = self.command(id, command).await;
                let _ = reply.send(answer);
            }
            Message::AddFinished { id, added } => self.add_finished(id, added).await,
            Message::FilesChanged(paths) => self.files_changed(&paths).await,
            Message::Stop(_) => unreachable!("handled by run"),
            #[cfg(test)]
            Message::Inspect(f) => f(self),
        }
    }

    async fn stop(&mut self) {
        let mut running = Vec::new();
        for torrent in self.torrents.values_mut() {
            let Some(hash) = &torrent.info_hash else { continue };
            if let Some(saved) = self.store.get_mut(hash) {
                saved.missing_at_shutdown = match &torrent.lifecycle.phase {
                    Phase::MissingFiles { gone } => gone.iter().map(|(&i, gone)| (i, gone.progress_bytes)).collect(),
                    _ => BTreeMap::new(),
                };
                saved.files_at_shutdown = torrent.handle.as_ref().map(|h| expected(h, saved.held.as_ref())).unwrap_or_default();
            }
            if let Some(handle) = &torrent.handle
                && matches!(handle.stats().state, TorrentStatsState::Live)
            {
                running.push(handle.clone());
            }
            if let Some(task) = torrent.add_task.take() {
                task.abort();
            }
            if let Some(waiting) = torrent.waiting.take() {
                let _ = waiting.send(Err(anyhow!("the service is stopping")));
            }
        }
        self.save();
        for handle in running {
            if let Err(e) = self.session.pause(&handle).await {
                warn!(error = %e, "failed to pause a torrent while stopping");
            }
        }
        self.session.stop().await;
    }

    /// Decides `event` for torrent `id`, then makes librqbit match. A refused command is an error
    /// with the reason.
    async fn event(&mut self, id: TorrentId, event: Event) -> Result<()> {
        let torrent = self.torrents.get_mut(&id).with_context(|| format!("no torrent with id {id}"))?;
        let next = decide(&torrent.lifecycle, &event).map_err(|refusal| refused(id, refusal))?;
        let wants_changed = next.wants != torrent.lifecycle.wants;
        torrent.lifecycle = next;
        if let Some(hash) = &torrent.info_hash {
            self.guard.set_unavailable(super::id20(hash)?, matches!(torrent.lifecycle.phase, Phase::DestinationUnavailable { .. }));
            if wants_changed && let Some(saved) = self.store.get_mut(hash) {
                saved.wants = torrent.lifecycle.wants;
                self.save();
            }
        }
        self.reconcile(id).await
    }

    /// Makes librqbit match the torrent.
    async fn reconcile(&mut self, id: TorrentId) -> Result<()> {
        let Some(torrent) = self.torrents.get(&id) else { return Ok(()) };
        let Some(handle) = torrent.handle.clone() else { return Ok(()) };
        let lib = match handle.stats().state {
            _ if !self.in_librqbit(&handle) => Lib::Absent,
            TorrentStatsState::Initializing { .. } => Lib::Checking,
            TorrentStatsState::Live => Lib::Running,
            TorrentStatsState::Paused => Lib::Stopped,
            TorrentStatsState::Error => Lib::Failed,
        };
        match reconcile(&torrent.lifecycle, lib) {
            None => Ok(()),
            Some(Do::Start) => self.session.unpause(&handle).await,
            Some(Do::Stop) => self.session.pause(&handle).await,
            Some(Do::AddAgain) => self.add_again(id).await,
        }
    }

    /// Takes the torrent out of librqbit and adds it again, stopped, at the same place with the same
    /// priorities, so librqbit opens its files afresh and checks them, keeping what's valid:
    /// librqbit can't check a torrent it has, and forgets the pieces it had along with the
    /// torrent. On failure after taking it out, it's an Error (librqbit no longer has it) until
    /// Resume or Check adds it again.
    async fn add_again(&mut self, id: TorrentId) -> Result<()> {
        let handle = self.handle_of(id)?.clone();
        let bytes = handle.with_metadata(|m| m.torrent_bytes.clone())?;
        // Priorities held during a check are applied by the add.
        let only_files = match self.held(&handle) {
            Some(held) => Some(held.iter().copied().collect()),
            None => handle.only_files(),
        };
        let output_folder = handle.output_folder().to_str().context("the destination isn't valid UTF-8")?.to_owned();
        if self.in_librqbit(&handle) {
            self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await?;
        }
        let opts = AddTorrentOptions { paused: true, overwrite: true, only_files, output_folder: Some(output_folder), ..Default::default() };
        let added = match self.session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => Ok(t),
            Ok(AddTorrentResponse::ListOnly(_)) => Err(anyhow!("unexpected list-only response")),
            Err(e) => Err(e),
        };
        let torrent = self.torrents.get_mut(&id).expect("has a handle");
        match added {
            Ok(added) => torrent.handle = Some(added),
            Err(e) => {
                let message = format!("failed to add it again: {e:#}");
                let failed = Event::Observation(Observation::Failed(message.clone()));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                bail!("torrent {id} {message}");
            }
        }
        if let Some(saved) = self.store.get_mut(&handle.info_hash().as_string()) {
            saved.held = None;
        }
        self.save();
        Ok(())
    }

    /// Whether librqbit still has this handle's torrent: not after a failed add again.
    fn in_librqbit(&self, handle: &Arc<ManagedTorrent>) -> bool {
        self.session.get(TorrentIdOrHash::Id(handle.id())).is_some_and(|t| Arc::ptr_eq(&t, handle))
    }

    /// Watches the destinations of the torrents with info, for files going missing.
    pub(super) fn watch_destinations(&mut self) {
        let Some(watcher) = &mut self.watcher else { return };
        let folders = self.torrents.values().filter_map(|t| t.handle.as_deref().and_then(destination)).map(AbsolutePath::into_path_buf).collect();
        watcher.watch(folders);
    }

    async fn size_check_all(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.size_check(id).await;
        }
    }

    /// Checks the files of the torrents whose files `changed` can touch.
    async fn files_changed(&mut self, changed: &[PathBuf]) {
        let ids: Vec<TorrentId> = self
            .torrents
            .iter()
            .filter(|(_, t)| {
                let location = t.handle.as_deref().and_then(|h| location(h).ok());
                location.is_some_and(|l| changed.iter().any(|c| files::touches(c, &l)))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.size_check(id).await;
        }
    }

    /// Size-checks the torrent: its destination, then its files on disk: with the destination unavailable it's
    /// Destination unavailable, and checked once the destination is back; with any file it's
    /// getting missing, it's Missing files; one that's Missing files with every missing file back
    /// is checked.
    async fn size_check(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(handle) = torrent.handle.clone() else { return };
        let unavailable = destination(&handle).and_then(|d| files::unavailable(&d));
        let expected = expected(&handle, self.held(&handle));
        let seen = match (unavailable, &torrent.lifecycle.phase) {
            (Some(reason), _) => Observation::DestinationGone { reason },
            (None, Phase::DestinationUnavailable { .. }) => Observation::DestinationBack,
            (None, Phase::MissingFiles { gone }) => match size_check_returned(&handle, gone, &expected) {
                Some(seen) => seen,
                None => return,
            },
            (None, _) => {
                let gone: BTreeMap<usize, Gone> =
                    expected.iter().filter(|f| files::is_missing(f)).map(|f| (f.index, Gone::now(f))).collect();
                if gone.is_empty() {
                    return;
                }
                Observation::FilesMissing { gone }
            }
        };
        if let Err(e) = self.event(id, Event::Observation(seen)).await {
            warn!(id, error = %e, "failed to follow the torrent's files");
        }
    }

    /// After a priority change: missing files set Off are no longer missing.
    async fn clear_missing_set_off(&mut self, id: TorrentId, indices: &[usize], priority: Priority) {
        let Some(Phase::MissingFiles { gone }) = self.torrents.get(&id).map(|t| &t.lifecycle.phase) else { return };
        if priority != Priority::Off {
            return;
        }
        let gone: BTreeMap<usize, Gone> = gone.iter().filter(|(i, _)| !indices.contains(i)).map(|(i, g)| (*i, g.clone())).collect();
        let seen = if gone.is_empty() { Observation::NoFilesMissing } else { Observation::FilesMissing { gone } };
        if let Err(e) = self.event(id, Event::Observation(seen)).await {
            warn!(id, error = %e, "failed to clear missing files set Off");
        }
    }

    async fn command(&mut self, id: TorrentId, command: Command) -> Result<Answer> {
        // Decided on what librqbit is doing now, not at the last poll.
        self.poll_torrent(id).await;
        let Some(decided) = command.lifecycle() else { return self.details(id).map(Answer::Details) };
        self.event(id, Event::Command(decided)).await?;
        match command {
            Command::Remove => self.take_off_list(id, false).await?,
            Command::Delete => self.take_off_list(id, true).await?,
            Command::SetPriority { files, priority } => {
                self.set_priority(id, &files, priority).await?;
                self.clear_missing_set_off(id, &files, priority).await;
            }
            Command::Pause | Command::Resume | Command::Check | Command::Details => {}
        }
        // So the answer's effect shows in the next listing: e.g. every file Off is Seeding.
        if self.torrents.contains_key(&id) {
            self.poll_torrent(id).await;
        }
        Ok(Answer::Done)
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
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(handle) = torrent.handle.clone() else { return };
        // An add again failed: nothing to follow until the next one.
        if !self.in_librqbit(&handle) {
            return;
        }
        let stats = handle.stats();
        let seen = match stats.state {
            TorrentStatsState::Initializing { .. } => Observation::Checking,
            TorrentStatsState::Live | TorrentStatsState::Paused => {
                Observation::Idle { not_back: not_back(&torrent.lifecycle.phase, &stats) }
            }
            TorrentStatsState::Error => Observation::Failed(stats.error.clone().unwrap_or_else(|| "unknown error".into())),
        };
        if let Err(e) = self.event(id, Event::Observation(seen)).await {
            warn!(id, error = %e, "failed to follow librqbit");
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

    fn add(&mut self, prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added>>) {
        let Prepared { add, info_hash, name, answer_at_once, destination } = prepared;
        if let Some(hash) = &info_hash
            && let Some(id) = self.store.id(hash)
        {
            let name = self.torrents.get(&id).and_then(|t| t.name());
            let _ = reply.send(Ok(Added { id, name, already_existed: true }));
            return;
        }
        let id = self.store.next_id();
        let wants = if paused { Wants::Stop } else { Wants::Run };
        if let Some(hash) = &info_hash {
            self.store.insert(hash.clone(), id, wants);
        }
        self.save();
        // Its own task: fetching info takes as long as it takes, and Remove can stop it.
        let (session, inbox, download_dir) = (self.session.clone(), self.inbox.clone(), self.download_dir.clone());
        let task = tokio::spawn(async move {
            let added = add_into(&session, add, &destination, &download_dir).await;
            if let Some(inbox) = inbox.upgrade() {
                let _ = inbox.send(Message::AddFinished { id, added }).await;
            }
        });
        let waiting = if answer_at_once {
            let _ = reply.send(Ok(Added { id, name: name.clone(), already_existed: false }));
            None
        } else {
            Some(reply)
        };
        let torrent = Torrent {
            lifecycle: Lifecycle::new(wants, Phase::FetchingInfo),
            info_hash,
            name,
            handle: None,
            add_task: Some(task),
            waiting,
        };
        self.torrents.insert(id, torrent);
    }

    async fn add_finished(&mut self, id: TorrentId, added: Result<AddTorrentResponse>) {
        let handle = match added {
            Ok(AddTorrentResponse::Added(_, t) | AddTorrentResponse::AlreadyManaged(_, t)) => Ok(t),
            Ok(AddTorrentResponse::ListOnly(_)) => Err(anyhow!("unexpected list-only response")),
            Err(e) => Err(e),
        };
        let Some(torrent) = self.torrents.get_mut(&id) else {
            // Removed meanwhile: take it back out, unless it's another torrent's.
            if let Ok(handle) = handle
                && self.store.id(&handle.info_hash().as_string()).is_none()
                && let Err(e) = self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await
            {
                warn!(error = %e, "failed to take a removed torrent back out");
            }
            return;
        };
        torrent.add_task = None;
        let handle = match handle {
            Ok(handle) => handle,
            // The client that asked gets the error instead.
            Err(e) if torrent.waiting.is_some() => {
                let torrent = self.torrents.remove(&id).expect("just found");
                if let Some(hash) = &torrent.info_hash {
                    self.store.forget(hash);
                    self.save();
                }
                let _ = torrent.waiting.expect("checked").send(Err(anyhow!("{e:#}")));
                return;
            }
            Err(e) => {
                let _ = self.event(id, Event::Observation(Observation::FetchFailed(format!("{e:#}")))).await;
                return;
            }
        };

        let hash = handle.info_hash().as_string();
        let (owner, answer) = match self.store.id(&hash) {
            // A .torrent URL's torrent turned out to be in the list already, maybe still
            // fetching info from a magnet: that one has it now.
            Some(owner) if owner != id => {
                let waiting = self.torrents.remove(&id).and_then(|t| t.waiting);
                (owner, waiting.map(|w| (w, Added { id: owner, name: handle.name(), already_existed: true })))
            }
            owner => {
                let torrent = self.torrents.get_mut(&id).expect("just found");
                if owner.is_none() {
                    self.store.insert(hash.clone(), id, torrent.lifecycle.wants);
                }
                torrent.info_hash = Some(hash.clone());
                let answer = torrent.waiting.take().map(|w| (w, Added { id, name: handle.name(), already_existed: false }));
                (id, answer)
            }
        };
        self.store.get_mut(&hash).expect("has an id").destination = destination(&handle);
        self.save();
        if let Some(torrent) = self.torrents.get_mut(&owner)
            && torrent.handle.is_none()
        {
            if let Some(task) = torrent.add_task.take() {
                task.abort();
            }
            if let Some(waiting) = torrent.waiting.take() {
                let name = handle.name();
                let _ = waiting.send(Ok(Added { id: owner, name, already_existed: false }));
            }
            let checking = matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
            torrent.handle = Some(handle);
            let _ = self.event(owner, Event::Observation(Observation::InfoArrived { checking })).await;
            self.poll_torrent(owner).await;
        }
        if let Some((waiting, added)) = answer {
            let _ = waiting.send(Ok(added));
        }
    }

    async fn take_off_list(&mut self, id: TorrentId, delete_files: bool) -> Result<()> {
        let torrent = self.torrents.remove(&id).expect("decided on it");
        if let Some(hash) = &torrent.info_hash {
            self.guard.set_unavailable(super::id20(hash)?, false);
            self.store.forget(hash);
            self.save();
        }
        if let Some(handle) = torrent.handle {
            return self.session.delete(TorrentIdOrHash::Id(handle.id()), delete_files).await;
        }
        // Fetching info: stop the add.
        if let Some(waiting) = torrent.waiting {
            let _ = waiting.send(Err(anyhow!("removed while fetching info")));
        }
        if let Some(task) = torrent.add_task {
            task.abort();
            let _ = task.await;
        }
        // An add cut short may have left the torrent in librqbit; its `AddFinished`, if any, is
        // ignored now that the torrent is gone.
        if let Some(hash) = torrent.info_hash
            && let Some(t) = self.session.get(TorrentIdOrHash::Hash(super::id20(&hash)?))
            && self.store.id(&hash).is_none()
        {
            self.session.delete(TorrentIdOrHash::Id(t.id()), false).await?;
        }
        Ok(())
    }

    fn handle_of(&self, id: TorrentId) -> Result<&Arc<ManagedTorrent>> {
        self.torrents.get(&id).and_then(|t| t.handle.as_ref()).with_context(|| format!("torrent {id} has no info yet"))
    }

    fn held(&self, handle: &ManagedTorrent) -> Option<&BTreeSet<usize>> {
        self.store.get(&handle.info_hash().as_string())?.held.as_ref()
    }

    fn details(&self, id: TorrentId) -> Result<Details> {
        let handle = self.handle_of(id)?;
        let missing = missing(&self.torrents[&id].lifecycle.phase);
        let files = files(handle, &handle.stats().file_progress, self.held(handle), &missing)?;
        let location = AbsolutePath::new(location(handle)?).map_err(|p| anyhow!("torrent {id} is at a relative path: {}", p.display()))?;
        Ok(Details { id, full_size_bytes: files.iter().map(|f| f.size_bytes).sum(), files, location })
    }

    /// Files set Off aren't downloaded and drop out of the torrent's Size, down to none at all:
    /// it's then finished. librqbit saves the choice with the torrent. While librqbit is
    /// checking the torrent the change is held in the store, shows at once, and is applied
    /// when the check is done.
    async fn set_priority(&mut self, id: TorrentId, indices: &[usize], priority: Priority) -> Result<()> {
        let handle = self.handle_of(id)?.clone();
        let held = self.held(&handle).cloned();
        let files = files(&handle, &[], held.as_ref(), &BTreeSet::new())?;
        if let Some(bad) = indices.iter().find(|&&i| !files.iter().any(|f| f.index == i)) {
            bail!("torrent {id} has no file {bad}");
        }
        let normal: BTreeSet<usize> = files
            .iter()
            .map(|f| (f.index, if indices.contains(&f.index) { priority } else { f.priority }))
            .filter(|(_, p)| *p == Priority::Normal)
            .map(|(i, _)| i)
            .collect();
        let checking = matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
        if !checking && held.is_none() {
            return self.session.update_only_files(&handle, &normal.into_iter().collect()).await;
        }
        self.store.get_mut(&handle.info_hash().as_string()).context("torrent without an id")?.held = Some(normal);
        self.save();
        Ok(())
    }

    /// Every torrent, including those fetching info, by id.
    fn listing(&self) -> Vec<TorrentInfo> {
        self.torrents
            .iter()
            .map(|(&id, t)| match &t.handle {
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
        self.handle.as_ref().and_then(|h| h.name()).or_else(|| self.name.clone())
    }
}

/// A refusal for the client: with its facts typed, worded by the client.
fn refused(id: TorrentId, refusal: Refusal) -> anyhow::Error {
    match refusal {
        Refusal::Because(reason) => anyhow!("torrent {id} {reason}"),
        Refusal::DestinationUnavailable { action, reason } => {
            Refused(proto::Refusal::TorrentDestinationUnavailable { id, action: action.action(), reason }).into()
        }
    }
}

/// Adds the torrent to librqbit with its files in `destination`: a torrent of several files in its
/// own folder there, named as librqbit names it. librqbit only names that folder inside its own
/// (`download_dir`), so it first fetches the info without adding, then adds it with the peers it
/// found meanwhile. `overwrite` lets a re-added torrent resume into files that already exist.
/// Stopped: the actor starts it if it's wanted running.
async fn add_into(
    session: &Arc<Session>,
    add: AddTorrent<'static>,
    destination: &Path,
    download_dir: &Path,
) -> Result<AddTorrentResponse> {
    let listed = session.add_torrent(add, Some(AddTorrentOptions { list_only: true, paused: true, ..Default::default() })).await?;
    let AddTorrentResponse::ListOnly(listed) = listed else { bail!("unexpected response to a list-only add") };
    let own_folder = listed.output_folder.strip_prefix(download_dir).context("librqbit put the torrent elsewhere")?;
    let output_folder = destination.join(own_folder);
    let output_folder = output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned();
    let opts = AddTorrentOptions {
        paused: true,
        overwrite: true,
        output_folder: Some(output_folder),
        initial_peers: Some(listed.seen_peers),
        ..Default::default()
    };
    session.add_torrent(AddTorrent::from_bytes(listed.torrent_bytes), Some(opts)).await
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

/// For a torrent that's Missing files, once librqbit isn't checking it (restoring it makes the
/// missing files again): more files gone, or every missing file back. `None`: nothing new.
fn size_check_returned(handle: &ManagedTorrent, gone: &BTreeMap<usize, Gone>, expected: &[Expected]) -> Option<Observation> {
    if matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
        return None;
    }
    let mut now = gone.clone();
    for g in now.values_mut().filter(|g| g.seen == Seen::NotYet) {
        g.seen = Seen::Was(files::stamp(&g.path));
    }
    for f in expected.iter().filter(|f| !gone.contains_key(&f.index) && files::is_missing(f)) {
        now.insert(f.index, Gone::now(f));
    }
    if now != *gone {
        return Some(Observation::FilesMissing { gone: now });
    }
    now.values().all(files::is_back).then_some(Observation::FilesBack)
}

/// Once a check of missing files put back ends: those holding less than they did before they went
/// aren't the files that went.
fn not_back(phase: &Phase, stats: &TorrentStats) -> BTreeMap<usize, Gone> {
    let Phase::Checking { returning } = phase else { return BTreeMap::new() };
    returning
        .iter()
        .filter(|(i, gone)| stats.file_progress.get(**i).copied().unwrap_or(0) < gone.progress_bytes)
        .map(|(i, gone)| (*i, Gone { seen: Seen::Was(files::stamp(&gone.path)), ..gone.clone() }))
        .collect()
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
        name: t.name.clone(),
        status: t.lifecycle.status(false),
        error: t.lifecycle.message().map(str::to_owned),
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
fn files(
    torrent: &ManagedTorrent,
    progress: &[u64],
    held: Option<&BTreeSet<usize>>,
    missing: &BTreeSet<usize>,
) -> Result<Vec<File>> {
    // librqbit gets every file until it's told otherwise.
    let normal: Option<BTreeSet<usize>> =
        held.cloned().or_else(|| torrent.only_files().map(|files| files.into_iter().collect()));
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
    let stats = torrent.stats();
    let live = stats.live.as_ref();
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
        info_hash: Some(torrent.info_hash().as_string()),
        name: torrent.name(),
        status: t.lifecycle.status(stats.finished),
        error: t.lifecycle.message().map(str::to_owned),
        progress_bytes,
        size_bytes,
        full_size_bytes: files.iter().map(|f| f.size_bytes).sum(),
        uploaded_bytes: stats.uploaded_bytes,
        download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
        upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
        live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
        actions: t.lifecycle.actions(),
    }
}
