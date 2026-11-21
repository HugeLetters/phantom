//! #331: a Rename gives a torrent a new name on disk where it is (a torrent of several files' own
//! folder, a torrent of one file's file), as a Move within its destination: taken out of librqbit,
//! renamed, and added back with the pieces librqbit had. It's on the undo stack: ⌘Z renames it
//! back. Every destination is the test's own temp dir, and so is the Trash.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use proto::{CantUndo, FileName, Leave, UndoAction};

use super::letgo::{engine_counting, saved_pieces};
use super::moving::{PER_FILE, bytes_in, many_pieces, move_to, moved};
use super::*;

fn rename(name: &str) -> TorrentCommand {
    TorrentCommand::Rename { name: FileName::new(name).unwrap() }
}

/// A .torrent of one file, `one.bin` (two pieces), written into the download dir: complete once
/// added.
async fn one_file(dir: &Path) -> AddSource {
    let downloads = dir.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let bytes: Vec<u8> = (0..2 * PIECE).map(|n| (n * 11) as u8).collect();
    std::fs::write(downloads.join("one.bin"), bytes).unwrap();
    let options = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec!["http://127.0.0.1:1/announce".into()],
        piece_length: Some(PIECE as u32),
    };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let created = librqbit::create_torrent(&downloads.join("one.bin"), options, &spawner).await.unwrap();
    AddSource::TorrentFile(created.as_bytes().unwrap().to_vec())
}

fn at(path: PathBuf) -> Option<AbsolutePath> {
    Some(AbsolutePath::new(path).unwrap())
}

/// A torrent renamed while it runs, while it's paused and while it's checked: its files are under
/// the new name, nothing at the old one, and it's listed by the new name, its info's as its
/// original. One that ran keeps what it had without a full check, as a Move on the same drive
/// does: librqbit only samples the pieces it saved.
#[tokio::test(flavor = "multi_thread")]
async fn a_renamed_torrent_keeps_its_progress_running_paused_or_checked() {
    let pieces = 3 * PER_FILE;
    for paused in [false, true] {
        let dir = temp();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = engine_counting(dir.path(), reads.clone()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, many_pieces(dir.path()).await, paused).await.unwrap().id;
        let settled = if paused { Status::Done { reached: None } } else { Status::Seeding };
        wait_for(&engine, id, settled.clone()).await;
        let hash = find(&engine, id).await.info_hash;
        let before = bytes_in(&downloads.join("pack"));
        reads.store(0, Ordering::SeqCst);

        engine.command(id, rename("Album (2024)")).await.unwrap();
        moved(&engine, id).await;

        wait_for(&engine, id, settled).await;
        let t = find(&engine, id).await;
        assert_eq!((t.name.as_deref(), t.original_name.as_deref()), (Some("Album (2024)"), Some("pack")), "paused {paused}");
        assert_eq!(t.location, at(downloads.join("Album (2024)")));
        assert_eq!(bytes_in(&downloads.join("Album (2024)")), before, "paused {paused}: every file, under the new name");
        assert!(!downloads.join("pack").exists(), "paused {paused}: nothing at the old name");
        assert_eq!(t.content.unwrap().progress_bytes, (pieces * PIECE) as u64, "paused {paused}: everything it had");
        assert_eq!(saved_pieces(dir.path(), &hash), pieces as u64);
        let read = reads.load(Ordering::SeqCst);
        assert!(read < pieces / 2, "paused {paused}: sampled, not checked in full ({pieces} pieces): {read} reads");
        assert_eq!(super::stack(&engine).await, [UndoAction::Rename], "paused {paused}: on the undo stack");
        engine.stop().await;
    }
}

