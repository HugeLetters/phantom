//! A file in the Trash (#151, #217, #218, [ADR 0008]): everything about a file a Move to Trash took,
//! while its undo may put it back.
//!
//! - A Move to Trash [takes](Trashed::take) each file with librqbit's changes to the torrent's
//!   files held still around it. From then on librqbit's storage ([`Files`], which every storage
//!   librqbit gets is) never changes the file where it went, nor makes it again at its path, until
//!   its undo puts it back. The file is read-only to librqbit: it's read where it went, so a check,
//!   a start and an upload find it as before the move, and Undo is a rename and the priorities.
//! - What librqbit writes into it meanwhile (a piece it shares with a file the torrent gets) is kept
//!   aside, in the data dir (`undo-parts/`, by torrent, file and offset), read from there, and goes
//!   into the file as its undo [puts it back](Trashed::put_back).
//!   One that can't be written is kept in memory and [tried again](Trashed::retry_parts) at every
//!   `follow`; failing again, it's [reported](Trashed::parts_not_saved) until a retry keeps it or
//!   its file leaves the Trash (#304).
//! - Which files those are is what the undo stack says ([`Trashed::follow`]): each running Move to
//!   Trash's, each entry's still in the Trash as it went, each undo's until it's over. What's kept of
//!   any other goes.
//! - An undo checks the torrent only when librqbit has more of a file back than it had as it went,
//!   and what came couldn't go into the file ([`needs_check`]).
//! - Whether a file is still where it was opened is looked at on disk only for the files Moves to
//!   Trash took: one moved by hand is noticed by the size check, as Missing files.
//! - One gone from the Trash (emptied, or deleted from it by hand) while librqbit has it open is
//!   [found](Trashed::gone) and [closed](Trashed::let_go), so its space is freed (#234); the actor
//!   then adds the torrent back without its pieces (`machine::letgo`).
//!
//! [ADR 0008]: ../../../docs/adr/0008-undo-is-inverse-commands-in-the-service.md

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use librqbit::storage::TorrentStorage;
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use tracing::warn;

use super::slots::{Handle, Opened, Slots};
use super::trash::{self, Carried, Plan, Trash, identity};
use super::undo::{Back, machine};

/// The files in the Trash of every torrent, shared by the actor and every storage librqbit gets.
#[derive(Clone, Default)]
pub struct Trashed {
    torrents: Arc<Mutex<HashMap<String, Arc<Torrent>>>>,
    /// Where what librqbit writes into a file in the Trash is kept (`undo::parts_dir`), so it
    /// outlasts a restart: none in a test that doesn't say, which keeps it in memory only.
    parts: Option<PathBuf>,
    /// Held by a write making a part's folders and file, and by [`Trashed::follow`] sweeping the
    /// empty ones: the write makes `<hash>/` and then `<hash>/<index>/`, and a sweep between the two
    /// takes the first away, so the second (and the part) never reaches the disk. A leaf lock: taken
    /// under a torrent's `changes` and a file's slot, and nothing else is taken while it's held.
    parts_folders: Arc<Mutex<()>>,
    #[cfg(test)]
    hook: Option<Hook>,
    #[cfg(test)]
    reads_of: Option<ReadsOf>,
}

/// Where a test holds librqbit's storage: as it reads a file (a check does, piece by piece), or
/// with that file in hand, changes one (a write, its length set, a removal), or sets a length on a
/// file found in place.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Read,
    Change,
    /// A part of a file in the Trash being kept on disk, its torrent's folder made and its own not
    /// yet (the folders' lock held).
    Keep,
    Length,
    /// A read with its file in hand, as one from a disk that doesn't answer holds it.
    Reading,
    /// A look closing a file deleted by hand ([`Trashed::close_deleted`]), off the actor.
    Close,
}

/// A test's hold on librqbit's storage: it returns once what's at `At` may go on.
#[cfg(test)]
pub type Hook = Arc<dyn Fn(At) + Send + Sync>;

/// A test's hold on librqbit's reads of torrent `.0` (its info hash): it returns once the read may go on.
#[cfg(test)]
pub type ReadsOf = Arc<dyn Fn(&str) + Send + Sync>;

/// A torrent's files in the Trash, shared by every storage of it.
#[derive(Default)]
struct Torrent {
    /// Held to read by each change librqbit makes to a file while it makes it, to write while a
    /// file moves or comes back: a move waits for the change under way, and none reaches the file
    /// meanwhile.
    changes: RwLock<()>,
    files: Mutex<Away>,
    /// Whether a Move to Trash ever took a file of it: a torrent with none pays nothing to ask.
    any: AtomicBool,
    /// Its open files, which every storage of it has a handle of.
    slots: Arc<Slots>,
}

#[derive(Default)]
struct Away {
    /// The files in the Trash, by path: their index and where they went.
    away: BTreeMap<PathBuf, (usize, trash::Trashed)>,
    /// Taken by a Move to Trash still running, which the stack may not say yet.
    taking: BTreeMap<PathBuf, (usize, trash::Trashed)>,
    /// Every file a Move to Trash took since the start, or the stack said at it: only these are
    /// looked for on disk. One found moved that's no longer in the Trash for an undo is made again
    /// where librqbit writes it, as any file set Off that isn't there.
    taken: BTreeSet<PathBuf>,
    /// What librqbit wrote into each file in the Trash, by index and offset.
    kept: BTreeMap<usize, BTreeMap<u64, Vec<u8>>>,
    /// The files in the Trash that couldn't be opened where they went, by index: librqbit can't read
    /// them, so a check finds less of them.
    unread: BTreeSet<usize>,
    /// The parts in `kept` whose write to disk failed, by file index and offset, until one succeeds.
    unsaved: BTreeMap<(usize, u64), Unsaved>,
}

/// A part that didn't reach the disk (#304).
struct Unsaved {
    /// The file's path where it was, before it went to the Trash.
    file: PathBuf,
    /// Why the last write failed.
    reason: String,
    /// Whether a retry failed too: only then is it [reported](Trashed::parts_not_saved).
    retried: bool,
}

impl Trashed {
    /// Keeping what librqbit writes into a file in the Trash in `dir`.
    pub fn keeping_parts_in(self, dir: PathBuf) -> Self {
        Self { parts: Some(dir), ..self }
    }

    /// With librqbit's storage holding at `hook`.
    #[cfg(test)]
    pub fn with_hook(self, hook: Hook) -> Self {
        Self { hook: Some(hook), ..self }
    }

    /// With each read librqbit's storage makes held at `reads_of`, given its torrent's info hash.
    #[cfg(test)]
    pub fn with_reads_of(self, reads_of: ReadsOf) -> Self {
        Self { reads_of: Some(reads_of), ..self }
    }

    /// Where librqbit's storage holds now.
    #[cfg(test)]
    pub fn hook(&self) -> Option<Hook> {
        self.hook.clone()
    }

    /// Moves `plan`'s items of torrent `info_hash` to the Trash, its files `files` (by index and
    /// path), telling `went` of each that goes, until `stop`: [`trash::carry_out_until`], each item
    /// moving with librqbit's changes to the torrent's files held still, and from then on in the
    /// Trash, read-only to librqbit. On the blocking task that moves it, never the actor's.
    pub fn take(
        &self,
        info_hash: &str,
        plan: &Plan,
        files: &[(usize, PathBuf)],
        trash: &Trash,
        went: impl FnMut(&trash::Trashed),
        stop: &AtomicBool,
    ) -> Carried {
        let torrent = self.torrent(info_hash);
        trash::carry_out_holding(plan, trash, went, stop, |take| {
            // Poisoned only by a panic while a move held it: it guards no data, so that means nothing.
            let _held = torrent.changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            let taken = take();
            if let Ok(item) = &taken
                && let Some((index, _)) = files.iter().find(|(_, path)| *path == item.from)
            {
                let mut away = torrent.lock();
                away.taking.insert(item.from.clone(), (*index, item.clone()));
                away.away.insert(item.from.clone(), (*index, item.clone()));
                away.taken.insert(item.from.clone());
                torrent.any.store(true, Ordering::SeqCst);
            }
            taken
        })
    }

