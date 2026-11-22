//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable), statuses, priorities and what an add
//! did. Shared by the CLI and the app; the service only states the facts.

use std::num::NonZeroU16;
use std::time::Duration;

use proto::{
    AbsolutePath, Action, Added, Answer, CantUndo, Direction, Found, Kept, KeptWhy, LimitProblem, ListenPort, NotMoved, Priority, Ratio,
    Reached, Refusal, SeedTime, SeedingLimit, ServiceError, SpeedLimit, Status, TorrentFailure, TorrentRefusal, Unavailable, UndoAction,
    Undoable, Volume, Why as AnswerWhy,
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

/// The popover's button that dismisses every notice at once, shown while there are several.
pub fn dismiss_all() -> &'static str {
    "Dismiss all"
}

/// The action that shows a file in Finder.
pub fn reveal_in_finder() -> &'static str {
    "Reveal in Finder"
}

/// The status bar's pill after the newest notice's line: how many notices there are, a count, not
/// a place among them.
pub fn notices_count(total: usize) -> String {
    total.to_string()
}

/// What follows the bar's notice: › for "the list opens".
pub fn notices_open_glyph() -> &'static str {
    "›"
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
        Status::Moving { .. } => "Moving",
        Status::WaitingToMove => "Waiting to move",
        Status::Error { .. } => "Error",
    }
}

/// How far a move is, beside its status, while it copies: "1.2 GiB of 3.0 GiB". None while
/// nothing is copied (a rename on the same drive, a move ending or going back).
pub fn move_progress(status: &Status) -> Option<String> {
    match status {
        Status::Moving { done, total } if *total > 0 => Some(format!("{} of {}", bytes(*done), bytes(*total))),
        _ => None,
    }
}

/// The row menu's item that moves a torrent's files: "Move to…".
pub fn move_to() -> &'static str {
    "Move to…"
}

/// The row menu's item that takes a Move out of the queue: "Cancel move".
pub fn cancel_move() -> &'static str {
    "Cancel move"
}

/// The title of the folder picker of a Move: "Move Album to".
pub fn move_picker(name: &str) -> String {
    format!("Move {name} to")
}

/// The title of the folder picker of a Move of `n` torrents: "Move 3 torrents to".
pub fn move_picker_several(n: usize) -> String {
    format!("Move {n} torrents to")
}

/// What a Move of a selection tells, as the bar's line, once it ended with some not moved: "Moved
/// 2 of 3". The torrents that didn't, and why, are its list.
pub fn moved_of(moved: usize, selected: usize) -> String {
    format!("Moved {moved} of {selected}")
}

fn folder_name(to: &AbsolutePath) -> String {
    to.file_name().map_or_else(|| to.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The row menu's Move to, which opens a submenu of folders (#87): no "…", as it asks nothing.
pub fn move_to_submenu() -> &'static str {
    "Move to"
}

/// Beside the default folder in Move to ▸.
pub fn default_folder() -> &'static str {
    "Default"
}

/// Move to ▸'s last item, which asks for a folder.
pub fn other_folder() -> &'static str {
    "Other…"
}

/// The palette's entry for a folder Move to ▸ offers: "Move to Films".
pub fn move_to_folder(folder: &str) -> String {
    format!("Move to {folder}")
}

/// The clash dialog's title, the question it asks, with where it is among several: "Album is
/// already in Films · 2 of 3". One alone has no counter.
pub fn clash_title(question: &str, at: usize, of: usize) -> String {
    match of {
        0 | 1 => question.to_owned(),
        _ => format!("{question} · {at} of {of}"),
    }
}

/// What a clash asks, before a Move: "3 files of Album are already in Films".
pub fn clash_question(name: &str, files: &[AbsolutePath], to: &AbsolutePath) -> String {
    let folder = folder_name(to);
    match files {
        [one] => match one.file_name().map(|n| n.to_string_lossy().into_owned()) {
            // A torrent of one file is named after it.
            Some(file) if file == name => format!("{name} is already in {folder}"),
            Some(file) => format!("{file} of {name} is already in {folder}"),
            None => format!("{} of {name} is already in {folder}", one.display()),
        },
        _ => format!("{} files of {name} are already in {folder}", files.len()),
    }
}

/// What a clash with another listed torrent's files says (#270), as its title: "Album can't go
/// into Films".
pub fn clash_question_another(name: &str, to: &AbsolutePath) -> String {
    format!("{name} can't go into {}", folder_name(to))
}

/// Over the other torrent's path, in a clash with its files.
pub fn clash_another_keeps() -> &'static str {
    "Another torrent in the list keeps its files there:"
}

/// The clash dialog's button that shows the other torrent, in a clash with its files.
pub fn clash_show_in_list() -> &'static str {
    "Show in list"
}

/// Under the paths a clash shows, for those it leaves out: "and 2 more".
pub fn clash_more(more: usize) -> String {
    format!("and {more} more")
}

/// What the clash dialog asks once Rename… is pressed: "A new name for Album in Films".
pub fn clash_new_name(name: &str, to: &AbsolutePath) -> String {
    format!("A new name for {name} in {}", folder_name(to))
}

/// The choices a clash offers, in order: use what's there, put the torrent's over it, don't move,
/// show them, and move it in under another name; and the name field's own, which moves it.
pub fn clash_choices() -> [&'static str; 6] {
    ["Use existing", "Replace", "Cancel", "Reveal in Finder", "Rename…", "Move"]
}

/// The clash dialog's checkbox when `remaining` more torrents of the selection clash: "Apply to
/// the remaining 2".
pub fn clash_apply_to_remaining(remaining: usize) -> String {
    format!("Apply to the remaining {remaining}")
}

/// What each of a clash's choices does, as its tooltip: Use existing, Replace, Rename….
pub fn clash_choice_help() -> [&'static str; 3] {
    ["Keep the files there; this torrent's go to the Trash", "Move the files there to the Trash", "Move it in under another name"]
}

/// Why a Move didn't happen, as a sentence's tail: "the new destination's disk is full".
pub fn not_moved(why: &NotMoved) -> String {
    match why {
        NotMoved::Unavailable { reason } => unavailable(reason),
        NotMoved::DiskFull => "The new destination's disk is full".into(),
        NotMoved::CopyGone { path } => format!("{} went from the new destination while it was copied", path.display()),
        NotMoved::NotAFolder { path } => format!("Something at {} is in the way", path.display()),
        NotMoved::Taken { path } => format!("Something was put at {} while it moved", path.display()),
        NotMoved::Cancelled => "Cancelled".into(),
        NotMoved::Interrupted => "Phantom stopped while it moved".into(),
        NotMoved::Failed { detail } => detail.clone(),
    }
}

/// [`not_moved`] for a list: no word that repeats the line, a path on a line of its own.
pub fn not_moved_reason(why: &NotMoved) -> Reason {
    match why {
        NotMoved::Unavailable { reason } => unavailable_reason(reason),
        NotMoved::CopyGone { path } => Reason::at("Gone from the new destination while it was copied", path),
        NotMoved::NotAFolder { path } => Reason::at("Something is in the way at", path),
        NotMoved::Taken { path } => Reason::at("Something was put there while it moved", path),
        other => Reason::new(not_moved(other)),
    }
}

