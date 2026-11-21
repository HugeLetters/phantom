//! The fake's side of Undo's machine ([`lifecycle::undo`], #190), the engine's own: every command
//! for a torrent is a step in its lane from the moment it comes; a Remove, a Delete and a Move to
//! Trash are on the undo stack too, in the order they came. The machine says which to start; the
//! fake carries each out, at once or once its time on the app clock has passed (`trash_takes`,
//! `undo_takes`), and reports how it ended. What the snapshot lists is what the steps make of the
//! torrents, as the engine's: a torrent whose Remove or Delete came is gone from the list at once;
//! one an Undo is bringing back is listed again as it was; files a Move to Trash or its undo is for
//! have their new priorities at once.

use std::collections::{BTreeMap, BTreeSet};

use lifecycle::undo::{self, Arrival, Do, Observed, Part, Phase, What, view};
use lifecycle::{Event, Phase as Lifecycle, State};
use proto::{Named, NotBack, Priority, Refusal, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable, Undone};

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
    /// A Rename's new name.
    pub rename: Option<String>,
}

pub(super) type Machine = undo::State<Run, Left, TorrentCommand>;

/// How the parts of an undo went so far, by the seq of the action it takes back.
#[derive(Default)]
pub(super) struct Tally {
    action: Option<UndoAction>,
    back: Vec<Named>,
    refused: Vec<NotBack>,
    /// A part's failure, as it was: an undo of one torrent is answered with it, as the engine's.
    failed: Option<Failure>,
}

/// Why an undo that ended so couldn't be done, as the engine says it: its own refusal's reason, or
/// else the failure itself.
fn cant_undo(failure: &Failure) -> (proto::CantUndo, Vec<proto::AbsolutePath>) {
    match failure {
        Failure::Refused(Refusal::Undo { why, stranded, .. }) => (why.clone(), stranded.clone()),
        Failure::Failed(proto::ServiceError::Internal { detail }) => (proto::CantUndo::Failed { detail: detail.clone() }, Vec::new()),
        Failure::Failed(error) => (proto::CantUndo::Error { error: error.clone() }, Vec::new()),
        Failure::Refused(refusal) => (proto::CantUndo::Refused { refusal: Box::new(refusal.clone()) }, Vec::new()),
        Failure::Error(e) => (proto::CantUndo::Failed { detail: format!("{e:#}") }, Vec::new()),
    }
}

/// A command sent for several torrents: answered once each of its parts ended, with those that failed.
pub(super) struct Batch {
    left: usize,
    failed: Vec<(TorrentId, Failure)>,
    done: Done<Vec<(TorrentId, Failure)>>,
}

