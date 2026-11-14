//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable), statuses, priorities and what an add
//! did. Shared by the CLI and the app; the service only states the facts.

use std::num::NonZeroU16;
use std::time::Duration;

use proto::{
    Action, Added, Answer, Direction, Kept, KeptWhy, LimitProblem, ListenPort, Priority, Ratio, Reached, Refusal, SeedTime, SeedingLimit,
    SpeedLimit, Status, TorrentFailure, TorrentRefusal, Unavailable, Volume, Why as AnswerWhy,
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

/// What the status bar's notice says it does on hover: opens the popover, or dismisses the only
/// notice when there's nothing more to read.
pub fn notices_press(opens: bool) -> &'static str {
    if opens { "Show them" } else { "Click to dismiss" }
}

/// A told notice's dismiss button in the popover: the glyph it shows, and what it says to
/// VoiceOver.
pub fn dismiss_glyph() -> &'static str {
    "×"
}

pub fn dismiss() -> &'static str {
    "Dismiss"
}

/// The action that shows a file in Finder.
pub fn reveal_in_finder() -> &'static str {
    "Reveal in Finder"
}

/// After the status bar's newest notice, which of how many it is: " · 1/3", or nothing when it's
/// the only one.
pub fn notices_count(total: usize) -> String {
    match total {
        0 | 1 => String::new(),
        n => format!(" · 1/{n}"),
    }
}

/// The popover's place among the notices, newest first: "2 of 3" (`at` counts from 0).
pub fn notice_place(at: usize, total: usize) -> String {
    format!("{} of {total}", at + 1)
}

/// The popover's step buttons: the glyph each shows, and what it says to VoiceOver. ‹ goes to a
/// newer notice, › to an older one.
pub fn newer_glyph() -> &'static str {
    "‹"
}

pub fn newer() -> &'static str {
    "Newer notice"
}

pub fn older_glyph() -> &'static str {
    "›"
}

pub fn older() -> &'static str {
    "Older notice"
}

/// What a disk is called: the name Finder shows ("Macintosh HD", "MyDisk"), or its mount when the
/// service gave none.
pub fn disk(volume: &Volume) -> String {
    if volume.name.is_empty() { volume.mount.display().to_string() } else { volume.name.clone() }
}

/// A disk's warning: "Torrents on Macintosh HD need 8.0 GiB; 5.0 GiB free". Without what's
/// free (the disk doesn't answer), only what's needed.
pub fn disk_short(volume: &Volume) -> String {
    let needs = format!("Torrents on {} need {}", disk(volume), bytes(volume.needs_bytes()));
    match volume.free_bytes {
        Some(free) => format!("{needs}; {} free", bytes(free)),
        None => needs,
    }
}

/// A torrent the list doesn't have (yet) where a warning names it: "Torrent 3".
pub fn unlisted_torrent(id: proto::TorrentId) -> String {
    format!("Torrent {id}")
}

/// What a disk's warning opens to explains: why, and what can be done.
pub fn disk_short_why() -> &'static str {
    "They download at the same time, so the disk fills before they're done. Pause some, set files Off, or free some space: Phantom doesn't stop them."
}

/// A torrent that alone needs more than is free, on hover over its status: "It needs 6.0 GiB more, and Macintosh HD has 5.0 GiB free. It goes on
/// downloading until the disk is full."
pub fn too_big_why(needs: u64, free: u64, volume: &Volume) -> String {
    format!("It needs {} more, and {} has {} free. It goes on downloading until the disk is full.", bytes(needs), disk(volume), bytes(free))
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
        Status::Done { .. } => "Done",
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

/// The note under a setting's label, from its key in the settings file; `None` when it has none.
pub fn setting_note(key: &str) -> Option<&'static str> {
    match key {
        "upnp" => Some("Asks the router to forward the listening port to Phantom"),
        "keep_awake" => Some("While downloading, fetching torrent info or checking torrents"),
        _ => None,
    }
}

