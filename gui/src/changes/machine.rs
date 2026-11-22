//! Which change ⌘Z takes (#221, #255), as a machine. Pure; [`super`] carries it out.
//!
//! The app keeps one list of the changes it knows, in the order it learned of them, newest last:
//! the settings changes, the last [`KEPT`], its own as they're made (an app setting's, which it
//! undoes itself, and a service setting's, the moment it's sent), and another client's as the
//! service lists them; and the torrent actions (Remove, Delete, Move to Trash) (#255), its own as
//! they're sent, another client's as the snapshots list the undo stack. ⌘Z, in the main window or in
//! Settings, takes the newest not taken yet. An app setting's it undoes at once. A service setting's it asks
//! the service to undo (`UndoSetting`), which always undoes its own newest, so the service's
//! settings go back in the order it made them, even when another client's change came in between
//! the press and its answer: the answer says which it undid. One it sent waits until the service
//! names it: in a snapshot (by the app's id for it) or in its answer, whichever comes first; it keeps
//! its place and a press either way.
//!
//! A torrent action the app sends is pending until the stack lists its entry (the first after the
//! entries the app knew when it sent it, of one of its torrents) or its answer names it, whichever
//! comes first; it keeps its place and a press either way, as a sent setting's change does. A torrent
//! action's undo the app asks of the service (`Undo`), which, likewise, undoes the top of its stack
//! and says which entry it took, undone or refused: so the actions go back in the order of the
//! stack. A refused one is gone (ADR 0008); an answer that names none (nothing to undo, the
//! connection lost) leaves the entry to be taken again. The stack is taken from the newest snapshot
//! only, by its `(epoch, version)`, and outlasts the service restarting. Of another client's
//! settings change and torrent action that one snapshot lists first, the settings change counts as
//! the older: the snapshot doesn't say which came first.
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
//! | Edited { id, of } (a step in the open sheet) | added newest, held out of the last 20 | | |
//! | Press, the newest not taken | taken (Undo it) | taken: waits until listed | taken (Ask) |
//! | Asked | | | asked |
//! | AppUndone | gone | | |
//! | Answered (its SetSetting) | | listed, in place, when it made one the service may still have; else gone, its press, if taken, on the newest not taken | |
//! | Undid { asked, took, by } | | `by` it: gone, its press, if taken, on the newest not taken | `took` gone, never added back; `asked`, if it isn't `took`, untaken: ⌘Z can take it again |
//! | Refused { asked } | | | gone, never added back |
//! | Listed { at, changes }, newer | | listed when `by` it | gone unless listed; another client's added, newest |
//! | Listed, not newer | | | |
//!
//! | PressApp { id } (⌘Z in a Look Preset's editor, or the App icon's sheet) | taken, if not yet; else nothing | | |
//! | Fold { ids, into } (the sheet closed) | `ids` gone; `into`, if any, added newest | | |
//!
//! | Event \ the torrent action it's about | Pending | Action |
//! |---|---|---|
//! | Acted (the app sent one) | added newest | |
//! | Press, the newest not taken | taken: waits until listed | taken (AskAction) |
//! | PressAction { seq } (its notice's Undo) | | taken, if not yet; else nothing |
//! | AskedAction | | asked |
//! | Made { id, seq } (its command's answer) | listed, in place, unless it made none or its entry was undone: then gone, its press, if taken, on the newest not taken | |
//! | ActionAnswered { asked, took } | `took`'s (the first sent before it, of one of its torrents): gone, its press, if taken, on the newest not taken | `took` gone, never added back; `asked`, if it isn't `took`, untaken |
//! | Stack { at, entries }, not older | listed, in place, at its entry | gone unless listed; another client's added, newest, in the stack's order, but those an answer took off |
//! | Stack, older | | |
//!
//! Entries are told apart by their seq alone, and none is hidden for being older than one listed
//! before: a service that lost its `undo.json` numbers them afresh, and its actions are listed all
//! the same.

use proto::{SettingChange, SettingName};

/// How many settings changes the list keeps; the oldest goes. The torrent actions are those the
/// service's stack lists, as many as it keeps.
pub const KEPT: usize = SettingChange::KEPT;

/// An app setting: the App icon, a theme setting, by its key and label, the Look Presets (a
/// pick from a saved one, or a change of the saved ones), named by what the change kept, or a
/// Look Preset's editor's changes, folded into one by its Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSetting {
    Icon,
    Theme { key: &'static str, label: &'static str },
    Presets,
    Edit,
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
    /// A torrent action the app sent as `id`, on `torrents`, when the newest entry it knew was
    /// `after`; not listed yet.
    Pending { id: u64, after: u64, torrents: Vec<u64>, press: Press },
}

