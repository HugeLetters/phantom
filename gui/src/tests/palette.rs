//! The command palette (#33) and the command list's places, driven as a person would against the
//! fake: ⌘P opens it over the window with the commands available for the selection, the same the
//! row menu offers, and the app's; typing filters; ↑ ↓ move; Return runs, Escape closes. And the
//! row menu: as wide as its widest item, inside the window, an icon on every item.

use proto::{TorrentCommand, TorrentId};
use rune::{Key, KeyEvent, Mods};

use super::*;
use crate::commands::{InApp, OnTorrents};
use crate::model::palette::row_key;

fn press(rt: &mut Runtime<Phantom>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    settle(rt);
}

fn cmd_p(rt: &mut Runtime<Phantom>) {
    rt.key(shortcuts::keys(InApp::Palette)[0].event());
    settle(rt);
}

fn type_text(rt: &mut Runtime<Phantom>, text: &str) {
    for c in text.chars() {
        rt.key(KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() });
    }
    settle(rt);
}

/// What the open palette draws, in order: each command's label and the key shown beside it.
fn drawn_palette(rt: &Runtime<Phantom>) -> Vec<(String, Option<String>)> {
    let elements = rt.elements();
    let Some(panel) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")) else { return Vec::new() };
    let inside = format!("{}/", panel.path);
    let rows: Vec<&rune::ElementInfo> =
        elements.iter().filter(|e| !e.exiting && e.role == "option" && e.path.starts_with(&inside)).collect();
    rows.iter()
        .map(|row| {
            let below = format!("{}/", row.path);
            let hint =
                elements.iter().find(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("hint")).and_then(|c| c.text.clone());
            // Every command has its icon but the priorities, which have none (the file menu marks the
            // current one with a check).
            let icon = elements.iter().any(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("icon"));
            let priority = matches!(row.label.as_deref(), Some("Priority: Normal" | "Priority: Off"));
            assert_eq!(icon, !priority, "{:?} has its icon, a priority has none", row.label);
            (row.label.clone().expect("a row has its label"), hint)
        })
        .collect()
}

fn labels(rt: &Runtime<Phantom>) -> Vec<String> {
    drawn_palette(rt).into_iter().map(|(l, _)| l).collect()
}

/// The palette's field has the focus.
fn field_focused(rt: &Runtime<Phantom>) -> bool {
    rt.elements().iter().any(|e| e.focused && !e.exiting && e.path.contains("palette-field"))
}

/// Selects torrent `id` alone, its details in.
fn select(rt: &mut Runtime<Phantom>, id: TorrentId) {
    tap(rt, &format!("rows/{id}"));
    settle(rt);
}

/// ⌘P lists what the row menu offers the selection, worded the same, with the same keys, then the
/// app's own: for one torrent, for every torrent (⌘A), and for none.
#[test]
fn the_palette_lists_what_the_row_menu_offers_and_the_apps_commands() {
    let mut rt = app(with_an_album());
    let app_commands = || vec![("Add file…".to_owned(), Some("⇧⌘A".to_owned()))];
    for selection in ["one", "all"] {
        select(&mut rt, 1);
        if selection == "all" {
            rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
            settle(&mut rt);
        }
        right_click_row(&mut rt, 1);
        let menu: Vec<(String, Option<String>)> = generated::menu_hints(&rt, "row-menu");
        press(&mut rt, Key::Escape);
        cmd_p(&mut rt);
        assert!(rt.state.palette.is_open() && field_focused(&rt), "{selection}: open, its field focused");
        // Move to ▸ is Move to… (Other…) there, then "Move to <folder>" for each of its folders.
        let folders = crate::model::recent::offered(&rt.state);
        let mut want: Vec<(String, Option<String>)> = Vec::new();
        for (label, hint) in menu {
            if label != "Move to" {
                want.push((label, hint));
                continue;
            }
            want.push(("Move to…".into(), None));
            want.extend(folders.iter().map(|f| (format!("Move to {}", crate::model::recent::name(&f.folder)), None)));
        }
        want.extend(app_commands());
        assert_eq!(drawn_palette(&rt), want, "{selection}");
        press(&mut rt, Key::Escape);
    }
    rt.update(|s, _| s.table.clear_selection());
    settle(&mut rt);
    cmd_p(&mut rt);
    assert_eq!(drawn_palette(&rt), app_commands(), "nothing selected: only the app's");
}

