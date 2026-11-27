//! #371: undoing a Remove or a Delete is refused, and dropped, while another listed torrent has the
//! files: nothing of the other torrent is touched, nothing is taken out of the Trash.

use super::moving::{album, bytes_in};
use super::*;
use std::collections::BTreeMap;

/// What the other torrent is, against `A`'s two files at `downloads/A`.
#[derive(Debug, Clone, Copy)]
enum Other {
    /// The same paths, seeding.
    Running,
    /// The same paths, paused.
    Stopped,
    /// A single file torrent named `x.bin` whose folder is `A`'s: one of `A`'s paths.
    InsideItsFolder,
    /// The same paths, with the folder spelled `a`.
    InAnotherCase,
}

/// `A` (an album of two files in `downloads`) removed or deleted, then `other` added: its id, its
/// name, `A`'s id and info hash.
async fn removed_then(dir: &Path, engine: &Engine, delete: bool, other: Other) -> (u64, &'static str, u64, String) {
    let downloads = dir.join("downloads");
    let a = add(engine, &downloads, album(dir, "A", 1).await, false).await.unwrap().id;
    wait_for(engine, a, Status::Seeding).await;
    let hash = find(engine, a).await.info_hash;
    engine.command(a, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove }).await.unwrap();
    let (b, name) = match other {
        Other::Running | Other::Stopped => {
            (add(engine, &downloads, album(dir, "A", 2).await, matches!(other, Other::Stopped)).await.unwrap().id, "A")
        }
        Other::InsideItsFolder => {
            std::fs::create_dir_all(downloads.join("A")).unwrap();
            // A removed torrent's is there still, too long for the other's: never cut short (#374).
            let _ = std::fs::remove_file(downloads.join("A/x.bin"));
            (add(engine, &downloads.join("A"), torrent_file("x.bin"), false).await.unwrap().id, "x.bin")
        }
        Other::InAnotherCase => (add(engine, &downloads, album(dir, "a", 2).await, false).await.unwrap().id, "a"),
    };
    match other {
        Other::Running | Other::InAnotherCase => wait_for(engine, b, Status::Seeding).await,
        // It makes its file at its path as it starts: before the test looks at the folder.
        Other::InsideItsFolder => {
            for _ in 0..1000 {
                if std::fs::metadata(downloads.join("A/x.bin")).is_ok_and(|m| m.len() == 16384) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Other::Stopped => {}
    }
    if matches!(other, Other::InsideItsFolder) {
        until(engine, b, |s| matches!(s, Status::Downloading { .. })).await;
    }
    (b, name, a, hash)
}

fn the_magnet_of(hash: &str, name: &str) -> String {
    format!("magnet:?xt=urn:btih:{hash}&dn={name}&tr=http%3A%2F%2F127.0.0.1%3A1%2Fannounce")
}

/// A Remove or a Delete isn't undone while another listed torrent has the files, whatever that
/// torrent is doing or how its place is spelled: refused with both torrents named, dropped, nothing
/// kept for it, the other torrent exactly as it was, and what the Delete moved still in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_is_refused_and_dropped_while_another_torrent_has_the_files() {
    for other in [Other::Running, Other::Stopped, Other::InsideItsFolder, Other::InAnotherCase] {
        for delete in [false, true] {
            let case = format!("{other:?}, delete: {delete}");
            let dir = temp();
            let engine = engine(dir.path()).await;
            let (b, name, a, hash) = removed_then(dir.path(), &engine, delete, other).await;
            let trash = bytes_in(&dir.path().join("trash"));
            let (theirs, status) = (bytes_in(&dir.path().join("downloads")), find(&engine, b).await.status);

            let (undo, why) = cant_undo(engine.undo().await);
            let action = if delete { proto::UndoAction::Delete } else { proto::UndoAction::Remove };
            assert_eq!(undo, undoable(action, a, "A"), "{case}");
            let proto::CantUndo::AnotherHasFiles { other: named, path, magnet } = why else { panic!("{case}: {why:?}") };
            assert_eq!(named, proto::Named { id: b, name: name.into() }, "{case}");
            assert_eq!(magnet, Some(the_magnet_of(&hash, "A")), "{case}: the undone torrent's own link");
            assert_eq!(path.to_path_buf(), dir.path().join("downloads/A/x.bin"), "{case}: the file of the undone torrent where they clash");

            assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [b], "{case}: the other, alone");
            assert_eq!(find(&engine, b).await.status, status, "{case}: not paused, resumed or checked");
            assert_eq!(bytes_in(&dir.path().join("downloads")), theirs, "{case}: nothing at its place written");
            assert_eq!(bytes_in(&dir.path().join("trash")), trash, "{case}: nothing taken out of the Trash");
            assert_eq!(top(&engine).await, None, "{case}: dropped");
            assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "{case}");
            engine.stop().await;
        }
    }
}

