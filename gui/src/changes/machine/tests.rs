use std::collections::BTreeMap;

use lifecycle::every;
use lifecycle::machine::{DecideRow, Every, Row, decide_table, sequences};
use lifecycle::settings::Changes as ServiceChanges;
use proptest::prelude::*;

use super::*;

const ICON: AppSetting = AppSetting::Icon;
const PORT: SettingName = SettingName::ListenPort;

fn app(id: u64, press: Press) -> Change {
    Change::App { id, of: ICON, press }
}

fn sent(by: u64, press: Press) -> Change {
    Change::Sent { by, name: PORT, press }
}

fn listed(seq: u64, press: Press) -> Change {
    Change::Listed { seq, name: PORT, press }
}

fn change(seq: u64, by: Option<u64>) -> SettingChange {
    SettingChange { seq, name: PORT, by }
}

fn state(list: Vec<Change>, seen: u64) -> State {
    State { list, seen }
}

// Changes 1 and 2 (app ids or service seqs), sent by 10; the service lists seqs 1 to 3.
every!(State {
    State { .. } => [
        state(vec![], 0),
        state(vec![app(1, Press::No)], 0),
        state(vec![listed(1, Press::No), app(2, Press::No)], 1),
        state(vec![app(1, Press::No), sent(10, Press::No)], 0),
        state(vec![app(1, Press::Taken), listed(1, Press::Taken)], 1),
        state(vec![listed(1, Press::Asked), app(2, Press::No)], 1),
        state(vec![listed(2, Press::No), listed(1, Press::Asked)], 2),
        state(vec![sent(10, Press::Taken)], 3),
        state((0..KEPT as u64).map(|id| app(id, Press::No)).collect(), 0),
    ],
});

every!(Command {
    Command::Changed { .. } => [Command::Changed { id: 5, of: ICON }],
    Command::Sent { .. } => [Command::Sent { by: 11, name: PORT }],
    Command::Press => [Command::Press],
    Command::Asked { .. } => [Command::Asked { seq: 1 }, Command::Asked { seq: 2 }],
});

every!(Observed {
    Observed::AppUndone { .. } => [Observed::AppUndone { id: 1 }, Observed::AppUndone { id: 2 }],
    Observed::Answered { .. } => [Observed::Answered { by: 10 }, Observed::Answered { by: 11 }],
    Observed::Undid { .. } => [Observed::Undid { asked: 1, took: 1 }, Observed::Undid { asked: 1, took: 3 }],
    Observed::Refused { .. } => [Observed::Refused { asked: 1 }],
    Observed::Listed(_) => [
        Observed::Listed(vec![]),
        Observed::Listed(vec![change(1, None)]),
        Observed::Listed(vec![change(1, None), change(2, Some(10))]),
        Observed::Listed(vec![change(3, Some(10))]),
        Observed::Listed(vec![change(1, None), change(2, None), change(3, None)]),
    ],
});

every!(Event {
    Event::Command(_) => Command::every().into_iter().map(Event::Command),
    Event::Observed(_) => Observed::every().into_iter().map(Event::Observed),
});

type R = DecideRow<Changes>;

/// `list` with `f` applied to each change it keeps, in order.
fn same_but(before: &State, after: &Result<State, Nothing>, f: impl Fn(&Change) -> Option<Change>) -> bool {
    after.as_ref().is_ok_and(|n| n.list == before.list.iter().filter_map(f).collect::<Vec<_>>())
}

fn pushed(s: &State, r: &Result<State, Nothing>, new: Change) -> bool {
    let mut list = s.list.clone();
    list.push(new);
    if list.len() > KEPT {
        list.remove(0);
    }
    r.as_ref().is_ok_and(|n| n.list == list && n.seen == s.seen)
}

