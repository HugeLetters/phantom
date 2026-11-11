//! The details pane of the one selected torrent: which torrent it shows ([`machine`]), what the
//! service was told to watch for it, and the shown torrent's details, from the same snapshot as
//! the list, so everything in it is of one moment (#111).

pub mod machine;

use proto::{Details, TorrentId};
use rune_kit::TableState;

pub struct DetailsPane {
    pub state: machine::State,
    /// The shown torrent's details, from the latest snapshot, sorted as the files' table is.
    pub details: Option<Details>,
    /// What the service was last told to send the details of.
    pub watching: Vec<TorrentId>,
    /// Rows are keyed by file index.
    pub files: TableState,
}

impl DetailsPane {
    pub fn new() -> Self {
        Self { state: machine::State::Empty, details: None, watching: Vec::new(), files: TableState::new().key("files").multi_select(true) }
    }
}

impl Default for DetailsPane {
    fn default() -> Self {
        Self::new()
    }
}
