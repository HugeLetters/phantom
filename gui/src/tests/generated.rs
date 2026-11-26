//! Generated UI sequences against `MemoryService`: a person adding magnets (pasted, or typed into
//! the link field and sent with Return or Add), files dropped and links added in batches where some
//! fail, dismissing the error notices the status bar's popover tells of, choosing or cancelling the folder picker, selecting
//! rows, right-clicking them for their menu, double-clicking them, pausing, resuming, checking, removing, deleting, removing
//! every row in one press (⌘A ⌫, one undo step: #235), undoing (⌘Z, in the main window or in
//! Settings: #257) and setting file priorities (a file set Off and gone from disk is made again,
//! from nothing, once it's Normal: #276), right-clicking files for their menu, opening Settings (⌘,), closing the window in front
//! (⌘W), bringing either to the front and the main window back (the Dock), moving a torrent to
//! another folder from its row's menu (#24: Moving meanwhile, a clash answered in its dialog, one
//! that fails told as an error), while files
//! are deleted and put back outside Phantom, destinations go and come back, and time passes. Adds
//! take a while to be answered, as with a busy service (a .torrent URL longer), so the person
//! keeps typing while an answer is on its way. After every step the screen must agree with the
//! app's state and the service's answers must make sense together. The reference model knows
//! which rows there are and which one is selected, so steps can name them, and what the field
//! holds, which adds are on their way and when each is answered, and which windows show: ⌘W closes
//! only the one in front.
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

use super::{Desk, FRAME, Showing, desk, drawn_menu, enabled, on_screen, pick_choice_then, settle, with_an_album};
use crate::bar::machine::NoticeId;
use crate::model::{
    Phantom, add, details_torrent, link_problem, selected_files, selected_for, selected_torrents, shown_torrent, size_label,
};
use crate::notice::{Notice, Severity};
use crate::platform::{Show, Shown};
use crate::service::{AddRequest, Disk, DiskChange, Probe};
use crate::shortcuts;
use crate::windows::Which;

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

/// The torrent of three files the app starts with (`with_an_album`).
const ALBUM: TorrentId = 1;

/// How many frames a move takes (and a half, so it never ends on a frame's edge).
const MOVE_FRAMES: u32 = 47;

/// Where a move goes: a folder of its own, one holding a file of every torrent's already (a clash),
/// and one whose disk is full (it fails halfway).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Folder {
    Films,
    Held,
    Full,
}

impl Folder {
    fn path(self) -> &'static str {
        match self {
            Folder::Films => "/srv/films",
            Folder::Held => "/srv/held",
            Folder::Full => "/srv/full",
        }
    }
}

/// What the person does in a clash dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClashPick {
    UseExisting,
    Replace,
    Cancel,
    Escape,
    Reveal,
}

/// A move on its way: torrent `id` into `folder`, ending on frame `lands`, failing if it `fails`.
#[derive(Debug, Clone)]
struct Moving {
    id: TorrentId,
    folder: Folder,
    lands: u64,
    fails: bool,
}

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
    /// Open the popover on the n-th error notice (click the bar, step with ‹ ›), and click its ×:
    /// that notice is dismissed, and no other.
    DismissEntry {
        n: usize,
    },
    /// Escape, with no field focused: clears the selection (the popover is never open between steps).
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
    /// ⌘P: the command palette lists what the selection's row menu offers, then the app's commands
    /// (Undo named after what it takes back, while there is something); filtered by `query`'s
    /// letters, if any; then Escape closes it, nothing run, the selection kept.
    Palette {
        query: Option<&'static str>,
    },
    /// A click on a row's toggle: that torrent alone is paused, or resumed; the selection stays.
    Toggle(TorrentId),
    Check,
    /// ⌫: removes the selection at once, keeping its files.
    Remove,
    /// ⌘A then ⌫: every row removed in one press, one step on the undo stack (#235).
    RemoveAll,
    /// ⌘Z, in the main window or in Settings: the last Remove or Delete is undone, all of it: its
    /// rows back under their ids, the last selected; each refused while a row of its magnet is in
    /// the list.
    Undo(Which),
    /// ⌘, in the window in front: Settings opens, or comes to the front.
    OpenSettings,
    /// ⌘W: the window in front closes, the other staying.
    CloseWindow,
    /// A click on a window: it's in front.
    Front(Which),
    /// The Dock icon: the main window back, in front.
    Reopen,
    /// Click the priority of the file at this row of the pane, selecting the file first if
    /// `select`; the album's (torrent 1, three files), selected first while it's listed.
    SetFile {
        row: usize,
        select: bool,
    },
    /// Right-click the file at this row of the pane (the album's, selected first while it's listed):
    /// its menu offers what the selected files allow (Move to Trash only while one is on disk, #276),
    /// then Escape closes it.
    FileMenu {
        row: usize,
    },
    /// The album's file at this row, selected, set Off (if it isn't), deleted by hand, then set Normal
    /// again or put back (#276): Off and gone, it shows none of it and its menu has no Move to Trash;
    /// Normal again, it's made at its path and downloaded from nothing, not Missing files.
    OffGoneNormal {
        row: usize,
        /// Put back by hand instead of set Normal: it still has none of it, let go of, till a check.
        back: bool,
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
    /// In the clash dialog of a Move.
    Clash(ClashPick),
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
    /// Move to…, then the picker: a folder chosen, or (`None`) cancelled.
    Move(Option<Folder>),
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
    /// The disk has almost no room, so every torrent that runs is short of space.
    tight: bool,
}

impl Setup {
    /// Moves are asked for in it: its answers are never late and its adds never ask where, so the
    /// model knows to the frame when a move's answer lands, and no other picker is up.
    fn moves(&self) -> bool {
        self.default_destination && self.stale_frames == 0
    }
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
    /// The field's, and how it was sent: its answer may touch the field.
    field: Option<Via>,
    /// Into a folder chosen in the picker.
    chosen: bool,
    /// The frame its answer lands on.
    lands: u64,
}

/// How the field's add was sent.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Via {
    Return,
    /// A click on Add, which leaves the focus on the button.
    Button,
}

/// What an answer that landed says, for the screen to show from the frame it lands on.
#[derive(Debug, Clone, PartialEq)]
enum Said {
    /// Its row is in the list, and the status bar tells of it.
    Added(TorrentId),
    /// The batch is answered and some of it failed: its note shows.
    Noted(Note),
    /// Refused for want of a destination: the picker asks.
    AsksForFolder,
}

/// A note about a batch of adds, or a move that failed, as the screen shows it until it's
/// dismissed.
#[derive(Debug, Clone, PartialEq)]
struct Note {
    /// How many were in the batch.
    total: usize,
    /// Each failure: the file or link, and why.
    failed: Vec<(String, client::display::Reason)>,
    /// A move's, not adds'.
    moved: bool,
    /// An undo's that another torrent's files refused (#371): "Couldn't undo the Remove", the two
    /// names (the fake's own, which the model leaves to it) and Copy magnet link.
    undo: bool,
}

impl Note {
    fn title(&self) -> String {
        match (self.undo, self.moved) {
            (true, _) => "Couldn't undo the Remove".into(),
            (false, true) => client::display::commands_failed(Do::Move, 1, 1),
            (false, false) => client::display::adds_failed(self.failed.len(), self.total),
        }
    }

    /// An undo of a Remove that another torrent's files refused.
    fn undo_refused() -> Note {
        Note { total: 1, failed: Vec::new(), moved: false, undo: true }
    }

    /// A move of torrent `id` that failed, for `why`.
    fn move_failed(id: TorrentId, why: proto::NotMoved) -> Note {
        let name = if id == 0 { "a" } else { "Album" };
        Note::move_refused(id, proto::TorrentRefusal::NotMoved { name: name.into(), why })
    }

    /// A move of torrent `id` refused, for `why`.
    fn move_refused(id: TorrentId, why: proto::TorrentRefusal) -> Note {
        let name = if id == 0 { "a" } else { "Album" };
        let failure = client::Failure::Refused(proto::Refusal::Torrent { id, why });
        let (what, why) = client::display::command_failure(name, &failure);
        Note { total: 1, failed: vec![(what, why)], moved: true, undo: false }
    }

