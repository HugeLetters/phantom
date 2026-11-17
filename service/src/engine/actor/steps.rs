//! Steps (#190, ADR 0008): the actor's side of Undo's machine ([`lifecycle::undo`]). Every command
//! for a torrent is a step there from the moment it comes, in its torrent's lane: a Remove, a
//! Delete or a Move to Trash also on the undo stack, in the order the commands came; an undo of one;
//! any other command. The machine says which to start ([`stack::next`]); the actor carries each out
//! (the file moves on blocking tasks, as before) and reports how it ended. Lanes run side by side,
//! so one torrent's Delete never holds up another's.
//!
//! What clients see is what the steps make of the torrents ([`Shown`]): from the moment a Remove or
//! a Delete comes its torrent isn't listed; once ⌘Z takes it back it's listed again as it was when
//! the step came, until it's back for real; the files of a Move to Trash, or of its undo, have their
//! new priorities at once. A command for a torrent clients don't see is refused as not in the list.

use std::collections::{BTreeMap, BTreeSet};

use proto::{Priority, TorrentCommand, TorrentId, TorrentInfo, UndoAction, Undoable};
use tokio::sync::oneshot;

use super::super::Failure;
use super::super::undo::machine::{self as stack, Arrival, Command, Do, Observed, Phase, What};
use super::super::undo::{self, Back, Running, Seen};
use lifecycle::undo::view;
use super::undoing::name_of;
use super::{Actor, Event, Refusal, State};

pub(super) type Reply = oneshot::Sender<Result<(), Failure>>;
pub(super) type UndoReply = oneshot::Sender<Result<Undoable, Failure>>;

/// What's known of a step as it ends: whether an Undo aborted it, and what it is.
pub(super) struct Ending {
    aborted: bool,
    undoable: Option<Undoable>,
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
        let behind = self.undo.lanes.contains_key(&id);
        if behind
            && let Some(listed) = self.listed(id)
            && let proto::Answer::Refused { why } = listed.actions.get(command.action())
        {
            return self.answer(reply, Err(Failure::torrent(id, why.clone())));
        }
        // A file it doesn't have is known now too: refused before it's on the stack.
        if let (true, TorrentCommand::TrashFiles { files } | TorrentCommand::SetPriority { files, .. }) =
            (self.undo.lanes.contains_key(&id), &command)
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
        let cancel = matches!(what, Arrival::Command(TorrentCommand::CancelCheck));
        let seq = self.arrived(id, what);
        if behind && cancel {
            self.cancels_judged.insert(seq);
        }
        self.replies.insert(seq, reply);
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

    /// A step for torrent `id` comes: its seq.
    pub(super) fn arrived(&mut self, id: TorrentId, what: Arrival<Running, TorrentCommand>) -> u64 {
        // From the clock, not only past the last: seqs stay larger than any a client was told of,
        // even once `undo.json` is lost (a notice's Undo goes by them).
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        self.undo_event(Observed::Arrived { seq: nanos, torrents: BTreeSet::from([id]), what });
        self.undo.last
    }

    /// Starts each step the machine says to, and stops each it says to abort: after every message
    /// and every end of a task, never from inside a step (so a step ending at once doesn't recurse).
    pub(super) async fn run_steps(&mut self) {
        while let Some(todo) = stack::next(&self.undo, &self.world) {
            match todo {
                Do::Start(seq) => {
                    self.world.started.insert(seq);
                    self.start(seq).await;
                }
                Do::Abort(seq) => {
                    self.world.aborted.insert(seq);
                    self.abort(seq);
                }
            }
        }
    }

    /// Step `seq` ended as `how` says: the actor doesn't carry it out any more. Adds that waited for
    /// its torrent go on once nothing runs on it.
    pub(super) fn ended(&mut self, how: Observed) {
        if let Observed::Finished(seq, _) | Observed::Failed(seq) = &how {
            self.world.started.remove(seq);
            self.world.aborted.remove(seq);
        }
        self.undo_event(how);
        for (prepared, paused, reply) in std::mem::take(&mut self.adds_waiting) {
            self.add(prepared, paused, reply);
        }
    }

