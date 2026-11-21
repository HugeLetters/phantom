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
    /// Use existing, Replace, Rename… or Cancel; one not offered (Replace, for another torrent's
    /// files) is Rename… there.
    choice: Pick,
    /// "Apply to the remaining N" ticked, where it's there.
    rest: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pick {
    Use(Clash),
    Rename,
    Cancel,
}

fn answer() -> impl Strategy<Value = Answer> {
    let pick = prop_oneof![Just(Pick::Use(Clash::UseExisting)), Just(Pick::Use(Clash::Replace)), Just(Pick::Rename), Just(Pick::Cancel)];
    (pick, any::<bool>()).prop_map(|(choice, rest)| Answer { choice, rest })
}

proptest! {
    /// Over any number of torrents that all clash, some with another listed torrent's files, and
    /// any answers to their dialogs: each dialog asks about the next torrent in the list's order,
    /// counting those still to ask, and offers Replace and Use existing only for files that aren't
    /// another torrent's. An answer with the box ticked is every one's after it that it's offered
    /// to (Cancel's every one's, the others' those with no other torrent's files), and a name only
    /// its own torrent's. The Moves sent are each torrent's, in the list's order, but for those
    /// Cancel skipped, each with its own choice or name.
    #[test]
    fn every_clash_is_answered_by_its_own_choice_or_the_one_applied_to_the_rest(
        theirs in prop::collection::vec(any::<bool>(), 2..=5),
        answers in prop::collection::vec(answer(), 5),
    ) {
        let n = theirs.len();
        // Torrent i, and for each that clashes with another's, a torrent of the same name in /Films.
        let mut torrents: Vec<_> = (0..n as u64).map(|i| torrent(i, &format!("T{i}"))).collect();
        let mut service = MemoryService::new();
        for (i, _) in theirs.iter().enumerate().filter(|(_, t)| **t) {
            torrents.push(torrent(10 + i as u64, &format!("T{i}")));
        }
        service = service.with_torrents(torrents).holding("/Films");
        for (i, _) in theirs.iter().enumerate().filter(|(_, t)| **t) {
            service = service.kept_in(10 + i as u64, "/Films");
        }
        let probe = service.probe();
        let mut rt = app(service);
        wait(&mut rt, 0.5);
        let ids: Vec<u64> = (0..n as u64).collect();
        let start_ids = ids.clone();
        rt.update(move |s, _| crate::model::moving::start(s, start_ids, to("/Films")));
        settle(&mut rt);

        // The reference: a fold over the dialogs, the torrents still to be asked about in order.
        let mut expected: Vec<(u64, Option<Clash>, Option<String>)> = Vec::new();
        let mut left: Vec<usize> = (0..n).collect();
        let mut given = answers.into_iter().cycle();
        while let Some(&next) = left.first() {
            let a = given.next().expect("an answer for each dialog");
            let remaining = left.len() - 1;
            prop_assert_eq!(asked(&rt), Some((next as u64, remaining)), "the dialog asks about the next, counting the rest");
            let others = theirs[next];
            prop_assert_eq!(rt.find("clash-replace").is_some(), !others, "Replace only for files no other torrent has");
            let pick = match a.choice {
                Pick::Use(_) if others => Pick::Rename,
                pick => pick,
            };
            if remaining > 0 && a.rest {
                tap(&mut rt, "clash-apply-to-rest");
            }
            let name = format!("T{next} (2)");
            match pick {
                Pick::Use(Clash::UseExisting) => press(&mut rt, "clash-use-existing"),
                Pick::Use(Clash::Replace) => press(&mut rt, "clash-replace"),
                Pick::Cancel => press(&mut rt, "clash-cancel"),
                Pick::Rename => {
                    press(&mut rt, "clash-rename");
                    type_name(&mut rt, &name);
                    enter(&mut rt);
                }
            }
            let applied: Vec<usize> = match (pick, remaining > 0 && a.rest) {
                (Pick::Rename, _) | (_, false) => vec![next],
                (Pick::Cancel, true) => left.clone(),
                (Pick::Use(_), true) => left.iter().copied().filter(|&i| i == next || !theirs[i]).collect(),
            };
            for &i in &applied {
                match pick {
                    Pick::Use(c) => expected.push((i as u64, Some(c), None)),
                    Pick::Rename => expected.push((i as u64, None, Some(name.clone()))),
                    Pick::Cancel => {}
                }
            }
            left.retain(|i| !applied.contains(i));
        }
        wait(&mut rt, 0.2);
        prop_assert_eq!(asked(&rt), None);
        expected.sort_by_key(|(id, ..)| *id);
        let sent: Vec<_> = moves_sent(&probe).into_iter().zip(names_sent(&probe)).map(|((id, c), (_, name))| (id, c, name)).collect();
        prop_assert_eq!(sent, expected);
        // Each answer holds for the files its own torrent's dialog was told of (the fake's, found
        // with the torrent's id), and only those (#270).
        for (id, answer) in answers_sent(&probe) {
            let Some(answer) = answer else { continue };
            prop_assert!(!answer.found.is_empty() && answer.found.iter().all(|f| f.ino == id), "torrent {}: {:?}", id, answer.found);
        }
    }
}

