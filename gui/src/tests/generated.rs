//! Generated UI sequences against `MemoryService`: a person adding magnets (pasted, or typed into
//! the link field and sent with Return or Add), files dropped and links added in batches where some
//! fail, dismissing the errors listed in the error list, choosing or cancelling the folder picker, selecting
//! rows, right-clicking them for their menu, double-clicking them, pausing, resuming, checking, removing, deleting and setting file priorities, while files
//! are deleted and put back outside Phantom, destinations go and come back, and time passes. Adds
//! take a while to be answered, as with a busy service (a .torrent URL longer), so the person
//! keeps typing while an answer is on its way. After every step the screen must agree with the
//! app's state and the service's answers must make sense together. The reference model knows
//! which rows there are and which one is selected, so steps can name them, and what the field
//! holds, which adds are on their way and when each is answered.
//!
//! Only `Wait` moves the app clock; every other step takes no time on it, so the model knows to
//! the frame when an answer lands.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES=1000 cargo test -p gui generated` runs more. Failing
//! sequences, shrunk, are saved to `gui/proptest-regressions/` and run first after.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{Action as Do, Status, TorrentId, TorrentInfo, Unavailable};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{FRAME, app, drawn_menu, enabled, pick_choice_then, settle, with_an_album};
use crate::model::{Phantom, add, details_torrent, link_problem, selected_for, selected_torrents, shown_torrent, size_label};
use crate::platform::{Show, Shown};
use crate::service::{AddRequest, Disk, DiskChange, Probe};
use crate::shortcuts;
use crate::views::toolbar::why_off;

/// Magnets by number: each its own info hash.
const MAGNETS: u8 = 3;
/// At most this many rows, so every one is on screen with the pane open.
const MAX_ROWS: usize = 6;

fn magnet(n: u8) -> String {
    format!("magnet:?xt=urn:btih:{:040x}&dn=Magnet+{n}", 0xa0 + n as u64)
}

/// What the person puts in the link field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Magnet(u8),
    /// A .torrent URL: slower to add.
    TorrentUrl,
    /// A link the service can't add: nobody has it.
    Unknown,
    /// Not a link: Add is off for it.
    NotALink,
    /// A magnet link without an info hash: Add is off for it too.
    NoInfoHash,
    /// Nothing: the field cleared.
    Empty,
    /// A .torrent file added from outside, by number: it's added.
    File(u8),
    /// A .torrent file the service can't add.
    BadFile(u8),
}

/// Files by number, in a folder that needn't exist: the fake service never reads them.
const FILES: u8 = 3;

const TORRENT_URL: &str = "https://example.com/films/Sintel.torrent";
const UNKNOWN: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";

impl Link {
    fn text(self) -> String {
        match self {
            Link::Magnet(n) => magnet(n),
            Link::TorrentUrl => TORRENT_URL.into(),
            Link::Unknown => UNKNOWN.into(),
            Link::NotALink => "hello".into(),
            Link::NoInfoHash => "magnet:?dn=Big+Buck+Bunny".into(),
            Link::Empty => String::new(),
            Link::File(n) => format!("/tmp/phantom-generated/file-{n}.torrent"),
            Link::BadFile(n) => format!("/tmp/phantom-generated/bad-{n}.torrent"),
        }
    }

    fn is_file(self) -> bool {
        matches!(self, Link::File(_) | Link::BadFile(_))
    }

    /// Why the service can't add it.
    fn reason(self) -> &'static str {
        match self {
            Link::BadFile(_) => "Not a valid .torrent file",
            _ => "No peers have this torrent",
        }
    }

    /// What a note names it by: a file by its name, a link as it is.
    fn named(self) -> String {
        match self {
            Link::File(_) | Link::BadFile(_) => self.text().rsplit('/').next().unwrap().to_owned(),
            link => link.text(),
        }
    }

    /// The service can't add it.
    fn fails(self) -> bool {
        matches!(self, Link::Unknown | Link::BadFile(_))
    }

    fn of(text: &str) -> Option<Link> {
        let links = (0..MAGNETS).map(Link::Magnet).chain([Link::TorrentUrl, Link::Unknown, Link::NotALink, Link::NoInfoHash, Link::Empty]);
        links.into_iter().find(|l| l.text() == text.trim())
    }
}

#[derive(Debug, Clone)]
enum Action {
    /// Pasted with no field focused: added at once.
    PasteMagnet(u8),
    /// Typed over whatever the field holds.
    Type(Link),
    /// Return in the field, or Add.
    Send {
        button: bool,
    },
    /// Files dropped on the window, or links added by something else outside it, all at once.
    AddAll(Vec<Link>),
    /// Click a line of the n-th entry of the error list, its title or one of its failures (the
    /// `line`-th text it draws): that entry is dismissed, and no other.
    DismissEntry {
        n: usize,
        line: usize,
    },
    /// Click Clear all.
    ClearAll,
    /// Wheel the error list from its top to its end and back: every entry can be scrolled into
    /// view, fully, and the list is back where it began.
    ScrollThrough,
    /// Escape, with no field focused: clears the selection and the error list.
    Escape,
    /// Chooses a folder in the picker, or cancels it.
    Choose(bool),
    Select(TorrentId),
    /// Right-click a row, which selects it if it isn't, then leave its menu.
    RowMenu {
        id: TorrentId,
        leave: Leave,
        /// In a window as small as the app allows (640x320), where a menu may not fit.
        small: bool,
    },
    /// Double-click a row: it's selected, and opened if it can be.
    DoubleClick(TorrentId),
    Pause,
    Resume,
    Check,
    /// ⌫: removes the selection at once, keeping its files.
    Remove,
    /// Click the priority of the file at this row of the pane, selecting the file first if
    /// `select`.
    SetFile {
        row: usize,
        select: bool,
    },
    FilesDeleted {
        id: TorrentId,
        files: Vec<usize>,
    },
    FilesPutBack {
        id: TorrentId,
        files: Vec<usize>,
    },
    DestinationGone(TorrentId),
    DestinationBack(TorrentId),
    /// The service restarts, every torrent as it was: its snapshots are a new epoch.
    Restart,
    Wait(f64),
}

/// What the person picks from a row's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Command(Do),
    Open,
    Reveal,
    CopyMagnet,
    /// Remove, or Delete if `delete` and it's offered.
    Remove {
        delete: bool,
    },
}

/// How the person leaves an open row menu: by pointing at an item and clicking it, by moving to
/// it with the keys and pressing Return, by reading the shortcut it shows, closing the menu and
/// pressing that (an item that shows none is clicked), with Escape, or with a click outside. A pick
/// the menu doesn't offer is left with Escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    Click(Pick),
    Keys(Pick),
    Hint(Pick),
    Escape,
    Outside,
}

/// How the fake service behaves in a case: how many frames an add takes to be answered (that
/// and a half, so an answer never lands on a frame's edge; 0 for the next poll), a .torrent URL's
/// at least as long, whether there's a default destination, how many frames a selected
/// torrent's details take to come, how late a command's own snapshot lands (times 2, 0 and 1
/// in turn, so out of order and after newer ones), and whether the torrents start paused, so the
/// service's snapshots change only when something is done (one late can then be as new as the
/// latest, built for what was watched before).
#[derive(Debug, Clone, Copy)]
struct Setup {
    add_frames: u32,
    torrent_url_frames: u32,
    default_destination: bool,
    details_frames: u32,
    stale_frames: u32,
    paused: bool,
}

