//! Generated UI sequences against `MemoryService`: a person adding magnets (pasted, or typed into
//! the link field and sent with Return or Add), choosing or cancelling the folder picker, selecting
//! rows, pausing, resuming, checking, removing, deleting and setting file priorities, while files
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
use proto::{Action as Do, Status, TorrentId, Unavailable};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{FRAME, app, enabled, settle, wait, with_an_album};
use crate::model::{Phantom, details_torrent, link_problem, selected_for, selected_torrents, shown_torrent, size_label};
use crate::service::{Disk, DiskChange};

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
        }
    }

    fn of(text: &str) -> Option<Link> {
        let links = (0..MAGNETS).map(Link::Magnet).chain([Link::TorrentUrl, Link::Unknown, Link::NotALink, Link::NoInfoHash]);
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
    /// Chooses a folder in the picker, or cancels it.
    Choose(bool),
    Select(TorrentId),
    Pause,
    Resume,
    Check,
    /// Remove…, then Delete if `delete` and it's offered, else Remove.
    Remove {
        delete: bool,
    },
    /// Select the file at this row of the pane, then set it Off or Normal.
    SetFile {
        row: usize,
        off: bool,
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
    Wait(f64),
}

/// How the fake service behaves in a case: how many frames an add takes to be answered (that
/// and a half, so an answer never lands on a frame's edge; 0 for the next poll), a .torrent URL's
/// at least as long, whether there's a default destination, and how many frames a selected
/// torrent's details take to come.
#[derive(Debug, Clone, Copy)]
struct Setup {
    add_frames: u32,
    torrent_url_frames: u32,
    default_destination: bool,
    details_frames: u32,
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
    link: Link,
    /// The field's: its answer may touch the field.
    field: bool,
    /// Into a folder chosen in the picker.
    chosen: bool,
    /// The frame its answer lands on.
    lands: u64,
}

/// What the last answer of a step said, for the screen to show.
#[derive(Debug, Clone, PartialEq)]
enum Said {
    Added,
    FailedByAdd(String),
    FailedInStatusBar,
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
    /// The last answer that landed in the last step.
    said: Option<Said>,
}

impl Rows {
    /// Rows there are or may be: so the list never outgrows the screen.
    fn room(&self) -> bool {
        self.rows.len() + self.sent.len() + self.picker.len() < MAX_ROWS
    }

    fn send(&mut self, link: Link, field: bool, chosen: bool) {
        let lands = self.frame + self.setup.takes(link);
        self.sent.push(Sent { link, field, chosen, lands });
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
            self.answer(sent);
        }
    }

    fn answer(&mut self, sent: Sent) {
        let text = sent.link.text();
        let holds = sent.field && self.field.trim() == text;
        if !sent.chosen && !self.setup.default_destination {
            self.picker.push_back(sent);
            return;
        }
        if sent.link == Link::Unknown {
            self.said = Some(if holds {
                self.error_by_add = Some(text);
                Said::FailedByAdd(self.field.clone())
            } else {
                Said::FailedInStatusBar
            });
            return;
        }
        match self.rows.iter().find(|(_, l)| **l == Some(sent.link)) {
            // Already in the list: selected instead.
            Some((&id, _)) => self.selected = Some(id),
            None => {
                self.rows.insert(self.next_id, Some(sent.link));
                self.next_id += 1;
            }
        }
        if holds {
            self.field.clear();
        }
        self.said = Some(Said::Added);
    }
}

struct Screen;

impl ReferenceStateMachine for Screen {
    type State = Rows;
    type Transition = Action;

