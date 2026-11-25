//! The fake's Move (#24), on the shared lifecycle (`lifecycle::moving`) as the engine's: a Move is a
//! step on its torrent's lane, running until the move is over, which the fake's copy takes
//! [`moves_take`](MemoryService::moves_take) seconds of the app clock to do, its progress shown
//! meanwhile. What's asked of the torrent waits its turn behind it; a Remove or a Delete cancels it
//! first, and so does one waiting behind it on its lane; its destination going restores it. Refused
//! as the engine refuses it: into the folder it's in already, or into one inside its own. As it
//! starts, the Moves to Trash of it leave the stack, moved (the shared rule, `lifecycle::undo`). A folder
//! [`holding`](MemoryService::holding) a file of the torrent's already clashes, refused until the
//! person chooses; one a move [`fails into`](MemoryService::move_failing) fails it halfway, and it's
//! back where it was.

use std::path::PathBuf;

use lifecycle::moving::{self as stages, Stage};
use lifecycle::undo::{Part, What};
use lifecycle::{Event, Observation, Phase};
use proto::{AbsolutePath, Action, Clash, NotMoved, Refusal, TorrentId, TorrentRefusal, UndoAction};

use client::Failure;

use super::{MemoryService, Pretend};

/// A move under way: its step, where to, whether it takes files there, and when it started on the
/// app clock (filled in by the first poll after it starts).
#[derive(Clone)]
pub(super) struct Moving {
    seq: u64,
    to: AbsolutePath,
    /// The name it takes there (#270), once moved.
    name: Option<String>,
    adopts: bool,
    since: Option<f64>,
}

impl MemoryService {
    /// Moves take `secs` on the app clock (none by default: at once, at the next poll).
    pub fn moves_take(mut self, secs: f64) -> Self {
        self.move_secs = secs;
        self
    }

    /// `folder` holds a file of every torrent's already: a Move into it clashes.
    pub fn holding(mut self, folder: impl Into<PathBuf>) -> Self {
        self.holding.insert(folder.into());
        self
    }

    /// `folder` holds every file of every torrent's already: a Move into it clashes, about them all.
    pub fn holding_all(mut self, folder: impl Into<PathBuf>) -> Self {
        let folder = folder.into();
        self.holding_all.insert(folder.clone());
        self.holding(folder)
    }

    /// Torrent `id`'s files are in `folder`, its destination.
    pub fn kept_in(mut self, id: TorrentId, folder: impl Into<PathBuf>) -> Self {
        self.destinations.insert(id, folder.into());
        let refresh = self.torrents.iter().any(|t| t.id == id);
        if refresh {
            self.refresh(id);
        }
        self
    }

    /// A Move into `folder` fails halfway, for `why`.
    pub fn move_failing(mut self, folder: impl Into<PathBuf>, why: NotMoved) -> Self {
        self.move_failing.insert(folder.into(), why);
        self
    }

