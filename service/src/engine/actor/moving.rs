//! A Move (#24): the actor's side of `lifecycle::moving`. A Move is a step on its torrent's lane
//! (#190) from the moment it comes: once it runs, it's decided on the torrent as it is, its plan is
//! made (refused with the clash, if nothing says what to do with what's in the new destination,
//! or what's there isn't what the clash was answered for),
//! the torrent is kept aside with the pieces librqbit has (its `.bitv`, as for Undo), written ahead
//! in its record, and taken out of librqbit; then its files move on blocking tasks, each reporting
//! back (`moving.rs`), and the step ends, its client answered, once it's moved or back where it was.
//! What comes for the torrent meanwhile waits its turn behind it there, shown done at once; a Remove
//! or a Delete cancels it first, as it comes. A move a restart finishes runs on no step: what comes
//! for its torrent meanwhile is held, as while librqbit samples its pieces (`steps.rs`).
//!
//! A Rename (#331) is a move within its torrent's own destination under a new name ([`Actor::begin`]
//! with a name), each file landing at its path under it: an action on the undo stack, whose part
//! runs until it's renamed or back as it was, and whose undo is a rename back. The record names
//! the new name from where it names the new destination on (`Moving::names`), so a start after a
//! crash finishes or puts back a rename as it does a move. Refused before anything moves when
//! anything at all is at the new name (never merged into a folder there), or another torrent in
//! the list keeps its files there.
//!
//! Once it's over the torrent is size-checked where its files are now, then added back to librqbit
//! with the pieces it had, which librqbit samples as at a start: no full check. Unless a file of it
//! wasn't the one librqbit had open (replaced by hand): then it's checked in full.
//!
//! Each write-ahead save that the move relies on must land: one that fails before the take-out
//! refuses the move, one that fails as the files are staged fails it, restored, and one that fails
//! once they've all landed takes them back into staging, then restores it, before any original
//! goes. From that last save on (the record's `landed`) a move never goes back: a start after a
//! crash goes straight on removing the old. A file blocked
//! from its place in the end (something of the person's put at its new path) has the record name
//! the old destination again, and the move restores, always: a file deleted by hand meanwhile stays
//! deleted, its copy in the Trash, and the torrent is back where it was, Missing files. A shutdown
//! puts back a move whose files aren't in place yet; an engine restart for new settings waits for
//! it, as for any long work (ADR 0008). A move drops the stack's entries that would put files of the
//! torrent back from the Trash into its old destination: they leave as moved.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use lifecycle::moving::{self as stages, Stage, Task};
use proto::{AbsolutePath, Action, Answered, CantUndo, FileName, NotMoved, TorrentId, TorrentRefusal, UndoAction};
use tracing::warn;

use super::super::files::Destination;
use super::super::moving::{self, Refused, Restored, Stop};
use super::super::store::{Moving, Names};
use super::super::undo::{Back, Entry};
use super::super::{Failure, bitv};
use super::steps::Over;
use super::{Actor, Event, Observation, Phase, lib};

/// A move under way, the actor's side: the step whose part it is (none for one a restart
/// finishes), the task running of it, what stops its copy, and the files librqbit had open that
/// weren't the ones at their paths as it began (replaced by hand).
#[derive(Default)]
pub(in super::super) struct Mover {
    step: Option<StepOf>,
    task: Option<Task>,
    stop: Arc<AtomicBool>,
    replaced: BTreeSet<usize>,
}

/// The step a move is the part of, which ends with it: a Move's (#24), a Rename's (#331: an action
/// on the stack), or the undo of a Rename, which renames it back.
#[derive(Debug, Clone)]
pub(super) enum StepOf {
    Move(u64),
    Rename(u64),
    Undo { seq: u64, of: u64, entry: Entry },
}

impl StepOf {
    fn seq(&self) -> u64 {
        match self {
            StepOf::Move(seq) | StepOf::Rename(seq) | StepOf::Undo { seq, .. } => *seq,
        }
    }

    fn renames(&self) -> bool {
        match self {
            StepOf::Move(_) => false,
            StepOf::Rename(_) | StepOf::Undo { .. } => true,
        }
    }
}

/// How a move's task ended.
pub(super) enum Moved {
    Staged(Result<Vec<(usize, moving::Ino)>, Stop>),
    /// Every staged file took its place, or why not (those that had are staged again).
    Landed(Result<moving::Landed, NotMoved>),
    /// A landing that couldn't be written ahead is taken back, for this reason.
    Unlanded(NotMoved),
    /// The old removed, with what couldn't be.
    Finished(Vec<String>),
    Restored(Restored),
}

/// The test's hold on moves: the gate each copy waits at after each chunk, the gate the end waits
/// at after each file takes its place, whether a move on the same drive copies all the same,
/// whether the move's saves fail, whether the save that every file landed fails, a crash just
/// after the move is written ahead, before the torrent is taken out of librqbit, and one once the
/// old files are gone, before the record says the move is over.
#[cfg(test)]
#[derive(Default, Clone)]
pub(in super::super) struct Hold {
    pub gate: Option<super::super::trash::Gate>,
    pub finish_gate: Option<super::super::trash::Gate>,
    pub copies: bool,
    pub saves_fail: bool,
    pub landed_save_fails: bool,
    pub crash_before_take_out: Option<Arc<AtomicBool>>,
    pub crash_once_moved: Option<Arc<AtomicBool>>,
}

