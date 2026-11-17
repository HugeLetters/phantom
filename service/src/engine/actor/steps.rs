//! Steps (#190, ADR 0008): the actor's side of Undo's machine ([`lifecycle::undo`]). Every command
//! for a torrent is a step there from the moment it comes, in its torrent's lane: a Remove, a
//! Delete or a Move to Trash also on the undo stack, in the order the commands came; an undo of one;
//! any other command. [`Steps`] holds the machine and the clients waiting on it: a step
//! [arrives](Steps::arrive), the actor carries out what [`Steps::next`] says (the file moves on
//! blocking tasks), [reports](Steps::report) what each did, and its client hears how it went
//! ([`Steps::answer`]); ⌘Z is [`Steps::undo`]. Lanes run side by side, so one torrent's Delete never
//! holds up another's.
//!
//! What clients see is what the steps make of the torrents ([`view`]): from the moment a Remove or
//! a Delete comes its torrent isn't listed; once ⌘Z takes it back it's listed again as it was when
//! the step came, until it's back for real; the files of a Move to Trash, or of its undo, have their
//! new priorities at once. A command for a torrent clients don't see is refused as not in the list.

use std::collections::{BTreeMap, BTreeSet};

use lifecycle::undo::view;
use proto::{Priority, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable};
use tokio::sync::oneshot;
use tracing::warn;

use super::super::Failure;
use super::super::undo::machine::{self as stack, Arrival, Command, Do, Observed, Phase, What};
use super::super::undo::{self, Back, Running, Seen};
use super::undoing::name_of;
use super::{Actor, Event, Refusal, State};

pub(super) type Reply = oneshot::Sender<Result<(), Failure>>;
pub(super) type UndoReply = oneshot::Sender<Result<Undoable, Failure>>;

/// Undo's machine as the actor drives it: the stack and the lanes, the steps the actor carries out,
/// and the clients waiting on them. The actor saves it after each change, written ahead.
pub(in super::super) struct Steps {
    machine: stack::State,
    /// The steps the actor carries out now: the machine's world.
    world: stack::World,
    /// The client waiting for each step, by its seq.
    replies: BTreeMap<u64, Reply>,
    /// The client waiting for each undo, by the seq of the action it takes back: none for one
    /// finished at start.
    undo_replies: BTreeMap<u64, UndoReply>,
    /// The steps that waited behind an undo bringing their torrent back that failed: one that fails
    /// in turn is answered `Undone`, as the undo's own refusal says why (#217's re-review, S2).
    orphaned: BTreeSet<u64>,
    /// The Cancel checks judged on the torrent as clients saw it, behind other steps: one that finds
    /// no check at its turn is done, as asked.
    cancels_judged: BTreeSet<u64>,
}

/// What the actor finds that ends no step.
pub(super) enum Fact {
    /// Running action `seq` moved an item: it holds this now.
    Moved(u64, Running),
    /// The service is stopping: what waits doesn't start, but the undos.
    Stopping,
    /// The engine starts again, after a stop or a crash.
    Restarted,
}

/// How a step ended: an action with what undoes it (none: it changed nothing), an undo carried out,
/// a command done; or one that couldn't.
pub(super) enum Over {
    Finished(Option<undo::Entry>),
    Failed,
}

/// A step over ([`Steps::end`]), whose client is still to hear how it went: whether an Undo aborted
/// it, what it was, and the action an undo was of. Only [`Steps::answer`] (or, for a step nobody
/// waits on, [`Steps::unanswered`]) takes it.
#[must_use = "its client hears how it went: `Steps::answer`, or `Steps::unanswered` when nobody waits"]
pub(super) struct Ended {
    seq: u64,
    aborted: bool,
    undoable: Option<Undoable>,
    of: Option<u64>,
}

/// How a step went, for its client: a command's or an action's, or an undo's.
pub(super) enum Outcome {
    Done(Result<(), Failure>),
    Undone(Result<Undoable, Failure>),
}

/// An answer to hand a client, once what it did is published.
pub(super) enum Answer {
    Step(Reply, Result<(), Failure>),
    Undo(UndoReply, Result<Undoable, Failure>),
}

