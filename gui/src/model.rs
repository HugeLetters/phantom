//! The app's state, [`Phantom`], and what actions do to it: applying the service's live data and
//! answering its commands, reading
//! the selection, adding (into the default destination, or asking for one when there's none), removing and
//! deleting, sorting, and the details pane of the one selected torrent with its files' priorities.
//! No drawing; views call these. Each helper takes only the pieces it uses.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use client::{LinkProblem, OpenRefusal, display};
use proto::{Action, Added, Details, Priority, Status, TorrentCommand, TorrentId, TorrentInfo};
use rune::{AppHandle, FileDialogOptions, TextEdit, Tx};
use rune_kit::{SortDir, TableState};

use crate::errors::ErrorList;
use crate::notice::Notice;
use crate::service::{AddError, AddRequest, Connection, Done, Live, Service};
use crate::ui::{Failure, Ui};

pub struct Phantom {
    pub service: Box<dyn Service>,
    /// To bring the window back for a link opened while it's closed; None in tests.
    pub window: Option<AppHandle<Phantom>>,
    pub torrents: Vec<TorrentInfo>,
    /// Rows are keyed by torrent id.
    pub table: TableState,
    pub magnet: TextEdit,
    /// What Add's popover says, until the field's text changes: why the text can't be added, or
    /// why the add failed.
    pub add_note: Option<AddNote>,
    /// Set while Copy magnet link shows it worked, for a moment.
    pub copied: Option<CopyFeedback>,
    pub connection: Connection,
    pub notice: Option<Notice>,
    /// Where errors with no better place go.
    pub errors: ErrorList,
    pub pane: DetailsPane,
    pub adds: Adds,
}

/// One batch of adds made with no control in the window (files dropped on it or opened from
/// Finder, a magnet clicked in a browser) in which some failed: "Couldn't add 2 of 5". The
/// batch stays one entry in the error list.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedAdds {
    /// How many were in the batch.
    pub total: usize,
    pub failed: Vec<AddFailure>,
}

impl FailedAdds {
    /// The entry as a [`Notice`]: "Couldn't add 2 of 5", and each failure as its view.
    pub fn notice(&self) -> Notice {
        let failed = self.failed.clone();
        let list = move |_: &Phantom, ui: &Ui| {
            let failures: Vec<_> = failed.iter().map(|f| Failure { what: &f.what, why: &f.why }).collect();
            ui.failure_list(&failures)
        };
        Notice { short: display::adds_failed(self.failed.len(), self.total), more: Some(Rc::new(list)) }
    }
}

/// An add that failed: the file or link, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct AddFailure {
    pub what: String,
    pub why: String,
}

impl AddFailure {
    pub fn refused(request: &AddRequest, why: OpenRefusal) -> Self {
        Self { what: what(request), why: display::open_refusal(why).into() }
    }
}

/// The file's name, or the link, as a note names an add.
fn what(request: &AddRequest) -> String {
    match request {
        AddRequest::File(path) => path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned(),
        AddRequest::Url(link) => link.clone(),
    }
}

/// A message for the Add button, about the field's text as it was when it arose.
#[derive(Clone)]
pub struct AddNote {
    pub text: String,
    pub message: String,
    /// A failed add: the note is in the error colour, and Add stays red for [`ALERT_SECS`].
    pub error: bool,
    /// The app clock when it was first drawn; `None` until the next poll sees it.
    since: Option<f64>,
    pub alerting: bool,
}

const ALERT_SECS: f64 = 2.0;

impl AddNote {
    /// Why the text can't be added.
    pub fn about(field: &TextEdit, message: impl Into<String>) -> Self {
        Self { text: field.text.clone(), message: message.into(), error: false, since: None, alerting: false }
    }

    /// Why the add failed.
    fn failed(field: &TextEdit, message: impl Into<String>) -> Self {
        Self { error: true, alerting: true, ..Self::about(field, message) }
    }
}