// Rename… in the clash dialog (#270, after #331): a torrent moves in under a new name. For a clash
// with any file, the dialog offers it with Replace, Use existing and Cancel; for one with another
// listed torrent's files, only Rename… and Cancel, naming that torrent.

/// The dialog's choices, by key, as drawn (Reveal in Finder, by the files, is no choice).
fn buttons(rt: &Runtime<Phantom>) -> Vec<&'static str> {
    ["clash-cancel", "clash-rename", "clash-replace", "clash-use-existing", "clash-move"]
        .into_iter()
        .filter(|k| rt.find(k).is_some())
        .collect()
}

/// Each Move sent, with the name it takes in its new folder.
fn names_sent(service: &crate::service::Probe) -> Vec<(u64, Option<String>)> {
    let moves = service.commands().into_iter().filter_map(|(ids, c)| match c {
        TorrentCommand::Move { name, .. } => Some((ids[0], name.map(|n| n.to_string()))),
        _ => None,
    });
    moves.collect()
}

fn enter(rt: &mut Runtime<Phantom>) {
    rt.key(rune::KeyEvent { key: Key::Enter, mods: rune::Mods::default() });
    settle(rt);
}

fn type_name(rt: &mut Runtime<Phantom>, name: &str) {
    rt.update(|s, _| {
        let naming = s.clash.as_mut().and_then(|c| c.renaming.as_mut()).expect("the name field is open");
        naming.field = rune::TextEdit::new(name);
        naming.field.focused = true;
    });
    settle(rt);
}

/// A clash with a file of the person's: Replace, Use existing, Rename… and Cancel. Rename… opens a
/// name field holding the torrent's name, all of it selected (A's one file has no extension); Return
/// sends the Move under the name typed, with no clash's answer, and the row then shows that name,
/// in the new folder.
#[test]
fn a_clash_offers_rename_and_the_move_lands_under_the_name_typed() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    assert_eq!(buttons(&rt), ["clash-cancel", "clash-rename", "clash-replace", "clash-use-existing"]);

    press(&mut rt, "clash-rename");
    let field = rt.state.clash.as_ref().and_then(|c| c.renaming.as_ref()).map(|n| n.field.clone()).expect("the name field");
    assert_eq!(
        (field.text.as_str(), field.anchor, field.focus, field.focused),
        ("A (2)", 0, 5, true),
        "a name that is free, not the one that clashes"
    );
    assert_eq!(buttons(&rt), ["clash-cancel", "clash-move"], "a name to give, or Cancel");
    type_name(&mut rt, "A (2)");
    enter(&mut rt);
    assert!(rt.state.clash.is_none(), "answered");
    wait(&mut rt, 3.0);
    assert_eq!(names_sent(&probe), [(0, Some("A (2)".into()))]);
    assert_eq!(answers_sent(&probe), [(0, None)], "nothing there to answer for under the new name");
    let a = find_torrent(&rt, 0);
    assert_eq!((a.name.as_deref(), a.original_name.as_deref()), (Some("A (2)"), Some("A")));
    assert_eq!(a.location, Some(to("/Films/A (2)")));
}

