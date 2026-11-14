//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable), statuses, priorities and what an add
//! did. Shared by the CLI and the app; the service only states the facts.

use std::num::NonZeroU16;
use std::time::Duration;

use proto::{
    Action, Added, Answer, Kept, KeptWhy, ListenPort, Priority, Refusal, Status, TorrentFailure, TorrentRefusal, Unavailable,
    Why as AnswerWhy,
};

use crate::machine::{State, Why};
use crate::setting::ValueProblem;
use crate::{Failure, LinkProblem, OpenRefusal};

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, s / 60 % 60),
    }
}

/// What a torrent is doing, as a list shows it: "Fetching info".
pub fn status(status: &Status) -> &'static str {
    match status {
        Status::FetchingInfo => "Fetching info",
        Status::Checking => "Checking",
        Status::Downloading => "Downloading",
        Status::Seeding => "Seeding",
        Status::Paused => "Paused",
        Status::Done => "Done",
        Status::MissingFiles => "Missing files",
        Status::DestinationUnavailable { .. } => "Destination unavailable",
        Status::Moving => "Moving",
        Status::Deleting => "Deleting",
        Status::Error { .. } => "Error",
    }
}

pub fn priority(priority: Priority) -> &'static str {
    match priority {
        Priority::Normal => "Normal",
        Priority::Off => "Off",
    }
}

/// What `n` selected torrents are called in a menu: "torrent", "3 torrents".
fn torrents(n: usize) -> String {
    if n == 1 { "torrent".into() } else { format!("{n} torrents") }
}

/// The menus' Remove: "Remove torrent", or "Remove 3 torrents (keep files)" where it says it
/// keeps them (beside Delete, or where Delete can't be).
pub fn remove_choice(n: usize, keeps_files: bool) -> String {
    let keep = if keeps_files { " (keep files)" } else { "" };
    format!("Remove {}{keep}", torrents(n))
}

/// The menus' Delete: "Delete torrent (files to the Trash)".
pub fn delete_choice(n: usize) -> String {
    format!("Delete {} (files to the Trash)", torrents(n))
}

/// Add file…'s tooltip, with its shortcut: "Add a .torrent file (⇧⌘A)".
pub fn add_file_help(shortcut: &str) -> String {
    format!("Add a .torrent file ({shortcut})")
}

/// That Copy magnet link worked: "Copied the magnet link of Big Buck Bunny".
pub fn copied_magnet(name: &str) -> String {
    format!("Copied the magnet link of {name}")
}

/// What an add did: "Added Big Buck Bunny", or "Big Buck Bunny is already in the list".
pub fn added(added: &Added) -> String {
    match (&added.name, added.already_existed) {
        (Some(name), false) => format!("Added {name}"),
        (Some(name), true) => format!("{name} is already in the list"),
        (None, false) => "Added the torrent".into(),
        (None, true) => "The torrent is already in the list".into(),
    }
}

/// What a person calls a setting, from its key in the settings file; a key without a label is
/// shown as it is.
pub fn setting_label(key: &str) -> &str {
    match key {
        "listen_port" => "Listening port",
        "upnp" => "UPnP",
        "default_destination" => "Default download folder",
        "app_icon" => "App icon",
        key => key,
    }
}

/// What reading a settings file did when some values were invalid: "2 settings were reset to their
/// defaults: UPnP, Listening port". `None` when none were.
pub fn settings_reset(names: &[String]) -> Option<String> {
    let labels = names.iter().map(|name| setting_label(name)).collect::<Vec<_>>().join(", ");
    match names.len() {
        0 => None,
        1 => Some(format!("1 setting was reset to its default: {labels}")),
        count => Some(format!("{count} settings were reset to their defaults: {labels}")),
    }
}

/// What a settings-reset notice opens to: why, and where the file as it was is kept.
pub fn settings_reset_why(copy: &str) -> String {
    format!("They couldn't be read, so they're back to their defaults. The file as it was is kept as {copy} in the data dir.")
}