/// Ends Add's red after [`ALERT_SECS`].
pub fn age_add_note(note: &mut Option<AddNote>, now: f64, tx: &mut Tx) {
    let Some(note) = note.as_mut().filter(|n| n.alerting) else { return };
    let since = *note.since.get_or_insert(now);
    if now - since >= ALERT_SECS {
        note.alerting = false;
    } else {
        tx.wake_after(std::time::Duration::from_secs_f64(ALERT_SECS - (now - since)));
    }
}

/// Adds the service refused for want of a destination, waiting for the folder picker: one at a
/// time. Each entry is one action's torrents (several files dropped together), which get one
/// destination.
#[derive(Default)]
pub struct Adds {
    waiting: VecDeque<Waiting>,
    asking: bool,
}

/// An add waiting for a destination. Its batch goes on with it, so the whole batch is told of
/// once, when the last of it is answered.
struct Waiting {
    requests: Vec<AddRequest>,
    batch: Batch,
}

type OnAdded = Done<Vec<Result<Added, AddError>>>;

/// What one action's adds have come to so far. Its failures are told of together, once none of
/// it waits for a destination any more.
struct Batch {
    /// The link the field sent, if this is the field's add: the field answers for it.
    field: Option<String>,
    /// Answered so far, or refused before reaching the service.
    total: usize,
    failed: Vec<AddFailure>,
}

impl Batch {
    /// A batch of adds with `refused` already failed before the service is asked.
    fn new(field: Option<String>, refused: Vec<AddFailure>) -> Self {
        Self { field, total: refused.len(), failed: refused }
    }

    /// What the service's answer to `requests` does to the batch.
    fn answer(self, requests: &[AddRequest]) -> OnAdded {
        let labels: Vec<String> = requests.iter().map(what).collect();
        Box::new(move |s, results, tx| self.answered(s, &labels, results, tx))
    }

    /// Each add that was added is told of in the status bar, the last one showing; those refused
    /// for want of a destination wait for the picker, as one. The field's own add, while the
    /// field still holds its link, empties the field once added, and a failure shows by Add, with
    /// the link selected to be fixed; any other failure goes to the batch's note.
    fn answered(mut self, s: &mut Phantom, labels: &[String], results: Vec<Result<Added, AddError>>, tx: &mut Tx) {
        let holds = self.field.as_deref().is_some_and(|link| s.magnet.text.trim() == link);
        let mut requests = Vec::new();
        for (label, result) in labels.iter().zip(results) {
            match result {
                Ok(added) => {
                    self.total += 1;
                    if holds {
                        s.magnet.clear();
                    }
                    s.notice = Some(show_added(&mut s.table, &added, tx));
                }
                Err(AddError::NoDestination(request)) => requests.push(request),
                Err(AddError::Failed(e)) if holds => {
                    s.magnet.set_selection(0, s.magnet.text.len());
                    s.add_note = Some(AddNote::failed(&s.magnet, display::add_failure(&e)));
                }
                Err(AddError::Failed(e)) => {
                    self.total += 1;
                    self.failed.push(AddFailure { what: label.clone(), why: display::add_failure(&e) });
                }
            }
        }
        if requests.is_empty() {
            self.finish(s);
        } else {
            s.adds.waiting.push_back(Waiting { requests, batch: self });
            ask_destination(s, tx);
        }
    }

    /// Nothing of the batch is left to answer: its failures, if any, are reported as one entry.
    fn finish(self, s: &mut Phantom) {
        if !self.failed.is_empty() {
            s.errors.report(FailedAdds { total: self.total, failed: self.failed }.notice());
        }
    }
}

/// A copy's confirmation, until it has been shown for [`COPY_FEEDBACK_SECS`].
pub struct CopyFeedback {
    /// The app clock when it first showed; `None` until the next poll sees it.
    pub since: Option<f64>,
}

const COPY_FEEDBACK_SECS: f64 = 1.5;

/// The details pane: the files of the one selected torrent.
pub struct DetailsPane {
    /// The torrent the service sends details of.
    pub watching: Option<TorrentId>,
    /// The latest details received: of `watching`, or, until they arrive, of the torrent watched
    /// before, which the pane goes on showing (see [`shown_torrent`]).
    pub details: Option<Details>,
    /// Rows are keyed by file index.
    pub files: TableState,
}

