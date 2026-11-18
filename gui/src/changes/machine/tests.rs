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
    State { list, seen, at: 5, listing: 5, left: Vec::new() }
}

// Changes 1 and 2 (app ids or service seqs), sent by 10; the service lists seqs 1 to 3 (and made 4,
// by 10); the settings taken are at 5, those at 3 are older, those at 7 newer.
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
        State { at: 0, listing: 0, ..state(vec![sent(10, Press::No), app(1, Press::No)], 0) },
        State { at: 6, listing: 2, ..state(vec![sent(10, Press::No), sent(11, Press::Taken)], 0) },
        State { left: vec![3], ..state(vec![listed(1, Press::Asked)], 0) },
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
    Observed::Answered { .. } => [
        Observed::Answered { by: 10, seq: Some(4), at: 7 },
        Observed::Answered { by: 10, seq: Some(4), at: 3 },
        Observed::Answered { by: 10, seq: None, at: 7 },
        Observed::Answered { by: 11, seq: Some(4), at: 7 },
    ],
    Observed::Undid { .. } => [Observed::Undid { asked: 1, took: 1 }, Observed::Undid { asked: 1, took: 3 }],
    Observed::Refused { .. } => [Observed::Refused { asked: 1 }],
    Observed::Listed { .. } => [
        Observed::Listed { at: 7, changes: vec![] },
        Observed::Listed { at: 7, changes: vec![change(1, None)] },
        Observed::Listed { at: 7, changes: vec![change(1, None), change(2, Some(10))] },
        Observed::Listed { at: 7, changes: vec![change(3, Some(10))] },
        Observed::Listed { at: 7, changes: vec![change(1, None), change(2, None), change(3, None)] },
        Observed::Listed { at: 3, changes: vec![] },
        Observed::Listed { at: 5, changes: vec![change(3, None)] },
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
    r.as_ref().is_ok_and(|n| n.list == list && n.seen == s.seen && n.at == s.at && n.listing == s.listing)
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
            says: "a change sent and answered is listed in place, its press kept, unless it made none, was undone, or settings taken since don't list it: then its press, if taken, goes to the newest not taken; older settings are older",
            when: |_, e| matches!(e, O(Observed::Answered { .. })),
            then: |s, e, r| {
                let O(Observed::Answered { by, seq, at }) = e else { unreachable!() };
                let new = seq.filter(|seq| *at > s.listing && !s.list.iter().any(|c| c.seq() == Some(*seq)) && !s.left.contains(seq));
                let first = s.list.iter().position(|c| matches!(c, Change::Sent { by: b, .. } if b == by));
                let expected: Vec<Change> = s
                    .list
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match c {
                        Change::Sent { by: b, name, press } if b == by => {
                            new.filter(|_| Some(i) == first).map(|seq| Change::Listed { seq, name: *name, press: *press })
                        }
                        _ => Some(c.clone()),
                    })
                    .collect();
                let mut expected = expected;
                let taken_gone = s.list.iter().enumerate().any(|(i, c)| {
                    matches!(c, Change::Sent { by: b, press, .. } if b == by && *press != Press::No && !(new.is_some() && Some(i) == first))
                });
                if let Some(i) = expected.iter().rposition(|c| c.press() == Press::No).filter(|_| taken_gone) {
                    expected[i] = expected[i].with(Press::Taken);
                }
                let placed = r.as_ref().is_ok_and(|n| n.list == expected);
                placed && r.as_ref().is_ok_and(|n| n.at == s.at.max(at - 1) && n.seen == s.seen && n.listing == s.listing)
            },
        },
        Row {
            says: "what the service undid leaves; the change asked for, if another was undone, can be taken again",
            when: |_, e| matches!(e, O(Observed::Undid { .. })),
            then: |s, e, r| {
                let O(Observed::Undid { asked, took }) = e else { unreachable!() };
                let kept = same_but(s, r, |c| match c.seq() {
                    Some(seq) if seq == *took => None,
                    Some(seq) if seq == *asked && c.press() == Press::Asked => Some(c.with(Press::No)),
                    Some(_) | None => Some(c.clone()),
                });
                kept && r.as_ref().is_ok_and(|n| n.left.contains(took))
            },
        },
        Row {
            says: "a refused undo is dropped",
            when: |_, e| matches!(e, O(Observed::Refused { .. })),
            then: |s, e, r| {
                let O(Observed::Refused { asked }) = e else { unreachable!() };
                same_but(s, r, |c| (c.seq() != Some(*asked)).then(|| c.clone())) && r.as_ref().is_ok_and(|n| n.left.contains(asked))
            },
        },
        Row {
            says: "settings no newer than those taken change nothing",
            when: |s, e| matches!(e, O(Observed::Listed { at, .. }) if *at <= s.at),
            then: |s, _, r| r.as_ref() == Ok(s),
        },
        Row {
            says: "newer settings: the service's list keeps what it lists, in place; its own sent change is listed; another client's new ones are the newest, but those an answer took off",
            when: |s, e| matches!(e, O(Observed::Listed { at, .. }) if *at > s.at),
            then: |s, e, r| {
                let O(Observed::Listed { at, changes }) = e else { unreachable!() };
                if r.as_ref().is_ok_and(|n| (n.at, n.listing) != (*at, *at)) {
                    return false;
                }
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
                    .filter(|l| l.seq > s.seen && !list.iter().any(|c| c.seq() == Some(l.seq)) && !s.left.contains(&l.seq))
                    .map(|l| Change::Listed { seq: l.seq, name: l.name, press: Press::No })
                    .collect();
                list.extend(new);
                let over = list.len().saturating_sub(KEPT);
                let left: Vec<u64> = s.left.iter().copied().filter(|seq| changes.iter().any(|c| c.seq == *seq)).collect();
                n.list == list[over..] && n.seen == changes.iter().map(|c| c.seq).fold(s.seen, u64::max) && n.left == left
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

/// The review's first case (#241): a port sent, then an App icon picked, the port's answer before
/// any snapshot lists it. The icon is still the newest, as it was picked after.
#[test]
fn a_sent_change_answered_before_any_listing_keeps_its_place() {
    let icon = Event::Command(Command::Changed { id: 2, of: ICON });
    let events = [
        Event::Command(Command::Sent { by: 10, name: PORT }),
        icon,
        Event::Observed(Observed::Answered { by: 10, seq: Some(4), at: 7 }),
        Event::Observed(Observed::Listed { at: 8, changes: vec![change(4, Some(10))] }),
    ];
    let state = events.iter().fold(State::default(), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, [listed(4, Press::No), app(2, Press::No)], "the icon is still the newest");
    assert_eq!(state.top(), Some(&app(2, Press::No)));
}

/// The review's second case (#241): ⌘Z on a change still waiting for its answer, which comes
/// before any snapshot lists it. The press isn't lost: the change is asked for once it's known.
#[test]
fn a_press_on_a_change_waiting_for_its_answer_is_kept() {
    let events = [
        Event::Command(Command::Sent { by: 10, name: PORT }),
        Event::Command(Command::Press),
        Event::Observed(Observed::Answered { by: 10, seq: Some(4), at: 7 }),
        Event::Observed(Observed::Listed { at: 8, changes: vec![change(4, Some(10))] }),
    ];
    let state = events.iter().fold(State::default(), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, [listed(4, Press::Taken)], "taken, and listed once");
    assert_eq!(next(&state), Some(Do::Ask { seq: 4 }));
}

/// An undo refused as the service stopped takes its change off; a listing from before that still
/// has it doesn't add it back, as the newest.
#[test]
fn a_change_an_answer_took_off_isnt_listed_again() {
    let events =
        [Event::Observed(Observed::Refused { asked: 1 }), Event::Observed(Observed::Listed { at: 9, changes: vec![change(1, None)] })];
    let state = events.iter().fold(state(vec![listed(1, Press::Asked)], 0), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, []);
}

/// Two presses, the second on a change still waiting for its answer, which the service undid for
/// the first (its newest): when the answer says so, the second press takes the next change, the
/// first's, so two presses are two undos.
#[test]
fn a_press_whose_change_another_press_undid_takes_the_next() {
    let events = [
        Event::Command(Command::Sent { by: 10, name: PORT }),
        Event::Observed(Observed::Answered { by: 10, seq: Some(4), at: 7 }),
        Event::Command(Command::Press),
        Event::Command(Command::Asked { seq: 4 }),
        Event::Command(Command::Sent { by: 11, name: PORT }),
        Event::Command(Command::Press),
        Event::Observed(Observed::Undid { asked: 4, took: 5 }),
        Event::Observed(Observed::Answered { by: 11, seq: Some(5), at: 9 }),
    ];
    let state = events.iter().fold(State::default(), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, [listed(4, Press::Taken)]);
}

// The whole of it, generated: the app, the service and another client (the CLI), with every message
// between the app and the service on its way for as long as the sequence says, delivered in any
// order (each command has its own connection) and older settings among them; the service may
// restart. The truth the app's list is checked against is when each change was made: an app
// change and a service change the app sends, when the person makes it; another client's, when the
// app first gets settings newer than any it had that list it.

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
    /// The service stops and another starts: its changes gone, the values kept, every request on
    /// its way answered that it's stopping.
    Restart,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        3 => (0..2usize).prop_map(Op::App),
        3 => (0..2usize).prop_map(Op::Service),
        2 => (0..2usize).prop_map(Op::Cli),
        1 => Just(Op::CliUndo),
        3 => Just(Op::Press),
        3 => (0..4usize).prop_map(Op::ToService),
        4 => (0..6usize).prop_map(Op::ToApp),
        2 => Just(Op::Stream),
        1 => Just(Op::Restart),
    ];
    // Fewer than 20 changes in all, so none is pushed past the last 20.
    proptest::collection::vec(op, 0..36)
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
    Answered { by: u64, seq: Option<u64>, at: u64 },
    Undid { asked: u64, took: Option<u64> },
}

