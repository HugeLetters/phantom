//! The UI driven like a person would, against `MemoryService` on Rune's virtual clock.

use std::path::Path;

use proto::{File, Priority, Status, TorrentId, TorrentInfo};
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use rune_kit::{SortDir, TableState};

use crate::model::{Phantom, details_torrent, reveal_target, selected_torrents, selected_with_info, sort};
use crate::platform::open_link;
use crate::service::{Event, MemoryService, Service};
use crate::{on_key, poll, views};

const MAGNET: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000aa&dn=Big+Buck+Bunny";
const FRAME: f64 = 1.0 / 60.0;

fn app(service: MemoryService) -> Runtime<Phantom> {
    let env = Env { system_dark: true, scale: 2.0 };
    let mut rt = Runtime::new(Phantom::new(Box::new(service)), views::view, (960.0, 540.0), env);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.frame(FRAME);
    rt
}

/// Two frames: handlers run during a frame, after that frame's `poll`, so the service's answer
/// to an action lands on the next one (as with the real service, only sooner).
fn settle(rt: &mut Runtime<Phantom>) {
    rt.frame(FRAME);
    rt.frame(FRAME);
}

fn tap(rt: &mut Runtime<Phantom>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn wait(rt: &mut Runtime<Phantom>, secs: f64) {
    for _ in 0..(secs / FRAME) as usize {
        rt.frame(FRAME);
    }
}

fn torrent(id: TorrentId, name: &str) -> TorrentInfo {
    TorrentInfo {
        id,
        info_hash: Some(format!("{id:040x}")),
        name: Some(name.into()),
        status: Status::Downloading,
        error: None,
        progress_bytes: 0,
        size_bytes: 1 << 30,
        full_size_bytes: 1 << 30,
        uploaded_bytes: 0,
        download_bytes_per_sec: 1 << 20,
        upload_bytes_per_sec: 0,
        live_peers: 3,
    }
}

const MIB: u64 = 1 << 20;

/// Torrent 0 ("a", one file) and torrent 1 ("Album", three files of 100, 200 and 300 MiB).
fn with_an_album() -> MemoryService {
    let file = |index, path: &str, mib: u64| File {
        index,
        path: path.into(),
        size_bytes: mib * MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
    };
    MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), torrent(1, "Album")])
        .with_files(1, vec![file(0, "01 Intro.flac", 100), file(1, "02 Song.flac", 200), file(2, "Cover/front.jpg", 300)])
}

fn shift(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods { shift: true, ..Default::default() } }
}

fn files(rt: &Runtime<Phantom>) -> Vec<(String, Priority)> {
    let details = rt.state.pane.details.as_ref().expect("details");
    details.files.iter().map(|f| (f.path.to_string_lossy().into_owned(), f.priority)).collect()
}

fn find_torrent(rt: &Runtime<Phantom>, id: TorrentId) -> &TorrentInfo {
    rt.state.torrents.iter().find(|t| t.id == id).unwrap()
}

/// A button can be pressed: rune-kit takes a disabled one out of the focus order.
fn enabled(rt: &Runtime<Phantom>, key: &str) -> bool {
    let elements = rt.elements();
    let button = elements.iter().position(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key:?}"));
    elements.iter().any(|e| e.parent == Some(button) && e.focusable)
}

/// The element keyed `key` is drawn where a click would reach it, not scrolled out of the list.
fn on_screen(rt: &Runtime<Phantom>, key: &str) -> bool {
    let Some((x, y)) = rt.find(key) else { return false };
    rt.element_at(x, y).is_some_and(|picked| picked.path().contains(key))
}

fn notice(rt: &Runtime<Phantom>) -> Option<&str> {
    rt.state.notice.as_ref().map(|n| n.text.as_str())
}

#[test]
fn a_submitted_magnet_fetches_info_then_downloads() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);

    assert_eq!(rt.state.magnet.text, "", "submitting empties the field");
    assert_eq!(rt.state.torrents.len(), 1);
    let t = &rt.state.torrents[0];
    assert_eq!(t.status, Status::FetchingInfo);
    assert_eq!(t.name.as_deref(), Some("Big Buck Bunny"), "named from the magnet's dn=");
    assert_eq!(notice(&rt), Some("Added Big Buck Bunny"));
    let id = t.id;

    wait(&mut rt, 3.0);
    let t = &rt.state.torrents[0];
    assert_eq!(t.id, id, "the same torrent, once its info arrived");
    assert_eq!(t.status, Status::Downloading);
    assert!(t.progress_bytes > 0, "and it's downloading");
}

#[test]
fn pasting_with_no_field_focused_adds_the_magnet() {
    let mut rt = app(MemoryService::new());
    rt.paste(MAGNET);
    settle(&mut rt);
    assert_eq!(rt.state.torrents.len(), 1);
}

