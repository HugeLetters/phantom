//! The status bar's notices as a machine (#120): which notices it holds, and whether their
//! popover is open. Pure; [`super::Bar`] carries it out.
//!
//! A notice is **told** (something happened: an add, the settings reset) and stays until it's
//! dismissed, or **live** (a condition: a disk short of space), there exactly while the latest
//! snapshot says so. The bar shows the newest, with how many there are; a click opens the popover,
//! which lists them all, newest first, each with its explanation.
//!
//! | State \ Event | Tell(n) | Dismiss(id) | Live(list) | Press | Close |
//! |---|---|---|---|---|---|
//! | any | appended (the oldest told one dropped past [`MAX_TOLD`]) | a told one goes; a live one or none: refused | live ones become `list`: same key keeps its place | something to open: open. Else the only notice told: it goes. Else refused | open: closed. Closed: refused, so Escape goes on to the page |
//!
//! There's something to open while there are several notices, or one with more to read. The
//! popover is open only while there is.
//!
//! Escape goes to the popover first: while it's open, Escape closes it and does nothing else (the
//! selection and the error list stay), and only the next Escape reaches the page.

use crate::notice::Notice;

pub type NoticeId = u64;

/// The most told notices kept: older ones go as new ones come.
pub const MAX_TOLD: usize = 5;

/// Where a notice comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Told once; stays until dismissed.
    Told,
    /// A condition, by its key (a disk's mount): there while it holds.
    Live(String),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: NoticeId,
    pub source: Source,
    pub notice: Notice,
}

/// Two entries are the same if their ids, sources and what they say are: a notice's view is a
/// closure, which can't be compared.
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.source, &self.notice.short, self.notice.severity, self.notice.more.is_some())
            == (other.id, &other.source, &other.notice.short, other.notice.severity, other.notice.more.is_some())
    }
}

/// The notices, oldest first, and whether the popover is open. Ids are never reused.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub entries: Vec<Entry>,
    pub open: bool,
    next_id: NoticeId,
}

impl State {
    /// Whether a click has something to open: several notices, or one with more to read.
    pub fn openable(&self) -> bool {
        self.entries.len() > 1 || self.entries.iter().any(|e| e.notice.more.is_some())
    }

