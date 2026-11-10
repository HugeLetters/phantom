//! A torrent's lifecycle as a state machine ([ADR 0006]): its [`State`] is what the user wants of it
//! ([`Wants`]) and what it's doing ([`Phase`]); its Status follows from the two and whether it's
//! complete. [`decide`] says what an [`Event`] does, one written decision per state and event: a
//! client's command sets what's wanted (and may start a check), or is refused with a
//! [`Refusal`]; an [`Observation`] of librqbit or of the disk changes only the phase, and is never
//! refused. The actor reports raw facts (what librqbit is doing, what's on disk) and the rules of
//! what they mean live here. [`next`] says what makes librqbit match: the [`Do`] the actor carries
//! out. Pure: the actor in `actor.rs` carries both out.
//!
//! The reference machine: every machine names these the same, and its `decide` matches every
//! `(state, event)` pair with no `_` arm, so a new state or event fails to compile until decided.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

use std::collections::BTreeMap;

use proto::{Action, Status, Unavailable};
use serde::{Deserialize, Serialize};

use super::files::{Disk, Gone, Seen};

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
}

/// Why a command for a torrent is refused: data for the client, which words it.
pub use proto::TorrentRefusal as Refusal;

pub mod inlib;

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
        }
    }

    /// The commands that do something now: those not refused, less Pause, Resume and Check where
    /// they'd change nothing.
    pub fn actions(&self) -> Vec<Action> {
        use Action::*;
        [Pause, Resume, Check, SetPriority, Remove, Delete]
            .into_iter()
            .filter(|action| match decide(self, &Event::Command(*action)) {
                Ok(next) => next != *self || matches!(action, SetPriority | Remove | Delete),
                Err(_) => false,
            })
            .collect()
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
            Observed(O::Checking | O::Idle { .. } | O::Failed(_) | O::AddedAgain | O::Files(_) | O::DestinationGone { .. }),
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
                | O::DestinationGone { .. },
            ),
        ) => stays(),

        // A check due waits for librqbit to have the torrent again; nothing it does before matters.
        (P::CheckDue { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::CheckDue { .. }, Command(C::Resume)) => wants(Wants::Run),
        (P::CheckDue { .. }, Command(C::Check | C::SetPriority | C::Remove | C::Delete)) => stays(),
        (P::CheckDue { .. }, Observed(O::InfoArrived { .. } | O::FetchFailed(_) | O::Idle { .. } | O::Files(_))) => stays(),
        // Adding it again failed.
        (P::CheckDue { .. }, Observed(O::Failed(message))) => phase(P::Error { message: message.clone() }),
        // librqbit checks it already, or now that it has it again.
        (P::CheckDue { returning }, Observed(O::Checking | O::AddedAgain)) => phase(P::Checking { returning: returning.clone() }),
        (P::CheckDue { .. }, Observed(O::DestinationGone { reason })) => phase(P::DestinationUnavailable { reason: reason.clone() }),

        // Nothing interrupts a check; a priority change during one is held until it ends.
        (P::Checking { .. }, Command(C::Pause)) => wants(Wants::Stop),
        (P::Checking { .. }, Command(C::Resume)) => wants(Wants::Run),
        (P::Checking { .. }, Command(C::Check | C::SetPriority | C::Remove | C::Delete)) => stays(),
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
        (P::Ready, Command(C::SetPriority | C::Remove | C::Delete)) => stays(),
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
        (P::MissingFiles { .. }, Command(C::SetPriority | C::Remove | C::Delete)) => stays(),
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
        (P::Error { .. }, Command(C::SetPriority | C::Remove | C::Delete)) => stays(),
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

/// What makes librqbit, doing `lib`, match `s`. A check due is entered by adding the torrent
/// again, unless librqbit is checking it already. Nothing interrupts a check: one that ended is
/// stopped, until the actor sees so.
pub fn next(s: &State, lib: Lib) -> Option<Do> {
    use Lib as L;
    use Phase as P;
    match (&s.phase, s.wants, lib) {
        (P::CheckDue { .. }, _, L::Checking) => None,
        (P::CheckDue { .. }, _, L::Running | L::Stopped | L::Failed | L::Absent) => Some(Do::AddAgain),
        (P::Checking { .. }, _, L::Checking | L::Running | L::Stopped | L::Failed | L::Absent) => None,
        (P::Ready, Wants::Run, L::Stopped) => Some(Do::Start),
        (P::Ready, Wants::Stop, L::Running) => Some(Do::Stop),
        (P::Ready, Wants::Run, L::Checking | L::Running | L::Failed | L::Absent) => None,
        (P::Ready, Wants::Stop, L::Checking | L::Stopped | L::Failed | L::Absent) => None,
        // Nothing more is written into deleted files, nor where the destination was.
        (P::MissingFiles { .. } | P::DestinationUnavailable { .. }, _, L::Running) => Some(Do::Stop),
        (P::MissingFiles { .. } | P::DestinationUnavailable { .. }, _, L::Checking | L::Stopped | L::Failed | L::Absent) => None,
        // librqbit doesn't have it, or it's an error until Resume or Check.
        (P::FetchingInfo | P::FetchFailed { .. } | P::Error { .. }, _, L::Checking | L::Running | L::Stopped | L::Failed | L::Absent) => {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use proptest::prelude::*;
    use proto::AbsolutePath;

    use super::super::files::{Expected, Found, Stamp};
    use super::*;

    // What every state and event is made of: files 0 and 1, each 10 bytes, of which librqbit had 5.

    fn stamp(inode: u64, len: u64) -> Stamp {
        Stamp { inode, len, modified: None }
    }

    /// What was at a missing file's path when it went, and another file of a size it can have.
    const WAS: u64 = 1;
    const OTHER: u64 = 2;

    fn gone_as(seen: Seen) -> Gone {
        Gone { path: PathBuf::from("/x/a"), size_bytes: 10, progress_bytes: 5, seen }
    }

    fn gone() -> BTreeMap<usize, Gone> {
        [(0, gone_as(Seen::Was(Some(stamp(WAS, 10)))))].into()
    }

    fn found(index: usize, at: Option<Stamp>) -> (usize, Found) {
        let file = Expected { index, path: PathBuf::from(format!("/x/{index}")), size_bytes: 10, progress_bytes: 5 };
        (index, Found { file, at })
    }

    fn disk(files: impl IntoIterator<Item = (usize, Option<Stamp>)>) -> Disk {
        Disk { files: files.into_iter().map(|(i, at)| found(i, at)).collect() }
    }

    fn drive() -> Unavailable {
        Unavailable::DriveNotConnected { name: "MyDisk".into() }
    }

    fn every_phase() -> Vec<Phase> {
        vec![
            Phase::FetchingInfo,
            Phase::FetchFailed { message: "no peers".into() },
            check_due(),
            Phase::CheckDue { returning: gone() },
            checking(),
            Phase::Checking { returning: gone() },
            Phase::Ready,
            Phase::MissingFiles { gone: gone() },
            Phase::MissingFiles { gone: [(0, gone_as(Seen::NotYet)), (1, gone_as(Seen::Was(None)))].into() },
            Phase::DestinationUnavailable { reason: drive() },
            Phase::Error { message: "disk full".into() },
        ]
    }

    fn every_state() -> Vec<State> {
        every_phase().into_iter().flat_map(|phase| [Wants::Run, Wants::Stop].map(|wants| State::new(wants, phase.clone()))).collect()
    }

    fn commands() -> [Action; 6] {
        use Action::*;
        [Pause, Resume, SetPriority, Remove, Delete, Check]
    }

    /// Every observation, each message also the same as and other than the one in
    /// [`every_phase`], since a repeated message may be decided differently; and disks with each
    /// file missing, back, as it was, or no longer got (Off).
    fn observations() -> Vec<Observation> {
        use Observation::*;
        let fits = Some(stamp(OTHER, 10));
        vec![
            InfoArrived { checking: true },
            InfoArrived { checking: false },
            FetchFailed("no peers".into()),
            Checking,
            Idle { have: vec![] },
            Idle { have: vec![10, 10] },
            Idle { have: vec![0, 10] },
            // Exactly what librqbit had of each file before it went.
            Idle { have: vec![5, 5] },
            Failed("disk full".into()),
            Failed("no space".into()),
            AddedAgain,
            Files(disk([])),
            Files(disk([(0, fits), (1, fits)])),
            Files(disk([(0, None), (1, fits)])),
            Files(disk([(0, Some(stamp(OTHER, 2))), (1, fits)])),
            Files(disk([(0, Some(stamp(WAS, 10))), (1, fits)])),
            Files(disk([(0, fits), (1, None)])),
            Files(disk([(1, fits)])),
            Files(disk([(1, None)])),
            DestinationGone { reason: drive() },
            DestinationGone { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() } },
        ]
    }

    fn every_event() -> Vec<Event> {
        commands().map(Event::Command).into_iter().chain(observations().into_iter().map(Event::Observed)).collect()
    }

    fn every_lib() -> [Lib; 5] {
        [Lib::Checking, Lib::Running, Lib::Stopped, Lib::Failed, Lib::Absent]
    }

    fn has_info(phase: &Phase) -> bool {
        !matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. })
    }

    // The table: every state and event is in exactly one row, whose property what `decide` says of
    // it must have. A new phase or event needs a row before this passes.

    type Decided = Result<State, Refusal>;

    struct Row {
        says: &'static str,
        when: fn(&State, &Event) -> bool,
        then: fn(&State, &Event, &Decided) -> bool,
    }

    fn command(event: &Event) -> Option<Action> {
        match event {
            Event::Command(c) => Some(*c),
            Event::Observed(_) => None,
        }
    }

    fn observed(event: &Event) -> Option<&Observation> {
        match event {
            Event::Command(_) => None,
            Event::Observed(o) => Some(o),
        }
    }

    fn is(c: Action) -> impl Fn(&State, &Event) -> bool {
        move |s, e| has_info(&s.phase) && command(e) == Some(c)
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
                when: |s, e| !has_info(&s.phase) && command(e).is_some(),
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
                    Phase::MissingFiles { .. } | Phase::Error { .. } => *r == Ok(State::new(Wants::Run, check_due())),
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
                says: "priorities, Remove and Delete are the actor's to carry out; Delete is refused while its destination is unavailable",
                when: |s, e| has_info(&s.phase) && matches!(command(e), Some(Action::SetPriority | Action::Remove | Action::Delete)),
                then: |s, e, r| match (&s.phase, command(e).unwrap()) {
                    (Phase::DestinationUnavailable { .. }, Action::Delete) => unavailable(s, e, r),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "its info arriving or failing again changes nothing",
                when: |s, e| {
                    has_info(&s.phase) && matches!(observed(e), Some(Observation::InfoArrived { .. } | Observation::FetchFailed(_)))
                },
                then: |s, _, r| stays(s, r),
            },
            Row {
                says: "librqbit checking by itself checks one Ready or in Error, or one whose check is due, and nothing else",
                when: |s, e| has_info(&s.phase) && observed(e) == Some(&Observation::Checking),
                then: |s, _, r| match &s.phase {
                    Phase::Ready | Phase::Error { .. } => becomes(s, r, checking()),
                    Phase::CheckDue { returning } => becomes(s, r, Phase::Checking { returning: returning.clone() }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "added again, a check due is librqbit's, with the same files returning; nothing else is added again",
                when: |s, e| has_info(&s.phase) && observed(e) == Some(&Observation::AddedAgain),
                then: |s, _, r| match &s.phase {
                    Phase::CheckDue { returning } => becomes(s, r, Phase::Checking { returning: returning.clone() }),
                    _ => stays(s, r),
                },
            },
            Row {
                says: "librqbit idle ends a check: a file put back is back if librqbit holds at least what it had before it went, \
                       and missing again, not judged yet, if not; none missing, it's Ready. It clears an Error",
                when: |s, e| has_info(&s.phase) && matches!(observed(e), Some(Observation::Idle { .. })),
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
                when: |s, e| has_info(&s.phase) && matches!(observed(e), Some(Observation::Failed(_))),
                then: |s, e, r| {
                    let Some(Observation::Failed(message)) = observed(e) else { unreachable!() };
                    match &s.phase {
                        Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. } => stays(s, r),
                        _ => becomes(s, r, Phase::Error { message: message.clone() }),
                    }
                },
            },
            Row {
                says: "a size check: files gone make it Missing files, set Off or not judged yet are dropped or judged, all back is checked; \
                       a check catches them itself; the destination back is checked",
                when: |s, e| has_info(&s.phase) && matches!(observed(e), Some(Observation::Files(_))),
                then: |s, e, r| {
                    let Some(Observation::Files(disk)) = observed(e) else { unreachable!() };
                    match &s.phase {
                        Phase::CheckDue { .. } | Phase::Checking { .. } => stays(s, r),
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
                when: |s, e| has_info(&s.phase) && matches!(observed(e), Some(Observation::DestinationGone { .. })),
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
                says: "a command given twice does nothing the second time",
                when: all,
                then: |_, e, r| command(e).is_none() || r.as_ref().ok().is_none_or(|n| decide(n, e).as_ref() == Ok(n)),
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

    fn holds(row: &Row, s: &State, e: &Event) {
        let r = decide(s, e);
        assert!((row.then)(s, e, &r), "{}: {e:?} on {s:?} is {r:?}", row.says);
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        let (rows, always) = (rows(), always());
        for s in every_state() {
            for e in every_event() {
                let mut matched = rows.iter().filter(|row| (row.when)(&s, &e));
                let row = matched.next().unwrap_or_else(|| panic!("no row for {e:?} on {s:?}"));
                assert!(matched.next().is_none(), "{e:?} on {s:?} is in two rows");
                holds(row, &s, &e);
                always.iter().for_each(|p| holds(p, &s, &e));
            }
        }
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
            let rows = rows();
            let row = rows.iter().find(|row| (row.when)(&s, &e)).unwrap();
            holds(row, &s, &e);
            always().iter().for_each(|p| holds(p, &s, &e));
        }
    }

    // The rest of the machine.

    #[test]
    fn a_status_follows_from_the_torrent_alone() {
        for s in every_state() {
            for complete in [false, true] {
                let failed = matches!(s.phase, Phase::FetchFailed { .. } | Phase::Error { .. });
                assert_eq!(failed, matches!(s.status(complete), Status::Error { .. }), "{s:?}");
            }
        }
    }

    #[test]
    fn actions_are_the_commands_that_do_something() {
        for s in every_state() {
            for c in commands() {
                let does = match decide(&s, &Event::Command(c)) {
                    Ok(n) => n != s || matches!(c, Action::SetPriority | Action::Remove | Action::Delete),
                    Err(_) => false,
                };
                assert_eq!(s.actions().contains(&c), does, "{c:?} on {s:?}");
            }
        }
    }

    #[test]
    fn only_a_check_due_is_added_again_unless_librqbit_is_checking() {
        for s in every_state() {
            for lib in every_lib() {
                let add_again = matches!(s.phase, Phase::CheckDue { .. }) && lib != Lib::Checking;
                assert_eq!(next(&s, lib) == Some(Do::AddAgain), add_again, "{s:?} with librqbit {lib:?}");
            }
        }
    }

    #[test]
    fn librqbit_is_started_only_for_a_ready_torrent_wanted_running_and_stopped_where_it_must_not_run() {
        for s in every_state() {
            let ready_to_run = (&s.phase, s.wants) == (&Phase::Ready, Wants::Run);
            for lib in every_lib() {
                let todo = next(&s, lib);
                assert_eq!(todo == Some(Do::Start), ready_to_run && lib == Lib::Stopped, "{s:?} with librqbit {lib:?}");
                let must_stop =
                    matches!(s.phase, Phase::Ready | Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. }) && !ready_to_run;
                assert_eq!(todo == Some(Do::Stop), must_stop && lib == Lib::Running, "{s:?} with librqbit {lib:?}");
            }
        }
    }
}
