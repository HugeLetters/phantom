//! Trashing files (#151): the files chosen are set Off first, then moved to the Trash, in that order,
//! so the size check never sees a file the torrent gets go missing. The move runs on a blocking task
//! (the same trashing a Delete does: [`trash::carry_out`], never through a symbolic link) and the
//! client's answer waits for it; the torrent's other commands, and Undo, wait too (parked, run in
//! order once it ends), so nothing meets half of it. What moved goes on the undo stack as one entry
//! that puts the files back and each file's priority as it was ([`Back::Files`]).
//!
//! The entry is written once the move has ended, not ahead as a Delete's items are: a crash in
//! between leaves the files in the Trash and Off, never half-restored, and only the Undo is lost.

use std::collections::BTreeSet;
use std::path::PathBuf;

use proto::{KeptWhy, Priority, TorrentId, UndoAction};
use tokio::sync::oneshot;

use super::super::undo::{Back, Entry};
use super::super::{Failure, trash};
use super::undoing::name_of;
use super::{Actor, Event, Kept, Message, Refusal, ToTrash, to_trash};

/// A file trash under way for one torrent: the client waiting, and the priorities as they were.
pub(super) struct Pending {
    reply: oneshot::Sender<Result<(), Failure>>,
    previous: Vec<(usize, Priority, PathBuf)>,
}

/// How a trashing ended: of how many items it planned, what went and what didn't.
pub(super) struct Ended {
    pub id: TorrentId,
    pub carried: trash::Carried,
}

impl Actor {
    /// TrashFiles: `indices` Off, then those on disk to the Trash.
    pub(super) async fn trash_files(&mut self, id: TorrentId, indices: Vec<usize>, reply: oneshot::Sender<Result<(), Failure>>) {
        match self.start_trashing_files(id, indices).await {
            Ok(previous) => {
                self.file_trashes.insert(id, Pending { reply, previous });
            }
            Err(e) => self.answer(reply, Err(e)),
        }
    }

    /// Decides and sets the files Off, then starts the move: the priorities they had.
    async fn start_trashing_files(&mut self, id: TorrentId, indices: Vec<usize>) -> Result<Vec<(usize, Priority, PathBuf)>, Failure> {
        // As any command: decided on librqbit and the disk as they are now.
        self.poll_torrent(id).await;
        self.size_check(id).await;
        self.event(id, Event::Command(proto::Action::TrashFiles)).await?;
        let handle = self.handle_of(id)?.clone();
        let held = self.held(&handle).cloned();
        let listed = super::files(&handle, &[], held.as_ref(), &BTreeSet::new())?;
        if let Some(bad) = indices.iter().find(|&&i| !listed.iter().any(|f| f.index == i)) {
            return Err(Failure::torrent(id, Refusal::NoFile { index: *bad }));
        }
        let ToTrash { location, files: paths, single } = to_trash(&handle)?;
        let previous: Vec<(usize, Priority, PathBuf)> = listed
            .iter()
            .filter(|f| indices.contains(&f.index))
            .filter_map(|f| Some((f.index, f.priority, paths.get(f.index)?.clone())))
            .collect();
        let chosen: Vec<PathBuf> = previous.iter().map(|(_, _, path)| path.clone()).collect();
        // Off first: a file that goes while it's Normal would make the torrent Missing files.
        let chosen_indices: Vec<usize> = previous.iter().map(|(i, _, _)| *i).collect();
        self.set_priority(id, &chosen_indices, Priority::Off).await?;
        self.size_check(id).await;
        self.poll_torrent(id).await;
        let trash = self.trash.clone();
        self.file_trashing.spawn_blocking(move || {
            let carried = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plan = trash::plan_files(&location, &chosen, single);
                trash::carry_out(&plan, &trash, |_| {})
            }));
            let failed = |path: &PathBuf| (path.clone(), KeptWhy::Failed { detail: "trashing stopped".into() });
            let carried = carried.unwrap_or_else(|_| trash::Carried { moved: Vec::new(), kept: chosen.iter().map(failed).collect() });
            Ended { id, carried }
        });
        Ok(previous)
    }

    /// How a file trash ended: what moved goes on the undo stack, and the client hears of what
    /// couldn't move (those files are Off all the same). Then what waited for it runs.
    pub(super) async fn files_trashed(&mut self, ended: Ended) {
        let Ended { id, carried: trash::Carried { moved, kept } } = ended;
        let Some(Pending { reply, previous }) = self.file_trashes.remove(&id) else { return };
        let name = self.torrents.get(&id).map(name_of);
        if let (false, Some(name), Some(info_hash)) = (moved.is_empty(), name.clone(), self.torrents.get(&id).map(|t| t.info_hash.clone()))
        {
            let count = moved.len();
            self.push(Entry {
                seq: 0,
                action: UndoAction::TrashFiles { count },
                name,
                info_hash,
                id,
                back: Back::Files { previous },
                put_back: moved,
            });
        }
        self.size_check(id).await;
        self.poll_torrent(id).await;
        let answer = match kept.is_empty() {
            true => Ok(()),
            false => {
                let kept = kept
                    .into_iter()
                    .map(|(path, why)| Kept { path: proto::AbsolutePath::new(path).expect("librqbit's paths are absolute"), why })
                    .collect();
                Err(Failure::torrent(id, Refusal::FilesNotTrashed { name: name.unwrap_or_else(|| format!("torrent {id}")), kept }))
            }
        };
        self.answer(reply, answer);
        for message in std::mem::take(self.parked.get_mut().expect("never locked")) {
            self.handle(message).await;
        }
    }

    /// Whether `message` waits for the file trash under way: a command for that torrent, or an Undo
    /// (which would otherwise undo what came before the trash).
    pub(super) fn parks(&self, message: &Message) -> bool {
        match message {
            Message::Command { id, .. } => self.file_trashes.contains_key(id),
            Message::Undo(_) => !self.file_trashes.is_empty(),
            Message::Add { .. }
            | Message::AddFinished { .. }
            | Message::FilesChanged(_)
            | Message::Space(_)
            | Message::Stop(_)
            | Message::Seeding(_) => false,
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => false,
        }
    }
}
