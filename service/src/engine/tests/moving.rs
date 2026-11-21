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
    TorrentCommand::Move { to: AbsolutePath::new(dir).unwrap(), clash: None }
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
        assert_eq!(refusal(refused), (id, TorrentRefusal::Clash { files: vec![AbsolutePath::new(&theirs).unwrap()] }));
        assert!(file(dir.path(), "a.bin").exists() && find(&engine, id).await.status == Status::Seeding, "nothing moved");

        let to = AbsolutePath::new(&elsewhere).unwrap();
        engine.command(id, TorrentCommand::Move { to, clash: Some(clash) }).await.unwrap();
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
            let to = AbsolutePath::new(to).unwrap();
            let refused = engine.command(id, TorrentCommand::Move { to: to.clone(), clash }).await;
            assert_eq!(refusal(refused), (id, TorrentRefusal::AlreadyThere), "{} with {clash:?}", to.display());
        }
    }
    let inside = AbsolutePath::new(dir.path().join("alias/pack")).unwrap();
    let refused = engine.command(id, TorrentCommand::Move { to: inside, clash: Some(proto::Clash::Replace) }).await;
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
            async move { if restart { engine.stop_to_restart().await } else { engine.stop().await } }
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

/// The files of `bytes`, taken out of their `pack/` folder.
fn bytes_in_named(bytes: &BTreeMap<PathBuf, Vec<u8>>) -> BTreeMap<PathBuf, Vec<u8>> {
    bytes.iter().map(|(p, b)| (p.strip_prefix("pack").unwrap_or(p).to_owned(), b.clone())).collect()
}
