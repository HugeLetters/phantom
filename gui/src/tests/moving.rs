//! Move to… for a selection (#270): offered when any selected torrent can move; one folder for all;
//! the clash dialog asks about each torrent that clashes, one after another in the list's order,
//! with "Apply to the remaining N"; the moves run one after another, each saying Waiting to move
//! until its turn, and Cancel move takes a waiting one out of the queue.

use proto::{AbsolutePath, Action, Clash, Status, TorrentCommand};
use rune::{Key, Runtime};

use super::{app, find_torrent, row_menu, settle, shift, tap, torrent, wait};
use crate::model::Phantom;
use crate::service::MemoryService;

fn to(folder: &str) -> AbsolutePath {
    AbsolutePath::new(folder).unwrap()
}

/// Three torrents, A, B and C, in the list's order, whose moves take 2 s each.
fn three() -> MemoryService {
    MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B"), torrent(2, "C")]).moves_take(2.0)
}

/// Selects rows 0 to 2, as a person does: a click and Shift+↓ twice.
fn select_all_three(rt: &mut Runtime<Phantom>) {
    tap(rt, "rows/0");
    for _ in 0..2 {
        rt.key(shift(Key::Down));
        settle(rt);
    }
    assert_eq!(rt.state.table.selected_len(), 3);
}

fn start(rt: &mut Runtime<Phantom>, folder: &str) {
    rt.update(|s, _| crate::model::moving::start(s, vec![0, 1, 2], to(folder)));
    settle(rt);
}

/// The torrent the clash dialog asks about, and how many more it'll ask about.
fn asked(rt: &Runtime<Phantom>) -> Option<(u64, usize)> {
    rt.state.clash.as_ref().map(|c| (c.id, c.remaining))
}

fn press(rt: &mut Runtime<Phantom>, key: &str) {
    tap(rt, key);
    settle(rt);
}

fn moves_sent(service: &crate::service::Probe) -> Vec<(u64, Option<Clash>)> {
    answers_sent(service).into_iter().map(|(id, answer)| (id, answer.map(|a| a.choice))).collect()
}

/// Each Move sent, with its clash's answer as sent: the choice and the files it holds for.
fn answers_sent(service: &crate::service::Probe) -> Vec<(u64, Option<proto::Answered>)> {
    service
        .commands()
        .into_iter()
        .filter_map(|(ids, c)| match c {
            TorrentCommand::Move { clash, .. } => Some((ids[0], clash)),
            _ => None,
        })
        .collect()
}

/// Offered when at least one selected torrent can move: those that can't are skipped (here, one
/// still fetching its info).
#[test]
fn move_to_is_offered_when_any_selected_torrent_can_move() {
    let fetching = || torrent(2, "C");
    let mut c = fetching();
    c.status = Status::FetchingInfo;
    let mut d = torrent(3, "D");
    d.status = Status::FetchingInfo;
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B"), c, d]);
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    let offers = |rt: &mut Runtime<Phantom>, id| row_menu(rt, id).iter().any(|l| l.starts_with("Move to"));

    tap(&mut rt, "rows/2");
    assert!(!offers(&mut rt, 2), "one that can't move: not offered");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    assert!(!offers(&mut rt, 3), "two that can't move: not offered");

    tap(&mut rt, "rows/1");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    assert!(offers(&mut rt, 1), "one that can and one that can't: offered");
    tap(&mut rt, "rows/0");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    assert!(offers(&mut rt, 0), "two that can: offered");
}

/// One dialog per clashing torrent, in the list's order; each Move takes the choice made for its
/// torrent; "Apply to the remaining N" gives the one after it to those still to be asked, and the
/// dialog counts them.
#[test]
fn each_clashing_torrent_is_asked_in_turn_and_apply_to_the_remaining_answers_the_rest() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");

    assert_eq!(asked(&rt), Some((0, 2)), "A first, two more to ask");
    assert!(rt.find("clash-apply-to-rest").is_some(), "the checkbox, with the others waiting");
    assert!(moves_sent(&probe).is_empty(), "nothing moves before every clash is answered");
    press(&mut rt, "clash-replace");
    assert_eq!(asked(&rt), Some((1, 1)), "then B, one more");
    tap(&mut rt, "clash-apply-to-rest");
    assert!(rt.state.clash.as_ref().is_some_and(|c| c.apply_to_rest), "ticked");
    press(&mut rt, "clash-use-existing");
    assert_eq!(asked(&rt), None, "C was answered with B's");
    assert!(rt.state.asking.is_none() && rt.state.clash.is_none());
    wait(&mut rt, 0.2);
    assert_eq!(
        moves_sent(&probe),
        [(0, Some(Clash::Replace)), (1, Some(Clash::UseExisting)), (2, Some(Clash::UseExisting))],
        "one Move each, in the list's order, each with its choice"
    );
}

/// The last torrent to ask about has no "Apply to the remaining": there are none.
#[test]
fn the_last_dialog_has_no_checkbox() {
    let service = three().holding("/Films");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    press(&mut rt, "clash-replace");
    press(&mut rt, "clash-replace");
    assert_eq!(asked(&rt), Some((2, 0)));
    assert!(rt.find("clash-apply-to-rest").is_none());
}