impl Actor {
    /// Move step `seq` of torrent `id` into `to`: its step runs until the move is over, and its
    /// client hears how it went then. Refused, or moving nowhere, it's over at once.
    pub(super) async fn start_move(&mut self, seq: u64, id: TorrentId, to: AbsolutePath, clash: Option<Answered>, name: Option<FileName>) {
        // A Remove or a Delete came for it while it waited its turn: it's cancelled, as one under way.
        if self.removed_behind(seq, id) {
            let name = self.torrents.get(&id).map_or_else(|| format!("torrent {id}"), |t| name_of(&t.record));
            return self.step_ended(
                seq,
                id,
                Over::Finished(None),
                Err(Failure::torrent(id, TorrentRefusal::NotMoved { name, why: NotMoved::Cancelled })),
            );
        }
        match self.begin_move(id, to, clash, name).await {
            Ok(true) => {
                if let Some(mover) = self.moves.get_mut(&id) {
                    mover.step = Some(StepOf::Move(seq));
                }
            }
            Ok(false) => self.step_ended(seq, id, Over::Finished(None), Ok(())),
            Err(e) => self.step_ended(seq, id, Over::Finished(None), Err(e)),
        }
    }

    /// Renames torrent `id` to `name` on disk (#331), as `step`: a Rename's part, or its undo's.
    /// Its step runs until it's renamed, or back as it was. Refused, or to the name it has, it's
    /// over at once. A Rename writes ahead the name it had, which its undo gives back.
    pub(super) async fn start_rename(&mut self, id: TorrentId, name: String, step: StepOf) {
        if self.removed_behind(step.seq(), id) {
            let named = self.torrents.get(&id).map_or_else(|| format!("torrent {id}"), |t| name_of(&t.record));
            return self.move_over(
                step,
                id,
                Err(Failure::torrent(id, TorrentRefusal::NotRenamed { name: named, why: NotMoved::Cancelled })),
            );
        }
        let was = self.torrents.get(&id).and_then(|t| t.record.disk_name());
        if let (StepOf::Rename(seq), Some(mut running)) = (&step, self.action_part(step.seq(), id).cloned())
            && let Some(renaming) = running.renaming.as_mut()
        {
            renaming.was = was;
            self.report(super::steps::Fact::Moved(*seq, id, running));
        }
        match self.begin_rename(id, name).await {
            Ok(true) => {
                if let Some(mover) = self.moves.get_mut(&id) {
                    mover.step = Some(step);
                }
            }
            Ok(false) => self.move_over(step, id, Ok(false)),
            Err(e) => self.move_over(step, id, Err(e)),
        }
    }

    /// Step `step`'s part on torrent `id` is over: moved or renamed (`true`), nothing to do
    /// (`false`), or why not. A Rename that renamed is committed with what undoes it.
    fn move_over(&mut self, step: StepOf, id: TorrentId, outcome: Result<bool, Failure>) {
        match step {
            StepOf::Move(seq) => self.step_ended(seq, id, Over::Finished(None), outcome.map(|_| ())),
            StepOf::Rename(seq) => {
                self.let_go_under_rename(id, &outcome);
                let was = self.action_part(seq, id).and_then(|r| r.renaming.as_ref()).and_then(|r| r.was.clone());
                let over = match (&outcome, was) {
                    (Ok(true), Some(was)) => self.torrents.get(&id).map_or(Over::Failed, |t| Over::Finished(Some(renamed(t, was)))),
                    (Ok(true), None) => {
                        warn!(id, "a rename without the name it had: it can't be undone");
                        Over::Failed
                    }
                    (Ok(false), _) => Over::Finished(None),
                    (Err(_), _) => Over::Failed,
                };
                self.step_ended(seq, id, over, outcome.map(|_| ()));
            }
            StepOf::Undo { seq, of, entry } => {
                self.let_go_under_rename(id, &outcome);
                let answer = outcome.map(|_| entry.undoable(of)).map_err(|failure| match failure {
                    Failure::Refused(proto::Refusal::Torrent { why, .. }) => {
                        let why = undo_refused(id, why);
                        Failure::Refused(proto::Refusal::Undo { undo: entry.undoable(of), why, stranded: Vec::new() })
                    }
                    other => other,
                });
                self.undo_ended(seq, id, answer);
            }
        }
    }

    /// A rename of torrent `id` that renamed (`outcome` `true`) took it out of librqbit and added it
    /// back, so what it had of a file still Off, in the Trash for a Move to Trash kept on the stack
    /// under it, is gone: those Moves to Trash's undos check the torrent.
    fn let_go_under_rename(&mut self, id: TorrentId, outcome: &Result<bool, Failure>) {
        if let (Ok(true), Some(t)) = (outcome, self.torrents.get(&id)) {
            self.steps.let_go_under(id, |i| !t.record.gets(i));
        }
    }

