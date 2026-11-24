//! A torrent a Move or a Rename found where it is now (#358) is held while it's checked there:
//! saved with the place it's found at (`Record::held`), it's added to librqbit with storage that touches
//! nothing ([`Guard::held`](super::super::files::Guard::held)), and never started, so nothing there
//! is made, written, grown or stamped. Once the check ends:
//!
//! - any of its pieces found there (in a file set Off too), and its files still those the check read,
//!   unchanged, each its own to write ([`AsChecked`]): the hold is lifted. It's taken out of librqbit,
//!   kept aside with the pieces it found, and added back with them (as letting go of a file is,
//!   `letgo.rs`) in storage that looks at each file again through what it opened to write, then makes
//!   what isn't there and sets each file's length; then it runs or stays stopped as wanted.
//! - its files changed, or others, since the check opened them: checked again, held. After
//!   [`CHECKS`] times in a row, Unchecked, held, for the person to check once they stay as they are.
//! - one gone, or not its own to write any more (a link, longer, with another hard link): Missing
//!   files, held (set Off, so not missing: stopped, as with none of its pieces there).
//! - none of its pieces: it's stopped, still held, its files as they were ("Files don't match", which
//!   clients tell). Resume lifts the hold, by the same rule, and it downloads over them, the person's
//!   choice.
//!
//! A crash meanwhile leaves it held: the next start checks it in full again, held.

use std::path::PathBuf;
use std::sync::Arc;

use librqbit::TorrentStatsState;
use proto::{Action, Held, TorrentId};
use tracing::warn;

use super::super::trashed::{AsChecked, Lift};
use super::super::{bitv, files};
use super::{Actor, Event, Observation, Phase, Wants, lib, on_disk};

/// How many checks in a row found the files changed meanwhile before it's left Unchecked.
const CHECKS: u8 = 3;

/// What the actor keeps of a torrent's hold, in memory only.
#[derive(Default)]
pub struct Hold {
    /// It's being lifted: what its next add to librqbit may open to write.
    pub(super) lifting: Option<Arc<Lift>>,
    /// How many checks in a row found its files changed meanwhile.
    changed: u8,
    /// It's being judged: the events that follow from that judge nothing themselves.
    judging: bool,
}

