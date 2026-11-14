//! The status bar's notices (#120): what it tells, told once (an add, the settings reset) or live
//! while a condition holds (a disk short of space). It shows the newest, with how many there are;
//! a click opens a popover above it showing one at a time, with its explanation and actions, and
//! ‹ › (or ← →) to step through them. The rules are [`machine`]'s; anyone with the app state calls
//! these, and drawing follows.

pub mod machine;

use crate::notice::Notice;
use machine::{Command, Entry, Event, NoticeId, Observed, State, Way, decide};

#[derive(Default)]
pub struct Bar {
    state: State,
}

impl Bar {
    /// Decides `event`; one refused (a click that raced another) changes nothing.
    fn send(&mut self, event: Event) {
        if let Ok(state) = decide(&self.state, &event) {
            self.state = state;
        }
    }

    pub fn tell(&mut self, notice: Notice) {
        self.send(Event::Command(Command::Tell(notice)));
    }

    /// The conditions that hold now, by key, each as its notice.
    pub fn live(&mut self, live: Vec<(String, Notice)>) {
        self.send(Event::Observed(Observed::Live(live)));
    }

    /// A click on the bar's notice: opens the popover, or, with only one notice and nothing more
    /// to read, dismisses it.
    pub fn press(&mut self) {
        self.send(Event::Command(Command::Press));
    }

    /// Escape, or a click outside the popover: whether it was the bar's, closing the popover. When
    /// it wasn't, Escape goes on to the page.
    pub fn close(&mut self) -> bool {
        let state = decide(&self.state, &Event::Command(Command::Close));
        if let Ok(state) = &state {
            self.state = state.clone();
        }
        state.is_ok()
    }

    pub fn dismiss(&mut self, id: NoticeId) {
        self.send(Event::Command(Command::Dismiss(id)));
    }

    /// ← and ‹ (newer), → and › (older), with the popover open; whether it was the bar's. At
    /// either end it stops, and the key is still the popover's.
    pub fn step(&mut self, way: Way) -> bool {
        let open = self.state.open();
        self.send(Event::Command(Command::Step(way)));
        open
    }

    pub fn is_open(&self) -> bool {
        self.state.open()
    }

    /// The notice the popover shows, with its place among them: newest first, from 0, and how
    /// many there are.
    pub fn shown(&self) -> Option<(&Entry, usize, usize)> {
        Some((self.state.shown()?, self.state.index()?, self.state.entries.len()))
    }

    /// The notices, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.state.entries
    }

    /// The notice the bar shows: the newest.
    pub fn newest(&self) -> Option<&Notice> {
        self.state.newest().map(|e| &e.notice)
    }

    /// Whether a click opens something.
    pub fn openable(&self) -> bool {
        self.state.openable()
    }
}