/// An entry of the torrents' undo stack: its seq, and the torrents it's of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub seq: u64,
    pub torrents: Vec<u64>,
}

impl Entry {
    /// Whether it's the one `change`, a torrent action the app sent, made: after the entries the
    /// app knew then, of one of its torrents.
    pub fn made(&self, change: &Change) -> bool {
        match change {
            Change::Pending { after, torrents, .. } => self.seq > *after && self.torrents.iter().any(|t| torrents.contains(t)),
            Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } | Change::Action { .. } => false,
        }
    }
}

impl Change {
    pub fn press(&self) -> Press {
        match self {
            Change::App { press, .. }
            | Change::Sent { press, .. }
            | Change::Listed { press, .. }
            | Change::Action { press, .. }
            | Change::Pending { press, .. } => *press,
        }
    }

    pub fn with(&self, press: Press) -> Change {
        match self.clone() {
            Change::App { id, of, .. } => Change::App { id, of, press },
            Change::Sent { by, name, .. } => Change::Sent { by, name, press },
            Change::Listed { seq, name, .. } => Change::Listed { seq, name, press },
            Change::Action { seq, .. } => Change::Action { seq, press },
            Change::Pending { id, after, torrents, .. } => Change::Pending { id, after, torrents, press },
        }
    }

    /// What Edit ▸ Undo names it by.
    pub fn of(&self) -> Of {
        match self {
            Change::App { of, .. } => Of::App(*of),
            Change::Sent { name, .. } | Change::Listed { name, .. } => Of::Service(*name),
            Change::Action { seq, .. } => Of::Action(*seq),
            Change::Pending { .. } => Of::Pending,
        }
    }

    /// A service setting's change, as the service lists it: its `seq`.
    fn seq(&self) -> Option<u64> {
        match self {
            Change::Listed { seq, .. } => Some(*seq),
            Change::App { .. } | Change::Sent { .. } | Change::Action { .. } | Change::Pending { .. } => None,
        }
    }

    /// A torrent action: its entry on the stack.
    pub fn entry(&self) -> Option<u64> {
        match self {
            Change::Action { seq, .. } => Some(*seq),
            Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } | Change::Pending { .. } => None,
        }
    }

    /// A settings change, not a torrent action.
    pub fn is_setting(&self) -> bool {
        match self {
            Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } => true,
            Change::Action { .. } | Change::Pending { .. } => false,
        }
    }
}

/// What a change is of: a setting, or the torrent action of an entry on the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Of {
    App(AppSetting),
    Service(SettingName),
    Action(u64),
    /// A torrent action the app sent, not listed yet.
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// Oldest first.
    pub list: Vec<Change>,
    /// The app changes in `list` made in an open sheet ([`Command::Edited`]): they don't count
    /// toward the last [`KEPT`].
    pub held: Vec<u64>,
    /// The largest `seq` the service has listed: a change listed again after it left (it was
    /// refused while the stream still had it) isn't added back.
    pub seen: u64,
    /// Settings no newer than this [`proto::Configured::at`] change nothing: those of the listing
    /// taken last, or from before a change an answer named (right after it, they're news).
    pub at: u64,
    /// The `at` of the listing taken last: a change from before it that it didn't list is gone.
    pub listing: u64,
    /// The service's changes an answer took off the list (undone, or a refused undo's), until a
    /// listing lacks them and lists a newer change (so is from after they left, not from before
    /// they were made): one from before the answer still has them, and mustn't add them back.
    pub left: Vec<u64>,
    /// The torrents' undo stack, as the snapshots list it.
    pub stack: Stack,
}

