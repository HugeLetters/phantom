//! A torrent's librqbit side (#45). Phantom's record is the torrent (its .torrent, destination,
//! priorities and what's wanted); librqbit only runs it, and the engine adds it there itself, at
//! a start too. [`Lib`] says whether librqbit is adding it, has it, or doesn't; [`reconcile`]
//! says what makes librqbit match the torrent's lifecycle and record: add it, start or stop it,
//! take it out, or push its priorities.
//!
//! ```text
//! Adding --its add ends--> In                  Adding --fetching info fails--> Out
//! Unadded (a start, an undo) --Add--> In       Out, Unadded --Check--> In
//! Out --Resume--> Resuming --read--> In        In, Resuming --TakeOut--> Out
//! ```
//!
//! A torrent that's away, missing files or not checked is out of librqbit: it gets a full check
//! when it comes back, as its files may have changed. One Ready comes back with the pieces librqbit
//! saved of it (its `.bitv`, which librqbit only samples): one listed anew (at a start, or brought
//! back by an undo) as librqbit restoring it would, its lifecycle hearing what librqbit does; one
//! taken out (to let go of a file gone from the Trash, or its missing file set Off) resuming, its
//! row as it was.
//!
//! Generic over librqbit's torrent `H` and the add's task `T`, so it's tested on plain values; the
//! actor's are an `Arc<ManagedTorrent>` and a `JoinHandle`.

use std::collections::BTreeSet;

use super::{Phase, State};

/// Where the torrent is in librqbit.
#[derive(Debug, Clone)]
pub enum Lib<H, T> {
    /// librqbit is fetching its info and adding it: the add's task, which a Remove stops.
    Adding(T),
    /// librqbit has it.
    In(H),
    /// Added with the pieces it had (at a start, by an undo, or letting go of a file gone from the
    /// Trash, #234): librqbit is reading a few of them to trust the rest. Nothing is done to it, and
    /// its lifecycle hears nothing, until that's over: its row stays as it was.
    Resuming(H),
    /// librqbit doesn't have it: it never had it, or it's been taken out. What clients see of it
    /// is what librqbit last said.
    Out,
    /// librqbit hasn't had it since it was listed: at a start, or brought back by an undo.
    Unadded,
}

impl<H, T> Lib<H, T> {
    /// librqbit's torrent, if it has it.
    pub fn torrent(&self) -> Option<&H> {
        match self {
            Lib::In(h) | Lib::Resuming(h) => Some(h),
            Lib::Adding(_) | Lib::Out | Lib::Unadded => None,
        }
    }
}

/// What librqbit says of a torrent it has.
pub trait Librqbit {
    /// What it's doing with it.
    fn doing(&self) -> super::Lib;
    /// The files it gets, by index.
    fn gets(&self) -> BTreeSet<usize>;
}

/// What the actor carries out on librqbit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Add it from its record, stopped, with the pieces librqbit saved, which it samples: its
    /// lifecycle hears what librqbit does, as when librqbit restored it.
    Add,
    /// Add it from its record, stopped, with the pieces librqbit saved, resuming
    /// ([`Lib::Resuming`]).
    Resume,
    /// Add it from its record, stopped, with none of the pieces librqbit saved, so it checks them
    /// all: its lifecycle hears it's added again.
    Check,
    Start,
    Stop,
    /// Take it out of librqbit, keeping its files.
    TakeOut,
    /// Have librqbit get the files the record says (those Normal).
    Priorities,
}