/// Typing keeps the commands whose name has its letters in order, the best first: "ren" puts
/// Rename… on top, and a text nothing has lists none.
#[test]
fn typing_filters_by_name_best_first() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let all = labels(&rt);
    type_text(&mut rt, "ren");
    let listed = labels(&rt);
    assert_eq!(listed.first().map(String::as_str), Some("Rename…"));
    let matching: Vec<&String> = all.iter().filter(|l| crate::model::palette::score("ren", l).is_some()).collect();
    assert_eq!(listed.len(), matching.len(), "every match and only those: {listed:?}");
    assert!(listed.iter().all(|l| matching.contains(&l)));
    type_text(&mut rt, "zzz");
    assert!(labels(&rt).is_empty());
    assert!(rt.find("empty").is_some(), "it says nothing matches");
}

/// ↓ and ↑ move the highlight and wrap around at the ends: ↑ on the first goes to the last, ↓ on the
/// last to the first; typing puts it back on the first.
#[test]
fn the_arrows_move_the_highlight_and_wrap_around_at_the_ends() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let n = labels(&rt).len();
    assert!(n >= 3);
    let highlighted = |rt: &Runtime<Phantom>| {
        let elements = rt.elements();
        let hover = elements
            .iter()
            .find(|e| e.key.as_deref() == Some(row_key(crate::model::palette::highlighted(&rt.state)).as_str()))
            .map(|e| e.fill);
        (
            crate::model::palette::highlighted(&rt.state),
            (0..n).filter(|i| elements.iter().any(|e| e.key.as_deref() == Some(row_key(*i).as_str()) && Some(e.fill) == hover)).count(),
        )
    };
    assert_eq!(highlighted(&rt), (0, 1), "the first, alone");
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt), (n - 1, 1), "↑ on the first goes to the last");
    press(&mut rt, Key::Down);
    assert_eq!(highlighted(&rt), (0, 1), "↓ on the last goes to the first");
    for i in 1..n {
        press(&mut rt, Key::Down);
        assert_eq!(highlighted(&rt), (i, 1));
    }
    press(&mut rt, Key::Down);
    assert_eq!(highlighted(&rt).0, 0, "round again");
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt).0, 0);
    press(&mut rt, Key::Up);
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt).0, n - 2);
    type_text(&mut rt, "c");
    assert_eq!(highlighted(&rt).0, 0, "typing goes back to the first");
}

