//! #24: a Move takes a torrent's files to a new destination, renamed on the same drive or copied
//! to another (here a copy is forced on the temp dir's one drive), and adds it back there with the
//! pieces librqbit had. Every destination is the test's own temp dir.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use super::super::actor::moving::Hold;
use super::letgo::{engine_counting, saved_pieces};
use super::*;

/// Holds the engine's moves: copies even on the same drive, and each copy waits at `gate` after
/// each chunk while it's held.
async fn holding(engine: &Engine, copies: bool, gate: Option<super::super::trash::Gate>) {
    engine.inspect(move |a| a.move_hold = Hold { gate, copies, ..Hold::default() }).await;
}

/// Waits until torrent `id` is done moving: back in librqbit, past sampling, or out of it for good.
pub(super) async fn moved(engine: &Engine, id: u64) {
    until(engine, id, |s| !matches!(s, Status::Moving { .. })).await;
    sampled(engine, id).await;
}

pub(super) fn move_to(dir: &Path) -> TorrentCommand {
    TorrentCommand::Move { to: AbsolutePath::new(dir).unwrap(), clash: None, name: None }
}

/// The bytes of each file under `folder`, by path inside it.
pub(super) fn bytes_in(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut dirs = vec![folder.to_owned()];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                _ => drop(found.insert(e.path().strip_prefix(folder).unwrap().to_owned(), std::fs::read(e.path()).unwrap())),
            }
        }
    }
    found
}

/// How many pieces each file of [`many_pieces`] has.
pub(super) const PER_FILE: usize = 20;

/// As `complete_torrent`, with [`PER_FILE`] pieces in each of its three files: librqbit's sampling
/// at a start reads one of each file and a few more (each next one less likely), so it reads far
/// fewer than all of them, where with two each it reads all six one time in six.
pub(super) async fn many_pieces(dir: &Path) -> AddSource {
    let pack = dir.join("downloads/pack");
    std::fs::create_dir_all(&pack).unwrap();
    for (i, name) in ["a.bin", "b.bin", "c.bin"].iter().enumerate() {
        let bytes: Vec<u8> = (0..PER_FILE * PIECE).map(|n| (n * 7 + i * 31 + n / PIECE) as u8).collect();
        std::fs::write(pack.join(name), bytes).unwrap();
    }
    let options = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec!["http://127.0.0.1:1/announce".into()],
        piece_length: Some(PIECE as u32),
    };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let created = librqbit::create_torrent(&pack, options, &spawner).await.unwrap();
    AddSource::TorrentFile(created.as_bytes().unwrap().to_vec())
}

/// Pinned (#24): a torrent moved keeps what it had without a full check, renamed or copied. Phantom
/// keeps librqbit's `<hash>.bitv` aside as it takes the torrent out, and puts it back before adding
/// it again: librqbit then samples a few pieces, as at a start. A librqbit that keeps its pieces
/// elsewhere, or names them otherwise, fails here: the torrent would be checked in full (every
/// piece read), or come back with nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_torrent_keeps_its_progress_without_a_full_check() {
    let pieces = 3 * PER_FILE;
    for copies in [false, true] {
        let dir = temp();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = engine_counting(dir.path(), reads.clone()).await;
        holding(&engine, copies, None).await;
        let id = add(&engine, &dir.path().join("downloads"), many_pieces(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let hash = find(&engine, id).await.info_hash;
        let before = bytes_in(&dir.path().join("downloads"));
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        reads.store(0, Ordering::SeqCst);

        engine.command(id, move_to(&elsewhere)).await.unwrap();
        moved(&engine, id).await;

        wait_for(&engine, id, Status::Seeding).await;
        let t = find(&engine, id).await;
        assert_eq!(t.content.unwrap().progress_bytes, (pieces * PIECE) as u64, "copies {copies}: everything it had");
        assert_eq!(t.location, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap()));
        assert_eq!(bytes_in(&elsewhere), before, "copies {copies}: the same bytes, in the new destination");
        assert!(!dir.path().join("downloads/pack").exists(), "copies {copies}: nothing left in the old one");
        assert_eq!(saved_pieces(dir.path(), &hash), pieces as u64, "copies {copies}: librqbit has every piece saved");
        let read = reads.load(Ordering::SeqCst);
        assert!(read < pieces / 2, "copies {copies}: sampled, not checked in full ({pieces} pieces): {read} reads");
        engine.stop().await;
    }
}

/// A Move into a destination where a file of the torrent's is already: refused with that file, and
/// nothing moved; sent again with a choice, it moves. Use existing keeps that file and checks the
/// torrent there (the person's bytes aren't its own: that file is had none of); Replace puts the
/// torrent's over it. The person's other files there stay either way.
#[tokio::test(flavor = "multi_thread")]
async fn a_clash_is_refused_with_its_files_and_each_choice_does_as_it_says() {
    for clash in [proto::Clash::UseExisting, proto::Clash::Replace] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let elsewhere = dir.path().join("elsewhere");
        let (theirs, other) = (elsewhere.join("pack/a.bin"), elsewhere.join("pack/mine.txt"));
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        std::fs::write(&theirs, vec![9; 2 * PIECE]).unwrap();
        std::fs::write(&other, b"mine").unwrap();
        let a_bin = std::fs::read(file(dir.path(), "a.bin")).unwrap();

        let refused = engine.command(id, move_to(&elsewhere)).await;
        let (who, TorrentRefusal::Clash { files: found }) = refusal(refused) else { panic!("a clash") };
        assert_eq!((who, proto::Found::paths(&found)), (id, vec![AbsolutePath::new(&theirs).unwrap()]));
        assert!(file(dir.path(), "a.bin").exists() && find(&engine, id).await.status == Status::Seeding, "nothing moved");

        let to = AbsolutePath::new(&elsewhere).unwrap();
        engine.command(id, TorrentCommand::Move { to, clash: Some(proto::Answered { choice: clash, found }), name: None }).await.unwrap();
        moved(&engine, id).await;
        until(&engine, id, |s| *s != Status::Checking).await;
        let a = index_of(&engine, id, "a.bin").await;
        let progress = engine.details(id).await.unwrap().files[a].progress_bytes;
        let trashed = bytes_in(&dir.path().join("trash")).into_values().collect::<Vec<_>>();
        match clash {
            proto::Clash::UseExisting => {
                assert_eq!(std::fs::read(&theirs).unwrap(), vec![9; 2 * PIECE], "theirs kept");
                assert_eq!(progress, 0, "checked there: theirs isn't the torrent's file");
                assert_eq!(trashed, vec![a_bin], "its own copy in the Trash, not erased");
            }
            proto::Clash::Replace => {
                assert_eq!((progress, find(&engine, id).await.status), (FULL, Status::Seeding), "the torrent's own");
                assert_eq!(trashed, vec![vec![9; 2 * PIECE]], "theirs in the Trash, not erased");
            }
        }
        assert_eq!(std::fs::read(&other).unwrap(), b"mine", "{clash:?}: the person's other file stays");
        assert!(!dir.path().join("downloads/pack").exists(), "{clash:?}: nothing left in the old destination");
        engine.stop().await;
    }
}

/// Blocking finding 1 of #264's review: a Move into the folder the torrent is in, spelled another
/// way (through a link, with `..`, in another case where the drive ignores case), is the same
/// folder: refused, whatever the clash choice, and nothing moves, nor is anything lost. Into a
/// folder inside its own, spelled through the link, likewise.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_into_its_own_folder_spelled_another_way_moves_nothing_and_says_so() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = bytes_in(&downloads);
    std::os::unix::fs::symlink(&downloads, dir.path().join("alias")).unwrap();
    std::fs::create_dir_all(dir.path().join("x")).unwrap();
    let mut spellings = vec![dir.path().join("alias"), dir.path().join("x/../downloads"), downloads.clone()];
    if dir.path().join("DOWNLOADS").exists() {
        spellings.push(dir.path().join("DOWNLOADS"));
    }
    for to in &spellings {
        for clash in [None, Some(proto::Clash::UseExisting), Some(proto::Clash::Replace)] {
            let clash = clash.map(|choice| proto::Answered { choice, found: Vec::new() });
            let to = AbsolutePath::new(to).unwrap();
            let refused = engine.command(id, TorrentCommand::Move { to: to.clone(), clash: clash.clone(), name: None }).await;
            assert_eq!(refusal(refused), (id, TorrentRefusal::AlreadyThere), "{} with {clash:?}", to.display());
        }
    }
    let inside = AbsolutePath::new(dir.path().join("alias/pack")).unwrap();
    let clash = Some(proto::Answered { choice: proto::Clash::Replace, found: Vec::new() });
    let refused = engine.command(id, TorrentCommand::Move { to: inside, clash, name: None }).await;
    assert_eq!(refusal(refused), (id, TorrentRefusal::IntoItself));
    assert_eq!(bytes_in(&downloads), before, "nothing moved, nothing lost");
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.content.unwrap().progress_bytes), (Status::Seeding, 3 * FULL));
    engine.stop().await;
}

