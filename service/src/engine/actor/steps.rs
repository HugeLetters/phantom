//! Steps (#190, ADR 0008): the actor's side of Undo's machine ([`lifecycle::undo`]). Every command
//! for a torrent is a step there from the moment it comes, in its torrent's lane: a Remove, a
//! Delete or a Move to Trash also on the undo stack, in the order the commands came (one of several
//! torrents is one step, in all their lanes, with a part per torrent: #235); an undo of one; any
//! other command. [`Steps`] holds the machine and the clients waiting on it: a step
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

use lifecycle::undo::{Part, view};
use proto::{Named, NotBack, Priority, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable, Undone};
use tokio::sync::oneshot;
use tracing::warn;

use super::super::undo::machine::{self as stack, Arrival, Command, Do, Observed, Phase, What};
use super::super::undo::{self, Back, Running, Seen};
use super::super::{Commanded, Failure};
use super::undoing::name_of;
use super::{Actor, Event, Refusal, State, lib};

/// A command's client: it hears, once each torrent's step did as asked or failed, those that failed,
/// and those a Move or a Rename found where they are now (#358).
pub(super) type Reply = oneshot::Sender<Commanded>;
pub(super) type UndoReply = oneshot::Sender<Result<Undone, Failure>>;

/// Undo's machine as the actor drives it: the stack and the lanes, the parts the actor carries out,
/// and the clients waiting on them. The actor saves it after each change, written ahead.
pub(in super::super) struct Steps {
    machine: stack::State,
    /// The parts the actor carries out now: the machine's world.
    world: stack::World,
    /// Each command's client, by batch, with what failed so far and how many parts it waits on.
    batches: BTreeMap<u64, Batch>,
    /// The batch of each part, by seq and torrent.
    batch_of: BTreeMap<(u64, TorrentId), u64>,
    next_batch: u64,
    /// The client waiting for each undo, by the seq of the action it takes back, with what that
    /// action was when the Undo came: none for one finished at start.
    undo_replies: BTreeMap<u64, (UndoReply, Option<Undoable>)>,
    /// How each part of an undo went so far, by the seq of the action it takes back.
    tallies: BTreeMap<u64, Tally>,
    /// The steps that waited behind an undo bringing their torrent back that failed: one that fails
    /// in turn is answered `Undone`, as the undo's own refusal says why (#217's re-review, S2).
    orphaned: BTreeSet<(u64, TorrentId)>,
    /// The Cancel checks judged on the torrent as clients saw it, behind other steps: one that finds
    /// no check at its turn is done, as asked.
    cancels_judged: BTreeSet<u64>,
}

/// The Moves an engine restart for new settings didn't start (ADR 0008: it waits only for the one
/// under way), each with its client, for the engine that starts next to run in their order.
#[derive(Default)]
pub struct Carried {
    /// Their clients, each with what failed so far of its command.
    batches: Vec<Batch>,
    /// Each Move, with its client's place in `batches`.
    moves: Vec<(TorrentId, TorrentCommand, usize)>,
}

/// A command's client, waiting on `left` parts.
struct Batch {
    reply: Reply,
    failed: Vec<(TorrentId, Failure)>,
    found: Vec<TorrentId>,
    stayed: Vec<TorrentId>,
    left: usize,
    /// Every part has arrived: it's answered once `left` is none.
    closed: bool,
}

/// How the parts of an undo went so far.
#[derive(Default)]
struct Tally {
    action: Option<UndoAction>,
    back: Vec<Named>,
    refused: Vec<NotBack>,
    /// A part's failure, as it was: an undo of one torrent is answered with it.
    failed: Option<Failure>,
}

/// What the actor finds that ends no step.
pub(super) enum Fact {
    /// Running action `seq` moved an item of torrent `id`: that part holds this now.
    Moved(u64, TorrentId, Running),
    /// Torrent `id`'s files start moving to another destination (#24): what would put files of it
    /// back from the Trash leaves the stack.
    Moving(TorrentId),
    /// The service is stopping: what waits doesn't start, but the undos.
    Stopping,
    /// An engine restart for new settings begins: no Move waiting its turn starts.
    Restarting,
    /// The engine starts again, after a stop or a crash.
    Restarted,
}

/// How a step's part ended: an action's with what undoes it (none: it changed nothing), an undo's
/// carried out, a command done; or one that couldn't.
pub(super) enum Over {
    Finished(Option<undo::Entry>),
    Failed,
    /// An undo refused, for this reason (#200).
    Refused(proto::CantUndo, Vec<proto::AbsolutePath>),
}

/// A step's part over ([`Steps::end`]), whose client is still to hear how it went: whether an Undo
/// aborted it, the action an undo was of, and whether the step is over with it. Only
/// [`Steps::answer`] (or, for a step nobody waits on, [`Steps::unanswered`]) takes it.
#[must_use = "its client hears how it went: `Steps::answer`, or `Steps::unanswered` when nobody waits"]
pub(super) struct Ended {
    seq: u64,
    id: TorrentId,
    aborted: bool,
    of: Option<u64>,
    /// What the part undoes, and its torrent's name, as the part had them.
    named: Option<(UndoAction, Named)>,
    /// The step is over with it (or, aborted, rolled back from now).
    last: bool,
}

