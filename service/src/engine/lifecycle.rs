//! A torrent's lifecycle as a plain-enum state machine ([ADR 0006]): [`decide`] says what an
//! [`Event`] does to a torrent in a given [`State`]: a transition, an allowed command that
//! doesn't change the state, a harmless no-op, or a refusal with a reason for the client. Pure:
//! the actor in `actor.rs` carries out the decision, entering the new state by making librqbit
//! match it.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

use proto::{Action, Status, Unavailable};

/// What a torrent is doing; [`State::status`] is what clients see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// librqbit doesn't have it yet. `paused`: it was added paused.
    FetchingInfo { paused: bool },
    /// Fetching its info failed. It stays in the list until it's removed.
    FetchFailed { message: String },
    /// librqbit is verifying its files. `paused`: it stays stopped afterwards (Paused or Done).
    Checking { paused: bool },
    Downloading,
    Seeding,
    Paused,
    Done,
    /// Files it's getting are gone from disk (or changed size); stopped in librqbit, which still
    /// holds the old files open. Leaving it takes the torrent out of librqbit and adds it again.
    MissingFiles,
    /// Its destination can't be reached, for `reason`; stopped in librqbit. `running`:
    /// it was running when its destination went (and wasn't paused since), so it runs again
    /// once it's back.
    DestinationUnavailable { reason: Unavailable, running: bool },
    /// Its files are moving to a new destination; out of librqbit meanwhile. `paused`: it stays
    /// stopped afterwards. Entered by #24.
    Moving { paused: bool },
    /// librqbit stopped it with an error.
    Error { message: String },
}

/// Something that happens to a torrent. Commands come from clients and are answered; the rest
/// are what the actor sees and are never refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // Commands. Their arguments (which files, which priority) don't change the decision, so
    // the actor keeps them.
    Pause,
    Resume,
    SetPriority,
    Details,
    Remove,
    Delete,
    /// Verify the files on disk: take the torrent out of librqbit and add it again, which checks
    /// them and keeps what's valid.
    Check,
    /// Move the torrent's files to a new destination. No client asks for it until #24.
    #[allow(dead_code, reason = "sent once moving exists (#24)")]
    Move,

    // What librqbit is doing, seen by polling it.
    /// The add finished: librqbit has the torrent and its info.
    InfoArrived,
    /// The add failed while fetching info.
    FetchFailed(String),
    /// librqbit is checking the torrent's files.
    SeenChecking,
    /// librqbit is running the torrent; `complete`: it has every file it's getting.
    SeenRunning { complete: bool },
    /// librqbit has it paused (or the check was cut short by a pause).
    SeenStopped { complete: bool },
    /// librqbit stopped it with an error.
    SeenFailed(String),

    // From the file system: sent by the file and destination checks of `files.rs`.
    /// A file it's getting is gone from disk or has a size it can't have.
    FilesMissing,
    /// None of the files it's getting is missing any more (those that were are now Off);
    /// `complete`: it has every file it's getting.
    NoFilesMissing { complete: bool },
    /// The missing files are back on disk; a check tells whether they're what they were.
    FilesBack,
    /// Its destination can't be reached, for this reason.
    DestinationGone { reason: Unavailable },
    /// Its destination can be reached again.
    DestinationBack,
    // From the mover of #24.
    #[allow(dead_code, reason = "sent by the mover (#24)")]
    MoveFinished,
    #[allow(dead_code, reason = "sent by the mover (#24)")]
    MoveFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Enter this state (possibly the same one again, which makes librqbit match it). A command
    /// is answered when it's entered.
    To(State),
    /// The command goes ahead without changing the state: details, priorities, remove.
    Allow,
    /// Nothing to do; a command is answered as done (e.g. Pause while Paused).
    Ignore,
    /// Take the torrent out of librqbit and add it again, so it checks the files on disk afresh
    /// (opening them again), then enter `Checking { paused }`. A command is answered once it's added.
    Recheck { paused: bool },
    /// Refused; nothing changes, and the reason goes back to the client, after "torrent N".
    Reject(Refusal),
}

/// Why a command is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Because(&'static str),
    /// Its destination can't be reached, so it can't do `what`.
    Away { what: Blocked, reason: Unavailable },
}

/// What a torrent whose destination is away can't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    Start,
    Check,
    Delete,
    Move,
}

