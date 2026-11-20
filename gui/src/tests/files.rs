//! The file menu and the file list's shortcuts (#151), over generated selections of generated files:
//! each file Normal or Off, maybe one missing, a contiguous selection of them. After a right-click on
//! a selected file:
//! - the menu offers exactly what its selection allows: Open and Reveal in Finder for one file that
//!   is on disk, the priorities under their title (the common one checked, none for a mix), and Move
//!   to Trash while a selected file is on disk, which sets those Off and moves them (#276);
//! - every hint it shows is its command's from the one shortcuts table, and pressing that key with
//!   the file list focused does what picking the item does (the same commands, the same Finder
//!   calls), never to the torrent.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{File, Priority, TorrentCommand};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{MIB, app, pick_choice, settle, shift, tap, torrent};
use crate::model::Phantom;
use crate::platform::{Show, Shown};
use crate::service::MemoryService;
use crate::shortcuts::{self, Command};

const NAMES: [&str; 3] = ["01 Intro.flac", "02 Song.flac", "Cover/front.jpg"];

/// The album, its files with these priorities, one of them maybe missing (which stops the torrent).
fn album(offs: [bool; 3], progress: [bool; 3], missing: Option<usize>) -> MemoryService {
    let file = |index: usize| File {
        index,
        path: NAMES[index].into(),
        size_bytes: (index as u64 + 1) * 100 * MIB,
        progress_bytes: if progress[index] { 10 * MIB } else { 0 },
        priority: if offs[index] { Priority::Off } else { Priority::Normal },
        missing: false,
        on_disk: false,
    };
    let service = MemoryService::new().with_torrents(vec![torrent(1, "Album")]).with_files(1, (0..3).map(file).collect());
    match missing {
        Some(i) => service.with_missing(1, &[i]),
        None => service,
    }
}

/// The album's pane open with files `start..start + len` selected, the file list focused.
fn selected(service: MemoryService, start: usize, len: usize) -> Runtime<Phantom> {
    let mut rt = app(service);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, &format!("files/viewport/rows/{start}"));
    for _ in 1..len {
        rt.key(shift(Key::Down));
    }
    settle(&mut rt);
    rt
}

fn right_click_file(rt: &mut Runtime<Phantom>, row: usize) {
    let (x, y) = rt.find(&format!("files/viewport/rows/{row}/path")).unwrap_or_else(|| panic!("no file row {row}"));
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
}

/// An item as the menu draws it: its label, its hint, and for a choice among values whether it's
/// checked.
type Item = (String, Option<String>, Option<bool>);

/// What the open file menu draws: each item, `None` for a separator.
fn drawn(rt: &Runtime<Phantom>) -> Vec<Option<Item>> {
    let elements = rt.elements();
    let surface = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("file-menu-menu")).expect("a file menu");
    let inside = format!("{}/", surface.path);
    elements
        .iter()
        .filter(|e| !e.exiting && e.path.starts_with(&inside))
        .filter_map(|e| match (e.role, e.key.as_deref()) {
            ("menuitem" | "menuitemcheckbox", _) => {
                let label = e.label.clone()?;
                let below = format!("{}/", e.path);
                let hint = elements.iter().filter(|c| c.path.starts_with(&below)).filter_map(|c| c.text.clone()).find(|t| *t != label);
                // The check is the icon in the gutter, where the row menu has its Copy icon; the kit's own
                // mark at the right edge is blanked (the item is still a menuitemcheckbox, whose checked
                // state screen readers hear).
                let checked = (e.role == "menuitemcheckbox")
                    .then(|| elements.iter().any(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("icon") && !c.exiting));
                Some(Some((label, hint, checked)))
            }
            (_, Some(key)) if key.starts_with("sep") => Some(None),
            _ => None,
        })
        .collect()
}

/// Whether the menu draws its section title.
fn has_title(rt: &Runtime<Phantom>, title: &str) -> bool {
    let elements = rt.elements();
    let Some(surface) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("file-menu-menu")) else { return false };
    let inside = format!("{}/", surface.path);
    elements.iter().any(|e| !e.exiting && e.path.starts_with(&inside) && e.text.as_deref() == Some(title))
}

