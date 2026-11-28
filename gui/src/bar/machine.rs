//! The status bar's notices as a machine (#120): which notices it holds, and which one its
//! popover shows, if it's open. Pure; [`super::Bar`] carries it out.
//!
//! A notice is **told**: something happened (an add, the settings reset, a failure) and it stays
//! until it's dismissed. The bar shows the newest, in its own color, and a pill with how many
//! there are, tinted by the worst of them ([`State::pill`]); a click always opens the
//! popover, which shows **one notice at a time**, newest first, and steps through them: ‹ to a
//! newer one, › to an older one, stopping at the ends (no wrap: the count says where you are, and
//! an end is where the list ends). Opening shows the newest. A click never dismisses: only ×
//! (and Dismiss all) do (#87).
//!
//! | State \ Event | Tell(n) | Dismiss(id) | DismissAll | Left(list) | Press | Close | Step(way) | Passed(now) |
//! |---|---|---|---|---|---|---|---|---|
//! | any | appended (past [`MAX_TOLD`] the oldest one goes, an error like any other); one whose every action has an Undo under way goes out of the list at once and costs the oldest nothing (#200); the popover opens or moves for none | it goes; none: refused | every one goes, the popover closes; none: refused | an entry of `list` undone: each notice that tells of it loses it from what it tells of, and goes when that was the last of them (an undone action's notice, #200); one absorbed by a newer entry: the notices telling of it tell of that one instead; one refused is no longer under way, so a notice out of the list for it is back in its place (past [`MAX_TOLD`] the oldest one goes, never one just back, #349, #474); the others (aged, dropped): nothing changes. Nothing changes at all: refused | a notice: open at the newest (open already: as it was), never a dismissal. None: refused | open: closed. Closed: refused, so Escape goes on to the page | open and one lies that way: it's shown. Else refused | every timed notice whose clock ran out goes, as a dismissal; a clock waiting to start starts, `now` + its length ([`BRIEF_SECS`](crate::notice::BRIEF_SECS) for a receipt, [`LONG_SECS`](crate::notice::LONG_SECS) for an action's notice with its Undo), while the popover is closed. Nothing changed: refused |
//!
//! Opening the popover stops every clock; **Close** (or anything else that closes it) makes every timed notice's clock wait to start again, so each gets its full time again.
//!
//! **An undone action's notices leave** (#200): a notice that tells of an action on the undo stack
//! (`Notice::of`: a Remove's, a Delete's, its partly failed one's) goes once every action it tells
//! of is undone, whoever undid it. The "is back" notice, which tells of none, stays, and so do the
//! notices of other actions. When the shown one goes, the popover moves on as for any other.
//!
//! **Undo is optimistic** (#200): `Undoing(seqs)`, an Undo pressed, takes the notices of those
//! actions out of the list at once (`hidden`, with the entries `pending`); the service's word that
//! they were undone ends them, that it was refused brings them back in their place. Out of the list,
//! a notice costs nobody their place (#299); back in it, it counts, so on a full list the oldest
//! goes as for any notice told (#349), but never one just back (#474). A notice of
//! several actions is out only while every one is under way.
//!
//! **A timed notice** (`Notice::lasts` is `Brief` or `Long`) is told like any other and leaves by itself:
//! its length (5 s, or 60 s for `Long`) after the popover is closed, whether or not it's the one the bar shows. Its
//! clock runs only while the popover is closed: opening stops it, and closing gives it a fresh
//! full time. One told while the popover is open starts no clock until it closes. The machine
//! never reads a clock: the app tells it the time with `Passed`, and a clock starts at the first
//! time it's told after the notice is told or the popover closed (a frame late at most). The
//! notice then goes as a dismissal does.
//!
//! **Errors** are notices of severity Error: a failure that has no better place (a refused add in
//! a batch, a failed command, Open…), told like any other, a batch as one with its list as its
//! view, shown in the bar in the error color. **An error never opens the popover**: it shows in
//! the bar, the pill red, and a click opens it, as for any notice. Notices, errors included, are
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

use crate::notice::{Lasts, Notice, Severity};
use proto::{Leave, Left};

pub type NoticeId = u64;