/// How a step's part went, for its client: a command's or an action's, or an undo's.
pub(super) enum Outcome {
    Done(Result<(), Failure>),
    Undone(Result<Undoable, Failure>),
}

/// An answer to hand a client, once what it did is published.
pub(super) enum Answer {
    Step(Reply, Commanded),
    Undo(UndoReply, Result<Undone, Failure>),
}

impl Steps {
    pub(super) fn new(machine: stack::State) -> Self {
        Self {
            machine,
            world: Default::default(),
            batches: BTreeMap::new(),
            batch_of: BTreeMap::new(),
            next_batch: 0,
            undo_replies: BTreeMap::new(),
            tallies: BTreeMap::new(),
            orphaned: BTreeSet::new(),
            cancels_judged: BTreeSet::new(),
        }
    }

    /// The stack and the lanes, as they are.
    pub(in super::super) fn machine(&self) -> &stack::State {
        &self.machine
    }

    /// A command's client, those of its torrents refused at once `failed`: its steps arrive in the
    /// batch this returns, which [`Steps::close`] closes.
    pub(super) fn batch(&mut self, reply: Reply, failed: Vec<(TorrentId, Failure)>) -> u64 {
        let batch = self.next_batch;
        self.next_batch += 1;
        self.batches.insert(batch, Batch { reply, failed, found: Vec::new(), stayed: Vec::new(), left: 0, closed: false });
        batch
    }

    /// Every step of `batch` has arrived: answered now if none waits.
    pub(super) fn close(&mut self, batch: u64) -> Vec<Answer> {
        if let Some(b) = self.batches.get_mut(&batch) {
            b.closed = true;
        }
        self.batch_ended(batch)
    }

    fn batch_ended(&mut self, batch: u64) -> Vec<Answer> {
        match self.batches.get(&batch) {
            Some(b) if b.closed && b.left == 0 => {
                let b = self.batches.remove(&batch).expect("just found");
                vec![Answer::Step(b.reply, Commanded { failed: b.failed, found: b.found, stayed: b.stayed })]
            }
            Some(_) | None => Vec::new(),
        }
    }

    /// A step arrives, its client's `batch` waiting on each of its parts: its seq. Numbered from the
    /// clock, not only past the last, so seqs stay larger than any a client was told of, even once
    /// `undo.json` is lost (a notice's Undo goes by them).
    pub(super) fn arrive(&mut self, what: Arrival<Running, TorrentCommand>, batch: Option<u64>) -> u64 {
        let judged = match &what {
            Arrival::Command(id, TorrentCommand::CancelCheck) => self.machine.lanes.contains_key(id),
            Arrival::Command(..) | Arrival::Action(_) => false,
        };
        let torrents: Vec<TorrentId> = match &what {
            Arrival::Action(parts) => parts.keys().copied().collect(),
            Arrival::Command(id, _) => vec![*id],
        };
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        self.decide(stack::Event::Observed(Observed::Arrived { seq: nanos, what }));
        let seq = self.machine.last;
        if judged {
            self.cancels_judged.insert(seq);
        }
        if let Some(batch) = batch.filter(|b| self.batches.contains_key(b)) {
            for id in torrents {
                self.batch_of.insert((seq, id), batch);
                self.batches.get_mut(&batch).expect("just found").left += 1;
            }
        }
        seq
    }

    /// What the actor found that ends no step: an item a running action moved, the service stopping
    /// (the answers of those it drops), the engine started again.
    pub(super) fn report(&mut self, fact: Fact) -> Vec<Answer> {
        let (observed, stopping) = match fact {
            Fact::Moved(seq, id, running) => (Observed::Moved(seq, id, running), false),
            Fact::Moving(id) => (Observed::Moving(id), false),
            Fact::Stopping => (Observed::Stopping, true),
            Fact::Restarting => (Observed::Restarting, false),
            Fact::Restarted => (Observed::Restarted, false),
        };
        let before: Vec<(u64, Vec<TorrentId>)> =
            self.machine.steps.iter().map(|(seq, s)| (*seq, s.torrents.iter().copied().collect())).collect();
        self.decide(stack::Event::Observed(observed));
        // Dropped by the stop: each client hears the service is stopping.
        let gone: Vec<(u64, TorrentId)> = before
            .into_iter()
            .filter(|(seq, _)| stopping && !self.machine.steps.contains_key(seq))
            .flat_map(|(seq, ids)| ids.into_iter().map(move |id| (seq, id)))
            .collect();
        gone.into_iter().flat_map(|(seq, id)| self.answered(seq, id, Err(proto::Refusal::Stopping.into()))).collect()
    }

