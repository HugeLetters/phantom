//! The status bar's notices as a machine (#120): which notices it holds, and which one its
//! popover shows, if it's open. Pure; [`super::Bar`] carries it out.
//!
//! A notice is **told**: something happened (an add, the settings reset, a failure) and it stays
//! until it's dismissed. The bar shows the newest, with how many there are; a click opens the
//! popover, which shows **one notice at a time**, newest first, and steps through them: ‹ to a
//! newer one, › to an older one, stopping at the ends (no wrap: the count says where you are, and
//! an end is where the list ends). Opening shows the newest.
//!
//! | State \ Event | Tell(n) | Dismiss(id) | DismissAll | Left(list) | Press | Close | Step(way) |
//! |---|---|---|---|---|---|---|---|
//! | any | appended (past [`MAX_TOLD`] the oldest one goes, an error like any other); the popover opens or moves for none | it goes; none: refused | every one goes, the popover closes; none: refused | an entry of `list` undone: each notice that tells of it loses it from what it tells of, and goes when that was the last of them (an undone action's notice, #200); one absorbed by a newer entry: the notices telling of it tell of that one instead; the others (refused, aged, dropped): nothing changes. Nothing changes at all: refused | something to open: open at the newest (open already: as it was). Else the only notice: it goes. Else refused | open: closed. Closed: refused, so Escape goes on to the page | open and one lies that way: it's shown. Else refused |
//!
//! **An undone action's notices leave** (#200): a notice that tells of an action on the undo stack
//! (`Notice::of`: a Remove's, a Delete's, its partly failed one's) goes once every action it tells
//! of is undone, whoever undid it. The "is back" notice, which tells of none, stays, and so do the
//! notices of other actions. When the shown one goes, the popover moves on as for any other.
//!
//! **Undo is optimistic** (#200): `Undoing(seqs)`, an Undo pressed, takes the notices of those
//! actions out of the list at once (`hidden`, with the entries `pending`); the service's word that
//! they were undone ends them, that it was refused brings them back in their place. A notice of
//! several actions is out only while every one is under way.
//!
//! There's something to open while there are several notices, or one with more to read.
//!
//! **Errors** are notices of severity Error: a failure that has no better place (a refused add in
//! a batch, a failed command, Open…), told like any other, a batch as one with its list as its
//! view, shown in the bar in the error colour. **An error never opens the popover**: it shows in
//! the bar with the count, and a click opens it, as for any notice. Notices, errors included, are
//! kept up to [`MAX_TOLD`], the oldest dropped first. Dismiss all clears every one at once.
//!
//! **What's shown follows the notice, not a place in the list**: the shown notice is kept by its
//! id. So a new notice, or any other one going, never moves what you're looking at. When the shown
//! one itself goes (its ×, an undone action's, the oldest dropped) the next older one is shown,
//! or, with none older, the nearest newer; with none left, the popover closes. While open, the
//! popover stays open as long as there's a notice.
//!
//! Escape goes to the popover first: while it's open, Escape closes it and does nothing else (the
//! selection stays), and only the next Escape reaches the page.
//!
//! # Conditions have no place here
//!
//! A condition (a disk short of space, a drive that isn't connected) isn't an event: it holds
//! while something is the case. One with a natural place in the window is shown there, from the
//! latest update, while it holds (a torrent row's ⚠ and its hover, the Settings row): nothing to
//! dismiss, because it isn't in the way. This list is for what has no such place.
//!
//! If a condition with no place of its own ever appears, this is how to show it here (it was built
//! once for disks short of space, in #209, and taken out when the rows turned out to be its place):
//!
//! - **Derive** conditions from each update, never tell them: the machine takes the list that
//!   holds now, each with a **key** (which condition: a disk's mount, a drive's name) and an
//!   **extent** (what it affects: the torrents on that disk). One that isn't in the list is gone;
//!   a kept one keeps its place and says what it says now.
//! - A **dismissal** stores the key and the extent seen, and takes the notice out of the list. A
//!   condition with that key stays hidden while it holds, and while it affects no more than it
//!   did.
//! - A dismissed condition **shows again**, as a new notice, when
//!   1. it **ended and came back**: a dismissal is forgotten the moment its key isn't in the list;
//!   2. it **got worse**: its extent has something the stored one hasn't (another torrent doesn't
//!      fit); the stored extent doesn't shrink, so one that left and came back isn't worse;
//!   3. it was **touched**: the user did something on the disk or drive it's about (an add into
//!      it, a Resume, a file set back to Normal), which names the keys it touches, and only those
//!      come back, once, if they still hold. A touch elsewhere changes nothing.
//! - What a dismissed notice does to the popover is what any notice going does. Dismiss all
//!   hides the conditions as it clears the rest.