    /// The files of torrent `info_hash` an undo put back at their paths (by index and path), its
    /// priorities set: the torrent's again, as the stack says once the undo is reported over, next.
    /// What librqbit wrote into each while it was away goes into it first: the indices of those
    /// something went into. Holds the torrent's files still meanwhile: on a blocking task, never the
    /// actor's.
    pub fn put_back(&self, info_hash: &str, files: &[(usize, PathBuf)]) -> PutBack {
        let torrent = self.torrent(info_hash);
        // librqbit's changes wait meanwhile; its reads, which only look at what's away, don't.
        let _held = torrent.changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let indices: BTreeSet<usize> = files.iter().map(|(index, _)| *index).collect();
        let (kept, unread): (Vec<_>, BTreeSet<usize>) = {
            let away = torrent.lock();
            let kept = files.iter().filter_map(|(index, path)| Some((*index, path, away.kept.get(index).cloned()?))).collect();
            (kept, away.unread.intersection(&indices).copied().collect())
        };
        let mut written = BTreeSet::new();
        for (index, path, kept) in kept.into_iter().filter(|(_, _, k)| !k.is_empty()) {
            match write_parts(path, &kept) {
                Ok(()) => drop(written.insert(index)),
                Err(e) => warn!("writing what was kept of {path:?} while it was in the Trash: {e}"),
            }
        }
        let mut away = torrent.lock();
        for (index, path) in files {
            away.kept.remove(index);
            away.unsaved.retain(|(i, _), _| i != index);
            away.unread.remove(index);
            away.away.remove(path);
            if let Some(dir) = self.parts_of(info_hash) {
                let _ = std::fs::remove_dir_all(dir.join(index.to_string()));
            }
        }
        PutBack { written, unread }
    }

    /// The undo stack is `stack`: the files in the Trash are those it names (each running Move to
    /// Trash's, each entry's still in the Trash as it went, each undo's while it runs), and
    /// what a running move took that it doesn't say yet. What's kept of any other goes. At a start,
    /// before any torrent is added to librqbit; then whenever the stack changes, and as the Trash may
    /// be emptied.
    pub fn follow(&self, stack: &machine::State) {
        let mut named: HashMap<String, Named> = HashMap::new();
        let take = |named: &mut HashMap<String, Named>,
                    info_hash: &str,
                    previous: &[(usize, proto::Priority, PathBuf)],
                    items: &[trash::Trashed],
                    undoing: bool| {
            let named = named.entry(info_hash.to_owned()).or_default();
            for item in items {
                let Some((index, ..)) = previous.iter().find(|(_, _, path)| *path == item.from) else { continue };
                let there = undoing || item.to.as_deref().is_some_and(|to| identity(to).is_some() && identity(to) == item.seen);
                named.files.insert(item.from.clone(), (*index, item.clone(), there));
            }
        };
        // Each entry's, and each part over of an action still running (its entry until it's committed).
        let over = stack.steps.values().flat_map(|s| match &s.what {
            machine::What::Action(parts) => parts
                .values()
                .filter_map(|p| match p {
                    lifecycle::undo::Part::Over(Some(e)) => Some(e),
                    lifecycle::undo::Part::Over(None) | lifecycle::undo::Part::Doing(_) => None,
                })
                .collect(),
            machine::What::Undo { .. } | machine::What::Command(_) => Vec::new(),
        });
        for entry in stack.stack.iter().filter_map(|p| p.done.as_ref()).flat_map(|d| d.values()).chain(over) {
            if let Back::Files { previous, .. } = &entry.back {
                take(&mut named, &entry.info_hash, previous, &entry.put_back, false);
            }
        }
        for step in stack.steps.values() {
            match (&step.what, step.phase) {
                (machine::What::Action(parts), lifecycle::undo::Phase::Running | lifecycle::undo::Phase::Aborting) => {
                    for r in parts.values().filter_map(lifecycle::undo::Part::doing) {
                        if matches!(r.action, proto::UndoAction::TrashFiles { .. }) {
                            take(&mut named, &r.info_hash, &r.previous, &r.moved, false);
                            named.entry(r.info_hash.clone()).or_default().moving = true;
                        }
                    }
                }
                (machine::What::Undo { parts, .. }, _) => {
                    for entry in parts.values().filter_map(lifecycle::undo::Part::doing) {
                        if let Back::Files { previous, .. } = &entry.back {
                            take(&mut named, &entry.info_hash, previous, &entry.put_back, true);
                        }
                    }
                }
                (machine::What::Action(_), lifecycle::undo::Phase::Queued) | (machine::What::Command(_), _) => {}
            }
        }
        let hashes: BTreeSet<String> = self.torrents.lock().unwrap().keys().chain(named.keys()).cloned().collect();
        let mut kept: HashMap<String, BTreeSet<usize>> = HashMap::new();
        for hash in hashes {
            let named = named.remove(&hash).unwrap_or_default();
            let torrent = self.torrent(&hash);
            let mut away = torrent.lock();
            if !named.moving {
                away.taking.clear();
            }
            let mut now: BTreeMap<PathBuf, (usize, trash::Trashed)> =
                named.files.into_iter().filter(|(_, f)| f.2).map(|(path, f)| (path, (f.0, f.1))).collect();
            now.extend(away.taking.iter().map(|(path, f)| (path.clone(), f.clone())));
            away.taken.extend(now.keys().cloned());
            if !away.taken.is_empty() {
                torrent.any.store(true, Ordering::SeqCst);
            }
            let indices: BTreeSet<usize> = now.values().map(|(index, _)| *index).collect();
            away.kept.retain(|index, _| indices.contains(index));
            away.unread.retain(|index| indices.contains(index));
            away.unsaved.retain(|(index, _), _| indices.contains(index));
            away.away = now;
            kept.insert(hash, indices);
        }
        // What's kept on disk of files no longer in the Trash for an undo.
        let Some(dir) = &self.parts else { return };
        self.retry_parts();
        let _folders = self.parts_folders.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for torrent in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let hash = torrent.file_name().to_string_lossy().into_owned();
            for file in std::fs::read_dir(torrent.path()).into_iter().flatten().flatten() {
                let index = file.file_name().to_str().and_then(|n| n.parse::<usize>().ok());
                if index.is_none_or(|i| !kept.get(&hash).is_some_and(|k| k.contains(&i))) {
                    let _ = std::fs::remove_dir_all(file.path());
                }
            }
            let _ = std::fs::remove_dir(torrent.path());
        }
    }

    /// Keeps on disk again each part whose write failed (#304), at each [`follow`](Self::follow):
    /// the next change to the undo stack, and the size check. One that fails again is
    /// [reported](Self::parts_not_saved); one that succeeds isn't unsaved any more. A part held in
    /// memory only is lost by a restart, so no start can retry it.
    pub fn retry_parts(&self) {
        let torrents: Vec<(String, Arc<Torrent>)> = self.torrents.lock().unwrap().iter().map(|(h, t)| (h.clone(), t.clone())).collect();
        for (hash, torrent) in torrents {
            let Some(dir) = self.parts_of(&hash) else { continue };
            let todo: Vec<((usize, u64), Vec<u8>)> = {
                let away = torrent.lock();
                away.unsaved.keys().filter_map(|&k| Some((k, away.kept.get(&k.0)?.get(&k.1)?.clone()))).collect()
            };
            if todo.is_empty() {
                continue;
            }
            let tried: Vec<_> = {
                let _folders = self.parts_folders.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                todo.into_iter()
                    .map(|((index, offset), bytes)| (index, offset, keep_part(&dir, index, offset, &bytes, || {}), bytes))
                    .collect()
            };
            let mut away = torrent.lock();
            for (index, offset, result, bytes) in tried {
                // A newer write of the same part has its own say.
                if away.kept.get(&index).and_then(|k| k.get(&offset)) != Some(&bytes) {
                    continue;
                }
                match result {
                    Ok(()) => drop(away.unsaved.remove(&(index, offset))),
                    Err(e) => {
                        warn!("keeping what librqbit wrote into a file in the Trash, again: {e}");
                        if let Some(unsaved) = away.unsaved.get_mut(&(index, offset)) {
                            unsaved.reason = e.to_string();
                            unsaved.retried = true;
                        }
                    }
                }
            }
        }
    }

    /// The files in the Trash a part of which couldn't be kept on disk even at a retry, with why:
    /// what a restart would lose of them (#304). Only what's in memory, so the actor may ask.
    pub fn parts_not_saved(&self) -> Vec<proto::PartNotSaved> {
        let torrents: Vec<Arc<Torrent>> = self.torrents.lock().unwrap().values().cloned().collect();
        let mut failed: BTreeMap<PathBuf, String> = BTreeMap::new();
        for torrent in torrents {
            for unsaved in torrent.lock().unsaved.values().filter(|u| u.retried) {
                failed.entry(unsaved.file.clone()).or_insert_with(|| unsaved.reason.clone());
            }
        }
        failed.into_iter().map(|(file, reason)| proto::PartNotSaved { file, reason }).collect()
    }

    /// The files (by index) of torrent `info_hash` in the Trash for an undo, or being moved there:
    /// librqbit reads them where they went. Only what's in memory, so the actor may ask.
    pub fn in_trash(&self, info_hash: &str) -> BTreeSet<usize> {
        let Some(torrent) = self.torrents.lock().unwrap().get(info_hash).cloned() else { return BTreeSet::new() };
        let away = torrent.lock();
        away.away.values().chain(away.taking.values()).map(|(index, _)| *index).collect()
    }

    /// The files of torrent `info_hash` a Move to Trash took that are gone from the Trash (emptied,
    /// or deleted from it by hand), by index, while librqbit's storage still has them open: the
    /// Trash no longer has them for an undo ([`Trashed::follow`]), and the file it has open has no
    /// name left anywhere. Looks at each of them open (`fstat`), and waits
    /// for nothing: one being opened or closed is looked at next time. A torrent no move took a file
    /// of pays nothing.
    pub fn gone(&self, info_hash: &str) -> BTreeSet<usize> {
        let Some(torrent) = self.torrents.lock().unwrap().get(info_hash).cloned() else { return BTreeSet::new() };
        if !torrent.any.load(Ordering::SeqCst) {
            return BTreeSet::new();
        }
        // Not one the Trash still has for an undo, as `follow` last found: librqbit's handle may be
        // to a file replaced by hand at its path before it moved, which is whole where it went.
        let (taken, away): (BTreeSet<PathBuf>, BTreeSet<PathBuf>) = {
            let away = torrent.lock();
            (away.taken.clone(), away.away.keys().cloned().collect())
        };
        let mut gone = BTreeSet::new();
        torrent.slots.each(|index, path, file| {
            if let Ok(opened) = file.try_read()
                && path.is_some_and(|p| taken.contains(p) && !away.contains(p))
                && unlinked(&opened)
            {
                gone.insert(index);
            }
        });
        gone
    }

    /// The files librqbit's storage of torrent `info_hash` has open that aren't the ones at their
    /// paths any more (replaced, moved or deleted by hand), by index: librqbit's pieces of them are
    /// of the files it has open, not of what's there. One being read or written now counts, as
    /// nothing can tell. Each is looked at on disk (`fstat`, `stat`).
    pub fn replaced(&self, info_hash: &str) -> BTreeSet<usize> {
        let mut replaced = BTreeSet::new();
        self.torrent(info_hash).slots.each(|index, path, file| {
            let moved = match file.try_read() {
                Ok(opened) => matches!(&*opened, Opened::File(file) if moved(path, file)),
                Err(_) => true,
            };
            if moved {
                replaced.insert(index);
            }
        });
        replaced
    }

    /// The files librqbit's storage of torrent `info_hash` has open, by index, wherever they are now;
    /// one being opened or closed now counts. Only what's in memory, so the actor may ask.
    pub fn open(&self, info_hash: &str) -> BTreeSet<usize> {
        let mut open = BTreeSet::new();
        self.torrent(info_hash).slots.each(|index, _, file| {
            if file.try_read().map_or(true, |opened| matches!(&*opened, Opened::File(_))) {
                open.insert(index);
            }
        });
        open
    }

    /// Runs `f` while every file of torrent `info_hash` is in use, as by a read or a write of librqbit's.
    #[cfg(test)]
    pub fn in_use(&self, info_hash: &str, f: impl FnOnce()) {
        self.torrent(info_hash).slots.in_use(f);
    }

    /// Closes the files `files` (by index) of torrent `info_hash`, which [`Trashed::gone`] found gone
    /// from disk, so their space is freed: librqbit can't read them any
    /// more, and the torrent is to be added back without their pieces. Touches no disk: a file with
    /// no name left never gets one back, so the look's answer stands. Waits for nothing: one
    /// librqbit is reading or opening now is left open, for the next look to find again. Which of
    /// them it closed.
    pub fn let_go(&self, info_hash: &str, files: &BTreeSet<usize>) -> BTreeSet<usize> {
        let torrent = self.torrent(info_hash);
        let mut closed = BTreeSet::new();
        torrent.slots.each(|index, _, file| {
            if !files.contains(&index) {
                return;
            }
            // Swapped under the slot's own lock, which every read and write of it holds: the next
            // one finds it not there (and a write makes it again at its path, as for any file set
            // Off that isn't there).
            if let Ok(mut opened) = file.try_write()
                && matches!(*opened, Opened::File(_))
            {
                *opened = Opened::NotThere;
                closed.insert(index);
            }
        });
        closed
    }

    /// Closes the files `files` (by index) of torrent `info_hash` (those missing, or let go of) that
    /// are gone for good (deleted by hand while librqbit has them open: no name left anywhere), so their space is freed at once; one moved elsewhere has its space still, and
    /// stays open, and one a Move to Trash took is [`Trashed::gone`]'s, written ahead. Looks at each
    /// (`fstat`), so never on the actor; waits for nothing, as [`Trashed::let_go`]: one librqbit is
    /// reading or opening now is left for the next look. Which of them it closed. One set Off is let
    /// go of, as one gone from the Trash is, so librqbit offers none of its pieces (the actor writes
    /// that ahead as it's set Off).
    pub fn close_deleted(&self, info_hash: &str, files: &BTreeSet<usize>) -> BTreeSet<usize> {
        let Some(torrent) = self.torrents.lock().unwrap().get(info_hash).cloned() else { return BTreeSet::new() };
        let taken = torrent.lock().taken.clone();
        let mut closed = BTreeSet::new();
        torrent.slots.each(|index, path, file| {
            if !files.contains(&index) || path.is_some_and(|p| taken.contains(p)) {
                return;
            }
            #[cfg(test)]
            if let Some(hook) = &self.hook {
                hook(At::Close);
            }
            if let Ok(mut opened) = file.try_write()
                && unlinked(&opened)
            {
                *opened = Opened::NotThere;
                closed.insert(index);
            }
        });
        closed
    }

    /// librqbit's storage of torrent `info_hash`'s files, laid out as `layout`, in `destination`.
    pub(super) fn storage(&self, info_hash: &str, layout: Layout, destination: PathBuf) -> Files {
        Files {
            layout,
            destination,
            files: Handle::default(),
            torrent: self.torrent(info_hash),
            parts: self.parts_of(info_hash),
            parts_folders: self.parts_folders.clone(),
            #[cfg(test)]
            hook: self.hook.clone(),
            #[cfg(test)]
            reads_of: self.reads_of.clone().map(|r| (r, info_hash.to_owned())),
        }
    }

    /// The torrent's files in the Trash, made once, with what was kept of them before a restart.
    fn torrent(&self, info_hash: &str) -> Arc<Torrent> {
        let mut torrents = self.torrents.lock().unwrap();
        if let Some(torrent) = torrents.get(info_hash) {
            return torrent.clone();
        }
        let torrent = Arc::new(Torrent::default());
        if let Some(dir) = self.parts_of(info_hash) {
            let mut away = torrent.lock();
            for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if let Some(index) = file.file_name().to_str().and_then(|n| n.parse().ok()) {
                    away.kept.insert(index, read_parts(&file.path()));
                }
            }
        }
        torrents.insert(info_hash.to_owned(), torrent.clone());
        torrent
    }

    fn parts_of(&self, info_hash: &str) -> Option<PathBuf> {
        self.parts.as_ref().map(|p| p.join(info_hash))
    }
}

