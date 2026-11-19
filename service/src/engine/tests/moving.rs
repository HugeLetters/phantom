//! #24: a Move takes a torrent's files to a new destination, renamed on the same drive or copied
//! to another (here a copy is forced on the temp dir's one drive), and adds it back there with the
//! pieces librqbit had. Every destination is the test's own temp dir.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
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
async fn moved(engine: &Engine, id: u64) {
    until(engine, id, |s| !matches!(s, Status::Moving { .. })).await;
    sampled(engine, id).await;
}

fn move_to(dir: &Path) -> TorrentCommand {
    TorrentCommand::Move { to: AbsolutePath::new(dir).unwrap(), clash: None }
}

/// The bytes of each file under `folder`, by path inside it.
fn bytes_in(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
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
const PER_FILE: usize = 20;

/// As `complete_torrent`, with [`PER_FILE`] pieces in each of its three files: librqbit's sampling
/// at a start reads one of each file and a few more (each next one less likely), so it reads far
/// fewer than all of them, where with two each it reads all six one time in six.
async fn many_pieces(dir: &Path) -> AddSource {
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

/// While a Move to Trash of some of its files can be undone, Move to… is off with why, and a Move
/// is refused: the undo would put them back where they were. Once it's undone, it moves.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_waits_while_files_of_it_are_in_the_trash_for_an_undo() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    let off = proto::Answer::Refused { why: TorrentRefusal::TrashedFiles };
    assert_eq!(*find(&engine, id).await.actions.get(Action::Move), off, "off, with why");
    assert_eq!(refusal(engine.command(id, move_to(&elsewhere)).await), (id, TorrentRefusal::TrashedFiles));
    engine.undo().await.unwrap();
    sampled(&engine, id).await;
    assert_eq!(*find(&engine, id).await.actions.get(Action::Move), proto::Answer::Does);
    engine.command(id, move_to(&elsewhere)).await.unwrap();
    moved(&engine, id).await;
    assert_eq!(find(&engine, id).await.location, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap()));
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

/// What the person puts at a file's new path during a move with no way back.
#[derive(Clone, Copy, Debug)]
enum Appears {
    /// A file of theirs, holding `b"mine"`.
    File,
    /// A symbolic link to a file of theirs outside any destination, holding `b"mine"`.
    Link,
}

/// A torrent moved with no way back, a file left out of the move (the re-review's repro on #264):
/// during the copy the person deletes the original of the file being copied, and puts something of
/// their own (`appears`) at another file's new path in `elsewhere`.
struct LeftOut {
    engine: Engine,
    id: u64,
    elsewhere: PathBuf,
    /// The left-out file's name, and its path in `elsewhere`, which holds the person's.
    other: &'static str,
    mine: PathBuf,
    /// The person's file: `mine` itself, or the link's target.
    theirs: PathBuf,
    /// The torrent's own bytes of the left-out file, back in the old destination.
    own: Vec<u8>,
}

async fn left_out(dir: &Path, appears: Appears) -> LeftOut {
    let gate = super::super::trash::Gate::default();
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    holding(&engine, true, Some(gate.clone())).await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let answer = tokio::spawn(engine.command_later(id, move_to(&elsewhere)));
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    // The file being copied loses its original, and the person's is put at another's new path.
    let hash = find(&engine, id).await.info_hash;
    let staging = elsewhere.join(format!(".phantom-moving-{hash}/pack"));
    let copying = std::fs::read_dir(&staging).unwrap().next().unwrap().unwrap().file_name();
    std::fs::remove_file(file(dir, copying.to_str().unwrap())).unwrap();
    let other = ["a.bin", "b.bin", "c.bin"].into_iter().find(|n| *n != copying).unwrap();
    let own = std::fs::read(file(dir, other)).unwrap();
    let mine = elsewhere.join("pack").join(other);
    let theirs = match appears {
        Appears::File => mine.clone(),
        Appears::Link => dir.join("outside/theirs.bin"),
    };
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"mine").unwrap();
    if let Appears::Link = appears {
        std::os::unix::fs::symlink(&theirs, &mine).unwrap();
    }
    super::super::trash::hold(&gate, false);

    let (_, why) = refusal(answer.await.unwrap());
    assert_eq!(why, TorrentRefusal::MovedWithout { name: "pack".into(), files: vec![AbsolutePath::new(&mine).unwrap()] });
    moved(&engine, id).await;
    holding(&engine, false, None).await;
    LeftOut { engine, id, elsewhere, other, mine, theirs, own }
}

