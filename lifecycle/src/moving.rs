//! Moving a torrent's files to a new destination (#24): the stages of [`Phase::Moving`](crate::Phase::Moving),
//! as a machine of their own. librqbit fixes a torrent's folder when it's added, so a move takes it
//! out of librqbit (keeping what it saved of its pieces aside), moves the files, and adds it again
//! there with those pieces, without a full check.
//!
//! ```text
//! TakingOut --Copied--> Copying --Placed--> RemovingOld --Ended--> (out of Moving, at the new destination)
//! TakingOut, Copying --Failed, or Cancel--> Restoring --Ended--> (out of Moving, where it was)
//! RemovingOld --Blocked--> Restoring
//! ```
//!
//! The files go first into a folder of the move's own in the new destination (staged), renamed on
//! the same drive, copied in chunks to another. Once all are there the record names the new
//! destination (written ahead), and only then do they take their places and what's left in the old
//! one goes (`RemovingOld`): from that point a crash finishes the move. Before it, a failure, a
//! Remove or a Delete, or a crash puts back what was renamed and removes what was copied
//! (`Restoring`), so the files are always in exactly one destination. Each file is checked again
//! just before it takes its place: one blocked there (something put at its path meanwhile, a link
//! on the way) sends back those that had, the record names the old destination again, and it's
//! restored (`Blocked`), always: a file deleted by hand meanwhile stays deleted.
//!
//! [`next`] says what the actor runs: the copy (once librqbit let go), its stop, the end of the move
//! or its undoing, each a task of its own that reports back.

use machine::Machine;
use proto::NotMoved;

/// Where a move is.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// Kept aside, and taken out of librqbit, before its files move.
    TakingOut,
    /// Its files go into the move's own folder in the new destination: `done` of `total` bytes
    /// copied (none for a rename).
    Copying { done: u64, total: u64 },
    /// All of them are there and the record names the new destination: they take their places, and
    /// what's left of them in the old one goes. `check`: it took files that were there already
    /// (Use existing), so it's checked once it's moved.
    RemovingOld { check: bool },
    /// It didn't move, for `why`: what was renamed goes back, what was copied goes.
    Restoring { why: NotMoved },
}

/// How the move's tasks went.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The copy copied `done` of `total` bytes so far.
    Copied { done: u64, total: u64 },
    /// Every file is in the new destination, and the record says so.
    Placed { check: bool },
    /// It can't go on, for this reason.
    Failed(NotMoved),
    /// A file couldn't take its place, for this reason: those that had are staged again, and the
    /// record names the old destination again.
    Blocked(NotMoved),
    /// Moved and the old removed, or what was copied removed.
    Ended,
}

/// What the actor runs of it now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// The copy, and whether it was told to stop.
    Copy { stopping: bool },
    /// Putting the files in place and removing the old.
    Finish,
    /// Putting back what was renamed, removing what was copied.
    Restore,
}

/// What [`next`] looks at: the task under way, if any, and whether librqbit let go of the torrent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct World {
    pub task: Option<Task>,
    pub out: bool,
}

/// What the actor carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Stage the files in the new destination, reporting what's copied.
    Copy,
    /// Stop the copy after the chunk in flight.
    Stop,
    /// Put the files in place, then remove the old ones.
    Finish,
    /// Put back what was renamed, remove what was copied.
    Restore,
}

/// Where the move is once `event` happened, or `None` once it's over. Never refused: each is a
/// fact. A stage's news that comes late is stale: it changes nothing.
pub fn decide(stage: &Stage, event: &Event) -> Option<Stage> {
    use Event as E;
    use Stage as S;
    let stays = || Some(stage.clone());
    match (stage, event) {
        (S::TakingOut | S::Copying { .. }, E::Copied { done, total }) => Some(S::Copying { done: *done, total: *total }),
        (S::TakingOut | S::Copying { .. }, E::Placed { check }) => Some(S::RemovingOld { check: *check }),
        (S::TakingOut | S::Copying { .. }, E::Failed(why)) => Some(S::Restoring { why: why.clone() }),
        // Not moved yet, nor stopped: nothing ends but by being placed or failing.
        (S::TakingOut | S::Copying { .. }, E::Ended | E::Blocked(_)) => stays(),
        (S::RemovingOld { .. }, E::Blocked(why)) => Some(S::Restoring { why: why.clone() }),
        // Past the point of no return, or going back already: only the end ends it.
        (S::RemovingOld { .. }, E::Copied { .. } | E::Placed { .. } | E::Failed(_)) => stays(),
        (S::Restoring { .. }, E::Copied { .. } | E::Placed { .. } | E::Failed(_) | E::Blocked(_)) => stays(),
        (S::RemovingOld { .. } | S::Restoring { .. }, E::Ended) => None,
    }
}

/// What makes the move go on, in `world`.
pub fn next(stage: &Stage, world: World) -> Option<Do> {
    use Stage as S;
    match (stage, world.task) {
        // Its files move once librqbit has let go of them.
        (S::TakingOut | S::Copying { .. }, None) => world.out.then_some(Do::Copy),
        (S::TakingOut | S::Copying { .. }, Some(Task::Copy { .. } | Task::Finish | Task::Restore)) => None,
        (S::RemovingOld { .. }, None) => Some(Do::Finish),
        (S::RemovingOld { .. }, Some(Task::Copy { .. } | Task::Finish | Task::Restore)) => None,
        // A copy under way stops after its chunk in flight, then what it did is undone.
        (S::Restoring { .. }, Some(Task::Copy { stopping: false })) => Some(Do::Stop),
        (S::Restoring { .. }, None) => Some(Do::Restore),
        (S::Restoring { .. }, Some(Task::Copy { stopping: true } | Task::Finish | Task::Restore)) => None,
    }
}