/// With files selected in the details pane's Files list, the palette also lists the file commands,
/// worded as the file menu words them, between the torrent's and the app's; with none selected, it
/// doesn't.
#[test]
fn the_palette_lists_the_selected_files_commands_as_the_file_menu_words_them() {
    let file = |index, path: &str| proto::File {
        index,
        path: path.into(),
        size_bytes: 100 * MIB,
        progress_bytes: 0,
        priority: proto::Priority::Normal,
        missing: false,
        on_disk: true,
    };
    let service = MemoryService::new()
        .with_torrents(vec![torrent(1, "Album")])
        .with_files(1, vec![file(0, "01 Intro.flac"), file(1, "02 Song.flac")]);
    let mut rt = app(service);
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let without = labels(&rt);
    assert!(!without.iter().any(|l| l.starts_with("Priority") || l == "Move to Trash"), "no file selected: {without:?}");
    press(&mut rt, Key::Escape);

    tap(&mut rt, "files/viewport/rows/0");
    settle(&mut rt);
    let (x, y) = rt.find("files/viewport/rows/0/path").expect("a file row");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    // Every item of the file menu, the priorities (menuitemcheckbox) too, with its key.
    let elements = rt.elements();
    let file_menu: Vec<(String, Option<String>)> = elements
        .iter()
        .filter(|e| !e.exiting && e.role.starts_with("menuitem") && e.path.contains("file-menu"))
        .filter_map(|item| {
            let label = item.label.clone()?;
            let inside = format!("{}/", item.path);
            let hint = elements.iter().filter(|e| e.path.starts_with(&inside)).filter_map(|e| e.text.clone()).find(|t| *t != label);
            Some((label, hint))
        })
        .collect();
    drop(elements);
    let wording: Vec<&str> = file_menu.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(wording, ["Open", "Reveal in Finder", "Normal", "Off", "Move to Trash"]);
    press(&mut rt, Key::Escape);

    cmd_p(&mut rt);
    let drawn = drawn_palette(&rt);
    let torrent_commands = without.len() - 1;
    let files: Vec<(String, Option<String>)> = drawn[torrent_commands..drawn.len() - 1].to_vec();
    // The palette has no heading over the priorities, so each item names what it sets.
    let in_palette: Vec<(String, Option<String>)> = file_menu
        .iter()
        .map(|(l, k)| (if matches!(l.as_str(), "Normal" | "Off") { format!("Priority: {l}") } else { l.clone() }, k.clone()))
        .collect();
    assert_eq!(files, in_palette, "the file commands, as the file menu has them but for the priorities, before the app's");
    for query in ["priority", "normal", "off"] {
        type_text(&mut rt, query);
        let found = labels(&rt);
        assert!(found.iter().any(|l| l == "Priority: Normal") || query == "off", "{query:?} finds Priority: Normal: {found:?}");
        assert!(found.iter().any(|l| l == "Priority: Off") || query == "normal", "{query:?} finds Priority: Off: {found:?}");
        for _ in query.chars() {
            press(&mut rt, Key::Backspace);
        }
    }
    assert_eq!(drawn.last().map(|(l, _)| l.as_str()), Some("Add file…"));
    type_text(&mut rt, "move to trash");
    assert!(labels(&rt).iter().any(|l| l == "Move to Trash"), "filtering finds a file command: {:?}", labels(&rt));
    press(&mut rt, Key::Enter);
    assert!(!rt.state.palette.is_open());
    let priorities: Vec<proto::Priority> = rt.state.pane.details.as_ref().expect("details").files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [proto::Priority::Off, proto::Priority::Normal], "Move to Trash ran on the selected file only");
}

/// The palette is never narrower than its minimum, and anchored on its left: when a long command
/// widens it (Undo names what it takes back), or a filter narrows it again, only its right edge moves.
#[test]
fn the_palette_has_a_minimum_width_and_widens_to_the_right() {
    const LONG: &str = "The Complete Studio Recordings, Remastered Deluxe Edition (2024) [FLAC 24-96 Hi-Res]";
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, LONG)]));
    let panel = |rt: &Runtime<Phantom>| {
        rt.elements().iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")).expect("the palette").rect
    };
    select(&mut rt, 0);
    cmd_p(&mut rt);
    let narrow = panel(&rt);
    assert!(narrow[2] >= 400.0, "a minimum width: {}", narrow[2]);
    press(&mut rt, Key::Escape);
    rt.update(|s, _| s.table.clear_selection());
    cmd_p(&mut rt);
    assert!((panel(&rt)[2] - narrow[2]).abs() < 0.5, "short lists are all the minimum wide");
    press(&mut rt, Key::Escape);

    tap(&mut rt, "rows/1");
    rt.key(shortcuts::keys(OnTorrents::Remove)[0].event());
    settle(&mut rt);
    select(&mut rt, 0);
    cmd_p(&mut rt);
    let wide = panel(&rt);
    assert!(wide[2] > narrow[2] + 20.0, "the long Undo widens it: {} against {}", wide[2], narrow[2]);
    assert!((wide[0] - narrow[0]).abs() < 0.5, "its left edge stays: {} against {}", wide[0], narrow[0]);
    assert!(wide[0] + wide[2] <= 960.0 - 16.0 + 0.5, "it stays inside the window, 16 from its side: {wide:?}");
    type_text(&mut rt, "copy");
    let again = panel(&rt);
    assert!((again[2] - narrow[2]).abs() < 0.5, "filtered to short commands, back at the minimum: {}", again[2]);
    assert!((again[0] - narrow[0]).abs() < 0.5, "its left edge stays");
}

