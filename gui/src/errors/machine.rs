//! The error list as a machine: what it holds and how events change it. Pure; [`super::ErrorList`]
//! carries it out.
//!
//! | State \ Event | Report(n)      | Dismiss(id)         | ClearAll / Escape        |
//! |---------------|----------------|---------------------|--------------------------|
//! | Empty         | one entry      | refused: no such    | refused: nothing to clear |
//! | Listing(n≥1)  | appended last  | that entry goes     | Empty                    |
//!
//! The list is open exactly while it's non-empty ([`next`] opens and closes it).

use crate::notice::Notice;

pub type EntryId = u64;

/// One thing that went wrong, as the user reads it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub notice: Notice,
}

/// The entries, oldest first. Empty or listing; the ids are never reused.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub entries: Vec<Entry>,
    next_id: EntryId,
}

#[derive(Debug, Clone)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

/// What a sender asks for.
#[derive(Debug, Clone)]
pub enum Command {
    Report(Notice),
    Dismiss(EntryId),
    ClearAll,
}

/// What the window saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    EscapePressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Dismissed already: a click that raced another.
    NoSuchEntry(EntryId),
    NothingToClear,
}

/// What the actor carries out: the panel opens when there's something to show, closes when not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Open,
    Close,
}

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    let mut next = state.clone();
    match event {
        Event::Command(Command::Report(notice)) => {
            next.entries.push(Entry { id: next.next_id, notice: notice.clone() });
            next.next_id += 1;
        }
        Event::Command(Command::Dismiss(id)) => {
            let at = next.entries.iter().position(|e| e.id == *id).ok_or(Refusal::NoSuchEntry(*id))?;
            next.entries.remove(at);
        }
        Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed) => {
            if next.entries.is_empty() {
                return Err(Refusal::NothingToClear);
            }
            next.entries.clear();
        }
    }
    Ok(next)
}

/// What to do about the panel, given whether it's `open`: level-triggered.
pub fn next(state: &State, open: bool) -> Option<Do> {
    match (state.entries.is_empty(), open) {
        (false, false) => Some(Do::Open),
        (true, true) => Some(Do::Close),
        (false, true) | (true, false) => None,
    }
}

#[cfg(test)]
mod tests {
    //! The table: every state × event pair, with the property of its row.

    use super::*;

    fn listing(n: usize) -> State {
        let mut state = State::default();
        for i in 0..n {
            state = decide(&state, &Event::Command(Command::Report(Notice::new(format!("error {i}"))))).unwrap();
        }
        state
    }

    fn ids(state: &State) -> Vec<EntryId> {
        state.entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn every_state_and_event_is_decided_by_its_row() {
        for n in 0..4 {
            let before = listing(n);
            let events = [
                Event::Command(Command::Report(Notice::new("new"))),
                Event::Command(Command::ClearAll),
                Event::Observed(Observed::EscapePressed),
                Event::Command(Command::Dismiss(n as EntryId + 100)),
            ]
            .into_iter()
            .chain(ids(&before).into_iter().map(|id| Event::Command(Command::Dismiss(id))));

            for event in events {
                let after = decide(&before, &event);
                match (&event, after) {
                    (Event::Command(Command::Report(_)), Ok(after)) => {
                        assert_eq!(after.entries.len(), n + 1, "Report appends");
                        assert_eq!(&ids(&after)[..n], &ids(&before)[..], "after the others, which stay");
                        assert!(!ids(&before).contains(&after.entries[n].id), "under an id of its own");
                        assert_eq!(after.entries[n].notice.short, "new");
                    }
                    (Event::Command(Command::Dismiss(id)), Ok(after)) => {
                        let expected: Vec<_> = ids(&before).into_iter().filter(|i| i != id).collect();
                        assert_eq!(ids(&after), expected, "Dismiss({id}) removes that entry only");
                        assert!(ids(&before).contains(id));
                    }
                    (Event::Command(Command::Dismiss(id)), Err(refusal)) => {
                        assert_eq!(refusal, Refusal::NoSuchEntry(*id));
                        assert!(!ids(&before).contains(id), "refused only for an entry that isn't there");
                    }
                    (Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed), Ok(after)) => {
                        assert!(after.entries.is_empty(), "everything goes");
                        assert!(n > 0);
                    }
                    (Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed), Err(refusal)) => {
                        assert_eq!((refusal, n), (Refusal::NothingToClear, 0), "refused only when empty");
                    }
                    (event, after) => panic!("{n} entries, {event:?}: {after:?}"),
                }
            }
        }
    }

    #[test]
    fn an_id_is_never_reused() {
        let first = listing(1);
        let dismissed = decide(&first, &Event::Command(Command::Dismiss(first.entries[0].id))).unwrap();
        let again = decide(&dismissed, &Event::Command(Command::Report(Notice::new("again")))).unwrap();
        assert_ne!(again.entries[0].id, first.entries[0].id);
    }

    #[test]
    fn the_panel_is_open_exactly_while_the_list_has_entries() {
        for n in 0..3 {
            for open in [false, true] {
                let state = listing(n);
                match next(&state, open) {
                    Some(Do::Open) => assert!(n > 0 && !open),
                    Some(Do::Close) => assert!(n == 0 && open),
                    None => assert_eq!(n > 0, open, "already as it should be"),
                }
            }
        }
    }
}