fn paths(files: &[AbsolutePath]) -> String {
    files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
}

/// The row menu's item that turns the torrent's name into a field (#331): it needs the new name.
pub fn rename() -> &'static str {
    "Rename…"
}

/// A name that differs from the torrent's only in case, on a drive that doesn't tell cases apart.
fn only_normalization() -> &'static str {
    "Only the Unicode form of its letters differs, and this drive doesn't tell them apart"
}

fn only_case() -> &'static str {
    "Only the case differs, and this drive doesn't tell cases apart"
}

/// Why a name typed for a Rename isn't one a file can have (#331): "A name can't contain “/”".
pub fn name_problem(problem: proto::NameProblem) -> String {
    use proto::NameProblem as P;
    match problem {
        P::Empty => "A name can't be empty".into(),
        P::Slash => "A name can't contain “/”".into(),
        P::Colon => "A name can't contain “:”".into(),
        P::Dots => "A name can't be “.” or “..”".into(),
        P::TooLong => "This name is too long".into(),
        P::Nul => "A name can't contain a NUL character".into(),
        P::Phantoms => "Names starting with “.phantom-” are Phantom's own".into(),
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

/// That Copy magnet link worked, as the bar's line: "Copied the magnet link". The torrent's name
/// is the list's.
pub fn copied_magnet() -> &'static str {
    "Copied the magnet link"
}

/// What an add did, as the bar's line: "Added the torrent", or "Already in the list". The
/// torrent's name ([`Added::name`]) is the list's.
pub fn added_line(added: &Added) -> &'static str {
    match added.already_existed {
        false => "Added the torrent",
        true => already_in_list(),
    }
}

/// What a notice's list shows of the torrents or files it tells of: one a line.
pub fn names_listed(names: &[String]) -> String {
    names.join("\n")
}

/// A line of a notice's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A name or a text of Phantom's, in the text's own colour; it wraps.
    Name(String),
    /// A reason or a label, muted; it wraps.
    Muted(String),
    /// A path or a link, on a line of its own: shortened in the middle when it doesn't fit, whole
    /// on hover. Never inside a sentence.
    Path(String),
}

impl Line {
    /// What the line says.
    pub fn text(&self) -> &str {
        match self {
            Line::Name(t) | Line::Muted(t) | Line::Path(t) => t,
        }
    }
}

/// What a notice's list says under its line, as data: groups of lines, a group (a name and what's
/// to say of it) set apart from the next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub groups: Vec<Vec<Line>>,
}

impl Listing {
    /// `names`, a line each, then `why`, muted.
    pub fn new(names: &str, why: impl Into<String>) -> Self {
        let mut groups: Vec<Vec<Line>> = names.lines().filter(|n| !n.is_empty()).map(|n| vec![Line::Name(n.to_owned())]).collect();
        let why = why.into();
        if !why.is_empty() {
            groups.push(vec![Line::Muted(why)]);
        }
        Self { groups }
    }

    /// Each of `failures`: what failed, and why under it.
    pub fn of_failures(failures: impl IntoIterator<Item = (String, String)>) -> Self {
        let groups = failures
            .into_iter()
            .map(|(what, why)| std::iter::once(Line::Name(what)).chain((!why.is_empty()).then_some(Line::Muted(why))).collect())
            .collect();
        Self { groups }
    }

    /// Each of `failures`: what failed (a link, a file or a torrent's name), and its reason under it.
    pub fn of_entries<'a>(failures: impl IntoIterator<Item = (&'a str, &'a Reason)>) -> Self {
        Self { groups: failures.into_iter().map(|(what, why)| std::iter::once(add_what(what)).chain(why.lines()).collect()).collect() }
    }

    /// One group: `first`, then the `reason`'s lines.
    pub fn failure(first: Line, reason: &Reason) -> Self {
        Self { groups: vec![std::iter::once(first).chain(reason.lines()).collect()] }
    }

    /// `self` with one more group at its end.
    pub fn then(mut self, group: Vec<Line>) -> Self {
        if !group.is_empty() {
            self.groups.push(group);
        }
        self
    }

    /// Every line, in order.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.groups.iter().flatten()
    }

    /// Its lines, one each, as plain text.
    pub fn plain(&self) -> String {
        self.lines().map(Line::text).collect::<Vec<_>>().join("\n")
    }

    /// Every text it shows, for measuring.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.lines().map(Line::text).filter(|t| !t.is_empty())
    }

    /// Whether a group has more than one line: the groups are then set further apart.
    pub fn is_apart(&self) -> bool {
        self.groups.iter().any(|g| g.len() > 1)
    }
}

/// Why something didn't happen, split for a list: words, then the paths each on a line of its own,
/// under a `label` when the words don't lead into them ("Still at:").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reason {
    pub text: String,
    pub label: String,
    pub paths: Vec<String>,
}

impl Reason {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), label: String::new(), paths: Vec::new() }
    }

    /// `text`, which leads into `path`: "Something is already at".
    pub fn at(text: impl Into<String>, path: &std::path::Path) -> Self {
        Self { paths: vec![path.display().to_string()], ..Self::new(text) }
    }

    /// `self`, with `paths` under `label`.
    pub fn still(self, label: &str, paths: Vec<String>) -> Self {
        Self { label: label.into(), paths, ..self }
    }

    /// The lines it makes: the words and the label muted, the paths each a line.
    pub fn lines(&self) -> Vec<Line> {
        let words = [&self.text, &self.label].into_iter().filter(|t| !t.is_empty()).map(|t| Line::Muted(t.clone()));
        words.chain(self.paths.iter().map(|p| Line::Path(p.clone()))).collect()
    }

    /// On one line, for where a list isn't (the CLI, a tooltip): "Something is already at: /a/b".
    pub fn inline(&self) -> String {
        match (self.paths.is_empty(), self.label.is_empty()) {
            (true, _) => self.text.clone(),
            (false, true) => format!("{}: {}", self.text, self.paths.join(", ")),
            (false, false) => format!("{}. {} {}", self.text, self.label, self.paths.join(", ")),
        }
    }
}

impl From<String> for Reason {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for Reason {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

/// An old and a new value, in a list: "10 MB/s → 2 MB/s".
pub fn changed(old: &str, new: &str) -> String {
    format!("{old} → {new}")
}

/// `text` without the OS's own code in its tail: "Operation not permitted (os error 1)" is
/// "Operation not permitted".
pub fn without_os_code(text: &str) -> &str {
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => &text[..at],
        _ => text,
    }
}

/// What a link or a file named in a failed add's list is: a magnet link with a name says the name;
/// a link without one is the link, on a line of its own, shortened in the middle; a file is its name.
pub fn add_what(what: &str) -> Line {
    if let Ok(crate::Link::Magnet { name: Some(name), .. }) = crate::parse_link(what) {
        return Line::Name(name);
    }
    let link = ["magnet:", "http://", "https://"].iter().any(|scheme| what.starts_with(scheme));
    if link { Line::Path(what.to_owned()) } else { Line::Name(what.to_owned()) }
}

/// What the status bar says when opening or revealing a file in Finder failed: "Can't open the
/// file". The path and the reason are the list's ([`show_failed_more`]).
pub fn cant_show(action: Action) -> &'static str {
    match action {
        Action::Reveal => "Can't reveal the file",
        _ => "Can't open the file",
    }
}