    fn init_state() -> BoxedStrategy<Rows> {
        let frames = || prop_oneof![Just(0u32), Just(47), Just(107)];
        let setup = (frames(), prop_oneof![Just(0u32), Just(78)], any::<bool>(), frames()).prop_map(
            |(add_frames, more, default_destination, details_frames)| Setup {
                add_frames,
                torrent_url_frames: add_frames + more,
                default_destination,
                details_frames,
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
                said: None,
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
            2 => Just(Link::Unknown),
            1 => Just(Link::NotALink),
            1 => Just(Link::NoInfoHash),
        ];
        let field = prop_oneof![
            3 => link.prop_map(Action::Type),
            3 => any::<bool>().prop_map(|button| Action::Send { button }),
            1 => any::<bool>().prop_map(Action::Choose),
        ];
        let waits = prop_oneof![Just(0.5), Just(1.5), Just(2.5)].prop_map(Action::Wait);
        if ids.is_empty() {
            return prop_oneof![1 => paste, 2 => field, 1 => waits].boxed();
        }
        let id = proptest::sample::select(ids);
        prop_oneof![
            2 => paste,
            4 => field,
            4 => id.clone().prop_map(Action::Select),
            2 => Just(Action::Pause),
            2 => Just(Action::Resume),
            2 => Just(Action::Check),
            1 => any::<bool>().prop_map(|delete| Action::Remove { delete }),
            3 => (0..3usize, any::<bool>()).prop_map(|(row, off)| Action::SetFile { row, off }),
            2 => (id.clone(), files.clone()).prop_map(|(id, files)| Action::FilesDeleted { id, files }),
            2 => (id.clone(), files).prop_map(|(id, files)| Action::FilesPutBack { id, files }),
            1 => id.clone().prop_map(Action::DestinationGone),
            1 => id.prop_map(Action::DestinationBack),
            3 => waits,
        ]
        .boxed()
    }

    fn preconditions(rows: &Rows, action: &Action) -> bool {
        match action {
            Action::PasteMagnet(_) | Action::Send { .. } => rows.room(),
            Action::Select(id)
            | Action::FilesDeleted { id, .. }
            | Action::FilesPutBack { id, .. }
            | Action::DestinationGone(id)
            | Action::DestinationBack(id) => rows.rows.contains_key(id),
            _ => true,
        }
    }

    fn apply(mut rows: Rows, action: &Action) -> Rows {
        rows.said = None;
        match action {
            Action::PasteMagnet(n) => rows.send(Link::Magnet(*n), false, false),
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
                    let link = Link::of(&rows.field).expect("a link from the pool");
                    rows.send(link, true, false);
                }
            }
            Action::Choose(chosen) => {
                // Cancelled, it's answered with nothing added: nothing to see.
                if let Some(waiting) = rows.picker.pop_front()
                    && *chosen
                {
                    rows.send(waiting.link, waiting.field, true);
                }
            }
            Action::Select(id) => rows.selected = Some(*id),
            Action::Remove { .. } => {
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

struct Sut {
    rt: Runtime<Phantom>,
    disk: Disk,
    /// Torrents seen Paused or Done (wanted stopped) and not resumed since.
    stopped: BTreeSet<TorrentId>,
}

impl StateMachineTest for Screen {
    type SystemUnderTest = Sut;
    type Reference = Screen;

    fn init_test(rows: &Rows) -> Sut {
        let Setup { add_frames, torrent_url_frames, default_destination, details_frames } = rows.setup;
        let service = with_an_album()
            .details_after(Setup::secs(details_frames))
            .rejecting(UNKNOWN)
            .adds_after(Setup::secs(add_frames), Setup::secs(torrent_url_frames))
            .with_default_destination(default_destination.then(|| "/srv/torrents".into()));
        let disk = service.disk();
        let mut rt = app(service);
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(&mut rt);
        Sut { rt, disk, stopped: BTreeSet::new() }
    }

    fn apply(mut sut: Sut, rows: &Rows, action: Action) -> Sut {
        let resumed = match action {
            Action::Resume => selected_for(&sut.rt.state.table, &sut.rt.state.torrents, Do::Resume),
            _ => Vec::new(),
        };
        let rt = &mut sut.rt;
        match &action {
            Action::PasteMagnet(n) => {
                rt.update(|_, tx| tx.blur());
                rt.paste(&magnet(*n));
            }
            Action::Type(link) => {
                press(rt, "magnet-field");
                rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                rt.paste(&link.text());
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
            Action::Pause => press(rt, "pause"),
            Action::Resume => press(rt, "resume"),
            Action::Check => press(rt, "check"),
            Action::Remove { delete } => {
                press(rt, "remove");
                if let Some(menu) = rt.popup_menu() {
                    let pick = if *delete && menu.find("delete").is_some() { "delete" } else { "remove" };
                    rt.resolve_popup(Some(pick)).unwrap();
                }
            }
            Action::SetFile { row, off } => {
                press(rt, &format!("files/viewport/rows/{row}"));
                press(rt, if *off { "file-off" } else { "file-normal" });
            }
            Action::FilesDeleted { id, files } => sut.disk.change(DiskChange::FilesDeleted { id: *id, files: files.clone() }),
            Action::FilesPutBack { id, files } => sut.disk.change(DiskChange::FilesPutBack { id: *id, files: files.clone() }),
            Action::DestinationGone(id) => {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                sut.disk.change(DiskChange::DestinationGone { id: *id, reason });
            }
            Action::DestinationBack(id) => sut.disk.change(DiskChange::DestinationBack { id: *id }),
            Action::Wait(secs) => wait(rt, *secs),
        }
        step(rt);

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
        // Every add that lands shows: its row is in the list (checked below), or its error is
        // shown, by Add or in the status bar.
        let shown_by_add = |text: &str| s.add_note.as_ref().is_some_and(|n| n.error && n.text == text && n.text == s.magnet.text);
        match &rows.said {
            Some(Said::Added) => assert!(s.notice.as_ref().is_some_and(|n| !n.error), "the add is told of: {:?}", s.notice),
            Some(Said::FailedByAdd(text)) => assert!(shown_by_add(text), "the failure shows by Add"),
            Some(Said::FailedInStatusBar) => assert!(s.notice.as_ref().is_some_and(|n| n.error), "the failure shows in the status bar"),
            None => {}
        }
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
            assert!(t.progress_bytes <= t.size_bytes && t.size_bytes <= t.full_size_bytes, "torrent {}: {t:?}", t.id);
            if t.status != Status::FetchingInfo {
                assert_eq!(cells[&t.id].get("size"), Some(&client::display::bytes(t.size_bytes)), "torrent {}'s size", t.id);
            }
        }

        // A button is on when the service lists its action for a torrent it applies to.
        let selected = selected_torrents(&s.table);
        let any = |action| s.torrents.iter().any(|t| selected.contains(&t.id) && t.actions.contains(&action));
        assert_eq!(enabled(rt, "pause"), any(Do::Pause), "Pause");
        assert_eq!(enabled(rt, "resume"), any(Do::Resume), "Resume");
        assert_eq!(enabled(rt, "remove"), s.table.selected_len() > 0, "Remove…");

        let Some(selected) = details_torrent(&s.table, &s.torrents) else {
            assert!(rt.find("details").is_none(), "a pane only for exactly one torrent selected");
            assert_eq!(s.pane.watching, None, "and details only for it");
            return;
        };
        // The selected torrent, or the last one until the selected one's details arrive.
        let t = shown_torrent(&s.table, &s.torrents, &s.pane).expect("a torrent selected");
        assert!(rt.find("details").is_some(), "the pane for torrent {}", t.id);
        assert_eq!(enabled(rt, "check"), t.actions.contains(&Do::Check), "Check");
        let Some(details) = s.pane.details.as_ref() else { return };
        assert_eq!(s.pane.watching, Some(selected.id), "details come for the selected torrent");
        assert_eq!(details.id, t.id, "the pane shows one torrent: the files are its");
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

/// Two frames that take no time on the app clock, after a step: only `Wait` moves it.
fn step(rt: &mut Runtime<Phantom>) {
    rt.frame(0.0);
    rt.frame(0.0);
}

/// Presses the element at `key`, if it's there; a disabled button does nothing. A note open by
/// Add takes the first click anywhere (it closes), so the person closes it first.
fn press(rt: &mut Runtime<Phantom>, key: &str) {
    if let Some((x, y)) = rt.find("add-note-scrim") {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
    if let Some((x, y)) = rt.find(key) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        step(rt);
    }
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

fn list_cells(rt: &Runtime<Phantom>) -> BTreeMap<TorrentId, BTreeMap<String, String>> {
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