/// The user's rule on #264: a Move goes ahead while files of the torrent are in the Trash for an
/// undo. As it starts, that Move to Trash leaves the stack (moved: its undo would put them back in
/// the old destination), and nothing else does; the files stay in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_drops_the_undo_of_a_move_to_trash_of_its_torrent_and_nothing_else() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    let other = add(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    engine.command(other, TorrentCommand::Remove).await.unwrap();
    let stack = super::seqs(&engine);
    let [trashed, removed] = [stack[0], stack[1]];
    assert_eq!(*find(&engine, id).await.actions.get(Action::Move), proto::Answer::Does, "Move to… is on");

    engine.command(id, move_to(&elsewhere)).await.unwrap();
    moved(&engine, id).await;
    assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap()));
    assert_eq!(super::seqs(&engine), [removed], "only the Move to Trash left");
    let left = super::left(&engine);
    assert!(left.contains(&proto::Left { seq: trashed, why: proto::Leave::Moved }), "{left:?}");
    assert_eq!(bytes_in(&dir.path().join("trash")).len(), 1, "its file, still in the Trash");
    engine.stop().await;
}

/// A move whose write-ahead can't be saved (a full internal disk) doesn't happen: refused before
/// anything moves, the torrent where it was, as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_cant_be_written_ahead_doesnt_happen() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = bytes_in(&dir.path().join("downloads"));
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    engine.inspect(|a| a.move_hold.saves_fail = true).await;
    let (_, why) = refusal(engine.command(id, move_to(&elsewhere)).await);
    assert!(matches!(why, TorrentRefusal::NotMoved { why: proto::NotMoved::Failed { .. }, .. }), "{why:?}");
    assert_eq!(bytes_in(&dir.path().join("downloads")), before);
    assert!(bytes_in(&elsewhere).is_empty());
    assert_eq!(find(&engine, id).await.status, Status::Seeding);
    engine.stop().await;
}

/// A crash just after a move is written ahead, before the torrent is taken out of librqbit:
/// the next start puts it back, nothing staged, the torrent where it was with what it had.
#[tokio::test(flavor = "multi_thread")]
async fn a_crash_after_a_move_is_written_ahead_is_put_back_at_the_start() {
    let dir = temp();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (id, before) = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let before = bytes_in(&dir.path().join("downloads"));
        let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = crashed.clone();
        engine.inspect(move |a| a.move_hold.crash_before_take_out = Some(hold)).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), engine.command(id, move_to(&elsewhere))).await;
        assert!(crashed.load(Ordering::SeqCst), "it crashed there");
        (id, before)
    };
    let engine = engine(dir.path()).await;
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(bytes_in(&dir.path().join("downloads")), before, "back where it was");
    assert!(bytes_in(&elsewhere).is_empty(), "nothing in the new destination, nothing staged");
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 3 * FULL);
    engine.stop().await;
}

/// Should-fix 1a of #264's seventh review: a crash once a move's old files are gone, before its
/// record says it's over. A file the person put at a new path while Phantom was down doesn't send
/// it back (the originals are gone): the record says every file had landed, so the next start
/// finishes it, the torrent in the new folder, the person's file taken there as replaced by hand.
#[tokio::test(flavor = "multi_thread")]
async fn a_crash_once_the_old_files_are_gone_finishes_the_move_at_the_start() {
    let dir = temp();
    let (downloads, elsewhere) = (dir.path().join("downloads"), dir.path().join("elsewhere"));
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (id, hash, before) = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let before = bytes_in(&downloads);
        let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = crashed.clone();
        engine.inspect(move |a| a.move_hold.crash_once_moved = Some(hold)).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), engine.command(id, move_to(&elsewhere))).await;
        assert!(crashed.load(Ordering::SeqCst), "it crashed there");
        (id, find(&engine, id).await.info_hash, before)
    };
    let mine = elsewhere.join("pack/a.bin");
    std::fs::remove_file(&mine).unwrap();
    std::fs::write(&mine, b"mine").unwrap();
    let engine = engine(dir.path()).await;
    moved(&engine, id).await;
    assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap()));
    assert!(!downloads.join("pack").exists(), "nothing back in the old folder");
    let mut want = bytes_in(&elsewhere);
    // Taken as the torrent's file there, replaced by hand: librqbit sizes it, nothing more offline.
    let a = want.remove(Path::new("pack/a.bin")).unwrap();
    assert!(a.starts_with(b"mine"), "the person's file, not one of the torrent's own");
    let mut had = before;
    had.remove(Path::new("pack/a.bin"));
    assert_eq!(want, had, "the rest moved, nothing staged");
    assert!(!elsewhere.join(format!(".phantom-moving-{hash}")).exists());
    engine.stop().await;
}

/// Should-fix 1b of #264's seventh review: a move whose files all took their places, but whose
/// record can't say so (the internal disk full by then), stops before any original goes and goes
/// back: every file where it was, nothing left in the new folder, the torrent as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_whose_landing_cant_be_written_ahead_goes_back_with_every_original() {
    for copies in [false, true] {
        let dir = temp();
        let (downloads, elsewhere) = (dir.path().join("downloads"), dir.path().join("elsewhere"));
        std::fs::create_dir_all(&elsewhere).unwrap();
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let before = bytes_in(&downloads);
        holding(&engine, copies, None).await;
        engine.inspect(|a| a.move_hold.landed_save_fails = true).await;
        let (_, why) = refusal(engine.command(id, move_to(&elsewhere)).await);
        assert!(matches!(why, TorrentRefusal::NotMoved { why: proto::NotMoved::Failed { .. }, .. }), "copies {copies}: {why:?}");
        moved(&engine, id).await;
        assert_eq!(bytes_in(&downloads), before, "copies {copies}: every original where it was");
        assert!(bytes_in(&elsewhere).is_empty(), "copies {copies}: nothing in the new folder");
        assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(downloads.join("pack")).unwrap()));
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
    }
}

/// Should-fix 1c of #264's seventh review: a mark that an earlier move's files had all landed, left
/// in the staging folder, never counts for this move: its files take their places as any move's,
/// and none is left staged with its original gone.
#[tokio::test(flavor = "multi_thread")]
async fn a_landed_mark_an_earlier_move_left_doesnt_count_for_this_one() {
    let dir = temp();
    let (downloads, elsewhere) = (dir.path().join("downloads"), dir.path().join("elsewhere"));
    let engine = engine(dir.path()).await;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = bytes_in(&downloads);
    let staging = elsewhere.join(format!(".phantom-moving-{}", find(&engine, id).await.info_hash));
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join(".phantom-moving-landed"), b"").unwrap();

    engine.command(id, move_to(&elsewhere)).await.unwrap();
    moved(&engine, id).await;
    assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap()));
    assert_eq!(bytes_in(&elsewhere.join("pack")), bytes_in_named(&before), "every file at its new path");
    assert!(!downloads.join("pack").exists());
    assert!(!bytes_in(&staging).keys().any(|p| p.extension().is_some_and(|e| e == "bin")), "nothing left staged");
    engine.stop().await;
}