/// How an undo's put-back went, for [`needs_check`]: the files something kept went into, and those
/// librqbit couldn't read where they went (opened there failed after a restart, the Trash out of
/// reach), which it has less of than it had.
#[derive(Debug, Default, PartialEq)]
pub struct PutBack {
    pub written: BTreeSet<usize>,
    pub unread: BTreeSet<usize>,
}

/// What the stack names of a torrent: each file (its index, where it went, whether it's in the Trash
/// for an undo: there still as it went, or an undo under way has it), and whether a move of it runs.
#[derive(Default)]
struct Named {
    files: BTreeMap<PathBuf, (usize, trash::Trashed, bool)>,
    moving: bool,
}

impl Torrent {
    fn lock(&self) -> std::sync::MutexGuard<'_, Away> {
        self.files.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Whether `opened` is a file with no name left anywhere: gone from disk, still open.
fn unlinked(opened: &Opened) -> bool {
    use std::os::unix::fs::MetadataExt;
    match opened {
        Opened::File(file) => file.metadata().is_ok_and(|m| m.nlink() == 0),
        Opened::NotThere | Opened::HandedOn => false,
    }
}

/// Whether the undo of a Move to Trash, which put back the files `back` (by index), must check the
/// torrent: librqbit has more of one now than it had as it moved (`had`), a piece it shares with a
/// file the torrent gets, and what came couldn't go into the file (not in `put.written`); or it
/// couldn't read one where it went (`put.unread`), so a check found less of it. Nothing else changes
/// what librqbit has of a file in the Trash: it's read as it went.
pub fn needs_check(had: &[(usize, u64)], back: &[usize], now: impl Fn(usize) -> u64, put: &PutBack) -> bool {
    back.iter().any(|i| put.unread.contains(i))
        || had.iter().filter(|(i, _)| back.contains(i)).any(|&(i, was)| now(i) > was && !put.written.contains(&i))
}

/// Where a torrent's files go and which it gets.
#[derive(Clone)]
pub struct Layout {
    /// librqbit's output folder: the destination, or the torrent's own folder in it.
    folder: PathBuf,
    /// The files it gets (those Normal); `None`: all of them.
    normal: Option<Arc<BTreeSet<usize>>>,
    /// The name of a torrent of one file's file, renamed (#331): librqbit names it as its info does.
    one_file: Option<PathBuf>,
}

impl Layout {
    pub fn new(folder: PathBuf, normal: Option<&[usize]>) -> Self {
        Self { folder, normal: normal.map(|n| Arc::new(n.iter().copied().collect())), one_file: None }
    }

