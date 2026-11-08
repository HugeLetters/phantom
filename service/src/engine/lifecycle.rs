//! A torrent's lifecycle ([ADR 0006]): what the user wants of it ([`Wants`]) and what it's doing
//! ([`Phase`]); its Status follows from the two and whether it's complete. [`decide`] says what an
//! [`Event`] does: a [`Command`] from a client sets what's wanted (and may start a check), or is
//! refused with a reason; an [`Observation`] of librqbit, the files or the mover changes only the
//! phase, and is never refused. [`reconcile`] says what makes librqbit match. Pure: the actor in
//! `actor.rs` carries both out.
//!
//! [ADR 0006]: ../../../docs/adr/0006-torrent-lifecycle-as-an-actor.md

use std::collections::BTreeMap;

use proto::{Action, Status, Unavailable};
use serde::{Deserialize, Serialize};

use super::files::Gone;

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
    /// librqbit is checking its files. `returning`: missing files put back, which the check
    /// tells are what they were or not.
    Checking { returning: BTreeMap<usize, Gone> },
    /// In librqbit, running or stopped as wanted.
    Ready,
    /// Files it's getting (by index) are gone from disk; stopped in librqbit, which still holds the
    /// old files open.
    MissingFiles { gone: BTreeMap<usize, Gone> },
    /// Its destination can't be reached; stopped in librqbit.
    DestinationUnavailable { reason: Unavailable },
    /// Its files are moving to a new destination; out of librqbit meanwhile. Entered by #24.
    Moving,
    /// librqbit stopped it with an error, or adding it again failed.
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lifecycle {
    pub wants: Wants,
    pub phase: Phase,
}

/// Something that happens to a torrent, by where it comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Command(Command),
    Observation(Observation),
}

/// What a client asks of a torrent. Their arguments (which files, which priority) don't change
/// the decision, so the actor keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    SetPriority,
    Remove,
    Delete,
    /// Verify the files on disk, keeping what's valid.
    Check,
    /// Move the torrent's files to a new destination. No client asks for it until #24.
    #[allow(dead_code, reason = "sent once moving exists (#24)")]
    Move,
}

/// What the actor sees happen to a torrent.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// librqbit has the torrent and its info; `checking`: it's checking the files.
    InfoArrived { checking: bool },
    FetchFailed(String),
    /// librqbit is checking the files.
    Checking,
    /// librqbit is running it or has it stopped. `not_back`: of the missing files put back, those
    /// the check that just ended found aren't what they were.
    Idle { not_back: BTreeMap<usize, Gone> },
    /// librqbit stopped it with an error.
    Failed(String),
    /// Files it's getting are gone from disk (all of them, for one already Missing files).
    FilesMissing { gone: BTreeMap<usize, Gone> },
    /// None of the files it's getting is missing any more: those that were are now Off.
    NoFilesMissing,
    /// Every missing file is back on disk.
    FilesBack,
    DestinationGone { reason: Unavailable },
    DestinationBack,
    #[allow(dead_code, reason = "sent by the mover (#24)")]
    MoveFinished,
}

/// Why a command is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Because(&'static str),
    /// Its destination can't be reached, so it can't do `action`.
    DestinationUnavailable { action: Command, reason: Unavailable },
}

impl Command {
    /// The client's request it is.
    pub fn action(self) -> Action {
        match self {
            Command::Pause => Action::Pause,
            Command::Resume => Action::Resume,
            Command::SetPriority => Action::SetPriority,
            Command::Remove => Action::Remove,
            Command::Delete => Action::Delete,
            Command::Check => Action::Check,
            Command::Move => unreachable!("no client moves a torrent until #24"),
        }
    }
}

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

/// What makes librqbit match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Start,
    Stop,
    /// Take it out of librqbit and add it again, stopped, so it opens the files afresh and checks
    /// them: librqbit can't check a torrent it has.
    AddAgain,
}