    /// Step `seq`'s part on torrent `id` is over as `over` says: the actor carries it out no more.
    /// Its client hears how it went once the actor knows, through what this returns.
    pub(super) fn end(&mut self, seq: u64, id: TorrentId, over: Over) -> Ended {
        let mut ended = Ended { seq, id, aborted: false, of: None, named: None, last: false };
        let mut kind = None;
        if let Some(step) = self.machine.steps.get(&seq) {
            ended.aborted = step.phase == Phase::Aborting;
            kind = Some(std::mem::discriminant(&step.what));
            match &step.what {
                What::Action(parts) => {
                    ended.named = match parts.get(&id) {
                        Some(Part::Doing(r)) => Some((r.action, Named { id, name: r.name.clone() })),
                        Some(Part::Over(_)) | None => None,
                    }
                }
                What::Undo { of, parts } => {
                    ended.of = Some(*of);
                    if let Some(Part::Doing(entry)) = parts.get(&id) {
                        ended.named = Some((entry.action, Named { id, name: entry.name.clone() }));
                        // An undo that didn't bring its torrent back: what waits behind it fails as
                        // not in the list, which the undo's own refusal says better.
                        if matches!(over, Over::Failed | Over::Refused(..)) && matches!(entry.back, Back::Gone { .. } | Back::InList { .. })
                        {
                            let behind = self.machine.lanes.get(&entry.id).into_iter().flatten().filter(|s| **s != seq);
                            let behind: Vec<(u64, TorrentId)> = behind.map(|s| (*s, entry.id)).collect();
                            self.orphaned.extend(behind);
                        }
                    }
                }
                What::Command(_) => {}
            }
        }
        let observed = match over {
            Over::Finished(entry) => Observed::Finished(seq, id, entry),
            Over::Failed => Observed::Failed(seq, id),
            Over::Refused(why, stranded) => Observed::Refused { seq, torrent: id, why, stranded },
        };
        self.decide(stack::Event::Observed(observed));
        ended.last = self.machine.steps.get(&seq).map(|s| std::mem::discriminant(&s.what)) != kind;
        ended
    }

    /// A step over that no client waits on: one ended at a start, or committed there.
    pub(super) fn unanswered(&mut self, ended: Ended) {
        let waits = self.batch_of.contains_key(&(ended.seq, ended.id))
            || [Some(ended.seq), ended.of].into_iter().flatten().any(|s| self.undo_replies.contains_key(&s));
        debug_assert!(!waits, "step {} has a client, who must hear how it went", ended.seq);
    }

    /// The client of step `ended` hears how its part went: one an Undo aborted hears it was undone,
    /// and the Undo's hears it's back once the whole step is over, if it changed nothing (else once
    /// its rollback is over). An undo's client hears once each part is over: what came back, and
    /// what didn't, with why.
    pub(super) fn answer(&mut self, ended: Ended, outcome: Outcome) -> Vec<Answer> {
        let (seq, id) = (ended.seq, ended.id);
        match outcome {
            Outcome::Done(answer) if !ended.aborted => self.answered(seq, id, answer),
            Outcome::Done(_) => {
                let mut answers = self.answered(seq, id, Err(proto::Refusal::Undone.into()));
                let rolling_back = self.machine.steps.get(&seq).is_some_and(|s| matches!(s.what, What::Undo { .. }));
                if ended.last
                    && !rolling_back
                    && let Some((reply, Some(undo))) = self.undo_replies.remove(&seq)
                {
                    answers.push(Answer::Undo(reply, Ok(Undone { undo, refused: Vec::new() })));
                }
                answers
            }
            Outcome::Undone(answer) => {
                let Some(of) = ended.of else { return Vec::new() };
                let tally = self.tallies.entry(of).or_default();
                let (action, named) = ended.named.unwrap_or_else(|| (UndoAction::Remove, Named { id, name: format!("torrent {id}") }));
                match answer {
                    Ok(undo) => {
                        tally.action.get_or_insert(undo.action);
                        tally.back.extend(undo.torrents);
                    }
                    Err(failure) => {
                        let (why, stranded) = failure.cant_undo();
                        tally.action.get_or_insert(action);
                        tally.refused.push(NotBack { torrent: named, action, why, stranded });
                        // Of one torrent, answered with its failure as it was.
                        tally.failed = Some(failure);
                    }
                }
                if !ended.last {
                    return Vec::new();
                }
                let tally = self.tallies.remove(&of).unwrap_or_default();
                let (reply, asked) = match self.undo_replies.remove(&of) {
                    Some((reply, asked)) => (Some(reply), asked),
                    None => (None, None),
                };
                let action = tally.action.or(asked.as_ref().map(|u| u.action)).unwrap_or(UndoAction::Remove);
                let result = match (tally.back.is_empty(), tally.failed, &tally.refused[..]) {
                    // Of one torrent, its failure as it was.
                    (true, Some(failed), [_]) => Err(failed),
                    (false, _, _) | (true, None, _) | (true, Some(_), [] | [_, _, ..]) => {
                        lifecycle::undo::undone(of, action, tally.back, tally.refused).map_err(Failure::Refused)
                    }
                };
                match reply {
                    Some(reply) => vec![Answer::Undo(reply, result)],
                    None => {
                        if let Err(e) = result {
                            warn!(error = ?e, "failed to finish an undo cut short");
                        }
                        Vec::new()
                    }
                }
            }
        }
    }

