//! A torrent librqbit has, as Phantom sees it: what it has of each file is told from its pieces.

use std::ops::Deref;
use std::sync::{Arc, Weak};

use librqbit::api::TorrentIdOrHash;
use librqbit::{Api, ManagedTorrent, Session, TorrentStats};

use super::bitv;

/// librqbit's torrent, with its [`stats`](Held::stats) put right.
#[derive(Clone)]
pub struct Held {
    torrent: Arc<ManagedTorrent>,
    /// The session it's in, the only one that tells its pieces.
    session: Weak<Session>,
}

impl Held {
    pub fn new(torrent: Arc<ManagedTorrent>, session: &Arc<Session>) -> Self {
        Self { torrent, session: Arc::downgrade(session) }
    }

    /// librqbit's stats, with what it has of each file counted from the pieces it has (#414).
    /// Its own count per file isn't: it adds a piece to it in a step of its own, after having it. A
    /// pause between the two leaves the piece out for good, and one got twice (its file set Off and
    /// Normal while it's hashed) is added twice, so a whole file counts as more than its size. Once
    /// librqbit counts right, this can go.
    pub fn stats(&self) -> TorrentStats {
        // Its pieces after its stats: what it's finished with is always had.
        let mut stats = self.torrent.stats();
        // It says nothing per file while it checks, or failed.
        if !stats.file_progress.is_empty()
            && let Some(had) = self.file_progress()
        {
            stats.file_progress = had;
        }
        stats
    }

    /// What its pieces say it has of each file; `None` unless the session still has it, running or
    /// stopped.
    fn file_progress(&self) -> Option<Vec<u64>> {
        let session = self.session.upgrade()?;
        let id = TorrentIdOrHash::Id(self.torrent.id());
        // Taken out and added again, its id is another's.
        session.get(id).filter(|t| Arc::ptr_eq(t, &self.torrent))?;
        let (pieces, _) = Api::new(session, None).api_dump_haves(id).ok()?;
        let (lens, piece_length) = self
            .torrent
            .with_metadata(|m| (m.file_infos.iter().map(|f| f.len).collect::<Vec<_>>(), m.info.lengths().default_piece_length()))
            .ok()?;
        Some(bitv::file_progress(pieces.as_raw_slice(), &lens, piece_length))
    }
}

impl Deref for Held {
    type Target = Arc<ManagedTorrent>;

    fn deref(&self) -> &Arc<ManagedTorrent> {
        &self.torrent
    }
}