impl MemoryService {
    /// `command` for each of `ids`, answered together through `done`: a Remove, a Delete or a Move to
    /// Trash one step on all their lanes (one press, one step: #235), any other a step each. As the
    /// engine's: refused at once for a torrent clients don't see, or when what they see of it (as
    /// the steps before it on its lane will leave it) refuses it.
    pub(super) fn arrive_all(
        &mut self,
        ids: &[TorrentId],
        command: TorrentCommand,
        clashes: &BTreeMap<TorrentId, proto::Answered>,
        done: Done<Vec<(TorrentId, Failure)>>,
    ) {
        // A Move of several is told as each torrent's own, in order, with the choice made for it.
        match &command {
            TorrentCommand::Move { to, .. } => {
                let mut probe = self.probe.1.lock().unwrap();
                probe.extend(ids.iter().map(|id| (vec![*id], TorrentCommand::Move { to: to.clone(), clash: clashes.get(id).cloned() })));
            }
            _ => self.probe.1.lock().unwrap().push((ids.to_vec(), command.clone())),
        }
        let action = command.action();
        // Cancel move takes a Move out of the queue as it comes, on no lane (#270).
        if command == TorrentCommand::CancelMove {
            let failed = ids
                .iter()
                .filter_map(|&id| {
                    let why = match (self.listed(id).is_some(), self.dequeue_move(id)) {
                        (false, _) => proto::TorrentRefusal::NotInList,
                        (true, true) => return None,
                        (true, false) => proto::TorrentRefusal::NotWaitingToMove,
                    };
                    Some((id, Failure::Refused(Refusal::Torrent { id, why })))
                })
                .collect();
            self.ready.push((answer(done, failed), 0.0));
            self.changed = true;
            return;
        }
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
                | TorrentCommand::Delete
                | TorrentCommand::Move { .. }
                | TorrentCommand::Rename { .. }
                | TorrentCommand::CancelMove => &[],
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
        let action = match &command {
            TorrentCommand::Remove => Some((UndoAction::Remove, Vec::new())),
            TorrentCommand::Delete => Some((UndoAction::Delete, Vec::new())),
            TorrentCommand::TrashFiles { files } => Some((UndoAction::TrashFiles { count: files.len() }, files.clone())),
            TorrentCommand::Rename { .. } => Some((UndoAction::Rename, Vec::new())),
            TorrentCommand::Pause
            | TorrentCommand::Resume
            | TorrentCommand::Check
            | TorrentCommand::CancelCheck
            | TorrentCommand::SetPriority { .. }
            | TorrentCommand::Move { .. }
            | TorrentCommand::CancelMove => None,
        };
        // A Remove or a Delete cancels a move of its torrent first, as it comes (#24).
        if let TorrentCommand::Remove | TorrentCommand::Delete = &command {
            for &id in &doing {
                self.cancel_move(id, command.action());
            }
        }
        match action {
            Some((action, asked)) if !doing.is_empty() => {
                let rename = match &command {
                    TorrentCommand::Rename { name } => Some(name.to_string()),
                    _ => None,
                };
                let parts = doing.iter().map(|&id| (id, Run { rename: rename.clone(), ..self.run(id, action, asked.clone()) })).collect();
                self.decide(Observed::Arrived { seq: 0, what: Arrival::Action(parts) });
                let seq = self.undo.last;
                self.batch_of.extend(doing.iter().map(|&id| ((seq, id), batch)));
            }
            Some(_) => {}
            None => {
                for id in doing {
                    let command = match (&command, clashes.get(&id)) {
                        (TorrentCommand::Move { to, .. }, Some(clash)) => {
                            TorrentCommand::Move { to: to.clone(), clash: Some(clash.clone()) }
                        }
                        _ => command.clone(),
                    };
                    self.decide(Observed::Arrived { seq: 0, what: Arrival::Command(id, command) });
                    self.batch_of.insert((self.undo.last, id), batch);
                }
            }
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
        Run { action, left, asked, previous: Vec::new(), rename: None }
    }

    pub(super) fn decide(&mut self, observed: Observed<Run, Left, TorrentCommand>) {
        let event = undo::Event::Observed(observed);
        self.undo = undo::decide(&self.undo, &event).expect("an observation is never refused");
        self.world.follow(&event, &self.undo);
        self.changed = true;
    }

    /// Step `seq`'s part on torrent `id` ended: `failed`, if it did. Its batch is answered once all
    /// of its parts did.
    pub(super) fn step_answered(&mut self, seq: u64, id: TorrentId, failed: Option<(TorrentId, Failure)>) {
        let failed = match (self.orphaned.remove(&(seq, id)), failed) {
            (true, Some((id, _))) => Some((id, Failure::Refused(Refusal::Undone))),
            (_, failed) => failed,
        };
        let Some(batch) = self.batch_of.remove(&(seq, id)) else { return };
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
    pub(super) fn undo(&mut self, done: Done<Result<Undone, Failure>>) {
        let Some(top) = self.undo.top().cloned() else {
            return self.ready.push((answer(done, Err(Failure::Refused(Refusal::NothingToUndo))), 0.0));
        };
        let asked = self.undoable(top.seq);
        // A Rename's whose torrent is moving is refused rather than wait (#331), as the engine's.
        if self.busy_rename()
            && let Some(undo) = asked.clone()
        {
            let why = proto::CantUndo::Busy;
            self.undo = undo::decide(&self.undo, &undo::Event::Command(undo::Command::Refuse(why.clone()))).expect("on the stack");
            self.changed = true;
            return self.ready.push((answer(done, Err(Failure::Refused(Refusal::Undo { undo, why, stranded: Vec::new() }))), 0.0));
        }
        let queued = self.undo.steps.get(&top.seq).filter(|s| s.phase == Phase::Queued).map(|s| s.torrents.clone());
        self.undo = undo::decide(&self.undo, &undo::Event::Command(undo::Command::Undo)).expect("something on the stack");
        self.changed = true;
        match (queued, asked) {
            // Dropped before it started: it changed nothing.
            (Some(torrents), Some(undo)) => {
                for id in torrents {
                    self.step_answered(top.seq, id, Some((id, Failure::Refused(Refusal::Undone))));
                }
                self.ready.push((answer(done, Ok(Undone { undo, refused: Vec::new() })), 0.0));
            }
            // Running or done: answered once it's back, or why not.
            (_, asked) => drop(self.undo_replies.insert(top.seq, (done, asked))),
        }
    }

    /// Starts each step the machine says to, at `now` on the app clock.
    pub(super) fn run_steps(&mut self, now: f64) {
        while let Some(todo) = undo::next(&self.undo, &self.world) {
            match todo {
                Do::Start(seq, id) => {
                    self.world.started.insert((seq, id));
                    self.start(seq, id, now);
                }
                // The fake's moves are one, in flight once started: it ends as it would.
                Do::Abort(seq) => drop(self.world.aborted.insert(seq)),
            }
        }
    }

    fn start(&mut self, seq: u64, id: TorrentId, now: f64) {
        let Some(step) = self.undo.steps.get(&seq) else { return };
        let secs = match &step.what {
            What::Action(parts) => match parts.get(&id) {
                Some(Part::Doing(run)) => match run.action {
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
                    // Renamed as it starts: the fake's disk renames at once.
                    UndoAction::Rename => return self.renamed(seq, id, run.rename.clone().unwrap_or_default()),
                },
                Some(Part::Over(_)) | None => return,
            },
            What::Undo { .. } => self.undo_secs,
            // Runs until the move is over (`moving.rs`), but refused or there already.
            What::Command(proto::TorrentCommand::Move { to, clash }) => {
                let (to, clash) = (to.clone(), clash.clone());
                if let Some(failed) = self.start_move(seq, id, &to, clash.as_ref()) {
                    self.decide(Observed::Finished(seq, id, None));
                    self.step_answered(seq, id, failed);
                }
                return;
            }
            What::Command(command) => {
                let command = command.clone();
                let failed = self.command_now(id, &command);
                self.decide(Observed::Finished(seq, id, None));
                return self.step_answered(seq, id, failed);
            }
        };
        match secs > 0.0 {
            true => drop(self.due.insert((seq, id), now + secs)),
            false => self.finish(seq, id),
        }
    }

    /// The steps whose time on the app clock has come end, and what they let start starts.
    pub(super) fn steps_due(&mut self, now: f64) {
        loop {
            let due: Vec<(u64, TorrentId)> = self.due.iter().filter(|(_, at)| **at <= now).map(|(part, _)| *part).collect();
            if due.is_empty() {
                return;
            }
            for (seq, id) in due {
                self.due.remove(&(seq, id));
                self.finish(seq, id);
            }
            self.run_steps(now);
        }
    }

    /// Step `seq`'s part on torrent `id`, started, ends: a Delete's files go to the Trash, a Move to
    /// Trash's, an undo puts back what its action did.
    fn finish(&mut self, seq: u64, id: TorrentId) {
        let Some(step) = self.undo.steps.get(&seq).cloned() else { return };
        let aborted = step.phase == Phase::Aborting;
        match step.what {
            What::Action(parts) => {
                let Some(Part::Doing(run)) = parts.get(&id).cloned() else { return };
                let how = match run.action {
                    UndoAction::Delete => {
                        let partly =
                            self.torrents.iter().find(|t| t.id == id).is_some_and(|t| self.trash_partly.contains(t.display_name()));
                        let failed = self.trashed(id);
                        let how = match self.torrents.iter().any(|t| t.id == id) {
                            false => Observed::Finished(seq, id, Some(run.left.clone())),
                            // Partly done: it stays in the list, and what moved is on the stack, as the engine's.
                            true if partly => {
                                let moved = self.first_in_the_trash(id).into_iter().map(|(i, stamp)| (i, stamp, 0)).collect();
                                let previous = self.files.get(&id).into_iter().flatten().map(|f| (f.index, f.priority)).collect();
                                let trashed = Some(FilesTrashed { previous, moved });
                                Observed::Finished(seq, id, Some(Left { trashed, ..run.left.clone() }))
                            }
                            // Refused: it stays as it was, and changed nothing.
                            true => Observed::Failed(seq, id),
                        };
                        (how, failed)
                    }
                    // Over as it started (`renamed`).
                    UndoAction::Rename => return,
                    UndoAction::Remove | UndoAction::TrashFiles { .. } => {
                        let (left, failed) = self.files_trashed(id, &run);
                        let how = match (left, aborted) {
                            (Some(left), _) => Observed::Finished(seq, id, Some(left)),
                            // Aborted, it puts each file's priority back all the same.
                            (None, true) => {
                                let trashed = FilesTrashed { previous: run.previous.clone(), moved: Vec::new() };
                                Observed::Finished(seq, id, Some(Left { trashed: Some(trashed), ..run.left.clone() }))
                            }
                            (None, false) => Observed::Failed(seq, id),
                        };
                        (how, failed)
                    }
                };
                self.ended(seq, id, how.0, how.1, aborted);
            }
            What::Undo { of, parts } => {
                let Some(Part::Doing(entry)) = parts.get(&id).cloned() else { return };
                let undone = self.undone(&entry, of);
                // It didn't bring its torrent back: what waits behind it is undone too.
                if undone.is_err() && entry.trashed.is_none() {
                    let behind = self.undo.lanes.get(&id).into_iter().flatten().filter(|s| **s != seq);
                    let behind: Vec<(u64, TorrentId)> = behind.map(|s| (*s, id)).collect();
                    self.orphaned.extend(behind);
                }
                // Failed for no reason of Undo's: still refused, with the failure as its reason, as the engine's.
                self.decide(match &undone {
                    Ok(_) => Observed::Finished(seq, id, None),
                    Err(failure) => {
                        let (why, stranded) = cant_undo(failure);
                        Observed::Refused { seq, torrent: id, why, stranded }
                    }
                });
                let tally = self.tallies.entry(of).or_default();
                let named = Named { id, name: entry.info.display_name().to_owned() };
                match undone {
                    Ok(undo) => {
                        tally.action.get_or_insert(undo.action);
                        tally.back.extend(undo.torrents);
                    }
                    Err(failure) => {
                        let (why, stranded) = cant_undo(&failure);
                        tally.action.get_or_insert(entry.action);
                        tally.refused.push(NotBack { torrent: named, action: entry.action, why, stranded });
                        tally.failed = Some(failure);
                    }
                }
                // Once each part is over, its client hears what came back, and what didn't.
                if !self.undo.steps.contains_key(&seq) {
                    self.undo_answered(of, entry.action);
                }
            }
            What::Command(_) => self.decide(Observed::Finished(seq, id, None)),
        }
    }

    /// Every part of the undo of action `of` is over: its client hears what came back, and what didn't.
    fn undo_answered(&mut self, of: u64, action: UndoAction) {
        let tally = self.tallies.remove(&of).unwrap_or_default();
        let result = match (tally.back.is_empty(), tally.failed, &tally.refused[..]) {
            // Of one torrent, its failure as it was.
            (true, Some(failed), [_]) => Err(failed),
            (false, _, _) | (true, None, _) | (true, Some(_), [] | [_, _, ..]) => {
                undo::undone(of, tally.action.unwrap_or(action), tally.back, tally.refused).map_err(Failure::Refused)
            }
        };
        if let Some((done, _)) = self.undo_replies.remove(&of) {
            self.ready.push((answer(done, result), 0.0));
        }
    }

    /// Action `seq`'s part on torrent `id` ended as `how`, `failed` if it did. One aborted is answered
    /// as undone, and rolled back next, once every part is over, if it changed anything; nothing
    /// changed, its Undo is answered.
    fn ended(
        &mut self,
        seq: u64,
        id: TorrentId,
        how: Observed<Run, Left, TorrentCommand>,
        failed: Option<(TorrentId, Failure)>,
        aborted: bool,
    ) {
        self.decide(how);
        if !aborted {
            return self.step_answered(seq, id, failed);
        }
        self.step_answered(seq, id, Some((id, Failure::Refused(Refusal::Undone))));
        let over = !self.undo.steps.contains_key(&seq);
        if over && let Some((done, Some(undo))) = self.undo_replies.remove(&seq) {
            self.ready.push((answer(done, Ok(Undone { undo, refused: Vec::new() })), 0.0));
        }
    }

    /// A partly done Delete moved the torrent's first file on disk to the Trash: what it was there.
    fn first_in_the_trash(&mut self, id: TorrentId) -> Vec<(usize, crate::service::memory::Stamp)> {
        let Some(machine) = self.machines.get_mut(&id) else { return Vec::new() };
        let first = machine.on_disk.iter_mut().find_map(|(index, stamp)| Some((*index, stamp.take()?)));
        first.into_iter().collect()
    }

    /// Remove, step `seq`'s part on torrent `id`: it leaves the list; what brings it back ends the part.
    fn removed(&mut self, seq: u64, id: TorrentId) {
        self.event(id, Event::Command(proto::Action::Remove));
        let left = self.part(seq, id).map(|run| run.left.clone());
        let aborted = self.undo.steps.get(&seq).is_some_and(|s| s.phase == Phase::Aborting);
        self.forget(&[id]);
        let how = match left {
            Some(left) => Observed::Finished(seq, id, Some(left)),
            None => Observed::Failed(seq, id),
        };
        self.ended(seq, id, how, None, aborted);
    }

    /// Rename, step `seq`'s part on torrent `id`, to `name`: renamed now, or why not; a Remove or a
    /// Delete waiting behind it cancels it, as the engine's.
    fn renamed(&mut self, seq: u64, id: TorrentId, name: String) {
        let aborted = self.undo.steps.get(&seq).is_some_and(|s| s.phase == Phase::Aborting);
        let refused = |why| Some((id, Failure::Refused(Refusal::Torrent { id, why })));
        let named = || self.torrents.iter().find(|t| t.id == id).map_or_else(|| format!("torrent {id}"), |t| t.display_name().to_owned());
        if self.removed_behind(seq, id) {
            let failed = refused(proto::TorrentRefusal::NotRenamed { name: named(), why: proto::NotMoved::Cancelled });
            return self.ended(seq, id, Observed::Failed(seq, id), failed, aborted);
        }
        let (how, failed) = match self.rename(id, &name) {
            Ok(Some(was)) => {
                let left = Left { renamed: Some(was), ..self.keep(id, UndoAction::Rename) };
                (Observed::Finished(seq, id, Some(left)), None)
            }
            Ok(None) => (Observed::Finished(seq, id, None), None),
            Err(why) => (Observed::Failed(seq, id), refused(why)),
        };
        self.ended(seq, id, how, failed, aborted);
    }

    /// What action `seq`'s part on torrent `id` holds, while it's under way.
    fn part(&self, seq: u64, id: TorrentId) -> Option<&Run> {
        match &self.undo.steps.get(&seq)?.what {
            What::Action(parts) => parts.get(&id)?.doing(),
            What::Undo { .. } | What::Command(_) => None,
        }
    }

    /// Move to Trash, step `seq`, starts: the files asked for are Off at once, the priorities they
    /// had kept in the step.
    fn files_off(&mut self, seq: u64, id: TorrentId, asked: &[usize]) {
        // Nothing for one not on disk (#276), as the engine's.
        let there = |i: &usize| self.machines.get(&id).is_some_and(|m| m.on_disk.get(i).is_some_and(Option::is_some));
        let asked: Vec<usize> = asked.iter().copied().filter(there).collect();
        let previous: Vec<(usize, Priority)> =
            self.files.get(&id).into_iter().flatten().filter(|f| asked.contains(&f.index)).map(|f| (f.index, f.priority)).collect();
        for f in self.files.get_mut(&id).into_iter().flatten().filter(|f| asked.contains(&f.index)) {
            f.priority = Priority::Off;
        }
        // Off first, so the size check that follows the move never finds a file it gets gone.
        self.size_check(id);
        self.refresh(id);
        if let Some(run) = self.part(seq, id) {
            let run = Run { previous, ..run.clone() };
            self.decide(Observed::Moved(seq, id, run));
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
        self.undo.stack.iter().filter_map(|p| self.undoable(p.seq)).collect()
    }

    /// What clients see of action `seq` on the stack, running or done, as the engine's: its
    /// torrents, but those whose part changed nothing; a Delete if any part is one.
    fn undoable(&self, seq: u64) -> Option<Undoable> {
        let done = self.undo.stack.iter().find(|p| p.seq == seq).and_then(|p| p.done.as_ref());
        let parts: Vec<Undoable> = match (done, self.undo.steps.get(&seq).map(|s| &s.what)) {
            (Some(done), _) => done.values().map(|left| left.undoable(seq)).collect(),
            (None, Some(What::Action(parts))) => parts
                .values()
                .filter_map(|p| match p {
                    Part::Doing(run) => Some(Undoable { action: run.action, ..run.left.undoable(seq) }),
                    Part::Over(Some(left)) => Some(left.undoable(seq)),
                    Part::Over(None) => None,
                })
                .collect(),
            (None, Some(What::Undo { .. } | What::Command(_)) | None) => Vec::new(),
        };
        let mut parts = parts.into_iter();
        let mut one = parts.next()?;
        for part in parts {
            if part.action == UndoAction::Delete {
                one.action = UndoAction::Delete;
            }
            one.torrents.extend(part.torrents);
        }
        Some(one)
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

    fn now(&self, id: TorrentId) -> Option<view::Now> {
        let listed = self.torrents.iter().any(|t| t.id == id)
            && self.machines.get(&id).is_none_or(|m| !matches!(m.state.phase, Lifecycle::Deleting { .. } | Lifecycle::Deleted));
        listed.then(|| self.machines.get(&id).map_or(view::Now::Without, |m| view::Now::With(m.state.clone())))
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
        match (view::leaves(run.action), run.action) {
            (_, UndoAction::Rename) => view::Acting::Renames,
            (true, _) => {
                view::Acting::Leaves { was: Some(view::Was { info: &run.left.info, state: Some(run.left.machine.state.clone()) }) }
            }
            (false, _) => view::Acting::Trashes { asked: &run.asked, previous: run.previous.clone() },
        }
    }

    fn undoing<'a>(&'a self, left: &'a Left) -> view::Undoing<'a> {
        match (&left.trashed, &left.renamed) {
            (_, Some(_)) => view::Undoing::Renames,
            (None, None) => {
                view::Undoing::BringsBack { was: Some(view::Was { info: &left.info, state: Some(left.machine.state.clone()) }) }
            }
            (Some(trashed), None) => view::Undoing::Restores { previous: trashed.previous.clone() },
        }
    }
}