impl Lifecycle {
    pub fn new(wants: Wants, phase: Phase) -> Self {
        Self { wants, phase }
    }

    /// What clients see. `complete`: it has every file it's getting.
    pub fn status(&self, complete: bool) -> Status {
        match (&self.phase, self.wants) {
            (Phase::FetchingInfo, _) => Status::FetchingInfo,
            (Phase::Checking { .. }, _) => Status::Checking,
            (Phase::Ready, Wants::Run) if complete => Status::Seeding,
            (Phase::Ready, Wants::Run) => Status::Downloading,
            (Phase::Ready, Wants::Stop) if complete => Status::Done,
            (Phase::Ready, Wants::Stop) => Status::Paused,
            (Phase::MissingFiles { .. }, _) => Status::MissingFiles,
            (Phase::DestinationUnavailable { reason }, _) => Status::DestinationUnavailable { reason: reason.clone() },
            (Phase::Moving, _) => Status::Moving,
            (Phase::FetchFailed { .. } | Phase::Error { .. }, _) => Status::Error,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match &self.phase {
            Phase::FetchFailed { message } | Phase::Error { message } => Some(message),
            _ => None,
        }
    }

    /// The commands that do something now: those not refused, less Pause, Resume and Check where
    /// they'd change nothing.
    pub fn actions(&self) -> Vec<Action> {
        use Command as C;
        [
            (C::Pause, Action::Pause),
            (C::Resume, Action::Resume),
            (C::Check, Action::Check),
            (C::SetPriority, Action::SetPriority),
            (C::Remove, Action::Remove),
            (C::Delete, Action::Delete),
        ]
        .into_iter()
        .filter(|(command, _)| match command_on(self, *command) {
            Ok(next) => next != *self || matches!(command, C::SetPriority | C::Remove | C::Delete),
            Err(_) => false,
        })
        .map(|(_, action)| action)
        .collect()
    }
}

/// What `event` does to torrent `t`: what it becomes, or why a command is refused.
pub fn decide(t: &Lifecycle, event: &Event) -> Result<Lifecycle, Refusal> {
    match event {
        Event::Command(command) => command_on(t, *command),
        Event::Observation(seen) => Ok(Lifecycle { wants: t.wants, phase: observe(&t.phase, seen) }),
    }
}

fn checking() -> Phase {
    Phase::Checking { returning: BTreeMap::new() }
}

fn command_on(t: &Lifecycle, command: Command) -> Result<Lifecycle, Refusal> {
    use Command as C;
    use Phase as P;
    use Refusal::Because;
    let to = |wants, phase| Ok(Lifecycle { wants, phase });
    let unavailable = |reason: &Unavailable| Err(Refusal::DestinationUnavailable { action: command, reason: reason.clone() });
    match (&t.phase, command) {
        // librqbit doesn't have it: only Remove and Delete apply, and stop fetching.
        (P::FetchingInfo | P::FetchFailed { .. }, C::Remove | C::Delete) => Ok(t.clone()),
        (P::FetchingInfo, _) => Err(Because("has no info yet")),
        (P::FetchFailed { .. }, _) => Err(Because("has no info: fetching it failed")),
        // The mover has it until it's done.
        (P::Moving, _) => Err(Because("is moving")),
        // Its files are out of reach; once they're back it runs again if it's wanted to.
        (P::DestinationUnavailable { reason }, C::Resume | C::Check | C::Delete | C::Move) => unavailable(reason),

        (_, C::Pause) => to(Wants::Stop, t.phase.clone()),
        // A check is the way back into librqbit: it opens the files afresh, keeps what's valid,
        // and then it downloads the rest.
        (P::MissingFiles { .. } | P::Error { .. }, C::Resume) => to(Wants::Run, checking()),
        (_, C::Resume) => to(Wants::Run, t.phase.clone()),
        (P::Checking { .. }, C::Check) => Ok(t.clone()),
        (P::Ready | P::MissingFiles { .. } | P::Error { .. }, C::Check) => to(t.wants, checking()),
        (P::Ready, C::Move) => to(t.wants, P::Moving),
        (P::Checking { .. }, C::Move) => Err(Because("is being checked")),
        (P::MissingFiles { .. }, C::Move) => Err(Because("is missing files")),
        (P::Error { .. }, C::Move) => Err(Because("has an error")),
        // The actor does these; a priority change during a check is held until it ends.
        (_, C::SetPriority | C::Remove | C::Delete) => Ok(t.clone()),
    }
}

/// The phase a torrent in `phase` is in once `seen`; anything not listed leaves it as it is.
fn observe(phase: &Phase, seen: &Observation) -> Phase {
    use Observation as O;
    use Phase as P;
    match (phase, seen) {
        (P::FetchingInfo, O::InfoArrived { checking: true }) => checking(),
        (P::FetchingInfo, O::InfoArrived { checking: false }) => P::Ready,
        (P::FetchingInfo, O::FetchFailed(message)) => P::FetchFailed { message: message.clone() },

        // librqbit checks again by itself, or is past an error.
        (P::Ready | P::Error { .. }, O::Checking) => checking(),
        (P::Error { .. }, O::Idle { .. }) => P::Ready,
        // A check ends: files put back that aren't what they were are missing again.
        (P::Checking { .. }, O::Idle { not_back }) if not_back.is_empty() => P::Ready,
        (P::Checking { .. }, O::Idle { not_back }) => P::MissingFiles { gone: not_back.clone() },
        (P::Checking { .. } | P::Ready | P::Error { .. }, O::Failed(message)) => P::Error { message: message.clone() },

        // A check finds missing files for itself; they're caught once it ends.
        (P::Ready | P::MissingFiles { .. } | P::Error { .. }, O::FilesMissing { gone }) => P::MissingFiles { gone: gone.clone() },
        (P::MissingFiles { .. }, O::NoFilesMissing) => P::Ready,
        (P::MissingFiles { gone }, O::FilesBack) => P::Checking { returning: gone.clone() },

        // What was missing is out of reach too: it's checked once the destination is back.
        (P::Checking { .. } | P::Ready | P::MissingFiles { .. } | P::Error { .. } | P::DestinationUnavailable { .. }, O::DestinationGone { reason }) => {
            P::DestinationUnavailable { reason: reason.clone() }
        }
        (P::DestinationUnavailable { .. }, O::DestinationBack) => checking(),

        // Back in librqbit, at the new destination or (failed) the old one.
        (P::Moving, O::MoveFinished) => checking(),
        _ => phase.clone(),
    }
}

/// What makes librqbit, doing `lib`, match `t`. A check is entered by adding the torrent again:
/// librqbit isn't checking it yet. Nothing interrupts a check.
pub fn reconcile(t: &Lifecycle, lib: Lib) -> Option<Do> {
    use Phase as P;
    match (&t.phase, t.wants, lib) {
        (P::Checking { .. }, _, Lib::Running | Lib::Stopped | Lib::Failed | Lib::Absent) => Some(Do::AddAgain),
        (P::Ready, Wants::Run, Lib::Stopped) => Some(Do::Start),
        (P::Ready, Wants::Stop, Lib::Running) => Some(Do::Stop),
        // Nothing more is written into deleted files, nor where the destination was.
        (P::MissingFiles { .. } | P::DestinationUnavailable { .. }, _, Lib::Running) => Some(Do::Stop),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use proto::AbsolutePath;

    use super::*;
    use std::path::PathBuf;

    use super::super::files::Seen;

    fn gone() -> BTreeMap<usize, Gone> {
        let file = Gone { path: PathBuf::from("/x/a"), size_bytes: 10, progress_bytes: 5, seen: Seen::Was(None) };
        [(0, file)].into()
    }

    fn drive() -> Unavailable {
        Unavailable::DriveNotConnected { name: "MyDisk".into() }
    }

    fn every_phase() -> Vec<Phase> {
        vec![
            Phase::FetchingInfo,
            Phase::FetchFailed { message: "no peers".into() },
            checking(),
            Phase::Checking { returning: gone() },
            Phase::Ready,
            Phase::MissingFiles { gone: gone() },
            Phase::DestinationUnavailable { reason: drive() },
            Phase::Moving,
            Phase::Error { message: "disk full".into() },
        ]
    }

    fn every_torrent() -> Vec<Lifecycle> {
        every_phase()
            .into_iter()
            .flat_map(|phase| [Wants::Run, Wants::Stop].map(|wants| Lifecycle::new(wants, phase.clone())))
            .collect()
    }

    fn commands() -> [Command; 7] {
        use Command::*;
        [Pause, Resume, SetPriority, Remove, Delete, Check, Move]
    }

    /// Every observation, each message also the same as and other than the one in
    /// [`every_phase`], since a repeated message may be decided differently.
    fn observations() -> Vec<Observation> {
        use Observation::*;
        vec![
            InfoArrived { checking: true },
            InfoArrived { checking: false },
            FetchFailed("no peers".into()),
            Checking,
            Idle { not_back: BTreeMap::new() },
            Idle { not_back: gone() },
            Failed("disk full".into()),
            Failed("no space".into()),
            FilesMissing { gone: gone() },
            NoFilesMissing,
            FilesBack,
            DestinationGone { reason: drive() },
            DestinationGone { reason: Unavailable::NoPermission { path: AbsolutePath::new("/x").unwrap() } },
            DestinationBack,
            MoveFinished,
        ]
    }

    fn every_lib() -> [Lib; 5] {
        [Lib::Checking, Lib::Running, Lib::Stopped, Lib::Failed, Lib::Absent]
    }

    fn command(t: &Lifecycle, c: Command) -> Result<Lifecycle, Refusal> {
        decide(t, &Event::Command(c))
    }

    fn has_info(phase: &Phase) -> bool {
        !matches!(phase, Phase::FetchingInfo | Phase::FetchFailed { .. })
    }

    // Properties over every torrent and event.

    #[test]
    fn a_refusal_says_why_and_changes_nothing_else() {
        for t in every_torrent() {
            for c in commands() {
                if let Err(Refusal::Because(reason)) = command(&t, c) {
                    assert!(!reason.trim().is_empty(), "{c:?} on {t:?}");
                }
            }
        }
    }

    #[test]
    fn pause_and_resume_set_what_is_wanted_and_nothing_else_does() {
        for t in every_torrent() {
            for c in commands() {
                let Ok(next) = command(&t, c) else { continue };
                let wants = match c {
                    Command::Pause => Wants::Stop,
                    Command::Resume => Wants::Run,
                    _ => t.wants,
                };
                assert_eq!(next.wants, wants, "{c:?} on {t:?}");
            }
        }
    }

    #[test]
    fn a_command_given_twice_does_nothing_the_second_time() {
        for t in every_torrent() {
            for c in commands() {
                let Ok(next) = command(&t, c) else { continue };
                // Ok and the same, or refused (Move while moving).
                assert!(command(&next, c).map_or(true, |again| again == next), "{c:?} twice on {t:?}");
            }
        }
    }

    #[test]
    fn priorities_remove_and_delete_never_change_the_torrent() {
        for t in every_torrent() {
            for c in [Command::SetPriority, Command::Remove, Command::Delete] {
                assert!(matches!(command(&t, c), Ok(next) if next == t) || command(&t, c).is_err(), "{c:?} on {t:?}");
            }
        }
    }

    #[test]
    fn nothing_without_info_is_ready_or_checked_until_its_info_arrives() {
        for t in every_torrent().into_iter().filter(|t| !has_info(&t.phase)) {
            for c in commands() {
                assert!(command(&t, c).is_ok_and(|next| next.phase == t.phase) || command(&t, c).is_err(), "{c:?} on {t:?}");
            }
            for seen in observations() {
                let next = observe(&t.phase, &seen);
                if has_info(&next) {
                    assert!(matches!(seen, Observation::InfoArrived { .. }), "{seen:?} on {t:?}");
                }
            }
        }
    }

    #[test]
    fn what_the_actor_sees_never_moves_a_torrent_back_to_fetching_info() {
        for t in every_torrent().into_iter().filter(|t| has_info(&t.phase)) {
            for seen in observations() {
                assert!(has_info(&observe(&t.phase, &seen)), "{seen:?} on {t:?}");
            }
        }
    }

    #[test]
    fn a_status_follows_from_the_torrent_alone() {
        for t in every_torrent() {
            for complete in [false, true] {
                assert_eq!(t.message().is_some(), t.status(complete) == Status::Error, "{t:?}");
            }
        }
    }

    #[test]
    fn a_check_is_only_entered_by_adding_again_unless_librqbit_is_checking() {
        for t in every_torrent().into_iter().filter(|t| matches!(t.phase, Phase::Checking { .. })) {
            for lib in every_lib() {
                let expected = if lib == Lib::Checking { None } else { Some(Do::AddAgain) };
                assert_eq!(reconcile(&t, lib), expected, "{t:?} with librqbit {lib:?}");
            }
        }
    }

    #[test]
    fn librqbit_is_started_only_for_a_ready_torrent_wanted_running_and_stopped_where_it_must_not_run() {
        for t in every_torrent() {
            let ready_to_run = (&t.phase, t.wants) == (&Phase::Ready, Wants::Run);
            for lib in every_lib() {
                let todo = reconcile(&t, lib);
                if todo == Some(Do::Start) {
                    assert!(ready_to_run, "{t:?} with librqbit {lib:?}");
                }
                let must_stop = matches!(t.phase, Phase::Ready | Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. }) && !ready_to_run;
                if must_stop && lib == Lib::Running {
                    assert_eq!(todo, Some(Do::Stop), "{t:?}");
                }
            }
        }
    }


    #[test]
    fn actions_are_the_commands_that_do_something() {
        for t in every_torrent() {
            let actions = t.actions();
            assert_eq!(actions.contains(&Action::Pause), t.wants == Wants::Run && command(&t, Command::Pause).is_ok(), "{t:?}");
            assert_eq!(actions.contains(&Action::Delete), command(&t, Command::Delete).is_ok(), "{t:?}");
        }
    }

    // The pairs that carry intent, which the properties alone don't pin.

    #[test]
    fn resume_checks_only_what_librqbit_cannot_just_start() {
        let stopped = |phase| Lifecycle::new(Wants::Stop, phase);
        assert_eq!(command(&stopped(Phase::Ready), Command::Resume), Ok(Lifecycle::new(Wants::Run, Phase::Ready)));
        let missing = stopped(Phase::MissingFiles { gone: gone() });
        assert_eq!(command(&missing, Command::Resume), Ok(Lifecycle::new(Wants::Run, checking())), "to download them again");
        let error = stopped(Phase::Error { message: "x".into() });
        assert_eq!(command(&error, Command::Resume), Ok(Lifecycle::new(Wants::Run, checking())));
    }

    #[test]
    fn check_keeps_it_running_or_stopped() {
        for wants in [Wants::Run, Wants::Stop] {
            for phase in [Phase::Ready, Phase::MissingFiles { gone: gone() }, Phase::Error { message: "x".into() }] {
                assert_eq!(command(&Lifecycle::new(wants, phase), Command::Check), Ok(Lifecycle::new(wants, checking())));
            }
        }
    }

    #[test]
    fn refusals() {
        let refused = |phase, c| command(&Lifecycle::new(Wants::Run, phase), c).unwrap_err();
        assert_eq!(refused(Phase::FetchingInfo, Command::Pause), Refusal::Because("has no info yet"));
        assert_eq!(refused(Phase::Moving, Command::Pause), Refusal::Because("is moving"));
        assert_eq!(refused(checking(), Command::Move), Refusal::Because("is being checked"));
        let away = Phase::DestinationUnavailable { reason: drive() };
        assert_eq!(refused(away.clone(), Command::Resume), Refusal::DestinationUnavailable { action: Command::Resume, reason: drive() });
        assert_eq!(refused(away, Command::Delete), Refusal::DestinationUnavailable { action: Command::Delete, reason: drive() });
    }

    #[test]
    fn a_torrent_goes_from_fetching_info_through_checking_to_ready() {
        let t = Lifecycle::new(Wants::Run, Phase::FetchingInfo);
        assert_eq!(t.status(false), Status::FetchingInfo);
        let checking = observe(&t.phase, &Observation::InfoArrived { checking: true });
        assert_eq!(checking, super::checking());
        assert_eq!(observe(&checking, &Observation::Checking), checking, "still checking");
        assert_eq!(observe(&checking, &Observation::Idle { not_back: BTreeMap::new() }), Phase::Ready);
        assert_eq!(Lifecycle::new(Wants::Run, Phase::Ready).status(false), Status::Downloading);
        assert_eq!(Lifecycle::new(Wants::Run, Phase::Ready).status(true), Status::Seeding);
        assert_eq!(Lifecycle::new(Wants::Stop, Phase::Ready).status(false), Status::Paused);
        assert_eq!(Lifecycle::new(Wants::Stop, Phase::Ready).status(true), Status::Done);
    }

    #[test]
    fn missing_files_put_back_are_checked_and_those_that_are_not_what_they_were_are_missing_again() {
        let missing = Phase::MissingFiles { gone: gone() };
        let returning = observe(&missing, &Observation::FilesBack);
        assert_eq!(returning, Phase::Checking { returning: gone() });
        assert_eq!(observe(&returning, &Observation::Idle { not_back: gone() }), missing);
        assert_eq!(observe(&missing, &Observation::NoFilesMissing), Phase::Ready, "set Off");
    }

    #[test]
    fn a_destination_gone_and_back_checks_the_torrent_and_keeps_what_is_wanted() {
        for wants in [Wants::Run, Wants::Stop] {
            let t = Lifecycle::new(wants, Phase::Ready);
            let away = decide(&t, &Event::Observation(Observation::DestinationGone { reason: drive() })).unwrap();
            assert_eq!(away, Lifecycle::new(wants, Phase::DestinationUnavailable { reason: drive() }));
            let back = decide(&away, &Event::Observation(Observation::DestinationBack)).unwrap();
            assert_eq!(back, Lifecycle::new(wants, checking()));
        }
        let away = Lifecycle::new(Wants::Run, Phase::DestinationUnavailable { reason: drive() });
        assert_eq!(command(&away, Command::Pause), Ok(Lifecycle::new(Wants::Stop, Phase::DestinationUnavailable { reason: drive() })), "stays paused once back");
        assert_eq!(observe(&Phase::FetchingInfo, &Observation::DestinationGone { reason: drive() }), Phase::FetchingInfo);
    }

    #[test]
    fn librqbit_is_made_to_match() {
        let t = |wants, phase| Lifecycle::new(wants, phase);
        assert_eq!(reconcile(&t(Wants::Run, Phase::Ready), Lib::Stopped), Some(Do::Start));
        assert_eq!(reconcile(&t(Wants::Stop, Phase::Ready), Lib::Running), Some(Do::Stop));
        assert_eq!(reconcile(&t(Wants::Run, Phase::MissingFiles { gone: gone() }), Lib::Running), Some(Do::Stop));
        assert_eq!(reconcile(&t(Wants::Stop, checking()), Lib::Checking), None, "a check isn't interrupted");
        assert_eq!(reconcile(&t(Wants::Run, Phase::Ready), Lib::Checking), None);
    }
}