/// What makes librqbit, as `lib` says, match the lifecycle `s` and the files it gets, `normal`.
/// One step at a time: asked again once it's done, it says the next, until librqbit matches.
pub fn reconcile<H: Librqbit, T>(s: &State, lib: &Lib<H, T>, normal: &BTreeSet<usize>) -> Option<Do> {
    use Phase as P;
    match lib {
        // Nothing until the add ends, or librqbit trusts the pieces it had.
        Lib::Adding(_) | Lib::Resuming(_) => None,
        Lib::Out | Lib::Unadded => match &s.phase {
            // A check is entered by adding it, with nothing saved.
            P::CheckDue { .. } | P::Checking { .. } => Some(Do::Check),
            // Back with what it had.
            P::Ready if matches!(lib, Lib::Unadded) => Some(Do::Add),
            P::Ready => Some(Do::Resume),
            // Out until it's checked, or for good: never added, an error until Resume or Check, its
            // files away, missing, unchecked, or on their way to the Trash.
            P::FetchingInfo
            | P::FetchFailed { .. }
            | P::Unchecked { .. }
            | P::MissingFiles { .. }
            | P::DestinationUnavailable { .. }
            | P::Error { .. }
            | P::DeleteFailed { .. }
            | P::Deleting { .. }
            | P::Deleted => None,
        },
        Lib::In(h) => {
            let doing = h.doing();
            match (&s.phase, doing) {
                // Away, missing files or not checked: out, so librqbit writes nothing there and
                // offers nothing of them, until a check brings it back.
                (P::Unchecked { .. } | P::MissingFiles { .. } | P::DestinationUnavailable { .. }, _) => Some(Do::TakeOut),
                // A check reads files on their way to the Trash: it's taken out, which ends it, as a
                // Delete's undo checks it again in full.
                (P::Deleting { .. }, super::Lib::Checking) => Some(Do::TakeOut),
                (
                    P::FetchingInfo
                    | P::FetchFailed { .. }
                    | P::CheckDue { .. }
                    | P::Checking { .. }
                    | P::Ready
                    | P::Error { .. }
                    | P::DeleteFailed { .. }
                    | P::Deleting { .. }
                    | P::Deleted,
                    _,
                ) => match super::next(s, doing) {
                    Some(super::Do::Start) => Some(Do::Start),
                    Some(super::Do::Stop) => Some(Do::Stop),
                    // librqbit can't check a torrent it has: out, then in again for a check.
                    Some(super::Do::AddAgain) => Some(Do::TakeOut),
                    // librqbit takes a change of files only between checks; one not checked gets its
                    // priorities as it's checked, added again with them.
                    None => (doing != super::Lib::Checking && !s.phase.unchecked() && h.gets() != *normal).then_some(Do::Priorities),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use lifecycle::every;
    use lifecycle::machine::{Every, Row, table};

    use super::super::Lib as Doing;
    use super::*;

    /// librqbit's torrent, as what it does and which files it gets.
    #[derive(Debug, Clone, PartialEq)]
    struct Fake {
        doing: Doing,
        gets: BTreeSet<usize>,
    }

    impl Librqbit for Fake {
        fn doing(&self) -> Doing {
            self.doing
        }

        fn gets(&self) -> BTreeSet<usize> {
            self.gets.clone()
        }
    }

    /// What the record says it gets: files 0 and 1.
    fn normal() -> BTreeSet<usize> {
        [0, 1].into()
    }

    fn fakes() -> Vec<Fake> {
        let gets = [normal(), [0].into()];
        Doing::every().into_iter().flat_map(|doing| gets.clone().map(|gets| Fake { doing, gets })).collect()
    }

    type L = Lib<Fake, ()>;

    every!(L {
        Lib::Adding(_) => [Lib::Adding(())],
        Lib::In(_) => fakes().into_iter().map(Lib::In),
        Lib::Resuming(_) => fakes().into_iter().map(Lib::Resuming),
        Lib::Out => [Lib::Out],
        Lib::Unadded => [Lib::Unadded],
    });

    fn doing(lib: &L) -> Option<Doing> {
        lib.torrent().map(|h| h.doing)
    }

    /// Where the lifecycle wants it: out of librqbit for good, or until it's checked.
    fn out(phase: &Phase) -> bool {
        matches!(phase, Phase::Unchecked { .. } | Phase::MissingFiles { .. } | Phase::DestinationUnavailable { .. })
    }

    #[test]
    fn librqbit_is_made_to_match_the_lifecycle_and_the_record_one_step_at_a_time() {
        let rows: [Row<State, L, Option<Do>>; 6] = [
            Row {
                says: "nothing is done while it's being added, or while librqbit reads the pieces it was added back with",
                when: |_, lib| matches!(lib, Lib::Adding(_) | Lib::Resuming(_)),
                then: |_, _, todo| todo.is_none(),
            },
            Row {
                says: "out of librqbit, one due a check or checked is added with nothing saved, one ready with what it had (resuming, \
                       once taken out), and no other",
                when: |_, lib| matches!(lib, Lib::Out | Lib::Unadded),
                then: |s, lib, todo| {
                    *todo
                        == match (&s.phase, lib) {
                            (Phase::CheckDue { .. } | Phase::Checking { .. }, _) => Some(Do::Check),
                            (Phase::Ready, Lib::Unadded) => Some(Do::Add),
                            (Phase::Ready, _) => Some(Do::Resume),
                            _ => None,
                        }
                },
            },
            Row {
                says: "away, missing files or not checked, it's taken out, whatever librqbit does",
                when: |s, lib| matches!(lib, Lib::In(_)) && out(&s.phase),
                then: |_, _, todo| *todo == Some(Do::TakeOut),
            },
            Row {
                says: "being deleted, a check is taken out, which ends it",
                when: |s, lib| {
                    matches!(lib, Lib::In(_)) && matches!(s.phase, Phase::Deleting { .. }) && doing(lib) == Some(Doing::Checking)
                },
                then: |_, _, todo| *todo == Some(Do::TakeOut),
            },
            Row {
                says: "in librqbit, it's started, stopped or taken out to be checked as the lifecycle says",
                when: |s, lib| {
                    matches!(lib, Lib::In(_))
                        && !out(&s.phase)
                        && !(matches!(s.phase, Phase::Deleting { .. }) && doing(lib) == Some(Doing::Checking))
                        && super::super::next(s, doing(lib).unwrap()).is_some()
                },
                then: |s, lib, todo| {
                    *todo
                        == Some(match super::super::next(s, doing(lib).unwrap()).unwrap() {
                            super::super::Do::Start => Do::Start,
                            super::super::Do::Stop => Do::Stop,
                            super::super::Do::AddAgain => Do::TakeOut,
                        })
                },
            },
            Row {
                says: "else its priorities are pushed when librqbit gets other files, isn't checking and it isn't unchecked",
                when: |s, lib| {
                    matches!(lib, Lib::In(_))
                        && !out(&s.phase)
                        && !(matches!(s.phase, Phase::Deleting { .. }) && doing(lib) == Some(Doing::Checking))
                        && super::super::next(s, doing(lib).unwrap()).is_none()
                },
                then: |s, lib, todo| {
                    let Lib::In(h) = lib else { unreachable!() };
                    *todo == (h.doing != Doing::Checking && !s.phase.unchecked() && h.gets != normal()).then_some(Do::Priorities)
                },
            },
        ];
        table(|s, lib| reconcile(s, lib, &normal()), &rows, &[]);
    }
}
