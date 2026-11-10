//! The error list: where errors with no better place go. Adds from outside the window, Open…,
//! the folder picker and failed commands report here; errors that have a place stay there (the
//! link field's by Add, a disabled button's reason on the button).
//!
//! It's an actor: it owns its entries, and anyone with the app state sends it an [`Event`] (a
//! [`Notice`] to `report`, say). The events wait in a mailbox that the UI thread drains once per
//! frame ([`ErrorList::pump`]), so senders know nothing of how, or whether, it's shown. The
//! rules are [`machine`]'s.

pub mod machine;

use std::collections::VecDeque;

use crate::notice::Notice;
use machine::{Command, Do, Entry, EntryId, Event, Observed, State, decide, next};

#[derive(Default)]
pub struct ErrorList {
    state: State,
    open: bool,
    mailbox: VecDeque<Event>,
}

impl ErrorList {
    pub fn send(&mut self, event: Event) {
        self.mailbox.push_back(event);
    }

    pub fn report(&mut self, notice: Notice) {
        self.send(Event::Command(Command::Report(notice)));
    }

    pub fn dismiss(&mut self, id: EntryId) {
        self.send(Event::Command(Command::Dismiss(id)));
    }

    pub fn clear_all(&mut self) {
        self.send(Event::Command(Command::ClearAll));
    }

    pub fn escape_pressed(&mut self) {
        self.send(Event::Observed(Observed::EscapePressed));
    }

    /// Handles what's in the mailbox, in order: a refused event (a click on an entry already
    /// gone) changes nothing. Returns whether anything was handled.
    pub fn pump(&mut self) -> bool {
        let handled = !self.mailbox.is_empty();
        while let Some(event) = self.mailbox.pop_front() {
            if let Ok(state) = decide(&self.state, &event) {
                self.state = state;
            }
        }
        match next(&self.state, self.open) {
            Some(Do::Open) => self.open = true,
            Some(Do::Close) => self.open = false,
            None => {}
        }
        handled
    }

    /// Whether the panel is up: while there are entries.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.state.entries
    }
}