/// What the status bar says when Finder itself can't be started: "Can't open Finder". The reason
/// is the list's.
pub fn cant_start_finder() -> &'static str {
    "Can't open Finder"
}

/// The list of a failed Open or Reveal: the path on a line of its own, then why.
pub fn show_failed_listing(path: &std::path::Path, reason: &str) -> Listing {
    Listing { groups: vec![vec![Line::Path(path.display().to_string()), Line::Muted(sentence(reason))]] }
}

/// The button of a failed Open or Reveal that opens the file's folder, when that is on disk.
pub fn show_folder() -> &'static str {
    "Show folder"
}

/// Why Open or Reveal found nothing to show.
pub fn not_on_disk() -> &'static str {
    "It isn't on disk"
}

/// What an add did, as the CLI prints it: "Added Big Buck Bunny", or "Big Buck Bunny is already in
/// the list". The status bar's line is [`added_line`].
pub fn added(added: &Added) -> String {
    match (&added.name, added.already_existed) {
        (Some(name), false) => format!("Added {name}"),
        (Some(name), true) => format!("{name} is already in the list"),
        (None, false) => "Added the torrent".into(),
        (None, true) => "The torrent is already in the list".into(),
    }
}

/// What a Remove, Delete or Move to Trash that was done tells, as the bar's line: "Deleted",
/// "Removed 3 torrents", "Moved a file to the Trash", "Moved 3 files to the Trash". `torrents`
/// is how many torrents it was of; their names (or the file's) are the list's.
pub fn happened(action: UndoAction, torrents: usize) -> String {
    let done = match action {
        UndoAction::Remove => "Removed",
        UndoAction::Delete => "Deleted",
        UndoAction::TrashFiles { count: 1 } => return "Moved a file to the Trash".into(),
        UndoAction::TrashFiles { count } => return format!("Moved {count} files to the Trash"),
        UndoAction::Rename => "Renamed",
    };
    match torrents {
        1 => done.into(),
        many => format!("{done} {many} torrents"),
    }
}

/// What a Remove, Delete, Move to Trash or Rename that was done lists: the `names` of the
/// torrents (or the torrent and the files, or "old → new"), then what it did that the line leaves
/// out: a Delete's files are in the Trash, a moved file is Off.
pub fn happened_listing(action: UndoAction, count: usize, names: &[String]) -> Listing {
    let note = match (action, count) {
        (UndoAction::Delete, 1) => "Its files are in the Trash.",
        (UndoAction::Delete, _) => "Their files are in the Trash.",
        (UndoAction::TrashFiles { .. }, _) => "Set to Off",
        (UndoAction::Remove | UndoAction::Rename, _) => "",
    };
    Listing::new(&names_listed(names), note)
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
    /// It was pushed off the stack by [`DEPTH`](proto::UNDO_DEPTH) newer ones.
    PastLimit,
    /// Its undo was refused, for this reason.
    Refused(CantUndo, Vec<AbsolutePath>),
    /// A Move to Trash whose torrent moved to another folder since (#24): its files stay in the
    /// Trash.
    Moved,
}

pub fn undo_off(why: &UndoOff) -> String {
    match why {
        UndoOff::Newer => "A newer Remove, Delete, Move to Trash or Rename comes first: undo that one before this".into(),
        UndoOff::PastLimit => {
            format!("Only the last {} Removes, Deletes, Moves to Trash and Renames can be undone", proto::UNDO_DEPTH)
        }
        UndoOff::Refused(why, stranded) => couldnt_undo(&cant_undo_whole(why, stranded)),
        UndoOff::Moved => "Moved to another folder since".into(),
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
        UndoAction::Rename => "Rename",
    }
}

/// What an undo did, as the CLI prints it: "Album is back in the list", "Album is back, with its
/// files", "3 torrents are back".
pub fn undone(undo: &Undoable) -> String {
    let name = undo_names(undo);
    match (undo.action, &undo.torrents[..]) {
        (UndoAction::Remove | UndoAction::Delete, [_, _, ..]) => several_undone(undo.torrents.len()),
        (UndoAction::Remove, _) => format!("{name} is back in the list"),
        (UndoAction::Delete, _) => format!("{name} is back, with its files"),
        (UndoAction::TrashFiles { count: 1 }, _) => format!("A file of {name} is back from the Trash"),
        (UndoAction::TrashFiles { count }, _) => format!("{count} files of {name} are back from the Trash"),
        (UndoAction::Rename, _) => format!("{name} has its old name back"),
    }
}

/// An undo that couldn't be done, as the CLI prints it: "Couldn't undo Delete of Album", and why,
/// with what stayed out of the Trash, if anything did.
pub fn cant_undo(undo: &Undoable, why: &CantUndo, stranded: &[AbsolutePath]) -> (String, String) {
    (format!("Couldn't undo {} of {}", undo_action(undo.action), undo_names(undo)), sentence(&cant_undo_whole(why, stranded)))
}

/// A torrent of an undo of several that didn't come back, as the CLI prints it; one added again
/// since: "Album is already in the list", and nothing more (nothing was touched).
pub fn not_back(not: &proto::NotBack) -> (String, String) {
    match &not.why {
        CantUndo::InList => (format!("{} is already in the list", not.torrent.name), String::new()),
        why => cant_undo(&Undoable { seq: 0, action: not.action, torrents: vec![not.torrent.clone()] }, why, &not.stranded),
    }
}

/// What an undo did, as the bar's line: "Back in the list", "Back, with its files", "3 torrents
/// are back", "A file is back from the Trash". The torrents' names are the list's ([`undone_more`]).
/// The CLI prints [`undone`].
pub fn undone_line(undo: &Undoable) -> String {
    match (undo.action, &undo.torrents[..]) {
        (UndoAction::Remove | UndoAction::Delete, [_, _, ..]) => several_undone(undo.torrents.len()),
        (UndoAction::Remove, _) => "Back in the list".into(),
        (UndoAction::Delete, _) => "Back, with its files".into(),
        (UndoAction::TrashFiles { count: 1 }, _) => "A file is back from the Trash".into(),
        (UndoAction::TrashFiles { count }, _) => format!("{count} files are back from the Trash"),
        (UndoAction::Rename, _) => "Renamed back".into(),
    }
}

/// What an undo's list shows: the names of the torrents it brought back. `did` is what the action's
/// own notice listed, if this app told it: the files a Move to Trash moved are named, and a Rename's
/// names go back as "renamed → original".
pub fn undone_listing(undo: &Undoable, did: Option<&[String]>) -> Listing {
    let torrents = || undo.torrents.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    let names = match (undo.action, did) {
        (UndoAction::Rename, Some([was, now])) => vec![changed(now, was)],
        (UndoAction::TrashFiles { .. }, Some(did)) => did.to_vec(),
        _ => torrents(),
    };
    Listing::new(&names_listed(&names), "")
}

