//! Which change ⌘Z takes (#221, #255), as a machine. Pure; [`super`] carries it out.
//!
//! The app keeps one list of the changes it knows, in the order it learned of them, newest last:
//! the settings changes, the last [`KEPT`], its own as they're made (an app setting's, which it
//! undoes itself, and a service setting's, the moment it's sent), and another client's as the
//! service lists them; and the torrent actions (Remove, Delete, Move to Trash), whoever made them,
//! as the snapshots list the undo stack (#255). ⌘Z, in the main window or in Settings, takes the
//! newest not taken yet. An app setting's it undoes at once. A service setting's it asks
//! the service to undo (`UndoSetting`), which always undoes its own newest, so the service's
//! settings go back in the order it made them, even when another client's change came in between
//! the press and its answer: the answer says which it undid. One it sent waits until the service
//! names it: in a snapshot (by the app's id for it) or in its answer, whichever comes first; it keeps
//! its place and a press either way.
//!
//! A torrent action's undo the app asks of the service (`Undo`), which, likewise, undoes the top of
//! its stack and says which entry it took, undone or refused: so the actions go back in the order of
//! the stack. A refused one is gone (ADR 0008); an answer that names none (nothing to undo, the
//! service stopping) leaves the entry to be taken again. The stack is taken from the newest
//! snapshot only, by its `(epoch, version)`, and outlasts the service restarting.
//!
//! No numbers are compared for the order: the app's list is the order, and the service's ids only
//! say which change is which. The settings are taken from the newest snapshot only, by their
//! `at`: an older one, from another connection, changes nothing. A change the service no longer
//! lists (undone by another client, pushed past the last 20, or gone with a service that stopped)
//! leaves the list.
//!
//! | Event \ the change it's about | App | Sent | Listed |
//! |---|---|---|---|
//! | Changed / Sent | added newest | added newest | |
//! | Press, the newest not taken | taken (Undo it) | taken: waits until listed | taken (Ask) |
//! | Asked | | | asked |
//! | AppUndone | gone | | |
//! | Answered (its SetSetting) | | listed, in place, when it made one the service may still have; else gone, its press, if taken, on the newest not taken | |
//! | Undid { asked, took } | | | `took` gone, never added back; `asked`, if it isn't `took`, untaken: ⌘Z can take it again |
//! | Refused { asked } | | | gone, never added back |
//! | Listed { at, changes }, newer | | listed when `by` it | gone unless listed; another client's added, newest |
//! | Listed, not newer | | | |
//!
//! | Event \ the torrent action it's about | Action |
//! |---|---|
//! | Press, the newest not taken | taken (AskAction) |
//! | PressAction { seq } (its notice's Undo) | taken, if not yet; else nothing |
//! | AskedAction | asked |
//! | ActionAnswered { asked, took } | `took` gone, never added back; `asked`, if it isn't `took`, untaken |
//! | Stack { at, entries }, not older | gone unless listed; new ones added, newest, in the stack's order |
//! | Stack, older | |

use proto::{SettingChange, SettingName};

/// How many settings changes the list keeps; the oldest goes. The torrent actions are those the
/// service's stack lists, as many as it keeps.
pub const KEPT: usize = SettingChange::KEPT;

/// An app setting: the App icon, or a theme setting, by its key and label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSetting {
    Icon,
    Theme { key: &'static str, label: &'static str },
}

/// Whether ⌘Z took a change, and, for a service setting's, where its undo is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    No,
    /// Taken: to undo, or ask the service to.
    Taken,
    /// The service was asked, and hasn't answered.
    Asked,
}

/// A change the app knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Of an app setting, by the app's id for it.
    App { id: u64, of: AppSetting, press: Press },
    /// Of a service setting, sent `by` the app's id for it, not listed by the service yet.
    Sent { by: u64, name: SettingName, press: Press },
    /// Of a service setting, as the service lists it.
    Listed { seq: u64, name: SettingName, press: Press },
    /// A torrent action (a Remove, a Delete, a Move to Trash), by its entry on the service's undo
    /// stack.
    Action { seq: u64, press: Press },
}