/// ADR 0008: an engine restart for new settings waits for a move under way, which ends moved; a
/// shutdown puts one back, and its client hears it was interrupted.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_for_new_settings_waits_for_a_move_and_a_shutdown_puts_it_back() {
    for restart in [true, false] {
        let dir = temp();
        let gate = super::super::trash::Gate::default();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let before = bytes_in(&dir.path().join("downloads"));
        holding(&engine, true, Some(gate.clone())).await;
        super::super::trash::hold(&gate, true);
        let _opens = OpensTrash(gate.clone());
        let answer = tokio::spawn(engine.command_later(id, move_to(&elsewhere)));
        until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
        let engine = Arc::new(engine);
        let stopping = tokio::spawn({
            let engine = engine.clone();
            async move { if restart { drop(engine.stop_to_restart().await) } else { engine.stop().await } }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        super::super::trash::hold(&gate, false);
        stopping.await.unwrap();
        let answer = answer.await.unwrap();
        match restart {
            true => {
                answer.expect("moved: the restart waited for it");
                assert_eq!(bytes_in(&elsewhere.join("pack")), bytes_in_named(&before), "restart: moved");
                assert!(!dir.path().join("downloads/pack").exists());
            }
            false => {
                let (_, why) = refusal(answer);
                assert!(matches!(why, TorrentRefusal::NotMoved { why: proto::NotMoved::Interrupted, .. }), "{why:?}");
                assert_eq!(bytes_in(&dir.path().join("downloads")), before, "shutdown: back where it was");
                assert!(bytes_in(&elsewhere).is_empty());
            }
        }
    }
}

/// The user's rule on #264: a file of the person's put at a new path sends the move back, even with
/// an original deleted by hand meanwhile (here, during the copy, the original of the file being
/// copied). The person's file stays as it is. The deleted file stays deleted: its copy goes to the
/// Trash, not back into the old folder, and the torrent is back there Missing files, as after a
/// deletion with no move. Nothing downloads by itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_of_the_persons_at_a_new_path_sends_the_move_back_and_a_file_deleted_by_hand_stays_deleted() {
    let dir = temp();
    let (downloads, elsewhere) = (dir.path().join("downloads"), dir.path().join("elsewhere"));
    let engine = engine(dir.path()).await;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let gate = super::super::trash::Gate::default();
    std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
    holding(&engine, true, Some(gate.clone())).await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let answer = tokio::spawn(engine.command_later(id, move_to(&elsewhere)));
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    let hash = find(&engine, id).await.info_hash;
    let copying = std::fs::read_dir(elsewhere.join(format!(".phantom-moving-{hash}/pack"))).unwrap().next().unwrap().unwrap().file_name();
    let own = std::fs::read(downloads.join("pack").join(&copying)).unwrap();
    std::fs::remove_file(downloads.join("pack").join(&copying)).unwrap();
    let other = ["a.bin", "b.bin", "c.bin"].into_iter().find(|n| *n != copying).unwrap();
    let mine = elsewhere.join("pack").join(other);
    std::fs::write(&mine, b"mine").unwrap();
    super::super::trash::hold(&gate, false);

    let (_, why) = refusal(answer.await.unwrap());
    assert!(matches!(why, TorrentRefusal::NotMoved { why: proto::NotMoved::Taken { .. }, .. }), "{why:?}");
    moved(&engine, id).await;
    holding(&engine, false, None).await;
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.location), (Status::MissingFiles, Some(AbsolutePath::new(downloads.join("pack")).unwrap())));
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file, untouched");
    assert!(!downloads.join("pack").join(&copying).exists(), "deleted by hand, it stays deleted");
    let trashed: Vec<Vec<u8>> = bytes_in(&dir.path().join("trash")).into_values().collect();
    assert_eq!(trashed, [own], "its copy, in the Trash");
    let rest = ["a.bin", "b.bin", "c.bin"].into_iter().filter(|n| **n != *copying);
    assert!(rest.clone().all(|n| file(dir.path(), n).exists()), "the rest, back where they were");
    assert_eq!(bytes_in(&elsewhere), BTreeMap::from([(PathBuf::from("pack").join(other), b"mine".to_vec())]), "nothing else there");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "nothing downloads by itself");
    assert!(!downloads.join("pack").join(&copying).exists());
    engine.stop().await;
}

/// A nit of the fifth review on #264: a torrent naming a file or folder as Phantom's own in a
/// destination (`.phantom-moving-…`, where a move stages its files) is refused as it's added, so it
/// never shares one: by its own name (one file, or its folder) or by a path in it.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_naming_phantoms_own_folders_is_refused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let hash = "ab".repeat(20);
    let moving = format!(".phantom-moving-{hash}");
    let inside = format!("{moving}/0");
    // In any case: APFS folds it.
    let shouting = format!(".PHANTOM-Moving-{hash}");
    let sources = [&moving, &shouting].map(|name| torrent_file(name));
    for source in sources.into_iter().chain([super::multi_file_torrent(&["a.bin", &inside])]) {
        let refused = add(&engine, &downloads, source, true).await;
        assert!(
            matches!(&refused, Err(Failure::Error(proto::ServiceError::InvalidTorrentFile { detail })) if detail.contains("Phantom's own")),
            "{refused:?}"
        );
    }
    assert!(engine.listing().await.is_empty());
    let named = torrent_file(".phantom-other");
    assert!(add(&engine, &downloads, named, true).await.is_ok(), "any other name is the torrent's own");
    engine.stop().await;
}

/// The sixth review's should-fix 2 on #264: a magnet's info arrives only once it's added, so one
/// naming Phantom's own folders (here in capitals, which APFS folds) is refused then, before any
/// of its files is made: not even its first file, nor its folder. On librqbit's sessions
/// directly, with a local seeder, as the engine's tests run offline.
#[tokio::test(flavor = "multi_thread")]
async fn a_magnet_naming_phantoms_own_folders_is_refused_before_any_file_is_made() {
    use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerOptions, SessionOptions};
    let dir = temp();
    let AddSource::TorrentFile(bytes) = super::multi_file_torrent(&["a.bin", ".PHANTOM-MOVING-x/b.bin"]) else { unreachable!() };
    let hash = librqbit::torrent_from_bytes(&bytes).unwrap().info_hash.as_string();
    let quiet = |listen: bool| SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: listen.then(|| ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
        ..Default::default()
    };
    let seed = dir.path().join("seed");
    let seeder = librqbit::Session::new_with_opts(seed.clone(), quiet(true)).await.unwrap();
    let opts = AddTorrentOptions { overwrite: true, output_folder: Some(seed.to_str().unwrap().to_owned()), ..Default::default() };
    let Ok(AddTorrentResponse::Added(..)) = seeder.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await else { panic!("seeded") };
    let leech_session = dir.path().join("leecher-session");
    let leecher = librqbit::Session::new_with_opts(leech_session.clone(), quiet(false)).await.unwrap();
    let destination = dir.path().join("downloads");
    std::fs::create_dir_all(&destination).unwrap();

    let magnet = AddTorrent::from_url(format!("magnet:?xt=urn:btih:{hash}"));
    let at = AbsolutePath::new(destination.clone()).unwrap();
    let peers = vec![seeder.listen_addr().unwrap()];
    let guard = super::super::files::Guard::default();
    let added = tokio::time::timeout(
        Duration::from_secs(20),
        super::super::actor::add_from(leecher.clone(), 0, magnet, at, leech_session, guard, peers),
    )
    .await
    .expect("its info arrived");
    let refused = added.err().map(|e| format!("{e:#}"));
    assert!(refused.as_deref().is_some_and(|e| e.contains("takes the name of a folder of Phantom's own")), "{refused:?}");
    let made: Vec<_> = std::fs::read_dir(&destination).unwrap().flatten().map(|e| e.path()).collect();
    assert!(made.is_empty(), "nothing made in the destination: {made:?}");
    leecher.stop().await;
    seeder.stop().await;
}