use std::collections::BTreeSet;

use crate::notice::Notice;
use proto::{Leave, Left};

pub type NoticeId = u64;

/// The most told notices kept, errors or not: older ones go as new ones come, so a stream of
/// failures stays bounded and ‹ › short.
pub const MAX_TOLD: usize = 50;

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: NoticeId,
    pub notice: Notice,
}

/// Two entries are the same if their ids and what they say are: a notice's view is a closure,
/// which can't be compared.
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.notice.short, self.notice.severity, self.notice.more.is_some(), &self.notice.of)
            == (other.id, &other.notice.short, other.notice.severity, other.notice.more.is_some(), &other.notice.of)
    }
}

/// The notices, oldest first, and which one the popover shows while it's open. Ids are never
/// reused. `current` is a notice in `entries` while open, and `None` while closed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub entries: Vec<Entry>,
    /// Notices whose actions' Undo is under way: out of the list from the moment it's pressed (#200),
    /// and back, with their `of` as it was, if the undo is refused.
    pub hidden: Vec<Entry>,
    /// The undo-stack entries (by seq) an Undo was pressed for, that a notice tells of, until they're
    /// told undone or refused.
    pub pending: BTreeSet<u64>,
    pub current: Option<NoticeId>,
    next_id: NoticeId,
}

impl State {
    /// How many notices there are in the list: what Dismiss all takes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the popover is open.
    pub fn open(&self) -> bool {
        self.current.is_some()
    }

    /// Whether a click has something to open: several notices, or one with more to read.
    pub fn openable(&self) -> bool {
        self.entries.len() > 1 || self.entries.iter().any(|e| e.notice.more.is_some())
    }

    /// The newest notice, which the bar shows.
    pub fn newest(&self) -> Option<&Entry> {
        self.entries.last()
    }

    /// The notice the popover shows.
    pub fn shown(&self) -> Option<&Entry> {
        let id = self.current?;
        self.entries.iter().find(|e| e.id == id)
    }

    /// Where the shown notice is among them, newest first: 0 is the newest.
    pub fn index(&self) -> Option<usize> {
        let id = self.current?;
        self.entries.iter().rev().position(|e| e.id == id)
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Command(Command),
    Observed(Observed),
}

/// Which way to step through the notices, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// ‹ and ←: toward the newest.
    Newer,
    /// › and →: toward the oldest.
    Older,
}

#[derive(Debug, Clone)]
pub enum Command {
    Tell(Notice),
    /// A notice's ×.
    Dismiss(NoticeId),
    /// Dismiss all: every notice, in the popover.
    DismissAll,
    /// A click on the bar: opens the popover, or dismisses its only notice if that has nothing
    /// more to read.
    Press,
    /// Escape, or a click outside the popover.
    Close,
    /// ‹, ›, ← or →.
    Step(Way),
    /// An Undo was pressed (⌘Z, Edit ▸ Undo, or a notice's own) for these undo-stack entries (by
    /// seq): the notices of those actions leave at once (#200), and come back if it's refused.
    Undoing(Vec<u64>),
}

