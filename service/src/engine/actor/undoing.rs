//! The actor's side of Undo ([`undo`]): what a Remove and a Delete put on the stack, and an Undo
//! carried out. An Undo is checked first, on the disk as it is (every item still where it went in
//! the Trash, nothing at its path, the destination there, the torrent not added since); then a
//! torrent still in the list (a partial Delete) is taken out, so every undo brings back one that
//! left; then what Delete moved goes back, on a blocking task the actor never waits on; then the
//! torrent is added again from its stash, under its id. Meanwhile an add of the same torrent waits
//! for it: it's then in the list, or the undo didn't bring it back and the add is a new one.

use std::path::PathBuf;

use anyhow::{Context, anyhow};
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, TorrentStatsState};
use proto::{AbsolutePath, CantUndo, TorrentId, UndoAction, Undoable};
use tokio::sync::oneshot;
use tracing::warn;

use super::super::store::{self, Saved};
use super::super::undo::machine::{self as stack, Command, Observed, World};
use super::super::undo::{self, Back, Entry, Source, Stash};
use super::super::{Failure, Fetched, bitv, files, prepare};
use super::Trashed;
use super::{Actor, Phase, Polled, State, Torrent, inlib, restored_phase, seeding, sorted_trackers, trash};

impl Actor {
    /// Records where an item of torrent `id`'s Delete went, as it goes.
    pub(super) fn went(&mut self, id: TorrentId, moved: Trashed) {
        let Some(hash) = self.torrents.get(&id).map(|t| t.info_hash.clone()) else { return };
        let Some(saved) = self.store.get_mut(&hash) else { return };
        if saved.trashed.iter().all(|t| t.from != moved.from) {
            saved.trashed.push(moved);
            self.save();
        }
    }

    /// Records every move a trashing reported so far; then `message`.
    pub(super) fn went_so_far<T>(&mut self, message: T) -> T {
        while let Ok((id, moved)) = self.went.try_recv() {
            self.went(id, moved);
        }
        message
    }

    /// Ends torrent `id`'s Delete: taken off the list once all of it went. Either way what it moved
    /// can be undone, once it `moved` something: the torrent comes back as it was before its first
    /// Delete, and a partial Delete is undone with it still in the list.
    pub(super) async fn delete_ended(&mut self, id: TorrentId, moved: bool) -> Result<(), Failure> {
        let Some(torrent) = self.torrents.get(&id) else { return Ok(()) };
        let deleted = torrent.lifecycle.phase == Phase::Deleted;
        let (hash, name) = (torrent.info_hash.clone(), name_of(torrent));
        let saved = self.store.get(&hash).cloned();
        let pieces = self.pieces(id);
        let undoable = saved.as_ref().filter(|s| !s.trashed.is_empty() && (deleted || moved));
        let entry = match undoable.map(|s| (s, s.stash.clone())) {
            None => None,
            Some((_, None)) => {
                warn!(id, "nothing kept of the torrent to add it again: its Delete can't be undone");
                None
            }
            Some((saved, Some(stash))) => {
                let back = match deleted {
                    true => Back::Gone { saved: whole(saved.clone()), source: Source::Stash(stash) },
                    false => Back::InList { stash },
                };
                let put_back = saved.trashed.clone();
                Some(Entry { seq: 0, action: UndoAction::Delete, name, info_hash: hash, id, back, put_back })
            }
        };
        // Written ahead: what brings it back is kept before its record goes.
        if let Some(entry) = &entry {
            self.push(entry.clone());
        }
        if deleted {
            self.take_off_list(id).await?;
            if let Some(Entry { back: Back::Gone { source: Source::Stash(stash), .. }, .. }) = &entry {
                self.wait_for_stash(stash, pieces).await;
            }
        }
        Ok(())
    }