/// The third review's should-fix 5 on #264: a file replaced by hand (another file at its path, not
/// the one librqbit has open) isn't moved with librqbit's pieces of the one it had: the moved
/// torrent is checked in full, every piece read, and has what the file there holds, none of the
/// replaced file's pieces.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_replaced_by_hand_is_checked_in_full_once_moved() {
    let pieces = 3 * PER_FILE;
    let dir = temp();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let engine = engine_counting(dir.path(), reads.clone()).await;
    let id = add(&engine, &dir.path().join("downloads"), many_pieces(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let a = dir.path().join("downloads/pack/a.bin");
    let anew = a.with_extension("new");
    std::fs::write(&anew, vec![1; PER_FILE * PIECE]).unwrap();
    std::fs::rename(&anew, &a).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    reads.store(0, Ordering::SeqCst);

    engine.command(id, move_to(&elsewhere)).await.unwrap();
    moved(&engine, id).await;

    let full = eventually(10, || reads.load(Ordering::SeqCst) >= pieces).await;
    assert!(full, "checked in full: {} reads of {pieces} pieces", reads.load(Ordering::SeqCst));
    until(&engine, id, |s| *s != Status::Checking).await;
    // The listing follows the check's end a poll later.
    let has = (2 * PER_FILE * PIECE) as u64;
    let mut progress = 0;
    for _ in 0..50 {
        progress = find(&engine, id).await.content.unwrap().progress_bytes;
        if progress == has {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(progress, has, "the two files it has, not the replaced one");
    assert_eq!(std::fs::read(elsewhere.join("pack/a.bin")).unwrap(), vec![1; PER_FILE * PIECE], "the file there, moved");
    engine.stop().await;
}

/// Should-fix 2 of #264's seventh review: a ⌘Z pressed while a Move waits its turn takes the
/// torrent's Move to Trash, whose undo waits behind the Move (lanes keep the order the steps came
/// in). At its turn the torrent has moved: the undo is refused, as moved to another folder since,
/// and its file stays in the Trash. So too into the folder that holds the old one, where the file's
/// old path is still inside the new destination.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_waiting_behind_a_move_of_its_torrent_is_refused_as_moved() {
    for into_parent in [false, true] {
        let dir = temp();
        let gate = super::super::trash::Gate::default();
        let engine = engine_with(dir.path(), Duration::from_millis(20), Default::default(), gate.clone()).await;
        let _opens = OpensTrash(gate.clone());
        let movies = dir.path().join("downloads/Movies");
        let id = add(&engine, &movies, complete_torrent_in(dir.path(), "downloads/Movies", 0).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let a_bin = std::fs::read(movies.join("pack/a.bin")).unwrap();
        let (a, b) = (index_of(&engine, id, "a.bin").await, index_of(&engine, id, "b.bin").await);
        engine.command(id, TorrentCommand::TrashFiles { files: vec![a] }).await.unwrap();
        let trashed = super::seqs(&engine)[0];
        engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
        // The undo of the second Move to Trash holds the lane, at the Trash's gate.
        super::super::trash::hold(&gate, true);
        let first_undo = tokio::spawn(engine.undo_later());
        held_at_the_gate(&gate).await;
        let to = if into_parent { dir.path().join("downloads") } else { dir.path().join("elsewhere") };
        std::fs::create_dir_all(&to).unwrap();
        let moving = engine.sent(id, move_to(&to)).await;
        on_its_lane(&engine, id, 2).await;
        let second_undo = tokio::spawn(engine.undo_later());
        on_its_lane(&engine, id, 3).await;
        super::super::trash::hold(&gate, false);

        first_undo.await.unwrap().expect("the second Move to Trash undone");
        moving.await.expect("moved");
        let refused = second_undo.await.unwrap();
        assert!(
            matches!(&refused, Err(Failure::Refused(proto::Refusal::Undo { why: proto::CantUndo::Moved, .. }))),
            "into the parent {into_parent}: {refused:?}"
        );
        let left = super::left(&engine);
        let why = proto::Leave::Refused { why: proto::CantUndo::Moved, stranded: Vec::new() };
        assert!(left.contains(&proto::Left { seq: trashed, why }), "into the parent {into_parent}: {left:?}");
        assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(to.join("pack")).unwrap()));
        assert!(!movies.join("pack/a.bin").exists(), "into the parent {into_parent}: not put back");
        assert!(!to.join("pack/a.bin").exists(), "into the parent {into_parent}: not moved");
        let in_trash: Vec<Vec<u8>> = bytes_in(&dir.path().join("trash")).into_values().collect();
        assert_eq!(in_trash, [a_bin], "into the parent {into_parent}: still in the Trash");
        engine.stop().await;
    }
}

/// Waits until `n` steps are on torrent `id`'s lane.
async fn on_its_lane(engine: &Engine, id: u64, n: usize) {
    for _ in 0..500 {
        if engine.inspect(move |a| a.steps.machine().lanes.get(&id).map_or(0, |l| l.len())).await == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never {n} steps on its lane");
}

/// Waits until a step newer than `seq` has arrived.
async fn until_step_after(engine: &Engine, seq: u64) {
    for _ in 0..500 {
        if engine.inspect(|a| a.steps.machine().last).await > seq {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no step after {seq}");
}

/// The files of `bytes`, taken out of their `pack/` folder.
fn bytes_in_named(bytes: &BTreeMap<PathBuf, Vec<u8>>) -> BTreeMap<PathBuf, Vec<u8>> {
    bytes.iter().map(|(p, b)| (p.strip_prefix("pack").unwrap_or(p).to_owned(), b.clone())).collect()
}

// Several torrents (#270): the Moves of a selection run one after another, in the order asked.

/// A complete torrent called `name` of two files, in `downloads/<name>`, no two alike.
async fn album(dir: &Path, name: &str, seed: usize) -> AddSource {
    let folder = dir.join("downloads").join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let bytes: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + seed * 31 + n / PIECE) as u8).collect();
    std::fs::write(folder.join("x.bin"), bytes.clone()).unwrap();
    std::fs::write(folder.join("y.bin"), bytes.iter().rev().copied().collect::<Vec<u8>>()).unwrap();
    let options = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec!["http://127.0.0.1:1/announce".into()],
        piece_length: Some(PIECE as u32),
    };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let created = librqbit::create_torrent(&folder, options, &spawner).await.unwrap();
    AddSource::TorrentFile(created.as_bytes().unwrap().to_vec())
}

/// Three albums, seeding, in downloads: their ids, in the order they were added.
async fn three(dir: &Path, engine: &Engine) -> [u64; 3] {
    let mut ids = [0; 3];
    for (i, name) in ["A", "B", "C"].into_iter().enumerate() {
        ids[i] = add(engine, &dir.join("downloads"), album(dir, name, i).await, false).await.unwrap().id;
        wait_for(engine, ids[i], Status::Seeding).await;
    }
    ids
}

/// Moves of `ids` into `to`, asked as one: answered with those that failed.
fn moves_into(engine: &Engine, to: &Path, ids: &[u64]) -> tokio::task::JoinHandle<Result<Vec<(u64, Failure)>, Failure>> {
    let moves = ids.iter().map(|id| proto::MoveOf { id: *id, clash: None, name: None }).collect();
    tokio::spawn(engine.moves_later(AbsolutePath::new(to).unwrap(), moves))
}

fn at(dir: &Path, folder: &str, name: &str) -> PathBuf {
    dir.join(folder).join(name).join("x.bin")
}

/// Moves of three torrents, asked in this order, each into a folder of its own so the test can
/// hold the one under way: each starts only once the one before it ended, the rest saying Waiting
/// to move, which Cancel move and nothing else is offered on.
#[tokio::test(flavor = "multi_thread")]
async fn moves_run_one_after_another_in_the_order_asked_the_rest_waiting_to_move() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, c] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    let [to_a, to_b, to_c] = ["to_a", "to_b", "to_c"].map(|n| dir.path().join(n));
    for to in [&to_a, &to_b, &to_c] {
        std::fs::create_dir_all(to).unwrap();
    }
    // The second's copy is held: the first goes through, the third can't start.
    super::super::trash::hold_under(&gate, &to_b);
    let mut answers = Vec::new();
    for (id, to) in [(a, &to_a), (b, &to_b), (c, &to_c)] {
        let before = engine.inspect(|a| a.steps.machine().last).await;
        answers.push(tokio::spawn(engine.command_later(id, move_to(to))));
        // Arrived: a step is numbered for it (a Move waiting its turn is on no lane).
        until_step_after(&engine, before).await;
    }

    until(&engine, b, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    assert!(at(dir.path(), "to_a", "A").exists() && !at(dir.path(), "downloads", "A").exists(), "the first moved");
    let (second, third) = (find(&engine, b).await, find(&engine, c).await);
    assert!(matches!(second.status, Status::Moving { .. }));
    assert_eq!(third.status, Status::WaitingToMove, "the third waits for the second");
    assert_eq!(third.actions.get(Action::Move), &proto::Answer::Refused { why: TorrentRefusal::WaitingToMove });
    assert_eq!(third.actions.get(Action::CancelMove), &proto::Answer::Does);
    assert_eq!(second.actions.get(Action::CancelMove), &proto::Answer::Refused { why: TorrentRefusal::NotWaitingToMove });
    assert!(at(dir.path(), "downloads", "C").exists() && bytes_in(&to_c).is_empty(), "nothing of the third moved yet");

    super::super::trash::hold(&gate, false);
    for answer in answers {
        answer.await.unwrap().unwrap();
    }
    for id in [a, b, c] {
        moved(&engine, id).await;
    }
    assert!(["to_a", "to_b", "to_c"].iter().zip(["A", "B", "C"]).all(|(to, n)| at(dir.path(), to, n).exists()));
    assert!(!at(dir.path(), "downloads", "C").exists(), "the last moved too");
    engine.stop().await;
}

/// Cancel move takes a waiting Move out of the queue: its torrent stays where it is, its Move is
/// answered cancelled, the others go on; one that runs is no one's to cancel.
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_waiting_move_takes_it_out_of_the_queue_and_the_others_go_on() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, c] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&engine, &to, &[a, b, c]);
    until(&engine, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&engine, c, |s| *s == Status::WaitingToMove).await;

    let (id, why) = refusal(engine.command(a, TorrentCommand::CancelMove).await);
    assert_eq!((id, why), (a, TorrentRefusal::NotWaitingToMove), "it runs");
    engine.command(b, TorrentCommand::CancelMove).await.unwrap();
    let cancelled = find(&engine, b).await;
    assert_eq!(cancelled.status, Status::Seeding, "back to what it was doing");
    assert_eq!(cancelled.actions.get(Action::Move), &proto::Answer::Does);

    super::super::trash::hold(&gate, false);
    let failed = answers.await.unwrap().unwrap();
    let [(who, Failure::Refused(proto::Refusal::Torrent { why, .. }))] = &failed[..] else { panic!("only B failed: {failed:?}") };
    assert_eq!(*who, b);
    assert!(matches!(why, TorrentRefusal::NotMoved { why: proto::NotMoved::Cancelled, .. }), "{why:?}");
    for id in [a, c] {
        moved(&engine, id).await;
    }
    assert!(at(dir.path(), "elsewhere", "A").exists() && at(dir.path(), "elsewhere", "C").exists());
    assert!(at(dir.path(), "downloads", "B").exists() && !at(dir.path(), "elsewhere", "B").exists(), "B stayed");
    engine.stop().await;
}

