//! A torrent moved or renamed in Finder (#358), driven as a person would against the fake: its row
//! says Missing files; Rename… in the details pane's header to the name its folder has now, or
//! Move to… the folder it's in now, finds it there. Nothing moves: the status bar says "Found",
//! its name and where it is in the list, it's checked there and back to what it was doing. A
//! found Rename's ⌘Z points it back, Missing files again.

use proto::{AbsolutePath, Status, TorrentId};
use rune::{Key, KeyEvent, Mods};

use super::*;
use crate::service::DOWNLOADS;

/// The album (torrent 1, a folder of three files), seeding: its files all there.
fn seeding_album() -> MemoryService {
    let mut album = torrent(1, "Album");
    album.status = Status::Seeding;
    album.content = Some(proto::Content { size_bytes: 600 * MIB, full_size_bytes: 600 * MIB, progress_bytes: 600 * MIB });
    let file = |index, path: &str, mib: u64| File {
        index,
        path: path.into(),
        size_bytes: mib * MIB,
        progress_bytes: mib * MIB,
        priority: Priority::Normal,
        missing: false,
        on_disk: true,
    };
    MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), album])
        .with_files(1, vec![file(0, "01 Intro.flac", 100), file(1, "02 Song.flac", 200), file(2, "Cover/front.jpg", 300)])
}

fn status(rt: &Runtime<Phantom>, id: TorrentId) -> Status {
    find_torrent(rt, id).status.clone()
}

/// The newest notice, as its list draws it: its line, then the rest.
fn newest_list(rt: &mut Runtime<Phantom>) -> String {
    let id = rt.state.bar.entries().last().expect("a notice").id;
    super::generated::show_notice(rt, id);
    let list = super::generated::texts_under(rt, &format!("notice/{id}")).join("\n");
    super::generated::close_popover(rt);
    list
}

/// Its folder renamed in Finder to "Album (2024)": Missing files. Rename… in the header to that
/// name finds it: "Found", with its name and where it is; it's checked there and seeds again, as
/// before. ⌘Z points it back at "Album": Missing files again.
#[test]
fn rename_in_the_header_finds_a_folder_renamed_in_finder() {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let mut rt = app(seeding_album().moved_by_hand(1, &renamed));
    wait(&mut rt, 0.5);
    assert_eq!(status(&rt, 1), Status::MissingFiles);

    tap(&mut rt, "rows/1");
    settle(&mut rt);
    row_pick(&mut rt, 1, "Rename…");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    rt.paste("Album (2024)");
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    settle(&mut rt);
    wait(&mut rt, 0.5);

    assert_eq!(notice(&rt), Some("Found"));
    assert_eq!(newest_list(&mut rt), format!("Found\nAlbum (2024)\n{renamed}\nUndo\n⌘Z\nReveal in Finder"));
    let t = find_torrent(&rt, 1);
    assert_eq!((t.display_name(), t.location.clone()), ("Album (2024)", Some(AbsolutePath::new(&renamed).unwrap())));
    wait(&mut rt, 3.0);
    assert_eq!(status(&rt, 1), Status::Seeding, "checked there, and seeding as before");

    shortcut(&mut rt, InApp::Undo);
    wait(&mut rt, 1.0);
    let t = find_torrent(&rt, 1);
    assert_eq!((t.display_name(), t.status.clone()), ("Album", Status::MissingFiles), "pointed back, nothing moved");
}

/// Found by a Rename, then its folder renamed back to "Album" in Finder: Missing files again, and
/// ⌘Z points it back at "Album", where it's found too: checked there, and seeding again.
#[test]
fn undo_of_a_found_rename_finds_it_back_under_its_old_name() {
    use crate::service::DiskChange;
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let service = seeding_album().moved_by_hand(1, &renamed);
    let disk = service.disk();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    crate::model::rename(&mut rt.state, 1, proto::FileName::new("Album (2024)").unwrap());
    settle(&mut rt);
    wait(&mut rt, 3.0);
    assert_eq!(status(&rt, 1), Status::Seeding);

    disk.change(DiskChange::MovedByHand { id: 1, to: format!("{DOWNLOADS}/Album").into() });
    wait(&mut rt, 0.5);
    assert_eq!(status(&rt, 1), Status::MissingFiles);
    shortcut(&mut rt, InApp::Undo);
    wait(&mut rt, 3.0);
    let t = find_torrent(&rt, 1);
    assert_eq!((t.display_name(), t.status.clone()), ("Album", Status::Seeding), "pointed back, found there");
}