    /// Its one file under `name` in its folder, whatever librqbit calls it: a torrent of one file
    /// renamed (#331).
    pub fn one_file_as(mut self, name: Option<PathBuf>) -> Self {
        self.one_file = name;
        self
    }

    /// Where librqbit's file at `relative` (in its output folder, as the info names it) is.
    fn at(&self, relative: &Path) -> PathBuf {
        self.folder.join(self.one_file.as_deref().unwrap_or(relative))
    }

    fn is_off(&self, index: usize) -> bool {
        self.normal.as_ref().is_some_and(|normal| !normal.contains(&index))
    }
}

/// librqbit's file storage (`FilesystemStorage`), except for the files set Off: `init` opens one
/// only if it's there, and never makes it, so a check (which adds the torrent again) doesn't bring
/// back, empty, one deleted (#67). One that isn't there is made when librqbit writes into it, as
/// for a piece it shares with a file the torrent gets (a stub, of that file's length: librqbit
/// only sets the length of the files it gets), with the folders to it, but never the destination.
/// Nothing is ever removed but what librqbit asks.
///
/// A file in the Trash for an undo is never changed where it went (#217's fourth review): librqbit
/// keeps it open, and a write or a length set through it would change it there, so its undo
/// wouldn't find it as it went. Its length isn't set; what's written into it is kept aside, and read
/// over the file; the file itself is read where it went (#218's S1), opened there after a restart.
pub(super) struct Files {
    layout: Layout,
    destination: PathBuf,
    /// Its files, as opened; padding (BEP 47) is never opened.
    files: Handle,
    torrent: Arc<Torrent>,
    /// Where what's written into a file in the Trash is kept, by file, outlasting a restart.
    parts: Option<PathBuf>,
    /// [`Trashed::parts_folders`].
    parts_folders: Arc<Mutex<()>>,
    #[cfg(test)]
    hook: Option<Hook>,
    /// A test's hold on its reads, with its torrent's info hash.
    #[cfg(test)]
    reads_of: Option<(ReadsOf, String)>,
}

/// Where an open file stands.
enum Place<'a> {
    /// Open, where it was opened.
    InPlace(&'a std::fs::File),
    /// In the Trash for an undo: read where it went (if it's open), never made again at its path.
    Trashed(Option<&'a std::fs::File>),
    /// Moved or gone some other way, or never there: made again where it's written.
    Elsewhere,
}

impl Files {
    #[cfg(test)]
    fn at(&self, at: At) {
        if let Some(hook) = &self.hook {
            hook(at);
        }
    }

    /// Where the file of `path`, as `opened`, stands now. Looked at on disk only for a file a Move to
    /// Trash took: its undo puts it back, and may take it back to the Trash, without a move.
    fn place<'a>(&self, path: Option<&Path>, opened: &'a Opened) -> Place<'a> {
        let file = match opened {
            Opened::File(file) => Some(file),
            Opened::NotThere | Opened::HandedOn => None,
        };
        let (looked, trashed) = match (path, self.torrent.any.load(Ordering::SeqCst)) {
            (Some(path), true) => {
                let away = self.torrent.lock();
                (away.taken.contains(path), away.away.contains_key(path))
            }
            (_, _) => (false, false),
        };
        match file {
            Some(file) if !looked || !moved(path, file) => Place::InPlace(file),
            _ if trashed => Place::Trashed(file),
            _ => Place::Elsewhere,
        }
    }

    /// Writes `bufs`, one after another, at `offset` into file `index`, made first if it isn't
    /// there; aside if it's in the Trash. Where it is is looked at once. Writing no bytes makes
    /// nothing (#376).
    fn write(&self, index: usize, offset: u64, bufs: &[&[u8]]) -> Result<()> {
        // librqbit writes 0 bytes into the file before a chunk: that makes nothing (#376).
        if bufs.iter().all(|buf| buf.is_empty()) {
            return Ok(());
        }
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        let written = self.files.read(index, |path, opened| {
            match self.place(path, opened) {
                Place::InPlace(file) => write_at(file, offset, bufs)?,
                Place::Trashed(_) => self.keep(path, index, offset, bufs.concat()),
                Place::Elsewhere => return Ok(false),
            }
            Ok(true)
        })?;
        if written {
            return Ok(());
        }
        self.made(index)?;
        self.files.read(index, |_, opened| match opened {
            Opened::File(f) => Ok(write_at(f, offset, bufs)?),
            _ => bail!("file {index} was handed on"),
        })
    }

    /// Keeps `bytes`, written at `offset` into file `index` of `path`, in the Trash.
    fn keep(&self, path: Option<&Path>, index: usize, offset: u64, bytes: Vec<u8>) {
        // On disk too, so a restart keeps it: librqbit's saved pieces say it has it.
        let failed = self.parts.as_ref().and_then(|dir| {
            let _folders = self.parts_folders.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            keep_part(dir, index, offset, &bytes, || {
                #[cfg(test)]
                self.at(At::Keep);
            })
            .err()
        });
        let mut away = self.torrent.lock();
        match failed {
            // Retried at the next change to the undo stack, then reported (#304).
            Some(e) => {
                warn!("keeping what librqbit wrote into a file in the Trash: {e}");
                let file = path.map(Path::to_path_buf).unwrap_or_default();
                away.unsaved.insert((index, offset), Unsaved { file, reason: e.to_string(), retried: false });
            }
            None => drop(away.unsaved.remove(&(index, offset))),
        }
        away.kept.entry(index).or_default().insert(offset, bytes);
    }

    /// Makes file `index` again at its path: one moved by hand, or never there.
    fn made(&self, index: usize) -> Result<()> {
        self.files.change(index, |path, opened| {
            if let Place::InPlace(_) = self.place(path, opened) {
                return Ok(());
            }
            let path = path.context("a padding file is never written")?;
            make_folders(&self.destination, path.parent().context("a file without a folder")?)?;
            *opened = Opened::File(torrent_file(path)?);
            Ok(())
        })
    }

    /// File `index`, set Off, at `path` if it's there, or where it went to the Trash for an undo. One
    /// that can't be opened there is remembered, for its undo to check the torrent.
    fn open_off(&self, index: usize, path: &Path) -> Result<Opened> {
        match open(path, false) {
            Ok(file) => Ok(Opened::File(file)),
            // A link there isn't it (#267): as if nothing were.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound || is_link(&e) => match self.in_trash(path).map(|to| open(&to, false)) {
                Some(Ok(file)) => Ok(Opened::File(file)),
                Some(Err(e)) => {
                    warn!("opening {path:?} where it went to the Trash: {e}");
                    self.torrent.lock().unread.insert(index);
                    Ok(Opened::NotThere)
                }
                None => Ok(Opened::NotThere),
            },
            Err(e) => Err(e).with_context(|| format!("opening {path:?}")),
        }
    }

    /// Where file `index`, set Off and not at `path`, went to the Trash for an undo, if it's there
    /// as it went.
    fn in_trash(&self, path: &Path) -> Option<PathBuf> {
        let away = self.torrent.lock();
        let (_, item) = away.away.get(path)?;
        let to = item.to.clone()?;
        (item.seen.is_some() && identity(&to) == item.seen).then_some(to)
    }
}

/// Whether `file`, opened at `path`, is no longer what's there: moved (to the Trash, say), or
/// gone. A symbolic link at the path isn't followed (#267): it's never the torrent's file, even one
/// to the file it has open. Only looked at for the files a Move to Trash took; for any other file
/// the open handle serves its own file until the size check finds the link, and `init` opens it
/// again.
fn moved(path: Option<&Path>, file: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(path) = path else { return false };
    match (file.metadata(), std::fs::symlink_metadata(path)) {
        (Ok(open), Ok(there)) => (open.dev(), open.ino()) != (there.dev(), there.ino()),
        _ => true,
    }
}

/// Makes `file` `len` bytes long if it's shorter: the one place a torrent's file gets its length, and
/// it's never cut (#374). A longer one isn't the torrent's, and the size check finds it missing. Its
/// length is asked of the open file; there's no call that only lengthens one, so a program making
/// it longer than `len` between that and the next line has what it just added cut off.
pub(super) fn grow(file: &std::fs::File, len: u64) -> std::io::Result<()> {
    if file.metadata()?.len() < len {
        file.set_len(len)?;
    }
    Ok(())
}

fn write_at(file: &std::fs::File, mut offset: u64, bufs: &[&[u8]]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    for buf in bufs {
        file.write_all_at(buf, offset)?;
        offset += buf.len() as u64;
    }
    Ok(())
}

/// What's kept in `dir`, by offset.
/// Makes folder `path`, its parent there: one that's there already is as good.
fn make_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::create_dir(path) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        done => done,
    }
}

