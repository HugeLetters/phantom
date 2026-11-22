//! Rename… (#331), driven as a person would against the fake: the row menu's item, offered for
//! one torrent only, turns the details pane's header name into a field, as wide as the name may be; Return or a click away
//! renames, Escape cancels; a name a file can't have keeps it open, said in a tooltip under it;
//! what the service refuses, in the status bar; ⌘Z renames it back.

use proto::{File, Priority, TorrentCommand, TorrentId};
use rune::{Key, KeyEvent, Mods};

use super::*;

const RENAME: &str = "Rename…";

/// The album (torrent 1, a folder of three files) and torrent 2, one file, `Movie.mkv`.
fn with_a_movie() -> MemoryService {
    let movie = File {
        index: 0,
        path: "Movie.mkv".into(),
        size_bytes: 700 * MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
        on_disk: false,
    };
    MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), torrent(1, "Album"), torrent(2, "Movie.mkv")])
        .with_files(1, with_an_album_files())
        .with_files(2, vec![movie])
}

fn with_an_album_files() -> Vec<File> {
    let file = |index, path: &str| File {
        index,
        path: path.into(),
        size_bytes: 100 * MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
        on_disk: false,
    };
    vec![file(0, "01 Intro.flac"), file(1, "02 Song.flac"), file(2, "Cover/front.jpg")]
}

fn enter(rt: &mut Runtime<Phantom>) {
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    settle(rt);
}

fn escape(rt: &mut Runtime<Phantom>) {
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(rt);
}

/// Opens torrent `id`'s field from its row menu, its details in first (as a person's selecting
/// the row brings them).
fn open_field(rt: &mut Runtime<Phantom>, id: TorrentId) {
    tap(rt, &format!("rows/{id}"));
    settle(rt);
    row_pick(rt, id, RENAME);
}

fn renames_sent(service: &crate::service::Probe) -> Vec<(Vec<TorrentId>, TorrentCommand)> {
    service.commands().into_iter().filter(|(_, c)| matches!(c, TorrentCommand::Rename { .. })).collect()
}

/// With one torrent selected the row menu has Rename…, with Move to…; with several it isn't
/// there at all, as Move to… isn't.
#[test]
fn rename_is_offered_for_one_torrent_only() {
    let mut rt = app(with_a_movie());
    tap(&mut rt, "rows/1");
    assert!(row_menu(&mut rt, 1).iter().any(|l| l == RENAME), "one torrent: offered");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    let menu = row_menu(&mut rt, 1);
    assert!(!menu.iter().any(|l| l == RENAME), "several: not offered: {menu:?}");
}

/// What torrent `id`'s row menu offers, opened on its `column` cell (Name's may not be there).
fn row_menu_at(rt: &mut Runtime<Phantom>, id: TorrentId, column: crate::model::layout::Column) -> Vec<String> {
    let (x, y) = rt.find(&format!("rows/{id}/{}", column.id())).expect("the cell");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
    let menu = drawn_menu(rt, "row-menu").expect("a row menu").into_iter().flatten().collect();
    escape(rt);
    menu
}

/// Whether the field is in the details pane's header.
fn in_header(rt: &Runtime<Phantom>) -> bool {
    let elements = rt.elements();
    let Some(header) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("details-header")) else { return false };
    let inside = format!("{}/", header.path);
    elements.iter().any(|e| !e.exiting && e.key.as_deref() == Some("rename-field") && e.path.starts_with(&inside))
}