impl Change {
    pub fn press(&self) -> Press {
        match self {
            Change::App { press, .. } | Change::Sent { press, .. } | Change::Listed { press, .. } | Change::Action { press, .. } => *press,
        }
    }

    fn with(&self, press: Press) -> Change {
        match self.clone() {
            Change::App { id, of, .. } => Change::App { id, of, press },
            Change::Sent { by, name, .. } => Change::Sent { by, name, press },
            Change::Listed { seq, name, .. } => Change::Listed { seq, name, press },
            Change::Action { seq, .. } => Change::Action { seq, press },
        }
    }

    /// What Edit ▸ Undo names it by.
    pub fn of(&self) -> Of {
        match self {
            Change::App { of, .. } => Of::App(*of),
            Change::Sent { name, .. } | Change::Listed { name, .. } => Of::Service(*name),
            Change::Action { seq, .. } => Of::Action(*seq),
        }
    }

    /// A service setting's change, as the service lists it: its `seq`.
    fn seq(&self) -> Option<u64> {
        match self {
            Change::Listed { seq, .. } => Some(*seq),
            Change::App { .. } | Change::Sent { .. } | Change::Action { .. } => None,
        }
    }

    /// A torrent action: its entry on the stack.
    pub fn entry(&self) -> Option<u64> {
        match self {
            Change::Action { seq, .. } => Some(*seq),
            Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } => None,
        }
    }
}

/// What a change is of: a setting, or the torrent action of an entry on the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Of {
    App(AppSetting),
    Service(SettingName),
    Action(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// Oldest first.
    pub list: Vec<Change>,
    /// The largest `seq` the service has listed: a change listed again after it left (it was
    /// refused while the stream still had it) isn't added back.
    pub seen: u64,
    /// Settings no newer than this [`proto::Configured::at`] change nothing: those of the listing
    /// taken last, or from before a change an answer named (right after it, they're news).
    pub at: u64,
    /// The `at` of the listing taken last: a change from before it that it didn't list is gone.
    pub listing: u64,
    /// The service's changes an answer took off the list (undone, or a refused undo's), until a
    /// listing no longer has them: one from before the answer still does, and mustn't add it back.
    pub left: Vec<u64>,
    /// The torrents' undo stack, as the snapshots list it.
    pub stack: Stack,
}

/// What the app has taken of the service's undo stack: the same as for the settings, by the
/// snapshots' own count.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stack {
    /// The `(epoch, version)` of the snapshot taken last: an older one changes nothing.
    pub at: (u64, u64),
    /// The largest entry listed: one listed again after it left isn't added back.
    pub seen: u64,
    /// The entries an answer took off the list, until a listing no longer has them.
    pub left: Vec<u64>,
}

