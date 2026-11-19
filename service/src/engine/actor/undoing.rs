//! The actor's side of Undo ([`undo`]): what a Remove and a Delete put on the stack, and an undo
//! carried out, on its torrent's lane (`steps.rs`), side by side with other torrents'. An undo is
//! checked first, on the disk as it is (every item still where it went in the Trash, nothing at its
//! path, the destination there, the torrent not added since); then a torrent still in the list (a
//! partial Delete) is taken out, so every undo brings back one that left; then what Delete moved
//! goes back, on a blocking task the actor never waits on; then the torrent is added again from its
//! stash, under its id. Meanwhile an add of the same torrent waits for it: it's then in the list,
//! or the undo didn't bring it back and the add is a new one.

use std::path::PathBuf;

use anyhow::{Context, anyhow};
use librqbit::TorrentStatsState;
use proto::{AbsolutePath, CantUndo, Priority, TorrentId, UndoAction, Undoable};
use tracing::warn;

use super::super::machine::letgo;
use super::super::store::{Info, Record, Saved};
use super::super::undo::machine::{Phase as StepPhase, What};
use super::super::undo::{self, Back, Entry, Source, Stash};
use super::super::{Failure, Fetched, bitv, files, prepare, trashed};
use super::steps::{Ended, Over};
use super::{Actor, Phase, Polled, PutBack, State, Torrent, Trashed, lib, seeding, trash};

