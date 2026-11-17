//! Undo's machine over every state × event (the table), and over generated sequences against a
//! pretend actor that moves items and reports them: lanes, the stack, no deadlock, no torrent
//! waiting on another's unless a selection ties them, and undos bringing back the start. Steps of
//! several torrents (a selection's Remove or Delete, #235) have a part per torrent, which reports
//! on its own.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proto::{CantUndo, Leave, Left};

use super::*;
use crate::every;
use crate::machine::{DecideRow, Every, Row, decide_table, sequences, table};

/// What an action did so far on one torrent: the items it moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run(u8);

/// What undoes an action on one torrent: the items it moved. A partial Delete, which a later
/// Delete of the same torrent absorbs; a Move to Trash, which absorbs none and is absorbed by none.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    torrent: Torrent,
    items: u8,
    partial: bool,
    files: bool,
}

impl Undoes for Entry {
    fn absorbs(&self, older: &Self) -> bool {
        self.torrent == older.torrent && !self.files && !older.files && older.partial
    }
}

/// A command waiting its turn.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cmd(u8);

type S = State<Run, Entry, Cmd>;
type E = Event<Run, Entry, Cmd>;
type M = Steps<Run, Entry, Cmd>;

fn on(torrents: &[Torrent]) -> BTreeSet<Torrent> {
    torrents.iter().copied().collect()
}

fn action(torrents: &[Torrent]) -> E {
    Event::Observed(Observed::Arrived { seq: 0, what: Arrival::Action(torrents.iter().map(|t| (*t, Run(0))).collect()) })
}

fn command(torrent: Torrent) -> E {
    Event::Observed(Observed::Arrived { seq: 0, what: Arrival::Command(torrent, Cmd(1)) })
}

fn entry(torrent: Torrent, items: u8) -> Entry {
    Entry { torrent, items, partial: false, files: false }
}

fn finished(seq: u64, t: Torrent, items: u8) -> E {
    Event::Observed(Observed::Finished(seq, t, (items > 0).then_some(entry(t, items))))
}

fn partial(seq: u64, t: Torrent) -> E {
    Event::Observed(Observed::Finished(seq, t, Some(Entry { partial: true, ..entry(t, 1) })))
}

const UNDO: E = Event::Command(Command::Undo);

/// The state `events` make from none.
fn after(events: &[E]) -> S {
    events.iter().fold(S::default(), |s, e| decide(&s, e).unwrap_or(s))
}

every!(State<Run, Entry, Cmd> {
    State { .. } => [
        S::default(),
        // 1 committed on torrent 2; 2 running on 1, 3 queued behind it, a command (4) behind that.
        after(&[action(&[2]), finished(1, 2, 2), action(&[1]), action(&[1]), command(1)]),
        // 1 committed then undone: its undo (3) running on 2; 2 running on 1, aborted by ⌘Z.
        after(&[action(&[2]), finished(1, 2, 1), action(&[1]), UNDO, UNDO]),
        // 1 running on 1, a selection of 1 and 2 (2) waiting for it, 3 running on 3.
        after(&[action(&[1]), action(&[1, 2]), action(&[3])]),
        // 1 a partial Delete of torrent 1, committed; 2 a Delete of torrent 1 running, which takes its place.
        after(&[action(&[1]), partial(1, 1), action(&[1])]),
        // 1 a selection of 1 and 2, committed partly deleted on 1; 2 a Delete of 1 running, which
        // takes that part's place.
        after(&[action(&[1, 2]), partial(1, 1), finished(1, 2, 1), action(&[1])]),
        // A selection of 1 and 2 running, its part on 1 over (moved one), on 2 under way.
        after(&[action(&[1, 2]), finished(1, 1, 1)]),
        // The same, its part on 1 over having changed nothing.
        after(&[action(&[1, 2]), finished(1, 1, 0)]),
        // The same aborted by ⌘Z.
        after(&[action(&[1, 2]), finished(1, 1, 1), UNDO]),
        // A selection of 1 and 2 committed, then ⌘Z: its undo running, back on 1, under way on 2.
        after(&[action(&[1, 2]), finished(1, 1, 1), finished(1, 2, 1), UNDO, finished(2, 1, 0)]),
        // The stack full of committed ones, a partial one among them.
        (0..DEPTH as u64).fold(S::default(), |s, n| {
            let s = decide(&s, &action(&[n % 3])).unwrap();
            let seq = s.top().unwrap().seq;
            let done = if n == 3 { partial(seq, n % 3) } else { finished(seq, n % 3, 1) };
            decide(&s, &done).unwrap()
        }),
    ],
});

every!(Event<Run, Entry, Cmd> {
    Event::Command(Command::Undo) => [UNDO],
    Event::Observed(Observed::Arrived { .. }) => [action(&[1]), action(&[1, 2]), action(&[]), command(2)],
    Event::Observed(Observed::Moved(..)) => [1, 2, 3, 4, 99].into_iter().flat_map(|seq| [1, 2].map(|t| Event::Observed(Observed::Moved(seq, t, Run(1))))),
    Event::Observed(Observed::Finished(..)) => [1, 2, 3, 4, 99]
        .into_iter()
        .flat_map(|seq| [1, 2].into_iter().flat_map(move |t| [finished(seq, t, 0), finished(seq, t, 2), partial(seq, t)]))
        .chain([Event::Observed(Observed::Finished(2, 1, Some(Entry { files: true, ..entry(1, 1) })))]),
    Event::Observed(Observed::Failed(..)) => [1, 2, 3, 4, 99].into_iter().flat_map(|seq| [1, 2].map(|t| Event::Observed(Observed::Failed(seq, t)))),
    Event::Observed(Observed::Refused { .. }) => [1, 2, 3, 4, 99].into_iter().flat_map(|seq| {
        [1, 2].map(|torrent| Event::Observed(Observed::Refused { seq, torrent, why: CantUndo::NotKept, stranded: Vec::new() }))
    }),
    Event::Observed(Observed::Stopping) => [Event::Observed(Observed::Stopping)],
    Event::Observed(Observed::Restarted) => [Event::Observed(Observed::Restarted)],
});

