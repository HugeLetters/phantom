//! The buttons of a failure's notice (#367): Try again and the Move to… and Check beside it. Per
//! button: it's shown while it applies, left out once it doesn't (torrent gone, command already
//! gone through), and it runs the ordinary command.

use proto::{Action, Status};
use rune::Runtime;

use super::generated::{show_notice, texts_under};
use super::{app, settle, tap, torrent, wait};
use crate::bar::machine::NoticeId;
use crate::model::{AddFailure, FailedAdds, Phantom, retry};
use crate::notice::Notice;
use crate::service::{AddRequest, MemoryService};

const MAGNET: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000aa&dn=Big+Buck+Bunny";

/// The texts of notice `id`'s list, its buttons last.
fn list_of(rt: &mut Runtime<Phantom>, id: NoticeId) -> Vec<String> {
    show_notice(rt, id);
    let list = texts_under(rt, &format!("notice/{id}"));
    super::generated::close_popover(rt);
    list
}

fn newest(rt: &Runtime<Phantom>) -> NoticeId {
    rt.state.bar.entries().last().expect("a notice").id
}

/// Presses button `at` of notice `id`.
fn press(rt: &mut Runtime<Phantom>, id: NoticeId, at: usize) {
    show_notice(rt, id);
    tap(rt, &format!("notice-action/{at}"));
}

fn told(rt: &mut Runtime<Phantom>, notice: Notice) -> NoticeId {
    rt.update(|s, _| s.bar.tell(notice));
    settle(rt);
    newest(rt)
}

fn paused(id: u64, name: &str) -> proto::TorrentInfo {
    proto::TorrentInfo { status: Status::Paused, ..torrent(id, name) }
}

/// Row: Couldn't delete. Try again deletes what's left and tells its own failure; the notice it
/// was pressed on is gone, so it can't run twice.
#[test]
fn a_failed_delete_offers_try_again_which_deletes_again_once() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).trash_refusing("A");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::remove_ids(s, vec![0], true));
    wait(&mut rt, 1.0);
    let first = newest(&rt);
    assert_eq!(super::notice(&rt), Some("Couldn't delete the torrent"));
    assert_eq!(list_of(&mut rt, first).last().map(String::as_str), Some("Try again"));
    press(&mut rt, first, 0);
    wait(&mut rt, 1.0);
    let ids: Vec<_> = rt.state.bar.entries().iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), 1, "the notice it was pressed on is gone; the retry's refusal is told once");
    assert_ne!(ids[0], first);
    assert_eq!(super::notice(&rt), Some("Couldn't delete the torrent"), "the ordinary Delete refused it again");
}

/// Row: Couldn't delete, torrent gone: nothing to try again.
#[test]
fn try_again_is_not_offered_for_a_torrent_that_is_gone() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).trash_refusing("A");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::remove_ids(s, vec![0], true));
    wait(&mut rt, 1.0);
    let id = newest(&rt);
    rt.update(|s, _| {
        crate::model::command(&mut *s.service, &[0], proto::TorrentCommand::Remove);
    });
    wait(&mut rt, 1.0);
    assert!(!list_of(&mut rt, id).contains(&"Try again".to_owned()), "its torrent was removed");
}

/// Row: Couldn't pause or resume. Try again acts on the ones still stopped; one that runs already
/// is left out, and with none left the button isn't there.
#[test]
fn try_again_for_a_resume_acts_on_the_failed_ones_that_still_apply() {
    let mut rt = app(MemoryService::new().with_torrents(vec![paused(0, "A"), paused(1, "B"), torrent(2, "C")]));
    wait(&mut rt, 0.5);
    let id = told(&mut rt, Notice::error("Couldn't resume 3 of 3").with_actions(vec![retry::command(vec![0, 1, 2], Action::Resume)]));
    assert_eq!(list_of(&mut rt, id).last().map(String::as_str), Some("Try again"));
    press(&mut rt, id, 0);
    wait(&mut rt, 1.0);
    let status = |rt: &Runtime<Phantom>, id| rt.state.torrents.iter().find(|t| t.id == id).map(|t| t.status.clone());
    assert_ne!(status(&rt, 0), Some(Status::Paused));
    assert_ne!(status(&rt, 1), Some(Status::Paused));
    assert!(rt.state.bar.entries().iter().all(|e| e.id != id), "pressed, its notice goes");
    // Everything it was for runs now: another notice of the same failure has nothing to offer.
    let again = told(&mut rt, Notice::error("Couldn't resume 2 of 2").with_actions(vec![retry::command(vec![0, 1], Action::Resume)]));
    assert!(!list_of(&mut rt, again).contains(&"Try again".to_owned()), "already resumed");
}

/// Row: Couldn't add. Try again sends the failed adds again, as the first time.
#[test]
fn try_again_for_a_failed_add_adds_it_again() {
    let mut rt = app(MemoryService::new());
    wait(&mut rt, 0.5);
    let request = AddRequest::link(MAGNET).expect("a link");
    let failed = AddFailure { what: "Big Buck Bunny".into(), why: "No peers have this torrent".into(), request: Some(request) };
    let refused = AddFailure { what: "notes.txt".into(), why: "Not a torrent file".into(), request: None };
    let id = told(&mut rt, FailedAdds { total: 2, failed: vec![failed, refused] }.notice());
    assert_eq!(list_of(&mut rt, id).last().map(String::as_str), Some("Try again"));
    press(&mut rt, id, 0);
    wait(&mut rt, 2.0);
    assert_eq!(rt.state.torrents.len(), 1, "the link was added");
    assert!(rt.state.bar.entries().iter().all(|e| e.id != id));
}