impl Steps {
    pub(super) fn new(machine: stack::State) -> Self {
        Self {
            machine,
            world: Default::default(),
            replies: BTreeMap::new(),
            undo_replies: BTreeMap::new(),
            orphaned: BTreeSet::new(),
            cancels_judged: BTreeSet::new(),
        }
    }

    /// The stack and the lanes, as they are.
    pub(in super::super) fn machine(&self) -> &stack::State {
        &self.machine
    }

    /// A step for torrent `id` arrives, its client waiting on `reply`: its seq. Numbered from the
    /// clock, not only past the last, so seqs stay larger than any a client was told of, even once
    /// `undo.json` is lost (a notice's Undo goes by them).
    pub(super) fn arrive(&mut self, id: TorrentId, what: Arrival<Running, TorrentCommand>, reply: Option<Reply>) -> u64 {
        let judged = self.machine.lanes.contains_key(&id) && matches!(what, Arrival::Command(TorrentCommand::CancelCheck));
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        self.decide(stack::Event::Observed(Observed::Arrived { seq: nanos, torrents: BTreeSet::from([id]), what }));
        let seq = self.machine.last;
        if judged {
            self.cancels_judged.insert(seq);
        }
        if let Some(reply) = reply {
            self.replies.insert(seq, reply);
        }
        seq
    }

    /// What the actor found that ends no step: an item a running action moved, the service stopping
    /// (the answers of those it drops), the engine started again.
    pub(super) fn report(&mut self, fact: Fact) -> Vec<Answer> {
        let (observed, stopping) = match fact {
            Fact::Moved(seq, running) => (Observed::Moved(seq, running), false),
            Fact::Stopping => (Observed::Stopping, true),
            Fact::Restarted => (Observed::Restarted, false),
        };
        let before: Vec<u64> = self.machine.steps.keys().copied().collect();
        self.decide(stack::Event::Observed(observed));
        // Dropped by the stop: each client hears the service is stopping.
        let gone = before.into_iter().filter(|seq| stopping && !self.machine.steps.contains_key(seq));
        gone.collect::<Vec<_>>().into_iter().flat_map(|seq| self.answered(seq, Err(proto::Refusal::Stopping.into()))).collect()
    }

    /// Step `seq` is over as `over` says: the actor carries it out no more. Its client hears how it
    /// went once the actor knows, through what this returns.
    pub(super) fn end(&mut self, seq: u64, over: Over) -> Ended {
        self.world.started.remove(&seq);
        self.world.aborted.remove(&seq);
        let mut ended = Ended { seq, aborted: false, undoable: None, of: None };
        if let Some(step) = self.machine.steps.get(&seq) {
            ended.aborted = step.phase == Phase::Aborting;
            match &step.what {
                What::Action(r) => ended.undoable = Some(r.undoable(seq)),
                What::Undo { of, .. } => ended.of = Some(*of),
                What::Command(_) => {}
            }
            // An undo that didn't bring its torrent back: what waits behind it fails as not in the
            // list, which the undo's own refusal says better.
            if let (Over::Failed, What::Undo { entry, .. }) = (&over, &step.what)
                && matches!(entry.back, Back::Gone { .. } | Back::InList { .. })
            {
                let behind = self.machine.lanes.get(&entry.id).into_iter().flatten().filter(|s| **s != seq);
                self.orphaned.extend(behind.copied().collect::<Vec<_>>());
            }
        }
        let observed = match over {
            Over::Finished(entry) => Observed::Finished(seq, entry),
            Over::Failed => Observed::Failed(seq),
        };
        self.decide(stack::Event::Observed(observed));
        ended
    }

    /// A step over that no client waits on: one ended at a start, or committed there.
    pub(super) fn unanswered(&mut self, ended: Ended) {
        let waits = self.replies.contains_key(&ended.seq)
            || [Some(ended.seq), ended.of].into_iter().flatten().any(|s| self.undo_replies.contains_key(&s));
        debug_assert!(!waits, "step {} has a client, who must hear how it went", ended.seq);
    }