/// What the menu must offer, from the files and the selection alone: items with their hints and
/// checks, `None` for a separator. A priority is checked only while every selected file has it.
fn expected(
    offs: [bool; 3],
    progress: [bool; 3],
    missing: Option<usize>,
    start: usize,
    len: usize,
) -> Vec<Option<(&'static str, Option<&'static str>, Option<bool>)>> {
    // A file set Off isn't one the torrent misses; one it gets that's gone stops the torrent, which Open and
    // Reveal then don't offer. On disk is what's at a file's path, whatever it downloaded (#276): every
    // file but the one deleted, Off or not.
    let _ = progress;
    let torrent_missing = missing.is_some_and(|i| !offs[i]);
    let on_disk = |i: usize| missing != Some(i);
    let one_on_disk = len == 1 && on_disk(start) && !torrent_missing;
    let any_on_disk = (start..start + len).any(on_disk);
    let all = |off: bool| offs[start..start + len].iter().all(|o| *o == off);
    let mut items = Vec::new();
    if one_on_disk {
        items.extend([Some(("Open", Some("⌘O"), None)), Some(("Reveal in Finder", Some("⇧⌘O"), None)), None]);
    }
    items.extend([Some(("Normal", None, Some(all(false)))), Some(("Off", None, Some(all(true))))]);
    // Hidden with no selected file on disk, as Open and Reveal are.
    if any_on_disk {
        items.extend([None, Some(("Move to Trash", Some("⌘⌫"), None))]);
    }
    items
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32);
    Config { cases, ..Config::default() }
}

/// A selection: where it starts and how many files, within the three.
fn selection() -> impl Strategy<Value = (usize, usize)> {
    (0..3usize).prop_flat_map(|start| (Just(start), 1..=3 - start))
}

proptest! {
    #![proptest_config(config())]

    /// The menu offers exactly what its selection allows, each hint is its command's, and a
    /// priority is checked only when every selected file has it.
    #[test]
    fn the_file_menu_offers_what_its_selection_allows(
        offs in any::<[bool; 3]>(),
        progress in any::<[bool; 3]>(),
        missing in prop::option::weighted(0.3, 0..3usize),
        (start, len) in selection(),
    ) {
        let mut rt = selected(album(offs, progress, missing), start, len);
        right_click_file(&mut rt, start);
        let menu = drawn(&rt);
        let want = expected(offs, progress, missing, start, len);
        let shown: Vec<Option<(&str, Option<&str>, Option<bool>)>> =
            menu.iter().map(|i| i.as_ref().map(|(l, h, c)| (l.as_str(), h.as_deref(), *c))).collect();
        prop_assert_eq!(shown, want);
        prop_assert!(has_title(&rt, "Priority"), "the priorities sit under their title");
        // Each hint is the one the table shows for the item's command.
        for (label, hint, _) in menu.iter().flatten() {
            let command = match label.as_str() {
                "Open" => Some(Command::Open),
                "Reveal in Finder" => Some(Command::Reveal),
                "Move to Trash" => Some(Command::Delete),
                _ => None,
            };
            let shown_for = command.map(shortcuts::hint);
            prop_assert_eq!(hint.as_deref(), shown_for.as_deref(), "{}", label);
        }
        // The title isn't something to pick.
        let picked = rt.elements().iter().filter(|e| e.role == "menuitem" && e.label.as_deref() == Some("Priority")).count();
        prop_assert_eq!(picked, 0);
    }

    /// Each item with a hint does the same picked as keyed (with the file list focused): the same
    /// commands, for the torrent's files, and the same call to Finder.
    #[test]
    fn every_hint_the_file_menu_shows_runs_its_item(
        offs in any::<[bool; 3]>(),
        missing in prop::option::weighted(0.3, 0..3usize),
        (start, len) in selection(),
        which in 0..3usize,
    ) {
        let command = [Command::Open, Command::Reveal, Command::Delete][which];
        let label = ["Open", "Reveal in Finder", "Move to Trash"][which];
        let effect = |keyed: bool| {
            let service = album(offs, [false; 3], missing);
            let probe = service.probe();
            let mut rt = selected(service, start, len);
            let shown = Shown::default();
            rt.state.finder = Box::new(shown.clone());
            if keyed {
                rt.key(shortcuts::keys(command)[0].event());
                settle(&mut rt);
            } else {
                right_click_file(&mut rt, start);
                if drawn(&rt).iter().flatten().all(|(l, ..)| l != label) {
                    // Not offered for this selection: nothing to pick.
                    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                    settle(&mut rt);
                } else {
                    pick_choice(&mut rt, "file-menu", label);
                }
            }
            let priorities: Vec<Priority> = rt.state.pane.details.as_ref().map(|d| d.files.iter().map(|f| f.priority).collect()).unwrap_or_default();
            (probe.commands(), shown.all(), priorities, rt.state.torrents.len())
        };
        let (picked, keyed) = (effect(false), effect(true));
        prop_assert_eq!(&picked, &keyed, "{:?} keyed or picked", command);
        // Never the torrent: ⌘⌫ with a file focused moves files, it doesn't Delete the torrent.
        prop_assert_eq!(keyed.3, 1);
        prop_assert!(keyed.0.iter().all(|(_, c)| !matches!(c, TorrentCommand::Delete | TorrentCommand::Remove)));
        // Move to Trash moves only the selected files on disk, and is nothing with none (#276).
        if command == Command::Delete {
            let there: Vec<usize> = (start..start + len).filter(|i| missing != Some(*i)).collect();
            let sent: Vec<&Vec<usize>> = keyed.0.iter().filter_map(|(_, c)| match c {
                TorrentCommand::TrashFiles { files } => Some(files),
                _ => None,
            }).collect();
            let want: Vec<&Vec<usize>> = if there.is_empty() { Vec::new() } else { vec![&there] };
            prop_assert_eq!(sent, want);
        }
    }
}