    /// ⌘Z: the top step at once, whatever runs: one done gets its undo on its torrents' lanes, one
    /// running is aborted, one queued is dropped (its client hears it was undone, and the Undo's
    /// that it's back). Otherwise the Undo's client hears once it's back, or why not.
    pub(super) fn undo(&mut self, reply: UndoReply) -> Vec<Answer> {
        let Some(top) = self.machine.top().cloned() else { return vec![Answer::Undo(reply, Err(proto::Refusal::NothingToUndo.into()))] };
        let asked = undo::undoable(&self.machine, top.seq);
        let queued = self.machine.steps.get(&top.seq).filter(|s| s.phase == Phase::Queued).map(|s| s.torrents.clone());
        self.decide(stack::Event::Command(Command::Undo));
        match (queued, asked) {
            (Some(torrents), Some(undo)) => {
                let mut answers: Vec<Answer> =
                    torrents.into_iter().flat_map(|id| self.answered(top.seq, id, Err(proto::Refusal::Undone.into()))).collect();
                answers.push(Answer::Undo(reply, Ok(Undone { undo, refused: Vec::new() })));
                answers
            }
            // Running or done: the Undo's client hears once it's back, or why not.
            (_, asked) => {
                self.undo_replies.insert(top.seq, (reply, asked));
                Vec::new()
            }
        }
    }

    /// ⌘Z on a committed action whose undo can't run now and won't wait, for `why` (#331): it
    /// leaves the stack refused, and the Undo's client hears why. On any other top, as [`Steps::undo`].
    pub(super) fn refuse(&mut self, reply: UndoReply, why: proto::CantUndo) -> Vec<Answer> {
        let Some(undo) = self.machine.top().filter(|p| p.done.is_some()).and_then(|p| undo::undoable(&self.machine, p.seq)) else {
            return self.undo(reply);
        };
        self.decide(stack::Event::Command(Command::Refuse(why.clone())));
        vec![Answer::Undo(reply, Err(proto::Refusal::Undo { undo, why, stranded: Vec::new() }.into()))]
    }

    /// What the actor starts or aborts now; it carries it out from then on.
    pub(super) fn next(&mut self) -> Option<Do> {
        let todo = stack::next(&self.machine, &self.world)?;
        match todo {
            Do::Start(seq, id) => self.world.started.insert((seq, id)),
            Do::Abort(seq) => self.world.aborted.insert(seq),
        };
        Some(todo)
    }

    /// What undo step `seq` puts back on torrent `id`, written again: a partial Delete's, once its
    /// torrent is taken out of the list, puts back one gone.
    pub(super) fn rewrite(&mut self, seq: u64, id: TorrentId, entry: undo::Entry) {
        if let Some(step) = self.machine.steps.get_mut(&seq)
            && let What::Undo { parts, .. } = &mut step.what
            && let Some(Part::Doing(kept)) = parts.get_mut(&id)
        {
            *kept = entry;
        }
    }

    /// A Rename took torrent `id` out of librqbit and added it back (#335): what it had of a file a
    /// Move to Trash kept under it, still Off (`off`), went with it, and was never counted again. So
    /// each such Move to Trash's undo checks the torrent, as for a file it let go of (#328).
    pub(super) fn let_go_under(&mut self, id: TorrentId, off: impl Fn(usize) -> bool) {
        let kept = self.machine.stack.iter_mut().filter_map(|p| p.done.as_mut()).filter_map(|done| done.get_mut(&id));
        for entry in kept {
            if let Back::Files { previous, let_go, .. } = &mut entry.back {
                for (i, _, _) in previous.iter().filter(|(i, _, _)| off(*i)) {
                    if !let_go.contains(i) {
                        let_go.push(*i);
                    }
                }
            }
        }
    }

    fn decide(&mut self, event: stack::Event) {
        self.machine = stack::decide(&self.machine, &event).expect("only an Undo is ever refused, of an empty stack, which `undo` answers");
        self.world.follow(&event, &self.machine);
    }

    /// Torrent `id`'s Move that waits its turn leaves the queue and never starts: its client hears
    /// `failure`. None: none waits (a Move that runs is no one's to cancel).
    pub(super) fn dequeue(&mut self, id: TorrentId, failure: Failure) -> Option<Vec<Answer>> {
        let seq = self.machine.waiting(id)?;
        let event = stack::Event::Command(Command::Dequeue(id));
        self.machine = stack::decide(&self.machine, &event).ok()?;
        self.world.follow(&event, &self.machine);
        Some(self.answered(seq, id, Err(failure)))
    }

