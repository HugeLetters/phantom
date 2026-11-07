//! A torrent's lifecycle as a plain-enum state machine ([ADR 0006]): [`decide`] says what an
//! [`Event`] does to a torrent in a given [`State`]: a transition, an allowed command that
//! doesn't change the state, a harmless no-op, or a refusal with a reason for the client. Pure:
//! the actor in `actor.rs` carries out the decision, entering the new state by making librqbit
//! match it.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

use proto::Status;

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
    /// Its destination can't be reached; stopped in librqbit. Entered by #23.
    DestinationUnavailable { reason: String },
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

    // From the file system: sent by the file checks of `files.rs` and the destination checks of #23.
    /// A file it's getting is gone from disk or has a size it can't have.
    FilesMissing,
    /// None of the files it's getting is missing any more (those that were are now Off);
    /// `complete`: it has every file it's getting.
    NoFilesMissing { complete: bool },
    /// The missing files are back on disk; a check tells whether they're what they were.
    FilesBack,
    #[allow(dead_code, reason = "sent by the destination checks (#23)")]
    DestinationGone { reason: String },
    #[allow(dead_code, reason = "sent by the destination checks (#23)")]
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
    Reject(String),
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
            State::DestinationUnavailable { reason } => Status::DestinationUnavailable { reason: reason.clone() },
            State::Moving { .. } => Status::Moving,
            State::FetchFailed { .. } | State::Error { .. } => Status::Error,
        }
    }

    /// Running, or about to: what Resume asks for. Recorded at shutdown to resume on start.
    pub fn is_running(&self) -> bool {
        matches!(self, State::Downloading | State::Seeding | State::Checking { paused: false })
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

fn reject(reason: &str) -> Decision {
    Decision::Reject(reason.into())
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
        (S::Checking { .. }, E::DestinationGone { reason }) => To(S::DestinationUnavailable { reason: reason.clone() }),
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
        (S::Downloading | S::Seeding, E::DestinationGone { reason }) => {
            To(S::DestinationUnavailable { reason: reason.clone() })
        }
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
        (S::Paused | S::Done, E::DestinationGone { reason }) => To(S::DestinationUnavailable { reason: reason.clone() }),
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
        (S::MissingFiles, E::DestinationGone { reason }) => To(S::DestinationUnavailable { reason: reason.clone() }),
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

        // Destination unavailable (#23): stopped, and Resume waits for the destination.
        (S::DestinationUnavailable { .. }, E::Pause) => Ignore,
        (S::DestinationUnavailable { reason }, E::Resume | E::Move) => {
            Decision::Reject(format!("can't start: its destination is unavailable ({reason})"))
        }
        (S::DestinationUnavailable { reason }, E::Check) => {
            Decision::Reject(format!("can't be checked: its destination is unavailable ({reason})"))
        }
        (S::DestinationUnavailable { .. }, E::SetPriority | E::Details | E::Remove | E::Delete) => Allow,
        // A new reason replaces the old one.
        (S::DestinationUnavailable { reason: old }, E::DestinationGone { reason }) if old == reason => Ignore,
        (S::DestinationUnavailable { .. }, E::DestinationGone { reason }) => {
            To(S::DestinationUnavailable { reason: reason.clone() })
        }
        // Back: stopped until resumed (#23 may check it first).
        (S::DestinationUnavailable { .. }, E::DestinationBack) => To(S::Paused),
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
        (S::DestinationUnavailable { reason }, E::SeenRunning { .. }) => {
            To(S::DestinationUnavailable { reason: reason.clone() })
        }

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
        (S::Error { .. }, E::DestinationGone { reason }) => To(S::DestinationUnavailable { reason: reason.clone() }),
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
            State::DestinationUnavailable { reason: "drive not connected".into() },
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
            Event::DestinationGone { reason: "drive not connected".into() },
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

    fn r(reason: &str) -> Decision {
        Decision::Reject(reason.into())
    }

    /// Every state × command, written out: what a client gets for each.
    #[test]
    fn every_command_in_every_state() {
        use Decision::Recheck;
        use State as S;
        let unavailable = "can't start: its destination is unavailable (drive not connected)";
        let unavailable_check = "can't be checked: its destination is unavailable (drive not connected)";
        let no_info = "has no info: fetching it failed";
        #[rustfmt::skip]
        let table: Vec<(State, [Decision; 8])> = vec![
            //                                     Pause                 Resume                                SetPriority          Details              Remove               Delete               Check                              Move
            (S::FetchingInfo { paused: false },  [r("has no info yet"), r("has no info yet"),                   r("has no info yet"), r("has no info yet"), Allow,               Allow,               r("has no info yet"),              r("has no info yet")]),
            (S::FetchingInfo { paused: true },   [r("has no info yet"), r("has no info yet"),                   r("has no info yet"), r("has no info yet"), Allow,               Allow,               r("has no info yet"),              r("has no info yet")]),
            (S::FetchFailed { message: "no peers".into() },
                                                 [r(no_info),           r(no_info),                             r(no_info),          r(no_info),          Allow,               Allow,               r(no_info),                        r(no_info)]),
            (S::Checking { paused: false },      [To(S::Paused),         Ignore,                               Allow,               Allow,               Allow,               Allow,               Ignore,                            r("is being checked")]),
            (S::Checking { paused: true },       [To(S::Paused),         To(S::Checking { paused: false }),    Allow,               Allow,               Allow,               Allow,               Ignore,                            r("is being checked")]),
            (S::Downloading,                     [To(S::Paused),         Ignore,                               Allow,               Allow,               Allow,               Allow,               Recheck { paused: false },         To(S::Moving { paused: false })]),
            (S::Seeding,                         [To(S::Done),           Ignore,                               Allow,               Allow,               Allow,               Allow,               Recheck { paused: false },         To(S::Moving { paused: false })]),
            (S::Paused,                          [Ignore,                To(S::Downloading),                   Allow,               Allow,               Allow,               Allow,               Recheck { paused: true },          To(S::Moving { paused: true })]),
            (S::Done,                            [Ignore,                To(S::Seeding),                       Allow,               Allow,               Allow,               Allow,               Recheck { paused: true },          To(S::Moving { paused: true })]),
            (S::MissingFiles,                    [Ignore,                Recheck { paused: false },            Allow,               Allow,               Allow,               Allow,               Recheck { paused: true },          r("is missing files")]),
            (S::DestinationUnavailable { reason: "drive not connected".into() },
                                                 [Ignore,                r(unavailable),                       Allow,               Allow,               Allow,               Allow,               r(unavailable_check),              r(unavailable)]),
            (S::Moving { paused: false },        [r("is moving"),        r("is moving"),                       r("is moving"),      Allow,               r("is moving"),      r("is moving"),      r("is moving"),                    r("is moving")]),
            (S::Moving { paused: true },         [r("is moving"),        r("is moving"),                       r("is moving"),      Allow,               r("is moving"),      r("is moving"),      r("is moving"),                    r("is moving")]),
            (S::Error { message: "disk full".into() },
                                                 [Ignore,                Recheck { paused: false },            Allow,               Allow,               Allow,               Allow,               Recheck { paused: true },          r("has an error")]),
        ];
        assert_eq!(table.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(), every_state(), "every state, in order");
        for (state, expected) in table {
            for (command, expected) in commands().iter().zip(expected) {
                assert_eq!(decide(&state, command), expected, "{command:?} in {state:?}");
            }
        }
    }

    #[test]
    fn what_the_actor_sees_is_never_refused() {
        for state in every_state() {
            for event in observations() {
                let decision = decide(&state, &event);
                assert!(!matches!(decision, Decision::Reject(_)), "{event:?} in {state:?}: {decision:?}");
                assert_ne!(decision, Allow, "{event:?} in {state:?}: Allow is for commands");
                if event != Event::FilesBack {
                    assert!(!matches!(decision, Decision::Recheck { .. }), "{event:?} in {state:?}: only a client re-adds");
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
    fn destination_and_moving_hooks() {
        let gone = Event::DestinationGone { reason: "drive not connected".into() };
        let unavailable = State::DestinationUnavailable { reason: "drive not connected".into() };
        assert_eq!(decide(&State::MissingFiles, &gone), To(unavailable.clone()));
        assert_eq!(decide(&State::Done, &gone), To(unavailable.clone()));
        assert_eq!(decide(&unavailable, &gone), Ignore);
        assert_eq!(decide(&unavailable, &Event::DestinationBack), To(State::Paused));
        assert_eq!(decide(&State::Moving { paused: true }, &Event::MoveFinished), To(State::Checking { paused: true }));
        assert_eq!(decide(&State::Moving { paused: false }, &Event::MoveFailed), To(State::Checking { paused: false }));
        assert_eq!(unavailable.status(), Status::DestinationUnavailable { reason: "drive not connected".into() });
    }

    #[test]
    fn running_is_what_resumes_after_a_restart() {
        let running: Vec<State> = every_state().into_iter().filter(State::is_running).collect();
        assert_eq!(running, [State::Checking { paused: false }, State::Downloading, State::Seeding]);
    }
}
