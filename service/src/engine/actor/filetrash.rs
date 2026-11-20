//! Moving files to the Trash (#151): the files chosen are set Off first, then moved to the Trash, in that order,
//! so the size check never sees a file the torrent gets go missing. The move runs on a blocking task
//! (the same trashing a Delete does: [`trash::carry_out_until`], never through a symbolic link) and
//! the client's answer waits for it. It's a step (#190, `steps.rs`): on the undo stack from the
//! moment its command came, and its torrent's other commands wait on its lane. What moved is one
//! entry that puts the files back and each file's priority as it was ([`Back::Files`]).
//!
//! Each item is written ahead into the step as it moves, as a Delete's are: a crash in between
//! ends the step with what it moved, which Undo puts back.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Context;
use proto::{KeptWhy, Priority, TorrentId, UndoAction};

use super::super::undo::{self, Back, Entry};
use super::super::{Failure, trash};
use super::steps::{Fact, Over};
use super::undoing::name_of;
use super::{Actor, Event, Kept, Refusal, ToTrash, Trashed, to_trash};

/// A Move to Trash under way for one torrent: its step, the priorities as they were, and what
/// stops it after the item in flight.
pub(super) struct Pending {
    pub seq: u64,
    previous: Vec<(usize, Priority, PathBuf)>,
    pub stop: Arc<AtomicBool>,
}

/// How a trashing ended: of how many items it planned, what went and what didn't.
pub(super) struct Ended {
    pub id: TorrentId,
    pub carried: trash::Carried,
}

impl Actor {
    /// TrashFiles, step `seq`: those of `indices` on disk Off, then to the Trash; nothing for the
    /// others.
    pub(super) async fn trash_files(&mut self, id: TorrentId, seq: u64, indices: Vec<usize>) {
        match self.start_trashing_files(id, seq, indices).await {
            Ok((previous, stop)) => drop(self.file_trashes.insert(id, Pending { seq, previous, stop })),
            Err(e) => self.step_ended(seq, id, Over::Failed, Err(e)),
        }
    }

