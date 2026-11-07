//! The app's state, [`Phantom`], and what actions do to it: applying the service's events, reading
//! the selection, adding, removing and deleting, sorting, and the details pane of the one selected
//! torrent with its files' priorities. No drawing; views call these. Each helper takes only the
//! pieces it uses.

use std::path::{Path, PathBuf};

use proto::{Added, Details, Priority, Status, TorrentId, TorrentInfo};
use rune::{AppHandle, TextEdit, Tx};
use client::format;
use rune_kit::{SortDir, TableState};

use crate::service::{AddRequest, Connection, Event, Notice, Service};

pub struct Phantom {
    pub service: Box<dyn Service>,
    /// To bring the window back for a link opened while it's closed; None in tests.
    pub window: Option<AppHandle<Phantom>>,
    pub torrents: Vec<TorrentInfo>,
    /// Rows are keyed by torrent id.
    pub table: TableState,
    pub magnet: TextEdit,
    pub connection: Connection,
    pub notice: Option<Notice>,
    pub pane: DetailsPane,
}

/// The details pane: the files of the one selected torrent.
pub struct DetailsPane {
    /// The torrent the service sends details of.
    pub watching: Option<TorrentId>,
    /// The latest details of `watching`; `None` until they arrive.
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
            connection: Connection::Connecting,
            notice: None,
            pane: DetailsPane::new(),
        }
    }

    /// `tx` scrolls the list, to show a torrent that was added again.
    pub fn apply(&mut self, event: Event, tx: &mut Tx) {
        match event {
            Event::Listing(torrents) => {
                self.torrents = torrents;
                if let Some((column, dir)) = self.table.sort() {
                    let column = column.to_owned();
                    sort(&mut self.torrents, &column, dir);
                }
            }
            Event::Details(mut details) if self.pane.watching == Some(details.id) => {
                if let Some((column, dir)) = self.pane.files.sort() {
                    let column = column.to_owned();
                    sort_files(&mut details, &column, dir);
                }
                self.pane.details = Some(details);
            }
            // Of a torrent no longer selected, answered late.
            Event::Details(_) => {}
            Event::Added(added) => self.notice = Some(show_added(&mut self.table, &added, tx)),
            Event::Connection(connection) => self.connection = connection,
            Event::Notice(notice) => self.notice = Some(notice),
        }
    }
}

/// What to tell the user about an add. A torrent that was already in the list gets selected and
/// scrolled to, instead of a second row.
fn show_added(table: &mut TableState, added: &Added, tx: &mut Tx) -> Notice {
    let name = added.name.as_deref().unwrap_or("The torrent");
    if added.already_existed {
        table.reveal(added.id.to_string(), tx);
        Notice::info(format!("{name} is already in the list"))
    } else {
        Notice::info(format!("Added {name}"))
    }
}

pub fn selected_torrents(table: &TableState) -> Vec<TorrentId> {
    table.selected().filter_map(|id| id.parse().ok()).collect()
}

/// The selected torrents that can be paused and resumed: not those fetching info.
pub fn selected_with_info(table: &TableState, torrents: &[TorrentInfo]) -> Vec<TorrentId> {
    let selected = selected_torrents(table);
    torrents.iter().filter(|t| t.status != Status::FetchingInfo && selected.contains(&t.id)).map(|t| t.id).collect()
}

/// The torrent the details pane shows: the selection, when it's exactly one torrent.
pub fn details_torrent<'a>(table: &TableState, torrents: &'a [TorrentInfo]) -> Option<&'a TorrentInfo> {
    match selected_torrents(table)[..] {
        [id] => torrents.iter().find(|t| t.id == id),
        _ => None,
    }
}

/// Has the service send the details of the torrent the pane shows, once it has info, and forgets
/// the last one's. Runs when the selection or the list changes.
pub fn follow_selection(service: &mut dyn Service, table: &TableState, torrents: &[TorrentInfo], pane: &mut DetailsPane) {
    let wanted = details_torrent(table, torrents).filter(|t| t.status != Status::FetchingInfo).map(|t| t.id);
    if wanted == pane.watching {
        return;
    }
    service.watch_details(wanted);
    // Leaving a torrent: its files and their selection go.
    if pane.watching.is_some() {
        *pane = DetailsPane::new();
    }
    pane.watching = wanted;
}

/// What Reveal in Finder shows: where the torrent is on disk, the file itself for a single-file
/// torrent and its folder otherwise. Nothing while it's fetching info or its details are loading.
pub fn reveal_target<'a>(torrent: &TorrentInfo, pane: &'a DetailsPane) -> Option<&'a Path> {
    let details = pane.details.as_ref().filter(|d| d.id == torrent.id && torrent.status != Status::FetchingInfo)?;
    Some(&details.location)
}

/// Whether Check applies to the torrent the pane shows: it has info and its files can be read now
/// (not while fetching info, being checked or moved, or away with its destination).
pub fn can_check(torrent: &TorrentInfo, pane: &DetailsPane) -> bool {
    let has_info = pane.details.as_ref().is_some_and(|d| d.id == torrent.id);
    has_info
        && !matches!(
            torrent.status,
            Status::FetchingInfo | Status::Checking | Status::Moving | Status::DestinationUnavailable { .. }
        )
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
        service.set_priority(details.id, &files, priority);
    }
}

/// "Size of Full size" when some files are Off, else just the Size.
pub fn size_label(t: &TorrentInfo) -> String {
    if t.size_bytes == t.full_size_bytes {
        format::bytes(t.size_bytes)
    } else {
        format!("{} of {}", format::bytes(t.size_bytes), format::bytes(t.full_size_bytes))
    }
}

/// Whether the text is something Add accepts: a magnet link with an info hash (40 hex digits, or
/// 32 base32 letters and digits) or an http(s) URL.
pub fn is_link(text: &str) -> bool {
    let text = text.trim();
    if let Some(query) = text.strip_prefix("magnet:?") {
        let hash = query.split('&').find_map(|kv| kv.strip_prefix("xt=urn:btih:"));
        return hash.is_some_and(|h| match h.len() {
            40 => h.bytes().all(|b| b.is_ascii_hexdigit()),
            32 => h.bytes().all(|b| b.is_ascii_alphanumeric()),
            _ => false,
        });
    }
    ["http://", "https://"].iter().any(|scheme| text.strip_prefix(scheme).is_some_and(|rest| !rest.is_empty()))
}

/// Sends the field's URL to the service and empties the field; does nothing unless it's a link.
pub fn submit_url(service: &mut dyn Service, field: &mut TextEdit) {
    if is_link(&field.text) {
        service.add(AddRequest::Url(field.text.trim().to_owned()));
        field.clear();
    }
}

pub fn add_files(service: &mut dyn Service, files: &[PathBuf]) {
    files.iter().for_each(|f| service.add(AddRequest::File(f.clone())));
}

/// Removes the selected torrents, or deletes them with their files if `delete`. Torrents
/// fetching info have no files yet and stop either way.
pub fn remove_selected(service: &mut dyn Service, table: &mut TableState, delete: bool) {
    let ids = selected_torrents(table);
    if delete { service.delete(&ids) } else { service.remove(&ids) }
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
