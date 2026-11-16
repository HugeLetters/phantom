//! What waits for a Delete under way (#190): every Remove and Delete (of any torrent) and every
//! Undo. A Delete's entry goes on the undo stack when its trashing ends, so one of these answered
//! meanwhile would put its entry on (or take one off) first: they're parked instead, and run in
//! the order they came once it ends. Entries then go on the stack in the order the commands came
//! (ADR 0008), and the other commands, of any torrent, are answered meanwhile as before.

use super::{Actor, Message};

impl Actor {
    /// Whether `message` waits for the Delete under way: a Remove, a Delete or an Undo, while one is.
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
        stacks && !self.deletes.is_empty()
    }

    /// What waited runs now, in the order it came (a Delete among it parks the rest again); at a
    /// stop it doesn't run: each client hears the service is stopping, as the Undos queued behind
    /// an undo do (ADR 0008).
    pub(super) async fn replay_parked(&mut self, stopping: bool) {
        for message in std::mem::take(self.parked.get_mut().expect("never locked")) {
            if !stopping {
                self.handle(message).await;
                continue;
            }
            match message {
                Message::Command { reply, .. } => self.answer(reply, Err(proto::Refusal::Stopping.into())),
                Message::Undo(reply) => self.answer(reply, Err(proto::Refusal::Stopping.into())),
                // Only commands and Undos are parked.
                Message::Add { .. }
                | Message::AddFinished { .. }
                | Message::FilesChanged(_)
                | Message::Space(_)
                | Message::Stop(_)
                | Message::Seeding(_) => {}
                #[cfg(test)]
                Message::Inspect(_) | Message::Kill(_) => {}
            }
        }
    }
}