/// A clash with another listed torrent's files (B is called A too, and is in /Films): only Rename…
/// and Cancel; its title says A can't go there, and its body where B keeps its files, with no word
/// of why. Cancel moves nothing; Rename… moves A in beside B's.
#[test]
fn a_clash_with_another_torrents_files_offers_only_rename_and_cancel_and_shows_its_place() {
    let torrents = vec![torrent(0, "A"), torrent(1, "A")];
    let service = MemoryService::new().with_torrents(torrents).kept_in(1, "/Films").moves_take(2.0);
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    assert_eq!(buttons(&rt), ["clash-cancel", "clash-rename"]);
    assert_eq!(rt.state.clash.as_ref().and_then(|c| c.other.as_deref()), Some("A"));
    assert_eq!(super::texts(&rt, "clash-title"), ["A can't go into Films"]);
    let body = super::texts(&rt, "clash-body");
    assert_eq!(body[..2], ["Another torrent keeps its files there:", "/Films/A"], "{body:?}");
    press(&mut rt, "clash-cancel");
    wait(&mut rt, 0.2);
    assert!(names_sent(&probe).is_empty(), "Cancel: nothing moves");

    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    press(&mut rt, "clash-rename");
    type_name(&mut rt, "A (2)");
    enter(&mut rt);
    wait(&mut rt, 3.0);
    assert_eq!(names_sent(&probe), [(0, Some("A (2)".into()))]);
    assert_eq!(find_torrent(&rt, 0).location, Some(to("/Films/A (2)")));
    assert_eq!(find_torrent(&rt, 1).location, Some(to("/Films/A")), "B's untouched");
}

/// Two torrents of one selection with the same paths (both called A), moved into an empty folder:
/// the first moves; the second is asked about up front, as clashing with the first's files-to-be,
/// so only Rename… and Cancel, naming the first (#270).
#[test]
fn two_torrents_of_one_selection_with_the_same_paths_are_asked_about_up_front() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "A")]).moves_take(2.0);
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0, 1], to("/Films")));
    settle(&mut rt);
    assert_eq!(buttons(&rt), ["clash-cancel", "clash-rename"]);
    let clash = rt.state.clash.as_ref().expect("the dialog");
    assert_eq!((clash.id, clash.other.as_deref(), clash.remaining), (1, Some("A"), 0), "only the second is asked about");
}

/// Rename… never opens with a name that clashes: A's own is at /Films, and another torrent is
/// called `A (2)` there, so the field starts with `A (3)`.
#[test]
fn rename_opens_with_a_name_nothing_in_the_folder_has() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "A (2)")]).kept_in(1, "/Films").holding("/Films");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    press(&mut rt, "clash-rename");
    let field = rt.state.clash.as_ref().and_then(|c| c.renaming.as_ref()).map(|n| n.field.text.clone());
    assert_eq!(field.as_deref(), Some("A (3)"));
}

/// The fifth review's blocking item (#270): a name ` (2)` takes past 255 bytes gets a suggestion
/// with its stem cut to fit, and the request ends (a timeout on the whole run would catch a loop).
#[test]
fn rename_opens_with_a_cut_name_when_the_torrents_is_too_long_for_a_number() {
    let long = "L".repeat(252);
    let service = MemoryService::new().with_torrents(vec![torrent(0, &long)]).holding("/Films");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    press(&mut rt, "clash-rename");
    let field = rt.state.clash.as_ref().and_then(|c| c.renaming.as_ref()).map(|n| n.field.text.clone());
    assert_eq!(field, Some(format!("{} (2)", "L".repeat(251))));
}