impl DetailsPane {
    fn new() -> Self {
        Self { watching: None, details: None, files: TableState::new().key("files").multi_select(true) }
    }
}

impl Phantom {
    pub fn new(service: Box<dyn Service>) -> Self {
        Self {
            service,
            window: None,
            torrents: Vec::new(),
            table: TableState::new().multi_select(true),
            magnet: TextEdit::new(""),
            add_note: None,
            copied: None,
            connection: Connection::Connecting,
            notice: None,
            errors: ErrorList::default(),
            pane: DetailsPane::new(),
            adds: Adds::default(),
        }
    }

    pub fn apply(&mut self, live: Live) {
        match live {
            Live::Listing(torrents) => {
                self.torrents = torrents;
                if let Some((column, dir)) = self.table.sort() {
                    let column = column.to_owned();
                    sort(&mut self.torrents, &column, dir);
                }
            }
            Live::Details(mut details) if self.pane.watching == Some(details.id) => {
                // Another torrent's than the pane held: its files and their selection go.
                if self.pane.details.as_ref().is_some_and(|d| d.id != details.id) {
                    self.pane.files = DetailsPane::new().files;
                }
                if let Some((column, dir)) = self.pane.files.sort() {
                    let column = column.to_owned();
                    sort_files(&mut details, &column, dir);
                }
                self.pane.details = Some(details);
            }
            // Of a torrent no longer selected, answered late.
            Live::Details(_) => {}
            Live::Connection(connection) => self.connection = connection,
        }
    }
}

/// What to tell the user about an add. A torrent that was already in the list gets selected and
/// scrolled to, instead of a second row.
fn show_added(table: &mut TableState, added: &Added, tx: &mut Tx) -> Notice {
    if added.already_existed {
        table.reveal(added.id.to_string(), tx);
    }
    Notice::new(display::added(added))
}

pub fn selected_torrents(table: &TableState) -> Vec<TorrentId> {
    table.selected().filter_map(|id| id.parse().ok()).collect()
}

/// The selected torrents that `action` does something to now, as the service says.
pub fn selected_for(table: &TableState, torrents: &[TorrentInfo], action: Action) -> Vec<TorrentId> {
    let selected = selected_torrents(table);
    torrents.iter().filter(|t| t.actions.contains(&action) && selected.contains(&t.id)).map(|t| t.id).collect()
}

/// The torrent the details pane shows: the selection, when it's exactly one torrent.
pub fn details_torrent<'a>(table: &TableState, torrents: &'a [TorrentInfo]) -> Option<&'a TorrentInfo> {
    match selected_torrents(table)[..] {
        [id] => torrents.iter().find(|t| t.id == id),
        _ => None,
    }
}

/// The torrent the pane shows: the selected one, or, while its details are on the way, the one
/// whose details the pane holds. A switch lands whole once they arrive, rather than through a
/// pane with no files and its buttons off.
pub fn shown_torrent<'a>(table: &TableState, torrents: &'a [TorrentInfo], pane: &DetailsPane) -> Option<&'a TorrentInfo> {
    let selected = details_torrent(table, torrents)?;
    let held = pane.details.as_ref().and_then(|d| torrents.iter().find(|t| t.id == d.id));
    Some(held.unwrap_or(selected))
}

/// Has the service send the details of the selected torrent, once it has info. The last one's
/// stay until the new ones replace them, unless none are coming. Runs when the selection or the
/// list changes.
pub fn follow_selection(service: &mut dyn Service, table: &TableState, torrents: &[TorrentInfo], pane: &mut DetailsPane) {
    let wanted = details_torrent(table, torrents).filter(|t| t.status != Status::FetchingInfo).map(|t| t.id);
    if wanted == pane.watching {
        return;
    }
    service.watch_details(wanted);
    if wanted.is_none() {
        *pane = DetailsPane::new();
    }
    pane.watching = wanted;
}

/// Whether the torrent's destination can't be reached.
pub fn is_away(t: &TorrentInfo) -> bool {
    matches!(t.status, Status::DestinationUnavailable { .. })
}