impl Blocked {
    /// The client's request it refuses.
    pub fn action(self) -> Action {
        match self {
            Blocked::Start => Action::Resume,
            Blocked::Check => Action::Check,
            Blocked::Delete => Action::Delete,
            Blocked::Move => unreachable!("no client moves a torrent until #24"),
        }
    }
}

impl State {
    pub fn status(&self) -> Status {
        match self {
            State::FetchingInfo { .. } => Status::FetchingInfo,
            State::Checking { .. } => Status::Checking,
            State::Downloading => Status::Downloading,
            State::Seeding => Status::Seeding,
            State::Paused => Status::Paused,
            State::Done => Status::Done,
            State::MissingFiles => Status::MissingFiles,
            State::DestinationUnavailable { reason, .. } => Status::DestinationUnavailable { reason: reason.clone() },
            State::Moving { .. } => Status::Moving,
            State::FetchFailed { .. } | State::Error { .. } => Status::Error,
        }
    }

    /// Running, or about to: what Resume asks for. Recorded at shutdown to resume on start.
    pub fn is_running(&self) -> bool {
        matches!(self, State::Downloading | State::Seeding | State::Checking { paused: false })
    }

    /// Running, or waiting for its destination to run again: what's recorded at shutdown.
    pub fn wants_to_run(&self) -> bool {
        self.is_running() || matches!(self, State::DestinationUnavailable { running: true, .. })
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            State::FetchFailed { message } | State::Error { message } => Some(message),
            _ => None,
        }
    }
}

/// Running or stopped, by whether it's complete.
fn running(complete: bool) -> State {
    if complete { State::Seeding } else { State::Downloading }
}

fn stopped(complete: bool) -> State {
    if complete { State::Done } else { State::Paused }
}

fn reject(reason: &'static str) -> Decision {
    Decision::Reject(Refusal::Because(reason))
}

fn unavailable(reason: &Unavailable, running: bool) -> Decision {
    Decision::To(State::DestinationUnavailable { reason: reason.clone(), running })
}

fn away(what: Blocked, reason: &Unavailable) -> Decision {
    Decision::Reject(Refusal::Away { what, reason: reason.clone() })
}