/// A torrent renamed while librqbit checks it: the check is taken out with it, and done again
/// under the new name, where it finds everything.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_renamed_while_it_is_checked_is_checked_again_under_its_new_name() {
    let dir = temp();
    let (engine, gate, _opens) = super::checking_engine(dir.path()).await;
    let id = super::checking_torrent(&engine, dir.path()).await;
    let downloads = dir.path().join("downloads");

    let renamed = engine.sent(id, rename("Checked")).await;
    gate.hold(false);
    renamed.await.unwrap();
    moved(&engine, id).await;
    until(&engine, id, |s| !matches!(s, Status::Checking)).await;
    let t = find(&engine, id).await;
    assert_eq!(t.location, at(downloads.join("Checked")));
    assert!(downloads.join("Checked/a.bin").exists() && !downloads.join("pack").exists());
    assert!(!matches!(t.status, Status::Error { .. } | Status::MissingFiles), "{:?}", t.status);
    engine.stop().await;
}

/// A torrent of one file: its file is renamed, the extension too if the name says so, and after
/// a restart librqbit opens it under its new name (never makes one at the info's), so it's still
/// complete, unchecked.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_of_one_file_has_its_file_renamed_and_keeps_it_after_a_restart() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, one_file(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, rename("renamed.bin")).await.unwrap();
        moved(&engine, id).await;
        wait_for(&engine, id, Status::Seeding).await;
        assert_eq!(engine.details(id).await.unwrap().files[0].path, PathBuf::from("renamed.bin"), "its file, as listed");
        engine.stop().await;
        id
    };
    let engine = engine(dir.path()).await;
    sampled(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!((t.name.as_deref(), t.original_name.as_deref()), (Some("renamed.bin"), Some("one.bin")));
    assert_eq!(t.location, at(downloads.join("renamed.bin")));
    assert_eq!(t.content.unwrap().progress_bytes, (2 * PIECE) as u64);
    assert!(!downloads.join("one.bin").exists(), "nothing made at the info's name");
    engine.stop().await;
}

/// Something at the new name, a file, a folder, a link (dangling or not): refused with its path,
/// nothing moved, nothing followed or written over, and nothing on the undo stack. A name that is
/// another listed torrent's place, its files not there (yet), is refused too.
#[tokio::test(flavor = "multi_thread")]
async fn a_name_taken_on_disk_or_by_another_torrent_is_refused_and_nothing_moves() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    let other = add(&engine, &downloads, torrent_file("Theirs"), true).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // Its file not on disk (deleted by hand, or never made): only the list says the name is taken.
    let _ = std::fs::remove_file(downloads.join("Theirs"));
    let before = bytes_in(&downloads.join("pack"));
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(downloads.join("A file"), b"mine").unwrap();
    std::fs::create_dir_all(downloads.join("A folder")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, downloads.join("A link")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), downloads.join("A dangling link")).unwrap();

    for name in ["A file", "A folder", "A link", "A dangling link"] {
        let why = refusal(engine.command(id, rename(name)).await);
        assert_eq!(why, (id, TorrentRefusal::NameTaken { path: AbsolutePath::new(downloads.join(name)).unwrap() }), "{name}");
    }
    let why = refusal(engine.command(id, rename("theirs")).await);
    let path = AbsolutePath::new(downloads.join("theirs")).unwrap();
    assert_eq!(why, (id, TorrentRefusal::NameOfAnother { path, other: "Theirs".into() }), "another torrent's, in any case");
    assert_eq!(bytes_in(&downloads.join("pack")), before, "nothing moved");
    assert_eq!(std::fs::read(downloads.join("A file")).unwrap(), b"mine", "nothing written over");
    assert!(bytes_in(&elsewhere).is_empty() && !dir.path().join("nowhere").exists(), "no link followed");
    assert_eq!(find(&engine, id).await.status, Status::Seeding);
    assert!(super::stack(&engine).await.is_empty(), "nothing to undo");
    let _ = other;
    engine.stop().await;
}

/// Its own name does nothing: nothing moves, and there's nothing to undo.
#[tokio::test(flavor = "multi_thread")]
async fn its_own_name_does_nothing() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, rename("pack")).await.unwrap();
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.original_name), (Status::Seeding, None));
    assert!(super::stack(&engine).await.is_empty());
    engine.stop().await;
}