/// A Move of the other torrent that is under way, copying towards `A`'s place, has its files already
/// (as it would for a Move of its own): the undo is refused, the Move goes on and ends there, and
/// `A`'s files stay in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_is_refused_while_another_torrent_is_moving_to_its_place() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let first = album(dir.path(), "A", 1).await;
    std::fs::rename(dir.path().join("downloads/A"), other.join("A")).unwrap();
    let a = add(&engine, &other, first, false).await.unwrap().id;
    let b = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 2).await, false).await.unwrap().id;
    for id in [a, b] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    engine.command(a, TorrentCommand::Delete).await.unwrap();
    let trash = bytes_in(&dir.path().join("trash"));
    let ours = bytes_in(&dir.path().join("downloads/A"));

    let gate = super::super::trash::Gate::default();
    super::moving::holding(&engine, true, Some(gate.clone())).await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let moving = tokio::spawn(engine.command_later(b, super::moving::move_to(&other)));
    until(&engine, b, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;

    let (undo, why) = cant_undo(engine.undo().await);
    assert_eq!(undo, undoable(proto::UndoAction::Delete, a, "A"));
    assert!(
        matches!(&why, proto::CantUndo::AnotherHasFiles { other: named, path, .. }
            if named.id == b && **path == other.join("A/x.bin")),
        "{why:?}"
    );
    assert_eq!(top(&engine).await, None, "dropped");
    assert_eq!(bytes_in(&dir.path().join("trash")), trash, "nothing taken out of the Trash");

    super::super::trash::hold(&gate, false);
    moving.await.unwrap().unwrap();
    super::moving::moved(&engine, b).await;
    assert_eq!(bytes_in(&other.join("A")), ours, "the other's files, moved there whole");
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [b]);
    engine.stop().await;
}