impl Actor {
    /// Records where an item of torrent `id`'s Delete went, as it goes.
    pub(super) fn went(&mut self, id: TorrentId, moved: Trashed) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        if torrent.record.trashed.iter().all(|t| t.from != moved.from) {
            torrent.record.trashed.push(moved);
            self.save();
        }
    }

    /// Records every move a trashing reported so far; then `message`.
    pub(super) fn went_so_far<T>(&mut self, message: T) -> T {
        while let Ok((id, moved)) = self.went.try_recv() {
            self.went(id, moved);
        }
        while let Ok((seq, id, moved)) = self.files_went.try_recv() {
            self.files_went(seq, id, moved);
        }
        message
    }

    /// Ends torrent `id`'s Delete, step `seq` (none: one ended at a start that no step on the stack
    /// is running for): taken off the list once all of it went. Either way what it moved can be
    /// undone, once it `moved` something: the torrent comes back as it was before its first Delete,
    /// and a partial Delete is undone with its torrent still in the list. Nothing moved, the step is gone.
    /// Its step over, for its client to hear how it went (none without a step); and how taking it
    /// off the list went.
    pub(super) async fn delete_ended(&mut self, id: TorrentId, moved: bool, seq: Option<u64>) -> (Option<Ended>, Result<(), Failure>) {
        let Some(torrent) = self.torrents.get(&id) else {
            return (seq.map(|seq| self.end(seq, id, Over::Failed)), Ok(()));
        };
        let deleted = torrent.lifecycle.phase == Phase::Deleted;
        let (hash, name) = (torrent.record.info_hash.clone(), name_of(torrent));
        let saved = Some(torrent.kept());
        let pieces = self.pieces(id);
        // Taken off the list, it changed something even with nothing to move (#190): undone, it's
        // back as a Remove's would be.
        let undoable = saved.as_ref().filter(|s| deleted || (moved && !s.trashed.is_empty()));
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
                // Listed as before its first Delete: a partial one's, if it takes its place.
                let partial = self.steps.machine().stack.iter().filter_map(|p| p.done.as_ref()?.get(&id)).find_map(|e| match e.action {
                    UndoAction::Delete => e.listed.clone(),
                    UndoAction::Remove | UndoAction::TrashFiles { .. } => None,
                });
                let partial = partial.or_else(|| self.action_part(seq?, id)?.listed.clone());
                Some(Entry { action: UndoAction::Delete, name, info_hash: hash, id, back, put_back, listed: partial })
            }
        };
        // Ended again at a start after a crash, its entry on the stack already.
        let committed = |e: &Entry| e.id == id && e.action == UndoAction::Delete && matches!(e.back, Back::Gone { .. });
        let already =
            seq.is_none() && self.steps.machine().stack.iter().any(|p| p.done.as_ref().is_some_and(|d| d.values().any(committed)));
        // Written ahead: what brings it back is kept before its record goes.
        let ended = match (&entry, seq) {
            (Some(entry), Some(seq)) => Some(self.end(seq, id, Over::Finished(Some(entry.clone())))),
            (Some(_), None) if already => None,
            (Some(entry), None) => {
                self.commit_new(entry.clone());
                None
            }
            (None, Some(seq)) => Some(self.end(seq, id, Over::Failed)),
            (None, None) => None,
        };
        if deleted {
            if let Err(e) = self.take_off_list(id).await {
                return (ended, Err(e.into()));
            }
            if let Some(Entry { back: Back::Gone { source: Source::Stash(stash), .. }, .. }) = &entry {
                self.wait_for_stash(stash, pieces).await;
            }
        }
        (ended, Ok(()))
    }

    /// Remove, step `seq`: takes torrent `id` off the list, and what adds it again ends the step,
    /// as undoing `action` (a Delete of one without files is a Remove): the step over, for its client
    /// to hear how taking it off went.
    pub(super) async fn remove(&mut self, id: TorrentId, action: UndoAction, seq: u64) -> (Ended, anyhow::Result<()>) {
        // librqbit stops writing first, so the files are kept aside as they'll stay, with the
        // pieces it has.
        self.stop_writing(id).await;
        self.finish_letting_go(id).await;
        let listed = self.action_part(seq, id).and_then(|r| r.listed.clone());
        let entry = self.removal(id).map(|entry| Entry { action, listed, ..entry });
        // Written ahead: what brings it back is kept before its record goes.
        let ended = match &entry {
            Some(entry) => self.end(seq, id, Over::Finished(Some(entry.clone()))),
            None => self.end(seq, id, Over::Failed),
        };
        (ended, self.take_off_for(id, entry.as_ref()).await)
    }

    /// Stops librqbit running torrent `id`.
    async fn stop_writing(&mut self, id: TorrentId) {
        if let Some(lib::Lib::In(handle)) = self.torrents.get(&id).map(|t| &t.lib)
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
        let record = &torrent.record;
        let source = match (&record.stash, record.info.is_some()) {
            (Some(stash), _) => Source::Stash(stash.clone()),
            (None, true) => Source::Stash(self.stash_of(id)?),
            (None, false) => Source::Magnet(undo::magnet(&record.info_hash, torrent.name().as_deref(), &record.trackers)),
        };
        let back = Back::Gone { saved: torrent.kept(), source };
        Some(Entry {
            action: UndoAction::Remove,
            name: name_of(torrent),
            info_hash: record.info_hash.clone(),
            id,
            back,
            put_back: Vec::new(),
            listed: None,
        })
    }

    /// Keeps torrent `id` aside: its .torrent, from its record, and the pieces librqbit saved of it.
    pub(super) fn stash_of(&self, id: TorrentId) -> Option<Stash> {
        let record = &self.torrents.get(&id)?.record;
        let info = record.info.as_ref()?;
        undo::stash(&self.data_dir, &record.info_hash, &info.bytes, &record.paths())
            .inspect_err(|e| warn!(id, error = %e, "failed to keep the torrent aside for Undo"))
            .ok()
    }

    /// How many pieces librqbit has of torrent `id`, at least: none while it checks; while it
    /// resumes, those it was added back with.
    pub(super) fn pieces(&self, id: TorrentId) -> u64 {
        let Some(torrent) = self.torrents.get(&id) else { return 0 };
        let handle = match &torrent.lib {
            lib::Lib::In(handle) => handle,
            lib::Lib::Resuming(_) => {
                let saved = bitv::path(&self.session_folder(), &torrent.record.info_hash);
                return std::fs::read(saved).map_or(0, |bits| bitv::pieces(&bits));
            }
            lib::Lib::Adding(_) | lib::Lib::Out => return 0,
        };
        let stats = handle.stats();
        let Some(info) = &torrent.record.info else { return 0 };
        if matches!(stats.state, TorrentStatsState::Initializing { .. }) {
            return 0;
        }
        bitv::pieces_in(stats.progress_bytes, info.piece_length)
    }

    /// Waits for librqbit's last save of the pieces, into the stash it shares a file with.
    async fn wait_for_stash(&self, stash: &Stash, pieces: u64) {
        if stash.bitv().exists() {
            bitv::wait_for(&[(stash.bitv(), pieces)]).await;
        }
    }

    /// Starts undo step `seq`, of action `of`, `entry`: checked on the disk as it is, then carried
    /// out; one a crash cut short goes on as it was. Refused, it's over and its client hears why.
    pub(super) async fn start_undo(&mut self, seq: u64, of: u64, entry: Entry) {
        let id = entry.id;
        if self.resumed.contains(&(seq, id)) {
            return self.resume_undo(seq, of, entry).await;
        }
        let refused = match self.can_undo(&entry) {
            Err(why) => Some(refused(entry.undoable(of), why, Vec::new())),
            Ok(()) => self.take_out_for_undo(seq, &entry).await.err(),
        };
        if let Some(refused) = refused {
            return self.undo_ended(seq, id, Err(refused));
        }
        // Written ahead: a crash from here on finishes it at the next start.
        self.save_undo();
        self.put_back(seq, id);
    }

    /// Undo step `seq`'s part on torrent `id`: the action it's of, and what it puts back.
    fn undoing(&self, seq: u64, id: TorrentId) -> Option<(u64, Entry)> {
        match &self.steps.machine().steps.get(&seq)?.what {
            What::Undo { of, parts } => Some((*of, parts.get(&id)?.doing()?.clone())),
            What::Action(_) | What::Command(_) => None,
        }
    }

    /// Undo step `seq` is over, carried out or not: its client hears how it went, and its lanes go on.
    pub(super) fn undo_ended(&mut self, seq: u64, id: TorrentId, answer: Result<Undoable, Failure>) {
        self.resumed.remove(&(seq, id));
        // Failed for no reason of Undo's: still refused, with the failure as its reason (#200).
        let over = match &answer {
            Ok(_) => Over::Finished(None),
            Err(failure) => {
                let (why, stranded) = failure.cant_undo();
                Over::Refused(why, stranded)
            }
        };
        let ended = self.end(seq, id, over);
        self.answer_step(ended, super::steps::Outcome::Undone(answer));
    }

    /// Whether `entry` can be undone now: found before anything moves.
    fn can_undo(&self, entry: &Entry) -> Result<(), CantUndo> {
        let kept = match &entry.back {
            Back::Gone { source: Source::Stash(stash), .. } | Back::InList { stash } => Some(stash),
            Back::Gone { source: Source::Magnet(_), .. } | Back::Files { .. } => None,
        };
        if kept.is_some_and(|stash| !stash.readable()) {
            return Err(CantUndo::NotKept);
        }
        let in_list = self.by_hash(&entry.info_hash);
        let destination = match &entry.back {
            Back::Gone { .. } if in_list.is_some() => return Err(CantUndo::InList),
            Back::Gone { saved, .. } => saved.destination.clone(),
            // Its torrent is in the list, the same one (added again since is another), and one trashing
            // files would do something to now.
            Back::Files { previous, .. } => {
                let there = in_list.filter(|t| t.id == entry.id);
                undo::files_back(there.map(|t| &t.lifecycle))?;
                // Moved since (#24): an undo pressed while a Move of it waited its turn comes after
                // it. Its files' paths aren't the ones they went to the Trash from any more, even
                // where the new destination holds the old one.
                let paths = there.map(|t| t.record.paths()).unwrap_or_default();
                if previous.iter().any(|(i, _, path)| paths.get(*i) != Some(path)) {
                    return Err(CantUndo::Moved);
                }
                in_list.map(|t| t.record.destination.clone())
            }
            Back::InList { .. } => match self.torrents.get(&entry.id).map(|t| (&t.lifecycle.phase, &t.record)) {
                None => return Err(CantUndo::NotInList),
                Some((Phase::DeleteFailed { away: Some(reason), .. }, _)) => return Err(CantUndo::Destination { reason: reason.clone() }),
                Some((Phase::DeleteFailed { away: None, .. }, record)) => Some(record.destination.clone()),
                // Only a Delete tried again takes a partly deleted torrent elsewhere in the list.
                Some((_, _)) => return Err(CantUndo::Deleting),
            },
        };
        // Without one, nothing says which folders it may make.
        let Some(destination) = destination else { return Err(CantUndo::NoDestination) };
        if let Some(reason) = files::unavailable(&destination) {
            return Err(CantUndo::Destination { reason });
        }
        // A Move to Trash puts back what's still in the Trash, and refuses only when none is.
        let left = match entry.back {
            Back::Files { .. } => trash::still_in_trash(&entry.put_back),
            Back::Gone { .. } | Back::InList { .. } => entry.put_back.clone(),
        };
        if let (true, Some(first)) = (left.is_empty(), entry.put_back.first()) {
            return Err(CantUndo::NotInTrash { path: absolute(first.from.clone()) });
        }
        trash::can_put_back(&left, &destination).map_err(blocked)
    }

    /// A partial Delete's torrent, still in the list, is taken out first, so it comes back as any
    /// other that left: with its record, now whole again.
    async fn take_out_for_undo(&mut self, seq: u64, entry: &Entry) -> Result<(), Failure> {
        let Back::InList { stash } = &entry.back else { return Ok(()) };
        let saved = self.torrents.get(&entry.id).map(Torrent::kept).context("a torrent in the list has a record")?;
        let pieces = self.pieces(entry.id);
        // Written ahead: what brings it back is kept before its record goes. A crash after this
        // finds it still in the list, partly deleted, and takes it out then (`resume_undo`).
        let back = Back::Gone { saved: whole(saved), source: Source::Stash(stash.clone()) };
        self.steps.rewrite(seq, entry.id, Entry { back, ..entry.clone() });
        self.save_undo();
        self.take_off_list(entry.id).await?;
        self.wait_for_stash(stash, pieces).await;
        Ok(())
    }

    /// Starts putting back what step `seq`'s undo puts back.
    fn put_back(&mut self, seq: u64, id: TorrentId) {
        let Some((_, entry)) = self.undoing(seq, id) else { return };
        // A Move to Trash's undo puts back what's still in the Trash, and nothing is asked of an item
        // emptied from it (a file of the user's at its path doesn't matter: the file stays Off). One
        // cut short by a crash is #162's: the whole list, those not in the Trash being what it put
        // back before, or emptied since.
        let (trash, resumed) = (self.trash.clone(), self.resumed.contains(&(seq, id)));
        let items = match entry.back {
            Back::Files { .. } if !resumed => trash::still_in_trash(&entry.put_back),
            Back::Files { .. } | Back::Gone { .. } | Back::InList { .. } => entry.put_back.clone(),
        };
        let destination = match &entry.back {
            Back::Gone { saved, .. } => saved.destination.as_ref().map(|d| d.to_path_buf()),
            Back::InList { .. } => None,
            Back::Files { .. } => self.by_hash(&entry.info_hash).map(|t| t.record.destination.to_path_buf()),
        };
        self.putting_back.spawn_blocking(move || {
            let first = entry.put_back.first().map_or_else(|| PathBuf::from("/"), |t| t.from.clone());
            // Checked before it started; one finished after a crash may have lost it since.
            let Some(destination) = destination else {
                return (seq, id, Err((trash::PutBackError::Failed { path: first, detail: "no destination".into() }, Vec::new())));
            };
            let put = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trash::put_back(&items, &destination, &trash, resumed)));
            let put = put
                .unwrap_or_else(|_| Err((trash::PutBackError::Failed { path: first, detail: "putting back stopped".into() }, Vec::new())));
            (seq, id, put)
        });
    }

    /// How the undo under way's put-back went: the torrent is added again, or it's refused with
    /// what had moved back in the Trash. Adding it again failing, its files go back to the Trash
    /// too: an undo does all or nothing. Either way it's off the stack.
    pub(super) async fn put_back_ended(&mut self, seq: u64, id: TorrentId, put: PutBack) {
        let Some((of, entry)) = self.undoing(seq, id) else { return };
        let answer = match put {
            Err((why, stranded)) => {
                let why = match why {
                    trash::PutBackError::Blocked(b) => blocked(b),
                    trash::PutBackError::Failed { path, detail } => CantUndo::PutBackFailed { path: absolute(path), detail },
                };
                Err(refused(entry.undoable(of), why, stranded))
            }
            Ok(back) => {
                let added = match &entry.back {
                    Back::Gone { saved, source } => self.add_back(entry.id, &entry.info_hash, saved.clone(), source.clone()).await,
                    Back::InList { .. } => Err(anyhow!("a partial Delete's torrent is taken out before its undo").into()),
                    Back::Files { previous, had } => {
                        let restored = self.restore_priorities(entry.id, previous, &entry.put_back, &back).await;
                        if restored.is_ok() {
                            self.files_back(entry.id, &entry.info_hash, previous, had, &back).await;
                        }
                        restored
                    }
                };
                added.map_err(|failure| {
                    let detail = match &failure {
                        Failure::Refused(r) => format!("{r:?}"),
                        Failure::Error(proto::ServiceError::Internal { detail }) => detail.clone(),
                        Failure::Error(e) => format!("{e:?}"),
                    };
                    let why = match entry.back {
                        Back::Files { .. } => CantUndo::PriorityFailed { detail },
                        Back::Gone { .. } | Back::InList { .. } => CantUndo::AddFailed { detail },
                    };
                    // Only what's back: an item emptied from the Trash while a crash cut the undo short
                    // isn't, and what's at its path now isn't the torrent's.
                    let stranded = trash::take_back(&self.trash, back.iter());
                    refused(entry.undoable(of), why, stranded)
                })
            }
        };
        self.undo_ended(seq, id, answer.map(|()| entry.undoable(of)));
    }

    /// A Move to Trash's undo put `back` back: the torrent's files again ([`trashed::Trashed::put_back`]),
    /// and the undo leaves the torrent as it was before the move. A file in the Trash is read where
    /// it went, so librqbit has of it what it had; only a piece it shares with a file the torrent
    /// gets that came meanwhile and couldn't go into it makes the undo check the torrent
    /// ([`trashed::needs_check`]). Not one Unchecked: nothing is known of its files either way, as
    /// before the move.
    async fn files_back(
        &mut self,
        id: TorrentId,
        info_hash: &str,
        previous: &[(usize, Priority, PathBuf)],
        had: &[(usize, u64)],
        back: &[Trashed],
    ) {
        let returned: Vec<(usize, PathBuf)> = previous
            .iter()
            .filter(|(_, _, path)| back.iter().any(|b| path.starts_with(&b.from)))
            .map(|(i, _, p)| (*i, p.clone()))
            .collect();
        let (trashed, hash, files) = (self.guard.trashed().clone(), info_hash.to_owned(), returned.clone());
        let put = tokio::task::spawn_blocking(move || trashed.put_back(&hash, &files)).await.unwrap_or_default();
        self.poll_torrent(id).await;
        let Some(t) = self.torrents.get(&id) else { return };
        let back: Vec<usize> = returned.iter().map(|(i, _)| *i).collect();
        let now = |i: usize| t.polled.file_progress.get(i).copied().unwrap_or(0);
        if !t.lifecycle.phase.unchecked()
            && trashed::needs_check(had, &back, now, &put)
            && let Err(e) = self.command(id, proto::TorrentCommand::Check).await
        {
            warn!("checking torrent {id} again once its files are back: {e:?}");
        }
    }

    /// A Move to Trash's undo, once its files are back: each file asked about has the priority it had,
    /// those Off first (but a file moved that didn't come back stays Off). The torrent's lifecycle is asked as for any priority change.
    async fn restore_priorities(
        &mut self,
        id: TorrentId,
        previous: &[(usize, Priority, PathBuf)],
        moved: &[Trashed],
        back: &[Trashed],
    ) -> Result<(), Failure> {
        self.event(id, super::Event::Command(proto::Action::SetPriority)).await?;
        // A file that was moved and didn't come back (emptied from the Trash) stays Off.
        // (A file at an emptied one's path isn't it coming back: only what the put-back says is.)
        let came_back = |path: &PathBuf| moved.iter().all(|t| &t.from != path) || back.iter().any(|t| &t.from == path);
        let put_back: std::collections::BTreeSet<usize> =
            previous.iter().filter(|(_, _, path)| back.iter().any(|t| path.starts_with(&t.from))).map(|(i, _, _)| *i).collect();
        for (priority, files) in restore_order(previous, came_back) {
            self.set_priority_back(id, &files, priority, &put_back).await?;
        }
        self.size_check(id).await;
        self.poll_torrent(id).await;
        Ok(())
    }

    /// Adds torrent `id` again under its id, with its record, from `source`: from a stash, with the
    /// pieces it had (librqbit only samples them) while its files are as they were kept, then run as
    /// it's wanted. Failing to add it, it's not back: the list is as it was.
    async fn add_back(&mut self, id: TorrentId, hash: &str, mut saved: Saved, source: Source) -> Result<(), Failure> {
        // Kept aside by a let-go when it was removed or deleted: what it comes back from was copied
        // without those pieces, so nothing's left to let go of.
        if matches!(saved.letting_go, letgo::State::Stashed { .. }) {
            saved.letting_go = letgo::State::Holding;
        }
        self.store.ids_above(id);
        let stash = match source {
            Source::Magnet(link) => {
                let destination = saved.destination.clone().context("a torrent's record has its destination")?;
                let prepared = prepare(Fetched::Magnet(link), destination)?;
                let record = Record::parse(hash, saved, None, None).context("a torrent's record has its destination")?;
                self.fetch(record, prepared.add);
                self.save();
                return Ok(());
            }
            Source::Stash(stash) => stash,
        };
        let bytes = stash.torrent()?;
        let info = Info::parse(&bytes)?;
        self.store.keep_info(hash, &bytes)?;
        // The pieces it had hold while its files are as they were; else librqbit checks them all.
        let pieces = bitv::path(&self.session_folder(), hash);
        if stash.unchanged() {
            stash.restore_pieces(&pieces)?;
        } else {
            tracing::info!(id, "its files changed since it was kept aside: checked in full");
            bitv::forget(&pieces);
        }
        let phase = match (!saved.delete_failed.is_empty(), saved.unchecked) {
            (true, unchecked) => Phase::DeleteFailed { kept: saved.delete_failed.clone(), away: None, unchecked },
            // Its check was cancelled: back as it went, as at a restart.
            (false, true) => Phase::Unchecked { gone: Default::default() },
            (false, false) => Phase::Ready,
        };
        // Its priorities as it had them: from its record, or, kept by an older build, the stash's.
        let mut record =
            Record::parse(hash, saved, Some(info), stash.only_files.clone()).context("a torrent's record has its destination")?;
        // Shown with what librqbit had of each file as it left, until librqbit has it again; partly
        // in the Trash, nothing is known of it.
        let had = std::mem::take(&mut record.had);
        let polled = if matches!(phase, Phase::DeleteFailed { .. }) { Polled::default() } else { Polled::kept(&record, had) };
        let torrent = Torrent {
            id,
            lifecycle: State::new(record.wants, phase),
            lib: lib::Lib::Out,
            polled,
            there: Default::default(),
            meter: seeding::Meter::new(record.round),
            record,
            #[cfg(test)]
            added: 0,
        };
        self.torrents.insert(id, torrent);
        if let Err(e) = self.carry_out(id).await {
            self.torrents.remove(&id);
            self.store.forget_info(hash);
            return Err(e);
        }
        self.save();
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
            .steps
            .machine()
            .stack
            .iter()
            .filter_map(|p| p.done.as_ref())
            .flat_map(|d| d.values())
            .filter(|e| {
                matches!(e.back, Back::Gone { .. }) && e.put_back.is_empty() && self.by_hash(&e.info_hash).map(|t| t.id) == Some(e.id)
            })
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

    /// The actions a crash cut short, found at start, other than Deletes (`run` ends those): a Move to
    /// Trash ends with what it moved, as written ahead; any other changed nothing yet and goes.
    pub(super) fn end_cut_short(&mut self) {
        let running: Vec<(u64, undo::Running, bool)> = self
            .steps
            .machine()
            .steps
            .iter()
            .flat_map(|(seq, s)| match (&s.what, s.phase) {
                (What::Action(parts), StepPhase::Running | StepPhase::Aborting) => {
                    parts.values().filter_map(|p| p.doing()).map(|r| (*seq, r.clone(), s.phase == StepPhase::Aborting)).collect()
                }
                (What::Action(_), StepPhase::Queued) | (What::Undo { .. } | What::Command(_), _) => Vec::new(),
            })
            .collect();
        for (seq, r, aborted) in running {
            // One aborted puts each file's priority back even with nothing moved, as it does live.
            match (r.action, aborted || !r.moved.is_empty()) {
                (UndoAction::TrashFiles { .. }, true) => {
                    let action = UndoAction::TrashFiles { count: r.moved.len() };
                    let back = Back::Files { previous: r.previous, had: r.had };
                    let entry = Entry { action, name: r.name, info_hash: r.info_hash, id: r.id, back, put_back: r.moved, listed: r.listed };
                    let ended = self.end(seq, r.id, Over::Finished(Some(entry)));
                    self.steps.unanswered(ended);
                }
                (UndoAction::TrashFiles { .. } | UndoAction::Remove | UndoAction::Delete, _) => {
                    let ended = self.end(seq, r.id, Over::Failed);
                    self.steps.unanswered(ended);
                }
            }
        }
    }

    /// The undos a crash cut short, found at start, rollbacks of actions it cut short among them:
    /// each goes on as it was ([`Actor::resume_undo`]) once the machine starts it.
    pub(super) fn resume_undos(&mut self) {
        let cut_short =
            self.steps.machine().steps.iter().filter(|(_, s)| matches!(s.what, What::Undo { .. }) && s.phase != StepPhase::Queued);
        self.resumed = cut_short.flat_map(|(seq, s)| s.underway().map(move |id| (*seq, id))).collect();
    }

    /// Undo step `seq`, of action `of`, `entry`, a crash cut short: done, if its torrent made it
    /// back; else what's still in the Trash goes back, and it's added again, or, one item blocked,
    /// what's back goes to the Trash again ([`trash::put_back`]).
    async fn resume_undo(&mut self, seq: u64, of: u64, entry: Entry) {
        // A Move to Trash's torrent never left the list: what's still in the Trash goes back, and then
        // the priorities.
        if matches!(entry.back, Back::Files { .. }) {
            return self.put_back(seq, entry.id);
        }
        if let Some((id, partly_deleted)) = self.by_hash(&entry.info_hash).map(|t| (t.id, !t.record.delete_failed.is_empty())) {
            // A partial Delete's torrent the crash left in the list: taken out now, then on. Any
            // other in the list made it back.
            let half_taken = id == entry.id && entry.action == UndoAction::Delete && partly_deleted;
            if !half_taken {
                return self.undo_ended(seq, entry.id, Ok(entry.undoable(of)));
            }
            if let Err(e) = self.take_off_list(id).await {
                warn!(id, error = %e, "failed to take out a partly deleted torrent to finish its undo");
                return self.undo_ended(seq, entry.id, Err(Failure::from(e)));
            }
        }
        self.put_back(seq, entry.id);
    }

    /// Keeps the stack, lets go of the stashes nothing names any more, and has the files in the
    /// Trash follow it.
    pub(super) fn save_undo(&self) {
        if let Err(e) = undo::save(&self.data_dir, self.steps.machine()) {
            warn!(error = %e, "failed to save the undo stack");
        }
        undo::tidy(&self.data_dir, self.steps.machine(), self.torrents.values().map(|t| &t.record));
        self.guard.trashed().follow(self.steps.machine());
    }
}