    /// The client of step `ended` hears how it went: one an Undo aborted hears it was undone, and the
    /// Undo's hears it's back, if it changed nothing (else once its rollback is over).
    pub(super) fn answer(&mut self, ended: Ended, outcome: Outcome) -> Vec<Answer> {
        let (seq, ending) = (ended.seq, ended);
        match outcome {
            Outcome::Done(answer) if !ending.aborted => self.answered(seq, answer),
            Outcome::Done(_) => {
                let mut answers = self.answered(seq, Err(proto::Refusal::Undone.into()));
                let rolling_back = self.machine.steps.get(&seq).is_some_and(|s| matches!(s.what, What::Undo { .. }));
                if !rolling_back && let (Some(reply), Some(undoable)) = (self.undo_replies.remove(&seq), ending.undoable) {
                    answers.push(Answer::Undo(reply, Ok(undoable)));
                }
                answers
            }
            Outcome::Undone(answer) => match ending.of.and_then(|of| self.undo_replies.remove(&of)) {
                Some(reply) => vec![Answer::Undo(reply, answer)],
                None => {
                    if let Err(e) = answer {
                        warn!(error = ?e, "failed to finish an undo cut short");
                    }
                    Vec::new()
                }
            },
        }
    }

    /// ⌘Z: the top step at once, whatever runs: one done gets its undo on its torrent's lane, one
    /// running is aborted, one queued is dropped (its client hears it was undone, and the Undo's
    /// that it's back). Otherwise the Undo's client hears once it's back, or why not.
    pub(super) fn undo(&mut self, reply: UndoReply) -> Vec<Answer> {
        let Some(top) = self.machine.top().cloned() else { return vec![Answer::Undo(reply, Err(proto::Refusal::NothingToUndo.into()))] };
        let queued = self.machine.steps.get(&top.seq).filter(|s| s.phase == Phase::Queued).map(|s| s.what.clone());
        self.decide(stack::Event::Command(Command::Undo));
        match queued {
            Some(What::Action(running)) => {
                let mut answers = self.answered(top.seq, Err(proto::Refusal::Undone.into()));
                answers.push(Answer::Undo(reply, Ok(running.undoable(top.seq))));
                answers
            }
            Some(What::Undo { .. } | What::Command(_)) | None => {
                self.undo_replies.insert(top.seq, reply);
                Vec::new()
            }
        }
    }

    /// What the actor starts or aborts now; it carries it out from then on.
    pub(super) fn next(&mut self) -> Option<Do> {
        let todo = stack::next(&self.machine, &self.world)?;
        match todo {
            Do::Start(seq) => self.world.started.insert(seq),
            Do::Abort(seq) => self.world.aborted.insert(seq),
        };
        Some(todo)
    }

    /// What undo step `seq` puts back, written again: a partial Delete's, once its torrent is taken
    /// out of the list, puts back one gone.
    pub(super) fn rewrite(&mut self, seq: u64, entry: undo::Entry) {
        if let Some(step) = self.machine.steps.get_mut(&seq)
            && let What::Undo { entry: kept, .. } = &mut step.what
        {
            *kept = entry;
        }
    }

    fn decide(&mut self, event: stack::Event) {
        self.machine = stack::decide(&self.machine, &event).expect("only an Undo is ever refused, of an empty stack, which `undo` answers");
    }

    /// Step `seq`'s client, if it waits, hears `answer`: one that waited behind an undo that failed
    /// hears it was undone; a Cancel check judged on what clients saw that finds no check at its
    /// turn is done, as asked.
    fn answered(&mut self, seq: u64, answer: Result<(), Failure>) -> Vec<Answer> {
        let answer = match (self.cancels_judged.remove(&seq), answer) {
            (true, Err(Failure::Refused(proto::Refusal::Torrent { why: Refusal::NotChecking, .. }))) => Ok(()),
            (_, answer) => answer,
        };
        let answer = match (self.orphaned.remove(&seq), answer) {
            (true, Err(_)) => Err(proto::Refusal::Undone.into()),
            (_, answer) => answer,
        };
        self.replies.remove(&seq).map(|reply| Answer::Step(reply, answer)).into_iter().collect()
    }
}