    /// Starts renaming torrent `id` to `name` (#331), decided on it as it is now: whether it
    /// moves. Nothing is ever merged into what's at the new name: anything there refuses it.
    async fn begin_rename(&mut self, id: TorrentId, name: String) -> Result<bool, Failure> {
        let refused = |why| Failure::torrent(id, why);
        if !self.torrents.contains_key(&id) {
            return Err(refused(TorrentRefusal::NotInList));
        }
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(Action::Rename)).await?;
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        if record.disk_name().as_deref() == Some(name.as_str()) {
            return Ok(false);
        }
        self.name_free(id, &record.destination, &name)?;
        let to = record.destination.clone();
        let plan = self.plan_in(id, &to, Some(&name), None)?;
        self.begin(id, to, Some(name), plan).await
    }

    /// Whether torrent `id` can take the name `name` in the folder `to` (#331), as a Rename into its
    /// own destination or a Move under a new name (#270): refused if anything is at it (never merged
    /// into), or it's another listed torrent's place.
    fn name_free(&self, id: TorrentId, to: &std::path::Path, name: &str) -> Result<(), Failure> {
        let refused = |why| Failure::torrent(id, why);
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        let location = record.location().context("a torrent with info has a location")?;
        let at = to.join(name);
        let path = || AbsolutePath::new(at.clone()).map_err(|p| anyhow::anyhow!("not an absolute path: {}", p.display()));
        if let Ok(there) = std::fs::symlink_metadata(&at) {
            // The torrent itself, on a drive that doesn't tell cases apart.
            let own = std::fs::symlink_metadata(&location).ok().map(|m| moving::Ino::of(&m));
            let had = record.disk_name().unwrap_or_default();
            return Err(refused(match (own == Some(moving::Ino::of(&there)), moving::normalized(&had) == moving::normalized(&name)) {
                (true, true) => TorrentRefusal::OnlyNormalization,
                (true, false) => TorrentRefusal::OnlyCase,
                (false, _) => TorrentRefusal::NameTaken { path: path()? },
            }));
        }
        let folded = |p: &std::path::Path| moving::normalized(&p.to_string_lossy().to_lowercase());
        let other = self.torrents.values().filter(|t| t.record.id != id).find(|t| {
            t.record.location().is_some_and(|theirs| {
                let (theirs, ours) = (folded(&theirs), folded(&at));
                theirs == ours || theirs.starts_with(&format!("{ours}/"))
            })
        });
        match other {
            Some(other) => Err(refused(TorrentRefusal::NameOfAnother { path: path()?, other: name_of(&other.record) })),
            None => Ok(()),
        }
    }

    /// Whether a Remove or a Delete of torrent `id` waits behind step `seq` on its lane (a Move
    /// joins its lane at its turn, so by place, not by seq).
    fn removed_behind(&self, seq: u64, id: TorrentId) -> bool {
        let machine = self.steps.machine();
        let behind = machine.lanes.get(&id).into_iter().flatten().skip_while(|s| **s != seq).skip(1);
        behind.filter_map(|s| machine.steps.get(s)).any(|step| {
            matches!(&step.what, lifecycle::undo::What::Action(parts)
                if matches!(parts.get(&id), Some(lifecycle::undo::Part::Doing(r)) if matches!(r.action, UndoAction::Remove | UndoAction::Delete)))
        })
    }

    /// Torrent `id`'s plan for a move into `to`, decided on it as it is now: whether it moves,
    /// and what takes what. Folders are told by what they are on disk, not by how they're spelled:
    /// one it's in already, or one inside its own, is refused. Nothing is changed.
    async fn plan_move(
        &mut self,
        id: TorrentId,
        to: AbsolutePath,
        name: Option<&str>,
        clash: Option<&Answered>,
    ) -> Result<(AbsolutePath, moving::Plan), Failure> {
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(Action::Move)).await?;
        let refused = |why| Failure::torrent(id, why);
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        let to = Destination::parse(to).map_err(|reason| Failure::Refused(proto::Refusal::Destination { reason }))?.into_path();
        if moving::same_folder(&record.destination, &to) {
            return Err(refused(TorrentRefusal::AlreadyThere));
        }
        let location = record.location().context("a torrent with info has a location")?;
        if moving::inside(&to, &location) {
            return Err(refused(TorrentRefusal::IntoItself));
        }
        if let Some(name) = name {
            self.name_free(id, &to, name)?;
        }
        let plan = self.plan_in(id, &to, name, clash)?;
        Ok((to, plan))
    }

    /// Torrent `id`'s plan for a move into `to`, under `name` there if it's given one, `clash`
    /// answering what's at its paths there. Another torrent's file at one of its new paths is never
    /// replaced nor taken (#270), whatever the clash's answer: refused before it's asked about.
    fn plan_in(&self, id: TorrentId, to: &AbsolutePath, name: Option<&str>, clash: Option<&Answered>) -> Result<moving::Plan, Failure> {
        let refused = |why| Failure::torrent(id, why);
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        let not_done = not_done(record, to);
        let files = inside(record);
        let news: Vec<(usize, PathBuf)> = files.iter().map(|(i, path)| (*i, moving::new_path(path, name))).collect();
        if let Some((path, _)) = moving::anothers(to, &news, self.others_files(id)) {
            return match AbsolutePath::new(path) {
                Ok(path) => Err(refused(TorrentRefusal::AnothersFiles { path })),
                Err(path) => Err(anyhow::anyhow!("not an absolute path: {}", path.display()).into()),
            };
        }
        let renames = moving::same_volume(&record.destination, to);
        #[cfg(test)]
        let renames = renames && !self.move_hold.copies;
        moving::plan(&record.destination, to, name, &record.info_hash, &files, clash, renames).map_err(|r| match r {
            Refused::Clash(files) => refused(TorrentRefusal::Clash { files }),
            Refused::Changed(path) => match AbsolutePath::new(path) {
                Ok(path) => refused(TorrentRefusal::ClashChanged { path }),
                Err(path) => anyhow::anyhow!("not an absolute path: {}", path.display()).into(),
            },
            Refused::NotAFolder(path) => match AbsolutePath::new(path) {
                Ok(path) => refused(not_done(NotMoved::NotAFolder { path })),
                Err(path) => anyhow::anyhow!("not an absolute path: {}", path.display()).into(),
            },
        })
    }

    /// Where every torrent in the list but `id` has its files: at their paths, and for one moving,
    /// in its old destination, its staging folder and its new one too; and those being added, or
    /// brought back by an undo ([`super::super::places`]).
    pub(super) fn others_files(&self, id: TorrentId) -> impl Iterator<Item = (TorrentId, PathBuf)> + '_ {
        self.torrents
            .iter()
            .filter(move |(other, _)| **other != id)
            .flat_map(|(other, t)| {
                let moving = t.record.moving.iter().flat_map(|m| m.plan.paths());
                t.record.paths().into_iter().chain(moving.collect::<Vec<_>>()).map(|p| (*other, p))
            })
            .chain(self.guard.places().others(id).into_iter().filter(|(other, _)| {
                // Not an add taken off the list meanwhile.
                self.torrents.contains_key(other) || self.guard.places().name(*other).is_some()
            }))
    }

    /// Which of `ids` a move into `to` would find files at (`Request::Clashes`): in the order of
    /// `ids`. A torrent that can't move at all, or not there, is left out. One whose files would go
    /// where another listed torrent's are is in, with that torrent and the files there now: only a
    /// new name moves it (#270). Another listed torrent includes one an earlier id of `ids` would
    /// move there: the second of two with the same paths is told as clashing with the first.
    pub(super) async fn clashes(&mut self, ids: Vec<TorrentId>, to: AbsolutePath) -> Vec<proto::Clashing> {
        let mut found = Vec::new();
        let mut earlier: Vec<(TorrentId, PathBuf)> = Vec::new();
        for id in ids {
            let own = match self.plan_move(id, to.clone(), None, None).await {
                Ok(_) => None,
                Err(Failure::Refused(proto::Refusal::Torrent { why: TorrentRefusal::Clash { files }, .. })) => Some(files),
                Err(Failure::Refused(proto::Refusal::Torrent { why: TorrentRefusal::AnothersFiles { .. }, .. })) => {
                    let Some(record) = self.torrents.get(&id).map(|t| &t.record) else { continue };
                    let Some((_, other)) = moving::anothers(&to, &inside(record), self.others_files(id)) else { continue };
                    let files = moving::found_at(&to, &inside(record));
                    found.push(proto::Clashing { id, files, other: Some(other) });
                    continue;
                }
                Err(_) => continue,
            };
            let Some(record) = self.torrents.get(&id).map(|t| &t.record) else { continue };
            let files = inside(record);
            if let Some((_, other)) = moving::anothers(&to, &files, earlier.iter().cloned()) {
                found.push(proto::Clashing { id, files: moving::found_at(&to, &files), other: Some(other) });
                continue;
            }
            earlier.extend(files.iter().map(|(_, path)| (id, to.join(path))));
            if let Some(files) = own {
                found.push(proto::Clashing { id, files, other: None });
            }
        }
        found
    }

    /// A name torrent `id` could take in the folder `to`: its own with ` (2)`, ` (3)`… added (before
    /// the extension, for a torrent of one file), the first that `name_free` and `plan_in` accept
    /// now, as a Move under that name would be: nothing at it, no other torrent's path.
    pub(super) fn free_name(&self, id: TorrentId, to: &AbsolutePath) -> Result<proto::FileName, Failure> {
        let record = &self.torrents.get(&id).ok_or(Failure::torrent(id, TorrentRefusal::NotInList))?.record;
        let name = record.disk_name().context("a torrent with info has a name")?;
        let one_file = matches!(&inside(record)[..], [(_, path)] if path.components().count() == 1);
        let (stem, extension) = match name.rfind('.') {
            Some(dot) if one_file && dot > 0 && dot + 1 < name.len() => name.split_at(dot),
            _ => (name.as_str(), ""),
        };
        let files = inside(record);
        for n in 2..=proto::FileName::MAX_NUMBER {
            let free = proto::FileName::numbered(stem, n, extension).context("no name fits")?;
            match self.name_free(id, to, free.as_str()) {
                Ok(()) => {}
                // Taken, or the torrent's own place spelled so: the next number.
                Err(Failure::Refused(proto::Refusal::Torrent {
                    why:
                        TorrentRefusal::NameTaken { .. }
                        | TorrentRefusal::NameOfAnother { .. }
                        | TorrentRefusal::OnlyCase
                        | TorrentRefusal::OnlyNormalization,
                    ..
                })) => continue,
                // Anything else won't change with the number: no suggestion.
                Err(other) => return Err(other),
            }
            let news: Vec<(usize, PathBuf)> = files.iter().map(|(i, path)| (*i, moving::new_path(path, Some(free.as_str())))).collect();
            if moving::anothers(to, &news, self.others_files(id)).is_none() {
                return Ok(free);
            }
        }
        Err(Failure::from(anyhow::anyhow!("no free name")))
    }

    /// Starts moving torrent `id` into `to`, decided on it as it is now ([`Actor::plan_move`]).
    async fn begin_move(
        &mut self,
        id: TorrentId,
        to: AbsolutePath,
        clash: Option<Answered>,
        name: Option<FileName>,
    ) -> Result<bool, Failure> {
        let name = name.map(String::from);
        let (to, plan) = self.plan_move(id, to, name.as_deref(), clash.as_ref()).await?;
        self.begin(id, to, name, plan).await
    }

    /// Starts moving torrent `id`'s files into the folder `to` as `plan` says, under `name` there if
    /// it's given one (its own folder's, or its one file's: a Rename is `to` its own destination);
    /// decided and checked by the caller (`begin_move`, `begin_rename`). Whether it moves.
    async fn begin(&mut self, id: TorrentId, to: AbsolutePath, name: Option<String>, plan: moving::Plan) -> Result<bool, Failure> {
        let refused = |why| Failure::torrent(id, why);
        let record = &self.torrents.get(&id).ok_or(refused(TorrentRefusal::NotInList))?.record;
        let not_done = not_done(record, &to);
        // A copy that won't fit is refused at once, rather than filling the disk first.
        if !moving::room(&plan) {
            return Err(refused(not_done(NotMoved::DiskFull)));
        }
        // Into another folder: what Undo would put back from the Trash would go to the old one.
        let elsewhere = !moving::same_folder(&record.destination, &to);
        let names =
            name.map(|name| Names { was: record.on_disk.clone(), now: Some(name).filter(|n| Some(n) != record.info_name().as_ref()) });
        // Kept aside as it is, with the pieces librqbit has: they come back with it. Written ahead:
        // one that can't be written doesn't move.
        let had = self.pieces(id);
        let stash = self.stash_of(id);
        // librqbit trusts the pieces it saved of a file it can read, whatever its bytes: those of a
        // file replaced by hand were of the one it had open, which doesn't move.
        let replaced = self.guard.trashed().replaced(&record.info_hash);
        let torrent = self.torrents.get_mut(&id).expect("just found");
        torrent.record.moving =
            Some(Moving { plan, stash: stash.clone(), placed: false, landed: false, replaced: !replaced.is_empty(), names });
        if let Err(e) = self.save_move() {
            let torrent = self.torrents.get_mut(&id).expect("just found");
            torrent.record.moving = None;
            let why = NotMoved::Failed { detail: format!("couldn't save where it goes: {e:#}") };
            return Err(refused(not_done(why)));
        }
        // As if the process died here: written ahead, not taken out.
        #[cfg(test)]
        if let Some(crashed) = self.move_hold.crash_before_take_out.take() {
            self.session.stop().await;
            self.killed = true;
            crashed.store(true, Ordering::SeqCst);
            return Ok(true);
        }
        self.moves.insert(id, Mover { replaced, ..Mover::default() });
        // Undo can't put files of it back from the Trash any more: they'd go to the old destination.
        // Renamed where it is, they can, once the Rename is undone (an undo above them on the stack).
        if elsewhere {
            self.report(super::steps::Fact::Moving(id));
        }
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
            let landed = torrent.record.moving.as_ref().is_some_and(|m| m.landed);
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
                // Landed, written ahead: the old goes.
                stages::Do::Finish if landed => {
                    mover.task = Some(Task::Finish);
                    let trash = self.trash.clone();
                    self.move_tasks.spawn_blocking(move || (id, Moved::Finished(moving::remove_old(&plan, &trash))));
                }
                stages::Do::Finish => {
                    mover.task = Some(Task::Finish);
                    let trash = self.trash.clone();
                    #[cfg(test)]
                    let gate = self.move_hold.finish_gate.clone();
                    self.move_tasks.spawn_blocking(move || {
                        let between = |_placed: &std::path::Path| {
                            #[cfg(test)]
                            if let Some(gate) = &gate {
                                return super::super::trash::wait_at(gate, _placed);
                            }
                            true
                        };
                        (id, Moved::Landed(moving::land(&plan, &trash, &between)))
                    });
                }
                stages::Do::Restore => {
                    mover.task = Some(Task::Restore);
                    let trash = self.trash.clone();
                    self.move_tasks.spawn_blocking(move || (id, Moved::Restored(moving::restore(&plan, &trash))));
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

    /// A move's task ended as `ended` says: staged, the record names the new destination and what's
    /// staged of each file (written ahead: from here a start finishes the move) and the files take
    /// their places; one that failed is undone; one blocked from its place names the old one again,
    /// and is undone; once finished or undone the move is over.
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
            Moved::Staged(Ok(staged)) if placing => match self.placed(id, |plan| plan.staged_as(&staged), true) {
                Ok(check) => stages::Event::Placed { check },
                Err(e) => {
                    self.placed(id, |plan| plan.staged_as(&[]), false).ok();
                    stages::Event::Failed(NotMoved::Failed { detail: format!("couldn't save where it goes: {e:#}") })
                }
            },
            // Stopped already: it's restored.
            Moved::Staged(Ok(_) | Err(Stop::Stopped)) => return,
            Moved::Staged(Err(Stop::Failed(why))) => stages::Event::Failed(why),
            // Landed: written ahead before any original goes. One that can't be is taken back,
            // and the move goes back with every original there.
            Moved::Landed(Ok(landed)) => {
                let Err(e) = self.landed(id) else { return };
                let why = NotMoved::Failed { detail: format!("couldn't save that its files are in place: {e:#}") };
                let (Some(plan), Some(mover)) =
                    (self.torrents.get(&id).and_then(|t| t.record.moving.as_ref()).map(|m| m.plan.clone()), self.moves.get_mut(&id))
                else {
                    return;
                };
                mover.task = Some(Task::Finish);
                let trash = self.trash.clone();
                self.move_tasks.spawn_blocking(move || {
                    moving::unland(&plan, &trash, &landed);
                    (id, Moved::Unlanded(why))
                });
                return;
            }
            Moved::Landed(Err(why)) | Moved::Unlanded(why) => {
                if let Err(e) = self.placed(id, |_| {}, false) {
                    warn!(id, error = %e, "couldn't save that a move goes back");
                }
                stages::Event::Blocked(why)
            }
            Moved::Restored(Restored { warnings, replaced }) => {
                if let Some(moving) = self.torrents.get_mut(&id).and_then(|t| t.record.moving.as_mut()) {
                    moving.replaced |= replaced;
                }
                for warning in warnings {
                    warn!(id, warning, "a move going back left something behind");
                }
                return self.move_ended(id).await;
            }
            Moved::Finished(warnings) => {
                for warning in warnings {
                    warn!(id, warning, "a move left something behind");
                }
                // As if the process died here: the old files gone, the move not over in the record.
                #[cfg(test)]
                if let Some(crashed) = self.move_hold.crash_once_moved.take() {
                    self.session.stop().await;
                    self.killed = true;
                    crashed.store(true, Ordering::SeqCst);
                    return;
                }
                return self.move_ended(id).await;
            }
        };
        if let Err(e) = self.event(id, moved(event)).await {
            warn!(id, error = ?e, "failed to follow a move");
        }
    }

    /// Torrent `id`'s move is placed (`placed`: the record names the new destination) or not (the
    /// old one), `change` made to its plan, written ahead: whether it takes files that were there
    /// already, or why the save failed.
    fn placed(&mut self, id: TorrentId, change: impl FnOnce(&mut moving::Plan), placed: bool) -> anyhow::Result<bool> {
        let torrent = self.torrents.get_mut(&id).context("moving, not in the list")?;
        let moving = torrent.record.moving.as_mut().context("moving without a plan")?;
        change(&mut moving.plan);
        moving.placed = placed;
        let to = if placed { &moving.plan.to } else { &moving.plan.from };
        torrent.record.destination = AbsolutePath::new(to.clone()).map_err(|p| anyhow::anyhow!("a move to {}", p.display()))?;
        if let Some(names) = &moving.names {
            torrent.record.on_disk = if placed { names.now.clone() } else { names.was.clone() };
        }
        let check = moving.plan.adopts();
        self.save_move()?;
        Ok(check)
    }

    /// Every file of torrent `id`'s move landed, written ahead before any original goes: why not.
    fn landed(&mut self, id: TorrentId) -> anyhow::Result<()> {
        let moving = self.torrents.get_mut(&id).and_then(|t| t.record.moving.as_mut()).context("moving without a plan")?;
        moving.landed = true;
        #[cfg(test)]
        let saved = if self.move_hold.landed_save_fails { Err(anyhow::anyhow!("the disk is full")) } else { self.save_move() };
        #[cfg(not(test))]
        let saved = self.save_move();
        if saved.is_err()
            && let Some(moving) = self.torrents.get_mut(&id).and_then(|t| t.record.moving.as_mut())
        {
            moving.landed = false;
        }
        saved
    }

    /// Saves the records, a move's write-ahead: an error if it can't.
    fn save_move(&self) -> anyhow::Result<()> {
        #[cfg(test)]
        if self.move_hold.saves_fail {
            anyhow::bail!("the disk is full");
        }
        self.try_save()
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
        let renames = mover.step.as_ref().is_some_and(StepOf::renames);
        let name = name_of(&torrent.record);
        // Moved: what isn't at its new path is out of it, missing there.
        if let (Stage::RemovingOld { .. }, Some(moving)) = (stage, torrent.record.moving.as_mut()) {
            moving::left_out(&mut moving.plan);
        }
        let plan = torrent.record.moving.as_ref().map(|m| m.plan.clone());
        // Where its files are: back where they were, or moved.
        let at = plan.as_ref().map(|p| if matches!(stage, Stage::Restoring { .. }) { &p.from } else { &p.to });
        if let Some(at) = at.and_then(|at| AbsolutePath::new(at.clone()).ok()) {
            torrent.record.destination = at;
        }
        // And the name it has there: the one it had, or its new one.
        if let Some(names) = torrent.record.moving.as_ref().and_then(|m| m.names.clone()) {
            torrent.record.on_disk = if matches!(stage, Stage::Restoring { .. }) { names.was } else { names.now };
        }
        let answer = match stage {
            Stage::Restoring { why } if renames => Err(Failure::torrent(id, TorrentRefusal::NotRenamed { name, why: why.clone() })),
            Stage::Restoring { why } => Err(Failure::torrent(id, TorrentRefusal::NotMoved { name, why: why.clone() })),
            // Renamed: a file missing is missing under its new name, as it was under the old.
            Stage::TakingOut | Stage::Copying { .. } | Stage::RemovingOld { .. } if renames => Ok(()),
            Stage::TakingOut | Stage::Copying { .. } | Stage::RemovingOld { .. } => {
                let paths = torrent.record.paths();
                let missing: Vec<AbsolutePath> = plan
                    .iter()
                    .flat_map(|p| p.missing())
                    .filter(|i| torrent.record.gets(*i))
                    .filter_map(|i| AbsolutePath::new(paths.get(i)?.clone()).ok())
                    .collect();
                match missing.is_empty() {
                    true => Ok(()),
                    false => Err(Failure::torrent(id, TorrentRefusal::MovedWithout { name, files: missing })),
                }
            }
        };
        let before = torrent.lifecycle.phase.clone();
        torrent.lifecycle = super::decide(&torrent.lifecycle, &moved(stages::Event::Ended)).expect("never refused");
        super::track_uncounted(&mut torrent.polled.uncounted, &before, &torrent.lifecycle.phase);
        match torrent.record.moving.take() {
            // Checked in full once librqbit has it again. Till then (Missing files keeps it out),
            // none of a file replaced by hand is had: what librqbit said of it was of the one it had
            // open.
            Some(moving) if moving.replaced => {
                for &i in &mover.replaced {
                    if let Some(p) = torrent.polled.file_progress.get_mut(i) {
                        *p = 0;
                    }
                }
            }
            Some(Moving { stash: Some(stash), .. }) => {
                if let Err(e) = stash.restore_pieces(&bitv::path(&session, &torrent.record.info_hash)) {
                    warn!(id, error = %e, "couldn't put back the pieces of a torrent moved: it's checked in full");
                }
            }
            Some(_) | None => {}
        }
        torrent.keep_known();
        self.save();
        // The stash isn't named any more.
        self.save_undo();
        // Judged where its files are now before librqbit has it, which would make the missing ones again.
        self.size_check(id).await;
        if let Err(e) = self.carry_out(id).await {
            warn!(id, error = ?e, "couldn't add a torrent back once it moved");
        }
        match mover.step {
            Some(step) => self.move_over(step, id, answer.map(|()| true)),
            None => {
                if let Err(e) = answer {
                    warn!(id, error = ?e, "a move a start finished");
                }
            }
        }
    }

    /// ⌘Z on a running Rename, step `seq` (#331): its rename stops, as any move cancelled, and is put
    /// back, unless its files are in place already; then it ends renamed, and is renamed back as
    /// its rollback.
    pub(super) async fn abort_renames(&mut self, seq: u64) {
        let ids: Vec<TorrentId> =
            self.moves.iter().filter(|(_, m)| matches!(m.step, Some(StepOf::Rename(s)) if s == seq)).map(|(id, _)| *id).collect();
        for id in ids {
            if let Err(e) = self.event(id, moved(stages::Event::Failed(NotMoved::Cancelled))).await {
                warn!(id, error = ?e, "failed to stop a rename");
            }
        }
        self.go_on_moving();
    }

    /// Torrent `id`'s Move that waits its turn behind another's never starts (Cancel move, a Remove
    /// or a Delete): its client hears it was cancelled. Whether one waited.
    pub(super) fn dequeue_move(&mut self, id: TorrentId, why: NotMoved) -> bool {
        let name = self.torrents.get(&id).map_or_else(|| format!("torrent {id}"), |t| name_of(&t.record));
        let Some(answers) = self.steps.dequeue(id, Failure::torrent(id, TorrentRefusal::NotMoved { name, why })) else { return false };
        self.save_undo();
        // Its row shows what it was doing again, whether or not its client has an answer yet.
        self.publish();
        self.deliver(answers);
        true
    }

    /// A Remove or a Delete came for torrent `id`: a move of it that waits its turn never starts,
    /// and one whose files aren't in place yet is cancelled first, what it moved going back, before
    /// the Remove or the Delete runs.
    pub(super) async fn cancel_move(&mut self, id: TorrentId, action: Action) {
        self.dequeue_move(id, NotMoved::Cancelled);
        if matches!(self.torrents.get(&id).map(|t| &t.lifecycle.phase), Some(Phase::Moving { .. }))
            && let Err(e) = self.event(id, Event::Command(action)).await
        {
            warn!(id, error = ?e, "failed to cancel a move");
        }
    }

    /// The service shuts down: each move whose files aren't in place yet goes back, as a start
    /// after a crash would have it, and the stop waits for it. (An engine restart for new settings
    /// waits for the moves instead, as for any long work.)
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