/// Its folder moved in Finder to /Volumes/Films: Missing files. Move to… that folder finds it: a
/// brief "Found", with its name and where it is, and it seeds there again. Not undone, as a Move
/// isn't.
#[test]
fn move_to_finds_a_folder_moved_in_finder() {
    let mut rt = app(seeding_album().moved_by_hand(1, "/Volumes/Films/Album"));
    wait(&mut rt, 0.5);
    assert_eq!(status(&rt, 1), Status::MissingFiles);

    tap(&mut rt, "rows/1");
    rt.update(|s, _| crate::model::moving::start(s, vec![1], AbsolutePath::new("/Volumes/Films").unwrap()));
    settle(&mut rt);
    wait(&mut rt, 0.5);

    assert_eq!(notice(&rt), Some("Found"));
    assert_eq!(newest_list(&mut rt), "Found\nAlbum\n/Volumes/Films/Album\nReveal in Finder");
    assert_eq!(find_torrent(&rt, 1).location, Some(AbsolutePath::new("/Volumes/Films/Album").unwrap()));
    wait(&mut rt, 3.0);
    assert_eq!(status(&rt, 1), Status::Seeding);
    assert!(rt.state.undo.is_empty(), "a Move isn't undone");
}

/// Its folder renamed in Finder, and its files there of their names and lengths but other bytes:
/// Rename… to that name finds it, and its check finds none of its pieces there. It's stopped, and a
/// warning says "Files don't match", with its name and where it is, and Reveal in Finder. Resume
/// then downloads over them, the person's choice.
#[test]
fn a_found_torrent_whose_files_do_not_match_stays_stopped_with_a_warning() {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let mut rt = app(seeding_album().moved_by_hand_onto_others(1, &renamed));
    wait(&mut rt, 0.5);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    row_pick(&mut rt, 1, "Rename…");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    rt.paste("Album (2024)");
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    settle(&mut rt);
    wait(&mut rt, 3.0);

    assert_eq!(notice(&rt), Some("Files don't match"));
    assert_eq!(
        newest_list(&mut rt),
        format!("Files don't match\nAlbum (2024)\n{renamed}\nCheck again\nDownload over them\nReveal in Finder")
    );
    let t = find_torrent(&rt, 1);
    assert_eq!((t.status.clone(), t.held), (Status::Paused, Some(proto::Held::DontMatch)), "stopped, untouched");
    let entries = rt.state.bar.entries().iter().filter(|e| e.notice.short == "Files don't match").count();
    wait(&mut rt, 2.0);
    assert_eq!(rt.state.bar.entries().iter().filter(|e| e.notice.short == "Files don't match").count(), entries, "told once");

    row_pick(&mut rt, 1, "Resume");
    wait(&mut rt, 1.0);
    let t = find_torrent(&rt, 1);
    assert_eq!((t.status.clone(), t.held), (Status::Downloading, None), "downloads over them");
}