    /// The Moves still waiting their turn as an engine restart for new settings ends, out of the
    /// machine, with their clients: the next engine's ([`Steps::resume`]).
    pub(super) fn carry(&mut self) -> Carried {
        let mut carried = Carried::default();
        let mut placed: BTreeMap<u64, usize> = BTreeMap::new();
        let waiting: Vec<(u64, TorrentId, TorrentCommand)> = self
            .machine
            .steps
            .iter()
            .filter(|(_, s)| s.alone() && s.phase == Phase::Queued)
            .filter_map(|(seq, s)| match (&s.what, s.torrents.first()) {
                (What::Command(command), Some(id)) => Some((*seq, *id, command.clone())),
                _ => None,
            })
            .collect();
        for (seq, id, command) in waiting {
            self.decide(stack::Event::Command(Command::Dequeue(id)));
            let Some(batch) = self.batch_of.remove(&(seq, id)) else { continue };
            let at = match placed.get(&batch) {
                Some(at) => *at,
                None => match self.batches.remove(&batch) {
                    Some(b) => {
                        carried.batches.push(Batch { left: 0, ..b });
                        placed.insert(batch, carried.batches.len() - 1);
                        carried.batches.len() - 1
                    }
                    None => continue,
                },
            };
            carried.moves.push((id, command, at));
        }
        carried
    }

    /// The Moves the engine before this one carried over its restart for new settings
    /// ([`Steps::carry`]) arrive, in their order, each with its client.
    pub(super) fn resume(&mut self, carried: Carried) -> Vec<Answer> {
        let Carried { batches, moves } = carried;
        let ids: Vec<u64> = batches.into_iter().map(|b| self.adopt(b)).collect();
        for (id, command, at) in moves {
            self.arrive(Arrival::Command(id, command), Some(ids[at]));
        }
        ids.into_iter().flat_map(|batch| self.batch_ended(batch)).collect()
    }

    /// A client carried over a restart, waiting on what arrives for it next.
    fn adopt(&mut self, batch: Batch) -> u64 {
        let id = self.next_batch;
        self.next_batch += 1;
        self.batches.insert(id, batch);
        id
    }

    /// Step `seq`'s part on torrent `id` found it where it is now, moving nothing (#358): its batch
    /// hears so with its answer.
    pub(super) fn found(&mut self, seq: u64, id: TorrentId) {
        if let Some(b) = self.batch_of.get(&(seq, id)).and_then(|batch| self.batches.get_mut(batch)) {
            b.found.push(id);
        }
    }

    /// Step `seq`'s part on torrent `id`, a Delete, left its files where they are (#358): its batch
    /// hears so with its answer.
    pub(super) fn stayed(&mut self, seq: u64, id: TorrentId) {
        if let Some(b) = self.batch_of.get(&(seq, id)).and_then(|batch| self.batches.get_mut(batch)) {
            b.stayed.push(id);
        }
    }

    /// Step `seq`'s part on torrent `id`: its batch hears `answer`, once each of its parts did. One
    /// that waited behind an undo that failed hears it was undone; a Cancel check judged on what
    /// clients saw that finds no check at its turn is done, as asked.
    fn answered(&mut self, seq: u64, id: TorrentId, answer: Result<(), Failure>) -> Vec<Answer> {
        let answer = match (self.cancels_judged.remove(&seq), answer) {
            (true, Err(Failure::Refused(proto::Refusal::Torrent { why: Refusal::NotChecking, .. }))) => Ok(()),
            (_, answer) => answer,
        };
        let answer = match (self.orphaned.remove(&(seq, id)), answer) {
            (true, Err(_)) => Err(proto::Refusal::Undone.into()),
            (_, answer) => answer,
        };
        let Some(batch) = self.batch_of.remove(&(seq, id)) else { return Vec::new() };
        if let Some(b) = self.batches.get_mut(&batch) {
            b.failed.extend(answer.err().map(|e| (id, e)));
            b.left = b.left.saturating_sub(1);
        }
        self.batch_ended(batch)
    }
}

impl Actor {
    /// `command` for each of `ids`: a Remove, a Delete or a Move to Trash one step on all their lanes
    /// (one press, one step: #235), any other a step on each one's. A torrent is refused at once
    /// when clients don't see it, or, while something runs on it, when its lifecycle refuses it
    /// now; one decided later and refused then is a part that changed nothing. `reply` hears, once
    /// each did as asked or failed, those that failed.
    pub(super) async fn arrive(&mut self, ids: Vec<TorrentId>, command: TorrentCommand, reply: Reply) {
        // Each torrent once, as named first.
        let mut ids = ids;
        let mut seen = BTreeSet::new();
        ids.retain(|id| seen.insert(*id));
        // Cancel move takes a Move out of the queue as it comes, on no lane: it would wait for what it cancels.
        if command == TorrentCommand::CancelMove {
            let mut failed = Vec::new();
            for id in ids {
                let refused = match (self.shown(id).is_some(), self.dequeue_move(id, proto::NotMoved::Cancelled)) {
                    (false, _) => Some(Refusal::NotInList),
                    (true, true) => None,
                    (true, false) => Some(Refusal::NotWaitingToMove),
                };
                failed.extend(refused.map(|why| (id, Failure::torrent(id, why))));
            }
            drop(reply.send(Commanded { failed, found: Vec::new(), stayed: Vec::new() }));
            return self.run_steps().await;
        }
        let (mut doing, mut failed) = (Vec::new(), Vec::new());
        for id in ids {
            match self.judged(id, &command) {
                Ok(()) => doing.push(id),
                Err(e) => failed.push((id, e)),
            }
        }
        let batch = self.steps.batch(reply, failed);
        let action = match &command {
            TorrentCommand::Remove => Some(UndoAction::Remove),
            TorrentCommand::Delete => Some(UndoAction::Delete),
            TorrentCommand::TrashFiles { files } => Some(UndoAction::TrashFiles { count: files.len() }),
            TorrentCommand::Rename { .. } => Some(UndoAction::Rename),
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
            for id in &doing {
                self.cancel_move(*id, command.action()).await;
            }
        }
        match action {
            Some(action) => {
                let asked = match &command {
                    TorrentCommand::TrashFiles { files } => files.clone(),
                    _ => Vec::new(),
                };
                let renaming = match &command {
                    TorrentCommand::Rename { name } => Some(undo::Renaming { to: name.to_string(), was: None, found: false }),
                    _ => None,
                };
                let parts: BTreeMap<TorrentId, Running> = doing
                    .iter()
                    .map(|id| (*id, Running { renaming: renaming.clone(), ..self.running(*id, action, asked.clone()) }))
                    .collect();
                if !parts.is_empty() {
                    self.steps.arrive(Arrival::Action(parts), Some(batch));
                }
            }
            None => {
                for id in doing {
                    self.steps.arrive(Arrival::Command(id, command.clone()), Some(batch));
                }
            }
        }
        let answers = self.steps.close(batch);
        self.save_undo();
        self.deliver(answers);
        self.run_steps().await;
    }

