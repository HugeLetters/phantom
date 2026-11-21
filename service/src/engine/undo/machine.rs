//! The undo stack's machine is the shared one ([`lifecycle::undo`], #150, #190), which the GUI's fake
//! service runs too, over the engine's own [`Running`] steps and [`Entry`]s.

pub use lifecycle::undo::{Arrival, Command, DEPTH, Do, Phase, World, decide, next};
use proto::TorrentCommand;

use super::{Entry, Running};

pub type State = lifecycle::undo::State<Running, Entry, TorrentCommand>;
pub type Event = lifecycle::undo::Event<Running, Entry, TorrentCommand>;
pub type Observed = lifecycle::undo::Observed<Running, Entry, TorrentCommand>;
pub type Step = lifecycle::undo::Step<Running, Entry, TorrentCommand>;
pub type What = lifecycle::undo::What<Running, Entry, TorrentCommand>;
pub type Pressed = lifecycle::undo::Pressed<Entry>;

impl lifecycle::undo::Undoes for Entry {
    fn absorbs(&self, older: &Entry) -> bool {
        Entry::absorbs(self, older)
    }

    fn named(&self) -> (proto::UndoAction, proto::Named) {
        (self.action, proto::Named { id: self.id, name: self.name.clone() })
    }

    fn files(&self) -> bool {
        matches!(self.back, super::Back::Files { .. })
    }
}

/// `n` entries, each of its own torrent, Removes and Deletes alternating, a partial Delete among
/// them.
#[cfg(test)]
pub fn samples(n: usize) -> Vec<Entry> {
    use super::{Back, Source, Stash};
    (0..n)
        .map(|i| {
            let stash = Stash { dir: format!("/stash/{i}").into(), files: Vec::new() };
            let back = match i % 3 {
                _ if i % 7 == 4 => Back::Files {
                    previous: vec![(0, proto::Priority::Normal, "/d/0".into()), (1, proto::Priority::Off, "/d/1".into())],
                    had: Vec::new(),
                    let_go: Vec::new(),
                    replacing: Vec::new(),
                },
                2 => Back::InList { stash },
                _ => Back::Gone { saved: super::Saved::sample(), source: Source::Stash(stash) },
            };
            let action = match i {
                _ if i % 7 == 4 => proto::UndoAction::TrashFiles { count: 2 },
                _ if i % 3 == 0 => proto::UndoAction::Remove,
                _ => proto::UndoAction::Delete,
            };
            Entry { action, name: format!("t{i}"), info_hash: format!("{i:040x}"), id: i as u64, back, put_back: Vec::new(), listed: None }
        })
        .collect()
}

/// A state with some of each: [`samples`] done on the stack, one running, an undo under way.
#[cfg(test)]
pub fn sample_state() -> State {
    let mut state = State::default();
    let one = |id: u64| std::iter::once(id).collect();
    for (i, e) in samples(3).into_iter().enumerate() {
        let seq = i as u64 + 1;
        state.stack.push(Pressed { seq, torrents: one(e.id), done: Some(std::collections::BTreeMap::from([(e.id, e)])) });
    }
    let running = Running {
        action: proto::UndoAction::Delete,
        name: "t9".into(),
        info_hash: format!("{:040x}", 9),
        id: 9,
        listed: None,
        previous: Vec::new(),
        had: Vec::new(),
        let_go: Vec::new(),
        moved: Vec::new(),
        asked: Vec::new(),
        renaming: None,
    };
    state.stack.push(Pressed { seq: 4, torrents: one(9), done: None });
    state.steps.insert(
        4,
        Step { torrents: one(9), what: What::Action([(9, lifecycle::undo::Part::Doing(running))].into()), phase: Phase::Running },
    );
    let undoing = samples(6).pop().unwrap();
    let parts = [(undoing.id, lifecycle::undo::Part::Doing(undoing.clone()))].into();
    state.steps.insert(5, Step { torrents: one(undoing.id), what: What::Undo { of: 0, parts }, phase: Phase::Running });
    state.lanes.insert(9, [4].into());
    state.lanes.insert(undoing.id, [5].into());
    state.last = 5;
    // Why entries left: one of each (#200).
    let refused = proto::Leave::Refused { why: proto::CantUndo::NotKept, stranded: Vec::new() };
    let why = [
        proto::Leave::Undone { refused: Vec::new() },
        refused,
        proto::Leave::Aged,
        proto::Leave::Absorbed { by: 3 },
        proto::Leave::Dropped,
    ];
    state.left = why.into_iter().enumerate().map(|(i, why)| proto::Left { seq: 10 + i as u64, why }).collect();
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Delete takes the place of its torrent's Delete before it, and of nothing else.
    #[test]
    fn a_delete_absorbs_only_its_own_torrents_delete() {
        let entries = samples(6);
        let partial = &entries[2];
        let mut whole = partial.clone();
        whole.back = super::super::Back::Gone { saved: super::super::Saved::sample(), source: super::super::Source::Magnet(String::new()) };
        assert!(whole.absorbs(partial));
        assert!(entries.iter().filter(|e| *e != partial).all(|e| !whole.absorbs(e)));
        let mut again = whole.clone();
        again.id += 100;
        assert!(!again.absorbs(partial), "the same info hash, added again since: another torrent");
    }
}