fn rows() -> Vec<R> {
    use Event::{Command as C, Observed as O};
    vec![
        Row {
            says: "an app setting's change is the newest; past the last 20, the oldest goes",
            when: |_, e| matches!(e, C(Command::Changed { .. })),
            then: |s, e, r| {
                let C(Command::Changed { id, of }) = e else { unreachable!() };
                pushed(s, r, Change::App { id: *id, of: *of, press: Press::No })
            },
        },
        Row {
            says: "a service setting's change is the newest the moment it's sent",
            when: |_, e| matches!(e, C(Command::Sent { .. })),
            then: |s, e, r| {
                let C(Command::Sent { by, name }) = e else { unreachable!() };
                pushed(s, r, Change::Sent { by: *by, name: *name, press: Press::No })
            },
        },
        Row {
            says: "⌘Z takes the newest change not taken yet, and nothing else",
            when: |s, e| matches!(e, C(Command::Press)) && s.top().is_some(),
            then: |s, _, r| {
                let at = s.list.iter().rposition(|c| c.press() == Press::No).unwrap();
                let mut list = s.list.clone();
                list[at] = list[at].with(Press::Taken);
                r.as_ref().is_ok_and(|n| n.list == list)
            },
        },
        Row {
            says: "⌘Z with every change taken does nothing",
            when: |s, e| matches!(e, C(Command::Press)) && s.top().is_none(),
            then: |_, _, r| *r == Err(Nothing),
        },
        Row {
            says: "a listed change taken is asked of the service, once",
            when: |_, e| matches!(e, C(Command::Asked { .. })),
            then: |s, e, r| {
                let C(Command::Asked { seq }) = e else { unreachable!() };
                same_but(s, r, |c| Some(if c.seq() == Some(*seq) && c.press() == Press::Taken { c.with(Press::Asked) } else { c.clone() }))
            },
        },
        Row {
            says: "an app setting's change undone leaves",
            when: |_, e| matches!(e, O(Observed::AppUndone { .. })),
            then: |s, e, r| {
                let O(Observed::AppUndone { id }) = e else { unreachable!() };
                same_but(s, r, |c| (!matches!(c, Change::App { id: i, .. } if i == id)).then(|| c.clone()))
            },
        },
        Row {
            says: "a change sent and answered that the service doesn't list leaves: it changed nothing, or is gone",
            when: |_, e| matches!(e, O(Observed::Answered { .. })),
            then: |s, e, r| {
                let O(Observed::Answered { by }) = e else { unreachable!() };
                same_but(s, r, |c| (!matches!(c, Change::Sent { by: b, .. } if b == by)).then(|| c.clone()))
            },
        },
        Row {
            says: "what the service undid leaves; the change asked for, if another was undone, can be taken again",
            when: |_, e| matches!(e, O(Observed::Undid { .. })),
            then: |s, e, r| {
                let O(Observed::Undid { asked, took }) = e else { unreachable!() };
                same_but(s, r, |c| match c.seq() {
                    Some(seq) if seq == *took => None,
                    Some(seq) if seq == *asked && c.press() == Press::Asked => Some(c.with(Press::No)),
                    Some(_) | None => Some(c.clone()),
                })
            },
        },
        Row {
            says: "a refused undo is dropped",
            when: |_, e| matches!(e, O(Observed::Refused { .. })),
            then: |s, e, r| {
                let O(Observed::Refused { asked }) = e else { unreachable!() };
                same_but(s, r, |c| (c.seq() != Some(*asked)).then(|| c.clone()))
            },
        },
        Row {
            says: "the service's list keeps what it lists, in place; its own sent change is listed; another client's new ones are the newest",
            when: |_, e| matches!(e, O(Observed::Listed(_))),
            then: |s, e, r| {
                let O(Observed::Listed(changes)) = e else { unreachable!() };
                let Ok(n) = r else { return false };
                let kept = s.list.iter().filter_map(|c| match c {
                    Change::Listed { seq, .. } => changes.iter().any(|l| l.seq == *seq).then(|| c.clone()),
                    Change::Sent { by, name, press } => Some(match changes.iter().find(|l| l.by == Some(*by)) {
                        Some(l) => Change::Listed { seq: l.seq, name: *name, press: *press },
                        None => c.clone(),
                    }),
                    Change::App { .. } => Some(c.clone()),
                });
                let mut list: Vec<Change> = kept.collect();
                let new: Vec<Change> = changes
                    .iter()
                    .filter(|l| l.seq > s.seen && !list.iter().any(|c| c.seq() == Some(l.seq)))
                    .map(|l| Change::Listed { seq: l.seq, name: l.name, press: Press::No })
                    .collect();
                list.extend(new);
                let over = list.len().saturating_sub(KEPT);
                n.list == list[over..] && n.seen == changes.iter().map(|c| c.seq).fold(s.seen, u64::max)
            },
        },
    ]
}