/// Rename… opens the field in the details pane's header, so it's offered whenever the pane shows
/// one torrent, whether or not the list shows its Name column: hidden by the header menu, or
/// dropped by a narrow window (ADR 0013).
#[test]
fn rename_opens_in_the_header_with_the_name_column_hidden_or_dropped() {
    use crate::model::layout::Column;
    let mut rt = app(with_a_movie());
    rt.update(|s, _| {
        s.prefs.kept.hidden.insert(Column::Name);
    });
    settle(&mut rt);
    tap(&mut rt, "rows/1");
    let menu = row_menu_at(&mut rt, 1, Column::Progress);
    assert!(menu.iter().any(|l| l == RENAME), "Name hidden: offered: {menu:?}");
    let (x, y) = rt.find("rows/1/progress").expect("the cell");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    pick_choice(&mut rt, "row-menu", RENAME);
    settle(&mut rt);
    assert_eq!(rt.state.rename.id, Some(1));
    assert!(in_header(&rt), "the field is the header's name");
    assert!(rt.elements().iter().any(|e| e.focused && e.path.contains("rename-field")), "focused");
    escape(&mut rt);

    rt.update(|s, _| {
        s.prefs.kept.hidden.remove(&Column::Name);
        let order = [Column::Progress, Column::Status, Column::Size, Column::Down, Column::Up, Column::Peers, Column::Eta, Column::Name];
        s.prefs.kept.order = crate::model::layout::ColumnOrder::try_from(order.to_vec()).unwrap();
    });
    rt.set_viewport((640.0, 540.0), 2.0);
    for _ in 0..3 {
        settle(&mut rt);
    }
    assert!(rt.find("rows/1/name").is_none(), "Name dropped by the narrow window");
    let menu = row_menu_at(&mut rt, 1, Column::Progress);
    assert!(menu.iter().any(|l| l == RENAME), "Name dropped: offered: {menu:?}");
}

/// Rename… turns the header's name into a field with the name, focused: of a torrent of one file the
/// part before its extension selected, as Finder selects a file's; of a folder, all of it.
#[test]
fn the_field_opens_with_the_name_and_the_part_before_the_extension_selected() {
    let mut rt = app(with_a_movie());
    for (id, name, selected) in [(2, "Movie.mkv", "Movie"), (1, "Album", "Album")] {
        open_field(&mut rt, id);
        let r = &rt.state.rename;
        assert_eq!((r.id, r.field.text.as_str()), (Some(id), name));
        assert_eq!(&r.field.text[r.field.anchor.min(r.field.focus)..r.field.anchor.max(r.field.focus)], selected, "{name}");
        assert!(r.field.focused && in_header(&rt), "{name}: drawn in the header, focused");
        escape(&mut rt);
    }
}

/// Typing over the selection and Return renames it: the row shows the new name (the extension
/// kept), so do search and sort, and its details its info's own as the original; the status bar
/// says it's renamed, with an Undo. ⌘Z renames it back, and the original name leaves the pane.
#[test]
fn return_renames_it_and_undo_renames_it_back() {
    let mut rt = app(with_a_movie());
    open_field(&mut rt, 2);
    rt.paste("Film");
    enter(&mut rt);
    assert!(rt.state.rename.id.is_none() && rt.find("rename-field").is_none(), "the field closes");
    assert_eq!(find_torrent(&rt, 2).display_name(), "Film.mkv");
    assert!(texts(&rt, "rows/2/name").contains(&"Film.mkv".to_owned()), "the row");
    assert!(texts(&rt, "details-name").contains(&"Film.mkv".to_owned()), "the header");
    tap(&mut rt, "rows/2");
    assert_eq!(name_hover(&rt).as_deref(), Some("Film.mkv\nMovie.mkv"), "the details pane's name, on hover (#33)");
    assert_eq!(notice(&rt), Some("Renamed"));
    sort(&mut rt.state.torrents, "name", SortDir::Ascending);
    let names: Vec<&str> = rt.state.torrents.iter().map(|t| t.display_name()).collect();
    assert_eq!(names, ["a", "Album", "Film.mkv"], "sorted by its new name");

    shortcut(&mut rt, InApp::Undo);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 2).display_name(), "Movie.mkv");
    assert_eq!(name_hover(&rt), None, "no original name once it's its own again");
}

/// What the details pane's name says on hover: its original name, when a Rename gave it another.
fn name_hover(rt: &Runtime<Phantom>) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some("details-name")).and_then(|e| e.help)
}