/// A Remove of a waiting torrent takes its Move out of the queue as it comes: it doesn't wait for
/// the moves before it.
#[tokio::test(flavor = "multi_thread")]
async fn a_remove_of_a_waiting_torrent_doesnt_wait_for_the_moves_before_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, _] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&engine, &to, &[a, b]);
    until(&engine, b, |s| *s == Status::WaitingToMove).await;
    engine.command(b, TorrentCommand::Remove).await.unwrap();
    assert!(engine.listing().await.iter().all(|t| t.id != b), "removed while the first still moves");
    super::super::trash::hold(&gate, false);
    let failed = answers.await.unwrap().unwrap();
    assert_eq!(failed.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [b], "its Move was cancelled; the first moved");
    engine.stop().await;
}

/// ADR 0008: a shutdown puts back the move under way, and those not started don't happen: every
/// file is where it was, and each client hears why.
#[tokio::test(flavor = "multi_thread")]
async fn a_shutdown_puts_back_the_move_under_way_and_the_waiting_ones_never_start() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, c] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&engine, &to, &[a, b, c]);
    until(&engine, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&engine, c, |s| *s == Status::WaitingToMove).await;
    let engine = Arc::new(engine);
    let stopping = tokio::spawn({
        let engine = engine.clone();
        async move { engine.stop().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    super::super::trash::hold(&gate, false);
    stopping.await.unwrap();

    let failed = answers.await.unwrap().unwrap();
    assert_eq!(failed.len(), 3, "every client hears why: {failed:?}");
    for (id, failure) in &failed {
        match (*id == a, failure) {
            (
                true,
                Failure::Refused(proto::Refusal::Torrent {
                    why: TorrentRefusal::NotMoved { why: proto::NotMoved::Interrupted, .. }, ..
                }),
            ) => {}
            (false, Failure::Refused(proto::Refusal::Stopping)) => {}
            other => panic!("torrent {id}: {other:?}"),
        }
    }
    assert!(["A", "B", "C"].iter().all(|n| at(dir.path(), "downloads", n).exists()), "every file where it was");
    assert!(bytes_in(&to).is_empty(), "nothing left in the new folder");
}

/// One whose Move is refused at its turn (it's in that folder already) doesn't hold the queue: the
/// others move, and its client hears why.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_fails_at_its_turn_lets_the_rest_go_on() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, _, c] = three(dir.path(), &engine).await;
    let there = dir.path().join("there");
    std::fs::create_dir_all(&there).unwrap();
    let source = album(dir.path(), "D", 7).await;
    std::fs::rename(dir.path().join("downloads/D"), there.join("D")).unwrap();
    let b = add(&engine, &there, source, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    let failed = moves_into(&engine, &there, &[a, b, c]).await.unwrap().unwrap();
    let [(who, Failure::Refused(proto::Refusal::Torrent { why, .. }))] = &failed[..] else { panic!("only B failed: {failed:?}") };
    assert_eq!((*who, why), (b, &TorrentRefusal::AlreadyThere));
    for id in [a, c] {
        moved(&engine, id).await;
    }
    assert!(at(dir.path(), "there", "A").exists() && at(dir.path(), "there", "C").exists());
    engine.stop().await;
}

/// A clash is decided for each torrent before anything moves (`Request::Clashes`): those that
/// clash, in the order asked, each with its files; each Move then takes its own choice, and a
/// torrent whose clash isn't answered is refused at its turn without holding the others.
#[tokio::test(flavor = "multi_thread")]
async fn a_clash_is_found_for_each_torrent_and_each_move_takes_its_own_choice() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, c] = three(dir.path(), &engine).await;
    let elsewhere = dir.path().join("elsewhere");
    let (theirs_a, theirs_c) = (at(dir.path(), "elsewhere", "A"), at(dir.path(), "elsewhere", "C"));
    for theirs in [&theirs_a, &theirs_c] {
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        std::fs::write(theirs, vec![9; 2 * PIECE]).unwrap();
    }
    let to = AbsolutePath::new(&elsewhere).unwrap();
    let found = engine.clashes(vec![a, b, c], to.clone()).await.unwrap();
    let paths = |found: &[proto::Clashing]| found.iter().map(|c| (c.id, proto::Found::paths(&c.files))).collect::<Vec<_>>();
    let files = |p: &PathBuf| vec![AbsolutePath::new(p).unwrap()];
    assert_eq!(paths(&found), [(a, files(&theirs_a)), (c, files(&theirs_c))], "A and C, not B");
    assert!([a, b, c].iter().all(|id| at(dir.path(), "downloads", ["A", "B", "C"][*id as usize - a as usize]).exists()), "nothing moved");

    let moves = vec![
        proto::MoveOf { id: a, clash: Some(proto::Answered { choice: proto::Clash::Replace, found: found[0].files.clone() }), name: None },
        proto::MoveOf { id: b, clash: None, name: None },
        proto::MoveOf { id: c, clash: None, name: None },
    ];
    let failed = engine.moves(to.clone(), moves).await.unwrap();
    let [(who, Failure::Refused(proto::Refusal::Torrent { why, .. }))] = &failed[..] else { panic!("only C failed: {failed:?}") };
    assert_eq!((*who, why), (c, &TorrentRefusal::Clash { files: found[1].files.clone() }), "unanswered, refused at its turn");
    for id in [a, b] {
        moved(&engine, id).await;
    }
    assert_ne!(std::fs::read(&theirs_a).unwrap(), vec![9; 2 * PIECE], "A's own over theirs");
    assert_eq!(std::fs::read(&theirs_c).unwrap(), vec![9; 2 * PIECE], "C's untouched");
    assert!(at(dir.path(), "downloads", "C").exists(), "C stayed");
    engine.stop().await;
}

// A clash answer holds for what the person was shown (#270's review, blocking 1): what's in the new
// folder when a Move's turn comes is compared with what `Clashes` found when the person was asked.

/// The Moves of `ids` into `to`, sent as one, each answered `choice` for what `Clashes` finds now:
/// what a client sends once its dialogs are answered.
async fn answered_moves(
    engine: &Engine,
    to: &Path,
    ids: &[u64],
    choice: proto::Clash,
) -> tokio::task::JoinHandle<Result<Vec<(u64, Failure)>, Failure>> {
    let to = AbsolutePath::new(to).unwrap();
    let found = engine.clashes(ids.to_vec(), to.clone()).await.unwrap();
    let moves = ids
        .iter()
        .map(|id| proto::MoveOf {
            id: *id,
            clash: found.iter().find(|c| c.id == *id).map(|c| proto::Answered { choice, found: c.files.clone() }),
            name: None,
        })
        .collect();
    tokio::spawn(engine.moves_later(to, moves))
}

/// Torrent `id`'s Move failed for `why`, and every other torrent's moved.
fn only_failed(failed: &[(u64, Failure)], id: u64) -> TorrentRefusal {
    let [(who, Failure::Refused(proto::Refusal::Torrent { why, .. }))] = failed else { panic!("only {id} failed: {failed:?}") };
    assert_eq!(*who, id);
    why.clone()
}

/// The reviewer's first probe: B clashes on `B/x.bin` and is answered Replace while A's copy runs;
/// meanwhile a file of the person's appears at `B/y.bin`. At B's turn it's refused with that path,
/// and nothing of the person's goes to the Trash: neither the new file nor the one Replace was
/// chosen for. B stays where it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_put_in_the_new_folder_while_a_move_waits_refuses_its_answered_clash() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, _] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    let (theirs, appears) = (at(dir.path(), "elsewhere", "B"), dir.path().join("elsewhere/B/y.bin"));
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"theirs").unwrap();
    let answers = answered_moves(&engine, &to, &[a, b], proto::Clash::Replace).await;
    until(&engine, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&engine, b, |s| *s == Status::WaitingToMove).await;
    std::fs::write(&appears, b"new").unwrap();
    super::super::trash::hold(&gate, false);

    let failed = answers.await.unwrap().unwrap();
    assert_eq!(only_failed(&failed, b), TorrentRefusal::ClashChanged { path: AbsolutePath::new(&appears).unwrap() });
    moved(&engine, a).await;
    assert_eq!((std::fs::read(&theirs).unwrap(), std::fs::read(&appears).unwrap()), (b"theirs".to_vec(), b"new".to_vec()), "both stay");
    assert!(bytes_in(&dir.path().join("trash")).values().all(|b| b != b"theirs" && b != b"new"), "nothing of theirs in the Trash");
    assert!(at(dir.path(), "downloads", "B").exists(), "B stayed");
    assert_eq!(find(&engine, b).await.status, Status::Seeding);
    engine.stop().await;
}

