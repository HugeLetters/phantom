//! A Move (#24): the actor's side of `lifecycle::moving`. A Move is a step on its torrent's lane
//! (#190) from the moment it comes: once it runs, it's decided on the torrent as it is, its plan is
//! made (refused with the clash, if nothing says what to do with what's in the new destination),
//! the torrent is kept aside with the pieces librqbit has (its `.bitv`, as for Undo), written ahead
//! in its record, and taken out of librqbit; then its files move on blocking tasks, each reporting
//! back (`moving.rs`), and the step ends, its client answered, once it's moved or back where it was.
//! What comes for the torrent meanwhile waits its turn behind it there, shown done at once; a Remove
//! or a Delete cancels it first, as it comes. A move a restart finishes runs on no step: what comes
//! for its torrent meanwhile is held, as while librqbit samples its pieces (`steps.rs`).
//!
//! Once it's over the torrent is size-checked where its files are now, then added back to librqbit
//! with the pieces it had, which librqbit samples as at a start: no full check.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use lifecycle::moving::{self as stages, Stage, Task};
use proto::{AbsolutePath, Action, Clash, NotMoved, TorrentId, TorrentRefusal};
use tracing::warn;

use super::super::files::Destination;
use super::super::moving::{self, Refused, Stop};
use super::super::store::Moving;
use super::super::{Failure, bitv};
use super::steps::Over;
use super::{Actor, Event, Observation, Phase, lib};

/// A move under way, the actor's side: the step whose client waits (none for one a restart
/// finishes), the task running of it, what stops its copy, and the files that weren't in the old
/// destination.
#[derive(Default)]
pub(in super::super) struct Mover {
    seq: Option<u64>,
    task: Option<Task>,
    stop: Arc<AtomicBool>,
    missing: Vec<usize>,
}

/// How a move's task ended.
pub(super) enum Moved {
    Staged(Result<Vec<usize>, Stop>),
    Finished(Vec<String>),
    Restored(Vec<String>),
}

/// The test's hold on moves: the gate each copy waits at after each chunk, and whether a move on
/// the same drive copies all the same.
#[cfg(test)]
#[derive(Default, Clone)]
pub(in super::super) struct Hold {
    pub gate: Option<super::super::trash::Gate>,
    pub copies: bool,
}

impl Actor {
    /// Move step `seq` of torrent `id` into `to`: its step runs until the move is over, and its
    /// client hears how it went then. Refused, or moving nowhere, it's over at once.
    pub(super) async fn start_move(&mut self, seq: u64, id: TorrentId, to: AbsolutePath, clash: Option<Clash>) {
        match self.begin_move(id, to, clash).await {
            Ok(true) => {
                if let Some(mover) = self.moves.get_mut(&id) {
                    mover.seq = Some(seq);
                }
            }
            Ok(false) => self.step_ended(seq, id, Over::Finished(None), Ok(())),
            Err(e) => self.step_ended(seq, id, Over::Finished(None), Err(e)),
        }
    }