fn always() -> Vec<R> {
    vec![Row::always("at most 20 changes, each service change once", |_, _, r| {
        r.as_ref().map_or(true, |n| {
            let seqs: Vec<u64> = n.list.iter().filter_map(Change::seq).collect();
            n.list.len() <= KEPT && seqs.iter().all(|s| seqs.iter().filter(|t| *t == s).count() == 1)
        })
    })]
}

#[test]
fn every_state_and_event_is_decided_as_its_row_says() {
    decide_table::<Changes>(&rows(), &always());
}

#[test]
fn every_state_reached_from_none_is_decided_as_its_row_says() {
    sequences::<Changes>(vec![State::default()], 24, &rows(), &always());
}

/// What `next` carries out: the newest change taken, an app's to undo, a listed one to ask for; a
/// change sent and not listed yet waits.
#[test]
fn next_carries_out_the_newest_change_taken() {
    for s in State::every() {
        let expected = s.list.iter().rev().find_map(|c| match (c, c.press()) {
            (Change::App { id, .. }, Press::Taken) => Some(Do::Undo { id: *id }),
            (Change::Listed { seq, .. }, Press::Taken) => Some(Do::Ask { seq: *seq }),
            _ => None,
        });
        assert_eq!(next(&s), expected, "{s:?}");
    }
}

// The whole of it, generated: the app, the service and another client (the CLI), with every message
// between the app and the service on its way for as long as the sequence says, and delivered in any
// order, as each command has its own connection; the app keeps the newest settings it's sent.

/// What happens next.
#[derive(Debug, Clone)]
enum Op {
    /// The app changes its setting `0..2` or the service's `0..2`.
    App(usize),
    Service(usize),
    /// The CLI changes the service's setting, or undoes its newest change.
    Cli(usize),
    CliUndo,
    /// ⌘Z.
    Press,
    /// The request on its way at this index reaches the service; a message to the app reaches it.
    ToService(usize),
    ToApp(usize),
    /// The service sends the app its settings, as a snapshot.
    Stream,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        3 => (0..2usize).prop_map(Op::App),
        3 => (0..2usize).prop_map(Op::Service),
        2 => (0..2usize).prop_map(Op::Cli),
        1 => Just(Op::CliUndo),
        3 => Just(Op::Press),
        3 => (0..4usize).prop_map(Op::ToService),
        3 => (0..4usize).prop_map(Op::ToApp),
        2 => Just(Op::Stream),
    ];
    // Fewer than 20 changes in all, so none is pushed past the last 20.
    proptest::collection::vec(op, 0..30)
        .prop_filter("under 20 changes", |ops| ops.iter().filter(|o| matches!(o, Op::App(_) | Op::Service(_) | Op::Cli(_))).count() < KEPT)
}

const SERVICE: [SettingName; 2] = [SettingName::ListenPort, SettingName::Upnp];

#[derive(Debug)]
enum Request {
    Set { name: SettingName, value: u64, by: u64 },
    Undo { asked: u64 },
}

#[derive(Debug)]
enum Message {
    Snapshot { at: u64, changes: Vec<SettingChange> },
    Answered { by: u64 },
    Undid { asked: u64, took: Option<u64> },
}

/// The service, as `lifecycle::settings` keeps its changes; a value per setting, each change a new
/// one.
struct Service {
    values: BTreeMap<SettingName, u64>,
    changes: ServiceChanges<u64>,
    at: u64,
}

impl Service {
    fn set(&mut self, name: SettingName, value: u64, by: Option<u64>) {
        let was = self.values.insert(name, value).unwrap();
        self.changes.push(name, by, was);
        self.at += 1;
    }

    fn undo(&mut self) -> Option<SettingChange> {
        let (change, was) = self.changes.take_newest()?;
        self.values.insert(change.name, was);
        self.at += 1;
        Some(change)
    }

    fn snapshot(&self) -> Message {
        Message::Snapshot { at: self.at, changes: self.changes.listed() }
    }
}

/// The app: the machine, its two settings' values and what each change of them was before, and the
/// settings' `at` it has.
struct App {
    state: State,
    values: [u64; 2],
    was: BTreeMap<u64, (usize, u64)>,
    at: u64,
    next: u64,
}

impl App {
    fn decide(&mut self, event: Event) -> Result<(), Nothing> {
        self.state = decide(&self.state, &event)?;
        Ok(())
    }