/// Two torrents whose files take the same paths (two `A/`), both clashing with an old `A/x.bin`
/// and both answered with the same choice ("Apply to the remaining"). The first moves in; at the
/// second's turn what's there is the first's, not what the person was asked about, so it's
/// refused: Replace never sends the first's freshly moved files to the Trash, and Use existing
/// never takes them as the second's to check and write into. The first seeds its own bytes.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_never_replaces_or_takes_the_files_an_earlier_move_put_at_its_paths() {
    for choice in [proto::Clash::Replace, proto::Clash::UseExisting] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let first = album(dir.path(), "A", 1).await;
        std::fs::rename(dir.path().join("downloads/A"), other.join("A")).unwrap();
        let first = add(&engine, &other, first, false).await.unwrap().id;
        let second = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 2).await, false).await.unwrap().id;
        for id in [first, second] {
            wait_for(&engine, id, Status::Seeding).await;
        }
        let ours = bytes_in(&other.join("A"));
        let to = dir.path().join("elsewhere");
        let old = at(dir.path(), "elsewhere", "A");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, vec![9; 2 * PIECE]).unwrap();

        let failed = answered_moves(&engine, &to, &[first, second], choice).await.await.unwrap().unwrap();
        let why = only_failed(&failed, second);
        let paths = [old.clone(), to.join("A/y.bin")].map(|p| AbsolutePath::new(p).unwrap());
        assert!(matches!(&why, TorrentRefusal::AnothersFiles { path } if paths.contains(path)), "{choice:?}: {why:?}");
        moved(&engine, first).await;
        until(&engine, first, |s| *s != Status::Checking).await;
        let here = bytes_in(&to.join("A"));
        match choice {
            proto::Clash::Replace => assert_eq!(here, ours, "the first's own, untouched"),
            proto::Clash::UseExisting => {
                assert_eq!(here.get(Path::new("y.bin")), ours.get(Path::new("y.bin")), "the first's own, untouched");
                assert_eq!(here.get(Path::new("x.bin")), Some(&vec![9; 2 * PIECE]), "the old one it kept, not written by the second");
            }
        }
        assert_eq!(bytes_in(&dir.path().join("downloads/A")).len(), 2, "{choice:?}: the second stayed");
        assert_eq!(find(&engine, second).await.status, Status::Seeding, "{choice:?}");
        engine.stop().await;
    }
}

/// The re-review's case (#270, blocking 1): two `A/` torrents, and an old `elsewhere/A` holding an
/// exact copy of every file of the first. The first is answered Use existing and takes them as its
/// own, unchanged (the same inode, length and time as when the second was asked about them). At
/// the second's turn they're the first's files, so its Move is refused, whether it was answered
/// Replace or Use existing: nothing of the first's goes to the Trash, nor is taken as the second's
/// to write into. The first seeds its own bytes there and the second stays where it was; asked
/// again, its clash names the first, and its old answer is still refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_never_replaces_or_takes_the_files_an_earlier_move_took_as_they_were() {
    for choice in [proto::Clash::Replace, proto::Clash::UseExisting] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let first = album(dir.path(), "A", 1).await;
        std::fs::rename(dir.path().join("downloads/A"), other.join("A")).unwrap();
        let first = add(&engine, &other, first, false).await.unwrap().id;
        let second = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 2).await, false).await.unwrap().id;
        for id in [first, second] {
            wait_for(&engine, id, Status::Seeding).await;
        }
        let (ours, theirs) = (bytes_in(&other.join("A")), bytes_in(&dir.path().join("downloads/A")));
        let to = dir.path().join("elsewhere");
        std::fs::create_dir_all(to.join("A")).unwrap();
        for (path, bytes) in &ours {
            std::fs::write(to.join("A").join(path), bytes).unwrap();
        }
        let to_abs = AbsolutePath::new(&to).unwrap();
        let found = engine.clashes(vec![first, second], to_abs.clone()).await.unwrap();
        let answer = |i: usize, choice| Some(proto::Answered { choice, found: found[i].files.clone() });
        let moves = vec![
            proto::MoveOf { id: first, clash: answer(0, proto::Clash::UseExisting), name: None },
            proto::MoveOf { id: second, clash: answer(1, choice), name: None },
        ];

        let failed = engine.moves(to_abs.clone(), moves).await.unwrap();
        let why = only_failed(&failed, second);
        let paths = ["x.bin", "y.bin"].map(|f| AbsolutePath::new(to.join("A").join(f)).unwrap());
        assert!(matches!(&why, TorrentRefusal::AnothersFiles { path } if paths.contains(path)), "{choice:?}: {why:?}");
        moved(&engine, first).await;
        until(&engine, first, |s| *s != Status::Checking).await;
        assert_eq!(bytes_in(&to.join("A")), ours, "{choice:?}: the first's own, untouched");
        assert_eq!(find(&engine, first).await.status, Status::Seeding, "{choice:?}");
        assert_eq!(bytes_in(&dir.path().join("downloads/A")), theirs, "{choice:?}: the second stayed");
        assert_eq!(find(&engine, second).await.status, Status::Seeding, "{choice:?}");
        assert!(
            bytes_in(&dir.path().join("trash")).values().all(|b| !theirs.values().any(|t| t == b)),
            "{choice:?}: nothing of the second's in the Trash"
        );

        let asked = engine.clashes(vec![second], to_abs.clone()).await.unwrap();
        assert_eq!(asked.iter().map(|c| (c.id, c.other)).collect::<Vec<_>>(), [(second, Some(first))], "{choice:?}: the first's files");
        let again = engine.moves(to_abs, vec![proto::MoveOf { id: second, clash: answer(1, choice), name: None }]).await.unwrap();
        assert!(matches!(only_failed(&again, second), TorrentRefusal::AnothersFiles { .. }), "{choice:?}");
        engine.stop().await;
    }
}

/// The third review's case (#270, should fix 2): the first torrent's files, in `other/A`, were
/// deleted by hand, so nothing is at its paths. The second, which has the same paths, is moved
/// into `other` all the same: refused, as turning the first's files back on or checking it again
/// would write into the second's. The second stays where it was, seeding.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_never_lands_at_the_paths_of_another_torrent_whose_files_are_gone() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let first = album(dir.path(), "A", 1).await;
    std::fs::rename(dir.path().join("downloads/A"), other.join("A")).unwrap();
    let first = add(&engine, &other, first, false).await.unwrap().id;
    let second = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 2).await, false).await.unwrap().id;
    for id in [first, second] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    let theirs = bytes_in(&dir.path().join("downloads/A"));
    for file in ["x.bin", "y.bin"] {
        std::fs::remove_file(other.join("A").join(file)).unwrap();
    }

    let to = AbsolutePath::new(&other).unwrap();
    let failed = engine.moves(to, vec![proto::MoveOf { id: second, clash: None, name: None }]).await.unwrap();
    let why = only_failed(&failed, second);
    let paths = ["x.bin", "y.bin"].map(|f| AbsolutePath::new(other.join("A").join(f)).unwrap());
    assert!(matches!(&why, TorrentRefusal::AnothersFiles { path } if paths.contains(path)), "{why:?}");
    assert_eq!(bytes_in(&dir.path().join("downloads/A")), theirs, "the second stayed");
    assert_eq!(bytes_in(&other.join("A")).len(), 0, "nothing was put at the first's paths");
    assert_eq!(find(&engine, second).await.status, Status::Seeding);
    engine.stop().await;
}

/// The fourth review's case (#270, should fix 1): `Z`'s folder, `elsewhere/Z`, was deleted by hand,
/// so it isn't on disk. A plain Move of `z` into `elsewhere` is refused all the same: its paths are
/// `Z`'s on a drive that ignores case, however the folder is spelled.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_never_lands_at_the_paths_of_another_torrent_whose_folder_is_gone_in_another_case() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let z = album(dir.path(), "Z", 5).await;
    std::fs::rename(dir.path().join("downloads/Z"), elsewhere.join("Z")).unwrap();
    let z = add(&engine, &elsewhere, z, false).await.unwrap().id;
    let lower = add(&engine, &dir.path().join("downloads"), album(dir.path(), "z", 2).await, false).await.unwrap().id;
    for id in [z, lower] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    engine.command(z, TorrentCommand::Pause).await.unwrap();
    std::fs::remove_dir_all(elsewhere.join("Z")).unwrap();

    let to = AbsolutePath::new(&elsewhere).unwrap();
    let failed = engine.moves(to, vec![proto::MoveOf { id: lower, clash: None, name: None }]).await.unwrap();
    let why = only_failed(&failed, lower);
    assert!(matches!(why, TorrentRefusal::AnothersFiles { .. }), "{why:?}");
    assert!(!elsewhere.join("z").exists(), "nothing was put there");
    assert_eq!(find(&engine, lower).await.status, Status::Seeding);
    engine.stop().await;
}

