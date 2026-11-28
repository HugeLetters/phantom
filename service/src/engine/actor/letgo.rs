//! Letting go of a file (`machine::letgo`): one set Off that's gone from its path (moved to the
//! Trash by a Move to Trash, ADR 0014; deleted or moved away by hand, #276) is written ahead in its
//! torrent's record, then closed. Once the torrent can be taken out of librqbit, it's kept aside
//! (librqbit's `.bitv`, as for Undo), taken out of librqbit, and what librqbit saved of its pieces
//! is put back without those files' pieces; then it's added back as any torrent Ready and out of
//! librqbit is (#45, [`lib::reconcile`]): stopped, with the pieces it had, resuming
//! ([`lib::Lib::Resuming`]): its lifecycle hears nothing until librqbit has read the few pieces it
//! samples, so its row stays as it was, then it runs as wanted. Its peers drop once and connect
//! again, and nothing offers those pieces any more: librqbit can't withdraw a piece it has, and
//! drops a peer whose piece it can't read.
//!
//! The files a torrent misses or lets go of are looked at off the actor, in a thread per torrent
//! with a timeout, as the destinations' disks are (`looks.rs`): one deleted for good is closed
//! there, so its space is freed (#244).
//!
//! A crash between the steps is finished at the next start ([`clear_at_start`]), before any torrent
//! is added: its pieces are cleared in what librqbit saved, or put back from what was kept.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use librqbit::TorrentStatsState;
use proto::TorrentId;
use tracing::warn;

use super::super::bitv;
use super::super::looks;
use super::super::machine::letgo::{self, Pieces};
use super::super::store::Record;
use super::super::trashed::Trashed;
use super::super::undo::Stash;
use super::{Actor, Phase, lib};

/// Where a test kills the engine while it lets go of a file: once it's closed, as the torrent is
/// taken out of librqbit, or once it's out, before it's added back.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CrashAt {
    Closing,
    TakingOut,
    AddingBack,
}

/// The torrents whose open files are looked at, by info hash, each with the files it's missing or
/// lets go of (by index), whose handles are closed if they're deleted for good.
pub(in super::super) type Wanted = BTreeMap<String, BTreeSet<usize>>;