/// Resumes torrent `id` and waits past its check, and a while for librqbit to write.
async fn resumed(engine: &Engine, id: u64) {
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until(engine, id, |s| *s != Status::Checking).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// The re-review's blocking finding on #264, as it found it: the move goes forward without the
/// left-out file, answered `MovedWithout` it, the torrent's own copy of it still in the old
/// destination, and the person's file untouched, even once the torrent is resumed and checked there.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_way_back_a_file_put_at_a_new_path_stays_the_persons() {
    let dir = temp();
    let LeftOut { engine, id, elsewhere, other, mine, own, .. } = left_out(dir.path(), Appears::File).await;
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.location), (Status::MissingFiles, Some(AbsolutePath::new(elsewhere.join("pack")).unwrap())));
    assert_eq!(std::fs::read(file(dir.path(), other)).unwrap(), own, "its own copy, where it was");
    let rest = ["a.bin", "b.bin", "c.bin"].into_iter().filter(|n| *n != other);
    assert!(rest.clone().all(|n| !file(dir.path(), n).exists() && elsewhere.join("pack").join(n).exists()), "the rest moved");
    resumed(&engine, id).await;
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file, untouched");
    engine.stop().await;
}

/// The third review's should-fix 4 on #264: a link the person put at the left-out file's path is
/// theirs too, and never followed: the file it points to keeps its bytes once the torrent resumes.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_way_back_a_link_put_at_a_new_path_is_never_followed() {
    let dir = temp();
    let LeftOut { engine, id, mine, theirs, .. } = left_out(dir.path(), Appears::Link).await;
    resumed(&engine, id).await;
    assert_eq!(std::fs::read(&theirs).unwrap(), b"mine", "the file the link points to, untouched");
    assert!(std::fs::symlink_metadata(&mine).unwrap().file_type().is_symlink(), "the link stays a link");
    engine.stop().await;
}

/// The third review's blocking 1 on #264: a second Move of a torrent with a file of the person's at
/// a file's path leaves that file where it is, untouched: it isn't the torrent's, so it doesn't
/// move. The torrent's file is still missing, in the third destination now, and made there.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_move_leaves_the_persons_file_where_it_is() {
    let dir = temp();
    let LeftOut { engine, id, other, mine, .. } = left_out(dir.path(), Appears::File).await;
    let third = dir.path().join("third");
    std::fs::create_dir_all(&third).unwrap();
    let ino = std::fs::metadata(&mine).unwrap().ino();

    let (_, why) = refusal(engine.command(id, move_to(&third)).await);
    let missing = third.join("pack").join(other);
    assert_eq!(why, TorrentRefusal::MovedWithout { name: "pack".into(), files: vec![AbsolutePath::new(&missing).unwrap()] });
    moved(&engine, id).await;
    assert_eq!(std::fs::metadata(&mine).unwrap().ino(), ino, "the person's file stays where it was");
    assert!(!missing.exists(), "not moved as the torrent's");
    resumed(&engine, id).await;
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file, untouched");
    assert_eq!(std::fs::metadata(&mine).unwrap().ino(), ino);
    engine.stop().await;
}

