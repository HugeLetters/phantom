//! The details pane as a machine (#111): which torrent it shows, and which it waits for. Pure;
//! the model carries it out, and the pane draws only from the snapshot its decision was on.
//!
//! | State \ Event          | Select(None) | Select(w)                       | Snapshot                                       |
//! |------------------------|--------------|---------------------------------|------------------------------------------------|
//! | Empty                  | Empty        | Waiting { w }                   | Empty                                          |
//! | Waiting { w }          | Empty        | Waiting { w }                   | Showing { w } once w's details are in it       |
//! | Showing { s }          | Empty        | Showing { s } if w is s, else Switching { s, w } | Waiting { s } if s's details aren't |
//! | Switching { s, w }     | Empty        | as Showing { s }                | Showing { w } once w's are in it; else as Showing { s } |
//!
//! While switching it keeps watching the shown torrent, live, until the wanted one's details
//! arrive: nothing goes blank, nothing flickers, and there's no timeout. A torrent gone from the
//! list is neither shown nor waited for.

use std::collections::BTreeSet;
use std::convert::Infallible;

use proto::TorrentId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Not exactly one torrent selected.
    Empty,
    /// `wanted` is selected; its details haven't come (yet: a torrent fetching info has none).
    Waiting {
        wanted: TorrentId,
    },
    Showing {
        shown: TorrentId,
    },
    /// `wanted` is selected and on its way; `shown` stays, live, until it comes.
    Switching {
        shown: TorrentId,
        wanted: TorrentId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The one torrent selected, or none (none or several).
    Select(Option<TorrentId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// A snapshot came: the torrents it lists, and those it has the details of.
    Snapshot { listed: BTreeSet<TorrentId>, detailed: BTreeSet<TorrentId> },
}

/// What the model carries out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Do {
    /// Have the service send the details of these torrents.
    Watch(Vec<TorrentId>),
}

impl State {
    /// The torrent whose details the pane draws.
    pub fn shown(&self) -> Option<TorrentId> {
        match self {
            State::Showing { shown } | State::Switching { shown, .. } => Some(*shown),
            State::Empty | State::Waiting { .. } => None,
        }
    }

    /// What it needs the details of.
    pub fn watching(&self) -> Vec<TorrentId> {
        match self {
            State::Empty => vec![],
            State::Waiting { wanted } => vec![*wanted],
            State::Showing { shown } => vec![*shown],
            State::Switching { shown, wanted } => vec![*shown, *wanted],
        }
    }
}

/// What `event` does to the pane. Never refused.
pub fn decide(state: &State, event: &Event) -> State {
    use Command::Select;
    use Event::{Command as C, Observed as O};
    use State::*;
    match (state, event) {
        (Empty | Waiting { .. } | Showing { .. } | Switching { .. }, C(Select(None))) => Empty,
        (Empty | Waiting { .. }, C(Select(Some(wanted)))) => Waiting { wanted: *wanted },
        (Showing { shown } | Switching { shown, .. }, C(Select(Some(wanted)))) if wanted == shown => Showing { shown: *shown },
        (Showing { shown } | Switching { shown, .. }, C(Select(Some(wanted)))) => Switching { shown: *shown, wanted: *wanted },
        (Empty, O(Observed::Snapshot { .. })) => Empty,
        (Waiting { wanted }, O(Observed::Snapshot { listed, detailed })) => arrived(*wanted, listed, detailed).unwrap_or(state.clone()),
        (Showing { shown }, O(Observed::Snapshot { listed, detailed })) => still(*shown, listed, detailed),
        (Switching { shown, wanted }, O(Observed::Snapshot { listed, detailed })) => {
            arrived(*wanted, listed, detailed).unwrap_or_else(|| match still(*shown, listed, detailed) {
                // The shown one can't stay: wait for the wanted one, if it's still there.
                Showing { .. } => state.clone(),
                Empty | Waiting { .. } | Switching { .. } if listed.contains(wanted) => Waiting { wanted: *wanted },
                Empty | Waiting { .. } | Switching { .. } => Empty,
            })
        }
    }
}

/// `wanted` shown once its details are in the snapshot, or nothing if it's gone from the list;
/// `None`: still on its way.
fn arrived(wanted: TorrentId, listed: &BTreeSet<TorrentId>, detailed: &BTreeSet<TorrentId>) -> Option<State> {
    match (detailed.contains(&wanted), listed.contains(&wanted)) {
        (true, _) => Some(State::Showing { shown: wanted }),
        (false, true) => None,
        (false, false) => Some(State::Empty),
    }
}

/// `shown` goes on showing while its details are in the snapshot; waited for while it's listed
/// without them; gone from the list, nothing.
fn still(shown: TorrentId, listed: &BTreeSet<TorrentId>, detailed: &BTreeSet<TorrentId>) -> State {
    match (detailed.contains(&shown), listed.contains(&shown)) {
        (true, _) => State::Showing { shown },
        (false, true) => State::Waiting { wanted: shown },
        (false, false) => State::Empty,
    }
}

/// What makes the service send what the pane needs, given what it was last told to watch.
pub fn next(state: &State, watching: &[TorrentId]) -> Option<Do> {
    let wanted = state.watching();
    (wanted != watching).then_some(Do::Watch(wanted))
}

/// The pane as a [`Machine`](lifecycle::machine::Machine), given what the service watches. Only
/// ever named, as a type.
#[allow(dead_code)]
pub struct Pane;

