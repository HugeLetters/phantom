//! The fake's Rename (#331), as the engine's: an action on the undo stack, carried out as it starts
//! (the fake's disk renames at once), whose undo renames it back. Refused as the engine refuses
//! it: by its lifecycle (as a Move), where something is at the new name on the pretend disk
//! ([`taken_on_disk`](MemoryService::taken_on_disk)), or where another torrent in the list keeps its
//! files; its own name does nothing. The row shows the new name, and its info's as the original.
//! ⌘Z on a Rename whose torrent is moving is refused rather than wait. One renamed by hand to the
//! new name is found there (#358, `found.rs`), and its undo points it back.

use std::path::{Path, PathBuf};

use lifecycle::undo::{Part, What};
use lifecycle::{Event, Phase};
use proto::{AbsolutePath, Action, CantUndo, TorrentCommand, TorrentId, TorrentRefusal};

use super::MemoryService;

impl MemoryService {
    /// Something is at `path` on the pretend disk: a Rename to it, or back to it, is refused.
    #[cfg(test)]
    pub fn taken_on_disk(mut self, path: impl Into<PathBuf>) -> Self {
        self.taken_on_disk.insert(path.into());
        self
    }

    /// The name torrent `id` has on disk: its one file's, or its own folder's.
    fn disk_name(&self, id: TorrentId) -> Option<String> {
        match self.files.get(&id).map(Vec::as_slice) {
            Some([only]) => Some(only.path.to_string_lossy().into_owned()),
            _ => self.torrents.iter().find(|t| t.id == id)?.name.clone(),
        }
    }

    /// Renames torrent `id` to `name` now: the name it had, if it had another; none to its own
    /// name; or why not, as the engine refuses it. Its files renamed there by hand, it's found
    /// there (#358): nothing moves, and they're back at its paths.
    pub(super) fn rename(&mut self, id: TorrentId, name: &str) -> Result<Option<String>, TorrentRefusal> {
        let m = self.machines.get(&id).ok_or(TorrentRefusal::NotInList)?;
        lifecycle::decide(&m.state, &Event::Command(Action::Rename))?;
        let was = self.disk_name(id).ok_or(TorrentRefusal::NoInfoYet)?;
        if was == name {
            return Ok(None);
        }
        let destination = self.destinations.get(&id).cloned().unwrap_or_else(super::downloads);
        let found = self.found_at(id, &destination.join(name));
        self.name_free(id, &destination, name, found)?;
        self.take_name(id, name);
        if found {
            self.take_found(id);
        }
        Ok(Some(was))
    }

    /// Whether torrent `id` can take the name `name` in `folder`, as the engine judges it for a
    /// Rename and a Move under a new name (#270): nothing on the pretend disk at it (but its own
    /// files, `found` there: #358), and it isn't another listed torrent's place.
    pub(super) fn name_free(&self, id: TorrentId, folder: &Path, name: &str, found: bool) -> Result<(), TorrentRefusal> {
        let at = folder.join(name);
        let path = || AbsolutePath::new(at.clone()).expect("absolute");
        let folded = |p: &Path| p.to_string_lossy().to_lowercase();
        let by_hand = self.by_hand.values().map(|(p, _)| p);
        if !found && self.taken_on_disk.iter().chain(by_hand).any(|p| folded(p) == folded(&at)) {
            return Err(TorrentRefusal::NameTaken { path: path() });
        }
        let theirs = self.torrents.iter().filter(|t| t.id != id).find(|t| t.location.as_deref().is_some_and(|l| folded(l) == folded(&at)));
        match theirs {
            Some(other) => Err(TorrentRefusal::NameOfAnother { path: path(), other: other.display_name().to_owned() }),
            None => Ok(()),
        }
    }

    /// Torrent `id` is called `name` on disk from now: its row shows it, its info's as the original.
    pub(super) fn take_name(&mut self, id: TorrentId, name: &str) {
        if let Some([only]) = self.files.get_mut(&id).map(Vec::as_mut_slice) {
            only.path = PathBuf::from(name);
        }
        if let Some(t) = self.torrent_mut(id) {
            let original = t.original_name.take().or_else(|| t.name.clone());
            t.name = Some(name.to_owned());
            t.original_name = original.filter(|o| o != name);
        }
        self.refresh(id);
        self.changed = true;
    }

    /// Whether the top of the stack is a committed Rename whose torrent is moving now, or has a Move
    /// waiting its turn: its undo would wait for the move, so it's refused instead.
    pub(super) fn busy_rename(&self) -> bool {
        let Some(done) = self.undo.top().and_then(|p| p.done.as_ref()) else { return false };
        done.values().filter(|left| left.renamed.is_some()).any(|left| {
            let id = left.info.id;
            let moving = self.machines.get(&id).is_some_and(|m| matches!(m.state.phase, Phase::Moving { .. }));
            let lane = self.undo.lanes.get(&id).into_iter().flatten().filter_map(|s| self.undo.steps.get(s));
            moving || lane.into_iter().any(|s| matches!(s.what, What::Command(TorrentCommand::Move { .. })))
        })
    }

    /// What a Rename's undo, refused as a Rename is, says: the name it goes back to is taken, say.
    pub(super) fn undo_refused(id: TorrentId, why: TorrentRefusal) -> CantUndo {
        match why {
            TorrentRefusal::NameTaken { path } | TorrentRefusal::NameOfAnother { path, .. } => CantUndo::PathTaken { path },
            TorrentRefusal::NotInList => CantUndo::NotInList,
            TorrentRefusal::Moving => CantUndo::Busy,
            TorrentRefusal::DestinationUnavailable { reason, .. } => CantUndo::Destination { reason },
            TorrentRefusal::Deleting | TorrentRefusal::PartlyDeleted => CantUndo::Deleting,
            why => CantUndo::Refused { refusal: Box::new(proto::Refusal::Torrent { id, why }) },
        }
    }

    /// Whether a Remove or a Delete of torrent `id` waits behind step `seq`: a Rename waiting for it
    /// is cancelled, as a Move is.
    pub(super) fn removed_behind(&self, seq: u64, id: TorrentId) -> bool {
        let behind = self.undo.lanes.get(&id).into_iter().flatten().filter(|s| **s > seq).filter_map(|s| self.undo.steps.get(s));
        behind.into_iter().any(|s| {
            matches!(&s.what, What::Action(parts)
                if matches!(parts.get(&id), Some(Part::Doing(r)) if matches!(r.action, proto::UndoAction::Remove | proto::UndoAction::Delete)))
        })
    }
}