    /// The newest notice, which the bar shows.
    pub fn newest(&self) -> Option<&Entry> {
        self.entries.last()
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

#[derive(Debug, Clone)]
pub enum Command {
    Tell(Notice),
    /// A told notice's ×.
    Dismiss(NoticeId),
    /// A click on the bar: opens the popover, or dismisses its only notice if that has nothing
    /// more to read.
    Press,
    /// Escape, or a click outside the popover.
    Close,
}

#[derive(Debug, Clone)]
pub enum Observed {
    /// The conditions that hold now, by key, each as its notice.
    Live(Vec<(String, Notice)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Dismissed already, or never there: a click that raced another.
    NoSuchNotice(NoticeId),
    /// A live notice goes when its condition does.
    Live(NoticeId),
    NothingToOpen,
    /// Escape with the popover closed: it's not the bar's.
    NotOpen,
}

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    let mut next = state.clone();
    match event {
        Event::Command(Command::Tell(notice)) => {
            next.entries.push(Entry { id: next.next_id, source: Source::Told, notice: notice.clone() });
            next.next_id += 1;
            while next.entries.iter().filter(|e| e.source == Source::Told).count() > MAX_TOLD {
                let oldest = next.entries.iter().position(|e| e.source == Source::Told).expect("more than none");
                next.entries.remove(oldest);
            }
        }
        Event::Command(Command::Dismiss(id)) => match next.entries.iter().position(|e| e.id == *id) {
            Some(at) if next.entries[at].source == Source::Told => {
                next.entries.remove(at);
            }
            Some(_) => return Err(Refusal::Live(*id)),
            None => return Err(Refusal::NoSuchNotice(*id)),
        },
        Event::Command(Command::Press) if state.openable() => next.open = true,
        Event::Command(Command::Press) => match state.entries.as_slice() {
            [only] => return decide(state, &Event::Command(Command::Dismiss(only.id))),
            _ => return Err(Refusal::NothingToOpen),
        },
        Event::Command(Command::Close) if state.open => next.open = false,
        Event::Command(Command::Close) => return Err(Refusal::NotOpen),
        Event::Observed(Observed::Live(live)) => {
            next.entries.retain(|e| match &e.source {
                Source::Told => true,
                Source::Live(key) => live.iter().any(|(k, _)| k == key),
            });
            for (key, notice) in live {
                let source = Source::Live(key.clone());
                match next.entries.iter_mut().find(|e| e.source == source) {
                    Some(entry) => entry.notice = notice.clone(),
                    None => {
                        next.entries.push(Entry { id: next.next_id, source, notice: notice.clone() });
                        next.next_id += 1;
                    }
                }
            }
        }
    }
    next.open &= next.openable();
    Ok(next)
}

/// The status bar's notices as a [`Machine`](lifecycle::machine::Machine): drawing follows the
/// state, so there's nothing to carry out. Only ever named.
#[allow(dead_code)]
pub struct Notices;

impl lifecycle::machine::Machine for Notices {
    type State = State;
    type Event = Event;
    type Refusal = Refusal;
    type World = ();
    type Do = std::convert::Infallible;

    fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
        decide(state, event)
    }

    fn next(_: &State, _: &()) -> Option<std::convert::Infallible> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use lifecycle::every;
    use lifecycle::machine::{DecideRow, Every, Row, decide_table, sequences};

    use super::*;

    fn plain(short: &str) -> Notice {
        Notice::warning(short)
    }

    fn with_more(short: &str) -> Notice {
        Notice { more: Some(Rc::new(|_, _| rune::el())), ..Notice::warning(short) }
    }

    fn state(entries: Vec<(Source, Notice)>, open: bool) -> State {
        let entries: Vec<Entry> =
            entries.into_iter().enumerate().map(|(i, (source, notice))| Entry { id: i as u64, source, notice }).collect();
        let next_id = entries.len() as u64;
        State { entries, open, next_id }
    }

    every!(State {
        State { .. } => {
            let told = |s: &str| (Source::Told, plain(s));
            let told_more = |s: &str| (Source::Told, with_more(s));
            let live = |k: &str| (Source::Live(k.into()), with_more(k));
            let many: Vec<_> = (0..MAX_TOLD).map(|i| told(&format!("t{i}"))).collect();
            let shapes = vec![vec![], vec![told("a")], vec![told_more("a")], vec![live("disk")], vec![told("a"), live("disk")], vec![live("disk"), live("usb")], many];
            shapes.into_iter().flat_map(|e| [false, true].map(move |open| (e.clone(), open))).map(|(e, open)| state(e, open)).filter(|s| !s.open || s.openable()).collect::<Vec<_>>()
        },
    });

    every!(Event {
        Event::Command(Command::Tell(_)) => [plain("new"), with_more("new")].map(|n| Event::Command(Command::Tell(n))),
        Event::Command(Command::Dismiss(_)) => (0..3).map(|id| Event::Command(Command::Dismiss(id))),
        Event::Command(Command::Press) => [Event::Command(Command::Press)],
        Event::Command(Command::Close) => [Event::Command(Command::Close)],
        Event::Observed(Observed::Live(_)) => [vec![], vec![("disk".to_owned(), with_more("disk, now"))], vec![("usb".to_owned(), with_more("usb"))]]
            .map(|l| Event::Observed(Observed::Live(l))),
    });

    fn told(s: &State) -> Vec<&Entry> {
        s.entries.iter().filter(|e| e.source == Source::Told).collect()
    }

    fn rows() -> Vec<DecideRow<Notices>> {
        vec![
            Row {
                says: "a notice told is the newest, the oldest told dropped past the most kept",
                when: |_, e| matches!(e, Event::Command(Command::Tell(_))),
                then: |s, e, out| {
                    let (Event::Command(Command::Tell(n)), Ok(out)) = (e, out) else { return false };
                    let newest = out.newest().unwrap();
                    newest.notice.short == n.short && newest.source == Source::Told && told(out).len() == (told(s).len() + 1).min(MAX_TOLD)
                },
            },
            Row {
                says: "a told notice dismissed goes, and only it",
                when: |s, e| matches!(e, Event::Command(Command::Dismiss(id)) if told(s).iter().any(|t| t.id == *id)),
                then: |s, e, out| {
                    let Event::Command(Command::Dismiss(id)) = e else { return false };
                    out.as_ref().is_ok_and(|o| o.entries.iter().map(|x| x.id).eq(s.entries.iter().map(|x| x.id).filter(|x| x != id)))
                },
            },
            Row {
                says: "a live notice isn't dismissed, nor one not there",
                when: |s, e| matches!(e, Event::Command(Command::Dismiss(id)) if !told(s).iter().any(|t| t.id == *id)),
                then: |s, e, out| {
                    let Event::Command(Command::Dismiss(id)) = e else { return false };
                    *out == Err(if s.entries.iter().any(|x| x.id == *id) { Refusal::Live(*id) } else { Refusal::NoSuchNotice(*id) })
                },
            },
            Row {
                says: "a click opens the popover when there's something to open, and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && s.openable(),
                then: |s, _, out| out.as_ref().is_ok_and(|o| o.open && o.entries == s.entries),
            },
            Row {
                says: "with nothing to open, a click dismisses the only notice if it's told; a live one stays, and with none there's nothing to do",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && !s.openable(),
                then: |s, _, out| match s.entries.as_slice() {
                    [only] if only.source == Source::Told => out.as_ref().is_ok_and(|o| o.entries.is_empty() && !o.open),
                    [only] => *out == Err(Refusal::Live(only.id)),
                    _ => *out == Err(Refusal::NothingToOpen),
                },
            },
            Row {
                says: "Escape or a click outside closes the open popover and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Close)) && s.open,
                then: |s, _, out| out.as_ref().is_ok_and(|o| !o.open && o.entries == s.entries),
            },
            Row {
                says: "with the popover closed, Escape isn't the bar's: refused, so it goes on to the page",
                when: |s, e| matches!(e, Event::Command(Command::Close)) && !s.open,
                then: |_, _, out| *out == Err(Refusal::NotOpen),
            },
            Row {
                says: "the live notices become those that hold, one each, saying what they say now, a kept one in its place; told ones stay",
                when: |_, e| matches!(e, Event::Observed(_)),
                then: |s, e, out| {
                    let (Event::Observed(Observed::Live(live)), Ok(out)) = (e, out) else { return false };
                    let keys: Vec<&String> =
                        out.entries.iter().filter_map(|x| if let Source::Live(k) = &x.source { Some(k) } else { None }).collect();
                    let kept_ids = s.entries.iter().filter(|x| matches!(&x.source, Source::Live(k) if live.iter().any(|(l, _)| l == k)));
                    let says_now = live
                        .iter()
                        .all(|(k, n)| out.entries.iter().any(|x| x.source == Source::Live(k.clone()) && x.notice.short == n.short));
                    keys.len() == live.len()
                        && live.iter().all(|(k, _)| keys.contains(&k))
                        && says_now
                        && told(out) == told(s)
                        && kept_ids.into_iter().all(|k| out.entries.iter().any(|x| x.id == k.id))
                },
            },
        ]
    }

    fn always() -> Vec<DecideRow<Notices>> {
        vec![
            Row::always("the popover is open only while there's something to open", |_, _, out| {
                out.as_ref().is_ok_and(|o| !o.open || o.openable()) || out.is_err()
            }),
            Row::always("ids are never reused", |s, _, out| {
                out.as_ref().is_ok_and(|o| o.entries.iter().all(|x| s.entries.iter().any(|y| y.id == x.id) || x.id >= s.next_id))
                    || out.is_err()
            }),
        ]
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<Notices>(&rows(), &always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        sequences::<Notices>(State::every(), 16, &rows(), &always());
    }
}
