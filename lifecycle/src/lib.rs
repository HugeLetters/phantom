//! A torrent's lifecycle as a state machine ([ADR 0006]): its [`State`] is what the user wants of it
//! ([`Wants`]) and what it's doing ([`Phase`]); its Status follows from the two and whether it's
//! complete. [`decide`] says what an [`Event`] does, one written decision per state and event: a
//! client's command sets what's wanted (and may start a check), or is refused with a
//! [`Refusal`]; an [`Observation`] of librqbit or of the disk changes only the phase, and is never
//! refused. The actor reports raw facts (what librqbit is doing, what's on disk) and the rules of
//! what they mean live here. [`next`] says what makes librqbit match: the [`Do`] the actor carries
//! out. Pure: an actor carries both out (the engine's in `service/src/engine/actor.rs`).
//!
//! The reference machine: every machine names these the same, and its `decide` matches every
//! `(state, event)` pair with no `_` arm, so a new state or event fails to compile until decided.
//!
//! Shared ([#95]): the engine runs it against librqbit and the disk; the GUI's fake service runs
//! it against a pretend librqbit and disk, so the GUI's tests meet the engine's own rules.
//! [`disk`] is what a size check finds, as plain data; [`machine`] is what every machine is, and
//! the harness that tests one.
//!
//! [ADR 0006]: ../../docs/adr/0006-torrent-lifecycle-as-an-actor.md
//! [#95]: https://github.com/HugeLetters/phantom/issues/95

pub mod disk;
pub mod machine;
#[cfg(feature = "harness")]
pub mod samples;

use std::collections::BTreeMap;

use proto::{Action, Kept, Status, Unavailable};
use serde::{Deserialize, Serialize};

use disk::{Disk, Gone, Seen};
use machine::Machine;

/// Run or stop: set by Pause and Resume (and the add), and by nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wants {
    Run,
    #[default]
    Stop,
}

/// What a torrent is doing, whatever is wanted of it.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// librqbit doesn't have it yet.
    FetchingInfo,
    /// Fetching its info failed. It stays in the list until it's removed.
    FetchFailed { message: String },
    /// To be checked: librqbit is added again, which checks it (librqbit can't check a torrent it
    /// has). `returning`: missing files put back, which the check tells are what they were or not.
    CheckDue { returning: BTreeMap<usize, Gone> },
    /// librqbit is checking its files, with those `returning`.
    Checking { returning: BTreeMap<usize, Gone> },
    /// In librqbit, running or stopped as wanted.
    Ready,
    /// Files it's getting (by index) are gone from disk; stopped in librqbit, which still holds the
    /// old files open.
    MissingFiles { gone: BTreeMap<usize, Gone> },
    /// Its destination can't be reached; stopped in librqbit.
    DestinationUnavailable { reason: Unavailable },
    /// librqbit stopped it with an error, or adding it again failed.
    Error { message: String },
    /// Delete moved some of its files to the Trash but not these: stopped in librqbit, still in
    /// the list, until Delete is tried again (or it's resumed, checked or removed).
    DeleteFailed { kept: Vec<Kept> },
    /// Delete is moving its files to the Trash, from `was`: stopped in librqbit, and every
    /// command is refused until the trash says how it went.
    Deleting { was: Box<Phase> },
    /// All its files are in the Trash: the actor takes it off the list.
    Deleted,
}

/// A torrent's lifecycle: what's wanted of it, and its phase.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub wants: Wants,
    pub phase: Phase,
}

/// Something that happens to a torrent, by where it comes from: a client's command (its arguments,
/// which files and which priority, don't change the decision, so the actor keeps them) or what the
/// actor sees.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Command(Action),
    Observed(Observation),
}

/// What the actor sees of a torrent: facts, not what they mean for it.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// librqbit has the torrent and its info; `checking`: it's checking the files.
    InfoArrived {
        checking: bool,
    },
    FetchFailed(String),
    /// librqbit is checking the files.
    Checking,
    /// librqbit is running it or has it stopped, with `have` bytes of each file, by index.
    Idle {
        have: Vec<u64>,
    },
    /// librqbit stopped it with an error.
    Failed(String),
    /// The actor added it to librqbit again, which checks it.
    AddedAgain,
    /// A size check: the destination can be reached, and these are its files on disk.
    Files(Disk),
    /// A size check: the destination can't be reached.
    DestinationGone {
        reason: Unavailable,
    },
    /// How Delete's trashing went: `kept`, what couldn't go, and whether anything `moved`.
    Trashed {
        kept: Vec<Kept>,
        moved: bool,
    },
}

/// Why a command for a torrent is refused: data for the client, which words it.
pub use proto::TorrentRefusal as Refusal;

/// What librqbit is doing with a torrent that has info.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lib {
    Checking,
    Running,
    Stopped,
    Failed,
    /// librqbit doesn't have it: adding it again failed.
    Absent,
}

/// What makes librqbit match: what the actor carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Start,
    Stop,
    /// Take it out of librqbit and add it again, stopped, so it opens the files afresh and checks
    /// them: librqbit can't check a torrent it has.
    AddAgain,
}

impl State {
    pub fn new(wants: Wants, phase: Phase) -> Self {
        Self { wants, phase }
    }

