use std::collections::BTreeMap;

use lifecycle::settings::Changes as ServiceChanges;
use machine::every;
use machine::{DecideRow, Every, Row, decide_table, sequences};
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

fn action(seq: u64, press: Press) -> Change {
    Change::Action { seq, press }
}

/// The app's torrent action 20 (on torrent 7, sent when the newest entry it knew was 1).
fn pending(press: Press) -> Change {
    Change::Pending { id: 20, after: 1, torrents: vec![7], press }
}

fn change(seq: u64, by: Option<u64>) -> SettingChange {
    SettingChange { seq, name: PORT, by }
}

/// Entry `seq` of the stack: 2 is of torrent 7 (the app's action 20's), the others of their own.
fn entry(seq: u64) -> Entry {
    Entry { seq, torrents: vec![if seq == 2 { 7 } else { seq }] }
}

fn entries(seqs: &[u64]) -> Vec<Entry> {
    seqs.iter().map(|seq| entry(*seq)).collect()
}

/// The stack as the snapshot at `(1, 5)` listed it.
fn state(list: Vec<Change>, seen: u64) -> State {
    State { list, seen, at: 5, listing: 5, left: Vec::new(), stack: Stack { at: (1, 5), left: Vec::new() } }
}

// Changes 1 and 2 (app ids or service seqs), sent by 10; the service lists seqs 1 to 3 (and made 4,
// by 10); the settings taken are at 5, those at 3 are older, those at 7 newer. Torrent actions 1 and
// 2, and 3 that the app hasn't learned of; the app's own action 20, not listed yet, made entry 2;
// the stack taken is the snapshot (1, 5)'s, those from (2, …) a new service's.
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
        state(vec![action(1, Press::No)], 0),
        state(vec![action(1, Press::No), app(2, Press::No), action(2, Press::No)], 0),
        state(vec![listed(1, Press::No), action(1, Press::Taken)], 1),
        state(vec![action(1, Press::Asked), action(2, Press::No)], 0),
        state(vec![action(1, Press::Asked), sent(10, Press::No), action(2, Press::Asked)], 0),
        State { stack: Stack { at: (1, 5), left: vec![3] }, ..state(vec![action(1, Press::Asked)], 0) },
        state((0..KEPT as u64).map(|id| app(id, Press::No)).chain([action(1, Press::No)]).collect(), 0),
        state(vec![action(1, Press::No), pending(Press::No)], 0),
        state(vec![pending(Press::Taken), app(2, Press::No)], 0),
        state(vec![pending(Press::No), action(2, Press::No)], 0),
        State { stack: Stack { at: (1, 5), left: vec![2] }, ..state(vec![action(1, Press::No), pending(Press::Taken)], 0) },
        state(vec![listed(1, Press::No), sent(11, Press::Taken), listed(3, Press::Asked)], 3),
        state(vec![app(1, Press::No), listed(1, Press::Asked), sent(10, Press::No)], 1),
    ],
});

every!(Command {
    Command::Changed { .. } => [Command::Changed { id: 5, of: ICON }],
    Command::Sent { .. } => [Command::Sent { by: 11, name: PORT }],
    Command::Press => [Command::Press],
    Command::Asked { .. } => [Command::Asked { seq: 1 }, Command::Asked { seq: 2 }],
    Command::Acted { .. } => [Command::Acted { id: 21, after: 2, torrents: vec![9] }],
    Command::PressAction { .. } => [Command::PressAction { seq: 1 }, Command::PressAction { seq: 2 }],
    Command::AskedAction { .. } => [Command::AskedAction { seq: 1 }, Command::AskedAction { seq: 2 }],
    Command::PressApp { .. } => [Command::PressApp { id: 1 }, Command::PressApp { id: 2 }],
    Command::Fold { .. } => [
        Command::Fold { ids: vec![1, 2], into: Some((6, AppSetting::Edit)) },
        Command::Fold { ids: vec![2], into: None },
        Command::Fold { ids: vec![], into: Some((6, AppSetting::Edit)) },
    ],
});

