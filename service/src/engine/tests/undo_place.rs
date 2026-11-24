//! #371: undoing a Remove or a Delete is refused, and dropped, while another listed torrent has the
//! files: nothing of the other torrent is touched, nothing is taken out of the Trash.

use super::moving::{album, bytes_in};
use super::*;

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
            let proto::CantUndo::AnotherHasFiles { other: named, magnet } = why else { panic!("{case}: {why:?}") };
            assert_eq!(named, proto::Named { id: b, name: name.into() }, "{case}");
            assert_eq!(magnet, Some(the_magnet_of(&hash, "A")), "{case}: the undone torrent's own link");

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
    assert!(matches!(&why, proto::CantUndo::AnotherHasFiles { other, .. } if other.id == b), "{why:?}");
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
/// the Trash again (every undo does all or nothing). Nothing is lost: every byte of `A` is in the
/// Trash or at its place, and `A` isn't listed.
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
        let b = add(&engine, &downloads.join("pack"), torrent_file("a.bin"), false).await.unwrap().id;
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
        assert_eq!(listed, [b], "crash: {crash}: only the other torrent is listed");
        assert_eq!(top(&engine).await, None, "crash: {crash}: dropped");
        let found: Vec<Vec<u8>> = bytes_in(dir.path()).into_values().collect();
        // (`a.bin` is the other torrent's too: its add sized it, which the undo has no part in.)
        for (name, bytes) in ours.into_iter().filter(|(name, _)| name != "a.bin") {
            assert!(found.contains(&bytes), "crash: {crash}: {name} is in the Trash or at its place");
        }
        engine.stop().await;
    }
}

/// A Delete dropped by the refusal loses only what any dropped step loses: what was kept aside to
/// add the torrent again (its stash, with the `.torrent` and the pieces it had). Every file the
/// Delete moved, and the one a Move to Trash below it moved, stays in the Trash, where
/// Finder's Put Back works, and what is kept of a file in the Trash for an undo isn't erased.
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
    let (trash, parts) = (bytes_in(&dir.path().join("trash")), bytes_in(&dir.path().join("data/undo-parts")));
    assert_eq!(trash.len(), 2, "x.bin from the Move to Trash, and what the Delete moved");
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 1, "kept aside for the Delete");

    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(why, proto::CantUndo::AnotherHasFiles { .. }), "{why:?}");
    assert_eq!(bytes_in(&dir.path().join("trash")), trash, "every file still in the Trash");
    assert_eq!(bytes_in(&dir.path().join("data/undo-parts")), parts, "no kept part erased");
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
        matches!(&undone.refused[0].why, proto::CantUndo::AnotherHasFiles { other, magnet: Some(_) } if other.id == b),
        "{:?}",
        undone.refused
    );
    wait_for(&engine, c, Status::Seeding).await;
    let listed: BTreeSet<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, [b, c].into(), "A stays off the list");
    engine.stop().await;
}
