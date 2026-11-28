//! The menu bar as it's built (#377): what it holds in each context, what a pick does, and the keys
//! it shows run in the window as they do without it.

use super::*;
use crate::menu_bar::{Around, Item, Keys, Menu, Own, Pick, build, run};
use crate::model::palette;
use crate::platform::menu_bar::around;

const MAIN: Around = Around { front: Some(Which::Main), panel: false };
const SETTINGS: Around = Around { front: Some(Which::Settings), panel: false };
const NO_WINDOW: Around = Around { front: None, panel: false };
const PANEL: Around = Around { front: Some(Which::Main), panel: true };

fn own_line(own: &Own, indent: &str) -> String {
    let keys = match own.keys {
        Keys::None => String::new(),
        Keys::Matched(k) | Keys::Shown(k) => format!("  {}", k.hint()),
    };
    let tick = if own.checked { "✓ " } else { "" };
    format!("{indent}{tick}{}{keys}{}", own.title, if own.enabled { "" } else { "  (greyed)" })
}

/// A menu as its items show, a line each: the title, its key, what's greyed.
fn dump(menu: &Menu) -> Vec<String> {
    let mut lines = Vec::new();
    for item in &menu.items {
        match item {
            Item::Separator => lines.push("—".into()),
            Item::Own(own) => lines.push(own_line(own, "")),
            Item::Submenu { title, enabled, items } => {
                lines.push(format!("{title} ▸{}", if *enabled { "" } else { "  (greyed)" }));
                lines.extend(items.iter().map(|i| own_line(i, "    ")));
            }
            Item::Standard { shown: false, .. } => {}
            Item::Standard { title, keys, enabled, .. } => {
                lines.push(format!("{title}  {}{}", keys.hint(), if *enabled { "" } else { "  (greyed)" }))
            }
            Item::System { title, keys } => lines.push(format!("{title}{}", keys.map_or(String::new(), |k| format!("  {}", k.hint())))),
        }
    }
    lines
}

fn menu(s: &Phantom, around: Around, name: &str) -> Vec<String> {
    dump(build(s, around).iter().find(|m| m.name == name).expect("a menu"))
}

fn enabled(s: &Phantom, around: Around, id: &str) -> bool {
    item(s, around, id).enabled
}

fn item(s: &Phantom, around: Around, id: &str) -> Own {
    build(s, around).iter().flat_map(Menu::own).find(|o| o.id == id).unwrap_or_else(|| panic!("no {id}")).clone()
}

/// Picks item `id` as the menu bar has it now.
fn pick(rt: &mut Runtime<Phantom>, id: &str) {
    let own = item(&rt.state, around(&rt.state), id);
    assert!(own.enabled, "{id} is greyed");
    pick_old(rt, own.pick);
}

/// Picks an item a menu opened earlier had.
fn pick_old(rt: &mut Runtime<Phantom>, pick: Pick) {
    rt.update(move |s, tx| run(s, &pick, around(s), tx));
    settle(rt);
}

fn cmd(c: char) -> KeyEvent {
    KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, ..Mods::default() } }
}

fn plain(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods::default() }
}

fn press(rt: &mut Runtime<Phantom>, ev: KeyEvent) {
    rt.key(ev);
    settle(rt);
}

fn space(rt: &mut Runtime<Phantom>) {
    press(rt, plain(Key::Text(" ".into())));
}

/// Three torrents: two running, the third paused; the first two selected if `both`.
fn three() -> Runtime<Phantom> {
    let t = |id, name: &str, status| crate::t(id, name, status, 10, 100, 0, 0, 0);
    let mut rt = app(MemoryService::new().with_torrents(vec![
        t(1, "Album", Status::Downloading),
        t(2, "Beta", Status::Downloading),
        t(3, "Cut", Status::Paused),
    ]));
    settle(&mut rt);
    rt
}

fn select(rt: &mut Runtime<Phantom>, id: u64) {
    tap(rt, &format!("rows/{id}"));
    settle(rt);
}

fn status(rt: &Runtime<Phantom>, id: u64) -> Status {
    rt.state.torrents.iter().find(|t| t.id == id).expect("listed").status.clone()
}

fn focused_in_files(rt: &Runtime<Phantom>) -> bool {
    rt.elements().iter().any(|e| e.focused && !e.exiting && e.path.contains("files"))
}