/// The move as a [`Machine`]: its state is the stage, `None` once it's over.
pub struct Move;

impl Machine for Move {
    type State = Option<Stage>;
    type Event = Event;
    type Refusal = std::convert::Infallible;
    type World = World;
    type Do = Do;

    fn decide(state: &Option<Stage>, event: &Event) -> Result<Option<Stage>, Self::Refusal> {
        Ok(state.as_ref().and_then(|s| decide(s, event)))
    }

    fn next(state: &Option<Stage>, world: &World) -> Option<Do> {
        state.as_ref().and_then(|s| next(s, *world))
    }
}

#[cfg(test)]
mod tests {
    use machine::{Row, every, table};

    use super::*;

    every!(Stage {
        Stage::TakingOut => [Stage::TakingOut],
        Stage::Copying { .. } => [Stage::Copying { done: 0, total: 0 }, Stage::Copying { done: 3, total: 10 }],
        Stage::RemovingOld { .. } => [Stage::RemovingOld { check: false }, Stage::RemovingOld { check: true }],
        Stage::Restoring { .. } => [Stage::Restoring { why: NotMoved::DiskFull }, Stage::Restoring { why: NotMoved::Cancelled }],
    });

    every!(Event {
        Event::Copied { .. } => [Event::Copied { done: 5, total: 10 }],
        Event::Placed { .. } => [Event::Placed { check: false }, Event::Placed { check: true }],
        Event::Failed(_) => [Event::Failed(NotMoved::Interrupted)],
        Event::Blocked(_) => [Event::Blocked(NotMoved::Cancelled)],
        Event::Ended => [Event::Ended],
    });

    every!(Task {
        Task::Copy { .. } => [Task::Copy { stopping: false }, Task::Copy { stopping: true }],
        Task::Finish => [Task::Finish],
        Task::Restore => [Task::Restore],
    });

    every!(World {
        World { .. } => [None].into_iter().chain(Task::every().into_iter().map(Some))
            .flat_map(|task| [true, false].map(|out| World { task, out })),
    });

    fn copying(s: &Stage) -> bool {
        matches!(s, Stage::TakingOut | Stage::Copying { .. })
    }

    #[test]
    fn every_stage_and_event_is_decided_as_its_row_says() {
        let rows: [Row<Stage, Event, Option<Stage>>; 5] = [
            Row {
                says: "before its files are placed, what's copied is counted, all placed removes the old, a failure restores",
                when: |s, e| copying(s) && matches!(e, Event::Copied { .. } | Event::Placed { .. } | Event::Failed(_)),
                then: |_, e, n| {
                    *n == Some(match e {
                        Event::Copied { done, total } => Stage::Copying { done: *done, total: *total },
                        Event::Placed { check } => Stage::RemovingOld { check: *check },
                        Event::Failed(why) => Stage::Restoring { why: why.clone() },
                        Event::Ended | Event::Blocked(_) => unreachable!(),
                    })
                },
            },
            Row {
                says: "before its files are placed, nothing ends it but being placed or failing",
                when: |s, e| copying(s) && matches!(e, Event::Ended | Event::Blocked(_)),
                then: |s, _, n| n.as_ref() == Some(s),
            },
            Row {
                says: "removing the old, a file blocked from its place restores it",
                when: |s, e| matches!(s, Stage::RemovingOld { .. }) && matches!(e, Event::Blocked(_)),
                then: |_, e, n| matches!((e, n), (Event::Blocked(why), Some(Stage::Restoring { why: w })) if w == why),
            },
            Row {
                says: "removing the old or restoring, news of the copy is stale, and so is the other way's",
                when: |s, e| !copying(s) && !matches!(e, Event::Ended) && !matches!((s, e), (Stage::RemovingOld { .. }, Event::Blocked(_))),
                then: |s, _, n| n.as_ref() == Some(s),
            },
            Row {
                says: "removing the old or restoring, its end ends the move",
                when: |s, e| !copying(s) && matches!(e, Event::Ended),
                then: |_, _, n| n.is_none(),
            },
        ];
        table(decide, &rows, &[]);
    }

    #[test]
    fn the_move_goes_on_as_its_row_says() {
        let rows: [Row<Stage, World, Option<Do>>; 3] = [
            Row {
                says: "before its files are placed, they're copied once librqbit let go and nothing runs of it",
                when: |s, _| copying(s),
                then: |_, w, d| *d == (w.task.is_none() && w.out).then_some(Do::Copy),
            },
            Row {
                says: "placed, they're finished once nothing runs of it, librqbit having it or not",
                when: |s, _| matches!(s, Stage::RemovingOld { .. }),
                then: |_, w, d| *d == w.task.is_none().then_some(Do::Finish),
            },
            Row {
                says: "restoring, a copy under way is stopped, then what it did is undone",
                when: |s, _| matches!(s, Stage::Restoring { .. }),
                then: |_, w, d| {
                    *d == match w.task {
                        None => Some(Do::Restore),
                        Some(Task::Copy { stopping: false }) => Some(Do::Stop),
                        Some(Task::Copy { stopping: true } | Task::Finish | Task::Restore) => None,
                    }
                },
            },
        ];
        table(|s, w| next(s, *w), &rows, &[]);
    }
}