/// A menu is never narrower than 280 points, however short its items, so it doesn't change width
/// from one menu to the next.
#[test]
fn a_short_menu_keeps_its_minimum_width() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    rt.update(|s, _| s.pane.files.select("0"));
    settle(&mut rt);
    let (x, y) = rt.find("files/viewport/rows/0/path").expect("a file row");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    let rect = rt.elements().iter().find(|e| !e.exiting && e.key.as_deref() == Some("file-menu-menu")).expect("the menu").rect;
    assert!((rect[2] - 280.0).abs() < 0.5, "280 wide: {}", rect[2]);
}

/// Return runs the highlighted command on the selection and closes the palette: Pause pauses the
/// selected torrent, and only it.
#[test]
fn return_runs_the_highlighted_command() {
    let service = with_an_album();
    let probe = service.probe();
    let mut rt = app(service);
    select(&mut rt, 1);
    cmd_p(&mut rt);
    type_text(&mut rt, "pause");
    assert_eq!(labels(&rt).first().map(String::as_str), Some("Pause"));
    press(&mut rt, Key::Enter);
    assert!(!rt.state.palette.is_open(), "closed");
    assert!(probe.commands().contains(&(vec![1], TorrentCommand::Pause)), "{:?}", probe.commands());
    assert_eq!(find_torrent(&rt, 1).status, proto::Status::Paused);
    assert_eq!(find_torrent(&rt, 0).status, proto::Status::Downloading, "the other untouched");
}

/// Select a torrent, ⌘P, "ren", Return: its Name cell is the field, focused, with its name; a name
/// typed and Return renames it.
#[test]
fn rename_from_the_palette_opens_the_name_field_of_the_selected_torrent() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    cmd_p(&mut rt);
    type_text(&mut rt, "ren");
    press(&mut rt, Key::Enter);
    assert!(!rt.state.palette.is_open());
    assert_eq!(rt.state.rename.id, Some(1));
    assert_eq!(rt.state.rename.field.text, "Album");
    assert!(rt.elements().iter().any(|e| e.focused && e.path.contains("rename-field")), "the field has the focus");
    rt.update(|s, _| {
        s.rename.field = rune::TextEdit::new("Best of");
        s.rename.field.focused = true;
    });
    press(&mut rt, Key::Enter);
    assert_eq!(find_torrent(&rt, 1).display_name(), "Best of");
}

/// Rename… opens in the details pane's header: the palette lists it while one torrent is
/// selected, the Name column hidden too, and opens the field there.
#[test]
fn the_palette_has_rename_with_the_name_column_hidden() {
    let mut rt = app(with_an_album());
    rt.update(|s, _| {
        s.prefs.kept.hidden.insert(crate::model::layout::Column::Name);
    });
    settle(&mut rt);
    select(&mut rt, 1);
    cmd_p(&mut rt);
    type_text(&mut rt, "ren");
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.rename.id, Some(1));
    assert!(rt.find("rename-field").is_some(), "the field, in the header");
}

/// Escape and a click outside close it, running nothing and keeping the selection; with nothing
/// matching, Return runs nothing either.
#[test]
fn escape_a_click_outside_or_no_match_runs_nothing() {
    let service = with_an_album();
    let probe = service.probe();
    let mut rt = app(service);
    select(&mut rt, 1);
    let before = probe.commands().len();
    cmd_p(&mut rt);
    type_text(&mut rt, "zzz");
    press(&mut rt, Key::Enter);
    assert!(rt.state.palette.is_open(), "Return with nothing listed leaves it open");
    press(&mut rt, Key::Escape);
    assert!(!rt.state.palette.is_open());
    assert_eq!(selected_torrents(&rt.state.table), [1], "Escape closed the palette only");
    cmd_p(&mut rt);
    rt.pointer_down(950.0, 530.0);
    rt.pointer_up(950.0, 530.0);
    settle(&mut rt);
    assert!(!rt.state.palette.is_open(), "a click outside closes it");
    assert_eq!(probe.commands().len(), before, "nothing ran");
}