/// The torrents of an undo entry: its one torrent's name, or "3 torrents".
fn undo_names(undo: &Undoable) -> String {
    match &undo.torrents[..] {
        [one] => one.name.clone(),
        many => format!("{} torrents", many.len()),
    }
}

/// An undo that couldn't be done, as a notice: "Couldn't undo the Delete" for the bar's line; the
/// list has the torrents' names, then why, and what stayed out of the Trash, if anything did.
pub fn cant_undo_line(undo: &Undoable, why: &CantUndo, stranded: &[AbsolutePath]) -> (String, Listing) {
    let names: Vec<String> = undo.torrents.iter().map(|t| t.name.clone()).collect();
    let listing = Listing::new(&names_listed(&names), "").then(cant_undo_reason(why).lines());
    let stranded = (!stranded.is_empty()).then(|| {
        std::iter::once(Line::Muted("Still in place:".into())).chain(stranded.iter().map(|p| Line::Path(p.display().to_string()))).collect()
    });
    (format!("Couldn't undo the {}", undo_action(undo.action)), listing.then(stranded.unwrap_or_default()))
}

/// A torrent of an undo of several that didn't come back, as [`cant_undo_line`] words it; one added
/// again since: "Already in the list", its name in the list (nothing was touched).
pub fn not_back_line(not: &proto::NotBack) -> (String, Listing) {
    match &not.why {
        CantUndo::InList => (already_in_list().into(), Listing::new(&not.torrent.name, "")),
        why => cant_undo_line(&Undoable { seq: 0, action: not.action, torrents: vec![not.torrent.clone()] }, why, &not.stranded),
    }
}

/// Why an undo couldn't be done, as a list says it: short words, the path of what's in the way on
/// a line of its own.
pub fn cant_undo_reason(why: &CantUndo) -> Reason {
    match why {
        CantUndo::NotInTrash { path } => Reason::at("Not in the Trash any more", path),
        CantUndo::PathTaken { path } => Reason::at("Something is already at", path),
        CantUndo::NotAFolder { path } => Reason::at("A link or a file is where a folder should be", path),
        CantUndo::PutBackFailed { path, detail } => {
            Reason::at(format!("Couldn't go back ({}), so everything stays in the Trash", without_os_code(detail)), path)
        }
        CantUndo::Destination { reason } => unavailable_reason(reason),
        other => Reason::new(sentence(&cant_undo_why(other))),
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
        CantUndo::Moved => "it moved to another folder since".into(),
        CantUndo::Busy => "it's being moved to another folder now".into(),
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
        "saved_presets" => "Look Presets",
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

/// Edit ▸ Undo naming the torrent action it takes back: "Undo Remove of Album", "Undo Delete of 3
/// torrents", "Undo Move to Trash of Album".
pub fn undo_entry(undo: &Undoable) -> String {
    format!("Undo {} of {}", undo_action(undo.action), undo_names(undo))
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

/// What the bar says of an undo of a change to the saved Look Presets: the preset's name, which the
/// user chose, is the list's ([`preset_change`]).
pub fn look_preset_undone() -> &'static str {
    "Look Preset change undone"
}

/// A theme setting of a saved Look Preset, as the bar's line names it: no name the user chose,
/// "Accent colour (Dark) of a saved Look Preset".
pub fn of_a_saved_preset(short: &str) -> String {
    format!("{short} of a saved Look Preset")
}

/// What an undo of a setting tells, short: "Download limit undone"; `short` is [`setting_short`]'s,
/// or a theme setting's name ("Accent colour (Dark)").
pub fn setting_undone_short(short: &str) -> String {
    format!("{short} undone")
}

/// What a refused undo says in the list: "Couldn't be undone: the service is stopping" (#200's words).
pub fn couldnt_undo(why: &str) -> String {
    format!("Couldn't be undone: {why}")
}

/// What the bar says of a change of a setting the service failed to make; the reason is the list's.
pub fn setting_not_changed() -> &'static str {
    "Couldn't change the setting"
}

/// What the bar says when the file picker failed; the reason is the list's.
pub fn cant_ask_for_file() -> &'static str {
    "Can't open the file dialog"
}

/// What the bar says when `gui.json` couldn't be read; the reason is the list's.
pub fn cant_read_gui_json() -> &'static str {
    "Can't read the window settings"
}

/// What the list behind [`cant_read_gui_json`] says: the file, why it couldn't be read, and what
/// Phantom did about it.
pub fn gui_json_listing(file: &std::path::Path, reason: &str) -> Listing {
    Listing { groups: vec![vec![Line::Path(file.display().to_string()), Line::Muted(sentence(reason))]] }
        .then(vec![Line::Muted("The window settings are back to their defaults".into())])
}

/// What the bar says when the service kept the torrents' database aside, unreadable; the list has
/// where ([`records_set_aside_why`]).
pub fn records_set_aside() -> &'static str {
    "Couldn't read the torrent list"
}

/// What the list behind [`records_set_aside`] says: that the database was kept aside, where, and
/// that Phantom started without torrents.
pub fn records_set_aside_why(kept: &std::path::Path) -> String {
    format!("The torrent list was kept aside as it was, at {}. Phantom started with an empty list.", kept.display())
}

/// What the list behind [`records_set_aside`] says as data: where the database was kept, on a line
/// of its own, and that Phantom started with an empty list.
pub fn records_set_aside_listing(kept: &std::path::Path) -> Listing {
    Listing { groups: vec![vec![Line::Muted("The torrent list was kept aside as it was:".into()), Line::Path(kept.display().to_string())]] }
        .then(vec![Line::Muted("Phantom started with an empty list.".into())])
}

/// What the bar says when a part of a file in the Trash couldn't be kept on disk, even at a retry;
/// the list has the file and why ([`part_not_saved_listing`]).
pub fn part_not_saved() -> &'static str {
    "Couldn't save part of a file in the Trash"
}

/// What the list behind [`part_not_saved`] says: the file on a line of its own, then why the disk
/// write failed.
pub fn part_not_saved_listing(file: &std::path::Path, reason: &str) -> Listing {
    Listing { groups: vec![vec![Line::Path(file.display().to_string()), Line::Muted(sentence(without_os_code(reason)))]] }
}

/// What the bar says of an undo that failed, whatever the reason and whatever it was of (torrents
/// or a setting): one failure, one wording. The list has what it was of, and why.
pub fn undo_failed() -> &'static str {
    "Couldn't undo the change"
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

/// What a settings-reset notice calls something reset in the saved Look Preset `preset`: `what`
/// of it ("Tone in Dark of Calm"), or, without, the preset, dropped ("Look Preset Calm").
pub fn saved_reset(preset: &str, what: Option<&str>) -> String {
    match what {
        Some(what) => format!("{what} of {preset}"),
        None => format!("Look Preset {preset}"),
    }
}

/// [`saved_reset`] for the status bar's line, which holds no name the user chose: "Tone in Dark of a
/// saved Look Preset", "A saved Look Preset".
pub fn saved_reset_line(what: Option<&str>) -> String {
    match what {
        Some(what) => format!("{what} of a saved Look Preset"),
        None => "A saved Look Preset".into(),
    }
}

