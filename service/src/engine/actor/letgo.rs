//! Letting go of a file gone from the Trash (#234, `machine::letgo`): at each size check, the files
//! Moves to Trash took that are gone from the Trash while librqbit has them open are written ahead
//! in their torrent's record, then closed, which frees their space. The torrent is then kept aside
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
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, TorrentStatsState};
use proto::TorrentId;
use tracing::warn;

use super::super::bitv;
use super::super::machine::letgo::{self, Pieces};
use super::super::store::{self, Saved, Store};
use super::super::undo::Stash;
use super::{Actor, Broke, Event, Observation, Phase, Polled, State, Torrent, decide, inlib, seeding, sorted_trackers};

/// Where a test kills the engine while it lets go of a file: as it takes the torrent out of
/// librqbit, or once it's out, before it's added back.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CrashAt {
    TakingOut,
    AddingBack,
}

impl Actor {
    /// Finds the files gone from the Trash, writes them ahead and closes them, then adds back each
    /// torrent with something to let go of that can be taken out of librqbit now.
    pub(super) async fn let_go(&mut self) {
        let ids: Vec<TorrentId> = self.torrents.keys().copied().collect();
        for id in ids {
            self.find_gone(id).await;
            let Some(hash) = self.torrents.get(&id).map(|t| t.info_hash.clone()) else { continue };
            let Some(state) = self.store.get(&hash).map(|s| s.letting_go.clone()) else { continue };
            match letgo::next(&state, letgo::World::Running { free: self.free(id) }) {
                None => {}
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

    /// The files of torrent `id` a Move to Trash took that are gone from the Trash, set Off, while
    /// librqbit has them open: written ahead, then closed. One it gets that's gone is Missing files,
    /// the size check's.
    async fn find_gone(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(handle) = torrent.inlib.info().cloned() else { return };
        let hash = torrent.info_hash.clone();
        // On the actor, as a size check's look: of files open already, only of a torrent a Move to
        // Trash took a file of.
        let gone = self.guard.trashed().gone(&hash);
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
        let (trashed, files) = (self.guard.trashed().clone(), gone);
        let _ = tokio::task::spawn_blocking(move || trashed.let_go(&hash, &files)).await;
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
            | inlib::State::Resuming(_) => false,
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

    /// The torrents a crash took out of librqbit to let go of a file, before they were added back:
    /// added back now from what was kept, as they were (Unchecked if their check was cancelled).
    /// One that can't be is gone, as any librqbit didn't restore.
    pub(super) async fn add_back_at_start(&mut self) {
        let out: Vec<(String, Saved)> = self
            .store
            .iter()
            .filter(|(hash, _)| !self.torrents.values().any(|t| t.info_hash == **hash))
            .map(|(hash, saved)| (hash.clone(), saved.clone()))
            .collect();
        for (hash, saved) in out {
            let Some(letgo::Do::AddBack { pieces, stash }) = letgo::next(&saved.letting_go, letgo::World::Starting { saved: false }) else {
                continue;
            };
            let id = saved.id;
            match self.add_from(&hash, &stash, &pieces, saved.held.as_ref()).await {
                Ok(handle) => {
                    let phase = if saved.unchecked { Phase::Unchecked { gone: BTreeMap::new() } } else { Phase::Ready };
                    let torrent = Torrent {
                        lifecycle: State::new(saved.wants, phase),
                        info_hash: hash.clone(),
                        trackers: sorted_trackers(&handle),
                        polled: Polled::of(&handle.stats(), &Polled::default()),
                        inlib: inlib::State::Resuming(handle),
                        meter: seeding::Meter::new(saved.round),
                    };
                    self.torrents.insert(id, torrent);
                    self.letting_go(&hash, &letgo::Event::Cleared);
                }
                Err(e) => {
                    warn!(id, error = %e, "couldn't add back a torrent a crash took out of librqbit as it let go of a file");
                    self.store.forget(&hash);
                    self.save();
                }
            }
        }
        self.save_undo();
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
