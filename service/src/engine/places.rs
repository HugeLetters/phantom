//! The places undos under way are putting a torrent's files back to (#371), kept for them, as a
//! Move's new paths are for it: nothing else of Phantom's writes there till the undo is over. A
//! torrent's first add is the one writer whose paths the actor doesn't know as it begins (its info
//! comes later), so the add itself [`Places::claim`]s them, before librqbit makes any file: it waits
//! while an undo has the place, and an undo that comes after it sees it. Whatever else could go there
//! asks the actor ([`Places::others`], part of `Actor::others_files`): a Move there is refused, and
//! so is another undo.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use proto::TorrentId;
use tokio::sync::Notify;

use super::moving::{self, Place};

#[derive(Clone, Default)]
pub struct Places {
    state: Arc<Mutex<State>>,
    freed: Arc<Notify>,
}

#[derive(Default)]
struct State {
    /// By the torrent each brings back.
    undoing: BTreeMap<TorrentId, Kept>,
    /// Where each torrent being added for the first time has its files, until its record says it.
    adding: BTreeMap<TorrentId, Vec<PathBuf>>,
    next: u64,
}

#[derive(Clone)]
struct Kept {
    /// Which came first: of two undos into one place, the first has it.
    order: u64,
    name: String,
    files: Vec<PathBuf>,
    /// What goes back: the torrent's whole folder, say, with nothing else let into it meanwhile.
    items: Vec<PathBuf>,
}

impl Places {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps the place of torrent `id`, which an undo brings back: its `files`, and the `items` that
    /// go back.
    pub fn reserve(&self, id: TorrentId, name: String, files: Vec<PathBuf>, items: Vec<PathBuf>) {
        let mut state = self.state();
        let order = state.next;
        state.next += 1;
        state.undoing.insert(id, Kept { order, name, files, items });
    }

    /// The undo bringing torrent `id` back is over: what waited for its place goes on.
    pub fn release(&self, id: TorrentId) {
        if self.state().undoing.remove(&id).is_some() {
            self.freed.notify_waiters();
        }
    }

    /// Torrent `id`'s first add ended: its record says where its files are, or it has none.
    pub fn added(&self, id: TorrentId) {
        self.state().adding.remove(&id);
    }

    /// The files of the torrents but `id` being added for the first time, or brought back by an undo:
    /// one that came before `id`'s own, if an undo brings `id` back.
    pub fn others(&self, id: TorrentId) -> Vec<(TorrentId, PathBuf)> {
        let state = self.state();
        let own = state.undoing.get(&id).map_or(u64::MAX, |kept| kept.order);
        let undoing = state.undoing.iter().filter(|(other, kept)| **other != id && kept.order < own).map(|(id, kept)| (id, &kept.files));
        let adding = state.adding.iter().filter(|(other, _)| **other != id);
        undoing.chain(adding).flat_map(|(id, files)| files.iter().map(|f| (*id, f.clone()))).collect()
    }

    /// Whether an undo is bringing torrent `id` back, and its name.
    pub fn name(&self, id: TorrentId) -> Option<String> {
        self.state().undoing.get(&id).map(|kept| kept.name.clone())
    }

    /// Says where torrent `id`, being added, has its `files`, and returns once no undo is putting
    /// anything back there. Nothing of them is made before. The disk is looked at off the actor and
    /// holding nothing: said first, an undo that comes meanwhile sees them.
    pub async fn claim(&self, id: TorrentId, files: Vec<PathBuf>) {
        let files = Arc::new(files);
        self.state().adding.insert(id, files.to_vec());
        loop {
            let freed = self.freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            let kept: Vec<Kept> = self.state().undoing.iter().filter(|(other, _)| **other != id).map(|(_, kept)| kept.clone()).collect();
            if kept.is_empty() {
                return;
            }
            let files = files.clone();
            let taken = tokio::task::spawn_blocking(move || kept.iter().any(|kept| overlap(&files, kept))).await;
            // A look that failed is no reason to write there.
            if matches!(taken, Ok(false)) {
                return;
            }
            freed.await;
        }
    }

    /// Whether the add of torrent `id` said where its files go.
    #[cfg(test)]
    pub fn says(&self, id: TorrentId) -> bool {
        self.state().adding.contains_key(&id)
    }
}

/// Whether any of `files` is one of `kept`'s, or inside or around what goes back, however either is
/// spelled or reached.
fn overlap(files: &[PathBuf], kept: &Kept) -> bool {
    let relative: Vec<(usize, PathBuf)> = files.iter().cloned().enumerate().collect();
    if moving::anothers(Path::new("/"), &relative, kept.files.iter().map(|f| ((), f.clone()))).is_some() {
        return true;
    }
    let mut seen = BTreeMap::new();
    let ours: Vec<&PathBuf> = files.iter().collect();
    let theirs: Vec<&PathBuf> = kept.files.iter().chain(&kept.items).collect();
    inside(&ours, &theirs, &mut seen) || inside(&theirs, &ours, &mut seen)
}

/// Whether any of `paths` is one of `others`, or inside one.
fn inside(paths: &[&PathBuf], others: &[&PathBuf], seen: &mut BTreeMap<PathBuf, Place>) -> bool {
    let mut place = |path: &Path| seen.entry(path.to_owned()).or_insert_with(|| moving::folder_place(path)).clone();
    let others: BTreeSet<Place> = others.iter().map(|p| place(p)).collect();
    paths.iter().any(|path| path.ancestors().any(|a| others.contains(&place(a))))
}