impl Setup {
    fn secs(frames: u32) -> f64 {
        if frames == 0 { 0.0 } else { (frames as f64 + 0.5) * FRAME }
    }

    /// Frames from the add until its answer lands.
    fn takes(&self, link: Link) -> u64 {
        let frames = if link == Link::TorrentUrl { self.torrent_url_frames } else { self.add_frames };
        if frames == 0 { 0 } else { frames as u64 + 1 }
    }
}

/// An add on its way.
#[derive(Debug, Clone)]
struct Sent {
    /// One add, or the batch an action added together: they're answered together.
    links: Vec<Link>,
    /// The field's: its answer may touch the field.
    field: bool,
    /// Into a folder chosen in the picker.
    chosen: bool,
    /// The frame its answer lands on.
    lands: u64,
}

/// What an answer that landed says, for the screen to show from the frame it lands on.
#[derive(Debug, Clone, PartialEq)]
enum Said {
    /// Its row is in the list, and the status bar tells of it.
    Added(TorrentId),
    /// The field held its link: the error shows by Add.
    FailedByAdd(String),
    /// The batch is answered and some of it failed: its note shows.
    Noted(Note),
    /// Refused for want of a destination: the picker asks.
    AsksForFolder,
}

/// A note about a batch of adds, as the screen shows it until it's dismissed.
#[derive(Debug, Clone, PartialEq)]
struct Note {
    /// How many were in the batch.
    total: usize,
    /// Each failure: the file or link, and why.
    failed: Vec<(String, &'static str)>,
}

impl Note {
    fn title(&self) -> String {
        client::display::adds_failed(self.failed.len(), self.total)
    }

    /// What its entry draws: the title, then each failure and its reason.
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.title()];
        lines.extend(self.failed.iter().flat_map(|(what, why)| [what.clone(), why.to_string()]));
        lines
    }
}

/// An answer that landed in the last step, on this frame.
#[derive(Debug, Clone)]
struct Landed {
    frame: u64,
    said: Said,
}

#[derive(Debug, Clone)]
struct Rows {
    setup: Setup,
    /// Each row's link, if it was added from one.
    rows: BTreeMap<TorrentId, Option<Link>>,
    selected: Option<TorrentId>,
    next_id: TorrentId,
    /// Frames of the app clock so far.
    frame: u64,
    /// What the link field holds.
    field: String,
    /// The link the error by Add is about, while one may show.
    error_by_add: Option<String>,
    /// Adds on their way, in the order they were sent.
    sent: Vec<Sent>,
    /// Adds refused for want of a destination, waiting for the picker; the first is asking.
    picker: VecDeque<Sent>,
    /// The error list, oldest first: each batch with failures is one entry, until dismissed.
    errors: Vec<Note>,
    /// The answers that landed in the last step, in the order they landed.
    landed: Vec<Landed>,
    /// The frame the last step started on, and the one an answer being taken lands on.
    frame_before: u64,
    frame_of_landing: u64,
}

impl Rows {
    /// Rows there are or may be: so the list never outgrows the screen.
    fn room(&self, adding: usize) -> bool {
        let waiting = self.sent.iter().chain(&self.picker).map(|s| s.links.len()).sum::<usize>();
        self.rows.len() + waiting + adding <= MAX_ROWS
    }

    fn send(&mut self, links: Vec<Link>, field: bool, chosen: bool) {
        // A batch is answered when its slowest add is.
        let lands = self.frame + links.iter().map(|l| self.setup.takes(*l)).max().unwrap_or(0);
        self.sent.push(Sent { links, field, chosen, lands });
    }

    /// The answers whose frame has come, in the order they land, as the app takes them.
    fn land(&mut self) {
        let frame = self.frame;
        let (mut landing, waiting): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.sent).into_iter().enumerate().partition(|(_, s)| s.lands <= frame);
        self.sent = waiting.into_iter().map(|(_, s)| s).collect();
        // By the frame each lands on, and those landing together in the order they were sent.
        landing.sort_by_key(|(i, s)| (s.lands, *i));
        for (_, sent) in landing {
            // Within a `Wait` it lands on its own frame; otherwise on this step's.
            self.frame_of_landing = sent.lands.max(self.frame_before);
            self.answer(sent);
        }
    }

    fn answer(&mut self, sent: Sent) {
        if !sent.chosen && !self.setup.default_destination {
            self.picker.push_back(sent);
            self.landed.push(Landed { frame: self.frame_of_landing, said: Said::AsksForFolder });
            return;
        }
        // The field's add: its answer may touch the field, while it still holds the link.
        let holds = sent.field && sent.links.iter().all(|l| self.field.trim() == l.text());
        let mut note = Note { total: 0, failed: Vec::new() };
        for link in sent.links {
            let said = if link.fails() {
                if holds {
                    self.error_by_add = Some(link.text());
                    Said::FailedByAdd(self.field.clone())
                } else {
                    note.total += 1;
                    note.failed.push((link.named(), link.reason()));
                    continue;
                }
            } else {
                note.total += 1;
                Said::Added(self.added(link, holds))
            };
            self.landed.push(Landed { frame: self.frame_of_landing, said });
        }
        if !note.failed.is_empty() {
            self.errors.push(note.clone());
            self.landed.push(Landed { frame: self.frame_of_landing, said: Said::Noted(note) });
        }
    }

    /// The row of an add that succeeded.
    fn added(&mut self, link: Link, holds: bool) -> TorrentId {
        let existing = self.rows.iter().find(|(_, l)| !link.is_file() && **l == Some(link)).map(|(&id, _)| id);
        let id = match existing {
            // Already in the list: selected instead.
            Some(id) => {
                self.selected = Some(id);
                id
            }
            None => {
                self.rows.insert(self.next_id, Some(link));
                self.next_id += 1;
                self.next_id - 1
            }
        };
        if holds {
            self.field.clear();
        }
        id
    }
}

struct Screen;

impl ReferenceStateMachine for Screen {
    type State = Rows;
    type Transition = Action;

    fn init_state() -> BoxedStrategy<Rows> {
        let frames = || prop_oneof![Just(0u32), Just(47), Just(107)];
        let setup = (frames(), prop_oneof![Just(0u32), Just(78)], any::<bool>(), frames(), frames(), any::<bool>()).prop_map(
            |(add_frames, more, default_destination, details_frames, stale_frames, paused)| Setup {
                add_frames,
                torrent_url_frames: add_frames + more,
                default_destination,
                details_frames,
                stale_frames,
                paused,
            },
        );
        setup
            .prop_map(|setup| Rows {
                setup,
                rows: [(0, None), (1, None)].into(),
                selected: None,
                next_id: 2,
                frame: 0,
                field: String::new(),
                error_by_add: None,
                sent: Vec::new(),
                picker: VecDeque::new(),
                errors: Vec::new(),
                landed: Vec::new(),
                frame_before: 0,
                frame_of_landing: 0,
            })
            .boxed()
    }