impl State {
    /// The newest change ⌘Z would take now.
    pub fn top(&self) -> Option<&Change> {
        self.list.iter().rev().find(|c| c.press() == Press::No)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// An app setting changed, as the app's change `id`.
    Changed { id: u64, of: AppSetting },
    /// A service setting's change was sent, `by` the app's id for it.
    Sent { by: u64, name: SettingName },
    /// ⌘Z in Settings.
    Press,
    /// The service was asked to undo, for the change `seq`.
    Asked { seq: u64 },
    /// The Undo in the notice of the torrent action `seq`.
    PressAction { seq: u64 },
    /// The service was asked to undo its top torrent action, for the entry `seq`.
    AskedAction { seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The app undid its change `id`.
    AppUndone { id: u64 },
    /// The answer to the change sent `by` this came: the change it made (its `seq`), if any, and
    /// the settings' `at` right after it.
    Answered { by: u64, seq: Option<u64>, at: u64 },
    /// The service undid `took`, asked for `asked`.
    Undid { asked: u64, took: u64 },
    /// The service refused the undo asked for `asked` (or had nothing to undo).
    Refused { asked: u64 },
    /// The service's changes, as settings it sent at `at` list them, oldest first.
    Listed { at: u64, changes: Vec<SettingChange> },
    /// The service's undo stack, as the snapshot `at` (its epoch and version) lists it, oldest
    /// first.
    Stack { at: (u64, u64), entries: Vec<u64> },
    /// The answer to the undo asked for the entry `asked`: the entry the service took (undone, or
    /// refused), or none (nothing to undo, or no answer).
    ActionAnswered { asked: u64, took: Option<u64> },
}

/// Why ⌘Z does nothing: no change left to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nothing;

/// What the app carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Undo the app's change `id`.
    Undo { id: u64 },
    /// Ask the service to undo its newest, for the change `seq`.
    Ask { seq: u64 },
    /// Ask the service to undo its top torrent action, for the entry `seq`.
    AskAction { seq: u64 },
}

pub fn decide(state: &State, event: &Event) -> Result<State, Nothing> {
    use Event::{Command as C, Observed as O};
    let mut next = state.clone();
    match event {
        C(Command::Changed { id, of }) => next.push(Change::App { id: *id, of: *of, press: Press::No }),
        C(Command::Sent { by, name }) => next.push(Change::Sent { by: *by, name: *name, press: Press::No }),
        C(Command::Press) => {
            let at = next.list.iter().rposition(|c| c.press() == Press::No).ok_or(Nothing)?;
            next.list[at] = next.list[at].with(Press::Taken);
        }
        C(Command::Asked { seq }) => next.set(*seq, Press::Taken, Press::Asked),
        C(Command::PressAction { seq }) => {
            let at = next.list.iter().position(|c| c.entry() == Some(*seq) && c.press() == Press::No).ok_or(Nothing)?;
            next.list[at] = next.list[at].with(Press::Taken);
        }
        C(Command::AskedAction { seq }) => next.set_entry(*seq, Press::Taken, Press::Asked),
        O(Observed::AppUndone { id }) => next.list.retain(|c| !matches!(c, Change::App { id: i, .. } if i == id)),
        // The change it made is listed in place, keeping its press, unless the settings taken
        // are from after it (and so would have listed it): then it's gone. Settings from before
        // it are older from now on; those right after it, at `at`, aren't.
        // Gone and taken, its press goes to the newest change not taken: two presses, two undos.
        O(Observed::Answered { by, seq, at }) => {
            // Once: a change already listed, or undone, isn't listed again.
            let mut place =
                seq.filter(|seq| *at > state.listing && !state.list.iter().any(|c| c.seq() == Some(*seq)) && !state.left.contains(seq));
            let mut passed = false;
            next.list = next
                .list
                .into_iter()
                .filter_map(|c| match c {
                    Change::Sent { by: b, name, press } if b == *by => {
                        let placed = place.take().map(|seq| Change::Listed { seq, name, press });
                        passed |= placed.is_none() && press != Press::No;
                        placed
                    }
                    Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } | Change::Action { .. } => Some(c),
                })
                .collect();
            if let Some(at) = next.list.iter().rposition(|c| c.press() == Press::No).filter(|_| passed) {
                next.list[at] = next.list[at].with(Press::Taken);
            }
            next.at = state.at.max(at.saturating_sub(1));
        }
        O(Observed::Undid { asked, took }) => {
            next.list.retain(|c| c.seq() != Some(*took));
            next.left.push(*took);
            next.set(*asked, Press::Asked, Press::No);
        }
        O(Observed::Refused { asked }) => {
            next.list.retain(|c| c.seq() != Some(*asked));
            next.left.push(*asked);
        }
        O(Observed::Listed { at, .. }) if *at <= state.at => {}
        O(Observed::Listed { at, changes }) => {
            (next.at, next.listing) = (*at, *at);
            let listed = |seq: u64| changes.iter().any(|c| c.seq == seq);
            let by = |by: u64| changes.iter().find(|c| c.by == Some(by));
            next.list = next
                .list
                .into_iter()
                .filter_map(|c| match c {
                    Change::App { .. } | Change::Action { .. } => Some(c),
                    Change::Sent { by: b, name, press } => Some(match by(b) {
                        Some(change) => Change::Listed { seq: change.seq, name, press },
                        None => Change::Sent { by: b, name, press },
                    }),
                    Change::Listed { seq, .. } => listed(seq).then_some(c),
                })
                .collect();
            let known: Vec<u64> = next.list.iter().filter_map(Change::seq).collect();
            next.left.retain(|seq| changes.iter().any(|c| c.seq == *seq));
            for change in changes.iter().filter(|c| c.seq > state.seen && !known.contains(&c.seq) && !state.left.contains(&c.seq)) {
                next.push(Change::Listed { seq: change.seq, name: change.name, press: Press::No });
            }
            next.seen = changes.iter().map(|c| c.seq).fold(state.seen, u64::max);
        }
        O(Observed::Stack { at, .. }) if *at < state.stack.at => {}
        O(Observed::Stack { at, entries }) => {
            next.stack.at = *at;
            next.list.retain(|c| c.entry().is_none_or(|seq| entries.contains(&seq)));
            next.stack.left.retain(|seq| entries.contains(seq));
            let known: Vec<u64> = next.list.iter().filter_map(Change::entry).collect();
            for seq in entries.iter().filter(|seq| **seq > state.stack.seen && !known.contains(seq) && !state.stack.left.contains(seq)) {
                next.push(Change::Action { seq: *seq, press: Press::No });
            }
            next.stack.seen = entries.iter().copied().fold(state.stack.seen, u64::max);
        }
        O(Observed::ActionAnswered { asked, took }) => {
            if let Some(took) = took {
                next.list.retain(|c| c.entry() != Some(*took));
                next.stack.left.push(*took);
            }
            next.set_entry(*asked, Press::Asked, Press::No);
        }
    }
    Ok(next)
}