    /// Move step `seq` of torrent `id` into `to` starts: decided as the engine decides it, refused
    /// (there already, too) it's over at once; else its files start moving.
    pub(super) fn start_move(
        &mut self,
        seq: u64,
        id: TorrentId,
        to: &AbsolutePath,
        clash: Option<&proto::Answered>,
        new_name: Option<&str>,
    ) -> Option<Option<(TorrentId, Failure)>> {
        let refused = |why| Some(Some((id, Failure::Refused(Refusal::Torrent { id, why }))));
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let name = t.display_name().to_owned();
        // A Remove or a Delete waits behind it: it's cancelled, as one under way.
        let behind =
            self.undo.lanes.get(&id).into_iter().flatten().skip_while(|s| **s != seq).skip(1).filter_map(|s| self.undo.steps.get(s));
        if behind
            .into_iter()
            .any(|s| matches!(&s.what, What::Action(parts) if matches!(parts.get(&id), Some(Part::Doing(r)) if matches!(r.action, UndoAction::Remove | UndoAction::Delete))))
        {
            return refused(TorrentRefusal::NotMoved { name, why: NotMoved::Cancelled });
        }
        let m = self.machines.get(&id)?;
        if let Err(why) = lifecycle::decide(&m.state, &Event::Command(Action::Move)) {
            return refused(why);
        }
        let from = self.destinations.get(&id).cloned().unwrap_or_else(super::downloads);
        if from == **to {
            return refused(TorrentRefusal::AlreadyThere);
        }
        if let Some(location) = self.location(t)
            && to.starts_with(&location)
        {
            return refused(TorrentRefusal::IntoItself);
        }
        if let Some(new_name) = new_name
            && let Err(why) = self.name_free(id, to, new_name)
        {
            return refused(why);
        }
        if self.anothers(id, to, new_name).is_some() {
            let path = AbsolutePath::new(to.join(new_name.unwrap_or(&name))).expect("absolute");
            return refused(TorrentRefusal::AnothersFiles { path });
        }
        // What the folder holds is at the torrent's own name, not a new one.
        let taken = new_name.is_none() && self.holding.contains(&**to);
        let files = self.clash_files(id, to).unwrap_or_default();
        match clash {
            None if taken => return refused(TorrentRefusal::Clash { files }),
            // Answered for what's there no more, as the engine judges it (#270).
            Some(answer) if taken && let Some(changed) = files.iter().find(|f| !answer.found.contains(f)) => {
                return refused(TorrentRefusal::ClashChanged { path: changed.path.clone() });
            }
            None | Some(_) => {}
        }
        let adopts = taken && clash.is_some_and(|a| a.choice == Clash::UseExisting);
        self.moving.insert(id, Moving { seq, to: to.clone(), name: new_name.map(String::from), adopts, since: None });
        // Undo can't put files of it back from the Trash any more: they'd go to the old destination.
        self.decide(lifecycle::undo::Observed::Moving(id));
        self.event(id, Event::Observed(Observation::MoveStarted { to: to.clone() }));
        if let Some(m) = self.machines.get_mut(&id) {
            m.librqbit = Pretend::Stopped;
        }
        self.refresh(id);
        None
    }

    /// A name torrent `id` could take in `to`, as the engine finds one: its own with ` (2)`, ` (3)`…
    /// (before the extension, for a torrent of one file), the first that `name_free` accepts and no
    /// other torrent has a path at.
    pub(super) fn free_name_for(&self, id: TorrentId, to: &AbsolutePath) -> Result<proto::FileName, Failure> {
        let gone = || Failure::Refused(Refusal::Torrent { id, why: TorrentRefusal::NotInList });
        let t = self.torrents.iter().find(|t| t.id == id).ok_or_else(gone)?;
        let name = t.display_name();
        let one_file = self.files.get(&id).is_some_and(|fs| fs.len() == 1);
        let (stem, extension) = match name.rfind('.') {
            Some(dot) if one_file && dot > 0 && dot + 1 < name.len() => name.split_at(dot),
            _ => (name, ""),
        };
        for n in 2..=proto::FileName::MAX_NUMBER {
            let free = proto::FileName::numbered(stem, n, extension).ok_or_else(gone)?;
            match self.name_free(id, to, &free) {
                Ok(()) => {}
                Err(TorrentRefusal::NameTaken { .. } | TorrentRefusal::NameOfAnother { .. }) => continue,
                Err(why) => return Err(Failure::Refused(Refusal::Torrent { id, why })),
            }
            if self.anothers(id, to, Some(&free)).is_none() {
                return Ok(free);
            }
        }
        Err(gone())
    }

    /// The torrent in the list, other than `id`, whose place is where torrent `id` would go in `to`
    /// (under `name`, if it's given one): a Move never replaces nor takes its files (#270).
    pub(super) fn anothers(&self, id: TorrentId, to: &AbsolutePath, name: Option<&str>) -> Option<TorrentId> {
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let at = to.join(name.unwrap_or(t.display_name()));
        let folded = |p: &std::path::Path| p.to_string_lossy().to_lowercase();
        let theirs = |o: &&proto::TorrentInfo| o.id != id && self.location(o).is_some_and(|l| folded(&l) == folded(&at));
        self.torrents.iter().find(theirs).map(|o| o.id)
    }