/// Why typed text can't be a setting's value.
pub fn value_problem(problem: ValueProblem) -> &'static str {
    match problem {
        ValueProblem::NotAPort => "A port is a whole number, like 51413",
        ValueProblem::PortOutOfRange => "A port is a number from 1 to 65535",
        ValueProblem::NotOnOrOff => "Either on or off",
    }
}

/// Where the listening port came from, after the port: "picked automatically", "set by you", or
/// that a pinned one is taken: "set by you, but another app has it: listening on 51234 for now".
pub fn listen_port(port: ListenPort, listening_on: NonZeroU16) -> String {
    match port {
        ListenPort::Picked(_) => "picked automatically".into(),
        ListenPort::Pinned(pinned) if pinned == listening_on => "set by you".into(),
        ListenPort::Pinned(_) => format!("set by you, but another app has it: listening on {listening_on} for now"),
    }
}

/// Why the link field's text can't be added.
pub fn link_problem(problem: LinkProblem) -> &'static str {
    match problem {
        LinkProblem::NotALink => "Not a magnet link, a .torrent URL or an info hash",
        LinkProblem::NoInfoHash => "The magnet link has no complete info hash",
    }
}

/// Why something handed to the app from outside isn't added.
pub fn open_refusal(refusal: OpenRefusal) -> &'static str {
    match refusal {
        OpenRefusal::NotOnDisk => "It isn't on disk",
        OpenRefusal::NotATorrentFile => "Phantom only opens .torrent files",
        OpenRefusal::NotAMagnetLink => "Phantom only opens magnet links",
    }
}

/// Why an add failed, as a sentence: "No peers have this torrent".
pub fn add_failure(failure: &Failure) -> String {
    sentence(&format!("{failure:#}"))
}

/// The error list's title.
pub fn errors_title() -> &'static str {
    "Errors"
}

/// The error list's button that dismisses everything.
pub fn clear_all() -> &'static str {
    "Clear all"
}

/// The title of the note about a batch of adds, `failed` of `total` not added: "Couldn't add 2
/// of 5", or "Couldn't add the torrent" when it was one alone.
pub fn adds_failed(failed: usize, total: usize) -> String {
    match total {
        1 => "Couldn't add the torrent".into(),
        _ => format!("Couldn't add {failed} of {total}"),
    }
}

/// Why a destination can't be reached, as a sentence: "Drive MyDisk isn't connected".
pub fn unavailable(reason: &Unavailable) -> String {
    match reason {
        Unavailable::DriveNotConnected { name } => format!("Drive {name} isn't connected"),
        Unavailable::FolderMissing { path } => format!("Folder {} doesn't exist", path.display()),
        Unavailable::NoPermission { path } => format!("No permission to open folder {}", path.display()),
        Unavailable::NotAFolder { path } => format!("{} isn't a folder", path.display()),
        Unavailable::CantOpen { path } => format!("Can't open folder {}", path.display()),
    }
}

/// Why the status needs the user, when it says: "Drive MyDisk isn't connected", or the error.
pub fn status_reason(name: &str, status: &Status) -> Option<String> {
    match status {
        Status::DestinationUnavailable { reason } => Some(unavailable(reason)),
        Status::Error { failure } => Some(torrent_failure(name, failure)),
        _ => None,
    }
}

/// What went wrong with a torrent, as a sentence: Phantom's part worded here, librqbit's text as
/// it says it.
pub fn torrent_failure(name: &str, failure: &TorrentFailure) -> String {
    match failure {
        TorrentFailure::FetchFailed { detail } => format!("Couldn't fetch its info: {detail}"),
        TorrentFailure::AddAgainFailed { detail } => format!("Couldn't check it: {detail}"),
        TorrentFailure::Librqbit { detail } => sentence(detail),
        TorrentFailure::NotTrashed { kept } => sentence(&not_trashed(name, kept)),
    }
}

