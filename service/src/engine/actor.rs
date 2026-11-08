//! The actor: one task that owns every torrent's [`State`] and librqbit's session, and handles
//! one [`Message`] at a time: client commands (each answered through its own reply channel),
//! the end of an add, a regular look at what librqbit is doing, and a look at the files on disk
//! (every few seconds, and at once when the watcher sees a path go: `files.rs`). Each
//! per-torrent event goes through [`decide`]; entering a state makes librqbit match it. Slow work
//! (fetching info, downloading a .torrent URL) runs in its own task and comes back as a message.
//!
//! Missing files: a torrent with a file it's getting gone from disk stops in librqbit (so nothing
//! more is written into the deleted file) and is Missing files, with the files that are missing
//! remembered until it leaves that state. Setting them Off leaves it Paused or Done; Resume and
//! Check take it out of librqbit and add it again, which opens the files afresh and checks them.
//! Once every missing file is back on disk it's added again stopped, so they're checked: it's
//! then Paused or Done, unless a file put back holds less than it did before it went, which makes
//! it Missing files again (until that file changes once more).
//!
//! Destination unavailable: before its files, a torrent's destination is looked at (every few
//! seconds, when the watcher sees a path go, and when librqbit fails). One that can't be reached
//! stops the torrent in librqbit, which is then Destination unavailable with the reason; it
//! remembers whether it was running. Once the destination is back, it's added again (so its files
//! are opened afresh and checked) and runs again if it was running. librqbit creates a torrent's
//! files whenever it adds one: [`Guard`] gives the torrents whose destination is away storage
//! that touches nothing, so restoring them at start makes no folders where a drive isn't.
//!
//! Lifecycle across restarts:
//! - Stop: torrents running (Downloading, Seeding, Checking to run, or Destination unavailable
//!   having been running) are recorded as such in the store, with every torrent's missing files and the files it's getting; then they're paused in
//!   librqbit, so its session state saves every torrent paused.
//! - Start: torrents whose destination can't be reached are Destination unavailable, restored by
//!   librqbit without touching their files. The others' recorded files are checked before librqbit
//!   restores its torrents (restoring creates missing files again, empty), and torrents missing
//!   some are Missing files. librqbit
//!   restores the rest paused after a clean stop; those recorded as running are resumed.
//! - A crash skips the stop: librqbit restores torrents as they were when it last saved them
//!   (it saves on every pause and resume), so running ones start running again by themselves,
//!   unchecked; nothing is recorded as running, so the start resumes nothing more.

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
use super::lifecycle::{Decision, Event, Refusal, State, decide};
use super::store::Store;

/// How often the actor looks at what librqbit is doing, between commands.
const LOOK_EVERY: Duration = Duration::from_millis(500);

pub(super) enum Message {
    List(oneshot::Sender<Vec<TorrentInfo>>),
    Add { prepared: Prepared, paused: bool, reply: oneshot::Sender<Result<Added>> },
    Command { id: TorrentId, command: Command, reply: oneshot::Sender<Result<Answer>> },
    /// librqbit's add of torrent `id` ended.
    AddFinished { id: TorrentId, added: Result<AddTorrentResponse> },
    /// The watcher saw these paths removed or renamed.
    FilesChanged(Vec<PathBuf>),
    /// Record what's running, pause it, stop the session, answer and end.
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
    fn event(&self) -> Event {
        match self {
            Command::Pause => Event::Pause,
            Command::Resume => Event::Resume,
            Command::Remove => Event::Remove,
            Command::Delete => Event::Delete,
            Command::Details => Event::Details,
            Command::SetPriority { .. } => Event::SetPriority,
            Command::Check => Event::Check,
        }
    }
}

/// What the actor works with, besides what's kept across restarts.
pub(super) struct Setup {
    pub session: Arc<Session>,
    pub data_dir: PathBuf,
    /// librqbit's own folder, from which it names a torrent's own folder.
    pub download_dir: PathBuf,
    pub guard: Guard,
    pub check_files_every: Duration,
    /// For add tasks and the watcher to report back.
    pub inbox: mpsc::WeakSender<Message>,
}