    /// The torrent in the list that has the place `left`, a torrent that left it, goes back to, or is
    /// moving there: Move's rule for another torrent's place (`anothers`), as the engine's undo takes
    /// it: one of its files is at the path of one of `left`'s, in any case.
    pub(super) fn another_has(&self, left: &super::Left) -> Option<&proto::TorrentInfo> {
        let paths = |destination: &std::path::Path, name: &str, files: &[proto::File]| -> std::collections::BTreeSet<String> {
            let at = |f: &proto::File| match files {
                [_] => destination.join(&f.path),
                _ => destination.join(name).join(&f.path),
            };
            files.iter().map(|f| at(f).to_string_lossy().to_lowercase()).collect()
        };
        let ours = paths(
            &left.destination.clone().unwrap_or_else(super::downloads),
            left.info.display_name(),
            left.files.as_deref().unwrap_or_default(),
        );
        let takes = |o: &&proto::TorrentInfo| {
            let Some(files) = self.files.get(&o.id) else { return false };
            let here = self.destinations.get(&o.id).cloned().unwrap_or_else(super::downloads);
            let there = self.moving.get(&o.id);
            let there = there.map(|m| paths(&m.to, m.name.as_deref().unwrap_or(o.display_name()), files));
            !ours.is_disjoint(&paths(&here, o.display_name(), files)) || there.is_some_and(|t| !ours.is_disjoint(&t))
        };
        self.torrents.iter().filter(|o| o.id != left.info.id).find(takes)
    }

    /// The files of torrent `id` that a Move into `to` would find there already, if it would find
    /// any: as the fake has no disk, each the same as found every time till the folder's
    /// [`Rewritten`](super::DiskChange::Rewritten).
    pub(super) fn clash_files(&self, id: TorrentId, to: &AbsolutePath) -> Option<Vec<proto::Found>> {
        let t = self.torrents.iter().find(|t| t.id == id)?;
        // There already, or inside its own folder: refused for that, as the engine does first.
        let from = self.destinations.get(&id).cloned().unwrap_or_else(super::downloads);
        if !self.holding.contains(&**to) || from == **to || self.location(t).is_some_and(|l| to.starts_with(&l)) {
            return None;
        }
        let files: Vec<PathBuf> = match self.files.get(&id) {
            Some(fs) if self.holding_all.contains(&**to) => fs.iter().map(|f| f.path.clone()).collect(),
            fs => vec![fs.and_then(|fs| fs.first()).map(|f| f.path.clone()).unwrap_or_default()],
        };
        let several = self.files.get(&id).is_some_and(|fs| fs.len() > 1);
        let len = self.rewritten.get(&**to).copied().unwrap_or_default();
        let found = |(n, path)| proto::Found { path, dev: 1, ino: if n == 0 { id } else { id * 1000 + n as u64 }, len, secs: 0, nanos: 0 };
        let paths = files.into_iter().map(|file| if several { PathBuf::from(t.display_name()).join(file) } else { file });
        Some(paths.filter_map(|inside| AbsolutePath::new(to.join(inside)).ok()).enumerate().map(found).collect())
    }

    /// Torrent `id`'s Move that waits its turn never starts (Cancel move, a Remove or a Delete): its
    /// client hears it was cancelled. Whether one waited.
    pub(super) fn dequeue_move(&mut self, id: TorrentId) -> bool {
        let Some(seq) = self.undo.waiting(id) else { return false };
        let event = lifecycle::undo::Event::Command(lifecycle::undo::Command::Dequeue(id));
        let Ok(next) = lifecycle::undo::decide(&self.undo, &event) else { return false };
        self.undo = next;
        self.world.follow(&event, &self.undo);
        self.changed = true;
        let name = self.torrents.iter().find(|t| t.id == id).map(|t| t.display_name().to_owned()).unwrap_or_default();
        let why = TorrentRefusal::NotMoved { name, why: NotMoved::Cancelled };
        self.step_answered(seq, id, Some((id, Failure::Refused(Refusal::Torrent { id, why }))));
        true
    }

    /// A Remove or a Delete came for torrent `id`: a move of it that waits its turn never starts,
    /// and one under way is cancelled first.
    pub(super) fn cancel_move(&mut self, id: TorrentId, action: Action) {
        self.dequeue_move(id);
        if self.machines.get(&id).is_some_and(|m| matches!(m.state.phase, Phase::Moving { .. })) {
            self.event(id, Event::Command(action));
        }
    }