/// What Reveal in Finder shows: where the torrent is on disk, the file itself for a single-file
/// torrent and its folder otherwise. Nothing until its details arrive.
pub fn reveal_target<'a>(torrent: &TorrentInfo, pane: &'a DetailsPane) -> Option<&'a Path> {
    pane.details.as_ref().filter(|d| d.id == torrent.id).map(|d| &*d.location)
}

pub fn selected_files(pane: &DetailsPane) -> Vec<usize> {
    pane.files.selected().filter_map(|i| i.parse().ok()).collect()
}

/// Sets the priority of the files selected in the pane.
pub fn set_selected_priority(service: &mut dyn Service, pane: &DetailsPane, priority: Priority) {
    let files = selected_files(pane);
    if let Some(details) = &pane.details
        && !files.is_empty()
    {
        command(service, &[details.id], TorrentCommand::SetPriority { files, priority });
    }
}

/// The magnet link of a torrent, to copy: its info hash and name.
pub fn magnet_link(torrent: &TorrentInfo) -> String {
    let mut link = format!("magnet:?xt=urn:btih:{}", torrent.info_hash);
    if let Some(name) = &torrent.name {
        link.push_str("&dn=");
        for b in name.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => link.push(b as char),
                _ => link.push_str(&format!("%{b:02X}")),
            }
        }
    }
    link
}

/// Copies the magnet link of the torrent the pane shows, and starts the confirmation.
pub fn copy_magnet(s: &mut Phantom, tx: &mut Tx) {
    let Some(link) = details_torrent(&s.table, &s.torrents).map(magnet_link) else { return };
    s.copied = Some(CopyFeedback { since: None });
    tx.copy(link);
}

/// Starts the clock on a copy's feedback, and ends it when it has been shown long enough.
pub fn age_copy_feedback(copied: &mut Option<CopyFeedback>, now: f64, tx: &mut Tx) {
    let Some(feedback) = copied else { return };
    let since = *feedback.since.get_or_insert(now);
    if now - since >= COPY_FEEDBACK_SECS {
        *copied = None;
    } else {
        tx.wake_after(std::time::Duration::from_secs_f64(COPY_FEEDBACK_SECS - (now - since)));
    }
}

/// "Size of Full size" when some files are Off, else just the Size.
pub fn size_label(t: &TorrentInfo) -> String {
    if t.size_bytes == t.full_size_bytes {
        display::bytes(t.size_bytes)
    } else {
        format!("{} of {}", display::bytes(t.size_bytes), display::bytes(t.full_size_bytes))
    }
}

/// Whether the text is something Add accepts: a magnet link with an info hash, an info hash on
/// its own, or an http(s) URL. An info hash is 40 hex digits or 32 base32 letters and digits.
pub fn link_problem(text: &str) -> Option<LinkProblem> {
    let text = text.trim();
    let info_hash = |h: &str| match h.len() {
        40 => h.bytes().all(|b| b.is_ascii_hexdigit()),
        32 => h.bytes().all(|b| matches!(b.to_ascii_uppercase(), b'A'..=b'Z' | b'2'..=b'7')),
        _ => false,
    };
    if let Some(query) = text.strip_prefix("magnet:?") {
        let hash = query.split('&').find_map(|kv| kv.strip_prefix("xt=urn:btih:"));
        return if hash.is_some_and(info_hash) { None } else { Some(LinkProblem::NoInfoHash) };
    }
    let url = ["http://", "https://"].iter().any(|scheme| text.strip_prefix(scheme).is_some_and(|rest| !rest.is_empty()));
    if url || info_hash(text) { None } else { Some(LinkProblem::NotALink) }
}

/// Sends the field's link to the service, each time: one already in the list is answered so. If
/// it isn't a link, `note` says why instead. The answer is the field's: see [`Batch::answered`].
pub fn submit_url(service: &mut dyn Service, field: &TextEdit, note: &mut Option<AddNote>) {
    *note = link_problem(&field.text).map(|problem| AddNote::about(field, display::link_problem(problem)));
    if note.is_none() {
        let link = field.text.trim().to_owned();
        let requests = vec![AddRequest::Url(link.clone())];
        let batch = Batch::new(Some(link), Vec::new());
        service.add(requests.clone(), None, batch.answer(&requests));
    }
}