/// With `A (2)` to `A (999)` all taken in the folder there is nothing to suggest, and the field
/// opens with the torrent's own name, at once.
#[test]
fn rename_opens_with_the_current_name_when_every_number_is_taken() {
    let mut torrents = vec![torrent(0, "A")];
    torrents.extend((2..=proto::FileName::MAX_NUMBER).map(|n| torrent(n as u64 - 1, &format!("A ({n})"))));
    let mut service = MemoryService::new().with_torrents(torrents).holding("/Films");
    for n in 2..=proto::FileName::MAX_NUMBER {
        service = service.kept_in(n as u64 - 1, "/Films");
    }
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    press(&mut rt, "clash-rename");
    let field = rt.state.clash.as_ref().and_then(|c| c.renaming.as_ref()).map(|n| n.field.text.clone());
    assert_eq!(field.as_deref(), Some("A"));
}

/// The name field's notes are #335's: a name a file can't have keeps the field open with what was
/// typed, and says why under it, until the text changes. Nothing is sent. Escape closes the field,
/// back to the dialog's choices.
#[test]
fn the_name_field_says_why_a_name_cant_be_and_escape_goes_back_to_the_choices() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    press(&mut rt, "clash-rename");
    let long = "x".repeat(proto::FileName::MAX_BYTES + 1);
    for (typed, why) in [
        ("AC/DC", "A name can't contain “/”"),
        ("  ", "A name can't be empty"),
        (long.as_str(), "This name is too long"),
        (".phantom-moving", "Names starting with “.phantom-” are Phantom's own"),
    ] {
        type_name(&mut rt, typed);
        enter(&mut rt);
        let naming = rt.state.clash.as_ref().and_then(|c| c.renaming.as_ref()).expect("still open");
        assert_eq!(naming.field.text, typed, "kept as typed");
        assert_eq!(super::texts(&rt, "clash-name-note"), [why], "{typed:?}");
    }
    rt.paste("x");
    settle(&mut rt);
    assert!(super::texts(&rt, "clash-name-note").is_empty(), "the note goes once the text changes");

    rt.key(rune::KeyEvent { key: Key::Escape, mods: rune::Mods::default() });
    settle(&mut rt);
    assert!(rt.state.clash.as_ref().is_some_and(|c| c.renaming.is_none()), "back to the choices");
    assert_eq!(buttons(&rt), ["clash-cancel", "clash-rename", "clash-replace", "clash-use-existing"]);
    wait(&mut rt, 0.2);
    assert!(names_sent(&probe).is_empty());
}

/// A name is one torrent's: while the field is open, "Apply to the remaining N" is hidden, and the
/// name answers only that torrent, the next one being asked about. Escape brings the box back as
/// it was.
#[test]
fn the_checkbox_is_hidden_while_renaming_and_a_name_answers_only_its_torrent() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    tap(&mut rt, "clash-apply-to-rest");
    press(&mut rt, "clash-rename");
    assert!(rt.find("clash-apply-to-rest").is_none(), "hidden while renaming");
    rt.key(rune::KeyEvent { key: Key::Escape, mods: rune::Mods::default() });
    settle(&mut rt);
    assert!(rt.find("clash-apply-to-rest").is_some() && rt.state.clash.as_ref().is_some_and(|c| c.apply_to_rest), "back, ticked");

    press(&mut rt, "clash-rename");
    type_name(&mut rt, "A (2)");
    enter(&mut rt);
    assert_eq!(asked(&rt), Some((1, 1)), "B is asked next: the name was A's alone");
    press(&mut rt, "clash-replace");
    press(&mut rt, "clash-cancel");
    wait(&mut rt, 0.2);
    assert_eq!(names_sent(&probe), [(0, Some("A (2)".into())), (1, None)]);
    assert_eq!(moves_sent(&probe), [(0, None), (1, Some(Clash::Replace))]);
}

/// What the drawn element keyed `key` says on hover.
fn help(rt: &Runtime<Phantom>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key) && !e.exiting).and_then(|e| e.help)
}