    /// Each move under way goes on at `now`: its copy's progress, then, once it's done, or failed,
    /// cancelled or its destination gone, its end.
    pub(super) fn moves_go_on(&mut self, now: f64) {
        let ids: Vec<TorrentId> = self.moving.keys().copied().collect();
        for id in ids {
            let Some(stage) = self.stage(id) else { continue };
            if let Stage::TakingOut | Stage::Copying { .. } = stage {
                let mover = self.moving.get_mut(&id).expect("just found");
                let since = *mover.since.get_or_insert(now);
                let done = if self.move_secs > 0.0 { ((now - since) / self.move_secs).min(1.0) } else { 1.0 };
                let total = self.files.get(&id).into_iter().flatten().map(|f| f.progress_bytes).sum::<u64>();
                let failing = self.move_failing.get(&*mover.to).cloned();
                let adopts = mover.adopts;
                let event = match (failing, done) {
                    (Some(why), d) if d >= 0.5 => stages::Event::Failed(why),
                    (_, d) if d >= 1.0 => stages::Event::Placed { check: adopts },
                    (_, d) => stages::Event::Copied { done: (total as f64 * d) as u64, total },
                };
                self.event(id, Event::Observed(Observation::Move(event)));
            }
            if let Some(Stage::RemovingOld { .. } | Stage::Restoring { .. }) = self.stage(id) {
                self.move_ended(id);
            }
            self.refresh(id);
        }
    }

    /// Torrent `id`'s move is over: where its files are now, as it was before; its step ends, and
    /// its client hears how it went.
    fn move_ended(&mut self, id: TorrentId) {
        let Some(mover) = self.moving.remove(&id) else { return };
        let Some(stage) = self.stage(id) else { return };
        let name = self.torrents.iter().find(|t| t.id == id).map(|t| t.display_name().to_owned()).unwrap_or_default();
        let failed = match stage {
            Stage::Restoring { why } => Some((id, Failure::Refused(Refusal::Torrent { id, why: TorrentRefusal::NotMoved { name, why } }))),
            Stage::TakingOut | Stage::Copying { .. } | Stage::RemovingOld { .. } => {
                self.destinations.insert(id, mover.to.to_path_buf());
                if let Some(name) = &mover.name {
                    self.take_name(id, name);
                }
                None
            }
        };
        self.event(id, Event::Observed(Observation::Move(stages::Event::Ended)));
        self.refresh(id);
        self.decide(lifecycle::undo::Observed::Finished(mover.seq, id, None));
        self.step_answered(mover.seq, id, failed);
    }