    /// Carries out what the machine says: an app change undone at once, a service one asked.
    fn run(&mut self, requests: &mut Vec<Request>, undone: &mut Vec<Undone>) {
        while let Some(todo) = next(&self.state) {
            match todo {
                Do::Undo { id } => {
                    let (i, was) = self.was.remove(&id).unwrap();
                    self.values[i] = was;
                    undone.push(Undone::App(id));
                    self.decide(Event::Observed(Observed::AppUndone { id })).unwrap();
                }
                Do::Ask { seq } => {
                    requests.push(Request::Undo { asked: seq });
                    self.decide(Event::Command(Command::Asked { seq })).unwrap();
                }
            }
        }
    }

    /// The machine's list, as which changes they are: (app id) or (service seq, or the id sent by).
    fn known(&self) -> Vec<Known> {
        self.state
            .list
            .iter()
            .map(|c| match c {
                Change::App { id, .. } => Known::App(*id),
                Change::Sent { by, .. } => Known::Sent(*by),
                Change::Listed { seq, .. } => Known::Seq(*seq),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Known {
    App(u64),
    Sent(u64),
    Seq(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Undone {
    App(u64),
    /// The service undid `seq` for the app's press.
    Service(u64),
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64), ..ProptestConfig::default() })]

    /// Any interleaving of the app's changes, the service's (made in the app, or by the CLI) and the
    /// CLI's undos, with every message late and out of order: ⌘Z always takes the newest change the
    /// app knows, in the order it learned of them (a service one, the service's newest, which is
    /// newer still when another client's landed between the press and the answer), and pressing it
    /// until nothing's left puts every setting back as it was, newest first, with nothing left on
    /// either side.
    #[test]
    fn every_change_is_undone_newest_first(ops in ops()) {
        let mut service = Service { values: SERVICE.iter().map(|n| (*n, 0)).collect(), changes: ServiceChanges::new(100), at: 1 };
        let mut app = App { state: State::default(), values: [0, 0], was: BTreeMap::new(), at: 0, next: 1 };
        let (mut requests, mut messages) = (Vec::<Request>::new(), Vec::<Message>::new());
        let mut undone = Vec::new();
        let mut value = 1;
        // The order the app learned of each change in, as the test sees it: what the list must be.
        let mut learned: Vec<Known> = Vec::new();
        let mut deliver = |op: &Op, service: &mut Service, app: &mut App, requests: &mut Vec<Request>, messages: &mut Vec<Message>, learned: &mut Vec<Known>, undone: &mut Vec<Undone>| {
            value += 1;
            match op {
                Op::App(i) => {
                    let id = app.next;
                    app.next += 1;
                    app.was.insert(id, (*i, app.values[*i]));
                    app.values[*i] = value;
                    app.decide(Event::Command(Command::Changed { id, of: ICON })).unwrap();
                    learned.push(Known::App(id));
                }
                Op::Service(i) => {
                    let by = app.next;
                    app.next += 1;
                    requests.push(Request::Set { name: SERVICE[*i], value, by });
                    app.decide(Event::Command(Command::Sent { by, name: SERVICE[*i] })).unwrap();
                    learned.push(Known::Sent(by));
                }
                Op::Cli(i) => service.set(SERVICE[*i], value, None),
                Op::CliUndo => { service.undo(); }
                Op::Press => {
                    let top = app.state.top().cloned();
                    match app.decide(Event::Command(Command::Press)) {
                        Ok(()) => assert!(top.is_some()),
                        Err(Nothing) => assert!(top.is_none(), "{top:?} was there to take"),
                    }
                    app.run(requests, undone);
                }
                Op::ToService(i) if *i < requests.len() => {
                    match requests.remove(*i) {
                        Request::Set { name, value, by } => {
                            service.set(name, value, Some(by));
                            messages.push(service.snapshot());
                            messages.push(Message::Answered { by });
                        }
                        Request::Undo { asked } => {
                            let took = service.undo();
                            messages.push(service.snapshot());
                            messages.push(Message::Undid { asked, took: took.map(|c| c.seq) });
                        }
                    }
                }
                Op::ToApp(i) if *i < messages.len() => {
                    match messages.remove(*i) {
                        // The newest settings only, as the app keeps them.
                        Message::Snapshot { at, changes } if at > app.at => {
                            app.at = at;
                            for c in &changes {
                                match c.by {
                                    Some(by) => if let Some(k) = learned.iter_mut().find(|k| **k == Known::Sent(by)) { *k = Known::Seq(c.seq) },
                                    None => {}
                                }
                            }
                            learned.retain(|k| match k { Known::Seq(seq) => changes.iter().any(|c| c.seq == *seq), Known::App(_) | Known::Sent(_) => true });
                            let seen = app.state.seen;
                            let new: Vec<Known> = changes.iter().filter(|c| c.seq > seen && !learned.contains(&Known::Seq(c.seq))).map(|c| Known::Seq(c.seq)).collect();
                            learned.extend(new);
                            app.decide(Event::Observed(Observed::Listed(changes))).unwrap();
                        }
                        Message::Snapshot { .. } => {}
                        Message::Answered { by } => {
                            learned.retain(|k| *k != Known::Sent(by));
                            app.decide(Event::Observed(Observed::Answered { by })).unwrap();
                        }
                        Message::Undid { asked, took: Some(took) } => {
                            learned.retain(|k| *k != Known::Seq(took));
                            undone.push(Undone::Service(took));
                            app.decide(Event::Observed(Observed::Undid { asked, took })).unwrap();
                        }
                        Message::Undid { asked, took: None } => {
                            learned.retain(|k| *k != Known::Seq(asked));
                            app.decide(Event::Observed(Observed::Refused { asked })).unwrap();
                        }
                    }
                    app.run(requests, undone);
                }
                Op::ToService(_) | Op::ToApp(_) => {}
                Op::Stream => messages.push(service.snapshot()),
            }
            // App changes the app undid leave its list too.
            learned.retain(|k| match k { Known::App(id) => app.was.contains_key(id), Known::Sent(_) | Known::Seq(_) => true });
        };
        for op in &ops {
            deliver(op, &mut service, &mut app, &mut requests, &mut messages, &mut learned, &mut undone);
            prop_assert_eq!(app.known(), learned.clone(), "the list is what the app learned, in that order");
        }
        // Everything arrives; then ⌘Z until nothing's left, each press's messages delivered.
        let drain = |service: &mut Service, app: &mut App, requests: &mut Vec<Request>, messages: &mut Vec<Message>, learned: &mut Vec<Known>, undone: &mut Vec<Undone>, deliver: &mut dyn FnMut(&Op, &mut Service, &mut App, &mut Vec<Request>, &mut Vec<Message>, &mut Vec<Known>, &mut Vec<Undone>)| {
            while !requests.is_empty() || !messages.is_empty() {
                if !requests.is_empty() {
                    deliver(&Op::ToService(0), service, app, requests, messages, learned, undone);
                } else {
                    deliver(&Op::ToApp(0), service, app, requests, messages, learned, undone);
                }
            }
            deliver(&Op::Stream, service, app, requests, messages, learned, undone);
            deliver(&Op::ToApp(0), service, app, requests, messages, learned, undone);
        };
        drain(&mut service, &mut app, &mut requests, &mut messages, &mut learned, &mut undone, &mut deliver);
        let mut presses = 0;
        while app.state.top().is_some() {
            let before = app.known();
            let newest = *before.iter().rev().zip(app.state.list.iter().rev()).find(|(_, c)| c.press() == Press::No).unwrap().0;
            let count = undone.len();
            let service_newest = service.changes.listed().last().map(|c| c.seq);
            deliver(&Op::Press, &mut service, &mut app, &mut requests, &mut messages, &mut learned, &mut undone);
            drain(&mut service, &mut app, &mut requests, &mut messages, &mut learned, &mut undone, &mut deliver);
            let took = undone[count..].to_vec();
            // A service setting's, the service's newest: the app's newest too, unless two it sent
            // reached the service the other way round (each on its own connection).
            let expected = match newest {
                Known::App(id) => Undone::App(id),
                Known::Seq(_) => Undone::Service(service_newest.expect("listed, so there")),
                Known::Sent(_) => unreachable!("everything arrived"),
            };
            prop_assert_eq!(took, vec![expected], "a press takes the newest");
            presses += 1;
            prop_assert!(presses <= KEPT, "it ends");
        }
        prop_assert_eq!(app.values, [0, 0], "the app's settings are as they were");
        prop_assert!(service.values.values().all(|v| *v == 0), "and the service's: {:?}", service.values);
        prop_assert!(service.changes.listed().is_empty() && app.state.list.is_empty(), "nothing left on either side");
    }
}