#[derive(Debug, Clone)]
pub enum Observed {
    /// These undo-stack entries left it, and why, as the service says: undone by anyone, or taken
    /// by a newer entry. Refused, aged and dropped ones change no notice.
    Left(Vec<Left>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Dismissed already, or never there: a click that raced another.
    NoSuchNotice(NoticeId),
    NothingToOpen,
    /// Dismiss all with no notice.
    NothingToDismiss,
    /// An `Undoing` or a `Left` that changes nothing: no notice tells of an entry it names.
    NotTold,
    /// Escape or a step with the popover closed: it's not the bar's.
    NotOpen,
    /// Stepping past the newest or the oldest: it stops there.
    NoFurther(Way),
}

pub fn decide(state: &State, event: &Event) -> Result<State, Refusal> {
    let mut next = state.clone();
    match event {
        Event::Command(Command::Tell(notice)) => {
            next.entries.push(Entry { id: next.next_id, notice: notice.clone() });
            next.next_id += 1;
            if next.entries.len() > MAX_TOLD {
                next.entries.remove(0);
            }
        }
        Event::Command(Command::Dismiss(id)) => match next.entries.iter().position(|e| e.id == *id) {
            Some(at) => {
                next.entries.remove(at);
            }
            None => return Err(Refusal::NoSuchNotice(*id)),
        },
        Event::Command(Command::DismissAll) => {
            if state.entries.is_empty() {
                return Err(Refusal::NothingToDismiss);
            }
            next.entries.clear();
            next.current = None;
        }
        Event::Command(Command::Press) if state.openable() => {
            next.current = state.current.or(state.newest().map(|e| e.id));
        }
        Event::Command(Command::Press) => match state.entries.as_slice() {
            [only] => return decide(state, &Event::Command(Command::Dismiss(only.id))),
            [] | [_, _, ..] => return Err(Refusal::NothingToOpen),
        },
        Event::Command(Command::Close) if state.open() => next.current = None,
        Event::Command(Command::Close) => return Err(Refusal::NotOpen),
        Event::Command(Command::Step(way)) => {
            let at = state.index().ok_or(Refusal::NotOpen)?;
            let to = match way {
                Way::Newer => at.checked_sub(1),
                Way::Older => Some(at + 1).filter(|to| *to < state.entries.len()),
            }
            .ok_or(Refusal::NoFurther(*way))?;
            next.current = Some(state.entries[state.entries.len() - 1 - to].id);
        }
        Event::Command(Command::Undoing(seqs)) => {
            let told = |q: &u64| state.entries.iter().chain(&state.hidden).any(|e| e.notice.of.contains(q));
            next.pending.extend(seqs.iter().filter(|q| told(q)));
            if next.pending == state.pending {
                return Err(Refusal::NotTold);
            }
        }
        Event::Observed(Observed::Left(list)) => {
            let undone = |q: &u64| list.iter().any(|l| l.seq == *q && matches!(l.why, Leave::Undone { .. }));
            let refused = |q: &u64| list.iter().any(|l| l.seq == *q && matches!(l.why, Leave::Refused { .. }));
            let taken_by = |q: &u64| {
                list.iter().find_map(|l| if l.seq == *q { if let Leave::Absorbed { by } = l.why { Some(by) } else { None } } else { None })
            };
            let refit = |e: &Entry| {
                // What it tells of, once what's undone is off and what was absorbed is its absorber.
                let mut of: Vec<u64> = Vec::new();
                for q in &e.notice.of {
                    let q = taken_by(q).unwrap_or(*q);
                    if !undone(&q) && !of.contains(&q) {
                        of.push(q);
                    }
                }
                let tells_of_some = !e.notice.of.is_empty();
                (!tells_of_some || !of.is_empty()).then(|| Entry { notice: Notice { of, ..e.notice.clone() }, ..e.clone() })
            };
            next.entries = state.entries.iter().filter_map(refit).collect();
            next.hidden = state.hidden.iter().filter_map(refit).collect();
            // What's undone or refused is no longer under way; what was absorbed follows its absorber.
            next.pending = state.pending.iter().filter(|q| !refused(q)).map(|q| taken_by(q).unwrap_or(*q)).filter(|q| !undone(q)).collect();
        }
    }
    settle(&mut next);
    if matches!(event, Event::Observed(Observed::Left(_)))
        && next.entries == state.entries
        && next.hidden == state.hidden
        && next.pending == state.pending
    {
        return Err(Refusal::NotTold);
    }
    next.current = next.current.and_then(|shown| survivor(&state.entries, shown, &next.entries));
    Ok(next)
}

/// Whether a notice is out of the list: it tells of actions, and an Undo is under way for every one.
fn hidden(entry: &Entry, pending: &BTreeSet<u64>) -> bool {
    !entry.notice.of.is_empty() && entry.notice.of.iter().all(|q| pending.contains(q))
}

/// Puts each notice where it belongs: the list, or out of it while its undo is under way (back in its
/// place, by id, when that's refused); and the list within [`MAX_TOLD`].
fn settle(next: &mut State) {
    let mut all: Vec<Entry> = next.entries.drain(..).chain(next.hidden.drain(..)).collect();
    all.sort_by_key(|e| e.id);
    (next.hidden, next.entries) = all.into_iter().partition(|e| hidden(e, &next.pending));
    while next.entries.len() > MAX_TOLD {
        next.entries.remove(0);
    }
}

/// What the popover shows once `entries` are the notices, having shown `shown` among `before`:
/// the same notice while it's there; else the next older one that is, else the nearest newer.
fn survivor(before: &[Entry], shown: NoticeId, entries: &[Entry]) -> Option<NoticeId> {
    let there = |id: &NoticeId| entries.iter().any(|e| e.id == *id);
    if there(&shown) {
        return Some(shown);
    }
    let at = before.iter().position(|e| e.id == shown)?;
    before[..at].iter().rev().chain(&before[at + 1..]).map(|e| e.id).find(there)
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
    use crate::notice::Severity;

    fn plain(short: &str) -> Notice {
        Notice::warning(short)
    }

    fn with_more(short: &str) -> Notice {
        Notice { more: Some(Rc::new(|_, _| rune::el())), ..Notice::warning(short) }
    }

    fn failed(short: &str) -> Notice {
        Notice::error(short)
    }

    /// `n`, telling of the undo-stack entries `seqs`.
    fn of(n: Notice, seqs: &[u64]) -> Notice {
        Notice { of: seqs.to_vec(), ..n }
    }

    fn failed_with_list(short: &str) -> Notice {
        Notice { more: Some(Rc::new(|_, _| rune::el())), ..Notice::error(short) }
    }

    /// `shown`: the position of the notice the popover shows, newest first (0 is the newest).
    fn state(notices: Vec<Notice>, shown: Option<usize>) -> State {
        let entries: Vec<Entry> = notices.into_iter().enumerate().map(|(i, notice)| Entry { id: i as u64, notice }).collect();
        let next_id = entries.len() as u64;
        let current = shown.map(|at| entries[entries.len() - 1 - at].id);
        State { entries, hidden: Vec::new(), pending: BTreeSet::new(), current, next_id }
    }

    every!(State {
        State { .. } => {
            let told = |s: &str| plain(s);
            let told_more = |s: &str| with_more(s);
            let failed = |s: &str| failed_with_list(s);
            // As many as are kept, errors among them: telling one more drops the oldest, an error
            // or not.
            let many: Vec<_> =
                (0..MAX_TOLD).map(|i| if i % 3 == 0 { failed(&format!("e{i}")) } else { told(&format!("t{i}")) }).collect();
            let many_errors: Vec<_> = (0..MAX_TOLD).map(|i| failed(&format!("e{i}"))).collect();
            let acted = |s: &str, seqs: &[u64]| of(with_more(s), seqs);
            // Notices of actions beside the "is back" notice they gave way to, a partly failed
            // Delete's error with the same entry: as an undo meets them.
            let many_acted: Vec<_> = (0..MAX_TOLD)
                .map(|i| if i % 4 == 0 { acted(&format!("r{i}"), &[i as u64, i as u64 + 1]) } else { told(&format!("t{i}")) })
                .collect();
            let shapes = vec![
                vec![acted("Removed a", &[1]), told("b is back")],
                vec![acted("Removed a", &[1, 2]), of(failed_with_list("Couldn't delete b"), &[2]), acted("Removed c", &[3]), told("c is back")],
                vec![acted("Removed a", &[1]), acted("Deleted b", &[2]), acted("Removed c", &[3])],
                many_acted,
                vec![],
                vec![told("a")],
                vec![told_more("a")],
                vec![told("a"), told("b")],
                vec![failed("e")],
                vec![told("a"), failed("e"), told_more("c")],
                vec![failed("e"), failed("f")],
                many_errors,
                many,
            ];
            // Each, and with an Undo pressed for some of what it tells of: those notices out of the list.
            shapes
                .into_iter()
                .flat_map(|e| {
                    let shown = std::iter::once(None).chain((0..e.len()).map(Some)).collect::<Vec<_>>();
                    shown.into_iter().flat_map(move |shown| {
                        let plain = state(e.clone(), shown);
                        let pressed = [vec![1], vec![1, 2], vec![3]].into_iter().filter_map(|seqs| {
                            decide(&plain, &Event::Command(Command::Undoing(seqs))).ok()
                        });
                        std::iter::once(plain.clone()).chain(pressed.collect::<Vec<_>>())
                    }).collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        },
    });

    every!(Event {
        Event::Command(Command::Tell(_)) => {
            [plain("new"), with_more("new"), failed("new"), failed_with_list("new")].map(|n| Event::Command(Command::Tell(n)))
        },
        Event::Command(Command::Dismiss(_)) => (0..MAX_TOLD as u64 + 1).map(|id| Event::Command(Command::Dismiss(id))),
        Event::Command(Command::DismissAll) => [Event::Command(Command::DismissAll)],
        Event::Command(Command::Press) => [Event::Command(Command::Press)],
        Event::Command(Command::Close) => [Event::Command(Command::Close)],
        Event::Command(Command::Step(_)) => [Way::Newer, Way::Older].map(|w| Event::Command(Command::Step(w))),
        Event::Command(Command::Undoing(_)) => [vec![], vec![1], vec![2], vec![1, 2], vec![3], vec![0, 1, 2, 3], vec![99]]
            .map(|seqs| Event::Command(Command::Undoing(seqs))),
        Event::Observed(Observed::Left(_)) => {
            let l = |seq: u64, why: Leave| Left { seq, why };
            let refused = Leave::Refused { why: proto::CantUndo::NotKept, stranded: Vec::new() };
            [
                vec![],
                vec![l(1, Leave::Undone { refused: Vec::new() })],
                vec![l(2, Leave::Undone { refused: Vec::new() })],
                vec![l(1, Leave::Undone { refused: Vec::new() }), l(2, Leave::Undone { refused: Vec::new() })],
                vec![l(3, Leave::Undone { refused: Vec::new() })],
                vec![l(1, Leave::Absorbed { by: 3 })],
                vec![l(2, Leave::Absorbed { by: 1 })],
                vec![l(1, Leave::Absorbed { by: 3 }), l(3, Leave::Undone { refused: Vec::new() })],
                vec![l(1, refused.clone())],
                vec![l(1, Leave::Aged), l(2, Leave::Dropped)],
                vec![l(2, refused), l(1, Leave::Aged), l(3, Leave::Dropped)],
                (0..5).map(|n| l(n, Leave::Undone { refused: Vec::new() })).collect(),
                vec![l(99, Leave::Undone { refused: Vec::new() })],
            ]
            .map(|list| Event::Observed(Observed::Left(list)))
        },
    });

    fn ids(s: &State) -> Vec<NoticeId> {
        s.entries.iter().map(|e| e.id).collect()
    }

    /// The popover's step from `s`: where it lands, if there's a notice that way.
    fn lands(s: &State, way: Way) -> Option<NoticeId> {
        let at = s.index()?;
        let to = match way {
            Way::Newer => at.checked_sub(1)?,
            Way::Older => at + 1,
        };
        s.entries.iter().rev().nth(to).map(|e| e.id)
    }

    /// What telling a notice does, of any severity: it's the newest, the notices are the old ones
    /// and it, the last [`MAX_TOLD`], and the popover is as it was,
    /// closed or open on what it showed (the next one, if that was the oldest and dropped).
    fn told_as_it_should(s: &State, e: &Event, out: &Result<State, Refusal>) -> bool {
        let (Event::Command(Command::Tell(n)), Ok(out)) = (e, out) else { return false };
        let newest = out.newest().unwrap();
        let mut now = ids(s);
        now.push(newest.id);
        if now.len() > MAX_TOLD {
            now.remove(0);
        }
        let shown_survives = s.current.is_none_or(|id| out.entries.iter().any(|x| x.id == id));
        newest.notice.short == n.short && ids(out) == now && s.open() == out.open() && (!shown_survives || out.current == s.current)
    }

    /// What the notices are, as ids with what each tells of: those in the list, those out of it while
    /// their undo is under way, and the entries under way.
    type View = (Vec<(NoticeId, Vec<u64>)>, Vec<(NoticeId, Vec<u64>)>, BTreeSet<u64>);

    fn view(s: &State) -> View {
        let of = |v: &[Entry]| v.iter().map(|x| (x.id, x.notice.of.clone())).collect::<Vec<_>>();
        (of(&s.entries), of(&s.hidden), s.pending.clone())
    }

    /// What an Undoing or a Left should make of `s`, worked out the long way round: the notices and
    /// the entries under way are changed first, then each notice is in the list unless it tells of
    /// actions and an undo is under way for every one.
    fn reference(s: &State, e: &Event) -> Option<View> {
        let mut pool: Vec<(NoticeId, Vec<u64>)> = s.entries.iter().chain(&s.hidden).map(|x| (x.id, x.notice.of.clone())).collect();
        pool.sort_by_key(|(id, _)| *id);
        let mut pending = s.pending.clone();
        match e {
            Event::Command(Command::Undoing(seqs)) => {
                pending.extend(seqs.iter().filter(|q| pool.iter().any(|(_, of)| of.contains(q))));
            }
            Event::Observed(Observed::Left(list)) => {
                let says = |q: u64, f: fn(&Leave) -> bool| list.iter().any(|l| l.seq == q && f(&l.why));
                let absorber = |q: u64| {
                    list.iter().find_map(|l| match l.why {
                        Leave::Absorbed { by } if l.seq == q => Some(by),
                        _ => None,
                    })
                };
                let undone = |q: u64| says(q, |w| matches!(w, Leave::Undone { .. }));
                let refused = |q: u64| says(q, |w| matches!(w, Leave::Refused { .. }));
                pool = pool
                    .into_iter()
                    .filter_map(|(id, of)| {
                        let mut now: Vec<u64> = Vec::new();
                        for q in &of {
                            let q = absorber(*q).unwrap_or(*q);
                            if !undone(q) && !now.contains(&q) {
                                now.push(q);
                            }
                        }
                        (of.is_empty() || !now.is_empty()).then_some((id, now))
                    })
                    .collect();
                pending = pending.iter().filter(|q| !refused(**q)).map(|q| absorber(*q).unwrap_or(*q)).filter(|q| !undone(*q)).collect();
            }
            _ => return None,
        }
        let out = |hide: bool, pool: &[(NoticeId, Vec<u64>)]| {
            pool.iter()
                .filter(|(_, of)| (!of.is_empty() && of.iter().all(|q| pending.contains(q))) == hide)
                .map(|(id, of)| (*id, of.clone()))
                .collect::<Vec<_>>()
        };
        let (mut shown, hidden) = (out(false, &pool), out(true, &pool));
        while shown.len() > MAX_TOLD {
            shown.remove(0);
        }
        Some((shown, hidden, pending))
    }

    fn rows() -> Vec<DecideRow<Notices>> {
        vec![
            Row {
                says: "an Info or a Warning told is the newest of the told, the oldest told dropped past the most kept; it never opens the popover",
                when: |_, e| matches!(e, Event::Command(Command::Tell(n)) if n.severity != Severity::Error),
                then: told_as_it_should,
            },
            Row {
                says: "an error told is the same: the newest of the told, the oldest told (an error or not) dropped past the most kept; it never opens the popover",
                when: |_, e| matches!(e, Event::Command(Command::Tell(n)) if n.severity == Severity::Error),
                then: told_as_it_should,
            },
            Row {
                says: "a notice dismissed goes, and only it",
                when: |s, e| matches!(e, Event::Command(Command::Dismiss(id)) if s.entries.iter().any(|t| t.id == *id)),
                then: |s, e, out| {
                    let Event::Command(Command::Dismiss(id)) = e else { return false };
                    out.as_ref().is_ok_and(|o| o.entries.iter().map(|x| x.id).eq(s.entries.iter().map(|x| x.id).filter(|x| x != id)))
                },
            },
            Row {
                says: "dismissing all takes every notice and closes the popover; with none it's refused",
                when: |_, e| matches!(e, Event::Command(Command::DismissAll)),
                then: |s, _, out| match out {
                    Ok(o) => !s.entries.is_empty() && o.entries.is_empty() && !o.open(),
                    Err(refusal) => s.entries.is_empty() && *refusal == Refusal::NothingToDismiss,
                },
            },
            Row {
                says: "one not there: refused",
                when: |s, e| matches!(e, Event::Command(Command::Dismiss(id)) if !s.entries.iter().any(|t| t.id == *id)),
                then: |_, e, out| {
                    let Event::Command(Command::Dismiss(id)) = e else { return false };
                    *out == Err(Refusal::NoSuchNotice(*id))
                },
            },
            Row {
                says: "a click opens the popover at the newest (as it was, if open) when there's something to open, and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && s.openable(),
                then: |s, _, out| out.as_ref().is_ok_and(|o| o.entries == s.entries && o.current == s.current.or(s.newest().map(|e| e.id))),
            },
            Row {
                says: "with nothing to open, a click dismisses the only notice; with none there's nothing to do",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && !s.openable(),
                then: |s, _, out| match s.entries.as_slice() {
                    [_] => out.as_ref().is_ok_and(|o| o.entries.is_empty() && !o.open()),
                    [] | [_, _, ..] => *out == Err(Refusal::NothingToOpen),
                },
            },
            Row {
                says: "Escape or a click outside closes the open popover and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Close)) && s.open(),
                then: |s, _, out| out.as_ref().is_ok_and(|o| !o.open() && o.entries == s.entries),
            },
            Row {
                says: "with the popover closed, Escape isn't the bar's: refused, so it goes on to the page",
                when: |s, e| matches!(e, Event::Command(Command::Close)) && !s.open(),
                then: |_, _, out| *out == Err(Refusal::NotOpen),
            },
            Row {
                says: "a step shows the next notice that way, and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Step(way)) if lands(s, *way).is_some()),
                then: |s, e, out| {
                    let Event::Command(Command::Step(way)) = e else { return false };
                    out.as_ref().is_ok_and(|o| o.entries == s.entries && o.current == lands(s, *way))
                },
            },
            Row {
                says: "a step past the newest or the oldest stops there: refused",
                when: |s, e| matches!(e, Event::Command(Command::Step(way)) if s.open() && lands(s, *way).is_none()),
                then: |_, e, out| {
                    let Event::Command(Command::Step(way)) = e else { return false };
                    *out == Err(Refusal::NoFurther(*way))
                },
            },
            Row {
                says: "with the popover closed, a step isn't the bar's: refused",
                when: |s, e| matches!(e, Event::Command(Command::Step(_))) && !s.open(),
                then: |_, _, out| *out == Err(Refusal::NotOpen),
            },
            Row {
                says: "an Undo pressed takes the notices of those actions out of the list at once, a notice whose every action is under way (a notice of several keeps showing while part of it isn't); a press that no notice tells of is refused",
                when: |_, e| matches!(e, Event::Command(Command::Undoing(_))),
                then: |s, e, out| match (reference(s, e), out) {
                    (Some(want), Ok(o)) => want != view(s) && view(o) == want,
                    (Some(want), Err(r)) => want == view(s) && *r == Refusal::NotTold,
                    (None, _) => false,
                },
            },
            Row {
                says: "an entry undone goes from what each notice tells of and from what's under way (a notice goes when it was the last it told of, from the list or from out of it); one absorbed is replaced there by what took its place; one refused is no longer under way, so its notice is back in its place; aged and dropped ones change nothing; nothing changed: refused",
                when: |_, e| matches!(e, Event::Observed(Observed::Left(_))),
                then: |s, e, out| match (reference(s, e), out) {
                    (Some(want), Ok(o)) => want != view(s) && view(o) == want,
                    (Some(want), Err(r)) => want == view(s) && *r == Refusal::NotTold,
                    (None, _) => false,
                },
            },
        ]
    }

    fn always() -> Vec<DecideRow<Notices>> {
        vec![
            Row::always(
                "a notice that tells of none is never touched by an Undo or an entry that left: it stays, saying the same",
                |s, e, out| {
                    let (Event::Observed(Observed::Left(_)) | Event::Command(Command::Undoing(_)), Ok(o)) = (e, out) else { return true };
                    s.entries.iter().filter(|x| x.notice.of.is_empty()).all(|x| o.entries.iter().any(|y| y == x))
                },
            ),
            Row::always("a notice is out of the list exactly while an undo is under way for every action it tells of", |_, _, out| {
                out.as_ref()
                    .map_or(true, |o| o.entries.iter().all(|x| !hidden(x, &o.pending)) && o.hidden.iter().all(|x| hidden(x, &o.pending)))
            }),
            Row::always("entries refused, aged or dropped never lose a notice: those out of the list come back, none goes", |s, e, out| {
                let (Event::Observed(Observed::Left(list)), Ok(o)) = (e, out) else { return true };
                let keeps = list.iter().all(|l| !matches!(l.why, Leave::Undone { .. } | Leave::Absorbed { .. }));
                !keeps
                    || s.entries
                        .iter()
                        .chain(&s.hidden)
                        .all(|x| o.entries.iter().chain(&o.hidden).any(|y| y.id == x.id) || s.entries.len() >= MAX_TOLD)
            }),
            Row::always("the popover shows a notice that's there, and is open only while there is one", |_, _, out| {
                out.as_ref().map_or(true, |o| o.current.is_none() || (o.shown().is_some() && o.index().is_some()))
            }),
            Row::always("no notice told, an error included, opens the popover", |s, e, out| {
                !matches!(e, Event::Command(Command::Tell(_))) || s.open() || out.as_ref().is_ok_and(|o| !o.open())
            }),
            Row::always("ids are never reused", |s, _, out| {
                out.as_ref().map_or(true, |o| {
                    o.entries.iter().all(|x| s.entries.iter().chain(&s.hidden).any(|y| y.id == x.id) || x.id >= s.next_id)
                })
            }),
            Row::always("what's shown stays shown while it's there, whatever comes or goes around it", |s, e, out| {
                let (Some(shown), Ok(o)) = (s.current, out) else { return true };
                // (Dismiss all closes it.)
                let around = matches!(e, Event::Command(Command::Tell(_) | Command::Dismiss(_) | Command::Press) | Event::Observed(_));
                !(around && o.entries.iter().any(|x| x.id == shown)) || o.current == Some(shown)
            }),
            Row::always("when what's shown goes, the next older one is shown, else the nearest newer; none left closes", |s, e, out| {
                let (Some(shown), Ok(o)) = (s.current, out) else { return true };
                // (Dismiss all closes it, whatever stays.)
                if o.entries.iter().any(|x| x.id == shown) || matches!(e, Event::Command(Command::DismissAll)) {
                    return true;
                }
                let pos = |id: NoticeId| s.entries.iter().position(|x| x.id == id).unwrap();
                let survivors: Vec<usize> =
                    s.entries.iter().enumerate().filter(|(_, x)| o.entries.iter().any(|y| y.id == x.id)).map(|(i, _)| i).collect();
                let gone = pos(shown);
                let Some(to) = o.current else { return survivors.is_empty() };
                let to = pos(to);
                // Nothing that's left lies between the notice that went and the one now shown, and
                // an older one is chosen over a newer one.
                let between = survivors.iter().any(|i| (gone.min(to) + 1..gone.max(to)).contains(i));
                let older_left = survivors.iter().any(|i| *i < gone);
                !between && (to < gone) == older_left
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