pub fn add_files(s: &mut Phantom, files: &[PathBuf]) {
    add(s, files.iter().cloned().map(AddRequest::File).collect(), Vec::new());
}

/// Adds the torrents into the default destination, for adds with no control in the window; the
/// `refused` ones never reach the service, and fail in the same note as the rest. Without a
/// default destination the service says so ([`AddError::NoDestination`]), and the user picks a
/// folder for them all first; cancelling adds none. Failures are told of in one note, once the
/// whole batch is answered.
pub fn add(s: &mut Phantom, requests: Vec<AddRequest>, refused: Vec<AddFailure>) {
    let batch = Batch::new(None, refused);
    if requests.is_empty() {
        batch.finish(s);
    } else {
        let answer = batch.answer(&requests);
        s.service.add(requests, None, answer);
    }
}

/// Sends `command`; a failure shows in the status bar.
pub fn command(service: &mut dyn Service, ids: &[TorrentId], command: TorrentCommand) {
    service.command(
        ids,
        command,
        Box::new(|s: &mut Phantom, result: Result<(), client::Failure>, _: &mut Tx| {
            if let Err(e) = result {
                s.errors.report(Notice::new(display::sentence(&format!("{e:#}"))));
            }
        }),
    );
}

/// Asks where the next refused torrents go: one picker at a time, so adds refused while it asks
/// wait their turn.
fn ask_destination(s: &mut Phantom, tx: &mut Tx) {
    if s.adds.asking {
        return;
    }
    let Some(Waiting { requests, mut batch }) = s.adds.waiting.pop_front() else { return };
    s.adds.asking = true;
    let options = FileDialogOptions { title: destination_title(&requests), directory: downloads(), ..Default::default() };
    tx.open_folder(options, move |s: &mut Phantom, chosen, tx| {
        s.adds.asking = false;
        let folder = match chosen {
            Ok(Some(folder)) => client::absolute(&folder).map_err(|e| Some(format!("{e:#}"))),
            Ok(None) => Err(None),
            Err(e) => Err(Some(format!("can't ask where to download: {e}"))),
        };
        match folder {
            Ok(folder) => {
                let answer = batch.answer(&requests);
                s.service.add(requests, Some(folder), answer);
            }
            // None added: the batch fails for them, with the reason, unless cancelled.
            Err(why) => {
                if let Some(why) = why {
                    batch.total += requests.len();
                    batch.failed.extend(requests.iter().map(|r| AddFailure { what: what(r), why: display::sentence(&why) }));
                }
                batch.finish(s);
            }
        }
        ask_destination(s, tx);
    });
}

/// What the folder picker asks.
fn destination_title(requests: &[AddRequest]) -> String {
    match requests {
        [AddRequest::File(path)] => match path.file_stem() {
            Some(name) => format!("Choose where to download {}", name.to_string_lossy()),
            None => "Choose where to download the torrent".into(),
        },
        [_] => "Choose where to download the torrent".into(),
        _ => format!("Choose where to download {} torrents", requests.len()),
    }
}

/// Where the folder picker starts: `~/Downloads`.
fn downloads() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"))
}

/// Removes the selected torrents, or deletes them with their files if `delete`. Torrents
/// fetching info have no files yet and stop either way.
pub fn remove_selected(service: &mut dyn Service, table: &mut TableState, delete: bool) {
    let ids = selected_torrents(table);
    command(service, &ids, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove });
    table.clear_selection();
}

pub fn sort(torrents: &mut [TorrentInfo], column: &str, dir: SortDir) {
    torrents.sort_by(|a, b| {
        dir.apply(match column {
            "name" => rune_kit::natural_cmp(a.name.as_deref().unwrap_or(""), b.name.as_deref().unwrap_or("")),
            "progress" => a.progress().total_cmp(&b.progress()),
            "status" => a.status.cmp(&b.status),
            "size" => a.size_bytes.cmp(&b.size_bytes),
            "down" => a.download_bytes_per_sec.cmp(&b.download_bytes_per_sec),
            "up" => a.upload_bytes_per_sec.cmp(&b.upload_bytes_per_sec),
            "peers" => a.live_peers.cmp(&b.live_peers),
            "eta" => a.eta().cmp(&b.eta()),
            _ => a.id.cmp(&b.id),
        })
    });
}