/// Another torrent comes while an undo's files are going back, and has some of them when they're
/// back: the undo is refused as it would have been, not added beside it, and what went back goes to
/// the Trash again (every undo does all or nothing), each file as it was. Across a crash the add,
/// which waited without its info, is forgotten as any is, having touched nothing: the undo finishes.
#[tokio::test(flavor = "multi_thread")]
async fn another_torrent_added_while_the_files_go_back_refuses_the_undo() {
    for crash in [false, true] {
        let dir = temp();
        let gate = super::super::trash::Gate::default();
        let _opens = OpensTrash(gate.clone());
        let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone());
        let engine = start().await;
        let downloads = dir.path().join("downloads");
        let a = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, a, Status::Seeding).await;
        let ours = pack_bytes(dir.path());
        engine.command(a, TorrentCommand::Delete).await.unwrap();

        super::super::trash::hold(&gate, true);
        let undoing = tokio::spawn(engine.undo_later());
        held_at_the_gate(&gate).await;
        // Its folder is back; another torrent takes one of its files' paths now.
        // A torrent of one file, `a.bin`, in `A`'s folder: its path is one of `A`'s.
        let b = add_now(&engine, torrent_file("a.bin"), &downloads.join("pack")).await;
        says_its_place(&engine, b).await;
        let engine = if crash {
            engine.kill().await;
            super::super::trash::abandon(&gate);
            let _ = undoing.await;
            // The restart finishes the undo it cut short: the other torrent is in the list by then.
            let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
            for _ in 0..500 {
                if engine.inspect(|actor| super::super::undo::undoing(actor.steps.machine()).next().is_none()).await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            engine
        } else {
            super::super::trash::hold(&gate, false);
            let (undo, why) = cant_undo(undoing.await.unwrap());
            assert_eq!(undo, undoable(proto::UndoAction::Delete, a, "pack"));
            assert!(matches!(&why, proto::CantUndo::AnotherHasFiles { other, .. } if other.id == b), "{why:?}");
            engine
        };

        let listed: Vec<u64> = engine.listing().await.iter().map(|t| t.id).collect();
        assert_eq!(listed, [if crash { a } else { b }], "crash: {crash}");
        assert_eq!(top(&engine).await, None, "crash: {crash}: dropped");
        if crash {
            wait_for(&engine, a, Status::Seeding).await;
            assert_eq!(pack_bytes(dir.path()), ours);
        } else {
            let found: Vec<Vec<u8>> = bytes_in(&dir.path().join("trash")).into_values().collect();
            for (name, bytes) in ours {
                assert!(found.contains(&bytes), "{name} is in the Trash as it was");
            }
        }
        engine.stop().await;
    }
}

/// A Delete dropped by the refusal loses only what any dropped step loses: what was kept aside to
/// add the torrent again (its stash, with the `.torrent` and the pieces it had). Every file the
/// Delete moved, and the one a Move to Trash below it moved, stays in the Trash, where
/// Finder's Put Back works.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_dropped_by_the_refusal_leaves_every_file_it_moved_in_the_trash() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let a = add(&engine, &downloads, album(dir.path(), "A", 1).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;
    let first = engine.details(a).await.unwrap().files.iter().position(|f| f.path.ends_with("x.bin")).unwrap();
    engine.command(a, TorrentCommand::TrashFiles { files: vec![first] }).await.unwrap();
    engine.command(a, TorrentCommand::Delete).await.unwrap();
    let b = add(&engine, &downloads, album(dir.path(), "A", 2).await, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    let trash = bytes_in(&dir.path().join("trash"));
    assert_eq!(trash.len(), 2, "x.bin from the Move to Trash, and what the Delete moved");
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 1, "kept aside for the Delete");

    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(why, proto::CantUndo::AnotherHasFiles { .. }), "{why:?}");
    assert_eq!(bytes_in(&dir.path().join("trash")), trash, "every file still in the Trash");
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 0, "only what was kept aside for it is gone");
    assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }], "the Move to Trash below it stays");
    engine.stop().await;
}

/// Two torrents removed together, one's files taken by another torrent since: the other comes back
/// by itself, and the answer, and the word every client reads, say which didn't and why.
#[tokio::test(flavor = "multi_thread")]
async fn a_selection_undone_brings_back_the_torrent_whose_files_are_free_and_says_which_isnt() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let a = add(&engine, &downloads, album(dir.path(), "A", 1).await, false).await.unwrap().id;
    let c = add(&engine, &downloads, album(dir.path(), "C", 3).await, false).await.unwrap().id;
    for id in [a, c] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    engine.commands(vec![a, c], TorrentCommand::Remove).await.unwrap();
    let b = add(&engine, &downloads, album(dir.path(), "A", 2).await, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;

    let undone = engine.undo().await.unwrap();
    assert_eq!(undone.undo.torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [c], "the other is back");
    let refused: Vec<u64> = undone.refused.iter().map(|r| r.torrent.id).collect();
    assert_eq!(refused, [a]);
    assert!(
        matches!(&undone.refused[0].why, proto::CantUndo::AnotherHasFiles { other, magnet: Some(_), .. } if other.id == b),
        "{:?}",
        undone.refused
    );
    wait_for(&engine, c, Status::Seeding).await;
    let listed: BTreeSet<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, [b, c].into(), "A stays off the list");
    engine.stop().await;
}

