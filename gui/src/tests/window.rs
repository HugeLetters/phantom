//! The main window's layout (#33): the list's columns are in the person's order, a drag on a header
//! moving one, and hide from the right end as it narrows, never cut and never all; the header's
//! menu hides and shows any of them; all kept in `gui.json`. The details pane is as tall as its
//! content, taking the room a short list leaves, its divider drags and is kept; its header is two
//! lines with a left and a right side; its tabs switch and are kept. The selected row stays in
//! view as the list shortens. The link field is centred in the titlebar band.

use proptest::prelude::*;
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{FRAME, MIB, app, on_screen, settle, tap, toggle, torrent};
use crate::model::Phantom;
use crate::model::layout::{
    self, COLUMN_ORDER, COLUMN_WIDTHS, Column, ColumnOrder, ColumnWidths, HIDDEN_COLUMNS, Header, INFO_LINE, INFO_PAD, Kept, NAME_MIN,
    PANE_HEIGHT, PANE_TAB, PaneTab, ROW, TABLE_HEADER, TABS, WindowPrefs,
};
use crate::service::{DiskChange, MemoryService};
use crate::settings::file;
use crate::views::toolbar::{self, PLACEHOLDERS};
use proto::{Content, File, Priority, Status, TorrentFailure, TorrentInfo, Unavailable};

/// The lines Info has for a torrent as `torrent` makes it: what it uploaded, its ratio, its info hash and its magnet link.
const INFO_LINES: usize = 4;

/// The torrent list's header cells, in order, with their rects.
fn header(rt: &Runtime<Phantom>) -> Vec<(Column, [f32; 4])> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "columnheader" && e.path.contains("/list/table/header/"))
        .filter_map(|e| Some((Column::from_id(e.key.as_deref()?)?, e.rect)))
        .collect()
}

fn shown(rt: &Runtime<Phantom>) -> Vec<Column> {
    header(rt).into_iter().map(|(c, _)| c).collect()
}

/// The element keyed `key`, or whose path ends with it (`grips/size`).
fn element(rt: &Runtime<Phantom>, key: &str) -> rune::ElementInfo {
    let suffix = format!("/{key}");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && (e.key.as_deref() == Some(key) || e.path.ends_with(&suffix)))
        .unwrap_or_else(|| panic!("no {key:?}"))
}

fn rect(rt: &Runtime<Phantom>, key: &str) -> [f32; 4] {
    element(rt, key).rect
}

fn texts(rt: &Runtime<Phantom>, key: &str) -> Vec<String> {
    super::generated::texts_under(rt, key)
}

fn resize(rt: &mut Runtime<Phantom>, w: f32, h: f32) {
    rt.set_viewport((w, h), 2.0);
    for _ in 0..3 {
        settle(rt);
    }
}

/// `gui.json` in a temp dir, and the window's choices kept there.
struct Kept_ {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl Kept_ {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui.json");
        Self { _dir: dir, path }
    }

    fn prefs(&self, kept: Kept) -> WindowPrefs {
        let path = self.path.clone();
        WindowPrefs::new(kept, move |key, value: serde_json::Value| file::keep_at(&path, key, Some(&value)).unwrap())
    }

    /// What the next start reads.
    fn read(&self) -> Kept {
        let hidden: Vec<Column> = file::read_at(&self.path, HIDDEN_COLUMNS).unwrap_or_default();
        Kept {
            order: file::read_at(&self.path, COLUMN_ORDER).unwrap_or_default(),
            hidden: hidden.into_iter().collect(),
            widths: file::read_at(&self.path, COLUMN_WIDTHS).unwrap_or_default(),
            pane_height: file::read_at(&self.path, PANE_HEIGHT),
            tab: file::read_at(&self.path, PANE_TAB).unwrap_or_default(),
        }
    }
}

/// Five torrents, one of them in each of the widest statuses.
fn listed() -> MemoryService {
    let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
    MemoryService::new()
        .with_torrents((0..5).map(|i| torrent(i, &format!("Torrent number {i}"))).collect())
        .at(0.0, DiskChange::DestinationGone { id: 4, reason })
}

/// `n` tracks of 10 MiB.
fn tracks(n: usize) -> Vec<File> {
    let file = |index| File {
        index,
        path: format!("{index:02} Track.flac").into(),
        size_bytes: 10 * MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
        on_disk: false,
    };
    (0..n).map(file).collect()
}

/// A torrent ("Album", id 1) of `n` files, beside a torrent of one ("a", id 0).
fn album(n: usize) -> MemoryService {
    MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "Album")]).with_files(1, tracks(n))
}

/// `rows` torrents, the one with id 1 of `n` files.
fn long_list(rows: usize, n: usize) -> MemoryService {
    MemoryService::new().with_torrents((0..rows as u64).map(|i| torrent(i, &format!("Torrent {i:02}"))).collect()).with_files(1, tracks(n))
}

/// Presses `key`'s centre and drags it by `dx` in steps, then lets go there.
fn drag(rt: &mut Runtime<Phantom>, key: &str, dx: f32) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    for step in 1..=20 {
        rt.pointer_move(x + dx * step as f32 / 20.0, y);
        rt.frame(FRAME);
    }
    rt.pointer_up(x + dx, y);
    settle(rt);
}

fn order() -> impl Strategy<Value = ColumnOrder> {
    Just(Column::ALL.to_vec()).prop_shuffle().prop_map(|o| ColumnOrder::try_from(o).unwrap())
}

/// The most a rect read back is off the height the layout used: the window is laid out at scale 2,
/// so rects snap to the device pixel, 0.5 point, and rounding to the nearest moves one at most half
/// of that.
const SNAP: f32 = 0.25;

fn pane_is_as_tall_as_its_content(w: f32, h: f32, n: usize) -> Result<(), TestCaseError> {
    let mut rt = app(album(n));
    resize(&mut rt, w, h);
    tap(&mut rt, "rows/1");
    for _ in 0..3 {
        settle(&mut rt);
    }
    let room = rect(&rt, "body")[3];
    let pane = rect(&rt, "details")[3];
    let list = rect(&rt, "list")[3];
    let under =
        |header: Header| header.height() + TABS + (TABLE_HEADER + n as f32 * ROW).max(2.0 * INFO_PAD + INFO_LINES as f32 * INFO_LINE);
    let whole = under(Header::Whole);
    prop_assert!(pane <= whole + 0.5, "no blank under the files: {} of {}", pane, whole);
    prop_assert!(list >= layout::LIST_MIN - 0.5, "the list keeps its header and three rows: {}", list);
    if whole <= layout::pane_most(room) {
        prop_assert!((pane - whole).abs() < 0.5, "all of it, a list of two leaving the room: {} of {}", pane, whole);
    }
    let header = rect(&rt, "details-header")[3];
    if (header - Header::OneLine.height()).abs() < 0.5 {
        prop_assert!(pane - Header::Whole.height() - TABS - TABLE_HEADER < 2.0 * ROW + SNAP, "one line only when short: {pane}");
        prop_assert!(pane <= under(Header::OneLine) + 0.5);
    }
    Ok(())
}

/// #388: at 640 x 371.4992 the list's room is 293.9992 and the layout, short of two files by that
/// much, draws a one-line header over a pane whose snapped height, 170, reads as exactly two.
#[test]
fn the_pane_is_as_tall_as_its_content_at_a_fractional_height() {
    pane_is_as_tall_as_its_content(640.0, 371.4992, 1).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(12).min(64), ..ProptestConfig::default() })]

    /// At any width the window can have and in any order, the columns shown are the start of the
    /// order (they hide from the right end), at least one; each is whole inside the list, Name has
    /// at least its least, and a wider window shows every column a narrower one does.
    #[test]
    fn columns_hide_from_the_right_end_and_are_never_cut(a in 640.0f32..1600.0, b in 640.0f32..1600.0, order in order()) {
        let mut rt = app(listed());
        rt.state.prefs = WindowPrefs::unkept(Kept { order: order.clone(), ..Kept::default() });
        let mut seen = Vec::new();
        for width in [a.min(b), a.max(b)] {
            resize(&mut rt, width, 540.0);
            let cells = header(&rt);
            let shown: Vec<Column> = cells.iter().map(|(c, _)| *c).collect();
            prop_assert!(!shown.is_empty());
            prop_assert_eq!(&shown[..], &order.columns()[..shown.len()], "the right end gone");
            let list = rect(&rt, "table");
            for (c, r) in &cells {
                prop_assert!(r[0] >= list[0] - 0.5 && r[0] + r[2] <= list[0] + list[2] + 0.5, "{:?} whole at {}: {:?} in {:?}", c, width, r, list);
            }
            if let Some((_, name)) = cells.iter().find(|(c, _)| *c == Column::Name) {
                prop_assert!(name[2] >= NAME_MIN - 0.5, "Name {} at {}", name[2], width);
            }
            seen.push(shown);
        }
        prop_assert!(seen[0].iter().all(|c| seen[1].contains(c)), "wider shows more: {:?}", seen);
    }

    /// The pane is never taller than its content, it's all of it when it fits in its share of the
    /// room, and the list keeps its header and three rows; the header is one line only where the
    /// whole one would leave fewer than two files showing.
    #[test]
    fn the_pane_is_as_tall_as_its_content(w in 640.0f32..1400.0, h in 320.0f32..1000.0, n in 1usize..14) {
        pane_is_as_tall_as_its_content(w, h, n)?;

    }
}