impl State {
    /// `change`, the newest; past the last [`KEPT`] settings changes, the oldest of them goes.
    fn push(&mut self, change: Change) {
        self.list.push(change);
        if self.list.iter().filter(|c| c.entry().is_none()).count() > KEPT
            && let Some(oldest) = self.list.iter().position(|c| c.entry().is_none())
        {
            self.list.remove(oldest);
        }
    }

    /// The torrent action `seq`, `from` taken so, now `to`.
    fn set_entry(&mut self, seq: u64, from: Press, to: Press) {
        for c in &mut self.list {
            if c.entry() == Some(seq) && c.press() == from {
                *c = c.with(to);
            }
        }
    }

    /// The listed change `seq`, `from` taken so, now `to`.
    fn set(&mut self, seq: u64, from: Press, to: Press) {
        for c in &mut self.list {
            if c.seq() == Some(seq) && c.press() == from {
                *c = c.with(to);
            }
        }
    }
}

/// What to carry out now: the newest change taken and not carried out yet, if it can be (a change
/// sent and not listed waits).
pub fn next(state: &State) -> Option<Do> {
    state.list.iter().rev().find_map(|c| match c {
        Change::App { id, press: Press::Taken, .. } => Some(Do::Undo { id: *id }),
        Change::Listed { seq, press: Press::Taken, .. } => Some(Do::Ask { seq: *seq }),
        Change::Action { seq, press: Press::Taken } => Some(Do::AskAction { seq: *seq }),
        Change::App { press: Press::No | Press::Asked, .. }
        | Change::Listed { press: Press::No | Press::Asked, .. }
        | Change::Action { press: Press::No | Press::Asked, .. }
        | Change::Sent { .. } => None,
    })
}

/// The list as a [`Machine`](lifecycle::machine::Machine). Only ever named, as a type.
#[allow(dead_code)]
pub struct Changes;

impl lifecycle::machine::Machine for Changes {
    type State = State;
    type Event = Event;
    type Refusal = Nothing;
    type World = ();
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Nothing> {
        decide(state, event)
    }

    fn next(state: &State, _: &()) -> Option<Do> {
        next(state)
    }
}

#[cfg(test)]
mod tests;