/// ⌘Z renames it back, as one step: its files under the old name, which is its info's again (no
/// original name shown). With something at the old name by then, the undo is refused with that
/// path and dropped, and nothing is written over: it keeps its new name.
#[tokio::test(flavor = "multi_thread")]
async fn undo_renames_it_back_unless_something_is_at_the_old_name() {
    for taken in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let before = bytes_in(&downloads.join("pack"));
        engine.command(id, rename("New")).await.unwrap();
        moved(&engine, id).await;
        let seq = super::seqs(&engine)[0];
        if taken {
            std::fs::create_dir_all(downloads.join("pack")).unwrap();
            std::fs::write(downloads.join("pack/mine.txt"), b"mine").unwrap();
        }

        let undone = engine.undo().await;
        moved(&engine, id).await;
        let t = find(&engine, id).await;
        match taken {
            false => {
                let undone = undone.unwrap();
                assert_eq!((undone.undo.seq, undone.undo.action), (seq, UndoAction::Rename));
                assert_eq!((t.name.as_deref(), t.original_name, t.location), (Some("pack"), None, at(downloads.join("pack"))));
                assert_eq!(bytes_in(&downloads.join("pack")), before);
                assert!(!downloads.join("New").exists());
            }
            true => {
                let path = AbsolutePath::new(downloads.join("pack")).unwrap();
                assert!(
                    matches!(&undone, Err(Failure::Refused(proto::Refusal::Undo { why: CantUndo::PathTaken { path: p }, .. })) if *p == path),
                    "{undone:?}"
                );
                assert_eq!(t.location, at(downloads.join("New")), "it keeps its new name");
                assert_eq!(bytes_in(&downloads.join("New")), before);
                assert_eq!(bytes_in(&downloads.join("pack")), BTreeMap::from([(PathBuf::from("mine.txt"), b"mine".to_vec())]));
                let left = super::left(&engine);
                let why = Leave::Refused { why: CantUndo::PathTaken { path }, stranded: Vec::new() };
                assert!(left.contains(&proto::Left { seq, why }), "{left:?}");
            }
        }
        wait_for(&engine, id, Status::Seeding).await;
        assert!(super::stack(&engine).await.is_empty(), "taken {taken}: off the stack either way");
        engine.stop().await;
    }
}

/// ⌘Z on a Rename whose torrent is moving to another folder now is refused at once rather than
/// wait for the move: the move goes on, and the Rename is off the stack.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_rename_whose_torrent_is_moving_is_refused_rather_than_wait() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, rename("New")).await.unwrap();
    moved(&engine, id).await;
    let gate = super::super::trash::Gate::default();
    engine
        .inspect({
            let gate = gate.clone();
            move |a| a.move_hold = super::super::actor::moving::Hold { gate: Some(gate), copies: true, ..Default::default() }
        })
        .await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let moving = tokio::spawn(engine.command_later(id, move_to(&elsewhere)));
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;

    let undone = tokio::time::timeout(Duration::from_secs(2), engine.undo()).await.expect("answered at once");
    assert!(matches!(&undone, Err(Failure::Refused(proto::Refusal::Undo { why: CantUndo::Busy, .. }))), "{undone:?}");
    super::super::trash::hold(&gate, false);
    moving.await.unwrap().expect("moved");
    moved(&engine, id).await;
    assert_eq!(find(&engine, id).await.location, at(elsewhere.join("New")), "moved, under the new name");
    assert!(super::stack(&engine).await.is_empty());
    engine.stop().await;
}