/// Dragging Size's header to the list's start moves it there, the others making way, and the rows'
/// cells follow; let go, the order is kept in `gui.json` and is the same after a restart. A click on
/// a header still sorts, and a drag doesn't.
#[test]
fn a_column_drags_to_another_place_and_is_kept() {
    let kept = Kept_::new();
    let mut rt = app(listed());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 1400.0, 540.0);
    assert_eq!(shown(&rt), Column::ALL);
    let size = rect(&rt, "grips/size")[0];
    let name = rect(&rt, "grips/name")[0];
    drag(&mut rt, "grips/size/grip", name - size);
    let expected = [Column::Size, Column::Name, Column::Progress, Column::Status, Column::Down, Column::Up, Column::Peers, Column::Eta];
    assert_eq!(shown(&rt), expected, "Size first");
    let cell = |rt: &Runtime<Phantom>, column: &str| {
        rt.elements().into_iter().find(|e| e.path.ends_with(&format!("rows/0/{column}"))).unwrap().rect
    };
    assert!(cell(&rt, "size")[0] < cell(&rt, "name")[0], "the rows' cells follow");
    assert_eq!(rt.state.table.sort(), None, "a drag doesn't sort");
    assert_eq!(kept.read().order.columns(), expected);

    let mut again = app(listed());
    again.state.prefs = kept.prefs(kept.read());
    resize(&mut again, 1400.0, 540.0);
    assert_eq!(shown(&again), expected, "the same after a restart");
    tap(&mut again, "grips/peers/grip");
    assert_eq!(again.state.table.sort(), Some(("peers", rune_kit::SortDir::Ascending)), "a click sorts");
}

/// Escape during a drag puts the column back, and nothing is kept.
#[test]
fn a_cancelled_drag_puts_the_column_back() {
    let kept = Kept_::new();
    let mut rt = app(listed());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 1400.0, 540.0);
    let (x, y) = rt.find("grips/eta/grip").unwrap();
    rt.pointer_down(x, y);
    for step in 1..=20 {
        rt.pointer_move(x - step as f32 * 30.0, y);
        rt.frame(FRAME);
    }
    assert_ne!(shown(&rt), Column::ALL, "ETA moves as it's dragged");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.pointer_up(x - 600.0, y);
    settle(&mut rt);
    assert_eq!(shown(&rt), Column::ALL, "back where it was");
    assert_eq!(file::read_at::<ColumnOrder>(&kept.path, COLUMN_ORDER), None);
}

/// As the window narrows, the columns hide from the right end of the person's order, one at a time:
/// with ETA moved first, it's Peers, Up and Down that go, and ETA stays.
#[test]
fn narrowing_hides_from_the_right_end() {
    let mut rt = app(listed());
    let order = [Column::Eta, Column::Name, Column::Progress, Column::Status, Column::Size, Column::Down, Column::Up, Column::Peers];
    rt.state.prefs = WindowPrefs::unkept(Kept { order: ColumnOrder::try_from(order.to_vec()).unwrap(), ..Kept::default() });
    let mut last = order.len();
    for width in (640..=1400).rev().step_by(40) {
        resize(&mut rt, width as f32, 540.0);
        let now = shown(&rt);
        assert_eq!(now, order[..now.len()], "at {width}");
        assert!(now.len() + 1 >= last, "one at a time at {width}: {now:?}");
        last = now.len();
    }
    assert!(shown(&rt).contains(&Column::Eta), "ETA stays at the start");
    assert!(last < order.len(), "something hid at 640");
}

/// A right-click on the header lists every column, in the list's order, each ticked while it's
/// wanted; a pick hides or shows it, Name too, kept in `gui.json`, the same after a restart. The
/// last column showing stays, whatever's picked.
#[test]
fn the_header_menu_hides_any_column_but_the_last() {
    let kept = Kept_::new();
    let mut rt = app(listed());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 1400.0, 540.0);

    let menu = |rt: &mut Runtime<Phantom>| -> Vec<(String, bool)> {
        let grip = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.path.contains("/grips/") && e.key.as_deref() == Some("grip"))
            .expect("the header");
        let (x, y) = (grip.rect[0] + grip.rect[2] / 2.0, grip.rect[1] + grip.rect[3] / 2.0);
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        settle(rt);
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.role == "menuitemcheckbox" && e.path.contains("columns-menu"))
            .map(|e| {
                (
                    e.label.clone().unwrap_or_default(),
                    rt.elements().iter().any(|c| c.path.starts_with(&format!("{}/", e.path)) && c.key.as_deref() == Some("icon")),
                )
            })
            .collect()
    };
    let pick = |rt: &mut Runtime<Phantom>, label: &str| {
        menu(rt);
        let item =
            rt.elements().into_iter().find(|e| !e.exiting && e.role == "menuitemcheckbox" && e.label.as_deref() == Some(label)).unwrap();
        let (x, y) = (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(rt);
        // The menu stays open as columns are ticked; a right-click outside it closes it.
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        settle(rt);
    };
    let all: Vec<(String, bool)> = Column::ALL.iter().map(|c| (c.title().to_owned(), true)).collect();
    assert_eq!(menu(&mut rt), all, "every column, all ticked");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);
    pick(&mut rt, "Name");
    assert!(!shown(&rt).contains(&Column::Name), "Name hides like any column");
    assert_eq!(file::read_at::<Vec<Column>>(&kept.path, HIDDEN_COLUMNS), Some(vec![Column::Name]));
    let ticks: Vec<bool> = menu(&mut rt).into_iter().map(|(_, on)| on).collect();
    assert_eq!(ticks, [false, true, true, true, true, true, true, true], "Name unticked");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);

    let mut again = app(listed());
    again.state.prefs = kept.prefs(kept.read());
    resize(&mut again, 1400.0, 540.0);
    assert!(!shown(&again).contains(&Column::Name), "hidden after a restart");
    for c in ["Progress", "Status", "Size", "Down", "Up", "Peers"] {
        pick(&mut again, c);
    }
    assert_eq!(shown(&again), [Column::Eta]);
    pick(&mut again, "ETA");
    assert_eq!(shown(&again), [Column::Eta], "the last column stays");
    pick(&mut again, "Name");
    assert_eq!(shown(&again), [Column::Name, Column::Eta], "shown again, in its place");
}

/// The last column on screen stays when others are wanted but have no room: in a window that
/// shows only Name, a pick of Name leaves it, though seven more columns are wanted. (With all the
/// others hidden too, the check is the one a count of the wanted makes; this is the other.)
#[test]
fn the_last_column_on_screen_stays_though_others_are_wanted() {
    let mut rt = app(listed());
    resize(&mut rt, 300.0, 540.0);
    assert_eq!(shown(&rt), [Column::Name], "only Name has room");
    let grip = rt.elements().into_iter().find(|e| !e.exiting && e.path.contains("/grips/") && e.key.as_deref() == Some("grip")).unwrap();
    let (x, y) = (grip.rect[0] + grip.rect[2] / 2.0, grip.rect[1] + grip.rect[3] / 2.0);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    let item =
        rt.elements().into_iter().find(|e| !e.exiting && e.role == "menuitemcheckbox" && e.label.as_deref() == Some("Name")).unwrap();
    let (x, y) = (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.prefs.hidden().is_empty(), "Name stays wanted: {:?}", rt.state.prefs.hidden());
    assert_eq!(shown(&rt), [Column::Name]);
}