/// The re-review's probe (#270, 2): B is answered Use existing for `B/x.bin` in the new folder,
/// and while its other file copies, the person swaps that file for another. The Move doesn't take
/// the new one as B's: it goes back as a failed Move does, B where it was with its own files, and
/// nothing goes to the Trash, neither B's own `x.bin` nor the new file, which stays as it is.
#[tokio::test(flavor = "multi_thread")]
async fn a_kept_file_swapped_while_the_move_copies_sends_the_move_back() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [_, b, _] = three(dir.path(), &engine).await;
    let ours = bytes_in(&dir.path().join("downloads/B"));
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    let kept = at(dir.path(), "elsewhere", "B");
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, b"theirs").unwrap();
    let answers = answered_moves(&engine, &to, &[b], proto::Clash::UseExisting).await;
    until(&engine, b, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    let swapped = dir.path().join("swapped");
    std::fs::write(&swapped, b"another").unwrap();
    std::fs::rename(&swapped, &kept).unwrap();
    super::super::trash::hold(&gate, false);

    let failed = answers.await.unwrap().unwrap();
    let why = proto::NotMoved::Taken { path: AbsolutePath::new(&kept).unwrap() };
    assert!(matches!(only_failed(&failed, b), TorrentRefusal::NotMoved { why: w, .. } if w == why), "{failed:?}");
    moved(&engine, b).await;
    assert_eq!(std::fs::read(&kept).unwrap(), b"another", "the new file, untouched");
    assert_eq!(bytes_in(&dir.path().join("downloads/B")), ours, "B back with its own files");
    assert!(bytes_in(&dir.path().join("trash")).is_empty(), "nothing in the Trash");
    assert_eq!(find(&engine, b).await.status, Status::Seeding);
    engine.stop().await;
}

/// The reviewer's probe for #270's review, 3: a Move waiting its turn holds no lane. B waits behind
/// A's copy; a Pause of B is carried out at once, not after A, and B still says Waiting to move.
/// Once A is over, B's Move runs, on the torrent as the Pause left it: paused where it's moved.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_for_a_torrent_waiting_to_move_is_carried_out_at_once() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, _] = three(dir.path(), &engine).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&engine, &to, &[a, b]);
    until(&engine, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&engine, b, |s| *s == Status::WaitingToMove).await;

    let paused = tokio::time::timeout(Duration::from_secs(10), engine.command(b, TorrentCommand::Pause)).await;
    assert!(matches!(paused, Ok(Ok(()))), "answered while A still copies: {paused:?}");
    assert!(engine.inspect(move |actor| actor.steps.machine().lanes.get(&b).is_none()).await, "nothing left on B's lane");
    let waiting = find(&engine, b).await;
    assert_eq!(waiting.status, Status::WaitingToMove);
    assert_eq!(waiting.actions.get(Action::Resume), &proto::Answer::Does, "paused already");

    super::super::trash::hold(&gate, false);
    assert!(answers.await.unwrap().unwrap().is_empty(), "both moved");
    moved(&engine, b).await;
    assert_eq!(find(&engine, b).await.actions.get(Action::Resume), &proto::Answer::Does, "moved as the Pause left it: stopped");
    assert!(at(dir.path(), "elsewhere", "B").exists());
    engine.stop().await;
}

/// #270's review, 2: an engine restart for new settings waits only for the Move under way (ADR
/// 0008). A moves, B and C wait their turn: the restart ends once A has moved, B and C still where
/// they were; a Cancel move of C meanwhile is answered at once, by the engine stopping. The next
/// engine runs B in its turn, and the Moves' client hears once it's moved, of C only, cancelled.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_for_new_settings_waits_only_for_the_move_under_way_and_the_next_engine_runs_the_rest() {
    let dir = temp();
    let old = engine(dir.path()).await;
    let [a, b, c] = three(dir.path(), &old).await;
    let gate = super::super::trash::Gate::default();
    holding(&old, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&old, &to, &[a, b, c]);
    until(&old, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&old, c, |s| *s == Status::WaitingToMove).await;
    let old = Arc::new(old);
    let stopping = tokio::spawn({
        let old = old.clone();
        async move { old.stop_to_restart().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let cancelled = tokio::time::timeout(Duration::from_secs(10), old.command(c, TorrentCommand::CancelMove)).await;
    assert!(matches!(cancelled, Ok(Ok(()))), "Cancel move while it restarts: {cancelled:?}");
    super::super::trash::hold(&gate, false);
    let carried = stopping.await.unwrap();
    assert!(at(dir.path(), "elsewhere", "A").exists(), "the one under way moved");
    assert!(at(dir.path(), "downloads", "B").exists() && !to.join("B").exists(), "B didn't start");
    assert!(!answers.is_finished(), "the Moves' client still waits, for B");
    drop(old);

    let next = engine(dir.path()).await;
    next.resume(carried).await;
    let failed = tokio::time::timeout(Duration::from_secs(30), answers).await.expect("answered once B moved").unwrap().unwrap();
    assert!(matches!(only_failed(&failed, c), TorrentRefusal::NotMoved { why: proto::NotMoved::Cancelled, .. }));
    moved(&next, b).await;
    assert!(at(dir.path(), "elsewhere", "B").exists() && !at(dir.path(), "downloads", "B").exists(), "B moved in the next engine");
    assert!(at(dir.path(), "downloads", "C").exists() && !to.join("C").exists(), "C stayed");
    next.stop().await;
}

/// #270's review, mutation 3: a Move is judged again on its torrent as it is at its turn. B's
/// destination goes away while B waits behind A's copy: at its turn B's Move is refused as its
/// destination isn't there, and nothing of it moves; A's goes on.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_whose_destination_went_while_it_waited_is_refused_at_its_turn() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, _, _] = three(dir.path(), &engine).await;
    let (own, away) = (dir.path().join("own"), dir.path().join("own.away"));
    std::fs::create_dir_all(&own).unwrap();
    let source = album(dir.path(), "D", 7).await;
    std::fs::rename(dir.path().join("downloads/D"), own.join("D")).unwrap();
    let b = add(&engine, &own, source, false).await.unwrap().id;
    wait_for(&engine, b, Status::Seeding).await;
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold(&gate, true);
    let to = dir.path().join("elsewhere");
    std::fs::create_dir_all(&to).unwrap();
    let answers = moves_into(&engine, &to, &[a, b]);
    until(&engine, a, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    until(&engine, b, |s| *s == Status::WaitingToMove).await;
    std::fs::rename(&own, &away).unwrap();
    let gone =
        move |a: &mut super::super::actor::Actor| matches!(a.torrents[&b].lifecycle.phase, lifecycle::Phase::DestinationUnavailable { .. });
    for _ in 0..500 {
        if engine.inspect(move |a| gone(a)).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(engine.inspect(move |a| gone(a)).await, "its destination's gone, seen while it waits");
    super::super::trash::hold(&gate, false);

    let failed = tokio::time::timeout(Duration::from_secs(30), answers).await.expect("both answered").unwrap().unwrap();
    assert!(matches!(only_failed(&failed, b), TorrentRefusal::DestinationUnavailable { action: Action::Move, .. }));
    moved(&engine, a).await;
    assert!(!to.join("D").exists(), "nothing of B moved");
    assert_eq!(bytes_in(&away.join("D")).len(), 2, "B's files where they were");
    engine.stop().await;
}

// A clash's Rename… (#270, after #331): a Move under a new name lands at `to/name`, its own folder
// named so, checked at its turn as a Rename is.

/// Torrent `id`'s Move into `to` under `name`.
fn move_named(to: &Path, name: &str) -> TorrentCommand {
    let name = Some(proto::FileName::new(name).unwrap());
    TorrentCommand::Move { to: AbsolutePath::new(to).unwrap(), clash: None, name }
}

/// A's Move is held while B's waits behind it, under a new name: at B's turn B lands at
/// `elsewhere/B 2`, its files in it, and is listed by that name, the info's as its original one.
/// Nothing is left at its old place, nor made at its old name in the new folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_move_under_a_new_name_lands_at_that_name() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, _] = three(dir.path(), &engine).await;
    let ours = bytes_in(&dir.path().join("downloads/B"));
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    let (to_a, elsewhere) = (dir.path().join("to_a"), dir.path().join("elsewhere"));
    for to in [&to_a, &elsewhere] {
        std::fs::create_dir_all(to).unwrap();
    }
    super::super::trash::hold_under(&gate, &to_a);
    let first = tokio::spawn(engine.command_later(a, move_to(&to_a)));
    until(&engine, a, |s| matches!(s, Status::Moving { .. })).await;
    let second = tokio::spawn(engine.command_later(b, move_named(&elsewhere, "B 2")));
    until(&engine, b, |s| *s == Status::WaitingToMove).await;

    super::super::trash::hold(&gate, false);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    moved(&engine, b).await;
    assert_eq!(bytes_in(&elsewhere.join("B 2")), ours, "its own files, under the new name");
    assert!(!elsewhere.join("B").exists() && !dir.path().join("downloads/B").exists());
    let t = find(&engine, b).await;
    assert_eq!((t.name.as_deref(), t.original_name.as_deref()), (Some("B 2"), Some("B")));
    assert_eq!(t.status, Status::Seeding);
    engine.stop().await;
}

/// The new name is checked at the Move's turn, as a Rename's is: a folder put at `elsewhere/B 2`
/// while B waits refuses B's Move with that path, and nothing is merged into it; B stays where it
/// was, seeding.
#[tokio::test(flavor = "multi_thread")]
async fn something_put_at_the_new_name_while_the_move_waits_refuses_it_at_its_turn() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let [a, b, _] = three(dir.path(), &engine).await;
    let ours = bytes_in(&dir.path().join("downloads/B"));
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    let (to_a, elsewhere) = (dir.path().join("to_a"), dir.path().join("elsewhere"));
    for to in [&to_a, &elsewhere] {
        std::fs::create_dir_all(to).unwrap();
    }
    super::super::trash::hold_under(&gate, &to_a);
    let first = tokio::spawn(engine.command_later(a, move_to(&to_a)));
    until(&engine, a, |s| matches!(s, Status::Moving { .. })).await;
    let second = tokio::spawn(engine.command_later(b, move_named(&elsewhere, "B 2")));
    until(&engine, b, |s| *s == Status::WaitingToMove).await;
    std::fs::create_dir_all(elsewhere.join("B 2")).unwrap();
    std::fs::write(elsewhere.join("B 2/theirs.txt"), b"theirs").unwrap();

    super::super::trash::hold(&gate, false);
    first.await.unwrap().unwrap();
    let (who, why) = refusal(second.await.unwrap());
    assert_eq!((who, why), (b, TorrentRefusal::NameTaken { path: AbsolutePath::new(elsewhere.join("B 2")).unwrap() }));
    assert_eq!(bytes_in(&elsewhere.join("B 2")), BTreeMap::from([(PathBuf::from("theirs.txt"), b"theirs".to_vec())]), "nothing merged");
    assert_eq!(bytes_in(&dir.path().join("downloads/B")), ours, "B stayed");
    assert_eq!(find(&engine, b).await.status, Status::Seeding);
    engine.stop().await;
}

/// The fourth review's probe (#270): C's Move under the name `Q` into `elsewhere` is copying, so
/// its new paths, `elsewhere/Q/…`, are C's. D, already in `elsewhere`, is renamed `Q` meanwhile: it
/// is refused with C's path, nothing is made there for D.
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_never_takes_the_paths_another_torrents_move_is_copying_to() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let d = album(dir.path(), "D", 4).await;
    std::fs::rename(dir.path().join("downloads/D"), elsewhere.join("D")).unwrap();
    let d = add(&engine, &elsewhere, d, false).await.unwrap().id;
    let c = add(&engine, &dir.path().join("downloads"), album(dir.path(), "C", 2).await, false).await.unwrap().id;
    for id in [c, d] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    let ours = bytes_in(&elsewhere.join("D"));
    let gate = super::super::trash::Gate::default();
    holding(&engine, true, Some(gate.clone())).await;
    let _opens = OpensTrash(gate.clone());
    super::super::trash::hold_under(&gate, &elsewhere);
    let copying = tokio::spawn(engine.command_later(c, move_named(&elsewhere, "Q")));
    until(&engine, c, |s| matches!(s, Status::Moving { .. })).await;

    let rename = TorrentCommand::Rename { name: proto::FileName::new("Q").unwrap() };
    let why = refusal(engine.command(d, rename).await).1;
    let path = AbsolutePath::new(elsewhere.join("Q/x.bin")).unwrap();
    assert!(matches!(&why, TorrentRefusal::AnothersFiles { path: p } if *p == path), "{why:?}");
    assert_eq!(bytes_in(&elsewhere.join("D")), ours, "D stayed");

    super::super::trash::hold(&gate, false);
    copying.await.unwrap().unwrap();
    assert_eq!(find(&engine, d).await.status, Status::Seeding);
    engine.stop().await;
}