/// What the app has taken of the service's undo stack, by the snapshots' own count.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stack {
    /// The `(epoch, version)` of the snapshot taken last: an older one changes nothing.
    pub at: (u64, u64),
    /// The entries an answer took off the list, until a listing lacks them and lists a newer entry
    /// or is of another service run (so is from after they left, not from before they were made):
    /// one from before the answer still has them, and mustn't add them back.
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
    /// A step in an open sheet (a Look Preset's editor, the App icon's): as `Changed`, but kept
    /// out of the last [`KEPT`] until the sheet closes ([`Command::Fold`]), so it pushes no older change off the list.
    Edited { id: u64, of: AppSetting },
    /// A service setting's change was sent, `by` the app's id for it.
    Sent { by: u64, name: SettingName },
    /// ⌘Z in Settings.
    Press,
    /// The service was asked to undo, for the change `seq`.
    Asked { seq: u64 },
    /// The app sent a torrent action as `id`, on `torrents`, the newest entry it knew being `after`.
    Acted { id: u64, after: u64, torrents: Vec<u64> },
    /// The Undo in the notice of the torrent action `seq`.
    PressAction { seq: u64 },
    /// The service was asked to undo its top torrent action, for the entry `seq`.
    AskedAction { seq: u64 },
    /// ⌘Z in an open sheet (a Look Preset's editor, the App icon's): its newest change, the app's `id` (#87).
    PressApp { id: u64 },
    /// The sheet closed: its changes `ids` leave; Done folds them into one, `into`, the newest,
    /// if they changed anything (the editor's Cancel, none).
    Fold { ids: Vec<u64>, into: Option<(u64, AppSetting)> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// The app undid its change `id`.
    AppUndone { id: u64 },
    /// The answer to the change sent `by` this came: the change it made (its `seq`), if any, and
    /// the settings' `at` right after it.
    Answered { by: u64, seq: Option<u64>, at: u64 },
    /// The service undid `took`, asked for `asked`; `by` the app's id for it, if the app sent it.
    Undid { asked: u64, took: u64, by: Option<u64> },
    /// The service refused the undo asked for `asked` (or had nothing to undo).
    Refused { asked: u64 },
    /// The service's changes, as settings it sent at `at` list them, oldest first.
    Listed { at: u64, changes: Vec<SettingChange> },
    /// The service's undo stack, as the snapshot `at` (its epoch and version) lists it, oldest
    /// first.
    Stack { at: (u64, u64), entries: Vec<Entry> },
    /// The answer to the torrent action sent as `id`: the entry it made, if it made one.
    Made { id: u64, seq: Option<u64> },
    /// The answer to the undo asked for the entry `asked`: the entry the service took (undone, or
    /// refused), or none (nothing to undo, or no answer).
    ActionAnswered { asked: u64, took: Option<Entry> },
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
        C(Command::Edited { id, of }) => {
            next.held.push(*id);
            next.push(Change::App { id: *id, of: *of, press: Press::No });
        }
        C(Command::Sent { by, name }) => next.push(Change::Sent { by: *by, name: *name, press: Press::No }),
        C(Command::Press) => {
            let at = next.list.iter().rposition(|c| c.press() == Press::No).ok_or(Nothing)?;
            next.list[at] = next.list[at].with(Press::Taken);
        }
        C(Command::Asked { seq }) => next.set(*seq, Press::Taken, Press::Asked),
        C(Command::Acted { id, after, torrents }) => {
            next.push(Change::Pending { id: *id, after: *after, torrents: torrents.clone(), press: Press::No })
        }
        C(Command::PressAction { seq }) => {
            let at = next.list.iter().position(|c| c.entry() == Some(*seq) && c.press() == Press::No).ok_or(Nothing)?;
            next.list[at] = next.list[at].with(Press::Taken);
        }
        C(Command::AskedAction { seq }) => next.set_entry(*seq, Press::Taken, Press::Asked),
        C(Command::PressApp { id }) => {
            let at = next.list.iter().position(|c| matches!(c, Change::App { id: i, press: Press::No, .. } if i == id)).ok_or(Nothing)?;
            next.list[at] = next.list[at].with(Press::Taken);
        }
        C(Command::Fold { ids, into }) => {
            next.held.clear();
            next.list.retain(|c| !matches!(c, Change::App { id, .. } if ids.contains(id)));
            if let Some((id, of)) = into {
                next.push(Change::App { id: *id, of: *of, press: Press::No });
            }
        }
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
                    Change::App { .. } | Change::Sent { .. } | Change::Listed { .. } | Change::Action { .. } | Change::Pending { .. } => {
                        Some(c)
                    }
                })
                .collect();
            if passed {
                next.pass();
            }
            next.at = state.at.max(at.saturating_sub(1));
        }
        // The change the app sent, if it's the one undone, goes too, whether listed yet or not:
        // its press, if taken while it waited, goes to the newest change not taken.
        O(Observed::Undid { asked, took, by }) => {
            let sent = |c: &Change| matches!(c, Change::Sent { by: b, .. } if Some(*b) == *by);
            let passed = next.list.iter().any(|c| sent(c) && c.press() != Press::No);
            next.list.retain(|c| c.seq() != Some(*took) && !sent(c));
            next.left.push(*took);
            next.set(*asked, Press::Asked, Press::No);
            if passed {
                next.pass();
            }
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
                    Change::App { .. } | Change::Action { .. } | Change::Pending { .. } => Some(c),
                    Change::Sent { by: b, name, press } => Some(match by(b) {
                        Some(change) => Change::Listed { seq: change.seq, name, press },
                        None => Change::Sent { by: b, name, press },
                    }),
                    Change::Listed { seq, .. } => listed(seq).then_some(c),
                })
                .collect();
            let known: Vec<u64> = next.list.iter().filter_map(Change::seq).collect();
            // Not listing one proves it's gone only if a newer one is listed: they're numbered in the
            // order they're made, so a listing without any newer is from before it was made.
            next.left.retain(|seq| !changes.iter().any(|c| c.seq > *seq) || changes.iter().any(|c| c.seq == *seq));
            for change in changes.iter().filter(|c| c.seq > state.seen && !known.contains(&c.seq) && !state.left.contains(&c.seq)) {
                next.push(Change::Listed { seq: change.seq, name: change.name, press: Press::No });
            }
            next.seen = changes.iter().map(|c| c.seq).fold(state.seen, u64::max);
        }
        O(Observed::Stack { at, .. }) if *at < state.stack.at => {}
        O(Observed::Stack { at, entries }) => {
            let same_service = at.0 == state.stack.at.0;
            next.stack.at = *at;
            let listed = |seq: u64| entries.iter().any(|e| e.seq == seq);
            next.list.retain(|c| c.entry().is_none_or(listed));
            // Not having an entry proves it left only if the snapshot is of another service run (which
            // may number afresh) or lists a newer entry: they're numbered in the order they're made,
            // so a snapshot of this run without any newer is from before it was made.
            next.stack.left.retain(|seq| listed(*seq) || (same_service && !entries.iter().any(|e| e.seq > *seq)));
            // The app's own, at their entries, in place: each the first after what it knew, of one of
            // its torrents, that no other change is.
            for i in 0..next.list.len() {
                let known: Vec<u64> = next.list.iter().filter_map(Change::entry).collect();
                let entry = entries.iter().find(|e| e.made(&next.list[i]) && !known.contains(&e.seq) && !next.stack.left.contains(&e.seq));
                if let Some(entry) = entry {
                    next.list[i] = Change::Action { seq: entry.seq, press: next.list[i].press() };
                }
            }
            let known: Vec<u64> = next.list.iter().filter_map(Change::entry).collect();
            let new: Vec<u64> =
                entries.iter().map(|e| e.seq).filter(|seq| !known.contains(seq) && !next.stack.left.contains(seq)).collect();
            for seq in new {
                next.push(Change::Action { seq, press: Press::No });
            }
        }
        // Listed in place, keeping its press, unless it made none or an answer took its entry
        // already: then its press, if taken, goes to the newest change not taken. (One another
        // client undid before the answer came leaves with the next stack.)
        O(Observed::Made { id, seq }) => {
            let pending = |c: &Change| matches!(c, Change::Pending { id: i, .. } if i == id);
            if let Some(i) = next.list.iter().position(pending) {
                let known = |seq: &u64| state.list.iter().any(|c| c.entry() == Some(*seq)) || state.stack.left.contains(seq);
                match seq.filter(|seq| !known(seq)) {
                    Some(seq) => next.list[i] = Change::Action { seq, press: next.list[i].press() },
                    None => {
                        let passed = next.list.remove(i).press() != Press::No;
                        if passed {
                            next.pass();
                        }
                    }
                }
            }
        }
        // The entry taken goes, listed yet or still pending (the app's own, undone before it
        // learned its entry); a press taken on a pending one goes to the newest change not taken.
        O(Observed::ActionAnswered { asked, took }) => {
            let mut passed = false;
            if let Some(took) = took {
                next.list.retain(|c| c.entry() != Some(took.seq));
                if let Some(i) = next.list.iter().position(|c| took.made(c)) {
                    passed = next.list.remove(i).press() != Press::No;
                }
                next.stack.left.push(took.seq);
            }
            next.set_entry(*asked, Press::Asked, Press::No);
            if passed {
                next.pass();
            }
        }
    }
    Ok(next)
}

impl State {
    /// `change`, the newest; past the last [`KEPT`] settings changes, the oldest of them goes.
    fn push(&mut self, change: Change) {
        self.list.push(change);
        let counts = |c: &Change| c.is_setting() && !self.held.iter().any(|id| matches!(c, Change::App { id: i, .. } if i == id));
        if self.list.iter().filter(|c| counts(c)).count() > KEPT
            && let Some(oldest) = self.list.iter().position(|c| counts(c))
        {
            self.list.remove(oldest);
        }
    }

    /// A press whose change went before it was carried out: the newest change not taken takes it,
    /// so two presses are still two undos.
    fn pass(&mut self) {
        if let Some(at) = self.list.iter().rposition(|c| c.press() == Press::No) {
            self.list[at] = self.list[at].with(Press::Taken);
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
        | Change::Sent { .. }
        | Change::Pending { .. } => None,
    })
}

/// The list as a [`Machine`](machine::Machine). Only ever named, as a type.
#[allow(dead_code)]
pub struct Changes;

impl machine::Machine for Changes {
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