/// `complete_torrent`, seeding in `downloads`, deleted, and its undo held with its folder back and
/// the torrent not added yet: its id, its files' bytes, and the undo.
async fn held_putting_back(
    dir: &Path,
    engine: &Engine,
    gate: &super::super::trash::Gate,
) -> (u64, BTreeMap<PathBuf, Vec<u8>>, tokio::task::JoinHandle<Result<proto::Undone, Failure>>) {
    let a = add(engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(engine, a, Status::Seeding).await;
    let ours = bytes_in(&dir.join("downloads/pack"));
    engine.command(a, TorrentCommand::Delete).await.unwrap();
    super::super::trash::hold(gate, true);
    let undoing = tokio::spawn(engine.undo_later());
    held_at_the_gate(gate).await;
    (a, ours, undoing)
}

/// A .torrent's add, answered as it's listed, not waited for.
async fn add_now(engine: &Engine, source: AddSource, destination: &Path) -> u64 {
    engine.add(super::super::fetched(source).await.unwrap(), false, parsed(destination)).await.unwrap().id
}

/// Waits until the add of torrent `id` got as far as its info: it says where its files go.
async fn says_its_place(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if engine.inspect(move |actor| actor.guard.places().says(id)).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the add never said where its files go");
}

/// A torrent added into a place an undo is putting files back to touches nothing there till the undo
/// is over. One with the undone torrent's files refuses it: what went back is in the Trash again,
/// as it went, and the other torrent then makes its own file and keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn an_add_into_the_place_waits_while_the_files_go_back() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let (_, ours, undoing) = held_putting_back(dir.path(), &engine, &gate).await;
    let place = dir.path().join("downloads/pack");
    let b = add_now(&engine, torrent_file("a.bin"), &place).await;
    says_its_place(&engine, b).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(bytes_in(&place), ours, "the add touched nothing yet");

    super::super::trash::hold(&gate, false);
    let (_, why) = cant_undo(undoing.await.unwrap());
    assert!(matches!(&why, proto::CantUndo::AnotherHasFiles { other, .. } if other.id == b), "{why:?}");
    let trash: BTreeMap<PathBuf, Vec<u8>> =
        bytes_in(&dir.path().join("trash")).into_iter().map(|(p, b)| (p.file_name().unwrap().into(), b)).collect();
    assert!(trash == ours, "all in the Trash, as it went: {:?}", trash.keys());
    until(&engine, b, |s| !matches!(s, Status::FetchingInfo)).await;
    for _ in 0..1000 {
        if bytes_in(&place).contains_key(Path::new("a.bin")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(4 * SIZE_CHECK_EVERY).await;
    assert!(!matches!(find(&engine, b).await.status, Status::MissingFiles { .. }), "{:?}", find(&engine, b).await.status);
    assert_eq!(bytes_in(&place), BTreeMap::from([("a.bin".into(), vec![0; 16384])]), "its own file, and only it");
    engine.stop().await;
}

/// One added inside the folder, with files of its own, waits too, and then both are in the list: they
/// share no file.
#[tokio::test(flavor = "multi_thread")]
async fn an_add_inside_the_folder_sharing_no_file_waits_and_both_are_listed() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let (a, ours, undoing) = held_putting_back(dir.path(), &engine, &gate).await;
    // However it's spelled.
    let place = dir.path().join("downloads/pack");
    let b = add_now(&engine, torrent_file("own.bin"), &dir.path().join("downloads/PACK")).await;
    says_its_place(&engine, b).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(bytes_in(&place), ours, "the add touched nothing yet");

    super::super::trash::hold(&gate, false);
    undoing.await.unwrap().unwrap();
    wait_for(&engine, a, Status::Seeding).await;
    until(&engine, b, |s| !matches!(s, Status::FetchingInfo)).await;
    let mut now = bytes_in(&place);
    now.remove(Path::new("own.bin"));
    assert_eq!(now, ours);
    engine.stop().await;
}

/// Another torrent told to move into a place an undo is putting files back to, replacing what's
/// there, is refused: its files stay where they are, whole, as does what the undo brought back.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_into_the_place_is_refused_while_the_files_go_back() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let other = dir.path().join("other");
    let b = add(&engine, &other, complete_torrent_in(dir.path(), "other", 9).await, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    let theirs = bytes_in(&other);
    let (a, ours, undoing) = held_putting_back(dir.path(), &engine, &gate).await;

    let replace = TorrentCommand::Move { to: AbsolutePath::new(dir.path().join("downloads")).unwrap(), clash: None, name: None };
    let (id, why) = refusal(engine.command(b, replace).await);
    assert_eq!(id, b);
    assert!(matches!(why, TorrentRefusal::AnothersFiles { .. }), "{why:?}");
    assert_eq!(bytes_in(&other), theirs);

    super::super::trash::hold(&gate, false);
    undoing.await.unwrap().unwrap();
    wait_for(&engine, a, Status::Seeding).await;
    wait_for(&engine, b, Status::Seeding).await;
    assert_eq!(bytes_in(&other), theirs, "the other keeps every byte");
    assert_eq!(bytes_in(&dir.path().join("downloads/pack")), ours);
    engine.stop().await;
}

/// Two torrents that had the same place, one after the other, undone at once: the first undo has the
/// place, the second is refused, naming it, and its files stay in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn of_two_undos_into_one_place_the_second_is_refused() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let downloads = dir.path().join("downloads");
    let mut ids = Vec::new();
    for k in [1, 2] {
        let id = add(&engine, &downloads, album(dir.path(), "A", k).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, TorrentCommand::Delete).await.unwrap();
        ids.push(id);
    }
    super::super::trash::hold(&gate, true);
    let newest = tokio::spawn(engine.undo_later());
    held_at_the_gate(&gate).await;
    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(&why, proto::CantUndo::AnotherHasFiles { other, .. } if other.id == ids[1] && other.name == "A"), "{why:?}");
    super::super::trash::hold(&gate, false);
    newest.await.unwrap().unwrap();
    wait_for(&engine, ids[1], Status::Seeding).await;
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [ids[1]]);
    engine.stop().await;
}