/// The torrent's name as clients see it, or its info hash.
/// How a move of `record`'s files into `to` that doesn't happen is refused: a Rename's, into its
/// own destination, or a Move's.
fn not_done(record: &super::Record, to: &std::path::Path) -> impl Fn(NotMoved) -> TorrentRefusal + use<> {
    let (name, renaming) = (name_of(record), moving::same_folder(&record.destination, to));
    move |why| match renaming {
        true => TorrentRefusal::NotRenamed { name: name.clone(), why },
        false => TorrentRefusal::NotMoved { name: name.clone(), why },
    }
}

fn name_of(record: &super::Record) -> String {
    record.shown_name().unwrap_or_else(|| record.info_hash.clone())
}

/// What undoes a Rename of torrent `t`, renamed from the name `was` it had on disk.
fn renamed(t: &super::Torrent, was: String) -> Entry {
    let now = t.record.disk_name().unwrap_or_default();
    let record = &t.record;
    Entry {
        action: UndoAction::Rename,
        name: name_of(record),
        info_hash: record.info_hash.clone(),
        id: record.id,
        back: Back::Renamed { name: was, now },
        put_back: Vec::new(),
        listed: None,
    }
}

/// A Rename's refusal, as its undo's: the name it goes back to is taken, say.
fn undo_refused(id: TorrentId, why: TorrentRefusal) -> CantUndo {
    use TorrentRefusal as R;
    match why {
        R::NameTaken { path } | R::NameOfAnother { path, .. } | R::AnothersFiles { path } => CantUndo::PathTaken { path },
        R::NotInList => CantUndo::NotInList,
        R::Deleting | R::PartlyDeleted => CantUndo::Deleting,
        R::DestinationUnavailable { reason, .. } => CantUndo::Destination { reason },
        R::Moving | R::WaitingToMove => CantUndo::Busy,
        R::NotRenamed { why: NotMoved::NotAFolder { path }, .. } => CantUndo::NotAFolder { path },
        R::NotRenamed { why: NotMoved::Taken { path }, .. } => CantUndo::PathTaken { path },
        R::NotRenamed { why: NotMoved::Unavailable { reason }, .. } => CantUndo::Destination { reason },
        why @ (R::NotRenamed { .. }
        | R::NoInfoYet
        | R::NotChecking
        | R::FetchFailed
        | R::NoFile { .. }
        | R::NotTrashed { .. }
        | R::FilesNotTrashed { .. }
        | R::Clash { .. }
        | R::IntoItself
        | R::AlreadyThere
        | R::NotMoved { .. }
        | R::MovedWithout { .. }
        | R::OnlyCase
        | R::OnlyNormalization
        | R::ClashChanged { .. }
        | R::NotWaitingToMove) => CantUndo::Refused { refusal: Box::new(proto::Refusal::Torrent { id, why }) },
    }
}

/// Each of its files (not BEP 47's padding, never written) by index, with its path inside its
/// destination: its own folder's, under the name it has on disk, then the file's in it.
pub(super) fn inside(record: &super::Record) -> Vec<(usize, PathBuf)> {
    let Some(info) = &record.info else { return Vec::new() };
    let paths = record.paths();
    let at = |i: usize| paths.get(i).and_then(|p| p.strip_prefix(&record.destination).ok()).map(PathBuf::from);
    info.files.iter().enumerate().filter(|(_, f)| !f.padding).filter_map(|(i, _)| Some((i, at(i)?))).collect()
}

/// A start finds torrents a crash left moving: the stage each is at, from its record's move. One
/// past the point where the record names the new destination is finished; any other goes back.
pub(in super::super) fn at_start(moving: &Moving) -> Stage {
    match moving.placed {
        true => Stage::RemovingOld { check: moving.plan.adopts() },
        false => Stage::Restoring { why: NotMoved::Interrupted },
    }
}