impl Actor {
    /// A command for torrent `id`: a step on its lane. Refused at once when clients don't see the
    /// torrent, or, while something runs on it, when its lifecycle refuses it now; one decided
    /// later and refused then never shows on the stack, which it leaves at once.
    pub(super) async fn arrive(&mut self, id: TorrentId, command: TorrentCommand, reply: Reply) {
        if self.shown(id).is_none() {
            return self.answer(reply, Err(Failure::torrent(id, Refusal::NotInList)));
        }
        // Judged on the torrent as clients see it: as the steps before it will leave it (back, once
        // an Undo took back its Delete; paused, once a Pause waiting before it has run).
        let behind = self.steps.machine().lanes.contains_key(&id);
        if behind
            && let Some(listed) = self.listed(id)
            && let proto::Answer::Refused { why } = listed.actions.get(command.action())
        {
            return self.answer(reply, Err(Failure::torrent(id, why.clone())));
        }
        // A file it doesn't have is known now too: refused before it's on the stack.
        if let (true, TorrentCommand::TrashFiles { files } | TorrentCommand::SetPriority { files, .. }) = (behind, &command)
            && let Some(Ok(listed)) = self.handle_of(id).ok().map(|h| super::files(h, &[], None, &BTreeSet::new()))
            && let Some(bad) = files.iter().find(|&&i| !listed.iter().any(|f| f.index == i))
        {
            return self.answer(reply, Err(Failure::torrent(id, Refusal::NoFile { index: *bad })));
        }
        let what = match command {
            TorrentCommand::Remove => Arrival::Action(self.running(id, UndoAction::Remove, Vec::new())),
            TorrentCommand::Delete => Arrival::Action(self.running(id, UndoAction::Delete, Vec::new())),
            TorrentCommand::TrashFiles { files } => Arrival::Action(self.running(id, UndoAction::TrashFiles { count: files.len() }, files)),
            other @ (TorrentCommand::Pause
            | TorrentCommand::Resume
            | TorrentCommand::Check
            | TorrentCommand::CancelCheck
            | TorrentCommand::SetPriority { .. }) => Arrival::Command(other),
        };
        self.steps.arrive(id, what, Some(reply));
        self.save_undo();
        self.run_steps().await;
    }

    /// What an action of torrent `id` holds as it comes: the torrent as clients see it now.
    fn running(&self, id: TorrentId, action: UndoAction, asked: Vec<usize>) -> Running {
        let listed = self.listed(id);
        let (name, info_hash) = match (self.torrents.get(&id), &listed) {
            (Some(t), _) => (name_of(t), t.info_hash.clone()),
            (None, Some(l)) => (l.display_name().to_owned(), l.info_hash.clone()),
            (None, None) => (format!("torrent {id}"), String::new()),
        };
        // As it was, at rest: its rates are of a moment that's gone once it's back.
        let listed = listed.map(|l| Seen { info: TorrentInfo { download_bytes_per_sec: 0, upload_bytes_per_sec: 0, live_peers: 0, ..l } });
        Running { action, name, info_hash, id, listed, previous: Vec::new(), had: Vec::new(), moved: Vec::new(), asked }
    }

    /// Starts each step the machine says to, and stops each it says to abort: after every message
    /// and every end of a task, never from inside a step (so a step ending at once doesn't recurse).
    pub(super) async fn run_steps(&mut self) {
        while let Some(todo) = self.steps.next() {
            match todo {
                Do::Start(seq) => self.start(seq).await,
                Do::Abort(seq) => self.abort(seq),
            }
        }
    }

    /// Reports what the actor found that ends no step ([`Steps::report`]), written ahead.
    pub(super) fn report(&mut self, fact: Fact) {
        let answers = self.steps.report(fact);
        self.save_undo();
        self.deliver(answers);
    }

    /// Step `seq` is over ([`Steps::end`]), written ahead: the adds that waited for its torrent go on
    /// once nothing runs on it. Its client hears how it went through [`Actor::answer_step`].
    pub(super) fn end(&mut self, seq: u64, over: Over) -> Ended {
        let ended = self.steps.end(seq, over);
        self.save_undo();
        for (prepared, paused, reply) in std::mem::take(&mut self.adds_waiting) {
            self.add(prepared, paused, reply);
        }
        ended
    }

    /// The client of step `ended` hears how it went ([`Steps::answer`]).
    pub(super) fn answer_step(&mut self, ended: Ended, outcome: Outcome) {
        let answers = self.steps.answer(ended, outcome);
        self.deliver(answers);
    }

    /// Hands each client its answer, once what it did is published.
    pub(super) fn deliver(&self, answers: Vec<Answer>) {
        if answers.is_empty() {
            return;
        }
        self.publish();
        for answer in answers {
            match answer {
                Answer::Step(reply, answer) => drop(reply.send(answer)),
                Answer::Undo(reply, answer) => drop(reply.send(answer)),
            }
        }
    }

