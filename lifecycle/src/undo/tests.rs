//! Undo's machine over every state × event (the table), and over generated sequences against a
//! pretend actor that moves items and reports them: lanes, the stack, no deadlock, no torrent
//! waiting on another's, and undos bringing back the start.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;
use crate::every;
use crate::machine::{DecideRow, Every, Row, decide_table, sequences, table};

/// What an action did so far: the items it moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run(u8);

/// What undoes an action: the items it moved, of each torrent it touched. A partial Delete, which a
/// later Delete of the same torrents absorbs; a Move to Trash, which absorbs none and is absorbed by
/// none.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    items: u8,
    partial: bool,
    files: bool,
}

impl Undoes for Entry {
    fn absorbs(&self, older: &Self) -> bool {
        !self.files && !older.files && older.partial
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
    Event::Observed(Observed::Arrived { seq: 0, torrents: on(torrents), what: Arrival::Action(Run(0)) })
}

fn command(torrents: &[Torrent]) -> E {
    Event::Observed(Observed::Arrived { seq: 0, torrents: on(torrents), what: Arrival::Command(Cmd(1)) })
}

fn finished(seq: u64, items: u8) -> E {
    Event::Observed(Observed::Finished(seq, (items > 0).then_some(Entry { items, partial: false, files: false })))
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
        after(&[action(&[2]), finished(1, 2), action(&[1]), action(&[1]), command(&[1])]),
        // 1 committed then undone: its undo (3) running on 2; 2 running on 1, aborted by ⌘Z.
        after(&[action(&[2]), finished(1, 1), action(&[1]), UNDO, UNDO]),
        // 1 running on 1, a step for 1 and 2 (2) waiting for it, 3 running on 3.
        after(&[action(&[1]), action(&[1, 2]), action(&[3])]),
        // The stack full of committed ones, a partial one among them.
        (0..DEPTH as u64).fold(S::default(), |s, n| {
            let s = decide(&s, &action(&[n % 3])).unwrap();
            let seq = s.top().unwrap().seq;
            decide(&s, &Event::Observed(Observed::Finished(seq, Some(Entry { items: 1, partial: n == 3, files: false })))).unwrap()
        }),
    ],
});

every!(Event<Run, Entry, Cmd> {
    Event::Command(Command::Undo) => [UNDO],
    Event::Observed(Observed::Arrived { .. }) => [action(&[1]), action(&[1, 2]), command(&[2])],
    Event::Observed(Observed::Moved(..)) => [1, 2, 3, 4, 99].map(|seq| Event::Observed(Observed::Moved(seq, Run(1)))),
    Event::Observed(Observed::Finished(..)) => [1, 2, 3, 4, 99]
        .into_iter()
        .flat_map(|seq| [finished(seq, 0), finished(seq, 2)])
        .chain([Event::Observed(Observed::Finished(2, Some(Entry { items: 1, partial: false, files: true })))]),
    Event::Observed(Observed::Failed(_)) => [1, 2, 3, 4, 99].map(|seq| Event::Observed(Observed::Failed(seq))),
    Event::Observed(Observed::Stopping) => [Event::Observed(Observed::Stopping)],
    Event::Observed(Observed::Restarted) => [Event::Observed(Observed::Restarted)],
});

/// The step an event names, as it is in `s`.
fn target<'a>(s: &'a S, e: &E) -> Option<&'a Step<Run, Entry, Cmd>> {
    match e {
        Event::Observed(Observed::Moved(seq, _) | Observed::Finished(seq, _) | Observed::Failed(seq)) => s.steps.get(seq),
        Event::Command(_) | Event::Observed(Observed::Arrived { .. } | Observed::Stopping | Observed::Restarted) => None,
    }
}

fn seq_of(e: &E) -> u64 {
    match e {
        Event::Observed(Observed::Moved(seq, _) | Observed::Finished(seq, _) | Observed::Failed(seq)) => *seq,
        Event::Command(_) | Event::Observed(Observed::Arrived { .. } | Observed::Stopping | Observed::Restarted) => u64::MAX,
    }
}

fn is_action(s: Option<&Step<Run, Entry, Cmd>>, phases: &[Phase]) -> bool {
    s.is_some_and(|s| matches!(s.what, What::Action(_)) && phases.contains(&s.phase))
}

