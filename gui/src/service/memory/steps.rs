//! The fake's side of Undo's machine ([`lifecycle::undo`], #190), the engine's own: every command
//! for a torrent is a step in its lane from the moment it comes; a Remove, a Delete and a Move to
//! Trash are on the undo stack too, in the order they came. The machine says which to start; the
//! fake carries each out, at once or once its time on the app clock has passed (`trash_takes`,
//! `undo_takes`), and reports how it ended. What the snapshot lists is what the steps make of the
//! torrents, as the engine's: a torrent whose Remove or Delete came is gone from the list at once;
//! one an Undo is bringing back is listed again as it was; files a Move to Trash or its undo is for
//! have their new priorities at once.

use std::collections::{BTreeMap, BTreeSet};

use lifecycle::undo::{self, Arrival, Do, Observed, Phase, What, view};
use lifecycle::{Event, Phase as Lifecycle, State};
use proto::{Priority, Refusal, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable};

use client::Failure;

use super::{FilesTrashed, Left, MemoryService};
use crate::service::{Done, Incoming, answer};

/// What an action holds while it runs: what brings its torrent back, as it was when the command
/// came, the files a Move to Trash was asked for, and the priorities they had once it started.
#[derive(Clone)]
pub(super) struct Run {
    pub action: UndoAction,
    pub left: Left,
    pub asked: Vec<usize>,
    pub previous: Vec<(usize, Priority)>,
}

pub(super) type Machine = undo::State<Run, Left, TorrentCommand>;

/// A command sent for several torrents: answered once each of its steps ended, with those that failed.
pub(super) struct Batch {
    left: usize,
    failed: Vec<(TorrentId, Failure)>,
    done: Done<Vec<(TorrentId, Failure)>>,
}

impl MemoryService {
    /// `command` for each of `ids`, each a step of its own, answered together through `done`. As the
    /// engine's: refused at once for a torrent clients don't see, or when what they see of it (as
    /// the steps before it on its lane will leave it) refuses it.
    pub(super) fn arrive_all(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Vec<(TorrentId, Failure)>>) {
        self.probe.1.lock().unwrap().push((ids.to_vec(), command.clone()));
        let action = command.action();
        let mut refused = Vec::new();
        let mut doing = Vec::new();
        for &id in ids {
            let files = match &command {
                TorrentCommand::TrashFiles { files } | TorrentCommand::SetPriority { files, .. } => files.as_slice(),
                TorrentCommand::Pause
                | TorrentCommand::Resume
                | TorrentCommand::Check
                | TorrentCommand::CancelCheck
                | TorrentCommand::Remove
                | TorrentCommand::Delete => &[],
            };
            let has = |i: &usize| self.files.get(&id).is_none_or(|fs| fs.iter().any(|f| f.index == *i));
            let why = match self.listed(id) {
                None => Some(proto::TorrentRefusal::NotInList),
                Some(t) => match t.actions.get(action) {
                    proto::Answer::Refused { why } => Some(why.clone()),
                    proto::Answer::Does | proto::Answer::DoesNothing { .. } => {
                        files.iter().find(|i| !has(i)).map(|index| proto::TorrentRefusal::NoFile { index: *index })
                    }
                },
            };
            match why {
                Some(why) => refused.push((id, Failure::Refused(Refusal::Torrent { id, why }))),
                None => doing.push(id),
            }
        }
        let batch = self.next_batch;
        self.next_batch += 1;
        self.batches.insert(batch, Batch { left: doing.len(), failed: refused, done });
        for id in doing {
            let what = match &command {
                TorrentCommand::Remove => Arrival::Action(self.run(id, UndoAction::Remove, Vec::new())),
                TorrentCommand::Delete => Arrival::Action(self.run(id, UndoAction::Delete, Vec::new())),
                TorrentCommand::TrashFiles { files } => {
                    Arrival::Action(self.run(id, UndoAction::TrashFiles { count: files.len() }, files.clone()))
                }
                other @ (TorrentCommand::Pause
                | TorrentCommand::Resume
                | TorrentCommand::Check
                | TorrentCommand::CancelCheck
                | TorrentCommand::SetPriority { .. }) => Arrival::Command(other.clone()),
            };
            self.decide(Observed::Arrived { seq: 0, torrents: BTreeSet::from([id]), what });
            self.batch_of.insert(self.undo.last, batch);
        }
        self.batch_ended(batch);
        self.changed = true;
    }