/// The service, as `lifecycle::settings` keeps its changes; a value per setting, each change a new
/// one.
struct Service {
    values: BTreeMap<SettingName, u64>,
    changes: ServiceChanges<u64>,
    at: u64,
    /// The values when it started: what undoing all it has goes back to.
    start: BTreeMap<SettingName, u64>,
}

impl Service {
    fn set(&mut self, name: SettingName, value: u64, by: Option<u64>) -> SettingChange {
        let was = self.values.insert(name, value).unwrap();
        self.at += 1;
        self.changes.push(name, by, was)
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

/// A change, as the test knows it: an app setting's by id, a service setting's the app sent by the
/// id it was sent by, another client's by its `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Made {
    App(u64),
    Sent(u64),
    Other(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Undone {
    App(u64),
    /// The service undid `seq` for the app's press.
    Service(u64),
}

struct World {
    service: Service,
    app: State,
    /// The app's two settings, and what each of its changes was before.
    values: [u64; 2],
    was: BTreeMap<u64, (usize, u64)>,
    next: u64,
    requests: Vec<Request>,
    messages: Vec<Message>,
    undone: Vec<Undone>,
    /// Every change, in the order it was made.
    made: Vec<Made>,
    /// The `seq` the service gave each change the app sent, by what it was sent by.
    seqs: BTreeMap<u64, u64>,
    /// The newest settings the app has had, by their `at`: another client's change is known
    /// from the first newer that lists it.
    newest: u64,
    /// The changes ⌘Z took, until undone or gone.
    taken: Vec<Made>,
    value: u64,
}

impl World {
    fn decide(&mut self, event: Event) -> Result<(), Nothing> {
        self.app = decide(&self.app, &event)?;
        Ok(())
    }

    /// The change an entry of the app's list is.
    fn which(&self, change: &Change) -> Made {
        match change {
            Change::App { id, .. } => Made::App(*id),
            Change::Sent { by, .. } => Made::Sent(*by),
            Change::Listed { seq, .. } => self.seqs.iter().find(|(_, s)| *s == seq).map_or(Made::Other(*seq), |(by, _)| Made::Sent(*by)),
        }
    }

    /// Carries out what the machine says: an app change undone at once, a service one asked.
    fn run(&mut self) {
        while let Some(todo) = next(&self.app) {
            match todo {
                Do::Undo { id } => {
                    let (i, was) = self.was.remove(&id).unwrap();
                    self.values[i] = was;
                    self.undone.push(Undone::App(id));
                    self.taken.retain(|t| *t != Made::App(id));
                    self.decide(Event::Observed(Observed::AppUndone { id })).unwrap();
                }
                Do::Ask { seq } => {
                    self.requests.push(Request::Undo { asked: seq });
                    self.decide(Event::Command(Command::Asked { seq })).unwrap();
                }
            }
        }
    }

    fn step(&mut self, op: &Op) {
        self.value += 1;
        let value = self.value;
        match op {
            Op::App(i) => {
                let id = self.next;
                self.next += 1;
                self.was.insert(id, (*i, self.values[*i]));
                self.values[*i] = value;
                self.made.push(Made::App(id));
                self.decide(Event::Command(Command::Changed { id, of: ICON })).unwrap();
            }
            Op::Service(i) => {
                let by = self.next;
                self.next += 1;
                self.requests.push(Request::Set { name: SERVICE[*i], value, by });
                self.made.push(Made::Sent(by));
                self.decide(Event::Command(Command::Sent { by, name: SERVICE[*i] })).unwrap();
            }
            Op::Cli(i) => {
                self.service.set(SERVICE[*i], value, None);
            }
            Op::CliUndo => {
                self.service.undo();
            }
            Op::Press => {
                let top = self.app.top().cloned();
                match self.decide(Event::Command(Command::Press)) {
                    Ok(()) => self.taken.push(self.which(&top.expect("there was one to take"))),
                    Err(Nothing) => assert!(top.is_none(), "{top:?} was there to take"),
                }
                self.run();
            }
            Op::ToService(i) if *i < self.requests.len() => match self.requests.remove(*i) {
                Request::Set { name, value, by } => {
                    let change = self.service.set(name, value, Some(by));
                    self.seqs.insert(by, change.seq);
                    self.messages.push(Message::Answered { by, seq: Some(change.seq), at: self.service.at });
                }
                Request::Undo { asked } => {
                    let took = self.service.undo();
                    self.messages.push(Message::Undid { asked, took: took.map(|c| c.seq) });
                }
            },
            Op::ToApp(i) if *i < self.messages.len() => {
                match self.messages.remove(*i) {
                    Message::Snapshot { at, changes } => {
                        if at > self.newest {
                            self.newest = at;
                            let new: Vec<Made> = changes
                                .iter()
                                .filter(|c| c.by.is_none())
                                .map(|c| Made::Other(c.seq))
                                .filter(|m| !self.made.contains(m))
                                .collect();
                            self.made.extend(new);
                        }
                        self.decide(Event::Observed(Observed::Listed { at, changes })).unwrap();
                    }
                    Message::Answered { by, seq, at } => {
                        // Settings before it are older; those right after it aren't.
                        self.newest = self.newest.max(at.saturating_sub(1));
                        let waiting =
                            self.app.list.iter().any(|c| matches!(c, Change::Sent { by: b, press, .. } if *b == by && *press != Press::No));
                        self.decide(Event::Observed(Observed::Answered { by, seq, at })).unwrap();
                        // Taken while it waited for this, and gone: two presses are still two
                        // undos, so its press goes to the newest change not taken, in the order
                        // they were made.
                        let listed = self.app.list.iter().any(|c| self.which(c) == Made::Sent(by));
                        if waiting && !listed {
                            self.taken.retain(|t| *t != Made::Sent(by));
                            let untaken = self.app.list.iter().map(|c| self.which(c)).filter(|m| !self.taken.contains(m));
                            if let Some(next) = untaken.max_by_key(|m| self.made.iter().position(|x| x == m)) {
                                self.taken.push(next);
                            }
                        }
                    }
                    Message::Undid { asked, took: Some(took) } => {
                        self.undone.push(Undone::Service(took));
                        // The press on `asked` is spent; `took`'s, if the app knows it as listed, goes with it.
                        let known = self.app.list.iter().any(|c| c.seq() == Some(took));
                        let gone = [Some(self.which(&listed(asked, Press::No))), known.then(|| self.which(&listed(took, Press::No)))];
                        self.taken.retain(|t| !gone.contains(&Some(*t)));
                        self.decide(Event::Observed(Observed::Undid { asked, took })).unwrap();
                    }
                    Message::Undid { asked, took: None } => {
                        let made = self.which(&listed(asked, Press::No));
                        self.taken.retain(|t| *t != made);
                        self.decide(Event::Observed(Observed::Refused { asked })).unwrap();
                    }
                }
                self.run();
            }
            Op::ToService(_) | Op::ToApp(_) => {}
            Op::Stream => self.messages.push(self.service.snapshot()),
            Op::Restart => {
                let base = self.service.at + 1_000;
                self.service = Service {
                    values: self.service.values.clone(),
                    changes: ServiceChanges::new(base),
                    at: base,
                    start: self.service.values.clone(),
                };
                for request in std::mem::take(&mut self.requests) {
                    self.messages.push(match request {
                        Request::Set { by, .. } => Message::Answered { by, seq: None, at: 0 },
                        Request::Undo { asked } => Message::Undid { asked, took: None },
                    });
                }
            }
        }
    }

    /// Whether `made` is still to be undone.
    fn live(&self, made: Made) -> bool {
        let on_service = |seq: u64| self.service.changes.listed().iter().any(|c| c.seq == seq);
        match made {
            Made::App(id) => self.was.contains_key(&id),
            Made::Sent(by) => self.seqs.get(&by).is_some_and(|seq| on_service(*seq)),
            Made::Other(seq) => on_service(seq),
        }
    }

    /// The app's list is in the order the changes were made, and holds a press exactly on what ⌘Z
    /// took.
    fn check(&self) -> Result<(), TestCaseError> {
        let at: Vec<usize> = self
            .app
            .list
            .iter()
            .map(|c| self.made.iter().position(|m| *m == self.which(c)).unwrap_or_else(|| panic!("{c:?} was never made")))
            .collect();
        prop_assert!(at.windows(2).all(|w| w[0] < w[1]), "in the order made: {:?} at {:?} of {:?}", self.app.list, at, self.made);
        for c in &self.app.list {
            prop_assert_eq!(c.press() != Press::No, self.taken.contains(&self.which(c)), "taken exactly when ⌘Z took it: {:?}", c);
        }
        Ok(())
    }

    /// Every request reaches the service, every message the app, and then the newest settings.
    fn drain(&mut self) {
        while !self.requests.is_empty() || !self.messages.is_empty() {
            let op = if self.requests.is_empty() { Op::ToApp(0) } else { Op::ToService(0) };
            self.step(&op);
        }
        self.step(&Op::Stream);
        self.step(&Op::ToApp(0));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64), ..ProptestConfig::default() })]