/// `s` without step `seq`, in its lanes and on the stack, but its steps' phases as `out` has them.
fn without(s: &S, seq: u64) -> (Vec<u64>, BTreeSet<u64>) {
    (s.stack.iter().map(|p| p.seq).filter(|p| *p != seq).collect(), s.steps.keys().copied().filter(|k| *k != seq).collect())
}

fn shape(s: &S) -> (Vec<u64>, BTreeSet<u64>) {
    (s.stack.iter().map(|p| p.seq).collect(), s.steps.keys().copied().collect())
}

fn decide_rows() -> Vec<DecideRow<M>> {
    vec![
        Row {
            says: "⌘Z with nothing on the stack is refused",
            when: |s, e| *e == UNDO && s.stack.is_empty(),
            then: |_, _, out| *out == Err(Refusal::Nothing),
        },
        Row {
            says: "⌘Z on a committed action takes it off the stack, and its undo arrives in its torrents' lanes",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| p.done.is_some()),
            then: |s, _, out| {
                let (Ok(out), Some(top)) = (out, s.top()) else { return false };
                let undo = out.steps.iter().find(|(_, st)| matches!(st.what, What::Undo { of, .. } if of == top.seq));
                out.stack[..] == s.stack[..s.stack.len() - 1]
                    && undo.is_some_and(|(seq, st)| {
                        st.torrents == top.torrents && st.torrents.iter().all(|t| out.lanes[t].back() == Some(seq))
                    })
            },
        },
        Row {
            says: "⌘Z on an action queued drops it: off the stack and its lanes",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| s.steps.get(&p.seq).is_some_and(|st| st.phase == Phase::Queued)),
            then: |s, _, out| out.as_ref().is_ok_and(|out| shape(out) == without(s, s.top().unwrap().seq)),
        },
        Row {
            says: "⌘Z on an action running aborts it: off the stack, still in its lanes",
            when: |s, e| *e == UNDO && s.top().is_some_and(|p| s.steps.get(&p.seq).is_some_and(|st| st.phase != Phase::Queued)),
            then: |s, _, out| {
                let (Ok(out), Some(top)) = (out, s.top()) else { return false };
                out.stack[..] == s.stack[..s.stack.len() - 1] && out.steps[&top.seq].phase == Phase::Aborting
            },
        },
        Row {
            says: "a step arrives at the end of each of its lanes; an action on top of the stack too",
            when: |_, e| matches!(e, Event::Observed(Observed::Arrived { .. })),
            then: |s, e, out| {
                let (Event::Observed(Observed::Arrived { torrents, what, .. }), Ok(out)) = (e, out) else { return false };
                let seq = s.last_seq() + 1;
                let stacked = match what {
                    Arrival::Action(_) => out.top().is_some_and(|p| p.seq == seq && p.done.is_none()),
                    Arrival::Command(_) => out.stack == s.stack,
                };
                stacked && out.steps[&seq].torrents == *torrents && torrents.iter().all(|t| out.lanes[t].back() == Some(&seq))
            },
        },
        Row {
            says: "what a running action did is logged in it",
            when: |s, e| matches!(e, Event::Observed(Observed::Moved(..))) && is_action(target(s, e), &[Phase::Running, Phase::Aborting]),
            then: |_, e, out| {
                let (Event::Observed(Observed::Moved(seq, r)), Ok(out)) = (e, out) else { return false };
                out.steps[seq].what == What::Action(*r)
            },
        },
        Row {
            says: "a running action that changed something is committed in its place, without what it absorbs",
            when: |s, e| matches!(e, Event::Observed(Observed::Finished(_, Some(_)))) && is_action(target(s, e), &[Phase::Running]),
            then: |s, e, out| {
                let (Event::Observed(Observed::Finished(seq, Some(entry))), Ok(out)) = (e, out) else { return false };
                let want: Vec<(u64, Option<Entry>)> = s
                    .stack
                    .iter()
                    .filter(|p| p.seq == *seq || !p.done.as_ref().is_some_and(|old| entry.absorbs(old)))
                    .map(|p| (p.seq, if p.seq == *seq { Some(*entry) } else { p.done }))
                    .collect();
                !out.steps.contains_key(seq) && out.stack.iter().map(|p| (p.seq, p.done)).collect::<Vec<_>>() == want
            },
        },
        Row {
            says: "an action that changed nothing is gone, off the stack and its lanes",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(_, None) | Observed::Failed(_)))
                    && is_action(target(s, e), &[Phase::Running, Phase::Aborting])
            },
            then: |s, e, out| out.as_ref().is_ok_and(|out| shape(out) == without(s, seq_of(e))),
        },
        Row {
            says: "an aborted action that changed something is rolled back in its place: its undo, running",
            when: |s, e| matches!(e, Event::Observed(Observed::Finished(_, Some(_)))) && is_action(target(s, e), &[Phase::Aborting]),
            then: |s, e, out| {
                let (Event::Observed(Observed::Finished(seq, Some(entry))), Ok(out)) = (e, out) else { return false };
                let back = &out.steps[seq];
                back.what == What::Undo { of: *seq, entry: *entry }
                    && back.phase == Phase::Running
                    && back.torrents.iter().all(|t| out.lanes[t].front() == Some(seq))
                    && out.lanes == s.lanes
                    && out.stack == s.stack
            },
        },
        Row {
            says: "an undo or a command that ends is over",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Finished(..) | Observed::Failed(_)))
                    && target(s, e).is_some_and(|t| !matches!(t.what, What::Action(_)) && t.phase != Phase::Queued)
            },
            then: |s, e, out| out.as_ref().is_ok_and(|out| shape(out) == without(s, seq_of(e))),
        },
        Row {
            says: "news of a step that isn't running changes nothing",
            when: |s, e| {
                matches!(e, Event::Observed(Observed::Moved(..) | Observed::Finished(..) | Observed::Failed(_)))
                    && target(s, e).is_none_or(|t| {
                        t.phase == Phase::Queued
                            || (matches!(e, Event::Observed(Observed::Moved(..))) && !matches!(t.what, What::Action(_)))
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
                })
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
                })
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
    let lanes_real = s.lanes.values().all(|l| !l.is_empty() && l.iter().all(|seq| s.steps.contains_key(seq)));
    // Running exactly when first in all its lanes: at most one per lane.
    let running = s.steps.iter().all(|(seq, st)| (st.phase != Phase::Queued) == s.first_everywhere(*seq));
    let one_per_lane = s.lanes.values().all(|l| l.iter().filter(|seq| s.steps[seq].phase != Phase::Queued).count() <= 1);
    let stacked = s.stack.iter().all(|p| p.done.is_some() || s.steps.get(&p.seq).is_some_and(|st| matches!(st.what, What::Action(_))));
    let aborted_off = s.steps.iter().all(|(seq, st)| st.phase != Phase::Aborting || s.stack.iter().all(|p| p.seq != *seq));
    lanes_ordered && stack_ordered && in_their_lanes && lanes_real && running && one_per_lane && stacked && aborted_off
}

