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

use librqbit::{ManagedTorrent, TorrentStatsState};
use proto::{Action, Held, TorrentId};
use tracing::warn;

use super::super::trashed::{AsChecked, Check, Lift, Opening};
use super::super::{bitv, files};
use super::{Actor, Event, Observation, Phase, Wants, lib, on_disk};

/// How many checks in a row found the files changed meanwhile before it's left Unchecked.
const CHECKS: u8 = 3;

/// What the actor keeps of a torrent's hold, in memory only.
#[derive(Default)]
pub struct Hold {
    /// The check librqbit's torrent of it is checked by, with that torrent: evidence only while
    /// librqbit still has that very one ([`Actor::evidence`]).
    check: Option<(Arc<ManagedTorrent>, Arc<Check>)>,
    /// "Download over them": the files the person chose to write over, as the last check of them
    /// opened them. Of those only, unchanged: others were never chosen. Carried out, or not, by the
    /// first judging of the hold, and never kept past it.
    pressed: Option<Opening>,
    /// It's being lifted: what its next add to librqbit may open to write.
    pub(super) lifting: Option<Arc<Lift>>,
    /// How many checks in a row found its files changed meanwhile.
    changed: u8,
    /// It's being judged: the events that follow from that judge nothing themselves.
    judging: bool,
}

impl Hold {
    /// It was added to librqbit (`of`: as which of its torrents, checked by which check; none: with
    /// no check of its hold, or not added). What was known of the torrent librqbit had before is of
    /// that one.
    pub(super) fn checked(&mut self, of: Option<(&Arc<ManagedTorrent>, Arc<Check>)>) {
        self.check = of.map(|(handle, check)| (handle.clone(), check));
    }
}

impl Actor {
    /// The evidence of what's at torrent `id`'s place: the check in full of the torrent librqbit has
    /// of it now, once that's over and its storage opened its files. None of one librqbit was given
    /// since (added again, moved, brought back), nor after a restart.
    fn evidence(&self, id: TorrentId) -> Option<Arc<Check>> {
        let torrent = self.torrents.get(&id)?;
        let (of, check) = torrent.hold.check.as_ref()?;
        let lib::Lib::In(handle) = &torrent.lib else { return None };
        let over = !matches!(handle.stats().state, TorrentStatsState::Initializing { .. });
        (Arc::ptr_eq(of, handle) && over && check.read()).then(|| check.clone())
    }