/// With a change to take back, Undo is listed by what it takes back, as Edit ▸ Undo names it, and
/// runs it.
#[test]
fn undo_is_listed_by_what_it_takes_back() {
    let mut rt = app(with_an_album());
    select(&mut rt, 0);
    press(&mut rt, Key::Backspace);
    cmd_p(&mut rt);
    let undo = crate::changes::menu_title(&rt.state);
    assert!(undo.starts_with("Undo ") && undo.len() > "Undo ".len(), "{undo:?}");
    assert!(drawn_palette(&rt).contains(&(undo.clone(), Some("⌘Z".into()))), "{:?}", drawn_palette(&rt));
    type_text(&mut rt, "undo");
    press(&mut rt, Key::Enter);
    wait(&mut rt, 0.5);
    assert!(rt.state.torrents.iter().any(|t| t.id == 0), "the torrent is back");
}

/// Every label of the row menu shows whole, however long ("Delete 2 torrents (files to the
/// Trash)"): the menu is as wide as its widest item. Each item has its icon.
#[test]
fn the_row_menu_is_as_wide_as_its_widest_item() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    right_click_row(&mut rt, 1);
    let elements = rt.elements();
    let look = rt.state.looks.borrow().shown(crate::theme::preset::Appearance::Dark).clone();
    for item in elements.iter().filter(|e| !e.exiting && e.role == "menuitem" && e.path.contains("row-menu")) {
        let label = item.label.clone().unwrap();
        let below = format!("{}/", item.path);
        let text = elements.iter().find(|c| c.path.starts_with(&below) && c.text.as_deref() == Some(label.as_str())).unwrap();
        let needs = look.ui.label_width(&label);
        assert!(text.rect[2] + 0.5 >= needs, "{label:?} is cut: {} of {needs}", text.rect[2]);
        assert!(elements.iter().any(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("icon")), "{label:?} has its icon");
    }
    let menu = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("row-menu-menu")).expect("the menu");
    assert!(menu.rect[2] > 280.0, "wider than the 280 points that cut Delete's label: {}", menu.rect[2]);
}

/// In the smallest window, a menu opened near the bottom moves up and shows whole, inside the window.
#[test]
fn a_row_menu_near_the_bottom_moves_up_inside_the_window() {
    let mut rt = app(with_an_album());
    rt.set_viewport((640.0, 320.0), 2.0);
    settle(&mut rt);
    select(&mut rt, 1);
    right_click_row(&mut rt, 1);
    let elements = rt.elements();
    let menu = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("row-menu-menu")).expect("the menu");
    let [x, y, w, h] = menu.rect;
    assert!(x >= 0.0 && y >= 0.0 && x + w <= 640.0 && y + h <= 320.0, "inside the window: {:?}", menu.rect);
    for item in elements.iter().filter(|e| !e.exiting && e.role == "menuitem" && e.path.contains("row-menu")) {
        let (iy, ih) = (item.rect[1], item.rect[3]);
        assert!(iy >= y - 0.5 && iy + ih <= y + h + 0.5, "{:?} shows without scrolling", item.label);
    }
}

/// Each screen shown registers the commands it offers: the torrent list every torrent command but
/// Rename…, the details pane's header Rename… and its Files list every file command while they
/// show; a screen not shown, none. The
/// default theme's conformance test: a theme drawing its own screens must pass it too.
#[test]
fn each_screen_shown_registers_its_commands() {
    use crate::commands::{OnFiles, OnTorrents};
    let mut rt = app(with_an_album());
    let offered = |rt: &Runtime<Phantom>| {
        (
            OnTorrents::ALL.iter().filter(|c| rt.state.mounted.offers(**c)).count(),
            OnFiles::ALL.iter().filter(|c| rt.state.mounted.offers(**c)).count(),
        )
    };
    assert_eq!(offered(&rt), (OnTorrents::ALL.len() - 1, 0), "the list, no pane");
    assert!(!rt.state.mounted.offers(OnTorrents::Rename), "no header, no Rename…");
    select(&mut rt, 1);
    assert_eq!(offered(&rt), (OnTorrents::ALL.len(), OnFiles::ALL.len()), "the list, the header and the Files list");
    press(&mut rt, Key::Escape);
    assert_eq!(offered(&rt), (OnTorrents::ALL.len() - 1, 0), "the pane closed");
}

