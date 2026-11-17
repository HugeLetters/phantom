//! What became of the undo stack's entries (#200): the app sees only the stack, as each snapshot
//! lists it, so it works out why an entry left it. Pure; [`Phantom`](super::Phantom) carries it.
//!
//! An entry leaves from the bottom when newer ones push it past [`DEPTH`] (**aged**); otherwise
//! an undo took it, whoever asked (this app, the CLI, another client: **undone**), or this app's
//! own undo of it was refused, which its answer says (**refused**: the snapshot already lacks the
//! entry, as a refused one is dropped, so while an undo of ours is out the snapshot's word is held
//! back and the answer decides). An undo of another client that was refused can't be told from one
//! that worked: the stack looks the same.

use std::collections::{BTreeMap, BTreeSet};

use lifecycle::undo::DEPTH;
use proto::Undoable;

/// What became of an entry that's no longer on the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gone {
    Undone,
    /// Pushed past the last [`DEPTH`] by newer ones.
    Aged,
    /// Its undo was refused, for this reason (a clause: see `display::cant_undo_reason`).
    Refused(String),
}

/// How many entries' fates are kept: far more than the notices that can still ask (the last 50
/// told, each of at most [`DEPTH`] entries).
const KEPT: usize = 1000;

#[derive(Debug, Default)]
pub struct Undos {
    /// The seqs on the stack in the latest snapshot, oldest first.
    seen: Vec<u64>,
    gone: BTreeMap<u64, Gone>,
    /// Undos of this app asked and not yet answered.
    asked: usize,
    /// Entries gone from the stack while an undo of ours was out: undone, unless the answer says
    /// theirs was refused.
    held: BTreeSet<u64>,
}

/// The entries of `before` that `now` pushed past [`DEPTH`]: the oldest, as many as newer entries
/// came beyond what fits.
pub fn aged(before: &[u64], now: &[u64]) -> Vec<u64> {
    let newest = before.last().copied();
    let came = now.iter().filter(|s| newest.is_none_or(|n| **s > n)).count();
    let over = (before.len() + came).saturating_sub(DEPTH);
    before.iter().take(over).copied().filter(|s| !now.contains(s)).collect()
}

impl Undos {
    /// The stack the latest snapshot lists: the entries that were undone by it, to tell the
    /// notices.
    pub fn observe(&mut self, stack: &[Undoable]) -> Vec<u64> {
        let now: Vec<u64> = stack.iter().map(|u| u.seq).collect();
        let aged = aged(&self.seen, &now);
        let mut undone = Vec::new();
        for seq in self.seen.iter().filter(|s| !now.contains(s)) {
            if self.gone.contains_key(seq) {
                continue;
            }
            if aged.contains(seq) {
                self.gone.insert(*seq, Gone::Aged);
            } else if self.asked > 0 {
                self.held.insert(*seq);
            } else {
                self.gone.insert(*seq, Gone::Undone);
                undone.push(*seq);
            }
        }
        self.seen = now;
        self.trim();
        undone
    }

    /// An undo of this app goes out.
    pub fn ask(&mut self) {
        self.asked += 1;
    }

    /// An undo of this app was answered: the entries that were undone by it or that it held, to
    /// tell the notices.
    pub fn answer(&mut self, answer: &Result<Undoable, client::Failure>) -> Vec<u64> {
        self.asked = self.asked.saturating_sub(1);
        let mut undone = Vec::new();
        match answer {
            Ok(done) => {
                self.held.remove(&done.seq);
                self.gone.insert(done.seq, Gone::Undone);
                undone.push(done.seq);
            }
            Err(client::Failure::Refused(proto::Refusal::Undo { undo, why, stranded })) => {
                self.held.remove(&undo.seq);
                self.gone.insert(undo.seq, Gone::Refused(client::display::cant_undo_reason(why, stranded)));
            }
            Err(_) => {}
        }
        if self.asked == 0 {
            for seq in std::mem::take(&mut self.held) {
                self.gone.insert(seq, Gone::Undone);
                undone.push(seq);
            }
        }
        self.trim();
        undone
    }

    /// Whether `seq` left the stack while an undo of ours was out, and the answer is still to say
    /// whether it was undone.
    pub fn held(&self, seq: u64) -> bool {
        self.held.contains(&seq)
    }

    /// What became of `seq`, if it's off the stack and that's known.
    pub fn of(&self, seq: u64) -> Option<&Gone> {
        self.gone.get(&seq)
    }

