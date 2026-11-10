//! The error list as a machine: what it holds and how events change it. Pure; [`super::ErrorList`]
//! carries it out.
//!
//! An entry is Shown, then Leaving once dismissed (it fades, taking nothing's place yet), then
//! gone when its fade is over, so the entries below it move up only once nothing of it is drawn.
//!
//! | Event \ Entry | Shown            | Leaving                | (no such entry)        |
//! |---------------|------------------|------------------------|------------------------|
//! | Dismiss(id)   | Leaving          | refused                | refused: no such entry |
//! | FadedOut(id)  | refused          | gone                   | refused: no such entry |
//!
//! | State \ Event | Report(n) | ClearAll / Escape                              |
//! |---------------|-----------|------------------------------------------------|
//! | no Shown      | appended  | refused: nothing to clear                      |
//! | some Shown    | appended  | every Shown entry becomes Leaving              |
//!
//! The list is open exactly while it has entries, Leaving ones too ([`next`] opens and closes it).

use crate::notice::Notice;

pub type EntryId = u64;

/// One thing that went wrong, as the user reads it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub notice: Notice,
    /// Dismissed, and fading until it's gone.
    pub leaving: bool,
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
    /// A leaving entry has faded out.
    FadedOut(EntryId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Dismissed already, or never there: a click that raced another.
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
            next.entries.push(Entry { id: next.next_id, notice: notice.clone(), leaving: false });
            next.next_id += 1;
        }
        Event::Command(Command::Dismiss(id)) => {
            let entry = next.entries.iter_mut().find(|e| e.id == *id && !e.leaving).ok_or(Refusal::NoSuchEntry(*id))?;
            entry.leaving = true;
        }
        Event::Observed(Observed::FadedOut(id)) => {
            let at = next.entries.iter().position(|e| e.id == *id && e.leaving).ok_or(Refusal::NoSuchEntry(*id))?;
            next.entries.remove(at);
        }
        Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed) => {
            if next.entries.iter().all(|e| e.leaving) {
                return Err(Refusal::NothingToClear);
            }
            next.entries.iter_mut().for_each(|e| e.leaving = true);
        }
    }
    Ok(next)
}

/// The error list as a [`Machine`](lifecycle::machine::Machine): [`decide`] and [`next`], given
/// whether the panel is open. Only ever named, as a type.
#[allow(dead_code)]
pub struct Errors;

impl lifecycle::machine::Machine for Errors {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = bool;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(state: &State, open: &bool) -> Option<Do> {
        next(state, *open)
    }
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

    fn report(state: &State, short: &str) -> State {
        decide(state, &Event::Command(Command::Report(Notice::new(short)))).unwrap()
    }

    /// `shown` entries Shown, then `leaving` more reported and dismissed.
    fn state(shown: usize, leaving: usize) -> State {
        let mut state = State::default();
        for i in 0..shown + leaving {
            state = report(&state, &format!("error {i}"));
        }
        for i in shown..shown + leaving {
            state = decide(&state, &Event::Command(Command::Dismiss(i as EntryId))).unwrap();
        }
        state
    }

    fn ids(state: &State, leaving: bool) -> Vec<EntryId> {
        state.entries.iter().filter(|e| e.leaving == leaving).map(|e| e.id).collect()
    }

    fn all(state: &State) -> Vec<EntryId> {
        state.entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn every_state_and_event_is_decided_by_its_row() {
        for shown in 0..3 {
            for leaving in 0..3 {
                let before = state(shown, leaving);
                let unknown = 100;
                let mut events = vec![
                    Event::Command(Command::Report(Notice::new("new"))),
                    Event::Command(Command::ClearAll),
                    Event::Observed(Observed::EscapePressed),
                    Event::Command(Command::Dismiss(unknown)),
                    Event::Observed(Observed::FadedOut(unknown)),
                ];
                for id in all(&before) {
                    events.push(Event::Command(Command::Dismiss(id)));
                    events.push(Event::Observed(Observed::FadedOut(id)));
                }
                for event in events {
                    let after = decide(&before, &event);
                    match (&event, after) {
                        (Event::Command(Command::Report(_)), Ok(after)) => {
                            assert_eq!(&all(&after)[..before.entries.len()], &all(&before)[..], "Report appends after the rest");
                            assert_eq!(after.entries.len(), before.entries.len() + 1);
                            let new = after.entries.last().unwrap();
                            assert!(!new.leaving && !all(&before).contains(&new.id), "Shown, under an id of its own");
                        }
                        (Event::Command(Command::Dismiss(id)), Ok(after)) => {
                            assert!(ids(&before, false).contains(id), "only a Shown entry is dismissed");
                            assert_eq!(all(&after), all(&before), "it stays, leaving");
                            assert!(ids(&after, true).contains(id) && ids(&after, false).len() + 1 == ids(&before, false).len());
                        }
                        (Event::Command(Command::Dismiss(id)) | Event::Observed(Observed::FadedOut(id)), Err(refusal)) => {
                            assert_eq!(refusal, Refusal::NoSuchEntry(*id));
                            let is_dismiss = matches!(event, Event::Command(_));
                            let fits = if is_dismiss { ids(&before, false).contains(id) } else { ids(&before, true).contains(id) };
                            assert!(!fits, "refused only for an entry in the wrong phase, or none");
                        }
                        (Event::Observed(Observed::FadedOut(id)), Ok(after)) => {
                            assert!(ids(&before, true).contains(id), "only a Leaving entry goes");
                            assert_eq!(all(&after), all(&before).into_iter().filter(|i| i != id).collect::<Vec<_>>());
                        }
                        (Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed), Ok(after)) => {
                            assert!(shown > 0);
                            assert_eq!(all(&after), all(&before), "they stay, leaving");
                            assert!(ids(&after, false).is_empty(), "none is left Shown");
                        }
                        (Event::Command(Command::ClearAll) | Event::Observed(Observed::EscapePressed), Err(refusal)) => {
                            assert_eq!((refusal, shown), (Refusal::NothingToClear, 0), "refused only with none Shown");
                        }
                        (event, after) => panic!("{shown}+{leaving} entries, {event:?}: {after:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn an_id_is_never_reused() {
        let first = state(1, 0);
        let leaving = decide(&first, &Event::Command(Command::Dismiss(0))).unwrap();
        let gone = decide(&leaving, &Event::Observed(Observed::FadedOut(0))).unwrap();
        assert_ne!(report(&gone, "again").entries[0].id, 0);
    }

    #[test]
    fn the_panel_is_open_exactly_while_the_list_has_entries() {
        for shown in 0..2 {
            for leaving in 0..2 {
                for open in [false, true] {
                    let has = shown + leaving > 0;
                    match next(&state(shown, leaving), open) {
                        Some(Do::Open) => assert!(has && !open),
                        Some(Do::Close) => assert!(!has && open),
                        None => assert_eq!(has, open, "already as it should be"),
                    }
                }
            }
        }
    }
}