/// What a person calls a setting, from its key in the settings file; a key without a label is
/// shown as it is.
pub fn setting_label(key: &str) -> &str {
    match key {
        "listen_port" => "Listening port",
        "upnp" => "UPnP",
        "default_destination" => "Default download folder",
        "keep_awake" => "Keep awake",
        "download_limit" => "Download limit",
        "upload_limit" => "Upload limit",
        "seed_ratio" => "Stop seeding at ratio",
        "seed_time" => "Stop seeding after",
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
        ValueProblem::NotASpeed => "A speed is a number and a unit, like 500 KiB/s or 2 MiB/s",
        ValueProblem::Limit(LimitProblem::TooLow) => "The lowest limit is 16 KiB/s",
        ValueProblem::Limit(LimitProblem::TooHigh) => "The highest limit is just under 4 GiB/s",
        ValueProblem::NotARatio => "A ratio is a number, like 2 or 1.5",
        ValueProblem::Ratio(LimitProblem::TooLow) => "The lowest ratio is 0.01",
        ValueProblem::Ratio(LimitProblem::TooHigh) => "The highest ratio is 1000",
        ValueProblem::NotATime => "A time is a number of hours, or with a unit, like 24, 90 min or 2 d",
        ValueProblem::Time(LimitProblem::TooLow) => "The shortest time is a minute",
        ValueProblem::Time(LimitProblem::TooHigh) => "The longest time is ten years",
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

/// Why the CLI's `add` of `source` stops before the service: it's no file, nor a link Add accepts.
/// Lowercase, as an error's tail.
pub fn not_added(source: &str, problem: LinkProblem) -> String {
    match problem {
        LinkProblem::NotALink => format!("{source} is no file, magnet link, .torrent URL or info hash"),
        LinkProblem::NoInfoHash => format!("{source}: the magnet link has no complete info hash"),
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
        Status::Done { reached: Some(reached) } => Some(stopped_seeding(reached)),
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

/// Why a disabled button is off, from the selected torrents' answers for its action (#89), as the
/// note under it says: just the reason, naming no torrent and counting none. `None` while the
/// action does something to one of them. With several reasons, the most common one; a tie goes to
/// a refusal, which the user can act on, then to the reason that sorts first.
pub fn why_off(selected: &[&Answer]) -> Option<String> {
    if selected.is_empty() {
        return Some("No torrent selected".into());
    }
    // (reason, whether it's a refusal, how many torrents have it)
    let mut reasons: Vec<(String, bool, usize)> = Vec::new();
    for answer in selected {
        let (reason, refused) = match answer {
            Answer::Does => return None,
            Answer::DoesNothing { why } => (does_nothing(*why), false),
            Answer::Refused { why } => (torrent_reason(why), true),
        };
        match reasons.iter_mut().find(|(r, _, _)| *r == reason) {
            Some((_, _, n)) => *n += 1,
            None => reasons.push((reason, refused, 1)),
        }
    }
    reasons.into_iter().max_by(|a, b| (a.2, a.1).cmp(&(b.2, b.1)).then_with(|| b.0.cmp(&a.0))).map(|(reason, _, _)| reason)
}

/// Why an action does nothing: "Already paused".
fn does_nothing(why: AnswerWhy) -> String {
    match why {
        AnswerWhy::AlreadyStopped => "Already paused",
        AnswerWhy::AlreadyRunning => "Already running",
        AnswerWhy::AlreadyChecking => "Already being checked",
        AnswerWhy::NoFilesYet => "No files yet",
        AnswerWhy::FilesMissing => "Its files are missing from disk",
    }
    .into()
}

/// Why a torrent refuses, alone, as a sentence naming no torrent: "Drive MyDisk isn't connected".
fn torrent_reason(why: &TorrentRefusal) -> String {
    match why {
        TorrentRefusal::NotInList => "Not in the list".into(),
        TorrentRefusal::NoInfoYet => "No info yet".into(),
        TorrentRefusal::FetchFailed => "No info: fetching it failed".into(),
        TorrentRefusal::Deleting => "Being deleted".into(),
        TorrentRefusal::PartlyDeleted => "Partly in the Trash: delete it again, or remove it".into(),
        TorrentRefusal::NoFile { index } => format!("No file {index}"),
        TorrentRefusal::NotTrashed { kept, .. } if no_trash(kept) => "The drive has no Trash".into(),
        TorrentRefusal::NotTrashed { name, kept } => sentence(&not_trashed(name, kept)),
        TorrentRefusal::DestinationUnavailable { reason, .. } => unavailable(reason),
    }
}

/// Why a torrent is Done: "Stopped seeding at ratio 2.0", "Stopped seeding after 24 h".
pub fn stopped_seeding(reached: &Reached) -> String {
    match reached {
        Reached::Ratio(ratio) => format!("Stopped seeding at ratio {}", self::ratio(*ratio)),
        Reached::Time(time) => format!("Stopped seeding after {}", seed_time(*time)),
    }
}

/// A ratio, exactly, with one decimal at least: "2.0", "1.25".
pub fn ratio(ratio: Ratio) -> String {
    let h = ratio.hundredths();
    match h % 100 {
        0 => format!("{}.0", h / 100),
        c if c % 10 == 0 => format!("{}.{}", h / 100, c / 10),
        c => format!("{}.{c:02}", h / 100),
    }
}

/// A seeding time, exactly, in the largest unit that holds it whole: "24 h", "90 min".
pub fn seed_time(time: SeedTime) -> String {
    let m = time.minutes();
    if m % 60 == 0 { format!("{} h", m / 60) } else { format!("{m} min") }
}

/// What the ratio limit means, and how to change it.
pub fn seed_ratio_meaning(ratio: Option<Ratio>) -> String {
    match ratio {
        Some(r) => format!("A finished torrent stops once it uploaded {} times its size; clear it for never", self::ratio(r)),
        None => "Never: type a ratio, like 2, and press Return".into(),
    }
}

/// What the time limit means, and how to change it.
pub fn seed_time_meaning(time: Option<SeedTime>) -> String {
    match time {
        Some(t) => {
            format!("A finished torrent stops after seeding {}, or at the ratio if that comes first; clear it for never", seed_time(t))
        }
        None => "Never: type hours, like 24, or 90 min, and press Return".into(),
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
        Refusal::Torrent { id, why } => torrent(&format!("torrent {id}"), why),
        Refusal::Stopping => "the service is stopping".into(),
        Refusal::StopSeeding { at, problem } => {
            let problem = match at {
                SeedingLimit::Ratio => ValueProblem::Ratio(*problem),
                SeedingLimit::Time => ValueProblem::Time(*problem),
            };
            format!("can't stop seeding there: {}", value_problem(problem).to_lowercase())
        }
        Refusal::SpeedLimit { direction, problem } => {
            let limit = value_problem(ValueProblem::Limit(*problem)).to_lowercase();
            format!("can't use it as the {} limit: {limit}", direction_word(*direction))
        }
    }
}

fn direction_word(direction: Direction) -> &'static str {
    match direction {
        Direction::Download => "download",
        Direction::Upload => "upload",
    }
}

/// A limit exactly, in the largest unit that holds it whole: "2 MiB/s", "500 KiB/s", "20000 B/s".
/// What a limit's field shows, and reads back.
pub fn speed_limit(limit: SpeedLimit) -> String {
    let bytes = u64::from(limit);
    let (size, unit) = [(1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB"), (1, "B")]
        .into_iter()
        .find(|(size, _)| bytes % size == 0)
        .expect("1 divides every number");
    format!("{} {unit}/s", bytes / size)
}

/// What a limit means, and how to change it: "Up to 2 MiB/s for all torrents together; clear it
/// for no limit", or how to set one.
pub fn speed_limit_meaning(limit: Option<SpeedLimit>) -> String {
    match limit {
        Some(limit) => format!("Up to {} for all torrents together; clear it for no limit", speed_limit(limit)),
        None => "Unlimited: type a speed, like 500 KiB/s, and press Return".into(),
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

/// Why the torrent called `name` (or "torrent 3") refuses, lowercase where the name is: the CLI
/// names it, since nothing is selected there.
fn torrent(name: &str, why: &TorrentRefusal) -> String {
    match why {
        TorrentRefusal::NotInList => format!("{name} isn't in the list"),
        TorrentRefusal::NoInfoYet => format!("{name} has no info yet"),
        TorrentRefusal::FetchFailed => format!("{name} has no info: fetching it failed"),
        TorrentRefusal::Deleting => format!("{name} is being deleted"),
        TorrentRefusal::PartlyDeleted => format!("{name} is partly in the Trash: delete it again, or remove it"),
        TorrentRefusal::NoFile { index } => format!("{name} has no file {index}"),
        // Names the torrent itself.
        TorrentRefusal::NotTrashed { name, kept } if no_trash(kept) => not_trashed(name, kept),
        TorrentRefusal::NotTrashed { name: torrent, kept } => format!("{name}: {}", not_trashed(torrent, kept)),
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
            format!("{name} can't {what}: its destination is unavailable. {}", unavailable(reason))
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

    const KEYS: [&str; 9] = [
        "listen_port",
        "upnp",
        "default_destination",
        "keep_awake",
        "download_limit",
        "upload_limit",
        "seed_ratio",
        "seed_time",
        "app_icon",
    ];

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
    fn a_button_off_says_just_the_reason() {
        use proto::{Answer, Why};
        let paused = Answer::DoesNothing { why: Why::AlreadyStopped };
        let running = Answer::DoesNothing { why: Why::AlreadyRunning };
        let away = |name: &str| Answer::Refused {
            why: TorrentRefusal::DestinationUnavailable {
                action: Action::Resume,
                reason: Unavailable::DriveNotConnected { name: name.into() },
            },
        };
        let (disk, usb) = (away("MyDisk"), away("USB"));
        assert_eq!(why_off(&[]).as_deref(), Some("No torrent selected"));
        assert_eq!(why_off(&[&paused]).as_deref(), Some("Already paused"));
        assert_eq!(why_off(&[&running, &running]).as_deref(), Some("Already running"));
        assert_eq!(why_off(&[&disk]).as_deref(), Some("Drive MyDisk isn't connected"));
        assert_eq!(why_off(&[&disk, &disk]).as_deref(), Some("Drive MyDisk isn't connected"));
        assert_eq!(why_off(&[&paused, &disk, &disk]).as_deref(), Some("Drive MyDisk isn't connected"), "the most common");
        assert_eq!(why_off(&[&disk, &paused, &paused]).as_deref(), Some("Already paused"), "the most common");
        assert_eq!(why_off(&[&paused, &disk]).as_deref(), Some("Drive MyDisk isn't connected"), "a tie: the refusal");
        assert_eq!(why_off(&[&disk, &paused]).as_deref(), Some("Drive MyDisk isn't connected"), "in either order");
        assert_eq!(why_off(&[&usb, &disk]).as_deref(), Some("Drive MyDisk isn't connected"), "equal refusals: the one that sorts first");
        assert_eq!(why_off(&[&disk, &usb]).as_deref(), Some("Drive MyDisk isn't connected"));
        assert_eq!(why_off(&[&paused, &Answer::Does]), None, "on: it does something to one of them");
    }

    #[test]
    fn the_cli_names_the_torrent_a_refusal_is_about() {
        let away = TorrentRefusal::DestinationUnavailable {
            action: Action::Resume,
            reason: Unavailable::DriveNotConnected { name: "MyDisk".into() },
        };
        let refused = Refusal::Torrent { id: 3, why: away };
        assert_eq!(refusal(&refused), "torrent 3 can't start: its destination is unavailable. Drive MyDisk isn't connected");
        assert_eq!(refusal(&Refusal::Torrent { id: 3, why: TorrentRefusal::NoInfoYet }), "torrent 3 has no info yet");
    }

    /// Every reason a button can be off, each kind once: a new one fails to compile here.
    fn every_answer_off() -> Vec<proto::Answer> {
        use proto::{Answer, Why};
        let path = || proto::AbsolutePath::new("/d/a").unwrap();
        let kept = vec![Kept { path: path(), why: KeptWhy::Failed { detail: "locked".into() } }];
        let no_trash = vec![Kept { path: path(), why: KeptWhy::NoTrash }];
        let why = [Why::AlreadyStopped, Why::AlreadyRunning, Why::AlreadyChecking, Why::NoFilesYet, Why::FilesMissing];
        let refusals = [
            TorrentRefusal::NotInList,
            TorrentRefusal::NoInfoYet,
            TorrentRefusal::FetchFailed,
            TorrentRefusal::Deleting,
            TorrentRefusal::PartlyDeleted,
            TorrentRefusal::NoFile { index: 2 },
            TorrentRefusal::NotTrashed { name: "Album".into(), kept },
            TorrentRefusal::NotTrashed { name: "Album".into(), kept: no_trash },
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
        /// The note names no torrent and counts none: any number of torrents with the same answer
        /// get the words one alone gets, whatever it's called.
        #[test]
        fn the_note_names_no_torrent_and_counts_none(n in 1usize..40, k in 0usize..14) {
            let answers = every_answer_off();
            let answer = answers[k % answers.len()].clone();
            // The refusal that carries a name is the one that must not show it.
            let named = match answer {
                proto::Answer::Refused { why: TorrentRefusal::NotTrashed { kept, .. } } => {
                    proto::Answer::Refused { why: TorrentRefusal::NotTrashed { name: "Movie 0".into(), kept } }
                }
                other => other,
            };
            let selected: Vec<&proto::Answer> = (0..n).map(|_| &named).collect();
            let text = why_off(&selected).expect("off");
            proptest::prop_assert_eq!(&text, &why_off(&selected[..1]).expect("off"));
            proptest::prop_assert!(!text.contains("Movie 0") && !text.contains("All"), "{text}");
        }

        /// With different reasons the note is the most common one; on a tie, a refusal.
        #[test]
        fn mixed_reasons_give_the_most_common_one_and_a_tie_prefers_a_refusal(
            counts in proptest::collection::vec(0usize..4, 14),
            order in proptest::collection::vec(proptest::num::u32::ANY, 40),
        ) {
            let answers = every_answer_off();
            let mut selected: Vec<&proto::Answer> = Vec::new();
            for (answer, &n) in answers.iter().zip(&counts) {
                selected.extend(std::iter::repeat_n(answer, n));
            }
            proptest::prop_assume!(!selected.is_empty());
            // Any order: the reasons are counted, not read first-to-last.
            let mut keyed: Vec<(u32, &proto::Answer)> =
                selected.iter().enumerate().map(|(i, a)| (order[i % order.len()], *a)).collect();
            keyed.sort_by_key(|(key, _)| *key);
            let selected: Vec<&proto::Answer> = keyed.into_iter().map(|(_, a)| a).collect();

            let text = why_off(&selected).expect("off");
            let most = counts.iter().copied().max().unwrap();
            let top_refused = answers
                .iter()
                .zip(&counts)
                .any(|(a, &n)| n == most && matches!(a, proto::Answer::Refused { .. }));
            // The winners: those with the highest count, refusals first when there's one.
            let winners: Vec<String> = answers
                .iter()
                .zip(&counts)
                .filter(|(a, n)| **n == most && matches!(a, proto::Answer::Refused { .. }) == top_refused)
                .map(|(a, _)| why_off(&[a]).unwrap())
                .collect();
            proptest::prop_assert!(winners.contains(&text), "{text} not among {winners:?}");
        }
    }

    #[test]
    fn the_cli_says_why_it_adds_nothing() {
        assert_eq!(not_added("deb.torrnt", LinkProblem::NotALink), "deb.torrnt is no file, magnet link, .torrent URL or info hash");
        assert_eq!(not_added("magnet:?dn=x", LinkProblem::NoInfoHash), "magnet:?dn=x: the magnet link has no complete info hash");
    }
}

#[cfg(test)]
mod disk_tests {
    use proptest::prelude::*;
    use proto::AbsolutePath;

    use super::*;

    fn volume(mount: &str, name: &str) -> Volume {
        Volume {
            mount: AbsolutePath::new(mount).unwrap(),
            name: name.into(),
            free_bytes: Some(5 << 30),
            needs: vec![proto::Needs { id: 0, bytes: 8 << 30 }],
        }
    }

    proptest! {
        /// A disk is called by the name it has, wherever it's mounted.
        #[test]
        fn a_disk_is_called_by_its_own_name(name in "[A-Za-z0-9 ]{1,12}", mount in "/[a-z]{1,6}(/[a-z]{1,6})?") {
            let v = volume(&mount, &name);
            prop_assert_eq!(disk(&v), name.clone());
            prop_assert_eq!(disk_short(&v), format!("Torrents on {name} need 8.0 GiB; 5.0 GiB free"));
        }
    }

    #[test]
    fn a_disk_with_no_name_is_called_by_its_mount() {
        assert_eq!(disk(&volume("/Volumes/MyDisk", "")), "/Volumes/MyDisk");
    }

    #[test]
    fn a_warning_names_the_disk() {
        let v = volume("/System/Volumes/Data", "Macintosh HD");
        assert_eq!(disk_short(&v), "Torrents on Macintosh HD need 8.0 GiB; 5.0 GiB free");
        assert!(too_big_why(6 << 30, 5 << 30, &v).contains("and Macintosh HD has 5.0 GiB free"));
    }
}