    fn trim(&mut self) {
        while self.gone.len() > KEPT {
            self.gone.pop_first();
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proto::{CantUndo, Refusal, UndoAction};

    use super::*;

    fn entry(seq: u64) -> Undoable {
        Undoable { seq, action: UndoAction::Remove, id: seq, name: format!("t{seq}") }
    }

    fn stack(seqs: &[u64]) -> Vec<Undoable> {
        seqs.iter().copied().map(entry).collect()
    }

    fn refused(seq: u64) -> Result<Undoable, client::Failure> {
        Err(client::Failure::Refused(Refusal::Undo { undo: entry(seq), why: CantUndo::NotKept, stranded: Vec::new() }))
    }

    #[test]
    fn entries_pushed_past_the_last_twenty_are_aged_not_undone() {
        let before: Vec<u64> = (1..=20).collect();
        assert_eq!(aged(&before, &[3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22]), [1, 2]);
        assert_eq!(aged(&before, &(2..=21).collect::<Vec<_>>()), [1]);
        // One taken from the top as one comes: nothing was pushed past anything.
        assert_eq!(aged(&before, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21]), Vec::<u64>::new());
        assert_eq!(aged(&[], &[1]), Vec::<u64>::new());
    }

    /// A partly emptied Trash: the undo of a Move to Trash puts back what's left and answers as any
    /// undo does, so it counts as undone, whatever it put back.
    #[test]
    fn a_move_to_trash_undone_with_part_of_its_files_emptied_counts_as_undone() {
        let mut undos = Undos::default();
        undos.observe(&[Undoable { action: UndoAction::TrashFiles { count: 3 }, ..entry(1) }]);
        undos.ask();
        let put_back = Ok(Undoable { action: UndoAction::TrashFiles { count: 1 }, ..entry(1) });
        assert_eq!(undos.answer(&put_back), [1]);
        assert_eq!(undos.of(1), Some(&Gone::Undone));
    }

    /// The snapshot with an undo's effect can come before its answer: a refusal then isn't taken
    /// for an undo; and after it, it still is.
    #[test]
    fn a_refused_undo_is_not_taken_for_an_undo_whichever_comes_first() {
        for snapshot_first in [true, false] {
            let mut undos = Undos::default();
            undos.observe(&stack(&[1]));
            undos.ask();
            let mut told = Vec::new();
            if snapshot_first {
                told.extend(undos.observe(&[]));
            }
            told.extend(undos.answer(&refused(1)));
            if !snapshot_first {
                told.extend(undos.observe(&[]));
            }
            assert_eq!(told, Vec::<u64>::new(), "snapshot first: {snapshot_first}");
            assert!(matches!(undos.of(1), Some(Gone::Refused(_))));
        }
    }

    #[test]
    fn an_undo_by_another_client_is_seen_in_the_stack() {
        let mut undos = Undos::default();
        undos.observe(&stack(&[1, 2]));
        assert_eq!(undos.observe(&stack(&[1])), [2]);
        assert_eq!(undos.of(2), Some(&Gone::Undone));
    }

    #[derive(Debug, Clone)]
    enum Op {
        /// This many more actions.
        Push(usize),
        /// Another client's undo of the top.
        Foreign,
        /// Ours of the top, which the service refuses or not, its snapshot before or after the answer.
        Own { refuse: bool, snapshot_first: bool },
    }

    fn ops() -> impl Strategy<Value = Vec<Op>> {
        let op = prop_oneof![
            4 => (1usize..6).prop_map(Op::Push),
            2 => Just(Op::Foreign),
            2 => (any::<bool>(), any::<bool>()).prop_map(|(refuse, snapshot_first)| Op::Own { refuse, snapshot_first }),
        ];
        prop::collection::vec(op, 1..40)
    }

    proptest! {
        /// Each entry that leaves the stack is told as what became of it, once: aged when pushed
        /// past the last twenty, undone when an undo took it (ours answered, or another client's),
        /// refused when ours was; and the seqs handed on as undone are exactly the undone.
        #[test]
        fn every_entry_that_leaves_is_told_as_what_became_of_it(ops in ops()) {
            let mut undos = Undos::default();
            let mut stack: Vec<u64> = Vec::new();
            let mut next = 1u64;
            let mut want: BTreeMap<u64, Gone> = BTreeMap::new();
            let mut told: Vec<u64> = Vec::new();
            for op in ops {
                match op {
                    Op::Push(k) => {
                        stack.extend(next..next + k as u64);
                        next += k as u64;
                        while stack.len() > DEPTH {
                            want.insert(stack.remove(0), Gone::Aged);
                        }
                        told.extend(undos.observe(&super::tests::stack(&stack)));
                    }
                    Op::Foreign => {
                        if let Some(top) = stack.pop() {
                            want.insert(top, Gone::Undone);
                        }
                        told.extend(undos.observe(&super::tests::stack(&stack)));
                    }
                    Op::Own { refuse, snapshot_first } => {
                        undos.ask();
                        let Some(top) = stack.pop() else {
                            told.extend(undos.answer(&Err(client::Failure::Refused(Refusal::NothingToUndo))));
                            continue;
                        };
                        let answer = if refuse { refused(top) } else { Ok(entry(top)) };
                        want.insert(top, if refuse { Gone::Refused(client::display::cant_undo_reason(&CantUndo::NotKept, &[])) } else { Gone::Undone });
                        if snapshot_first {
                            told.extend(undos.observe(&super::tests::stack(&stack)));
                        }
                        told.extend(undos.answer(&answer));
                        if !snapshot_first {
                            told.extend(undos.observe(&super::tests::stack(&stack)));
                        }
                    }
                }
            }
            for (seq, gone) in &want {
                prop_assert_eq!(undos.of(*seq), Some(gone), "seq {}", seq);
            }
            let mut told_sorted = told.clone();
            told_sorted.sort();
            told_sorted.dedup();
            prop_assert_eq!(told.len(), told_sorted.len(), "each undone entry told once");
            let undone: Vec<u64> = want.iter().filter(|(_, g)| **g == Gone::Undone).map(|(s, _)| *s).collect();
            prop_assert_eq!(told_sorted, undone);
        }
    }
}