/// ⌘Z on that Rename: pointed back, it's Missing files under its old name, and no longer held.
#[test]
fn undo_of_a_found_rename_whose_files_do_not_match_points_it_back() {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let mut rt = app(seeding_album().moved_by_hand_onto_others(1, &renamed));
    wait(&mut rt, 0.5);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    row_pick(&mut rt, 1, "Rename…");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    rt.paste("Album (2024)");
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    settle(&mut rt);
    wait(&mut rt, 3.0);
    assert_eq!(find_torrent(&rt, 1).held, Some(proto::Held::DontMatch));
    rt.key(KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    wait(&mut rt, 1.0);
    let t = find_torrent(&rt, 1);
    assert_eq!((t.status.clone(), t.held), (Status::MissingFiles, None));
}

/// The album found on files that don't match, then deleted: only taken off the list. Its notice
/// says its files don't match and stay; ⌘Z brings it back, still held, stopped.
#[test]
fn a_delete_of_files_that_do_not_match_leaves_them_and_says_so() {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let mut rt = app(seeding_album().moved_by_hand_onto_others(1, &renamed));
    wait(&mut rt, 0.5);
    crate::model::rename(&mut rt.state, 1, proto::FileName::new("Album (2024)").unwrap());
    settle(&mut rt);
    wait(&mut rt, 3.0);
    assert_eq!(find_torrent(&rt, 1).held, Some(proto::Held::DontMatch));

    tap(&mut rt, "rows/1");
    rt.update(|s, _| crate::model::remove_selected(s, true));
    settle(&mut rt);
    wait(&mut rt, 1.0);
    assert_eq!(notice(&rt), Some("Deleted"));
    assert_eq!(newest_list(&mut rt), "Deleted\nAlbum (2024)\nIts files don't match and stay.\nUndo\n⌘Z");
    assert!(rt.state.torrents.iter().all(|t| t.id != 1), "off the list");

    shortcut(&mut rt, InApp::Undo);
    wait(&mut rt, 1.0);
    let t = find_torrent(&rt, 1);
    assert_eq!((t.status.clone(), t.held), (Status::Paused, Some(proto::Held::DontMatch)), "back, held and stopped");
}

/// Two deleted in one press, one with files that don't match and one ordinary: each by its own state,
/// both told in the one notice.
#[test]
fn a_delete_of_two_tells_each_ones_files() {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let mut rt = app(seeding_album().moved_by_hand_onto_others(1, &renamed));
    wait(&mut rt, 0.5);
    crate::model::rename(&mut rt.state, 1, proto::FileName::new("Album (2024)").unwrap());
    settle(&mut rt);
    wait(&mut rt, 3.0);

    tap(&mut rt, "rows/0");
    rt.key(rune::KeyEvent { key: Key::Down, mods: Mods { shift: true, ..Default::default() } });
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    rt.update(|s, _| crate::model::remove_selected(s, true));
    settle(&mut rt);
    wait(&mut rt, 1.0);
    assert_eq!(notice(&rt), Some("Deleted 2 torrents"));
    assert_eq!(
        newest_list(&mut rt),
        "Deleted 2 torrents\na\nIts files are in the Trash.\nAlbum (2024)\nIts files don't match and stay.\nUndo\n⌘Z"
    );
}

/// The found album whose files don't match, as the app shows it: its row stopped, the warning in the bar.
fn dont_match_album() -> (Runtime<Phantom>, crate::service::Probe) {
    let renamed = format!("{DOWNLOADS}/Album (2024)");
    let service = seeding_album().moved_by_hand_onto_others(1, &renamed);
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    crate::model::rename(&mut rt.state, 1, proto::FileName::new("Album (2024)").unwrap());
    settle(&mut rt);
    wait(&mut rt, 3.0);
    assert_eq!(find_torrent(&rt, 1).held, Some(proto::Held::DontMatch));
    (rt, probe)
}

/// "Download over them" is Resume: the notice's button runs it on its own torrent, whatever is
/// selected, and the notice goes once it's resumed. The row menu and Space keep the name Resume.
#[test]
fn download_over_them_resumes_its_own_torrent_and_the_notice_goes() {
    let (mut rt, probe) = dont_match_album();
    // Another torrent selected: the notice's button is about the album alone.
    tap(&mut rt, "rows/0");
    let id = rt.state.bar.entries().last().unwrap().id;
    super::generated::show_notice(&mut rt, id);
    tap(&mut rt, "notice-action/1");
    wait(&mut rt, 1.0);
    let sent: Vec<_> = probe.commands().into_iter().filter(|(_, c)| matches!(c, TorrentCommand::Resume)).collect();
    assert_eq!(sent, [(vec![1], TorrentCommand::Resume)], "its own torrent, not the selected one");
    let t = find_torrent(&rt, 1);
    assert_eq!((t.status.clone(), t.held), (Status::Downloading, None));
    assert!(rt.state.bar.entries().iter().all(|e| e.notice.short != "Files don't match"), "settled: the notice goes");
}

/// "Check again" checks its own torrent; it stays held and the notice stays while its files still
/// don't match. A notice whose torrent was removed goes with it, and a button that would do nothing
/// isn't offered.
#[test]
fn check_again_checks_its_own_torrent_and_the_notice_goes_with_a_removed_one() {
    let (mut rt, probe) = dont_match_album();
    let id = rt.state.bar.entries().last().unwrap().id;
    super::generated::show_notice(&mut rt, id);
    tap(&mut rt, "notice-action/0");
    let sent: Vec<_> = probe.commands().into_iter().filter(|(_, c)| matches!(c, TorrentCommand::Check)).collect();
    assert_eq!(sent, [(vec![1], TorrentCommand::Check)]);
    wait(&mut rt, 3.0);
    assert_eq!(find_torrent(&rt, 1).held, Some(proto::Held::DontMatch));
    assert!(rt.state.bar.entries().iter().any(|e| e.notice.short == "Files don't match"), "still so: it stays");
    super::generated::close_popover(&mut rt);

    tap(&mut rt, "rows/1");
    rt.update(|s, _| crate::model::remove_selected(s, false));
    settle(&mut rt);
    wait(&mut rt, 1.0);
    assert!(rt.state.bar.entries().iter().all(|e| e.notice.short != "Files don't match"), "its torrent is gone: so is the notice");
}

/// A button that would do nothing now isn't shown: Check again, while it's being checked.
#[test]
fn a_button_that_would_do_nothing_is_not_offered() {
    let (mut rt, _) = dont_match_album();
    row_pick(&mut rt, 1, "Check");
    settle(&mut rt);
    let list = newest_list(&mut rt);
    assert!(!list.contains("Check again"), "{list}");
}
