//! A torrent a Move or a Rename found where it is now (#358) is held while it's checked there:
//! written ahead with its move (`Record::held`), it's added to librqbit with storage that touches
//! nothing ([`Guard::held`](super::super::files::Guard::held)), and never started, so nothing there
//! is made, written, grown or stamped. Once the check ends:
//!
//! - any of its pieces found there: the hold is lifted. It's taken out of librqbit, kept aside with
//!   the pieces it found, and added back with them in its normal storage (as letting go of a file
//!   is, `letgo.rs`), which makes what isn't there and sets each file's length; then it runs or
//!   stays stopped as wanted.
//! - none: it's stopped, still held, its files as they were ("Files don't match", which clients
//!   tell). Resume lifts the hold, and it downloads over them, the person's choice.
//!
//! A crash meanwhile leaves it held: the next start checks it in full again, held.

use proto::{Action, Held, TorrentId};
use tracing::warn;

use super::super::bitv;
use super::{Actor, Event, Phase, Wants, lib};

impl Actor {
    /// Ends torrent `id`'s hold, if it's held and its check is over: lifted once its pieces are
    /// there, or the person resumes it once none were; else it's stopped, its files untouched.
    pub(super) async fn judge_held(&mut self, id: TorrentId) {
        let Some(torrent) = self.torrents.get(&id) else { return };
        let Some(held) = torrent.record.held else { return };
        if torrent.lifecycle.phase != Phase::Ready || !matches!(torrent.lib, lib::Lib::In(_)) {
            return;
        }
        let wants = torrent.lifecycle.wants;
        match (held, self.pieces(id) > 0) {
            (_, true) => self.lift_hold(id).await,
            (Held::DontMatch, false) if wants == Wants::Run => self.lift_hold(id).await,
            (Held::DontMatch, false) => {}
            (Held::Checking, false) => {
                self.torrents.get_mut(&id).expect("just found").record.held = Some(Held::DontMatch);
                self.save();
                if let Err(e) = Box::pin(self.event(id, Event::Command(Action::Pause))).await {
                    warn!(id, error = ?e, "couldn't stop a found torrent whose files don't match");
                }
            }
        }
    }

    /// Lifts torrent `id`'s hold: out of librqbit, kept aside with the pieces its check found, then
    /// back in with them, in storage that touches its files. A crash before it's saved unheld
    /// checks it again, held.
    async fn lift_hold(&mut self, id: TorrentId) {
        let had = self.pieces(id);
        let stash = self.stash_of(id);
        if let Err(e) = self.take_out(id).await {
            return warn!(id, error = %e, "couldn't take a found torrent out of librqbit to lift its hold");
        }
        let Some(torrent) = self.torrents.get(&id) else { return };
        let saved = bitv::path(&self.session_folder(), &torrent.record.info_hash);
        if let Some(stash) = &stash {
            // librqbit's last save of its pieces lands in the stash, which shares the file.
            bitv::wait_for(&[(stash.bitv(), had)]).await;
            if let Err(e) = stash.restore_pieces(&saved) {
                warn!(id, error = %e, "couldn't put back the pieces a found torrent's check found: it's checked again");
            }
        }
        if let Some(torrent) = self.torrents.get_mut(&id) {
            torrent.record.held = None;
        }
        self.save();
        self.polled_as_saved(id);
        if let Err(e) = self.carry_out(id).await {
            warn!(id, error = ?e, "couldn't add a found torrent back once its hold was lifted");
        }
        // The stash isn't named any more.
        self.save_undo();
    }
}