/// The most told notices kept, errors or not: older ones go as new ones come, so a stream of
/// failures stays bounded and ‹ › short.
pub const MAX_TOLD: usize = 50;

/// A timed notice's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clock {
    /// Not running: the notice isn't timed, or the popover is open.
    Stopped,
    /// Due to run, waiting for the next `Passed` to say when it starts.
    Starting,
    /// Running: the notice goes at this time.
    Until(f64),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: NoticeId,
    pub notice: Notice,
    pub clock: Clock,
}

/// Two entries are the same if their ids and what they say are: a notice's buttons are closures,
/// which can't be compared.
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (
            self.id,
            &self.notice.short,
            self.notice.severity,
            &self.notice.more,
            self.notice.actions.len(),
            &self.notice.of,
            self.notice.lasts,
            self.clock,
        ) == (
            other.id,
            &other.notice.short,
            other.notice.severity,
            &other.notice.more,
            other.notice.actions.len(),
            &other.notice.of,
            other.notice.lasts,
            other.clock,
        )
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

    /// The bar's pill: how many notices there are, and the worst severity among them, which tints
    /// it. None below two: one notice is the line itself.
    pub fn pill(&self) -> Option<(usize, Severity)> {
        match self.entries.as_slice() {
            [] | [_] => None,
            all => Some((all.len(), all.iter().map(|e| e.notice.severity).max().unwrap_or(Severity::Info))),
        }
    }

    /// When the first running clock runs out: where the app wakes to redraw.
    pub fn next_due(&self) -> Option<f64> {
        self.entries.iter().chain(&self.hidden).filter_map(|e| if let Clock::Until(t) = e.clock { Some(t) } else { None }).reduce(f64::min)
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
    /// A click on the bar's notice: opens the list, at the newest. It never dismisses (#87).
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
    /// The app's clock reads this many seconds: timed notices whose time has come go, and clocks
    /// waiting to start start.
    Passed(f64),
    /// These undo-stack entries left it, and why, as the service says: undone by anyone, or taken
    /// by a newer entry. Refused, aged and dropped ones change no notice.
    Left(Vec<Left>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Dismissed already, or never there: a click that raced another.
    NoSuchNotice(NoticeId),
    NothingToOpen,
    /// A `Passed` that starts no clock and ends no notice.
    NothingDue,
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
            next.entries.push(Entry { id: next.next_id, notice: notice.clone(), clock: Clock::Stopped });
            next.next_id += 1;
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
        Event::Command(Command::Press) if state.entries.is_empty() => return Err(Refusal::NothingToOpen),
        Event::Command(Command::Press) => next.current = state.current.or(state.newest().map(|e| e.id)),
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
        Event::Observed(Observed::Passed(now)) => {
            let due = |e: &Entry| matches!(e.clock, Clock::Until(t) if t <= *now);
            next.entries.retain(|e| !due(e));
            next.hidden.retain(|e| !due(e));
            for e in next.entries.iter_mut().chain(&mut next.hidden).filter(|e| e.clock == Clock::Starting) {
                e.clock = Clock::Until(now + e.notice.lasts.secs().unwrap_or_default());
            }
            if next.entries == state.entries && next.hidden == state.hidden {
                return Err(Refusal::NothingDue);
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
    // After the popover's: it closes when its last notice goes.
    set_clocks(&mut next);
    Ok(next)
}

/// Whether a notice is out of the list: it tells of actions, and an Undo is under way for every one.
fn hidden(entry: &Entry, pending: &BTreeSet<u64>) -> bool {
    !entry.notice.of.is_empty() && entry.notice.of.iter().all(|q| pending.contains(q))
}

/// Puts each notice where it belongs: the list, or out of it while its undo is under way (back in its
/// place, by id, when that's refused); and the list within [`MAX_TOLD`].
fn settle(next: &mut State) {
    let was_out: Vec<NoticeId> = next.hidden.iter().map(|e| e.id).collect();
    let mut all: Vec<Entry> = next.entries.drain(..).chain(next.hidden.drain(..)).collect();
    all.sort_by_key(|e| e.id);
    (next.hidden, next.entries) = all.into_iter().partition(|e| hidden(e, &next.pending));
    // A notice just back from out of the list is never the one the cap drops (#474): the oldest of the others goes.
    while next.entries.len() > MAX_TOLD {
        let oldest = next.entries.iter().position(|e| !was_out.contains(&e.id)).unwrap_or(0);
        next.entries.remove(oldest);
    }
}

/// A timed notice's clock runs only while the popover is closed: open stops it, and one that's
/// stopped while closed (the popover just closed, or the notice just told) waits to start.
fn set_clocks(next: &mut State) {
    let open = next.open();
    for e in next.entries.iter_mut().chain(&mut next.hidden) {
        e.clock = match (e.notice.lasts, open, e.clock) {
            (Lasts::UntilDismissed, ..) | (Lasts::Brief | Lasts::Long, true, _) => Clock::Stopped,
            (Lasts::Brief | Lasts::Long, false, Clock::Stopped) => Clock::Starting,
            (Lasts::Brief | Lasts::Long, false, running) => running,
        };
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

/// The status bar's notices as a [`Machine`](machine::Machine): drawing follows the
/// state, so there's nothing to carry out. Only ever named.
#[allow(dead_code)]
pub struct Notices;

impl machine::Machine for Notices {
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
    use machine::every;
    use machine::{DecideRow, Every, Row, decide_table, sequences};

    use super::*;

    fn plain(short: &str) -> Notice {
        Notice::warning(short)
    }

    fn with_more(short: &str) -> Notice {
        Notice::warning(short).with_text("More to read")
    }

    fn failed(short: &str) -> Notice {
        Notice::error(short)
    }

    /// `n`, telling of the undo-stack entries `seqs`.
    fn of(n: Notice, seqs: &[u64]) -> Notice {
        Notice { of: seqs.to_vec(), ..n }
    }

    fn failed_with_list(short: &str) -> Notice {
        Notice::error(short).with_text("More to read")
    }

    /// `shown`: the position of the notice the popover shows, newest first (0 is the newest).
    fn state(notices: Vec<Notice>, shown: Option<usize>) -> State {
        let entries: Vec<Entry> =
            notices.into_iter().enumerate().map(|(i, notice)| Entry { id: i as u64, notice, clock: Clock::Stopped }).collect();
        let next_id = entries.len() as u64;
        let current = shown.map(|at| entries[entries.len() - 1 - at].id);
        let mut state = State { entries, hidden: Vec::new(), pending: BTreeSet::new(), current, next_id };
        set_clocks(&mut state);
        state
    }

    /// `s` with the clock of every timed notice running out `secs` after `at`: as one that began
    /// at `at` is, if the popover is closed.
    fn running(mut s: State, at: f64) -> State {
        for e in &mut s.entries {
            if e.clock == Clock::Starting {
                e.clock = Clock::Until(at + e.notice.lasts.secs().unwrap_or_default());
            }
        }
        s
    }

    fn timed(short: &str) -> Notice {
        Notice::new(short).brief()
    }

    fn long(short: &str) -> Notice {
        Notice::new(short).long()
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
                vec![timed("c")],
                vec![told("a"), timed("c")],
                vec![timed("c"), told_more("b"), timed("d")],
                vec![acted("Removed a", &[1]), timed("c")],
                vec![of(long("Removed a"), &[1]), timed("c"), told("b")],
                vec![of(long("Removed a"), &[1, 2]), long("Removed b")],
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
                        let (early, late) = (running(plain.clone(), 0.0), running(plain.clone(), 3.0));
                        std::iter::once(plain.clone()).chain([early, late]).chain(pressed.collect::<Vec<_>>())
                    }).collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        },
    });

    every!(Event {
        Event::Command(Command::Tell(_)) => {
            // The last two tell of an action: one whose Undo is under way is told out of the list.
            [plain("new"), with_more("new"), failed("new"), failed_with_list("new"), of(with_more("new"), &[1]), of(with_more("new"), &[1, 2])]
                .map(|n| Event::Command(Command::Tell(n)))
        },
        Event::Command(Command::Dismiss(_)) => (0..MAX_TOLD as u64 + 1).map(|id| Event::Command(Command::Dismiss(id))),
        Event::Command(Command::DismissAll) => [Event::Command(Command::DismissAll)],
        Event::Command(Command::Press) => [Event::Command(Command::Press)],
        Event::Command(Command::Close) => [Event::Command(Command::Close)],
        Event::Command(Command::Step(_)) => [Way::Newer, Way::Older].map(|w| Event::Command(Command::Step(w))),
        Event::Command(Command::Undoing(_)) => [vec![], vec![1], vec![2], vec![1, 2], vec![3], vec![0, 1, 2, 3], vec![99]]
            .map(|seqs| Event::Command(Command::Undoing(seqs))),
        Event::Observed(Observed::Passed(_)) => [0.0, 2.9, 5.0, 5.1, 8.0, 8.1, 59.9, 60.0, 63.0, 100.0].map(|t| Event::Observed(Observed::Passed(t))),
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
    }, generated: {
        use machine::proptest::prelude::*;
        // Where a clock ends up: a time the states and the other draws have started one at, and the
        // length of a brief or a long notice on (or none), a hair either side of the deadline.
        // Generated by itself, `Passed(deadline)` is a coin's toss from a state's clock; from a few
        // starting times it is a likely hit.
        let at_a_deadline = (
            prop_oneof![8 => Just(0.0), 4 => Just(3.0), 1 => 0.0..200.0f64],
            prop_oneof![Just(0.0), Just(crate::notice::BRIEF_SECS), Just(crate::notice::LONG_SECS)],
            prop_oneof![3 => Just(0.0), 3 => Just(-0.001), 3 => Just(0.001), 1 => Just(-1e-9), 1 => Just(1e-9)],
        )
            .prop_map(|(start, length, hair)| Event::Observed(Observed::Passed(start + length + hair)));
        // The samples tell only untimed notices; a sequence needs timed ones told, for clocks to start.
        let timed = prop_oneof![Just(timed("b")), Just(long("l"))].prop_map(|n| Event::Command(Command::Tell(n)));
        prop_oneof![3 => machine::samples::<Event>(), 2 => timed, 4 => at_a_deadline]
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

    /// Whether `n`, told now, is out of the list at once: it tells of actions, and an Undo is under
    /// way for every one.
    fn told_under_undo(s: &State, n: &Notice) -> bool {
        !n.of.is_empty() && n.of.iter().all(|q| s.pending.contains(q))
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
        let was_out: Vec<NoticeId> = s.hidden.iter().map(|x| x.id).collect();
        while shown.len() > MAX_TOLD {
            shown.remove(shown.iter().position(|(id, _)| !was_out.contains(id)).unwrap_or(0));
        }
        Some((shown, hidden, pending))
    }

    /// The same notices, whatever their clocks.
    fn same_told(a: &[Entry], b: &[Entry]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| Entry { clock: Clock::Stopped, ..x.clone() } == Entry { clock: Clock::Stopped, ..y.clone() })
    }

    /// What time `now` makes of the notices, worked out the long way round: each timed one with a
    /// clock running out by `now` is gone, each still waiting to start runs `BRIEF_SECS` from `now`.
    fn passed(s: &State, now: f64) -> (Vec<(NoticeId, Clock)>, Vec<(NoticeId, Clock)>) {
        let of = |v: &[Entry]| {
            v.iter()
                .filter_map(|x| match x.clock {
                    Clock::Until(t) if t <= now => None,
                    Clock::Starting => Some((x.id, Clock::Until(now + x.notice.lasts.secs().unwrap_or_default()))),
                    c => Some((x.id, c)),
                })
                .collect()
        };
        (of(&s.entries), of(&s.hidden))
    }

    fn clocks(s: &State) -> (Vec<(NoticeId, Clock)>, Vec<(NoticeId, Clock)>) {
        let of = |v: &[Entry]| v.iter().map(|x| (x.id, x.clock)).collect();
        (of(&s.entries), of(&s.hidden))
    }

    fn rows() -> Vec<DecideRow<Notices>> {
        vec![
            Row {
                says: "an Info or a Warning told is the newest of the told, the oldest told dropped past the most kept; it never opens the popover",
                when: |s, e| matches!(e, Event::Command(Command::Tell(n)) if n.severity != Severity::Error && !told_under_undo(s, n)),
                then: told_as_it_should,
            },
            Row {
                says: "an error told is the same: the newest of the told, the oldest told (an error or not) dropped past the most kept; it never opens the popover",
                when: |s, e| matches!(e, Event::Command(Command::Tell(n)) if n.severity == Severity::Error && !told_under_undo(s, n)),
                then: told_as_it_should,
            },
            Row {
                says: "a notice told of actions whose Undo is under way, every one, is out of the list at once (#200): it's among those out of the list, the list (the oldest kept) and the popover as they were, and it comes back if the Undo is refused",
                when: |s, e| matches!(e, Event::Command(Command::Tell(n)) if told_under_undo(s, n)),
                then: |s, e, out| {
                    let (Event::Command(Command::Tell(n)), Ok(o)) = (e, out) else { return false };
                    same_told(&o.entries, &s.entries)
                        && o.current == s.current
                        && o.hidden.last().is_some_and(|h| h.notice.short == n.short && h.notice.of == n.of)
                        && o.pending == s.pending
                },
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
                says: "a click on a notice opens the list at the newest (as it was, if open), even a lone notice with nothing more to read, and changes nothing else",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && !s.entries.is_empty(),
                then: |s, _, out| {
                    out.as_ref().is_ok_and(|o| same_told(&o.entries, &s.entries) && o.current == s.current.or(s.newest().map(|e| e.id)))
                },
            },
            Row {
                says: "with no notice, a click has nothing to open: refused",
                when: |s, e| matches!(e, Event::Command(Command::Press)) && s.entries.is_empty(),
                then: |_, _, out| *out == Err(Refusal::NothingToOpen),
            },
            Row {
                says: "Escape or a click outside closes the open popover and changes nothing else but the clocks: each timed notice's waits to start, its full time from the next time given, whatever it had left",
                when: |s, e| matches!(e, Event::Command(Command::Close)) && s.open(),
                then: |s, _, out| {
                    out.as_ref().is_ok_and(|o| {
                        let waits =
                            |x: &Entry| x.clock == if x.notice.lasts != Lasts::UntilDismissed { Clock::Starting } else { Clock::Stopped };
                        !o.open() && same_told(&o.entries, &s.entries) && o.entries.iter().all(waits)
                    })
                },
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
                says: "time passing ends each timed notice whose clock has run out, as a dismissal (the popover is closed, so it shows none), and starts each waiting clock its length on (5 s, or 60 s for an action's); nothing to end or start: refused",
                when: |_, e| matches!(e, Event::Observed(Observed::Passed(_))),
                then: |s, e, out| {
                    let Event::Observed(Observed::Passed(now)) = e else { return false };
                    match out {
                        Ok(o) => passed(s, *now) != clocks(s) && clocks(o) == passed(s, *now),
                        Err(r) => passed(s, *now) == clocks(s) && *r == Refusal::NothingDue,
                    }
                },
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
                says: "an entry undone goes from what each notice tells of and from what's under way (a notice goes when it was the last it told of, from the list or from out of it); one absorbed is replaced there by what took its place; one refused is no longer under way, so its notice is back in its place (on a full list, the oldest goes, #349); aged and dropped ones change nothing; nothing changed: refused",
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
                "a notice that tells of none is never touched by an Undo or an entry that left: it stays, saying the same, unless a refused one's notice came back to a full list and the cap took it as the oldest (#349)",
                |s, e, out| {
                    let (Event::Observed(Observed::Left(_)) | Event::Command(Command::Undoing(_)), Ok(o)) = (e, out) else { return true };
                    let capped = |x: &Entry| {
                        o.entries.len() == MAX_TOLD && o.entries.iter().all(|y| y.id > x.id || s.hidden.iter().any(|h| h.id == y.id))
                    };
                    s.entries.iter().filter(|x| x.notice.of.is_empty()).all(|x| o.entries.iter().any(|y| y == x) || capped(x))
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
            Row::always(
                "a clock runs only while the popover is closed, and only for a timed notice; one that's told, or left running by a close, waits to start",
                |_, _, out| {
                    out.as_ref().map_or(true, |o| {
                        o.entries
                            .iter()
                            .chain(&o.hidden)
                            .all(|x| (x.clock == Clock::Stopped) == (o.open() || x.notice.lasts == Lasts::UntilDismissed))
                    })
                },
            ),
            Row::always(
                "only time passing ends a notice by itself: a notice not timed never expires, and a running clock is kept while the popover stays closed",
                |s, e, out| {
                    let Ok(o) = out else { return true };
                    let kept = |x: &Entry| o.entries.iter().chain(&o.hidden).any(|y| y.id == x.id);
                    let dismissing =
                        matches!(e, Event::Command(Command::Dismiss(_) | Command::DismissAll) | Event::Observed(Observed::Left(_)));
                    let by_time = matches!(e, Event::Observed(Observed::Passed(_)));
                    let told = matches!(e, Event::Command(Command::Tell(_)));
                    let untimed_stay = dismissing
                        || told && s.entries.len() >= MAX_TOLD
                        || s.entries.iter().chain(&s.hidden).filter(|x| x.notice.lasts == Lasts::UntilDismissed).all(kept);
                    let running_kept = by_time
                        || dismissing
                        || s.open()
                        || o.open()
                        || s.entries.iter().all(|x| {
                            !matches!(x.clock, Clock::Until(_))
                                || o.entries.iter().chain(&o.hidden).any(|y| y.id == x.id && y.clock == x.clock)
                                || s.entries.len() >= MAX_TOLD
                        });
                    untimed_stay && running_kept
                },
            ),
            Row::always("the popover shows a notice that's there, and is open only while there is one", |_, _, out| {
                out.as_ref().map_or(true, |o| o.current.is_none() || (o.shown().is_some() && o.index().is_some()))
            }),
            Row::always("no notice told, an error included, opens the popover", |s, e, out| {
                !matches!(e, Event::Command(Command::Tell(_))) || s.open() || out.as_ref().is_ok_and(|o| !o.open())
            }),
            Row::always("a click never dismisses: only × and Dismiss all do, besides time and undo", |s, e, out| {
                let (Event::Command(Command::Press), Ok(o)) = (e, out) else { return true };
                same_told(&o.entries, &s.entries)
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
    fn a_notice_told_under_undo_on_a_full_list_costs_the_oldest_nothing() {
        let full = state((0..MAX_TOLD).map(|i| plain(&format!("t{i}"))).chain([]).collect(), None);
        let full = State {
            entries: full
                .entries
                .into_iter()
                .enumerate()
                .map(|(i, e)| if i == 1 { Entry { notice: of(e.notice, &[1, 2]), ..e } } else { e })
                .collect(),
            ..full
        };
        let pressed = decide(&full, &Event::Command(Command::Undoing(vec![1]))).unwrap();
        let before = ids(&pressed);
        let told = decide(&pressed, &Event::Command(Command::Tell(of(with_more("new"), &[1])))).unwrap();
        assert_eq!(ids(&told), before, "the list keeps every notice it had");
        assert_eq!(told.hidden.last().map(|h| h.notice.short.as_str()), Some("new"));
    }

    /// #349: on a full list, a notice told while its Undo runs costs nobody their place (#299), but
    /// once the Undo is refused it's back in its place, and the list keeps to [`MAX_TOLD`] as for any
    /// notice coming in: the oldest goes, though it tells of no action. Every step through the table.
    #[test]
    fn a_refused_undo_on_a_full_list_brings_its_notice_back_and_the_oldest_goes() {
        let step = |s: &State, e: Event| machine::check(decide, &rows(), &always(), s, &e).unwrap();
        let full = state((0..MAX_TOLD).map(|i| plain(&format!("t{i}"))).collect(), None);
        let told = step(&full, Event::Command(Command::Tell(of(with_more("new"), &[1, 2]))));
        assert_eq!(told.entries[0].notice.short, "t1", "told normally, it drops the oldest");
        let pressed = step(&told, Event::Command(Command::Undoing(vec![1, 2])));
        let refilled = step(&pressed, Event::Command(Command::Tell(plain("b"))));
        assert_eq!(refilled.len(), MAX_TOLD, "the list is full again, the undone notice out of it");
        let refused = Leave::Refused { why: proto::CantUndo::NotKept, stranded: Vec::new() };
        let back = step(&refilled, Event::Observed(Observed::Left(vec![Left { seq: 1, why: refused }])));
        let shorts: Vec<&str> = back.entries.iter().map(|e| e.notice.short.as_str()).collect();
        assert_eq!(back.len(), MAX_TOLD);
        assert_eq!(shorts[0], "t2", "the oldest went");
        assert_eq!(shorts[MAX_TOLD - 2..], ["new", "b"], "the notice is back in its place, before the one told since");
    }

    /// #474: on a full list, a refused Undo's notice that is the oldest once back must not be the one
    /// the cap drops: it has just come back, so the oldest of the others goes. Every step through the table.
    #[test]
    fn a_refused_undo_on_a_full_list_keeps_its_notice_even_when_it_is_the_oldest() {
        let step = |s: &State, e: Event| machine::check(decide, &rows(), &always(), s, &e).unwrap();
        let mut notices: Vec<Notice> = (0..MAX_TOLD).map(|i| plain(&format!("t{i}"))).collect();
        notices[1] = of(with_more("r1"), &[1]);
        let full = state(notices, None);
        let pressed = step(&full, Event::Command(Command::Undoing(vec![1])));
        let told = step(&pressed, Event::Command(Command::Tell(plain("new"))));
        let told = step(&told, Event::Command(Command::Tell(plain("new"))));
        assert_eq!(told.entries[0].notice.short, "t2", "told normally, the cap took the oldest, t0");
        let refused = Leave::Refused { why: proto::CantUndo::NotKept, stranded: Vec::new() };
        let back = step(&told, Event::Observed(Observed::Left(vec![Left { seq: 1, why: refused }])));
        assert_eq!(back.len(), MAX_TOLD);
        assert_eq!(back.entries[0].notice.short, "r1", "the refused notice is back, the oldest");
        assert_eq!(back.entries[1].notice.short, "t3", "the oldest of the others went");
    }

    /// The bar's pill: how many notices, tinted by the worst of them; none for one, whatever it has
    /// to list.
    #[test]
    fn the_pill_counts_every_notice_in_the_worst_severity_from_two_up() {
        assert_eq!(state(vec![], None).pill(), None, "no notice, no pill");
        assert_eq!(state(vec![timed("a")], None).pill(), None, "a lone notice");
        assert_eq!(state(vec![with_more("a")], None).pill(), None, "a lone notice with a list too");
        let mixed = vec![failed("e"), timed("b"), plain("w"), timed("c")];
        assert_eq!(state(mixed, None).pill(), Some((4, Severity::Error)), "an older error tints it, though the newest is a receipt");
        assert_eq!(state(vec![timed("a"), timed("b")], None).pill(), Some((2, Severity::Info)));
        assert_eq!(state(vec![plain("w"), timed("b")], None).pill(), Some((2, Severity::Warning)));
    }

    /// A click always opens the list, at the newest, a lone notice with nothing to read included;
    /// only × takes a notice away.
    #[test]
    fn a_click_opens_even_a_lone_notice_and_only_its_cross_dismisses_it() {
        let lone = state(vec![timed("Added the torrent")], None);
        let open = decide(&lone, &Event::Command(Command::Press)).unwrap();
        assert_eq!(open.shown().map(|e| e.notice.short.as_str()), Some("Added the torrent"));
        let id = open.current.unwrap();
        let gone = decide(&open, &Event::Command(Command::Dismiss(id))).unwrap();
        assert!(gone.entries.is_empty() && !gone.open());
        let several = state(vec![plain("old"), failed("older"), timed("new")], None);
        let open = decide(&several, &Event::Command(Command::Press)).unwrap();
        assert_eq!(open.index(), Some(0), "it opens at the newest");
    }

    #[test]
    fn decide_follows_the_table() {
        decide_table::<Notices>(&rows(), &always());
    }

    #[test]
    fn every_sequence_follows_the_table() {
        sequences::<Notices>(State::every(), 16, &rows(), &always());
    }

    /// From states with clocks running, where a deadline's hair either side is likely to be drawn
    /// (#275): a notice goes at its time, not a millisecond before.
    #[test]
    fn running_clocks_follow_the_table_at_their_deadlines() {
        let running: Vec<State> =
            State::every().into_iter().filter(|s| s.entries.iter().any(|e| matches!(e.clock, Clock::Until(_)))).collect();
        assert!(!running.is_empty());
        sequences::<Notices>(running, 12, &rows(), &always());
    }
}
