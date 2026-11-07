//! The app's state, [`Phantom`], and what actions do to it: applying the service's events, reading
//! the selection, adding, removing and deleting, sorting. No drawing; views call these. Each
//! helper takes only the pieces it uses.

use std::path::PathBuf;

use proto::{Added, Status, TorrentId, TorrentInfo};
use rune::{AppHandle, TextEdit, Tx};
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

/// Sends the field's URL to the service and empties the field; does nothing if it's blank.
pub fn submit_url(service: &mut dyn Service, field: &mut TextEdit) {
    let url = field.text.trim().to_owned();
    if !url.is_empty() {
        service.add(AddRequest::Url(url));
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