    /// What clients see. `complete`: it has every file it's getting.
    pub fn status(&self, complete: bool) -> Status {
        match (&self.phase, self.wants) {
            (Phase::FetchingInfo, _) => Status::FetchingInfo,
            (Phase::CheckDue { .. } | Phase::Checking { .. }, _) => Status::Checking,
            (Phase::Ready, Wants::Run) if complete => Status::Seeding,
            (Phase::Ready, Wants::Run) => Status::Downloading,
            (Phase::Ready, Wants::Stop) if complete => Status::Done,
            (Phase::Ready, Wants::Stop) => Status::Paused,
            (Phase::MissingFiles { .. }, _) => Status::MissingFiles,
            (Phase::DestinationUnavailable { reason }, _) => Status::DestinationUnavailable { reason: reason.clone() },
            (Phase::FetchFailed { message } | Phase::Error { message }, _) => Status::Error { message: message.clone() },
            (Phase::DeleteFailed { kept }, _) => Status::Error { message: not_trashed(kept) },
            (Phase::Deleting { .. } | Phase::Deleted, _) => Status::Deleting,
        }
    }

    /// The commands that do something now: those not refused, less Pause, Resume and Check where
    /// they'd change nothing.
    pub fn actions(&self) -> Vec<Action> {
        use Action::*;
        let commands =
            [Pause, Resume, Check, SetPriority, Remove, Delete].into_iter().filter(|action| match decide(self, &Event::Command(*action)) {
                Ok(next) => next != *self || matches!(action, SetPriority | Remove | Delete),
                Err(_) => false,
            });
        // Not commands: a client opens or shows the files itself, once they're there to see.
        let on_disk = matches!(
            self.phase,
            Phase::CheckDue { .. } | Phase::Checking { .. } | Phase::Ready | Phase::Error { .. } | Phase::DeleteFailed { .. }
        );
        commands.chain(on_disk.then_some([Open, Reveal]).into_iter().flatten()).collect()
    }
}

