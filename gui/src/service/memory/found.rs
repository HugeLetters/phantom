//! The fake's torrent moved or renamed by hand (#358), as the engine's: its files gone from its
//! place ([`moved_by_hand`](MemoryService::moved_by_hand)), it's Missing files; a Move or a Rename
//! to where the person put it finds it there: nothing moves, it points there, and the files back
//! at its paths make the size check find them, so it's checked, then runs as it did. A found
//! Rename's undo points it back, its files staying where they are.

use std::path::{Path, PathBuf};

use lifecycle::disk::Stamp;
use proto::{TorrentId, TorrentRefusal};

use super::{DiskChange, MemoryService};

impl MemoryService {
    /// Torrent `id`'s folder (or its one file) is moved or renamed by hand to `to`, an absolute path.
    pub fn moved_by_hand(mut self, id: TorrentId, to: impl Into<PathBuf>) -> Self {
        self.change(DiskChange::MovedByHand { id, to: to.into() });
        self
    }

    /// Torrent `id`'s folder (or its one file) is moved or renamed by hand to `to`, and its files
    /// there are of their names and lengths but other bytes: found there, none of its pieces is.
    pub fn moved_by_hand_onto_others(mut self, id: TorrentId, to: impl Into<PathBuf>) -> Self {
        self.others_by_hand.insert(id);
        self.moved_by_hand(id, to)
    }

    /// Torrent `id`'s check ended: found on files that aren't its own, none of its pieces is there,
    /// so it's stopped, its files untouched, as the engine's (#358). Whether it was.
    pub(super) fn checked_onto_others(&mut self, id: TorrentId) -> bool {
        if self.by_hand.contains_key(&id) || !self.others_by_hand.remove(&id) {
            return false;
        }
        for f in self.files.get_mut(&id).into_iter().flatten() {
            f.progress_bytes = 0;
        }
        // The engine's found Move is checked with none of its files returning: none is missing.
        if let Some(m) = self.machines.get_mut(&id)
            && let lifecycle::Phase::Checking { returning } = &mut m.state.phase
        {
            returning.clear();
        }
        self.dont_match.insert(id);
        true
    }

    /// Torrent `id`'s files leave its place for `to`: none is at its path any more.
    pub(super) fn left_by_hand(&mut self, id: TorrentId, to: PathBuf) {
        let Some(m) = self.machines.get_mut(&id) else { return };
        let was: std::collections::BTreeMap<usize, Option<Stamp>> = m.on_disk.iter().map(|(i, s)| (*i, *s)).collect();
        for stamp in m.on_disk.values_mut() {
            *stamp = None;
        }
        self.by_hand.insert(id, (to, was));
    }

    /// Whether torrent `id` is found at `at`, as the engine judges it: the person put it there, and
    /// none of its files is at its place.
    pub(super) fn found_at(&self, id: TorrentId, at: &Path) -> bool {
        let folded = |p: &Path| p.to_string_lossy().to_lowercase();
        let gone = self.machines.get(&id).is_some_and(|m| m.on_disk.values().all(Option::is_none));
        gone && self.by_hand.get(&id).is_some_and(|(to, _)| folded(to) == folded(at))
    }

    /// Torrent `id`, pointed where it's found: its files are at its paths again, as they were.
    pub(super) fn take_found(&mut self, id: TorrentId) {
        let Some((_, was)) = self.by_hand.remove(&id) else { return };
        if let Some(m) = self.machines.get_mut(&id) {
            m.on_disk.extend(was);
        }
        self.size_check(id);
        self.refresh(id);
        self.changed = true;
    }

    /// The undo of a Rename that found torrent `id`: it's called `was` again, moving nothing, never
    /// over anything at that name; its files stay where they are, so it's Missing files again. Put
    /// back under `was` by hand, it's found there.
    pub(super) fn point_back(&mut self, id: TorrentId, was: &str) -> Result<Option<String>, TorrentRefusal> {
        let destination = self.destinations.get(&id).cloned().unwrap_or_else(super::downloads);
        let t = self.torrents.iter().find(|t| t.id == id).ok_or(TorrentRefusal::NotInList)?;
        let at = self.location(t).ok_or(TorrentRefusal::NoInfoYet)?.to_path_buf();
        let found = self.found_at(id, &destination.join(was));
        self.name_free(id, &destination, was, found)?;
        self.take_name(id, was);
        if found {
            self.take_found(id);
            return Ok(None);
        }
        self.left_by_hand(id, at);
        self.size_check(id);
        self.refresh(id);
        Ok(None)
    }
}