/// The part an event is news of: its step's seq and torrent, and whether it ends it.
fn part_of(e: &E) -> Option<(u64, Torrent)> {
    match e {
        Event::Observed(o) => o.part().map(|(seq, t, _)| (seq, t)),
        Event::Command(_) => None,
    }
}

/// The step whose part under way `e` is news of, as it is in `s`.
fn target<'a>(s: &'a S, e: &E) -> Option<&'a Step<Run, Entry, Cmd>> {
    part_of(e).and_then(|(seq, t)| s.steps.get(&seq).filter(|st| st.doing(t)))
}

fn seq_of(e: &E) -> u64 {
    part_of(e).map_or(u64::MAX, |(seq, _)| seq)
}

/// Whether `e` ends its part (rather than reporting a move).
fn ends(e: &E) -> bool {
    matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(..) | Observed::Refused { .. }))
}

/// Whether the part `e` ends is the last of its step under way.
fn last_part(s: &S, e: &E) -> bool {
    let (Some(st), Some((_, t))) = (target(s, e), part_of(e)) else { return false };
    st.underway().all(|u| u == t)
}

/// What left the stack's record in the step from `s` to `out`: the entries of `out.left` that `s.left` didn't have.
fn new_left(s: &S, out: &S) -> Vec<Left> {
    out.left.iter().filter(|l| s.left.iter().all(|old| old.seq != l.seq)).cloned().collect()
}

fn left(seq: u64, why: Leave) -> Left {
    Left { seq, why }
}

fn is_action(s: Option<&Step<Run, Entry, Cmd>>) -> bool {
    s.is_some_and(|s| matches!(s.what, What::Action(_)))
}

fn is_undo(s: Option<&Step<Run, Entry, Cmd>>) -> bool {
    s.is_some_and(|s| matches!(s.what, What::Undo { .. }))
}

/// `s` without step `seq`, in its lanes and on the stack, but its steps' phases as `out` has them.
fn without(s: &S, seq: u64) -> (Vec<u64>, BTreeSet<u64>) {
    (s.stack.iter().map(|p| p.seq).filter(|p| *p != seq).collect(), s.steps.keys().copied().filter(|k| *k != seq).collect())
}

fn shape(s: &S) -> (Vec<u64>, BTreeSet<u64>) {
    (s.stack.iter().map(|p| p.seq).collect(), s.steps.keys().copied().collect())
}

/// What an action's parts hold once `e`, the end of one of them, is in: what undoes each that
/// changed something.
fn entries_after(s: &S, e: &E) -> BTreeMap<Torrent, Entry> {
    let (Some(What::Action(parts)), Some((_, t))) = (target(s, e).map(|st| &st.what), part_of(e)) else { return BTreeMap::new() };
    let ended = match e {
        Event::Observed(Observed::Finished(_, _, entry)) => *entry,
        _ => None,
    };
    parts
        .iter()
        .filter_map(|(u, p)| match (p, *u == t) {
            (_, true) => ended.map(|e| (*u, e)),
            (Part::Over(Some(e)), false) => Some((*u, *e)),
            (Part::Over(None) | Part::Doing(_), false) => None,
        })
        .collect()
}

/// `s`'s step for `e`'s part, with that part as `part`: what news of a part that isn't its step's
/// last does.
fn with_part(s: &S, e: &E, part: impl FnOnce(&mut What<Run, Entry, Cmd>, Torrent)) -> S {
    let mut want = s.clone();
    let (seq, t) = part_of(e).unwrap();
    part(&mut want.steps.get_mut(&seq).unwrap().what, t);
    want
}