/// Every command the table gives the file list maps to the item whose own shortcut it is, and no
/// other: the menu's hint and the key can't disagree.
#[test]
fn the_file_menu_and_the_keys_share_one_table() {
    use crate::model::FileItem;
    for (command, _) in shortcuts::TABLE {
        if let Some(item) = shortcuts::file_item(command) {
            assert_eq!(crate::views::file_menu::shortcut(item), Some(command), "{command:?}");
        }
    }
    for item in
        [FileItem::Open, FileItem::Reveal, FileItem::Priority(Priority::Normal), FileItem::Priority(Priority::Off), FileItem::MoveToTrash]
    {
        if let Some(command) = crate::views::file_menu::shortcut(item) {
            assert_eq!(shortcuts::file_item(command), Some(item), "{item:?}");
        }
    }
}

/// With the file list focused, Open and Reveal go to the selected file, not the torrent: the path of
/// the file in its folder (or the torrent's own location for a single file).
#[test]
fn open_and_reveal_with_a_file_focused_go_to_that_file() {
    let service = album([false; 3], [false; 3], None);
    let mut rt = selected(service, 1, 1);
    let shown = Shown::default();
    rt.state.finder = Box::new(shown.clone());
    let folder = rt.state.pane.details.as_ref().unwrap().location.to_path_buf();
    rt.key(shortcuts::keys(Command::Open)[0].event());
    rt.key(shortcuts::keys(Command::Reveal)[0].event());
    settle(&mut rt);
    let file = folder.join(NAMES[1]);
    assert_eq!(shown.all(), [(file.clone(), Show::Open), (file, Show::Reveal)]);
}

/// The torrent list keeps ⌘⌫ for torrents: the same key deletes the torrent when it has focus.
#[test]
fn the_same_key_deletes_the_torrent_when_the_torrent_list_has_focus() {
    let service = album([false; 3], [false; 3], None);
    let probe = service.probe();
    let mut rt = app(service);
    tap(&mut rt, "rows/1");
    rt.key(shortcuts::keys(Command::Delete)[0].event());
    settle(&mut rt);
    assert_eq!(probe.commands(), [(vec![1], TorrentCommand::Delete)]);
}