#[test]
fn something_that_is_not_a_link_shows_an_error() {
    let mut rt = app(MemoryService::new());
    rt.paste("hello");
    settle(&mut rt);
    let notice = rt.state.notice.as_ref().expect("a notice");
    assert!(notice.error);
    assert!(rt.state.torrents.is_empty());
}

#[test]
fn adding_a_magnet_already_in_the_list_selects_that_torrent() {
    let torrents: Vec<_> = (0..30).map(|id| torrent(id, &format!("t{id}"))).collect();
    let hash = torrents[29].info_hash.clone().unwrap();
    let mut rt = app(MemoryService::new().with_torrents(torrents));
    tap(&mut rt, "rows/0");
    assert!(!on_screen(&rt, "rows/29"), "the last row starts out of view");

    rt.paste(&format!("magnet:?xt=urn:btih:{hash}&dn=Another+name"));
    settle(&mut rt);
    wait(&mut rt, 1.0);
    assert_eq!(rt.state.torrents.len(), 30, "no second row");
    assert_eq!(notice(&rt), Some("t29 is already in the list"));
    assert_eq!(selected_torrents(&rt.state.table), [29], "the existing row is selected instead");
    assert!(on_screen(&rt, "rows/29"), "and scrolled into view");
}

#[test]
fn pause_and_resume_act_on_the_selected_rows_only() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/1");
    tap(&mut rt, "pause");
    let status = |rt: &Runtime<Phantom>, id| rt.state.torrents.iter().find(|t| t.id == id).unwrap().status;
    assert_eq!(status(&rt, 1), Status::Paused);
    assert_eq!(status(&rt, 0), Status::Downloading, "the other row is untouched");

    tap(&mut rt, "resume");
    assert_eq!(status(&rt, 1), Status::Downloading);
}

#[test]
fn a_complete_torrent_paused_is_done() {
    let mut seeding = torrent(0, "a");
    seeding.status = Status::Seeding;
    let mut rt = app(MemoryService::new().with_torrents(vec![seeding]));
    tap(&mut rt, "rows/0");
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].status, Status::Done);
}

#[test]
fn remove_offers_remove_and_delete_for_downloaded_torrents() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/0");
    tap(&mut rt, "remove");
    let menu = rt.popup_menu().expect("a menu asks remove or delete");
    assert!(menu.find("remove").is_some() && menu.find("delete").is_some());
    rt.resolve_popup(Some("delete")).unwrap();
    settle(&mut rt);
    let names: Vec<_> = rt.state.torrents.iter().map(|t| t.name.clone().unwrap()).collect();
    assert_eq!(names, ["b"]);
    assert_eq!(rt.state.table.selected_len(), 0, "the selection goes with the row");
}

#[test]
fn remove_on_a_torrent_fetching_info_stops_it() {
    let mut rt = app(MemoryService::new().info_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    wait(&mut rt, 5.0);
    assert_eq!(rt.state.torrents[0].status, Status::FetchingInfo, "still fetching");
    let id = rt.state.torrents[0].id;

    tap(&mut rt, &format!("rows/{id}"));
    tap(&mut rt, "remove");
    let menu = rt.popup_menu().expect("a menu");
    assert!(menu.find("delete").is_none(), "no files to delete yet");
    rt.resolve_popup(Some("remove")).unwrap();
    settle(&mut rt);
    assert!(rt.state.torrents.is_empty());
}

#[test]
fn pause_and_resume_are_off_without_a_torrent_selected() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a")]));
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].status, Status::Downloading, "nothing selected, nothing paused");
}

#[test]
fn torrents_fetching_info_are_left_out_of_pause_and_resume() {
    let mut fetching = torrent(1, "b");
    fetching.status = Status::FetchingInfo;
    let torrents = [torrent(0, "a"), fetching];
    let mut table = TableState::new().multi_select(true);
    table.select("1");
    assert_eq!(selected_with_info(&table, &torrents), [] as [TorrentId; 0]);
    table.select("0");
    assert_eq!(selected_with_info(&table, &torrents), [0]);
}

#[test]
fn sorting_by_size_puts_the_biggest_first_when_descending() {
    let mut torrents = vec![torrent(0, "small"), torrent(1, "big")];
    torrents[1].size_bytes *= 4;
    sort(&mut torrents, "size", SortDir::Descending);
    assert_eq!(torrents[0].name.as_deref(), Some("big"));
}

#[test]
fn a_magnet_link_opened_from_outside_is_added() {
    let mut service = MemoryService::new();
    open_link(&mut service, MAGNET).unwrap();
    let events = service.poll(0.0);
    assert!(events.iter().any(|e| matches!(e, Event::Listing(l) if l.len() == 1)));
}

#[test]
fn other_links_opened_from_outside_are_refused() {
    let mut service = MemoryService::new();
    let notice = open_link(&mut service, "file:///etc/passwd").unwrap_err();
    assert!(notice.error);
    let events = service.poll(0.0);
    assert!(!events.iter().any(|e| matches!(e, Event::Listing(l) if !l.is_empty())), "nothing added");
}

