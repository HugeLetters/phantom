//! PROTOTYPE (#174), test-only, not for merge: the torrent's [`decide`](super::decide) as a
//! hierarchical machine (superstates), beside the flat one, checked equal to it.
//!
//! ```text
//! Torrent                 every event: stays (what no state handles changes nothing)
//! ├─ Fetching { why }     the commands that need info: refused with `why`
//! │   ├─ FetchingInfo     (why: NoInfoYet)
//! │   └─ FetchFailed      (why: FetchFailed)
//! ├─ OnDisk               Pause, Resume, Check, Cancel check, Delete, Failed, DestinationGone
//! │   ├─ CheckDue, Checking, Ready, Unchecked, MissingFiles, Error
//! │   ├─ DestinationUnavailable
//! │   └─ DeleteFailed
//! └─ Leaving { why }      every command but Open and Reveal: refused with `why`
//!     ├─ Deleting         (why: Deleting)
//!     └─ Deleted          (why: NotInList)
//! ```
//!
//! A level says of each event [`Rule::Here`] (decided) or [`UP`] (its superstate's). Each level is
//! one match over every event, with no `_` arm: a new event doesn't compile until every phase and
//! every group says Here or UP, and a new phase doesn't compile until [`group`] places it.

use super::*;
use Action as C;
use Event::{Command, Observed};
use Observation as O;
use Phase as P;

type Decided = Result<State, Refusal>;

/// What one level of the hierarchy says of an event.
#[derive(Debug)]
pub enum Rule {
    /// Decided at this level.
    Here(Decided),
    /// Its superstate's to decide.
    Up,
}

/// The event is the superstate's.
pub const UP: Rule = Rule::Up;

/// Which level decided an event: for the printed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Phase,
    Group,
    Torrent,
}

/// The superstates. Placing every phase in one is what makes a new phase compile.
#[derive(Debug)]
pub enum Group {
    /// librqbit doesn't have its info: what needs info is refused, with `why`.
    Fetching { why: Refusal },
    /// It has files on disk, or had: what's done to them.
    OnDisk,
    /// Delete has it: every command is refused, with `why`.
    Leaving { why: Refusal },
}

pub fn group(phase: &Phase) -> Group {
    match phase {
        P::FetchingInfo => Group::Fetching { why: Refusal::NoInfoYet },
        P::FetchFailed { .. } => Group::Fetching { why: Refusal::FetchFailed },
        P::CheckDue { .. }
        | P::Checking { .. }
        | P::Ready
        | P::Unchecked { .. }
        | P::MissingFiles { .. }
        | P::Error { .. }
        | P::DestinationUnavailable { .. }
        | P::DeleteFailed { .. } => Group::OnDisk,
        P::Deleting { .. } => Group::Leaving { why: Refusal::Deleting },
        P::Deleted => Group::Leaving { why: Refusal::NotInList },
    }
}

/// The outcomes every level writes, for torrent `s`.
struct At<'a> {
    s: &'a State,
}

impl At<'_> {
    fn stays(&self) -> Rule {
        Rule::Here(Ok(self.s.clone()))
    }
    fn wants(&self, wants: Wants) -> Rule {
        Rule::Here(Ok(State { wants, phase: self.s.phase.clone() }))
    }
    /// Pause stops it; one stopped at a seeding limit is stopped already, and stays Done saying so.
    fn pause(&self) -> Rule {
        match self.s.wants {
            Wants::Run | Wants::Stop => self.wants(Wants::Stop),
            Wants::Seeded { .. } => self.stays(),
        }
    }
    fn phase(&self, phase: Phase) -> Rule {
        Rule::Here(Ok(State { wants: self.s.wants, phase }))
    }
    fn to(&self, wants: Wants, phase: Phase) -> Rule {
        Rule::Here(Ok(State { wants, phase }))
    }
    fn refuse(&self, why: Refusal) -> Rule {
        Rule::Here(Err(why))
    }
    fn unavailable(&self, action: Action, reason: &Unavailable) -> Rule {
        self.refuse(Refusal::DestinationUnavailable { action, reason: reason.clone() })
    }
}

/// The hierarchical `decide`: the phase's rule, else its group's, else the torrent's.
pub fn decide(s: &State, event: &Event) -> Decided {
    traced(s, event).0
}

/// [`decide`], with the level that decided.
pub fn traced(s: &State, event: &Event) -> (Decided, Level) {
    let at = At { s };
    if let Rule::Here(d) = phase_rule(&at, event) {
        return (d, Level::Phase);
    }
    if let Rule::Here(d) = group_rule(&at, &group(&s.phase), event) {
        return (d, Level::Group);
    }
    (torrent_rule(&at, event), Level::Torrent)
}