    /// What an action of torrent `id` holds as it comes: the torrent as clients see it now.
    fn run(&self, id: TorrentId, action: UndoAction, asked: Vec<usize>) -> Run {
        let mut left = self.keep(id, action);
        if let Some(info) = self.listed(id) {
            left.info = TorrentInfo { download_bytes_per_sec: 0, upload_bytes_per_sec: 0, live_peers: 0, ..info };
        }
        Run { action, left, asked, previous: Vec::new() }
    }

    pub(super) fn decide(&mut self, observed: Observed<Run, Left, TorrentCommand>) {
        if let Observed::Finished(seq, _) | Observed::Failed(seq) = &observed {
            self.world.started.remove(seq);
            self.world.aborted.remove(seq);
        }
        self.undo = undo::decide(&self.undo, &undo::Event::Observed(observed)).expect("an observation is never refused");
        self.changed = true;
    }

    /// Step `seq` of a command ended: `failed`, if it did. Its batch is answered once all of its did.
    fn step_answered(&mut self, seq: u64, failed: Option<(TorrentId, Failure)>) {
        let failed = match (self.orphaned.remove(&seq), failed) {
            (true, Some((id, _))) => Some((id, Failure::Refused(Refusal::Undone))),
            (_, failed) => failed,
        };
        let Some(batch) = self.batch_of.remove(&seq) else { return };
        if let Some(b) = self.batches.get_mut(&batch) {
            b.failed.extend(failed);
            b.left = b.left.saturating_sub(1);
        }
        self.batch_ended(batch);
    }

    fn batch_ended(&mut self, batch: usize) {
        if self.batches.get(&batch).is_some_and(|b| b.left == 0)
            && let Some(Batch { failed, done, .. }) = self.batches.remove(&batch)
        {
            let after = self.stale_answers * super::STALE[self.commands % super::STALE.len()];
            self.commands += 1;
            self.ready.push((answer(done, failed), after));
        }
    }

    /// Undo: the top step at once, whatever runs, as the engine's.
    pub(super) fn undo(&mut self, done: Done<Result<Undoable, Failure>>) {
        let Some(top) = self.undo.top().cloned() else {
            return self.ready.push((answer(done, Err(Failure::Refused(Refusal::NothingToUndo))), 0.0));
        };
        let queued = self.undo.steps.get(&top.seq).filter(|s| s.phase == Phase::Queued).map(|s| s.what.clone());
        self.undo = undo::decide(&self.undo, &undo::Event::Command(undo::Command::Undo)).expect("something on the stack");
        self.changed = true;
        match queued {
            // Dropped before it started: it changed nothing.
            Some(What::Action(run)) => {
                let id = run.left.info.id;
                self.step_answered(top.seq, Some((id, Failure::Refused(Refusal::Undone))));
                self.ready.push((answer(done, Ok(run.left.undoable(top.seq))), 0.0));
            }
            Some(What::Undo { .. } | What::Command(_)) | None => drop(self.undo_replies.insert(top.seq, done)),
        }
    }

    /// Starts each step the machine says to, at `now` on the app clock.
    pub(super) fn run_steps(&mut self, now: f64) {
        while let Some(todo) = undo::next(&self.undo, &self.world) {
            match todo {
                Do::Start(seq) => {
                    self.world.started.insert(seq);
                    self.start(seq, now);
                }
                // The fake's moves are one, in flight once started: it ends as it would.
                Do::Abort(seq) => drop(self.world.aborted.insert(seq)),
            }
        }
    }

    fn start(&mut self, seq: u64, now: f64) {
        let Some(step) = self.undo.steps.get(&seq) else { return };
        debug_assert_eq!(step.torrents.len(), 1, "a step of one torrent, as the engine's");
        let Some(&id) = step.torrents.first() else { return };
        let secs = match &step.what {
            What::Action(run) => match run.action {
                UndoAction::Remove => return self.removed(seq, id),
                UndoAction::Delete => {
                    self.event(id, Event::Command(proto::Action::Delete));
                    // Without info it has no files: it leaves, as on a Remove.
                    if !self.machines.get(&id).is_some_and(|m| matches!(m.state.phase, Lifecycle::Deleting { .. })) {
                        return self.removed(seq, id);
                    }
                    self.trash_secs
                }
                UndoAction::TrashFiles { .. } => {
                    let asked = run.asked.clone();
                    self.files_off(seq, id, &asked);
                    self.trash_secs
                }
            },
            What::Undo { .. } => self.undo_secs,
            What::Command(command) => {
                let command = command.clone();
                let failed = self.command_now(id, &command);
                self.decide(Observed::Finished(seq, None));
                return self.step_answered(seq, failed);
            }
        };
        match secs > 0.0 {
            true => drop(self.due.insert(seq, now + secs)),
            false => self.finish(seq),
        }
    }