    fn stage(&self, id: TorrentId) -> Option<Stage> {
        match &self.machines.get(&id)?.state.phase {
            Phase::Moving { stage, .. } => Some(stage.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proto::{Content, Status, TorrentCommand, TorrentInfo};

    use super::*;
    use crate::service::Service;

    fn seeding(id: TorrentId) -> TorrentInfo {
        TorrentInfo {
            id,
            info_hash: format!("{id:040x}"),
            name: Some("Album".into()),
            status: Status::Seeding,
            content: Some(Content { size_bytes: 1 << 30, full_size_bytes: 1 << 30, progress_bytes: 1 << 30 }),
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
            original_name: None,
            trackers: Vec::new(),
            location: None,
            actions: proto::Actions::default(),
        }
    }

    fn row(fake: &MemoryService) -> TorrentInfo {
        fake.probe().latest().expect("a snapshot").torrents.into_iter().find(|t| t.id == 0).expect("listed")
    }

    fn to(folder: &str) -> AbsolutePath {
        AbsolutePath::new(folder).unwrap()
    }

    fn send(fake: &mut MemoryService, command: TorrentCommand) {
        Service::command(fake, &[0], command, Box::new(|_, _, _| {}));
    }

    /// As the engine's: Moving with what's copied while its copy takes its time, then where it went,
    /// as it was; a Pause sent meanwhile waits its turn and applies once it's moved.
    #[test]
    fn a_move_shows_its_progress_then_the_torrent_where_it_went_as_it_was() {
        let mut fake = MemoryService::new().with_torrents(vec![seeding(0)]).moves_take(2.0);
        fake.poll(0.0);
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None, name: None });
        fake.poll(0.1);
        send(&mut fake, TorrentCommand::Pause);
        fake.poll(1.1);
        let moving = row(&fake);
        assert!(matches!(moving.status, Status::Moving { done, total } if done > 0 && done < total), "{:?}", moving.status);
        fake.poll(2.5);
        let moved = row(&fake);
        assert_eq!((moved.status, moved.location), (Status::Done { reached: None }, Some(to("/Films/Album"))));
    }

    /// Into a folder holding its file already, refused until the person chooses: it never moves
    /// meanwhile. One that fails halfway is back where it was; one a Remove cancels too.
    #[test]
    fn a_clash_waits_for_a_choice_and_a_move_that_fails_or_is_cancelled_stays_where_it_was() {
        let why = NotMoved::DiskFull;
        let mut fake = MemoryService::new().with_torrents(vec![seeding(0)]).moves_take(2.0).holding("/Films").move_failing("/Full", why);
        fake.poll(0.0);
        let before = row(&fake).location;
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None, name: None });
        fake.poll(1.0);
        assert_eq!((row(&fake).status, row(&fake).location), (Status::Seeding, before.clone()), "refused: a clash");
        send(&mut fake, TorrentCommand::Move { to: to("/Full"), clash: None, name: None });
        fake.poll(1.1);
        fake.poll(3.0);
        assert_eq!((row(&fake).status, row(&fake).location), (Status::Seeding, before.clone()), "failed halfway: back");
        send(
            &mut fake,
            TorrentCommand::Move {
                to: to("/Films"),
                clash: Some(proto::Answered { choice: Clash::Replace, found: Vec::new() }),
                name: None,
            },
        );
        fake.poll(3.1);
        send(&mut fake, TorrentCommand::Remove);
        fake.poll(3.2);
        assert!(fake.probe().latest().unwrap().torrents.is_empty(), "removed, its move cancelled first");
    }

    /// A torrent removed while it moves comes back with its undo where it was, as it was before the
    /// move: not moving.
    #[test]
    fn a_torrent_removed_while_it_moves_comes_back_with_its_undo_as_before_the_move() {
        let mut fake = MemoryService::new().with_torrents(vec![seeding(0)]).moves_take(2.0);
        fake.poll(0.0);
        let before = row(&fake).location;
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None, name: None });
        fake.poll(0.1);
        send(&mut fake, TorrentCommand::Remove);
        fake.poll(0.2);
        Service::undo(&mut fake, Box::new(|_, _, _| {}));
        for at in [0.3, 1.0, 5.0] {
            fake.poll(at);
            let back = row(&fake);
            assert_eq!(
                (back.status, back.location, back.actions.get(Action::Move).clone()),
                (Status::Seeding, before.clone(), proto::Answer::Does)
            );
        }
    }

    /// As the engine's: into the folder it's in, refused (it never moves). With a Move to Trash of
    /// it on the stack, it moves all the same, and that entry leaves the stack, moved: Undo has
    /// nothing left to take. A Remove right after it cancels it.
    #[test]
    fn a_move_is_refused_as_the_engines_is() {
        let mut fake = MemoryService::new().with_torrents(vec![seeding(0)]).moves_take(2.0);
        fake.poll(0.0);
        let before = row(&fake).location;
        let stays = |fake: &MemoryService, why: &str| {
            let r = row(fake);
            assert_eq!((r.status, r.location), (Status::Seeding, before.clone()), "{why}");
        };
        let downloads = AbsolutePath::new(super::super::downloads()).unwrap();
        send(&mut fake, TorrentCommand::Move { to: downloads, clash: None, name: None });
        fake.poll(0.1);
        stays(&fake, "there already");
        send(&mut fake, TorrentCommand::TrashFiles { files: vec![0] });
        fake.poll(0.2);
        fake.poll(1.0);
        let trashed = fake.probe().latest().unwrap().undo.last().map(|u| u.seq).expect("the Move to Trash, on the stack");
        assert_eq!(*row(&fake).actions.get(Action::Move), proto::Answer::Does);
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None, name: None });
        fake.poll(1.1);
        fake.poll(4.0);
        assert_eq!(row(&fake).location, Some(to("/Films/Album")), "moved, files of it in the Trash or not");
        let latest = fake.probe().latest().unwrap();
        assert!(latest.undo.is_empty(), "the Move to Trash left the stack");
        assert_eq!(latest.left.last(), Some(&proto::Left { seq: trashed, why: proto::Leave::Moved }));
        send(&mut fake, TorrentCommand::Move { to: AbsolutePath::new(super::super::downloads()).unwrap(), clash: None, name: None });
        fake.poll(4.1);
        fake.poll(7.0);
        // A Remove right after a Move cancels it, and runs.
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None, name: None });
        send(&mut fake, TorrentCommand::Remove);
        fake.poll(5.1);
        fake.poll(9.0);
        assert!(fake.probe().latest().unwrap().torrents.is_empty(), "removed");
    }
}