    /// A Move of each of `moves` into `to`, a step each in the order given, so they run one after
    /// another in it (#270): `reply` hears, once each moved or failed, those that didn't.
    pub(super) async fn arrive_moves(&mut self, to: proto::AbsolutePath, moves: Vec<proto::MoveOf>, reply: Reply) {
        let mut seen = BTreeSet::new();
        let (mut doing, mut failed) = (Vec::new(), Vec::new());
        for proto::MoveOf { id, clash, name } in moves.into_iter().filter(|m| seen.insert(m.id)) {
            let command = TorrentCommand::Move { to: to.clone(), clash, name };
            match self.judged(id, &command) {
                Ok(()) => doing.push((id, command)),
                Err(e) => failed.push((id, e)),
            }
        }
        let batch = self.steps.batch(reply, failed);
        for (id, command) in doing {
            self.steps.arrive(Arrival::Command(id, command), Some(batch));
        }
        let answers = self.steps.close(batch);
        self.save_undo();
        self.deliver(answers);
        self.run_steps().await;
    }

    /// Whether `command` may arrive for torrent `id`: refused at once when clients don't see it,
    /// or, judged on it as they see it, when it waits behind other steps and its lifecycle refuses
    /// it, or it has no such file.
    fn judged(&self, id: TorrentId, command: &TorrentCommand) -> Result<(), Failure> {
        if self.shown(id).is_none() {
            return Err(Failure::torrent(id, Refusal::NotInList));
        }
        // Judged on the torrent as clients see it: as the steps before it will leave it (back, once
        // an Undo took back its Delete; paused, once a Pause waiting before it has run).
        let machine = self.steps.machine();
        let behind = machine.lanes.contains_key(&id) || machine.waiting(id).is_some();
        if behind
            && let Some(listed) = self.listed(id)
            && let proto::Answer::Refused { why } = listed.actions.get(command.action())
        {
            return Err(Failure::torrent(id, why.clone()));
        }
        // A file it doesn't have is known now too: refused before it's on the stack.
        if let (true, TorrentCommand::TrashFiles { files } | TorrentCommand::SetPriority { files, .. }) = (behind, command)
            && let Ok(listed) = self.with_info(id).map(|r| super::files(r, &[], &BTreeSet::new()))
            && let Some(bad) = files.iter().find(|&&i| !listed.iter().any(|f| f.index == i))
        {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        Ok(())
    }

    /// What an action of torrent `id` holds as it comes: the torrent as clients see it now.
    fn running(&self, id: TorrentId, action: UndoAction, asked: Vec<usize>) -> Running {
        let listed = self.listed(id);
        let (name, info_hash) = match (self.torrents.get(&id), &listed) {
            (Some(t), _) => (name_of(t), t.record.info_hash.clone()),
            (None, Some(l)) => (l.display_name().to_owned(), l.info_hash.clone()),
            (None, None) => (format!("torrent {id}"), String::new()),
        };
        // As it was, at rest: its rates are of a moment that's gone once it's back. One moving (a
        // Remove or a Delete cancels its move first) as it is once it's back where it was.
        let listed = listed.map(|l| match self.torrents.get(&id).map(|t| (&t.lifecycle.phase, t.lifecycle.wants)) {
            Some((lifecycle::Phase::Moving { was, .. }, wants)) => view::seen_as(l, Some(&State::new(wants, (**was).clone()))),
            _ => l,
        });
        let listed = listed.map(|l| Seen { info: TorrentInfo { download_bytes_per_sec: 0, upload_bytes_per_sec: 0, live_peers: 0, ..l } });
        Running {
            action,
            name,
            info_hash,
            id,
            listed,
            previous: Vec::new(),
            had: Vec::new(),
            let_go: Vec::new(),
            moved: Vec::new(),
            asked,
            renaming: None,
        }
    }

    /// Starts each part the machine says to, and stops each step it says to abort: after every message
    /// and every end of a task, never from inside a step (so a step ending at once doesn't recurse).
    pub(super) async fn run_steps(&mut self) {
        // Killed as a crash: nothing more is carried out, as nothing would be.
        #[cfg(test)]
        if self.killed {
            return;
        }
        // What a move needs next.
        self.go_on_moving();
        // What was held while librqbit sampled its torrent's pieces, or while a move a start
        // finishes moved it, goes on once that's over.
        let over: Vec<(u64, TorrentId)> = self.held.iter().copied().filter(|(_, id)| !self.holds(*id)).collect();
        for (seq, id) in over {
            self.held.remove(&(seq, id));
            self.start_now(seq, id).await;
        }
        while let Some(todo) = self.steps.next() {
            // Killed as a crash: nothing more is carried out, as nothing would be.
            #[cfg(test)]
            if self.killed {
                return;
            }
            match todo {
                Do::Start(seq, id) => self.start(seq, id).await,
                Do::Abort(seq) => {
                    self.abort(seq);
                    self.abort_renames(seq).await;
                }
            }
        }
    }

    /// Reports what the actor found that ends no step ([`Steps::report`]), written ahead.
    pub(super) fn report(&mut self, fact: Fact) {
        let answers = self.steps.report(fact);
        self.save_undo();
        self.deliver(answers);
    }

    /// Step `seq`'s part on torrent `id` is over ([`Steps::end`]), written ahead: the adds that
    /// waited for its torrent go on once nothing runs on it. Its client hears how it went through
    /// [`Actor::answer_step`].
    pub(super) fn end(&mut self, seq: u64, id: TorrentId, over: Over) -> Ended {
        let ended = self.steps.end(seq, id, over);
        self.save_undo();
        for (prepared, paused, reply) in std::mem::take(&mut self.adds_waiting) {
            self.add_again(prepared, paused, reply);
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
                Answer::Step(reply, commanded) => drop(reply.send(commanded)),
                Answer::Undo(reply, answer) => drop(reply.send(answer)),
            }
        }
    }

