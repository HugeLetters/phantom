//! The status bar's notices (#120): what it tells, once (an add, the settings reset, a failure).
//! It shows the newest, with how many there are; a click always opens a popover above it showing
//! one at a time, with its list and buttons, and ‹ › (or ← →) to step through them. The rules are [`machine`]'s; anyone with the app state calls
//! these, and drawing follows.

pub mod machine;

use crate::notice::Notice;
use machine::{Command, Entry, Event, NoticeId, Observed, State, Way, decide};

pub struct Bar {
    state: State,
}

impl Default for Bar {
    fn default() -> Self {
        Self { state: State::default() }
    }
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

    /// The app clock reads `now` seconds: timed notices whose time has come go. Whether any did.
    pub fn passed(&mut self, now: f64) -> bool {
        let before = self.state.len();
        self.send(Event::Observed(Observed::Passed(now)));
        self.state.len() != before
    }

    /// When the first timed notice goes, on the app clock: the window wakes then.
    pub fn next_due(&self) -> Option<f64> {
        self.state.next_due()
    }

    /// These undo-stack entries left the stack, as the service says: the notices of those undone go,
    /// those of absorbed ones follow what took their place.
    pub fn left(&mut self, left: Vec<proto::Left>) {
        if !left.is_empty() {
            self.send(Event::Observed(Observed::Left(left)));
        }
    }

    /// An Undo was pressed for these undo-stack entries: the notices of those actions leave at once,
    /// and come back if it's refused.
    pub fn undoing(&mut self, seqs: Vec<u64>) {
        self.send(Event::Command(Command::Undoing(seqs)));
    }

    /// A click on the bar's notice: opens the list at the newest. It never dismisses.
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

    /// Dismiss all: every notice goes.
    pub fn dismiss_all(&mut self) {
        self.send(Event::Command(Command::DismissAll));
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

    /// How many notices there are.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.state.len()
    }

    /// The notice the popover shows, with its place among them: newest first, from 0, and how
    /// many there are.
    pub fn shown(&self) -> Option<(&Entry, usize, usize)> {
        Some((self.state.shown()?, self.state.index()?, self.state.entries.len()))
    }

    /// The notices, oldest first.
    #[cfg(test)]
    pub fn entries(&self) -> &[Entry] {
        &self.state.entries
    }

    /// The notice the bar shows: the newest.
    pub fn newest(&self) -> Option<&Notice> {
        self.state.newest().map(|e| &e.notice)
    }

    /// The Undo of the newest action's notice, while there is one.
    pub fn newest_undo(&self) -> Option<&crate::notice::NoticeAction> {
        self.state.entries.iter().rev().find_map(|e| e.notice.undo())
    }

    /// The bar's pill: how many notices, and the worst severity among them; none for a lone notice
    /// with nothing more to read.
    pub fn pill(&self) -> Option<(usize, crate::notice::Severity)> {
        self.state.pill()
    }
}