    /// The steps whose time on the app clock has come end, and what they let start starts.
    pub(super) fn steps_due(&mut self, now: f64) {
        loop {
            let due: Vec<u64> = self.due.iter().filter(|(_, at)| **at <= now).map(|(seq, _)| *seq).collect();
            if due.is_empty() {
                return;
            }
            for seq in due {
                self.due.remove(&seq);
                self.finish(seq);
            }
            self.run_steps(now);
        }
    }

    /// Step `seq`, started, ends: a Delete's files go to the Trash, a Move to Trash's, an undo
    /// puts back what its action did.
    fn finish(&mut self, seq: u64) {
        let Some(step) = self.undo.steps.get(&seq).cloned() else { return };
        let Some(&id) = step.torrents.first() else { return };
        let aborted = step.phase == Phase::Aborting;
        match step.what {
            What::Action(run) if run.action == UndoAction::Delete => {
                let failed = self.trashed(id);
                let how = match self.torrents.iter().any(|t| t.id == id) {
                    false => Observed::Finished(seq, Some(run.left.clone())),
                    // Refused, or partly done: it stays, and isn't on the stack.
                    true => Observed::Failed(seq),
                };
                self.ended(seq, id, how, failed, aborted, run.left.undoable(seq));
            }
            What::Action(run) => {
                let (left, failed) = self.files_trashed(id, &run);
                let how = match (left, aborted) {
                    (Some(left), _) => Observed::Finished(seq, Some(left)),
                    // Aborted, it puts each file's priority back all the same.
                    (None, true) => {
                        let trashed = FilesTrashed { previous: run.previous.clone(), moved: Vec::new() };
                        Observed::Finished(seq, Some(Left { trashed: Some(trashed), ..run.left.clone() }))
                    }
                    (None, false) => Observed::Failed(seq),
                };
                self.ended(seq, id, how, failed, aborted, run.left.undoable(seq));
            }
            What::Undo { of, entry } => {
                let undone = self.undone(&entry, of);
                // It didn't bring its torrent back: what waits behind it is undone too.
                if undone.is_err() && entry.trashed.is_none() {
                    let behind = self.undo.lanes.get(&id).into_iter().flatten().filter(|s| **s != seq);
                    self.orphaned.extend(behind.copied().collect::<Vec<_>>());
                }
                self.decide(match &undone {
                    Ok(_) => Observed::Finished(seq, None),
                    Err(_) => Observed::Failed(seq),
                });
                if let Some(done) = self.undo_replies.remove(&of) {
                    self.ready.push((answer(done, undone), 0.0));
                }
            }
            What::Command(_) => self.decide(Observed::Finished(seq, None)),
        }
    }

    /// Action `seq` of torrent `id` ended as `how`, `failed` if it did. One aborted is answered as
    /// undone, and rolled back next if it changed anything; nothing changed, its Undo is answered.
    fn ended(
        &mut self,
        seq: u64,
        id: TorrentId,
        how: Observed<Run, Left, TorrentCommand>,
        failed: Option<(TorrentId, Failure)>,
        aborted: bool,
        undoable: Undoable,
    ) {
        self.decide(how);
        if !aborted {
            return self.step_answered(seq, failed);
        }
        self.step_answered(seq, Some((id, Failure::Refused(Refusal::Undone))));
        let rolling_back = self.undo.steps.get(&seq).is_some_and(|s| matches!(s.what, What::Undo { .. }));
        if !rolling_back && let Some(done) = self.undo_replies.remove(&seq) {
            self.ready.push((answer(done, Ok(undoable)), 0.0));
        }
    }

    /// Remove, step `seq`: torrent `id` leaves the list; what brings it back ends the step.
    fn removed(&mut self, seq: u64, id: TorrentId) {
        self.event(id, Event::Command(proto::Action::Remove));
        let left = match self.undo.steps.get(&seq).map(|s| &s.what) {
            Some(What::Action(run)) => Some(run.left.clone()),
            Some(What::Undo { .. } | What::Command(_)) | None => None,
        };
        self.forget(&[id]);
        self.decide(match left {
            Some(left) => Observed::Finished(seq, Some(left)),
            None => Observed::Failed(seq),
        });
        self.step_answered(seq, None);
    }