/// A theme whose details screen leaves out the Files list and the header leaves their commands
/// offered nowhere: the file menu has nothing, the palette no Rename…, only the list's torrent
/// commands and the app's.
#[test]
fn a_screen_that_registers_no_command_offers_none() {
    use crate::commands::{OnFiles, on_files};
    use crate::theme::setting::{Setting, Values};
    use crate::theme::{Built, ScreenOverrides, Theme, ThemeLook};

    struct NoFiles;
    impl Theme for NoFiles {
        fn id(&self) -> &'static str {
            "no-files"
        }
        fn name(&self) -> &'static str {
            "No files"
        }
        fn parent(&self) -> Option<&'static dyn Theme> {
            Some(&crate::theme::phantom::PHANTOM)
        }
        fn settings(&self) -> &'static [Setting] {
            &[]
        }
        fn build(&self, _: &Values) -> Built {
            let details: crate::theme::DetailsScreen =
                std::rc::Rc::new(|_: &Phantom, _: &crate::ui::Ui, _: &proto::TorrentInfo, _| rune::el());
            let tokens = rune_kit::Kit::neutral(true);
            Built {
                colours: crate::theme::Colours::of(&tokens),
                spacing: crate::theme::Spacing::phantom(),
                tokens,
                blocks: Default::default(),
                screens: ScreenOverrides { details: Some(details), ..Default::default() },
                app_icon: "phantom",
            }
        }
    }
    static NO_FILES: NoFiles = NoFiles;

    let mut rt = app(with_an_album());
    let look = ThemeLook { theme: &NO_FILES, settings: Values::defaults(&[]) };
    rt.state.looks.borrow_mut().set(crate::theme::preset::Appearance::Dark, look);
    select(&mut rt, 1);
    rt.update(|s, _| s.pane.files.select("0"));
    settle(&mut rt);
    assert!(OnFiles::ALL.iter().all(|c| !rt.state.mounted.offers(*c)), "no Files list, no file command");
    assert!(on_files(&rt.state).is_empty());
    cmd_p(&mut rt);
    let listed = labels(&rt);
    assert!(listed.iter().any(|l| l == "Copy magnet link"), "the list's commands still are: {listed:?}");
    assert!(!listed.iter().any(|l| l == "Rename…"), "no header, no Rename…: {listed:?}");
}

/// The headings the open palette draws, top to bottom, with the labels of the commands under each.
fn drawn_groups(rt: &Runtime<Phantom>) -> Vec<(String, Vec<String>)> {
    let elements = rt.elements();
    let Some(panel) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")) else { return Vec::new() };
    let inside = format!("{}/", panel.path);
    let mut shown: Vec<&rune::ElementInfo> =
        elements.iter().filter(|e| !e.exiting && e.path.starts_with(&inside) && (e.role == "option" || e.role == "heading")).collect();
    shown.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for e in shown {
        if e.role == "heading" {
            groups.push((e.label.clone().expect("a heading has its text"), Vec::new()));
        } else {
            groups.last_mut().expect("a command under a heading").1.push(e.label.clone().expect("a label"));
        }
    }
    groups
}

fn headings(rt: &Runtime<Phantom>) -> Vec<String> {
    drawn_groups(rt).into_iter().map(|(h, _)| h).collect()
}

/// Two of the album's files selected in the Files list.
fn select_two_files(rt: &mut Runtime<Phantom>) {
    tap(rt, "files/viewport/rows/0");
    rt.key(shift(Key::Down));
    settle(rt);
}

