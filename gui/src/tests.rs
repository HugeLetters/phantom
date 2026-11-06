//! The UI driven like a person would, against `MemoryDaemon` on Rune's virtual clock.

use rune::{Env, Key, KeyEvent, Runtime};

use super::*;

const MAGNET: &str = "magnet:?xt=urn:btih:0000000000000000000000000000000000000001&dn=Big+Buck+Bunny";
const FRAME: f64 = 1.0 / 60.0;

fn app(daemon: MemoryDaemon) -> Runtime<Phantom> {
    let env = Env { system_dark: true, scale: 2.0 };
    let mut rt = Runtime::new(Phantom::new(Box::new(daemon)), view::view, (960.0, 540.0), env);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.frame(FRAME);
    rt
}

/// Two frames: handlers run during a frame, after that frame's `poll`, so the daemon's answer
/// to an action lands on the next one (as with the real daemon, only sooner).
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
        info_hash: format!("{id:040x}"),
        name: Some(name.into()),
        state: TorrentState::Live,
        finished: false,
        error: None,
        progress_bytes: 0,
        total_bytes: 1 << 30,
        uploaded_bytes: 0,
        download_bytes_per_sec: 1 << 20,
        upload_bytes_per_sec: 0,
        live_peers: 3,
    }
}

#[test]
fn a_submitted_magnet_waits_as_pending_then_downloads() {
    let mut rt = app(MemoryDaemon::new());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);

    assert_eq!(rt.state.magnet.text, "", "submitting empties the field");
    assert_eq!(rt.state.pending.len(), 1);
    assert_eq!(rt.state.pending[0].name.as_deref(), Some("Big Buck Bunny"), "named from the magnet's dn=");
    assert!(rt.state.torrents.is_empty());

    wait(&mut rt, 3.0);
    assert!(rt.state.pending.is_empty(), "metadata arrived");
    let t = &rt.state.torrents[0];
    assert_eq!(t.name.as_deref(), Some("Big Buck Bunny"));
    assert_eq!(t.state, TorrentState::Live);
    assert!(t.progress_bytes > 0, "and it's downloading");
}

#[test]
fn pasting_with_no_field_focused_adds_the_magnet() {
    let mut rt = app(MemoryDaemon::new());
    rt.paste(MAGNET);
    settle(&mut rt);
    assert_eq!(rt.state.pending.len(), 1);
}

#[test]
fn something_that_is_not_a_link_shows_an_error() {
    let mut rt = app(MemoryDaemon::new());
    rt.paste("hello");
    settle(&mut rt);
    let notice = rt.state.notice.as_ref().expect("a notice");
    assert!(notice.error);
    assert!(rt.state.pending.is_empty() && rt.state.torrents.is_empty());
}

#[test]
fn pause_and_resume_act_on_the_selected_rows_only() {
    let mut rt = app(MemoryDaemon::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/1");
    tap(&mut rt, "pause");
    let state = |rt: &Runtime<Phantom>, id| rt.state.torrents.iter().find(|t| t.id == id).unwrap().state;
    assert_eq!(state(&rt, 1), TorrentState::Paused);
    assert_eq!(state(&rt, 0), TorrentState::Live, "the other row is untouched");

    tap(&mut rt, "resume");
    assert_eq!(state(&rt, 1), TorrentState::Live);
}

#[test]
fn remove_asks_whether_to_keep_files_then_removes() {
    let mut rt = app(MemoryDaemon::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/0");
    tap(&mut rt, "remove");
    assert!(rt.popup_menu().is_some(), "a menu asks keep or delete");
    rt.resolve_popup(Some("delete")).unwrap();
    settle(&mut rt);
    let names: Vec<_> = rt.state.torrents.iter().map(|t| t.name.clone().unwrap()).collect();
    assert_eq!(names, ["b"]);
    assert_eq!(rt.state.table.selected_len(), 0, "the selection goes with the row");
}

#[test]
fn a_magnet_that_never_resolves_can_be_stopped() {
    let mut rt = app(MemoryDaemon::new().resolve_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    wait(&mut rt, 5.0);
    assert_eq!(rt.state.pending.len(), 1, "still waiting");
    assert!(rt.state.pending[0].elapsed_secs >= 4, "and the row says for how long");

    tap(&mut rt, "rows/p0");
    tap(&mut rt, "remove");
    rt.resolve_popup(Some("keep")).unwrap();
    settle(&mut rt);
    assert!(rt.state.pending.is_empty());
    assert_eq!(rt.state.notice.as_ref().map(|n| n.text.as_str()), Some("Stopped adding"));
}

#[test]
fn pause_and_resume_are_off_without_a_torrent_selected() {
    let mut rt = app(MemoryDaemon::new().with_torrents(vec![torrent(0, "a")]));
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].state, TorrentState::Live, "nothing selected, nothing paused");
}

#[test]
fn row_ids_tell_pending_adds_from_torrents() {
    let mut table = TableState::new().multi_select(true);
    table.select("p3");
    assert_eq!((selected_torrents(&table), selected_pending(&table)), (vec![], vec![3]));
    table.select("7");
    assert_eq!((selected_torrents(&table), selected_pending(&table)), (vec![7], vec![]));
}

#[test]
fn sorting_by_size_puts_the_biggest_first_when_descending() {
    let mut torrents = vec![torrent(0, "small"), torrent(1, "big")];
    torrents[1].total_bytes *= 4;
    sort(&mut torrents, "size", SortDir::Descending);
    assert_eq!(torrents[0].name.as_deref(), Some("big"));
}

#[test]
fn a_magnet_link_opened_from_outside_is_added() {
    let mut daemon = MemoryDaemon::new();
    open_link(&mut daemon, MAGNET).unwrap();
    let events = daemon.poll(0.0);
    assert!(events.iter().any(|e| matches!(e, Event::Listing(l) if l.pending.len() == 1)));
}

#[test]
fn other_links_opened_from_outside_are_refused() {
    let mut daemon = MemoryDaemon::new();
    let notice = open_link(&mut daemon, "file:///etc/passwd").unwrap_err();
    assert!(notice.error);
    let events = daemon.poll(0.0);
    assert!(!events.iter().any(|e| matches!(e, Event::Listing(l) if !l.pending.is_empty())), "nothing added");
}