    /// Starts step `seq`'s part on torrent `id`; one that ends at once is reported over. One whose
    /// torrent librqbit is reading the saved pieces of (resuming: at a start, after an undo or a
    /// let-go) is held until that's over, as behind a step running on its lane: it shows done already
    /// (`lifecycle::undo::view`), and what comes after waits behind it.
    async fn start(&mut self, seq: u64, id: TorrentId) {
        if self.holds(id) {
            self.held.insert((seq, id));
            return;
        }
        self.start_now(seq, id).await;
    }

    /// Whether what runs on torrent `id`'s lane is held: librqbit samples its pieces, or a move a
    /// start finishes moves it (one asked for is a step of the lane itself).
    fn holds(&self, id: TorrentId) -> bool {
        self.torrents
            .get(&id)
            .is_some_and(|t| matches!(t.lib, lib::Lib::Resuming(_)) || matches!(t.lifecycle.phase, lifecycle::Phase::Moving { .. }))
    }

    /// Starts what [`Actor::start`] held, whatever librqbit is doing: the service stops.
    pub(super) async fn start_held(&mut self) {
        for (seq, id) in std::mem::take(&mut self.held) {
            self.start_now(seq, id).await;
        }
    }

    async fn start_now(&mut self, seq: u64, id: TorrentId) {
        let Some(step) = self.steps.machine().steps.get(&seq).cloned() else { return };
        match step.what {
            What::Action(parts) => match parts.get(&id) {
                Some(Part::Doing(running)) => match running.action {
                    UndoAction::Remove => self.start_remove(id, seq).await,
                    UndoAction::Delete => self.delete(id, seq).await,
                    UndoAction::TrashFiles { .. } => self.trash_files(id, seq, running.asked.clone()).await,
                    UndoAction::Rename => match running.renaming.as_ref().map(|r| r.to.clone()) {
                        Some(name) => self.start_rename(id, name, super::moving::StepOf::Rename(seq)).await,
                        None => self.step_ended(seq, id, Over::Failed, Err(anyhow::anyhow!("a Rename without a name").into())),
                    },
                },
                Some(Part::Over(_)) | None => {}
            },
            What::Undo { of, parts } => match parts.get(&id) {
                Some(Part::Doing(entry)) => self.start_undo(seq, of, entry.clone()).await,
                Some(Part::Over(_)) | None => {}
            },
            What::Command(TorrentCommand::Move { to, clash, name }) => self.start_move(seq, id, to, clash, name).await,
            What::Command(command) => {
                let answer = self.command(id, command).await;
                let ended = self.end(seq, id, Over::Finished(None));
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
            Err(e) => (self.end(seq, id, Over::Failed), Err(e)),
        };
        self.answer_step(ended, Outcome::Done(removed));
    }

    /// Step `seq`'s part on torrent `id` is over as `over` says, its client answered `answer` at once.
    pub(super) fn step_ended(&mut self, seq: u64, id: TorrentId, over: Over, answer: Result<(), Failure>) {
        let ended = self.end(seq, id, over);
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
            let_go: Vec::new(),
            moved: Vec::new(),
            asked: Vec::new(),
            renaming: None,
        };
        let seq = self.steps.arrive(Arrival::Action(BTreeMap::from([(id, running)])), None);
        let ended = self.end(seq, id, Over::Finished(Some(entry)));
        self.steps.unanswered(ended);
    }