/// #276: Move to Trash of a selection where one file isn't on disk (deleted by hand) moves only
/// those that are, and its notice counts those.
#[test]
fn move_to_trash_of_a_mixed_selection_moves_only_the_files_on_disk() {
    let service = album([false; 3], [true; 3], Some(1));
    let probe = service.probe();
    let mut rt = selected(service, 0, 3);
    rt.key(shortcuts::keys(Command::Delete)[0].event());
    for _ in 0..30 {
        settle(&mut rt);
    }
    let sent: Vec<TorrentCommand> =
        probe.commands().into_iter().map(|(_, c)| c).filter(|c| matches!(c, TorrentCommand::TrashFiles { .. })).collect();
    assert_eq!(sent, [TorrentCommand::TrashFiles { files: vec![0, 2] }]);
    let told = rt.state.bar.newest().map(|n| n.short.clone());
    assert_eq!(told.as_deref(), Some("Moved 2 files to the Trash"));
}

/// #276, the user's steps: a file downloaded, set Off, then moved to the Trash shows none of it,
/// and its menu offers no Move to Trash (nor Open). Set Normal again, it's downloaded from nothing,
/// not Missing files. ⌘Z puts the old copy back, with what it had, and the torrent is checked.
#[test]
fn a_file_set_off_and_moved_to_the_trash_shows_none_and_its_undo_puts_it_back() {
    let service = album([true, false, false], [true; 3], None);
    let probe = service.probe();
    let mut rt = selected(service, 0, 1);
    let file = |rt: &Runtime<Phantom>| rt.state.pane.details.as_ref().unwrap().files[0].clone();
    assert_eq!(file(&rt).progress_bytes, 10 * MIB);
    rt.key(shortcuts::keys(Command::Delete)[0].event());
    for _ in 0..10 {
        settle(&mut rt);
    }
    let gone = file(&rt);
    assert!(!gone.on_disk && gone.progress_bytes == 0, "in the Trash, it shows none of it: {gone:?}");
    right_click_file(&mut rt, 0);
    let menu: Vec<String> = drawn(&rt).into_iter().flatten().map(|(label, ..)| label).collect();
    assert_eq!(menu, ["Normal", "Off"], "nothing to move or open");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0/priority");
    for _ in 0..10 {
        settle(&mut rt);
    }
    let again = file(&rt);
    assert!(again.on_disk && !again.missing && again.progress_bytes < 10 * MIB, "downloaded from nothing: {again:?}");
    assert_ne!(rt.state.torrents.iter().find(|t| t.id == 1).unwrap().status, proto::Status::MissingFiles);
    rt.key(shortcuts::keys(Command::Undo)[0].event());
    for _ in 0..200 {
        settle(&mut rt);
    }
    let back = file(&rt);
    assert_eq!((back.priority, back.on_disk, back.progress_bytes), (Priority::Off, true, 10 * MIB), "the old copy is back");
    assert!(probe.commands().iter().all(|(_, c)| !matches!(c, TorrentCommand::Check)), "the service checks it itself");
}

/// #276: an album file set Off, deleted by hand, shows 0 % once it's let go of, and still does when
/// it's put back by hand: the service has let go of its pieces, and a check finds them again. A
/// named test, as a pinned generated seed goes stale once the strategy changes shape.
#[test]
fn an_off_file_deleted_by_hand_and_put_back_shows_none_of_it() {
    use crate::service::DiskChange;
    let service = album([true, false, false], [true; 3], None);
    let disk = service.disk();
    let mut rt = selected(service, 0, 1);
    let file = |rt: &Runtime<Phantom>| rt.state.pane.details.as_ref().unwrap().files[0].clone();
    assert_eq!(file(&rt).progress_bytes, 10 * MIB, "set Off, it still has what it had");
    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![0] });
    for _ in 0..10 {
        settle(&mut rt);
    }
    let gone = file(&rt);
    assert!(!gone.on_disk && gone.progress_bytes == 0, "deleted: {gone:?}");
    disk.change(DiskChange::FilesPutBack { id: 1, files: vec![0] });
    for _ in 0..10 {
        settle(&mut rt);
    }
    let back = file(&rt);
    assert!(back.on_disk && back.progress_bytes == 0, "put back: let go of till a check: {back:?}");
}
