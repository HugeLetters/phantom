//! Free space (#127, #209) in the list: every torrent that still needs space on a disk that's short
//! has a ⚠ in its status cell, and the disk's whole picture on its hover. The disk is a condition
//! with a place in the window, the rows it affects, so there's no notice for it.

use client::display;
use proto::TorrentId;

use crate::model::Phantom;

/// Why torrent `id`'s row is warned, if its disk is short of space for the torrents that need it:
/// the hover over its ⚠.
pub fn short_on_space(s: &Phantom, id: TorrentId) -> Option<String> {
    s.volumes.iter().find(|v| v.short() && v.needs.iter().any(|n| n.id == id)).and_then(|v| display::short_on_space(v, id))
}