/// With every column hidden (a `gui.json` written by hand) the list still shows its first, and the
/// header menu ticks it: what shows is what's ticked.
#[test]
fn the_column_showing_when_all_are_hidden_is_ticked_in_the_menu() {
    let mut rt = app(listed());
    rt.state.prefs = WindowPrefs::unkept(Kept { hidden: Column::ALL.into_iter().collect(), ..Kept::default() });
    resize(&mut rt, 1400.0, 540.0);
    assert_eq!(shown(&rt), [Column::Name]);
    let grip = rt.elements().into_iter().find(|e| !e.exiting && e.path.contains("/grips/") && e.key.as_deref() == Some("grip")).unwrap();
    let (x, y) = (grip.rect[0] + grip.rect[2] / 2.0, grip.rect[1] + grip.rect[3] / 2.0);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    let ticked: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "menuitemcheckbox" && e.path.contains("columns-menu"))
        .filter(|e| rt.elements().iter().any(|c| c.path.starts_with(&format!("{}/", e.path)) && c.key.as_deref() == Some("icon")))
        .filter_map(|e| e.label)
        .collect();
    assert_eq!(ticked, ["Name"]);
}

/// Dragging the divider up makes the pane taller (never past what the list leaves); let go, the
/// height is kept in `gui.json`, and a window that reads it again has the pane that tall.
#[test]
fn the_divider_drags_and_its_height_is_kept() {
    let kept = Kept_::new();
    let service = || long_list(20, 30);
    let mut rt = app(service());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let before = rect(&rt, "details")[3];
    let (x, y) = rt.find("divider").expect("a divider");
    rt.pointer_down(x, y);
    for step in 1..=10 {
        rt.pointer_move(x, y - step as f32 * 10.0);
        rt.frame(FRAME);
    }
    rt.pointer_up(x, y - 100.0);
    settle(&mut rt);
    let after = rect(&rt, "details")[3];
    assert!((after - (before + 100.0)).abs() < 1.0, "{before} → {after}");
    let height = kept.read().pane_height.expect("kept");
    assert!((height - after).abs() < 1.0, "kept {height}");
    assert!(rect(&rt, "list")[3] >= layout::LIST_MIN - 0.5);

    let mut again = app(service());
    again.state.prefs = kept.prefs(kept.read());
    resize(&mut again, 960.0, 700.0);
    tap(&mut again, "rows/1");
    settle(&mut again);
    assert!((rect(&again, "details")[3] - after).abs() < 1.0, "the same after a restart");
}

/// Over a list of two torrents, a torrent of many files takes the room the list doesn't need, far
/// past 40 %; over a long list it keeps to its share.
#[test]
fn the_pane_takes_the_room_a_short_list_leaves() {
    let mut rt = app(album(40));
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let room = rect(&rt, "body")[3];
    let pane = rect(&rt, "details")[3];
    assert!((pane - layout::pane_most(room)).abs() < 0.5, "all but the list's least: {pane} of {room}");

    let mut rt = app(long_list(30, 40));
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let room = rect(&rt, "body")[3];
    assert!((rect(&rt, "details")[3] - room * layout::PANE_SHARE).abs() < 0.5, "its share");
}

/// A torrent of one file: the pane is its header, its tabs and the taller of its one file and its
/// info, with nothing under them.
#[test]
fn a_one_file_torrent_has_no_blank_under_it() {
    let mut rt = app(album(3));
    resize(&mut rt, 960.0, 800.0);
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    let pane = rect(&rt, "details")[3];
    let expected = Header::Whole.height() + TABS + (TABLE_HEADER + ROW).max(2.0 * INFO_PAD + INFO_LINES as f32 * INFO_LINE);
    assert!((pane - expected).abs() < 0.5, "{pane} of {expected}");
}

/// Files shows first; a click on Info shows its info hash and magnet link (and the Files tab's
/// table goes), the pane's height staying; the tab is kept in `gui.json` and shown after a restart.
#[test]
fn the_tabs_switch_and_the_last_is_kept() {
    let kept = Kept_::new();
    let mut rt = app(album(3));
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert!(rt.find("info").is_none(), "Files first");
    let height = rect(&rt, "details")[3];
    tap(&mut rt, "tab-info");
    assert_eq!(texts(&rt, "info-hash"), [format!("{:040x}", 1)]);
    assert!(texts(&rt, "info-magnet").concat().starts_with(&format!("magnet:?xt=urn:btih:{:040x}", 1)));
    assert!(rt.find("copy-magnet").is_some(), "Copy beside the magnet link");
    assert!(!rt.elements().iter().any(|e| !e.exiting && e.path.contains("/details/") && e.path.ends_with("/rows/2")), "the files are gone");
    assert!((rect(&rt, "details")[3] - height).abs() < 0.5, "the same height");
    assert_eq!(kept.read().tab, PaneTab::Info);

    tap(&mut rt, "copy-magnet");
    let copied = rt.copy_request.take().expect("a copy");
    assert!(copied.starts_with(&format!("magnet:?xt=urn:btih:{:040x}", 1)), "copied {}", &*copied);

    let mut again = app(album(3));
    again.state.prefs = kept.prefs(kept.read());
    resize(&mut again, 960.0, 700.0);
    tap(&mut again, "rows/1");
    settle(&mut again);
    assert!(again.find("info-hash").is_some(), "Info after a restart");
    tap(&mut again, "tab-files");
    assert!(again.find("info-hash").is_none());
    assert_eq!(kept.read().tab, PaneTab::Files);
}

/// A torrent of 3.3 GiB with 4.1 GiB Off, which uploaded 960 MiB, its disk full, named `name`.
pub(super) fn tears(name: &str) -> MemoryService {
    let t = TorrentInfo { status: Status::Error { failure: TorrentFailure::DiskFull }, uploaded_bytes: 960 * MIB, ..torrent(1, name) };
    let file = |index, size_bytes, priority| File {
        index,
        path: format!("{index}.mkv").into(),
        size_bytes,
        progress_bytes: 0,
        priority,
        missing: false,
        on_disk: false,
    };
    MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), t])
        .with_files(1, vec![file(0, 3379 * MIB, Priority::Normal), file(1, 4198 * MIB, Priority::Off)])
}

const SIZE_AND_OFF: &str = "3.3 GiB · 4.1 GiB Off";

/// The header's two lines: the name, and on its right the Size and how much is Off, nothing else;
/// where it is, and on its right the problem it has, if it has one. Each right side ends at the
/// pane's right edge.
#[test]
fn the_header_has_two_lines_with_a_right_side() {
    let mut rt = app(tears("Tears of Steel (2012)"));
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert_eq!(texts(&rt, "details-name"), ["Tears of Steel (2012)"]);
    assert_eq!(texts(&rt, "details-side"), [SIZE_AND_OFF]);
    assert_eq!(element(&rt, "details-side").help, None, "all of it shows");
    assert!(texts(&rt, "details-location").concat().contains("Tears of Steel"), "where it is");
    assert_eq!(texts(&rt, "details-reason"), ["The disk is full"]);
    let pane = rect(&rt, "details");
    for key in ["details-side", "details-reason"] {
        let r = rect(&rt, key);
        assert!((pane[0] + pane[2] - (r[0] + r[2]) - 10.0).abs() < 1.0, "{key} on the right: {r:?} in {pane:?}");
    }
    let (name, side, path, reason) =
        (rect(&rt, "details-name"), rect(&rt, "details-side"), rect(&rt, "details-location"), rect(&rt, "details-reason"));
    assert!(name[1] < path[1] && (side[1] + side[3] / 2.0 - name[1] - name[3] / 2.0).abs() < 1.0, "the size on the name's line");
    assert!((reason[1] - path[1]).abs() < 1.0, "the reason on the path's line");
}