    /// Ends torrent `id`'s hold, if it's held and its check is over: lifted once its pieces are
    /// there, or the person chose to download over the files that check read; else it's stopped, its
    /// files untouched.
    pub(super) async fn judge_held(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(held) = torrent.record.held() else { return };
        let lib::Lib::In(handle) = &torrent.lib else { return };
        if torrent.lifecycle.phase != Phase::Ready || matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
            return;
        }
        // Its check is over, whatever is decided of it and when.
        if let Some(check) = self.evidence(id) {
            check.end();
        }
        // Not under a step of its own (files of it on their way to the Trash, say): the next poll does.
        if self.steps.machine().lanes.get(&id).is_some_and(|lane| !lane.is_empty()) {
            return;
        }
        // Out of reach: the size check's to find.
        if files::unavailable(&torrent.record.destination).is_some() {
            return;
        }
        let evidence = self.evidence(id);
        // A piece the check found, in any file (set Off too), with a non-zero byte in what it read: a
        // piece of zeros may be another file's. What librqbit saved is this check's: an add that
        // checks drops what was saved before.
        let saved = std::fs::read(bitv::path(&self.session_folder(), &torrent.record.info_hash)).unwrap_or_default();
        let any = evidence.as_ref().is_some_and(|check| check.found_in(&saved));
        let wants = torrent.lifecycle.wants;
        let torrent = self.torrents.get_mut(&id).expect("just found");
        if std::mem::replace(&mut torrent.hold.judging, true) {
            return;
        }
        // Carried out now, on the files of the check it was made on, or never.
        let pressed = torrent.hold.pressed.take();
        let chosen = held == Held::DontMatch
            && wants == Wants::Run
            && matches!((&pressed, &evidence), (Some(pressed), Some(check)) if check.opening() == Some(pressed));
        match (held, evidence) {
            (_, Some(check)) if any || chosen => self.lift_hold(id, &check).await,
            (Held::DontMatch, _) => {}
            // Nothing checked it (its storage opened nothing, or twice): checked again.
            (Held::Checking, None) => self.check_held_again(id).await,
            (Held::Checking, Some(_)) => {
                // Nothing at all there any more: gone, not other bytes.
                let paths = self.torrents.get(&id).map(|t| t.record.paths()).unwrap_or_default();
                let gone = paths.iter().all(|p| files::stamp(p).is_none());
                if gone { self.not_its(id).await } else { self.dont_match(id).await }
            }
        }
        // Wanted running with files that don't match, and not lifted: a choice that wasn't carried out
        // (refused, or made of other files, or before a crash) isn't kept for later.
        self.dont_match_stops(id).await;
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.hold.judging = false;
        }
    }

    /// The files of torrent `id` the person chooses by "Download over them" (Resume, its files not
    /// matching), taken before the Resume is decided: those its last check opened and read to the
    /// end, if one did in this run. None after a restart, an undo or a move, nor of a check still
    /// reading: nothing read them yet.
    pub(super) fn choice(&self, id: TorrentId) -> Option<Option<Opening>> {
        let torrent = self.torrents.get(&id)?;
        (torrent.record.held() == Some(Held::DontMatch))
            .then(|| torrent.hold.check.as_ref().and_then(|(_, check)| check.read_to_the_end().cloned()))
    }

    /// Torrent `id` was resumed with `choice` ([`Actor::choice`]): carried out by the first judging of
    /// its hold ([`Actor::judge_held`]), once nothing runs on its lane, if the check then current
    /// opened those very files, unchanged (its lifecycle may have checked it again meanwhile, as it
    /// does one Missing files). With no check of them yet it's checked first, and lifted only if that
    /// finds its pieces: else it's stopped again, and the choice is to be made again, of what was read.
    pub(super) async fn pressed(&mut self, id: TorrentId, choice: Option<Opening>) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        let waits = matches!(torrent.lifecycle.phase, Phase::Ready | Phase::CheckDue { .. } | Phase::Checking { .. });
        torrent.hold.pressed = choice.filter(|_| waits);
        let unread = torrent.hold.pressed.is_none() && torrent.lifecycle.phase == Phase::Ready;
        if unread && torrent.record.held() == Some(Held::DontMatch) {
            self.check_held_again(id).await;
        }
    }

    /// Stops torrent `id`, held: its files don't match.
    async fn dont_match(&mut self, id: TorrentId) {
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.record.held_as(Held::DontMatch);
            torrent.hold.changed = 0;
        }
        self.dont_match_stops(id).await;
        self.save();
    }

    /// Torrent `id`, its files not matching, is stopped. In one save with what it's held as: saved as
    /// it doesn't match and wanted running, it would be as if resumed.
    async fn dont_match_stops(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        if torrent.record.held() != Some(Held::DontMatch) || torrent.lifecycle.wants != Wants::Run {
            return;
        }
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

    /// Checks torrent `id` again, held, as its files aren't as they were checked, or weren't checked;
    /// left Unchecked once that came [`CHECKS`] times in a row.
    async fn check_held_again(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        // Also one the person resumed: what they chose to write over isn't there any more.
        torrent.record.held_as(Held::Checking);
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

    /// Torrent `id`'s files now, as against those `check` read.
    fn as_checked(&self, id: TorrentId, check: &Check) -> AsChecked {
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
        self.guard.trashed().as_checked(&record.info_hash, check, &record.destination, &all)
    }

    /// Lifts torrent `id`'s hold, if its files are as `check` read them: out of librqbit, kept aside
    /// with the pieces that check found, then back in with them, in storage that opens only those
    /// files, to write. Saved unheld only with that storage's proof that it opened them and found them
    /// so ([`Record::lift`](super::super::store::Record::lift)): a crash before that, or an add that
    /// opened nothing, leaves it held.
    async fn lift_hold(&mut self, id: TorrentId, check: &Check) {
        let lift: Arc<Lift> = match self.as_checked(id, check) {
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
        torrent.hold.lifting = Some(lift.clone());
        self.polled_as_saved(id);
        #[cfg(test)]
        self.guard.trashed().at(super::super::files::At::Lifting);
        // Still held as it's added: only its storage is the lift's.
        let added = self.carry_out(id).await;
        let Some(torrent) = self.torrents.get_mut(&id) else { return };
        torrent.hold.lifting = None;
        match (added, lift.opened()) {
            (Ok(()), Some(lifted)) => {
                torrent.hold.changed = 0;
                torrent.record.lift(lifted);
                self.save();
                // Not held any more, it starts if it's wanted running.
                if let Err(e) = self.carry_out(id).await {
                    warn!(id, error = ?e, "couldn't start a found torrent whose hold is lifted");
                }
            }
            // In librqbit with storage that opened nothing: not lifted, whatever that add was.
            (Ok(()), None) => {
                warn!(id, "a found torrent was added back without its files being opened: it's checked again, held");
                if let Err(e) = self.take_out(id).await {
                    warn!(id, error = %e, "couldn't take it out again");
                }
                self.check_held_again(id).await;
            }
            (Err(e), _) => {
                warn!(id, error = ?e, "couldn't add a found torrent back to lift its hold");
                // As they were checked, it's an error as any add's.
                match self.as_checked(id, check) {
                    AsChecked::Same(_) => {}
                    AsChecked::Changed => self.check_held_again(id).await,
                    AsChecked::NotIts => self.not_its(id).await,
                }
            }
        }
        self.save();
        // The stash isn't named any more.
        self.save_undo();
    }
}