#[test]
fn selecting_a_torrent_shows_its_files() {
    let mut rt = app(with_an_album());
    assert!(rt.find("details").is_none(), "no pane without a selection");
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert!(on_screen(&rt, "details"));
    assert_eq!(
        files(&rt),
        [
            ("01 Intro.flac".into(), Priority::Normal),
            ("02 Song.flac".into(), Priority::Normal),
            ("Cover/front.jpg".into(), Priority::Normal),
        ]
    );
    assert!(on_screen(&rt, "files/viewport/rows/2"), "each file is a row");
    assert!(enabled(&rt, "reveal"));
    assert!(!enabled(&rt, "file-off"), "no file selected yet");
}

#[test]
fn a_file_set_off_drops_out_of_the_size() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/2");
    tap(&mut rt, "file-off");
    settle(&mut rt);

    assert_eq!(files(&rt)[2], ("Cover/front.jpg".into(), Priority::Off));
    let t = find_torrent(&rt, 1);
    assert_eq!((t.size_bytes, t.full_size_bytes), (300 * MIB, 600 * MIB), "Size leaves it out, Full size doesn't");

    tap(&mut rt, "file-normal");
    settle(&mut rt);
    assert_eq!(files(&rt)[2].1, Priority::Normal);
    assert_eq!(find_torrent(&rt, 1).size_bytes, 600 * MIB);
}

#[test]
fn several_selected_files_change_priority_together() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    tap(&mut rt, "file-off");
    settle(&mut rt);
    let priorities: Vec<_> = files(&rt).into_iter().map(|(_, p)| p).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Off, Priority::Normal]);
    assert_eq!(find_torrent(&rt, 1).size_bytes, 300 * MIB);
}

#[test]
fn every_file_off_finishes_the_torrent() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    tap(&mut rt, "file-off");
    settle(&mut rt);
    assert_eq!(notice(&rt), None, "no refusal");
    assert!(files(&rt).iter().all(|(_, p)| *p == Priority::Off));
    let t = find_torrent(&rt, 1);
    assert_eq!((t.size_bytes, t.full_size_bytes), (0, 600 * MIB));
    assert_eq!(t.status, Status::Seeding, "nothing left to get");
    assert_eq!(t.progress(), 1.0);

    tap(&mut rt, "file-normal");
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading, "Normal files have more to get");
}

#[test]
fn the_pane_shows_only_while_exactly_one_torrent_is_selected() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/0");
    assert!(on_screen(&rt, "details"));
    rt.key(shift(Key::Down));
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    assert!(rt.find("details").is_none(), "two torrents have no one pane");
    assert_eq!(rt.state.pane.watching, None, "and the service stops sending details");

    rt.key(KeyEvent { key: Key::Up, mods: Mods::default() });
    settle(&mut rt);
    assert!(on_screen(&rt, "details"), "one again");
    tap(&mut rt, "remove");
    rt.resolve_popup(Some("remove")).unwrap();
    settle(&mut rt);
    assert!(rt.find("details").is_none(), "the removed torrent's pane goes with it");
}

#[test]
fn reveal_shows_the_file_of_a_single_file_torrent_and_the_folder_of_others() {
    let mut rt = app(with_an_album());
    let target = |rt: &Runtime<Phantom>| {
        let torrent = details_torrent(&rt.state.table, &rt.state.torrents).unwrap();
        reveal_target(torrent, &rt.state.pane).map(Path::to_owned)
    };
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    assert!(target(&rt).unwrap().ends_with("Downloads/a"), "the file itself");
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let folder = target(&rt).unwrap();
    assert!(folder.ends_with("Downloads/Album"), "the folder: {folder:?}");
    assert!(!folder.ends_with("01 Intro.flac"));
}

#[test]
fn reveal_is_off_while_fetching_info() {
    let mut rt = app(MemoryService::new().info_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    let id = rt.state.torrents[0].id;
    tap(&mut rt, &format!("rows/{id}"));
    settle(&mut rt);
    assert!(on_screen(&rt, "details"), "the pane opens, with no files yet");
    assert!(on_screen(&rt, "no-files"));
    assert_eq!(reveal_target(&rt.state.torrents[0], &rt.state.pane), None);
    assert!(!enabled(&rt, "reveal"));
}

#[test]
fn the_status_bar_speeds_stay_put_while_an_action_is_in_flight() {
    // Changing a file's priority has an action in flight for a moment; the speeds, which hug the
    // right edge, mustn't move for it (they used to flicker).
    let totals = |in_flight| {
        let rt = app(with_an_album().in_flight(in_flight));
        rt.find("totals").expect("the totals")
    };
    assert_eq!(totals(0), totals(1));
}
