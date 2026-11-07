//! The UI driven like a person would, against `MemoryService` on Rune's virtual clock.

use proto::{Status, TorrentId, TorrentInfo};
use rune::{Env, Key, KeyEvent, Runtime};
use rune_kit::{SortDir, TableState};

use crate::model::{Phantom, selected_torrents, selected_with_info, sort};
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
        uploaded_bytes: 0,
        download_bytes_per_sec: 1 << 20,
        upload_bytes_per_sec: 0,
        live_peers: 3,
    }
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
