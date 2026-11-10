//! The error list: where errors with no better place go. Adds from outside the window, Open…,
//! the folder picker and failed commands report here; errors that have a place stay there (the
//! link field's by Add, a disabled button's reason on the button).
//!
//! It's an actor: it owns its entries, and anyone with the app state sends it an [`Event`] (a
//! [`Notice`] to `report`, say). The events wait in a mailbox that the UI thread drains once per
//! frame ([`ErrorList::pump`]), so senders know nothing of how, or whether, it's shown. The
//! rules are [`machine`]'s.

pub mod machine;

use std::collections::{BTreeMap, VecDeque};

use rune::Tx;

use crate::notice::Notice;
use machine::{Command, Do, Entry, EntryId, Event, Observed, State, decide, next};

/// How long an entry fades before the ones below it move up.
const FADE_SECS: f64 = 0.15;

#[derive(Default)]
pub struct ErrorList {
    state: State,
    /// The app clock when each leaving entry's fade began.
    since: BTreeMap<EntryId, f64>,
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

    /// Starts the fade clock of newly leaving entries, and sends `FadedOut` for those whose fade is
    /// over; asks to be woken for the next. `now` is the app clock.
    fn age(&mut self, now: f64, tx: &mut Tx) {
        let leaving: Vec<EntryId> = self.state.entries.iter().filter(|e| e.leaving).map(|e| e.id).collect();
        self.since.retain(|id, _| leaving.contains(id));
        for id in leaving {
            let since = *self.since.entry(id).or_insert(now);
            if now - since >= FADE_SECS {
                self.send(Event::Observed(Observed::FadedOut(id)));
            } else {
                tx.wake_after(std::time::Duration::from_secs_f64(FADE_SECS - (now - since)));
            }
        }
    }

    /// Handles what's in the mailbox, in order: a refused event (a click on an entry already
    /// gone) changes nothing. Returns whether anything was handled.
    pub fn pump(&mut self, now: f64, tx: &mut Tx) -> bool {
        let mut handled = false;
        // Handling an event may start a fade; one that ended sends the next event.
        for round in 0..2 {
            handled |= !self.mailbox.is_empty();
            while let Some(event) = self.mailbox.pop_front() {
                if let Ok(state) = decide(&self.state, &event) {
                    self.state = state;
                }
            }
            if round == 0 {
                self.age(now, tx);
            }
        }
        handled |= self.state.entries.iter().any(|e| e.leaving);
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