/// The list is grouped by what the commands act on, each group under a heading naming the actual
/// selection: the torrent's name, the file's, "Phantom" for the app's; for several, their count.
#[test]
fn the_palette_groups_its_commands_under_headings_naming_the_selection() {
    let mut rt = app(with_an_album());
    cmd_p(&mut rt);
    assert_eq!(headings(&rt), ["Phantom"], "nothing selected: the app's alone");
    press(&mut rt, Key::Escape);

    select(&mut rt, 1);
    cmd_p(&mut rt);
    assert_eq!(headings(&rt), ["Album", "Phantom"], "one torrent, by its name");
    let groups = drawn_groups(&rt);
    assert!(groups[0].1.contains(&"Pause".to_owned()) && !groups[0].1.contains(&"Add file…".to_owned()));
    assert!(groups[1].1.contains(&"Add file…".to_owned()) && !groups[1].1.contains(&"Pause".to_owned()));
    press(&mut rt, Key::Escape);

    tap(&mut rt, "files/viewport/rows/2");
    settle(&mut rt);
    cmd_p(&mut rt);
    assert_eq!(headings(&rt), ["Album", "front.jpg", "Phantom"], "one file, by its name, between the torrent's and the app's");
    let groups = drawn_groups(&rt);
    assert!(groups[1].1.contains(&"Move to Trash".to_owned()) && !groups[0].1.contains(&"Move to Trash".to_owned()));
    press(&mut rt, Key::Escape);

    select_two_files(&mut rt);
    cmd_p(&mut rt);
    assert_eq!(headings(&rt), ["Album", "2 files", "Phantom"]);
    press(&mut rt, Key::Escape);

    select(&mut rt, 1);
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    cmd_p(&mut rt);
    assert_eq!(headings(&rt).first().map(String::as_str), Some("2 torrents"), "several torrents, by count: {:?}", headings(&rt));
    assert_eq!(headings(&rt).last().map(String::as_str), Some("Phantom"));
}

/// While filtering, a heading shows only while one of its commands still matches; the fuzzy order
/// applies within a group.
#[test]
fn a_heading_is_hidden_once_nothing_under_it_matches() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    tap(&mut rt, "files/viewport/rows/2");
    settle(&mut rt);
    cmd_p(&mut rt);
    assert_eq!(headings(&rt), ["Album", "front.jpg", "Phantom"]);
    type_text(&mut rt, "add file");
    assert_eq!(headings(&rt), ["Phantom"], "only the app's command matches");
    type_text(&mut rt, "zzz");
    assert!(headings(&rt).is_empty(), "nothing matches: no heading either");
    for _ in 0..3 + 8 {
        press(&mut rt, Key::Backspace);
    }
    assert_eq!(headings(&rt), ["Album", "front.jpg", "Phantom"], "the text cleared, all are back");
    type_text(&mut rt, "trash");
    assert_eq!(headings(&rt), ["Album", "front.jpg"], "the Trash commands of the torrent and of the file");
    for _ in 0.."trash".len() {
        press(&mut rt, Key::Backspace);
    }
    type_text(&mut rt, "move to t");
    assert_eq!(headings(&rt), ["Album", "front.jpg"], "{:?}", drawn_groups(&rt));
    type_text(&mut rt, "rash");
    assert_eq!(headings(&rt), ["front.jpg"], "the torrent's group has nothing left: its heading goes");
    type_text(&mut rt, "x");
    assert!(headings(&rt).is_empty());
}

/// ↑ and ↓ step from command to command, skipping the headings, and wrap round the ends.
#[test]
fn the_arrows_skip_headings_and_wrap() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    tap(&mut rt, "files/viewport/rows/2");
    settle(&mut rt);
    cmd_p(&mut rt);
    let groups = drawn_groups(&rt);
    assert_eq!(groups.len(), 3, "three groups, so two headings to skip over");
    let commands: Vec<String> = groups.iter().flat_map(|(_, l)| l.clone()).collect();
    let highlighted = |rt: &Runtime<Phantom>| labels(rt)[crate::model::palette::highlighted(&rt.state)].clone();
    assert_eq!(highlighted(&rt), commands[0], "the first command, under the first heading");
    for i in 1..commands.len() {
        press(&mut rt, Key::Down);
        assert_eq!(highlighted(&rt), commands[i], "↓ lands on the next command, across a heading");
    }
    press(&mut rt, Key::Down);
    assert_eq!(highlighted(&rt), commands[0], "↓ on the last wraps to the first");
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt), *commands.last().unwrap(), "↑ on the first wraps to the last");
    assert_eq!(labels(&rt), commands, "the rows are the commands, in the groups' order");
}

