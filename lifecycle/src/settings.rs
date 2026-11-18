//! The service's settings changes (#221), the real service's and the GUI's fake's: each change of a
//! setting that took effect, with what its setting was before, newest last, the last
//! [`SettingChange::KEPT`]. An `UndoSetting` takes the newest, whoever made it. In memory only:
//! a new service starts with none. Every change has its own `seq`, larger than any before, from
//! the number it's started at (the service's start, in nanoseconds, so a later service's are
//! larger and never one a client still holds).

use std::collections::VecDeque;

use proto::{SettingChange, SettingName};

/// The changes, each with `T`, what its setting was before it.
#[derive(Debug, Clone)]
pub struct Changes<T> {
    kept: VecDeque<(SettingChange, T)>,
    next: u64,
}

impl<T> Changes<T> {
    /// None yet; the first change's `seq` is `first`.
    pub fn new(first: u64) -> Self {
        Self { kept: VecDeque::new(), next: first }
    }

    /// A change of `name`, `by` its client's id for it, whose setting was `was`: the newest. The
    /// oldest goes once there are more than [`SettingChange::KEPT`].
    pub fn push(&mut self, name: SettingName, by: Option<u64>, was: T) -> SettingChange {
        let change = SettingChange { seq: self.next, name, by };
        self.next += 1;
        self.kept.push_back((change.clone(), was));
        if self.kept.len() > SettingChange::KEPT {
            self.kept.pop_front();
        }
        change
    }

    /// The newest, taken off.
    pub fn take_newest(&mut self) -> Option<(SettingChange, T)> {
        self.kept.pop_back()
    }

    /// Every change kept, oldest first, as clients see them.
    pub fn listed(&self) -> Vec<SettingChange> {
        self.kept.iter().map(|(change, _)| change.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        /// Changes come off newest first, each with what it was given; past the last 20 the
        /// oldest are gone; every change's `seq` is larger than any before.
        #[test]
        fn the_newest_comes_off_first_and_only_the_last_twenty_are_kept(ops in proptest::collection::vec(any::<bool>(), 0..60)) {
            let mut changes = Changes::new(1_000);
            let mut model: Vec<u64> = Vec::new();
            let mut last = None;
            for (i, push) in ops.into_iter().enumerate() {
                if push {
                    let change = changes.push(SettingName::ALL[i % 8], Some(i as u64), i as u64);
                    prop_assert!(last.is_none_or(|l| change.seq > l), "seq grows");
                    last = Some(change.seq);
                    model.push(i as u64);
                    if model.len() > SettingChange::KEPT {
                        model.remove(0);
                    }
                } else {
                    let taken = changes.take_newest().map(|(change, was)| (change.by, was));
                    let expected = model.pop().map(|i| (Some(i), i));
                    prop_assert_eq!(taken, expected);
                }
                prop_assert_eq!(changes.listed().iter().map(|c| c.by.unwrap()).collect::<Vec<_>>(), model.clone());
            }
        }
    }
}