/// The size and Off stay whole as the pane narrows; it's the name that's shortened, in the middle,
/// its start and its end kept, and shown whole on hover (and not when it fits).
#[test]
fn a_long_name_is_shortened_in_the_middle_and_whole_on_hover() {
    let name = "Tears of Steel (2012) Open Movie Project by the Blender Foundation, the 4K Remastered Edition.mkv";
    let mut rt = app(tears(name));
    resize(&mut rt, 1600.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert_eq!(texts(&rt, "details-name"), [name], "whole where it fits");
    assert_eq!(element(&rt, "details-name").help, None, "nothing to add on hover");
    let mut cut = 0;
    for width in (640..=1400).rev().step_by(40) {
        resize(&mut rt, width as f32, 700.0);
        assert_eq!(texts(&rt, "details-side"), [SIZE_AND_OFF], "the right side is whole at {width}");
        let shown = texts(&rt, "details-name").concat();
        if shown != name {
            cut += 1;
            let (start, end) = shown.split_once('…').unwrap_or_else(|| panic!("no … in {shown:?} at {width}"));
            assert!(name.starts_with(start) && name.ends_with(end), "{shown:?} keeps its start and its end at {width}");
            assert!(!start.is_empty() && !end.is_empty(), "both kept: {shown:?}");
            assert_eq!(element(&rt, "details-name").help.as_deref(), Some(name), "whole on hover at {width}");
        }
        let (n, side) = (rect(&rt, "details-name"), rect(&rt, "details-side"));
        assert!(n[0] + n[2] <= side[0] + 0.5, "the name ends before the size at {width}");
    }
    assert!(cut > 0, "it was cut somewhere");
}

/// A long path is shortened in the middle as well, its last part kept, whole on hover.
#[test]
fn a_long_path_is_shortened_in_the_middle_and_whole_on_hover() {
    let name = "Tears of Steel (2012) Open Movie Project by the Blender Foundation, the 4K Remastered Edition";
    let mut rt = app(tears(name));
    resize(&mut rt, 640.0, 700.0);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let location = element(&rt, "details-location");
    let shown = texts(&rt, "details-location").concat();
    let whole = location.help.clone().expect("whole on hover");
    assert!(shown.contains('…') && shown.chars().count() < whole.chars().count(), "{shown:?} of {whole:?}");
    assert!(whole.ends_with(shown.rsplit('…').next().unwrap()), "its end: {shown:?}");
}

/// The name and the path stop at 60 % of the header's width however wide the window gets, a long
/// one shortened in the middle to that, its whole on hover; the pane's right side stays clear.
#[test]
fn a_long_name_and_path_stop_at_60_percent_of_the_header() {
    let name = "Tears of Steel (2012) Open Movie Project by the Blender Foundation, the 4K Remastered Edition, Director's Cut.mkv";
    let mut rt = app(tears(name));
    tap(&mut rt, "rows/1");
    for width in [640.0, 960.0, 1400.0, 2000.0] {
        resize(&mut rt, width, 700.0);
        let header = rect(&rt, "details-header");
        let cap = (header[2] - 20.0) * 0.6;
        for key in ["details-name", "details-location"] {
            let r = rect(&rt, key);
            assert!(r[2] <= cap + 1.0, "{key} is {} wide at {width}, the cap {cap}", r[2]);
        }
        if width == 960.0 {
            let shown = texts(&rt, "details-name").concat();
            assert!(shown.contains('…'), "the name shortened at {width} though the size is far: {shown:?}");
            assert_eq!(element(&rt, "details-name").help.as_deref(), Some(name), "whole on hover");
        }
    }
}

/// A renamed torrent's name, short enough to show whole, has its hover: the full new name and the
/// original name; a torrent never renamed has none.
#[test]
fn a_renamed_name_s_hover_has_the_new_name_and_the_original() {
    let renamed = TorrentInfo { original_name: Some("Big Buck Bunny (2008)".into()), ..torrent(1, "Bunny") };
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), renamed]));
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert_eq!(texts(&rt, "details-name"), ["Bunny"]);
    assert_eq!(element(&rt, "details-name").help.as_deref(), Some("Bunny\nBig Buck Bunny (2008)"));
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    assert_eq!(element(&rt, "details-name").help, None);
}

/// An all-well torrent's path line has nothing on its right; a problem is there, short, in the error
/// color. A seeding limit reached isn't a problem: it's on the Info tab.
#[test]
fn the_path_line_says_why_it_needs_the_user() {
    let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
    let failed = |status| proto::TorrentInfo { status, ..torrent(1, "Album") };
    let ratio = proto::Ratio::new(200).unwrap();
    let cases: Vec<(MemoryService, Option<&str>)> = vec![
        (album(3), None),
        (album(3).with_missing(1, &[0, 2]), Some("2 files are gone from disk")),
        (album(3).at(0.0, DiskChange::DestinationGone { id: 1, reason }), Some("Drive MyDisk isn't connected")),
        (MemoryService::new().with_torrents(vec![failed(Status::Error { failure: TorrentFailure::DiskFull })]), Some("The disk is full")),
        (
            MemoryService::new().with_torrents(vec![TorrentInfo {
                content: Some(Content { size_bytes: 1 << 30, full_size_bytes: 1 << 30, progress_bytes: 1 << 30 }),
                ..failed(Status::Done { reached: Some(proto::Reached::Ratio(ratio)) })
            }]),
            None,
        ),
    ];
    for (service, why) in cases {
        let mut rt = app(service);
        resize(&mut rt, 960.0, 800.0);
        tap(&mut rt, "rows/1");
        settle(&mut rt);
        assert!(texts(&rt, "details-location").concat().contains("Album"), "where it is");
        assert_eq!(texts(&rt, "details-reason"), why.into_iter().map(String::from).collect::<Vec<_>>());
    }
}

/// The selected row stays in view as the pane opens and the window shrinks.
#[test]
fn the_selected_row_stays_in_view() {
    let service = MemoryService::new().with_torrents((0..20).map(|i| torrent(i, &format!("Torrent {i:02}"))).collect());
    let mut rt = app(service);
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/0");
    rt.key(KeyEvent { key: Key::End, mods: Mods::default() });
    for _ in 0..3 {
        settle(&mut rt);
    }
    assert!(rt.find("details").is_some(), "the pane opened");
    assert!(on_screen(&rt, "rows/19"), "in view once the pane opened");
    for (w, h) in [(960.0, 480.0), (800.0, 400.0), (640.0, 320.0)] {
        resize(&mut rt, w, h);
        assert!(on_screen(&rt, "rows/19"), "in view at {w}×{h}");
    }
}

/// The link field, with Add at its end, sits in the window's middle in the titlebar band, Add file…
/// after it, with free room either side to drag the window by; its placeholder shows whole at
/// the default size and at the smallest window. In full screen (no traffic lights) it's in the middle too.
#[test]
fn the_link_field_is_centred_in_the_band() {
    let mut rt = app(listed());
    for (inset, width, placeholder) in [(67.0, 960.0, PLACEHOLDERS[0]), (0.0, 960.0, PLACEHOLDERS[0]), (67.0, 640.0, PLACEHOLDERS[0])] {
        rt.set_titlebar_inset(inset);
        resize(&mut rt, width, 400.0);
        let field = rect(&rt, "magnet-field");
        let add = rect(&rt, "add-focus");
        let band = rect(&rt, "toolbar");
        assert_eq!((band[1], band[3]), (0.0, crate::theme::Spacing::phantom().toolbar), "the band");
        assert!(field[1] >= 0.0 && field[1] + field[3] <= band[3], "the field in the band");
        assert!(add[0] > field[0] + field[2] && rect(&rt, "add-file")[0] > add[0] + add[2], "Add, then Add file…, after the field");
        let middle = (field[0] + add[0] + add[2]) / 2.0;
        assert!((middle - width / 2.0).abs() < 1.0, "in the middle at {width} (inset {inset}): {field:?} {add:?}");
        let (left, right) = (rect(&rt, "band-left"), rect(&rt, "band-right"));
        assert!(left[0] >= inset && left[0] + left[2] <= field[0] + 0.5, "free room left of the field");
        assert!(right[0] >= rect(&rt, "add-file")[0], "free room right of Add file…");
        assert!(left[2] >= toolbar::DRAG_MIN - 0.5 && right[2] >= toolbar::DRAG_MIN - 0.5, "room to drag at {width}: {left:?} {right:?}");
        assert!(
            right[0] >= rect(&rt, "add-file")[0] + rect(&rt, "add-file")[2] && (right[0] + right[2] - (width - 12.0)).abs() < 1.0,
            "free to the band's edge"
        );
        assert_eq!(texts(&rt, "magnet-field").concat(), placeholder, "at {width}");
        assert!(rect(&rt, "list")[1] >= band[3], "the list under the band");
    }
}