impl lifecycle::machine::Machine for Pane {
    type State = State;
    type Event = Event;
    type Refusal = Infallible;
    type World = Vec<TorrentId>;
    type Do = Do;

    fn decide(state: &State, event: &Event) -> Result<State, Infallible> {
        Ok(decide(state, event))
    }

    fn next(state: &State, watching: &Vec<TorrentId>) -> Option<Do> {
        next(state, watching)
    }
}

#[cfg(test)]
mod tests {
    use lifecycle::every;
    use lifecycle::machine::{DecideRow, Row, decide_table, sequences};

    use super::*;

    // Torrents 1, 2 and 3; snapshots listing some, with the details of some of those.

    every!(State {
        State::Empty => [State::Empty],
        State::Waiting { .. } => [State::Waiting { wanted: 1 }],
        State::Showing { .. } => [State::Showing { shown: 1 }],
        State::Switching { .. } => [State::Switching { shown: 1, wanted: 2 }],
    });

    every!(Command { Command::Select(_) => [Command::Select(None), Command::Select(Some(1)), Command::Select(Some(2)), Command::Select(Some(3))] });

    every!(Observed {
        Observed::Snapshot { .. } => {
            let ids = |ids: &[TorrentId]| ids.iter().copied().collect::<BTreeSet<_>>();
            let subsets: [&[TorrentId]; 5] = [&[], &[1], &[2], &[1, 2], &[1, 2, 3]];
            let mut all = Vec::new();
            for listed in subsets {
                for detailed in subsets.iter().filter(|d| d.iter().all(|id| listed.contains(id))) {
                    all.push(Observed::Snapshot { listed: ids(listed), detailed: ids(detailed) });
                }
            }
            all
        },
    });

    every!(Event {
        Event::Command(_) => <Command as lifecycle::machine::Every>::every().into_iter().map(Event::Command),
        Event::Observed(_) => <Observed as lifecycle::machine::Every>::every().into_iter().map(Event::Observed),
    });

    type R = DecideRow<Pane>;

    fn selected(e: &Event) -> Option<Option<TorrentId>> {
        match e {
            Event::Command(Command::Select(w)) => Some(*w),
            Event::Observed(_) => None,
        }
    }

    fn snapshot(e: &Event) -> Option<(&BTreeSet<TorrentId>, &BTreeSet<TorrentId>)> {
        match e {
            Event::Observed(Observed::Snapshot { listed, detailed }) => Some((listed, detailed)),
            Event::Command(_) => None,
        }
    }

    fn rows() -> Vec<R> {
        vec![
            Row {
                says: "nothing selected empties the pane",
                when: |_, e| selected(e) == Some(None),
                then: |_, _, r| *r == Ok(State::Empty),
            },
            Row {
                says: "a torrent selected is what the pane wants, and what it shows stays until it comes",
                when: |_, e| matches!(selected(e), Some(Some(_))),
                then: |s, e, r| {
                    let Some(Some(w)) = selected(e) else { unreachable!() };
                    let Ok(n) = r else { return false };
                    let wants =
                        matches!(n, State::Waiting { wanted } | State::Switching { wanted, .. } if *wanted == w) || n.shown() == Some(w);
                    wants && n.shown().is_none_or(|shown| s.shown() == Some(shown))
                },
            },
            Row {
                says: "a snapshot leaves the pane showing only what it has the details of, wanting only what it lists",
                when: |_, e| snapshot(e).is_some(),
                then: |s, e, r| {
                    let (listed, detailed) = snapshot(e).unwrap();
                    let Ok(n) = r else { return false };
                    let wanted = |s: &State| match s {
                        State::Waiting { wanted } | State::Switching { wanted, .. } => Some(*wanted),
                        State::Empty | State::Showing { .. } => None,
                    };
                    // What it wanted and could show, it shows; otherwise it keeps wanting it, if listed.
                    let goal = wanted(s).or(s.shown());
                    n.shown().is_none_or(|shown| detailed.contains(&shown))
                        && wanted(n).is_none_or(|w| listed.contains(&w) && !detailed.contains(&w))
                        && goal.is_none_or(|g| !listed.contains(&g) || n.shown() == Some(g) || wanted(n) == Some(g))
                },
            },
        ]
    }

    fn always() -> Vec<R> {
        vec![Row::always("it watches what it shows and what it wants, and no more", |_, _, r| {
            r.as_ref().is_ok_and(|n| {
                n.watching().iter().copied().collect::<BTreeSet<_>>()
                    == n.shown()
                        .into_iter()
                        .chain(match n {
                            State::Waiting { wanted } | State::Switching { wanted, .. } => Some(*wanted),
                            State::Empty | State::Showing { .. } => None,
                        })
                        .collect()
            })
        })]
    }

    #[test]
    fn every_state_and_event_is_decided_as_its_row_says() {
        decide_table::<Pane>(&rows(), &always());
    }

    #[test]
    fn every_state_reached_from_an_empty_pane_is_decided_as_its_row_says() {
        sequences::<Pane>(vec![State::Empty], 20, &rows(), &always());
    }

    #[test]
    fn it_asks_for_what_it_needs_only_when_that_changes() {
        for s in <State as lifecycle::machine::Every>::every() {
            assert_eq!(next(&s, &s.watching()), None, "{s:?}");
            assert_eq!(next(&s, &[9]), (s.watching() != [9]).then(|| Do::Watch(s.watching())), "{s:?}");
        }
    }
}