    /// Starts step `seq`; one that ends at once is reported over. Each step is of one torrent:
    /// every command is (a Delete of several is one per torrent).
    async fn start(&mut self, seq: u64) {
        let Some(step) = self.steps.machine().steps.get(&seq).cloned() else { return };
        debug_assert_eq!(step.torrents.len(), 1, "a step of one torrent");
        let Some(&id) = step.torrents.first() else { return };
        match step.what {
            What::Action(running) => match running.action {
                UndoAction::Remove => self.start_remove(id, seq).await,
                UndoAction::Delete => self.delete(id, seq).await,
                UndoAction::TrashFiles { .. } => self.trash_files(id, seq, running.asked).await,
            },
            What::Undo { of, entry } => self.start_undo(seq, of, entry).await,
            What::Command(command) => {
                let answer = self.command(id, command).await;
                let ended = self.end(seq, Over::Finished(None));
                self.answer_step(ended, Outcome::Done(answer));
            }
        }
    }

    /// Remove, step `seq`, decided on librqbit and the disk as they are now.
    async fn start_remove(&mut self, id: TorrentId, seq: u64) {
        self.poll_torrent(id).await;
        self.size_check(id).await;
        let (ended, removed) = match self.event(id, Event::Command(proto::Action::Remove)).await {
            Ok(()) => {
                let (ended, removed) = self.remove(id, UndoAction::Remove, seq).await;
                (ended, removed.map_err(Failure::from))
            }
            Err(e) => (self.end(seq, Over::Failed), Err(e)),
        };
        self.answer_step(ended, Outcome::Done(removed));
    }

    /// Step `seq` is over as `over` says, its client answered `answer` at once.
    pub(super) fn step_ended(&mut self, seq: u64, over: Over, answer: Result<(), Failure>) {
        let ended = self.end(seq, over);
        self.answer_step(ended, Outcome::Done(answer));
    }

    /// An action ended at a start with no step for it (one kept before #190): on the stack, done.
    pub(super) fn commit_new(&mut self, entry: undo::Entry) {
        let id = entry.id;
        let running = Running {
            action: entry.action,
            name: entry.name.clone(),
            info_hash: entry.info_hash.clone(),
            id,
            listed: entry.listed.clone(),
            previous: Vec::new(),
            had: Vec::new(),
            moved: Vec::new(),
            asked: Vec::new(),
        };
        let seq = self.steps.arrive(id, Arrival::Action(running), None);
        let ended = self.end(seq, Over::Finished(Some(entry)));
        self.steps.unanswered(ended);
    }

    /// The running action of torrent `id` that's `action`, aborted or not.
    pub(super) fn running_step(&self, id: TorrentId, action: UndoAction) -> Option<u64> {
        self.steps.machine().steps.iter().find_map(|(seq, s)| match &s.what {
            What::Action(r) if r.id == id && r.action == action && s.phase != Phase::Queued => Some(*seq),
            What::Action(_) | What::Undo { .. } | What::Command(_) => None,
        })
    }

    /// Undo ([`Steps::undo`]): the client hears once it's back, or why not.
    pub(super) async fn undo(&mut self, reply: UndoReply) {
        let answers = self.steps.undo(reply);
        self.save_undo();
        self.deliver(answers);
        self.run_steps().await;
    }