/// The title is the question, with where the dialog is among the selection's ("· 1 of 3"); a row
/// per choice, each saying what it does only on hover, and no text explaining them.
#[test]
fn the_title_asks_with_a_counter_and_each_choice_says_what_it_does_on_hover() {
    let mut rt = app(three().holding("/Films"));
    wait(&mut rt, 0.5);
    select_all_three(&mut rt);
    start(&mut rt, "/Films");
    assert_eq!(super::texts(&rt, "clash-title"), ["A is already in Films · 1 of 3"]);
    assert_eq!(help(&rt, "clash-use-existing").as_deref(), Some("Keep the files there; this torrent's go to the Trash"));
    assert_eq!(help(&rt, "clash-replace").as_deref(), Some("Move the files there to the Trash"));
    assert_eq!(help(&rt, "clash-rename").as_deref(), Some("Move it in under another name"));
    let body = super::texts(&rt, "clash-body");
    assert_eq!(body, ["/Films/A", "Reveal in Finder", "Use existing", "Replace", "Rename…"], "paths and choices only");
    press(&mut rt, "clash-replace");
    assert_eq!(super::texts(&rt, "clash-title"), ["B is already in Films · 2 of 3"]);

    // One torrent alone has no counter.
    let mut rt = app(three().holding("/Films"));
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![1], to("/Films")));
    settle(&mut rt);
    assert_eq!(super::texts(&rt, "clash-title"), ["B is already in Films"]);
}

/// Return is Use existing, wherever the focus is in the dialog.
#[test]
fn return_uses_what_is_there() {
    let service = three().holding("/Films");
    let probe = service.probe();
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    enter(&mut rt);
    wait(&mut rt, 0.2);
    assert_eq!(moves_sent(&probe), [(0, Some(Clash::UseExisting))]);
}

/// A torrent of several files with all of them there: "3 files of Album are already in Films",
/// three of the paths and "and N more" for the rest. At the smallest window, one path, so the
/// choices fit.
#[test]
fn several_files_show_three_paths_then_how_many_more() {
    let file = |index, path: &str| proto::File {
        index,
        path: path.into(),
        size_bytes: 1 << 20,
        progress_bytes: 0,
        priority: proto::Priority::Normal,
        missing: false,
        on_disk: false,
    };
    let files = (0..5).map(|i| file(i, &format!("0{i} Song.flac"))).collect();
    let service = MemoryService::new().with_torrents(vec![torrent(0, "Album")]).with_files(0, files).holding_all("/Films");
    let mut rt = app(service);
    wait(&mut rt, 0.5);
    rt.update(|s, _| crate::model::moving::start(s, vec![0], to("/Films")));
    settle(&mut rt);
    assert_eq!(super::texts(&rt, "clash-title"), ["5 files of Album are already in Films"]);
    let files = super::texts(&rt, "clash-files");
    assert_eq!(
        files,
        ["/Films/Album/00 Song.flac", "Reveal in Finder", "/Films/Album/01 Song.flac", "/Films/Album/02 Song.flac", "and 2 more"]
    );

    rt.set_viewport((640.0, 320.0), 2.0);
    settle(&mut rt);
    assert_eq!(super::texts(&rt, "clash-files"), ["/Films/Album/00 Song.flac", "Reveal in Finder", "and 4 more"]);
    let dialog = super::generated::rect_of(&rt, "clash-dialog").expect("the dialog");
    for choice in ["clash-use-existing", "clash-replace", "clash-rename", "clash-cancel"] {
        let at = super::generated::rect_of(&rt, choice).expect(choice);
        assert!(
            at[1] >= dialog[1] && at[1] + at[3] <= dialog[1] + dialog[3] && at[1] + at[3] <= 320.0,
            "{choice} fits: {at:?} in {dialog:?}"
        );
    }
}

