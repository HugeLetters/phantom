//! What clients see of the torrents at one moment: the list, and the details of the torrents
//! watched. The actor builds one after every event it handles and publishes it on a `watch`
//! channel when it changed; [`super::Engine`] answers `List` and `Details` from the latest, so
//! everything a client gets comes from one moment.

use std::collections::BTreeMap;

use proto::{Details, TorrentId, TorrentInfo};

use super::Failure;
use super::machine::Refusal;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    /// Every torrent, including those fetching info, by id.
    pub torrents: Vec<TorrentInfo>,
    /// The details of the watched torrents that have info.
    pub details: BTreeMap<TorrentId, Details>,
}

impl Snapshot {
    /// Torrent `id`'s details, or why there are none: it isn't in the list, or has no info yet.
    pub fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        if let Some(details) = self.details.get(&id) {
            return Ok(details.clone());
        }
        let why = if self.torrents.iter().any(|t| t.id == id) { Refusal::NoInfoYet } else { Refusal::NotInList };
        Err(Failure::torrent(id, why))
    }
}