/// Looks at the torrents `wanted` names every `every`, each in a thread of its own (an `fstat` of
/// an open file can hang on a network volume that stops answering), while `listening`: each look
/// closes the files the torrent is missing (or lets go of) that were deleted by hand, there and
/// then, so their space is freed at once, and the actor never touches the disk for it.
pub(super) fn watch(
    trashed: Trashed,
    wanted: tokio::sync::watch::Receiver<Wanted>,
    every: Duration,
    listening: impl Fn() -> bool + Send + Sync + 'static,
) {
    let look: Arc<dyn Fn(&String, &BTreeSet<usize>) -> BTreeSet<usize> + Send + Sync> =
        Arc::new(move |hash, closable| trashed.look(hash, closable));
    looks::watch(look, wanted, every, move |_, _| listening());
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
        let Some(state) = self.torrents.get(&id).map(|t| t.record.letting_go.clone()) else { return };
        // Closed already: by a Move to Trash, or as a size check found it ([`Actor::off_gone`]).
        #[cfg(test)]
        if matches!(state, letgo::State::Gone { .. }) && self.crashes_at(CrashAt::Closing).await {
            return;
        }
        #[cfg(test)]
        if self.letting_go_held {
            return;
        }
        // Taken out and added back only whole: a file it gets that went meanwhile is Missing files
        // first, or librqbit would make it again.
        if state != letgo::State::Holding && self.free(id) {
            self.size_check(id).await;
        }
        let world = letgo::World::Running { free: self.free(id), offered: self.offered(id, &state) };
        match letgo::next(&state, world) {
            None => {}
            Some(letgo::Do::End) => drop(self.letting_go(id, &letgo::Event::Cleared)),
            Some(letgo::Do::Stash) => match self.stash_of(id) {
                Some(stash) => {
                    let stashed = self.letting_go(id, &letgo::Event::Stashed(stash));
                    if let letgo::State::Stashed { pieces, stash } = stashed {
                        self.add_back_without(id, &pieces, &stash).await;
                    }
                }
                None => warn!(id, "couldn't keep the torrent aside to let go of a file: tried again later"),
            },
            Some(letgo::Do::AddBack { pieces, stash }) => self.add_back_without(id, &pieces, &stash).await,
            // Only at a start, before any torrent is added.
            Some(letgo::Do::ClearSaved { .. }) => {}
        }
    }

    /// The files of torrent `id` set Off that librqbit has some of and that aren't at their paths any
    /// more (moved away by hand, to Finder's Trash say, or deleted), as the size check finds them.
    /// Each is let go of, written ahead, then closed, as one a Move to Trash took is (#276): librqbit would count and offer its
    /// pieces, read through the file it has open, or not at all once that's closed. Only while it's
    /// Ready or Missing files with nothing on its lane: a Delete, a Move to Trash or a move takes
    /// files away on purpose. As the size check found them, which looks at every file.
    pub(super) fn off_gone(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let idle = self.steps.machine().lanes.get(&id).is_none_or(|lane| lane.is_empty());
        if !idle || !matches!(torrent.lifecycle.phase, Phase::Ready | Phase::MissingFiles { .. }) {
            return;
        }
        let (record, have) = (&torrent.record, super::progress(torrent));
        let Some(info) = &record.info else { return };
        let letting = record.letting_go.files();
        let pieces: Pieces = info
            .files
            .iter()
            .enumerate()
            .filter(|(i, f)| !f.padding && !record.gets(*i) && !letting.contains(i))
            .filter(|(i, _)| have.get(*i).is_some_and(|had| *had > 0))
            .filter(|(i, _)| !torrent.there.contains(i))
            .map(|(i, f)| (i, (f.pieces.start, f.pieces.end)))
            .collect();
        if pieces.is_empty() {
            return;
        }
        let (hash, gone) = (record.info_hash.clone(), pieces.keys().copied().collect());
        self.letting_go(id, &letgo::Event::Found(pieces));
        // Never waits: one librqbit is reading now is closed as it's added back.
        self.guard.trashed().let_go(&hash, &gone);
    }

    /// The files `files` (by index) of torrent `id` a Move to Trash took, closed as they went (ADR
    /// 0014): their pieces are let go of, written ahead, so nothing reads or offers them from the
    /// Trash. Its undo checks the torrent, which reads them back at their paths.
    pub(super) fn let_go_of_trashed(&mut self, id: TorrentId, files: &BTreeSet<usize>) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(info) = &torrent.record.info else { return };
        let pieces: Pieces = files
            .iter()
            .filter(|i| !torrent.record.gets(**i))
            .filter_map(|&i| Some((i, info.files.get(i).filter(|f| !f.padding)?.pieces.clone())))
            .map(|(i, r)| (i, (r.start, r.end)))
            .collect();
        if !pieces.is_empty() {
            self.letting_go(id, &letgo::Event::Found(pieces));
        }
    }

    /// Whether librqbit has any piece of the files torrent `id` lets go of: a check, or an add again,
    /// since may have dropped them all, and one out of librqbit offers none (it's checked in full
    /// when it comes back). One set Normal again counts with none had too: what librqbit has open of
    /// it isn't the file at its path, and only an add again opens that (#402).
    /// So does one set Off since with a file at its path: set Normal yet again, it's downloaded there.
    fn offered(&self, id: TorrentId, state: &letgo::State) -> bool {
        let files = match state {
            letgo::State::Holding => return false,
            letgo::State::Gone { pieces } | letgo::State::Stashed { pieces, .. } => pieces.keys(),
        };
        let Some(torrent) = self.torrents.get(&id) else { return false };
        let Some(handle) = torrent.lib.torrent() else { return false };
        let progress = handle.stats().file_progress;
        files.into_iter().any(|i| torrent.record.gets(*i) || torrent.there.contains(i) || progress.get(*i).is_none_or(|had| *had > 0))
    }

    /// Whether torrent `id` can be taken out of librqbit now: librqbit has it, not checking it, it's
    /// one librqbit runs or holds stopped as it is, and nothing runs on its lane.
    fn free(&self, id: TorrentId) -> bool {
        let Some(torrent) = self.torrents.get(&id) else { return false };
        let in_librqbit = match &torrent.lib {
            lib::Lib::In(handle) => !matches!(handle.stats().state, TorrentStatsState::Initializing { .. }),
            lib::Lib::Adding(_) | lib::Lib::Resuming(_) | lib::Lib::Out => false,
        };
        let phase = match &torrent.lifecycle.phase {
            Phase::Ready => true,
            // A check reads what's there; out of reach, missing files, unchecked or an error, it's
            // out of librqbit, and checked once it's back; being deleted, or partly, librqbit's
            // storage of it touches nothing.
            Phase::FetchingInfo
            | Phase::FetchFailed { .. }
            | Phase::CheckDue { .. }
            | Phase::Checking { .. }
            | Phase::Unchecked { .. }
            | Phase::MissingFiles { .. }
            | Phase::DestinationUnavailable { .. }
            | Phase::Error { .. }
            | Phase::DeleteFailed { .. }
            | Phase::Deleting { .. }
            | Phase::Deleted
            | Phase::Moving { .. } => false,
        };
        let idle = self.steps.machine().lanes.get(&id).is_none_or(|lane| lane.is_empty());
        in_librqbit && phase && idle
    }

    /// `event` for torrent `id`'s let-go, kept in its record before anything follows from it.
    fn letting_go(&mut self, id: TorrentId, event: &letgo::Event) -> letgo::State {
        let Some(torrent) = self.torrents.get_mut(&id) else { return letgo::State::Holding };
        match event {
            letgo::Event::Found(pieces) => torrent.record.let_go(pieces.clone()),
            letgo::Event::Stashed(_) | letgo::Event::Cleared | letgo::Event::Failed => {
                torrent.record.letting_go = letgo::decide(&torrent.record.letting_go, event);
            }
        }
        let now = torrent.record.letting_go.clone();
        self.save();
        now
    }

    /// Takes torrent `id` out of librqbit, kept aside in `stash`, and puts back what librqbit saved
    /// of its pieces without `pieces`: then it's added back as any torrent Ready and out of librqbit
    /// is, resuming. Out of librqbit already (a crash cut it short, and putting its pieces back
    /// failed at the start), only its pieces are put back. Until they are, it's tried again at the
    /// next look. Adding it back failing, it's an Error, as an add that fails is, until Resume or
    /// Check adds it again.
    async fn add_back_without(&mut self, id: TorrentId, pieces: &Pieces, stash: &Stash) {
        match self.torrents.get(&id).map(|t| &t.lib) {
            Some(lib::Lib::In(_)) => {
                let had = self.pieces(id);
                #[cfg(test)]
                if self.crashes_at(CrashAt::TakingOut).await {
                    return;
                }
                if let Err(e) = self.take_out(id).await {
                    return warn!(id, error = %e, "couldn't take the torrent out of librqbit to let go of a file");
                }
                // librqbit's last save of its pieces lands in the stash, which shares the file.
                bitv::wait_for(&[(stash.bitv(), had)]).await;
                #[cfg(test)]
                if self.crashes_at(CrashAt::AddingBack).await {
                    return;
                }
            }
            Some(lib::Lib::Out) => {}
            Some(lib::Lib::Adding(_) | lib::Lib::Resuming(_)) | None => return,
        }
        let hash = self.torrents[&id].record.info_hash.clone();
        if let Err(e) = without(stash, pieces, &bitv::path(&self.session_folder(), &hash)) {
            warn!(id, error = %e, "couldn't put back the pieces of a torrent letting go of a file: tried again later");
            return;
        }
        // Listed with what it's added back with while librqbit samples it, not what it had.
        self.polled_as_saved(id);
        let added = self.carry_out(id).await;
        if let Err(e) = &added {
            warn!(id, error = ?e, "couldn't add the torrent back once it let go of a file");
        }
        self.letting_go(id, if added.is_ok() { &letgo::Event::Cleared } else { &letgo::Event::Failed });
        // The stash isn't named any more.
        self.save_undo();
    }

    /// Finishes torrent `id`'s let-go a crash cut short, if it's waiting to: before a Remove or a
    /// Delete keeps it aside, so it's kept with the pieces it has.
    pub(super) async fn finish_letting_go(&mut self, id: TorrentId) {
        if matches!(self.torrents.get(&id).map(|t| &t.record.letting_go), Some(letgo::State::Stashed { .. })) {
            self.let_go_of(id).await;
        }
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

/// Writes ahead the let-go of the files of `record` set Off, with pieces in `saved`, with nothing at
/// their paths; `next` then clears them. A destination out of reach says nothing of its files.
fn off_gone_at_start(record: &mut Record, saved: &Path) {
    let Some(info) = &record.info else { return };
    if !matches!(record.letting_go, letgo::State::Holding) || crate::engine::files::unavailable(&record.destination).is_some() {
        return;
    }
    let Ok(bits) = std::fs::read(saved) else { return };
    let lens: Vec<u64> = info.files.iter().map(|f| f.len).collect();
    let had = bitv::file_progress(&bits, &lens, info.piece_length);
    let paths = record.paths();
    let pieces: Pieces = info
        .files
        .iter()
        .enumerate()
        .filter(|(i, f)| !f.padding && !record.gets(*i) && had[*i] > 0)
        .filter(|(i, _)| paths.get(*i).is_some_and(|p| crate::engine::files::stamp(p).is_none()))
        .map(|(i, f)| (i, (f.pieces.start, f.pieces.end)))
        .collect();
    if !pieces.is_empty() {
        record.let_go(pieces);
    }
}

/// Puts what `stash` kept of a torrent's pieces where librqbit looks for them (`saved`), without
/// `pieces`; with none of them kept, librqbit checks it in full.
fn without(stash: &Stash, pieces: &Pieces, saved: &Path) -> anyhow::Result<()> {
    bitv::clear(&stash.bitv(), pieces.values().copied()).context("failed to clear the pieces kept aside")?;
    stash.restore_pieces(saved)
}

/// At a start, before any torrent is added to librqbit, whose pieces it saved in `session`: the
/// let-go of each torrent a crash cut short is finished there. Its pieces are cleared in what
/// librqbit saved, or, taken out of librqbit already, put back from what was kept without them.
/// Either way it's then added as any torrent is, with the pieces it has.
///
/// A file set Off that librqbit saved pieces of and that's no longer at its path (deleted, moved
/// away, or moved to the Trash by a Move to Trash a crash cut short) is let go of here too (#276),
/// as `off_gone` does at a size check: librqbit would offer them until then.
pub(in super::super) fn clear_at_start(records: &mut [Record], session: &Path) {
    for record in records {
        let saved = bitv::path(session, &record.info_hash);
        off_gone_at_start(record, &saved);
        let cleared = match letgo::next(&record.letting_go, letgo::World::Starting { saved: saved.exists() }) {
            None => continue,
            Some(letgo::Do::ClearSaved { pieces }) => bitv::clear(&saved, pieces.values().copied()).map_err(anyhow::Error::from),
            Some(letgo::Do::AddBack { pieces, stash }) => without(&stash, &pieces, &saved),
            Some(letgo::Do::Stash | letgo::Do::End) => unreachable!("only while running"),
        };
        match cleared {
            Ok(()) => record.letting_go = letgo::decide(&record.letting_go, &letgo::Event::Cleared),
            // librqbit samples what it saved, and checks it in full on a piece of a file that's gone.
            Err(e) => warn!(hash = record.info_hash, error = %e, "couldn't clear the pieces of a file let go of"),
        }
    }
}
