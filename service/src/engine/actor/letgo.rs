//! Letting go of a file gone from the Trash (#234, `machine::letgo`): the files Moves to Trash took
//! that are gone from the Trash while librqbit has them open are looked for off the actor, in a
//! thread per torrent with a timeout, as the destinations' disks are (`looks.rs`), every size check;
//! what's found comes back as a message. They're written ahead in their torrent's record, then
//! closed, which frees their space, without waiting on librqbit (one it's reading is closed at a
//! later look). Once the torrent can be taken out of librqbit, it's kept aside
//! (its .torrent and librqbit's `.bitv`, as for Undo), taken out of librqbit, its `.bitv` cleared of
//! those files' pieces, and added back from there, stopped, as [`inlib::State::Resuming`]: its
//! lifecycle hears nothing until librqbit has read the few pieces it samples, so its row stays as
//! it was, then it runs as wanted. Its peers drop once and connect again, and nothing offers those
//! pieces any more: librqbit can't withdraw a piece it has, and drops a peer whose piece it can't
//! read.
//!
//! A crash between the steps is finished at the next start ([`clear_at_start`], then
//! [`Actor::add_back_at_start`]): librqbit restores the torrent from what it saved, its pieces
//! cleared first, or, taken out already, it's added back from what was kept.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, TorrentStatsState};
use proto::{TorrentId, Unavailable};
use tracing::warn;

use super::super::bitv;
use super::super::looks;
use super::super::machine::letgo::{self, Pieces};
use super::super::store::{self, Saved, Store};
use super::super::trashed::Trashed;
use super::super::undo::{self, Stash};
use super::{Actor, Broke, Event, Failure, Observation, Phase, Polled, State, ToTrash, Torrent, decide, inlib, seeding, sorted_trackers};

/// Where a test kills the engine while it lets go of a file: once it's closed, as the torrent is
/// taken out of librqbit, or once it's out, before it's added back.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CrashAt {
    Closing,
    TakingOut,
    AddingBack,
}

/// What a look at the torrents' open files found: the files of each (by info hash) a Move to Trash
/// took that are gone from the Trash, still open.
pub(in super::super) type Wanted = BTreeMap<String, ()>;

/// Looks at the torrents `wanted` names every `every`, each in a thread of its own (an `fstat` of
/// an open file can hang on a network volume that stops answering), and tells `report` what it
/// found. A look that hangs finds nothing until it answers.
pub(super) fn watch(
    trashed: Trashed,
    wanted: tokio::sync::watch::Receiver<Wanted>,
    every: Duration,
    report: impl Fn(String, BTreeSet<usize>) -> bool + Send + Sync + 'static,
) {
    let look: Arc<dyn Fn(&String, &()) -> BTreeSet<usize> + Send + Sync> = Arc::new(move |hash, ()| trashed.gone(hash));
    looks::watch(look, wanted, every, move |hash, found| match found {
        looks::Looked::Read(gone) => gone.is_empty() || report(hash, gone),
        looks::Looked::Stalled => true,
    });
}