/// A change of the saved Look Presets, or an Apply of one, as Edit ▸ Undo and its undo name it:
/// "Apply Evening", "Apply Evening's Dark", "Save Evening", "Replace Evening", "Delete Evening".
pub fn preset_change(verb: &str, preset: &str, half: Option<&str>) -> String {
    match half {
        Some(half) => format!("{verb} {preset}'s {half}"),
        None => format!("{verb} {preset}"),
    }
}

/// A setting `short` of the saved Look Preset `preset`, as an undo of it names it: "Accent colour
/// (Dark) of Evening".
pub fn of_preset(short: &str, preset: &str) -> String {
    format!("{short} of {preset}")
}

/// Edit ▸ Undo naming a change by `what` ([`preset_change`], [`of_preset`]): "Undo Apply Evening".
pub fn undo_named(what: &str) -> String {
    format!("Undo {what}")
}

/// What a chip of a saved Look Preset's half says it does: "Apply Evening's Light to the active look".
pub fn apply_half(preset: &str, half: &str) -> String {
    format!("Apply {preset}'s {half} to the active look")
}

/// Why a chip of a saved Look Preset's half that's in use does nothing: "Evening's Light is the
/// active look's Light already".
pub fn half_in_use(preset: &str, half: &str) -> String {
    format!("{preset}'s {half} is the active look's {half} already")
}

/// What the ◐ chip of a saved Look Preset says it does: "Apply Evening (Light and Dark) to the
/// active look".
pub fn apply_both(preset: &str) -> String {
    format!("Apply {preset} (Light and Dark) to the active look")
}

/// Why the ◐ chip of a saved Look Preset that's in use does nothing: "Evening is the active look
/// already".
pub fn both_in_use(preset: &str) -> String {
    format!("{preset} is the active look already")
}

/// What a chip of the Active look's half says it does: "Edit the active look's Light".
pub fn edit_active_half(half: &str) -> String {
    format!("Edit the active look's {half}")
}

/// What an App icon's ☀, ☾ or ◐ chip does: "Use Dark Hour in Dark".
pub fn use_icon(name: &str, halves: &str) -> String {
    format!("Use {name} in {halves}")
}

/// What the ◐ chip of an icon of a designed pair does: uses the pair, the light one first.
pub fn use_icon_pair(light: &str, dark: &str) -> String {
    format!("Use the pair: ☀ {light} · ☾ {dark}")
}

/// What Follow theme's ◐ chip does.
pub fn follow_theme_help() -> &'static str {
    "Follow the theme in Light and Dark"
}

/// Why a Look Preset can't be saved under what's typed: there's no name.
pub fn preset_name_blank() -> &'static str {
    "A Look Preset needs a name"
}

/// What Save as… asks for a name a saved Look Preset has: "Replace Evening?".
pub fn replace_preset(name: &str) -> String {
    format!("Replace {name}?")
}

/// What a settings-reset notice's list says: the settings reset, under a muted "Reset to the
/// default:", then where the file as it was is kept.
pub fn settings_reset_listing(names: &[String], copy: &str) -> Listing {
    let mut group = vec![Line::Muted("Reset to the default:".into())];
    group.extend(names.iter().map(|n| Line::Name(n.clone())));
    Listing { groups: vec![group] }.then(vec![Line::Muted(format!("Kept as {copy}"))])
}

/// The status bar's line of a settings-reset notice: the settings are named in its list (#87).
pub fn settings_reset_line() -> &'static str {
    "Settings reset"
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

/// [`unavailable`] for a list: the path of the folder on a line of its own.
pub fn unavailable_reason(reason: &Unavailable) -> Reason {
    match reason {
        Unavailable::DriveNotConnected { name } => Reason::new(format!("Drive {name} isn't connected")),
        Unavailable::FolderMissing { path } => Reason::at("Folder doesn't exist", path),
        Unavailable::NoPermission { path } => Reason::at("No permission to open the folder", path),
        Unavailable::NotAFolder { path } => Reason::at("Not a folder", path),
        Unavailable::CantOpen { path } => Reason::at("Can't open the folder", path),
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

/// Why a torrent of a list refused, as its line under its name says it: "It's in that folder
/// already"; a path it names on a line of its own.
pub fn refused_why(why: &TorrentRefusal) -> Reason {
    match why {
        TorrentRefusal::Clash { files } => {
            Reason::new("Already in the new destination:").still("", Found::paths(files).iter().map(|f| f.display().to_string()).collect())
        }
        TorrentRefusal::ClashChanged { path } => Reason::at("Its new folder changed while it waited", path),
        TorrentRefusal::AnothersFiles { path } => Reason::at("Another torrent has its files at", path),
        TorrentRefusal::MovedWithout { files, .. } => Reason::new("Moved, but these weren't in its old destination: it's Missing files")
            .still("", files.iter().map(|f| f.display().to_string()).collect()),
        TorrentRefusal::NameTaken { path } => Reason::at("Something is already at", path),
        TorrentRefusal::NameOfAnother { path, other } => Reason::at(format!("{other} keeps its files at"), path),
        TorrentRefusal::NotMoved { why, .. } | TorrentRefusal::NotRenamed { why, .. } => not_moved_reason(why),
        TorrentRefusal::DestinationUnavailable { reason, .. } => unavailable_reason(reason),
        TorrentRefusal::NotTrashed { kept, .. } if no_trash(kept) => Reason::new("The drive has no Trash"),
        TorrentRefusal::NotTrashed { kept, .. } => kept_reason("Couldn't move everything to the Trash", "Still at:", kept),
        TorrentRefusal::FilesNotTrashed { kept, .. } if no_trash(kept) => Reason::new("The drive has no Trash to move its files to"),
        TorrentRefusal::FilesNotTrashed { kept, .. } => {
            kept_reason("Couldn't move every file to the Trash", "Still at (set to Off):", kept)
        }
        other => Reason::new(torrent_reason(other)),
    }
}

/// What a Delete or Move to Trash left: the system's words if it gave any, else `stopped`; each
/// path left where it was on a line of its own, under `label`.
fn kept_reason(stopped: &str, label: &str, kept: &[Kept]) -> Reason {
    let detail = kept.iter().find_map(|k| match &k.why {
        KeptWhy::Failed { detail } => Some(sentence(without_os_code(detail))),
        KeptWhy::NoTrash | KeptWhy::Interrupted => None,
    });
    let text = detail.unwrap_or_else(|| stopped.replace("Couldn't move", "Phantom stopped before moving"));
    Reason::new(text).still(label, kept.iter().map(|k| k.path.display().to_string()).collect())
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
        TorrentRefusal::Moving => "Being moved".into(),
        TorrentRefusal::WaitingToMove => "Waiting to move".into(),
        TorrentRefusal::NotWaitingToMove => "Not waiting to move".into(),
        TorrentRefusal::Clash { files } => format!("Already in the new destination: {}", paths(&Found::paths(files))),
        TorrentRefusal::ClashChanged { path } => format!("Its new folder changed while it waited: {}", path.display()),
        TorrentRefusal::AnothersFiles { path } => format!("Another torrent has its files there: {}", path.display()),
        TorrentRefusal::IntoItself => "That folder is inside its own".into(),
        TorrentRefusal::AlreadyThere => "It's in that folder already".into(),
        TorrentRefusal::NotMoved { why, .. } => format!("Not moved: {}", lower(&not_moved(why))),
        TorrentRefusal::MovedWithout { files, .. } => {
            format!("Moved, but {} weren't in its old destination: it's Missing files", paths(files))
        }
        TorrentRefusal::NameTaken { path } => format!("Something is at {} already", path.display()),
        TorrentRefusal::NameOfAnother { path, other } => format!("{other} keeps its files at {}", path.display()),
        TorrentRefusal::OnlyCase => only_case().into(),
        TorrentRefusal::OnlyNormalization => only_normalization().into(),
        TorrentRefusal::NotRenamed { why, .. } => format!("Not renamed: {}", lower(&not_moved(why))),
    }
}

/// `s` with its first letter lowercase, as a sentence's tail.
fn lower(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| c.to_lowercase().chain(chars).collect())
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

/// What the ratio limit does, under its field.
pub fn seed_ratio_rule() -> &'static str {
    "Stops a finished torrent at this ratio. Empty: never."
}

