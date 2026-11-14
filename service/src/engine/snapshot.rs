//! What clients see of the torrents at one moment: the list, and the details of the torrents
//! watched. The actor builds one after every event it handles and publishes it on a `watch`
//! channel when it changed, numbered ([`Published`]); each subscriber gets the latest
//! ([`Published::for_client`]), so everything a client gets comes from one moment.

use std::collections::BTreeMap;

use proto::{Details, TorrentId, TorrentInfo};

/// The latest snapshot, and how many changes there were before it.
#[derive(Debug, Clone, Default)]
pub struct Published {
    /// This run of the engine's: the time it started, in nanoseconds, so a later run's is larger.
    pub epoch: u64,
    pub version: u64,
    pub(super) snapshot: Snapshot,
}

impl Published {
    /// What a subscriber watching `watching` gets, with the settings `reset` at start.
    pub fn for_client(&self, watching: &[TorrentId], reset: &[String]) -> proto::Snapshot {
        let Snapshot { torrents, details, volumes } = &self.snapshot;
        proto::Snapshot {
            epoch: self.epoch,
            version: self.version,
            watching: watching.to_vec(),
            torrents: torrents.clone(),
            details: watching.iter().filter_map(|id| details.get(id).cloned()).collect(),
            reset: reset.to_vec(),
            volumes: volumes.clone(),
        }
    }

    /// Whether some torrent is Downloading.
    pub fn downloading(&self) -> bool {
        self.snapshot.torrents.iter().any(|t| t.status == proto::Status::Downloading)
    }
}

#[cfg(test)]
use super::Failure;
#[cfg(test)]
use super::machine::Refusal;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    /// Every torrent, including those fetching info, by id.
    pub torrents: Vec<TorrentInfo>,
    /// The details of the watched torrents that have info.
    pub details: BTreeMap<TorrentId, Details>,
    /// The disks torrents still need space on ([`super::space`]).
    pub volumes: Vec<proto::Volume>,
}

impl Snapshot {
    /// Torrent `id`'s details, or why there are none: it isn't in the list, or has no info yet.
    #[cfg(test)]
    pub fn details(&self, id: TorrentId) -> Result<Details, Failure> {
        if let Some(details) = self.details.get(&id) {
            return Ok(details.clone());
        }
        let why = if self.torrents.iter().any(|t| t.id == id) { Refusal::NoInfoYet } else { Refusal::NotInList };
        Err(Failure::torrent(id, why))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proto::{AbsolutePath, Status, TorrentInfo, Unavailable};

    use super::*;

    fn status() -> impl Strategy<Value = Status> {
        let reason = Unavailable::FolderMissing { path: AbsolutePath::new("/x").unwrap() };
        proptest::sample::select(vec![
            Status::FetchingInfo,
            Status::Checking,
            Status::Downloading,
            Status::Seeding,
            Status::Paused,
            Status::Done,
            Status::MissingFiles,
            Status::DestinationUnavailable { reason },
            Status::Moving,
            Status::Error { message: "x".into() },
        ])
    }

    fn torrent(id: u64, status: Status) -> TorrentInfo {
        TorrentInfo {
            id,
            info_hash: format!("{id:040x}"),
            name: None,
            status,
            content: None,
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
            trackers: Vec::new(),
            location: None,
            actions: Vec::new(),
        }
    }

    proptest! {
        /// What keeps the Mac awake (#122): something is downloading exactly when some torrent's
        /// status is Downloading. Seeding, fetching info or checking isn't downloading.
        #[test]
        fn downloading_exactly_when_some_torrent_is_downloading(statuses in proptest::collection::vec(status(), 0..8)) {
            let torrents = statuses.iter().cloned().enumerate().map(|(id, s)| torrent(id as u64, s)).collect();
            let published = Published { snapshot: Snapshot { torrents, ..Snapshot::default() }, ..Published::default() };
            prop_assert_eq!(published.downloading(), statuses.contains(&Status::Downloading));
        }
    }
}