impl Actor {
    /// Adds back each torrent with something to let go of that can be taken out of librqbit now.
    pub(super) async fn let_go(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.let_go_of(id).await;
        }
    }

    /// Adds back torrent `id` without the files it lets go of, if it can be taken out of librqbit
    /// now; or ends that, if librqbit has none of their pieces any more.
    async fn let_go_of(&mut self, id: TorrentId) {
        let Some(hash) = self.torrents.get(&id).map(|t| t.info_hash.clone()) else { return };
        let Some(state) = self.store.get(&hash).map(|s| s.letting_go.clone()) else { return };
        let world = letgo::World::Running { free: self.free(id), offered: self.offered(id, &state) };
        {
            match letgo::next(&state, world) {
                None => {}
                Some(letgo::Do::End) => drop(self.letting_go(&hash, &letgo::Event::Cleared)),
                Some(letgo::Do::Stash) => match self.stash_of(id) {
                    Some(stash) => {
                        let stashed = self.letting_go(&hash, &letgo::Event::Stashed(stash));
                        if let letgo::State::Stashed { pieces, stash } = stashed {
                            self.add_back_without(id, &pieces, &stash).await;
                        }
                    }
                    None => warn!(id, "couldn't keep the torrent aside to let go of a file gone from the Trash: tried again later"),
                },
                Some(letgo::Do::AddBack { pieces, stash }) => self.add_back_without(id, &pieces, &stash).await,
                // Only at a start, before librqbit restores the torrents.
                Some(letgo::Do::ClearSaved { .. }) => {}
            }
        }
    }

    /// A look found the files `gone` (by index) of torrent `hash` a Move to Trash took gone from the
    /// Trash, still open. Those set Off are written ahead, then closed; one it gets that's gone is
    /// Missing files, the size check's. Then it's added back without them, if it can be now.
    pub(super) async fn found_gone(&mut self, hash: &str, gone: BTreeSet<usize>) {
        let Some((&id, torrent)) = self.torrents.iter().find(|(_, t)| t.info_hash == hash) else { return };
        let Some(handle) = torrent.inlib.info().cloned() else { return };
        let hash = hash.to_owned();
        let getting = store::getting(self.held(&handle), handle.only_files().as_deref());
        let gone: BTreeSet<usize> = gone.into_iter().filter(|i| getting.as_ref().is_some_and(|g| !g.contains(i))).collect();
        if gone.is_empty() {
            return;
        }
        let Ok(pieces) = handle.with_metadata(|m| -> Pieces {
            gone.iter().filter_map(|&i| Some((i, m.file_infos.get(i)?.piece_range.clone()))).map(|(i, r)| (i, (r.start, r.end))).collect()
        }) else {
            return;
        };
        self.letting_go(&hash, &letgo::Event::Found(pieces));
        // Never waits: one librqbit is reading now is found, and closed, at a later look.
        self.guard.trashed().let_go(&hash, &gone);
        #[cfg(test)]
        if self.crashes_at(CrashAt::Closing).await {
            return;
        }
        self.let_go_of(id).await;
    }

    /// Whether librqbit has any piece of the files torrent `id` lets go of: a check, or an add again,
    /// since may have dropped them all.
    fn offered(&self, id: TorrentId, state: &letgo::State) -> bool {
        let files = match state {
            letgo::State::Holding => return false,
            letgo::State::Gone { pieces } | letgo::State::Stashed { pieces, .. } => pieces.keys(),
        };
        let Some(handle) = self.torrents.get(&id).and_then(|t| t.inlib.info()) else { return true };
        let progress = handle.stats().file_progress;
        files.into_iter().any(|i| progress.get(*i).is_none_or(|had| *had > 0))
    }

    /// Whether torrent `id` can be taken out of librqbit now: librqbit has it, not checking it, it's
    /// one librqbit runs or holds stopped as it is, and nothing runs on its lane.
    fn free(&self, id: TorrentId) -> bool {
        let Some(torrent) = self.torrents.get(&id) else { return false };
        let lib = match &torrent.inlib {
            inlib::State::Has(handle) => !matches!(handle.stats().state, TorrentStatsState::Initializing { .. }),
            inlib::State::Adding { .. }
            | inlib::State::NotAdded { .. }
            | inlib::State::Halted(_)
            | inlib::State::TakenOut(_)
            | inlib::State::Resuming(_)
            | inlib::State::Kept { .. } => false,
        };
        let phase = match &torrent.lifecycle.phase {
            Phase::Ready | Phase::Unchecked { .. } | Phase::MissingFiles { .. } => true,
            // A check reads what's there; out of reach, or an error, it's checked once it's back;
            // being deleted, or partly, librqbit's storage of it touches nothing.
            Phase::FetchingInfo
            | Phase::FetchFailed { .. }
            | Phase::CheckDue { .. }
            | Phase::Checking { .. }
            | Phase::DestinationUnavailable { .. }
            | Phase::Error { .. }
            | Phase::DeleteFailed { .. }
            | Phase::Deleting { .. }
            | Phase::Deleted => false,
        };
        let idle = self.steps.machine().lanes.get(&id).is_none_or(|lane| lane.is_empty());
        lib && phase && idle
    }

    /// `event` for torrent `hash`'s let-go, kept in its record before anything follows from it.
    fn letting_go(&mut self, hash: &str, event: &letgo::Event) -> letgo::State {
        let Some(saved) = self.store.get_mut(hash) else { return letgo::State::Holding };
        saved.letting_go = letgo::decide(&saved.letting_go, event);
        let now = saved.letting_go.clone();
        self.save();
        now
    }

    /// Takes torrent `id` out of librqbit, kept aside in `stash`, and adds it back from there
    /// without `pieces`, as it was, resuming. Failing once it's out, it's an Error, as an add again
    /// that fails is, until Resume or Check adds it again.
    async fn add_back_without(&mut self, id: TorrentId, pieces: &Pieces, stash: &Stash) {
        let Some(inlib::State::Has(handle)) = self.torrents.get(&id).map(|t| t.inlib.clone()) else { return };
        let hash = handle.info_hash().as_string();
        let had = self.pieces(id);
        let held = self.held(&handle).cloned();
        #[cfg(test)]
        if self.crashes_at(CrashAt::TakingOut).await {
            return;
        }
        if let Err(e) = self.session.delete(TorrentIdOrHash::Id(handle.id()), false).await {
            return warn!(id, error = %e, "couldn't take the torrent out of librqbit to let go of a file gone from the Trash");
        }
        // librqbit's last save of its pieces lands in the stash, which shares the file.
        bitv::wait_for(&[(stash.bitv(), had)]).await;
        #[cfg(test)]
        if self.crashes_at(CrashAt::AddingBack).await {
            return;
        }
        let added = self.add_from(&hash, stash, pieces, held.as_ref()).await;
        let torrent = self.torrents.get_mut(&id).expect("never left the list");
        let event = match added {
            Ok(added) => {
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::AddedBack(added));
                torrent.meter.librqbit_started();
                letgo::Event::Cleared
            }
            Err(e) => {
                let detail = format!("{e:#}");
                warn!(id, error = %detail, "couldn't add the torrent back once it let go of a file gone from the Trash");
                let failed = Event::Observed(Observation::Failed(Broke::AddAgainFailed { detail }));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::TakenOut);
                letgo::Event::Failed
            }
        };
        self.letting_go(&hash, &event);
        // The stash isn't named any more.
        self.save_undo();
    }

    /// Adds torrent `hash` back to librqbit from `stash`, stopped, with the pieces it had there but
    /// `pieces`, its files and priorities as they were.
    async fn add_from(&self, hash: &str, stash: &Stash, pieces: &Pieces, held: Option<&BTreeSet<usize>>) -> Result<Arc<ManagedTorrent>> {
        bitv::clear(&stash.bitv(), pieces.values().copied()).context("failed to clear the pieces kept aside")?;
        stash.restore_pieces(&bitv::path(&self.data_dir.join("session"), hash))?;
        let bytes = stash.torrent()?;
        // Its folder and priorities known, the files set Off aren't made again (#67), nor those held.
        let getting = store::getting(held, stash.only_files.as_deref());
        let storage = self.guard.for_add(stash.output_folder.clone(), getting.as_deref());
        let opts = AddTorrentOptions {
            paused: true,
            overwrite: true,
            only_files: stash.only_files.clone(),
            output_folder: Some(stash.output_folder.to_str().context("the destination isn't valid UTF-8")?.to_owned()),
            preferred_id: Some(stash.librqbit_id),
            storage_factory: Some(storage.boxed()),
            ..Default::default()
        };
        match self.session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await? {
            AddTorrentResponse::Added(_, t) => Ok(t),
            AddTorrentResponse::AlreadyManaged(_, t) if t.info_hash().as_string() == hash => Ok(t),
            AddTorrentResponse::AlreadyManaged(..) | AddTorrentResponse::ListOnly(_) => {
                Err(anyhow!("librqbit has another torrent under its id"))
            }
        }
    }

    /// The torrents a crash took out of librqbit to let go of a file, before they were added back
    /// (listed from their records, [`kept`]): added back now from what was kept.
    pub(super) async fn add_back_at_start(&mut self) {
        let kept: Vec<TorrentId> =
            self.torrents.iter().filter(|(_, t)| matches!(t.inlib, inlib::State::Kept { .. })).map(|(id, _)| *id).collect();
        for id in kept {
            if let Err(e) = self.add_back_kept(id).await {
                warn!(id, error = ?e, "couldn't add back a torrent a crash took out of librqbit as it let go of a file");
            }
        }
    }

    /// Adds torrent `id`, kept aside, back to librqbit from what was kept, without the pieces it
    /// lets go of, as it was: resuming. One whose destination can't be reached gets storage that
    /// touches nothing, as a torrent restored then does, and is checked once it's back. Failing,
    /// it's an Error, its record and what was kept of it kept, until Resume or Check tries again.
    pub(super) async fn add_back_kept(&mut self, id: TorrentId) -> Result<(), Failure> {
        let Some(hash) = self.torrents.get(&id).map(|t| t.info_hash.clone()) else { return Ok(()) };
        let Some(saved) = self.store.get(&hash).cloned() else { return Ok(()) };
        let letgo::State::Stashed { pieces, stash } = &saved.letting_go else { return Ok(()) };
        let added = self.add_from(&hash, stash, pieces, saved.held.as_ref()).await;
        let torrent = self.torrents.get_mut(&id).expect("listed");
        match added {
            Ok(handle) => {
                torrent.trackers = sorted_trackers(&handle);
                torrent.polled = Polled::of(&handle.stats(), &Polled::default());
                torrent.inlib = inlib::decide(&torrent.inlib, &inlib::Event::AddedBack(handle));
                self.letting_go(&hash, &letgo::Event::Cleared);
                self.save_undo();
                Ok(())
            }
            Err(e) => {
                let detail = format!("{e:#}");
                let failed = Event::Observed(Observation::Failed(Broke::AddAgainFailed { detail: detail.clone() }));
                torrent.lifecycle = decide(&torrent.lifecycle, &failed).expect("never refused");
                Err(proto::ServiceError::AddAgainFailed { id, detail }.into())
            }
        }
    }

    /// What a let-go kept of torrent `id`, out of librqbit after a crash ([`inlib::State::Kept`]),
    /// copied, for a Remove's or a Delete's undo: its own stays its own while it's listed.
    pub(super) fn kept_copy(&self, id: TorrentId) -> Option<Stash> {
        let torrent = self.torrents.get(&id).filter(|t| matches!(t.inlib, inlib::State::Kept { .. }))?;
        let letgo::State::Stashed { stash, .. } = &self.store.get(&torrent.info_hash)?.letting_go else { return None };
        undo::copy(&self.data_dir, &torrent.info_hash, stash)
            .inspect_err(|e| warn!(id, error = %e, "failed to keep a torrent kept aside for Undo"))
            .ok()
    }

    /// Where torrent `id`'s files are, from what a let-go kept of it ([`inlib::State::Kept`]): for a
    /// Delete of one librqbit doesn't have.
    pub(super) fn kept_to_trash(&self, id: TorrentId) -> Option<Result<ToTrash>> {
        let torrent = self.torrents.get(&id).filter(|t| matches!(t.inlib, inlib::State::Kept { .. }))?;
        let letgo::State::Stashed { stash, .. } = &self.store.get(&torrent.info_hash)?.letting_go else { return None };
        Some(to_trash_of(stash))
    }

    /// Kills the engine here if a test says so: whether it did.
    #[cfg(test)]
    async fn crashes_at(&mut self, at: CrashAt) -> bool {
        let Some((_, crashed)) = self.crash_letting_go.take_if(|(when, _)| *when == at) else { return false };
        self.session.stop().await;
        self.killed = true;
        crashed.store(true, std::sync::atomic::Ordering::SeqCst);
        true
    }
}