impl Actor {
    /// Ends torrent `id`'s hold, if it's held and its check is over: lifted once its pieces are
    /// there, or the person resumes it once none were; else it's stopped, its files untouched.
    pub(super) async fn judge_held(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(held) = torrent.record.held else { return };
        let lib::Lib::In(handle) = &torrent.lib else { return };
        if torrent.lifecycle.phase != Phase::Ready || matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
            return;
        }
        // Not under a step of its own (files of it on their way to the Trash, say): the next poll does.
        if self.steps.machine().lanes.get(&id).is_some_and(|lane| !lane.is_empty()) {
            return;
        }
        // Out of reach: the size check's to find.
        if files::unavailable(&torrent.record.destination).is_some() {
            return;
        }
        // Every file's, set Off too: librqbit's own progress leaves those out.
        let any = handle.stats().file_progress.iter().any(|had| *had > 0);
        let resumed = held == Held::DontMatch && torrent.lifecycle.wants == Wants::Run;
        if std::mem::replace(&mut self.torrents.get_mut(&id).expect("just found").hold.judging, true) {
            return;
        }
        match held {
            _ if any || resumed => self.lift_hold(id).await,
            Held::DontMatch => {}
            Held::Checking => {
                // Nothing at all there any more: gone, not other bytes.
                let paths = self.torrents.get(&id).map(|t| t.record.paths()).unwrap_or_default();
                let gone = paths.iter().all(|p| files::stamp(p).is_none());
                if gone { self.not_its(id).await } else { self.dont_match(id).await }
            }
        }
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.hold.judging = false;
        }
    }

    /// Stops torrent `id`, held: its files don't match.
    async fn dont_match(&mut self, id: TorrentId) {
        // In one save with the Pause: saved as it doesn't match and wanted running, a start would take
        // it for a Resume.
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        if torrent.record.held == Some(Held::DontMatch) && torrent.lifecycle.wants != Wants::Run {
            return;
        }
        torrent.record.held = Some(Held::DontMatch);
        torrent.hold.changed = 0;
        if let Err(e) = Box::pin(self.event(id, Event::Command(Action::Pause))).await {
            warn!(id, error = ?e, "couldn't stop a found torrent whose files don't match");
        }
        self.save();
    }

    /// Torrent `id`'s files, or some, are gone or aren't its own to write: Missing files, held. Those
    /// it gets: if it's only one set Off, it's stopped.
    async fn not_its(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let seen = Observation::Files(on_disk(torrent));
        if let Err(e) = Box::pin(self.event(id, Event::Observed(seen))).await {
            warn!(id, error = ?e, "couldn't follow a found torrent's files");
        }
        match self.torrents.get(&id).map(|t| &t.lifecycle.phase) {
            Some(Phase::Ready) => self.dont_match(id).await,
            Some(_) | None => {}
        }
    }

    /// Checks torrent `id` again, held, as its files aren't as they were checked; left Unchecked once
    /// that came [`CHECKS`] times in a row.
    async fn check_held_again(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        // Also one the person resumed: what they chose to write over isn't there any more.
        torrent.record.held = Some(Held::Checking);
        torrent.hold.changed += 1;
        let enough = torrent.hold.changed >= CHECKS;
        if enough {
            torrent.hold.changed = 0;
        }
        self.save();
        let commands: &[Action] = if enough { &[Action::Check, Action::CancelCheck] } else { &[Action::Check] };
        for action in commands {
            if let Err(e) = Box::pin(self.event(id, Event::Command(*action))).await {
                warn!(id, error = ?e, "couldn't check a found torrent again");
            }
        }
    }

    /// Torrent `id`'s files now, as against those its held check read.
    fn as_checked(&self, id: TorrentId) -> AsChecked {
        let Some((record, info)) = self.torrents.get(&id).and_then(|t| Some((&t.record, t.record.info.as_ref()?))) else {
            return AsChecked::Changed;
        };
        let all: Vec<(usize, PathBuf, u64, bool)> = info
            .files
            .iter()
            .zip(record.paths())
            .enumerate()
            .filter(|(_, (f, _))| !f.padding)
            .map(|(i, (f, path))| (i, path, f.len, record.gets(i)))
            .collect();
        self.guard.trashed().as_checked(&record.info_hash, &record.destination, &all)
    }

    /// Lifts torrent `id`'s hold, if its files are as its check read them: out of librqbit, kept aside
    /// with the pieces its check found, then back in with them, in storage that touches its files, saved
    /// unheld once that opened them and found them so. A crash before that checks it again, held.
    async fn lift_hold(&mut self, id: TorrentId) {
        let lift: Arc<Lift> = match self.as_checked(id) {
            AsChecked::Same(lift) => lift,
            AsChecked::Changed => return self.check_held_again(id).await,
            AsChecked::NotIts => return self.not_its(id).await,
        };
        let Some(torrent) = self.torrents.get(&id) else { return };
        let saved = bitv::path(&self.session_folder(), &torrent.record.info_hash);
        let had = std::fs::read(&saved).map_or(0, |bits| bitv::pieces(&bits));
        let Some(stash) = self.stash_of(id) else {
            return warn!(id, "couldn't keep a found torrent aside to lift its hold: tried again later");
        };
        if let Err(e) = self.take_out(id).await {
            return warn!(id, error = %e, "couldn't take a found torrent out of librqbit to lift its hold");
        }
        // librqbit's last save of its pieces lands in the stash, which shares the file.
        bitv::wait_for(&[(stash.bitv(), had)]).await;
        // Without them, librqbit would check it in storage that writes.
        if let Err(e) = stash.restore_pieces(&saved).and_then(|()| Ok(std::fs::metadata(&saved)?)) {
            warn!(id, error = %e, "couldn't put back the pieces a found torrent's check found: it's checked again");
            return self.check_held_again(id).await;
        }
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        let held = torrent.record.held.take();
        torrent.hold.lifting = Some(lift);
        self.polled_as_saved(id);
        let added = self.carry_out(id).await;
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        torrent.hold.lifting = None;
        match added {
            Ok(()) => torrent.hold.changed = 0,
            Err(e) => {
                warn!(id, error = ?e, "couldn't add a found torrent back to lift its hold");
                torrent.record.held = held;
                // As they were checked, it's an error as any add's; else checked again, held.
                if !matches!(self.as_checked(id), AsChecked::Same(_)) {
                    self.check_held_again(id).await;
                }
            }
        }
        self.save();
        // The stash isn't named any more.
        self.save_undo();
    }
}
