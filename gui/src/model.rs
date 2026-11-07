//! The app's state, [`Phantom`], and what actions do to it: applying the service's events, naming
//! table rows, reading the selection, adding and removing, sorting. No drawing; views call these.
//! Each helper takes only the pieces it uses.

use std::path::PathBuf;

use client::format;
use proto::{Listing, PendingAdd, PendingId, TorrentId, TorrentInfo};
use rune::{AppHandle, TextEdit};
use rune_kit::{SortDir, TableState};

use crate::service::{AddRequest, Connection, Event, Notice, Service};

pub struct Phantom {
    pub service: Box<dyn Service>,
    /// To bring the window back for a link opened while it's closed; None in tests.
    pub window: Option<AppHandle<Phantom>>,
    pub torrents: Vec<TorrentInfo>,
    /// Adds the service is still resolving, shown above the torrents.
    pub pending: Vec<PendingAdd>,
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
            pending: Vec::new(),
            table: TableState::new().multi_select(true),
            magnet: TextEdit::new(""),
            connection: Connection::Connecting,
            notice: None,
        }
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Listing(Listing { torrents, pending }) => {
                self.pending = pending;
                self.torrents = torrents;
                if let Some((column, dir)) = self.table.sort() {
                    let column = column.to_owned();
                    sort(&mut self.torrents, &column, dir);
                }
            }
            Event::Connection(connection) => self.connection = connection,
            Event::Notice(notice) => self.notice = Some(notice),
        }
    }

    pub fn row_count(&self) -> usize {
        self.pending.len() + self.torrents.len()
    }
}

// Table rows are the pending adds (ids "p<n>") followed by the torrents (ids "<n>").

pub fn row_id(pending: &[PendingAdd], torrents: &[TorrentInfo], i: usize) -> String {
    match pending.get(i) {
        Some(p) => format!("p{}", p.id),
        None => torrents[i - pending.len()].id.to_string(),
    }
}

pub fn selected_torrents(table: &TableState) -> Vec<TorrentId> {
    table.selected().filter_map(|id| id.parse().ok()).collect()
}

pub fn selected_pending(table: &TableState) -> Vec<PendingId> {
    table.selected().filter_map(|id| id.strip_prefix('p')?.parse().ok()).collect()
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

/// Removes the selected torrents and stops the selected pending adds, which have no files yet.
pub fn remove_selected(service: &mut dyn Service, table: &mut TableState, delete_files: bool) {
    let (torrents, pending) = (selected_torrents(table), selected_pending(table));
    if !torrents.is_empty() {
        service.remove(&torrents, delete_files);
    }
    if !pending.is_empty() {
        service.cancel_adds(&pending);
    }
    table.clear_selection();
}

pub fn sort(torrents: &mut [TorrentInfo], column: &str, dir: SortDir) {
    torrents.sort_by(|a, b| {
        dir.apply(match column {
            "name" => rune_kit::natural_cmp(a.name.as_deref().unwrap_or(""), b.name.as_deref().unwrap_or("")),
            "progress" => a.progress().total_cmp(&b.progress()),
            "status" => format::status(a).cmp(format::status(b)),
            "size" => a.total_bytes.cmp(&b.total_bytes),
            "down" => a.download_bytes_per_sec.cmp(&b.download_bytes_per_sec),
            "up" => a.upload_bytes_per_sec.cmp(&b.upload_bytes_per_sec),
            "peers" => a.live_peers.cmp(&b.live_peers),
            "eta" => a.eta().cmp(&b.eta()),
            _ => a.id.cmp(&b.id),
        })
    });
}
