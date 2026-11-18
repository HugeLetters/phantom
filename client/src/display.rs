//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable), statuses, priorities and what an add
//! did. Shared by the CLI and the app; the service only states the facts.

use std::num::NonZeroU16;
use std::time::Duration;

use proto::{
    AbsolutePath, Action, Added, Answer, CantUndo, Direction, Kept, KeptWhy, LimitProblem, ListenPort, Priority, Ratio, Reached, Refusal,
    SeedTime, SeedingLimit, ServiceError, SpeedLimit, Status, TorrentFailure, TorrentRefusal, Unavailable, UndoAction, Undoable, Volume,
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

/// The file row menu's title over the priorities.
pub fn priority_title() -> &'static str {
    "Priority"
}

/// The item, in the file row menu, that sets files Off and moves them to the Trash.
pub fn move_to_trash() -> &'static str {
    "Move to Trash"
}

/// The popover's button that dismisses every told notice at once, shown while there are several
/// (`n` of them): it says how many go, as live notices, which stay, aren't messages.
pub fn dismiss_all(n: usize) -> String {
    format!("Dismiss {n} messages")
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

/// Why torrent `id`'s row is warned, on the hover over its ⚠ (#209): the disk's whole picture,
/// the same for every torrent that still needs space on it: "MyDisk is short on space: 3.0 GiB
/// free; this and 2 other torrents still need 12.0 GiB." A torrent that alone needs more than is
/// free says so of itself, whatever the others need: "…: 5.0 GiB free; this torrent still needs
/// 6.0 GiB."; so does one alone on its disk. A disk that isn't short, or whose free space isn't
/// known, has nothing to say (`None`).
pub fn short_on_space(volume: &Volume, id: proto::TorrentId) -> Option<String> {
    let free = bytes(volume.free_bytes.filter(|_| volume.short())?);
    let (who, needs) = match (volume.too_big(id), volume.needs.len()) {
        (Some(own), _) => ("this torrent still needs".to_owned(), own),
        (None, 0 | 1) => ("this torrent still needs".to_owned(), volume.needs_bytes()),
        (None, 2) => ("this and 1 other torrent still need".to_owned(), volume.needs_bytes()),
        (None, n) => (format!("this and {} other torrents still need", n - 1), volume.needs_bytes()),
    };
    Some(format!("{} is short on space: {free} free; {who} {}.", disk(volume), bytes(needs)))
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
        Status::Unchecked => "Unchecked",
        Status::Downloading => "Downloading",
        Status::Seeding => "Seeding",
        Status::Paused => "Paused",
        Status::Done { .. } => "Done",
        Status::MissingFiles => "Missing files",
        Status::DestinationUnavailable { .. } => "Destination unavailable",
        Status::Moving => "Moving",
        Status::Error { .. } => "Error",
    }
}

/// The row menu's item that stops a check under way: "Cancel check".
pub fn cancel_check() -> &'static str {
    "Cancel check"
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

/// What a Remove, Delete or Move to Trash that was done tells: "Deleted Album", "Removed 3 torrents",
/// "Moved 01 Intro.flac to the Trash", "Moved 3 files to the Trash". `names` are the torrents', or
/// the one file's.
pub fn happened(action: UndoAction, names: &[String]) -> String {
    let done = match action {
        UndoAction::Remove => "Removed",
        UndoAction::Delete => "Deleted",
        // `names` are the file's, when one was chosen: of several, how many moved is the service's to say.
        UndoAction::TrashFiles { count } => {
            return match (count, names) {
                (1, [one]) => format!("Moved {one} to the Trash"),
                (1, _) => "Moved a file to the Trash".into(),
                (count, _) => format!("Moved {count} files to the Trash"),
            };
        }
    };
    match names {
        [one] => format!("{done} {one}"),
        many => format!("{done} {} torrents", many.len()),
    }
}