/// A torrent selected before the window first lays out (as a scene, or a selection kept) is in
/// view at the smallest size, under its pane.
#[test]
fn a_row_selected_from_the_start_is_in_view() {
    let service = MemoryService::new().with_torrents((0..20).map(|i| torrent(i, &format!("Torrent {i:02}"))).collect());
    let mut rt = app(service);
    rt.state.table.select("15");
    resize(&mut rt, 640.0, 320.0);
    assert!(rt.find("details").is_some(), "the pane is open");
    assert!(on_screen(&rt, "rows/15"));
}

// ---- The row toggle ----

/// A torrent running, one paused, one in Error (disk full), ids 0, 1 and 2.
fn of_each_status() -> MemoryService {
    let with = |id, status| TorrentInfo { status, ..torrent(id, &format!("Torrent {id}")) };
    MemoryService::new().with_torrents(vec![
        torrent(0, "Torrent 0"),
        with(1, Status::Paused),
        with(2, Status::Error { failure: TorrentFailure::DiskFull }),
    ])
}

fn status_of(rt: &Runtime<Phantom>, id: u64) -> Status {
    rt.state.torrents.iter().find(|t| t.id == id).unwrap().status.clone()
}

/// A running torrent's toggle pauses it (❚❚), a paused one's resumes it (▶), and so does one in
/// Error's, which retries.
#[test]
fn a_row_toggle_pauses_what_runs_and_resumes_the_rest() {
    let mut rt = app(of_each_status());
    resize(&mut rt, 960.0, 600.0);
    assert_eq!([0, 1, 2].map(|id| toggle(&rt, id)), [Some("Pause".into()), Some("Resume".into()), Some("Resume".into())]);
}

/// A click on a toggle acts on its torrent only, and the selection stays where it was.
#[test]
fn a_click_on_a_toggle_acts_on_its_torrent_only_and_leaves_the_selection() {
    let mut rt = app(of_each_status());
    resize(&mut rt, 960.0, 600.0);
    tap(&mut rt, "rows/2");
    tap(&mut rt, "rows/0/toggle");
    assert_eq!(status_of(&rt, 0), Status::Paused, "its own torrent");
    assert_eq!(status_of(&rt, 1), Status::Paused, "the other rows are as they were");
    assert!(matches!(status_of(&rt, 2), Status::Error { .. }), "even the selected one");
    assert_eq!(crate::model::selected_torrents(&rt.state.table), [2], "the selection stays");
    assert_eq!(toggle(&rt, 0).as_deref(), Some("Resume"), "and it offers the way back");

    tap(&mut rt, "rows/1/toggle");
    assert_eq!(status_of(&rt, 1), Status::Downloading, "a paused one runs");
    assert_eq!(crate::model::selected_torrents(&rt.state.table), [2]);
}

/// Where the service lists neither Pause nor Resume (a torrent whose info couldn't be fetched) the
/// row's toggle is the same button greyed: a click on it does nothing and doesn't select the row, it
/// has no hover or press state, and its tip is the status word. Its icon is what it would do: ❚❚ where it isn't known stopped.
#[test]
fn a_toggle_the_service_lists_no_action_for_is_shown_disabled() {
    let failed = TorrentInfo {
        status: Status::Error { failure: TorrentFailure::FetchFailed { detail: "no peers".into() } },
        ..torrent(1, "Torrent 1")
    };
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Torrent 0"), failed]));
    resize(&mut rt, 960.0, 600.0);
    tap(&mut rt, "rows/0");
    let neither = rt.state.torrents.iter().find(|t| t.id == 1).unwrap();
    assert!(crate::model::row_toggle(neither).is_none(), "the service lists neither: {:?}", neither.actions);

    let off = button_of(&rt, 1);
    assert_eq!(off.help.as_deref(), Some("Error"), "its tip says why");
    assert!(off.opacity < 1.0 && off.fill.a > 0.0, "the same button, greyed: {} {:?}", off.opacity, off.fill);
    assert_eq!(button_of(&rt, 0).opacity, 1.0, "a torrent that can act isn't");
    assert_eq!(toggle(&rt, 1), Some("Pause".into()), "it isn't known stopped: ❚❚");
    let (x, y) = (off.rect[0] + off.rect[2] / 2.0, off.rect[1] + off.rect[3] / 2.0);
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert_eq!(button_of(&rt, 1).hover, 0.0, "no hover state");
    rt.pointer_down(x, y);
    settle(&mut rt);
    assert_eq!(button_of(&rt, 1).press, 0.0, "no press state");
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(crate::model::selected_torrents(&rt.state.table), [0], "a click on it doesn't select its row");
    assert!(matches!(status_of(&rt, 1), Status::Error { .. }), "and does nothing");
}

/// The toggle is no column: not in the order, not in the header menu, in front of every column
/// whatever the order, and there at the narrowest window, where columns hide.
#[test]
fn the_toggle_is_no_column_and_stays_first() {
    let mut rt = app(of_each_status());
    let mut order = Column::ALL.to_vec();
    order.reverse();
    rt.state.prefs = WindowPrefs::new(Kept { order: ColumnOrder::try_from(order).unwrap(), ..Kept::default() }, |_, _| {});
    for (w, h) in [(960.0, 600.0), (640.0, 320.0)] {
        resize(&mut rt, w, h);
        let toggle = rect(&rt, "rows/0/toggle");
        for (column, at) in header(&rt) {
            assert!(toggle[0] + toggle[2] <= at[0] + 0.5, "the toggle before {column:?} at {w}");
        }
        assert!(rt.find("rows/0/toggle").is_some() && !header(&rt).is_empty());
    }
    assert!(!rt.state.prefs.order().columns().iter().any(|c| c.id() == "toggle"));
    let grip = element(&rt, "grips/eta");
    rt.secondary_down(grip.rect[0] + grip.rect[2] / 2.0, grip.rect[1] + grip.rect[3] / 2.0);
    rt.secondary_up(grip.rect[0] + grip.rect[2] / 2.0, grip.rect[1] + grip.rect[3] / 2.0);
    settle(&mut rt);
    let items: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "menuitemcheckbox" && e.path.contains("columns-menu"))
        .filter_map(|e| e.label)
        .collect();
    assert_eq!(items.len(), Column::ALL.len(), "the header menu lists the columns only: {items:?}");
}

/// The toggle's glyph color in row `id`.
fn glyph(rt: &Runtime<Phantom>, id: u64) -> Option<rune::Color> {
    let inside = format!("/rows/{id}/");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some("toggle-glyph") && e.path.contains(&inside))
        .and_then(|e| e.text_color)
}

/// The toggle is muted, and in full strength while its row is hovered or selected.
#[test]
fn the_toggle_is_muted_until_its_row_is_hovered_or_selected() {
    let mut rt = app(of_each_status());
    resize(&mut rt, 960.0, 600.0);
    let rest = glyph(&rt, 1).expect("a glyph");
    assert_eq!([glyph(&rt, 0), glyph(&rt, 2)], [Some(rest); 2], "muted all alike");
    tap(&mut rt, "rows/2");
    let strong = glyph(&rt, 2).unwrap();
    assert_ne!(strong, rest, "selected: full strength");
    assert_eq!([glyph(&rt, 0), glyph(&rt, 1)], [Some(rest); 2], "the others rest");
    // Over any cell of a row, not the toggle's own.
    let (x, y) = rt.find("rows/1/name").unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert_eq!(glyph(&rt, 1), Some(strong), "hovered: full strength");
    assert_eq!(glyph(&rt, 0), Some(rest), "the row beside doesn't");
    let (x, y) = rt.find("rows/0/size").unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert_eq!([glyph(&rt, 0), glyph(&rt, 1)], [Some(strong), Some(rest)], "it follows the pointer");
}

fn button_of(rt: &Runtime<Phantom>, id: u64) -> rune::ElementInfo {
    let inside = format!("/rows/{id}/");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some("toggle-button") && e.path.contains(&inside))
        .expect("a toggle")
}