/// Cancel skips only that torrent: the others are asked about, and move. With the box ticked it
/// skips the rest. What didn't move is told, with why, in the list.
#[test]
fn cancel_skips_only_its_torrent_and_the_notice_lists_what_didnt_move() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    press(&mut rt, "clash-cancel");
    assert_eq!(asked(&rt), Some((1, 1)));
    press(&mut rt, "clash-replace");
    press(&mut rt, "clash-use-existing");
    wait(&mut rt, 7.0);
    assert_eq!(moves_sent(&probe), [(1, Some(Clash::Replace)), (2, Some(Clash::UseExisting))], "A isn't moved");
    assert_eq!(super::notice(&rt), Some("Moved 2 of 3"));

    let mut rt = app(three().holding("/Films"));
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    tap(&mut rt, "clash-apply-to-rest");
    press(&mut rt, "clash-cancel");
    assert_eq!(asked(&rt), None, "the rest are skipped too");
    assert_eq!(super::notice(&rt), Some("Moved 0 of 3"));
}

/// The Moves of a selection run one after another, in the list's order, each saying Waiting to move
/// until its turn; Cancel move takes a waiting one out, and what's cancelled is told with why.
#[test]
fn moves_run_in_the_lists_order_each_waiting_to_move_and_cancel_move_takes_one_out() {
    let mut rt = app(three());
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    wait(&mut rt, 0.5);
    let status = |rt: &Runtime<Phantom>, id| find_torrent(rt, id).status.clone();
    assert!(matches!(status(&rt, 0), Status::Moving { .. }), "{:?}", status(&rt, 0));
    assert_eq!((status(&rt, 1), status(&rt, 2)), (Status::WaitingToMove, Status::WaitingToMove));
    assert_eq!(client::display::status(&status(&rt, 1)), "Waiting to move");
    assert!(find_torrent(&rt, 2).actions.does(Action::CancelMove));
    let b = find_torrent(&rt, 1);
    assert_eq!((b.download_bytes_per_sec, b.live_peers), (1 << 20, 3), "it downloads meanwhile, and says so");

    tap(&mut rt, "rows/2");
    assert!(row_menu(&mut rt, 2).iter().any(|l| l == "Cancel move"), "offered for a waiting one");
    super::row_pick(&mut rt, 2, "Cancel move");
    wait(&mut rt, 0.2);
    assert_eq!(status(&rt, 2), Status::Downloading, "out of the queue, as it was");

    wait(&mut rt, 1.6);
    assert!(matches!(status(&rt, 1), Status::Moving { .. }), "B's turn: {:?}", status(&rt, 1));
    wait(&mut rt, 2.5);
    assert_eq!(find_torrent(&rt, 0).location.as_ref().map(|l| l.starts_with("/Films")), Some(true), "A moved");
    assert_eq!(find_torrent(&rt, 1).location.as_ref().map(|l| l.starts_with("/Films")), Some(true), "B moved");
    assert!(find_torrent(&rt, 2).location.as_ref().is_none_or(|l| !l.starts_with("/Films")), "C stayed");
    assert_eq!(super::notice(&rt), Some("Moved 2 of 3"));
}

use proptest::prelude::*;

/// What the person does in one clash dialog.
#[derive(Debug, Clone, Copy)]
struct Answer {
    /// Use existing, Replace or (None) Cancel.
    choice: Option<Clash>,
    /// "Apply to the remaining N" ticked, where it's there.
    rest: bool,
}

fn answer() -> impl Strategy<Value = Answer> {
    (prop_oneof![Just(Some(Clash::UseExisting)), Just(Some(Clash::Replace)), Just(None)], any::<bool>())
        .prop_map(|(choice, rest)| Answer { choice, rest })
}

proptest! {
    /// Over any number of torrents that all clash and any answers to their dialogs: each dialog asks
    /// about the next torrent in the list's order, counting those still to ask; an answer with the box
    /// ticked is every one's after it, and the others' own; the Moves sent are each torrent's, in the
    /// list's order, but for those Cancel skipped.
    #[test]
    fn every_clash_is_answered_by_its_own_choice_or_the_one_applied_to_the_rest(
        n in 2usize..=5,
        answers in prop::collection::vec(answer(), 5),
    ) {
        let torrents = (0..n as u64).map(|i| torrent(i, &format!("T{i}"))).collect();
        let service = MemoryService::new().with_torrents(torrents).holding("/Films");
        let probe = service.probe();
        let mut rt = app(service);
        wait(&mut rt, 0.5);
        let ids: Vec<u64> = (0..n as u64).collect();
        let start_ids = ids.clone();
        rt.update(move |s, _| crate::model::moving::start(s, start_ids, to("/Films")));
        settle(&mut rt);

        // The reference: a fold over the dialogs.
        let mut expected: Vec<(u64, Option<Clash>)> = Vec::new();
        let mut next = 0usize;
        let mut given = answers.into_iter();
        while next < n {
            let a = given.next().expect("an answer for each dialog");
            prop_assert_eq!(asked(&rt), Some((next as u64, n - next - 1)), "the dialog asks about the next, counting the rest");
            let remaining = n - next - 1;
            if remaining > 0 && a.rest {
                tap(&mut rt, "clash-apply-to-rest");
            }
            let key = match a.choice {
                Some(Clash::UseExisting) => "clash-use-existing",
                Some(Clash::Replace) => "clash-replace",
                None => "clash-cancel",
            };
            press(&mut rt, key);
            let upto = if remaining > 0 && a.rest { n } else { next + 1 };
            expected.extend((next..upto).filter(|_| a.choice.is_some()).map(|i| (i as u64, a.choice)));
            next = upto;
        }
        wait(&mut rt, 0.2);
        prop_assert_eq!(asked(&rt), None);
        prop_assert_eq!(moves_sent(&probe), expected);
        // Each answer holds for the files its own torrent's dialog was told of (the fake's, found
        // with the torrent's id), and only those (#270).
        for (id, answer) in answers_sent(&probe) {
            let found = answer.map(|a| a.found).unwrap_or_default();
            prop_assert!(!found.is_empty() && found.iter().all(|f| f.ino == id), "torrent {}: {:?}", id, found);
        }
    }
}