fn decide_rows() -> Vec<DecideRow<M>> {
    vec![
        Row {
            says: "⌘Z with nothing on the stack is refused",
            when: |s, e| *e == UNDO && s.stack.is_empty(),
            then: |_, _, out| *out == Err(Refusal::Nothing),
        },
        Row {
            says: "⌘Z on a committed action takes it off the stack, and its undo arrives in the lanes of the torrents it changed, a part each",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| p.done.is_some()),
            then: |s, _, out| {
                let (Ok(out), Some(top)) = (out, s.top()) else { return false };
                let done = top.done.clone().unwrap();
                let undo = out.steps.iter().find(|(_, st)| matches!(st.what, What::Undo { of, .. } if of == top.seq));
                out.stack[..] == s.stack[..s.stack.len() - 1]
                    && new_left(s, out).is_empty()
                    && undo.is_some_and(|(seq, st)| {
                        let parts: BTreeMap<Torrent, Part<Entry, Undid>> = done.iter().map(|(t, e)| (*t, Part::Doing(*e))).collect();
                        st.torrents == done.keys().copied().collect()
                            && matches!(&st.what, What::Undo { parts: p, .. } if *p == parts)
                            && st.torrents.iter().all(|t| out.lanes[t].back() == Some(seq))
                    })
            },
        },
        Row {
            says: "⌘Z on an action queued drops it: off the stack and its lanes",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| s.steps.get(&p.seq).is_some_and(|st| st.phase == Phase::Queued)),
            then: |s, _, out| {
                let top = s.top().unwrap().seq;
                out.as_ref().is_ok_and(|out| shape(out) == without(s, top) && new_left(s, out) == [left(top, Leave::Undone)])
            },
        },
        Row {
            says: "⌘Z on an action running aborts it: off the stack, still in its lanes",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| s.steps.get(&p.seq).is_some_and(|st| st.phase != Phase::Queued)),
            then: |s, _, out| {
                let (Ok(out), Some(top)) = (out, s.top()) else { return false };
                out.stack[..] == s.stack[..s.stack.len() - 1] && out.steps[&top.seq].phase == Phase::Aborting && new_left(s, out).is_empty()
            },
        },
        Row {
            says: "a step arrives at the end of each of its lanes, a part per torrent; an action on top of the stack too, one entry",
            when: |_, e| match e {
                Event::Observed(Observed::Arrived { what: Arrival::Action(parts), .. }) => !parts.is_empty(),
                Event::Observed(Observed::Arrived { what: Arrival::Command(..), .. }) => true,
                _ => false,
            },
            then: |s, e, out| {
                let (Event::Observed(Observed::Arrived { what, .. }), Ok(out)) = (e, out) else { return false };
                let seq = s.last_seq() + 1;
                let (stacked, torrents) = match what {
                    Arrival::Action(parts) => {
                        let doing: BTreeMap<Torrent, Part<Run, Option<Entry>>> = parts.iter().map(|(t, r)| (*t, Part::Doing(*r))).collect();
                        let fresh =
                            out.top().is_some_and(|p| p.seq == seq && p.done.is_none() && p.torrents == parts.keys().copied().collect());
                        (fresh && out.steps[&seq].what == What::Action(doing), parts.keys().copied().collect())
                    }
                    Arrival::Command(t, _) => (out.stack == s.stack, on(&[*t])),
                };
                // Past the last DEPTH the oldest are aged, and only they leave.
                let pushed = usize::from(matches!(what, Arrival::Action(_)));
                let over = (s.stack.len() + pushed).saturating_sub(DEPTH);
                let aged: Vec<Left> = s.stack.iter().take(over).map(|p| left(p.seq, Leave::Aged)).collect();
                stacked
                    && new_left(s, out) == aged
                    && out.steps[&seq].torrents == torrents
                    && torrents.iter().all(|t| out.lanes[t].back() == Some(&seq))
            },
        },
        Row {
            says: "an action of no torrent is nothing",
            when: |_, e| matches!(e, Event::Observed(Observed::Arrived { what: Arrival::Action(parts), .. }) if parts.is_empty()),
            then: |s, _, out| out.as_ref() == Ok(s),
        },
        Row {
            says: "what a running action did on a torrent is logged in that part",
            when: |s, e| matches!(e, Event::Observed(Observed::Moved(..))) && is_action(target(s, e)),
            then: |s, e, out| {
                let Event::Observed(Observed::Moved(_, _, r)) = e else { return false };
                let want = with_part(s, e, |what, t| {
                    if let What::Action(parts) = what {
                        parts.insert(t, Part::Doing(*r));
                    }
                });
                out.as_ref() == Ok(&want)
            },
        },
        Row {
            says: "an action's or an undo's part that ends while others run is over, with how it went; nothing else changes",
            when: |s, e| {
                ends(e)
                    && (is_action(target(s, e)) || is_undo(target(s, e)))
                    && !last_part(s, e)
                    && !(is_action(target(s, e)) && matches!(e, Event::Observed(Observed::Refused { .. })))
            },
            then: |s, e, out| {
                let want = with_part(s, e, |what, t| match (what, e) {
                    (What::Action(parts), Event::Observed(Observed::Finished(_, _, entry))) => drop(parts.insert(t, Part::Over(*entry))),
                    (What::Action(parts), _) => drop(parts.insert(t, Part::Over(None))),
                    (What::Undo { parts, .. }, Event::Observed(Observed::Finished(..))) => drop(parts.insert(t, Part::Over(Undid::Back))),
                    (What::Undo { parts, .. }, Event::Observed(Observed::Refused { why, stranded, .. })) => {
                        drop(parts.insert(t, Part::Over(Undid::Refused { why: why.clone(), stranded: stranded.clone() })))
                    }
                    (What::Undo { parts, .. }, _) => drop(parts.insert(t, Part::Over(Undid::Failed))),
                    (What::Command(_), _) => {}
                });
                out.as_ref() == Ok(&want)
            },
        },
        Row {
            says: "a running action whose last part ends, having changed something, is committed in its place with what each part holds, without the parts of older entries they absorb",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(..)))
                    && target(s, e).is_some_and(|st| matches!(st.what, What::Action(_)) && st.phase == Phase::Running)
                    && last_part(s, e)
                    && !entries_after(s, e).is_empty()
            },
            then: |s, e, out| {
                let (seq, entries) = (seq_of(e), entries_after(s, e));
                let mut absorbed = Vec::new();
                let want: Vec<(u64, BTreeSet<Torrent>, Option<BTreeMap<Torrent, Entry>>)> = s
                    .stack
                    .iter()
                    .filter_map(|p| {
                        let done = match (&p.done, p.seq == seq) {
                            (_, true) => entries.clone(),
                            (Some(done), false) => {
                                let kept: BTreeMap<Torrent, Entry> = done
                                    .iter()
                                    .filter(|(_, old)| !entries.values().any(|n| n.absorbs(old)))
                                    .map(|(t, e)| (*t, *e))
                                    .collect();
                                if kept.is_empty() {
                                    absorbed.push(left(p.seq, Leave::Absorbed { by: seq }));
                                    return None;
                                }
                                kept
                            }
                            (None, false) => return Some((p.seq, p.torrents.clone(), None)),
                        };
                        Some((p.seq, done.keys().copied().collect(), Some(done)))
                    })
                    .collect();
                let Ok(out) = out else { return false };
                !out.steps.contains_key(&seq)
                    && out.stack.iter().map(|p| (p.seq, p.torrents.clone(), p.done.clone())).collect::<Vec<_>>() == want
                    && new_left(s, out) == absorbed
            },
        },
        Row {
            says: "an action whose last part ends with none of them having changed anything is gone, off the stack and its lanes",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(..)))
                    && is_action(target(s, e))
                    && last_part(s, e)
                    && entries_after(s, e).is_empty()
            },
            then: |s, e, out| {
                let seq = seq_of(e);
                // One a ⌘Z took off the stack is undone; one that changed nothing had nothing to undo.
                // (One pushed past the last 20 meanwhile has said so already.)
                let want = match (s.steps[&seq].phase, s.stack.iter().any(|p| p.seq == seq)) {
                    (Phase::Aborting, _) => vec![left(seq, Leave::Undone)],
                    (_, true) => vec![left(seq, Leave::Dropped)],
                    (_, false) => Vec::new(),
                };
                out.as_ref().is_ok_and(|out| shape(out) == without(s, seq) && new_left(s, out) == want)
            },
        },
        Row {
            says: "an aborted action whose last part ends, having changed something, is rolled back in its place: its undo, running, of the torrents it changed only",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(..)))
                    && target(s, e).is_some_and(|st| matches!(st.what, What::Action(_)) && st.phase == Phase::Aborting)
                    && last_part(s, e)
                    && !entries_after(s, e).is_empty()
            },
            then: |s, e, out| {
                let (seq, entries) = (seq_of(e), entries_after(s, e));
                let Ok(out) = out else { return false };
                let back = &out.steps[&seq];
                let parts = entries.iter().map(|(t, e)| (*t, Part::Doing(*e))).collect();
                let kept: BTreeSet<Torrent> = entries.keys().copied().collect();
                back.what == What::Undo { of: seq, parts }
                    && back.phase == Phase::Running
                    && back.torrents == kept
                    && s.steps[&seq].torrents.iter().all(|t| (out.lanes.get(t).and_then(|l| l.front()) == Some(&seq)) == kept.contains(t))
                    && out.stack == s.stack
                    && new_left(s, out).is_empty()
            },
        },
        Row {
            says: "an undo whose last part ends is over; its action left: undone if a part came back, else refused (the first part's why), else dropped",
            when: |s, e| ends(e) && is_undo(target(s, e)) && last_part(s, e),
            then: |s, e, out| {
                let seq = seq_of(e);
                let What::Undo { of, parts } = &s.steps[&seq].what else { return false };
                let ended = match e {
                    Event::Observed(Observed::Finished(..)) => Undid::Back,
                    Event::Observed(Observed::Refused { why, stranded, .. }) => {
                        Undid::Refused { why: why.clone(), stranded: stranded.clone() }
                    }
                    _ => Undid::Failed,
                };
                let (_, t) = part_of(e).unwrap();
                let all: Vec<Undid> = parts
                    .iter()
                    .map(|(u, p)| match (p, *u == t) {
                        (Part::Over(o), false) => o.clone(),
                        (Part::Over(_) | Part::Doing(_), true) | (Part::Doing(_), false) => ended.clone(),
                    })
                    .collect();
                let why = match (all.contains(&Undid::Back), all.iter().find(|u| matches!(u, Undid::Refused { .. }))) {
                    (true, _) => Leave::Undone,
                    (false, Some(Undid::Refused { why, .. })) => Leave::Refused {
                        why: why.clone(),
                        stranded: all
                            .iter()
                            .flat_map(|u| match u {
                                Undid::Refused { stranded, .. } => stranded.clone(),
                                Undid::Back | Undid::Failed => Vec::new(),
                            })
                            .collect(),
                    },
                    (false, _) => Leave::Dropped,
                };
                out.as_ref().is_ok_and(|out| shape(out) == without(s, seq) && new_left(s, out) == [left(*of, why)])
            },
        },
        Row {
            says: "a command that ends is over",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(..)))
                    && target(s, e).is_some_and(|t| matches!(t.what, What::Command(_)))
            },
            then: |s, e, out| out.as_ref().is_ok_and(|out| shape(out) == without(s, seq_of(e)) && new_left(s, out).is_empty()),
        },
        Row {
            says: "news of a part that isn't under way changes nothing, nor a move of an undo or a command, nor a refusal of an action or a command",
            when: |s, e| {
                part_of(e).is_some()
                    && target(s, e).is_none_or(|t| {
                        (matches!(e, Event::Observed(Observed::Moved(..))) && !matches!(t.what, What::Action(_)))
                            || (matches!(e, Event::Observed(Observed::Refused { .. })) && !matches!(t.what, What::Undo { .. }))
                    })
            },
            then: |s, _, out| out.as_ref() == Ok(s),
        },
        Row {
            says: "a stop drops the actions and commands that wait; the undos, pressed, stay",
            when: |_, e| *e == Event::Observed(Observed::Stopping),
            then: |s, _, out| {
                let Ok(out) = out else { return false };
                s.steps.iter().all(|(seq, st)| {
                    let drops = st.phase == Phase::Queued && !matches!(st.what, What::Undo { .. });
                    out.steps.contains_key(seq) != drops && (!drops || out.stack.iter().all(|p| p.seq != *seq))
                }) && new_left(s, out).iter().all(|l| l.why == Leave::Dropped && s.stack.iter().any(|p| p.seq == l.seq))
            },
        },
        Row {
            says: "a start drops them too, and the commands running; what else ran is carried out again",
            when: |_, e| *e == Event::Observed(Observed::Restarted),
            then: |s, _, out| {
                let Ok(out) = out else { return false };
                s.steps.iter().all(|(seq, st)| {
                    let drops = match st.what {
                        What::Undo { .. } => false,
                        What::Command(_) => true,
                        What::Action(_) => st.phase == Phase::Queued,
                    };
                    out.steps.contains_key(seq) != drops
                }) && new_left(s, out).iter().all(|l| l.why == Leave::Dropped && s.stack.iter().any(|p| p.seq == l.seq))
            },
        },
    ]
}