    /// Any interleaving of the app's changes, the service's (made in the app, or by the CLI) and the
    /// CLI's undos, with every message late and out of order and the service restarting: the app's
    /// list is always in the order the changes were made, each press kept on what it took; once
    /// everything has arrived it holds exactly the changes still to undo; ⌘Z then takes the newest
    /// each time (a service one: the service's newest), and pressing it until nothing's left puts
    /// every setting back as it was, nothing left on either side.
    #[test]
    fn every_change_is_undone_newest_first(ops in ops()) {
        let values: BTreeMap<SettingName, u64> = SERVICE.iter().map(|n| (*n, 0)).collect();
        let service = Service { values: values.clone(), changes: ServiceChanges::new(100), at: 1, start: values };
        let mut w = World {
            service,
            app: State::default(),
            values: [0, 0],
            was: BTreeMap::new(),
            next: 1,
            requests: Vec::new(),
            messages: Vec::new(),
            undone: Vec::new(),
            made: Vec::new(),
            seqs: BTreeMap::new(),
            newest: 0,
            taken: Vec::new(),
            value: 1,
        };
        for op in &ops {
            w.step(op);
            w.check()?;
        }
        w.drain();
        w.check()?;
        let live: Vec<Made> = w.made.iter().copied().filter(|m| w.live(*m)).collect();
        let listed: Vec<Made> = w.app.list.iter().map(|c| w.which(c)).collect();
        prop_assert_eq!(listed, live, "everything arrived: the list is the changes still to undo");
        let mut presses = 0;
        while let Some(top) = w.app.top().cloned() {
            let newest = w.which(&top);
            let count = w.undone.len();
            let service_newest = w.service.changes.listed().last().map(|c| c.seq);
            w.step(&Op::Press);
            w.drain();
            w.check()?;
            // A service setting's, the service's newest: the app's newest too, unless two it sent
            // reached the service the other way round (each on its own connection).
            let expected = match newest {
                Made::App(id) => Undone::App(id),
                Made::Sent(_) | Made::Other(_) => Undone::Service(service_newest.expect("listed, so there")),
            };
            prop_assert_eq!(w.undone[count..].to_vec(), vec![expected], "a press takes the newest");
            presses += 1;
            prop_assert!(presses <= KEPT, "it ends");
        }
        prop_assert_eq!(w.values, [0, 0], "the app's settings are as they were");
        prop_assert_eq!(&w.service.values, &w.service.start, "and the service's, as it started");
        prop_assert!(w.service.changes.listed().is_empty() && w.app.list.is_empty(), "nothing left on either side");
    }
}