    /// What its entry draws: the title, then each failure and its reason.
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.title()];
        lines.extend(self.failed.iter().flat_map(|(what, why)| {
            let lines = std::iter::once(client::display::add_what(what)).chain(why.lines());
            lines.map(|l| l.text().to_owned()).collect::<Vec<_>>()
        }));
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
    /// Adds on their way, in the order they were sent.
    sent: Vec<Sent>,
    /// Adds refused for want of a destination, waiting for the picker; the first is asking.
    picker: VecDeque<Sent>,
    /// The error notices, oldest first: each batch with failures is one, until dismissed.
    errors: Vec<Note>,
    /// The one the last step dismissed, if it did.
    dismissed: Option<Note>,
    /// The answers that landed in the last step, in the order they landed.
    landed: Vec<Landed>,
    /// An add sent by a click on Add landed in the last step, the field still holding its link:
    /// the field takes the focus if it was on Add just before, and nothing moves it otherwise.
    refocus_due: bool,
    /// The frame the last step started on, and the one an answer being taken lands on.
    frame_before: u64,
    frame_of_landing: u64,
    /// The rows removed or deleted, oldest first, by press, as Undo brings them back: one press,
    /// one entry.
    removed: Vec<Vec<(TorrentId, Option<Link>)>>,
    /// What the last Undo did: the rows it brought back, how many it couldn't, and how many of those
    /// because another torrent has its files (#371), the rest because it was added again; nothing,
    /// with nothing to undo.
    undid: Option<(Vec<TorrentId>, usize, usize)>,
    /// The windows that show, and the one in front.
    shown: Showing,
    /// The move on its way, the clash dialog open (its torrent and folder), the folder each torrent
    /// moved to last, and the torrents whose destination is gone (which can't move).
    moving: Option<Moving>,
    clash: Option<(TorrentId, Folder)>,
    at: BTreeMap<TorrentId, Folder>,
    away: BTreeSet<TorrentId>,
}

impl Rows {
    /// The model before any step: two torrents' worth of room, nothing selected or on its way.
    fn start(setup: Setup) -> Rows {
        Rows {
            setup,
            rows: [(0, None), (1, None)].into(),
            selected: None,
            next_id: 2,
            frame: 0,
            field: String::new(),
            sent: Vec::new(),
            picker: VecDeque::new(),
            errors: Vec::new(),
            dismissed: None,
            landed: Vec::new(),
            refocus_due: false,
            frame_before: 0,
            frame_of_landing: 0,
            removed: Vec::new(),
            undid: None,
            shown: Showing { main: true, settings: false, front: Some(Which::Main) },
            moving: None,
            clash: None,
            at: BTreeMap::new(),
            away: BTreeSet::new(),
        }
    }

    /// Whether torrent `id` is one the person moves now: one of the album's two (a magnet's info
    /// comes on a clock the model doesn't follow), in a setup that moves, its destination there,
    /// and no move on its way.
    fn movable(&self, id: TorrentId) -> bool {
        self.setup.moves() && id < 2 && self.rows.contains_key(&id) && !self.away.contains(&id) && self.moving.is_none()
    }

    /// Torrent `id` sent into `folder`: it's there already (refused, and told, at once), it clashes
    /// (its dialog opens as the refusal lands, at once), or it moves, failing halfway into a full
    /// disk.
    fn move_into(&mut self, id: TorrentId, folder: Folder, chosen: bool) {
        if self.at.get(&id) == Some(&folder) {
            let note = Note::move_refused(id, proto::TorrentRefusal::AlreadyThere);
            self.errors.push(note.clone());
            self.landed.push(Landed { frame: self.frame, said: Said::Noted(note) });
            return;
        }
        if folder == Folder::Held && !chosen {
            self.clash = Some((id, folder));
            return;
        }
        let fails = folder == Folder::Full;
        let frames = if fails { ((MOVE_FRAMES as f64 + 0.5) / 2.0).ceil() as u64 } else { MOVE_FRAMES as u64 + 1 };
        self.moving = Some(Moving { id, folder, lands: self.frame + frames, fails });
    }

    /// The move on its way ends, if its frame has come: there, or told as an error.
    fn move_lands(&mut self) {
        let Some(m) = self.moving.take_if(|m| m.lands <= self.frame) else { return };
        match m.fails {
            true => {
                let note = Note::move_failed(m.id, proto::NotMoved::DiskFull);
                self.errors.push(note.clone());
                self.landed.push(Landed { frame: m.lands.max(self.frame_before), said: Said::Noted(note) });
            }
            false => drop(self.at.insert(m.id, m.folder)),
        }
    }

    /// Torrent `id` left the list: a move of it is cancelled first, which tells nothing.
    fn gone(&mut self, id: TorrentId) {
        self.moving.take_if(|m| m.id == id);
    }

    /// Rows there are or may be: so the list never outgrows the screen.
    fn room(&self, adding: usize) -> bool {
        let waiting = self.sent.iter().chain(&self.picker).map(|s| s.links.len()).sum::<usize>();
        self.rows.len() + waiting + adding <= MAX_ROWS
    }