    /// The running action whose part on torrent `id` is `action` and under way, aborted or not.
    pub(super) fn running_step(&self, id: TorrentId, action: UndoAction) -> Option<u64> {
        self.steps.machine().steps.iter().find_map(|(seq, s)| match &s.what {
            What::Action(parts) if s.phase != Phase::Queued => match parts.get(&id) {
                Some(Part::Doing(r)) if r.action == action => Some(*seq),
                Some(Part::Doing(_) | Part::Over(_)) | None => None,
            },
            What::Action(_) | What::Undo { .. } | What::Command(_) => None,
        })
    }

    /// What running action `seq`'s part on torrent `id` holds, while it's under way.
    pub(super) fn action_part(&self, seq: u64, id: TorrentId) -> Option<&Running> {
        match &self.steps.machine().steps.get(&seq)?.what {
            What::Action(parts) => match parts.get(&id)? {
                Part::Doing(r) => Some(r),
                Part::Over(_) => None,
            },
            What::Undo { .. } | What::Command(_) => None,
        }
    }

    /// Undo ([`Steps::undo`]): the client hears once it's back, or why not. A Rename's whose
    /// torrent is moving to another folder now is refused rather than wait for the move (#331).
    pub(super) async fn undo(&mut self, reply: UndoReply) {
        let answers = match self.busy_rename() {
            true => self.steps.refuse(reply, proto::CantUndo::Busy),
            false => self.steps.undo(reply),
        };
        self.save_undo();
        self.deliver(answers);
        self.run_steps().await;
    }

    /// Whether the top of the stack is a committed Rename whose torrent is moving now, or has a Move
    /// waiting its turn on its lane: its undo would wait for the move.
    fn busy_rename(&self) -> bool {
        let machine = self.steps.machine();
        let Some(done) = machine.top().and_then(|p| p.done.as_ref()) else { return false };
        done.values().filter(|e| matches!(e.back, Back::Renamed { .. })).any(|e| {
            let moving = self.torrents.get(&e.id).is_some_and(|t| matches!(t.lifecycle.phase, lifecycle::Phase::Moving { .. }));
            let lane = machine.lanes.get(&e.id).into_iter().flatten().filter_map(|s| machine.steps.get(s));
            moving || lane.into_iter().any(|s| matches!(s.what, What::Command(TorrentCommand::Move { .. })))
        })
    }

    /// Aborts running action `seq`: each part stops after the item in flight, and then it's rolled back.
    /// A part held while librqbit samples its torrent's pieces never started: it's over at once,
    /// having changed nothing, as one that waited its turn is dropped.
    fn abort(&mut self, seq: u64) {
        let held: Vec<(u64, TorrentId)> = self.held.iter().copied().filter(|(s, _)| *s == seq).collect();
        for (seq, id) in held {
            self.held.remove(&(seq, id));
            let ended = self.end(seq, id, Over::Finished(None));
            self.answer_step(ended, Outcome::Done(Ok(())));
        }
        let deletes = self.deletes.values().filter(|d| d.seq == seq).map(|d| &d.stop);
        for stop in deletes.chain(self.file_trashes.values().filter(|f| f.seq == seq).map(|f| &f.stop)) {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Whether something runs, or waits, on the torrent with this info hash: an add of it waits.
    pub(in super::super) fn hash_busy(&self, info_hash: &str) -> bool {
        let machine = self.steps.machine();
        machine.lanes.iter().any(|(id, lane)| {
            self.torrents.get(id).is_some_and(|t| t.record.info_hash == info_hash)
                || lane.iter().any(|seq| match machine.steps.get(seq).map(|s| &s.what) {
                    Some(What::Action(parts)) => match parts.get(id) {
                        Some(Part::Doing(r)) => r.info_hash == info_hash,
                        Some(Part::Over(Some(e))) => e.info_hash == info_hash,
                        Some(Part::Over(None)) | None => false,
                    },
                    Some(What::Undo { parts, .. }) => match parts.get(id) {
                        Some(Part::Doing(e)) => e.info_hash == info_hash,
                        Some(Part::Over(_)) | None => false,
                    },
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
            Back::InList { .. } => (self.by_hash(&entry.info_hash)?.record.saved(), false),
            Back::Files { .. } | Back::Renamed { .. } => return None,
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
            UndoAction::Rename => view::Acting::Renames,
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
            Back::Renamed { .. } => view::Undoing::Renames,
        }
    }
}