/// What holds of every state the machine makes.
fn sound(s: &S) -> bool {
    let lanes_ordered = s.lanes.values().all(|l| l.iter().zip(l.iter().skip(1)).all(|(a, b)| a < b));
    let stack_ordered = s.stack.windows(2).all(|w| w[0].seq < w[1].seq) && s.stack.len() <= DEPTH;
    let in_their_lanes = s.steps.iter().all(|(seq, st)| {
        !st.torrents.is_empty()
            && s.lanes.iter().all(|(t, l)| l.iter().filter(|x| *x == seq).count() == usize::from(st.torrents.contains(t)))
    });
    // A part per torrent of an action or an undo, one under way at least; a command of one torrent.
    let parts = s.steps.values().all(|st| match &st.what {
        What::Action(parts) => {
            parts.keys().copied().collect::<BTreeSet<_>>() == st.torrents && parts.values().any(|p| matches!(p, Part::Doing(_)))
        }
        What::Undo { parts, .. } => {
            parts.keys().copied().collect::<BTreeSet<_>>() == st.torrents && parts.values().any(|p| matches!(p, Part::Doing(_)))
        }
        What::Command(_) => st.torrents.len() == 1,
    });
    let lanes_real = s.lanes.values().all(|l| !l.is_empty() && l.iter().all(|seq| s.steps.contains_key(seq)));
    // Running exactly when first in all its lanes: at most one per lane.
    let running = s.steps.iter().all(|(seq, st)| (st.phase != Phase::Queued) == s.first_everywhere(*seq));
    let one_per_lane = s.lanes.values().all(|l| l.iter().filter(|seq| s.steps[seq].phase != Phase::Queued).count() <= 1);
    let stacked = s.stack.iter().all(|p| match &p.done {
        Some(done) => !done.is_empty() && done.keys().copied().collect::<BTreeSet<_>>() == p.torrents,
        None => s.steps.get(&p.seq).is_some_and(|st| matches!(st.what, What::Action(_))),
    });
    let aborted_off = s.steps.iter().all(|(seq, st)| st.phase != Phase::Aborting || s.stack.iter().all(|p| p.seq != *seq));
    lanes_ordered && stack_ordered && in_their_lanes && parts && lanes_real && running && one_per_lane && stacked && aborted_off
}

