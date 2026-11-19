//! The fake's Move (#24), on the shared lifecycle (`lifecycle::moving`) as the engine's: a Move is a
//! step on its torrent's lane, running until the move is over, which the fake's copy takes
//! [`moves_take`](MemoryService::moves_take) seconds of the app clock to do, its progress shown
//! meanwhile. What's asked of the torrent waits its turn behind it; a Remove or a Delete cancels it
//! first, and so does one waiting behind it on its lane; its destination going restores it. Refused
//! as the engine refuses it: into the folder it's in already, into one inside its own, and while a
//! Move to Trash of it can be undone (the shared rule, `lifecycle::undo::view`). A folder
//! [`holding`](MemoryService::holding) a file of the torrent's already clashes, refused until the
//! person chooses; one a move [`fails into`](MemoryService::move_failing) fails it halfway, and it's
//! back where it was.

use std::path::PathBuf;

use lifecycle::moving::{self as stages, Stage};
use lifecycle::undo::{Part, What, view};
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
        clash: Option<Clash>,
    ) -> Option<Option<(TorrentId, Failure)>> {
        let refused = |why| Some(Some((id, Failure::Refused(Refusal::Torrent { id, why }))));
        let t = self.torrents.iter().find(|t| t.id == id)?;
        let name = t.display_name().to_owned();
        // A Remove or a Delete waits behind it: it's cancelled, as one under way.
        let behind = self.undo.lanes.get(&id).into_iter().flatten().filter(|s| **s > seq).filter_map(|s| self.undo.steps.get(s));
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
        if view::trashed_for_undo(self, &self.undo, id) {
            return refused(TorrentRefusal::TrashedFiles);
        }
        let taken = self.holding.contains(&**to);
        if taken && clash.is_none() {
            let file = self.files.get(&id).and_then(|fs| fs.first()).map(|f| f.path.clone()).unwrap_or_default();
            let inside = if self.files.get(&id).is_some_and(|fs| fs.len() > 1) { PathBuf::from(t.display_name()).join(file) } else { file };
            let files = AbsolutePath::new(to.join(inside)).into_iter().collect();
            return refused(TorrentRefusal::Clash { files });
        }
        self.moving.insert(id, Moving { seq, to: to.clone(), adopts: taken && clash == Some(Clash::UseExisting), since: None });
        self.event(id, Event::Observed(Observation::MoveStarted { to: to.clone() }));
        if let Some(m) = self.machines.get_mut(&id) {
            m.librqbit = Pretend::Stopped;
        }
        self.refresh(id);
        None
    }

    /// A Remove or a Delete came for torrent `id`: a move of it is cancelled first.
    pub(super) fn cancel_move(&mut self, id: TorrentId, action: Action) {
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
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None });
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
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None });
        fake.poll(1.0);
        assert_eq!((row(&fake).status, row(&fake).location), (Status::Seeding, before.clone()), "refused: a clash");
        send(&mut fake, TorrentCommand::Move { to: to("/Full"), clash: None });
        fake.poll(1.1);
        fake.poll(3.0);
        assert_eq!((row(&fake).status, row(&fake).location), (Status::Seeding, before.clone()), "failed halfway: back");
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: Some(Clash::Replace) });
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
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None });
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

    /// As the engine's: into the folder it's in, refused (it never moves); while a Move to Trash
    /// of it can be undone, Move is off with why, and a Move refused; once that's undone, it moves.
    /// A Remove right after it cancels it.
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
        send(&mut fake, TorrentCommand::Move { to: downloads, clash: None });
        fake.poll(0.1);
        stays(&fake, "there already");
        send(&mut fake, TorrentCommand::TrashFiles { files: vec![0] });
        fake.poll(0.2);
        fake.poll(1.0);
        assert_eq!(*row(&fake).actions.get(Action::Move), proto::Answer::Refused { why: TorrentRefusal::TrashedFiles });
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None });
        fake.poll(1.1);
        fake.poll(4.0);
        stays(&fake, "files of it in the Trash for an undo");
        Service::undo(&mut fake, Box::new(|_, _, _| {}));
        fake.poll(4.1);
        fake.poll(5.0);
        assert_eq!(*row(&fake).actions.get(Action::Move), proto::Answer::Does);
        // A Remove right after a Move cancels it, and runs.
        send(&mut fake, TorrentCommand::Move { to: to("/Films"), clash: None });
        send(&mut fake, TorrentCommand::Remove);
        fake.poll(5.1);
        fake.poll(9.0);
        assert!(fake.probe().latest().unwrap().torrents.is_empty(), "removed");
    }
}