every!(Observed {
    Observed::AppUndone { .. } => [Observed::AppUndone { id: 1 }, Observed::AppUndone { id: 2 }],
    Observed::Answered { .. } => [
        Observed::Answered { by: 10, seq: Some(4), at: 7 },
        Observed::Answered { by: 10, seq: Some(4), at: 3 },
        Observed::Answered { by: 10, seq: None, at: 7 },
        Observed::Answered { by: 11, seq: Some(4), at: 7 },
    ],
    Observed::Undid { .. } => [
        Observed::Undid { asked: 1, took: 1, by: None },
        Observed::Undid { asked: 1, took: 3, by: None },
        Observed::Undid { asked: 3, took: 4, by: Some(10) },
        Observed::Undid { asked: 1, took: 1, by: Some(10) },
        Observed::Undid { asked: 3, took: 4, by: Some(11) },
    ],
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
    Observed::Stack { .. } => [
        Observed::Stack { at: (1, 7), entries: entries(&[]) },
        Observed::Stack { at: (1, 7), entries: entries(&[1]) },
        Observed::Stack { at: (1, 7), entries: entries(&[1, 2]) },
        Observed::Stack { at: (1, 7), entries: entries(&[1, 2, 3]) },
        Observed::Stack { at: (1, 7), entries: entries(&[3]) },
        Observed::Stack { at: (1, 7), entries: vec![entry(1), Entry { seq: 3, torrents: vec![7] }, entry(2)] },
        Observed::Stack { at: (1, 5), entries: entries(&[1, 3]) },
        Observed::Stack { at: (1, 3), entries: entries(&[1, 2, 3]) },
        Observed::Stack { at: (2, 0), entries: entries(&[2]) },
        Observed::Stack { at: (2, 0), entries: entries(&[1, 3]) },
    ],
    Observed::Made { .. } => [
        Observed::Made { id: 20, seq: Some(2) },
        Observed::Made { id: 20, seq: Some(1) },
        Observed::Made { id: 20, seq: None },
        Observed::Made { id: 21, seq: Some(2) },
    ],
    Observed::ActionAnswered { .. } => [
        Observed::ActionAnswered { asked: 1, took: Some(entry(1)) },
        Observed::ActionAnswered { asked: 1, took: Some(entry(3)) },
        Observed::ActionAnswered { asked: 2, took: Some(entry(1)) },
        Observed::ActionAnswered { asked: 1, took: Some(entry(2)) },
        Observed::ActionAnswered { asked: 1, took: None },
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

/// `list` with no more than the last [`KEPT`] settings changes, the torrent actions all kept.
fn capped(mut list: Vec<Change>) -> Vec<Change> {
    while list.iter().filter(|c| c.is_setting()).count() > KEPT {
        let oldest = list.iter().position(Change::is_setting).unwrap();
        list.remove(oldest);
    }
    list
}

/// `list`, with a press that went with its change, if one did, on the newest change not taken.
fn passing(mut list: Vec<Change>, passed: bool) -> Vec<Change> {
    if let Some(i) = list.iter().rposition(|c| c.press() == Press::No).filter(|_| passed) {
        list[i] = list[i].with(Press::Taken);
    }
    list
}

fn pushed(s: &State, r: &Result<State, Nothing>, new: Change) -> bool {
    let mut list = s.list.clone();
    list.push(new);
    let list = capped(list);
    r.as_ref().is_ok_and(|n| n.list == list && n.seen == s.seen && n.at == s.at && n.listing == s.listing && n.stack == s.stack)
}

fn rows() -> Vec<R> {
    use Event::{Command as C, Observed as O};
    vec![
        Row {
            says: "an app setting's change is the newest; past the last 20 settings changes, the oldest of them goes",
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
            says: "what the service undid leaves, listed yet or still sent (its press, if taken, on the newest not taken); the change asked for, if another was undone, can be taken again",
            when: |_, e| matches!(e, O(Observed::Undid { .. })),
            then: |s, e, r| {
                let O(Observed::Undid { asked, took, by }) = e else { unreachable!() };
                let sent = |c: &Change| matches!(c, Change::Sent { by: b, .. } if Some(*b) == *by);
                let passed = s.list.iter().any(|c| sent(c) && c.press() != Press::No);
                let list: Vec<Change> = s
                    .list
                    .iter()
                    .filter(|c| c.seq() != Some(*took) && !sent(c))
                    .map(|c| if c.seq() == Some(*asked) && c.press() == Press::Asked { c.with(Press::No) } else { c.clone() })
                    .collect();
                r.as_ref().is_ok_and(|n| n.list == passing(list, passed) && n.left.contains(took))
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
                    Change::App { .. } | Change::Action { .. } | Change::Pending { .. } => Some(c.clone()),
                });
                let mut list: Vec<Change> = kept.collect();
                let new: Vec<Change> = changes
                    .iter()
                    .filter(|l| l.seq > s.seen && !list.iter().any(|c| c.seq() == Some(l.seq)) && !s.left.contains(&l.seq))
                    .map(|l| Change::Listed { seq: l.seq, name: l.name, press: Press::No })
                    .collect();
                list.extend(new);
                let left: Vec<u64> = s
                    .left
                    .iter()
                    .copied()
                    .filter(|seq| changes.iter().any(|c| c.seq == *seq) || changes.iter().all(|c| c.seq < *seq))
                    .collect();
                n.list == capped(list) && n.seen == changes.iter().map(|c| c.seq).fold(s.seen, u64::max) && n.left == left
            },
        },
        Row {
            says: "a torrent action's notice's Undo takes that action, while ⌘Z hasn't",
            when: |s, e| matches!(e, C(Command::PressAction { seq }) if s.list.contains(&action(*seq, Press::No))),
            then: |s, e, r| {
                let C(Command::PressAction { seq }) = e else { unreachable!() };
                same_but(s, r, |c| Some(if *c == action(*seq, Press::No) { action(*seq, Press::Taken) } else { c.clone() }))
            },
        },
        Row {
            says: "a torrent action's notice's Undo on one taken, or not in the list, does nothing",
            when: |s, e| matches!(e, C(Command::PressAction { seq }) if !s.list.contains(&action(*seq, Press::No))),
            then: |_, _, r| *r == Err(Nothing),
        },
        Row {
            says: "⌘Z in a Look Preset's editor takes its newest change, while not taken",
            when: |s, e| matches!(e, C(Command::PressApp { id }) if s.list.contains(&app(*id, Press::No))),
            then: |s, e, r| {
                let C(Command::PressApp { id }) = e else { unreachable!() };
                same_but(s, r, |c| Some(if *c == app(*id, Press::No) { app(*id, Press::Taken) } else { c.clone() }))
            },
        },
        Row {
            says: "⌘Z in a Look Preset's editor on a change taken, or not in the list, does nothing",
            when: |s, e| matches!(e, C(Command::PressApp { id }) if !s.list.contains(&app(*id, Press::No))),
            then: |_, _, r| *r == Err(Nothing),
        },
        Row {
            says: "the editor closed: its changes leave, and Done's one change of them is the newest",
            when: |_, e| matches!(e, C(Command::Fold { .. })),
            then: |s, e, r| {
                let C(Command::Fold { ids, into }) = e else { unreachable!() };
                let mut list: Vec<Change> =
                    s.list.iter().filter(|c| !matches!(c, Change::App { id, .. } if ids.contains(id))).cloned().collect();
                if let Some((id, of)) = into {
                    list.push(Change::App { id: *id, of: *of, press: Press::No });
                }
                r.as_ref().is_ok_and(|n| n.list == capped(list) && (n.seen, n.at, &n.stack) == (s.seen, s.at, &s.stack))
            },
        },
        Row {
            says: "a torrent action taken is asked of the service, once",
            when: |_, e| matches!(e, C(Command::AskedAction { .. })),
            then: |s, e, r| {
                let C(Command::AskedAction { seq }) = e else { unreachable!() };
                same_but(s, r, |c| Some(if *c == action(*seq, Press::Taken) { action(*seq, Press::Asked) } else { c.clone() }))
            },
        },
        Row {
            says: "the entry the service took leaves, for good, listed yet or still the app's pending action (its press, if taken, on the newest not taken); the one asked for, if another was taken or none, can be taken again",
            when: |_, e| matches!(e, O(Observed::ActionAnswered { .. })),
            then: |s, e, r| {
                let O(Observed::ActionAnswered { asked, took }) = e else { unreachable!() };
                let seq = took.as_ref().map(|t| t.seq);
                let mut list: Vec<Change> = s.list.iter().filter(|c| c.entry().is_none_or(|e| Some(e) != seq)).cloned().collect();
                let pending = took.as_ref().and_then(|t| list.iter().position(|c| matches!(c, Change::Pending { after, torrents, .. } if t.seq > *after && t.torrents.iter().any(|x| torrents.contains(x)))));
                let passed = pending.is_some_and(|i| list.remove(i).press() != Press::No);
                let list: Vec<Change> = list
                    .into_iter()
                    .map(|c| if c.entry() == Some(*asked) && c.press() == Press::Asked { c.with(Press::No) } else { c })
                    .collect();
                r.as_ref().is_ok_and(|n| {
                    n.list == passing(list, passed) && seq.is_none_or(|t| n.stack.left.contains(&t)) && n.stack.at == s.stack.at
                })
            },
        },
        Row {
            says: "a stack older than the one taken changes nothing",
            when: |s, e| matches!(e, O(Observed::Stack { at, .. }) if *at < s.stack.at),
            then: |s, _, r| r.as_ref() == Ok(s),
        },
        Row {
            says: "a stack no older: the torrent actions it lists stay in place, those it doesn't leave; the app's own, sent, at its entry (the first after what it knew, of one of its torrents) in place, its press kept; another client's are the newest, in its order, but those an answer took off, which it forgets once a snapshot of the same service run lists a newer entry and not them, or one of another run lacks them; the settings changes stay",
            when: |s, e| matches!(e, O(Observed::Stack { at, .. }) if *at >= s.stack.at),
            then: |s, e, r| {
                let O(Observed::Stack { at, entries }) = e else { unreachable!() };
                let Ok(n) = r else { return false };
                let listed = |seq: u64| entries.iter().any(|e| e.seq == seq);
                let left: Vec<u64> = s
                    .stack
                    .left
                    .iter()
                    .copied()
                    .filter(|seq| listed(*seq) || (at.0 == s.stack.at.0 && !entries.iter().any(|e| e.seq > *seq)))
                    .collect();
                let mut list: Vec<Change> = s.list.iter().filter(|c| c.entry().is_none_or(listed)).cloned().collect();
                for i in 0..list.len() {
                    let Change::Pending { after, torrents, press, .. } = list[i].clone() else { continue };
                    let taken: Vec<u64> = list.iter().filter_map(Change::entry).collect();
                    let at = entries.iter().find(|e| {
                        e.seq > after
                            && e.torrents.iter().any(|t| torrents.contains(t))
                            && !taken.contains(&e.seq)
                            && !left.contains(&e.seq)
                    });
                    if let Some(e) = at {
                        list[i] = action(e.seq, press);
                    }
                }
                let known: Vec<u64> = list.iter().filter_map(Change::entry).collect();
                let new: Vec<Change> = entries
                    .iter()
                    .filter(|e| !known.contains(&e.seq) && !left.contains(&e.seq))
                    .map(|e| action(e.seq, Press::No))
                    .collect();
                list.extend(new);
                n.list == list
                    && n.stack == Stack { at: *at, left }
                    && (n.seen, n.at, n.listing, &n.left) == (s.seen, s.at, s.listing, &s.left)
            },
        },
        Row {
            says: "a torrent action the app sends is the newest the moment it's sent",
            when: |_, e| matches!(e, C(Command::Acted { .. })),
            then: |s, e, r| {
                let C(Command::Acted { id, after, torrents }) = e else { unreachable!() };
                pushed(s, r, Change::Pending { id: *id, after: *after, torrents: torrents.clone(), press: Press::No })
            },
        },
        Row {
            says: "a torrent action sent and answered is listed in place, its press kept, unless it made none or an answer took its entry already: then its press, if taken, goes to the newest not taken",
            when: |_, e| matches!(e, O(Observed::Made { .. })),
            then: |s, e, r| {
                let O(Observed::Made { id, seq }) = e else { unreachable!() };
                let is = |c: &Change| matches!(c, Change::Pending { id: i, .. } if i == id);
                let place = seq.filter(|seq| !s.list.iter().any(|c| c.entry() == Some(*seq)) && !s.stack.left.contains(seq));
                // The first sent as `id`: the sequences may send two.
                let first = s.list.iter().position(is);
                let passed = place.is_none() && first.is_some_and(|i| s.list[i].press() != Press::No);
                let list: Vec<Change> = s
                    .list
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| if Some(i) == first { place.map(|seq| action(seq, c.press())) } else { Some(c.clone()) })
                    .collect();
                r.as_ref().is_ok_and(|n| n.list == passing(list, passed) && n.stack == s.stack)
            },
        },
    ]
}