    fn transitions(rows: &Rows) -> BoxedStrategy<Action> {
        let ids: Vec<TorrentId> = rows.rows.keys().copied().collect();
        let files = proptest::collection::vec(0..3usize, 1..3);
        let paste = (0..MAGNETS).prop_map(Action::PasteMagnet);
        let link = prop_oneof![
            3 => (0..MAGNETS).prop_map(Link::Magnet),
            1 => Just(Link::TorrentUrl),
            3 => Just(Link::Unknown),
            1 => Just(Link::NotALink),
            1 => Just(Link::NoInfoHash),
            1 => Just(Link::Empty),
        ];
        let field = prop_oneof![
            3 => link.prop_map(Action::Type),
            3 => any::<bool>().prop_map(|button| Action::Send { button }),
            1 => any::<bool>().prop_map(Action::Choose),
        ];
        let waits = prop_oneof![Just(0.5), Just(1.5), Just(2.5)].prop_map(Action::Wait);
        // Several at once, a few of them failing: the same kind of source in a batch is added
        // together, as files dropped on the window or links opened from outside.
        let source = prop_oneof![
            3 => (0..FILES).prop_map(Link::File),
            2 => (0..FILES).prop_map(Link::BadFile),
            2 => (0..MAGNETS).prop_map(Link::Magnet),
            2 => Just(Link::Unknown),
        ];
        let batch = proptest::collection::vec(source, 2..5).prop_map(Action::AddAll);
        let listed = rows.errors.len();
        let dismiss = if listed > 0 {
            prop_oneof![(0..listed, 0..8usize).prop_map(|(n, line)| Action::DismissEntry { n, line }), Just(Action::ClearAll)].boxed()
        } else {
            Just(Action::Escape).boxed()
        };
        let scroll = if listed > 0 { Just(Action::ScrollThrough).boxed() } else { Just(Action::Escape).boxed() };
        let notes = prop_oneof![2 => dismiss, 1 => Just(Action::Escape), 1 => scroll];
        if ids.is_empty() {
            return prop_oneof![1 => paste, 2 => field, 2 => batch, 1 => notes, 1 => waits].boxed();
        }
        let id = proptest::sample::select(ids);
        let pick = prop_oneof![
            1 => Just(Pick::Command(Do::Pause)),
            1 => Just(Pick::Command(Do::Resume)),
            1 => Just(Pick::Command(Do::Check)),
            1 => Just(Pick::CopyMagnet),
            2 => Just(Pick::Open),
            1 => Just(Pick::Reveal),
            1 => any::<bool>().prop_map(|delete| Pick::Remove { delete }),
        ];
        let leave = prop_oneof![
            3 => pick.clone().prop_map(Leave::Click),
            3 => pick.clone().prop_map(Leave::Keys),
            3 => pick.prop_map(Leave::Hint),
            1 => Just(Leave::Escape),
            1 => Just(Leave::Outside),
        ];
        prop_oneof![
            2 => paste,
            10 => field,
            4 => batch,
            2 => notes,
            4 => id.clone().prop_map(Action::Select),
            3 => (id.clone(), leave, any::<bool>()).prop_map(|(id, leave, small)| Action::RowMenu { id, leave, small }),
            2 => id.clone().prop_map(Action::DoubleClick),
            2 => Just(Action::Pause),
            2 => Just(Action::Resume),
            2 => Just(Action::Check),
            1 => Just(Action::Remove),
            3 => (0..3usize, any::<bool>()).prop_map(|(row, select)| Action::SetFile { row, select }),
            2 => (id.clone(), files.clone()).prop_map(|(id, files)| Action::FilesDeleted { id, files }),
            2 => (id.clone(), files).prop_map(|(id, files)| Action::FilesPutBack { id, files }),
            1 => id.clone().prop_map(Action::DestinationGone),
            1 => id.prop_map(Action::DestinationBack),
            1 => Just(Action::Restart),
            3 => waits,
        ]
        .boxed()
    }

    fn preconditions(rows: &Rows, action: &Action) -> bool {
        match action {
            Action::PasteMagnet(_) | Action::Send { .. } => rows.room(1),
            Action::AddAll(links) => rows.room(links.len()),
            Action::DismissEntry { n, .. } => *n < rows.errors.len(),
            Action::ClearAll | Action::ScrollThrough => !rows.errors.is_empty(),
            Action::Select(id)
            | Action::RowMenu { id, .. }
            | Action::DoubleClick(id)
            | Action::FilesDeleted { id, .. }
            | Action::FilesPutBack { id, .. }
            | Action::DestinationGone(id)
            | Action::DestinationBack(id) => rows.rows.contains_key(id),
            _ => true,
        }
    }

    fn apply(mut rows: Rows, action: &Action) -> Rows {
        rows.landed.clear();
        rows.frame_before = rows.frame;
        // Whatever the person presses, they clear the list out of the way first (see `press`).
        if presses(action) {
            rows.errors.clear();
        }
        match action {
            Action::PasteMagnet(n) => rows.send(vec![Link::Magnet(*n)], false, false),
            Action::AddAll(links) => rows.send(links.clone(), false, false),
            // Exactly that entry goes, the others stay in order.
            Action::DismissEntry { n, .. } => {
                rows.errors.remove(*n);
            }
            Action::ClearAll => rows.errors.clear(),
            Action::Escape => {
                rows.selected = None;
                rows.errors.clear();
            }
            Action::Type(link) => {
                let text = link.text();
                if text != rows.field {
                    rows.error_by_add = None;
                }
                rows.field = text;
            }
            // Every send says something new by Add, or nothing.
            Action::Send { .. } => {
                rows.error_by_add = None;
                if link_problem(&rows.field).is_none() {
                    rows.send(vec![Link::of(&rows.field).expect("a link from the pool")], true, false);
                }
            }
            Action::Choose(chosen) => {
                // Cancelled, it's answered with nothing added: nothing to see.
                if let Some(waiting) = rows.picker.pop_front()
                    && *chosen
                {
                    rows.send(waiting.links, waiting.field, true);
                }
            }
            Action::Select(id) | Action::DoubleClick(id) => rows.selected = Some(*id),
            Action::RowMenu { id, leave, .. } => {
                rows.selected = Some(*id);
                if matches!(leave, Leave::Click(Pick::Remove { .. }) | Leave::Keys(Pick::Remove { .. }) | Leave::Hint(Pick::Remove { .. }))
                {
                    rows.selected = None;
                    rows.rows.remove(id);
                }
            }
            Action::Remove => {
                if let Some(id) = rows.selected.take() {
                    rows.rows.remove(&id);
                }
            }
            Action::Wait(secs) => rows.frame += (secs / FRAME) as u64,
            _ => {}
        }
        rows.land();
        rows
    }
}

/// Whether the step presses something with the pointer: which clears the error list out of the way.
fn presses(action: &Action) -> bool {
    matches!(action, Action::Type(_) | Action::Send { .. } | Action::Select(_) | Action::Pause | Action::Resume | Action::SetFile { .. })
}

struct Sut {
    rt: Runtime<Phantom>,
    disk: Disk,
    /// Torrents seen Paused or Done (wanted stopped) and not resumed since.
    stopped: BTreeSet<TorrentId>,
    /// What was opened or revealed in Finder.
    shown: Shown,
    /// The latest snapshot the service streamed.
    probe: Probe,
}

impl StateMachineTest for Screen {
    type SystemUnderTest = Sut;
    type Reference = Screen;

