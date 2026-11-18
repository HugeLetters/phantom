//! Which settings change ⌘Z in Settings takes (#221), as a machine. Pure; [`super`] carries it out.
//!
//! The app keeps one list of the settings changes it knows, in the order it learned of them, newest
//! last, the last [`KEPT`]: its own as they're made (an app setting's, which it undoes itself, and a
//! service setting's, the moment it's sent), and another client's as the service lists them. ⌘Z
//! takes the newest not taken yet. An app setting's it undoes at once. A service setting's it asks
//! the service to undo (`UndoSetting`), which always undoes its own newest, so the service's
//! settings go back in the order it made them, even when another client's change came in between
//! the press and its answer: the answer says which it undid. One it sent and the service hasn't
//! listed yet waits for that.
//!
//! No numbers are compared: the app's list is the order, and the service's ids only say which
//! change is which. A change the service no longer lists (undone by another client, pushed past the
//! last 20, or gone with a service that stopped) leaves the list.
//!
//! | Event \ the change it's about | App | Sent | Listed |
//! |---|---|---|---|
//! | Changed / Sent | added newest | added newest | |
//! | Press, the newest not taken | taken (Undo it) | taken: waits until listed | taken (Ask) |
//! | Asked | | | asked |
//! | AppUndone | gone | | |
//! | Answered (its SetSetting) | | gone unless listed | |
//! | Undid { asked, took } | | | `took` gone; `asked` not taken again if it isn't `took` |
//! | Refused { asked } | | | gone |
//! | Listed(changes) | | listed when `by` it | gone unless listed; another client's added, newest |

use proto::{SettingChange, SettingName};

/// How many changes the list keeps; the oldest goes.
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
}

impl Change {
    pub fn press(&self) -> Press {
        match self {
            Change::App { press, .. } | Change::Sent { press, .. } | Change::Listed { press, .. } => *press,
        }
    }

    fn with(&self, press: Press) -> Change {
        match self.clone() {
            Change::App { id, of, .. } => Change::App { id, of, press },
            Change::Sent { by, name, .. } => Change::Sent { by, name, press },
            Change::Listed { seq, name, .. } => Change::Listed { seq, name, press },
        }
    }

    /// What Edit ▸ Undo names it by.
    pub fn of(&self) -> Of {
        match self {
            Change::App { of, .. } => Of::App(*of),
            Change::Sent { name, .. } | Change::Listed { name, .. } => Of::Service(*name),
        }
    }

    fn seq(&self) -> Option<u64> {
        match self {
            Change::Listed { seq, .. } => Some(*seq),
            Change::App { .. } | Change::Sent { .. } => None,
        }
    }
}

/// The setting a change is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Of {
    App(AppSetting),
    Service(SettingName),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// Oldest first.
    pub list: Vec<Change>,
    /// The largest `seq` the service has listed: a change listed again after it left (it was
    /// refused while the stream still had it) isn't added back.
    pub seen: u64,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The app undid its change `id`.
    AppUndone { id: u64 },
    /// The answer to the change sent `by` this came.
    Answered { by: u64 },
    /// The service undid `took`, asked for `asked`.
    Undid { asked: u64, took: u64 },
    /// The service refused the undo asked for `asked` (or had nothing to undo).
    Refused { asked: u64 },
    /// The service's changes, as the latest settings it sent list them, oldest first.
    Listed(Vec<SettingChange>),
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
        O(Observed::AppUndone { id }) => next.list.retain(|c| !matches!(c, Change::App { id: i, .. } if i == id)),
        O(Observed::Answered { by }) => next.list.retain(|c| !matches!(c, Change::Sent { by: b, .. } if b == by)),
        O(Observed::Undid { asked, took }) => {
            next.list.retain(|c| c.seq() != Some(*took));
            next.set(*asked, Press::Asked, Press::No);
        }
        O(Observed::Refused { asked }) => next.list.retain(|c| c.seq() != Some(*asked)),
        O(Observed::Listed(changes)) => {
            let listed = |seq: u64| changes.iter().any(|c| c.seq == seq);
            let by = |by: u64| changes.iter().find(|c| c.by == Some(by));
            next.list = next
                .list
                .into_iter()
                .filter_map(|c| match c {
                    Change::App { .. } => Some(c),
                    Change::Sent { by: b, name, press } => Some(match by(b) {
                        Some(change) => Change::Listed { seq: change.seq, name, press },
                        None => Change::Sent { by: b, name, press },
                    }),
                    Change::Listed { seq, .. } => listed(seq).then_some(c),
                })
                .collect();
            let known: Vec<u64> = next.list.iter().filter_map(Change::seq).collect();
            for change in changes.iter().filter(|c| c.seq > state.seen && !known.contains(&c.seq)) {
                next.push(Change::Listed { seq: change.seq, name: change.name, press: Press::No });
            }
            next.seen = changes.iter().map(|c| c.seq).fold(state.seen, u64::max);
        }
    }
    Ok(next)
}

impl State {
    fn push(&mut self, change: Change) {
        self.list.push(change);
        if self.list.len() > KEPT {
            self.list.remove(0);
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
        Change::App { press: Press::No | Press::Asked, .. }
        | Change::Listed { press: Press::No | Press::Asked, .. }
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