/// The fourth review's nit (#270): two `A/` torrents of one selection, moved into an empty folder.
/// `Clashes` tells the second as clashing with the first's files-to-be, so the dialog offers it
/// Rename… and Cancel up front; the first isn't in it. Without a name the second is still refused
/// at its turn, after the first.
#[tokio::test(flavor = "multi_thread")]
async fn two_torrents_of_one_selection_with_the_same_paths_are_told_as_clashing() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    let target = dir.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let second = album(dir.path(), "A", 2).await;
    std::fs::rename(dir.path().join("downloads/A"), dir.path().join("a2")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::rename(dir.path().join("a2"), other.join("A")).unwrap();
    let first = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 1).await, false).await.unwrap().id;
    let second = add(&engine, &other, second, false).await.unwrap().id;
    for id in [first, second] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    let to = AbsolutePath::new(&target).unwrap();

    let found = engine.clashes(vec![first, second], to.clone()).await.unwrap();
    assert_eq!(found.iter().map(|c| (c.id, c.other)).collect::<Vec<_>>(), [(second, Some(first))]);

    let moves = [first, second].map(|id| proto::MoveOf { id, clash: None, name: None }).to_vec();
    let failed = engine.moves(to, moves).await.unwrap();
    assert!(matches!(only_failed(&failed, second), TorrentRefusal::AnothersFiles { .. }));
    engine.stop().await;
}

/// The fourth review's nit (#270): Rename…'s field opens with a name that nothing is at in the
/// folder and no other torrent has a path at, as the disk is when it opens: `A (2)` is a folder
/// there, and another torrent's place is `A (3)`, so `A (4)`.
#[tokio::test(flavor = "multi_thread")]
async fn a_free_name_skips_what_is_at_the_folder_and_what_other_torrents_have() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    std::fs::create_dir_all(other.join("A")).unwrap();
    std::fs::create_dir_all(other.join("A (2)")).unwrap();
    let third = album(dir.path(), "A (3)", 3).await;
    std::fs::rename(dir.path().join("downloads/A (3)"), other.join("A (3)")).unwrap();
    add(&engine, &other, third, false).await.unwrap();
    let a = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 1).await, false).await.unwrap().id;
    wait_for(&engine, a, Status::Seeding).await;

    let name = engine.free_name(a, AbsolutePath::new(&other).unwrap()).await.unwrap();
    assert_eq!(name.as_str(), "A (4)");
    engine.stop().await;
}

/// A torrent of one file gets its number before the extension.
#[tokio::test(flavor = "multi_thread")]
async fn a_free_name_for_a_single_file_counts_before_the_extension() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("one (2).bin"), b"x").unwrap();
    let id = add(&engine, &dir.path().join("downloads"), super::renaming::one_file(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;

    let name = engine.free_name(id, AbsolutePath::new(&other).unwrap()).await.unwrap();
    assert_eq!(name.as_str(), "one (3).bin");
    engine.stop().await;
}

/// Two `A/` torrents: the first's files are in `other/A`. `Clashes` for a Move of the second into
/// `other` names the first, whose files are there; Replace and Use existing are refused as ever,
/// and a new name moves it in beside them, the first's files untouched.
#[tokio::test(flavor = "multi_thread")]
async fn another_torrents_files_are_told_with_that_torrent_and_a_new_name_moves_beside_them() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let first = album(dir.path(), "A", 1).await;
    std::fs::rename(dir.path().join("downloads/A"), other.join("A")).unwrap();
    let first = add(&engine, &other, first, false).await.unwrap().id;
    let second = add(&engine, &dir.path().join("downloads"), album(dir.path(), "A", 2).await, false).await.unwrap().id;
    for id in [first, second] {
        wait_for(&engine, id, Status::Seeding).await;
    }
    let (ours, theirs) = (bytes_in(&other.join("A")), bytes_in(&dir.path().join("downloads/A")));
    let to = AbsolutePath::new(&other).unwrap();

    let found = engine.clashes(vec![second], to.clone()).await.unwrap();
    let [proto::Clashing { id, files, other: whose }] = &found[..] else { panic!("one clash: {found:?}") };
    assert_eq!((*id, *whose), (second, Some(first)), "the first's files are there");
    assert!(!files.is_empty());
    for choice in [proto::Clash::Replace, proto::Clash::UseExisting] {
        let clash = Some(proto::Answered { choice, found: files.clone() });
        let failed = engine.moves(to.clone(), vec![proto::MoveOf { id: second, clash, name: None }]).await.unwrap();
        assert!(matches!(only_failed(&failed, second), TorrentRefusal::AnothersFiles { .. }), "{choice:?}");
    }

    let name = Some(proto::FileName::new("A (2)").unwrap());
    let failed = engine.moves(to, vec![proto::MoveOf { id: second, clash: None, name }]).await.unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    moved(&engine, second).await;
    assert_eq!(bytes_in(&other.join("A (2)")), theirs, "the second's own, under its new name");
    assert_eq!(bytes_in(&other.join("A")), ours, "the first's untouched");
    let t = find(&engine, second).await;
    assert_eq!((t.name.as_deref(), t.status), (Some("A (2)"), Status::Seeding));
    assert_eq!(find(&engine, first).await.status, Status::Seeding);
    engine.stop().await;
}