    fn init_test(rows: &Rows) -> Sut {
        NOTE_WHILE_ON.with(|n| n.take());
        let Setup { add_frames, torrent_url_frames, default_destination, details_frames, stale_frames, paused } = rows.setup;
        let album = if paused { with_an_album().paused() } else { with_an_album() };
        let mut service = album
            .details_after(Setup::secs(details_frames))
            .stale_answers(Setup::secs(stale_frames))
            .rejecting(UNKNOWN)
            .adds_after(Setup::secs(add_frames), Setup::secs(torrent_url_frames))
            .with_default_destination(default_destination.then(|| "/srv/torrents".into()));
        for n in 0..FILES {
            service = service.rejecting(&Link::BadFile(n).text());
        }
        let (disk, probe) = (service.disk(), service.probe());
        let mut rt = app(service);
        rt.poll = Some(poll_watching_notes);
        let shown = Shown::default();
        rt.state.finder = Box::new(shown.clone());
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(&mut rt);
        Sut { rt, disk, stopped: BTreeSet::new(), shown, probe }
    }

    fn apply(mut sut: Sut, rows: &Rows, action: Action) -> Sut {
        let mut resumed = match action {
            Action::Resume => selected_for(&sut.rt.state.table, &sut.rt.state.torrents, Do::Resume),
            _ => Vec::new(),
        };
        let rt = &mut sut.rt;
        match &action {
            Action::PasteMagnet(n) => {
                rt.update(|_, tx| tx.blur());
                rt.paste(&magnet(*n));
            }
            Action::AddAll(links) if links.iter().all(|l| l.is_file()) => {
                rt.drop_files(links.iter().map(|l| l.text().into()).collect(), None);
                // The drop is handed over a frame later than a press is.
                rt.frame(0.0);
            }
            Action::AddAll(links) => {
                let requests = links
                    .iter()
                    .map(|l| if l.is_file() { AddRequest::File(l.text().into()) } else { AddRequest::link(&l.text()).expect("a link") });
                let requests = requests.collect();
                rt.update(|s, _| add(s, requests, Vec::new()));
            }
            Action::DismissEntry { n, line } => {
                let key = entry_keys(rt)[*n].clone();
                let (x, y) = line_of(rt, &key, *line);
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
            }
            Action::ClearAll => {
                let (x, y) = rt.find("clear-all").expect("the error list is shown");
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
            }
            Action::ScrollThrough => scroll_through(rt),
            // With no field focused: a focused field takes Escape itself.
            Action::Escape => {
                rt.update(|_, tx| tx.blur());
                rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            }
            Action::Type(link) => {
                press(rt, "magnet-field");
                rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                match link {
                    Link::Empty => rt.key(KeyEvent { key: Key::Backspace, mods: Mods::default() }),
                    link => rt.paste(&link.text()),
                }
            }
            Action::Send { button: true } => press(rt, "add"),
            Action::Send { button: false } => {
                press(rt, "magnet-field");
                rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Action::Choose(chosen) => {
                if rt.file_dialog().is_some() {
                    rt.resolve_file_dialog(chosen.then(|| "/tmp/phantom-generated".into())).unwrap();
                }
            }
            Action::Select(id) => press(rt, &format!("rows/{id}")),
            Action::RowMenu { id, leave, small } => {
                if *small {
                    rt.set_viewport((640.0, 320.0), 2.0);
                    // Scrolled to, so the row is there to click, whatever the size.
                    rt.update(|s, tx| s.table.reveal(id.to_string(), tx));
                    step(rt);
                }
                right_click(rt, &format!("rows/{id}/name"));
                let drawn = drawn_menu(rt, "row-menu").unwrap_or_else(|| panic!("a right-click on row {id} opens its menu"));
                assert!(rt.state.menu.is_open(), "the app knows the menu is open");
                assert_last_overlay(rt, "row-menu-menu");
                assert_labels_aligned(rt, "row-menu");
                assert_inside_window(rt, "row-menu-menu", if *small { (640.0, 320.0) } else { (960.0, 900.0) });
                // Nothing shows as focused until the first arrow, which shows the first choice.
                assert!(focused_choice(rt).is_none_or(|(_, visible)| !visible), "no choice is drawn focused when the menu opens");
                if let Leave::Keys(_) = leave {
                    rt.key(KeyEvent { key: Key::Down, mods: Mods::default() });
                    step(rt);
                    let first = drawn[0].clone().expect("a menu starts with a choice");
                    assert_eq!(focused_choice(rt), Some((first, true)), "the first arrow focuses the first choice");
                }
                let offered = check_row_menu(&rt.state, &drawn);
                let pick = match leave {
                    Leave::Click(pick) | Leave::Keys(pick) | Leave::Hint(pick) => Some(*pick),
                    Leave::Escape | Leave::Outside => None,
                };
                // The label of what's picked, if the menu offers it.
                let label = pick.and_then(|pick| {
                    let wanted = match pick {
                        Pick::Command(Do::Pause) => "Pause",
                        Pick::Command(Do::Resume) => "Resume",
                        Pick::Command(_) => "Check",
                        Pick::CopyMagnet => "Copy magnet link",
                        Pick::Open => "Open",
                        Pick::Reveal => "Reveal in Finder",
                        Pick::Remove { delete: true } if offered.iter().any(|l| l.starts_with("Delete")) => "Delete",
                        Pick::Remove { .. } => "Remove",
                    };
                    offered.iter().find(|l| l.starts_with(wanted)).cloned()
                });
                if label.as_deref().is_some_and(|l| l.starts_with("Resume")) {
                    resumed = selected_for(&rt.state.table, &rt.state.torrents, Do::Resume);
                }
                let before = sut.shown.all().len();
                // Every shortcut shown is the table's for its item.
                for (label, hint) in menu_hints(rt, "row-menu") {
                    assert_eq!(hint, item_shortcut(&label).map(shortcuts::hint), "the shortcut shown by {label:?}");
                }
                let hint = label.as_ref().and_then(|l| menu_hints(rt, "row-menu").into_iter().find(|(m, _)| m == l)).and_then(|(_, h)| h);
                match (leave, &label) {
                    (Leave::Hint(_), Some(label)) if hint.is_some() => {
                        // Read, closed, and pressed: what it shows must do what the item does.
                        let command = shortcuts::by_hint(hint.as_deref().unwrap()).expect("a hint the table has");
                        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                        step(rt);
                        // Off the link field, which keeps ⌫ and Return for its text.
                        rt.update(|_, tx| tx.blur());
                        rt.key(shortcuts::keys(command)[0].event());
                        step(rt);
                        // ⌫ removes at once, so the item was Remove.
                        if command == shortcuts::Command::Remove {
                            assert!(label.starts_with("Remove"), "⌫ shows by Remove, not {label:?}");
                        }
                    }
                    (Leave::Click(_) | Leave::Hint(_), Some(label)) => {
                        // Along the items with the pointer first, as a person looks for one.
                        for e in rt.elements().into_iter().filter(|e| e.role == "menuitem" && e.path.contains("row-menu")) {
                            rt.pointer_move(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0);
                            step(rt);
                        }
                        pick_in(rt, "row-menu", label);
                    }
                    (Leave::Keys(_), Some(label)) => {
                        // Down until the keyboard is on it, round the menu if need be: the first
                        // item has the focus from the start, unseen, and Return needs it seen.
                        for _ in 0..=2 * drawn.len() {
                            if focused_choice(rt).is_some_and(|(l, visible)| visible && l == *label) {
                                break;
                            }
                            rt.key(KeyEvent { key: Key::Down, mods: Mods::default() });
                            step(rt);
                        }
                        assert_eq!(focused_choice(rt), Some((label.clone(), true)), "the keys reach {label:?}");
                        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
                    }
                    (Leave::Outside, _) => {
                        rt.pointer_down(2.0, 2.0);
                        rt.pointer_up(2.0, 2.0);
                    }
                    _ => rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
                }
                step(rt);
                assert!(drawn_menu(rt, "row-menu").is_none() && !rt.state.menu.is_open(), "the menu closes after {leave:?}");
                let how = match label.as_deref() {
                    Some("Open") => Some(Show::Open),
                    Some("Reveal in Finder") => Some(Show::Reveal),
                    _ => None,
                };
                assert_eq!(sut.shown.all()[before..], shown_by(rt, *id, how), "what {leave:?} shows");
                if *small {
                    rt.set_viewport((960.0, 900.0), 2.0);
                    step(rt);
                }
            }
            // Exactly what Open does: the same files, or nothing where it isn't offered.
            Action::DoubleClick(id) => {
                let before = sut.shown.all().len();
                double_click(rt, &format!("rows/{id}/name"));
                assert_eq!(sut.shown.all()[before..], shown_by(rt, *id, Some(Show::Open)), "what a double-click on row {id} shows");
            }
            // Pressed whether on or off: off, it says why (#89).
            Action::Pause | Action::Resume => {
                let (key, action) = if matches!(action, Action::Pause) { ("pause", Do::Pause) } else { ("resume", Do::Resume) };
                close_notes(rt);
                let off = !enabled(rt, key);
                press(rt, key);
                if off {
                    let why = why_off(&rt.state, action).expect("off for a reason");
                    assert_eq!(text(rt, &format!("{key}-note-content")), why, "a click on {key} while it's off says why");
                }
            }
            // From the row menu, the only place Check is now.
            Action::Check => {
                if let Some(id) = rows.selected {
                    right_click(rt, &format!("rows/{id}/name"));
                    match drawn_menu(rt, "row-menu").into_iter().flatten().flatten().find(|l| l == "Check") {
                        Some(_) => pick_in(rt, "row-menu", "Check"),
                        None => rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
                    }
                    step(rt);
                }
            }
            // ⌫, with focus off the link field: the selection is removed at once, with no menu.
            Action::Remove => {
                rt.update(|_, tx| tx.blur());
                rt.key(shortcuts::keys(shortcuts::Command::Remove)[0].event());
                step(rt);
                assert_eq!(rt.state.table.selected_len(), 0, "⌫ removes what is selected, and the selection goes with it");
            }
            Action::SetFile { row, select } => {
                if *select {
                    press(rt, &format!("files/viewport/rows/{row}"));
                }
                press(rt, &format!("files/viewport/rows/{row}/priority"));
            }
            Action::FilesDeleted { id, files } => sut.disk.change(DiskChange::FilesDeleted { id: *id, files: files.clone() }),
            Action::FilesPutBack { id, files } => sut.disk.change(DiskChange::FilesPutBack { id: *id, files: files.clone() }),
            Action::DestinationGone(id) => {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                sut.disk.change(DiskChange::DestinationGone { id: *id, reason });
            }
            Action::DestinationBack(id) => sut.disk.change(DiskChange::DestinationBack { id: *id }),
            Action::Restart => sut.disk.change(DiskChange::Restarted),
            // A frame at a time: each answer shows from the frame it lands on.
            Action::Wait(secs) => {
                let frames = (secs / FRAME) as u64;
                for frame in rows.frame - frames + 1..=rows.frame {
                    rt.frame(FRAME);
                    check_landed(rt, rows, frame);
                    // Every frame: a late snapshot taken for the latest would show only until the
                    // next one.
                    on_the_latest(rt, &sut.probe);
                }
            }
        }
        step(rt);
        if !matches!(action, Action::Wait(_)) {
            check_landed(&sut.rt, rows, rows.frame);
        }

        let ids: Vec<TorrentId> = sut.rt.state.torrents.iter().map(|t| t.id).collect();
        assert_eq!(ids, rows.rows.keys().copied().collect::<Vec<_>>(), "the rows after {action:?}");
        assert_eq!(selected_torrents(&sut.rt.state.table), Vec::from_iter(rows.selected), "the selection after {action:?}");
        // Only Resume starts a torrent that's stopped, and only one it's given.
        sut.stopped.retain(|id| !resumed.contains(id) && ids.contains(id));
        for t in &sut.rt.state.torrents {
            if sut.stopped.contains(&t.id) {
                assert!(!matches!(t.status, Status::Downloading | Status::Seeding), "{action:?} started stopped torrent {}", t.id);
            }
            if matches!(t.status, Status::Paused | Status::Done) {
                sut.stopped.insert(t.id);
            }
        }
        sut
    }

    fn check_invariants(sut: &Sut, rows: &Rows) {
        let rt = &sut.rt;
        let s = &rt.state;

        // The field loses what was typed only to its own link's add.
        assert_eq!(s.magnet.text, rows.field, "the field's text");
        assert_eq!(enabled(rt, "add"), link_problem(&s.magnet.text).is_none(), "Add is on for a link");
        // An error by Add is about the link in the field, and no other.
        if let Some(note) = s.add_note.as_ref().filter(|n| n.error && n.text == s.magnet.text) {
            assert_eq!(rows.error_by_add.as_deref(), Some(note.text.trim()), "the error by Add is about {:?}", note.text);
        }
        assert_eq!(rt.file_dialog().is_some(), !rows.picker.is_empty(), "the picker asks while an add waits for a folder");
        // The status bar tells of no failed add.
        assert!(s.notice.as_ref().is_none_or(|n| !tells_of_failure(&n.short)), "the status bar tells of no add's failure: {:?}", s.notice);
        // Every failure from every fallback source is in the error list until dismissed, one entry
        // per batch, oldest first; nothing else is. It's up while there are entries.
        let expected: Vec<Vec<String>> = rows.errors.iter().map(Note::lines).collect();
        assert_eq!(drawn_entries(rt), expected, "the error list as drawn");
        assert_eq!(
            s.errors.entries().iter().filter(|e| !e.leaving).map(|e| e.notice.short.clone()).collect::<Vec<_>>(),
            rows.errors.iter().map(Note::title).collect::<Vec<_>>(),
            "the error list's entries"
        );
        // Dismissed entries fade where they are before they go, so the panel is up while any is left.
        assert_eq!(rt.find("error-list").is_some(), !s.errors.entries().is_empty(), "the panel is up while there are entries");
        assert!(rows.errors.is_empty() || !s.errors.entries().is_empty(), "and the model's entries are in it");
        // The panel never covers the message by Add, nor the toolbar.
        if let (Some(panel), Some(add)) = (rect_of(rt, "error-list"), rect_of(rt, "add-note-content")) {
            assert!(!overlaps(panel, add), "the error list {panel:?} overlaps the message by Add {add:?}");
        }
        if let Some(panel) = rect_of(rt, "error-list") {
            let window = 900.0;
            assert!(panel[1] + panel[3] <= window - crate::ui::BOTTOM_ROOM + 0.5, "the panel {panel:?} stops above the status bar");
            assert!(panel[3] <= crate::ui::MAX_HEIGHT + 0.5, "and is no taller than its cap: {panel:?}");
        }
        if let (Some(panel), Some(toolbar)) = (rect_of(rt, "error-list"), rect_of(rt, "toolbar")) {
            assert!(panel[1] >= toolbar[1] + toolbar[3], "the error list {panel:?} is under the toolbar {toolbar:?}");
        }
        on_the_latest(rt, &sut.probe);

        let cells = list_cells(rt);
        let shown: Vec<TorrentId> = cells.keys().copied().collect();
        let mut listed: Vec<TorrentId> = s.torrents.iter().map(|t| t.id).collect();
        listed.sort();
        assert_eq!(shown, listed, "a row for every torrent, and no more");

        for t in &s.torrents {
            assert_eq!(
                cells[&t.id].get("status").map(String::as_str),
                Some(client::display::status(&t.status)),
                "torrent {}'s status",
                t.id
            );
            // Its sizes are known exactly once its info is (#98).
            assert_eq!(t.content.is_none(), t.status == Status::FetchingInfo, "torrent {}: {t:?}", t.id);
            if let Some(c) = t.content {
                assert!(c.progress_bytes <= c.size_bytes && c.size_bytes <= c.full_size_bytes, "torrent {}: {t:?}", t.id);
                assert_eq!(cells[&t.id].get("size"), Some(&client::display::bytes(c.size_bytes)), "torrent {}'s size", t.id);
            }
        }

        assert!(!s.menu.is_open(), "every step leaves its menu closed");
        assert!(drawn_menu(rt, "row-menu").is_none(), "no menu is drawn");

        // A button is on when the service lists its action for a torrent it applies to.
        let selected = selected_torrents(&s.table);
        let any = |action| s.torrents.iter().any(|t| selected.contains(&t.id) && t.actions.does(action));
        assert_eq!(enabled(rt, "pause"), any(Do::Pause), "Pause");
        assert_eq!(enabled(rt, "resume"), any(Do::Resume), "Resume");
        // A note shows only after a press while its button was off, with no frame since in which
        // it was on: Pause and Resume ask for it only so, and no frame ended with one asked for while on.
        if let Some(frame) = NOTE_WHILE_ON.with(|n| n.borrow().clone()) {
            panic!("a note stayed asked for while its button was on: {frame}");
        }
        // Off exactly while there's a reason to say, and a note open says it, worded from the
        // torrents as they are now.
        for (key, action) in [("pause", Do::Pause), ("resume", Do::Resume)] {
            let why = why_off(s, action);
            assert_eq!(enabled(rt, key), why.is_none(), "{key} is off exactly with a reason: {why:?}");
            let note = rt.find(&format!("{key}-note-content")).is_some();
            assert_eq!(note, s.why_off == Some(action) && why.is_some(), "{key}'s note shows while it's asked for and off");
            if note {
                assert_eq!(Some(text(rt, &format!("{key}-note-content"))), why, "{key}'s note");
            }
        }

        let Some(selected) = details_torrent(&s.table, &s.torrents) else {
            assert!(rt.find("details").is_none(), "a pane only for exactly one torrent selected");
            assert!(s.pane.watching.is_empty(), "and details only for it");
            return;
        };
        // The selected torrent, or the last one until the selected one's details arrive.
        let t = shown_torrent(&s.table, &s.torrents, &s.pane).expect("a torrent selected");
        assert!(rt.find("details").is_some(), "the pane for torrent {}", t.id);
        let Some(details) = s.pane.details.as_ref() else { return };
        assert!(s.pane.watching.contains(&selected.id), "details come for the selected torrent");
        assert_eq!(details.id, t.id, "the pane shows one torrent: the files are its");
        // Everything in the pane is of one moment (#111): its status and its files agree, the
        // selected torrent's or the last one's, shown live until the selected one's details come.
        let missing = details.files.iter().any(|f| f.missing);
        assert_eq!(t.status == Status::MissingFiles, missing && t.status != Status::Checking, "{t:?}: {details:?}");
        if !matches!(t.status, Status::Checking) {
            assert_eq!(size_label(t), text(rt, "details-size"), "the pane's size is the list's");
        }
        let file_cells = file_cells(rt);
        assert_eq!(file_cells.len(), details.files.len(), "a row for every file");
        for f in &details.files {
            let shows_missing = file_cells[&f.index].get("progress").map(String::as_str) == Some("Missing");
            assert_eq!(shows_missing, f.missing, "file {}: {f:?}", f.index);
        }
    }
}

/// The list and the pane are of the latest snapshot the service streamed for what the pane
/// watches: never an older one, nor one from before a restart, however late a command's own lands.
/// And the pane never stays blank: a selected torrent whose details came shows them.
fn on_the_latest(rt: &Runtime<Phantom>, probe: &Probe) {
    let s = &rt.state;
    let Some(latest) = probe.latest() else { return };
    let set = |ids: &[TorrentId]| ids.iter().copied().collect::<BTreeSet<_>>();
    if set(&latest.watching) != set(&s.pane.watching) {
        return;
    }
    assert_eq!(s.latest, (latest.epoch, latest.version), "the app is on the latest snapshot");
    let by_id = |ts: &[TorrentInfo]| ts.iter().map(|t| (t.id, t.clone())).collect::<BTreeMap<_, _>>();
    assert_eq!(by_id(&s.torrents), by_id(&latest.torrents), "the list is the latest");
    if let Some(selected) = details_torrent(&s.table, &s.torrents)
        && latest.details.iter().any(|d| d.id == selected.id)
    {
        let pane = s.pane.details.as_ref().map(|d| d.id);
        assert_eq!(pane, Some(selected.id), "the pane shows the selected torrent once its details came");
    }
}

/// Every answer that lands on `frame` shows, once the frame is drawn: an add's row is in the list,
/// a failure shows by Add, or in its batch's note, which stays until dismissed. The status bar
/// tells of the last torrent added among those landing together.
fn check_landed(rt: &Runtime<Phantom>, rows: &Rows, frame: u64) {
    let s = &rt.state;
    let landed: Vec<&Said> = rows.landed.iter().filter(|l| l.frame == frame).map(|l| &l.said).collect();
    let last_added = landed.iter().rposition(|said| matches!(said, Said::Added(_)));
    for (i, said) in landed.iter().enumerate() {
        match said {
            Said::Added(id) => assert!(s.torrents.iter().any(|t| t.id == *id), "added torrent {id} is listed from frame {frame}"),
            Said::FailedByAdd(text) => {
                let shown = s.add_note.as_ref().is_some_and(|n| n.error && n.text == *text && n.text == s.magnet.text);
                assert!(shown, "the failure of {text:?} shows by Add from frame {frame}");
            }
            Said::Noted(note) => {
                assert!(drawn_entries(rt).contains(&note.lines()), "{note:?} is in the error list from frame {frame}");
            }
            Said::AsksForFolder => {}
        }
        if Some(i) == last_added {
            let short = s.notice.as_ref().map(|n| n.short.as_str());
            assert!(
                short.is_some_and(|short| !tells_of_failure(short)),
                "the status bar tells of {said:?} from frame {frame}, not {short:?}"
            );
        }
    }
}

/// Whether a status bar line is about an add that failed: the words of a failure's reason or a note's title.
fn tells_of_failure(short: &str) -> bool {
    short.starts_with("Couldn't add") || short.contains("No peers have") || short.contains("Not a valid .torrent")
}

/// The centre of the `line`-th text drawn inside the entry keyed `key` (wrapping round).
fn line_of(rt: &Runtime<Phantom>, key: &str, line: usize) -> (f32, f32) {
    let elements = rt.elements();
    let at = elements.iter().find(|e| e.key.as_deref() == Some(key)).expect("the entry is shown").path.clone();
    let lines: Vec<_> = elements.iter().filter(|e| e.path.starts_with(&at) && !e.exiting && e.text.is_some()).collect();
    let e = lines[line % lines.len()];
    (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
}

/// Whether the entry keyed `key` is entirely inside the list's viewport.
fn fully_shown(rt: &Runtime<Phantom>, key: &str) -> bool {
    let (Some(entry), Some(view)) = (rect_of(rt, key), rect_of(rt, "error-scroll")) else { return false };
    entry[1] >= view[1] - 0.5 && entry[1] + entry[3] <= view[1] + view[3] + 0.5
}

/// Wheels the error list down to its end and back up, checking that each entry is shown whole at
/// some point (a taller one than the viewport is shown from its top, then its bottom), so a list
/// that fades its edges can be read to the end.
pub(super) fn scroll_through(rt: &mut Runtime<Phantom>) {
    let keys = entry_keys(rt);
    let Some((x, y)) = rt.find("error-scroll") else { return };
    let view = rect_of(rt, "error-scroll").unwrap();
    let reached = |rt: &Runtime<Phantom>, key: &str| {
        let entry = rect_of(rt, key).unwrap();
        fully_shown(rt, key)
            || (entry[3] > view[3] && entry[1] <= view[1] + 0.5 || entry[3] > view[3] && entry[1] + entry[3] <= view[1] + view[3] + 0.5)
    };
    let mut seen = vec![false; keys.len()];
    for direction in [1.0, -1.0] {
        for _ in 0..200 {
            for (i, key) in keys.iter().enumerate() {
                seen[i] |= reached(rt, key);
            }
            let before = rect_of(rt, &keys[0]).unwrap()[1];
            rt.wheel(x, y, -40.0 * direction);
            step(rt);
            if rect_of(rt, &keys[0]).unwrap()[1] == before {
                break;
            }
        }
    }
    assert!(seen.iter().all(|s| *s), "every entry is scrolled into view: {seen:?} of {keys:?}");
    assert!(fully_shown(rt, &keys[0]) || rect_of(rt, &keys[0]).unwrap()[1] >= view[1] - 0.5, "and the list is back at its top");
}

/// The keys of the error list's entries, in order.
pub(super) fn entry_keys(rt: &Runtime<Phantom>) -> Vec<String> {
    let shown = rt.elements().into_iter().filter(|e| !e.exiting && e.label.as_deref() != Some(crate::ui::LEAVING));
    shown.filter_map(|e| e.key).filter(|k| k.starts_with("error/")).collect()
}

/// What each entry of the error list draws, in order: its short line, then each failure.
pub(super) fn drawn_entries(rt: &Runtime<Phantom>) -> Vec<Vec<String>> {
    entry_keys(rt).iter().map(|key| texts_under(rt, key)).collect()
}

/// An element's rect, x, y, width, height.
pub(super) fn rect_of(rt: &Runtime<Phantom>, key: &str) -> Option<[f32; 4]> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).map(|e| e.rect)
}

fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// The texts drawn inside the element keyed `key`, in order.
fn texts_under(rt: &Runtime<Phantom>, key: &str) -> Vec<String> {
    let elements = rt.elements();
    let Some(at) = elements.iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.path.clone()) else { return Vec::new() };
    elements.into_iter().filter(|e| e.path.starts_with(&at) && !e.exiting).filter_map(|e| e.text).collect()
}

thread_local! {
    /// The first frame whose poll left a note asked for while its button was on, if any.
    static NOTE_WHILE_ON: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The app's poll, then a look at every frame: a note asked for is for a button that's off.
fn poll_watching_notes(s: &mut Phantom, now: f64, tx: &mut rune::Tx) -> bool {
    let changed = crate::poll(s, now, tx);
    if let Some(action) = s.why_off
        && why_off(s, action).is_none()
    {
        NOTE_WHILE_ON.with(|n| n.borrow_mut().get_or_insert_with(|| format!("{action:?} at {now}")).clone());
    }
    changed
}

/// Clicks the choice `label` in `menu`, taking no time on the app clock.
fn pick_in(rt: &mut Runtime<Phantom>, menu: &str, label: &str) {
    pick_choice_then(rt, menu, label, step);
}

/// Two frames that take no time on the app clock, after a step: only `Wait` moves it.
fn step(rt: &mut Runtime<Phantom>) {
    rt.frame(0.0);
    // The panel's size and its entries ease in on springs; only the app clock is held still.
    rt.settle_all();
    rt.frame(0.0);
}

/// Presses the element at `key`, if it's there; a disabled button does nothing. A note open by
/// Add takes the first click anywhere (it closes), so the person closes it first; so does the error
/// list, which can cover what's pressed: the person clears it.
fn press(rt: &mut Runtime<Phantom>, key: &str) {
    if let Some((x, y)) = rt.find("clear-all") {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
    close_notes(rt);
    if let Some((x, y)) = rt.find(key) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
}

/// Closes the notes open by Add, Pause or Resume, as clicks elsewhere do: in the window's corner,
/// which the error list, in the middle, never covers. A click closes the topmost note's scrim only,
/// so with several open it takes one click for each.
fn close_notes(rt: &mut Runtime<Phantom>) {
    const SCRIMS: [&str; 3] = ["add-note-scrim", "pause-note-scrim", "resume-note-scrim"];
    for _ in 0..SCRIMS.len() {
        if SCRIMS.iter().all(|scrim| rt.find(scrim).is_none()) {
            return;
        }
        rt.pointer_down(2.0, 2.0);
        rt.pointer_up(2.0, 2.0);
        step(rt);
    }
    assert!(SCRIMS.iter().all(|scrim| rt.find(scrim).is_none()), "a click outside closes each note in turn");
}

/// Right-clicks the element at `key`, if it's there, closing the note open by Add first, as
/// [`press`] does.
fn right_click(rt: &mut Runtime<Phantom>, key: &str) {
    close_notes(rt);
    if let Some((x, y)) = rt.find(key) {
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        step(rt);
    }
}

/// What opening or revealing (`how`, if anything) torrent `id` shows: its location, when the
/// service lists the action for it, and the torrent is the only one selected.
fn shown_by(rt: &Runtime<Phantom>, id: TorrentId, how: Option<Show>) -> Vec<(std::path::PathBuf, Show)> {
    let s = &rt.state;
    let Some(how) = how else { return Vec::new() };
    let offered = s.torrents.iter().find(|t| t.id == id).filter(|t| t.actions.does(how.action()));
    match (offered.and_then(|t| t.location.clone()), selected_torrents(&s.table).len()) {
        (Some(location), 1) => vec![(location.into_path_buf(), how)],
        _ => Vec::new(),
    }
}

/// Double-clicks the element at `key`, if it's there.
fn double_click(rt: &mut Runtime<Phantom>, key: &str) {
    close_notes(rt);
    if let Some((x, y)) = rt.find(key) {
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        step(rt);
    }
}

/// The row menu's choices, in order: the label they start with, the action the service must list
/// for it (Copy magnet link has none), and its group.
const MENU_ITEMS: [(&str, Option<Do>, u8); 8] = [
    ("Resume", Some(Do::Resume), 0),
    ("Pause", Some(Do::Pause), 0),
    ("Check", Some(Do::Check), 0),
    ("Open", Some(Do::Open), 1),
    ("Reveal in Finder", Some(Do::Reveal), 1),
    ("Copy magnet link", None, 2),
    ("Remove", Some(Do::Remove), 3),
    ("Delete", Some(Do::Delete), 3),
];

/// Checks what the open row menu draws is exactly the selected torrent's actions (less setting a
/// file's priority) and Copy magnet link, in groups with one separator between neighbouring groups
/// and none at an edge or beside another. Returns the choices' labels.
fn check_row_menu(s: &Phantom, drawn: &[Option<String>]) -> Vec<String> {
    let t = details_torrent(&s.table, &s.torrents).expect("the right-clicked row is the one selected");
    let mut expected: Vec<Option<&str>> = Vec::new();
    let mut last = None;
    for (label, action, group) in MENU_ITEMS {
        if action.is_some_and(|a| !t.actions.does(a)) {
            continue;
        }
        if last.is_some_and(|g| g != group) {
            expected.push(None);
        }
        last = Some(group);
        expected.push(Some(label));
    }
    assert_eq!(drawn.len(), expected.len(), "the menu for {t:?}: {drawn:?}");
    for (drawn, expected) in drawn.iter().zip(&expected) {
        match (drawn, expected) {
            (None, None) => {}
            (Some(label), Some(prefix)) => assert!(label.starts_with(prefix), "{label:?} where {prefix:?} belongs: {drawn:?}"),
            _ => panic!("a separator out of place in {drawn:?}, for {expected:?}"),
        }
    }
    drawn.iter().flatten().cloned().collect()
}

/// Each item of the open menu `menu`, by label, with the shortcut it shows, if any: the texts the
/// item draws besides its label.
fn menu_hints(rt: &Runtime<Phantom>, menu: &str) -> Vec<(String, Option<String>)> {
    let elements = rt.elements();
    elements
        .iter()
        .filter(|e| !e.exiting && e.role == "menuitem" && e.path.contains(menu))
        .filter_map(|item| {
            let label = item.label.clone()?;
            let inside = format!("{}/", item.path);
            let hint = elements.iter().filter(|e| e.path.starts_with(&inside)).filter_map(|e| e.text.clone()).find(|t| *t != label);
            Some((label, hint))
        })
        .collect()
}

/// Every item of the open menu `menu` starts its label at the same x, with an icon or without.
pub(super) fn assert_labels_aligned(rt: &Runtime<Phantom>, menu: &str) {
    let elements = rt.elements();
    let xs: Vec<(String, f32)> = elements
        .iter()
        .filter(|e| !e.exiting && e.role == "menuitem" && e.path.contains(menu))
        .filter_map(|item| {
            let label = item.label.clone()?;
            let inside = format!("{}/", item.path);
            let text = elements.iter().find(|e| e.path.starts_with(&inside) && e.text.as_deref() == Some(label.as_str()))?;
            Some((label, text.rect[0]))
        })
        .collect();
    assert!(xs.windows(2).all(|w| (w[0].1 - w[1].1).abs() < 0.5), "every label starts at the same x: {xs:?}");
}

/// The shortcut that does what the row menu's item labelled `label` does.
fn item_shortcut(label: &str) -> Option<shortcuts::Command> {
    use shortcuts::Command;
    match label {
        "Open" => Some(Command::Open),
        "Reveal in Finder" => Some(Command::Reveal),
        l if l.starts_with("Remove") => Some(Command::Remove),
        l if l.starts_with("Delete") => Some(Command::Delete),
        _ => None,
    }
}

/// A menu is the last overlay: nothing is drawn after it but its own parts, or focus could rest on
/// something behind it (a modal takes what follows it into its own focus).
fn assert_last_overlay(rt: &Runtime<Phantom>, menu: &str) {
    let elements = rt.elements();
    let at = elements.iter().position(|e| !e.exiting && e.key.as_deref() == Some(menu)).expect("the menu is drawn");
    let path = &elements[at].path;
    let root = path.split('/').next().unwrap();
    let after: Vec<&str> = elements[at + 1..]
        .iter()
        .filter(|e| !e.exiting && e.path.starts_with(&format!("{root}/")) && !e.path.starts_with(&format!("{path}/")))
        .filter(|e| e.path.matches('/').count() == 1)
        .filter_map(|e| e.key.as_deref())
        .filter(|k| !k.starts_with(menu.trim_end_matches("-menu")))
        .collect();
    assert!(after.is_empty(), "{menu} is drawn before {after:?}");
}

/// The open menu keyed `menu` lies inside the window, whatever its size: it shifts or flips to fit,
/// and scrolls only when it still doesn't.
fn assert_inside_window(rt: &Runtime<Phantom>, menu: &str, (w, h): (f32, f32)) {
    let surface = rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some(menu)).expect("the menu is drawn");
    let [x, y, mw, mh] = surface.rect;
    let slack = 0.5;
    assert!(
        x >= -slack && y >= -slack && x + mw <= w + slack && y + mh <= h + slack,
        "{menu} at {:?} is outside the {w}x{h} window",
        surface.rect
    );
}

/// The label of the menu choice that has the keyboard focus, and whether it shows.
fn focused_choice(rt: &Runtime<Phantom>) -> Option<(String, bool)> {
    rt.elements().into_iter().find(|e| e.focused && e.role == "menuitem").and_then(|e| Some((e.label?, e.focus_visible)))
}

/// The texts of the rows of the table in the element keyed `table`, by row key and column.
fn cells(rt: &Runtime<Phantom>, table: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let elements = rt.elements();
    let mut cells: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let Some(at) = elements.iter().find(|e| e.key.as_deref() == Some(table)).map(|e| e.path.clone()) else { return cells };
    let Some(rows) = elements.iter().find(|e| e.path.starts_with(&at) && e.key.as_deref() == Some("rows")) else { return cells };
    let prefix = format!("{}/", rows.path);
    for e in elements.iter().filter(|e| !e.exiting) {
        let Some(rest) = e.path.strip_prefix(&prefix) else { continue };
        let mut parts = rest.split('/');
        let (Some(row), column) = (parts.next(), parts.next()) else { continue };
        let row = cells.entry(row.to_owned()).or_default();
        if let (Some(column), Some(text)) = (column, e.text.clone()) {
            row.entry(column.to_owned()).or_insert(text);
        }
    }
    cells
}

pub(super) fn list_cells(rt: &Runtime<Phantom>) -> BTreeMap<TorrentId, BTreeMap<String, String>> {
    let rows = cells(rt, "list");
    rows.into_iter().map(|(id, cells)| (id.parse().unwrap(), cells)).collect()
}

fn file_cells(rt: &Runtime<Phantom>) -> BTreeMap<usize, BTreeMap<String, String>> {
    let rows = cells(rt, "files");
    rows.into_iter().map(|(i, cells)| (i.parse().unwrap(), cells)).collect()
}

/// The text drawn inside the element keyed `key`.
fn text(rt: &Runtime<Phantom>, key: &str) -> String {
    let elements = rt.elements();
    let at = elements.iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key:?}")).path.clone();
    elements.into_iter().filter(|e| e.path.starts_with(&at)).find_map(|e| e.text).unwrap_or_default()
}

/// 32 cases, or `PROPTEST_CASES`.
fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32);
    Config { cases, max_shrink_iters: 1024, ..Config::default() }
}

prop_state_machine! {
    #![proptest_config(config())]

    #[test]
    fn the_screen_agrees_with_the_app_and_the_service(sequential 1..20 => Screen);
}