fn always() -> Vec<R> {
    vec![Row::always("at most 20 settings changes, each service change once, each torrent action once", |_, _, r| {
        r.as_ref().map_or(true, |n| {
            let once = |ids: Vec<u64>| ids.iter().all(|s| ids.iter().filter(|t| *t == s).count() == 1);
            let settings = n.list.iter().filter(|c| c.is_setting()).count();
            settings <= KEPT
                && once(n.list.iter().filter_map(Change::seq).collect())
                && once(n.list.iter().filter_map(Change::entry).collect())
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

/// What `next` carries out: the newest change taken, an app's to undo, a listed one or a torrent
/// action to ask for; a change sent and not listed yet waits.
#[test]
fn next_carries_out_the_newest_change_taken() {
    for s in State::every() {
        let expected = s.list.iter().rev().find_map(|c| match (c, c.press()) {
            (Change::App { id, .. }, Press::Taken) => Some(Do::Undo { id: *id }),
            (Change::Listed { seq, .. }, Press::Taken) => Some(Do::Ask { seq: *seq }),
            (Change::Action { seq, .. }, Press::Taken) => Some(Do::AskAction { seq: *seq }),
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
        Event::Observed(Observed::Undid { asked: 4, took: 5, by: Some(11) }),
        Event::Observed(Observed::Answered { by: 11, seq: Some(5), at: 9 }),
    ];
    let state = events.iter().fold(State::default(), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, [listed(4, Press::Taken)]);
}

/// A service that lost its `undo.json` numbers its entries afresh, from 1 again (#255's review):
/// the actions it lists are the app's to undo all the same, whatever the earlier one numbered.
#[test]
fn a_new_service_numbering_its_entries_afresh_has_its_actions_undone() {
    let events = [
        Event::Observed(Observed::Stack { at: (1, 5), entries: entries(&[1, 3, 4]) }),
        Event::Command(Command::Press),
        Event::Command(Command::AskedAction { seq: 4 }),
        Event::Observed(Observed::ActionAnswered { asked: 4, took: Some(entry(4)) }),
        // A new service, its stack lost: nothing, then entry 1 again, and 4.
        Event::Observed(Observed::Stack { at: (2, 0), entries: entries(&[]) }),
        Event::Observed(Observed::Stack { at: (2, 1), entries: entries(&[1, 4]) }),
    ];
    let state = events.iter().fold(State::default(), |s, e| decide(&s, e).unwrap());
    assert_eq!(state.list, [action(1, Press::No), action(4, Press::No)], "both listed");
    let pressed = decide(&state, &Event::Command(Command::Press)).unwrap();
    assert_eq!(next(&pressed), Some(Do::AskAction { seq: 4 }), "and ⌘Z takes the newest");
}

// The whole of it, generated: the app, the service and another client (the CLI), with every message
// between the app and the service on its way for as long as the sequence says, delivered in any
// order (each command has its own connection) and older snapshots among them, an answer to a
// torrent request lost as a connection that drops loses it (the request having reached the service
// or not); the service may restart. Settings changes and torrent actions (#255) are one list. The
// truth the app's list is checked against is when the app learned of each change: the app's own
// (a setting's, a service setting's or a torrent action it sends) when the person makes it;
// another client's when the app first gets a snapshot newer than any it had that lists it; and the
// app's own torrent action whose answer was lost before any snapshot listed it, as another
// client's: the app can't tell it's its own.

/// What happens next.
#[derive(Debug, Clone)]
enum Op {
    /// The app changes its setting `0..2` or the service's `0..2`.
    App(usize),
    Service(usize),
    /// The app removes, deletes or moves to the Trash: a torrent action, on its own connection.
    Act,
    /// The CLI changes the service's setting, or undoes its newest change; makes a torrent action,
    /// or undoes the top one (`phantom undo`).
    Cli(usize),
    CliUndo,
    CliAct,
    CliUndoAction,
    /// ⌘Z.
    Press,
    /// The request on its way at this index (modulo how many there are) reaches the service; a
    /// message to the app reaches it.
    ToService(usize),
    ToApp(usize),
    /// The torrent request on its way at this index (modulo how many) is answered with nothing: its
    /// connection dropped, after it reached the service or before.
    Lost(usize, bool),
    /// The torrent undo on its way at this index (modulo how many) reaches the service, which
    /// refuses it: the entry is gone (ADR 0008), and the refusal comes alone, with no snapshot after
    /// it (`service/socket.rs` takes one only after a command that succeeded).
    Refuse(usize),
    /// The service sends the app its snapshot: the settings, their changes and the undo stack.
    Stream,
    /// The service stops and another starts: its settings changes gone, the values and the undo
    /// stack kept, every request on its way answered that it's stopping.
    Restart,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        3 => (0..2usize).prop_map(Op::App),
        3 => (0..2usize).prop_map(Op::Service),
        3 => Just(Op::Act),
        2 => (0..2usize).prop_map(Op::Cli),
        1 => Just(Op::CliUndo),
        2 => Just(Op::CliAct),
        1 => Just(Op::CliUndoAction),
        3 => Just(Op::Press),
        3 => (0..4usize).prop_map(Op::ToService),
        4 => (0..6usize).prop_map(Op::ToApp),
        1 => (0..3usize, any::<bool>()).prop_map(|(i, reached)| Op::Lost(i, reached)),
        1 => (0..3usize).prop_map(Op::Refuse),
        3 => Just(Op::Stream),
        1 => Just(Op::Restart),
    ];
    // Fewer than 20 settings changes and 20 torrent actions in all, so none is pushed past the last 20.
    proptest::collection::vec(op, 0..40)
        .prop_filter("under 20 settings changes", |ops| {
            ops.iter().filter(|o| matches!(o, Op::App(_) | Op::Service(_) | Op::Cli(_))).count() < KEPT
        })
        .prop_filter("under 20 torrent actions", |ops| ops.iter().filter(|o| matches!(o, Op::Act | Op::CliAct)).count() < proto::UNDO_DEPTH)
}

/// Short sequences of the torrents' races: answers lost or refused (with no snapshot after them)
/// against snapshots still on their way, the app's own actions and another client's, and ⌘Z.
fn races() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        2 => Just(Op::Act),
        2 => Just(Op::CliAct),
        1 => Just(Op::App(0)),
        3 => Just(Op::Stream),
        3 => Just(Op::Press),
        2 => (0..3usize).prop_map(Op::ToService),
        2 => (0..3usize).prop_map(Op::Refuse),
        1 => (0..3usize, any::<bool>()).prop_map(|(i, reached)| Op::Lost(i, reached)),
        4 => (0..4usize).prop_map(Op::ToApp),
    ];
    proptest::collection::vec(op, 0..12)
}

const SERVICE: [SettingName; 2] = [SettingName::ListenPort, SettingName::Upnp];

#[derive(Debug)]
enum Request {
    Set {
        name: SettingName,
        value: u64,
        by: u64,
    },
    Undo {
        asked: u64,
    },
    /// The app's torrent action `id`, of its own torrent.
    Act {
        id: u64,
        torrent: u64,
    },
    UndoAction {
        asked: u64,
    },
}

#[derive(Debug)]
enum Message {
    Snapshot {
        at: u64,
        changes: Vec<SettingChange>,
        version: (u64, u64),
        stack: Vec<Entry>,
    },
    Answered {
        by: u64,
        seq: Option<u64>,
        at: u64,
    },
    Undid {
        asked: u64,
        took: Option<SettingChange>,
    },
    Made {
        id: u64,
        seq: Option<u64>,
    },
    ActionUndid {
        asked: u64,
        took: Option<Entry>,
    },
    /// A command's answer after the snapshot its connection takes once the service has published
    /// its effect (`service/socket.rs`), the two delivered together.
    Reply {
        snapshot: Box<Message>,
        answer: Box<Message>,
    },
}

/// The service, as `lifecycle::settings` keeps its changes; a value per setting, each change a new
/// one. Its undo stack, numbered on, and its snapshots by `(epoch, version)`.
struct Service {
    values: BTreeMap<SettingName, u64>,
    changes: ServiceChanges<u64>,
    at: u64,
    /// The values when it started: what undoing all it has goes back to.
    start: BTreeMap<SettingName, u64>,
    stack: Vec<Entry>,
    entries: u64,
    version: (u64, u64),
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

    /// A torrent action on `torrent`: its entry.
    fn act(&mut self, torrent: u64) -> u64 {
        self.entries += 1;
        self.stack.push(Entry { seq: self.entries, torrents: vec![torrent] });
        self.version.1 += 1;
        self.entries
    }

    /// The top entry, undone or refused: off the stack either way.
    fn undo_action(&mut self) -> Option<Entry> {
        let top = self.stack.pop()?;
        self.version.1 += 1;
        Some(top)
    }

    fn top(&self) -> Option<u64> {
        self.stack.last().map(|e| e.seq)
    }

    fn snapshot(&self) -> Message {
        Message::Snapshot { at: self.at, changes: self.changes.listed(), version: self.version, stack: self.stack.clone() }
    }
}

/// A change, as the test knows it: an app setting's by id, a service setting's the app sent by the
/// id it was sent by, another client's by its `seq`; a torrent action the app sent by its id,
/// another client's by its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Made {
    App(u64),
    Sent(u64),
    Other(u64),
    Acted(u64),
    Action(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Undone {
    App(u64),
    /// The service undid `seq` for the app's press.
    Service(u64),
    /// The service took the entry `seq` for the app's press.
    Action(u64),
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
    /// Every change, in the order the app learned of it.
    made: Vec<Made>,
    /// The `seq` the service gave each change the app sent, by what it was sent by; and the entry
    /// each torrent action the app sent made, by its id, while the app can know it as its own.
    seqs: BTreeMap<u64, u64>,
    acts: BTreeMap<u64, u64>,
    /// The newest settings the app has had, by their `at`, and the newest stack, by its snapshot's
    /// `(epoch, version)`, with the newest entry on it.
    newest: u64,
    newest_stack: (u64, u64),
    after: u64,
    /// The changes ⌘Z took, until undone or gone.
    taken: Vec<Made>,
    /// The changes an answer said the service undid (or refused): never listed again.
    answered: Vec<Made>,
    value: u64,
}

impl World {
    fn new() -> World {
        let values: BTreeMap<SettingName, u64> = SERVICE.iter().map(|n| (*n, 0)).collect();
        let service = Service {
            values: values.clone(),
            changes: ServiceChanges::new(100),
            at: 1,
            start: values,
            stack: Vec::new(),
            entries: 0,
            version: (1, 1),
        };
        World {
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
            acts: BTreeMap::new(),
            newest: 0,
            newest_stack: (0, 0),
            after: 0,
            taken: Vec::new(),
            answered: Vec::new(),
            value: 1,
        }
    }

    fn decide(&mut self, event: Event) -> Result<(), Nothing> {
        self.app = decide(&self.app, &event)?;
        Ok(())
    }

    /// The change an entry of the stack is.
    fn of_entry(&self, seq: u64) -> Made {
        self.acts.iter().find(|(_, s)| **s == seq).map_or(Made::Action(seq), |(id, _)| Made::Acted(*id))
    }

    /// The change an entry of the app's list is.
    fn which(&self, change: &Change) -> Made {
        match change {
            Change::App { id, .. } => Made::App(*id),
            Change::Sent { by, .. } => Made::Sent(*by),
            Change::Listed { seq, .. } => self.seqs.iter().find(|(_, s)| *s == seq).map_or(Made::Other(*seq), |(by, _)| Made::Sent(*by)),
            Change::Action { seq, .. } => self.of_entry(*seq),
            Change::Pending { id, .. } => Made::Acted(*id),
        }
    }

    /// A press whose change went before it was carried out goes to the newest change not taken, in
    /// the order the app learned of them: two presses are still two undos.
    fn pass(&mut self) {
        let untaken = self.app.list.iter().map(|c| self.which(c)).filter(|m| !self.taken.contains(m));
        if let Some(next) = untaken.max_by_key(|m| self.made.iter().position(|x| x == m)) {
            self.taken.push(next);
        }
    }

    /// Carries out what the machine says: an app change undone at once, a service one or a torrent
    /// action asked.
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
                Do::AskAction { seq } => {
                    self.requests.push(Request::UndoAction { asked: seq });
                    self.decide(Event::Command(Command::AskedAction { seq })).unwrap();
                }
            }
        }
    }

    /// The request at `i` reaches the service: what it answers.
    fn serve(&mut self, i: usize) -> Message {
        match self.requests.remove(i) {
            Request::Set { name, value, by } => {
                let change = self.service.set(name, value, Some(by));
                self.seqs.insert(by, change.seq);
                Message::Answered { by, seq: Some(change.seq), at: self.service.at }
            }
            Request::Undo { asked } => Message::Undid { asked, took: self.service.undo() },
            Request::Act { id, torrent } => {
                let seq = self.service.act(torrent);
                self.acts.insert(id, seq);
                self.reply(Message::Made { id, seq: Some(seq) })
            }
            Request::UndoAction { asked } => {
                let took = self.service.undo_action();
                self.reply(Message::ActionUndid { asked, took })
            }
        }
    }

    fn reply(&self, answer: Message) -> Message {
        Message::Reply { snapshot: Box::new(self.service.snapshot()), answer: Box::new(answer) }
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
            Op::Act => {
                let id = self.next;
                self.next += 1;
                // Each action its own torrent: the app's, `1_000 +` its id.
                self.requests.push(Request::Act { id, torrent: 1_000 + id });
                self.made.push(Made::Acted(id));
                self.decide(Event::Command(Command::Acted { id, after: self.after, torrents: vec![1_000 + id] })).unwrap();
            }
            Op::Cli(i) => {
                self.service.set(SERVICE[*i], value, None);
            }
            Op::CliUndo => {
                self.service.undo();
            }
            Op::CliAct => {
                let torrent = 2_000 + value;
                self.service.act(torrent);
            }
            Op::CliUndoAction => {
                self.service.undo_action();
            }
            Op::Press => {
                let top = self.app.top().cloned();
                match self.decide(Event::Command(Command::Press)) {
                    Ok(()) => self.taken.push(self.which(&top.expect("there was one to take"))),
                    Err(Nothing) => assert!(top.is_none(), "{top:?} was there to take"),
                }
                self.run();
            }
            Op::ToService(i) if !self.requests.is_empty() => {
                let answer = self.serve(*i % self.requests.len());
                self.messages.push(answer);
            }
            Op::Lost(i, reached) => {
                let torrent = |r: &Request| matches!(r, Request::Act { .. } | Request::UndoAction { .. });
                let at: Vec<usize> = (0..self.requests.len()).filter(|j| torrent(&self.requests[*j])).collect();
                if at.is_empty() {
                    return;
                }
                let j = at[*i % at.len()];
                let answer = match reached {
                    true => self.serve(j),
                    false => self.requests.remove(j).lost(),
                };
                // Lost with its connection: no snapshot either.
                let answer = match answer {
                    Message::Reply { answer, .. } => *answer,
                    other => other,
                };
                self.messages.push(match answer {
                    Message::Made { id, .. } => Message::Made { id, seq: None },
                    Message::ActionUndid { asked, .. } => Message::ActionUndid { asked, took: None },
                    other => other,
                });
            }
            Op::Refuse(i) => {
                let at: Vec<usize> = (0..self.requests.len()).filter(|j| matches!(self.requests[*j], Request::UndoAction { .. })).collect();
                if at.is_empty() {
                    return;
                }
                if let Message::Reply { answer, .. } = self.serve(at[*i % at.len()]) {
                    self.messages.push(*answer);
                }
            }
            Op::ToApp(i) if !self.messages.is_empty() => {
                let message = self.messages.remove(*i % self.messages.len());
                self.deliver(message);
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
                    stack: std::mem::take(&mut self.service.stack),
                    entries: self.service.entries,
                    version: (self.service.version.0 + 1, 0),
                };
                for request in std::mem::take(&mut self.requests) {
                    self.messages.push(request.lost());
                }
            }
        }
    }

    /// A message reaches the app.
    fn deliver(&mut self, message: Message) {
        match message {
            Message::Reply { snapshot, answer } => {
                self.deliver(*snapshot);
                self.deliver(*answer);
            }
            // The settings first, then the torrents, as the app takes a snapshot.
            Message::Snapshot { at, changes, version, stack } => {
                if at > self.newest {
                    self.newest = at;
                    let new: Vec<Made> =
                        changes.iter().filter(|c| c.by.is_none()).map(|c| Made::Other(c.seq)).filter(|m| !self.made.contains(m)).collect();
                    self.made.extend(new);
                }
                self.decide(Event::Observed(Observed::Listed { at, changes })).unwrap();
                if version >= self.newest_stack {
                    self.newest_stack = version;
                    self.after = stack.iter().map(|e| e.seq).max().unwrap_or(0);
                    let new: Vec<Made> = stack.iter().map(|e| self.of_entry(e.seq)).filter(|m| !self.made.contains(m)).collect();
                    self.made.extend(new);
                }
                self.decide(Event::Observed(Observed::Stack { at: version, entries: stack })).unwrap();
            }
            Message::Answered { by, seq, at } => {
                // Settings before it are older; those right after it aren't.
                self.newest = self.newest.max(at.saturating_sub(1));
                let waiting =
                    self.app.list.iter().any(|c| matches!(c, Change::Sent { by: b, press, .. } if *b == by && *press != Press::No));
                self.decide(Event::Observed(Observed::Answered { by, seq, at })).unwrap();
                // Taken while it waited for this, and gone: its press goes on.
                let listed = self.app.list.iter().any(|c| self.which(c) == Made::Sent(by));
                if waiting && !listed {
                    self.taken.retain(|t| *t != Made::Sent(by));
                    self.pass();
                }
            }
            Message::Undid { asked, took: Some(took) } => {
                self.undone.push(Undone::Service(took.seq));
                let made = self.which(&listed(took.seq, Press::No));
                self.answered.push(made);
                // The press on `asked` is spent; `took`'s goes with it, listed or still
                // sent, and if it was taken while it waited, it goes on.
                let waiting =
                    self.app.list.iter().any(|c| self.which(c) == made && matches!(c, Change::Sent { .. }) && c.press() != Press::No);
                let known = self.app.list.iter().any(|c| self.which(c) == made);
                let gone = [Some(self.which(&listed(asked, Press::No))), known.then_some(made)];
                self.taken.retain(|t| !gone.contains(&Some(*t)));
                self.decide(Event::Observed(Observed::Undid { asked, took: took.seq, by: took.by })).unwrap();
                if waiting {
                    self.pass();
                }
            }
            Message::Undid { asked, took: None } => {
                let made = self.which(&listed(asked, Press::No));
                self.taken.retain(|t| *t != made);
                self.decide(Event::Observed(Observed::Refused { asked })).unwrap();
            }
            Message::Made { id, seq } => {
                let pending = self.app.list.iter().find(|c| matches!(c, Change::Pending { id: i, .. } if *i == id)).cloned();
                let gone = |seq: u64| self.app.list.iter().any(|c| c.entry() == Some(seq)) || self.app.stack.left.contains(&seq);
                if let Some(pending) = pending
                    && seq.is_none_or(gone)
                {
                    // Its answer lost before any snapshot listed it: as another client's from now.
                    if seq.is_none() {
                        self.acts.remove(&id);
                    }
                    if pending.press() != Press::No {
                        self.taken.retain(|t| *t != Made::Acted(id));
                        self.decide(Event::Observed(Observed::Made { id, seq })).unwrap();
                        self.pass();
                        self.run();
                        return;
                    }
                }
                self.decide(Event::Observed(Observed::Made { id, seq })).unwrap();
            }
            // The press on `asked` is spent, and `took` is gone, whichever it was.
            // An entry taken that's the app's own, pending yet: it goes, and a press taken on it goes on.
            Message::ActionUndid { asked, took } => {
                self.undone.extend(took.as_ref().map(|e| Undone::Action(e.seq)));
                let gone = took.as_ref().map(|e| self.of_entry(e.seq));
                self.answered.extend(gone);
                let waiting = self
                    .app
                    .list
                    .iter()
                    .any(|c| matches!(c, Change::Pending { .. }) && Some(self.which(c)) == gone && c.press() != Press::No);
                let asked_made = self.of_entry(asked);
                self.taken.retain(|t| *t != asked_made && Some(*t) != gone);
                self.decide(Event::Observed(Observed::ActionAnswered { asked, took })).unwrap();
                if waiting {
                    self.pass();
                }
            }
        }
    }

    /// Whether `made` is still to be undone.
    fn live(&self, made: Made) -> bool {
        let on_service = |seq: u64| self.service.changes.listed().iter().any(|c| c.seq == seq);
        let on_stack = |seq: u64| self.service.stack.iter().any(|e| e.seq == seq);
        match made {
            Made::App(id) => self.was.contains_key(&id),
            Made::Sent(by) => self.seqs.get(&by).is_some_and(|seq| on_service(*seq)),
            Made::Other(seq) => on_service(seq),
            Made::Acted(id) => self.acts.get(&id).is_some_and(|seq| on_stack(*seq)),
            Made::Action(seq) => self.of_entry(seq) == made && on_stack(seq),
        }
    }

    /// The app's list is in the order the app learned of the changes, and holds a press exactly on
    /// what ⌘Z took.
    fn check(&self) -> Result<(), TestCaseError> {
        let at: Vec<usize> = self
            .app
            .list
            .iter()
            .map(|c| self.made.iter().position(|m| *m == self.which(c)).unwrap_or_else(|| panic!("{c:?} was never made")))
            .collect();
        prop_assert!(at.windows(2).all(|w| w[0] < w[1]), "in the order learned: {:?} at {:?} of {:?}", self.app.list, at, self.made);
        for c in &self.app.list {
            prop_assert_eq!(c.press() != Press::No, self.taken.contains(&self.which(c)), "taken exactly when ⌘Z took it: {:?}", c);
            prop_assert!(!self.answered.contains(&self.which(c)), "an answer said {:?} is undone: it's never listed again", c);
        }
        Ok(())
    }

    /// Every request reaches the service, every message the app, and then the newest snapshot.
    fn drain(&mut self) {
        while !self.requests.is_empty() || !self.messages.is_empty() {
            let op = if self.requests.is_empty() { Op::ToApp(0) } else { Op::ToService(0) };
            self.step(&op);
        }
        self.step(&Op::Stream);
        self.step(&Op::ToApp(0));
    }
}

impl Request {
    /// The answer to it when it never reached the service: it's stopping, or the connection went.
    fn lost(self) -> Message {
        match self {
            Request::Set { by, .. } => Message::Answered { by, seq: None, at: 0 },
            Request::Undo { asked } => Message::Undid { asked, took: None },
            Request::Act { id, .. } => Message::Made { id, seq: None },
            Request::UndoAction { asked } => Message::ActionUndid { asked, took: None },
        }
    }
}

/// Plays `ops`, then everything arrives and ⌘Z is pressed until nothing's left: the property.
fn play(ops: &[Op]) -> Result<(), TestCaseError> {
    let mut w = World::new();
    for op in ops {
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
        let stack_top = w.service.top();
        w.step(&Op::Press);
        w.drain();
        w.check()?;
        // A service setting's, the service's newest; a torrent action, the top of the stack: the
        // app's newest too, unless two it sent reached the service the other way round (each on its
        // own connection).
        let expected = match newest {
            Made::App(id) => Undone::App(id),
            Made::Sent(_) | Made::Other(_) => Undone::Service(service_newest.expect("listed, so there")),
            Made::Acted(_) | Made::Action(_) => Undone::Action(stack_top.expect("listed, so there")),
        };
        prop_assert_eq!(w.undone[count..].to_vec(), vec![expected], "a press takes the newest");
        presses += 1;
        prop_assert!(presses <= KEPT + proto::UNDO_DEPTH, "it ends");
    }
    prop_assert_eq!(w.values, [0, 0], "the app's settings are as they were");
    prop_assert_eq!(&w.service.values, &w.service.start, "and the service's, as it started");
    prop_assert!(w.service.changes.listed().is_empty() && w.app.list.is_empty(), "nothing left on either side");
    prop_assert!(w.service.stack.is_empty(), "and no torrent action left on the stack");
    Ok(())
}

/// The review's blocking case (#258): a port sent and ⌘Z pressed for it; the port sent again
/// reaches the service before the undo, which so undoes the second, and says so before the
/// second's own answer. The second leaves the list at once: it's undone.
#[test]
fn a_sent_change_the_service_undid_by_a_race_leaves_at_once() {
    let ops = [
        Op::Service(0),
        Op::ToService(0),
        Op::ToApp(0),
        Op::Press,
        Op::Service(0),
        // The second change, then the undo: each answered in that order, the undo's delivered first.
        Op::ToService(1),
        Op::ToService(0),
        Op::ToApp(1),
    ];
    play(&ops).unwrap();
}

/// An entry the service took for a press, which the app hadn't learned of (another client's), and
/// refused (so its answer comes with no snapshot), and a snapshot from before the undo, newer than
/// any the app had, that still lists it: it isn't added back (#258's M2).
#[test]
fn an_entry_an_answer_took_isnt_listed_by_an_older_snapshot() {
    let ops = [
        Op::CliAct,
        Op::Stream,
        Op::ToApp(0),
        Op::CliAct,
        Op::Stream,
        Op::Press,
        Op::Refuse(0),
        // The refusal, then the snapshot from before it.
        Op::ToApp(1),
        Op::ToApp(0),
    ];
    play(&ops).unwrap();
}

/// ⌫ and then ⌘Z before any snapshot lists the Remove, with a settings change made before it: ⌘Z
/// takes the Remove, the newest (#258's should-fix 2).
#[test]
fn a_press_right_after_the_apps_own_action_takes_it() {
    let mut w = World::new();
    for op in [Op::App(0), Op::Act, Op::Press] {
        w.step(&op);
    }
    assert!(
        matches!(w.app.list[..], [Change::App { press: Press::No, .. }, Change::Pending { press: Press::Taken, .. }]),
        "{:?}",
        w.app.list
    );
    play(&[Op::App(0), Op::Act, Op::Press]).unwrap();
}

/// An undo of the app's own action 3 (entry 4) is answered before the
/// snapshots that list entry 4: one from before it was made (entries 1 to 3) comes first, and no
/// longer having it must not read as it having left, or the next snapshot, which still lists it,
/// adds back an entry that is already undone: ⌘Z then takes a change that isn't there.
#[test]
fn an_entry_undone_before_its_listing_arrives_isnt_listed_when_an_older_snapshot_came_first() {
    let ops = [
        Op::Act,
        Op::Press,
        Op::ToService(0),
        Op::Act,
        Op::CliAct,
        Op::ToService(0),
        Op::Act,
        Op::ToService(0),
        Op::Stream,
        Op::ToApp(0),
        Op::Refuse(0),
        Op::Service(0),
        Op::Restart,
        Op::ToApp(3),
        Op::Press,
    ];
    play(&ops).unwrap();
}

/// The service restarts, and the app asks to undo entries 2 and 1; the
/// answer for 2 comes with the new run's snapshot, which still has entry 1 (taken just after), and
/// 1's answer reaches the app first. That snapshot lists 1: of the new run, so it may be a new
/// entry numbered afresh, and it's no proof 1 isn't there; the answer's word is kept, 1 isn't listed.
#[test]
fn an_entry_undone_is_kept_off_the_list_by_a_new_runs_older_snapshot_that_lists_it() {
    let ops = [
        Op::Act,
        Op::Cli(0),
        Op::CliAct,
        Op::ToService(0),
        Op::Restart,
        Op::Press,
        Op::ToApp(0),
        Op::Press,
        Op::ToService(0),
        Op::Refuse(0),
        Op::ToApp(1),
        Op::ToApp(0),
    ];
    play(&ops).unwrap();
}

/// A service setting undone is answered before the listing that has it, and a listing from before it
/// was made (without it, but not newer) comes first: no longer having it isn't it having left, or
/// the next listing, which still has it, adds back a change already undone.
#[test]
fn a_setting_undone_before_its_listing_arrives_isnt_listed_when_an_older_listing_came_first() {
    let ops = [
        Op::Act,
        Op::Cli(0),
        Op::Stream,
        Op::Cli(0),
        Op::ToApp(0),
        Op::Stream,
        Op::Press,
        Op::ToService(2),
        Op::Cli(0),
        Op::Stream,
        Op::ToService(0),
        Op::ToApp(3),
        Op::Stream,
        Op::ToApp(0),
        Op::Press,
        Op::ToService(0),
        Op::ToApp(5),
    ];
    play(&ops).unwrap();
}

proptest! {
    // Cheap (pure, a few ms a thousand), so many by default: the races it looks for are rare.
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(1024), ..ProptestConfig::default() })]

    /// Any interleaving of the app's changes, the service's (made in the app, or by the CLI), torrent
    /// actions (the app's or the CLI's) and the CLI's undos of either, with every message late and
    /// out of order, answers lost with their connection, and the service restarting: the app's list
    /// is always in the order the app learned of the changes, each press kept on what it took, never
    /// listing what an answer said is undone; once everything has arrived it holds exactly the
    /// changes still to undo; ⌘Z then takes the newest each time (a service setting's: the service's
    /// newest; a torrent action: the top of the stack), and pressing it until nothing's left puts
    /// every setting back as it was and empties the stack, nothing left on either side.
    #[test]
    fn every_change_is_undone_newest_first(ops in ops()) {
        play(&ops)?;
    }

    /// The same, over short sequences where the torrents' races are dense.
    #[test]
    fn every_change_is_undone_newest_first_through_the_torrents_races(ops in races()) {
        play(&ops)?;
    }
}