/// Why a disabled button for `action` is off, from the selected torrents' names and answers for
/// it (#89); `None` while it does something to one of them. One reason shared by every one is
/// worded for it ("Big Buck Bunny is already paused", "All 3 are already paused"); different
/// reasons get one line for the action.
pub fn why_off(action: Action, selected: &[(&str, &Answer)]) -> Option<String> {
    let verb = match action {
        Action::Pause => "paused",
        Action::Resume => "resumed",
        Action::Check => "checked",
        Action::SetPriority => "given priorities",
        Action::Remove => "removed",
        Action::Delete => "deleted",
        Action::Open => "opened",
        Action::Reveal => "shown in Finder",
    };
    let (first, rest) = match selected {
        [] => return Some(format!("Select a torrent to have it {verb}")),
        [first, rest @ ..] => (first, rest),
    };
    if selected.iter().any(|(_, a)| **a == Answer::Does) {
        return None;
    }
    if rest.iter().any(|(_, a)| *a != first.1) {
        return Some(format!("None of the selected torrents can be {verb}"));
    }
    let subject = match selected.len() {
        1 => Subject::One(first.0),
        n => Subject::Several(n),
    };
    Some(match first.1 {
        Answer::Does => unreachable!("handled above"),
        Answer::DoesNothing { why } => {
            let is = subject.is();
            match why {
                AnswerWhy::AlreadyStopped => format!("{subject} {is} already paused"),
                AnswerWhy::AlreadyRunning => format!("{subject} {is} already running"),
                AnswerWhy::AlreadyChecking => format!("{subject} {is} already being checked"),
                AnswerWhy::NoFilesYet => format!("{subject} {} no files yet", subject.has()),
                AnswerWhy::FilesMissing => match subject {
                    Subject::One(name) => format!("{name}'s files are missing from disk"),
                    Subject::Several(_) => format!("{subject} have files missing from disk"),
                },
            }
        }
        Answer::Refused { why } => torrent(subject, why),
    })
}

/// Who a sentence is about: one torrent, by its name or as "torrent 3", which stays as it is; or
/// several, "All 3", which starts a sentence. The verbs and pronouns after it agree.
#[derive(Clone, Copy)]
enum Subject<'a> {
    One(&'a str),
    Several(usize),
}

impl Subject<'_> {
    /// The word that agrees with it: `one`'s for one torrent, `several` for more.
    fn pick(self, one: &'static str, several: &'static str) -> &'static str {
        if let Subject::One(_) = self { one } else { several }
    }

    fn is(self) -> &'static str {
        self.pick("is", "are")
    }

    fn has(self) -> &'static str {
        self.pick("has", "have")
    }

    fn its(self) -> &'static str {
        self.pick("its", "their")
    }
}

impl std::fmt::Display for Subject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Subject::One(name) => f.write_str(name),
            Subject::Several(n) => write!(f, "All {n}"),
        }
    }
}

/// `text` with a capital first letter. The service's and `refusal`'s wording is lowercase so it
/// composes after "error: " and in chains; a surface that shows it alone makes it a sentence.
pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The connection to the service, as a status bar shows it.
pub fn connection(state: &State) -> String {
    match state {
        State::Connecting => "Connecting to the service…".into(),
        State::Replacing { .. } => "Restarting the service…".into(),
        State::Spawning { .. } => "Starting the service…".into(),
        State::Connected { .. } => "Connected".into(),
        State::Lost { why, .. } => format!("Service unavailable: {}", lost(why)),
        State::Outdated => sentence(&lost(&Why::Outdated)),
    }
}

/// Why a client lost the service, or never reached it.
pub fn lost(why: &Why) -> String {
    match why {
        Why::OldDidNotStop => "the old service didn't stop".into(),
        Why::DidNotStart => "the service didn't start; see service.log in the data dir".into(),
        Why::StoppedEarly => "the service stopped right after it started; see service.log in the data dir".into(),
        Why::Taken => "a service of another build started instead".into(),
        Why::Outdated => "Phantom was updated: restart the app".into(),
        Why::Failed(e) => e.clone(),
    }
}