/// Row: Couldn't add, nothing sent before: no button for what never reached the service.
#[test]
fn a_failed_add_that_was_never_sent_offers_nothing() {
    let mut rt = app(MemoryService::new());
    let refused = AddFailure { what: "notes.txt".into(), why: "Not a torrent file".into(), request: None };
    let id = told(&mut rt, FailedAdds { total: 1, failed: vec![refused] }.notice());
    assert!(!list_of(&mut rt, id).contains(&"Try again".to_owned()));
}

/// A Move of two torrents of which one was cancelled in its clash dialog: "Moved 1 of 2".
fn partial_move() -> (Runtime<Phantom>, NoticeId) {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).holding("/Films");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0, 1], proto::AbsolutePath::new("/Films").unwrap()));
    settle(&mut rt);
    tap(&mut rt, "clash-use-existing");
    tap(&mut rt, "clash-cancel");
    wait(&mut rt, 1.0);
    assert_eq!(super::notice(&rt), Some("Moved 1 of 2"));
    let id = newest(&rt);
    (rt, id)
}

/// Row: Couldn't move, or moved only some. Try again and Move to… are for the ones left; Try again
/// asks again, its clash dialog coming back.
#[test]
fn a_partial_move_offers_try_again_and_move_to_for_the_ones_left() {
    let (mut rt, id) = partial_move();
    let list = list_of(&mut rt, id);
    assert_eq!(&list[list.len() - 2..], ["Try again", "Move to…"], "{list:?}");
    press(&mut rt, id, 0);
    settle(&mut rt);
    assert!(rt.find("clash-use-existing").is_some(), "the same Move, its clash asked again");
    assert!(rt.state.bar.entries().iter().all(|e| e.id != id));
}

/// Row: Couldn't move, torrents gone from the list: nothing to move again.
#[test]
fn a_partial_move_offers_nothing_once_its_torrents_are_gone() {
    let (mut rt, id) = partial_move();
    rt.update(|s, _| {
        crate::model::command(&mut *s.service, &[0, 1], proto::TorrentCommand::Remove);
    });
    wait(&mut rt, 1.0);
    let list = list_of(&mut rt, id);
    assert!(!list.contains(&"Try again".to_owned()) && !list.contains(&"Move to…".to_owned()), "{list:?}");
}

/// Row: Can't open a file that isn't on disk. Check follows Show folder, and checks the torrent.
#[test]
fn a_failed_open_offers_check_which_checks_the_torrent() {
    use crate::commands::OnTorrents;
    let mut rt = app(super::with_an_album());
    let shown = super::finder(&mut rt);
    let location =
        rt.state.torrents.iter().find(|t| t.id == 1).and_then(crate::model::reveal_target).map(std::path::Path::to_owned).unwrap();
    tap(&mut rt, "rows/1");
    shown.lose(&location);
    rt.key(super::shortcuts::keys(OnTorrents::Open)[2].event());
    settle(&mut rt);
    let id = newest(&rt);
    let list = list_of(&mut rt, id);
    assert!(list.ends_with(&["Check".to_owned()]), "{list:?}");
    let status = |rt: &Runtime<Phantom>| rt.state.torrents.iter().find(|t| t.id == 1).map(|t| t.status.clone());
    let before = status(&rt);
    press(&mut rt, id, list_len_of_actions(&list) - 1);
    wait(&mut rt, 0.2);
    assert_ne!(status(&rt), before, "it was checked");
}

/// How many buttons a list ends with: those of the two this scene has.
fn list_len_of_actions(list: &[String]) -> usize {
    list.iter().rev().take_while(|t| ["Show folder", "Check"].contains(&t.as_str())).count()
}

/// The popover's width in points.
fn width(rt: &Runtime<Phantom>) -> f32 {
    super::generated::rect_of(rt, "notices-panel").expect("the panel")[2]
}

/// The width is the widest notice's need, the same on every page; the minimum fits the pager and
/// Dismiss all; a path counts for its real width, until it's cut at 480.
#[test]
fn the_popover_is_as_wide_as_the_widest_notice_needs_on_every_page() {
    let mut rt = app(MemoryService::new());
    told(&mut rt, Notice::new("Copied"));
    let last = newest(&rt);
    show_notice(&mut rt, last);
    let alone = width(&rt);
    assert_eq!(alone, 240.0, "a short notice: the minimum");
    // The notices are told while the popover is open; its width follows them.
    super::generated::close_popover(&mut rt);
    let short_path = "/Films/A.torrent";
    told(&mut rt, Notice::error("Couldn't add the torrent").with_listing(crate::notice::Listing::new(short_path, "")));
    let last = newest(&rt);
    show_notice(&mut rt, last);
    let with_path = width(&rt);
    assert!(with_path < 480.0, "a short path counts for its own width, not 480: {with_path}");
    let head = super::generated::rect_of(&rt, "notices-head").expect("a head");
    let panel = super::generated::rect_of(&rt, "notices-panel").expect("the panel");
    assert!(head[0] + head[2] <= panel[0] + panel[2], "the pager fits");
    let long = "/Volumes/An External Drive With a Long Name/Downloads/A Folder Deep Down/And Deeper/And Deeper Still/file.torrent";
    super::generated::close_popover(&mut rt);
    told(&mut rt, Notice::error("Couldn't add the torrent").with_listing(crate::notice::Listing::new(long, "")));
    let last = newest(&rt);
    show_notice(&mut rt, last);
    assert_eq!(width(&rt), 480.0, "a long path is cut at 480");
    // Stepping to the narrowest notice keeps it.
    tap(&mut rt, "notice-older");
    tap(&mut rt, "notice-older");
    assert_eq!(width(&rt), 480.0, "the same on every page");
}