/// A crash between an undo's start being saved (with another torrent's, of one selection) and its
/// check: the start after refuses it as the check would have. Nothing goes back into the other
/// torrent's place, not even for a moment, though none of its files is there.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_a_crash_cut_short_before_its_check_is_refused_with_nothing_put_back() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let a = add(&engine, &downloads, album(dir.path(), "A", 1).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;
    engine.command(a, TorrentCommand::Delete).await.unwrap();
    // The other, listed with A's paths, and nothing of it on disk.
    let b = add(&engine, &downloads, album(dir.path(), "A", 2).await, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    std::fs::remove_dir_all(downloads.join("A")).unwrap();
    engine.inspect(|actor| actor.save_undoing_unchecked()).await;
    engine.kill().await;

    // Held as its first item is back, if any goes back.
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    undos_over(&engine).await;
    assert!(!downloads.join("A").exists(), "nothing of A's at the other's place");
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [b]);
    assert_eq!(top(&engine).await, None, "dropped");
    assert_eq!(bytes_in(&dir.path().join("trash")).len(), 2, "both files in the Trash");
    assert!(engine.inspect(move |actor| actor.guard.places().name(a)).await.is_none(), "its place is let go of");
    engine.stop().await;
}

/// The same with what it puts back there already as the start after refuses it: it's returned to the
/// Trash, not left at the other torrent's place.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_a_crash_cut_short_once_files_were_back_is_refused_with_them_in_the_trash_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let a = add(&engine, &downloads, album(dir.path(), "A", 1).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;
    engine.command(a, TorrentCommand::Delete).await.unwrap();
    let trash = bytes_in(&dir.path().join("trash"));
    let b = add(&engine, &downloads, album(dir.path(), "A", 2).await, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    std::fs::remove_dir_all(downloads.join("A")).unwrap();
    engine.inspect(|actor| actor.save_undoing_unchecked()).await;
    engine.kill().await;
    // Back as the undo puts it there, by a rename.
    let went: Vec<PathBuf> = std::fs::read_dir(dir.path().join("trash")).unwrap().map(|e| e.unwrap().path()).collect();
    let [went] = went.as_slice() else { panic!("the folder whole: {went:?}") };
    std::fs::rename(went, downloads.join("A")).unwrap();

    let engine = super::engine(dir.path()).await;
    undos_over(&engine).await;
    assert!(!downloads.join("A").exists(), "nothing of A's at the other's place");
    assert_eq!(bytes_in(&dir.path().join("trash")), trash, "in the Trash again, as it went");
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [b]);
    engine.stop().await;
}

