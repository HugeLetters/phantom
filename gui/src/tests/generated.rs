//! Generated UI sequences against `MemoryService`: a person adding magnets, selecting rows,
//! pausing, resuming, checking, removing, deleting and setting file priorities, while files are
//! deleted and put back outside Phantom, destinations go and come back, and time passes. After every step the screen must agree
//! with the app's state and the service's answers must make sense together; the reference model
//! only knows which rows there are and which one is selected, so steps can name them.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES=1000 cargo test -p gui generated` runs more. Failing
//! sequences, shrunk, are saved to `gui/proptest-regressions/` and run first after.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{Action as Do, Status, TorrentId, Unavailable};
use rune::Runtime;

use super::{app, enabled, settle, wait, with_an_album};
use crate::model::{Phantom, details_torrent, selected_for, selected_torrents, shown_torrent, size_label};
use crate::service::{Disk, DiskChange};

/// Magnets by number: each its own info hash.
const MAGNETS: u8 = 3;
/// At most this many rows, so every one is on screen with the pane open.
const MAX_ROWS: usize = 6;

fn magnet(n: u8) -> String {
    format!("magnet:?xt=urn:btih:{:040x}&dn=Magnet+{n}", 0xa0 + n as u64)
}

#[derive(Debug, Clone)]
enum Action {
    PasteMagnet(u8),
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

#[derive(Debug, Clone)]
struct Rows {
    /// Each row's magnet, if it was added from one.
    rows: BTreeMap<TorrentId, Option<u8>>,
    selected: Option<TorrentId>,
    next_id: TorrentId,
}

struct Screen;

impl ReferenceStateMachine for Screen {
    type State = Rows;
    type Transition = Action;

    fn init_state() -> BoxedStrategy<Rows> {
        Just(Rows { rows: [(0, None), (1, None)].into(), selected: None, next_id: 2 }).boxed()
    }

    fn transitions(rows: &Rows) -> BoxedStrategy<Action> {
        let ids: Vec<TorrentId> = rows.rows.keys().copied().collect();
        let files = proptest::collection::vec(0..3usize, 1..3);
        let paste = (0..MAGNETS).prop_map(Action::PasteMagnet);
        if ids.is_empty() {
            return prop_oneof![paste, Just(Action::Wait(2.5))].boxed();
        }
        let id = proptest::sample::select(ids);
        prop_oneof![
            2 => paste,
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
            2 => prop_oneof![Just(0.5), Just(1.5), Just(2.5)].prop_map(Action::Wait),
        ]
        .boxed()
    }

    fn preconditions(rows: &Rows, action: &Action) -> bool {
        match action {
            Action::PasteMagnet(_) => rows.rows.len() < MAX_ROWS,
            Action::Select(id)
            | Action::FilesDeleted { id, .. }
            | Action::FilesPutBack { id, .. }
            | Action::DestinationGone(id)
            | Action::DestinationBack(id) => rows.rows.contains_key(id),
            _ => true,
        }
    }

    fn apply(mut rows: Rows, action: &Action) -> Rows {
        match action {
            Action::PasteMagnet(n) => match rows.rows.iter().find(|(_, m)| **m == Some(*n)) {
                // Already in the list: selected instead.
                Some((&id, _)) => rows.selected = Some(id),
                None => {
                    rows.rows.insert(rows.next_id, Some(*n));
                    rows.next_id += 1;
                }
            },
            Action::Select(id) => rows.selected = Some(*id),
            Action::Remove { .. } => {
                if let Some(id) = rows.selected.take() {
                    rows.rows.remove(&id);
                }
            }
            _ => {}
        }
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

    fn init_test(_: &Rows) -> Sut {
        let service = with_an_album();
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
            Action::PasteMagnet(n) => rt.paste(&magnet(*n)),
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
        settle(rt);

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

    fn check_invariants(sut: &Sut, _: &Rows) {
        let rt = &sut.rt;
        let s = &rt.state;
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

/// Presses the element at `key`, if it's there; a disabled button does nothing.
fn press(rt: &mut Runtime<Phantom>, key: &str) {
    if let Some((x, y)) = rt.find(key) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(rt);
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