    /// Decides and sets the files Off, then starts the move: the priorities they had, and its stop.
    async fn start_trashing_files(
        &mut self,
        id: TorrentId,
        step: u64,
        indices: Vec<usize>,
    ) -> Result<(Vec<(usize, Priority, PathBuf)>, Arc<AtomicBool>), Failure> {
        // As any command: decided on librqbit and the disk as they are now.
        self.poll_torrent(id).await;
        #[cfg(test)]
        if let Some(mut hook) = self.before_off.take() {
            hook(self);
        }
        self.size_check(id).await;
        self.event(id, Event::Command(proto::Action::TrashFiles)).await?;
        let record = self.with_info(id)?;
        let listed = super::files(record, &[], &BTreeSet::new());
        if let Some(bad) = indices.iter().find(|&&i| !listed.iter().any(|f| f.index == i)) {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        let hash = record.info_hash.clone();
        let ToTrash { location, files: paths, single } = to_trash(record).context("a torrent with info has its files")?;
        // Nothing for one that isn't at its path (#276): in the Trash already, deleted or never made.
        let there = self.torrents.get(&id).map(|t| t.there.clone()).unwrap_or_default();
        let previous: Vec<(usize, Priority, PathBuf)> = listed
            .iter()
            .filter(|f| indices.contains(&f.index) && there.contains(&f.index))
            .filter_map(|f| Some((f.index, f.priority, paths.get(f.index)?.clone())))
            .collect();
        // Written ahead with the step: what its undo sets them back to.
        self.log(step, id, |running| running.previous = previous.clone());
        let chosen: Vec<PathBuf> = previous.iter().map(|(_, _, path)| path.clone()).collect();
        // Off first: a file that goes while it's Normal would make the torrent Missing files.
        let chosen_indices: Vec<usize> = previous.iter().map(|(i, _, _)| *i).collect();
        self.set_priority(id, &chosen_indices, Priority::Off).await?;
        self.size_check(id).await;
        self.poll_torrent(id).await;
        // And what librqbit has of them once Off, as they go: only a piece they share with a file it
        // gets changes it while they're in the Trash, where they're read as they went, and the undo
        // checks again only if that couldn't go into them (`trashed::needs_check`). Read once they're
        // Off, a piece landing as they're set so isn't taken for one. Nothing is known of them during
        // a check, and nothing is recorded.
        let mut had: Vec<(usize, u64)> = match self.torrents.get(&id) {
            Some(t) if t.lifecycle.status(t.polled.finished) != Some(proto::Status::Checking) => {
                previous.iter().map(|(i, _, _)| (*i, t.polled.file_progress.get(*i).copied().unwrap_or(0))).collect()
            }
            Some(_) | None => Vec::new(),
        };
        // One librqbit has open that isn't the file at its path (replaced by hand) has pieces of the
        // one it has open, not of what goes (#268): they're let go of, so it goes with none counted
        // (`had` 0). Its undo checks the torrent (`let_go`), which reads the file put back: none of
        // an overwritten one, all of one written back as it was (#328). Nothing is counted that
        // can't be read, and nothing that can is left to download again.
        let let_go = self.off_replaced(id, &chosen_indices);
        for i in &let_go {
            if let Some((_, was)) = had.iter_mut().find(|(file, _)| file == i) {
                *was = 0;
            }
        }
        self.log(step, id, |running| {
            running.had = had;
            running.let_go = let_go.into_iter().collect();
        });
        let (trash, went) = (self.trash.clone(), self.files_went_tx.clone());
        // librqbit may be checking it, or still writing: each file moves with none of that under way,
        // and none reaches it once it's moved (`Trashed::take`), so its undo finds it as it went.
        let trashed = self.guard.trashed().clone();
        let files: Vec<(usize, PathBuf)> = previous.iter().map(|(i, _, path)| (*i, path.clone())).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        self.file_trashing.spawn_blocking(move || {
            let carried = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plan = trash::plan_files(&location, &chosen, single);
                trashed.take(&hash, &plan, &files, &trash, |moved| drop(went.send((step, id, moved.clone()))), &stopping)
            }));
            let failed = |path: &PathBuf| (path.clone(), KeptWhy::Failed { detail: "trashing stopped".into() });
            let carried = carried.unwrap_or_else(|_| trash::Carried { moved: Vec::new(), kept: chosen.iter().map(failed).collect() });
            Ended { id, carried }
        });
        Ok((previous, stop))
    }

    /// Changes what running action `seq`'s part on torrent `id` holds, written ahead.
    pub(super) fn log(&mut self, seq: u64, id: TorrentId, change: impl FnOnce(&mut undo::Running)) {
        let Some(mut running) = self.action_part(seq, id).cloned() else { return };
        change(&mut running);
        self.report(Fact::Moved(seq, id, running));
    }

    /// An item a Move to Trash moved, as it goes: written ahead into its step.
    pub(super) fn files_went(&mut self, seq: u64, id: TorrentId, moved: Trashed) {
        self.log(seq, id, |running| {
            if running.moved.iter().all(|t| t.from != moved.from) {
                running.moved.push(moved);
            }
        });
    }

    /// How a Move to Trash ended: what moved is its step's entry (nothing moved, it's gone from the
    /// stack), and the client hears of what couldn't move (those files are Off all the same). One an
    /// Undo aborted is put back instead, its priorities too, even with nothing moved. Then its
    /// torrent's lane goes on.
    pub(super) async fn files_trashed(&mut self, ended: Ended) {
        let Ended { id, carried: trash::Carried { moved, kept } } = ended;
        let Some(Pending { seq, previous, .. }) = self.file_trashes.remove(&id) else { return };
        let running = self.action_part(seq, id).cloned();
        let aborted = self.steps.machine().steps.get(&seq).is_some_and(|s| s.phase == lifecycle::undo::Phase::Aborting);
        let name = self.torrents.get(&id).map(name_of);
        let over = match (name.clone(), self.torrents.get(&id).map(|t| t.record.info_hash.clone()), running) {
            (Some(name), Some(info_hash), Some(running)) if aborted || !moved.is_empty() => {
                // An aborted one is told as it was asked, whatever moved before it stopped.
                let action = if aborted { running.action } else { UndoAction::TrashFiles { count: moved.len() } };
                let back = Back::Files { previous, had: running.had, let_go: running.let_go, replacing: Vec::new() };
                Over::Finished(Some(Entry { action, name, info_hash, id, back, put_back: moved, listed: running.listed }))
            }
            _ => Over::Failed,
        };
        let ended = self.end(seq, id, over);
        self.size_check(id).await;
        self.poll_torrent(id).await;
        let answer = match kept.is_empty() {
            true => Ok(()),
            false => {
                let kept = kept
                    .into_iter()
                    .map(|(path, why)| Kept { path: proto::AbsolutePath::new(path).expect("librqbit's paths are absolute"), why })
                    .collect();
                Err(Failure::torrent(id, Refusal::FilesNotTrashed { name: name.unwrap_or_else(|| format!("torrent {id}")), kept }))
            }
        };
        self.answer_step(ended, super::steps::Outcome::Done(answer));
    }
}
