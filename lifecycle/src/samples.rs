//! Samples of every state, event and input of the torrent's machine, for tables ([`Every`]): files
//! 0 and 1, each 10 bytes, of which librqbit had 5. Feature `harness`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proto::{AbsolutePath, Action, Unavailable};

use crate::disk::{Disk, Expected, Found, Gone, Seen, Stamp};
use crate::every;
use crate::machine::Every;
use crate::{Do, Event, Lib, Observation, Phase, State, Wants};

pub fn stamp(inode: u64, len: u64) -> Stamp {
    Stamp { inode, len, modified: None }
}

/// What was at a missing file's path when it went, and another file of a size it can have.
pub const WAS: u64 = 1;
pub const OTHER: u64 = 2;

pub fn gone_as(seen: Seen) -> Gone {
    Gone { path: PathBuf::from("/x/a"), size_bytes: 10, progress_bytes: 5, seen }
}

pub fn gone() -> BTreeMap<usize, Gone> {
    [(0, gone_as(Seen::Was(Some(stamp(WAS, 10)))))].into()
}

pub fn found(index: usize, at: Option<Stamp>) -> (usize, Found) {
    let file = Expected { index, path: PathBuf::from(format!("/x/{index}")), size_bytes: 10, progress_bytes: 5 };
    (index, Found { file, at })
}

pub fn disk(files: impl IntoIterator<Item = (usize, Option<Stamp>)>) -> Disk {
    Disk { files: files.into_iter().map(|(i, at)| found(i, at)).collect() }
}

pub fn drive() -> Unavailable {
    Unavailable::DriveNotConnected { name: "MyDisk".into() }
}

every!(Phase {
    Phase::FetchingInfo => [Phase::FetchingInfo],
    Phase::FetchFailed { .. } => [Phase::FetchFailed { message: "no peers".into() }],
    Phase::CheckDue { .. } => [Phase::CheckDue { returning: BTreeMap::new() }, Phase::CheckDue { returning: gone() }],
    Phase::Checking { .. } => [Phase::Checking { returning: BTreeMap::new() }, Phase::Checking { returning: gone() }],
    Phase::Ready => [Phase::Ready],
    Phase::MissingFiles { .. } => [
        Phase::MissingFiles { gone: gone() },
        Phase::MissingFiles { gone: [(0, gone_as(Seen::NotYet)), (1, gone_as(Seen::Was(None)))].into() },
    ],
    Phase::DestinationUnavailable { .. } => [Phase::DestinationUnavailable { reason: drive() }],
    Phase::Error { .. } => [Phase::Error { message: "disk full".into() }],
});

every!(Wants { Wants::Run => [Wants::Run], Wants::Stop => [Wants::Stop] });

/// Every phase, wanted running and stopped. A new field doesn't compile here.
impl Every for State {
    fn every() -> Vec<Self> {
        let all = |phase: Phase| Wants::every().into_iter().map(move |wants| State { wants, phase: phase.clone() });
        let states: Vec<State> = Phase::every().into_iter().flat_map(all).collect();
        for State { wants: _, phase: _ } in &states {}
        states
    }
}

every!(Action {
    Action::Pause => [Action::Pause],
    Action::Resume => [Action::Resume],
    Action::Check => [Action::Check],
    Action::SetPriority => [Action::SetPriority],
    Action::Remove => [Action::Remove],
    Action::Delete => [Action::Delete],
    Action::Open => [Action::Open],
    Action::Reveal => [Action::Reveal],
});

// Every observation, each message also the same as and other than the one in the phases,
// since a repeated message may be decided differently; and disks with each file missing,
// back, as it was, or no longer got (Off).
every!(Observation {
    Observation::InfoArrived { .. } => [Observation::InfoArrived { checking: true }, Observation::InfoArrived { checking: false }],
    Observation::FetchFailed(_) => [Observation::FetchFailed("no peers".into())],
    Observation::Checking => [Observation::Checking],
    Observation::Idle { .. } => [
        Observation::Idle { have: vec![] },
        Observation::Idle { have: vec![10, 10] },
        Observation::Idle { have: vec![0, 10] },
        // Exactly what librqbit had of each file before it went.
        Observation::Idle { have: vec![5, 5] },
    ],
    Observation::Failed(_) => [Observation::Failed("disk full".into()), Observation::Failed("no space".into())],
    Observation::AddedAgain => [Observation::AddedAgain],
    Observation::Files(_) => {
        let fits = Some(stamp(OTHER, 10));
        [
            disk([]),
            disk([(0, fits), (1, fits)]),
            disk([(0, None), (1, fits)]),
            disk([(0, Some(stamp(OTHER, 2))), (1, fits)]),
            disk([(0, Some(stamp(WAS, 10))), (1, fits)]),
            disk([(0, fits), (1, None)]),
            disk([(1, fits)]),
            disk([(1, None)]),
        ]
        .map(Observation::Files)
    },
    Observation::DestinationGone { .. } => [
        Observation::DestinationGone { reason: drive() },
        Observation::DestinationGone { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() } },
    ],
});

every!(Event {
    Event::Command(_) => Action::every().into_iter().map(Event::Command),
    Event::Observed(_) => Observation::every().into_iter().map(Event::Observed),
});

every!(Lib {
    Lib::Checking => [Lib::Checking],
    Lib::Running => [Lib::Running],
    Lib::Stopped => [Lib::Stopped],
    Lib::Failed => [Lib::Failed],
    Lib::Absent => [Lib::Absent],
});

every!(Do { Do::Start => [Do::Start], Do::Stop => [Do::Stop], Do::AddAgain => [Do::AddAgain] });