fn decide_always() -> Vec<DecideRow<M>> {
    vec![
        Row::always("only ⌘Z is refused", |_, e, out| out.is_ok() || matches!(e, Event::Command(_))),
        Row::always("lanes in arrival order, one step running in each, the stack in press order", |_, _, out| {
            out.as_ref().map_or(true, sound)
        }),
        Row::always("the record of why entries left is bounded, in order, and never names an entry still on the stack", |_, _, out| {
            out.as_ref().map_or(true, |o| o.left.len() <= Left::KEPT && o.stack.iter().all(|p| o.left.iter().all(|l| l.seq != p.seq)))
        }),
        Row::always("what's on the stack stays, or leaves with a reason: no entry just goes", |s, e, out| {
            let Ok(out) = out else { return true };
            let undid = matches!(e, Event::Command(_));
            s.stack.iter().all(|p| {
                out.stack.iter().any(|q| q.seq == p.seq)
                    || out.left.iter().any(|l| l.seq == p.seq)
                    // ⌘Z takes it: its undo, or its abort, ends later.
                    || (undid && s.top().is_some_and(|t| t.seq == p.seq))
            })
        }),
        Row::always("⌘Z takes the newest action left, all of it", |s, e, out| match (e, out) {
            (Event::Command(_), Ok(out)) => out.stack.iter().map(|p| p.seq).eq(s.stack[..s.stack.len() - 1].iter().map(|p| p.seq)),
            _ => true,
        }),
    ]
}

#[test]
fn decide_follows_the_table() {
    for s in S::every() {
        assert!(sound(&s), "{s:?}");
    }
    decide_table::<M>(&decide_rows(), &decide_always());
}

#[test]
fn every_sequence_follows_the_table() {
    sequences::<M>(State::every(), 60, &decide_rows(), &decide_always());
}

impl Every for World {
    fn every() -> Vec<Self> {
        let set = |seqs: &[u64]| seqs.iter().copied().collect::<BTreeSet<u64>>();
        let parts = |parts: &[(u64, Torrent)]| parts.iter().copied().collect::<BTreeSet<_>>();
        vec![
            World::default(),
            World { started: parts(&[(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2), (3, 3)]), aborted: BTreeSet::new() },
            World { started: parts(&[(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2), (3, 3)]), aborted: set(&[1, 2]) },
            // A selection's part on 1 started, on 2 not yet.
            World { started: parts(&[(1, 1), (2, 1), (3, 1)]), aborted: BTreeSet::new() },
        ]
    }
}