    /// Starts moving torrent `id` into `to`, decided on it as it is now: whether it moves (it's
    /// there already: no).
    async fn begin_move(&mut self, id: TorrentId, to: AbsolutePath, clash: Option<Clash>) -> Result<bool, Failure> {
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(Action::Move)).await?;
        let refused = |why| Failure::torrent(id, why);
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        if record.destination == to {
            return Ok(false);
        }
        let to = Destination::parse(to).map_err(|reason| Failure::Refused(proto::Refusal::Destination { reason }))?.into_path();
        let location = record.location().context("a torrent with info has a location")?;
        if to.starts_with(&location) {
            return Err(refused(TorrentRefusal::IntoItself));
        }
        if !self.guard.trashed().in_trash(&record.info_hash).is_empty() {
            return Err(refused(TorrentRefusal::TrashedFiles));
        }
        let renames = moving::same_volume(&record.destination, &to);
        #[cfg(test)]
        let renames = renames && !self.move_hold.copies;
        let plan = moving::plan(&record.destination, &to, &record.info_hash, &inside(record), clash, renames).map_err(|r| match r {
            Refused::Clash(taken) => {
                refused(TorrentRefusal::Clash { files: taken.into_iter().filter_map(|p| AbsolutePath::new(p).ok()).collect() })
            }
            Refused::NotAFolder(path) => match AbsolutePath::new(path) {
                Ok(path) => refused(TorrentRefusal::NotMoved { name: name(record), why: NotMoved::NotAFolder { path } }),
                Err(path) => anyhow::anyhow!("not an absolute path: {}", path.display()).into(),
            },
        })?;
        // Kept aside as it is, with the pieces librqbit has: they come back with it. Written ahead.
        let had = self.pieces(id);
        let stash = self.stash_of(id);
        let torrent = self.torrents.get_mut(&id).expect("just found");
        torrent.record.moving = Some(Moving { plan, stash: stash.clone(), placed: false });
        self.save();
        self.moves.insert(id, Mover::default());
        // librqbit lets go of it.
        if let Err(e) = self.event(id, Event::Observed(Observation::MoveStarted { to })).await {
            let detail = format!("{e:?}");
            let _ = self.event(id, moved(stages::Event::Failed(NotMoved::Failed { detail }))).await;
        }
        // Its last save of the pieces lands in the stash, which shares the file.
        if let Some(stash) = stash {
            bitv::wait_for(&[(stash.bitv(), had)]).await;
        }
        Ok(true)
    }

    /// Carries out what each move under way needs next ([`stages::next`]): after every message and
    /// every end of a task.
    pub(super) fn go_on_moving(&mut self) {
        let ids: Vec<TorrentId> =
            self.torrents.iter().filter(|(_, t)| matches!(t.lifecycle.phase, Phase::Moving { .. })).map(|(id, _)| *id).collect();
        for id in ids {
            let torrent = &self.torrents[&id];
            let Phase::Moving { stage, .. } = &torrent.lifecycle.phase else { continue };
            let Some(plan) = torrent.record.moving.as_ref().map(|m| m.plan.clone()) else {
                warn!(id, "a torrent moving without a plan in its record");
                continue;
            };
            let out = matches!(torrent.lib, lib::Lib::Out);
            let mover = self.moves.entry(id).or_default();
            let Some(todo) = stages::next(stage, stages::World { task: mover.task, out }) else { continue };
            match todo {
                stages::Do::Copy => {
                    mover.task = Some(Task::Copy { stopping: false });
                    let (stop, copied) = (mover.stop.clone(), self.copied_tx.clone());
                    #[cfg(test)]
                    let gate = self.move_hold.gate.clone();
                    self.move_tasks.spawn_blocking(move || {
                        let between = || {
                            #[cfg(test)]
                            if let Some(gate) = &gate {
                                return super::super::trash::wait_at(gate, &plan.to);
                            }
                            true
                        };
                        let report = |done, total| {
                            let _ = copied.send((id, done, total));
                        };
                        (id, Moved::Staged(moving::stage(&plan, report, &stop, &between)))
                    });
                }
                stages::Do::Stop => {
                    mover.stop.store(true, Ordering::SeqCst);
                    mover.task = Some(Task::Copy { stopping: true });
                }
                stages::Do::Finish => {
                    mover.task = Some(Task::Finish);
                    self.move_tasks.spawn_blocking(move || (id, Moved::Finished(moving::finish(&plan))));
                }
                stages::Do::Restore => {
                    mover.task = Some(Task::Restore);
                    self.move_tasks.spawn_blocking(move || (id, Moved::Restored(moving::restore(&plan))));
                }
            }
        }
    }

    /// What a move's copy copied so far: the latest of each torrent's, as reports come faster than
    /// they're shown.
    pub(super) async fn move_copied(&mut self, first: (TorrentId, u64, u64)) {
        let mut latest = std::collections::BTreeMap::from([(first.0, (first.1, first.2))]);
        while let Ok((id, done, total)) = self.copied.try_recv() {
            latest.insert(id, (done, total));
        }
        for (id, (done, total)) in latest {
            if let Err(e) = self.event(id, moved(stages::Event::Copied { done, total })).await {
                warn!(id, error = ?e, "failed to follow a move");
            }
        }
    }

    /// A move's task ended as `ended` says: staged, the record names the new destination (written
    /// ahead: from here a start finishes the move) and the files take their places; one that failed
    /// is undone; once finished or undone the move is over.
    pub(super) async fn move_task_ended(&mut self, id: TorrentId, ended: Moved) {
        if let Some(first) = self.copied.try_recv().ok() {
            self.move_copied(first).await;
        }
        if let Some(mover) = self.moves.get_mut(&id) {
            mover.task = None;
        }
        let placing = matches!(
            self.torrents.get(&id).map(|t| &t.lifecycle.phase),
            Some(Phase::Moving { stage: Stage::TakingOut | Stage::Copying { .. }, .. })
        );
        let event = match ended {
            Moved::Staged(Ok(missing)) if placing => {
                let Some(torrent) = self.torrents.get_mut(&id) else { return };
                let Some(moving) = torrent.record.moving.as_mut() else { return };
                moving.placed = true;
                let check = moving.plan.adopts();
                match AbsolutePath::new(moving.plan.to.clone()) {
                    Ok(to) => torrent.record.destination = to,
                    Err(to) => return warn!(id, to = %to.display(), "a move to a relative path"),
                }
                self.save();
                if let Some(mover) = self.moves.get_mut(&id) {
                    mover.missing = missing;
                }
                stages::Event::Placed { check }
            }
            // Stopped already: it's restored.
            Moved::Staged(Ok(_) | Err(Stop::Stopped)) => return,
            Moved::Staged(Err(Stop::Failed(why))) => stages::Event::Failed(why),
            Moved::Finished(warnings) | Moved::Restored(warnings) => {
                for warning in warnings {
                    warn!(id, warning, "a move left something behind");
                }
                return self.move_ended(id).await;
            }
        };
        if let Err(e) = self.event(id, moved(event)).await {
            warn!(id, error = ?e, "failed to follow a move");
        }
    }

    /// Torrent `id`'s move is over: as it was before, where its files are now, size-checked there
    /// first, then added back to librqbit with the pieces it had. The Move's client hears how it
    /// went: moved (without the files that weren't in the old destination: they're missing), or
    /// why not.
    async fn move_ended(&mut self, id: TorrentId) {
        let mover = self.moves.remove(&id).unwrap_or_default();
        let session = self.session_folder();
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        let Phase::Moving { stage, .. } = &torrent.lifecycle.phase else { return };
        let name = name(&torrent.record);
        let answer = match stage {
            Stage::Restoring { why } => Err(Failure::torrent(id, TorrentRefusal::NotMoved { name, why: why.clone() })),
            Stage::TakingOut | Stage::Copying { .. } | Stage::RemovingOld { .. } => {
                let paths = torrent.record.paths();
                let missing: Vec<AbsolutePath> = mover
                    .missing
                    .iter()
                    .filter(|i| torrent.record.gets(**i))
                    .filter_map(|i| AbsolutePath::new(paths.get(*i)?.clone()).ok())
                    .collect();
                match missing.is_empty() {
                    true => Ok(()),
                    false => Err(Failure::torrent(id, TorrentRefusal::MovedWithout { name, files: missing })),
                }
            }
        };
        torrent.lifecycle = super::decide(&torrent.lifecycle, &moved(stages::Event::Ended)).expect("never refused");
        if let Some(stash) = torrent.record.moving.take().and_then(|m| m.stash)
            && let Err(e) = stash.restore_pieces(&bitv::path(&session, &torrent.record.info_hash))
        {
            warn!(id, error = %e, "couldn't put back the pieces of a torrent moved: it's checked in full");
        }
        self.save();
        // The stash isn't named any more.
        self.save_undo();
        // Judged where its files are now before librqbit has it, which would make the missing ones again.
        self.size_check(id).await;
        if let Err(e) = self.carry_out(id).await {
            warn!(id, error = ?e, "couldn't add a torrent back once it moved");
        }
        match mover.seq {
            Some(seq) => self.step_ended(seq, id, Over::Finished(None), answer),
            None => {
                if let Err(e) = answer {
                    warn!(id, error = ?e, "a move a start finished");
                }
            }
        }
    }

    /// A Remove or a Delete came for torrent `id`: a move of it whose files aren't in place yet is
    /// cancelled first, and what it moved goes back, before the Remove or the Delete runs.
    pub(super) async fn cancel_move(&mut self, id: TorrentId, action: Action) {
        if matches!(self.torrents.get(&id).map(|t| &t.lifecycle.phase), Some(Phase::Moving { .. }))
            && let Err(e) = self.event(id, Event::Command(action)).await
        {
            warn!(id, error = ?e, "failed to cancel a move");
        }
    }

    /// The service stops: each move whose files aren't in place yet goes back, as a restart would
    /// have it, and the stop waits for it.
    pub(super) async fn interrupt_moves(&mut self) {
        let ids: Vec<TorrentId> = self
            .torrents
            .iter()
            .filter(|(_, t)| matches!(t.lifecycle.phase, Phase::Moving { stage: Stage::TakingOut | Stage::Copying { .. }, .. }))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let _ = self.event(id, moved(stages::Event::Failed(NotMoved::Interrupted))).await;
        }
        self.go_on_moving();
    }

    /// Whether a move is under way: the stop waits for it.
    pub(super) fn moving(&self) -> bool {
        !self.move_tasks.is_empty() || self.torrents.values().any(|t| matches!(t.lifecycle.phase, Phase::Moving { .. }))
    }
}

fn moved(event: stages::Event) -> Event {
    Event::Observed(Observation::Move(event))
}

fn name(record: &super::Record) -> String {
    record.name.clone().unwrap_or_else(|| record.info_hash.clone())
}

/// Each of its files (not BEP 47's padding, never written) by index, with its path inside its
/// destination: its own folder's, then the file's in it.
fn inside(record: &super::Record) -> Vec<(usize, PathBuf)> {
    let (Some(info), Some(folder)) = (&record.info, record.output_folder()) else { return Vec::new() };
    let own = folder.strip_prefix(&record.destination).map(PathBuf::from).unwrap_or_default();
    info.files.iter().enumerate().filter(|(_, f)| !f.padding).map(|(i, f)| (i, own.join(&f.path))).collect()
}

/// A start finds torrents a crash left moving: the stage each is at, from its record's move. One
/// past the point where the record names the new destination is finished; any other goes back.
pub(in super::super) fn at_start(moving: &Moving) -> Stage {
    match moving.placed {
        true => Stage::RemovingOld { check: moving.plan.adopts() },
        false => Stage::Restoring { why: NotMoved::Interrupted },
    }
}