/// The priorities a Move to Trash's undo sets, in the order it sets them: those Off first, Normal
/// last, so a failure between them never leaves a file Normal that the failed undo then moves back
/// to the Trash (which would make the torrent Missing files). A file moved that didn't come back
/// (`came_back` says) is left as it is: Off.
fn restore_order(previous: &[(usize, Priority, PathBuf)], came_back: impl Fn(&PathBuf) -> bool) -> Vec<(Priority, Vec<usize>)> {
    [Priority::Off, Priority::Normal]
        .into_iter()
        .map(|priority| {
            let files = previous
                .iter()
                .filter(|(_, p, path)| *p == priority && (priority == Priority::Off || came_back(path)))
                .map(|(i, _, _)| *i)
                .collect();
            (priority, files)
        })
        .filter(|(_, files): &(Priority, Vec<usize>)| !files.is_empty())
        .collect()
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

pub(super) fn name_of(torrent: &Torrent) -> String {
    torrent.name().unwrap_or_else(|| torrent.record.info_hash.clone())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        /// For any files and any that didn't come back: each file is set at most once, those Off before
        /// those Normal, and a file that didn't come back is never set Normal.
        #[test]
        fn priorities_are_restored_off_first_and_never_normal_for_a_file_that_didnt_come_back(
            files in prop::collection::vec((any::<bool>(), any::<bool>()), 0..8),
        ) {
            let previous: Vec<(usize, Priority, PathBuf)> = files
                .iter()
                .enumerate()
                .map(|(i, (off, _))| (i, if *off { Priority::Off } else { Priority::Normal }, PathBuf::from(format!("/d/{i}"))))
                .collect();
            let back = |path: &PathBuf| files[path.to_string_lossy()[3..].parse::<usize>().unwrap()].1;
            let order = restore_order(&previous, back);
            let set: Vec<(Priority, usize)> = order.iter().flat_map(|(p, fs)| fs.iter().map(move |f| (*p, *f))).collect();
            // Off before Normal.
            let firsts: Vec<Priority> = set.iter().map(|(p, _)| *p).collect();
            prop_assert!(!firsts.windows(2).any(|w| w[0] == Priority::Normal && w[1] == Priority::Off));
            for (p, f) in &set {
                prop_assert_eq!(*p, previous[*f].1);
                prop_assert!(*p == Priority::Off || files[*f].1, "a file that didn't come back is never set Normal");
            }
            prop_assert_eq!(set.iter().filter(|(p, _)| *p == Priority::Off).count(), files.iter().filter(|(off, _)| *off).count());
        }
    }
}