#[test]
fn next_starts_each_part_under_way_once_and_aborts_each_aborting_step_once() {
    let rows: Vec<Row<S, World, Option<Do>>> = vec![
        Row {
            says: "a part under way that isn't started is started first",
            when: |s, w| s.steps.iter().any(|(seq, st)| st.underway().any(|t| !w.started.contains(&(*seq, t)))),
            then: |s, w, d| matches!(d, Some(Do::Start(seq, t)) if s.steps[seq].doing(*t) && !w.started.contains(&(*seq, *t))),
        },
        Row {
            says: "then one aborting is told to stop, once",
            when: |s, w| {
                s.steps.iter().all(|(seq, st)| st.underway().all(|t| w.started.contains(&(*seq, t))))
                    && s.steps.iter().any(|(seq, st)| st.phase == Phase::Aborting && !w.aborted.contains(seq))
            },
            then: |s, _, d| matches!(d, Some(Do::Abort(seq)) if s.steps[seq].phase == Phase::Aborting),
        },
        Row {
            says: "else nothing",
            when: |s, w| {
                s.steps.iter().all(|(seq, st)| {
                    st.underway().all(|t| w.started.contains(&(*seq, t))) && (st.phase != Phase::Aborting || w.aborted.contains(seq))
                })
            },
            then: |_, _, d| d.is_none(),
        },
    ];
    table(next, &rows, &[]);
}

/// A pretend actor around the machine: it starts what [`next`] says, and a part it's told to runs
/// moves an item of its torrent away (`away`), or ends; an undo's part puts back what its entry
/// says. It records the order parts start in.
#[derive(Debug, Clone, Default)]
struct Sim {
    state: S,
    world: World,
    away: BTreeMap<Torrent, i64>,
    started: Vec<(u64, Torrent)>,
    actions: usize,
}

/// What the generated sequences do.
#[derive(Debug, Clone)]
enum Op {
    /// A Remove or Delete of a selection: one step.
    Action(BTreeSet<Torrent>),
    Command(Torrent),
    Undo,
    /// The `n`th part under way (of those, by seq and torrent, wrapping) moves an item, or ends,
    /// or fails (an action's that moved nothing, an undo's refused).
    Move(usize),
    End(usize),
    Fail(usize),
    /// The service stops: what waits goes but the undos.
    Stop,
    /// The engine starts again, after a stop or a crash: what waited goes but the undos, and the
    /// actor carries out again what ran (it knows of nothing under way).
    Restart,
}

fn torrents() -> impl Strategy<Value = BTreeSet<Torrent>> {
    prop::collection::btree_set(0u64..3, 1..=3)
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => torrents().prop_map(Op::Action),
        1 => (0u64..3).prop_map(Op::Command),
        2 => Just(Op::Undo),
        3 => (0usize..6).prop_map(Op::Move),
        3 => (0usize..6).prop_map(Op::End),
        1 => (0usize..6).prop_map(Op::Fail),
        1 => Just(Op::Stop),
        1 => Just(Op::Restart),
    ]
}

impl Sim {
    fn apply(&mut self, e: &E) {
        let Ok(next) = decide(&self.state, e) else { return };
        assert!(sound(&next), "{e:?} made {next:?}");
        self.state = next;
        self.world.follow(e, &self.state);
        while let Some(d) = super::next(&self.state, &self.world) {
            match d {
                Do::Start(seq, t) => {
                    self.world.started.insert((seq, t));
                    self.started.push((seq, t));
                }
                Do::Abort(seq) => drop(self.world.aborted.insert(seq)),
            }
        }
    }

    /// The parts under way, by seq and torrent.
    fn underway(&self) -> Vec<(u64, Torrent)> {
        self.state.steps.iter().flat_map(|(seq, s)| s.underway().map(move |t| (*seq, t))).collect()
    }

    fn arrive(&mut self, what: Arrival<Run, Cmd>) {
        self.actions += usize::from(matches!(what, Arrival::Action(_)));
        self.apply(&Event::Observed(Observed::Arrived { seq: 0, what }));
    }

    /// Part `t` of running step `seq` moves an item: an action's only.
    fn moves(&mut self, seq: u64, t: Torrent) {
        let Some(Step { what: What::Action(parts), .. }) = self.state.steps.get(&seq) else { return };
        let Some(Part::Doing(Run(n))) = parts.get(&t) else { return };
        let n = *n;
        *self.away.entry(t).or_default() += 1;
        self.apply(&Event::Observed(Observed::Moved(seq, t, Run(n + 1))));
    }

    /// Part `t` of running step `seq` ends: an action's with what it moved; an undo's putting it
    /// back. `fail`: an action's that moved nothing fails, an undo's is refused, keeping it away.
    fn ends(&mut self, seq: u64, t: Torrent, fail: bool) {
        let Some(step) = self.state.steps.get(&seq).cloned() else { return };
        let event = match (step.what, fail) {
            (What::Action(parts), _) => match parts.get(&t) {
                Some(Part::Doing(Run(0))) if fail => Event::Observed(Observed::Failed(seq, t)),
                Some(Part::Doing(Run(n))) => finished(seq, t, *n),
                Some(Part::Over(_)) | None => return,
            },
            (What::Undo { parts, .. }, false) => {
                let Some(Part::Doing(entry)) = parts.get(&t) else { return };
                *self.away.entry(t).or_default() -= i64::from(entry.items);
                Event::Observed(Observed::Finished(seq, t, None))
            }
            (What::Undo { .. }, true) => {
                Event::Observed(Observed::Refused { seq, torrent: t, why: CantUndo::NotKept, stranded: Vec::new() })
            }
            (What::Command(_), _) => Event::Observed(Observed::Finished(seq, t, None)),
        };
        self.apply(&event);
    }

    fn run(&mut self, op: &Op) {
        match op {
            Op::Action(t) => self.arrive(Arrival::Action(t.iter().map(|t| (*t, Run(0))).collect())),
            Op::Command(t) => self.arrive(Arrival::Command(*t, Cmd(0))),
            Op::Undo => self.apply(&UNDO),
            Op::Stop => self.apply(&Event::Observed(Observed::Stopping)),
            Op::Restart => {
                self.world = World::default();
                self.apply(&Event::Observed(Observed::Restarted));
            }
            Op::Move(n) | Op::End(n) | Op::Fail(n) => {
                let underway = self.underway();
                if underway.is_empty() {
                    return;
                }
                let (seq, t) = underway[n % underway.len()];
                match op {
                    Op::Move(_) => self.moves(seq, t),
                    Op::End(_) => self.ends(seq, t, false),
                    _ => self.ends(seq, t, true),
                }
            }
        }
    }