/// Two undos into one place, both cut short by a crash before their checks: one is carried out at the
/// start after, and only the other is refused, its files still in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn of_two_undos_into_one_place_a_crash_cut_short_one_is_carried_out() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let mut ids = Vec::new();
    for k in [1, 2] {
        let id = add(&engine, &downloads, album(dir.path(), "A", k).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, TorrentCommand::Delete).await.unwrap();
        ids.push(id);
    }
    for _ in 0..2 {
        engine.inspect(|actor| actor.save_undoing_unchecked()).await;
    }
    engine.kill().await;

    let engine = super::engine(dir.path()).await;
    undos_over(&engine).await;
    let listed: Vec<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, [ids[1]], "the one whose place was kept first");
    wait_for(&engine, listed[0], Status::Seeding).await;
    assert_eq!(bytes_in(&dir.path().join("trash")).len(), 2, "the other's files");
    engine.stop().await;
}

/// Taken off the list while its add waits for an undo's place, a torrent says nothing of it any more,
/// and the end of an add taken off never unsays the place of the one an undo brought back since.
#[tokio::test(flavor = "multi_thread")]
async fn an_add_taken_off_the_list_says_its_place_no_more() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let (a, ours, undoing) = held_putting_back(dir.path(), &engine, &gate).await;
    let place = dir.path().join("downloads/pack");
    let b = add_now(&engine, torrent_file("a.bin"), &place).await;
    says_its_place(&engine, b).await;
    engine.command(b, TorrentCommand::Remove).await.unwrap();
    assert!(!engine.inspect(move |actor| actor.guard.places().says(b)).await, "gone with it");
    // Added once more: an end of the first one's add that was still on its way.
    let b = add_now(&engine, torrent_file("a.bin"), &place).await;
    says_its_place(&engine, b).await;
    engine.inspect(move |actor| actor.old_add_ends(b)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(engine.inspect(move |actor| actor.guard.places().says(b)).await, "the add under way still says it");
    engine.command(b, TorrentCommand::Remove).await.unwrap();

    super::super::trash::hold(&gate, false);
    undoing.await.unwrap().unwrap();
    wait_for(&engine, a, Status::Seeding).await;
    assert_eq!(bytes_in(&place), ours);
    engine.stop().await;
}

