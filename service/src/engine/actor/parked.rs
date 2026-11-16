//! What waits for a change to the undo stack under way (#190): while a Delete or an undo is under
//! way, every Remove and Delete (of any torrent) and every Undo. A Delete's entry goes on the stack,
//! and an undo's put-back ends, after the actor has gone on to other messages; one of these handled
//! meanwhile would change the stack first. They're parked instead, and run in the order they came
//! once what's under way ends. Entries then go on the stack in the order the commands came, and an
//! Undo takes the last before it (ADR 0008). The other commands, of any torrent, are answered
//! meanwhile.

use super::{Actor, Ending, Message};

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
            | Message::Stop(..)
            | Message::Seeding(_) => false,
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => false,
        };
        stacks && (!self.deletes.is_empty() || self.undo.undoing.is_some())
    }

    /// What waited runs now, in the order it came: a Delete or an Undo among it starts, and parks
    /// the rest again. At a restart too, which waits for what it starts (`Actor::stop`); at a
    /// shutdown nothing new starts: each client hears the service is stopping.
    pub(super) async fn replay_parked(&mut self) {
        for message in std::mem::take(self.parked.get_mut().expect("never locked")) {
            match self.ending {
                None | Some(Ending::Restart) => self.handle(message).await,
                Some(Ending::Shutdown) => self.refuse_stopping(message),
            }
        }
    }

    /// A parked message, refused at a shutdown.
    fn refuse_stopping(&self, message: Message) {
        match message {
            Message::Command { reply, .. } => self.answer(reply, Err(proto::Refusal::Stopping.into())),
            Message::Undo(reply) => self.answer(reply, Err(proto::Refusal::Stopping.into())),
            Message::Add { .. }
            | Message::AddFinished { .. }
            | Message::FilesChanged(_)
            | Message::Space(_)
            | Message::Stop(..)
            | Message::Seeding(_) => unreachable!("only commands and Undos are parked"),
            #[cfg(test)]
            Message::Inspect(_) | Message::Kill(_) => unreachable!("only commands and Undos are parked"),
        }
    }
}