    /// Every part under way ends, but those of steps touching `slow`, until none is left that can.
    fn drain(&mut self, slow: Option<Torrent>) {
        for _ in 0..1000 {
            let can: Vec<(u64, Torrent)> =
                self.underway().into_iter().filter(|(seq, _)| slow.is_none_or(|x| !self.state.steps[seq].torrents.contains(&x))).collect();
            let Some((seq, t)) = can.first() else { return };
            self.ends(*seq, *t, false);
        }
        panic!("never drained: {:?}", self.state);
    }
}

proptest! {
    /// Once every part under way ends as its moves report, every step ends: no lane waits forever
    /// on another (steps enter all their lanes at once, so lanes that share them agree on order).
    /// Each lane ran its steps one at a time, in the order they came, a selection's among them.
    #[test]
    fn every_step_ends_once_the_running_ones_report_in_arrival_order_per_lane(ops in prop::collection::vec(op(), 0..60)) {
        let mut sim = Sim::default();
        for op in &ops {
            sim.run(op);
        }
        sim.drain(None);
        prop_assert!(sim.state.steps.is_empty() && sim.state.lanes.is_empty(), "{:?}", sim.state);
        for t in 0..3 {
            let mut order: Vec<u64> = sim.started.iter().filter(|(_, u)| *u == t).map(|(seq, _)| *seq).collect();
            // A rollback runs under its action's seq, in its place, straight after it.
            order.dedup();
            prop_assert!(order.windows(2).all(|w| w[0] < w[1]), "lane {t} ran {order:?}");
        }
    }

    /// With one torrent's parts never ending (a stalled drive), every step ends but those that touch
    /// it or wait behind one that waits: a slow step delays another torrent's only through a
    /// selection that holds both.
    #[test]
    fn a_slow_step_delays_another_torrents_only_through_a_selection_of_both(ops in prop::collection::vec(op(), 0..60), slow in 0u64..3) {
        let mut sim = Sim::default();
        for op in &ops {
            sim.run(op);
        }
        sim.drain(Some(slow));
        // Waiting on it: a step that touches it, or one behind such a step in one of its lanes.
        let mut waits: BTreeSet<u64> = sim.state.steps.iter().filter(|(_, s)| s.torrents.contains(&slow)).map(|(seq, _)| *seq).collect();
        loop {
            let more: BTreeSet<u64> = sim
                .state
                .lanes
                .values()
                .flat_map(|lane| lane.iter().skip_while(|seq| !waits.contains(seq)).copied().collect::<Vec<_>>())
                .collect();
            if more.is_subset(&waits) {
                break;
            }
            waits.extend(more);
        }
        for (seq, step) in &sim.state.steps {
            prop_assert!(waits.contains(seq), "step {seq} on {:?} waits though nothing it waits behind touches {slow}", step.torrents);
        }
    }

    /// Any actions, selections among them, with moves and ends among them, then as many ⌘Z, with
    /// more moves and ends among those (so ⌘Z meets actions queued, running, part done and done),
    /// then every step ending: everything is back where it was, and nothing is left on the stack.
    /// One ⌘Z brings back a whole selection.
    #[test]
    fn as_many_undos_as_actions_bring_back_the_start(
        before in prop::collection::vec(op().prop_filter("no ⌘Z", |o| !matches!(o, Op::Undo)), 0..30),
        between in prop::collection::vec(
            prop_oneof![8 => (0usize..6).prop_map(Op::Move), 8 => (0usize..6).prop_map(Op::End), 1 => Just(Op::Stop), 1 => Just(Op::Restart)],
            0..40,
        ),
    ) {
        let mut sim = Sim::default();
        for op in &before {
            sim.run(op);
        }
        prop_assume!(sim.actions <= DEPTH);
        let mut between = between.into_iter();
        for _ in 0..sim.actions {
            sim.run(&Op::Undo);
            if let Some(op) = between.next() {
                sim.run(&op);
            }
        }
        for op in between {
            sim.run(&op);
        }
        sim.drain(None);
        prop_assert!(sim.away.values().all(|n| *n == 0), "not all back: {:?}", sim.away);
        prop_assert!(sim.state.stack.is_empty() && sim.state.steps.is_empty(), "{:?}", sim.state);
    }
}

/// Past 20 actions the oldest is off the stack, and those left come off newest first.
#[test]
fn past_twenty_the_oldest_is_gone() {
    let mut s = S::default();
    for _ in 0..DEPTH + 5 {
        s = decide(&s, &action(&[1])).unwrap();
        let seq = s.top().unwrap().seq;
        s = decide(&s, &finished(seq, 1, 1)).unwrap();
    }
    let mut taken = Vec::new();
    while let Ok(next) = decide(&s, &UNDO) {
        taken.push(next.steps.values().find_map(|st| match st.what {
            What::Undo { of, .. } if !s.steps.values().any(|o| matches!(o.what, What::Undo { of: was, .. } if was == of)) => Some(of),
            _ => None,
        }));
        s = next;
    }
    assert_eq!(taken, (6..=DEPTH as u64 + 5).rev().map(Some).collect::<Vec<_>>());
}