/// Where the files of the torrent kept in `stash` are, from its .torrent, as [`super::to_trash`]
/// has them from librqbit's.
fn to_trash_of(stash: &Stash) -> Result<ToTrash> {
    let bytes = stash.torrent()?;
    let torrent = librqbit::torrent_from_bytes(&bytes)?;
    let data = &torrent.info.data;
    let part = |bytes: &[u8]| PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
    let files: Vec<PathBuf> = match &data.files {
        None => vec![stash.output_folder.join(part(data.name.as_ref().context("a torrent of one file names it")?.as_ref()))],
        Some(files) => files.iter().map(|f| f.path.iter().fold(stash.output_folder.clone(), |at, p| at.join(part(p.as_ref())))).collect(),
    };
    let single = files.len() == 1;
    let location = if single { files[0].clone() } else { stash.output_folder.clone() };
    Ok(ToTrash { location, files, single })
}

/// Torrent `hash`, `saved`, a crash took out of librqbit as it let go of a file, before it was added
/// back: listed from its record as it was (Unchecked if its check was cancelled; Destination
/// unavailable, `away`, if its destination can't be reached), kept aside, until `run` adds it back.
pub(super) fn kept(hash: &str, saved: &Saved, away: Option<Unavailable>) -> Option<Torrent> {
    let letgo::State::Stashed { stash, .. } = &saved.letting_go else { return None };
    let name = stash.torrent().ok().and_then(|bytes| {
        let torrent = librqbit::torrent_from_bytes(&bytes).ok()?;
        torrent.info.data.name.map(|n| String::from_utf8_lossy(n.as_ref()).into_owned())
    });
    let phase = match away {
        Some(reason) => Phase::DestinationUnavailable { reason },
        None if saved.unchecked => Phase::Unchecked { gone: BTreeMap::new() },
        None => Phase::Ready,
    };
    Some(Torrent {
        lifecycle: State::new(saved.wants, phase),
        info_hash: hash.to_owned(),
        trackers: Vec::new(),
        polled: Polled::default(),
        inlib: inlib::State::Kept { name },
        meter: seeding::Meter::new(saved.round),
    })
}

/// At a start, before librqbit restores its torrents from `session`: the pieces of each torrent
/// whose let-go a crash cut short while librqbit still had it saved are cleared there, so it
/// restores it without them; those it doesn't have (`saved` says which it has) are added back by
/// [`Actor::add_back_at_start`].
pub(in super::super) fn clear_at_start(store: &mut Store, session: &Path, saved: impl Fn(&str) -> bool) {
    for (hash, record) in store.iter_mut() {
        let Some(letgo::Do::ClearSaved { pieces }) = letgo::next(&record.letting_go, letgo::World::Starting { saved: saved(hash) }) else {
            continue;
        };
        match bitv::clear(&bitv::path(session, hash), pieces.values().copied()) {
            Ok(()) => record.letting_go = letgo::decide(&record.letting_go, &letgo::Event::Cleared),
            // librqbit samples what it saved, and checks it in full on a piece of a file that's gone.
            Err(e) => warn!(hash, error = %e, "couldn't clear the pieces of a file gone from the Trash"),
        }
    }
}