/// The root: what no state handles changes nothing. Open and Reveal are the client's; Remove and
/// priorities the actor's; a fact that doesn't concern the phase is stale.
fn torrent_rule(at: &At, event: &Event) -> Decided {
    match event {
        Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
        | Observed(
            O::InfoArrived { .. }
            | O::FetchFailed(_)
            | O::Checking
            | O::Idle { .. }
            | O::Failed(_)
            | O::AddedAgain
            | O::Files(_)
            | O::DestinationGone { .. }
            | O::Trashed { .. }
            | O::LimitReached(_)
            | O::Halted,
        ) => Ok(at.s.clone()),
    }
}

fn group_rule(at: &At, group: &Group, event: &Event) -> Rule {
    let s = at.s;
    match (group, event) {
        // Without info only Remove and Delete apply.
        (Group::Fetching { why }, Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority)) => at.refuse(why.clone()),
        (
            Group::Fetching { .. },
            Command(C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        (Group::OnDisk, Command(C::Pause)) => at.pause(),
        (Group::OnDisk, Command(C::Resume)) => at.wants(Wants::Run),
        (Group::OnDisk, Command(C::Check)) => at.phase(check_due()),
        (Group::OnDisk, Command(C::CancelCheck)) => at.refuse(Refusal::NotChecking),
        // Delete trashes its files, wherever it was: it's stopped meanwhile (`next`).
        (Group::OnDisk, Command(C::Delete)) => at.phase(P::Deleting { was: Box::new(s.phase.clone()) }),
        (Group::OnDisk, Observed(O::Failed(failure))) => at.phase(P::Error { failure: failure.clone() }),
        (Group::OnDisk, Observed(O::DestinationGone { reason })) => at.phase(P::DestinationUnavailable { reason: reason.clone() }),
        (
            Group::OnDisk,
            Command(C::SetPriority | C::Remove | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::AddedAgain
                | O::Files(_)
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // What's done to it is the trash's until it's done, and then it's off the list.
        (Group::Leaving { why }, Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete)) => {
            at.refuse(why.clone())
        }
        (
            Group::Leaving { .. },
            Command(C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,
    }
}

/// What each phase decides itself, overriding its group.
fn phase_rule(at: &At, event: &Event) -> Rule {
    let s = at.s;
    match (&s.phase, event) {
        // What's wanted is remembered: once its info arrives it runs or stays stopped as wanted.
        (P::FetchingInfo, Command(C::Pause)) => at.pause(),
        (P::FetchingInfo, Command(C::Resume)) => at.wants(Wants::Run),
        (P::FetchingInfo, Observed(O::InfoArrived { checking: true })) => at.phase(checking()),
        (P::FetchingInfo, Observed(O::InfoArrived { checking: false })) => at.phase(P::Ready),
        (P::FetchingInfo, Observed(O::FetchFailed(detail))) => at.phase(P::FetchFailed { detail: detail.clone() }),
        (
            P::FetchingInfo,
            Command(C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        (
            P::FetchFailed { .. },
            Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // A check due waits for librqbit to have the torrent again.
        (P::CheckDue { .. }, Command(C::Check)) => at.stays(),
        // librqbit checks it already, or now that it has it again.
        (P::CheckDue { returning }, Observed(O::Checking | O::AddedAgain)) => at.phase(P::Checking { returning: returning.clone() }),
        (
            P::CheckDue { .. },
            Command(C::Pause | C::Resume | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Idle { .. }
                | O::Failed(_)
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // Nothing interrupts a check; a priority change during one is held until it ends.
        (P::Checking { .. }, Command(C::Check)) => at.stays(),
        // Its work is lost; the files that were missing before are kept.
        (P::Checking { returning }, Command(C::CancelCheck)) => at.phase(P::Unchecked { gone: returning.clone() }),
        // Ended by a pause nobody here asked for: the check goes again.
        (P::Checking { returning }, Observed(O::Halted)) => at.phase(P::CheckDue { returning: returning.clone() }),
        // A check ends: files put back that aren't what they were are missing again.
        (P::Checking { returning }, Observed(O::Idle { have })) => at.phase(check_ended(returning, have)),
        (
            P::Checking { .. },
            Command(C::Pause | C::Resume | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_),
            ),
        ) => UP,

        // librqbit checks again by itself.
        (P::Ready, Observed(O::Checking)) => at.phase(checking()),
        // A check that was halted found nothing: it's checked again.
        (P::Ready, Observed(O::Halted)) => at.phase(check_due()),
        (P::Ready, Observed(O::Files(disk))) => at.phase(found_missing(disk).map_or(P::Ready, |gone| P::MissingFiles { gone })),
        // Seeding, it reached a seeding limit: it stops, saying which. One stopped already stays.
        (P::Ready, Observed(O::LimitReached(reached))) => match s.wants {
            Wants::Run => at.wants(Wants::Seeded { reached: *reached }),
            Wants::Stop | Wants::Seeded { .. } => at.stays(),
        },
        (
            P::Ready,
            Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::DestinationGone { .. }
                | O::Trashed { .. },
            ),
        ) => UP,

        // Stopped, with nothing known of its files: a check is the way out, by Check or Resume.
        (P::Unchecked { .. }, Command(C::Resume)) => at.to(Wants::Run, check_due()),
        // Stopped in librqbit, whatever it says: only a check says what's on disk.
        (P::Unchecked { .. }, Observed(O::Failed(_))) => at.stays(),
        (
            P::Unchecked { .. },
            Command(C::Pause | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // A check is the way back into librqbit.
        (P::MissingFiles { .. }, Command(C::Resume)) => at.to(Wants::Run, check_due()),
        // Stopped in librqbit, which doesn't know the files went.
        (P::MissingFiles { .. }, Observed(O::Failed(_))) => at.stays(),
        (P::MissingFiles { gone }, Observed(O::Files(disk))) => at.phase(missing_files(gone, disk)),
        (
            P::MissingFiles { .. },
            Command(C::Pause | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::AddedAgain
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        (P::Error { .. }, Command(C::Resume)) => at.to(Wants::Run, check_due()),
        // librqbit is past the error.
        (P::Error { .. }, Observed(O::Checking)) => at.phase(checking()),
        (P::Error { .. }, Observed(O::Idle { .. })) => at.phase(P::Ready),
        (P::Error { .. }, Observed(O::Files(disk))) => match found_missing(disk) {
            Some(gone) => at.phase(P::MissingFiles { gone }),
            None => at.stays(),
        },
        (
            P::Error { .. },
            Command(C::Pause | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Failed(_)
                | O::AddedAgain
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // Its files are out of reach; once they're back it's checked.
        (P::DestinationUnavailable { reason }, Command(action @ (C::Resume | C::Check | C::Delete))) => at.unavailable(*action, reason),
        // Stopped in librqbit, which can't reach the files either.
        (P::DestinationUnavailable { .. }, Observed(O::Failed(_))) => at.stays(),
        (P::DestinationUnavailable { .. }, Observed(O::Files(_))) => at.phase(check_due()),
        (
            P::DestinationUnavailable { .. },
            Command(C::Pause | C::CancelCheck | C::SetPriority | C::Remove | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::AddedAgain
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // Each would have librqbit make the trashed files again.
        (P::DeleteFailed { .. }, Command(C::Resume | C::Check | C::SetPriority)) => at.refuse(Refusal::PartlyDeleted),
        (P::DeleteFailed { away: Some(reason), .. }, Command(action @ C::Delete)) => at.unavailable(*action, reason),
        // Partly in the Trash on purpose: nothing librqbit says changes it.
        (P::DeleteFailed { .. }, Observed(O::Failed(_))) => at.stays(),
        // The drive goes and comes back, and it waits all the same.
        (P::DeleteFailed { kept, unchecked, .. }, Observed(O::DestinationGone { reason })) => {
            at.phase(P::DeleteFailed { kept: kept.clone(), away: Some(reason.clone()), unchecked: *unchecked })
        }
        (P::DeleteFailed { kept, unchecked, .. }, Observed(O::Files(_))) => {
            at.phase(P::DeleteFailed { kept: kept.clone(), away: None, unchecked: *unchecked })
        }
        (P::DeleteFailed { away: None, .. }, Command(C::Delete)) => UP,
        (
            P::DeleteFailed { .. },
            Command(C::Pause | C::CancelCheck | C::Remove | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::AddedAgain
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        // All of it went; nothing went, so it's as it was; or it waits, stopped, with what's left.
        (P::Deleting { was }, Observed(O::Trashed { kept, moved })) => at.phase(match (kept.is_empty(), moved) {
            (true, _) => P::Deleted,
            (false, false) => (**was).clone(),
            (false, true) => P::DeleteFailed { kept: kept.clone(), away: None, unchecked: was.unchecked() },
        }),
        (
            P::Deleting { .. },
            Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,

        (
            P::Deleted,
            Command(C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority | C::Remove | C::Delete | C::Open | C::Reveal)
            | Observed(
                O::InfoArrived { .. }
                | O::FetchFailed(_)
                | O::Checking
                | O::Idle { .. }
                | O::Failed(_)
                | O::AddedAgain
                | O::Files(_)
                | O::DestinationGone { .. }
                | O::Trashed { .. }
                | O::LimitReached(_)
                | O::Halted,
            ),
        ) => UP,
    }
}

/// The flat table, generated from the hierarchy: every phase sample × event sample, the level that
/// decides it and what it decides. "What does X do on Y" is one line of it.
pub fn flat_table() -> String {
    use machine::Every;
    use std::fmt::Write;
    let mut out = String::from("phase | event | decided by | outcome (wanted running)\n");
    for phase in Phase::every() {
        let s = State::new(Wants::Run, phase);
        for e in Event::every() {
            let (d, level) = traced(&s, &e);
            let outcome = match d {
                Ok(n) if n == s => "stays".to_string(),
                Ok(n) => format!("{:?} {:?}", n.wants, n.phase),
                Err(why) => format!("refused: {why:?}"),
            };
            writeln!(out, "{:?} | {e:?} | {level:?} | {outcome}", s.phase).unwrap();
        }
    }
    out
}

/// Who decides each phase × event kind, as a matrix: `P` the phase, `G` its group, `·` the
/// torrent (stays); `P/G` where samples of the kind differ.
pub fn origin_matrix() -> String {
    use machine::Every;
    use std::collections::BTreeSet;
    use std::fmt::Write;
    let kind = |debug: String| -> String {
        let inner = debug.strip_prefix("Command(").or_else(|| debug.strip_prefix("Observed(")).unwrap_or(&debug).to_string();
        inner.split(|c: char| !c.is_alphanumeric()).next().unwrap().to_string()
    };
    let events = Event::every();
    let mut kinds: Vec<String> = Vec::new();
    for e in &events {
        let k = kind(format!("{e:?}"));
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    let mut out = String::new();
    write!(out, "{:<24}", "").unwrap();
    for k in &kinds {
        write!(out, "{:>7}", &k[..k.len().min(6)]).unwrap();
    }
    out.push('\n');
    let mut seen: Vec<String> = Vec::new();
    for phase in Phase::every() {
        let name = kind(format!("{phase:?}"));
        if seen.contains(&name) {
            continue;
        }
        seen.push(name.clone());
        let samples: Vec<State> = State::every().into_iter().filter(|s| kind(format!("{:?}", s.phase)) == name).collect();
        write!(out, "{name:<24}").unwrap();
        for k in &kinds {
            let levels: BTreeSet<&str> = samples
                .iter()
                .flat_map(|s| events.iter().filter(|e| kind(format!("{e:?}")) == *k).map(move |e| traced(s, e).1))
                .map(|l| match l {
                    Level::Phase => "P",
                    Level::Group => "G",
                    Level::Torrent => "·",
                })
                .collect();
            write!(out, "{:>7}", levels.into_iter().collect::<Vec<_>>().join("/")).unwrap();
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::super::machine::Every;
    use super::*;

    /// The main evidence: the hierarchy decides every sampled state × event as the flat one does.
    #[test]
    fn the_hierarchy_decides_every_state_and_event_as_the_flat_one() {
        let events = Event::every();
        let mut pairs = 0;
        for s in State::every() {
            for e in &events {
                assert_eq!(decide(&s, e), super::super::decide(&s, e), "{e:?} on {s:?}");
                pairs += 1;
            }
        }
        assert!(pairs > 1000, "{pairs}");
    }

    proptest! {
        /// And on every state reached by generated sequences from every sample, which reach
        /// phases the samples don't hold (Deleting from Checking, Missing files judged).
        #[test]
        fn the_hierarchy_decides_every_reached_state_as_the_flat_one(
            start in proptest::sample::select(State::every()),
            events in proptest::collection::vec(proptest::sample::select(Event::every()), 0..=12),
        ) {
            let mut s = start;
            for e in &events {
                let flat = super::super::decide(&s, e);
                prop_assert_eq!(&decide(&s, e), &flat, "{:?} on {:?}", e, s);
                if let Ok(n) = flat {
                    s = n;
                }
            }
        }
    }

    /// The generated flat table: `cargo test -p lifecycle superstates::tests::prints -- --nocapture`.
    #[test]
    fn prints_the_flat_table() {
        let matrix = origin_matrix();
        let table = flat_table();
        println!("{matrix}\n{table}");
        assert_eq!(table.lines().count(), 1 + Phase::every().len() * Event::every().len());
    }
}