fn focus_link(rt: &mut Runtime<Phantom>) {
    tap(rt, "magnet-field");
    settle(rt);
    assert!(rt.state.magnet.focused);
}

/// The table of #377, with one running torrent selected.
#[test]
fn the_menus_with_a_torrent_selected() {
    let mut rt = three();
    select(&mut rt, 1);
    let s = &rt.state;
    let menus: Vec<_> = build(s, MAIN).iter().map(|m| m.name).collect();
    assert_eq!(menus, ["Phantom", "File", "Edit", "Torrent", "View", "Window", "Help"]);
    assert_eq!(menu(s, MAIN, "Phantom"), ["About Phantom", "Settings…  ⌘,", "—", "Hide Phantom  ⌘H", "—", "Quit Phantom  ⌘Q"]);
    assert_eq!(menu(s, MAIN, "File"), ["Add File…  ⇧⌘A", "—", "Close Window  ⌘W"]);
    assert_eq!(menu(s, MAIN, "Edit"), ["Undo  ⌘Z  (greyed)", "—", "Copy Magnet Link  ⌘C", "Paste and Add  ⌘V", "Select All  ⌘A"]);
    let titles: Vec<String> = menu(s, MAIN, "Torrent").into_iter().map(|l| l.trim_end_matches("  (greyed)").to_owned()).collect();
    assert_eq!(
        titles,
        [
            "Open  ⌘O",
            "Reveal in Finder  ⇧⌘O",
            "—",
            "Pause  Space",
            "Check",
            "Cancel Check",
            "Cancel Move",
            "—",
            "Move To…",
            "Rename…",
            "—",
            "Remove  ⌫",
            "Delete  ⌘⌫"
        ]
    );
    for (id, on) in [("pause", true), ("cancel-check", false), ("cancel-move", false), ("rename", true), ("remove", true)] {
        assert_eq!(enabled(s, MAIN, id), on, "{id}");
    }
    assert_eq!(menu(s, MAIN, "View").last().map(String::as_str), Some("Command Palette…  ⌘P"));
    assert_eq!(menu(s, MAIN, "Window"), ["Minimize  ⌘M", "Zoom", "—", "Phantom"]);
    assert!(menu(s, MAIN, "Help").is_empty(), "macOS's search only");
}

#[test]
fn the_edit_menu_by_context() {
    let mut rt = three();
    let edit = |rt: &Runtime<Phantom>, around| menu(&rt.state, around, "Edit");
    let nothing = ["Undo  ⌘Z  (greyed)", "—", "Copy  ⌘C  (greyed)", "Paste and Add  ⌘V", "Select All  ⌘A"];
    assert_eq!(edit(&rt, MAIN), nothing, "nothing selected");
    let off = ["Undo  ⌘Z  (greyed)", "—", "Copy  ⌘C  (greyed)", "Paste  ⌘V  (greyed)", "Select All  ⌘A  (greyed)"];
    assert_eq!(edit(&rt, SETTINGS), off, "Settings in front, no field");
    assert_eq!(edit(&rt, NO_WINDOW), off, "no window");
    let field = ["Undo  ⌘Z", "Redo  ⇧⌘Z", "—", "Cut  ⌘X", "Copy  ⌘C", "Paste  ⌘V", "Select All  ⌘A"];
    rt.state.settings_field = true;
    assert_eq!(edit(&rt, SETTINGS), field, "a field of Settings");
    assert_eq!(edit(&rt, PANEL), field, "a file picker's field");
    assert_eq!(edit(&rt, MAIN), nothing, "Settings' field isn't the main window's");
    select(&mut rt, 1);
    focus_link(&mut rt);
    assert_eq!(edit(&rt, MAIN), field, "the link field, with a torrent selected");
}

#[test]
fn undo_names_the_change_and_is_greyed_without_one() {
    let mut rt = three();
    select(&mut rt, 1);
    pick(&mut rt, "remove");
    assert_eq!(menu(&rt.state, MAIN, "Edit")[0], "Undo Remove of Album  ⌘Z");
    press(&mut rt, cmd('z'));
    assert_eq!(menu(&rt.state, MAIN, "Edit")[0], "Undo  ⌘Z  (greyed)");
    assert_eq!(rt.state.torrents.len(), 3);
}