/// What `event` does to a torrent in `state`. Every pair is listed: no catch-all arms, so a new
/// state or event doesn't compile until each of its pairs is decided.
pub fn decide(state: &State, event: &Event) -> Decision {
    use Decision::{Allow, Ignore, To};
    use Event as E;
    use State as S;

    const NO_INFO_YET: &str = "has no info yet";
    const NO_INFO: &str = "has no info: fetching it failed";
    const MOVING: &str = "is moving";

    match (state, event) {
        // Fetching info: librqbit doesn't have it, so only Remove and Delete apply.
        (S::FetchingInfo { .. }, E::Pause | E::Resume | E::SetPriority | E::Details | E::Check | E::Move) => {
            reject(NO_INFO_YET)
        }
        (S::FetchingInfo { .. }, E::Remove | E::Delete) => Allow,
        (S::FetchingInfo { paused }, E::InfoArrived) => To(S::Checking { paused: *paused }),
        (S::FetchingInfo { .. }, E::FetchFailed(message)) => To(S::FetchFailed { message: message.clone() }),
        (
            S::FetchingInfo { .. },
            E::SeenChecking
            | E::SeenRunning { .. }
            | E::SeenStopped { .. }
            | E::SeenFailed(_)
            | E::FilesMissing
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationGone { .. }
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,

        (S::FetchFailed { .. }, E::Pause | E::Resume | E::SetPriority | E::Details | E::Check | E::Move) => reject(NO_INFO),
        (S::FetchFailed { .. }, E::Remove | E::Delete) => Allow,
        (
            S::FetchFailed { .. },
            E::InfoArrived
            | E::FetchFailed(_)
            | E::SeenChecking
            | E::SeenRunning { .. }
            | E::SeenStopped { .. }
            | E::SeenFailed(_)
            | E::FilesMissing
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationGone { .. }
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,

        // Checking. Pausing cuts the check short (librqbit can't pause after it instead);
        // resuming starts it again.
        (S::Checking { .. }, E::Pause) => To(S::Paused),
        (S::Checking { paused: true }, E::Resume) => To(S::Checking { paused: false }),
        (S::Checking { paused: false }, E::Resume) => Ignore,
        // A priority change is held until the check ends.
        (S::Checking { .. }, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        (S::Checking { .. }, E::Check) => Ignore,
        (S::Checking { .. }, E::Move) => reject("is being checked"),
        (S::Checking { .. }, E::SeenChecking) => Ignore,
        // The check ended: on to what was asked, which also corrects librqbit if it disagrees
        // (it ignores a resume that comes during the check of a torrent added paused).
        (S::Checking { paused: false }, E::SeenRunning { complete } | E::SeenStopped { complete }) => {
            To(running(*complete))
        }
        (S::Checking { paused: true }, E::SeenRunning { complete } | E::SeenStopped { complete }) => {
            To(stopped(*complete))
        }
        (S::Checking { .. }, E::SeenFailed(message)) => To(S::Error { message: message.clone() }),
        // librqbit creates the files it checks, so one missing now shows in what the check finds;
        // one deleted after librqbit opened it is caught once the check ends.
        (S::Checking { .. }, E::FilesMissing | E::NoFilesMissing { .. } | E::FilesBack) => Ignore,
        (S::Checking { paused }, E::DestinationGone { reason }) => unavailable(reason, !paused),
        (S::Checking { .. }, E::InfoArrived | E::FetchFailed(_) | E::DestinationBack | E::MoveFinished | E::MoveFailed) => {
            Ignore
        }

        // Downloading and Seeding: running.
        (S::Downloading, E::Pause) => To(S::Paused),
        (S::Seeding, E::Pause) => To(S::Done),
        (S::Downloading | S::Seeding, E::Resume) => Ignore,
        (S::Downloading | S::Seeding, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        (S::Downloading | S::Seeding, E::Check) => Decision::Recheck { paused: false },
        (S::Downloading | S::Seeding, E::Move) => To(S::Moving { paused: false }),
        // librqbit checks again on its own, e.g. resuming after a check was cut short.
        (S::Downloading | S::Seeding, E::SeenChecking) => To(S::Checking { paused: false }),
        (S::Downloading, E::SeenRunning { complete: false }) | (S::Seeding, E::SeenRunning { complete: true }) => Ignore,
        // Finished, or a file set back to Normal.
        (S::Downloading | S::Seeding, E::SeenRunning { complete }) => To(running(*complete)),
        // librqbit stopped it without being asked: start it again.
        (S::Downloading | S::Seeding, E::SeenStopped { complete }) => To(running(*complete)),
        (S::Downloading | S::Seeding, E::SeenFailed(message)) => To(S::Error { message: message.clone() }),
        (S::Downloading | S::Seeding, E::FilesMissing) => To(S::MissingFiles),
        (S::Downloading | S::Seeding, E::DestinationGone { reason }) => unavailable(reason, true),
        (
            S::Downloading | S::Seeding,
            E::InfoArrived
            | E::FetchFailed(_)
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,

        // Paused and Done: stopped.
        (S::Paused | S::Done, E::Pause) => Ignore,
        (S::Paused, E::Resume) => To(S::Downloading),
        (S::Done, E::Resume) => To(S::Seeding),
        (S::Paused | S::Done, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        (S::Paused | S::Done, E::Check) => Decision::Recheck { paused: true },
        (S::Paused | S::Done, E::Move) => To(S::Moving { paused: true }),
        // A check cut short by a pause stays a check in librqbit until resumed.
        (S::Paused | S::Done, E::SeenChecking) => Ignore,
        (S::Paused, E::SeenStopped { complete: false }) | (S::Done, E::SeenStopped { complete: true }) => Ignore,
        // Every file set Off, or one set back to Normal.
        (S::Paused | S::Done, E::SeenStopped { complete }) => To(stopped(*complete)),
        // librqbit started it without being asked: stop it again.
        (S::Paused | S::Done, E::SeenRunning { complete }) => To(stopped(*complete)),
        (S::Paused | S::Done, E::SeenFailed(message)) => To(S::Error { message: message.clone() }),
        (S::Paused | S::Done, E::FilesMissing) => To(S::MissingFiles),
        (S::Paused | S::Done, E::DestinationGone { reason }) => unavailable(reason, false),
        (
            S::Paused | S::Done,
            E::InfoArrived
            | E::FetchFailed(_)
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,

        // Missing files: stopped until resumed or checked. librqbit still has the old files open,
        // so both add it again: it checks what's on disk, keeps what's valid and downloads the rest.
        (S::MissingFiles, E::Pause) => Ignore,
        (S::MissingFiles, E::Resume) => Decision::Recheck { paused: false },
        (S::MissingFiles, E::Check) => Decision::Recheck { paused: true },
        // Setting a missing file Off clears it.
        (S::MissingFiles, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        (S::MissingFiles, E::NoFilesMissing { complete }) => To(stopped(*complete)),
        // Put back: added again, stopped, so librqbit opens and checks them; it never resumes
        // by itself.
        (S::MissingFiles, E::FilesBack) => Decision::Recheck { paused: true },
        (S::MissingFiles, E::Move) => reject("is missing files"),
        (S::MissingFiles, E::DestinationGone { reason }) => unavailable(reason, false),
        // Stopped on purpose; librqbit failing on the missing files says nothing new.
        (
            S::MissingFiles,
            E::InfoArrived
            | E::FetchFailed(_)
            | E::SeenChecking
            | E::SeenStopped { .. }
            | E::SeenFailed(_)
            | E::FilesMissing
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,
        // librqbit started it without being asked: stop it again.
        (S::MissingFiles, E::SeenRunning { .. }) => To(S::MissingFiles),

        // Destination unavailable: stopped, and Resume waits for the destination. Pausing means it
        // stays stopped once it's back. Its files can't be deleted while they're away.
        (S::DestinationUnavailable { running: false, .. }, E::Pause) => Ignore,
        (S::DestinationUnavailable { reason, running: true }, E::Pause) => unavailable(reason, false),
        (S::DestinationUnavailable { reason, .. }, E::Resume) => away(Blocked::Start, reason),
        (S::DestinationUnavailable { reason, .. }, E::Move) => away(Blocked::Move, reason),
        (S::DestinationUnavailable { reason, .. }, E::Check) => away(Blocked::Check, reason),
        (S::DestinationUnavailable { reason, .. }, E::Delete) => away(Blocked::Delete, reason),
        (S::DestinationUnavailable { .. }, E::SetPriority | E::Details | E::Remove) => Allow,
        // A new reason replaces the old one.
        (S::DestinationUnavailable { reason: old, .. }, E::DestinationGone { reason }) if old == reason => Ignore,
        (S::DestinationUnavailable { running, .. }, E::DestinationGone { reason }) => unavailable(reason, *running),
        // Back: added again, so librqbit opens its files afresh and checks them, then it runs again
        // if it was running when its destination went.
        (S::DestinationUnavailable { running, .. }, E::DestinationBack) => Decision::Recheck { paused: !running },
        (
            S::DestinationUnavailable { .. },
            E::InfoArrived
            | E::FetchFailed(_)
            | E::SeenChecking
            | E::SeenStopped { .. }
            | E::SeenFailed(_)
            | E::FilesMissing
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,
        // librqbit started it without being asked: stop it again.
        (S::DestinationUnavailable { .. }, E::SeenRunning { .. }) => To(state.clone()),

        // Moving (#24): the mover owns the torrent until it's done.
        (S::Moving { .. }, E::Pause | E::Resume | E::SetPriority | E::Remove | E::Delete | E::Check | E::Move) => {
            reject(MOVING)
        }
        (S::Moving { .. }, E::Details) => Allow,
        // Back in librqbit, at the new destination or (failed) the old one: checked, then as before.
        (S::Moving { paused }, E::MoveFinished | E::MoveFailed) => To(S::Checking { paused: *paused }),
        (
            S::Moving { .. },
            E::InfoArrived
            | E::FetchFailed(_)
            | E::SeenChecking
            | E::SeenRunning { .. }
            | E::SeenStopped { .. }
            | E::SeenFailed(_)
            | E::FilesMissing
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationGone { .. }
            | E::DestinationBack,
        ) => Ignore,

        // Error: stopped by librqbit. Resume tries again, starting with a check of a fresh add
        // (which also works when an add again failed and librqbit no longer has it).
        (S::Error { .. }, E::Pause) => Ignore,
        (S::Error { .. }, E::Resume) => Decision::Recheck { paused: false },
        (S::Error { .. }, E::Check) => Decision::Recheck { paused: true },
        (S::Error { .. }, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        (S::Error { .. }, E::Move) => reject("has an error"),
        (S::Error { message: old }, E::SeenFailed(message)) if old == message => Ignore,
        (S::Error { .. }, E::SeenFailed(message)) => To(S::Error { message: message.clone() }),
        // librqbit is past the error.
        (S::Error { .. }, E::SeenChecking) => To(S::Checking { paused: false }),
        (S::Error { .. }, E::SeenRunning { complete }) => To(running(*complete)),
        (S::Error { .. }, E::SeenStopped { complete }) => To(stopped(*complete)),
        (S::Error { .. }, E::FilesMissing) => To(S::MissingFiles),
        (S::Error { .. }, E::DestinationGone { reason }) => unavailable(reason, false),
        (
            S::Error { .. },
            E::InfoArrived
            | E::FetchFailed(_)
            | E::NoFilesMissing { .. }
            | E::FilesBack
            | E::DestinationBack
            | E::MoveFinished
            | E::MoveFailed,
        ) => Ignore,
    }
}

#[cfg(test)]
mod tests {
    use proto::AbsolutePath;

    use super::Decision::{Allow, Ignore, To};
    use super::*;

    fn every_state() -> Vec<State> {
        vec![
            State::FetchingInfo { paused: false },
            State::FetchingInfo { paused: true },
            State::FetchFailed { message: "no peers".into() },
            State::Checking { paused: false },
            State::Checking { paused: true },
            State::Downloading,
            State::Seeding,
            State::Paused,
            State::Done,
            State::MissingFiles,
            State::DestinationUnavailable { reason: drive(), running: false },
            State::DestinationUnavailable { reason: drive(), running: true },
            State::Moving { paused: false },
            State::Moving { paused: true },
            State::Error { message: "disk full".into() },
        ]
    }

    fn commands() -> [Event; 8] {
        [Event::Pause, Event::Resume, Event::SetPriority, Event::Details, Event::Remove, Event::Delete, Event::Check, Event::Move]
    }

    fn observations() -> Vec<Event> {
        let mut seen = vec![
            Event::InfoArrived,
            Event::FetchFailed("no peers".into()),
            Event::SeenChecking,
            Event::SeenFailed("disk full".into()),
            Event::FilesMissing,
            Event::FilesBack,
            Event::DestinationGone { reason: drive() },
            Event::DestinationBack,
            Event::MoveFinished,
            Event::MoveFailed,
        ];
        for complete in [false, true] {
            seen.extend([
                Event::SeenRunning { complete },
                Event::SeenStopped { complete },
                Event::NoFilesMissing { complete },
            ]);
        }
        seen
    }

    fn drive() -> Unavailable {
        Unavailable::DriveNotConnected { name: "MyDisk".into() }
    }

    /// Every event: the commands and what the actor sees, with each message also the same as and
    /// other than the one in [`every_state`], since a repeated message may be decided differently.
    fn every_event() -> Vec<Event> {
        let mut events: Vec<Event> = commands().into();
        events.extend(observations());
        events.extend([
            Event::FetchFailed("timed out".into()),
            Event::SeenFailed("no space".into()),
            Event::DestinationGone { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() } },
        ]);
        events
    }

    /// The state a torrent is in once `decision` is carried out, as the actor does.
    fn after(state: &State, decision: &Decision) -> State {
        match decision {
            Decision::To(next) => next.clone(),
            Decision::Recheck { paused } => State::Checking { paused: *paused },
            Decision::Allow | Decision::Ignore | Decision::Reject(_) => state.clone(),
        }
    }

    /// librqbit has the torrent and its info: everything but fetching it.
    fn has_info(state: &State) -> bool {
        !matches!(state, State::FetchingInfo { .. } | State::FetchFailed { .. })
    }

    /// Stopped by the user (or added so), as opposed to stopped by what happened to it.
    fn stopped_on_purpose(state: &State) -> bool {
        matches!(
            state,
            State::FetchingInfo { paused: true }
                | State::Checking { paused: true }
                | State::Moving { paused: true }
                | State::Paused
                | State::Done
                | State::MissingFiles
                | State::DestinationUnavailable { running: false, .. }
        )
    }

    /// Every (state, event) pair, with its decision.
    fn every_pair() -> impl Iterator<Item = (State, Event, Decision)> {
        every_state().into_iter().flat_map(|state| {
            every_event().into_iter().map(move |event| {
                let decision = decide(&state, &event);
                (state.clone(), event, decision)
            })
        })
    }

    #[test]
    fn every_command_is_answered_and_a_refusal_says_why() {
        for (state, event, decision) in every_pair().filter(|(_, e, _)| commands().contains(e)) {
            if let Decision::Reject(Refusal::Because(reason)) = &decision {
                assert!(!reason.trim().is_empty(), "{event:?} in {state:?}: refused without a reason");
            }
        }
    }

    /// The pairs that carry the user's intent, which the properties alone don't pin: Resume
    /// re-checks only what librqbit can't just start, and Check keeps it stopped where it was.
    #[test]
    fn resume_and_check_where_it_matters() {
        use Decision::Recheck;
        assert_eq!(decide(&State::Paused, &Event::Resume), To(State::Downloading), "no check needed");
        assert_eq!(decide(&State::Done, &Event::Resume), To(State::Seeding));
        assert_eq!(decide(&State::MissingFiles, &Event::Resume), Recheck { paused: false }, "to download them again");
        assert_eq!(decide(&State::MissingFiles, &Event::Check), Recheck { paused: true });
        assert_eq!(decide(&State::Moving { paused: false }, &Event::Pause), r("is moving"));
        assert_eq!(decide(&State::FetchingInfo { paused: false }, &Event::Check), r("has no info yet"));
    }

    fn r(reason: &'static str) -> Decision {
        Decision::Reject(Refusal::Because(reason))
    }

    #[test]
    fn nothing_without_info_is_downloading_seeding_or_checked() {
        for (state, event, decision) in every_pair().filter(|(s, _, _)| !has_info(s)) {
            let next = after(&state, &decision);
            assert!(!matches!(decision, Decision::Recheck { .. } | Decision::To(State::Downloading | State::Seeding)), "{event:?} in {state:?}: {decision:?}");
            // Only its info arriving gives it info.
            if has_info(&next) {
                assert_eq!(event, Event::InfoArrived, "{state:?} to {next:?}");
            }
        }
    }

    #[test]
    fn a_command_given_twice_does_nothing_the_second_time() {
        for (state, event, decision) in every_pair().filter(|(_, e, _)| commands().contains(e)) {
            let next = after(&state, &decision);
            let again = decide(&next, &event);
            assert_eq!(after(&next, &again), next, "{event:?} twice from {state:?}: {decision:?}, then {again:?}");
            assert!(!matches!(again, Decision::Recheck { .. }), "{event:?} twice from {state:?}: checks again");
        }
    }

    #[test]
    fn details_priorities_remove_and_delete_never_change_the_state() {
        let leave_it = [Event::Details, Event::SetPriority, Event::Remove, Event::Delete];
        for (state, event, decision) in every_pair().filter(|(_, e, _)| leave_it.contains(e)) {
            assert!(matches!(decision, Allow | Decision::Reject(_)), "{event:?} in {state:?}: {decision:?}");
        }
    }

    #[test]
    fn pause_stops_resume_starts_and_check_keeps_which() {
        for state in every_state() {
            let paused = decide(&state, &Event::Pause);
            if !matches!(paused, Decision::Reject(_)) {
                assert!(!after(&state, &paused).is_running(), "Pause in {state:?}: {paused:?}");
            }
            let resumed = decide(&state, &Event::Resume);
            if !matches!(resumed, Decision::Reject(_)) {
                assert!(after(&state, &resumed).is_running(), "Resume in {state:?}: {resumed:?}");
            }
            let checked = decide(&state, &Event::Check);
            if let Decision::Recheck { paused } | Decision::To(State::Checking { paused }) = checked {
                assert_eq!(paused, !state.is_running(), "Check in {state:?}");
            }
        }
    }

    #[test]
    fn what_the_actor_sees_never_starts_a_torrent_stopped_on_purpose() {
        for (state, event, decision) in every_pair().filter(|(s, e, _)| stopped_on_purpose(s) && !commands().contains(e)) {
            let next = after(&state, &decision);
            assert!(!next.is_running(), "{event:?} in {state:?} starts it: {next:?}");
        }
    }

    #[test]
    fn what_the_actor_sees_only_stops_a_running_torrent_when_it_cannot_run() {
        for (state, event, decision) in every_pair().filter(|(s, e, _)| s.is_running() && !commands().contains(e)) {
            let next = after(&state, &decision);
            let cannot_run = matches!(next, State::MissingFiles | State::DestinationUnavailable { .. } | State::Error { .. });
            assert!(next.is_running() || cannot_run, "{event:?} in {state:?} stops it: {next:?}");
        }
    }

    #[test]
    fn a_status_follows_from_the_state_alone() {
        for (state, _, decision) in every_pair() {
            let next = after(&state, &decision);
            assert_eq!(next.message().is_some(), next.status() == Status::Error, "{next:?}");
        }
    }

    #[test]
    fn what_the_actor_sees_is_never_refused() {
        for state in every_state() {
            for event in observations() {
                let decision = decide(&state, &event);
                assert!(!matches!(decision, Decision::Reject(_)), "{event:?} in {state:?}: {decision:?}");
                assert_ne!(decision, Allow, "{event:?} in {state:?}: Allow is for commands");
                if !matches!(event, Event::FilesBack | Event::DestinationBack) {
                    assert!(
                        !matches!(decision, Decision::Recheck { .. }),
                        "{event:?} in {state:?}: only a client or files coming back re-add"
                    );
                }
            }
        }
    }

    #[test]
    fn a_torrent_goes_from_fetching_info_through_checking_to_downloading_or_seeding() {
        let checking = decide(&State::FetchingInfo { paused: false }, &Event::InfoArrived);
        assert_eq!(checking, To(State::Checking { paused: false }));
        let s = State::Checking { paused: false };
        assert_eq!(decide(&s, &Event::SeenChecking), Ignore, "still checking");
        assert_eq!(decide(&s, &Event::SeenRunning { complete: false }), To(State::Downloading));
        assert_eq!(decide(&s, &Event::SeenRunning { complete: true }), To(State::Seeding));
        assert_eq!(
            decide(&s, &Event::SeenStopped { complete: true }),
            To(State::Seeding),
            "librqbit left it paused, but it should run"
        );
        assert_eq!(decide(&State::Downloading, &Event::SeenRunning { complete: true }), To(State::Seeding));
        assert_eq!(decide(&State::Seeding, &Event::SeenRunning { complete: false }), To(State::Downloading));
    }

    #[test]
    fn a_torrent_added_paused_stops_after_its_check() {
        let checking = decide(&State::FetchingInfo { paused: true }, &Event::InfoArrived);
        assert_eq!(checking, To(State::Checking { paused: true }));
        let s = State::Checking { paused: true };
        assert_eq!(decide(&s, &Event::SeenStopped { complete: false }), To(State::Paused));
        assert_eq!(decide(&s, &Event::SeenStopped { complete: true }), To(State::Done));
        assert_eq!(decide(&s, &Event::SeenRunning { complete: true }), To(State::Done), "and stops librqbit");
    }

    #[test]
    fn stopped_torrents_follow_their_completeness_and_stay_stopped() {
        assert_eq!(decide(&State::Paused, &Event::SeenStopped { complete: true }), To(State::Done));
        assert_eq!(decide(&State::Done, &Event::SeenStopped { complete: false }), To(State::Paused));
        assert_eq!(decide(&State::Paused, &Event::SeenRunning { complete: false }), To(State::Paused));
        assert_eq!(decide(&State::Paused, &Event::SeenChecking), Ignore, "a check cut short by a pause");
    }

    #[test]
    fn failing_fetching_info_or_in_librqbit_is_an_error() {
        let failed = decide(&State::FetchingInfo { paused: false }, &Event::FetchFailed("no peers".into()));
        assert_eq!(failed, To(State::FetchFailed { message: "no peers".into() }));
        assert_eq!(State::FetchFailed { message: "no peers".into() }.status(), Status::Error);
        let error = decide(&State::Downloading, &Event::SeenFailed("disk full".into()));
        assert_eq!(error, To(State::Error { message: "disk full".into() }));
        let same = State::Error { message: "disk full".into() };
        assert_eq!(decide(&same, &Event::SeenFailed("disk full".into())), Ignore);
        assert_eq!(decide(&same, &Event::SeenStopped { complete: false }), To(State::Paused), "recovered");
    }

    #[test]
    fn missing_files_stop_a_torrent_with_its_files_until_resumed_or_checked() {
        for state in [State::Downloading, State::Seeding, State::Paused, State::Done, State::Error { message: "x".into() }] {
            assert_eq!(decide(&state, &Event::FilesMissing), To(State::MissingFiles), "{state:?}");
        }
        assert_eq!(decide(&State::Checking { paused: false }, &Event::FilesMissing), Ignore, "the check sees for itself");
        let missing = State::MissingFiles;
        assert_eq!(decide(&missing, &Event::FilesMissing), Ignore, "more missing files");
        assert_eq!(decide(&missing, &Event::SeenStopped { complete: true }), Ignore, "librqbit doesn't know");
        assert_eq!(decide(&missing, &Event::SeenRunning { complete: true }), To(State::MissingFiles), "stopped again");
        assert_eq!(decide(&missing, &Event::Resume), Decision::Recheck { paused: false }, "added again, to download them");
        assert_eq!(decide(&missing, &Event::Check), Decision::Recheck { paused: true }, "checked, staying stopped");
        assert_eq!(missing.status(), Status::MissingFiles);
        assert!(!missing.is_running());
    }

    #[test]
    fn setting_the_missing_files_off_stops_the_torrent_as_paused_or_done() {
        let missing = State::MissingFiles;
        assert_eq!(decide(&missing, &Event::NoFilesMissing { complete: false }), To(State::Paused));
        assert_eq!(decide(&missing, &Event::NoFilesMissing { complete: true }), To(State::Done));
        for state in every_state().into_iter().filter(|s| *s != missing) {
            assert_eq!(decide(&state, &Event::NoFilesMissing { complete: true }), Ignore, "{state:?}");
        }
    }

    #[test]
    fn missing_files_put_back_are_checked_and_the_torrent_stays_stopped() {
        assert_eq!(decide(&State::MissingFiles, &Event::FilesBack), Decision::Recheck { paused: true });
        for state in every_state().into_iter().filter(|s| *s != State::MissingFiles) {
            assert_eq!(decide(&state, &Event::FilesBack), Ignore, "{state:?}");
        }
    }

    #[test]
    fn check_adds_the_torrent_again_keeping_it_running_or_stopped() {
        assert_eq!(decide(&State::Seeding, &Event::Check), Decision::Recheck { paused: false });
        assert_eq!(decide(&State::Done, &Event::Check), Decision::Recheck { paused: true });
        assert_eq!(decide(&State::Checking { paused: false }, &Event::Check), Ignore, "already checking");
        // After the add, the check ends as any other.
        assert_eq!(decide(&State::Checking { paused: true }, &Event::SeenStopped { complete: false }), To(State::Paused));
    }

    #[test]
    fn a_torrent_whose_destination_goes_remembers_whether_it_was_running() {
        let gone = Event::DestinationGone { reason: drive() };
        let away = |running| State::DestinationUnavailable { reason: drive(), running };
        for state in [State::Downloading, State::Seeding, State::Checking { paused: false }] {
            assert_eq!(decide(&state, &gone), To(away(true)), "{state:?}");
        }
        for state in [State::Paused, State::Done, State::Checking { paused: true }, State::MissingFiles, State::Error { message: "x".into() }] {
            assert_eq!(decide(&state, &gone), To(away(false)), "{state:?}");
        }
        for state in [State::FetchingInfo { paused: false }, State::Moving { paused: false }] {
            assert_eq!(decide(&state, &gone), Ignore, "{state:?}: no files in librqbit yet, or the mover's");
        }
        assert_eq!(decide(&away(true), &gone), Ignore, "the same reason");
        let other = Event::DestinationGone { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() } };
        let replaced = State::DestinationUnavailable { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() }, running: true };
        assert_eq!(decide(&away(true), &other), To(replaced), "a new reason, still to run");
        assert_eq!(decide(&away(true), &Event::SeenRunning { complete: false }), To(away(true)), "stopped again");
        assert_eq!(away(true).status(), Status::DestinationUnavailable { reason: drive() });
        assert!(!away(true).is_running());
        assert!(away(true).wants_to_run() && !away(false).wants_to_run(), "recorded at shutdown");
    }

    #[test]
    fn a_destination_back_checks_the_torrent_then_runs_it_only_if_it_was_running() {
        let away = |running| State::DestinationUnavailable { reason: drive(), running };
        assert_eq!(decide(&away(true), &Event::DestinationBack), Decision::Recheck { paused: false });
        assert_eq!(decide(&away(false), &Event::DestinationBack), Decision::Recheck { paused: true });
        assert_eq!(decide(&away(true), &Event::Pause), To(away(false)), "paused while away: stays paused once back");
        for state in every_state().into_iter().filter(|s| !matches!(s, State::DestinationUnavailable { .. })) {
            assert_eq!(decide(&state, &Event::DestinationBack), Ignore, "{state:?}");
        }
    }

    #[test]
    fn moving_hooks() {
        assert_eq!(decide(&State::Moving { paused: true }, &Event::MoveFinished), To(State::Checking { paused: true }));
        assert_eq!(decide(&State::Moving { paused: false }, &Event::MoveFailed), To(State::Checking { paused: false }));
    }

    #[test]
    fn running_is_what_resumes_after_a_restart() {
        let running: Vec<State> = every_state().into_iter().filter(State::is_running).collect();
        assert_eq!(running, [State::Checking { paused: false }, State::Downloading, State::Seeding]);
    }
}
