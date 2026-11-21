//! The command palette (#33) and the command list's places, driven as a person would against the
//! fake: ⌘P opens it over the window with the commands available for the selection, the same the
//! row menu offers, and the app's; typing filters; ↑ ↓ move; Return runs, Escape closes. And the
//! row menu: as wide as its widest item, inside the window, an icon on every item.

use proto::{TorrentCommand, TorrentId};
use rune::{Key, KeyEvent, Mods};

use super::*;
use crate::commands::InApp;
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
            assert!(
                elements.iter().any(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("icon")),
                "{:?} has its icon",
                row.label
            );
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

/// ↓ and ↑ move the highlight, stopping at the ends; typing puts it back on the first.
#[test]
fn the_arrows_move_the_highlight_and_stop_at_the_ends() {
    let mut rt = app(with_an_album());
    select(&mut rt, 1);
    cmd_p(&mut rt);
    let n = labels(&rt).len();
    assert!(n >= 3);
    let highlighted = |rt: &Runtime<Phantom>| {
        let elements = rt.elements();
        let hover = elements.iter().find(|e| e.key.as_deref() == Some(row_key(rt.state.palette.at()).as_str())).map(|e| e.fill);
        (
            rt.state.palette.at(),
            (0..n).filter(|i| elements.iter().any(|e| e.key.as_deref() == Some(row_key(*i).as_str()) && Some(e.fill) == hover)).count(),
        )
    };
    assert_eq!(highlighted(&rt), (0, 1), "the first, alone");
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt).0, 0, "the first stays");
    for i in 1..n {
        press(&mut rt, Key::Down);
        assert_eq!(highlighted(&rt), (i, 1));
    }
    press(&mut rt, Key::Down);
    assert_eq!(highlighted(&rt).0, n - 1, "the last stays");
    press(&mut rt, Key::Up);
    assert_eq!(highlighted(&rt).0, n - 2);
    type_text(&mut rt, "c");
    assert_eq!(highlighted(&rt).0, 0, "typing goes back to the first");
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

/// Each screen shown registers the commands it offers: the torrent list every torrent command, the
/// details pane's Files list every file command while it shows; a screen not shown, none. The
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
    assert_eq!(offered(&rt), (OnTorrents::ALL.len(), 0), "the list, no pane");
    select(&mut rt, 1);
    assert_eq!(offered(&rt), (OnTorrents::ALL.len(), OnFiles::ALL.len()), "the list and the Files list");
    press(&mut rt, Key::Escape);
    assert_eq!(offered(&rt), (OnTorrents::ALL.len(), 0), "the pane closed");
}

/// A theme whose details screen leaves out the Files list leaves its commands offered nowhere: the
/// file menu has nothing, and the palette lists only the torrent's and the app's.
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
                std::rc::Rc::new(|_: &Phantom, _: &crate::ui::Ui, _: &proto::TorrentInfo| rune::el());
            Built {
                tokens: rune_kit::Kit::neutral(true),
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
    assert!(labels(&rt).iter().any(|l| l == "Rename…"), "the list's commands still are: {:?}", labels(&rt));
}