/// Behind a dialog, the palette, a menu, Settings, a file picker, or with no window, no command of
/// the list is offered; nor with nothing selected.
#[test]
fn what_is_behind_something_is_greyed() {
    let mut rt = three();
    let torrent = ["open", "pause", "check", "cancel-check", "cancel-move", "move", "rename", "remove", "delete", "reveal"];
    let none = |rt: &Runtime<Phantom>, around, why: &str| {
        for id in torrent.into_iter().chain(["palette", "sort-name", "column-size"]) {
            assert!(!enabled(&rt.state, around, id), "{id}: {why}");
        }
    };
    assert!(torrent.iter().all(|id| !enabled(&rt.state, MAIN, id)), "nothing selected");
    select(&mut rt, 1);
    assert!(enabled(&rt.state, MAIN, "remove") && enabled(&rt.state, MAIN, "palette"));
    for (around, why) in [(SETTINGS, "Settings"), (NO_WINDOW, "no window"), (PANEL, "a panel")] {
        // The palette opens in Settings too (#434): its item stays enabled there, as a pick opens
        // Settings' own.
        if around == SETTINGS {
            assert!(enabled(&rt.state, around, "palette"), "the palette's item is on in Settings");
            assert!(torrent.iter().chain(&["sort-name", "column-size"]).all(|id| !enabled(&rt.state, around, id)), "{why}");
        } else {
            none(&rt, around, why);
        }
    }
    assert!(enabled(&rt.state, NO_WINDOW, "add-file") && enabled(&rt.state, SETTINGS, "add-file"));
    assert!(!enabled(&rt.state, NO_WINDOW, "close-window") && enabled(&rt.state, NO_WINDOW, "main-window"));
    right_click_row(&mut rt, 1);
    none(&rt, MAIN, "the row menu");
    press(&mut rt, plain(Key::Escape));
    rt.update(|s, _| palette::open(s));
    settle(&mut rt);
    none(&rt, MAIN, "the palette");
    assert!(!enabled(&rt.state, MAIN, "add-file"));
    press(&mut rt, plain(Key::Escape));
    rt.state.clash = Some(clash());
    none(&rt, MAIN, "the clash dialog");
    press(&mut rt, cmd('p'));
    assert!(!rt.state.palette.is_open(), "nor by its key");
    assert_eq!(menu(&rt.state, MAIN, "Edit")[3], "Paste  ⌘V  (greyed)");
}

fn clash() -> crate::model::moving::Clash {
    crate::model::moving::Clash {
        id: 1,
        name: "Album".into(),
        to: proto::AbsolutePath::new("/Films").unwrap(),
        files: vec![proto::AbsolutePath::new("/Films/Album/a").unwrap()],
        remaining: 0,
        at: 1,
        of: 1,
        apply_to_rest: false,
        other: None,
        renaming: None,
        explained: None,
    }
}

/// ⌫ and ⌘⌫ in the link field are the field's, and Remove and Delete are greyed meanwhile; so is
/// Space. In the list each runs once.
#[test]
fn a_fields_keys_never_reach_the_torrents() {
    let mut rt = three();
    select(&mut rt, 1);
    focus_link(&mut rt);
    assert!(!enabled(&rt.state, MAIN, "remove") && !enabled(&rt.state, MAIN, "delete"));
    press(&mut rt, plain(Key::Backspace));
    press(&mut rt, KeyEvent { key: Key::Backspace, mods: Mods { cmd: true, ..Mods::default() } });
    space(&mut rt);
    press(&mut rt, cmd('c'));
    assert!(rt.copy_request.take().is_none(), "no magnet link from an empty field");
    assert_eq!((rt.state.torrents.len(), status(&rt, 1)), (3, Status::Downloading));
    select(&mut rt, 1);
    press(&mut rt, plain(Key::Backspace));
    assert_eq!(rt.state.torrents.len(), 2, "one, once");
}