/// Return runs the highlighted command, never a heading: arrowing from the torrent's group into the
/// file's, Return runs the file's command on the file.
#[test]
fn return_runs_the_highlighted_command_not_a_heading() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    tap(&mut rt, "files/viewport/rows/2");
    settle(&mut rt);
    cmd_p(&mut rt);
    let all = labels(&rt);
    let off = all.iter().position(|l| l == "Priority: Off").expect("the Off priority");
    for _ in 0..off {
        press(&mut rt, Key::Down);
    }
    press(&mut rt, Key::Enter);
    assert!(!rt.state.palette.is_open());
    let priorities: Vec<Priority> = files(&rt).into_iter().map(|(_, p)| p).collect();
    assert_eq!(priorities, [Priority::Normal, Priority::Normal, Priority::Off], "Off ran on the selected file");
}

/// Return runs the command that was highlighted, not whatever the position holds by then: arrowed
/// to "Remove", and a row above it going away (the torrent starts moving, so Rename… leaves), Return
/// still removes, and never deletes.
#[test]
fn return_runs_the_highlighted_command_when_rows_above_it_go_away() {
    let service = with_an_album().moves_take(5.0);
    let probe = service.probe();
    let mut rt = app(service);
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let before = labels(&rt);
    let remove = before.iter().position(|l| l.starts_with("Remove")).expect("Remove is offered");
    assert!(before.iter().position(|l| l == "Rename…").is_some_and(|rename| rename < remove), "Rename… is above it");
    for _ in 0..remove {
        press(&mut rt, Key::Down);
    }
    let folder = proto::AbsolutePath::new("/Films").unwrap();
    rt.update(|s, _| crate::model::moving::start(s, vec![1], folder));
    settle(&mut rt);
    let after = labels(&rt);
    assert!(!after.iter().any(|l| l == "Rename…"), "a moving torrent can't be renamed: {after:?}");
    assert!(after[crate::model::palette::highlighted(&rt.state)].starts_with("Remove"), "the highlight followed Remove");
    press(&mut rt, Key::Enter);
    let sent: Vec<TorrentCommand> = probe.commands().into_iter().map(|(_, c)| c).collect();
    assert!(sent.contains(&TorrentCommand::Remove), "Remove ran: {sent:?}");
    assert!(!sent.contains(&TorrentCommand::Delete), "Delete didn't: {sent:?}");
}

/// When the highlighted command itself goes away, the highlight moves to a neighbour that's drawn,
/// and Return runs that one: never a row that wasn't highlighted.
#[test]
fn return_runs_the_row_highlighted_when_the_highlighted_command_goes_away() {
    let service = with_an_album().moves_take(5.0);
    let probe = service.probe();
    let mut rt = app(service);
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let rename = labels(&rt).iter().position(|l| l == "Rename…").expect("Rename…");
    for _ in 0..rename {
        press(&mut rt, Key::Down);
    }
    let folder = proto::AbsolutePath::new("/Films").unwrap();
    rt.update(|s, _| crate::model::moving::start(s, vec![1], folder));
    settle(&mut rt);
    let after = labels(&rt);
    let shown = after[crate::model::palette::highlighted(&rt.state)].clone();
    assert_ne!(shown, "Rename…");
    press(&mut rt, Key::Enter);
    let sent: Vec<TorrentCommand> = probe.commands().into_iter().map(|(_, c)| c).collect();
    assert!(!sent.contains(&TorrentCommand::Delete) || shown.starts_with("Delete"), "ran {shown:?}, sent {sent:?}");
}

/// ADR 0013: a command no view registered is offered nowhere. With no torrent list registering the
/// torrent commands, a selected torrent has none in the palette, and ⌫ does nothing.
#[test]
fn a_torrent_command_is_not_offered_when_the_torrent_list_is_not_registered() {
    let service = with_an_album();
    let probe = service.probe();
    let mut rt = app(service);
    select(&mut rt, 1);
    rt.update(|s, _| s.mounted.begin());
    assert!(crate::commands::on_torrents(&rt.state).is_empty());
    assert!(crate::model::palette::offered(&rt.state).iter().all(|e| !matches!(e.command, crate::commands::Command::Torrents(_))));
    rt.key(shortcuts::keys(OnTorrents::Remove)[0].event());
    assert!(probe.commands().is_empty(), "⌫ ran nothing: {:?}", probe.commands());
}
