//! A torrent a Move or a Rename found where it is now (#358) is held: what's at that place may be the
//! person's own files, so nothing there is made, written, grown, stamped or removed until it's known
//! to be the torrent's, or the person chose to download over it.
//!
//! # The rule
//!
//! **A hold is lifted only on evidence made in this run, of the files as they are at the lift.**
//!
//! - *Evidence* is one check in full, over, by the storage of the torrent librqbit has now
//!   ([`Check`], [`Actor::evidence`]): each file as that storage opened it before reading any (volume,
//!   inode, length, modification and change times), and each piece it read a non-zero byte in. It's
//!   in memory only. Nothing saved stands in for it: a held torrent is never added with pieces
//!   librqbit saved (`Actor::add_to_librqbit`), so after a restart, a crash, an undo or a move there
//!   is none until it's checked again, and another add of the torrent has another check or none.
//! - *Lifted by match*: a piece that check found, with real content.
//! - *Lifted by choice*: "Download over them" (Resume, its files not matching) is a choice of the
//!   files a check read to the end ([`Actor::choice`]), and is carried out only by a check that opened
//!   those same files, unchanged ([`Opening`]), at the first judging after it. Made with no such
//!   check, it only has the torrent checked. Refused or not carried out, it's dropped, and the
//!   torrent is stopped: never kept for other files, or for later.
//! - *At the lift* the files are looked at by path ([`Trashed::as_checked`]), then opened to write by
//!   the lift's own storage, which compares each, through what it opened, with what the check opened
//!   ([`Lift`]), before anything is made or written. A storage that can't open them (the destination
//!   out of reach, the torrent kept untouched) fails the add.
//! - *Saved unheld* only by [`Record::lift`], which takes the proof only that storage gives
//!   ([`Lifted`]). The record's hold is private to `store.rs`: the other ways out are a place nothing
//!   of the torrent is at ([`Hold::nothing_there`]) and leaving the list.
//!
//! What it doesn't cover is in [`Files`](super::super::trashed)' docs: a change to the same file after
//! the lift's look, as any torrent's file may get.
//!
//! # Every way into, within and out of a hold
//!
//! Tests are `engine::tests::found`'s unless they say; "generated" is
//! `engine::tests::hold::nothing_changes_while_held_and_a_hold_is_lifted_only_on_what_was_read`, which
//! makes these in any order, with files changed by hand between and while the service is closed.
//!
//! | What happens | Held after | Evidence after | Test |
//! |---|---|---|---|
//! | **Into** | | | |
//! | Move to… or Rename… finds it (saved with the place, `placed`) | Checking | none till its check ends | `a_found_torrent_is_held_untouched_while_checked_then_back_as_it_was`; a crash before the save: `a_found_rename_a_crash_put_back_is_not_held_where_it_was`; after: `a_found_torrent_held_for_its_check_comes_back_held_after_a_crash` |
//! | Undo of a found Rename finds it back under its old name | Checking | none till its check ends | `undo_of_a_found_rename_finds_it_back_under_its_old_name` |
//! | A start (restart, crash, the engine restarted for new settings: all `Actor::new` from the records), Checking | Checking, checked in full | none till it ends | `a_found_torrent_held_for_its_check_comes_back_held_after_a_crash`, generated |
//! | A start, its files not matching | DontMatch, added with none of its pieces, nothing read | none | `pieces_an_earlier_check_saved_are_not_taken_for_files_swapped_across_a_crash`, `files_that_do_not_match_stay_held_across_a_restart_until_they_do`, generated |
//! | A start, Unchecked | as saved, not added | none | `a_held_torrent_whose_check_is_cancelled_stays_held` |
//! | Undo of its Remove or Delete | as it was, added as at a start | none | `a_remove_while_held_leaves_the_files_and_its_undo_brings_it_back_held`, `a_delete_of_files_not_verified_leaves_them_and_its_undo_brings_it_back_held`, generated |
//! | **Within** | | | |
//! | Its check ends, a real piece found, files as checked | lifted (below) | | `a_found_torrent_is_held_untouched_while_checked_then_back_as_it_was` |
//! | The same, files changed or others since | Checking, checked again; Unchecked after 3 in a row | none till it ends | `a_file_swapped_while_checked_is_checked_again`, `files_that_never_stay_as_they_are_are_left_unchecked`, `pieces_found_are_not_taken_for_bytes_changed_before_the_hold_is_judged` |
//! | The same, a file gone or not its own to write | as it was, Missing files | kept, of files that aren't its own | `a_file_that_is_not_its_own_once_checked_is_missing_files_untouched`, `a_file_set_off_that_is_not_its_own_is_missing_files_held` |
//! | Its check ends, no real piece | DontMatch, stopped, one save | that check | `found_files_none_of_whose_pieces_are_the_torrents_are_left_untouched_and_it_stays_stopped`, `another_file_that_shares_only_a_zero_piece_does_not_match` |
//! | Its check ends, nothing there at all | Checking, Missing files | that check | `a_found_folder_renamed_while_checked_is_missing_files_and_not_made_again` |
//! | Its check ends, its storage having opened nothing, or twice | Checking, checked again | none | `trashed::check_tests::a_check_tells_only_of_one_opening` (no command reaches it) |
//! | Check | as it was, checked in full | that check, once over | `files_that_do_not_match_stay_held_across_a_restart_until_they_do`, generated |
//! | Cancel check | as it was, Unchecked | none read to the end | `a_held_torrent_whose_check_is_cancelled_stays_held`, `a_file_grown_while_held_and_not_checked_is_not_cut` |
//! | Resume while it's checked | as it was, wanted running: lifted or stopped as its check ends | | `a_resume_while_held_waits_for_the_check` |
//! | Resume, files not matching, read to the end in this run and unchanged | lifted (below) | | `a_resume_of_files_that_do_not_match_writes_over_none_that_is_not_its_own`, `files_that_do_not_match_with_one_missing_are_left_untouched`, generated (its end) |
//! | Resume, files not matching, nothing read them in this run | Checking, checked; then lifted by match, or DontMatch, stopped | that check | `a_resume_after_a_restart_checks_first_and_downloads_over_nothing_it_did_not_read` |
//! | Resume not carried out (files changed, one not its own, paused before it's judged) | DontMatch (or Checking, checked again), stopped; the choice dropped | | `a_refused_resume_is_not_carried_out_later_on_other_files`, generated |
//! | Pause | as it was, wanted stopped | unchanged | generated; found paused: `a_found_torrent_is_held_untouched_while_checked_then_back_as_it_was` |
//! | A priority | as it was; nothing made | unchanged (which files it gets is read at the lift) | `a_file_set_normal_while_held_is_made_only_once_the_hold_is_lifted`, `pieces_in_files_set_off_match_too`, generated |
//! | Move to Trash of its files | as it was, not judged meanwhile | unchanged | `the_hold_is_not_lifted_under_a_move_to_trash`, `a_file_trashed_while_held_is_read_where_it_went_after_a_restart` |
//! | Move or Rename, taking its files | as it was, at the new place | none: added again | `a_move_while_held_takes_the_files_as_they_are_and_it_is_held_there` |
//! | Its destination goes | as it was, Destination unavailable; not judged, and a lift under way fails | none once it's back: added again | `a_destination_gone_as_the_hold_is_lifted_leaves_it_held`, `a_found_folder_gone_across_a_crash_is_missing_files`, generated |
//! | Its add fails (an error) | as it was, Error | none | `a_destination_gone_as_the_hold_is_lifted_leaves_it_held` (the lift's add); any other add is the same code |
//! | Its files changed by hand | as it was | stale: the lift's look and its storage find them changed | `a_file_changed_as_the_hold_is_lifted_is_left_as_it_is`, `a_file_grown_as_its_length_is_set_is_not_cut`, generated |
//! | **Out** | | | |
//! | Lifted: the lift's storage opened its files as checked, then saved | none | | `a_crash_as_the_hold_is_lifted_before_its_files_are_looked_at_leaves_it_held`, generated (a lift is only ever on what was read) |
//! | Undo of the found Rename, nothing of it under its old name | none there, Missing files | | `undo_of_a_found_rename_while_held_points_it_back_untouched` |
//! | The found Move or Rename goes back before it's placed | none, where it was, as it was | | `a_found_rename_a_crash_put_back_is_not_held_where_it_was` |
//! | Moved with no file left to take, where nothing of it is | none there, Missing files | | `a_held_torrent_with_no_file_left_moved_where_nothing_of_it_is_is_not_held_there` |
//! | Remove, Delete | off the list, its files left; its undo's record keeps the hold | | `a_remove_while_held_leaves_the_files_and_its_undo_brings_it_back_held`, `a_delete_of_files_not_verified_leaves_them_and_its_undo_brings_it_back_held`, `a_delete_of_several_goes_by_each_ones_own_state` |
//!
//! [`Check`]: super::super::trashed::Check
//! [`Opening`]: super::super::trashed::Opening
//! [`Lift`]: super::super::trashed::Lift
//! [`Lifted`]: super::super::trashed::Lifted
//! [`Trashed::as_checked`]: super::super::trashed::Trashed::as_checked
//! [`Record::lift`]: super::super::store::Record::lift
//! [`Hold::nothing_there`]: super::super::store::Hold::nothing_there

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