pub fn refusal(refusal: &Refusal) -> String {
    match refusal {
        // Each client adds its own fix: the CLI names its options, the app asks.
        Refusal::NoDestination => "no destination".into(),
        Refusal::Destination { reason } => {
            format!("can't add the torrent: its destination is unavailable. {}", unavailable(reason))
        }
        Refusal::DefaultDestination { reason } => {
            format!("can't use it as the default destination. {}", unavailable(reason))
        }
        Refusal::Torrent { id, why } => torrent(Subject::One(&format!("torrent {id}")), why),
        Refusal::Stopping => "the service is stopping".into(),
    }
}

/// The title of the entry about a command sent to `total` torrents, `failed` of which it didn't do:
/// "Couldn't delete 2 of 5", or "Couldn't delete the torrent" when it was sent to one.
pub fn commands_failed(action: Action, failed: usize, total: usize) -> String {
    let verb = match action {
        Action::Pause => "pause",
        Action::Resume => "resume",
        Action::Check => "check",
        Action::SetPriority => "change the priority of",
        Action::Remove => "remove",
        Action::Delete => "delete",
        Action::Open => "open",
        Action::Reveal => "show",
    };
    match total {
        1 => format!("Couldn't {verb} the torrent"),
        _ => format!("Couldn't {verb} {failed} of {total}"),
    }
}

/// A failed command for one torrent called `name`, as the error list's line: what, and why under
/// it when there's more than the sentence. A Delete the drive's missing Trash refused is one
/// sentence naming the torrent; the rest are the torrent's name and the refusal or error under it.
pub fn command_failure(name: &str, failure: &Failure) -> (String, String) {
    match failure {
        Failure::Refused(Refusal::Torrent { why: TorrentRefusal::NotTrashed { name, kept }, .. }) if no_trash(kept) => {
            (not_trashed(name, kept), String::new())
        }
        Failure::Refused(Refusal::Torrent { why: TorrentRefusal::NotTrashed { name, kept }, .. }) => {
            (name.clone(), sentence(&not_trashed(name, kept)))
        }
        other => (name.to_owned(), sentence(&format!("{other:#}"))),
    }
}

fn no_trash(kept: &[Kept]) -> bool {
    kept.iter().all(|k| k.why == KeptWhy::NoTrash)
}

/// What Delete of the torrent called `name` left where it was, and why. Where the drive has no
/// Trash it's a sentence, "Couldn't delete Album: the drive has no Trash"; otherwise lowercase,
/// as a refusal's tail. Nothing is ever erased instead.
pub fn not_trashed(name: &str, kept: &[Kept]) -> String {
    let paths: Vec<String> = kept.iter().map(|k| k.path.display().to_string()).collect();
    let paths = paths.join(", ");
    let what = if kept.len() == 1 { "it is" } else { "they are" };
    match kept.iter().find_map(|k| match &k.why {
        KeptWhy::Failed { detail } => Some(detail),
        KeptWhy::NoTrash | KeptWhy::Interrupted => None,
    }) {
        _ if no_trash(kept) => format!("Couldn't delete {name}: the drive has no Trash"),
        Some(detail) => format!("couldn't move everything to the Trash: {detail} ({what} still there: {paths})"),
        None => format!("Phantom stopped before moving everything to the Trash ({what} still there: {paths})"),
    }
}