pub(super) struct Actor {
    pub(super) session: Arc<Session>,
    data_dir: PathBuf,
    pub(super) download_dir: PathBuf,
    /// Which torrents librqbit may create files for: not those whose destination is away.
    guard: Guard,
    pub(super) store: Store,
    pub(super) torrents: BTreeMap<TorrentId, Torrent>,
    /// For add tasks and the watcher to report back.
    inbox: mpsc::WeakSender<Message>,
    /// `None` if it couldn't start: then only the regular look at the files finds missing ones.
    watcher: Option<Watcher>,
    check_files_every: Duration,
}

pub(super) struct Torrent {
    pub(super) state: State,
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
    /// The files found missing, by index; empty unless it's Missing files.
    pub(super) missing: BTreeMap<usize, Gone>,
    /// Missing files put back, being checked: whether they're what they were is judged once the
    /// check ends.
    returning: BTreeMap<usize, Gone>,
}

impl Actor {
    /// Takes over the torrents librqbit restored: ones it doesn't know get fresh ids; ids of
    /// torrents that were fetching info when the service stopped go. Those in `away` (by info
    /// hash, with why their destination can't be reached) are Destination unavailable; those in
    /// `missing` (the files missing at start, with what librqbit had of each) are Missing files.
    pub(super) fn new(
        setup: Setup,
        mut store: Store,
        mut missing: BTreeMap<String, BTreeMap<usize, u64>>,
        mut away: BTreeMap<String, Unavailable>,
    ) -> Self {
        let Setup { session, data_dir, download_dir, guard, check_files_every, inbox } = setup;
        let restored: Vec<Arc<ManagedTorrent>> = session.with_torrents(|ts| ts.map(|(_, t)| t.clone()).collect());
        let hashes: HashSet<String> = restored.iter().map(|t| t.info_hash().as_string()).collect();
        store.retain(|hash| hashes.contains(hash));
        let mut torrents = BTreeMap::new();
        for handle in restored {
            let hash = handle.info_hash().as_string();
            let id = store.id_or_assign(&hash);
            store.get_mut(&hash).expect("just assigned").destination = destination(&handle);
            let missing = gone_at_start(&handle, store.get(&hash).and_then(|s| s.held.as_ref()), missing.remove(&hash));
            // librqbit restored it paused; it stays so.
            let restored = restored_state(&handle.stats());
            let state = match away.remove(&hash) {
                Some(reason) => {
                    let running = restored.is_running() || store.get(&hash).is_some_and(|s| s.running_at_shutdown);
                    State::DestinationUnavailable { reason, running }
                }
                None if missing.is_empty() => restored,
                None => State::MissingFiles,
            };
            let torrent = Torrent {
                state,
                info_hash: Some(hash),
                name: None,
                handle: Some(handle),
                add_task: None,
                waiting: None,
                missing,
                returning: BTreeMap::new(),
            };
            torrents.insert(id, torrent);
        }
        let watcher_inbox = inbox.clone();
        let watcher = Watcher::start(move |paths| {
            // A full inbox drops the news; the regular look at the files catches it.
            if let Some(inbox) = watcher_inbox.upgrade() {
                let _ = inbox.try_send(Message::FilesChanged(paths));
            }
        });
        let watcher = watcher.inspect_err(|e| warn!(error = %e, "failed to start the file watcher")).ok();
        let actor = Self { session, data_dir, download_dir, guard, store, torrents, inbox, watcher, check_files_every };
        actor.save();
        actor
    }