    /// Starts step `seq`; one that ends at once is reported ended. Each step is of one torrent:
    /// every command is (a Delete of several is one per torrent).
    async fn start(&mut self, seq: u64) {
        let Some(step) = self.undo.steps.get(&seq).cloned() else { return };
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
                let answer = match (self.cancels_judged.remove(&seq), self.command(id, command).await) {
                    // Asked of the check clients saw, which ended meanwhile: no check runs, as asked.
                    (true, Err(Failure::Refused(proto::Refusal::Torrent { why: Refusal::NotChecking, .. }))) => Ok(()),
                    (_, answer) => answer,
                };
                self.answered(seq, answer);
                self.ended(Observed::Finished(seq, None));
            }
        }
    }

    /// Remove, step `seq`, decided on librqbit and the disk as they are now.
    async fn start_remove(&mut self, id: TorrentId, seq: u64) {
        self.poll_torrent(id).await;
        self.size_check(id).await;
        let removed = match self.event(id, Event::Command(proto::Action::Remove)).await {
            Ok(()) => self.remove(id, UndoAction::Remove, seq).await.map_err(Failure::from),
            Err(e) => {
                self.ended(Observed::Failed(seq));
                Err(e)
            }
        };
        self.answered(seq, removed);
    }

    /// Answers step `seq`'s client, if it waits: one an Undo took back hears it was undone.
    pub(super) fn answered(&mut self, seq: u64, answer: Result<(), Failure>) {
        let answer = match (self.orphaned.remove(&seq), answer) {
            (true, Err(_)) => Err(proto::Refusal::Undone.into()),
            (_, answer) => answer,
        };
        if let Some(reply) = self.replies.remove(&seq) {
            self.answer(reply, answer);
        }
    }

    /// Step `seq` ended as `how` says, its client answered `answer`: see [`Actor::ended_as`].
    pub(super) fn step_ended(&mut self, seq: u64, how: Observed, answer: Result<(), Failure>) {
        let ending = self.ending(seq);
        self.ended(how);
        self.ended_as(seq, ending, answer);
    }

    /// What's known of step `seq` before it's reported ended.
    pub(super) fn ending(&self, seq: u64) -> Ending {
        let step = self.undo.steps.get(&seq);
        let undoable = step.and_then(|s| match &s.what {
            What::Action(r) => Some(r.undoable(seq)),
            What::Undo { .. } | What::Command(_) => None,
        });
        Ending { aborted: step.is_some_and(|s| s.phase == Phase::Aborting), undoable }
    }

    /// Step `seq` was reported ended: its client hears `answer`. One aborted is rolled back next
    /// ([`Actor::run_steps`] starts it), if it changed anything: its client hears it was undone, and
    /// the Undo's, once it's back.
    pub(super) fn ended_as(&mut self, seq: u64, ending: Ending, answer: Result<(), Failure>) {
        if !ending.aborted {
            return self.answered(seq, answer);
        }
        self.answered(seq, Err(proto::Refusal::Undone.into()));
        let rolling_back = self.undo.steps.get(&seq).is_some_and(|s| matches!(s.what, What::Undo { .. }));
        if !rolling_back && let (Some(reply), Some(undoable)) = (self.undo_replies.remove(&seq), ending.undoable) {
            // It changed nothing: it's back as it was.
            self.answer(reply, Ok(undoable));
        }
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
        let seq = self.arrived(id, Arrival::Action(running));
        self.ended(Observed::Finished(seq, Some(entry)));
    }

    /// The running action of torrent `id` that's `action`, aborted or not.
    pub(super) fn running_step(&self, id: TorrentId, action: UndoAction) -> Option<u64> {
        self.undo.steps.iter().find_map(|(seq, s)| match &s.what {
            What::Action(r) if r.id == id && r.action == action && s.phase != Phase::Queued => Some(*seq),
            What::Action(_) | What::Undo { .. } | What::Command(_) => None,
        })
    }

    /// Undo: the top step, at once, whatever runs: one done gets its undo on its torrent's lane,
    /// one running is aborted, one queued is dropped. The client hears once it's back, or why not.
    pub(super) async fn undo(&mut self, reply: UndoReply) {
        let Some(top) = self.undo.top().cloned() else { return self.answer(reply, Err(proto::Refusal::NothingToUndo.into())) };
        let queued = self.undo.steps.get(&top.seq).filter(|s| s.phase == Phase::Queued).map(|s| s.what.clone());
        self.undo = stack::decide(&self.undo, &stack::Event::Command(Command::Undo)).expect("something on the stack");
        self.save_undo();
        match queued {
            // Dropped before it started: it changed nothing.
            Some(What::Action(running)) => {
                self.answered(top.seq, Err(proto::Refusal::Undone.into()));
                self.answer(reply, Ok(running.undoable(top.seq)));
            }
            Some(What::Undo { .. } | What::Command(_)) | None => {
                self.undo_replies.insert(top.seq, reply);
                self.run_steps().await;
            }
        }
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
        self.undo.lanes.iter().any(|(id, lane)| {
            self.torrents.get(id).is_some_and(|t| t.info_hash == info_hash)
                || lane.iter().any(|seq| match self.undo.steps.get(seq).map(|s| &s.what) {
                    Some(What::Action(r)) => r.info_hash == info_hash,
                    Some(What::Undo { entry, .. }) => entry.info_hash == info_hash,
                    Some(What::Command(_)) | None => false,
                })
        })
    }

    /// What clients see of torrent `id`: nothing, while a Remove or Delete of it is on the stack.
    pub(super) fn shown(&self, id: TorrentId) -> Option<view::Shown<'_>> {
        view::shown(self, &self.undo, id)
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
        view::listed(self, &self.undo, id)
    }

    /// The torrents clients see, by id: those in the list and those an undo is bringing back.
    pub(super) fn listing(&self) -> Vec<TorrentInfo> {
        view::listing(self, &self.undo)
    }
}

/// What the steps kept of the torrents, for the fold of what clients see ([`view`]).
impl view::Torrents<Running, undo::Entry> for Actor {
    fn ids(&self) -> BTreeSet<TorrentId> {
        self.torrents.keys().copied().collect()
    }

    fn now(&self, id: TorrentId) -> Option<Option<State>> {
        self.torrents.get(&id).filter(|t| t.lifecycle.status(false).is_some()).map(|t| Some(t.lifecycle.clone()))
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
            Back::Gone { .. } | Back::InList { .. } => {
                view::Undoing::BringsBack { was: entry.listed.as_ref().map(|s| view::Was { info: &s.info, state: self.coming_back(entry) }) }
            }
            Back::Files { previous, .. } => view::Undoing::Restores { previous: previous.iter().map(|(i, p, _)| (*i, *p)).collect() },
        }
    }
}
