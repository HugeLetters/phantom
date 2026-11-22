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
