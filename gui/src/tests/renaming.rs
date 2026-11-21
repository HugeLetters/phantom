//! Rename… (#331), driven as a person would against the fake: the row menu's item, offered for
//! one torrent only, turns its Name cell into a field; Return renames, Escape or a click away
//! cancels; a name a file can't have is said under the field; what the service refuses, in the
//! status bar; ⌘Z renames it back.

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

/// Rename… turns the Name cell into a field with the name, focused: of a torrent of one file the
/// part before its extension selected, as Finder selects a file's; of a folder, all of it.
#[test]
fn the_field_opens_with_the_name_and_the_part_before_the_extension_selected() {
    let mut rt = app(with_a_movie());
    for (id, name, selected) in [(2, "Movie.mkv", "Movie"), (1, "Album", "Album")] {
        open_field(&mut rt, id);
        let r = &rt.state.rename;
        assert_eq!((r.id, r.field.text.as_str()), (Some(id), name));
        assert_eq!(&r.field.text[r.field.anchor.min(r.field.focus)..r.field.anchor.max(r.field.focus)], selected, "{name}");
        assert!(r.field.focused && rt.find("rename-field").is_some(), "{name}: drawn in its row, focused");
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
    tap(&mut rt, "rows/2");
    assert_eq!(texts(&rt, "details-original"), ["Original name: Movie.mkv"], "the details pane");
    assert_eq!(notice(&rt), Some("Renamed"));
    sort(&mut rt.state.torrents, "name", SortDir::Ascending);
    let names: Vec<&str> = rt.state.torrents.iter().map(|t| t.display_name()).collect();
    assert_eq!(names, ["a", "Album", "Film.mkv"], "sorted by its new name");

    shortcut(&mut rt, Command::Undo);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 2).display_name(), "Movie.mkv");
    assert!(texts(&rt, "details-original").is_empty(), "no original name once it's its own again");
}

/// Escape cancels, and so does a click away: the name stays, nothing is sent.
#[test]
fn escape_or_a_click_away_cancels() {
    let probe;
    let mut rt = {
        let service = with_a_movie();
        probe = service.probe();
        app(service)
    };
    open_field(&mut rt, 1);
    rt.paste("Other");
    escape(&mut rt);
    assert!(rt.state.rename.id.is_none(), "Escape");
    open_field(&mut rt, 1);
    rt.paste("Other");
    tap(&mut rt, "magnet-field");
    assert!(rt.state.rename.id.is_none() && rt.find("rename-field").is_none(), "a click away");
    assert_eq!(find_torrent(&rt, 1).display_name(), "Album");
    assert!(renames_sent(&probe).is_empty(), "nothing sent");
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
    shortcut(&mut rt, Command::Undo);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).display_name(), "Live", "not renamed back");
    let renamed = rt.state.bar.entries().iter().find(|e| e.notice.short == "Renamed").expect("its notice stays").id;
    super::generated::show_notice(&mut rt, renamed);
    tap(&mut rt, "notice-action");
    let why = rt.state.why_off_note.clone().expect("its Undo is off and says why");
    assert_eq!(why, format!("Couldn't be undone: something else is at {} now", old.display()));
}