    /// Puts `entry` on the undo stack, after every entry before it.
    fn push(&mut self, mut entry: Entry) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let last = self.undo.stack.iter().chain(&self.undo.undoing).map(|e| e.seq).max().unwrap_or(0);
        entry.seq = nanos.max(last + 1);
        self.undo_event(Observed::Done(entry));
    }

    /// Remove: takes torrent `id` off the list, and puts what adds it again on the undo stack, as
    /// undoing `action` (a Delete of one without files is a Remove).
    pub(super) async fn remove(&mut self, id: TorrentId, action: UndoAction) -> anyhow::Result<()> {
        // librqbit stops writing first, so the files are kept aside as they'll stay.
        self.stop_writing(id).await;
        let entry = self.removal(id).map(|entry| Entry { action, ..entry });
        // Written ahead: what brings it back is kept before its record goes.
        if let Some(entry) = &entry {
            self.push(entry.clone());
        }
        self.take_off_for(id, entry.as_ref()).await
    }

    /// Stops librqbit running torrent `id`.
    async fn stop_writing(&mut self, id: TorrentId) {
        if let Some(inlib::State::Has(handle)) = self.torrents.get(&id).map(|t| t.inlib.clone())
            && matches!(handle.stats().state, TorrentStatsState::Live)
            && let Err(e) = self.session.pause(&handle).await
        {
            warn!(id, error = %e, "failed to stop the torrent before removing it");
        }
    }

    /// Takes torrent `id` off the list, `entry` undoing it: a stash made for it holds what librqbit
    /// saves as it drops the torrent.
    async fn take_off_for(&mut self, id: TorrentId, entry: Option<&Entry>) -> anyhow::Result<()> {
        let pieces = self.pieces(id);
        self.take_off_list(id).await?;
        if let Some(Entry { back: Back::Gone { saved, source: Source::Stash(stash) }, .. }) = entry
            && saved.stash.as_ref() != Some(stash)
        {
            self.wait_for_stash(stash, pieces).await;
        }
        Ok(())
    }

    /// What undoes torrent `id`'s Remove: its record, and its stash (the one its Delete made, if
    /// one did), or its magnet link without info.
    fn removal(&self, id: TorrentId) -> Option<Entry> {
        let torrent = self.torrents.get(&id)?;
        let saved = self.store.get(&torrent.info_hash)?.clone();
        let source = match (&saved.stash, torrent.inlib.info()) {
            (Some(stash), _) => Source::Stash(stash.clone()),
            (None, Some(_)) => Source::Stash(self.stash_of(id)?),
            (None, None) => Source::Magnet(undo::magnet(&torrent.info_hash, torrent.name().as_deref(), &torrent.trackers)),
        };
        let back = Back::Gone { saved, source };
        Some(Entry {
            seq: 0,
            action: UndoAction::Remove,
            name: name_of(torrent),
            info_hash: torrent.info_hash.clone(),
            id,
            back,
            put_back: Vec::new(),
        })
    }

    /// Keeps torrent `id` aside, as librqbit has it now.
    pub(super) fn stash_of(&self, id: TorrentId) -> Option<Stash> {
        let torrent = self.torrents.get(&id)?;
        let handle = torrent.inlib.info()?;
        let bytes = handle.with_metadata(|m| m.torrent_bytes.clone()).ok()?;
        let output_folder = handle.output_folder().to_owned();
        let files: Vec<PathBuf> =
            handle.with_metadata(|m| m.file_infos.iter().map(|f| output_folder.join(&f.relative_filename)).collect()).ok()?;
        undo::stash(&self.data_dir, &torrent.info_hash, &bytes, output_folder.clone(), handle.only_files(), handle.id(), &files)
            .inspect_err(|e| warn!(id, error = %e, "failed to keep the torrent aside for Undo"))
            .ok()
    }

    /// How many pieces librqbit has of torrent `id`, at least: none while it checks.
    fn pieces(&self, id: TorrentId) -> u64 {
        let Some(inlib::State::Has(handle)) = self.torrents.get(&id).map(|t| &t.inlib) else { return 0 };
        let stats = handle.stats();
        if matches!(stats.state, TorrentStatsState::Initializing { .. }) {
            return 0;
        }
        handle.with_metadata(|m| m.info.lengths().default_piece_length()).map_or(0, |length| bitv::pieces_in(stats.progress_bytes, length))
    }

    /// Waits for librqbit's last save of the pieces, into the stash it shares a file with.
    async fn wait_for_stash(&self, stash: &Stash, pieces: u64) {
        if stash.bitv().exists() {
            bitv::wait_for(&[(stash.bitv(), pieces)]).await;
        }
    }

    /// Undo: the top of the stack, checked, then carried out; the client hears once it's back. One
    /// asked while another is under way waits its turn (the machine counts it), and takes the
    /// then-top entry; one the stack has no entry for is answered at once as nothing to undo.
    pub(super) async fn undo(&mut self, reply: oneshot::Sender<Result<Undoable, Failure>>) {
        let next = match stack::decide(&self.undo, &stack::Event::Command(Command::Undo)) {
            Ok(next) => next,
            Err(stack::Refusal::Nothing) => return self.answer(reply, Err(proto::Refusal::NothingToUndo.into())),
        };
        let queued = self.undo.undoing.is_some();
        self.undo = next;
        if queued {
            self.undo_queue.push_back(reply);
            return;
        }
        self.run_undos(Some(reply)).await;
    }

    /// Carries out the undo under way, then (it refused or ended before moving anything) the next
    /// queued, each answering its own client: `reply` the first's, the queue's the others'.
    async fn run_undos(&mut self, mut reply: Option<oneshot::Sender<Result<Undoable, Failure>>>) {
        while let Some(entry) = self.undo.undoing.clone() {
            let refused = match self.can_undo(&entry) {
                Err(why) => Some(refused(entry.undoable(), why, Vec::new())),
                Ok(()) => self.take_out_for_undo(entry).await.err(),
            };
            let Some(refused) = refused else {
                // Written ahead: a crash from here on finishes it at the next start.
                self.save_undo();
                self.undo_reply = reply;
                self.resumed = false;
                self.put_back();
                return;
            };
            self.undo_event(Observed::Ended);
            if let Some(reply) = reply.take() {
                self.answer(reply, Err(refused));
            }
            // The next queued undo, if the end started one.
            reply = self.undo_queue.pop_front();
        }
    }

    /// Whether `entry` can be undone now: found before anything moves.
    fn can_undo(&self, entry: &Entry) -> Result<(), CantUndo> {
        let kept = match &entry.back {
            Back::Gone { source: Source::Stash(stash), .. } | Back::InList { stash } => Some(stash),
            Back::Gone { source: Source::Magnet(_), .. } => None,
        };
        if kept.is_some_and(|stash| !stash.readable()) {
            return Err(CantUndo::NotKept);
        }
        let destination = match &entry.back {
            Back::Gone { .. } if self.store.id(&entry.info_hash).is_some() => return Err(CantUndo::InList),
            Back::Gone { saved, .. } => saved.destination.clone(),
            Back::InList { .. } => match self.torrents.get(&entry.id).map(|t| &t.lifecycle.phase) {
                None => return Err(CantUndo::NotInList),
                Some(Phase::DeleteFailed { away: Some(reason), .. }) => return Err(CantUndo::Destination { reason: reason.clone() }),
                Some(Phase::DeleteFailed { away: None, .. }) => self.store.get(&entry.info_hash).and_then(|s| s.destination.clone()),
                // Only a Delete tried again takes a partly deleted torrent elsewhere in the list.
                Some(_) => return Err(CantUndo::Deleting),
            },
        };
        // Without one, nothing says which folders it may make.
        let Some(destination) = destination else { return Err(CantUndo::NoDestination) };
        if let Some(reason) = files::unavailable(&destination) {
            return Err(CantUndo::Destination { reason });
        }
        trash::can_put_back(&entry.put_back, &destination).map_err(blocked)
    }

    /// A partial Delete's torrent, still in the list, is taken out first, so it comes back as any
    /// other that left: with its record, now whole again.
    async fn take_out_for_undo(&mut self, entry: Entry) -> Result<(), Failure> {
        let Back::InList { stash } = &entry.back else { return Ok(()) };
        let saved = self.store.get(&entry.info_hash).cloned().context("a torrent in the list has a record")?;
        let pieces = self.pieces(entry.id);
        // Written ahead: what brings it back is kept before its record goes. A crash after this
        // finds it still in the list, partly deleted, and takes it out then (`resume_undo`).
        let back = Back::Gone { saved: whole(saved), source: Source::Stash(stash.clone()) };
        self.undo.undoing = Some(Entry { back, ..entry.clone() });
        self.save_undo();
        self.take_off_list(entry.id).await?;
        self.wait_for_stash(stash, pieces).await;
        Ok(())
    }

    /// Starts putting back the undo under way, if it isn't already.
    fn put_back(&mut self) {
        let world = if self.putting_back.is_empty() { World::Idle } else { World::Running };
        let Some(stack::Do::PutBack(entry)) = stack::next(&self.undo, &world) else { return };
        let (trash, resumed) = (self.trash.clone(), self.resumed);
        let destination = match &entry.back {
            Back::Gone { saved, .. } => saved.destination.as_ref().map(|d| d.to_path_buf()),
            Back::InList { .. } => None,
        };
        self.putting_back.spawn_blocking(move || {
            let first = entry.put_back.first().map_or_else(|| PathBuf::from("/"), |t| t.from.clone());
            // Checked before it started; one finished after a crash may have lost it since.
            let Some(destination) = destination else {
                return Err((trash::PutBackError::Failed { path: first, detail: "no destination".into() }, Vec::new()));
            };
            let put =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trash::put_back(&entry.put_back, &destination, &trash, resumed)));
            put.unwrap_or_else(|_| Err((trash::PutBackError::Failed { path: first, detail: "putting back stopped".into() }, Vec::new())))
        });
    }

    /// How the undo under way's put-back went: the torrent is added again, or it's refused with
    /// what had moved back in the Trash. Adding it again failing, its files go back to the Trash
    /// too: an undo does all or nothing. Either way it's off the stack.
    pub(super) async fn put_back_ended(&mut self, put: Result<Vec<Trashed>, (trash::PutBackError, Vec<PathBuf>)>, stopping: bool) {
        let entry = self.undo.undoing.clone().expect("an undo under way");
        let answer = match put {
            Err((why, stranded)) => {
                let why = match why {
                    trash::PutBackError::Blocked(b) => blocked(b),
                    trash::PutBackError::Failed { path, detail } => CantUndo::PutBackFailed { path: absolute(path), detail },
                };
                Err(refused(entry.undoable(), why, stranded))
            }
            Ok(back) => {
                let added = match &entry.back {
                    Back::Gone { saved, source } => self.add_back(entry.id, &entry.info_hash, saved.clone(), source.clone()).await,
                    Back::InList { .. } => Err(anyhow!("a partial Delete's torrent is taken out before its undo").into()),
                };
                added.map_err(|failure| {
                    let detail = match &failure {
                        Failure::Refused(r) => format!("{r:?}"),
                        Failure::Error(e) => format!("{e:#}"),
                    };
                    // Only what's back: an item emptied from the Trash while a crash cut the undo short
                    // isn't, and what's at its path now isn't the torrent's.
                    let stranded = trash::take_back(&self.trash, back.iter());
                    refused(entry.undoable(), CantUndo::AddFailed { detail }, stranded)
                })
            }
        };
        // A stop leaves the queued undos unrun: their clients hear the service is stopping.
        if stopping {
            self.undo_event(Observed::Stopping);
            self.undo_queue.clear();
        }
        self.undo_event(Observed::Ended);
        match self.undo_reply.take() {
            Some(reply) => self.answer(reply, answer.map(|()| entry.undoable())),
            None => {
                if let Err(e) = answer {
                    warn!(error = ?e, "failed to finish an undo cut short");
                }
            }
        }
        // The end started the next queued undo, if any.
        let next = self.undo_queue.pop_front();
        self.run_undos(next).await;
    }

    /// Adds torrent `id` again under its id, with its record, from `source`: from a stash,
    /// stopped, with the pieces it had (librqbit only samples them), then run as it's wanted.
    async fn add_back(&mut self, id: TorrentId, hash: &str, saved: Saved, source: Source) -> Result<(), Failure> {
        let stash = match source {
            Source::Magnet(link) => {
                let destination = saved.destination.clone().context("a torrent's record has its destination")?;
                let prepared = prepare(Fetched::Magnet(link), destination)?;
                let wants = saved.wants;
                self.store.restore(hash.to_owned(), saved);
                self.save();
                self.fetch(id, prepared, wants);
                return Ok(());
            }
            Source::Stash(stash) => stash,
        };
        let bytes = stash.torrent()?;
        // The pieces it had hold while its files are as they were; else librqbit checks them all.
        if stash.unchanged() {
            stash.restore_pieces(&bitv::path(&self.data_dir.join("session"), hash))?;
        } else {
            tracing::info!(id, "its files changed since it was kept aside: checked in full");
        }
        let untouched = !saved.delete_failed.is_empty();
        self.guard.set_destination(hash, saved.destination.clone());
        self.guard.keep_untouched(hash, untouched);
        // Its folder and priorities known, as an add again's: the files set Off aren't made (#67),
        // nor those held Off (#199). As at a restart, librqbit has those Normal until a check
        // applies them.
        let getting = store::getting(saved.held.as_ref(), stash.only_files.clone());
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
        let added = match self.session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await {
            Ok(AddTorrentResponse::Added(_, t)) => Ok(t),
            Ok(AddTorrentResponse::AlreadyManaged(_, t)) if t.info_hash().as_string() == hash => Ok(t),
            Ok(_) => Err(anyhow!("librqbit has another torrent under its id")),
            Err(e) => Err(e),
        };
        let handle = match added {
            Ok(handle) => handle,
            Err(e) => {
                self.guard.set_destination(hash, None);
                self.guard.keep_untouched(hash, false);
                return Err(e.context("failed to add the torrent again").into());
            }
        };
        let phase = match (untouched, saved.unchecked) {
            (true, unchecked) => Phase::DeleteFailed { kept: saved.delete_failed.clone(), away: None, unchecked },
            // Its check was cancelled: back as it went, as at a restart, whatever librqbit samples.
            (false, true) => Phase::Unchecked { gone: Default::default() },
            (false, false) => restored_phase(&handle.stats()),
        };
        let torrent = Torrent {
            lifecycle: State::new(saved.wants, phase),
            info_hash: hash.to_owned(),
            trackers: sorted_trackers(&handle),
            polled: Polled::of(&handle.stats(), &Polled::default()),
            inlib: inlib::State::Has(handle),
            meter: seeding::Meter::new(saved.round),
        };
        self.store.restore(hash.to_owned(), saved);
        self.save();
        self.torrents.insert(id, torrent);
        self.poll_torrent(id).await;
        self.size_check(id).await;
        Ok(())
    }

    /// The Removes a crash cut short between keeping what undoes them and their records going
    /// (#162): each torrent still in the list under the id its entry has is taken off it now, its
    /// entry and stash as they were kept. A Delete that moved files (one with items to put back)
    /// is ended as an interrupted Delete instead, before.
    pub(super) async fn finish_removes(&mut self) {
        let cut_short: Vec<Entry> = self
            .undo
            .stack
            .iter()
            .filter(|e| matches!(e.back, Back::Gone { .. }) && e.put_back.is_empty() && self.store.id(&e.info_hash) == Some(e.id))
            .cloned()
            .collect();
        for entry in cut_short {
            let id = entry.id;
            let removed = match self.event(id, super::Event::Command(proto::Action::Remove)).await {
                Ok(()) => {
                    self.stop_writing(id).await;
                    self.take_off_for(id, Some(&entry)).await.map_err(Failure::from)
                }
                Err(e) => Err(e),
            };
            if let Err(e) = removed {
                warn!(id, error = ?e, "failed to finish a Remove cut short");
            }
        }
    }

    /// An undo a crash cut short: done, if its torrent made it back; else what's still in the
    /// Trash goes back, and it's added again, or, one item blocked, what's back goes to the Trash
    /// again ([`trash::put_back`]).
    pub(super) async fn resume_undo(&mut self) {
        let Some(entry) = self.undo.undoing.clone() else { return };
        if let Some(id) = self.store.id(&entry.info_hash) {
            // A partial Delete's torrent the crash left in the list: taken out now, then on. Any
            // other in the list made it back.
            let partly_deleted = self.store.get(&entry.info_hash).is_some_and(|s| !s.delete_failed.is_empty());
            let half_taken = id == entry.id && entry.action == UndoAction::Delete && partly_deleted;
            if !half_taken {
                self.undo_event(Observed::Ended);
                return;
            }
            if let Err(e) = self.take_off_list(id).await {
                warn!(id, error = %e, "failed to take out a partly deleted torrent to finish its undo");
                self.undo_event(Observed::Ended);
                return;
            }
        }
        self.resumed = true;
        self.put_back();
    }

    /// Whether the undo under way is bringing back the torrent with this info hash.
    pub(in super::super) fn coming_back(&self, info_hash: &str) -> bool {
        self.undo.undoing.as_ref().is_some_and(|e| e.info_hash == info_hash)
    }

    pub(super) fn undo_event(&mut self, observed: Observed) {
        let ended = observed == Observed::Ended;
        self.undo = stack::decide(&self.undo, &stack::Event::Observed(observed)).expect("an observation is never refused");
        self.save_undo();
        if ended {
            for (prepared, paused, reply) in std::mem::take(&mut self.adds_waiting) {
                self.add(prepared, paused, reply);
            }
        }
    }

    /// Keeps the stack, and lets go of the stashes nothing names any more.
    pub(super) fn save_undo(&self) {
        if let Err(e) = undo::save(&self.data_dir, &self.undo) {
            warn!(error = %e, "failed to save the undo stack");
        }
        undo::tidy(&self.data_dir, &self.undo, self.store.iter().map(|(_, saved)| saved));
    }
}

fn refused(undo: Undoable, why: CantUndo, stranded: Vec<PathBuf>) -> Failure {
    Failure::Refused(proto::Refusal::Undo { undo, why, stranded: stranded.into_iter().map(absolute).collect() })
}

fn absolute(path: PathBuf) -> AbsolutePath {
    AbsolutePath::new(path).expect("a torrent's paths are absolute")
}

fn blocked(blocked: trash::Blocked) -> CantUndo {
    match blocked {
        trash::Blocked::NotInTrash(p) => CantUndo::NotInTrash { path: absolute(p) },
        trash::Blocked::Taken(p) => CantUndo::PathTaken { path: absolute(p) },
        trash::Blocked::NotAFolder(p) => CantUndo::NotAFolder { path: absolute(p) },
    }
}

/// A torrent's record as it was before its Delete: nothing in the Trash, nothing kept aside.
fn whole(mut saved: Saved) -> Saved {
    saved.delete_failed.clear();
    saved.deleting.clear();
    saved.trashed.clear();
    saved.stash = None;
    saved
}

fn name_of(torrent: &Torrent) -> String {
    torrent.name().unwrap_or_else(|| torrent.info_hash.clone())
}