/// The toggle looks like a button: a small rounded fill at rest (in the light look as in the dark),
/// a hover and a press state, inside its row's height, so the row doesn't grow.
#[test]
fn the_toggle_looks_like_a_button() {
    for dark in [false, true] {
        let mut rt = super::app_in(of_each_status(), dark);
        resize(&mut rt, 960.0, 600.0);
        let button = button_of(&rt, 0);
        assert!(button.fill.a > 0.0 && button.radius > 0.0, "a rounded fill at rest (dark: {dark}): {:?}", button.fill);
        assert_eq!(button.hover, 0.0, "not hovered");
        let row = rect(&rt, "rows/0");
        assert!(button.rect[3] <= row[3] - 4.0 && button.rect[2] <= 36.0, "compact: {:?} in {row:?}", button.rect);
        let (x, y) = (button.rect[0] + button.rect[2] / 2.0, button.rect[1] + button.rect[3] / 2.0);
        rt.pointer_move(x, y);
        settle(&mut rt);
        assert!(button_of(&rt, 0).hover > 0.0, "a hover state");
        rt.pointer_down(x, y);
        settle(&mut rt);
        assert!(button_of(&rt, 0).press > 0.0, "a pressed state");
        rt.pointer_up(x, y);
    }
}

/// The toolbar has the field, Add and Add file…: no Resume or Pause (the row has its toggle, the
/// row menu its items).
#[test]
fn the_toolbar_has_no_resume_or_pause() {
    let mut rt = app(listed());
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    assert!(rt.find("pause").is_none() && rt.find("resume").is_none());
    assert!(rt.find("add").is_some() && rt.find("add-file").is_some() && rt.find("magnet-field").is_some());
}

// ---- Info: the facts ----

/// Info holds what it uploaded and its ratio, with the info hash and the magnet link, in that order;
/// the original name after a Rename. None of it is in the header.
#[test]
fn the_info_tab_has_the_facts_the_hash_and_the_link() {
    let mut rt = app(tears("Tears of Steel (2012)"));
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    tap(&mut rt, "tab-info");
    assert_eq!(texts(&rt, "info-uploaded"), ["960.0 MiB"]);
    assert_eq!(texts(&rt, "info-ratio"), ["0.28"]);
    assert_eq!(texts(&rt, "info-hash"), [format!("{:040x}", 1)]);
    assert!(rt.find("info-limit").is_none() && rt.find("info-original").is_none(), "no limit, never renamed");
    let labels: Vec<String> =
        texts(&rt, "info").into_iter().filter(|t| ["Uploaded", "Ratio", "Info hash", "Magnet link"].contains(&t.as_str())).collect();
    assert_eq!(labels, ["Uploaded", "Ratio", "Info hash", "Magnet link"], "in this order");
    let header = texts(&rt, "details-header").concat();
    assert!(!header.contains("Uploaded") && !header.contains("Ratio") && !header.contains("960"), "not in the header: {header:?}");
}

/// A torrent stopped at a seeding limit says so on Info, and a renamed one its original name.
#[test]
fn the_info_tab_has_the_seeding_limit_and_the_original_name() {
    let ratio = proto::Ratio::new(200).unwrap();
    let done = TorrentInfo {
        status: Status::Done { reached: Some(proto::Reached::Ratio(ratio)) },
        content: Some(Content { size_bytes: 1 << 30, full_size_bytes: 1 << 30, progress_bytes: 1 << 30 }),
        uploaded_bytes: 2 << 30,
        original_name: Some("Big Buck Bunny (2008)".into()),
        ..torrent(1, "Bunny")
    };
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), done]));
    resize(&mut rt, 960.0, 700.0);
    tap(&mut rt, "rows/1");
    tap(&mut rt, "tab-info");
    assert_eq!(texts(&rt, "info-limit"), ["Stopped seeding at ratio 2.0"]);
    assert_eq!(texts(&rt, "info-ratio"), ["2.00"]);
    assert_eq!(texts(&rt, "info-original"), ["Big Buck Bunny (2008)"]);
    assert!(rt.find("details-reason").is_none(), "not a problem: nothing on the header's second line");
}

/// With nothing selected, a click on a toggle still selects nothing, and no pane opens.
#[test]
fn a_toggle_clicked_with_nothing_selected_selects_nothing() {
    let mut rt = app(super::with_an_album().paused());
    resize(&mut rt, 960.0, 900.0);
    tap(&mut rt, "rows/0/toggle");
    assert_eq!(status_of(&rt, 0), Status::Downloading, "it resumed");
    assert_eq!(crate::model::selected_torrents(&rt.state.table), Vec::<u64>::new());
    assert!(rt.find("details").is_none(), "no pane");
}

/// Presses the resize handle of `column` and drags it by `dx`.
fn resize_column(rt: &mut Runtime<Phantom>, column: Column, dx: f32) {
    drag(rt, &format!("grips/{}/resize", column.id()), dx);
}

/// The right edge of `column`'s header cell.
fn right_of(rt: &Runtime<Phantom>, column: Column) -> f32 {
    let r = header(rt).into_iter().find(|(c, _)| *c == column).unwrap_or_else(|| panic!("{column:?} not shown")).1;
    r[0] + r[2]
}

/// Where the last shown column ends, and where the list does.
fn ends(rt: &Runtime<Phantom>) -> (f32, f32) {
    let last = header(rt).last().map(|(_, r)| r[0] + r[2]).unwrap();
    let list = rect(rt, "table");
    (last, list[0] + list[2])
}

/// Dragging Progress's right edge by +40 moves that edge and nothing left of it: its own left
/// edge, Name's width and every column before it stay.
#[test]
fn a_dragged_edge_leaves_the_columns_left_of_it_in_place() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    let before = header(&rt);
    resize_column(&mut rt, Column::Progress, 40.0);
    let after = header(&rt);
    let progress = before.iter().position(|(c, _)| *c == Column::Progress).unwrap();
    for i in 0..=progress {
        assert!((after[i].1[0] - before[i].1[0]).abs() < 0.5, "{:?}'s left edge: {:?} then {:?}", after[i].0, before[i].1, after[i].1);
    }
    assert!((after[progress].1[2] - before[progress].1[2] - 40.0).abs() < 0.5, "Progress is 40 wider");
    for i in 0..progress {
        assert!((after[i].1[2] - before[i].1[2]).abs() < 0.5, "{:?} keeps its width", after[i].0);
    }
}

/// Name keeps the width it had when another column is dragged, wider or narrower.
#[test]
fn name_keeps_its_width_while_another_column_is_dragged() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    let name = width_of(&rt, Column::Name);
    resize_column(&mut rt, Column::Size, 60.0);
    assert!((width_of(&rt, Column::Name) - name).abs() < 0.5, "wider Size");
    resize_column(&mut rt, Column::Progress, -30.0);
    assert!((width_of(&rt, Column::Name) - name).abs() < 0.5, "narrower Progress");
}

/// The dragged edge is under the pointer at every step, whatever the columns do: the room a
/// narrower Name frees stays empty until the next column fits whole, then that column appears.
#[test]
fn a_drag_follows_the_pointer_and_a_column_appears_when_it_fits() {
    let mut rt = app(listed());
    resize(&mut rt, 800.0, 540.0);
    let (x, y) = rt.find("grips/name/resize").unwrap();
    let start = right_of(&rt, Column::Name);
    let (mut count, mut gap, mut appeared) = (shown(&rt).len(), false, false);
    assert!(count < Column::ALL.len());
    rt.pointer_down(x, y);
    for step in 1..=60 {
        let dx = -2.0 * step as f32;
        rt.pointer_move(x + dx, y);
        rt.frame(FRAME);
        let edge = right_of(&rt, Column::Name);
        assert!((edge - (start + dx)).abs() < 0.5, "step {step}: the edge is at {edge}, the pointer moved {dx} from {start}");
        let (last, end) = ends(&rt);
        assert!(last <= end + 0.5, "step {step}: columns reach {last}, the list {end}");
        let n = shown(&rt).len();
        if n > count {
            assert!(gap, "a column appears only after the room was empty");
            appeared = true;
        } else if end - last > 0.5 {
            gap = true;
        }
        assert!(n >= count, "no column goes while Name shrinks");
        count = n;
    }
    rt.pointer_up(x - 120.0, y);
    settle(&mut rt);
    assert!(appeared, "a column came");
}

