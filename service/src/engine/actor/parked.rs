//! What waits for a change to the undo stack under way (#190): while a Delete or an undo is under
//! way, every Remove and Delete (of any torrent) and every Undo. A Delete's entry goes on the stack,
//! and an undo's put-back ends, after the actor has gone on to other messages; one of these handled
//! meanwhile would change the stack first. They're parked instead, and run in the order they came
//! once what's under way ends. Entries then go on the stack in the order the commands came, and an
//! Undo takes the last before it (ADR 0008). The other commands, of any torrent, are answered
//! meanwhile.

use super::{Actor, Message};

impl Actor {
    /// Whether `message` waits: a Remove, a Delete or an Undo, while a Delete or an undo is under way.
    pub(super) fn parks(&self, message: &Message) -> bool {
        let stacks = match message {
            Message::Command { command, .. } => {
                use proto::TorrentCommand as C;
                match command {
                    C::Remove | C::Delete => true,
                    C::Pause | C::Resume | C::Check | C::CancelCheck | C::SetPriority { .. } => false,
                }
            }
            Message::Undo(_) => true,
            Message::Add { .. }
            | Message::AddFinished { .. }
            | Message::FilesChanged(_)
            | Message::Space(_)
            | Message::Stop(_)
            | Message::Seeding(_) => false,
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => false,
        };
        stacks && (!self.deletes.is_empty() || self.undo.undoing.is_some())
    }

    /// What waited runs now, in the order it came: a Delete or an Undo among it starts, and parks
    /// the rest again. At a stop too: the stop waits for what it starts (`Actor::stop`).
    pub(super) async fn replay_parked(&mut self) {
        for message in std::mem::take(self.parked.get_mut().expect("never locked")) {
            self.handle(message).await;
        }
    }
}