/// A partial Delete's undo cut short by a crash as it's saved as running, before its torrent is taken
/// out of the list, is begun at the start after: its torrent is back, whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_deletes_undo_a_crash_cut_short_before_it_began_brings_it_back() {
    let dir = temp();
    let refusing: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default()).await;
    let a = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;
    let ours = pack_bytes(dir.path());
    // Not only its own in the folder: its files go one by one, and one can't.
    std::fs::write(dir.path().join("downloads/pack/mine.txt"), b"mine").unwrap();
    *refusing.lock().unwrap() = vec![dir.path().join("downloads/pack/a.bin")];
    let _ = engine.command(a, TorrentCommand::Delete).await;
    assert_eq!(bytes_in(&dir.path().join("trash")).len(), 2, "partly deleted");
    engine.inspect(|actor| actor.save_undoing_unchecked()).await;
    engine.kill().await;

    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
    // Listed as back from the start: the undo itself is over.
    undos_over(&engine).await;
    wait_for(&engine, a, Status::Seeding).await;
    std::fs::remove_file(dir.path().join("downloads/pack/mine.txt")).unwrap();
    assert_eq!(pack_bytes(dir.path()), ours);
    engine.stop().await;
}

/// A partial Delete's undo cut short by a crash as its torrent is to be taken out of the list, and
/// refused at the start after: its torrent is listed still, partly deleted, so no magnet link is
/// offered to add it again. The same after a second crash, with the refusal.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_deletes_undo_refused_after_a_crash_keeps_its_torrent_listed_and_offers_no_magnet() {
    let dir = temp();
    let refusing: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default()).await;
    let pack = dir.path().join("downloads/pack");
    let a = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;
    // Not only its own in the folder: its files go one by one, and one can't.
    std::fs::write(pack.join("mine.txt"), b"mine").unwrap();
    *refusing.lock().unwrap() = vec![pack.join("a.bin")];
    let _ = engine.command(a, TorrentCommand::Delete).await;
    let trash = bytes_in(&dir.path().join("trash"));
    assert_eq!(trash.len(), 2, "partly deleted");
    let b = add(&engine, &pack, torrent_file("b.bin"), false).await.unwrap().id;
    until(&engine, b, |s| matches!(s, Status::Downloading { .. })).await;
    engine.inspect(move |actor| actor.save_undoing_taking_out(a)).await;
    engine.kill().await;

    for start in 1..=2 {
        let engine = super::engine(dir.path()).await;
        undos_over(&engine).await;
        let left = engine.subscribe().borrow().snapshot.left.last().unwrap().why.clone();
        let proto::Leave::Refused { why: proto::CantUndo::AnotherHasFiles { other, magnet, .. }, stranded } = left else {
            panic!("start {start}: {left:?}")
        };
        assert_eq!((other.id, magnet, stranded), (b, None, vec![]), "start {start}");
        assert_eq!(bytes_in(&dir.path().join("trash")), trash, "start {start}: all that went is in the Trash");
        assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<BTreeSet<_>>(), [a, b].into(), "start {start}");
        engine.kill().await;
    }
}

/// An undo's end is saved before its place is let go of: the moment it's free, no crash would resume
/// the undo into what comes there.
#[tokio::test(flavor = "multi_thread")]
async fn an_undos_place_is_let_go_of_once_its_end_is_saved() {
    let dir = temp();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let (a, _, undoing) = held_putting_back(dir.path(), &engine, &gate).await;
    let places = engine.inspect(|actor| actor.guard.places().clone()).await;
    assert!(places.name(a).is_some(), "kept");
    let data = dir.path().join("data");
    // Looked at without a pause, off the actor: saving takes far longer than this takes to see it.
    let saved = tokio::task::spawn_blocking(move || {
        while places.name(a).is_some() {
            std::hint::spin_loop();
        }
        super::super::undo::load(&data)
    });
    super::super::trash::hold(&gate, false);
    undoing.await.unwrap().unwrap();
    let saved = saved.await.unwrap();
    assert_eq!(super::super::undo::undoing(&saved).count(), 0, "its end is saved");
    engine.stop().await;
}

/// Waits until no undo is under way, or long enough for one held to show.
async fn undos_over(engine: &Engine) {
    for _ in 0..500 {
        if engine.inspect(|actor| super::super::undo::undoing(actor.steps.machine()).next().is_none()).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