/// Why `subject` refuses: lowercase where the subject is.
fn torrent(subject: Subject, why: &TorrentRefusal) -> String {
    let (has, its) = (subject.has(), subject.its());
    match why {
        TorrentRefusal::NotInList => format!("{subject} {} in the list", subject.pick("isn't", "aren't")),
        TorrentRefusal::NoInfoYet => format!("{subject} {has} no info yet"),
        TorrentRefusal::FetchFailed => format!("{subject} {has} no info: fetching {} failed", subject.pick("it", "theirs")),
        TorrentRefusal::Deleting => format!("{subject} {} being deleted", subject.is()),
        TorrentRefusal::PartlyDeleted => {
            let them = subject.pick("it", "them");
            format!("{subject} {} partly in the Trash: delete {them} again, or remove {them}", subject.is())
        }
        TorrentRefusal::NoFile { index } => format!("{subject} {has} no file {index}"),
        // Names the torrent itself.
        TorrentRefusal::NotTrashed { name, kept } if no_trash(kept) => not_trashed(name, kept),
        TorrentRefusal::NotTrashed { name, kept } => format!("{subject}: {}", not_trashed(name, kept)),
        TorrentRefusal::DestinationUnavailable { action, reason } => {
            let what = match action {
                Action::Pause => "pause",
                Action::Resume => "start",
                Action::Check => "be checked",
                Action::SetPriority => &format!("change {its} priorities"),
                Action::Remove => "be removed",
                Action::Delete => &format!("move {its} files to the Trash"),
                Action::Open => "be opened",
                Action::Reveal => "be shown in Finder",
            };
            let destinations = subject.pick("its destination is", "their destinations are");
            format!("{subject} can't {what}: {destinations} unavailable. {}", unavailable(reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use proto::AbsolutePath;

    use super::*;

    #[test]
    fn statuses_priorities_adds_and_link_problems_have_their_words() {
        assert_eq!(status(&Status::FetchingInfo), "Fetching info");
        assert_eq!(status(&Status::Error { failure: TorrentFailure::Librqbit { detail: "x".into() } }), "Error");
        let fetch = Status::Error { failure: TorrentFailure::FetchFailed { detail: "no peers".into() } };
        assert_eq!(status_reason("x", &fetch).as_deref(), Some("Couldn't fetch its info: no peers"));
        let again = Status::Error { failure: TorrentFailure::AddAgainFailed { detail: "disk full".into() } };
        assert_eq!(status_reason("x", &again).as_deref(), Some("Couldn't check it: disk full"), "Phantom's part worded here");
        assert_eq!(priority(Priority::Off), "Off");
        let new = Added { id: 3, name: Some("Big Buck Bunny".into()), already_existed: false };
        assert_eq!(added(&new), "Added Big Buck Bunny");
        assert_eq!(added(&Added { name: None, ..new.clone() }), "Added the torrent");
        assert_eq!(added(&Added { already_existed: true, ..new.clone() }), "Big Buck Bunny is already in the list");
        assert_eq!(added(&Added { already_existed: true, name: None, ..new }), "The torrent is already in the list");
        assert_eq!(link_problem(LinkProblem::NotALink), "Not a magnet link, a .torrent URL or an info hash");
        assert_eq!(link_problem(LinkProblem::NoInfoHash), "The magnet link has no complete info hash");
        assert_eq!(adds_failed(2, 5), "Couldn't add 2 of 5");
        assert_eq!(adds_failed(1, 1), "Couldn't add the torrent");
        assert_eq!(open_refusal(OpenRefusal::NotATorrentFile), "Phantom only opens .torrent files");
    }

    #[test]
    fn a_sentence_starts_with_a_capital() {
        assert_eq!(sentence("could not add the torrent"), "Could not add the torrent");
        assert_eq!(sentence("Already fine"), "Already fine");
        assert_eq!(sentence("épique"), "Épique");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn reasons_and_refusals_read_as_sentences() {
        let drive = Unavailable::DriveNotConnected { name: "MyDisk".into() };
        assert_eq!(unavailable(&drive), "Drive MyDisk isn't connected");
        assert_eq!(
            unavailable(&Unavailable::FolderMissing { path: AbsolutePath::new("/Users/me/Films").unwrap() }),
            "Folder /Users/me/Films doesn't exist"
        );
        let why = TorrentRefusal::DestinationUnavailable { action: Action::Resume, reason: drive.clone() };
        let start = Refusal::Torrent { id: 1, why };
        assert_eq!(refusal(&start), "torrent 1 can't start: its destination is unavailable. Drive MyDisk isn't connected");
        let default = Refusal::DefaultDestination { reason: drive };
        assert_eq!(refusal(&default), "can't use it as the default destination. Drive MyDisk isn't connected");
    }
}

#[cfg(test)]
mod label_tests {
    use proptest::prelude::*;

    use super::*;

    const KEYS: [&str; 4] = ["listen_port", "upnp", "default_destination", "app_icon"];

    proptest! {
        #[test]
        fn a_known_key_has_a_label_and_any_other_comes_back_unchanged(key in "[a-z_]{1,16}") {
            if KEYS.contains(&key.as_str()) {
                prop_assert_ne!(setting_label(&key), key.as_str());
            } else {
                prop_assert_eq!(setting_label(&key), key.as_str());
            }
        }

        #[test]
        fn the_notice_counts_and_labels_every_name_reported(keys in proptest::collection::vec(proptest::sample::select(KEYS.to_vec()), 0..=4)) {
            let names: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
            match settings_reset(&names) {
                None => prop_assert!(names.is_empty()),
                Some(notice) => {
                    prop_assert!(notice.starts_with(&names.len().to_string()));
                    for name in &names {
                        prop_assert!(notice.contains(setting_label(name)));
                        prop_assert!(!notice.contains(name.as_str()) || setting_label(name).contains(name.as_str()));
                    }
                }
            }
        }
    }

    #[test]
    fn what_delete_left_says_why_and_where() {
        let at = |p: &str| proto::AbsolutePath::new(p).unwrap();
        let no_trash = [Kept { path: at("/Volumes/USB/pack"), why: KeptWhy::NoTrash }];
        assert_eq!(not_trashed("Album", &no_trash), "Couldn't delete Album: the drive has no Trash");
        let locked = [
            Kept { path: at("/d/pack/a"), why: KeptWhy::Failed { detail: "it's locked".into() } },
            Kept { path: at("/d/pack/b"), why: KeptWhy::NoTrash },
        ];
        assert_eq!(
            not_trashed("Pack", &locked),
            "couldn't move everything to the Trash: it's locked (they are still there: /d/pack/a, /d/pack/b)"
        );
        let why = TorrentRefusal::NotTrashed { name: "Album".into(), kept: no_trash.to_vec() };
        let refused = Refusal::Torrent { id: 3, why };
        assert_eq!(refusal(&refused), "Couldn't delete Album: the drive has no Trash");
        let failure = crate::Failure::Refused(refused);
        assert_eq!(command_failure("Album", &failure), ("Couldn't delete Album: the drive has no Trash".to_owned(), String::new()));
        let waiting = Status::Error { failure: TorrentFailure::NotTrashed { kept: no_trash.to_vec() } };
        assert_eq!(status_reason("Album", &waiting).as_deref(), Some("Couldn't delete Album: the drive has no Trash"));
        let two = Refusal::Torrent { id: 4, why: TorrentRefusal::NotTrashed { name: "Film".into(), kept: no_trash.to_vec() } };
        assert_eq!(refusal(&two), "Couldn't delete Film: the drive has no Trash");
    }

    #[test]
    fn a_button_off_says_why_for_one_reason_and_one_line_for_several() {
        use proto::{Answer, Why};
        let paused = Answer::DoesNothing { why: Why::AlreadyStopped };
        let away = Answer::Refused {
            why: TorrentRefusal::DestinationUnavailable {
                action: Action::Resume,
                reason: Unavailable::DriveNotConnected { name: "MyDisk".into() },
            },
        };
        assert_eq!(why_off(Action::Pause, &[]).as_deref(), Some("Select a torrent to have it paused"));
        assert_eq!(
            why_off(Action::Pause, &[("debian.iso", &paused)]).as_deref(),
            Some("debian.iso is already paused"),
            "the name as it is"
        );
        assert_eq!(why_off(Action::Pause, &[("a", &paused), ("b", &paused)]).as_deref(), Some("All 2 are already paused"));
        assert_eq!(
            why_off(Action::Resume, &[("Album", &away)]).as_deref(),
            Some("Album can't start: its destination is unavailable. Drive MyDisk isn't connected")
        );
        assert_eq!(
            why_off(Action::Resume, &[("a", &paused), ("b", &away)]).as_deref(),
            Some("None of the selected torrents can be resumed")
        );
        assert_eq!(why_off(Action::Pause, &[("a", &paused), ("b", &Answer::Does)]), None, "on: one of them it does something to");
    }

    /// Every reason a button can be off, each kind once: a new one fails to compile here.
    fn every_answer_off() -> Vec<proto::Answer> {
        use proto::{Answer, Why};
        let kept = vec![Kept { path: proto::AbsolutePath::new("/d/a").unwrap(), why: KeptWhy::NoTrash }];
        let why = [Why::AlreadyStopped, Why::AlreadyRunning, Why::AlreadyChecking, Why::NoFilesYet, Why::FilesMissing];
        let refusals = [
            TorrentRefusal::NotInList,
            TorrentRefusal::NoInfoYet,
            TorrentRefusal::FetchFailed,
            TorrentRefusal::Deleting,
            TorrentRefusal::PartlyDeleted,
            TorrentRefusal::NoFile { index: 2 },
            TorrentRefusal::NotTrashed { name: "Album".into(), kept },
            TorrentRefusal::DestinationUnavailable {
                action: Action::SetPriority,
                reason: Unavailable::DriveNotConnected { name: "MyDisk".into() },
            },
        ];
        for r in &refusals {
            match r {
                TorrentRefusal::NotInList
                | TorrentRefusal::NoInfoYet
                | TorrentRefusal::FetchFailed
                | TorrentRefusal::Deleting
                | TorrentRefusal::PartlyDeleted
                | TorrentRefusal::NoFile { .. }
                | TorrentRefusal::NotTrashed { .. }
                | TorrentRefusal::DestinationUnavailable { .. } => {}
            }
        }
        for w in &why {
            match w {
                Why::AlreadyStopped | Why::AlreadyRunning | Why::AlreadyChecking | Why::NoFilesYet | Why::FilesMissing => {}
            }
        }
        why.into_iter().map(|why| Answer::DoesNothing { why }).chain(refusals.into_iter().map(|why| Answer::Refused { why })).collect()
    }

    proptest::proptest! {
        /// One torrent is named, as it is, with the singular; several (2, or n) are "All n", with
        /// the plural.
        #[test]
        fn a_reason_agrees_with_how_many_are_selected(n in proptest::prop_oneof![proptest::strategy::Just(1usize), proptest::strategy::Just(2), 3usize..40], k in 0usize..13) {
            let answers = every_answer_off();
            let answer = &answers[k % answers.len()];
            let names: Vec<String> = (0..n).map(|i| format!("Movie {i}")).collect();
            let selected: Vec<(&str, &proto::Answer)> = names.iter().map(|name| (name.as_str(), answer)).collect();
            let text = why_off(Action::Pause, &selected).expect("off");
            // The word after the subject agrees with it; "can't" and ": " agree with both.
            let subject = if n == 1 { "Movie 0".to_owned() } else { format!("All {n}") };
            proptest::prop_assert!(text.starts_with(&subject), "{text}");
            let rest = &text[subject.len()..];
            let next = rest.trim_start_matches(':').split(' ').nth(1).unwrap_or("");
            let (singular, plural) = (["is", "isn't", "has"], ["are", "aren't", "have"]);
            let neutral = rest.starts_with(':') || next == "can't";
            if n == 1 {
                proptest::prop_assert!(neutral || rest.starts_with("'s ") || singular.contains(&next), "{text}");
                proptest::prop_assert!(!text.contains(" their "), "{text}");
            } else {
                proptest::prop_assert!(neutral || plural.contains(&next), "{text}");
                proptest::prop_assert!(!text.contains(" its ") && !text.contains("'s "), "{text}");
            }
        }
    }
}