    pub(super) async fn run(mut self, mut inbox: mpsc::Receiver<Message>) {
        self.resume_running_at_shutdown().await;
        let mut look = tokio::time::interval(LOOK_EVERY);
        look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut check_files = tokio::time::interval(self.check_files_every);
        check_files.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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
                _ = look.tick() => self.look_at_all().await,
                _ = check_files.tick() => {
                    self.watch_destinations();
                    self.check_all_files().await;
                }
            }
        }
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::List(reply) => {
                self.look_at_all().await;
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

    /// Resumes the torrents that were running when the service last stopped cleanly, unless
    /// their files were found missing at start (`Engine::start`).
    async fn resume_running_at_shutdown(&mut self) {
        let ids: Vec<TorrentId> = self
            .torrents
            .iter()
            .filter(|(_, t)| t.info_hash.as_deref().and_then(|h| self.store.get(h)).is_some_and(|s| s.running_at_shutdown))
            .filter(|(_, t)| !matches!(t.state, State::MissingFiles | State::DestinationUnavailable { .. }))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Err(e) = self.event(id, Event::Resume).await {
                warn!(id, error = %e, "failed to resume a torrent that was running");
            }
        }
        let hashes: Vec<String> = self.torrents.values().filter_map(|t| t.info_hash.clone()).collect();
        for hash in hashes {
            if let Some(saved) = self.store.get_mut(&hash) {
                saved.running_at_shutdown = false;
            }
        }
        self.save();
    }

    async fn stop(&mut self) {
        let mut running = Vec::new();
        for torrent in self.torrents.values_mut() {
            let Some(hash) = &torrent.info_hash else { continue };
            if let Some(saved) = self.store.get_mut(hash) {
                saved.running_at_shutdown = torrent.state.wants_to_run();
                saved.missing_at_shutdown = torrent.missing.iter().map(|(&i, gone)| (i, gone.progress_bytes)).collect();
                saved.files_at_shutdown = torrent.handle.as_ref().map(|h| expected(h, saved.held.as_ref())).unwrap_or_default();
            }
            if let Some(handle) = &torrent.handle
                && torrent.state.is_running()
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
        // Recorded first: a crash between the two then resumes them on start rather than
        // leaving them paused.
        self.save();
        for handle in running {
            if let Err(e) = self.session.pause(&handle).await {
                warn!(error = %e, "failed to pause a torrent while stopping");
            }
        }
        self.session.stop().await;
    }

    /// Decides `event` for torrent `id` and carries the decision out. `Ok(true)`: a command may
    /// go ahead with its own work ([`Decision::Allow`]).
    async fn event(&mut self, id: TorrentId, event: Event) -> Result<bool> {
        let torrent = self.torrents.get(&id).with_context(|| format!("no torrent with id {id}"))?;
        let hash = torrent.info_hash.as_deref().map(super::id20).transpose()?;
        let next = match decide(&torrent.state, &event) {
            Decision::To(next) => {
                self.enter(id, &next).await?;
                next
            }
            Decision::Recheck { paused } => {
                // Its files can be made again.
                if let Some(hash) = hash {
                    self.guard.set_away(hash, false);
                }
                self.add_again(id, paused).await?;
                State::Checking { paused }
            }
            Decision::Allow => return Ok(true),
            Decision::Ignore => return Ok(false),
            Decision::Reject(Refusal::Because(reason)) => bail!("torrent {id} {reason}"),
            Decision::Reject(Refusal::Away { what, reason }) => {
                let refusal = proto::Refusal::TorrentDestinationUnavailable { id, action: what.action(), reason };
                return Err(Refused(refusal).into());
            }
        };
        if let Some(hash) = hash {
            self.guard.set_away(hash, matches!(next, State::DestinationUnavailable { .. }));
        }
        let torrent = self.torrents.get_mut(&id).expect("still there");
        if next != State::MissingFiles {
            torrent.missing.clear();
        }
        torrent.state = next;
        Ok(false)
    }

    /// Takes the torrent out of librqbit and adds it again at the same place with the same
    /// priorities, `paused` or not, so librqbit opens its files afresh and checks them, keeping
    /// what's valid: librqbit can't check a torrent it has, and forgets the pieces it had along
    /// with the torrent. On failure after taking it out, it's an Error (librqbit no longer has
    /// it) until Resume or Check adds it again.
    async fn add_again(&mut self, id: TorrentId, paused: bool) -> Result<()> {
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
        let opts = AddTorrentOptions { paused, overwrite: true, only_files, output_folder: Some(output_folder), ..Default::default() };
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
                torrent.state = State::Error { message: message.clone() };
                torrent.missing.clear();
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

    async fn check_all_files(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.check_files(id).await;
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
            self.check_files(id).await;
        }
    }

    /// Looks at the torrent's destination, then its files on disk: with the destination away it's
    /// Destination unavailable, and checked once the destination is back; with any file it's
    /// getting missing, it's Missing files; one that's Missing files with every missing file back
    /// is checked.
    async fn check_files(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(handle) = torrent.handle.clone() else { return };
        let away = destination(&handle).and_then(|d| files::unavailable(&d));
        let unavailable = matches!(torrent.state, State::DestinationUnavailable { .. });
        if away.is_some() || unavailable {
            let event = match away {
                Some(reason) => Event::DestinationGone { reason },
                None => Event::DestinationBack,
            };
            if let Err(e) = self.event(id, event).await {
                warn!(id, error = %e, "failed to follow the torrent's destination");
            }
            return;
        }
        let expected = expected(&handle, self.held(&handle));
        if torrent.state == State::MissingFiles {
            return self.look_for_files_back(id, &handle, &expected).await;
        }
        let missing: Vec<&Expected> = expected.iter().filter(|f| files::is_missing(f)).collect();
        if missing.is_empty() {
            return;
        }
        if let Err(e) = self.event(id, Event::FilesMissing).await {
            warn!(id, error = %e, "failed to stop a torrent with missing files");
        }
        if let Some(torrent) = self.torrents.get_mut(&id)
            && torrent.state == State::MissingFiles
        {
            torrent.missing.extend(missing.into_iter().map(|f| (f.index, Gone::now(f))));
        }
    }

    /// For a torrent that's Missing files: notes more files going, and checks it once every
    /// missing file is back.
    async fn look_for_files_back(&mut self, id: TorrentId, handle: &ManagedTorrent, expected: &[Expected]) {
        // librqbit restoring it is checking the files it made again; they're judged after.
        if matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
            return;
        }
        let torrent = self.torrents.get_mut(&id).expect("looked at");
        for gone in torrent.missing.values_mut().filter(|g| g.seen == Seen::NotYet) {
            gone.seen = Seen::Was(files::stamp(&gone.path));
        }
        for f in expected {
            if !torrent.missing.contains_key(&f.index) && files::is_missing(f) {
                torrent.missing.insert(f.index, Gone::now(f));
            }
        }
        if !torrent.missing.values().all(files::is_back) {
            return;
        }
        torrent.returning = std::mem::take(&mut torrent.missing);
        if let Err(e) = self.event(id, Event::FilesBack).await {
            warn!(id, error = %e, "failed to check files put back");
            let torrent = self.torrents.get_mut(&id).expect("still there");
            if torrent.state == State::MissingFiles {
                torrent.missing = std::mem::take(&mut torrent.returning);
            }
        }
    }

    /// Once the check of missing files put back ends: those holding less than they did before
    /// they went aren't the files that went, and make the torrent Missing files again.
    async fn judge_returned(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        if torrent.returning.is_empty() || matches!(torrent.state, State::Checking { .. }) {
            return;
        }
        let returned = std::mem::take(&mut torrent.returning);
        let (Some(handle), State::Paused | State::Done | State::Downloading | State::Seeding) = (&torrent.handle, &torrent.state)
        else {
            return;
        };
        let progress = handle.stats().file_progress;
        let still: BTreeMap<usize, Gone> = returned
            .into_iter()
            .filter(|(i, gone)| progress.get(*i).copied().unwrap_or(0) < gone.progress_bytes)
            .map(|(i, gone)| (i, Gone { seen: Seen::Was(files::stamp(&gone.path)), ..gone }))
            .collect();
        if still.is_empty() {
            return;
        }
        if let Err(e) = self.event(id, Event::FilesMissing).await {
            warn!(id, error = %e, "failed to stop a torrent whose files put back aren't them");
        }
        if let Some(torrent) = self.torrents.get_mut(&id)
            && torrent.state == State::MissingFiles
        {
            torrent.missing = still;
        }
    }

    /// After a priority change: with none of the files it's getting missing any more, a torrent
    /// that was Missing files is stopped.
    async fn clear_missing_set_off(&mut self, id: TorrentId, indices: &[usize], priority: Priority) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        if priority != Priority::Off || torrent.missing.is_empty() {
            return;
        }
        torrent.missing.retain(|i, _| !indices.contains(i));
        let Some(handle) = torrent.handle.clone().filter(|_| torrent.missing.is_empty()) else { return };
        let complete = handle.stats().finished;
        if let Err(e) = self.event(id, Event::NoFilesMissing { complete }).await {
            warn!(id, error = %e, "failed to stop a torrent no longer missing files");
        }
    }

    /// Makes librqbit match `state`. On failure the torrent stays as it was.
    async fn enter(&self, id: TorrentId, state: &State) -> Result<()> {
        let Some(handle) = self.torrents.get(&id).and_then(|t| t.handle.as_ref()) else { return Ok(()) };
        let lib = handle.stats().state;
        let run = match state {
            State::Downloading | State::Seeding | State::Checking { paused: false } => true,
            // Missing files: so nothing more is written into the deleted files; Destination
            // unavailable: nothing can be.
            State::Paused | State::Done | State::MissingFiles | State::DestinationUnavailable { .. } => false,
            // A torrent added paused checks first; it's stopped once the check ends.
            State::Checking { paused: true } => return Ok(()),
            // #24's mover takes it out of librqbit.
            State::Moving { .. } => return Ok(()),
            State::FetchingInfo { .. } | State::FetchFailed { .. } | State::Error { .. } => return Ok(()),
        };
        match lib {
            TorrentStatsState::Paused | TorrentStatsState::Error | TorrentStatsState::Initializing { paused: true }
                if run =>
            {
                self.session.unpause(handle).await
            }
            TorrentStatsState::Live | TorrentStatsState::Initializing { paused: false } if !run => {
                self.session.pause(handle).await
            }
            _ => Ok(()),
        }
    }

    async fn command(&mut self, id: TorrentId, command: Command) -> Result<Answer> {
        // Decided on what librqbit is doing now, not at the last look.
        self.look_at(id).await;
        let allowed = self.event(id, command.event()).await?;
        let answer = match command {
            _ if !allowed => Answer::Done,
            Command::Pause | Command::Resume | Command::Check => Answer::Done,
            Command::Remove => self.take_off_list(id, false).await.map(|()| Answer::Done)?,
            Command::Delete => self.take_off_list(id, true).await.map(|()| Answer::Done)?,
            Command::Details => Answer::Details(self.details(id)?),
            Command::SetPriority { files, priority } => {
                self.set_priority(id, &files, priority).await?;
                self.clear_missing_set_off(id, &files, priority).await;
                Answer::Done
            }
        };
        // So the answer's effect shows in the next listing: e.g. every file Off is Seeding.
        if self.torrents.contains_key(&id) {
            self.look_at(id).await;
        }
        Ok(answer)
    }

    async fn look_at_all(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.look_at(id).await;
        }
    }

    /// Tells the torrent's machine what librqbit is doing, and applies held priorities once
    /// librqbit can take them.
    async fn look_at(&mut self, id: TorrentId) {
        let Some(handle) = self.torrents.get(&id).and_then(|t| t.handle.clone()) else { return };
        // An add again failed: nothing to follow until the next one.
        if !self.in_librqbit(&handle) {
            return;
        }
        let stats = handle.stats();
        let seen = match stats.state {
            TorrentStatsState::Initializing { .. } => Event::SeenChecking,
            TorrentStatsState::Live => Event::SeenRunning { complete: stats.finished },
            TorrentStatsState::Paused => Event::SeenStopped { complete: stats.finished },
            // Failing to write where the destination was is its destination going, which keeps
            // whether it was running.
            TorrentStatsState::Error => match destination(&handle).and_then(|d| files::unavailable(&d)) {
                Some(reason) => Event::DestinationGone { reason },
                None => Event::SeenFailed(stats.error.unwrap_or_else(|| "unknown error".into())),
            },
        };
        if let Err(e) = self.event(id, seen).await {
            warn!(id, error = %e, "failed to follow librqbit");
        }
        if !matches!(stats.state, TorrentStatsState::Initializing { .. }) {
            self.apply_held(id, &handle).await;
        }
        self.judge_returned(id).await;
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
        if let Some(hash) = &info_hash {
            self.store.insert(hash.clone(), id);
        }
        self.save();
        // Its own task: fetching info takes as long as it takes, and Remove can stop it.
        let (session, inbox, download_dir) = (self.session.clone(), self.inbox.clone(), self.download_dir.clone());
        let task = tokio::spawn(async move {
            let added = add_into(&session, add, paused, &destination, &download_dir).await;
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
        let state = State::FetchingInfo { paused };
        let torrent = Torrent {
            state,
            info_hash,
            name,
            handle: None,
            add_task: Some(task),
            waiting,
            missing: BTreeMap::new(),
            returning: BTreeMap::new(),
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
                let _ = self.event(id, Event::FetchFailed(format!("{e:#}"))).await;
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
                if owner.is_none() {
                    self.store.insert(hash.clone(), id);
                }
                let torrent = self.torrents.get_mut(&id).expect("just found");
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
            torrent.handle = Some(handle);
            let _ = self.event(owner, Event::InfoArrived).await;
            self.look_at(owner).await;
        }
        if let Some((waiting, added)) = answer {
            let _ = waiting.send(Ok(added));
        }
    }

    async fn take_off_list(&mut self, id: TorrentId, delete_files: bool) -> Result<()> {
        let torrent = self.torrents.remove(&id).expect("decided on it");
        if let Some(hash) = &torrent.info_hash {
            self.guard.set_away(super::id20(hash)?, false);
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
        let missing = self.torrents[&id].missing.keys().copied().collect();
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

/// Adds the torrent to librqbit with its files in `destination`: a torrent of several files in its
/// own folder there, named as librqbit names it. librqbit only names that folder inside its own
/// (`download_dir`), so it first fetches the info without adding, then adds it with the peers it
/// found meanwhile. `overwrite` lets a re-added torrent resume into files that already exist.
async fn add_into(
    session: &Arc<Session>,
    add: AddTorrent<'static>,
    paused: bool,
    destination: &Path,
    download_dir: &Path,
) -> Result<AddTorrentResponse> {
    let listed = session.add_torrent(add, Some(AddTorrentOptions { list_only: true, paused, ..Default::default() })).await?;
    let AddTorrentResponse::ListOnly(listed) = listed else { bail!("unexpected response to a list-only add") };
    let own_folder = listed.output_folder.strip_prefix(download_dir).context("librqbit put the torrent elsewhere")?;
    let output_folder = destination.join(own_folder);
    let output_folder = output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned();
    let opts = AddTorrentOptions {
        paused,
        overwrite: true,
        output_folder: Some(output_folder),
        initial_peers: Some(listed.seen_peers),
        ..Default::default()
    };
    session.add_torrent(AddTorrent::from_bytes(listed.torrent_bytes), Some(opts)).await
}

/// The state of a torrent librqbit restored, before anything happened to it.
fn restored_state(stats: &TorrentStats) -> State {
    match &stats.state {
        TorrentStatsState::Initializing { paused } => State::Checking { paused: *paused },
        TorrentStatsState::Live if stats.finished => State::Seeding,
        TorrentStatsState::Live => State::Downloading,
        TorrentStatsState::Paused if stats.finished => State::Done,
        TorrentStatsState::Paused => State::Paused,
        TorrentStatsState::Error => State::Error { message: stats.error.clone().unwrap_or_default() },
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
        name: t.name.clone(),
        status: t.state.status(),
        error: t.state.message().map(str::to_owned),
        progress_bytes: 0,
        size_bytes: 0,
        full_size_bytes: 0,
        uploaded_bytes: 0,
        download_bytes_per_sec: 0,
        upload_bytes_per_sec: 0,
        live_peers: 0,
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
    let state = &t.state;
    let stats = torrent.stats();
    let live = stats.live.as_ref();
    // Size and progress from the files you're getting: librqbit counts whole pieces, which
    // spill into neighbouring files that may be Off.
    let missing = t.missing.keys().copied().collect();
    let files = files(torrent, &stats.file_progress, held, &missing).unwrap_or_default();
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
        status: state.status(),
        error: state.message().map(str::to_owned),
        progress_bytes,
        size_bytes,
        full_size_bytes: files.iter().map(|f| f.size_bytes).sum(),
        uploaded_bytes: stats.uploaded_bytes,
        download_bytes_per_sec: live.map_or(0, |l| l.download_speed.as_bytes()),
        upload_bytes_per_sec: live.map_or(0, |l| l.upload_speed.as_bytes()),
        live_peers: live.map_or(0, |l| l.snapshot.peer_stats.live),
    }
}