/// Keeps `bytes` at `offset` of part `index` in `dir` (a torrent's folder of parts), making its
/// folders one at a time: the gap between them, `between`, is where #302's sweep got in. The caller
/// holds [`Trashed::parts_folders`].
fn keep_part(dir: &Path, index: usize, offset: u64, bytes: &[u8], between: impl FnOnce()) -> std::io::Result<()> {
    if let Some(root) = dir.parent() {
        std::fs::create_dir_all(root)?;
    }
    make_dir(dir)?;
    between();
    let part = dir.join(index.to_string());
    make_dir(&part)?;
    std::fs::write(part.join(offset.to_string()), bytes)
}

fn read_parts(dir: &Path) -> BTreeMap<u64, Vec<u8>> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    entries.filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, std::fs::read(e.path()).ok()?))).collect()
}

/// `kept` into the file at `path`, there already.
fn write_parts(path: &Path, kept: &BTreeMap<u64, Vec<u8>>) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    let file = open(path, false)?;
    for (offset, bytes) in kept {
        file.write_all_at(bytes, *offset)?;
    }
    Ok(())
}

/// `buf`'s bytes from `offset` in `file`, up to its end: how many there were.
fn read_up_to(file: &std::fs::File, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    let mut read = 0;
    while read < buf.len() {
        match file.read_at(&mut buf[read..], offset + read as u64) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(read)
}

/// What `kept` holds of the bytes from `offset` goes into `buf`, over what's there, the first
/// `read` of them read already: whether `buf` is whole.
fn read_kept(kept: Option<&BTreeMap<u64, Vec<u8>>>, offset: u64, buf: &mut [u8], read: usize) -> bool {
    let end = offset + buf.len() as u64;
    let mut covered: Vec<(u64, u64)> = vec![(offset, offset + read as u64)];
    for (start, bytes) in kept.into_iter().flatten() {
        let (from, to) = ((*start).max(offset), (*start + bytes.len() as u64).min(end));
        if from >= to {
            continue;
        }
        buf[(from - offset) as usize..(to - offset) as usize].copy_from_slice(&bytes[(from - start) as usize..(to - start) as usize]);
        covered.push((from, to));
    }
    covered.sort();
    let mut at = offset;
    for (from, to) in covered {
        if from > at {
            return false;
        }
        at = at.max(to);
    }
    at >= end
}

/// The file at `path`, for reading and writing, `create`d if it isn't there (but not the folders
/// to it), never truncated. Never through a symbolic link at `path` (#267): one there, dangling or
/// not, isn't the torrent's file, and opening it fails ([`is_link`]); a link to a folder on the way
/// is the person's choice of where the files go, and is followed.
fn open(path: &Path, create: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).create(create).truncate(false).custom_flags(libc::O_NOFOLLOW).open(path)
}

/// The file a torrent gets at `path`, made if it isn't there: none is made through a symbolic link
/// there, which fails it, and the size check finds the file missing.
fn torrent_file(path: &Path) -> Result<std::fs::File> {
    match open(path, true) {
        Ok(file) => Ok(file),
        Err(e) if is_link(&e) => bail!("{path:?} is a symbolic link, not the torrent's file"),
        Err(e) => Err(e).with_context(|| format!("opening {path:?}")),
    }
}

/// Whether [`open`] failed for a symbolic link at the path.
fn is_link(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ELOOP)
}

/// Whether `name` takes the name of a folder of Phantom's own in a destination: a move's staging
/// (`.phantom-moving-<hash>`). No torrent's file or folder may. In any case: APFS folds it, so
/// `.PHANTOM-Moving-…` (or `.phantom-movinſ-…`, were there an `s`) is the same folder.
pub(super) fn phantoms(name: &std::ffi::OsStr) -> bool {
    // Upper, then lower: folds what lowering alone keeps apart, as `ſ` from `s`.
    let name: String = name.to_string_lossy().chars().flat_map(char::to_uppercase).flat_map(char::to_lowercase).collect();
    name.starts_with(".phantom-moving-")
}

/// Makes the folders from `destination` down to `dir`, one at a time, so a destination that's
/// gone is never made again.
/// Makes the file at `path`, in `destination`, again as librqbit makes a file a torrent gets: of
/// `len` bytes, sparse, with the folders to it (never `destination` itself). Never over anything
/// there, a symbolic link too: `create_new` never follows one.
pub(super) fn make_again(destination: &Path, path: &Path, len: u64) -> Result<()> {
    make_folders(destination, path.parent().context("a file without a folder")?)?;
    let file = std::fs::File::options().write(true).create_new(true).open(path).with_context(|| format!("making {path:?}"))?;
    grow(&file, len).with_context(|| format!("setting the length of {path:?}"))?;
    Ok(())
}

fn make_folders(destination: &Path, dir: &Path) -> Result<()> {
    let inside = dir.strip_prefix(destination).with_context(|| format!("{dir:?} isn't in {destination:?}"))?;
    let mut at = destination.to_owned();
    for part in inside.components() {
        at.push(part);
        match std::fs::create_dir(&at) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e).with_context(|| format!("making {at:?}")),
            _ => {}
        }
    }
    Ok(())
}

impl TorrentStorage for Files {
    fn init(&mut self, _: &ManagedTorrentShared, metadata: &TorrentMetadata) -> Result<()> {
        // A backstop to the add's check: never a file in a folder of Phantom's own, every path
        // looked at before any file is made.
        for info in &metadata.file_infos {
            let path = self.layout.at(&info.relative_filename);
            if let Ok(inside) = path.strip_prefix(&self.destination)
                && inside.components().any(|c| phantoms(c.as_os_str()))
            {
                bail!("{inside:?} takes the name of a folder of Phantom's own");
            }
        }
        let mut files = Vec::with_capacity(metadata.file_infos.len());
        for (index, info) in metadata.file_infos.iter().enumerate() {
            if info.attrs.padding {
                files.push((None, Opened::NotThere));
                continue;
            }
            let path = self.layout.at(&info.relative_filename);
            let file = if self.layout.is_off(index) {
                self.open_off(index, &path)?
            } else {
                std::fs::create_dir_all(path.parent().context("a file without a folder")?)?;
                match open(&path, true) {
                    Ok(file) => Opened::File(file),
                    // Not the torrent's file (#267): none of it is read, nor written, and the size
                    // check finds it missing.
                    Err(e) if is_link(&e) => Opened::NotThere,
                    Err(e) => return Err(e).with_context(|| format!("opening {path:?}")),
                }
            };
            files.push((Some(path), file));
        }
        self.files = self.torrent.slots.open(files);
        Ok(())
    }

    fn pread_exact(&self, file: usize, offset: u64, buf: &mut [u8]) -> Result<()> {
        use std::os::unix::fs::FileExt;
        #[cfg(test)]
        self.at(At::Read);
        #[cfg(test)]
        if let Some((reads_of, hash)) = &self.reads_of {
            reads_of(hash);
        }
        self.files.read(file, |path, opened| {
            #[cfg(test)]
            self.at(At::Reading);
            // Nothing to read: librqbit asks so of the file a piece starts right after, whether it's
            // there or not (one let go of, say), and a failed read would drop the peer that asked.
            if buf.is_empty() {
                return Ok(());
            }
            match (self.place(path, opened), opened) {
                (Place::InPlace(f), _) => Ok(f.read_exact_at(buf, offset)?),
                // In the Trash: as it went, with what was written meanwhile over it, which may reach past
                // its end (a stub).
                (Place::Trashed(Some(f)), _) => {
                    let read = read_up_to(f, offset, buf)?;
                    match read_kept(self.torrent.lock().kept.get(&file), offset, buf, read) {
                        true => Ok(()),
                        false => bail!("file {file} in the Trash ends before {}", offset + buf.len() as u64),
                    }
                }
                (Place::Trashed(None), _) => match read_kept(self.torrent.lock().kept.get(&file), offset, buf, 0) {
                    true => Ok(()),
                    false => bail!("file {file} is in the Trash, and couldn't be opened there"),
                },
                // Moved, and no undo will put it back (#223's review, S1): read where it went all the
                // same, as a read changes nothing, so a peer that asks for one of its pieces isn't
                // dropped. A write makes it again at its path.
                (Place::Elsewhere, Opened::File(f)) => Ok(f.read_exact_at(buf, offset)?),
                (Place::Elsewhere, Opened::NotThere) => bail!("file {file} isn't on disk"),
                (Place::Elsewhere, Opened::HandedOn) => bail!("file {file} was handed on"),
            }
        })
    }

    fn pwrite_all(&self, file: usize, offset: u64, buf: &[u8]) -> Result<()> {
        self.write(file, offset, &[buf])
    }