    fn send(&mut self, links: Vec<Link>, field: Option<Via>, chosen: bool) {
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
            // A move that ends before it is told first.
            if self.moving.as_ref().is_some_and(|m| m.lands < sent.lands) {
                self.move_lands();
            }
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
        let holds = sent.field.is_some() && sent.links.iter().all(|l| self.field.trim() == l.text());
        let mut note = Note { total: 0, failed: Vec::new(), moved: false, undo: false };
        let button = sent.field == Some(Via::Button);
        for link in sent.links {
            // A failure, the field's too, goes to its batch's notice.
            let said = if link.fails() {
                note.total += 1;
                note.failed.push((link.named(), link.reason().into()));
                continue;
            } else {
                note.total += 1;
                // A click on Add left the focus on the button: the field may take it back.
                self.refocus_due |= holds && button;
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
        let setup = (frames(), prop_oneof![Just(0u32), Just(78)], any::<bool>(), frames(), frames(), any::<bool>(), any::<bool>())
            .prop_map(|(add_frames, more, default_destination, details_frames, stale_frames, paused, tight)| Setup {
                add_frames,
                torrent_url_frames: add_frames + more,
                default_destination,
                details_frames,
                stale_frames,
                paused,
                tight,
            });
        setup.prop_map(Rows::start).boxed()
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
        let dismiss = if listed > 0 { (0..listed).prop_map(|n| Action::DismissEntry { n }).boxed() } else { Just(Action::Escape).boxed() };
        let notes = prop_oneof![2 => dismiss, 2 => Just(Action::Escape)];
        let undo = if rows.removed.is_empty() { 1 } else { 4 };
        let which = prop_oneof![Just(Which::Main), Just(Which::Settings)];
        // Settings opened, then the main window in front again, so ⌘W meets either.
        let windows = prop_oneof![
            2 => Just(Action::OpenSettings),
            2 => Just(Action::CloseWindow),
            3 => Just(Action::Front(Which::Main)),
            1 => Just(Action::Front(Which::Settings)),
            1 => Just(Action::Reopen),
        ];
        let undo_in = which.prop_map(Action::Undo);
        // A clash dialog is modal: it's answered first, or time passes.
        if rows.clash.is_some() {
            let pick = prop_oneof![
                Just(ClashPick::UseExisting),
                Just(ClashPick::Replace),
                Just(ClashPick::Cancel),
                Just(ClashPick::Escape),
                Just(ClashPick::Reveal)
            ];
            return prop_oneof![4 => pick.prop_map(Action::Clash), 1 => waits].boxed();
        }
        // Keys and clicks reach the main window only in front; ⌘Z, Settings too.
        if rows.shown.front != Some(Which::Main) {
            return prop_oneof![4 => windows, 1 => waits, undo => undo_in].boxed();
        }
        if ids.is_empty() {
            return prop_oneof![1 => paste, 2 => field, 2 => batch, 1 => notes, 1 => waits, undo => undo_in, 2 => windows].boxed();
        }
        // Only while the album is listed: a step for it is rejected otherwise.
        let album = if rows.rows.contains_key(&ALBUM) { 2 } else { 1 };
        let id = proptest::sample::select(ids);
        let pick = prop_oneof![
            1 => Just(Pick::Command(Do::Pause)),
            1 => Just(Pick::Command(Do::Resume)),
            1 => Just(Pick::Command(Do::Check)),
            2 => Just(Pick::Command(Do::CancelCheck)),
            1 => Just(Pick::CopyMagnet),
            2 => Just(Pick::Open),
            1 => Just(Pick::Reveal),
            1 => any::<bool>().prop_map(|delete| Pick::Remove { delete }),
            3 => proptest::option::weighted(0.85, prop_oneof![Just(Folder::Films), Just(Folder::Held), Just(Folder::Full)]).prop_map(Pick::Move),
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
            2 => proptest::option::weighted(0.4, proptest::sample::select(vec!["re", "ren", "p", "del", "undo", "zz"]))
                .prop_map(|query| Action::Palette { query }),
            4 => id.clone().prop_map(Action::Toggle),
            2 => Just(Action::Check),
            1 => Just(Action::Remove),
            1 => Just(Action::RemoveAll),
            undo => undo_in,
            4 => windows,
            3 => (0..3usize, any::<bool>()).prop_map(|(row, select)| Action::SetFile { row, select }),
            2 => (0..3usize).prop_map(|row| Action::FileMenu { row }),
            2 => (id.clone(), files.clone()).prop_map(|(id, files)| Action::FilesDeleted { id, files }),
            album => (0..3usize, any::<bool>()).prop_map(|(row, back)| Action::OffGoneNormal { row, back }),
            2 => (id.clone(), files).prop_map(|(id, files)| Action::FilesPutBack { id, files }),
            1 => id.clone().prop_map(Action::DestinationGone),
            1 => id.prop_map(Action::DestinationBack),
            1 => Just(Action::Restart),
            3 => waits,
        ]
        .boxed()
    }

    fn preconditions(rows: &Rows, action: &Action) -> bool {
        let shows = |which| match which {
            Which::Main => rows.shown.main,
            Which::Settings => rows.shown.settings,
        };
        // What happens off the windows: time, the disk, the service.
        let elsewhere = matches!(
            action,
            Action::Wait(_)
                | Action::FilesDeleted { .. }
                | Action::FilesPutBack { .. }
                | Action::DestinationGone(_)
                | Action::DestinationBack(_)
                | Action::Restart
        );
        match action {
            Action::Clash(_) => rows.clash.is_some(),
            Action::Wait(_) => true,
            _ if rows.clash.is_some() => false,
            Action::Undo(which) => shows(*which) && rows.room(rows.removed.last().map_or(1, Vec::len)),
            // Not under a move: what waits behind it would be refused once it's back, which only the
            // service knows (the engine's race tests, and the fake's own, take that).
            Action::DestinationGone(id) if rows.moving.as_ref().is_some_and(|m| m.id == *id) => false,
            Action::OpenSettings | Action::CloseWindow => rows.shown.front.is_some(),
            Action::Front(which) => shows(*which),
            Action::Reopen => !rows.shown.main,
            // Keys and clicks reach the main window in front.
            _ if !elsewhere && rows.shown.front != Some(Which::Main) => false,
            Action::PasteMagnet(_) | Action::Send { .. } => rows.room(1),
            Action::AddAll(links) => rows.room(links.len()),
            Action::DismissEntry { n } => *n < rows.errors.len(),
            Action::OffGoneNormal { .. } => rows.rows.contains_key(&ALBUM),
            Action::Select(id)
            | Action::RowMenu { id, .. }
            | Action::DoubleClick(id)
            | Action::Toggle(id)
            | Action::FilesDeleted { id, .. }
            | Action::FilesPutBack { id, .. }
            | Action::DestinationGone(id)
            | Action::DestinationBack(id) => rows.rows.contains_key(id),
            _ => true,
        }
    }

    fn apply(mut rows: Rows, action: &Action) -> Rows {
        rows.landed.clear();
        rows.refocus_due = false;
        rows.dismissed = None;
        rows.frame_before = rows.frame;
        match action {
            Action::PasteMagnet(n) => rows.send(vec![Link::Magnet(*n)], None, false),
            Action::AddAll(links) => rows.send(links.clone(), None, false),
            // Exactly that notice goes, the others stay in order.
            Action::DismissEntry { n } => {
                rows.dismissed = Some(rows.errors.remove(*n));
            }
            Action::Escape => rows.selected = None,
            Action::Type(link) => rows.field = link.text(),
            Action::Send { button } => {
                if link_problem(&rows.field).is_none() {
                    let via = if *button { Via::Button } else { Via::Return };
                    rows.send(vec![Link::of(&rows.field).expect("a link from the pool")], Some(via), false);
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
            Action::SetFile { .. } | Action::FileMenu { .. } if rows.rows.contains_key(&ALBUM) => rows.selected = Some(ALBUM),
            Action::OffGoneNormal { .. } => rows.selected = Some(ALBUM),
            Action::RowMenu { id, leave, .. } => {
                rows.selected = Some(*id);
                if matches!(leave, Leave::Click(Pick::Remove { .. }) | Leave::Keys(Pick::Remove { .. }) | Leave::Hint(Pick::Remove { .. }))
                {
                    rows.selected = None;
                    rows.gone(*id);
                    rows.removed.extend(rows.rows.remove_entry(id).map(|row| vec![row]));
                }
                if let Leave::Click(Pick::Move(Some(folder)))
                | Leave::Keys(Pick::Move(Some(folder)))
                | Leave::Hint(Pick::Move(Some(folder))) = leave
                    && rows.movable(*id)
                {
                    rows.move_into(*id, *folder, false);
                }
            }
            Action::Remove => {
                if let Some(id) = rows.selected.take() {
                    rows.gone(id);
                    rows.removed.extend(rows.rows.remove_entry(&id).map(|row| vec![row]));
                }
            }
            Action::Clash(pick) => {
                let (id, folder) = rows.clash.expect("open");
                match pick {
                    ClashPick::UseExisting | ClashPick::Replace => {
                        rows.clash = None;
                        rows.move_into(id, folder, true);
                    }
                    ClashPick::Cancel | ClashPick::Escape => rows.clash = None,
                    ClashPick::Reveal => {}
                }
            }
            // Out of reach, it can't move.
            Action::DestinationGone(id) => drop(rows.away.insert(*id)),
            Action::DestinationBack(id) => drop(rows.away.remove(id)),
            Action::RemoveAll => {
                if let Some(m) = rows.moving.clone() {
                    rows.gone(m.id);
                }
                rows.selected = None;
                let all: Vec<(TorrentId, Option<Link>)> = std::mem::take(&mut rows.rows).into_iter().collect();
                if !all.is_empty() {
                    rows.removed.push(all);
                }
            }
            Action::OpenSettings => (rows.shown.settings, rows.shown.front) = (true, Some(Which::Settings)),
            Action::CloseWindow => {
                let shown = &mut rows.shown;
                match shown.front {
                    Some(Which::Main) => (shown.main, shown.front) = (false, shown.settings.then_some(Which::Settings)),
                    Some(Which::Settings) => (shown.settings, shown.front) = (false, shown.main.then_some(Which::Main)),
                    None => {}
                }
            }
            Action::Front(which) => rows.shown.front = Some(*which),
            Action::Reopen => (rows.shown.main, rows.shown.front) = (true, Some(Which::Main)),
            // All of the entry, each row on its own: refused while a row of its magnet is listed, and
            // while another row of the same file is: two torrents of one file have its place, whatever
            // their hashes (#371), the one put back first has it.
            Action::Undo(which) => {
                rows.shown.front = Some(*which);
                rows.undid = rows.removed.pop().map(|entry| {
                    let mut back: Vec<(TorrentId, Option<Link>)> = Vec::new();
                    let (mut refused, mut places) = (0, 0);
                    for (id, link) in entry {
                        let listed = rows.rows.values().any(|l| l.is_some() && *l == link && !link.is_some_and(Link::is_file));
                        let place =
                            link.is_some_and(Link::is_file) && rows.rows.values().chain(back.iter().map(|(_, l)| l)).any(|l| *l == link);
                        match (listed, place) {
                            (true, _) => refused += 1,
                            (false, true) => {
                                (refused, places) = (refused + 1, places + 1);
                                rows.errors.push(Note::undo_refused());
                            }
                            (false, false) => back.push((id, link)),
                        }
                    }
                    for (id, link) in &back {
                        rows.rows.insert(*id, *link);
                    }
                    let back: Vec<TorrentId> = back.into_iter().map(|(id, _)| id).collect();
                    if let Some(&last) = back.iter().max() {
                        rows.selected = Some(last);
                    }
                    (back, refused, places)
                });
            }
            Action::Wait(secs) => rows.frame += (secs / FRAME) as u64,
            _ => {}
        }
        rows.land();
        rows.move_lands();
        rows
    }
}

struct Sut {
    /// The model as it was before the step: what the person could see and pick then.
    before: Rows,
    d: Desk,
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
        let Setup { add_frames, torrent_url_frames, default_destination, details_frames, stale_frames, paused, tight } = rows.setup;
        let album = if paused { with_an_album().paused() } else { with_an_album() };
        let album = if tight { album.with_free_space(1) } else { album };
        let mut service = album
            .details_after(Setup::secs(details_frames))
            .stale_answers(Setup::secs(stale_frames))
            .rejecting(UNKNOWN)
            .adds_after(Setup::secs(add_frames), Setup::secs(torrent_url_frames))
            .with_default_destination(default_destination.then(|| "/srv/torrents".into()))
            .moves_take(Setup::secs(MOVE_FRAMES))
            .holding(Folder::Held.path())
            .move_failing(Folder::Full.path(), proto::NotMoved::DiskFull);
        for n in 0..FILES {
            service = service.rejecting(&Link::BadFile(n).text());
        }
        let (disk, probe) = (service.disk(), service.probe());
        let mut d = desk(service);
        let rt = &mut d.main;
        rt.poll = Some(crate::poll);
        let shown = Shown::default();
        rt.state.finder = Box::new(shown.clone());
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(rt);
        Sut { before: rows.clone(), d, disk, stopped: BTreeSet::new(), shown, probe }
    }

    fn apply(mut sut: Sut, rows: &Rows, action: Action) -> Sut {
        let rows_before = sut.before.clone();
        let mut resumed = Vec::new();
        // The windows, and ⌘Z in Settings: frames of both, the app clock held still.
        let d = &mut sut.d;
        let cmd = |c| KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, ..Default::default() } };
        match &action {
            Action::OpenSettings => d.key(cmd(',')),
            Action::CloseWindow => d.key(cmd('w')),
            Action::Front(which) => d.front(*which),
            Action::Reopen => d.reopen(),
            Action::Undo(Which::Settings) => {
                d.front(Which::Settings);
                d.settings.update(|_, tx| tx.blur());
                d.key(shortcuts::keys(crate::commands::InApp::Undo)[0].event());
                // Settings takes the key, and hands it to the main window.
                d.frame(0.0);
            }
            Action::Undo(Which::Main) => d.front(Which::Main),
            _ => {}
        }
        for _ in 0..2 {
            d.frame(0.0);
        }
        let rt = &mut sut.d.main;
        let focus_before = focused_path(rt);
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
            Action::DismissEntry { n } => {
                let before = error_ids(&rt.state);
                let id = before[*n];
                show_notice(rt, id);
                // A batch is one notice, its list as the view: what failed, and why.
                // Its buttons (Try again, Move to…, #367) are offered while their torrents are there:
                // `notice_actions` tests them; here, only what failed and why.
                let note = rows.dismissed.as_ref().expect("dismissed");
                let mut drawn = texts_under(rt, &format!("notice/{id}"));
                match note.undo {
                    true => {
                        assert_eq!(drawn.first(), Some(&note.title()), "the notice says what failed");
                        let apart = "Its files would clash with another torrent's:";
                        assert!(
                            drawn.iter().any(|l| l == apart) && drawn.last().map(String::as_str) == Some("Copy magnet link"),
                            "{drawn:?}"
                        );
                    }
                    false => {
                        while matches!(drawn.last().map(String::as_str), Some("Try again" | "Move to…")) {
                            drawn.pop();
                        }
                        assert_eq!(drawn, note.lines(), "the notice says what failed");
                    }
                }
                let (x, y) = rt.find("notice-dismiss").expect("× on an error notice");
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
                step(rt);
                let expected: Vec<NoticeId> = before.iter().copied().filter(|i| *i != id).collect();
                assert_eq!(error_ids(&rt.state), expected, "× dismisses exactly the notice shown, and no other");
                close_popover(rt);
            }
            Action::Palette { query } => {
                let selected = selected_torrents(&rt.state.table);
                rt.key(KeyEvent { key: Key::Command('p'), mods: Mods { cmd: true, ..Default::default() } });
                step(rt);
                assert!(rt.state.palette.is_open(), "⌘P opens the palette");
                for c in query.unwrap_or("").chars() {
                    rt.key(KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() });
                }
                step(rt);
                check_palette(&rt.state, &palette_labels(rt), query.unwrap_or(""));
                rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                step(rt);
                assert!(!rt.state.palette.is_open(), "Escape closes it");
                assert_eq!(selected_torrents(&rt.state.table), selected, "and only it");
            }
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
                        Pick::Command(Do::CancelCheck) => "Cancel check",
                        Pick::Command(_) => "Check",
                        Pick::CopyMagnet => "Copy magnet link",
                        Pick::Open => "Open",
                        Pick::Reveal => "Reveal in Finder",
                        Pick::Remove { delete: true } if offered.iter().any(|l| l.starts_with("Delete")) => "Delete",
                        Pick::Remove { .. } => "Remove",
                        // Picked only where the model follows the move.
                        Pick::Move(_) if !rows_before.movable(*id) => return None,
                        Pick::Move(_) => "Move to",
                    };
                    let found = offered.iter().find(|l| l.starts_with(wanted)).cloned();
                    if matches!(pick, Pick::Move(_)) {
                        assert!(found.is_some(), "Move to… offered for torrent {id}: {offered:?}");
                    }
                    found
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
                        if command == crate::commands::OnTorrents::Remove.into() {
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
                // Move to ▸ opened its folders: Other… asks for one.
                if label.as_deref().is_some_and(|l| l.starts_with("Move to")) && rt.state.menu.is_open() {
                    pick_in(rt, "row-menu", client::display::other_folder());
                }
                // Other… asks where, starting where the torrent is, and the person answers.
                if let (Some(label), Leave::Click(Pick::Move(folder)) | Leave::Keys(Pick::Move(folder)) | Leave::Hint(Pick::Move(folder))) =
                    (&label, leave)
                    && label.starts_with("Move to")
                {
                    let (title, directory) = super::picker(rt).expect("Move to… asks where");
                    let t = rt.state.torrents.iter().find(|t| t.id == *id).expect("listed");
                    assert_eq!(title, client::display::move_picker(t.display_name()), "the picker's title");
                    assert_eq!(directory.as_deref(), t.location.as_deref().and_then(|l| l.parent()), "the picker starts where it is");
                    rt.resolve_file_dialog(folder.map(|f| f.path().into())).unwrap();
                    // The service says which clash first, then the Move is sent: two answers, no time on the app clock.
                    for _ in 0..3 {
                        step(rt);
                    }
                }
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
            // The row's own toggle: it acts on that torrent alone and leaves the selection. A click
            // under a dialog does nothing; one on a disabled toggle (a torrent moving) does nothing too.
            Action::Toggle(id) => {
                let key = format!("rows/{id}/toggle");
                // A note open by Add covers the window: the person closes it first.
                close_notes(rt);
                let torrent = rt.state.torrents.iter().find(|t| t.id == *id).cloned();
                let toggle = torrent.as_ref().and_then(crate::model::row_toggle);
                if rows.clash.is_none() && toggle == Some(Do::Resume) && on_screen(rt, &key) {
                    resumed = vec![*id];
                }
                press(rt, &key);
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
            Action::OpenSettings | Action::CloseWindow | Action::Front(_) | Action::Reopen => {}
            // ⌘Z, with focus off the link field: answered at the next poll. In Settings, pressed
            // above.
            Action::Undo(which) => {
                if *which == Which::Main {
                    rt.update(|_, tx| tx.blur());
                    rt.key(shortcuts::keys(crate::commands::InApp::Undo)[0].event());
                }
                step(rt);
                let short = newest_told(&rt.state).map(|n| n.short.clone()).unwrap_or_default();
                match &rows.undid {
                    Some((back, refused, places)) if !back.is_empty() => {
                        assert!(short.to_lowercase().contains("back"), "the status bar says what came back: {short:?}");
                        // Told just before it, from the snapshot: each of a selection's that didn't, as it's
                        // listed again (#235).
                        let shorts: Vec<&str> = rt.state.bar.entries().iter().map(|e| e.notice.short.as_str()).collect();
                        let before = &shorts[shorts.len().saturating_sub(1 + refused)..shorts.len().saturating_sub(1)];
                        let again = before.iter().filter(|s| **s == "Already in the list").count();
                        let taken = before.iter().filter(|s| s.starts_with("Couldn't undo the ")).count();
                        assert!(
                            before.len() == *refused && (again, taken) == (refused - places, *places),
                            "the status bar says which didn't come back: {shorts:?}"
                        );
                        let last = *back.iter().max().expect("some back");
                        assert_eq!(selected_torrents(&rt.state.table), [last], "the last row that came back is selected");
                    }
                    // The fake refuses a torrent added again, which the app tells as such, and one whose
                    // place another has, which it tells as an error (#371).
                    Some((_, _, places)) => match places {
                        0 => {
                            assert_eq!(short, client::display::already_in_list(), "the status bar says it's in the list already: {short:?}")
                        }
                        _ => assert!(
                            short == client::display::already_in_list() || short.starts_with("Couldn't undo the "),
                            "the status bar says why: {short:?}"
                        ),
                    },
                    None => {}
                }
            }
            // ⌘A ⌫, with focus off the link field: every row removed at once, in one press.
            Action::RemoveAll => {
                if let Some(first) = rt.state.torrents.first().map(|t| t.id) {
                    press(rt, &format!("rows/{first}"));
                    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                    step(rt);
                }
                rt.update(|_, tx| tx.blur());
                rt.key(shortcuts::keys(crate::commands::OnTorrents::Remove)[0].event());
                step(rt);
                assert_eq!(rt.state.table.selected_len(), 0, "⌫ removes what is selected, and the selection goes with it");
            }
            // ⌫, with focus off the link field: the selection is removed at once, with no menu.
            Action::Remove => {
                rt.update(|_, tx| tx.blur());
                rt.key(shortcuts::keys(crate::commands::OnTorrents::Remove)[0].event());
                step(rt);
                assert_eq!(rt.state.table.selected_len(), 0, "⌫ removes what is selected, and the selection goes with it");
            }
            Action::SetFile { row, select } => {
                if rows.rows.contains_key(&ALBUM) {
                    press(rt, &format!("rows/{ALBUM}"));
                }
                let file = |rt: &Runtime<Phantom>| rt.state.pane.details.as_ref().and_then(|d| d.files.get(*row).cloned());
                let before = file(rt);
                if *select {
                    press(rt, &format!("files/viewport/rows/{row}"));
                }
                press(rt, &format!("files/viewport/rows/{row}/priority"));
                // Off and not on disk, set Normal: made at its path and downloaded from nothing, not
                // missing (#276), once the pane has the service's answer.
                let reachable = shown_torrent(&rt.state.table, &rt.state.torrents, &rt.state.pane)
                    .is_some_and(|t| !matches!(t.status, Status::DestinationUnavailable { .. } | Status::Checking | Status::Moving { .. }));
                if let (Some(was), Some(now)) = (before, file(rt))
                    && was.priority == proto::Priority::Off
                    && !was.on_disk
                    && now.priority == proto::Priority::Normal
                    && reachable
                {
                    assert!(now.on_disk && !now.missing && now.progress_bytes == 0, "set Normal, made again from nothing: {now:?}");
                }
            }
            Action::OffGoneNormal { row, back } => {
                press(rt, &format!("rows/{ALBUM}"));
                let file = |rt: &Runtime<Phantom>| {
                    rt.state.pane.details.as_ref().filter(|d| d.id == ALBUM).and_then(|d| d.files.get(*row).cloned())
                };
                // Its details may be on their way still: then there's no file to click.
                if file(rt).is_some_and(|f| f.priority == proto::Priority::Normal) {
                    press(rt, &format!("files/viewport/rows/{row}"));
                    press(rt, &format!("files/viewport/rows/{row}/priority"));
                }
                sut.disk.change(DiskChange::FilesDeleted { id: ALBUM, files: vec![*row] });
                step(rt);
                let reachable = |rt: &Runtime<Phantom>| {
                    // Not while it moves, which a priority waits for, shown done at once.
                    rt.state.torrents.iter().find(|t| t.id == ALBUM).is_some_and(|t| {
                        !matches!(t.status, Status::DestinationUnavailable { .. } | Status::Checking | Status::Moving { .. })
                    })
                };
                // Let go of only while nothing runs on it: a priority waits behind a move.
                let idle = reachable(rt);
                if let Some(gone) = file(rt).filter(|f| f.priority == proto::Priority::Off && !f.on_disk) {
                    assert_eq!(gone.progress_bytes, 0, "Off and gone, it has none of it: {gone:?}");
                    press(rt, &format!("files/viewport/rows/{row}"));
                    right_click(rt, &format!("files/viewport/rows/{row}/path"));
                    if let Some(drawn) = drawn_menu(rt, "file-menu") {
                        assert!(drawn.iter().flatten().all(|l| l != "Move to Trash" && l != "Open"), "nothing to move or open: {drawn:?}");
                        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                        step(rt);
                    }
                    if *back {
                        sut.disk.change(DiskChange::FilesPutBack { id: ALBUM, files: vec![*row] });
                        step(rt);
                        if let Some(now) = file(rt).filter(|f| f.on_disk && idle) {
                            assert_eq!(now.progress_bytes, 0, "put back by hand once let go of, it has none of it: {now:?}");
                        }
                    } else {
                        press(rt, &format!("files/viewport/rows/{row}/priority"));
                        if let Some(now) = file(rt).filter(|f| f.priority == proto::Priority::Normal)
                            && reachable(rt)
                        {
                            assert!(now.on_disk && !now.missing && now.progress_bytes == 0, "set Normal, made again from nothing: {now:?}");
                        }
                    }
                }
            }
            Action::FileMenu { row } => {
                if rows.rows.contains_key(&ALBUM) {
                    press(rt, &format!("rows/{ALBUM}"));
                }
                right_click(rt, &format!("files/viewport/rows/{row}/path"));
                if let Some(drawn) = drawn_menu(rt, "file-menu") {
                    let s = &rt.state;
                    let (details, selected) = (s.pane.details.as_ref().expect("a file menu of the pane's files"), selected_files(&s.pane));
                    let torrent = shown_torrent(&s.table, &s.torrents, &s.pane).expect("a torrent shown");
                    let there = details.files.iter().filter(|f| selected.contains(&f.index) && f.on_disk && !f.missing).count();
                    let trash = drawn.iter().flatten().any(|l| l == "Move to Trash");
                    assert_eq!(trash, there > 0 && torrent.actions.does(Do::TrashFiles), "Move to Trash while a selected file is on disk");
                    let open = drawn.iter().flatten().any(|l| l == "Open");
                    assert_eq!(open, selected.len() == 1 && there == 1 && torrent.actions.does(Do::Open), "Open for one file on disk");
                    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                    step(rt);
                    assert!(drawn_menu(rt, "file-menu").is_none(), "Escape closes the file menu");
                }
            }
            Action::FilesDeleted { id, files } => sut.disk.change(DiskChange::FilesDeleted { id: *id, files: files.clone() }),
            Action::FilesPutBack { id, files } => sut.disk.change(DiskChange::FilesPutBack { id: *id, files: files.clone() }),
            Action::DestinationGone(id) => {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                sut.disk.change(DiskChange::DestinationGone { id: *id, reason });
            }
            Action::DestinationBack(id) => sut.disk.change(DiskChange::DestinationBack { id: *id }),
            Action::Restart => sut.disk.change(DiskChange::Restarted),
            Action::Clash(pick) => {
                let before = sut.shown.all().len();
                match pick {
                    ClashPick::UseExisting => press(rt, "clash-use-existing"),
                    ClashPick::Replace => press(rt, "clash-replace"),
                    ClashPick::Cancel => press(rt, "clash-cancel"),
                    ClashPick::Escape => rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
                    ClashPick::Reveal => press(rt, "clash-reveal-0"),
                }
                step(rt);
                let revealed = sut.shown.all()[before..].to_vec();
                match pick {
                    ClashPick::Reveal => {
                        let file = rt.state.clash.as_ref().and_then(|c| c.files.first()).expect("the dialog stays open");
                        assert_eq!(revealed, vec![(file.to_path_buf(), Show::Reveal)], "Reveal in Finder shows the file in the way");
                    }
                    _ => assert!(revealed.is_empty(), "{pick:?} shows nothing in Finder"),
                }
            }
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
        // An add sent by a click on Add landed: the link field takes the focus if it was on Add
        // just before (a click on Add leaves it there, the add turns Add off), and leaves it where
        // the person put it otherwise, nowhere included. The click's own step lands with the
        // focus on Add. An add that selects another torrent switches the pane: the focus in it
        // stays if what it was on stays, else goes to the torrent list, not to the field.
        if rows.refocus_due {
            let on_add = |path: &Option<String>| path.as_deref().is_some_and(|p| p.split('/').any(|part| part == "add"));
            let focused = focused_path(rt);
            if matches!(action, Action::Send { button: true }) || on_add(&focus_before) {
                assert!(
                    focused.as_deref().is_some_and(|p| p.split('/').any(|part| part == "magnet-field")),
                    "the focus after a click on Add landed is in the link field, not {focused:?} (it was {focus_before:?})"
                );
            } else if rows.selected != rows_before.selected && focus_before.as_deref().is_some_and(|p| p.contains("/details/")) {
                assert!(
                    focused == focus_before || focused.as_deref().is_some_and(in_the_list),
                    "the focus in the pane an add switched stays or goes to the list, not to {focused:?} (it was {focus_before:?})"
                );
            } else {
                assert_eq!(focused, focus_before, "the focus stays where it was put when an add lands");
            }
        }
        if !matches!(action, Action::Wait(_)) {
            check_landed(&sut.d.main, rows, rows.frame);
        }

        let ids: Vec<TorrentId> = sut.d.main.state.torrents.iter().map(|t| t.id).collect();
        assert_eq!(ids, rows.rows.keys().copied().collect::<Vec<_>>(), "the rows after {action:?}");
        assert_eq!(selected_torrents(&sut.d.main.state.table), Vec::from_iter(rows.selected), "the selection after {action:?}");
        // Only Resume starts a torrent that's stopped, and only one it's given.
        sut.stopped.retain(|id| !resumed.contains(id) && ids.contains(id));
        sut.before = rows.clone();
        for t in &sut.d.main.state.torrents {
            if sut.stopped.contains(&t.id) {
                assert!(!matches!(t.status, Status::Downloading | Status::Seeding), "{action:?} started stopped torrent {}", t.id);
            }
            if matches!(t.status, Status::Paused | Status::Done { .. }) {
                sut.stopped.insert(t.id);
            }
        }
        sut
    }

    fn check_invariants(sut: &Sut, rows: &Rows) {
        assert_eq!(sut.d.shown(), rows.shown, "the windows that show, and the one in front");
        assert_eq!(sut.d.settings.state.sheet.is_some(), rows.shown.settings, "Settings is open while it shows");
        let rt = &sut.d.main;
        let s = &rt.state;

        // The field loses what was typed only to its own link's add.
        assert_eq!(s.magnet.text, rows.field, "the field's text");
        // Nothing under a dialog takes a press: a clash's is modal.
        let modal = rows.clash.is_some();
        assert_eq!(enabled(rt, "add"), !modal && link_problem(&s.magnet.text).is_none(), "Add is on for a link, under no dialog");
        assert_eq!(rt.file_dialog().is_some(), !rows.picker.is_empty(), "the picker asks while an add waits for a folder");
        // The clash dialog is open exactly while the model says, over its torrent and folder.
        assert_eq!(
            s.clash.as_ref().map(|c| (c.id, c.to.to_path_buf())),
            rows.clash.map(|(id, f)| (id, f.path().into())),
            "the clash dialog"
        );
        assert_eq!(rt.find("clash-dialog").is_some(), rows.clash.is_some(), "the clash dialog drawn while it's open");
        // A torrent moved is where it went, once the move's over.
        for (id, folder) in &rows.at {
            if let Some(t) = s.torrents.iter().find(|t| t.id == *id).filter(|_| rows.moving.as_ref().is_none_or(|m| m.id != *id)) {
                let parent = t.location.as_deref().and_then(|l| l.parent());
                assert_eq!(parent, Some(std::path::Path::new(folder.path())), "torrent {id} moved into {folder:?}: {t:?}");
            }
        }
        // The status bar tells of no failed add but as an error.
        assert!(
            newest_told(s).is_none_or(|n| !tells_of_failure(&n.short)),
            "the status bar tells of no add's failure but as an error: {:?}",
            newest_told(s)
        );
        // Every failure from every fallback source is exactly one error notice in the status bar
        // until dismissed, one per batch, oldest first; nothing else is an error.
        let errors: Vec<&crate::bar::machine::Entry> = s.bar.entries().iter().filter(|e| e.notice.severity == Severity::Error).collect();
        assert_eq!(
            errors.iter().map(|e| e.notice.short.clone()).collect::<Vec<_>>(),
            rows.errors.iter().map(Note::title).collect::<Vec<_>>(),
            "the error notices"
        );
        // A disk short of space is in its rows, not the status bar: every torrent that still needs
        // space on it has the ⚠ and, on hover, the disk's whole picture, and no other row has.
        let expected: Vec<(TorrentId, String)> = s
            .volumes
            .iter()
            .filter(|v| v.short())
            .flat_map(|v| v.needs.iter().filter_map(|n| Some((n.id, client::display::short_on_space(v, n.id)?))))
            .collect();
        let mut drawn: Vec<TorrentId> = rt
            .elements()
            .iter()
            .filter(|e| e.role == "icon")
            .filter_map(|e| e.key.as_deref()?.strip_prefix("short-on-space/")?.parse().ok())
            .collect();
        drawn.sort();
        let mut want: Vec<TorrentId> = expected.iter().map(|(id, _)| *id).collect();
        want.sort();
        assert_eq!(drawn, want, "the ⚠ on every torrent that needs space on a disk that's short");
        let mut hovers: Vec<String> =
            rt.elements().iter().filter_map(|e| e.help.clone()).filter(|h| h.contains(" is short on space: ")).collect();
        hovers.sort();
        let mut said: Vec<String> = expected.into_iter().map(|(_, why)| why).collect();
        said.sort();
        assert_eq!(hovers, said, "each says the disk's picture");
        // A step ends with the popover closed, as the model has it.
        assert!(!s.bar.is_open() && rt.find("notices-panel").is_none(), "the popover is closed");
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
            // Its progress is what it has, but while it moves: what's copied of what it copies (#24).
            let fraction = match (&t.status, t.content) {
                (Status::Moving { done, total }, _) if *total > 0 => Some((*done as f64 / *total as f64) as f32),
                (_, Some(c)) => Some(c.progress()),
                (_, None) => None,
            };
            if let Some(f) = fraction {
                let shown = cells[&t.id].get("progress").cloned();
                assert_eq!(shown, Some(format!("{:.0}%", (f * 100.0).floor())), "torrent {}'s progress, as {:?}", t.id, t.status);
            }
            // Its sizes are known exactly once its info is (#98).
            assert_eq!(t.content.is_none(), t.status == Status::FetchingInfo, "torrent {}: {t:?}", t.id);
            if let Some(c) = t.content {
                assert!(c.progress_bytes <= c.size_bytes && c.size_bytes <= c.full_size_bytes, "torrent {}: {t:?}", t.id);
                assert_eq!(cells[&t.id].get("size"), Some(&client::display::bytes(c.size_bytes)), "torrent {}'s size", t.id);
            }
        }

        assert!(!s.menu.is_open(), "every step leaves its menu closed");
        assert!(drawn_menu(rt, "row-menu").is_none(), "no menu is drawn");

        // A row's toggle says what a click does: Pause while the service lists it, else Resume; where
        // it lists neither it's the same button disabled, its tip the status word.
        for t in &s.torrents {
            let button = rt
                .elements()
                .into_iter()
                .find(|e| !e.exiting && e.key.as_deref() == Some("toggle-button") && e.path.contains(&format!("/rows/{}/", t.id)));
            if rt.find(&format!("rows/{}", t.id)).is_some() {
                let button = button.unwrap_or_else(|| panic!("torrent {} has a toggle", t.id));
                let icon = crate::model::toggle_icon(t);
                assert_eq!(button.label.as_deref(), Some(if icon == Do::Pause { "Pause" } else { "Resume" }), "torrent {}'s toggle", t.id);
                let off = crate::model::row_toggle(t).is_none();
                assert_eq!(
                    button.help.as_deref() == Some(client::display::status(&t.status)),
                    off,
                    "torrent {}: disabled only when it can't act",
                    t.id
                );
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
            // The name line's right side: the Size first, then what room leaves of the rest, all
            // of it on hover when some gave way.
            let side = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("details-side")).expect("a side");
            let shown = text(rt, "details-side");
            let all = side.help.clone().unwrap_or_else(|| shown.clone());
            assert!(all.starts_with(&size_label(t)) && all.starts_with(&shown), "the pane's size is the list's: {shown:?} of {all:?}");
        }
        let file_cells = file_cells(rt);
        assert_eq!(file_cells.len(), details.files.len(), "a row for every file");
        for f in &details.files {
            let shown = file_cells[&f.index].get("progress").cloned();
            assert_eq!(shown.as_deref() == Some("Missing"), f.missing, "file {}: {f:?}", f.index);
            // One set Off and not on disk has none of it (#276), whatever it had.
            if f.priority == proto::Priority::Off && !f.on_disk {
                assert_eq!(f.progress_bytes, 0, "file {} is Off and not on disk: {f:?}", f.index);
            }
            if !f.missing {
                assert_eq!(shown, Some(format!("{:.0}%", (f.progress() * 100.0).floor())), "file {}'s progress: {f:?}", f.index);
            }
        }
    }
}