/// Escape cancels: the name stays, nothing is sent.
#[test]
fn escape_cancels() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("Other");
    escape(&mut rt);
    assert!(rt.state.rename.id.is_none() && rt.find("rename-field").is_none(), "Escape");
    assert_eq!(find_torrent(&rt, 1).display_name(), "Album");
    assert!(renames_sent(&probe).is_empty(), "nothing sent");
}

/// A click away renames, as Return does (Finder's way): the field closes, the row and the header
/// show the new name.
#[test]
fn a_click_away_renames() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("Other");
    settle(&mut rt);
    tap(&mut rt, "magnet-field");
    settle(&mut rt);
    assert!(rt.state.rename.id.is_none() && rt.find("rename-field").is_none(), "the field closes");
    assert_eq!(renames_sent(&probe).len(), 1, "renamed");
    assert_eq!(find_torrent(&rt, 1).display_name(), "Other");
    assert!(texts(&rt, "rows/1/name").contains(&"Other".to_owned()), "the row");
    assert!(texts(&rt, "details-name").contains(&"Other".to_owned()), "the header");
}

/// A click on another row, with the field open, renames the torrent whose field it is, not the
/// row clicked: the click takes the focus from the field (that renames), then the pane shows the
/// other torrent.
#[test]
fn a_click_on_another_row_renames_the_torrent_of_the_field_only() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("Other");
    settle(&mut rt);
    tap(&mut rt, "rows/2");
    settle(&mut rt);
    let sent: Vec<Vec<TorrentId>> = renames_sent(&probe).into_iter().map(|(ids, _)| ids).collect();
    assert_eq!(sent, [vec![1]], "only the album");
    assert_eq!(find_torrent(&rt, 1).display_name(), "Other");
    assert_eq!(find_torrent(&rt, 2).display_name(), "Movie.mkv", "the row clicked keeps its name");
}

/// The pane showing another torrent while the field keeps its focus (the selection changed under
/// it) cancels the rename, as Escape does: nothing is sent.
#[test]
fn the_pane_showing_another_torrent_cancels_the_open_field() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("Other");
    settle(&mut rt);
    rt.update(|s, _| crate::model::select_for_menu(&mut *s.service, &mut s.table, &s.torrents, &mut s.pane, 2));
    settle(&mut rt);
    assert!(rt.state.rename.id.is_none() && rt.find("rename-field").is_none(), "the field closes");
    assert!(renames_sent(&probe).is_empty(), "nothing is sent");
    assert_eq!(find_torrent(&rt, 1).display_name(), "Album");
}

/// A name a file can't have, with a click away: the field stays open, focused, with what was
/// typed, and the tooltip under it says why until the next keystroke. Nothing is sent.
#[test]
fn a_refused_name_keeps_the_field_open_after_a_click_away() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("AC/DC");
    settle(&mut rt);
    tap(&mut rt, "magnet-field");
    settle(&mut rt);
    let r = &rt.state.rename;
    assert_eq!((r.id, r.field.text.as_str(), r.field.focused), (Some(1), "AC/DC", true), "open, focused, as typed");
    assert_eq!(texts(&rt, "rename-note-content"), ["A name can't contain “/”"]);
    rt.paste("x");
    settle(&mut rt);
    assert!(texts(&rt, "rename-note-content").is_empty(), "gone on the next keystroke");
    assert!(renames_sent(&probe).is_empty());
}