    /// Aborts running action `seq`: it stops after the item in flight, and then is rolled back.
    fn abort(&mut self, seq: u64) {
        let delete = self.deletes.values().find(|d| d.seq == seq).map(|d| &d.stop);
        let stop = delete.or(self.file_trashes.values().find(|f| f.seq == seq).map(|f| &f.stop));
        if let Some(stop) = stop {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Whether something runs, or waits, on the torrent with this info hash: an add of it waits.
    pub(in super::super) fn hash_busy(&self, info_hash: &str) -> bool {
        let machine = self.steps.machine();
        machine.lanes.iter().any(|(id, lane)| {
            self.torrents.get(id).is_some_and(|t| t.info_hash == info_hash)
                || lane.iter().any(|seq| match machine.steps.get(seq).map(|s| &s.what) {
                    Some(What::Action(r)) => r.info_hash == info_hash,
                    Some(What::Undo { entry, .. }) => entry.info_hash == info_hash,
                    Some(What::Command(_)) | None => false,
                })
        })
    }

    /// What clients see of torrent `id`: nothing, while a Remove or Delete of it is on the stack.
    pub(super) fn shown(&self, id: TorrentId) -> Option<view::Shown<'_>> {
        view::shown(self, self.steps.machine(), id)
    }

    /// The lifecycle the undo of `entry` gives its torrent back, as `add_back` does: from its
    /// record, but Ready where librqbit's restore decides (it's checked, or ready, as it was).
    /// A torrent in Error before its Delete shows Ready here until it's back, Error again then.
    fn coming_back(&self, entry: &undo::Entry) -> Option<State> {
        let (saved, magnet) = match &entry.back {
            Back::Gone { saved, source } => (saved.clone(), matches!(source, undo::Source::Magnet(_))),
            Back::InList { .. } => (self.store.get(&entry.info_hash)?.clone(), false),
            Back::Files { .. } => return None,
        };
        let phase = match (magnet, !saved.delete_failed.is_empty(), saved.unchecked) {
            (true, _, _) => lifecycle::Phase::FetchingInfo,
            // A partial Delete's comes back whole.
            (false, true, unchecked) if matches!(entry.back, Back::Gone { .. }) => {
                lifecycle::Phase::DeleteFailed { kept: saved.delete_failed.clone(), away: None, unchecked }
            }
            (false, _, true) => lifecycle::Phase::Unchecked { gone: BTreeMap::new() },
            (false, _, false) => lifecycle::Phase::Ready,
        };
        Some(State::new(saved.wants, phase))
    }

    /// Torrent `id` as clients see it listed now, if they do.
    pub(super) fn listed(&self, id: TorrentId) -> Option<TorrentInfo> {
        view::listed(self, self.steps.machine(), id)
    }

    /// The torrents clients see, by id: those in the list and those an undo is bringing back.
    pub(super) fn listing(&self) -> Vec<TorrentInfo> {
        view::listing(self, self.steps.machine())
    }
}

/// What the steps kept of the torrents, for the fold of what clients see ([`view`]).
impl view::Torrents<Running, undo::Entry> for Actor {
    fn ids(&self) -> BTreeSet<TorrentId> {
        self.torrents.keys().copied().collect()
    }

    fn now(&self, id: TorrentId) -> Option<view::Now> {
        self.torrents.get(&id).filter(|t| t.lifecycle.status(false).is_some()).map(|t| view::Now::With(t.lifecycle.clone()))
    }

    fn row(&self, id: TorrentId, priorities: &BTreeMap<usize, Priority>, state: Option<&State>) -> Option<TorrentInfo> {
        self.listing_of(id, priorities, state)
    }

    fn acting<'a>(&'a self, running: &'a Running) -> view::Acting<'a> {
        match running.action {
            UndoAction::Remove | UndoAction::Delete => {
                // Back as it was while what it did is undone: in the phase its Delete came from.
                let state = self.torrents.get(&running.id).and_then(|t| match &t.lifecycle.phase {
                    lifecycle::Phase::Deleting { was } => Some(State::new(t.lifecycle.wants, (**was).clone())),
                    _ => None,
                });
                view::Acting::Leaves { was: running.listed.as_ref().map(|s| view::Was { info: &s.info, state }) }
            }
            UndoAction::TrashFiles { .. } => {
                view::Acting::Trashes { asked: &running.asked, previous: running.previous.iter().map(|(i, p, _)| (*i, *p)).collect() }
            }
        }
    }

    fn undoing<'a>(&'a self, entry: &'a undo::Entry) -> view::Undoing<'a> {
        match &entry.back {
            // As the undo will bring it back. Once it's back in librqbit the undo is over, and it's
            // shown as it is (librqbit's check, say).
            Back::Gone { .. } | Back::InList { .. } => view::Undoing::BringsBack {
                was: entry.listed.as_ref().map(|s| view::Was { info: &s.info, state: self.coming_back(entry) }),
            },
            Back::Files { previous, .. } => view::Undoing::Restores { previous: previous.iter().map(|(i, p, _)| (*i, *p)).collect() },
        }
    }
}