/// What the time limit does, under its field.
pub fn seed_time_rule() -> &'static str {
    "Stops a finished torrent after seeding this long. Empty: never."
}

/// `text` with a capital first letter. The service's and `refusal`'s wording is lowercase so it
/// composes after "error: " and in chains; a surface that shows it alone makes it a sentence.
pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The connection to the service, as a status bar shows it: short fixed words (#87). Why it was
/// lost is [`connection_why`]'s.
pub fn connection(state: &State) -> &'static str {
    match state {
        State::Connecting => "Connecting…",
        State::Replacing { .. } => "Restarting the service…",
        State::Spawning { .. } => "Starting the service…",
        State::Connected { .. } => "Connected",
        State::Lost { .. } => "Service unavailable",
        State::Outdated => "Phantom was updated",
    }
}

/// Why the connection is wrong, for the hover over the status bar's words; `None` while it's fine.
pub fn connection_why(state: &State) -> Option<String> {
    match state {
        State::Lost { why, .. } => Some(sentence(&lost(why))),
        State::Outdated => Some(sentence(&lost(&Why::Outdated))),
        _ => None,
    }
}

/// The button after "Service unavailable" that reveals the service's log in Finder.
pub fn show_log() -> &'static str {
    "Show log"
}

/// The button after "Phantom was updated" that restarts the app.
pub fn restart_app() -> &'static str {
    "Restart"
}

/// Restart couldn't start the app again.
pub fn cant_restart() -> &'static str {
    "Can't restart Phantom"
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

/// What a speed limit does, under its field.
pub fn speed_limit_rule() -> &'static str {
    "For all torrents together. Empty: unlimited."
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
        Action::Move => "move",
        Action::Rename => "rename",
        Action::CancelMove => "cancel the move of",
        Action::Open => "open",
        Action::Reveal => "show",
    };
    match total {
        1 => format!("Couldn't {verb} the torrent"),
        _ => format!("Couldn't {verb} {failed} of {total}"),
    }
}