fn decide_always() -> Vec<DecideRow<M>> {
    vec![
        Row::always("only ⌘Z is refused", |_, e, out| out.is_ok() || matches!(e, Event::Command(_))),
        Row::always("lanes in arrival order, one step running in each, the stack in press order", |_, _, out| {
            out.as_ref().map_or(true, sound)
        }),
        Row::always("⌘Z takes the newest action left", |s, e, out| match (e, out) {
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
        vec![
            World::default(),
            World { started: set(&[1, 2, 3]), aborted: BTreeSet::new() },
            World { started: set(&[1, 2, 3]), aborted: set(&[2]) },
        ]
    }
}

#[test]
fn next_starts_each_running_step_once_and_aborts_each_aborting_one_once() {
    let rows: Vec<Row<S, World, Option<Do>>> = vec![
        Row {
            says: "a step running or aborting that isn't started is started first",
            when: |s, w| s.steps.iter().any(|(seq, st)| st.phase != Phase::Queued && !w.started.contains(seq)),
            then: |s, w, d| matches!(d, Some(Do::Start(seq)) if s.steps[seq].phase != Phase::Queued && !w.started.contains(seq)),
        },
        Row {
            says: "then one aborting is told to stop, once",
            when: |s, w| {
                s.steps.iter().all(|(seq, st)| st.phase == Phase::Queued || w.started.contains(seq))
                    && s.steps.iter().any(|(seq, st)| st.phase == Phase::Aborting && !w.aborted.contains(seq))
            },
            then: |s, _, d| matches!(d, Some(Do::Abort(seq)) if s.steps[seq].phase == Phase::Aborting),
        },
        Row {
            says: "else nothing",
            when: |s, w| {
                s.steps.iter().all(|(seq, st)| match st.phase {
                    Phase::Queued => true,
                    Phase::Running => w.started.contains(seq),
                    Phase::Aborting => w.started.contains(seq) && w.aborted.contains(seq),
                })
            },
            then: |_, _, d| d.is_none(),
        },
    ];
    table(next, &rows, &[]);
}

/// A pretend actor around the machine: it starts what [`next`] says, and a step it's told to runs
/// moves an item of each of its torrents away (`away`), or ends; an undo puts back what its entry
/// says. It records the order steps start in, by torrent.
#[derive(Debug, Clone, Default)]
struct Sim {
    state: S,
    world: World,
    away: BTreeMap<Torrent, i64>,
    started: Vec<u64>,
    /// Each step's torrents, as it arrived.
    torrents: BTreeMap<u64, BTreeSet<Torrent>>,
    actions: usize,
}

/// What the generated sequences do.
#[derive(Debug, Clone)]
enum Op {
    Action(BTreeSet<Torrent>),
    Command(BTreeSet<Torrent>),
    Undo,
    /// The `n`th running step (of those running, by seq, wrapping) moves an item, or ends.
    Move(usize),
    End(usize),
    /// The service stops: what waits goes but the undos.
    Stop,
    /// The engine starts again, after a stop or a crash: what waited goes but the undos, and the
    /// actor carries out again what ran (it knows of nothing under way).
    Restart,
}

fn torrents() -> impl Strategy<Value = BTreeSet<Torrent>> {
    prop::collection::btree_set(0u64..3, 1..=2)
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => torrents().prop_map(Op::Action),
        1 => torrents().prop_map(Op::Command),
        2 => Just(Op::Undo),
        3 => (0usize..4).prop_map(Op::Move),
        3 => (0usize..4).prop_map(Op::End),
        1 => Just(Op::Stop),
        1 => Just(Op::Restart),
    ]
}

/// [`op`], each step on one torrent.
fn one_torrent_op() -> impl Strategy<Value = Op> {
    op().prop_map(|op| match op {
        Op::Action(t) => Op::Action(t.into_iter().take(1).collect()),
        Op::Command(t) => Op::Command(t.into_iter().take(1).collect()),
        other @ (Op::Undo | Op::Move(_) | Op::End(_) | Op::Stop | Op::Restart) => other,
    })
}

impl Sim {
    fn apply(&mut self, e: &E) {
        let Ok(next) = decide(&self.state, e) else { return };
        // What it reports the end of, it no longer carries out.
        if let Event::Observed(Observed::Finished(seq, _) | Observed::Failed(seq)) = e {
            self.world.started.remove(seq);
            self.world.aborted.remove(seq);
        }
        assert!(sound(&next), "{e:?} made {next:?}");
        self.state = next;
        while let Some(d) = super::next(&self.state, &self.world) {
            match d {
                Do::Start(seq) => {
                    self.world.started.insert(seq);
                    self.started.push(seq);
                }
                Do::Abort(seq) => drop(self.world.aborted.insert(seq)),
            }
        }
    }

    fn running(&self) -> Vec<u64> {
        self.state.steps.iter().filter(|(_, s)| s.phase != Phase::Queued).map(|(seq, _)| *seq).collect()
    }

    fn arrive(&mut self, torrents: &BTreeSet<Torrent>, what: Arrival<Run, Cmd>) {
        let seq = self.state.last_seq() + 1;
        self.torrents.insert(seq, torrents.clone());
        self.actions += usize::from(matches!(what, Arrival::Action(_)));
        self.apply(&Event::Observed(Observed::Arrived { seq: 0, torrents: torrents.clone(), what }));
    }

    /// Running step `seq` moves one item of each of its torrents: an action only.
    fn moves(&mut self, seq: u64) {
        let Some(step) = self.state.steps.get(&seq) else { return };
        let What::Action(Run(n)) = step.what else { return };
        for t in &step.torrents {
            *self.away.entry(*t).or_default() += 1;
        }
        self.apply(&Event::Observed(Observed::Moved(seq, Run(n + 1))));
    }

    /// Running step `seq` ends: an action with what it moved; an undo putting it back.
    fn ends(&mut self, seq: u64) {
        let Some(step) = self.state.steps.get(&seq).cloned() else { return };
        let event = match step.what {
            What::Action(Run(n)) => finished(seq, n),
            What::Undo { entry, .. } => {
                for t in &step.torrents {
                    *self.away.entry(*t).or_default() -= i64::from(entry.items);
                }
                Event::Observed(Observed::Finished(seq, None))
            }
            What::Command(_) => Event::Observed(Observed::Finished(seq, None)),
        };
        self.apply(&event);
    }

    fn run(&mut self, op: &Op) {
        match op {
            Op::Action(t) => self.arrive(t, Arrival::Action(Run(0))),
            Op::Command(t) => self.arrive(t, Arrival::Command(Cmd(0))),
            Op::Undo => self.apply(&UNDO),
            Op::Stop => self.apply(&Event::Observed(Observed::Stopping)),
            Op::Restart => {
                self.world = World::default();
                self.apply(&Event::Observed(Observed::Restarted));
            }
            Op::Move(n) | Op::End(n) => {
                let running = self.running();
                if running.is_empty() {
                    return;
                }
                let seq = running[n % running.len()];
                match op {
                    Op::Move(_) => self.moves(seq),
                    _ => self.ends(seq),
                }
            }
        }
    }

    /// Every running step ends, but those touching `slow`, until none is left that can.
    fn drain(&mut self, slow: Option<Torrent>) {
        for _ in 0..1000 {
            let can: Vec<u64> =
                self.running().into_iter().filter(|seq| slow.is_none_or(|x| !self.state.steps[seq].torrents.contains(&x))).collect();
            let Some(seq) = can.first() else { return };
            self.ends(*seq);
        }
        panic!("never drained: {:?}", self.state);
    }
}

proptest! {
    /// Once every running step ends as its moves report, every step ends: no lane waits forever on
    /// another (steps enter all their lanes at once, so lanes that share them agree on order).
    /// Each lane ran its steps one at a time, in the order they came.
    #[test]
    fn every_step_ends_once_the_running_ones_report_in_arrival_order_per_lane(ops in prop::collection::vec(op(), 0..60)) {
        let mut sim = Sim::default();
        for op in &ops {
            sim.run(op);
        }
        sim.drain(None);
        prop_assert!(sim.state.steps.is_empty() && sim.state.lanes.is_empty(), "{:?}", sim.state);
        for t in 0..3 {
            let order: Vec<u64> = sim.started.iter().copied().filter(|seq| sim.torrents.get(seq).is_some_and(|ts| ts.contains(&t))).collect();
            // A rollback runs under its action's seq, in its place, straight after it.
            let mut order = order;
            order.dedup();
            prop_assert!(order.windows(2).all(|w| w[0] < w[1]), "lane {t} ran {order:?}");
        }
    }

    /// With one torrent's steps never ending (a stalled drive), every step that doesn't touch it ends
    /// all the same: a slow step never delays another torrent's. Of steps each on one torrent, as the
    /// engine's are: one on two waits to be first in both, so it carries a wait from one to the other.
    #[test]
    fn a_slow_step_never_delays_another_torrents(ops in prop::collection::vec(one_torrent_op(), 0..60), slow in 0u64..3) {
        let mut sim = Sim::default();
        for op in &ops {
            sim.run(op);
        }
        sim.drain(Some(slow));
        for (seq, step) in &sim.state.steps {
            prop_assert!(step.torrents.contains(&slow), "step {seq} on {:?} waits though it doesn't touch {slow}", step.torrents);
        }
    }

    /// Any actions, with moves and ends among them, then as many ⌘Z, with more moves and ends among
    /// those (so ⌘Z meets actions queued, running and done), then every step ending: everything is
    /// back where it was, and nothing is left on the stack.
    #[test]
    fn as_many_undos_as_actions_bring_back_the_start(
        before in prop::collection::vec(op().prop_filter("no ⌘Z", |o| !matches!(o, Op::Undo)), 0..30),
        between in prop::collection::vec(
            prop_oneof![8 => (0usize..4).prop_map(Op::Move), 8 => (0usize..4).prop_map(Op::End), 1 => Just(Op::Stop), 1 => Just(Op::Restart)],
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
        s = decide(&s, &finished(seq, 1)).unwrap();
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
    sim.moves(1);
    sim.moves(2);
    sim.run(&Op::Undo);
    assert_eq!(sim.state.steps[&2].phase, Phase::Aborting, "B first");
    sim.run(&Op::Undo);
    assert_eq!(sim.state.steps[&1].phase, Phase::Aborting, "then A");
    assert!(sim.state.stack.is_empty());
    sim.ends(1);
    sim.ends(2);
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
    sim.moves(1);
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
        sim.moves(seq);
        sim.ends(seq);
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
