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

use proto::{KeptWhy, Priority, TorrentId, UndoAction};

use super::super::undo::machine::{Observed, What};
use super::super::undo::{self, Back, Entry};
use super::super::{Failure, trash};
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
    /// TrashFiles, step `seq`: `indices` Off, then those on disk to the Trash.
    pub(super) async fn trash_files(&mut self, id: TorrentId, seq: u64, indices: Vec<usize>) {
        match self.start_trashing_files(id, seq, indices).await {
            Ok((previous, stop)) => drop(self.file_trashes.insert(id, Pending { seq, previous, stop })),
            Err(e) => self.step_ended(seq, Observed::Failed(seq), Err(e)),
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
        self.size_check(id).await;
        self.event(id, Event::Command(proto::Action::TrashFiles)).await?;
        let handle = self.handle_of(id)?.clone();
        let held = self.held(&handle).cloned();
        let listed = super::files(&handle, &[], held.as_ref(), &BTreeSet::new())?;
        if let Some(bad) = indices.iter().find(|&&i| !listed.iter().any(|f| f.index == i)) {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        let ToTrash { location, files: paths, single } = to_trash(&handle)?;
        let previous: Vec<(usize, Priority, PathBuf)> = listed
            .iter()
            .filter(|f| indices.contains(&f.index))
            .filter_map(|f| Some((f.index, f.priority, paths.get(f.index)?.clone())))
            .collect();
        // Written ahead with the step: what its undo sets them back to, and what librqbit had of
        // them, which a check while they're in the Trash changes (its undo then checks again).
        // Nothing is known of it during a check: its undo checks again.
        let (progress, checking) = match self.torrents.get(&id) {
            Some(t) => (t.polled.file_progress.clone(), t.lifecycle.status(t.polled.finished) == Some(proto::Status::Checking)),
            None => (Vec::new(), false),
        };
        let had: Vec<(usize, u64)> =
            previous.iter().map(|(i, _, _)| (*i, if checking { u64::MAX } else { progress.get(*i).copied().unwrap_or(0) })).collect();
        self.log(step, |running| (running.previous, running.had) = (previous.clone(), had));
        let chosen: Vec<PathBuf> = previous.iter().map(|(_, _, path)| path.clone()).collect();
        // Off first: a file that goes while it's Normal would make the torrent Missing files.
        let chosen_indices: Vec<usize> = previous.iter().map(|(i, _, _)| *i).collect();
        self.set_priority(id, &chosen_indices, Priority::Off).await?;
        self.size_check(id).await;
        self.poll_torrent(id).await;
        let (trash, went) = (self.trash.clone(), self.files_went_tx.clone());
        // librqbit may be checking it, or still writing: each file moves with none of that under way,
        // and none reaches it once it's moved (`Guard::moving`), so its undo finds it as it went.
        let moving = self.guard.moving(&handle.info_hash().as_string());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        self.file_trashing.spawn_blocking(move || {
            let carried = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plan = trash::plan_files(&location, &chosen, single);
                trash::carry_out_holding(&plan, &trash, |moved| drop(went.send((step, moved.clone()))), &stopping, Some(&moving))
            }));
            let failed = |path: &PathBuf| (path.clone(), KeptWhy::Failed { detail: "trashing stopped".into() });
            let carried = carried.unwrap_or_else(|_| trash::Carried { moved: Vec::new(), kept: chosen.iter().map(failed).collect() });
            Ended { id, carried }
        });
        Ok((previous, stop))
    }

    /// Changes what running action `seq` holds, written ahead.
    pub(super) fn log(&mut self, seq: u64, change: impl FnOnce(&mut undo::Running)) {
        let Some(What::Action(mut running)) = self.undo.steps.get(&seq).map(|s| s.what.clone()) else { return };
        change(&mut running);
        self.undo_event(Observed::Moved(seq, running));
    }

    /// An item a Move to Trash moved, as it goes: written ahead into its step.
    pub(super) fn files_went(&mut self, seq: u64, moved: Trashed) {
        self.log(seq, |running| {
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
        let ending = self.ending(seq);
        let (running, aborted) = match self.undo.steps.get(&seq) {
            Some(step) => (
                match &step.what {
                    What::Action(r) => Some(r.clone()),
                    What::Undo { .. } | What::Command(_) => None,
                },
                step.phase == lifecycle::undo::Phase::Aborting,
            ),
            None => (None, false),
        };
        let name = self.torrents.get(&id).map(name_of);
        let how = match (name.clone(), self.torrents.get(&id).map(|t| t.info_hash.clone()), running) {
            (Some(name), Some(info_hash), Some(running)) if aborted || !moved.is_empty() => {
                // An aborted one is told as it was asked, whatever moved before it stopped.
                let action = if aborted { running.action } else { UndoAction::TrashFiles { count: moved.len() } };
                let back = Back::Files { previous, had: running.had };
                Observed::Finished(seq, Some(Entry { action, name, info_hash, id, back, put_back: moved, listed: running.listed }))
            }
            _ => Observed::Failed(seq),
        };
        self.ended(how);
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
        self.ended_as(seq, ending, answer);
    }
}