/// With the file list focused, ⌘⌫ is the files' (Trash) and ⌫ nothing of the torrent's: Remove and
/// Delete show no key, stay pickable, and follow the focus back to the list.
#[test]
fn the_file_lists_focus_takes_removes_keys_off_its_items() {
    let file = |index: usize| proto::File {
        index,
        path: format!("0{index}.flac").into(),
        size_bytes: 100,
        progress_bytes: 0,
        priority: proto::Priority::Normal,
        missing: false,
        on_disk: false,
    };
    let mut rt = app(MemoryService::new()
        .with_torrents(vec![crate::t(1, "Album", Status::Downloading, 10, 100, 0, 0, 0)])
        .with_files(1, (0..3).map(file).collect()));
    settle(&mut rt);
    select(&mut rt, 1);
    let keys = |rt: &Runtime<Phantom>| (item(&rt.state, MAIN, "remove"), item(&rt.state, MAIN, "delete"));
    let (remove, delete) = keys(&rt);
    assert!(matches!(remove.keys, Keys::Shown(_)) && matches!(delete.keys, Keys::Shown(_)), "the list's keys");
    tap(&mut rt, "files/viewport/rows/0");
    assert!(focused_in_files(&rt), "the file list has the focus");
    let (remove, delete) = keys(&rt);
    assert_eq!((remove.keys, delete.keys), (Keys::None, Keys::None), "no key");
    assert!(remove.enabled && delete.enabled, "still pickable");
    select(&mut rt, 1);
    let (remove, delete) = keys(&rt);
    assert!(matches!(remove.keys, Keys::Shown(_)) && matches!(delete.keys, Keys::Shown(_)), "the list again");
}

/// Space in the list, and Torrent ▸ Pause or Resume, go by the focused torrent.
#[test]
fn space_toggles_the_selection_by_the_focused_torrent() {
    let mut rt = three();
    select(&mut rt, 1);
    press(&mut rt, cmd('a'));
    assert_eq!(selected_torrents(&rt.state.table).len(), 3);
    assert_eq!(item(&rt.state, MAIN, "pause").title, "Pause");
    space(&mut rt);
    assert_eq!([1, 2, 3].map(|id| status(&rt, id)), [Status::Paused, Status::Paused, Status::Paused]);
    assert_eq!(item(&rt.state, MAIN, "resume").title, "Resume");
    pick(&mut rt, "resume");
    assert!([1, 2, 3].iter().all(|id| status(&rt, *id) != Status::Paused));
    press(&mut rt, plain(Key::Escape));
    space(&mut rt);
    assert!([1, 2, 3].iter().all(|id| status(&rt, *id) != Status::Paused), "nothing selected, nothing done");
}

#[test]
fn an_error_counts_as_running() {
    let failure = proto::TorrentFailure::DiskFull;
    let mut rt = app(MemoryService::new().with_torrents(vec![crate::t(1, "Album", Status::Error { failure }, 1, 2, 0, 0, 0)]));
    select(&mut rt, 1);
    assert_eq!(crate::commands::toggle(&rt.state), Some(OnTorrents::Pause));
}

/// A pick of a menu left open acts on what it was opened for, or not at all.
#[test]
fn a_pick_from_an_old_menu_does_nothing_once_the_selection_changed() {
    let mut rt = three();
    select(&mut rt, 1);
    let (remove, pause) = (item(&rt.state, MAIN, "remove").pick, item(&rt.state, MAIN, "pause").pick);
    select(&mut rt, 2);
    pick_old(&mut rt, remove.clone());
    assert_eq!(rt.state.torrents.len(), 3, "another is selected now");
    select(&mut rt, 1);
    pick_old(&mut rt, pause.clone());
    assert_eq!(status(&rt, 1), Status::Paused);
    pick_old(&mut rt, pause);
    assert_eq!(status(&rt, 1), Status::Paused, "the item is Resume now");
    pick_old(&mut rt, remove.clone());
    assert_eq!(rt.state.torrents.len(), 2, "the same selection: it acts");
    pick_old(&mut rt, remove);
    assert_eq!(rt.state.torrents.len(), 2);
}

/// Nor once its item is greyed: for the same torrent, but with a field focused, a file picker or
/// Settings in front, or the palette open.
#[test]
fn a_pick_from_an_old_menu_does_nothing_once_its_item_is_greyed() {
    let mut rt = three();
    select(&mut rt, 1);
    let delete = item(&rt.state, MAIN, "delete").pick;
    for around in [PANEL, SETTINGS, NO_WINDOW] {
        let delete = delete.clone();
        rt.update(move |s, tx| run(s, &delete, around, tx));
        settle(&mut rt);
        assert_eq!(rt.state.torrents.len(), 3, "{around:?}");
    }
    focus_link(&mut rt);
    pick_old(&mut rt, delete.clone());
    assert_eq!(rt.state.torrents.len(), 3, "a field");
    select(&mut rt, 1);
    rt.update(|s, _| palette::open(s));
    settle(&mut rt);
    pick_old(&mut rt, delete);
    assert_eq!(rt.state.torrents.len(), 3, "the palette");
}

