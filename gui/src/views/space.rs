//! Free space (#127) in the list: a torrent's own warning in its row when it alone needs more than
//! its disk has free. The disks' warnings are the status bar's ([`crate::model::space`]).

use client::display;
use proto::TorrentId;

use crate::model::Phantom;

/// Torrent `id`'s own warning, when it alone needs more than its disk has free: why, for its
/// status cell's hover.
pub fn too_big(s: &Phantom, id: TorrentId) -> Option<String> {
    s.volumes.iter().find_map(|v| {
        let (needs, free) = (v.too_big(id)?, v.free_bytes?);
        Some(display::too_big_why(needs, free, v))
    })
}