/// Growing a column pushes the last one out once it no longer fits, and the others stay where
/// they were.
#[test]
fn growing_a_column_pushes_the_last_out() {
    let mut rt = app(listed());
    resize(&mut rt, 900.0, 540.0);
    let before = shown(&rt);
    resize_column(&mut rt, Column::Name, -400.0);
    let roomy = shown(&rt);
    assert!(roomy.len() > before.len(), "Name narrower brings columns: {before:?} then {roomy:?}");
    let progress = header(&rt).into_iter().find(|(c, _)| *c == Column::Progress).unwrap().1;
    resize_column(&mut rt, Column::Progress, 250.0);
    assert!(shown(&rt).len() < roomy.len(), "the last column hides");
    let now = header(&rt).into_iter().find(|(c, _)| *c == Column::Progress).unwrap().1;
    assert!((now[0] - progress[0]).abs() < 0.5, "Progress's left edge stays");
    let (last, end) = ends(&rt);
    assert!(last <= end + 0.5);
}

/// A window resize: Name, unsized, takes the change; sized, it keeps its width and the room at
/// the right end grows and shrinks, columns appearing and hiding there.
#[test]
fn a_window_resize_goes_to_name_unless_it_was_sized() {
    let mut rt = app(listed());
    resize(&mut rt, 1000.0, 540.0);
    let name = width_of(&rt, Column::Name);
    resize(&mut rt, 1100.0, 540.0);
    assert!((width_of(&rt, Column::Name) - name - 100.0).abs() < 0.5, "Name takes the change");

    resize_column(&mut rt, Column::Name, -40.0);
    let sized = width_of(&rt, Column::Name);
    let count = shown(&rt).len();
    resize(&mut rt, 1300.0, 540.0);
    assert!((width_of(&rt, Column::Name) - sized).abs() < 0.5, "Name keeps its width");
    assert!(shown(&rt).len() >= count);
    resize(&mut rt, 760.0, 540.0);
    assert!((width_of(&rt, Column::Name) - sized).abs() < 0.5, "Name keeps its width");
    assert!(shown(&rt).len() < count, "columns hide at the right end");
    let (last, end) = ends(&rt);
    assert!(last <= end + 0.5);
}

/// Whether each grip lies over its header cell, and the columns end inside the list.
fn assert_grips_over_cells(rt: &Runtime<Phantom>, at: &str) {
    for (c, cell) in header(rt) {
        let grip = rect(rt, &format!("grips/{}", c.id()));
        assert!((grip[0] - cell[0]).abs() < 0.5 && (grip[2] - cell[2]).abs() < 0.5, "{at}: {c:?}: grip {grip:?} over its cell {cell:?}");
    }
    let (last, end) = ends(rt);
    assert!(last <= end + 0.5, "{at}: the columns reach {last}, the list {end}");
}

/// Every edge, in a narrow and a wide window, wider and narrower: the edge is under the pointer
/// until it reaches its bounds or the list's end, where it stops (the column never drags itself
/// out of the list); the grips stay over the cells at every step, and Escape puts it all back.
#[test]
fn every_edge_follows_the_pointer_until_it_stops_and_escape_puts_it_back() {
    for c in [Column::Name, Column::Progress, Column::Size, Column::Down, Column::Up, Column::Peers, Column::Eta] {
        for (w, sign) in [(700.0, 1.0), (700.0, -1.0), (1400.0, 1.0), (1400.0, -1.0)] {
            let mut rt = app(listed());
            resize(&mut rt, w, 540.0);
            if !shown(&rt).contains(&c) {
                continue;
            }
            let start = right_of(&rt, c);
            let (x, y) = rt.find(&format!("grips/{}/resize", c.id())).unwrap();
            rt.pointer_down(x, y);
            let mut stopped = false;
            for step in 1..=40 {
                let dx = sign * 7.0 * step as f32;
                rt.pointer_move(x + dx, y);
                rt.frame(FRAME);
                let at = format!("{c:?} at {w}, {dx}");
                assert!(shown(&rt).contains(&c), "{at}: it dragged itself out");
                let under = (right_of(&rt, c) - (start + dx)).abs() < 0.5;
                assert!(!(stopped && under), "{at}: under the pointer again after it stopped");
                stopped |= !under;
                assert_grips_over_cells(&rt, &at);
            }
            rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            rt.pointer_up(x, y);
            settle(&mut rt);
            assert!((right_of(&rt, c) - start).abs() < 0.5, "{c:?} at {w}: Escape puts the edge back");
            assert_eq!(rt.state.prefs.width(Column::Name), None, "{c:?} at {w}: and Name to taking the rest");
        }
    }
}

/// A drag that comes back to where it began leaves the edge there, not where it last moved from.
#[test]
fn a_drag_back_to_where_it_began_puts_the_edge_back() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    let own = width_of(&rt, Column::Size);
    let (x, y) = rt.find("grips/size/resize").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_move(x + 20.0, y);
    rt.frame(FRAME);
    rt.pointer_move(x, y);
    rt.frame(FRAME);
    assert!((width_of(&rt, Column::Size) - own).abs() < 0.5, "back under the pointer: {own} then {}", width_of(&rt, Column::Size));
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!((width_of(&rt, Column::Size) - own).abs() < 0.5, "and kept there");
}

/// `gui.json` widths far wider or narrower than any list: the first column is cut to the list,
/// the rest hide, the grips lie over the cells, and its edge still drags.
#[test]
fn a_kept_width_wider_than_the_window_is_cut_to_it() {
    let progress_first = ColumnOrder::try_from(vec![
        Column::Progress,
        Column::Name,
        Column::Status,
        Column::Size,
        Column::Down,
        Column::Up,
        Column::Peers,
        Column::Eta,
    ])
    .unwrap();
    for order in [ColumnOrder::default(), progress_first] {
        let widths = ColumnWidths::try_from(
            [(Column::Name, 1.0e30), (Column::Size, 1.0e-30)].into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        )
        .unwrap();
        let mut rt = app(listed());
        rt.state.prefs = WindowPrefs::unkept(Kept { order: order.clone(), widths, ..Kept::default() });
        resize(&mut rt, 1000.0, 540.0);
        assert_grips_over_cells(&rt, &format!("{order:?}"));
        let first = shown(&rt)[0];
        let edge = right_of(&rt, first);
        // Name, cut to the list, can only narrow; Progress, at its own width, widens.
        let dx = if first == Column::Name { -50.0 } else { 50.0 };
        resize_column(&mut rt, first, dx);
        assert!((right_of(&rt, first) - (edge + dx)).abs() < 0.5, "{first:?}'s edge drags");
        assert_grips_over_cells(&rt, &format!("{order:?}, dragged"));
    }
}

/// Name the only column wanted, sized or not: its edge follows the pointer and the grips stay
/// over it.
#[test]
fn names_edge_drags_when_it_is_the_only_column() {
    for sized in [false, true] {
        let widths: std::collections::BTreeMap<Column, f32> =
            if sized { [(Column::Name, 500.0)].into_iter().collect() } else { Default::default() };
        let widths = ColumnWidths::try_from(widths).unwrap();
        let hidden = Column::ALL.iter().copied().filter(|c| *c != Column::Name).collect();
        let mut rt = app(listed());
        rt.state.prefs = WindowPrefs::unkept(Kept { widths, hidden, ..Kept::default() });
        resize(&mut rt, 1000.0, 540.0);
        assert_eq!(shown(&rt), [Column::Name]);
        let start = right_of(&rt, Column::Name);
        let (x, y) = rt.find("grips/name/resize").unwrap();
        rt.pointer_down(x, y);
        for step in 1..=20 {
            let dx = -10.0 * step as f32;
            rt.pointer_move(x + dx, y);
            rt.frame(FRAME);
            assert!((right_of(&rt, Column::Name) - (start + dx)).abs() < 0.5, "sized {sized}, {dx}: under the pointer");
            assert_grips_over_cells(&rt, &format!("sized {sized}, {dx}"));
        }
        rt.pointer_up(x - 200.0, y);
        settle(&mut rt);
        assert_grips_over_cells(&rt, &format!("sized {sized}, let go"));
    }
}