/// What a Remove or Delete that was done means for the files of `count` torrents.
pub fn happened_more(action: UndoAction, count: usize) -> String {
    match (action, count) {
        (UndoAction::Remove, 1) => "Its files stay where they are.".into(),
        (UndoAction::Remove, _) => "Their files stay where they are.".into(),
        (UndoAction::Delete, 1) => "Its files are in the Trash.".into(),
        (UndoAction::Delete, _) => "Their files are in the Trash.".into(),
        (UndoAction::TrashFiles { .. }, 1) => "It's set to Off, so it won't be downloaded again.".into(),
        (UndoAction::TrashFiles { .. }, _) => "They're set to Off, so they won't be downloaded again.".into(),
    }
}

/// The action that undoes it.
pub fn undo() -> &'static str {
    "Undo"
}

/// Why a notice's Undo is off (#200): each reason has its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoOff {
    /// Another action came after, which an undo takes back first; or a newer Delete of the torrent
    /// took its place.
    Newer,
    /// It was pushed off the stack by [`DEPTH`](lifecycle::undo::DEPTH) newer ones.
    PastLimit,
    /// Its undo was refused, for this reason.
    Refused(CantUndo, Vec<AbsolutePath>),
}

pub fn undo_off(why: &UndoOff) -> String {
    match why {
        UndoOff::Newer => "A newer Remove, Delete or Move to Trash comes first: undo that one before this".into(),
        UndoOff::PastLimit => {
            format!("Only the last {} Removes, Deletes and Moves to Trash can be undone", lifecycle::undo::DEPTH)
        }
        UndoOff::Refused(why, stranded) => couldnt_undo(&cant_undo_whole(why, stranded)),
    }
}

/// An undo of a torrent that's in the list again (added since): nothing is touched.
pub fn already_in_list() -> &'static str {
    "Already in the list"
}

/// An undo of a selection that brought back only some of it: "Couldn't undo 1 of 3".
pub fn partly_undone(not_back: usize, total: usize) -> String {
    format!("Couldn't undo {not_back} of {total}")
}

/// What came back of an undo of several: "3 torrents are back".
pub fn several_undone(count: usize) -> String {
    format!("{count} torrents are back")
}

fn undo_action(action: UndoAction) -> &'static str {
    match action {
        UndoAction::Remove => "Remove",
        UndoAction::Delete => "Delete",
        UndoAction::TrashFiles { .. } => "Move to Trash",
    }
}

/// What an undo did: "Album is back in the list", "Album is back, with its files", "3 torrents
/// are back".
pub fn undone(undo: &Undoable) -> String {
    let name = undo_names(undo);
    match (undo.action, &undo.torrents[..]) {
        (UndoAction::Remove | UndoAction::Delete, [_, _, ..]) => several_undone(undo.torrents.len()),
        (UndoAction::Remove, _) => format!("{name} is back in the list"),
        (UndoAction::Delete, _) => format!("{name} is back, with its files"),
        (UndoAction::TrashFiles { count: 1 }, _) => format!("A file of {name} is back from the Trash"),
        (UndoAction::TrashFiles { count }, _) => format!("{count} files of {name} are back from the Trash"),
    }
}

/// The torrents of an undo entry: its one torrent's name, or "3 torrents".
fn undo_names(undo: &Undoable) -> String {
    match &undo.torrents[..] {
        [one] => one.name.clone(),
        many => format!("{} torrents", many.len()),
    }
}

/// An undo that couldn't be done, as a notice: "Couldn't undo Delete of Album", and why, with what
/// stayed out of the Trash, if anything did.
pub fn cant_undo(undo: &Undoable, why: &CantUndo, stranded: &[AbsolutePath]) -> (String, String) {
    (format!("Couldn't undo {} of {}", undo_action(undo.action), undo_names(undo)), sentence(&cant_undo_whole(why, stranded)))
}

/// A torrent of an undo of several that didn't come back, as [`cant_undo`] words it: "Couldn't
/// undo Delete of Album", and why; one added again since: "Album is already in the list", and
/// nothing more (nothing was touched).
pub fn not_back(not: &proto::NotBack) -> (String, String) {
    match &not.why {
        CantUndo::InList => (format!("{} is already in the list", not.torrent.name), String::new()),
        why => cant_undo(&Undoable { seq: 0, action: not.action, torrents: vec![not.torrent.clone()] }, why, &not.stranded),
    }
}

