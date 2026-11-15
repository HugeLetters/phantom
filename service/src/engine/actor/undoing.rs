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

use super::super::store::Saved;
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
        if deleted {
            self.take_off_list(id).await?;
        }
        let Some(saved) = saved.filter(|s| !s.trashed.is_empty() && (deleted || moved)) else { return Ok(()) };
        let Some(stash) = saved.stash.clone() else {
            warn!(id, "nothing kept of the torrent to add it again: its Delete can't be undone");
            return Ok(());
        };
        let back = if deleted {
            self.wait_for_stash(&stash, pieces).await;
            Back::Gone { saved: whole(saved.clone()), source: Source::Stash(stash) }
        } else {
            Back::InList { stash }
        };
        self.undo_event(Observed::Done(Entry { action: UndoAction::Delete, name, info_hash: hash, id, back, put_back: saved.trashed }));
        Ok(())
    }

    /// Remove: takes torrent `id` off the list, and puts what adds it again on the undo stack, as
    /// undoing `action` (a Delete of one without files is a Remove).
    pub(super) async fn remove(&mut self, id: TorrentId, action: UndoAction) -> anyhow::Result<()> {
        let entry = self.removal(id).map(|entry| Entry { action, ..entry });
        let pieces = self.pieces(id);
        self.take_off_list(id).await?;
        let Some(entry) = entry else { return Ok(()) };
        // A stash made now holds what librqbit saves as it drops the torrent.
        if let Back::Gone { saved, source: Source::Stash(stash) } = &entry.back
            && saved.stash.as_ref() != Some(stash)
        {
            self.wait_for_stash(stash, pieces).await;
        }
        self.undo_event(Observed::Done(entry));
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
        undo::stash(&self.data_dir, &torrent.info_hash, &bytes, output_folder, handle.only_files(), handle.id())
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

    /// Undo: the top of the stack, checked, then carried out; the client hears once it's back.
    pub(super) async fn undo(&mut self, reply: oneshot::Sender<Result<Undoable, Failure>>) {
        let next = match stack::decide(&self.undo, &stack::Event::Command(Command::Undo)) {
            Ok(next) => next,
            Err(stack::Refusal::Nothing) => return self.answer(reply, Err(proto::Refusal::NothingToUndo.into())),
            Err(stack::Refusal::UnderWay) => {
                let undo = self.undo.undoing.as_ref().expect("one under way").undoable();
                return self.answer(reply, Err(proto::Refusal::Undo { undo, why: CantUndo::UnderWay }.into()));
            }
        };
        self.undo = next;
        let entry = self.undo.undoing.clone().expect("just taken");
        if let Err(why) = self.can_undo(&entry) {
            self.undo_event(Observed::Ended);
            return self.answer(reply, Err(proto::Refusal::Undo { undo: entry.undoable(), why }.into()));
        }
        if let Err(e) = self.take_out_for_undo(entry).await {
            self.undo_event(Observed::Ended);
            return self.answer(reply, Err(e));
        }
        // Written ahead: a crash from here on finishes it at the next start.
        self.save_undo();
        self.undo_reply = Some(reply);
        self.resumed = false;
        self.put_back();
    }

    /// Whether `entry` can be undone now: found before anything moves.
    fn can_undo(&self, entry: &Entry) -> Result<(), CantUndo> {
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
        if let Some(reason) = destination.as_ref().and_then(files::unavailable) {
            return Err(CantUndo::Destination { reason });
        }
        let path = |p: PathBuf| AbsolutePath::new(p).expect("a torrent's paths are absolute");
        trash::can_put_back(&entry.put_back).map_err(|blocked| match blocked {
            trash::Blocked::NotInTrash(p) => CantUndo::NotInTrash { path: path(p) },
            trash::Blocked::Taken(p) => CantUndo::PathTaken { path: path(p) },
        })
    }

    /// A partial Delete's torrent, still in the list, is taken out first, so it comes back as any
    /// other that left: with its record, now whole again.
    async fn take_out_for_undo(&mut self, entry: Entry) -> Result<(), Failure> {
        let Back::InList { stash } = &entry.back else { return Ok(()) };
        let saved = self.store.get(&entry.info_hash).cloned().context("a torrent in the list has a record")?;
        let pieces = self.pieces(entry.id);
        self.take_off_list(entry.id).await?;
        self.wait_for_stash(stash, pieces).await;
        let back = Back::Gone { saved: whole(saved), source: Source::Stash(stash.clone()) };
        self.undo.undoing = Some(Entry { back, ..entry });
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
        let destination = destination.unwrap_or_else(|| PathBuf::from("/"));
        self.putting_back.spawn_blocking(move || {
            let put =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trash::put_back(&entry.put_back, &destination, &trash, resumed)));
            let first = entry.put_back.first().map_or_else(|| PathBuf::from("/"), |t| t.from.clone());
            put.unwrap_or_else(|_| Err((first, "putting back stopped".into())))
        });
    }

    /// How the undo under way's put-back went: the torrent is added again, or it's refused with
    /// what had moved back in the Trash. Either way it's off the stack.
    pub(super) async fn put_back_ended(&mut self, put: Result<(), (PathBuf, String)>) {
        let entry = self.undo.undoing.clone().expect("an undo under way");
        let answer = match put {
            Err((path, detail)) => {
                let path = AbsolutePath::new(path).expect("a torrent's paths are absolute");
                Err(proto::Refusal::Undo { undo: entry.undoable(), why: CantUndo::PutBackFailed { path, detail } }.into())
            }
            Ok(()) => match &entry.back {
                Back::Gone { saved, source } => self.add_back(entry.id, &entry.info_hash, saved.clone(), source.clone()).await,
                Back::InList { .. } => Err(anyhow!("a partial Delete's torrent is taken out before its undo").into()),
            },
        };
        self.undo_event(Observed::Ended);
        match self.undo_reply.take() {
            Some(reply) => self.answer(reply, answer.map(|()| entry.undoable())),
            None => {
                if let Err(e) = answer {
                    warn!(error = ?e, "failed to finish an undo cut short");
                }
            }
        }
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
        stash.restore_pieces(&bitv::path(&self.data_dir.join("session"), hash))?;
        let untouched = !saved.delete_failed.is_empty();
        self.guard.set_destination(hash, saved.destination.clone());
        self.guard.keep_untouched(hash, untouched);
        // Its folder and priorities known, as an add again's: the files set Off aren't made (#67).
        let storage = self.guard.for_add(stash.output_folder.clone(), stash.only_files.as_deref());
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
        let phase = match untouched {
            true => Phase::DeleteFailed { kept: saved.delete_failed.clone(), away: None },
            false => restored_phase(&handle.stats()),
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

    /// An undo a crash cut short: done, if its torrent made it back; else what's still in the
    /// Trash goes back, and it's added again.
    pub(super) async fn resume_undo(&mut self) {
        let Some(entry) = &self.undo.undoing else { return };
        if self.store.id(&entry.info_hash).is_some() {
            self.undo_event(Observed::Ended);
            return;
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