/// What `event` does to torrent `s`: what it becomes, or why a command is refused. One arm per
/// phase and event, by phase.
pub fn decide(s: &State, event: &Event) -> Result<State, Refusal> {
    use Action as C;
    use Event::{Command, Observed};
    use Observation as O;
    use Phase as P;
    let stays = || Ok(s.clone());
    let wants = |wants| Ok(State { wants, phase: s.phase.clone() });
    let phase = |phase| Ok(State { wants: s.wants, phase });
    let to = |wants, phase| Ok(State { wants, phase });
    let unavailable = |action, reason: &Unavailable| Err(Refusal::DestinationUnavailable { action, reason: reason.clone() });
    match (&s.phase, event) {
        // Done by the client, never sent: no phase decides it.
        (_, Command(C::Open | C::Reveal)) => stays(),
        // librqbit doesn't have it: only Remove and Delete apply, and stop fetching. What's wanted
        // is remembered: once its info arrives it runs or stays stopped as wanted.
        (P::FetchingInfo, Command(C::Pause)) => wants(Wants::Stop),
        (P::FetchingInfo, Command(C::Resume)) => wants(Wants::Run),
        (P::FetchingInfo, Command(C::Check | C::SetPriority)) => Err(Refusal::NoInfoYet),
        (P::FetchingInfo, Command(C::Remove | C::Delete)) => stays(),
        (P::FetchingInfo, Observed(O::InfoArrived { checking: true })) => phase(checking()),
        (P::FetchingInfo, Observed(O::InfoArrived { checking: false })) => phase(P::Ready),
        (P::FetchingInfo, Observed(O::FetchFailed(message))) => phase(P::FetchFailed { message: message.clone() }),
        // Neither librqbit nor the disk has anything of it yet.
        (
            P::FetchingInfo,
            Observed(
                O::Checking | O::Idle { .. } | O::Failed(_) | O::AddedAgain | O::Files(_) | O::DestinationGone { .. } | O::Trashed { .. },
            ),
        ) => stays(),

        (P::FetchFailed { .. }, Command(C::Pause | C::Resume | C::Check | C::SetPriority)) => Err(Refusal::FetchFailed),
        (P::FetchFailed { .. }, Command(C::Remove | C::Delete)) => stays(),
        (
            P::FetchFailed { .. },
            Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. },
            ),
        ) => stays(),

        // Delete trashes its files, wherever it was: it's stopped meanwhile (`next`), and the actor
        // reports how it went. Not where the destination can't be reached (below).
        (
            P::CheckDue { .. } | P::Checking { .. } | P::Ready | P::MissingFiles { .. } | P::Error { .. } | P::DeleteFailed { .. },
            Command(C::Delete),
        ) => phase(P::Deleting { was: Box::new(s.phase.clone()) }),
        // What's done to it is the trash's until it's done.
        (P::Deleting { .. }, Command(C::Pause | C::Resume | C::Check | C::SetPriority | C::Remove | C::Delete)) => Err(Refusal::Deleting),
        // All of it went; nothing went, so it's as it was; or it waits, stopped, with what's left.
        (P::Deleting { was }, Observed(O::Trashed { kept, moved })) => phase(match (kept.is_empty(), moved) {
            (true, _) => P::Deleted,
            (false, false) => (**was).clone(),
            (false, true) => P::DeleteFailed { kept: kept.clone() },
        }),
        // Its files are moving on purpose, and librqbit is stopped: what's seen meanwhile is stale.
        (
            P::Deleting { .. },
            Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. },
            ),
        ) => stays(),
        // Off the list as soon as it's decided: nothing reaches it.
        (P::Deleted, Command(C::Pause | C::Resume | C::Check | C::SetPriority | C::Remove | C::Delete)) => Err(Refusal::NotInList),
        (
            P::Deleted,
            Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. },
            ),
        ) => stays(),
        // A stale outcome, for a Delete that's over: nothing.
        (
            P::CheckDue { .. }
            | P::Checking { .. }
            | P::Ready
            | P::MissingFiles { .. }
            | P::DestinationUnavailable { .. }
            | P::Error { .. }
            | P::DeleteFailed { .. },
            Observed(O::Trashed { .. }),
        ) => stays(),

        (P::DeleteFailed { .. }, Command(C::Pause)) => wants(Wants::Stop),
        // As from an error: a check brings it back into librqbit, and finds what's gone.
        (P::DeleteFailed { .. }, Command(C::Resume)) => to(Wants::Run, check_due()),
        (P::DeleteFailed { .. }, Command(C::Check)) => phase(check_due()),
        // The actor does these.
        (P::DeleteFailed { .. }, Command(C::SetPriority | C::Remove)) => stays(),
        // Its files are partly in the Trash on purpose: nothing librqbit or the disk says changes it.
        (
            P::DeleteFailed { .. },
            Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Checking | O::Idle { .. } | O::Failed(_) | O::AddedAgain | O::Files(_)),
        ) => stays(),
        (P::DeleteFailed { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        // A check due waits for librqbit to have the torrent again; nothing it does before matters.
        (P::CheckDue { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::CheckDue { .. }, Command(C::Resume)) => wants(Wants::Run),
        (P::CheckDue { .. }, Command(C::Check | C::SetPriority | C::Remove)) => stays(),
        (P::CheckDue { .. }, Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Idle { .. } | O::Files(_))) => stays(),
        // Adding it again failed.
        (P::CheckDue { .. }, Observed(O::Failed(message))) => phase(P::Error { message: message.clone() }),
        // librqbit checks it already, or now that it has it again.
        (P::CheckDue { returning }, Observed(O::Checking | O::AddedAgain)) => phase(P::Checking { returning: returning.clone() }),
        (P::CheckDue { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        // Nothing interrupts a check; a priority change during one is held until it ends.
        (P::Checking { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::Checking { .. }, Command(C::Resume)) => wants(Wants::Run),
        (P::Checking { .. }, Command(C::Check | C::SetPriority | C::Remove)) => stays(),
        (P::Checking { .. }, Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Checking | O::AddedAgain)) => stays(),
        // A check ends: files put back that aren't what they were are missing again.
        (P::Checking { returning }, Observed(O::Idle { have })) => phase(check_ended(returning, have)),
        (P::Checking { .. }, Observed(O::Failed(message))) => phase(P::Error { message: message.clone() }),
        // A check finds missing files for itself; they're caught once it ends.
        (P::Checking { .. }, Observed(O::Files(_))) => stays(),
        (P::Checking { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        (P::Ready, Command(C::Pause)) => wants(Wants::Stop),
        (P::Ready, Command(C::Resume)) => wants(Wants::Run),
        (P::Ready, Command(C::Check)) => phase(check_due()),
        // The actor does these.
        (P::Ready, Command(C::SetPriority | C::Remove)) => stays(),
        (P::Ready, Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Idle { .. } | O::AddedAgain)) => stays(),
        // librqbit checks again by itself.
        (P::Ready, Observed(O::Checking)) => phase(checking()),
        (P::Ready, Observed(O::Failed(message))) => phase(P::Error { message: message.clone() }),
        (P::Ready, Observed(O::Files(disk))) => phase(found_missing(disk).map_or(P::Ready, |gone| P::MissingFiles { gone })),
        (P::Ready, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        (P::MissingFiles { .. }, Command(C::Pause)) => wants(Wants::Stop),
        // A check is the way back into librqbit: it opens the files afresh, keeps what's valid,
        // and then it downloads the rest.
        (P::MissingFiles { .. }, Command(C::Resume)) => to(Wants::Run, check_due()),
        (P::MissingFiles { .. }, Command(C::Check)) => phase(check_due()),
        // Files set Off are no longer missing: the next size check doesn't list them.
        (P::MissingFiles { .. }, Command(C::SetPriority | C::Remove)) => stays(),
        // Stopped in librqbit, which doesn't know the files went.
        (
            P::MissingFiles { .. },
            Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Checking | O::Idle { .. } | O::Failed(_) | O::AddedAgain),
        ) => stays(),
        (P::MissingFiles { gone }, Observed(O::Files(disk))) => phase(missing_files(gone, disk)),
        // What was missing is out of reach too: it's checked once the destination is back.
        (P::MissingFiles { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        // Its files are out of reach; once they're back it runs again if it's wanted to.
        (P::DestinationUnavailable { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::DestinationUnavailable { reason }, Command(action @ (C::Resume | C::Check | C::Delete))) => unavailable(*action, reason),
        (P::DestinationUnavailable { .. }, Command(C::SetPriority | C::Remove)) => stays(),
        // Stopped in librqbit, which can't reach the files either.
        (
            P::DestinationUnavailable { .. },
            Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Checking | O::Idle { .. } | O::Failed(_) | O::AddedAgain),
        ) => stays(),
        // Its files can be size-checked again: the destination is back.
        (P::DestinationUnavailable { .. }, Observed(O::Files(_))) => phase(check_due()),
        (P::DestinationUnavailable { .. }, Observed(O::DestinationGone { reason })) => {
            phase(P::DestinationUnavailable { reason: reason.clone() })
        }

        (P::Error { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::Error { .. }, Command(C::Resume)) => to(Wants::Run, check_due()),
        (P::Error { .. }, Command(C::Check)) => phase(check_due()),
        (P::Error { .. }, Command(C::SetPriority | C::Remove)) => stays(),
        (P::Error { .. }, Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::AddedAgain)) => stays(),
        // librqbit is past the error.
        (P::Error { .. }, Observed(O::Checking)) => phase(checking()),
        (P::Error { .. }, Observed(O::Idle { .. })) => phase(P::Ready),
        (P::Error { .. }, Observed(O::Failed(message))) => phase(P::Error { message: message.clone() }),
        (P::Error { .. }, Observed(O::Files(disk))) => match found_missing(disk) {
            Some(gone) => phase(P::MissingFiles { gone }),
            None => stays(),
        },
        (P::Error { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),
    }
}

fn check_due() -> Phase {
    Phase::CheckDue { returning: BTreeMap::new() }
}

fn checking() -> Phase {
    Phase::Checking { returning: BTreeMap::new() }
}

/// The files missing on `disk`, if any.
fn found_missing(disk: &Disk) -> Option<BTreeMap<usize, Gone>> {
    let gone: BTreeMap<usize, Gone> = disk.files.iter().filter(|(_, f)| f.is_missing()).map(|(&i, f)| (i, Gone::found(f))).collect();
    (!gone.is_empty()).then_some(gone)
}

/// A torrent missing `gone` size-checked: those it's no longer getting (set Off) drop out, those
/// not judged yet are judged as they are now, and others found missing join them. None left:
/// Ready; every one back: checked, which tells whether they're what they were.
fn missing_files(gone: &BTreeMap<usize, Gone>, disk: &Disk) -> Phase {
    let mut now: BTreeMap<usize, Gone> = gone
        .iter()
        .filter_map(|(i, g)| {
            let found = disk.files.get(i)?;
            let seen = if g.seen == Seen::NotYet { Seen::Was(found.at) } else { g.seen };
            Some((*i, Gone { seen, ..g.clone() }))
        })
        .collect();
    for (i, f) in disk.files.iter().filter(|(i, f)| !gone.contains_key(i) && f.is_missing()) {
        now.insert(*i, Gone::found(f));
    }
    if now.is_empty() {
        Phase::Ready
    } else if now.iter().all(|(i, g)| g.is_back(disk.files[i].at)) {
        Phase::CheckDue { returning: now }
    } else {
        Phase::MissingFiles { gone: now }
    }
}

/// A check of the files put back, `returning`, ended with librqbit having `have` bytes of each:
/// those holding less than they did before they went aren't the files that went, and what's at
/// their paths is judged at the next size check.
fn check_ended(returning: &BTreeMap<usize, Gone>, have: &[u64]) -> Phase {
    let not_back: BTreeMap<usize, Gone> = returning
        .iter()
        .filter(|(i, g)| have.get(**i).copied().unwrap_or(0) < g.progress_bytes)
        .map(|(i, g)| (*i, Gone { seen: Seen::NotYet, ..g.clone() }))
        .collect();
    if not_back.is_empty() { Phase::Ready } else { Phase::MissingFiles { gone: not_back } }
}

/// What a torrent whose Delete left `kept` says: the paths still there. A sentence until #100
/// makes failures data.
fn not_trashed(kept: &[Kept]) -> String {
    let paths: Vec<String> = kept.iter().map(|k| k.path.display().to_string()).collect();
    format!("Couldn't move everything to the Trash: {} still there", paths.join(", "))
}

/// What makes librqbit, doing `lib`, match `s`. A check due is entered by adding the torrent
/// again, unless librqbit is checking it already. Nothing interrupts a check: one that ended is
/// stopped, until the actor sees so.
pub fn next(s: &State, lib: Lib) -> Option<Do> {
    use Lib as L;
    use Phase as P;
    use Wants::{Run, Stop};
    match (&s.phase, s.wants, lib) {
        (P::CheckDue { .. }, Run | Stop, L::Checking) => None,
        (P::CheckDue { .. }, Run | Stop, L::Running | L::Stopped | L::Failed | L::Absent) => Some(Do::AddAgain),
        (P::Checking { .. }, Run | Stop, L::Checking | L::Running | L::Stopped | L::Failed | L::Absent) => None,
        (P::Ready, Run, L::Stopped) => Some(Do::Start),
        (P::Ready, Stop, L::Running) => Some(Do::Stop),
        (P::Ready, Run, L::Checking | L::Running | L::Failed | L::Absent) => None,
        (P::Ready, Stop, L::Checking | L::Stopped | L::Failed | L::Absent) => None,
        // Nothing more is written into deleted files, nor where the destination was, nor into files
        // on their way to the Trash.
        (
            P::MissingFiles { .. } | P::DestinationUnavailable { .. } | P::DeleteFailed { .. } | P::Deleting { .. },
            Run | Stop,
            L::Running,
        ) => Some(Do::Stop),
        (
            P::MissingFiles { .. } | P::DestinationUnavailable { .. } | P::DeleteFailed { .. } | P::Deleting { .. },
            Run | Stop,
            L::Checking | L::Stopped | L::Failed | L::Absent,
        ) => None,
        // librqbit doesn't have it, or it's an error until Resume or Check, or it's leaving the list.
        (
            P::FetchingInfo | P::FetchFailed { .. } | P::Error { .. } | P::Deleted,
            Run | Stop,
            L::Checking | L::Running | L::Stopped | L::Failed | L::Absent,
        ) => None,
    }
}

/// The torrent's lifecycle as a [`Machine`]: [`decide`] and [`next`].
pub struct Torrent;

impl Machine for Torrent {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = Lib;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, lib: &Lib) -> Option<Do> {
        next(state, *lib)
    }
}

#[cfg(test)]
mod tests {

    use proptest::prelude::*;

    use super::disk::Stamp;
    use super::machine::{Every, check, decide_table, sequences, table};
    use super::samples::{OTHER, WAS, disk, drive, gone_as, stamp};
    use super::*;

    fn has_info(phase: &Phase) -> bool {
        !matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. })
    }

    fn deleting(phase: &Phase) -> bool {
        matches!(phase, Phase::Deleting { .. } | Phase::Deleted)
    }

    /// With info, and not on its way out by Delete.
    fn active(phase: &Phase) -> bool {
        has_info(phase) && !deleting(phase)
    }

    // The table: every state and event is in exactly one row, whose property what `decide` says of
    // it must have. A new phase or event needs a row before this passes.

    type Decided = Result<State, Refusal>;
    type Row = machine::DecideRow<Torrent>;

    fn command(event: &Event) -> Option<Action> {
        match event {
            Event::Command(c) => Some(*c),
            Event::Observed(_) => None,
        }
    }

    /// Done by the client with what it's told of the torrent; the machine only lists them.
    fn client_side(c: Action) -> bool {
        matches!(c, Action::Open | Action::Reveal)
    }

    fn observed(event: &Event) -> Option<&Observation> {
        match event {
            Event::Command(_) => None,
            Event::Observed(o) => Some(o),
        }
    }

    fn is(c: Action) -> impl Fn(&State, &Event) -> bool {
        move |s, e| active(&s.phase) && command(e) == Some(c)
    }

    fn stays(s: &State, r: &Decided) -> bool {
        r.as_ref() == Ok(s)
    }

    fn becomes(s: &State, r: &Decided, phase: Phase) -> bool {
        r.as_ref() == Ok(&State::new(s.wants, phase))
    }

    fn unavailable(s: &State, e: &Event, r: &Decided) -> bool {
        let Phase::DestinationUnavailable { reason } = &s.phase else { return false };
        *r == Err(Refusal::DestinationUnavailable { action: command(e).unwrap(), reason: reason.clone() })
    }

    /// What a size check of `disk` makes of a torrent missing `gone`, row by row of the rule.
    fn after_size_check(gone: &BTreeMap<usize, Gone>, disk: &Disk) -> Phase {
        let judged = |i: &usize, g: &Gone| match g.seen {
            Seen::NotYet => Gone { seen: Seen::Was(disk.files[i].at), ..g.clone() },
            Seen::Was(_) => g.clone(),
        };
        let still: BTreeMap<usize, Gone> =
            gone.iter().filter(|(i, _)| disk.files.contains_key(i)).map(|(i, g)| (*i, judged(i, g))).collect();
        let more: BTreeMap<usize, Gone> =
            disk.files.iter().filter(|(i, f)| !gone.contains_key(i) && f.is_missing()).map(|(i, f)| (*i, Gone::found(f))).collect();
        let back = more.is_empty() && still.iter().all(|(i, g)| g.is_back(disk.files[i].at));
        let now: BTreeMap<usize, Gone> = still.into_iter().chain(more).collect();
        match (now.is_empty(), back) {
            (true, _) => Phase::Ready,
            (false, true) => Phase::CheckDue { returning: now },
            (false, false) => Phase::MissingFiles { gone: now },
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                says: "without info, Pause and Resume are remembered for when it arrives, Remove and Delete apply, the rest are refused",
                when: |s, e| !has_info(&s.phase) && command(e).is_some_and(|c| !client_side(c)),
                then: |s, e, r| match (&s.phase, command(e).unwrap()) {
                    (Phase::FetchingInfo, Action::Pause) => *r == Ok(State::new(Wants::Stop, Phase::FetchingInfo)),
                    (Phase::FetchingInfo, Action::Resume) => *r == Ok(State::new(Wants::Run, Phase::FetchingInfo)),
                    (_, Action::Remove | Action::Delete) => stays(s, r),
                    (Phase::FetchingInfo, _) => *r == Err(Refusal::NoInfoYet),
                    _ => *r == Err(Refusal::FetchFailed),
                },
            },
            Row {
                says: "without info, only its info arriving (checking or not) or failing to moves it",
                when: |s, e| !has_info(&s.phase) && observed(e).is_some(),
                then: |s, e, r| match (&s.phase, observed(e).unwrap()) {
                    (Phase::FetchingInfo, Observation::InfoArrived { checking: true }) => becomes(s, r, checking()),
                    (Phase::FetchingInfo, Observation::InfoArrived { checking: false }) => becomes(s, r, Phase::Ready),
                    (Phase::FetchingInfo, Observation::FetchFailed(m)) => becomes(s, r, Phase::FetchFailed { message: m.clone() }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "Pause stops it where it is",
                when: |s, e| is(Action::Pause)(s, e),
                then: |s, _, r| *r == Ok(State::new(Wants::Stop, s.phase.clone())),
            },
            Row {
                says: "Resume runs it, checking one Missing files or in Error (the way back into librqbit); refused while its destination is unavailable",
                when: |s, e| is(Action::Resume)(s, e),
                then: |s, e, r| match &s.phase {
                    Phase::DestinationUnavailable { .. } => unavailable(s, e, r),
                    Phase::MissingFiles { .. } | Phase::Error { .. } | Phase::DeleteFailed { .. } => {
                        *r == Ok(State::new(Wants::Run, check_due()))
                    }
                    _ => *r == Ok(State::new(Wants::Run, s.phase.clone())),
                },
            },
            Row {
                says: "Check checks it, keeping what's wanted; one going on goes on; refused while its destination is unavailable",
                when: |s, e| is(Action::Check)(s, e),
                then: |s, e, r| match &s.phase {
                    Phase::DestinationUnavailable { .. } => unavailable(s, e, r),
                    Phase::CheckDue { .. } | Phase::Checking { .. } => stays(s, r),
                    _ => becomes(s, r, check_due()),
                },
            },
            Row {
                says: "Open and Reveal are the client's: they change nothing",
                when: |_, e| command(e).is_some_and(client_side),
                then: |s, _, r| stays(s, r),
            },
            Row {
                says: "priorities and Remove are the actor's to carry out; Delete starts trashing, from wherever it is, \
                       but is refused while its destination is unavailable",
                when: |s, e| active(&s.phase) && matches!(command(e), Some(Action::SetPriority | Action::Remove | Action::Delete)),
                then: |s, e, r| match (&s.phase, command(e).unwrap()) {
                    (Phase::DestinationUnavailable { .. }, Action::Delete) => unavailable(s, e, r),
                    (_, Action::Delete) => becomes(s, r, Phase::Deleting { was: Box::new(s.phase.clone()) }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "while it's being deleted every command is refused, and once deleted it's not in the list",
                when: |s, e| deleting(&s.phase) && command(e).is_some_and(|c| !client_side(c)),
                then: |s, _, r| match &s.phase {
                    Phase::Deleted => *r == Err(Refusal::NotInList),
                    _ => *r == Err(Refusal::Deleting),
                },
            },
            Row {
                says: "while it's being deleted, or once deleted, nothing librqbit or the disk says changes it",
                when: |s, e| deleting(&s.phase) && observed(e).is_some_and(|o| !matches!(o, Observation::Trashed { .. })),
                then: |s, _, r| stays(s, r),
            },
            Row {
                says: "the trash's outcome: everything went, it's deleted; nothing went, it's as it was; some went, it waits stopped \
                       with what's left. Outside a Delete it's stale",
                when: |s, e| has_info(&s.phase) && matches!(observed(e), Some(Observation::Trashed { .. })),
                then: |s, e, r| {
                    let Some(Observation::Trashed { kept, moved }) = observed(e) else { unreachable!() };
                    match &s.phase {
                        Phase::Deleting { was } if kept.is_empty() => becomes(s, r, Phase::Deleted),
                        Phase::Deleting { was } if !moved => becomes(s, r, (**was).clone()),
                        Phase::Deleting { .. } => becomes(s, r, Phase::DeleteFailed { kept: kept.clone() }),
                        _ => stays(s, r),
                    }
                },
            },
            Row {
                says: "its info arriving or failing again changes nothing",
                when: |s, e| active(&s.phase) && matches!(observed(e), Some(Observation::InfoArrived { .. } | Observation::FetchFailed(_))),
                then: |s, _, r| stays(s, r),
            },
            Row {
                says: "librqbit checking by itself checks one Ready or in Error, or one whose check is due, and nothing else",
                when: |s, e| active(&s.phase) && observed(e) == Some(&Observation::Checking),
                then: |s, _, r| match &s.phase {
                    Phase::Ready | Phase::Error { .. } => becomes(s, r, checking()),
                    Phase::CheckDue { returning } => becomes(s, r, Phase::Checking { returning: returning.clone() }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "added again, a check due is librqbit's, with the same files returning; nothing else is added again",
                when: |s, e| active(&s.phase) && observed(e) == Some(&Observation::AddedAgain),
                then: |s, _, r| match &s.phase {
                    Phase::CheckDue { returning } => becomes(s, r, Phase::Checking { returning: returning.clone() }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "librqbit idle ends a check: a file put back is back if librqbit holds at least what it had before it went, \
                       and missing again, not judged yet, if not; none missing, it's Ready. It clears an Error",
                when: |s, e| active(&s.phase) && matches!(observed(e), Some(Observation::Idle { .. })),
                then: |s, e, r| {
                    let Some(Observation::Idle { have }) = observed(e) else { unreachable!() };
                    match &s.phase {
                        Phase::Checking { returning } => {
                            let none = BTreeMap::new();
                            let again = match r.as_ref().map(|n| &n.phase) {
                                Ok(Phase::Ready) => &none,
                                Ok(Phase::MissingFiles { gone }) => gone,
                                _ => return false,
                            };
                            let holds_all = |i: &usize, g: &Gone| have.get(*i).is_some_and(|&h| h >= g.progress_bytes);
                            r.as_ref().is_ok_and(|n| n.wants == s.wants)
                                && again.keys().all(|i| returning.contains_key(i))
                                && returning.iter().all(|(i, g)| match again.get(i) {
                                    None => holds_all(i, g),
                                    Some(missing) => !holds_all(i, g) && *missing == Gone { seen: Seen::NotYet, ..g.clone() },
                                })
                        }
                        Phase::Error { .. } => becomes(s, r, Phase::Ready),
                        _ => stays(s, r),
                    }
                },
            },
            Row {
                says: "librqbit failing is an Error, unless what's wrong is known already: its files or destination",
                when: |s, e| active(&s.phase) && matches!(observed(e), Some(Observation::Failed(_))),
                then: |s, e, r| {
                    let Some(Observation::Failed(message)) = observed(e) else { unreachable!() };
                    match &s.phase {
                        Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. } | Phase::DeleteFailed { .. } => stays(s, r),
                        _ => becomes(s, r, Phase::Error { message: message.clone() }),
                    }
                },
            },
            Row {
                says: "a size check: files gone make it Missing files, set Off or not judged yet are dropped or judged, all back is checked; \
                       a check catches them itself; the destination back is checked",
                when: |s, e| active(&s.phase) && matches!(observed(e), Some(Observation::Files(_))),
                then: |s, e, r| {
                    let Some(Observation::Files(disk)) = observed(e) else { unreachable!() };
                    match &s.phase {
                        // Its files are partly in the Trash on purpose.
                        Phase::CheckDue { .. } | Phase::Checking { .. } | Phase::DeleteFailed { .. } => stays(s, r),
                        Phase::DestinationUnavailable { .. } => becomes(s, r, check_due()),
                        Phase::MissingFiles { gone } => becomes(s, r, after_size_check(gone, disk)),
                        // A file found missing now is what's there now: it isn't back.
                        _ => match after_size_check(&BTreeMap::new(), disk) {
                            Phase::Ready => stays(s, r),
                            phase => matches!(phase, Phase::MissingFiles { .. }) && becomes(s, r, phase),
                        },
                    }
                },
            },
            Row {
                says: "its destination gone makes it Destination unavailable, with why, whatever it was doing",
                when: |s, e| active(&s.phase) && matches!(observed(e), Some(Observation::DestinationGone { .. })),
                then: |s, e, r| {
                    let Some(Observation::DestinationGone { reason }) = observed(e) else { unreachable!() };
                    becomes(s, r, Phase::DestinationUnavailable { reason: reason.clone() })
                },
            },
        ]
    }

    /// Properties of every state and event, whatever its row.
    fn always() -> Vec<Row> {
        let all = |_: &State, _: &Event| true;
        vec![
            Row {
                says: "Pause and Resume set what's wanted, and nothing else does",
                when: all,
                then: |s, e, r| {
                    let wants = match command(e) {
                        Some(Action::Pause) => Wants::Stop,
                        Some(Action::Resume) => Wants::Run,
                        _ => s.wants,
                    };
                    r.as_ref().ok().is_none_or(|n| n.wants == wants)
                },
            },
            Row { says: "what the actor sees is never refused", when: all, then: |_, e, r| command(e).is_some() || r.is_ok() },
            Row {
                says: "a command given twice does nothing the second time; Delete's second is refused, as it's deleting",
                when: all,
                then: |s, e, r| {
                    command(e).is_none()
                        || r.as_ref().ok().is_none_or(|n| match &n.phase {
                            Phase::Deleting { .. } if n != s => decide(n, e) == Err(Refusal::Deleting),
                            _ => decide(n, e).as_ref() == Ok(n),
                        })
                },
            },
            Row {
                says: "a torrent never goes back to fetching info, and only its info arriving gives it info",
                when: all,
                then: |s, e, r| {
                    r.as_ref().ok().is_none_or(|n| match (has_info(&s.phase), has_info(&n.phase)) {
                        (true, has) => has,
                        (false, true) => matches!(observed(e), Some(Observation::InfoArrived { .. })),
                        (false, false) => true,
                    })
                },
            },
        ]
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        decide_table::<Torrent>(&rows(), &always());
    }

    #[test]
    fn every_state_reached_from_an_add_is_decided_as_its_row_says() {
        let added = Wants::every().into_iter().map(|wants| State::new(wants, Phase::FetchingInfo)).collect();
        sequences::<Torrent>(added, 30, &rows(), &always());
    }

    // Size checks over generated files: three files, each missing or not, judged as what was there,
    // another file, or not yet, and found as no file, the one judged, another or one too short.

    fn seen() -> impl Strategy<Value = Seen> {
        prop_oneof![
            Just(Seen::NotYet),
            Just(Seen::Was(None)),
            Just(Seen::Was(Some(stamp(WAS, 10)))),
            Just(Seen::Was(Some(stamp(OTHER, 10))))
        ]
    }

    fn at() -> impl Strategy<Value = Option<Stamp>> {
        prop_oneof![Just(None), Just(Some(stamp(WAS, 10))), Just(Some(stamp(OTHER, 10))), Just(Some(stamp(OTHER, 2)))]
    }

    fn missing_state() -> impl Strategy<Value = State> {
        let gone = proptest::collection::btree_map(0..3usize, seen().prop_map(gone_as), 0..=3);
        let phase = prop_oneof![
            Just(Phase::Ready),
            Just(Phase::Error { message: "x".into() }),
            Just(check_due()),
            Just(checking()),
            Just(Phase::DestinationUnavailable { reason: drive() }),
            gone.prop_filter_map("missing some", |gone| (!gone.is_empty()).then_some(Phase::MissingFiles { gone })),
        ];
        (phase, prop_oneof![Just(Wants::Run), Just(Wants::Stop)]).prop_map(|(phase, wants)| State::new(wants, phase))
    }

    fn files_seen() -> impl Strategy<Value = Event> {
        proptest::collection::btree_map(0..3usize, at(), 0..=3).prop_map(|files| Event::Observed(Observation::Files(disk(files))))
    }

    proptest! {
        #[test]
        fn a_size_check_is_decided_as_its_row_says(s in missing_state(), e in files_seen()) {
            let _ = check(decide, &rows(), &always(), &s, &e);
        }
    }

    // The rest of the machine.

    #[test]
    fn a_status_follows_from_the_torrent_alone() {
        for s in State::every() {
            for complete in [false, true] {
                let failed = matches!(s.phase, Phase::FetchFailed { .. } | Phase::Error { .. } | Phase::DeleteFailed { .. });
                assert_eq!(failed, matches!(s.status(complete), Status::Error { .. }), "{s:?}");
            }
        }
    }

    #[test]
    fn actions_are_the_commands_that_do_something() {
        for s in State::every() {
            for c in Action::every() {
                let does = match c {
                    // Offered while the files are on disk to see.
                    Action::Open | Action::Reveal => {
                        matches!(
                            s.phase,
                            Phase::CheckDue { .. }
                                | Phase::Checking { .. }
                                | Phase::Ready
                                | Phase::Error { .. }
                                | Phase::DeleteFailed { .. }
                        )
                    }
                    _ => match decide(&s, &Event::Command(c)) {
                        Ok(n) => n != s || matches!(c, Action::SetPriority | Action::Remove | Action::Delete),
                        Err(_) => false,
                    },
                };
                assert_eq!(s.actions().contains(&c), does, "{c:?} on {s:?}");
            }
        }
    }

    /// Every state × what librqbit is doing, in exactly one row.
    #[test]
    fn librqbit_is_made_to_match_as_its_row_says() {
        type NextRow = machine::Row<State, Lib, Option<Do>>;
        let rows: [NextRow; 5] = [
            NextRow {
                says: "a check due is added again, unless librqbit is checking it already",
                when: |s, _| matches!(s.phase, Phase::CheckDue { .. }),
                then: |_, lib, todo| *todo == (*lib != Lib::Checking).then_some(Do::AddAgain),
            },
            NextRow {
                says: "nothing interrupts a check, nor starts one again once it ended",
                when: |s, _| matches!(s.phase, Phase::Checking { .. }),
                then: |_, _, todo| todo.is_none(),
            },
            NextRow {
                says: "a ready one is started if wanted running and stopped, and stopped if wanted stopped and running",
                when: |s, _| s.phase == Phase::Ready,
                then: |s, lib, todo| {
                    *todo
                        == match (s.wants, lib) {
                            (Wants::Run, Lib::Stopped) => Some(Do::Start),
                            (Wants::Stop, Lib::Running) => Some(Do::Stop),
                            _ => None,
                        }
                },
            },
            NextRow {
                says: "one whose files or destination are gone, or on their way to the Trash, is stopped, whatever is wanted",
                when: |s, _| {
                    matches!(
                        s.phase,
                        Phase::MissingFiles { .. }
                            | Phase::DestinationUnavailable { .. }
                            | Phase::DeleteFailed { .. }
                            | Phase::Deleting { .. }
                    )
                },
                then: |_, lib, todo| *todo == (*lib == Lib::Running).then_some(Do::Stop),
            },
            NextRow {
                says: "without info, in error until Resume or Check, or deleted, nothing is done",
                when: |s, _| matches!(s.phase, Phase::FetchingInfo | Phase::FetchFailed { .. } | Phase::Error { .. } | Phase::Deleted),
                then: |_, _, todo| todo.is_none(),
            },
        ];
        table(|s, lib| next(s, *lib), &rows, &[]);
    }
}