    /// Move to Trash, step `seq`, starts: the files asked for are Off at once, the priorities they
    /// had kept in the step.
    fn files_off(&mut self, seq: u64, id: TorrentId, asked: &[usize]) {
        let previous: Vec<(usize, Priority)> =
            self.files.get(&id).into_iter().flatten().filter(|f| asked.contains(&f.index)).map(|f| (f.index, f.priority)).collect();
        for f in self.files.get_mut(&id).into_iter().flatten().filter(|f| asked.contains(&f.index)) {
            f.priority = Priority::Off;
        }
        // Off first, so the size check that follows the move never finds a file it gets gone.
        self.size_check(id);
        self.refresh(id);
        if let Some(step) = self.undo.steps.get(&seq)
            && let What::Action(run) = &step.what
        {
            let run = Run { previous, ..run.clone() };
            self.decide(Observed::Moved(seq, run));
        }
    }

    /// What clients see of torrent `id`, as the engine's ([`view`]): none while its Remove or Delete
    /// is on the stack; listed as it was while an Undo brings it back; with the priorities the steps on
    /// its lane make, and the lifecycle they, and the commands waiting there, will leave it in.
    pub(super) fn shown(&self, id: TorrentId) -> Option<view::Shown<'_>> {
        view::shown(self, &self.undo, id)
    }

    /// Torrent `id` as clients see it listed now.
    pub(super) fn listed(&self, id: TorrentId) -> Option<TorrentInfo> {
        view::listed(self, &self.undo, id)
    }

    /// The torrents clients see, by id.
    pub(super) fn listing(&self) -> Vec<TorrentInfo> {
        view::listing(self, &self.undo)
    }

    /// What clients see of the undo stack, oldest first.
    pub(super) fn stacked(&self) -> Vec<Undoable> {
        self.undo
            .stack
            .iter()
            .filter_map(|p| match (&p.done, self.undo.steps.get(&p.seq).map(|s| &s.what)) {
                (Some(left), _) => Some(left.undoable(p.seq)),
                (None, Some(What::Action(run))) => Some(Undoable { action: run.action, ..run.left.undoable(p.seq) }),
                (None, Some(What::Undo { .. } | What::Command(_)) | None) => None,
            })
            .collect()
    }

    /// The answers ready to hand over.
    pub(super) fn take_ready(&mut self) -> Vec<(Incoming, f64)> {
        std::mem::take(&mut self.ready)
    }
}

/// What the steps kept of the torrents, for the fold of what clients see ([`view`]), the engine's.
impl view::Torrents<Run, Left> for MemoryService {
    fn ids(&self) -> BTreeSet<TorrentId> {
        self.torrents.iter().map(|t| t.id).collect()
    }

    fn now(&self, id: TorrentId) -> Option<Option<State>> {
        let listed = self.torrents.iter().any(|t| t.id == id)
            && self.machines.get(&id).is_none_or(|m| !matches!(m.state.phase, Lifecycle::Deleting { .. } | Lifecycle::Deleted));
        listed.then(|| self.machines.get(&id).map(|m| m.state.clone()))
    }

    fn row(&self, id: TorrentId, priorities: &BTreeMap<usize, Priority>, state: Option<&State>) -> Option<TorrentInfo> {
        let mut t = self.torrents.iter().find(|t| t.id == id)?.clone();
        if !priorities.is_empty() {
            let files: Vec<proto::File> = self
                .files
                .get(&id)?
                .iter()
                .map(|f| proto::File { priority: priorities.get(&f.index).copied().unwrap_or(f.priority), ..f.clone() })
                .collect();
            super::sync(&mut t, &files, &self.missing(id));
        }
        Some(view::seen_as(t, state))
    }

    fn acting<'a>(&'a self, run: &'a Run) -> view::Acting<'a> {
        match view::leaves(run.action) {
            true => view::Acting::Leaves { was: Some(view::Was { info: &run.left.info, state: Some(run.left.machine.state.clone()) }) },
            false => view::Acting::Trashes { asked: &run.asked, previous: run.previous.clone() },
        }
    }

    fn undoing<'a>(&'a self, left: &'a Left) -> view::Undoing<'a> {
        match &left.trashed {
            None => view::Undoing::BringsBack { was: Some(view::Was { info: &left.info, state: Some(left.machine.state.clone()) }) },
            Some(trashed) => view::Undoing::Restores { previous: trashed.previous.clone() },
        }
    }
}