    fn pwrite_all_vectored(&self, file: usize, offset: u64, bufs: [std::io::IoSlice<'_>; 2]) -> Result<usize> {
        let [a, b] = bufs;
        self.write(file, offset, &[&a, &b])?;
        Ok(a.len() + b.len())
    }

    fn remove_file(&self, _: usize, path: &Path) -> Result<()> {
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        Ok(std::fs::remove_file(self.layout.at(path))?)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> Result<()> {
        let path = self.layout.folder.join(path);
        if !path.is_dir() {
            bail!("cannot remove dir: {path:?} is not a directory")
        }
        if std::fs::read_dir(&path)?.next().is_none() {
            std::fs::remove_dir(&path).with_context(|| format!("error removing {path:?}"))?;
        }
        Ok(())
    }

    fn ensure_file_length(&self, file: usize, len: u64) -> Result<()> {
        // librqbit asks for the files it gets only. A file set Off but held, until a check applies it,
        // is one it still gets: neither made nor resized.
        if self.layout.is_off(file) {
            return Ok(());
        }
        let _changing = self.torrent.changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        self.at(At::Change);
        // Nor one moved from its path: librqbit sets the length as its check ends, even a check
        // paused, and one in the Trash stays as it went.
        let done = self.files.read(file, |path, opened| match (self.place(path, opened), opened) {
            (Place::InPlace(f), _) => {
                #[cfg(test)]
                self.at(At::Length);
                grow(f, len)?;
                Ok(true)
            }
            (Place::Trashed(_), _) | (Place::Elsewhere, Opened::File(_)) => Ok(true),
            (Place::Elsewhere, Opened::NotThere | Opened::HandedOn) => Ok(false),
        })?;
        if done {
            return Ok(());
        }
        self.made(file)?;
        self.files.read(file, |_, opened| match opened {
            Opened::File(f) => Ok(grow(f, len)?),
            _ => bail!("file {file} was handed on"),
        })
    }

    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Files {
            layout: self.layout.clone(),
            destination: self.destination.clone(),
            files: self.files.hand_on(),
            torrent: self.torrent.clone(),
            parts: self.parts.clone(),
            parts_folders: self.parts_folders.clone(),
            #[cfg(test)]
            hook: self.hook.clone(),
            #[cfg(test)]
            reads_of: self.reads_of.clone(),
        }))
    }
}