/// A failed command for one torrent called `name`, as an entry of an error notice's list: the name,
/// and why under it, a path on a line of its own.
pub fn command_failure(name: &str, failure: &Failure) -> (String, Reason) {
    match failure {
        Failure::Refused(Refusal::Torrent {
            why: why @ (TorrentRefusal::NotTrashed { name, .. } | TorrentRefusal::FilesNotTrashed { name, .. }),
            ..
        }) => (name.clone(), refused_why(why)),
        Failure::Refused(Refusal::Torrent { why, .. }) => (name.to_owned(), refused_why(why)),
        other => (name.to_owned(), Reason::new(sentence(&format!("{other:#}")))),
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
        TorrentRefusal::Moving => format!("{name} is being moved"),
        TorrentRefusal::WaitingToMove => format!("{name} is waiting to move"),
        TorrentRefusal::NotWaitingToMove => format!("{name} isn't waiting to move"),
        TorrentRefusal::Clash { files } => format!("{name}: already in the new destination: {}", paths(&Found::paths(files))),
        TorrentRefusal::ClashChanged { path } => format!("{name}: its new folder changed while it waited: {}", path.display()),
        TorrentRefusal::AnothersFiles { path } => format!("{name}: another torrent has its files there: {}", path.display()),
        TorrentRefusal::IntoItself => format!("{name} can't move into a folder inside its own"),
        TorrentRefusal::AlreadyThere => format!("{name} is in that folder already"),
        TorrentRefusal::NotMoved { name: torrent, why } => format!("Couldn't move {torrent}: {}", lower(&not_moved(why))),
        TorrentRefusal::MovedWithout { name: torrent, files } => {
            format!("Moved {torrent} without {}, which weren't in its old destination: it's Missing files", paths(files))
        }
        TorrentRefusal::NameTaken { path } => format!("{name} can't take that name: something is at {} already", path.display()),
        TorrentRefusal::NameOfAnother { path, other } => {
            format!("{name} can't take that name: {other} keeps its files at {}", path.display())
        }
        TorrentRefusal::OnlyCase => format!("{name} can't take that name: {}", lower(only_case())),
        TorrentRefusal::OnlyNormalization => format!("{name} can't take that name: {}", lower(only_normalization())),
        TorrentRefusal::NotRenamed { name: torrent, why } => format!("Couldn't rename {torrent}: {}", lower(&not_moved(why))),
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
                Action::Move => "be moved",
                Action::Rename => "be renamed",
                Action::CancelMove => "have its move cancelled",
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

    fn at(path: &str) -> AbsolutePath {
        AbsolutePath::new(path).unwrap()
    }

    /// A setting's undo says which, short, and in the popover from what to what.
    #[test]
    fn a_setting_undo_says_which_and_from_what_to_what() {
        assert_eq!(setting_undone_short("Download limit"), "Download limit undone");
        assert_eq!(setting_undone_short("Accent colour (Dark)"), "Accent colour (Dark) undone");
    }

    /// A change reads "old → new" in a list, the setting's name staying in the line (rule 6).
    #[test]
    fn a_change_reads_old_arrow_new() {
        assert_eq!(changed("10 MB/s", "2 MB/s"), "10 MB/s → 2 MB/s");
    }

    /// A Look Preset's name, which the user chose, is never in the bar's line (rule 8).
    #[test]
    fn the_line_of_a_look_preset_undo_names_no_preset() {
        assert_eq!(look_preset_undone(), "Look Preset change undone");
        assert_eq!(setting_undone_short(&of_a_saved_preset("Accent colour (Dark)")), "Accent colour (Dark) of a saved Look Preset undone");
    }

    /// One failure, one wording (rule 5): an undo that couldn't be done says "Couldn't undo" with its
    /// subject, for a setting or a torrent action alike.
    #[test]
    fn a_failed_undo_has_one_wording() {
        assert_eq!(undo_failed(), "Couldn't undo the change");
        let undo = Undoable { seq: 1, action: UndoAction::Delete, torrents: vec![proto::Named { id: 3, name: "Album".into() }] };
        assert_eq!(cant_undo_line(&undo, &CantUndo::NotInList, &[]).0, "Couldn't undo the Delete");
    }

    /// A reason is words, and a path on a line of its own, never inside the sentence (rules 1 and 3).
    #[test]
    fn a_reason_keeps_its_path_out_of_its_words() {
        let path = at("/Users/me/Downloads/Album");
        let taken = refused_why(&TorrentRefusal::NameTaken { path: path.clone() });
        assert_eq!((taken.text.as_str(), taken.paths.clone()), ("Something is already at", vec!["/Users/me/Downloads/Album".to_owned()]));
        assert_eq!(taken.inline(), "Something is already at: /Users/me/Downloads/Album");
        let full = refused_why(&TorrentRefusal::NotMoved { name: "Album".into(), why: NotMoved::DiskFull });
        assert_eq!(full.text, "The new destination's disk is full", "no \"Not moved:\", the line says it");
        let gone = cant_undo_reason(&CantUndo::NotInTrash { path });
        assert_eq!((gone.text.as_str(), gone.paths.len()), ("Not in the Trash any more", 1));
        let kept = [Kept { path: at("/d/pack/a"), why: KeptWhy::Failed { detail: "Operation not permitted (os error 1)".into() } }];
        let still = refused_why(&TorrentRefusal::NotTrashed { name: "Pack".into(), kept: kept.to_vec() });
        assert_eq!(
            (still.text.as_str(), still.label.as_str(), still.paths.clone()),
            ("Operation not permitted", "Still at:", vec!["/d/pack/a".to_owned()]),
            "the system's words on one line, no os error code, the path under a label"
        );
        let no_trash = [Kept { path: at("/Volumes/USB/pack"), why: KeptWhy::NoTrash }];
        assert_eq!(
            refused_why(&TorrentRefusal::NotTrashed { name: "Album".into(), kept: no_trash.to_vec() }).text,
            "The drive has no Trash"
        );
    }

    /// What the stranded files of a refused Delete's undo are listed as: names, the reason, then
    /// "Still in place:" and a path a line.
    #[test]
    fn a_refused_undos_stranded_paths_are_listed_each_on_a_line() {
        let undo = Undoable {
            seq: 1,
            action: UndoAction::Delete,
            torrents: vec![proto::Named { id: 1, name: "A".into() }, proto::Named { id: 2, name: "B".into() }],
        };
        let why = CantUndo::PutBackFailed { path: at("/d/B"), detail: "Operation not permitted".into() };
        let (_, list) = cant_undo_line(&undo, &why, &[at("/d/A")]);
        assert_eq!(list.lines().last(), Some(&Line::Path("/d/A".into())));
        let plain = list.plain();
        assert!(plain.contains("Still in place:") && plain.starts_with("A\nB\n"), "{plain}");
        assert!(list.lines().filter(|l| matches!(l, Line::Path(_))).count() == 2, "both paths on lines of their own: {plain}");
        assert!(list.lines().all(|l| matches!(l, Line::Path(_)) || !l.text().contains('/')), "no path inside a sentence: {plain}");
    }

    /// An add of a magnet link names the link's `dn=` name, else the link as a line of its own; a
    /// file is its name (rule 1).
    #[test]
    fn a_failed_adds_list_names_the_dn_name_or_the_link() {
        let hash = "00000000000000000000000000000000000000ff";
        assert_eq!(add_what(&format!("magnet:?xt=urn:btih:{hash}&dn=Big+Buck%20Bunny")), Line::Name("Big Buck Bunny".into()));
        let bare = format!("magnet:?xt=urn:btih:{hash}&tr=udp%3A%2F%2Ft.example");
        assert_eq!(add_what(&bare), Line::Path(bare));
        assert_eq!(add_what("bad-1.torrent"), Line::Name("bad-1.torrent".into()));
    }

    /// A Move to Trash is worded by how many files the service says moved; no file is named in the
    /// line, the list has the torrent and the file.
    #[test]
    fn a_move_to_trash_says_how_many_files_moved() {
        let files = |count| UndoAction::TrashFiles { count };
        assert_eq!(happened(files(1), 2), "Moved a file to the Trash");
        assert_eq!(happened(files(3), 1), "Moved 3 files to the Trash");
        let named = ["Album".to_owned(), "01 Intro.flac".to_owned()];
        assert_eq!(
            happened_listing(files(1), 1, &named).plain(),
            "Album\n01 Intro.flac\nSet to Off",
            "no \"so it won't be downloaded again\""
        );
        assert_eq!(happened_listing(files(3), 3, &named[..1]).plain(), "Album\nSet to Off");
        let undo = |count| Undoable { seq: 1, action: files(count), torrents: vec![proto::Named { id: 3, name: "Album".into() }] };
        assert_eq!(undone_line(&undo(1)), "A file is back from the Trash");
        assert_eq!(undone_listing(&undo(1), None).plain(), "Album");
        assert_eq!(undone_listing(&undo(1), Some(&named)).plain(), "Album\n01 Intro.flac", "the file is named too");
        assert_eq!(undone_line(&undo(2)), "2 files are back from the Trash");
        let (short, why) = cant_undo_line(&undo(2), &CantUndo::NotInTrash { path: AbsolutePath::new("/d/Album/a.flac").unwrap() }, &[]);
        assert_eq!(short, "Couldn't undo the Move to Trash");
        assert_eq!(why.plain(), "Album\nNot in the Trash any more\n/d/Album/a.flac");
    }

    /// What a Remove or a Rename lists carries no note: "Its files stay where they are" and "Its
    /// files have this name on disk now" went (rule 2); a Delete's files are in the Trash.
    #[test]
    fn a_remove_and_a_rename_have_no_note_and_a_delete_keeps_its_one() {
        let one = ["Album".to_owned()];
        assert_eq!(happened_listing(UndoAction::Remove, 1, &one).plain(), "Album");
        assert_eq!(happened_listing(UndoAction::Rename, 1, &[changed("Album", "Album 2")]).plain(), "Album → Album 2");
        assert_eq!(happened_listing(UndoAction::Delete, 1, &one).plain(), "Album\nIts files are in the Trash.");
        assert_eq!(happened_listing(UndoAction::Delete, 2, &one).plain(), "Album\nTheir files are in the Trash.");
    }

    /// A Rename's undo lists "renamed → original", once this app knows what it was renamed from.
    #[test]
    fn a_renamed_back_lists_old_arrow_new() {
        let undo = Undoable { seq: 1, action: UndoAction::Rename, torrents: vec![proto::Named { id: 3, name: "Album 2".into() }] };
        assert_eq!(undone_listing(&undo, Some(&["Album".to_owned(), "Album 2".to_owned()])).plain(), "Album 2 → Album");
        assert_eq!(undone_listing(&undo, None).plain(), "Album 2", "a Rename this app didn't tell has only the name");
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
        assert_eq!(
            cant_undo_line(&undo, &CantUndo::NotKept, &[]),
            ("Couldn't undo the Delete".into(), Listing::new("T1\nT2\nT3", "What Phantom kept of it can't be read"))
        );
        assert_eq!(undo_entry(&undo), "Undo Delete of 3 torrents", "Edit ▸ Undo names it so (#255)");
        let not = proto::NotBack { torrent: named(2), action: UndoAction::Delete, why: CantUndo::NotKept, stranded: Vec::new() };
        assert_eq!(not_back(&not), ("Couldn't undo Delete of T2".into(), "What Phantom kept of it can't be read".into()));
        let again = proto::NotBack { why: CantUndo::InList, ..not };
        assert_eq!(not_back(&again), ("T2 is already in the list".into(), String::new()), "nothing touched: nothing more to say");
        assert_eq!(not_back_line(&again), ("Already in the list".into(), Listing::new("T2", "")), "the name is the list's");
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
        assert_eq!(added_line(&new), "Added the torrent", "the name is the list's");
        assert_eq!(added_line(&Added { already_existed: true, ..new.clone() }), "Already in the list");
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
        assert_eq!(command_failure("Album", &failure), ("Album".to_owned(), Reason::new("The drive has no Trash")), "the name, then why");
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
        let found = || proto::Found { path: path(), dev: 1, ino: 2, len: 3, secs: 4, nanos: 5 };
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
            TorrentRefusal::Moving,
            TorrentRefusal::WaitingToMove,
            TorrentRefusal::NotWaitingToMove,
            TorrentRefusal::Clash { files: vec![found()] },
            TorrentRefusal::ClashChanged { path: path() },
            TorrentRefusal::AnothersFiles { path: path() },
            TorrentRefusal::IntoItself,
            TorrentRefusal::AlreadyThere,
            TorrentRefusal::NotMoved { name: "Album".into(), why: NotMoved::DiskFull },
            TorrentRefusal::MovedWithout { name: "Album".into(), files: vec![path()] },
            TorrentRefusal::NameTaken { path: path() },
            TorrentRefusal::NameOfAnother { path: path(), other: "Theirs".into() },
            TorrentRefusal::OnlyCase,
            TorrentRefusal::OnlyNormalization,
            TorrentRefusal::NotRenamed { name: "Album".into(), why: NotMoved::DiskFull },
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
                | TorrentRefusal::DestinationUnavailable { .. }
                | TorrentRefusal::Moving
                | TorrentRefusal::WaitingToMove
                | TorrentRefusal::NotWaitingToMove
                | TorrentRefusal::Clash { .. }
                | TorrentRefusal::ClashChanged { .. }
                | TorrentRefusal::AnothersFiles { .. }
                | TorrentRefusal::IntoItself
                | TorrentRefusal::AlreadyThere
                | TorrentRefusal::NotMoved { .. }
                | TorrentRefusal::MovedWithout { .. }
                | TorrentRefusal::NameTaken { .. }
                | TorrentRefusal::NameOfAnother { .. }
                | TorrentRefusal::OnlyCase
                | TorrentRefusal::OnlyNormalization
                | TorrentRefusal::NotRenamed { .. } => {}
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

/// The details pane's size beside the name (#33): the Size, and how much is Off when some files are:
/// "1.7 GiB · 4.2 GiB Off". Nothing until the info arrives.
pub fn size_and_off(content: Option<&proto::Content>) -> String {
    match content {
        None => String::new(),
        Some(c) => [Some(bytes(c.size_bytes)), off(c)].into_iter().flatten().collect::<Vec<_>>().join(" · "),
    }
}

/// How much of the torrent is Off, when some files are: "4.2 GiB Off".
pub fn off(c: &proto::Content) -> Option<String> {
    (c.size_bytes < c.full_size_bytes).then(|| format!("{} Off", bytes(c.full_size_bytes - c.size_bytes)))
}

/// The problem a torrent has, as the details header's second line says it (#33): short, the path
/// being on its left. `missing` is how many of its files are gone. The longer wording, where
/// there's one, is its hover: `(line, hover)`. `None` when there's nothing to say.
pub fn pane_reason(status: &Status, missing: usize) -> Option<(String, Option<String>)> {
    match status {
        Status::MissingFiles => Some((files_gone(missing), None)),
        Status::DestinationUnavailable { reason } => Some((short_unavailable(reason), Some(unavailable(reason)))),
        Status::Error { failure } => Some(short_failure(failure)),
        _ => None,
    }
}

/// The seeding limit it stopped at, on the details' Info tab: "Stopped seeding at ratio 2.0". Not a
/// problem, so the header never says it.
pub fn seeding_limit(status: &Status) -> Option<String> {
    match status {
        Status::Done { reached: Some(reached) } => Some(stopped_seeding(reached)),
        _ => None,
    }
}

/// "2 files are gone from disk", "1 file is gone from disk"; "Files are gone from disk" before the
/// files are known.
fn files_gone(n: usize) -> String {
    match n {
        0 => "Files are gone from disk".into(),
        1 => "1 file is gone from disk".into(),
        n => format!("{n} files are gone from disk"),
    }
}

/// Why a destination can't be reached, without its path, which the pane shows on the line above.
fn short_unavailable(reason: &Unavailable) -> String {
    match reason {
        Unavailable::DriveNotConnected { name } => format!("Drive {name} isn't connected"),
        Unavailable::FolderMissing { .. } => "Its folder doesn't exist".into(),
        Unavailable::NoPermission { .. } => "No permission to open its folder".into(),
        Unavailable::NotAFolder { .. } => "Its folder isn't a folder".into(),
        Unavailable::CantOpen { .. } => "Can't open its folder".into(),
    }
}

/// An error, short: Phantom's words, what librqbit or the OS said on hover.
fn short_failure(failure: &TorrentFailure) -> (String, Option<String>) {
    match failure {
        TorrentFailure::FetchFailed { detail } => ("Couldn't fetch its info".into(), Some(sentence(detail))),
        TorrentFailure::AddAgainFailed { detail } => ("Couldn't check it".into(), Some(sentence(detail))),
        TorrentFailure::Librqbit { detail } => ("Stopped with an error".into(), Some(sentence(detail))),
        TorrentFailure::DiskFull => ("The disk is full".into(), None),
        TorrentFailure::NotTrashed { kept } => {
            let n = kept.len();
            (if n == 1 { "1 file didn't go to the Trash".into() } else { format!("{n} files didn't go to the Trash") }, None)
        }
    }
}

/// What it's uploaded ÷ its Size, on the details' Info tab (#33): "0.54"; nothing for no size.
pub fn uploaded_ratio(uploaded: u64, size: u64) -> Option<String> {
    (size > 0).then(|| format!("{:.2}", uploaded as f64 / size as f64))
}

/// What each line of the details' Info tab is (#33).
pub struct InfoLabels {
    pub uploaded: &'static str,
    pub ratio: &'static str,
    pub limit: &'static str,
    pub hash: &'static str,
    pub magnet: &'static str,
    pub original: &'static str,
}

pub fn info_labels() -> InfoLabels {
    InfoLabels {
        uploaded: "Uploaded",
        ratio: "Ratio",
        limit: "Seeding limit",
        hash: "Info hash",
        magnet: "Magnet link",
        original: "Original name",
    }
}

/// Copies the magnet link, on the details pane's Info tab: it acts at once, so no "…".
pub fn copy() -> &'static str {
    "Copy"
}

/// Beside a column in the list header's menu that's wanted but doesn't fit the window (#33).
pub fn no_room() -> &'static str {
    "No room"
}