/// The user's sequence (#190), on the machine: Delete A, Delete B, ⌘Z, ⌘Z while both still run.
/// Each ⌘Z takes the next one down at once, and both roll back side by side.
#[test]
fn delete_a_delete_b_undo_undo_aborts_b_then_a_and_both_roll_back_at_once() {
    let mut sim = Sim::default();
    sim.run(&Op::Action(on(&[1])));
    sim.run(&Op::Action(on(&[2])));
    sim.moves(1, 1);
    sim.moves(2, 2);
    sim.run(&Op::Undo);
    assert_eq!(sim.state.steps[&2].phase, Phase::Aborting, "B first");
    sim.run(&Op::Undo);
    assert_eq!(sim.state.steps[&1].phase, Phase::Aborting, "then A");
    assert!(sim.state.stack.is_empty());
    sim.ends(1, 1, false);
    sim.ends(2, 2, false);
    let rollbacks: Vec<(u64, Phase)> = sim
        .state
        .steps
        .values()
        .map(|st| match st.what {
            What::Undo { of, .. } => (of, st.phase),
            What::Action(_) | What::Command(_) => (0, st.phase),
        })
        .collect();
    assert_eq!(rollbacks, [(1, Phase::Running), (2, Phase::Running)], "side by side");
    sim.drain(None);
    assert!(sim.away.values().all(|n| *n == 0));
}

/// The user's sequence (#190), on the machine: Move to Trash file 1 of A, file 2 of A, Delete A,
/// ⌘Z ×3, while the first move runs. The Delete and the second move, queued behind it, are dropped;
/// the first is aborted, then rolled back.
#[test]
fn two_moves_and_a_delete_of_one_torrent_then_three_undos_unwind_it_in_order() {
    let mut sim = Sim::default();
    for _ in 0..3 {
        sim.run(&Op::Action(on(&[1])));
    }
    sim.moves(1, 1);
    for _ in 0..3 {
        sim.run(&Op::Undo);
    }
    assert_eq!(sim.state.steps.keys().copied().collect::<Vec<_>>(), [1], "the Delete and the second move never started");
    assert_eq!(sim.state.steps[&1].phase, Phase::Aborting);
    sim.drain(None);
    assert!(sim.away.values().all(|n| *n == 0) && sim.started.len() == 2, "the first and its rollback: {:?}", sim.started);
}

/// The same, once all three are done: their undos queue on A's lane in press order, newest first.
#[test]
fn undos_of_one_torrents_steps_run_on_its_lane_in_press_order() {
    let mut sim = Sim::default();
    for seq in 1..=3 {
        sim.run(&Op::Action(on(&[1])));
        sim.moves(seq, 1);
        sim.ends(seq, 1, false);
    }
    for _ in 0..3 {
        sim.run(&Op::Undo);
    }
    let lane: Vec<u64> = sim.state.lanes[&1]
        .iter()
        .map(|seq| match sim.state.steps[seq].what {
            What::Undo { of, .. } => of,
            What::Action(_) | What::Command(_) => 0,
        })
        .collect();
    assert_eq!(lane, [3, 2, 1]);
    sim.drain(None);
    assert!(sim.away.values().all(|n| *n == 0));
}

/// Remove three torrents in one press, ⌘Z: one entry, and one ⌘Z brings all three back, their
/// parts side by side (#235).
#[test]
fn a_selection_of_three_is_one_entry_and_one_undo_brings_all_three_back() {
    let mut sim = Sim::default();
    sim.run(&Op::Action(on(&[0, 1, 2])));
    assert_eq!(sim.state.stack.len(), 1, "one press, one entry");
    for t in 0..3 {
        sim.moves(1, t);
    }
    for t in 0..3 {
        sim.ends(1, t, false);
    }
    assert_eq!(sim.state.stack[0].torrents, on(&[0, 1, 2]));
    sim.run(&Op::Undo);
    assert!(sim.state.stack.is_empty());
    assert_eq!(sim.underway().len(), 3, "its three parts put back side by side");
    sim.drain(None);
    assert!(sim.away.values().all(|n| *n == 0), "all three back: {:?}", sim.away);
    assert_eq!(sim.state.left.back(), Some(&left(1, Leave::Undone)));
}

/// A selection whose part on one torrent changed nothing (it was refused) is one entry holding what
/// did happen; its undo is of the others only.
#[test]
fn a_selection_failed_in_part_holds_what_did_happen() {
    let mut sim = Sim::default();
    sim.run(&Op::Action(on(&[0, 1, 2])));
    sim.moves(1, 0);
    sim.moves(1, 2);
    sim.ends(1, 1, true);
    sim.ends(1, 0, false);
    sim.ends(1, 2, false);
    assert_eq!(sim.state.stack.iter().map(|p| p.torrents.clone()).collect::<Vec<_>>(), [on(&[0, 2])]);
    sim.run(&Op::Undo);
    assert_eq!(sim.state.steps.values().next().map(|s| s.torrents.clone()), Some(on(&[0, 2])));
    sim.drain(None);
    assert!(sim.away.values().all(|n| *n == 0));
}

/// The record of why entries left holds the last [`Left::KEPT`], the newest.
#[test]
fn the_record_of_why_entries_left_is_bounded_to_the_newest() {
    let mut s = S::default();
    for _ in 0..DEPTH + Left::KEPT + 10 {
        s = decide(&s, &action(&[1])).unwrap();
    }
    assert_eq!(s.left.len(), Left::KEPT);
    assert!(s.left.iter().all(|l| l.why == Leave::Aged));
    assert!(s.left.iter().map(|l| l.seq).collect::<Vec<_>>().windows(2).all(|w| w[0] < w[1]), "oldest first");
    assert_eq!(s.left.back().map(|l| l.seq), Some(s.stack[0].seq - 1), "the newest to leave is the one just under the stack");
}
