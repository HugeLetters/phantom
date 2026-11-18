//! What became of the undo stack's entries (#200): the service says. Each snapshot carries why the
//! last entries left the stack ([`proto::Left`]); the app keeps what it has been told, once each,
//! and hands what's new on. Pure; [`Phantom`](super::Phantom) carries it.

use std::collections::BTreeMap;

use proto::{Leave, Left};

/// How many fates are kept: far more than the notices that can still ask (the last 50 told).
const KEPT: usize = 1000;

#[derive(Debug, Default)]
pub struct Undos {
    left: BTreeMap<u64, Leave>,
}

impl Undos {
    /// The snapshot's list of entries that left: those not told before, to tell the notices.
    /// Snapshots are the latest only and repeat what the service still keeps, so each is taken once
    /// (a word the app made up itself from an answer, and the service's later one, differ).
    pub fn observe(&mut self, left: &[Left]) -> Vec<Left> {
        let new: Vec<Left> = left.iter().filter(|l| self.left.get(&l.seq) != Some(&l.why)).cloned().collect();
        self.left.extend(new.iter().map(|l| (l.seq, l.why.clone())));
        while self.left.len() > KEPT {
            self.left.pop_first();
        }
        new
    }

    /// Why entry `seq` left the stack, if the service has said.
    pub fn why(&self, seq: u64) -> Option<&Leave> {
        self.left.get(&seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left(seq: u64, why: Leave) -> Left {
        Left { seq, why }
    }

    #[test]
    fn each_entry_is_handed_on_once_whatever_the_snapshots_repeat() {
        let mut undos = Undos::default();
        assert_eq!(undos.observe(&[left(1, Leave::Aged)]), [left(1, Leave::Aged)]);
        let again = [left(1, Leave::Aged), left(2, Leave::Undone { refused: Vec::new() })];
        assert_eq!(undos.observe(&again), [left(2, Leave::Undone { refused: Vec::new() })]);
        assert!(undos.observe(&again).is_empty());
        assert_eq!((undos.why(1), undos.why(2), undos.why(3)), (Some(&Leave::Aged), Some(&Leave::Undone { refused: Vec::new() }), None));
    }
}