/// ⌘C copies each selected torrent's link, a line each.
#[test]
fn copy_copies_every_selected_torrents_link() {
    let mut rt = three();
    select(&mut rt, 1);
    press(&mut rt, cmd('a'));
    press(&mut rt, cmd('c'));
    let copied = rt.copy_request.take().expect("copied");
    let links: Vec<String> = rt.state.torrents.iter().map(client::magnet_link).collect();
    assert_eq!(copied.lines().collect::<Vec<_>>(), links);
}

#[test]
fn view_ticks_the_columns_and_the_sort() {
    let mut rt = three();
    assert!(item(&rt.state, MAIN, "column-size").checked);
    pick(&mut rt, "column-size");
    assert!(!item(&rt.state, MAIN, "column-size").checked);
    pick(&mut rt, "sort-name");
    let first = rt.state.torrents[0].id;
    assert!(item(&rt.state, MAIN, "sort-name").checked && !item(&rt.state, MAIN, "sort-down").checked);
    pick(&mut rt, "sort-name");
    assert_ne!(rt.state.torrents[0].id, first, "again, the other way");
}

/// File ▸ Close Window closes the window in front; Add File… from Settings or with no window brings
/// the main window back.
#[test]
fn close_window_and_add_file_follow_the_front_window() {
    let mut d = desk(MemoryService::new());
    d.key(cmd(','));
    d.settle();
    assert_eq!(d.shown().front, Some(Which::Settings));
    d.main.update(|s, tx| run(s, &Pick::CloseWindow, around(s), tx));
    d.settle();
    assert!(!d.shown().settings && d.shown().main, "Settings only");
    d.main.update(|s, tx| run(s, &Pick::CloseWindow, around(s), tx));
    d.settle();
    assert!(!d.shown().main);
    d.main.update(|s, tx| run(s, &Pick::App(InApp::AddFile), NO_WINDOW, tx));
    d.settle();
    assert_eq!(d.shown().front, Some(Which::Main));
}

/// Every context's menus, for reading (`PHANTOM_MENU_DUMP=<file>`): an artifact, nothing checked.
#[test]
fn dump_for_reading() {
    let Some(file) = std::env::var_os("PHANTOM_MENU_DUMP") else { return };
    let mut out = String::new();
    let mut add = |title: &str, s: &Phantom, around| {
        out.push_str(&format!("==== {title} ====\n"));
        for m in build(s, around) {
            out.push_str(&format!("\n{}\n", m.name));
            dump(&m).iter().for_each(|l| out.push_str(&format!("    {l}\n")));
        }
        out.push('\n');
    };
    let mut rt = three();
    add("Nothing selected", &rt.state, MAIN);
    select(&mut rt, 1);
    add("One running torrent selected", &rt.state, MAIN);
    focus_link(&mut rt);
    add("The link field focused, the torrent still selected", &rt.state, MAIN);
    select(&mut rt, 3);
    add("One paused torrent selected", &rt.state, MAIN);
    press(&mut rt, cmd('a'));
    add("Every torrent selected", &rt.state, MAIN);
    add("Settings in front, no field focused", &rt.state, SETTINGS);
    rt.state.settings_field = true;
    add("Settings in front, a field focused", &rt.state, SETTINGS);
    add("No window open", &rt.state, NO_WINDOW);
    add("A file picker open", &rt.state, PANEL);
    select(&mut rt, 1);
    press(&mut rt, plain(Key::Backspace));
    add("After a Remove", &rt.state, MAIN);
    rt.update(|s, _| palette::open(s));
    settle(&mut rt);
    add("The palette open", &rt.state, MAIN);
    press(&mut rt, plain(Key::Escape));
    rt.state.clash = Some(clash());
    add("The clash dialog open", &rt.state, MAIN);
    std::fs::write(file, out).unwrap();
}