fn cant_undo_whole(why: &CantUndo, stranded: &[AbsolutePath]) -> String {
    match stranded {
        [] => cant_undo_why(why),
        paths => {
            let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
            format!("{}. {} couldn't go back to the Trash: it's where it was", cant_undo_why(why), paths.join(", "))
        }
    }
}

fn cant_undo_why(why: &CantUndo) -> String {
    match why {
        // As the app says it ("Already in the list"): added again since, nothing touched.
        CantUndo::InList => already_in_list().to_lowercase(),
        CantUndo::NotInList => "it isn't in the list any more".into(),
        CantUndo::Deleting => "it's being deleted".into(),
        CantUndo::NotInTrash { path } => format!("{} isn't in the Trash any more", path.display()),
        CantUndo::PathTaken { path } => format!("something else is at {} now", path.display()),
        CantUndo::NotAFolder { path } => format!("{} isn't a folder: a link or a file is there now", path.display()),
        CantUndo::NoDestination => "Phantom doesn't know where its files go".into(),
        CantUndo::NotKept => "what Phantom kept of it can't be read".into(),
        CantUndo::AddFailed { detail } => format!("adding it again failed ({detail}), so its files went back to the Trash"),
        CantUndo::PriorityFailed { detail } => {
            format!("setting the priorities of its files again failed ({detail}), so they went back to the Trash")
        }
        CantUndo::Destination { reason } => format!("its destination is unavailable. {}", unavailable(reason)),
        CantUndo::Failed { detail } => format!("it failed ({detail})"),
        CantUndo::Error { error } => format!("it failed: {}", service_error(error).to_lowercase()),
        CantUndo::Refused { refusal: why } => format!("it was refused: {}", refusal(why)),
        CantUndo::PutBackFailed { path, detail } => {
            format!("{} couldn't go back ({detail}), so everything stays in the Trash", path.display())
        }
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
        "look_preset" => "Theme",
        key => key,
    }
}

/// A setting's short name, as an undo of it tells it ("Port is back to 6881") and Edit ▸ Undo names
/// it ([`undo_change`]); one without a short name, as it is.
pub fn setting_short(key: &str) -> &str {
    match key {
        "listen_port" => "Port",
        "default_destination" => "Default folder",
        "seed_ratio" => "Seeding ratio",
        "seed_time" => "Seeding time",
        key => setting_label(key),
    }
}

/// What an undo tells of a setting `short` ([`setting_short`]) it took back: "Port is back to
/// 6881"; a theme setting split by Appearance is named with it, "Light Tone is back to Dark".
pub fn back_to(short: &str, value: &str) -> String {
    format!("{short} is back to {value}")
}

/// Edit ▸ Undo naming the change it takes back: "Undo Port Change".
pub fn undo_change(short: &str) -> String {
    let title: Vec<String> = short
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
        })
        .collect();
    format!("Undo {} Change", title.join(" "))
}

/// The value of the service setting `name` in `settings`, as an undo tells it: "6881", "automatic
/// (now 51413)" for a port the service picked, "on", "unlimited", the folder, "none".
pub fn setting_value(name: proto::SettingName, settings: &proto::Settings) -> String {
    use proto::SettingName;
    let on = |on: bool| if on { "on" } else { "off" }.to_owned();
    match name {
        SettingName::DefaultDestination => settings.default_destination.as_ref().map_or("none".into(), |f| f.display().to_string()),
        SettingName::ListenPort => match settings.listen_port {
            proto::ListenPort::Pinned(port) => port.to_string(),
            proto::ListenPort::Picked(port) => format!("automatic (now {port})"),
        },
        SettingName::Upnp => on(settings.upnp),
        SettingName::KeepAwake => on(settings.keep_awake),
        SettingName::DownloadLimit => settings.speed_limits.download.map_or("unlimited".into(), speed_limit),
        SettingName::UploadLimit => settings.speed_limits.upload.map_or("unlimited".into(), speed_limit),
        SettingName::SeedRatio => settings.seeding.ratio.map_or("never".into(), ratio),
        SettingName::SeedTime => settings.seeding.time.map_or("never".into(), seed_time),
    }
}