/// The third review's blocking 2 on #264: the person's file is theirs by its path, not by what tells
/// it apart on disk, which a volume mounted again changes (its device number). After a restart with
/// the person's file there under another identity (here: copied anew, another inode), it's guarded
/// all the same.
#[tokio::test(flavor = "multi_thread")]
async fn after_a_restart_the_file_at_the_persons_path_is_still_theirs() {
    let dir = temp();
    let LeftOut { engine, id, mine, .. } = left_out(dir.path(), Appears::File).await;
    engine.stop().await;
    let anew = mine.with_extension("new");
    std::fs::write(&anew, b"mine").unwrap();
    std::fs::rename(&anew, &mine).unwrap();

    let engine = super::engine(dir.path()).await;
    sampled(&engine, id).await;
    resumed(&engine, id).await;
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file, untouched");
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| *s != Status::Checking).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "untouched after a check too");
    engine.stop().await;
}

/// The third review's should-fix 3 on #264: Delete trashes the torrent's files, never the person's
/// at a file's path, which stays where it is.
#[tokio::test(flavor = "multi_thread")]
async fn delete_leaves_the_persons_file_at_a_files_path() {
    let dir = temp();
    let LeftOut { engine, id, elsewhere, other, mine, .. } = left_out(dir.path(), Appears::File).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    assert!(engine.listing().await.is_empty(), "deleted");
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file stays");
    let trashed = bytes_in(&dir.path().join("trash"));
    assert!(trashed.values().all(|b| b != b"mine"), "not in the Trash: {:?}", trashed.keys());
    let rest = ["a.bin", "b.bin", "c.bin"].into_iter().filter(|n| *n != other);
    assert!(rest.clone().all(|n| !elsewhere.join("pack").join(n).exists()), "the torrent's own went");
    assert_eq!(trashed.len(), 2, "the torrent's two files in the Trash: {:?}", trashed.keys());
    engine.stop().await;
}

/// The person's file at a file's path is theirs only while it's there: once they take it away, the
/// torrent makes its own file there, which is the torrent's from then on, the record says so too
/// (nothing of the person's is named), and it completes.
#[tokio::test(flavor = "multi_thread")]
async fn once_the_persons_file_is_gone_the_torrent_makes_its_own_there() {
    let dir = temp();
    let LeftOut { engine, id, mine, own, .. } = left_out(dir.path(), Appears::File).await;
    assert_eq!(theirs(&engine, id).await.len(), 1, "named the person's");
    std::fs::remove_file(&mine).unwrap();
    resumed(&engine, id).await;
    assert_eq!(std::fs::metadata(&mine).unwrap().len(), own.len() as u64, "the torrent's own, made there");
    let mut named = theirs(&engine, id).await;
    for _ in 0..50 {
        if named.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        named = theirs(&engine, id).await;
    }
    assert!(named.is_empty(), "the torrent's now: {named:?}");
    engine.stop().await;
}

/// The record's `theirs` of torrent `id`.
async fn theirs(engine: &Engine, id: u64) -> std::collections::BTreeSet<usize> {
    engine.inspect(move |a| a.torrents[&id].record.theirs.clone()).await
}

/// Move to Trash of the left-out file sets it Off and trashes nothing: what's at its path is the
/// person's.
#[tokio::test(flavor = "multi_thread")]
async fn move_to_trash_of_a_file_at_the_persons_path_leaves_theirs() {
    let dir = temp();
    let LeftOut { engine, id, other, mine, .. } = left_out(dir.path(), Appears::File).await;
    let index = index_of(&engine, id, other).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![index] }).await.unwrap();
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine", "the person's file stays");
    assert!(bytes_in(&dir.path().join("trash")).is_empty(), "nothing in the Trash");
    assert_eq!(engine.details(id).await.unwrap().files[index].priority, proto::Priority::Off);
    engine.stop().await;
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

/// The files of `bytes`, taken out of their `pack/` folder.
fn bytes_in_named(bytes: &BTreeMap<PathBuf, Vec<u8>>) -> BTreeMap<PathBuf, Vec<u8>> {
    bytes.iter().map(|(p, b)| (p.strip_prefix("pack").unwrap_or(p).to_owned(), b.clone())).collect()
}
