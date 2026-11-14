//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable), statuses, priorities and what an add
//! did. Shared by the CLI and the app; the service only states the facts.

use std::num::NonZeroU16;
use std::time::Duration;

use proto::{Action, Added, Kept, KeptWhy, ListenPort, Priority, Refusal, Status, TorrentRefusal, Unavailable};

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
pub fn status_reason(status: &Status) -> Option<String> {
    match status {
        Status::DestinationUnavailable { reason } => Some(unavailable(reason)),
        Status::Error { message } => Some(message.clone()),
        _ => None,
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
        Refusal::Torrent { id, why } => torrent(*id, why),
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

fn torrent(id: u64, why: &TorrentRefusal) -> String {
    match why {
        TorrentRefusal::NotInList => format!("no torrent with id {id}"),
        TorrentRefusal::NoInfoYet => format!("torrent {id} has no info yet"),
        TorrentRefusal::FetchFailed => format!("torrent {id} has no info: fetching it failed"),
        TorrentRefusal::Deleting => format!("torrent {id} is being deleted"),
        TorrentRefusal::PartlyDeleted => format!("torrent {id} is partly in the Trash: delete it again, or remove it"),
        TorrentRefusal::NoFile { index } => format!("torrent {id} has no file {index}"),
        // Names the torrent itself.
        TorrentRefusal::NotTrashed { name, kept } if no_trash(kept) => not_trashed(name, kept),
        TorrentRefusal::NotTrashed { name, kept } => format!("torrent {id}: {}", not_trashed(name, kept)),
        TorrentRefusal::DestinationUnavailable { action, reason } => {
            let what = match action {
                Action::Pause => "pause",
                Action::Resume => "start",
                Action::Check => "be checked",
                Action::SetPriority => "change its priorities",
                Action::Remove => "be removed",
                Action::Delete => "move its files to the Trash",
                Action::Open => "be opened",
                Action::Reveal => "be shown in Finder",
            };
            format!("torrent {id} can't {what}: its destination is unavailable. {}", unavailable(reason))
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
        assert_eq!(status(&Status::Error { message: "x".into() }), "Error");
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
        let two = Refusal::Torrent { id: 4, why: TorrentRefusal::NotTrashed { name: "Film".into(), kept: no_trash.to_vec() } };
        assert_eq!(refusal(&two), "Couldn't delete Film: the drive has no Trash");
    }
}