/// How many of this process's open files are the file at `path` now: for tests, which look for a
/// handle still open on it once it's gone from disk.
#[cfg(test)]
pub(super) fn open_on(path: &Path) -> impl Fn() -> usize + use<> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).expect("there to look at");
    let (dev, ino) = (meta.dev(), meta.ino());
    move || {
        // SAFETY: `fstat` only writes into `stat`, and fails on a number that isn't open.
        let max = unsafe { libc::getdtablesize() };
        (0..max)
            .filter(|&fd| unsafe {
                let mut stat: libc::stat = std::mem::zeroed();
                libc::fstat(fd, &mut stat) == 0 && stat.st_dev as u64 == dev as u64 && stat.st_ino == ino
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use proto::KeptWhy;

    use super::*;

    /// A trash into a folder of its own, as the engine's tests have.
    fn bin(dir: &Path) -> Trash {
        trash::into(dir.join("trash"), Default::default(), Default::default())
    }

    /// Storage of one file at `path`, opened, of torrent "t".
    fn storage(trashed: &Trashed, dir: &Path, path: &Path) -> Files {
        let mut files = trashed.storage("t", Layout::new(dir.to_owned(), None), dir.to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.to_owned()), Opened::File(open(path, false).unwrap()))]);
        files
    }

    /// A Move to Trash of the file at `path`, of torrent "t", as file 0: where it went.
    fn take(trashed: &Trashed, trash: &Trash, dir: &Path, path: &Path) -> trash::Trashed {
        let plan = Plan { location: dir.to_owned(), items: vec![path.to_owned()] };
        let carried = trashed.take("t", &plan, &[(0, path.to_owned())], trash, |_| {}, &AtomicBool::new(false));
        carried.moved.into_iter().next().expect("moved")
    }

    /// #376: storage of one file at `path`, which isn't there, set Off.
    fn absent(trashed: &Trashed, dir: &Path, path: &Path) -> Files {
        let mut files = trashed.storage("t", Layout::new(dir.to_owned(), None), dir.to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.to_owned()), Opened::NotThere)]);
        files
    }

    /// #376: librqbit writes 0 bytes into the file before a chunk. Not a byte, not a file, nor its
    /// folders: whether it's one write or two empty slices, as vectored.
    #[test]
    fn a_write_of_nothing_makes_no_file_nor_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder/deeper/off.bin");
        let files = absent(&Trashed::default(), dir.path(), &path);
        files.pwrite_all(0, 0, &[]).unwrap();
        files.pwrite_all(0, 7, &[]).unwrap();
        assert_eq!(files.pwrite_all_vectored(0, 0, [std::io::IoSlice::new(&[]), std::io::IoSlice::new(&[])]).unwrap(), 0);
        assert!(!dir.path().join("folder").exists(), "no folder");
    }

    /// An empty slice beside one with bytes still makes the file, and writes the bytes.
    #[test]
    fn a_vectored_write_of_one_empty_slice_and_one_not_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folder/off.bin");
        let files = absent(&Trashed::default(), dir.path(), &path);
        let n = files.pwrite_all_vectored(0, 2, [std::io::IoSlice::new(&[]), std::io::IoSlice::new(b"ab")]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"\0\0ab");
        let n = files.pwrite_all_vectored(0, 0, [std::io::IoSlice::new(b"x"), std::io::IoSlice::new(&[])]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"x\0ab");
    }

    /// A write of nothing to a file in the Trash keeps nothing aside.
    #[test]
    fn a_write_of_nothing_to_a_file_in_the_trash_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        let trashed = Trashed::default();
        let bin = bin(dir.path());
        let files = storage(&trashed, dir.path(), &path);
        take(&trashed, &bin, dir.path(), &path);
        files.pwrite_all(0, 3, &[]).unwrap();
        assert!(trashed.torrent("t").lock().kept.is_empty(), "nothing kept");
        assert!(!path.exists(), "nothing made at its path");
    }

    /// #217's fourth and fifth reviews, #218's S1: a file a Move to Trash took while open is left as
    /// it went. librqbit's length set skips it, a write (a piece it shares with a file the torrent
    /// gets) makes nothing at its path and is kept aside, and a read reads the file where it went,
    /// with what was written over it. Once it's back, it's the torrent's file again, with what was
    /// written in it.
    #[test]
    fn a_file_a_move_to_trash_took_is_read_only_where_it_went() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        let item = take(&trashed, &trash, dir.path(), &path);
        let away = item.to.clone().unwrap();
        let went = std::fs::metadata(&away).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        files.ensure_file_length(0, 150).unwrap();
        assert!(!path.exists(), "its length isn't set at its path either");
        let mut buf = [0; 10];
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [1; 10], "read where it went");
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        files.pwrite_all_vectored(0, 10, [std::io::IoSlice::new(&[3; 5]), std::io::IoSlice::new(&[4; 5])]).unwrap();
        assert!(!path.exists(), "a write makes nothing at its path");
        let mut buf = [0; 25];
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf[..20], [[2; 10].as_slice(), &[3; 5], &[4; 5]].concat(), "what was written is read over it");
        assert_eq!(buf[20..], [1; 5], "and the file as it went past that");
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "the one moved stays as it went");
        assert_eq!(std::fs::metadata(&away).unwrap().modified().unwrap(), went, "modified time too");
        // Put back, it's the torrent's again, with what was written meanwhile in it.
        std::fs::rename(&away, &path).unwrap();
        assert_eq!(trashed.put_back("t", &[(0, path.clone())]).written, BTreeSet::from([0]), "something went into it");
        assert_eq!(std::fs::read(&path).unwrap()[..20], buf[..20]);
        files.ensure_file_length(0, 150).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 150);
    }

    /// #302: a part kept on disk makes its torrent's folder and then the file's, and a sweep of the
    /// empty folders (`follow`) in between took the first away, so the file's was never made and the
    /// part never reached the disk. The test runs the sweep in that gap: the torrent's folder must be
    /// there still once the sweep has had its time: it waits for the write, or it doesn't get through.
    /// (The sweep that waits runs once the write is over, and, nothing naming the file in the Trash,
    /// takes the part with the rest.)
    #[test]
    fn a_sweep_of_empty_folders_between_the_two_folders_of_a_part_leaves_it_on_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path().join("parts");
        let trashed = Trashed::default().keeping_parts_in(parts.clone());
        let folder_in_the_gap = Arc::new(Mutex::new(None));
        let sweeping = Arc::new(Mutex::new(None));
        let hook: Hook = {
            let (trashed, parts, folder_in_the_gap, sweeping) =
                (trashed.clone(), parts.clone(), folder_in_the_gap.clone(), sweeping.clone());
            Arc::new(move |at| {
                if at != At::Keep {
                    return;
                }
                let (done, finished) = std::sync::mpsc::channel();
                let trashed = trashed.clone();
                *sweeping.lock().unwrap() = Some(std::thread::spawn(move || {
                    trashed.follow(&machine::State::default());
                    let _ = done.send(());
                }));
                // A sweep that's let through is over in no time; one that waits is still waiting.
                let _ = finished.recv_timeout(std::time::Duration::from_millis(300));
                *folder_in_the_gap.lock().unwrap() = Some(parts.join("t").is_dir());
            })
        };
        let trashed = trashed.with_hook(hook);
        let trash = bin(dir.path());
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        take(&trashed, &trash, dir.path(), &path);
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(*folder_in_the_gap.lock().unwrap(), Some(true), "the sweep took the torrent's folder between the two");
        sweeping.lock().unwrap().take().unwrap().join().unwrap();
    }

    /// A Trashed whose parts folder can't be written into (`undo-parts` can't be made), with a file
    /// at the path returned in the Trash, and the folder's own permissions to give back.
    fn unsaveable(dir: &Path) -> (Trashed, Files, PathBuf, std::fs::Permissions) {
        use std::os::unix::fs::PermissionsExt;
        let data = dir.join("data");
        std::fs::create_dir(&data).unwrap();
        // Not empty: a Move to Trash sweeps the empty folders.
        std::fs::write(data.join("settings.json"), "{}").unwrap();
        let mode = std::fs::metadata(&data).unwrap().permissions();
        let trashed = Trashed::default().keeping_parts_in(data.join("undo-parts"));
        let path = dir.join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir, &path);
        take(&trashed, &bin(dir), dir, &path);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
        (trashed, files, path, mode)
    }

    /// #304: a part that can't be kept on disk is kept in memory still and retried at the next
    /// change to the undo stack; if that fails too it's reported, with its file and why; a later
    /// retry that keeps it takes the report away.
    #[test]
    fn a_part_that_cannot_be_kept_is_retried_at_the_next_stack_change_then_reported_until_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, files, path, mode) = unsaveable(dir.path());
        let data = dir.path().join("data");
        files.pwrite_all(0, 4, &[2; 10]).unwrap();
        let mut buf = [0; 10];
        files.pread_exact(0, 4, &mut buf).unwrap();
        assert_eq!(buf, [2; 10], "kept in memory meanwhile");
        assert_eq!(trashed.parts_not_saved(), vec![], "told only once a retry has failed");
        trashed.retry_parts();
        let failed = trashed.parts_not_saved();
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert_eq!(failed[0].file, path);
        assert!(failed[0].reason.contains("Permission denied"), "{}", failed[0].reason);
        // Still failing: still told, once.
        trashed.retry_parts();
        assert_eq!(trashed.parts_not_saved().len(), 1);
        // The disk lets it in.
        std::fs::set_permissions(&data, mode).unwrap();
        trashed.retry_parts();
        assert_eq!(trashed.parts_not_saved(), vec![], "kept: nothing to tell");
        assert_eq!(std::fs::read(data.join("undo-parts/t/0/4")).unwrap(), [2; 10]);
    }

    /// #304: what's no longer in the Trash for an undo takes its failure with it: nothing to keep.
    #[test]
    fn a_part_of_a_file_no_longer_in_the_trash_is_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, files, _, _) = unsaveable(dir.path());
        files.pwrite_all(0, 4, &[2; 10]).unwrap();
        trashed.retry_parts();
        assert_eq!(trashed.parts_not_saved().len(), 1);
        trashed.follow(&machine::State::default());
        assert_eq!(trashed.parts_not_saved(), vec![]);
    }

    /// #304: a part kept by the retry before anything told of it is never told.
    #[test]
    fn a_part_kept_at_its_first_retry_is_never_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, files, _, mode) = unsaveable(dir.path());
        let data = dir.path().join("data");
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(trashed.parts_not_saved(), vec![]);
        std::fs::set_permissions(&data, mode).unwrap();
        trashed.retry_parts();
        assert_eq!(trashed.parts_not_saved(), vec![]);
        assert_eq!(std::fs::read(data.join("undo-parts/t/0/0")).unwrap(), [2; 10]);
    }

    /// #234: a file a Move to Trash took, gone from the Trash while librqbit has it open, is found and
    /// closed, so nothing has it open and its space is freed, once nothing reads it. One still in the
    /// Trash isn't, nor one no move took.
    #[test]
    fn a_file_gone_from_the_trash_is_found_and_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        let open = open_on(&away);
        assert_eq!(trashed.gone("t"), BTreeSet::new(), "still in the Trash");
        assert_eq!(open(), 1);
        std::fs::remove_file(&away).unwrap();
        assert_eq!(trashed.gone("t"), BTreeSet::new(), "the Trash has it for its undo, as far as it was last found");
        // Its undo no longer finds it there.
        trashed.follow(&machine::State::default());
        assert_eq!(trashed.gone("t"), BTreeSet::from([0]));
        assert_eq!(open(), 1, "found, not closed yet");
        // A read under way (stalled, say) holds it: it's left open, and nothing waits for it.
        trashed.in_use("t", || {
            assert_eq!(trashed.let_go("t", &BTreeSet::from([0])), BTreeSet::new());
            assert_eq!(open(), 1, "left open while it's read");
        });
        assert_eq!(trashed.gone("t"), BTreeSet::from([0]), "found again");
        assert_eq!(trashed.let_go("t", &BTreeSet::from([0])), BTreeSet::from([0]));
        assert_eq!(open(), 0, "closed: its space is freed");
        assert_eq!(trashed.gone("t"), BTreeSet::new(), "found once");
        assert!(files.pread_exact(0, 0, &mut [0; 10]).is_err(), "nothing to read any more");
        // One no move took isn't looked for, gone or not.
        let other = Trashed::default();
        let path = dir.path().join("b.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let _files = storage(&other, dir.path(), &path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(other.gone("t"), BTreeSet::new());
    }

    /// Found by the model (#242): a file written anew at its path by hand (librqbit's handle is to
    /// the one it replaced, which has no name left), then moved to the Trash whole, isn't one gone
    /// from it: the Trash has it for its undo, and it's read there.
    #[test]
    fn a_file_replaced_by_hand_then_moved_to_the_trash_isnt_taken_for_one_gone_from_it() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let _files = storage(&trashed, dir.path(), &path);
        let replaced = dir.path().join("a.bin.new");
        std::fs::write(&replaced, [1; 100]).unwrap();
        std::fs::rename(&replaced, &path).unwrap();
        take(&trashed, &trash, dir.path(), &path);
        assert_eq!(trashed.gone("t"), BTreeSet::new());
        trashed.follow(&machine::State::default());
        assert_eq!(trashed.gone("t"), BTreeSet::from([0]), "once its undo no longer has it, it's let go of");
    }

    /// An undo that fails once a file is back takes it to the Trash again, with no move: until its
    /// undo is over, such a file is looked at every time, and so is never written where it went.
    #[test]
    fn a_file_taken_back_to_the_trash_by_a_failed_undo_is_left_as_it_went() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        // Back for a while (the undo), and read there.
        std::fs::rename(&away, &path).unwrap();
        files.pread_exact(0, 0, &mut [0; 10]).unwrap();
        // Taken back to the Trash (the undo failed).
        std::fs::rename(&path, &away).unwrap();
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "nothing written where it went");
        assert!(!path.exists());
    }

    /// A file no undo will put back any more (its entry refused, or pushed off the stack) is the
    /// torrent's no more where it went: a piece it shares with a file the torrent gets makes it again
    /// at its path, as for any file set Off that isn't there, and the one in the Trash stays as it
    /// went.
    #[test]
    fn a_file_no_undo_will_put_back_is_made_again_at_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        trashed.follow(&machine::State::default());
        // Read where it went all the same (#223's review): a peer asking for its pieces isn't dropped.
        let mut buf = [0; 10];
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [1; 10]);
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap()[..10], [2; 10], "made again at its path");
        assert_eq!(std::fs::read(&away).unwrap(), [1; 100], "the one in the Trash stays as it went");
    }

    /// A file in the Trash shorter than what was kept of it (a stub a shared piece made, then
    /// trashed) reads what was kept past its end; past both, it fails.
    #[test]
    fn a_short_file_in_the_trash_reads_what_was_kept_past_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 10]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        take(&trashed, &trash, dir.path(), &path);
        files.pwrite_all(0, 8, &[2; 8]).unwrap();
        let mut buf = [0; 16];
        files.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(buf, [[1u8; 8].as_slice(), &[2; 8]].concat().as_slice());
        assert!(files.pread_exact(0, 0, &mut [0; 17]).is_err(), "past both");
    }

    /// librqbit's `take` as it sets a file's length (a check's end), or writes: it waits for that one.
    /// The old storage then changes and makes nothing, a file not there too, and the new one goes on
    /// with what was open, a file replaced by hand at its path meanwhile left as it is.
    #[test]
    fn a_storage_taken_ends_its_length_first_and_then_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (path, absent) = (dir.path().join("a.bin"), dir.path().join("folder/b.bin"));
        std::fs::write(&path, [1; 10]).unwrap();
        let gate = Arc::new(super::super::files::Gate::default());
        let trashed = Trashed::default().with_hook(Arc::new({
            let gate = gate.clone();
            move |at| (at == At::Length).then(|| gate.wait()).unwrap_or_default()
        }));
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), None), dir.path().to_owned());
        files.files = trashed
            .torrent("t")
            .slots
            .open(vec![(Some(path.clone()), Opened::File(open(&path, false).unwrap())), (Some(absent.clone()), Opened::NotThere)]);
        gate.hold(true);
        let (tx, rx) = std::sync::mpsc::channel();
        let taken = std::thread::scope(|s| {
            s.spawn(|| files.ensure_file_length(0, 20).unwrap());
            while !gate.waiting() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            s.spawn(|| tx.send(files.take().unwrap()).unwrap());
            let early = rx.recv_timeout(std::time::Duration::from_millis(100));
            let length = std::fs::metadata(&path).unwrap().len();
            gate.hold(false);
            assert!(early.is_err() && length == 10, "taken with a length under way");
            rx.recv().unwrap()
        });
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 20, "set as it would have been");

        std::fs::rename(&path, dir.path().join("moved.bin")).unwrap();
        std::fs::write(&path, [5; 20]).unwrap();
        for index in [0, 1] {
            assert!(files.pwrite_all(index, 0, &[2; 10]).is_err(), "written through the old storage");
            assert!(files.ensure_file_length(index, 30).is_err(), "sized through the old storage");
            assert!(files.pread_exact(index, 0, &mut [0; 10]).is_err(), "read through the old storage");
        }
        assert!(!dir.path().join("folder").exists(), "nothing made by the old storage");
        taken.pwrite_all(0, 0, &[3; 10]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [5; 20], "the one put at its path, as it was");
        assert_eq!(std::fs::read(dir.path().join("moved.bin")).unwrap()[..10], [3; 10], "the one open");
        taken.pwrite_all(1, 0, &[4; 10]).unwrap();
        drop(files);
        taken.pread_exact(1, 0, &mut [0; 10]).unwrap();
    }

    /// A write into a file in the Trash through a storage already taken, its peer late at a pause: kept
    /// aside as any other, even once the torrent was taken out.
    #[test]
    fn a_late_write_into_a_file_in_the_trash_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let files = storage(&trashed, dir.path(), &path);
        take(&trashed, &trash, dir.path(), &path);
        let taken = files.take().unwrap();
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        drop(taken);
        files.pwrite_all(0, 10, &[3; 10]).unwrap();
        files.pread_exact(0, 5, &mut [0; 10]).unwrap();
        assert!(files.pread_exact(0, 0, &mut [0; 30]).is_err(), "the file itself is no longer its own");
        assert!(!path.exists());
        assert_eq!(trashed.put_back("t", &[(0, path)]).written, BTreeSet::new(), "not back yet");
    }

    /// A file in the Trash that can't be opened where it went after a restart (the Trash out of
    /// reach) is remembered: its undo checks the torrent, which has less of it than is on disk.
    #[test]
    fn a_file_that_cannot_be_opened_in_the_trash_makes_its_undo_check() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (trashed, trash) = (Trashed::default(), bin(dir.path()));
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 10]).unwrap();
        let away = take(&trashed, &trash, dir.path(), &path).to.unwrap();
        let mode = std::fs::metadata(&away).unwrap().permissions();
        std::fs::set_permissions(&away, std::fs::Permissions::from_mode(0o000)).unwrap();
        let after_a_restart = trashed.storage("t", Layout::new(dir.path().to_owned(), Some(&[])), dir.path().to_owned());
        assert!(matches!(after_a_restart.open_off(0, &path), Ok(Opened::NotThere)));
        std::fs::set_permissions(&away, mode).unwrap();
        std::fs::rename(&away, &path).unwrap();
        let put = trashed.put_back("t", &[(0, path.clone())]);
        assert_eq!(put.unread, BTreeSet::from([0]));
        assert!(needs_check(&[], &[0], |_| 0, &put));
    }

    /// A symbolic link at a torrent file's path is never the file (#267): opening it fails, to make
    /// it as well, so nothing is made where a dangling one points; and it isn't where the file was
    /// opened, even a link to that file, so a write fails rather than go through it.
    #[test]
    fn a_link_at_a_files_path_is_never_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (path, real, nowhere) = (dir.path().join("a.bin"), dir.path().join("real.bin"), dir.path().join("nowhere.bin"));
        std::fs::write(&real, [1; 100]).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        let dangling = dir.path().join("b.bin");
        std::os::unix::fs::symlink(&nowhere, &dangling).unwrap();
        for create in [false, true] {
            assert!(open(&path, create).is_err_and(|e| is_link(&e)), "create {create}: a link to a file");
            assert!(open(&dangling, create).is_err_and(|e| is_link(&e)), "create {create}: a dangling link");
        }
        assert!(std::fs::symlink_metadata(&nowhere).is_err(), "nothing made where the dangling one points");

        // librqbit's storage, the file it has open being the one the link points to.
        let trashed = Trashed::default();
        let mut files = trashed.storage("t", Layout::new(dir.path().to_owned(), None), dir.path().to_owned());
        files.files = trashed.torrent("t").slots.open(vec![(Some(path.clone()), Opened::File(open(&real, false).unwrap()))]);
        assert_eq!(trashed.replaced("t"), BTreeSet::from([0]), "not the file at its path");
        // Looked at on disk, as one a move took.
        trashed.torrent("t").lock().taken.insert(path.clone());
        trashed.torrent("t").any.store(true, Ordering::SeqCst);
        assert!(files.pwrite_all(0, 0, &[2; 10]).is_err(), "not written through the link");
        assert_eq!(std::fs::read(&real).unwrap(), [1; 100], "the file it points to, as it was");
    }

    /// S2 of #217's fifth review: a panic while a Move to Trash holds the torrent's files still (the
    /// Trash's own code can panic) leaves the torrent's storage working: the lock guards no data.
    #[test]
    fn a_panic_during_a_move_leaves_the_storage_working() {
        struct Panics;
        impl trash::Bin for Panics {
            fn take(&self, _: &Path) -> Result<Option<PathBuf>, KeptWhy> {
                panic!("the Trash failed")
            }
            fn give_back(&self, _: &Path, _: &Path) -> Result<(), String> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [1; 100]).unwrap();
        let trashed = Trashed::default();
        let files = storage(&trashed, dir.path(), &path);
        let panics: Trash = Arc::new(Panics);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| take(&trashed, &panics, dir.path(), &path)));
        assert!(panicked.is_err());
        // The storage, and the next move, go on.
        files.pwrite_all(0, 0, &[2; 10]).unwrap();
        files.ensure_file_length(0, 100).unwrap();
        assert_eq!(std::fs::read(&path).unwrap()[..10], [2; 10]);
        take(&trashed, &bin(dir.path()), dir.path(), &path);
        assert!(!path.exists());
    }

    /// What's read of a file in the Trash with what was written into it meanwhile: the written bytes
    /// over the file's, whatever ranges they cover.
    #[test]
    fn what_was_kept_is_read_over_the_file() {
        let kept = BTreeMap::from([(2u64, vec![7u8; 3]), (8, vec![9; 4])]);
        let mut buf = [0u8; 10];
        assert!(!read_kept(Some(&kept), 0, &mut buf, 0), "not all of it");
        assert_eq!(buf, [0, 0, 7, 7, 7, 0, 0, 0, 9, 9]);
        let mut buf = [1u8; 3];
        assert!(read_kept(Some(&kept), 9, &mut buf, 0));
        assert_eq!(buf, [9, 9, 9]);
        assert!(!read_kept(None, 0, &mut [0; 1], 0));
        let mut buf = [1u8; 12];
        assert!(read_kept(Some(&kept), 0, &mut buf, 8), "read up to 8, the rest kept");
        assert_eq!(buf[8..], [9; 4]);
    }

    /// The checks an undo of a Move to Trash makes: only a file back that librqbit has more of than
    /// as it went, with nothing written into it.
    #[test]
    fn an_undo_checks_only_a_file_back_with_more_than_could_go_into_it() {
        let had = [(0, 100), (1, 100)];
        let now = |i: usize| [100, 150][i];
        let none = PutBack::default();
        assert!(needs_check(&had, &[0, 1], now, &none), "more of file 1, nothing written");
        assert!(
            !needs_check(&had, &[0, 1], now, &PutBack { written: BTreeSet::from([1]), ..Default::default() }),
            "what came went into it"
        );
        assert!(!needs_check(&had, &[0], now, &none), "file 1 isn't back");
        assert!(!needs_check(&had, &[0, 1], |_| 50, &none), "less: read where it went, it's a check's own");
        assert!(!needs_check(&[], &[0, 1], now, &none), "nothing known as it went");
        let unread = PutBack { unread: BTreeSet::from([0]), ..Default::default() };
        assert!(needs_check(&[], &[0, 1], |_| 0, &unread), "file 0 couldn't be read where it went");
        assert!(!needs_check(&[], &[1], |_| 0, &unread), "but it isn't back");
    }
}