/// The list and the pane are of the latest snapshot the service streamed for what the pane
/// watches: never an older one, nor one from before a restart, however late a command's own lands.
/// And the pane never stays blank: a selected torrent whose details came shows them.
fn on_the_latest(rt: &Runtime<Phantom>, probe: &Probe) {
    let s = &rt.state;
    let Some(latest) = probe.latest() else { return };
    // Only for a snapshot built for what the pane asks for now, in the order it asked: the service
    // answers a changed request at once, even one of the same torrents in another order, so a
    // snapshot for the old order is about to be followed by one the pane's state is decided on
    // (a frame, when it was applied just before the selection moved: Switching to a torrent whose
    // details that snapshot held, and the pane catches up on the next one).
    if latest.watching != s.pane.watching {
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
            Said::Noted(note) => {
                // The error is in the status bar from the frame it lands, in the error colour's severity,
                // with the popover still closed; its list is what a click on the bar shows.
                assert!(!s.bar.is_open(), "the error of {note:?} didn't open the popover at frame {frame}");
                assert!(
                    s.bar.entries().iter().any(|e| e.notice.severity == Severity::Error && e.notice.short == note.title()),
                    "{note:?} is in the status bar from frame {frame}"
                );
            }
            Said::AsksForFolder => {}
        }
        if Some(i) == last_added {
            let short = newest_told(s).map(|n| n.short.as_str());
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

/// The newest notice that isn't an error: what the status bar tells of an add, an undo.
fn newest_told(s: &Phantom) -> Option<&Notice> {
    s.bar.entries().iter().rev().map(|e| &e.notice).find(|n| n.severity != Severity::Error)
}

/// The ids of the error notices, oldest first.
fn error_ids(s: &Phantom) -> Vec<NoticeId> {
    s.bar.entries().iter().filter(|e| e.notice.severity == Severity::Error).map(|e| e.id).collect()
}

/// Closes the popover with Escape, off the fields: as often as it takes.
pub(super) fn close_popover(rt: &mut Runtime<Phantom>) {
    for _ in 0..3 {
        if !rt.state.bar.is_open() {
            return;
        }
        rt.update(|_, tx| tx.blur());
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        step(rt);
    }
    assert!(!rt.state.bar.is_open(), "Escape closes the popover");
}

/// Shows notice `id` in the popover: a click on the bar if it's closed, then ‹ or › until it's
/// the one shown.
pub(super) fn show_notice(rt: &mut Runtime<Phantom>, id: NoticeId) {
    if !rt.state.bar.is_open() {
        // A note open by Add takes the first click: the person closes it first.
        close_notes(rt);
        let (x, y) = rt.find("notice-text").expect("the bar shows a notice");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
    for _ in 0..=rt.state.bar.entries().len() {
        let (entry, at, total) = rt.state.bar.shown().expect("the popover is open");
        if entry.id == id {
            return;
        }
        let to = rt.state.bar.entries().iter().rev().position(|e| e.id == id).expect("the notice is there");
        assert!(to < total);
        let (x, y) = rt.find(if to < at { "notice-newer" } else { "notice-older" }).expect("‹ and › with several notices");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
    panic!("notice {id} is never shown");
}

/// What each error notice says, oldest first: its short line, then each failure. Looks through
/// the popover, and leaves it as it was found.
pub(super) fn error_lines(rt: &mut Runtime<Phantom>) -> Vec<Vec<String>> {
    let was = rt.state.bar.shown().map(|(e, _, _)| e.id);
    let lines = error_ids(&rt.state)
        .into_iter()
        .map(|id| {
            show_notice(rt, id);
            texts_under(rt, &format!("notice/{id}"))
        })
        .collect();
    match was {
        Some(id) if rt.state.bar.entries().iter().any(|e| e.id == id) => show_notice(rt, id),
        _ => close_popover(rt),
    }
    lines
}

/// An element's rect, x, y, width, height.
pub(super) fn rect_of<S: 'static>(rt: &Runtime<S>, key: &str) -> Option<[f32; 4]> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).map(|e| e.rect)
}

pub(super) fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// The texts drawn inside the element keyed `key`, in order.
pub(super) fn texts_under(rt: &Runtime<Phantom>, key: &str) -> Vec<String> {
    let elements = rt.elements();
    let Some(at) = elements.iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.path.clone()) else { return Vec::new() };
    elements.into_iter().filter(|e| e.path.starts_with(&at) && !e.exiting).filter_map(|e| e.text).collect()
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
/// Add takes the first click anywhere (it closes), so the person closes it first.
pub(crate) fn press(rt: &mut Runtime<Phantom>, key: &str) {
    close_notes(rt);
    if let Some((x, y)) = rt.find(key) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
}

/// Closes the note open by Add, as clicks elsewhere do: in the window's corner,
/// which nothing covers. A click closes the topmost note's scrim only,
/// so with several open it takes one click for each.
fn close_notes(rt: &mut Runtime<Phantom>) {
    // The clash dialog is modal and covers the corner: a note that lands while it's open stays under it.
    if rt.state.clash.is_some() {
        return;
    }
    const SCRIMS: [&str; 1] = ["add-note-scrim"];
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
const MENU_ITEMS: [(&str, Option<Do>, u8); 12] = [
    ("Resume", Some(Do::Resume), 0),
    ("Pause", Some(Do::Pause), 0),
    ("Check", Some(Do::Check), 0),
    ("Cancel check", Some(Do::CancelCheck), 0),
    ("Cancel move", Some(Do::CancelMove), 0),
    ("Open", Some(Do::Open), 1),
    ("Reveal in Finder", Some(Do::Reveal), 1),
    ("Move to", Some(Do::Move), 1),
    ("Rename…", Some(Do::Rename), 1),
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

/// What the open palette lists, in order.
fn palette_labels(rt: &Runtime<Phantom>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "option" && e.path.contains("palette-panel"))
        .filter_map(|e| e.label)
        .collect()
}

/// Checks the palette lists exactly what it must for `query`: of the selected torrent's row menu
/// items (as `check_row_menu` has them), the selected files' commands and the app's, those whose label has the query's
/// letters in order, each once.
fn check_palette(s: &Phantom, listed: &[String], query: &str) {
    let mut offered: Vec<String> = Vec::new();
    if let Some(t) = details_torrent(&s.table, &s.torrents) {
        let delete = t.actions.does(Do::Delete);
        for (label, action, _) in MENU_ITEMS {
            if action.is_none_or(|a| t.actions.does(a)) && (action.is_some_and(|a| matches!(a, Do::Open | Do::Reveal)) <= s.target_there) {
                match label {
                    "Remove" => offered.push(client::display::remove_choice(1, delete)),
                    "Delete" => offered.push(client::display::delete_choice(1)),
                    // Move to ▸: Move to… (Other…), and its folders, each by name.
                    "Move to" => {
                        offered.push(client::display::move_to().into());
                        let folders = crate::model::recent::offered(s);
                        offered.extend(crate::model::recent::names(&folders).iter().map(|n| client::display::move_to_folder(n)));
                    }
                    other => offered.push(other.into()),
                }
            }
        }
    }
    // With files selected in the Files list: the file menu's items, worded as it words them.
    // Worked out here from the pane's files and the torrent's actions, not from `commands`.
    let torrent = details_torrent(&s.table, &s.torrents);
    if let (Some(t), Some(details)) = (torrent, s.pane.details.as_ref()) {
        let selected = crate::model::selected_files(&s.pane);
        let files: Vec<&proto::File> = details.files.iter().filter(|f| t.id == details.id && selected.contains(&f.index)).collect();
        let on_disk = |f: &&proto::File| f.on_disk && !f.missing;
        let only_on_disk = s.file_there && matches!(files.as_slice(), [f] if on_disk(f));
        if !files.is_empty() {
            let names = [
                (only_on_disk && t.actions.does(Do::Open), "Open"),
                (only_on_disk && t.actions.does(Do::Reveal), "Reveal in Finder"),
                (t.actions.does(Do::SetPriority), "Priority: Normal"),
                (t.actions.does(Do::SetPriority), "Priority: Off"),
                (files.iter().any(on_disk) && t.actions.does(Do::TrashFiles), "Move to Trash"),
            ];
            offered.extend(names.into_iter().filter(|(shown, _)| *shown).map(|(_, name)| name.to_owned()));
        }
    }
    offered.push("Add file…".into());
    if s.changes.state.top().is_some() {
        offered.push(crate::changes::menu_title(s));
    }
    let mut want: Vec<&String> = offered.iter().filter(|l| crate::model::palette::score(query, l).is_some()).collect();
    let mut got: Vec<&String> = listed.iter().collect();
    want.sort();
    got.sort();
    assert_eq!(got, want, "the palette for {query:?}");
}

/// Each item of the open menu `menu`, by label, with the shortcut it shows, if any: the texts the
/// item draws besides its label.
pub(super) fn menu_hints(rt: &Runtime<Phantom>, menu: &str) -> Vec<(String, Option<String>)> {
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
fn item_shortcut(label: &str) -> Option<crate::commands::OnTorrents> {
    use crate::commands::OnTorrents;
    match label {
        "Open" => Some(OnTorrents::Open),
        "Reveal in Finder" => Some(OnTorrents::Reveal),
        "Copy magnet link" => Some(OnTorrents::CopyMagnet),
        l if l.starts_with("Remove") => Some(OnTorrents::Remove),
        l if l.starts_with("Delete") => Some(OnTorrents::Delete),
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

    /// [`Screen`] in setups that always move (#264's review): its answers never late, its adds
    /// never asking where, and Move to… picked more often, so every case moves.
    #[test]
    fn the_screen_agrees_with_the_app_and_the_service_as_torrents_move(sequential 1..20 => Moves);
}

/// [`Screen`], every case in a setup that moves ([`Setup::moves`]), Move to… a third of its steps.
struct Moves;

impl ReferenceStateMachine for Moves {
    type State = Rows;
    type Transition = Action;

    fn init_state() -> BoxedStrategy<Rows> {
        Screen::init_state().prop_map(|rows| Rows::start(Setup { default_destination: true, stale_frames: 0, ..rows.setup })).boxed()
    }

    fn transitions(rows: &Rows) -> BoxedStrategy<Action> {
        let ids: Vec<TorrentId> = rows.rows.keys().copied().filter(|id| rows.movable(*id)).collect();
        if ids.is_empty() || rows.clash.is_some() || rows.shown.front != Some(Which::Main) {
            return Screen::transitions(rows);
        }
        let folder = prop_oneof![Just(Folder::Films), Just(Folder::Held), Just(Folder::Full)];
        let pick = proptest::option::weighted(0.9, folder).prop_map(Pick::Move);
        let leave = prop_oneof![pick.clone().prop_map(Leave::Click), pick.clone().prop_map(Leave::Keys), pick.prop_map(Leave::Hint)];
        let moves =
            (proptest::sample::select(ids), leave, any::<bool>()).prop_map(|(id, leave, small)| Action::RowMenu { id, leave, small });
        prop_oneof![1 => moves, 2 => Screen::transitions(rows)].boxed()
    }

    fn preconditions(rows: &Rows, action: &Action) -> bool {
        Screen::preconditions(rows, action)
    }

    fn apply(rows: Rows, action: &Action) -> Rows {
        <Screen as ReferenceStateMachine>::apply(rows, action)
    }
}

impl StateMachineTest for Moves {
    type SystemUnderTest = Sut;
    type Reference = Moves;

    fn init_test(rows: &Rows) -> Sut {
        <Screen as StateMachineTest>::init_test(rows)
    }

    fn apply(sut: Sut, rows: &Rows, action: Action) -> Sut {
        <Screen as StateMachineTest>::apply(sut, rows, action)
    }

    fn check_invariants(sut: &Sut, rows: &Rows) {
        <Screen as StateMachineTest>::check_invariants(sut, rows)
    }
}

/// An example of what #236 found on master. In one frame the app applied a snapshot built for
/// watching [3, 0], with both torrents' details, and the selection moved to 3: the pane went
/// Switching { shown: 0, wanted: 3 } and asked for [0, 3]. For that frame it showed 0 while the
/// latest snapshot had 3's details; the service's answer to the new request put it right on the
/// next. So the claim holds only for a snapshot built for the request the pane has now.
#[test]
fn the_pane_shows_the_selected_torrent_once_its_details_came() {
    let setup = Setup {
        add_frames: 47,
        torrent_url_frames: 47,
        default_destination: true,
        details_frames: 47,
        stale_frames: 47,
        paused: true,
        tight: false,
    };
    let steps = [
        Action::AddAll(vec![Link::Magnet(1), Link::BadFile(0)]),
        Action::PasteMagnet(0),
        Action::Wait(1.5),
        Action::AddAll(vec![Link::Magnet(0), Link::File(0)]),
        Action::Wait(2.5),
        Action::Select(0),
        Action::PasteMagnet(0),
        Action::Wait(1.5),
    ];
    replay(setup, steps);
}

/// #148, found by the generated test: the fake sent two different snapshots for one
/// `(epoch, version, watching)`. A command's own snapshot, taken before the slow details came,
/// landed after the stream's with them, and the app took the equal version as current: the pane
/// went back to waiting and, the torrents being paused, stayed blank. The fake asserts the
/// protocol on every snapshot it sends, so the case fails if it breaks it again.
#[test]
fn the_fake_sends_one_snapshot_for_each_epoch_version_and_watching() {
    let setup = Setup {
        add_frames: 0,
        torrent_url_frames: 0,
        default_destination: false,
        details_frames: 47,
        stale_frames: 47,
        paused: true,
        tight: false,
    };
    let remove = Leave::Click(Pick::Remove { delete: false });
    let steps = [
        Action::RowMenu { id: 0, leave: remove, small: false },
        Action::Select(1),
        Action::Toggle(1),
        Action::Toggle(1),
        Action::Wait(1.5),
    ];
    replay(setup, steps);
}

/// #362, found by the generated test: a click on Add, then Enter sends the same link again; the
/// album's file list takes the focus, and both adds land. The second selects the row the first
/// added, so the pane switches and the focus on the album's files goes (as it does after Enter
/// alone). The model had it stay where it was put; it goes to the torrent list (#364).
#[test]
fn an_add_that_selects_a_listed_torrent_takes_the_focus_from_the_files_it_hides() {
    let setup = Setup {
        add_frames: 0,
        torrent_url_frames: 78,
        default_destination: true,
        details_frames: 0,
        stale_frames: 0,
        paused: false,
        tight: false,
    };
    let steps = [
        Action::Type(Link::TorrentUrl),
        Action::Send { button: true },
        Action::Send { button: false },
        Action::OffGoneNormal { row: 0, back: false },
        Action::Wait(1.5),
    ];
    replay(setup, steps);
}

/// #364: the focus the add took from the files goes to the torrent list, on the row just
/// selected: an arrow key moves the selection from it. (The replay's own checks hold the focus
/// to the list; this one presses the key.)
#[test]
fn an_add_that_hides_the_focused_files_leaves_the_focus_on_the_list_row_it_selected() {
    let setup = Setup {
        add_frames: 0,
        torrent_url_frames: 78,
        default_destination: true,
        details_frames: 0,
        stale_frames: 0,
        paused: false,
        tight: false,
    };
    let steps = [
        Action::Type(Link::TorrentUrl),
        Action::Send { button: true },
        Action::Send { button: false },
        Action::OffGoneNormal { row: 0, back: false },
        Action::Wait(1.5),
    ];
    let (_, mut sut) = replay(setup, steps);
    let rt = &mut sut.d.main;
    let before = selected_torrents(&rt.state.table);
    for key in [Key::Down, Key::Up] {
        rt.key(KeyEvent { key, mods: Mods::default() });
        step(rt);
        let after = selected_torrents(&rt.state.table);
        if after != before {
            assert_eq!(after.len(), 1, "an arrow key selects one row");
            return;
        }
    }
    panic!("no arrow key moved the list's selection: the focus is {:?}", focused_path(rt));
}

/// Runs `steps` in a screen set up as `setup`, the model and the checks after each step as in the
/// generated tests.
fn replay(setup: Setup, steps: impl IntoIterator<Item = Action>) -> (Rows, Sut) {
    let mut rows = Rows::start(setup);
    let mut sut = <Screen as StateMachineTest>::init_test(&rows);
    for action in steps {
        assert!(<Screen as ReferenceStateMachine>::preconditions(&rows, &action), "{action:?}");
        rows = <Screen as ReferenceStateMachine>::apply(rows, &action);
        sut = <Screen as StateMachineTest>::apply(sut, &rows, action);
        <Screen as StateMachineTest>::check_invariants(&sut, &rows);
    }
    (rows, sut)
}

/// Whether `path` is the torrent list's (the focus stop is the list itself, its selected row
/// shown as the one).
fn in_the_list(path: &str) -> bool {
    path.split('/').any(|part| part == "table")
}

/// The path of the element focused now, if any.
fn focused_path(rt: &Runtime<Phantom>) -> Option<String> {
    rt.elements().into_iter().find(|e| e.focused && !e.exiting).map(|e| e.path)
}