pub fn sort_files(details: &mut Details, column: &str, dir: SortDir) {
    details.files.sort_by(|a, b| {
        dir.apply(match column {
            "path" => rune_kit::natural_cmp(&a.path.to_string_lossy(), &b.path.to_string_lossy()),
            "progress" => a.progress().total_cmp(&b.progress()),
            "size" => a.size_bytes.cmp(&b.size_bytes),
            "priority" => a.priority.cmp(&b.priority),
            _ => a.index.cmp(&b.index),
        })
    });
}

#[cfg(test)]
mod link_problem_tests {
    //! What Add accepts, over generated text rather than examples.

    use proptest::prelude::*;

    use super::link_problem;
    use client::LinkProblem;

    /// An info hash: 40 hex digits, or 32 base32 letters and digits, in either case.
    fn info_hash() -> impl Strategy<Value = String> {
        prop_oneof!["[0-9a-fA-F]{40}", "[a-zA-Z2-7]{32}"]
    }

    /// Other magnet parameters, never `xt=`: their keys have no `x`.
    fn params() -> impl Strategy<Value = Vec<String>> {
        proptest::collection::vec("[a-w]{1,4}=[A-Za-z0-9%+.:/-]{0,12}", 0..4)
    }

    fn whitespace() -> impl Strategy<Value = String> {
        "[ \t\n\r]{0,3}"
    }

    proptest! {
        #[test]
        fn whitespace_around_never_changes_the_answer(text in "\\PC{0,60}", before in whitespace(), after in whitespace()) {
            prop_assert_eq!(link_problem(&format!("{before}{text}{after}")), link_problem(&text));
        }

        #[test]
        fn a_magnet_with_an_info_hash_is_a_link(hash in info_hash(), params in params(), at in 0..4usize) {
            let mut params = params;
            params.insert(at.min(params.len()), format!("xt=urn:btih:{hash}"));
            let link = format!("magnet:?{}", params.join("&"));
            prop_assert_eq!(link_problem(&link), None, "{}", link);
        }

        #[test]
        fn a_magnet_without_xt_has_no_info_hash(params in params()) {
            let link = format!("magnet:?{}", params.join("&"));
            prop_assert_eq!(link_problem(&link), Some(LinkProblem::NoInfoHash), "{}", link);
        }

        #[test]
        fn a_bare_info_hash_is_a_link(hash in info_hash()) {
            prop_assert_eq!(link_problem(&hash), None);
        }

        #[test]
        fn a_torrent_url_is_a_link(scheme in "https?", host in "[a-z0-9-]{1,12}(\\.[a-z]{2,6}){1,2}", path in "(/[A-Za-z0-9._%-]{1,10}){0,3}", name in "[A-Za-z0-9._%-]{1,16}") {
            let url = format!("{scheme}://{host}{path}/{name}.torrent");
            prop_assert_eq!(link_problem(&url), None, "{}", url);
        }

        /// Text with a character no info hash has, and no scheme in front.
        #[test]
        fn other_text_is_not_a_link(before in "\\PC{0,20}", odd in "[!_./ #?=-]", after in "\\PC{0,20}") {
            let text = format!("{before}{odd}{after}");
            let trimmed = text.trim();
            prop_assume!(!["magnet:?", "http://", "https://"].iter().any(|scheme| trimmed.starts_with(scheme)));
            prop_assert_eq!(link_problem(&text), Some(LinkProblem::NotALink), "{:?}", text);
        }

        #[test]
        fn nothing_or_only_whitespace_is_not_a_link(text in whitespace()) {
            prop_assert_eq!(link_problem(&text), Some(LinkProblem::NotALink));
        }
    }
}