/// A window too narrow for every column: Name dragged narrower gives the room back, so a hidden
/// column comes back once it fits.
#[test]
fn shrinking_name_brings_back_a_hidden_column() {
    let mut rt = app(listed());
    resize(&mut rt, 800.0, 540.0);
    let before = shown(&rt);
    assert!(before.len() < Column::ALL.len(), "something is hidden at 800: {before:?}");
    let name = rect(&rt, "grips/name")[2];
    resize_column(&mut rt, Column::Name, -(name - 130.0));
    let after = shown(&rt);
    assert!(after.len() > before.len(), "a column comes back: {before:?} then {after:?}");
    let (last, end) = ends(&rt);
    assert!(last <= end + 0.5, "the columns stay in the list: {last} of {end}");
}

/// After Name and two more columns are resized, the grips lie exactly over the header's cells, and
/// a double-click at each cell's drawn edge (the resize handle there, not covered by a grip) puts
/// that column back.
#[test]
fn every_resize_handle_can_be_grabbed_after_resizing() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    for (c, dx) in [(Column::Name, -60.0), (Column::Progress, 40.0), (Column::Size, -20.0)] {
        resize_column(&mut rt, c, dx);
    }
    for (c, cell) in header(&rt) {
        let grip = rect(&rt, &format!("grips/{}", c.id()));
        assert!((grip[0] - cell[0]).abs() < 0.5 && (grip[2] - cell[2]).abs() < 0.5, "{c:?}: grip {grip:?} over its cell {cell:?}");
    }
    for c in [Column::Name, Column::Progress, Column::Size] {
        assert!(rt.state.prefs.width(c).is_some(), "{c:?} was resized");
        let cell = header(&rt).into_iter().find(|(h, _)| *h == c).unwrap().1;
        let (x, y) = (cell[0] + cell[2] - 1.0, cell[1] + cell[3] / 2.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        settle(&mut rt);
        assert!(rt.state.prefs.width(c).is_none(), "{c:?}'s edge at {x} gives it back");
    }
}

/// A double-click on Name's edge puts Name back to taking the rest.
#[test]
fn a_double_click_on_names_edge_puts_it_back() {
    let mut rt = app(listed());
    resize(&mut rt, 800.0, 540.0);
    let before = shown(&rt);
    resize_column(&mut rt, Column::Name, -150.0);
    assert!(shown(&rt).len() > before.len(), "a narrower Name brings columns back");
    double_click_at(&mut rt, "grips/name/resize");
    assert_eq!(shown(&rt), before, "back to taking the rest");
    assert!(rt.state.prefs.width(Column::Name).is_none());
}

/// A double-click on another column's edge puts it back to its own width.
#[test]
fn a_double_click_on_a_columns_edge_puts_it_back() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    let own = header(&rt).into_iter().find(|(c, _)| *c == Column::Size).unwrap().1[2];
    resize_column(&mut rt, Column::Size, 50.0);
    let wider = header(&rt).into_iter().find(|(c, _)| *c == Column::Size).unwrap().1[2];
    assert!((wider - own - 50.0).abs() < 1.0, "Size follows the pointer: {own} then {wider}");
    double_click_at(&mut rt, "grips/size/resize");
    let back = header(&rt).into_iter().find(|(c, _)| *c == Column::Size).unwrap().1[2];
    assert!((back - own).abs() < 0.5, "back to {own}: {back}");
}

fn double_click_at(rt: &mut Runtime<Phantom>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    for _ in 0..2 {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
    }
    settle(rt);
}

fn width_of(rt: &Runtime<Phantom>, column: Column) -> f32 {
    header(rt).into_iter().find(|(c, _)| *c == column).unwrap_or_else(|| panic!("{column:?} not shown")).1[2]
}

/// The widths dragged are kept in `gui.json` when the drag is let go, and drawn again after a
/// restart; a width that isn't one (negative, for no column) is ignored, the file read as if it
/// had none.
#[test]
fn dragged_widths_are_kept_and_restored() {
    let kept = Kept_::new();
    let mut rt = app(listed());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 1400.0, 540.0);
    assert_eq!(file::read_at::<ColumnWidths>(&kept.path, COLUMN_WIDTHS), None, "nothing kept before");
    resize_column(&mut rt, Column::Size, 50.0);
    resize_column(&mut rt, Column::Name, -30.0);
    let (kept_widths, size) = (kept.read().widths.clone(), width_of(&rt, Column::Size));
    assert!(kept_widths != ColumnWidths::default(), "kept");

    let mut again = app(listed());
    again.state.prefs = kept.prefs(kept.read());
    resize(&mut again, 1400.0, 540.0);
    assert!((width_of(&again, Column::Size) - size).abs() < 0.5, "Size as it was");
    assert_eq!(again.state.prefs.width(Column::Name), rt.state.prefs.width(Column::Name), "Name's width too");

    for bad in [
        serde_json::json!({"size": -3.0}),
        serde_json::json!({"nonsense": 90.0}),
        serde_json::json!({"status": 90.0}),
        serde_json::json!("wide"),
        // NaN is written as null; 1e39 is no f32 (infinite).
        serde_json::json!({"size": null}),
        serde_json::json!({"size": 1e39}),
    ] {
        file::keep_at(&kept.path, COLUMN_WIDTHS, Some(&bad)).unwrap();
        assert_eq!(file::read_at::<ColumnWidths>(&kept.path, COLUMN_WIDTHS), None, "{bad} ignored");
    }
}

/// A drag cancelled with Escape keeps nothing.
#[test]
fn a_cancelled_resize_keeps_nothing() {
    let kept = Kept_::new();
    let mut rt = app(listed());
    rt.state.prefs = kept.prefs(Kept::default());
    resize(&mut rt, 1400.0, 540.0);
    let own = width_of(&rt, Column::Size);
    let (x, y) = rt.find("grips/size/resize").unwrap();
    rt.pointer_down(x, y);
    for step in 1..=10 {
        rt.pointer_move(x + step as f32 * 5.0, y);
        rt.frame(FRAME);
    }
    assert!(width_of(&rt, Column::Size) > own + 10.0, "it follows the pointer");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    rt.pointer_up(x + 50.0, y);
    settle(&mut rt);
    assert!((width_of(&rt, Column::Size) - own).abs() < 0.5, "back");
    assert_eq!(file::read_at::<ColumnWidths>(&kept.path, COLUMN_WIDTHS), None);
}

/// The columns menu's items, by label, with whether each is ticked; none while it isn't open.
fn columns_menu(rt: &Runtime<Phantom>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "menuitemcheckbox" && e.path.contains("columns-menu"))
        .filter_map(|e| e.label.clone())
        .collect()
}

fn open_columns_menu(rt: &mut Runtime<Phantom>) {
    let (x, y) = rt.find("grips/name/grip").expect("the header");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
}

fn pick_column(rt: &mut Runtime<Phantom>, label: &str) {
    let item = rt.elements().into_iter().find(|e| !e.exiting && e.role == "menuitemcheckbox" && e.label.as_deref() == Some(label)).unwrap();
    let (x, y) = (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// The header's menu stays open while columns are ticked on and off, and closes on Escape or a
/// press outside it.
#[test]
fn the_columns_menu_stays_open_across_picks() {
    let mut rt = app(listed());
    resize(&mut rt, 1400.0, 540.0);
    open_columns_menu(&mut rt);
    assert_eq!(columns_menu(&rt).len(), 8);
    pick_column(&mut rt, "Size");
    assert_eq!(columns_menu(&rt).len(), 8, "open after the first");
    pick_column(&mut rt, "Peers");
    assert_eq!(columns_menu(&rt).len(), 8, "open after the second");
    assert!(!shown(&rt).contains(&Column::Size) && !shown(&rt).contains(&Column::Peers), "both hidden");
    pick_column(&mut rt, "Size");
    assert!(shown(&rt).contains(&Column::Size), "and back");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);
    assert!(columns_menu(&rt).is_empty(), "Escape closes it");

    open_columns_menu(&mut rt);
    assert_eq!(columns_menu(&rt).len(), 8);
    rt.pointer_down(2.0, 2.0);
    rt.pointer_up(2.0, 2.0);
    settle(&mut rt);
    assert!(columns_menu(&rt).is_empty(), "a press outside closes it");
}
