//! #371: ⌘Z on a Remove or a Delete whose files another listed torrent has now is refused, and
//! dropped: an error notice, "Couldn't undo the Remove", with both torrents' names in its list and a
//! button that copies the magnet link of the one that stays off the list.

use proto::Status;
use rune::Runtime;

use super::{app, settle, tap, torrent, wait};
use crate::commands::{InApp, OnTorrents};
use crate::model::Phantom;
use crate::notice::Severity;
use crate::service::MemoryService;
use crate::shortcuts;

/// Where `Album` and the other torrent clash: the fake keeps a torrent's files in `Downloads`.
const PATH: &str = "/Users/me/Downloads/Album";

fn quiet(rt: &mut Runtime<Phantom>) {
    for _ in 0..8 {
        settle(rt);
    }
}

fn key(rt: &mut Runtime<Phantom>, event: rune::KeyEvent) {
    rt.update(|_, tx| tx.blur());
    rt.key(event);
    quiet(rt);
}

fn listed(rt: &Runtime<Phantom>) -> Vec<u64> {
    let mut ids: Vec<u64> = rt.state.torrents.iter().map(|t| t.id).collect();
    ids.sort();
    ids
}

fn removed(rt: &mut Runtime<Phantom>, id: u64) {
    tap(rt, &format!("rows/{id}"));
    key(rt, shortcuts::keys(OnTorrents::Remove)[0].event());
}

fn undo(rt: &mut Runtime<Phantom>) {
    key(rt, shortcuts::keys(InApp::Undo)[0].event());
}

fn shorts(rt: &Runtime<Phantom>) -> Vec<String> {
    rt.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect()
}

/// `Album` (id 0) and another torrent at its place (id 1, `other`'s name and status), `Album` removed.
fn album_removed_then(other_name: &str, status: Status, moving: bool) -> Runtime<Phantom> {
    let mut other = torrent(1, other_name);
    other.status = status;
    let mut service = MemoryService::new().with_torrents(vec![torrent(0, "Album"), other]);
    if moving {
        service = service.moves_take(1000.0).kept_in(1, "/elsewhere");
    }
    let mut rt = app(service);
    quiet(&mut rt);
    removed(&mut rt, 0);
    if moving {
        let to = proto::AbsolutePath::new("/Users/me/Downloads").unwrap();
        rt.update(move |s, _| crate::model::moving::start(s, vec![1], to));
        quiet(&mut rt);
    }
    rt
}

/// What is in the notice's list, line by line.
fn list_of(rt: &mut Runtime<Phantom>) -> Vec<String> {
    let id = rt.state.bar.entries().last().expect("a notice").id;
    super::generated::show_notice(rt, id);
    let lines = super::generated::texts_under(rt, &format!("notice/{id}"));
    super::generated::close_popover(rt);
    lines
}

/// Whatever the other torrent is doing, or how its place is spelled: the undo is refused as an error
/// that names both torrents, the other is untouched, and the step is dropped (the next ⌘Z has nothing).
#[test]
fn cmd_z_is_refused_while_another_torrent_has_the_place() {
    for (other, name, status, moving) in [
        ("running", "Album", Status::Downloading, false),
        ("stopped", "Album", Status::Paused, false),
        ("in another case", "ALBUM", Status::Seeding, false),
        ("moving towards it", "Album", Status::Downloading, true),
    ] {
        let mut rt = album_removed_then(name, status, moving);
        let kind = |rt: &Runtime<Phantom>| rt.state.torrents.iter().find(|t| t.id == 1).map(|t| std::mem::discriminant(&t.status));
        let before = kind(&rt);
        undo(&mut rt);
        assert_eq!(listed(&rt), [1], "{other}: Album stays off the list");
        let notice = rt.state.bar.entries().last().expect("a notice").notice.clone();
        assert_eq!((notice.short.as_str(), notice.severity), ("Couldn't undo the Remove", Severity::Error), "{other}");
        assert_eq!(
            list_of(&mut rt),
            ["Couldn't undo the Remove", "Album", "Its files would clash with another torrent's:", name, PATH, "Copy magnet link"],
            "{other}: both names, told apart"
        );
        assert_eq!(notice.actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Copy magnet link"], "{other}");
        assert_eq!(kind(&rt), before, "{other}: the other isn't touched");
        let told = shorts(&rt).len();
        undo(&mut rt);
        assert_eq!(shorts(&rt).len(), told, "{other}: dropped, so nothing is left to undo");
    }
}

/// Copy magnet link puts the refused torrent's own link on the clipboard, and says so, while its
/// step is dropped: it needs nothing from the stack.
#[test]
fn copy_magnet_link_in_the_refusal_copies_the_torrent_that_stays_off_the_list() {
    let mut rt = album_removed_then("Album", Status::Downloading, false);
    undo(&mut rt);
    assert!(rt.state.undo.is_empty(), "the step is dropped");
    let notice = rt.state.bar.entries().last().expect("a notice").id;
    super::generated::show_notice(&mut rt, notice);
    tap(&mut rt, "notice-action");
    let copied = rt.copy_request.take().expect("a copy");
    assert_eq!(&*copied, format!("magnet:?xt=urn:btih:{:040x}&dn=Album", 0));
    wait(&mut rt, 0.1);
    assert_eq!(shorts(&rt).last().map(String::as_str), Some("Copied the magnet link"));
}

/// The Remove's own notice, back in its place, has its Undo off and says why, with both names.
#[test]
fn the_removes_notice_says_why_its_undo_is_off() {
    let mut rt = album_removed_then("Album", Status::Downloading, false);
    undo(&mut rt);
    let first = rt.state.bar.entries().first().expect("the Remove's notice");
    let off = first.notice.actions.iter().find(|a| a.undo).and_then(|a| a.off.as_ref()).and_then(|off| off(&rt.state));
    assert_eq!(
        off.as_deref(),
        Some(format!("Couldn't be undone: its files would clash with another torrent's, Album, at {PATH}").as_str())
    );
}

/// Several removed together: the one whose place is free comes back by itself, the other is told
/// as the torrent that didn't, with its own magnet link to copy.
#[test]
fn a_selection_brings_back_what_it_can_and_tells_the_one_it_cant() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Album"), torrent(2, "Other"), torrent(5, "Album")]));
    quiet(&mut rt);
    tap(&mut rt, "rows/0");
    rt.key(super::shift(rune::Key::Down));
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    key(&mut rt, shortcuts::keys(OnTorrents::Remove)[0].event());
    assert_eq!(listed(&rt), [5], "Album and Other removed with one press");
    undo(&mut rt);
    assert_eq!(listed(&rt), [2, 5], "Other is back by itself; Album stays off");
    let refused = rt
        .state
        .bar
        .entries()
        .iter()
        .find(|e| e.notice.short == "Couldn't undo the Remove")
        .expect("the one that didn't come back")
        .clone();
    assert_eq!(refused.notice.severity, Severity::Error);
    assert_eq!(refused.notice.actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Copy magnet link"]);
    assert!(shorts(&rt).contains(&"Back in the list".to_owned()), "{:?}", shorts(&rt));
}