/// The field is as wide as the header's name may be (60 % of the header's text room), for a long
/// name (which scrolls inside it) as for a short one, ending before the size on the right; its text
/// starts where the name's did. Escape shows the name again.
#[test]
fn the_field_is_the_header_names_width() {
    let long = "ubuntu-24.04.3-desktop-amd64 (with a long description of the build).iso";
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, long)]));
    for id in [1, 0] {
        tap(&mut rt, &format!("rows/{id}"));
        settle(&mut rt);
        let name = super::generated::rect_of(&rt, "details-name").expect("the name");
        let header = super::generated::rect_of(&rt, "details-header").expect("the header");
        let side = super::generated::rect_of(&rt, "details-side").expect("the size");
        open_field(&mut rt, id);
        let field = super::generated::rect_of(&rt, "rename-over").expect("the field");
        let cap = 0.6 * (header[2] - 20.0);
        let text_x = field[0] + crate::ui::TEXT_INSET;
        assert!((text_x - name[0]).abs() <= 1.0, "torrent {id}: its text where the name was: {text_x} by {}", name[0]);
        assert!((field[2] - crate::ui::TEXT_INSET - cap).abs() <= 1.0, "torrent {id}: the cap's width: {field:?}, cap {cap}");
        assert!(field[0] + field[2] <= side[0], "torrent {id}: before the size: {field:?}, {side:?}");
        assert!(in_header(&rt));
        escape(&mut rt);
        assert!(rt.find("rename-field").is_none() && rt.find("details-name").is_some(), "torrent {id}: the name again");
    }
}

/// A name a file can't have: Return keeps the field open with what was typed, and a note under it
/// says why, in Phantom's words, until the text changes. Nothing is sent.
#[test]
fn a_name_a_file_cant_have_is_said_under_the_field() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    for (typed, why) in [
        ("AC/DC", "A name can't contain “/”"),
        ("Live: 1999", "A name can't contain “:”"),
        ("..", "A name can't be “.” or “..”"),
        ("  ", "A name can't be empty"),
    ] {
        rt.update(|s, _| {
            s.rename.field = rune::TextEdit::new(typed);
            s.rename.field.focused = true;
        });
        settle(&mut rt);
        enter(&mut rt);
        assert_eq!(rt.state.rename.field.text, typed, "kept as typed");
        assert_eq!(texts(&rt, "rename-note-content"), [why], "{typed:?}");
    }
    rt.paste("x");
    settle(&mut rt);
    assert!(texts(&rt, "rename-note-content").is_empty(), "the note goes once the text changes");
    assert!(rt.state.rename.id.is_some() && renames_sent(&probe).is_empty());
}

/// Something at the new name: the service refuses it, and the status bar says so as for any
/// command, its line Phantom's words, the torrent and the path in its list. The name stays.
#[test]
fn a_name_taken_is_refused_in_the_status_bar() {
    let taken = std::path::Path::new(crate::service::DOWNLOADS).join("Live");
    let mut rt = app(with_a_movie().taken_on_disk(&taken));
    open_field(&mut rt, 1);
    rt.paste("Live");
    enter(&mut rt);
    assert_eq!(find_torrent(&rt, 1).display_name(), "Album");
    let errors = listed(&mut rt);
    let [lines] = &errors[..] else { panic!("one error: {errors:?}") };
    assert_eq!(lines[0], "Couldn't rename the torrent");
    assert!(lines.iter().any(|l| l.contains(&taken.display().to_string())), "{lines:?}");
}

/// ⌘Z with something at the old name by then: refused, and the torrent keeps its new name; the
/// Rename's notice stays, its Undo off, saying why (as any refused undo's).
#[test]
fn an_undo_with_something_at_the_old_name_is_refused() {
    let old = std::path::Path::new(crate::service::DOWNLOADS).join("Album");
    let mut rt = app(with_a_movie().taken_on_disk(&old));
    open_field(&mut rt, 1);
    rt.paste("Live");
    enter(&mut rt);
    assert_eq!(find_torrent(&rt, 1).display_name(), "Live");
    shortcut(&mut rt, InApp::Undo);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).display_name(), "Live", "not renamed back");
    let renamed = rt.state.bar.entries().iter().find(|e| e.notice.short == "Renamed").expect("its notice stays").id;
    super::generated::show_notice(&mut rt, renamed);
    tap(&mut rt, "notice-action");
    let why = rt.state.why_off_note.clone().expect("its Undo is off and says why");
    assert_eq!(why, format!("Couldn't be undone: something else is at {} now", old.display()));
}