/// What Move to ▸'s submenu draws, in order, a separator as `None`.
fn move_to_menu(rt: &Runtime<Phantom>) -> Vec<Option<String>> {
    let elements = rt.elements();
    let Some(surface) = elements.iter().find(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.ends_with("-sub-menu"))) else {
        return Vec::new();
    };
    let inside = format!("{}/", surface.path);
    elements
        .iter()
        .filter(|e| !e.exiting && e.path.starts_with(&inside))
        .filter_map(|e| match (e.role, e.key.as_deref()) {
            ("menuitem", _) => Some(e.label.clone()),
            (_, Some(key)) if key.starts_with("sep") => Some(None),
            _ => None,
        })
        .collect()
}

/// Move to ▸ offers the default folder, then the folders torrents were moved to, newest first and
/// each once, then Other…; one picked moves the selection there, with no picker, and is the
/// newest from then. The palette lists the same, each as "Move to <folder>", after Move to….
#[test]
fn move_to_offers_the_default_folder_then_the_recent_ones_in_the_menu_and_the_palette() {
    let mut rt = app(three());
    wait(&mut rt, 0.5);
    for folder in ["/Volumes/Archive/Films", "/Volumes/Archive/Isos", "/Volumes/Archive/Films"] {
        rt.update(|s, _| crate::model::moving::start(s, vec![2], to(folder)));
        wait(&mut rt, 2.5);
    }
    tap(&mut rt, "rows/0");
    super::right_click_row(&mut rt, 0);
    super::pick_choice(&mut rt, "row-menu", "Move to");
    assert!(rt.state.menu.is_open(), "Move to opens its folders, the menu staying");
    let some = |l: &str| Some(l.to_owned());
    assert_eq!(move_to_menu(&rt), [some("Downloads"), some("Films"), some("Isos"), None, some("Other…")]);
    super::pick_choice(&mut rt, "row-menu", "Isos");
    assert!(rt.file_dialog().is_none(), "no picker");
    wait(&mut rt, 2.5);
    assert_eq!(find_torrent(&rt, 0).location, Some(to("/Volumes/Archive/Isos/A")), "moved there");
    assert_eq!(rt.state.recent.list(), [to("/Volumes/Archive/Isos"), to("/Volumes/Archive/Films")], "the newest first");

    rt.key(super::shortcuts::keys(crate::commands::InApp::Palette)[0].event());
    settle(&mut rt);
    for c in "move".chars() {
        rt.key(rune::KeyEvent { key: Key::Text(c.to_string()), mods: rune::Mods::default() });
    }
    settle(&mut rt);
    let listed: Vec<String> = crate::model::palette::listed(&rt.state).into_iter().map(|e| e.label).collect();
    for want in ["Move to…", "Move to Downloads", "Move to Isos", "Move to Films"] {
        assert!(listed.iter().any(|l| l == want), "{want:?} in {listed:?}");
    }
    let films = listed.iter().position(|l| l == "Move to Films").unwrap();
    rt.update(move |s, tx| crate::model::palette::pick(s, films, tx));
    wait(&mut rt, 2.5);
    assert_eq!(find_torrent(&rt, 0).location, Some(to("/Volumes/Archive/Films/A")), "moved from the palette");
}

/// Move to ▸ keeps the newest five besides the default; Other… asks with the picker.
#[test]
fn move_to_lists_at_most_five_recent_folders_and_other_asks() {
    let mut rt = app(three());
    wait(&mut rt, 0.5);
    rt.update(|s, _| {
        for f in ["/a", "/b", "/c", "/d", "/e", "/f", "/g"] {
            s.recent.record(to(f));
        }
    });
    tap(&mut rt, "rows/0");
    super::right_click_row(&mut rt, 0);
    super::pick_choice(&mut rt, "row-menu", "Move to");
    let names: Vec<String> = move_to_menu(&rt).into_iter().flatten().collect();
    assert_eq!(names, ["Downloads", "g", "f", "e", "d", "c", "Other…"]);
    super::pick_choice(&mut rt, "row-menu", "Other…");
    assert!(rt.file_dialog().is_some(), "Other… asks where");
}