/// What an undo of the service setting `name` tells: "Port is back to 6881".
pub fn setting_undone(name: proto::SettingName, settings: &proto::Settings) -> String {
    back_to(setting_short(name.key()), &setting_value(name, settings))
}

/// What a refused undo says: "Couldn't be undone: the service is stopping" (#200's words).
pub fn couldnt_undo(why: &str) -> String {
    format!("Couldn't be undone: {why}")
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

/// What a settings-reset notice calls a theme setting reset in one Appearance's look, by its
/// label ("Tone in Light"), or, without one, that look's theme ("Theme in Dark").
pub fn theme_reset(setting: Option<&str>, appearance: &str) -> String {
    format!("{} in {appearance}", setting.unwrap_or("Theme"))
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
        TorrentFailure::DiskFull => "The disk is full: free some space, then Resume".into(),
        TorrentFailure::NotTrashed { kept } => sentence(&not_trashed(name, kept)),
    }
}

/// Why a request failed, as the service says it: Phantom's part worded here, the text of
/// librqbit, the network or the OS as it is.
pub fn service_error(error: &ServiceError) -> String {
    match error {
        ServiceError::BadRequest { detail } => format!("bad request: {detail}"),
        ServiceError::NotHttpUrl { url } => format!("{url} isn't an http(s) URL"),
        ServiceError::TorrentUrlStatus { url, status, phrase } => {
            let answer = match phrase {
                Some(phrase) => format!("{status} {phrase}"),
                None => status.to_string(),
            };
            format!("downloading the .torrent: {url} answered {answer}")
        }
        ServiceError::TorrentDownload { detail } => format!("can't download the .torrent: {detail}"),
        ServiceError::InvalidMagnet => "not a valid magnet link".into(),
        ServiceError::NoV1InfoHash => "the magnet link has no BitTorrent v1 info hash".into(),
        ServiceError::InvalidTorrentFile { detail } => format!("not a valid .torrent file: {detail}"),
        ServiceError::AddAgainFailed { id, detail } => format!("torrent {id} failed to add it again: {detail}"),
        ServiceError::NotRestarted { detail } => format!("the torrents didn't start again with the new settings: {detail}"),
        ServiceError::Internal { detail } => detail.clone(),
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
        TorrentRefusal::NotChecking => "Not being checked".into(),
        TorrentRefusal::FetchFailed => "No info: fetching it failed".into(),
        TorrentRefusal::Deleting => "Being deleted".into(),
        TorrentRefusal::PartlyDeleted => "Partly in the Trash: delete it again, or remove it".into(),
        TorrentRefusal::NoFile { index } => format!("No file {index}"),
        TorrentRefusal::NotTrashed { kept, .. } if no_trash(kept) => "The drive has no Trash".into(),
        TorrentRefusal::NotTrashed { name, kept } => sentence(&not_trashed(name, kept)),
        TorrentRefusal::FilesNotTrashed { kept, .. } if no_trash(kept) => "The drive has no Trash to move its files to".into(),
        TorrentRefusal::FilesNotTrashed { name, kept } => sentence(&files_not_trashed(name, kept)),
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
        Refusal::NothingToUndo => "nothing to undo".into(),
        Refusal::Undone => "undone before it finished".into(),
        Refusal::Undo { undo, why, stranded } => {
            format!("can't undo {} of {}: {}", undo_action(undo.action), undo_names(undo), cant_undo_whole(why, stranded))
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
        Action::CancelCheck => "cancel the check of",
        Action::SetPriority => "change the priority of",
        Action::TrashFiles => "move files to the Trash of",
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

/// A failed command for one torrent called `name`, as an error notice's line: what, and why under
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
        Failure::Refused(Refusal::Torrent { why: TorrentRefusal::FilesNotTrashed { name, kept }, .. }) if no_trash(kept) => {
            (files_not_trashed(name, kept), String::new())
        }
        Failure::Refused(Refusal::Torrent { why: TorrentRefusal::FilesNotTrashed { name, kept }, .. }) => {
            (name.clone(), sentence(&files_not_trashed(name, kept)))
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

/// What Move to Trash of files of the torrent called `name` left where it was, and why: as
/// [`not_trashed`], for files. They're Off all the same.
pub fn files_not_trashed(name: &str, kept: &[Kept]) -> String {
    let paths: Vec<String> = kept.iter().map(|k| k.path.display().to_string()).collect();
    let paths = paths.join(", ");
    let what = if kept.len() == 1 { "it is" } else { "they are" };
    match kept.iter().find_map(|k| match &k.why {
        KeptWhy::Failed { detail } => Some(detail),
        KeptWhy::NoTrash | KeptWhy::Interrupted => None,
    }) {
        _ if no_trash(kept) => format!("Couldn't move files of {name} to the Trash: the drive has no Trash"),
        Some(detail) => format!("couldn't move every file to the Trash: {detail} ({what} still there, set to Off: {paths})"),
        None => format!("Phantom stopped before moving every file to the Trash ({what} still there, set to Off: {paths})"),
    }
}

/// Why the torrent called `name` (or "torrent 3") refuses, lowercase where the name is: the CLI
/// names it, since nothing is selected there.
fn torrent(name: &str, why: &TorrentRefusal) -> String {
    match why {
        TorrentRefusal::NotInList => format!("{name} isn't in the list"),
        TorrentRefusal::NoInfoYet => format!("{name} has no info yet"),
        TorrentRefusal::NotChecking => format!("{name} isn't being checked"),
        TorrentRefusal::FetchFailed => format!("{name} has no info: fetching it failed"),
        TorrentRefusal::Deleting => format!("{name} is being deleted"),
        TorrentRefusal::PartlyDeleted => format!("{name} is partly in the Trash: delete it again, or remove it"),
        TorrentRefusal::NoFile { index } => format!("{name} has no file {index}"),
        // Names the torrent itself.
        TorrentRefusal::NotTrashed { name, kept } if no_trash(kept) => not_trashed(name, kept),
        TorrentRefusal::NotTrashed { name: torrent, kept } => format!("{name}: {}", not_trashed(torrent, kept)),
        TorrentRefusal::FilesNotTrashed { name, kept } if no_trash(kept) => files_not_trashed(name, kept),
        TorrentRefusal::FilesNotTrashed { name: torrent, kept } => format!("{name}: {}", files_not_trashed(torrent, kept)),
        TorrentRefusal::DestinationUnavailable { action, reason } => {
            let what = match action {
                Action::Pause => "pause",
                Action::Resume => "start",
                Action::Check => "be checked",
                Action::CancelCheck => "have its check cancelled",
                Action::SetPriority => "change its priorities",
                Action::TrashFiles => "have its files moved to the Trash",
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

    /// A Move to Trash is worded by how many files the service says moved; the file is named only when
    /// it's the one chosen.
    #[test]
    fn a_move_to_trash_says_how_many_files_moved() {
        let files = |count| UndoAction::TrashFiles { count };
        assert_eq!(happened(files(1), &["01 Intro.flac".into()]), "Moved 01 Intro.flac to the Trash");
        assert_eq!(happened(files(1), &[]), "Moved a file to the Trash", "one of several chosen: unnamed");
        assert_eq!(happened(files(3), &[]), "Moved 3 files to the Trash");
        assert_eq!(happened_more(files(1), 1), "It's set to Off, so it won't be downloaded again.");
        assert_eq!(happened_more(files(3), 3), "They're set to Off, so they won't be downloaded again.");
        let undo = |count| Undoable { seq: 1, action: files(count), torrents: vec![proto::Named { id: 3, name: "Album".into() }] };
        assert_eq!(undone(&undo(1)), "A file of Album is back from the Trash");
        assert_eq!(undone(&undo(2)), "2 files of Album are back from the Trash");
        let (short, why) = cant_undo(&undo(2), &CantUndo::NotInTrash { path: AbsolutePath::new("/d/Album/a.flac").unwrap() }, &[]);
        assert_eq!(short, "Couldn't undo Move to Trash of Album");
        assert_eq!(why, "/d/Album/a.flac isn't in the Trash any more");
    }

    /// A failed part is worded as any service error or refusal, never as debug text.
    #[test]
    fn an_undo_that_failed_or_was_refused_otherwise_is_worded() {
        let error = CantUndo::Error { error: ServiceError::AddAgainFailed { id: 3, detail: "no room".into() } };
        assert!(!cant_undo_why(&error).contains("AddAgainFailed"), "{}", cant_undo_why(&error));
        let refused = CantUndo::Refused { refusal: Box::new(Refusal::Stopping) };
        assert_eq!(cant_undo_why(&refused), "it was refused: the service is stopping");
    }

    #[test]
    fn an_undo_of_a_torrent_in_the_list_again_says_so_as_the_app_does() {
        let undo = Undoable { seq: 1, action: UndoAction::Remove, torrents: vec![proto::Named { id: 3, name: "X".into() }] };
        let refused = Refusal::Undo { undo, why: CantUndo::InList, stranded: Vec::new() };
        assert_eq!(refusal(&refused), "can't undo Remove of X: already in the list");
    }

    /// An undo of a selection says how many came back, and of one that didn't, which and why (#235).
    #[test]
    fn an_undo_of_several_says_how_many_are_back_and_which_one_isnt() {
        let named = |id: u64| proto::Named { id, name: format!("T{id}") };
        let undo = Undoable { seq: 1, action: UndoAction::Delete, torrents: (1..=3).map(named).collect() };
        assert_eq!(undone(&undo), "3 torrents are back");
        assert_eq!(cant_undo(&undo, &CantUndo::NotKept, &[]).0, "Couldn't undo Delete of 3 torrents");
        let not = proto::NotBack { torrent: named(2), action: UndoAction::Delete, why: CantUndo::NotKept, stranded: Vec::new() };
        assert_eq!(not_back(&not), ("Couldn't undo Delete of T2".into(), "What Phantom kept of it can't be read".into()));
        let again = proto::NotBack { why: CantUndo::InList, ..not };
        assert_eq!(not_back(&again), ("T2 is already in the list".into(), String::new()), "nothing touched: nothing more to say");
    }

    #[test]
    fn statuses_priorities_adds_and_link_problems_have_their_words() {
        assert_eq!(status(&Status::FetchingInfo), "Fetching info");
        assert_eq!(status(&Status::Unchecked), "Unchecked");
        assert_eq!(status(&Status::Error { failure: TorrentFailure::Librqbit { detail: "x".into() } }), "Error");
        let fetch = Status::Error { failure: TorrentFailure::FetchFailed { detail: "no peers".into() } };
        assert_eq!(status_reason("x", &fetch).as_deref(), Some("Couldn't fetch its info: no peers"));
        let again = Status::Error { failure: TorrentFailure::AddAgainFailed { detail: "disk full".into() } };
        assert_eq!(status_reason("x", &again).as_deref(), Some("Couldn't check it: disk full"), "Phantom's part worded here");
        let full = Status::Error { failure: TorrentFailure::DiskFull };
        assert_eq!(status_reason("x", &full).as_deref(), Some("The disk is full: free some space, then Resume"));
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
    fn a_service_error_has_the_words_the_service_used_to_write() {
        let says = |error: ServiceError| service_error(&error);
        assert_eq!(says(ServiceError::BadRequest { detail: "eof".into() }), "bad request: eof");
        assert_eq!(says(ServiceError::NotHttpUrl { url: "ftp://x".into() }), "ftp://x isn't an http(s) URL");
        let status = |phrase: Option<&str>| ServiceError::TorrentUrlStatus {
            url: "http://x/a.torrent".into(),
            status: 404,
            phrase: phrase.map(str::to_owned),
        };
        assert_eq!(says(status(Some("Not Found"))), "downloading the .torrent: http://x/a.torrent answered 404 Not Found");
        assert_eq!(says(status(None)), "downloading the .torrent: http://x/a.torrent answered 404");
        assert_eq!(says(ServiceError::TorrentDownload { detail: "refused".into() }), "can't download the .torrent: refused");
        assert_eq!(says(ServiceError::InvalidMagnet), "not a valid magnet link");
        assert_eq!(says(ServiceError::NoV1InfoHash), "the magnet link has no BitTorrent v1 info hash");
        assert_eq!(says(ServiceError::InvalidTorrentFile { detail: "eof".into() }), "not a valid .torrent file: eof");
        assert_eq!(says(ServiceError::AddAgainFailed { id: 3, detail: "x".into() }), "torrent 3 failed to add it again: x");
        assert_eq!(
            says(ServiceError::NotRestarted { detail: "no room".into() }),
            "the torrents didn't start again with the new settings: no room"
        );
        assert_eq!(says(ServiceError::Internal { detail: "librqbit's own text".into() }), "librqbit's own text");
        assert_eq!(crate::Failure::Failed(ServiceError::InvalidMagnet).to_string(), "not a valid magnet link");
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

    const KEYS: [&str; 10] = [
        "listen_port",
        "upnp",
        "default_destination",
        "keep_awake",
        "download_limit",
        "upload_limit",
        "seed_ratio",
        "seed_time",
        "app_icon",
        "look_preset",
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
        assert_eq!(refusal(&Refusal::Torrent { id: 3, why: TorrentRefusal::NotChecking }), "torrent 3 isn't being checked");
    }

    /// Every reason a button can be off, each kind once: a new one fails to compile here.
    fn every_answer_off() -> Vec<proto::Answer> {
        use proto::{Answer, Why};
        let path = || proto::AbsolutePath::new("/d/a").unwrap();
        let kept = vec![Kept { path: path(), why: KeptWhy::Failed { detail: "locked".into() } }];
        let kept_files = kept.clone();
        let no_trash = vec![Kept { path: path(), why: KeptWhy::NoTrash }];
        let why = [Why::AlreadyStopped, Why::AlreadyRunning, Why::AlreadyChecking, Why::NoFilesYet, Why::FilesMissing];
        let refusals = [
            TorrentRefusal::NotInList,
            TorrentRefusal::NoInfoYet,
            TorrentRefusal::NotChecking,
            TorrentRefusal::FetchFailed,
            TorrentRefusal::Deleting,
            TorrentRefusal::PartlyDeleted,
            TorrentRefusal::NoFile { index: 2 },
            TorrentRefusal::NotTrashed { name: "Album".into(), kept },
            TorrentRefusal::NotTrashed { name: "Album".into(), kept: no_trash.clone() },
            TorrentRefusal::FilesNotTrashed { name: "Album".into(), kept: kept_files },
            TorrentRefusal::FilesNotTrashed { name: "Album".into(), kept: no_trash },
            TorrentRefusal::DestinationUnavailable {
                action: Action::SetPriority,
                reason: Unavailable::DriveNotConnected { name: "MyDisk".into() },
            },
        ];
        for r in &refusals {
            match r {
                TorrentRefusal::NotInList
                | TorrentRefusal::NoInfoYet
                | TorrentRefusal::NotChecking
                | TorrentRefusal::FetchFailed
                | TorrentRefusal::Deleting
                | TorrentRefusal::PartlyDeleted
                | TorrentRefusal::NoFile { .. }
                | TorrentRefusal::NotTrashed { .. }
                | TorrentRefusal::FilesNotTrashed { .. }
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
        fn the_note_names_no_torrent_and_counts_none(n in 1usize..40, k in 0usize..17) {
            let answers = every_answer_off();
            let answer = answers[k % answers.len()].clone();
            // The refusal that carries a name is the one that must not show it.
            let named = match answer {
                proto::Answer::Refused { why: TorrentRefusal::NotTrashed { kept, .. } } => {
                    proto::Answer::Refused { why: TorrentRefusal::NotTrashed { name: "Movie 0".into(), kept } }
                }
                proto::Answer::Refused { why: TorrentRefusal::FilesNotTrashed { kept, .. } } => {
                    proto::Answer::Refused { why: TorrentRefusal::FilesNotTrashed { name: "Movie 0".into(), kept } }
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
            counts in proptest::collection::vec(0usize..4, 17),
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
    use proto::{AbsolutePath, Needs};

    use super::*;

    /// A disk with `free` GiB and torrents 0, 1, … needing `needs` GiB each.
    fn volume(mount: &str, name: &str, free: u64, needs: &[u64]) -> Volume {
        Volume {
            mount: AbsolutePath::new(mount).unwrap(),
            name: name.into(),
            free_bytes: Some(free << 30),
            needs: needs.iter().enumerate().map(|(id, gib)| Needs { id: id as u64, bytes: gib << 30 }).collect(),
        }
    }

    proptest! {
        /// A disk is called by the name it has, wherever it's mounted, and the hover names it.
        #[test]
        fn a_disk_is_called_by_its_own_name(name in "[A-Za-z0-9 ]{1,12}", mount in "/[a-z]{1,6}(/[a-z]{1,6})?") {
            let v = volume(&mount, &name, 3, &[12]);
            prop_assert_eq!(disk(&v), name.clone());
            let said = short_on_space(&v, 0).unwrap();
            prop_assert!(said.starts_with(&format!("{name} is short on space: ")), "{}", said);
        }
    }

    #[test]
    fn a_disk_with_no_name_is_called_by_its_mount() {
        assert_eq!(disk(&volume("/Volumes/MyDisk", "", 3, &[12])), "/Volumes/MyDisk");
    }

    #[test]
    fn a_disk_that_isnt_short_has_nothing_to_say() {
        let mut v = volume("/Volumes/MyDisk", "MyDisk", 20, &[6, 6]);
        assert_eq!(short_on_space(&v, 0), None);
        v.free_bytes = None;
        assert_eq!(short_on_space(&v, 0), None, "free space not known: not short");
    }

    /// The three cases: one that can't fit even alone says so of itself, whatever the others need;
    /// the others say the disk's whole picture; one alone on its disk says "this torrent".
    #[test]
    fn the_hover_says_the_disk_s_picture_or_a_torrents_own() {
        // 5 GiB free; torrents needing 6, 4 and 2: the first can't fit alone.
        let v = volume("/Volumes/MyDisk", "MyDisk", 5, &[6, 4, 2]);
        assert_eq!(short_on_space(&v, 0).as_deref(), Some("MyDisk is short on space: 5.0 GiB free; this torrent still needs 6.0 GiB."));
        for id in [1, 2] {
            assert_eq!(
                short_on_space(&v, id).as_deref(),
                Some("MyDisk is short on space: 5.0 GiB free; this and 2 other torrents still need 12.0 GiB."),
                "torrent {id}, which fits alone"
            );
        }
        // Two torrents, neither too big alone.
        let two = volume("/Volumes/MyDisk", "MyDisk", 5, &[3, 3]);
        assert_eq!(
            short_on_space(&two, 1).as_deref(),
            Some("MyDisk is short on space: 5.0 GiB free; this and 1 other torrent still need 6.0 GiB.")
        );
        // Alone on its disk, and too big: "this torrent".
        let alone = volume("/Volumes/MyDisk", "MyDisk", 3, &[12]);
        assert_eq!(
            short_on_space(&alone, 0).as_deref(),
            Some("MyDisk is short on space: 3.0 GiB free; this torrent still needs 12.0 GiB.")
        );
        // One that isn't among those that need space has nothing to say of its own: it's never asked.
    }
}