/// A crash at each point a rename writes ahead: before the torrent is taken out of librqbit, the
/// next start puts it back under its old name, nothing staged, and nothing is on the stack to
/// undo; once its files are in place, before the record says it's over, the next start finishes
/// it under the new name, and ⌘Z renames it back.
#[tokio::test(flavor = "multi_thread")]
async fn a_crash_mid_rename_is_put_back_or_finished_at_the_start() {
    for once_moved in [false, true] {
        let dir = temp();
        let downloads = dir.path().join("downloads");
        let (id, hash, before) = {
            let engine = engine(dir.path()).await;
            let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
            wait_for(&engine, id, Status::Seeding).await;
            let before = bytes_in(&downloads.join("pack"));
            let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let hold = crashed.clone();
            match once_moved {
                false => engine.inspect(move |a| a.move_hold.crash_before_take_out = Some(hold)).await,
                true => engine.inspect(move |a| a.move_hold.crash_once_moved = Some(hold)).await,
            }
            let _ = tokio::time::timeout(Duration::from_secs(5), engine.command(id, rename("New"))).await;
            assert!(crashed.load(Ordering::SeqCst), "once moved {once_moved}: it crashed there");
            (id, find(&engine, id).await.info_hash, before)
        };
        let engine = engine(dir.path()).await;
        moved(&engine, id).await;
        wait_for(&engine, id, Status::Seeding).await;
        assert!(!downloads.join(format!(".phantom-moving-{hash}")).exists(), "once moved {once_moved}: nothing staged");
        let (name, other) = if once_moved { ("New", "pack") } else { ("pack", "New") };
        let t = find(&engine, id).await;
        assert_eq!((t.name.as_deref(), t.location), (Some(name), at(downloads.join(name))), "once moved {once_moved}");
        assert_eq!(bytes_in(&downloads.join(name)), before, "once moved {once_moved}");
        assert!(!downloads.join(other).exists(), "once moved {once_moved}");
        assert_eq!(t.content.unwrap().progress_bytes, 3 * FULL, "once moved {once_moved}");
        match once_moved {
            false => assert!(super::stack(&engine).await.is_empty(), "put back: nothing to undo"),
            true => {
                assert_eq!(super::stack(&engine).await, [UndoAction::Rename]);
                engine.undo().await.unwrap();
                moved(&engine, id).await;
                assert_eq!(bytes_in(&downloads.join("pack")), before, "renamed back");
            }
        }
        engine.stop().await;
    }
}

/// A restart keeps the name: its record has it, as it has its destination.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_keeps_the_new_name() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, rename("Kept")).await.unwrap();
        moved(&engine, id).await;
        engine.stop().await;
        id
    };
    let engine = engine(dir.path()).await;
    sampled(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!((t.name.as_deref(), t.original_name.as_deref(), t.location), (Some("Kept"), Some("pack"), at(downloads.join("Kept"))));
    assert_eq!(t.content.unwrap().progress_bytes, 3 * FULL);
    assert!(!downloads.join("pack").exists(), "nothing made at the info's name");
    assert_eq!(super::stack(&engine).await, [UndoAction::Rename], "still undoable");
    engine.stop().await;
}

/// ⌘Z while a Rename runs (here held mid-copy, as on a drive that can't rename) stops it after
/// the chunk in flight and puts it back: its files under the old name, nothing staged, nothing on
/// the stack; its client hears it was undone.
#[tokio::test(flavor = "multi_thread")]
async fn undo_while_a_rename_runs_puts_it_back() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let (before, hash) = (bytes_in(&downloads.join("pack")), find(&engine, id).await.info_hash);
    let gate = super::super::trash::Gate::default();
    engine
        .inspect({
            let gate = gate.clone();
            move |a| a.move_hold = super::super::actor::moving::Hold { gate: Some(gate), copies: true, ..Default::default() }
        })
        .await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let renaming = tokio::spawn(engine.command_later(id, rename("New")));
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;

    let undo = tokio::spawn(engine.undo_later());
    aborting(&engine).await;
    super::super::trash::hold(&gate, false);
    undo.await.unwrap().expect("undone");
    assert!(matches!(renaming.await.unwrap(), Err(Failure::Refused(proto::Refusal::Undone))), "its client hears it was undone");
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(find(&engine, id).await.location, at(downloads.join("pack")));
    assert_eq!(bytes_in(&downloads.join("pack")), before);
    assert!(!downloads.join("New").exists() && !downloads.join(format!(".phantom-moving-{hash}")).exists());
    assert!(super::stack(&engine).await.is_empty());
    engine.stop().await;
}

/// Waits until a step is aborting: a ⌘Z took it while it ran.
async fn aborting(engine: &Engine) {
    for _ in 0..500 {
        let aborting =
            engine.inspect(|a| a.steps.machine().steps.values().any(|s| s.phase == super::super::undo::machine::Phase::Aborting)).await;
        if aborting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("nothing aborting");
}
