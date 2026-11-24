//! #358: a torrent the person moved or renamed in Finder is found where it is now. Its files gone
//! from its place, Move to… the folder it's in now or Rename… to its new name moves nothing: the
//! record points there and the torrent is checked there, then it runs or stays stopped as before.
//! Only when none of its files is at its old place, and at the new one its own folder (or its one
//! file) is there, with each of its files there no longer than the torrent's own and of no other
//! hard link; else it's a Move or a Rename as ever. Every destination is the test's own temp dir, and so is the Trash.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::Ordering;

use proto::{FileName, UndoAction};

use super::letgo::engine_reading;
use super::moving::{bytes_in, move_to, moved};
use super::*;

fn rename(name: &str) -> TorrentCommand {
    TorrentCommand::Rename { name: FileName::new(name).unwrap() }
}

fn at(path: PathBuf) -> Option<AbsolutePath> {
    Some(AbsolutePath::new(path).unwrap())
}

/// What tells each file under `folder` from another and from itself changed: its inode, length and
/// modification time, by path inside it.
fn stamps(folder: &Path) -> BTreeMap<PathBuf, (u64, u64, i64, i64)> {
    bytes_in(folder)
        .into_keys()
        .map(|p| {
            let m = std::fs::symlink_metadata(folder.join(&p)).unwrap();
            (p, (m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
        })
        .collect()
}

/// Each file's inode under `folder`, by path inside it: what a rename or a copy would change.
fn inodes(folder: &Path) -> BTreeMap<PathBuf, u64> {
    stamps(folder).into_iter().map(|(p, (ino, ..))| (p, ino)).collect()
}

/// An engine whose reads of the torrents' files wait while the gate it returns is held, which
/// it isn't yet; the guard opens it when the test ends.
async fn reading_engine(dir: &Path) -> (Engine, Arc<super::super::files::Gate>, Arc<std::sync::atomic::AtomicUsize>, Opens) {
    let gate = Arc::new(super::super::files::Gate::default());
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let engine = engine_reading(dir, reads.clone(), gate.clone()).await;
    (engine, gate.clone(), reads, Opens(gate))
}

/// The torrent's folder renamed in Finder, then Rename… to that name, running or paused: found.
/// Nothing is moved, made or written there (each file is the same file, unchanged, while the
/// check reads it), the record names it there, it's checked in full, and it's back to what it
/// was doing, with everything. Its undo is on the stack, as a Rename's.
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_renamed_in_finder_is_found_by_rename_to_its_new_name() {
    for paused in [false, true] {
        let dir = temp();
        let (engine, gate, reads, _opens) = reading_engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, paused).await.unwrap().id;
        let settled = if paused { Status::Done { reached: None } } else { Status::Seeding };
        wait_for(&engine, id, settled.clone()).await;
        let before = bytes_in(&downloads.join("pack"));
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let stamped = stamps(&downloads.join("Album"));
        gate.hold(true);
        reads.store(0, Ordering::SeqCst);

        let answer = engine.commanded(vec![id], rename("Album")).await.unwrap();
        assert_eq!((answer.failed.len(), answer.found), (0, vec![id]), "paused {paused}: found, not renamed");
        until(&engine, id, |s| matches!(s, Status::Checking)).await;
        assert_eq!(stamps(&downloads.join("Album")), stamped, "paused {paused}: no file touched");
        assert!(!downloads.join("pack").exists(), "paused {paused}: nothing made at the old name");
        gate.hold(false);

        wait_for(&engine, id, settled).await;
        let t = find(&engine, id).await;
        assert_eq!((t.name.as_deref(), t.location), (Some("Album"), at(downloads.join("Album"))), "paused {paused}");
        assert_eq!(t.content.unwrap().progress_bytes, 3 * FULL, "paused {paused}: everything");
        assert_eq!(bytes_in(&downloads.join("Album")), before, "paused {paused}: the same bytes");
        let same: BTreeMap<PathBuf, u64> = stamped.into_iter().map(|(p, (ino, ..))| (p, ino)).collect();
        assert_eq!(inodes(&downloads.join("Album")), same, "paused {paused}: the same files");
        assert!(reads.load(Ordering::SeqCst) >= 6, "paused {paused}: checked in full, every piece read");
        assert_eq!(super::stack(&engine).await, [UndoAction::Rename], "paused {paused}");
        engine.stop().await;
    }
}

/// The torrent's folder moved to another folder in Finder, then Move to… that folder: found, as a
/// Rename finds one. Not on the undo stack, as a Move isn't.
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_moved_in_finder_is_found_by_move_to_the_folder_it_is_in() {
    let dir = temp();
    let (engine, gate, reads, _opens) = reading_engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = bytes_in(&downloads.join("pack"));
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::rename(downloads.join("pack"), elsewhere.join("pack")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let stamped = stamps(&elsewhere);
    gate.hold(true);
    reads.store(0, Ordering::SeqCst);

    let answer = engine.commanded(vec![id], move_to(&elsewhere)).await.unwrap();
    assert_eq!((answer.failed.len(), answer.found), (0, vec![id]));
    until(&engine, id, |s| matches!(s, Status::Checking)).await;
    assert_eq!(stamps(&elsewhere), stamped, "no file touched, nothing made beside them");
    assert!(!downloads.join("pack").exists(), "nothing made where it was");
    gate.hold(false);

    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!(t.location, at(elsewhere.join("pack")));
    assert_eq!(t.content.unwrap().progress_bytes, 3 * FULL);
    assert_eq!(bytes_in(&elsewhere.join("pack")), before);
    assert!(reads.load(Ordering::SeqCst) >= 6, "checked in full");
    assert!(super::stack(&engine).await.is_empty(), "a Move isn't undone");
    engine.stop().await;
}

/// A torrent of one file renamed in Finder, then Rename… to its new name: found.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_renamed_in_finder_is_found_by_rename() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, super::renaming::one_file(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("one.bin"), downloads.join("renamed.bin")).unwrap();
    let ino = std::fs::metadata(downloads.join("renamed.bin")).unwrap().ino();
    wait_for(&engine, id, Status::MissingFiles).await;

    let answer = engine.commanded(vec![id], rename("renamed.bin")).await.unwrap();
    assert_eq!(answer.found, vec![id]);
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!(t.location, at(downloads.join("renamed.bin")));
    assert_eq!(std::fs::metadata(downloads.join("renamed.bin")).unwrap().ino(), ino, "the same file");
    assert!(!downloads.join("one.bin").exists());
    engine.stop().await;
}

/// Some of its files still where it was: not found. A Rename to the name its other files are at
/// now is refused, as that name is taken, and a Move into the folder they're in is asked about,
/// as a clash; nothing moves either way.
#[tokio::test(flavor = "multi_thread")]
async fn files_partly_where_it_was_are_not_found() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
    std::fs::create_dir_all(downloads.join("Album")).unwrap();
    for name in ["b.bin", "c.bin"] {
        std::fs::copy(downloads.join("pack").join(name), elsewhere.join("pack").join(name)).unwrap();
        std::fs::rename(downloads.join("pack").join(name), downloads.join("Album").join(name)).unwrap();
    }
    wait_for(&engine, id, Status::MissingFiles).await;
    let (here, there, old) = (bytes_in(&downloads.join("Album")), bytes_in(&elsewhere), bytes_in(&downloads.join("pack")));

    let why = refusal(engine.command(id, rename("Album")).await);
    assert_eq!(why, (id, TorrentRefusal::NameTaken { path: AbsolutePath::new(downloads.join("Album")).unwrap() }));
    let why = refusal(engine.command(id, move_to(&elsewhere)).await);
    assert!(matches!(why, (_, TorrentRefusal::Clash { ref files }) if files.len() == 2), "{why:?}");
    assert_eq!((bytes_in(&downloads.join("Album")), bytes_in(&elsewhere), bytes_in(&downloads.join("pack"))), (here, there, old));
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.location), (Status::MissingFiles, at(downloads.join("pack"))));
    engine.stop().await;
}

/// Its folder moved into another listed torrent's place: refused as a Move into it is, whatever
/// is there, and a Rename to another torrent's name is refused too; nothing moves.
#[tokio::test(flavor = "multi_thread")]
async fn another_torrents_place_is_refused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    // Another "pack", with an a.bin, there; and another torrent named "Theirs" beside this one.
    let other = add_to(&engine, multi_file_torrent(&["a.bin", "z.bin"]), true, &elsewhere).await.unwrap().id;
    let theirs = add(&engine, &downloads, torrent_file("Theirs"), true).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let _ = std::fs::remove_file(downloads.join("Theirs"));
    let _ = std::fs::remove_dir_all(elsewhere.join("pack"));
    std::fs::rename(downloads.join("pack"), elsewhere.join("pack")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let there = bytes_in(&elsewhere);

    let why = refusal(engine.command(id, move_to(&elsewhere)).await);
    assert_eq!(why, (id, TorrentRefusal::AnothersFiles { path: AbsolutePath::new(elsewhere.join("pack/a.bin")).unwrap() }));
    std::fs::create_dir_all(downloads.join("Theirs")).unwrap();
    let why = refusal(engine.command(id, rename("Theirs")).await);
    assert!(matches!(why, (_, TorrentRefusal::NameOfAnother { .. } | TorrentRefusal::NameTaken { .. })), "{why:?}");
    assert_eq!(bytes_in(&elsewhere), there, "nothing moved");
    assert_eq!(find(&engine, id).await.location, at(downloads.join("pack")));
    let _ = (other, theirs);
    engine.stop().await;
}

/// At the new name, a file of the torrent's other bytes, as long as its own, and a file of the
/// person's beside them: found, and the check finds that file's pieces aren't the torrent's. The
/// person's file isn't touched, nor is anything outside the torrent's own files. A file there
/// longer than the torrent's own (the check would cut it to the torrent's length) isn't found:
/// the name is taken, as ever.
#[tokio::test(flavor = "multi_thread")]
async fn other_bytes_at_the_new_name_are_checked_and_a_longer_file_is_not_found() {
    for longer in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, true).await.unwrap().id;
        wait_for(&engine, id, Status::Done { reached: None }).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        let len = if longer { 2 * PIECE + 1 } else { 2 * PIECE };
        std::fs::write(downloads.join("Album/a.bin"), vec![9; len]).unwrap();
        std::fs::write(downloads.join("Album/mine.txt"), b"mine").unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let before = bytes_in(&downloads.join("Album"));
        let mine = stamps(&downloads.join("Album")).remove(Path::new("mine.txt"));

        let answer = engine.commanded(vec![id], rename("Album")).await;
        match longer {
            false => {
                assert_eq!(answer.unwrap().found, vec![id]);
                moved(&engine, id).await;
                until(&engine, id, |s| !matches!(s, Status::Checking)).await;
                let t = find(&engine, id).await;
                assert_eq!(t.location, at(downloads.join("Album")));
                assert_eq!(t.content.unwrap().progress_bytes, 2 * FULL, "a.bin's pieces aren't the torrent's");
                assert_eq!(bytes_in(&downloads.join("Album")), before, "stopped: nothing written");
            }
            true => {
                let mut answer = answer.unwrap();
                assert!(answer.found.is_empty(), "longer {longer}: not found");
                let why = refusal::<()>(Err(answer.failed.remove(0).1));
                assert_eq!(why, (id, TorrentRefusal::NameTaken { path: AbsolutePath::new(downloads.join("Album")).unwrap() }));
                assert_eq!(bytes_in(&downloads.join("Album")), before, "longer {longer}: nothing changed");
            }
        }
        assert_eq!(stamps(&downloads.join("Album")).remove(Path::new("mine.txt")), mine, "longer {longer}: the person's file as it was");
        engine.stop().await;
    }
}

/// ⌘Z on a Rename that found its torrent points it back at its old name, moving nothing: its files
/// stay where the person put them, and it's Missing files again, as before.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_found_rename_points_it_back_and_moves_nothing() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.command(id, rename("Album")).await.unwrap();
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    let found = inodes(&downloads.join("Album"));

    let undone = engine.undo().await.unwrap();
    assert_eq!(undone.undo.action, UndoAction::Rename);
    moved(&engine, id).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let t = find(&engine, id).await;
    assert_eq!((t.name.as_deref(), t.location), (Some("pack"), at(downloads.join("pack"))));
    assert_eq!(inodes(&downloads.join("Album")), found, "its files stay where the person put them");
    assert!(!downloads.join("pack").exists(), "nothing made at the old name");
    engine.stop().await;
}

/// Nothing is found through a link: the torrent's folder renamed in Finder, Rename… to the name
/// of a link to that folder, or to a folder whose file of the torrent's is a link to the person's
/// own copy elsewhere, isn't found, and the name is taken, as ever. What either link leads to is
/// as it was.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_found_through_a_link() {
    for link in ["folder", "file"] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, true).await.unwrap().id;
        wait_for(&engine, id, Status::Done { reached: None }).await;
        let mine = dir.path().join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        match link {
            "folder" => std::os::unix::fs::symlink(downloads.join("Album"), downloads.join("Linked")).unwrap(),
            _ => {
                std::fs::create_dir_all(downloads.join("Linked")).unwrap();
                std::fs::copy(downloads.join("Album/a.bin"), mine.join("a.bin")).unwrap();
                std::os::unix::fs::symlink(mine.join("a.bin"), downloads.join("Linked/a.bin")).unwrap();
            }
        }
        wait_for(&engine, id, Status::MissingFiles).await;
        let (album, theirs) = (stamps(&downloads.join("Album")), stamps(&mine));

        let why = refusal(engine.command(id, rename("Linked")).await);
        assert_eq!(why, (id, TorrentRefusal::NameTaken { path: AbsolutePath::new(downloads.join("Linked")).unwrap() }), "{link}");
        assert_eq!((stamps(&downloads.join("Album")), stamps(&mine)), (album, theirs), "{link}: nothing changed");
        let t = find(&engine, id).await;
        assert_eq!((t.status, t.location), (Status::MissingFiles, at(downloads.join("pack"))), "{link}");
        engine.stop().await;
    }
}

/// A file there with other hard links isn't found: the torrent's folder renamed or moved in Finder,
/// its last file then hard-linked from a folder of the person's, Rename… to the new name is refused
/// as the name is taken, and Move to… the folder asks about the clash, as ever. Both names keep
/// their bytes. With that other link gone, the same place is found.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_with_other_hard_links_is_not_found() {
    for how in ["rename", "move"] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, true).await.unwrap().id;
        wait_for(&engine, id, Status::Done { reached: None }).await;
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let (now, command) = match how {
            "rename" => (downloads.join("Album"), rename("Album")),
            _ => (elsewhere.join("pack"), move_to(&elsewhere)),
        };
        std::fs::rename(downloads.join("pack"), &now).unwrap();
        let mine = dir.path().join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::hard_link(now.join("c.bin"), mine.join("c.bin")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let (there, theirs) = (bytes_in(&now), bytes_in(&mine));

        let why = refusal(engine.command(id, command.clone()).await);
        match how {
            "rename" => assert_eq!(why, (id, TorrentRefusal::NameTaken { path: AbsolutePath::new(&now).unwrap() })),
            _ => assert!(matches!(why, (_, TorrentRefusal::Clash { ref files }) if files.len() == 3), "{why:?}"),
        }
        assert_eq!((bytes_in(&now), bytes_in(&mine)), (there.clone(), theirs), "{how}: both names' bytes as they were");
        let t = find(&engine, id).await;
        assert_eq!((t.status, t.location), (Status::MissingFiles, at(downloads.join("pack"))), "{how}");

        std::fs::remove_file(mine.join("c.bin")).unwrap();
        let answer = engine.commanded(vec![id], command).await.unwrap();
        assert_eq!(answer.found, vec![id], "{how}: one link only, found");
        moved(&engine, id).await;
        wait_for(&engine, id, Status::Done { reached: None }).await;
        assert_eq!(find(&engine, id).await.location, at(now.clone()), "{how}");
        assert_eq!(bytes_in(&now), there, "{how}");
        engine.stop().await;
    }
}

/// ⌘Z on a found Rename once the person has put its folder back under its old name in Finder:
/// it's found there too, pointed back and checked in full, nothing moved.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_found_rename_finds_it_back_under_its_old_name() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.command(id, rename("Album")).await.unwrap();
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("Album"), downloads.join("pack")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let found = inodes(&downloads.join("pack"));

    let undone = engine.undo().await.unwrap();
    assert_eq!(undone.undo.action, UndoAction::Rename);
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!((t.name.as_deref(), t.location), (Some("pack"), at(downloads.join("pack"))));
    assert_eq!(t.content.unwrap().progress_bytes, 3 * FULL);
    assert_eq!(inodes(&downloads.join("pack")), found, "the same files, where the person put them");
    assert!(!downloads.join("Album").exists(), "nothing made at the name it had");
    engine.stop().await;
}

/// A crash once a found Rename's record names its new name, before it's over: the next start
/// finishes it there, and its undo still points it back, moving nothing, as it was written ahead.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_rename_a_crash_cut_short_is_still_undone_moving_nothing() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = crashed.clone();
        engine.inspect(move |a| a.move_hold.crash_once_moved = Some(hold)).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), engine.command(id, rename("Album"))).await;
        assert!(crashed.load(Ordering::SeqCst), "it crashed there");
        id
    };
    let engine = engine(dir.path()).await;
    moved(&engine, id).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(find(&engine, id).await.location, at(downloads.join("Album")));
    let found = inodes(&downloads.join("Album"));

    engine.undo().await.unwrap();
    moved(&engine, id).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(find(&engine, id).await.location, at(downloads.join("pack")));
    assert_eq!(inodes(&downloads.join("Album")), found, "its files stay where the person put them");
    assert!(!downloads.join("pack").exists());
    engine.stop().await;
}

// Held while it's checked (#358): what's found isn't touched until the check found its pieces there.

/// Everything under `folder`, its files and folders, by path inside it: whether it's a folder, its
/// inode, length and modification time. A file made, written, grown or stamped, or a folder made,
/// changes it.
fn tree(folder: &Path) -> BTreeMap<PathBuf, (bool, u64, u64, i64, i64)> {
    let mut found = BTreeMap::new();
    let mut dirs = vec![folder.to_owned()];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                dirs.push(e.path());
            }
            let at = e.path().strip_prefix(folder).unwrap().to_owned();
            found.insert(at, (m.is_dir(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()));
        }
    }
    found
}

/// Waits until torrent `id` is held so, or isn't (`None`).
async fn until_held(engine: &Engine, id: u64, held: Option<proto::Held>) {
    for _ in 0..1000 {
        if find(engine, id).await.held == held {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("torrent {id} never held {held:?}: {:?}", find(engine, id).await);
}

/// An engine whose storage keeps what's under `folder` as each storage of librqbit's is made, in
/// the snapshots it returns: before what it makes, librqbit's own (held or not) making the files.
async fn keeping_trees(dir: &Path, folder: PathBuf) -> (Engine, Arc<std::sync::Mutex<Vec<BTreeMap<PathBuf, (bool, u64, u64, i64, i64)>>>>) {
    let trees = Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = trees.clone();
    let guard = super::super::files::Guard::default().with_hook(Arc::new(move |at| {
        if at == super::super::files::At::Init {
            kept.lock().unwrap().push(tree(&folder));
        }
    }));
    let engine = super::letgo::engine_guarded(dir, guard, Default::default()).await;
    (engine, trees)
}

/// A found torrent, one of its files not there, is held while it's checked: nothing there is made,
/// written, grown or stamped, the file that isn't there isn't made, nor any folder. Its pieces
/// found there, the hold is lifted, and only then are its own files touched (each set to its
/// length, the one not there made); it's back to running or stopped as it was, with what it had
/// there, and the person's file beside them stays as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_torrent_is_held_untouched_while_checked_then_back_as_it_was() {
    for paused in [false, true] {
        let dir = temp();
        let downloads = dir.path().join("downloads");
        let (engine, trees) = keeping_trees(dir.path(), downloads.join("Album")).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, paused).await.unwrap().id;
        let settled = if paused { Status::Done { reached: None } } else { Status::Seeding };
        wait_for(&engine, id, settled).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        std::fs::remove_file(downloads.join("Album/c.bin")).unwrap();
        std::fs::write(downloads.join("Album/mine.txt"), b"mine").unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let (before, bytes) = (tree(&downloads.join("Album")), bytes_in(&downloads.join("Album")));
        let made = trees.lock().unwrap().len();

        let answer = engine.commanded(vec![id], rename("Album")).await.unwrap();
        assert_eq!(answer.found, vec![id], "paused {paused}");
        let back = if paused { Status::Paused } else { Status::Downloading };
        wait_for(&engine, id, back).await;
        // Its held storage, for the check, then its normal one, once the check found its pieces:
        // up to then nothing there was touched.
        let since = trees.lock().unwrap()[made..].to_vec();
        assert!(since.len() >= 2, "paused {paused}: held, then lifted: {}", since.len());
        for (n, seen) in since.iter().enumerate() {
            assert_eq!(seen, &before, "paused {paused}: nothing touched or made while held, up to storage {n}");
        }
        let t = find(&engine, id).await;
        assert_eq!((t.held, t.content.unwrap().progress_bytes), (None, 2 * FULL), "paused {paused}: what it had there");
        let after = bytes_in(&downloads.join("Album"));
        assert_eq!(after.get(Path::new("c.bin")).map(Vec::len), Some(2 * PIECE), "paused {paused}: its own file made, once lifted");
        for name in ["a.bin", "b.bin", "mine.txt"] {
            assert_eq!(after.get(Path::new(name)), bytes.get(Path::new(name)), "paused {paused}: {name}'s bytes");
        }
        let mine = Path::new("mine.txt");
        assert_eq!(tree(&downloads.join("Album")).get(mine), before.get(mine), "paused {paused}: the person's file as it was");
        engine.stop().await;
    }
}

/// Found files of the torrent's names and lengths but other bytes: the check finds none of its
/// pieces there, so it stays stopped, though it was running, and is told "Files don't match".
/// Nothing there changed at all: no byte, length or time, no file or folder made. Resume then
/// downloads over them, as the person chooses.
#[tokio::test(flavor = "multi_thread")]
async fn found_files_none_of_whose_pieces_are_the_torrents_are_left_untouched_and_it_stays_stopped() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
    for name in ["a.bin", "b.bin", "c.bin"] {
        std::fs::write(downloads.join("Album").join(name), vec![9; 2 * PIECE]).unwrap();
    }
    std::fs::write(downloads.join("Album/mine.txt"), b"mine").unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let (before, bytes) = (tree(&downloads.join("Album")), bytes_in(&downloads.join("Album")));

    let answer = engine.commanded(vec![id], rename("Album")).await.unwrap();
    assert_eq!(answer.found, vec![id]);
    until_held(&engine, id, Some(proto::Held::DontMatch)).await;
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.content.unwrap().progress_bytes), (Status::Paused, 0), "stopped, none of it there");
    // A size check or two later, too.
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert_eq!(tree(&downloads.join("Album")), before, "nothing touched");
    assert_eq!(bytes_in(&downloads.join("Album")), bytes);
    assert_eq!(find(&engine, id).await.status, Status::Paused);

    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until_held(&engine, id, None).await;
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// A crash while a found torrent is held for its check: it comes back held, checked in full again
/// with nothing touched meanwhile, a file of it not there too, then runs as it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_torrent_held_for_its_check_comes_back_held_after_a_crash() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let (id, before) = {
        let (engine, gate, _reads, _opens) = reading_engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        std::fs::remove_file(downloads.join("Album/c.bin")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let before = tree(&downloads.join("Album"));
        gate.hold(true);
        assert_eq!(engine.commanded(vec![id], rename("Album")).await.unwrap().found, vec![id]);
        until(&engine, id, |s| matches!(s, Status::Checking)).await;
        assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking));
        engine.kill().await;
        (id, before)
    };
    let (gate, reads) = (Arc::new(super::super::files::Gate::default()), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    gate.hold(true);
    let _opens = Opens(gate.clone());
    let engine = engine_reading(dir.path(), reads.clone(), gate.clone()).await;
    let t = find(&engine, id).await;
    assert_eq!((t.held, t.location), (Some(proto::Held::Checking), at(downloads.join("Album"))), "back held");
    until(&engine, id, |s| matches!(s, Status::Checking)).await;
    assert_eq!(tree(&downloads.join("Album")), before, "nothing touched while held");
    gate.hold(false);
    wait_for(&engine, id, Status::Downloading).await;
    until_held(&engine, id, None).await;
    let t = find(&engine, id).await;
    assert_eq!((t.held, t.content.unwrap().progress_bytes), (None, 2 * FULL));
    assert!(reads.load(Ordering::SeqCst) >= 4, "checked in full");
    engine.stop().await;
}

/// The bytes of a torrent of other contents, of the same names and lengths, at `folder`.
fn other_bytes(folder: &Path) {
    for name in ["a.bin", "b.bin", "c.bin"] {
        std::fs::write(folder.join(name), vec![9; 2 * PIECE]).unwrap();
    }
}

/// What a test of the hold has: the torrent `id` renamed in Finder to "Album" and found there, its
/// check stopped at `gate` as it reads, and what's there (`before`).
struct Found {
    dir: TempDir,
    engine: Engine,
    gate: Arc<super::super::files::Gate>,
    id: u64,
    album: PathBuf,
    before: (BTreeMap<PathBuf, (bool, u64, u64, i64, i64)>, BTreeMap<PathBuf, Vec<u8>>),
    _opens: Opens,
}

impl Found {
    /// Nothing at its folder changed since it was found.
    fn untouched(&self) -> bool {
        (tree(&self.album), bytes_in(&self.album)) == self.before
    }
}

/// [`Found`], running or `paused` before, `changed` making what's at "Album" otherwise before it's found.
async fn found_and_held(paused: bool, changed: impl Fn(&Path)) -> Found {
    let dir = temp();
    let (engine, gate, _reads, opens) = reading_engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, paused).await.unwrap().id;
    wait_for(&engine, id, if paused { Status::Done { reached: None } } else { Status::Seeding }).await;
    let album = downloads.join("Album");
    std::fs::rename(downloads.join("pack"), &album).unwrap();
    changed(&album);
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = (tree(&album), bytes_in(&album));
    gate.hold(true);
    assert_eq!(engine.commanded(vec![id], rename("Album")).await.unwrap().found, vec![id]);
    until(&engine, id, |s| matches!(s, Status::Checking)).await;
    assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking));
    Found { dir, engine, gate, id, album, before, _opens: opens }
}

/// A file there shorter than the torrent's own (partly downloaded) isn't grown while it's held; its
/// hold lifted, it's set to its length, with the pieces it has.
#[tokio::test(flavor = "multi_thread")]
async fn a_shorter_found_file_is_grown_only_once_the_hold_is_lifted() {
    let f = found_and_held(false, |album| {
        std::fs::OpenOptions::new().write(true).open(album.join("c.bin")).unwrap().set_len(PIECE as u64 + 5).unwrap();
    })
    .await;
    assert!(f.untouched());
    f.gate.hold(false);
    wait_for(&f.engine, f.id, Status::Downloading).await;
    until_held(&f.engine, f.id, None).await;
    let t = find(&f.engine, f.id).await;
    assert_eq!(t.content.unwrap().progress_bytes, 2 * FULL + PIECE as u64, "all but c.bin's last piece");
    let after = bytes_in(&f.album);
    assert_eq!(after[Path::new("c.bin")].len(), 2 * PIECE, "set to its length");
    assert_eq!(after[Path::new("c.bin")][..PIECE + 5], f.before.1[Path::new("c.bin")][..], "its bytes kept");
    for name in ["a.bin", "b.bin"] {
        assert_eq!(after[Path::new(name)], f.before.1[Path::new(name)], "{name}");
    }
    f.engine.stop().await;
}

/// Resume pressed while it's held for its check doesn't get past it: checked all the same, and with
/// none of its pieces there it's stopped, nothing touched. Also one paused before.
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_while_held_waits_for_the_check() {
    for paused in [false, true] {
        let f = found_and_held(paused, other_bytes).await;
        f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
        tokio::time::sleep(SIZE_CHECK_EVERY * 2).await;
        assert_eq!(find(&f.engine, f.id).await.status, Status::Checking, "paused {paused}");
        f.gate.hold(false);
        until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
        tokio::time::sleep(SIZE_CHECK_EVERY * 2).await;
        assert_eq!(find(&f.engine, f.id).await.status, Status::Paused, "paused {paused}");
        assert!(f.untouched(), "paused {paused}");
        f.engine.stop().await;
    }
    // Its pieces there, one paused before runs once checked, as Resume asked.
    let f = found_and_held(true, |_| {}).await;
    f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
    assert!(f.untouched());
    f.gate.hold(false);
    wait_for(&f.engine, f.id, Status::Seeding).await;
    f.engine.stop().await;
}

/// None of its pieces there and one of its files not there either: nothing is made, it doesn't run
/// (Missing files), and it's told its files don't match. Resume makes it, and downloads.
#[tokio::test(flavor = "multi_thread")]
async fn files_that_do_not_match_with_one_missing_are_left_untouched() {
    let f = found_and_held(false, |album| {
        other_bytes(album);
        std::fs::remove_file(album.join("c.bin")).unwrap();
    })
    .await;
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    wait_for(&f.engine, f.id, Status::MissingFiles).await;
    tokio::time::sleep(SIZE_CHECK_EVERY * 2).await;
    assert!(f.untouched());
    f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
    until_held(&f.engine, f.id, None).await;
    wait_for(&f.engine, f.id, Status::Downloading).await;
    f.engine.stop().await;
}

/// Files that don't match, across a restart and a crash: still held, stopped, untouched. Check, the
/// person's own, reads them untouched too; with its files put there, it's lifted, and stays stopped.
#[tokio::test(flavor = "multi_thread")]
async fn files_that_do_not_match_stay_held_across_a_restart_until_they_do() {
    let f = found_and_held(false, other_bytes).await;
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    let mut now = Some(f.engine);
    for crash in [false, true] {
        match crash {
            true => now.take().unwrap().kill().await,
            false => now.take().unwrap().stop().await,
        }
        now = Some(engine(f.dir.path()).await);
        let engine = now.as_ref().unwrap();
        until(engine, f.id, |s| matches!(s, Status::Paused)).await;
        tokio::time::sleep(SIZE_CHECK_EVERY * 2).await;
        let t = find(engine, f.id).await;
        assert_eq!((t.status, t.held), (Status::Paused, Some(proto::Held::DontMatch)), "crash {crash}");
        assert_eq!((tree(&f.album), bytes_in(&f.album)), f.before, "crash {crash}");
    }
    let engine = now.unwrap();
    engine.command(f.id, TorrentCommand::Check).await.unwrap();
    until(&engine, f.id, |s| matches!(s, Status::Paused)).await;
    assert_eq!(find(&engine, f.id).await.held, Some(proto::Held::DontMatch));
    assert_eq!((tree(&f.album), bytes_in(&f.album)), f.before, "checked untouched");
    complete_torrent(&f.dir.path().join("again")).await;
    for name in ["a.bin", "b.bin", "c.bin"] {
        std::fs::copy(f.dir.path().join("again/downloads/pack").join(name), f.album.join(name)).unwrap();
    }
    engine.command(f.id, TorrentCommand::Check).await.unwrap();
    until_held(&engine, f.id, None).await;
    wait_for(&engine, f.id, Status::Done { reached: None }).await;
    engine.stop().await;
}

/// Cancel check while it's held: Unchecked and still held, a restart after too; Resume checks it,
/// held, as ever.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_torrent_whose_check_is_cancelled_stays_held() {
    let f = found_and_held(false, other_bytes).await;
    f.engine.command(f.id, TorrentCommand::CancelCheck).await.unwrap();
    f.gate.hold(false);
    let t = find(&f.engine, f.id).await;
    assert_eq!((t.status, t.held), (Status::Unchecked, Some(proto::Held::Checking)));
    f.engine.stop().await;
    let engine = engine(f.dir.path()).await;
    let t = find(&engine, f.id).await;
    assert_eq!((t.status, t.held), (Status::Unchecked, Some(proto::Held::Checking)), "after a restart");
    engine.command(f.id, TorrentCommand::Resume).await.unwrap();
    until_held(&engine, f.id, Some(proto::Held::DontMatch)).await;
    wait_for(&engine, f.id, Status::Paused).await;
    assert_eq!((tree(&f.album), bytes_in(&f.album)), f.before);
    engine.stop().await;
}

/// Removed while it's held, then ⌘Z: back held, its check starting again, its files untouched
/// throughout; other bytes there, it's stopped all the same.
#[tokio::test(flavor = "multi_thread")]
async fn a_remove_while_held_leaves_the_files_and_its_undo_brings_it_back_held() {
    for others in [false, true] {
        let f = found_and_held(false, |album| {
            if others {
                other_bytes(album)
            }
        })
        .await;
        f.engine.command(f.id, TorrentCommand::Remove).await.unwrap();
        assert!(f.untouched(), "others {others}: removed");
        f.engine.undo().await.unwrap();
        until(&f.engine, f.id, |s| matches!(s, Status::Checking)).await;
        assert_eq!(find(&f.engine, f.id).await.held, Some(proto::Held::Checking), "others {others}");
        assert!(f.untouched(), "others {others}: back");
        f.gate.hold(false);
        match others {
            true => until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await,
            false => wait_for(&f.engine, f.id, Status::Seeding).await,
        }
        if others {
            wait_for(&f.engine, f.id, Status::Paused).await;
            assert!(f.untouched(), "others {others}: checked");
        }
        f.engine.stop().await;
    }
}

/// Deleted while its files don't match, or while they're still checked: nothing was verified as the
/// torrent's, so it's only taken off the list. Nothing goes to the Trash, nothing is written or removed
/// at its paths, and the answer says its files stayed. ⌘Z brings it back held, stopped.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_of_files_not_verified_leaves_them_and_its_undo_brings_it_back_held() {
    for checking in [false, true] {
        let f = found_and_held(false, |album| {
            other_bytes(album);
            std::fs::write(album.join("mine.txt"), b"mine").unwrap();
        })
        .await;
        if !checking {
            f.gate.hold(false);
            until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
            wait_for(&f.engine, f.id, Status::Paused).await;
        }
        let answer = f.engine.commanded(vec![f.id], TorrentCommand::Delete).await.unwrap();
        assert!(answer.failed.is_empty(), "checking {checking}");
        assert_eq!(
            answer.stayed,
            vec![(f.id, if checking { proto::Held::Checking } else { proto::Held::DontMatch })],
            "checking {checking}: its files stayed"
        );
        assert!(f.engine.listing().await.is_empty(), "checking {checking}: off the list");
        assert!(f.untouched(), "checking {checking}: nothing written or removed");
        assert!(
            !f.dir.path().join("trash").exists() || std::fs::read_dir(f.dir.path().join("trash")).unwrap().next().is_none(),
            "nothing in the Trash"
        );
        f.engine.undo().await.unwrap();
        if checking {
            // Still checked, as it was.
            until(&f.engine, f.id, |s| matches!(s, Status::Checking)).await;
            assert_eq!(find(&f.engine, f.id).await.held, Some(proto::Held::Checking));
            f.gate.hold(false);
        }
        until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
        wait_for(&f.engine, f.id, Status::Paused).await;
        assert!(f.untouched(), "checking {checking}: back as they were");
        f.engine.stop().await;
    }
}

/// Several deleted in one press: each goes by its own state. The held one's files stay, the ordinary
/// one's go to the Trash, and the answer says which stayed.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_of_several_goes_by_each_ones_own_state() {
    let f = found_and_held(false, other_bytes).await;
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    let plain = complete_torrent_in(f.dir.path(), "plain", 5).await;
    let other = add_to(&f.engine, plain, false, &f.dir.path().join("plain")).await.unwrap().id;
    wait_for(&f.engine, other, Status::Seeding).await;
    let answer = f.engine.commanded(vec![f.id, other], TorrentCommand::Delete).await.unwrap();
    assert!(answer.failed.is_empty());
    assert_eq!(answer.stayed, vec![(f.id, proto::Held::DontMatch)]);
    assert!(f.untouched());
    assert!(!f.dir.path().join("plain/pack/a.bin").exists(), "the other one's files went to the Trash");
    f.engine.stop().await;
}

/// Moved to another folder while it's held, the files go there as they are and it's still held there:
/// checked, then stopped or back as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_while_held_takes_the_files_as_they_are_and_it_is_held_there() {
    for others in [false, true] {
        let f = found_and_held(false, |album| {
            if others {
                other_bytes(album)
            }
        })
        .await;
        let elsewhere = f.dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let moving = f.engine.sent(f.id, move_to(&elsewhere)).await;
        f.gate.hold(false);
        moving.await.unwrap();
        match others {
            true => {
                until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
                wait_for(&f.engine, f.id, Status::Paused).await;
                assert_eq!(bytes_in(&elsewhere.join("Album")), f.before.1, "as they were");
                let (now, files) = (tree(&elsewhere.join("Album")), &f.before.0);
                assert_eq!(&now, files, "nothing made, grown or stamped");
            }
            false => {
                wait_for(&f.engine, f.id, Status::Seeding).await;
                until_held(&f.engine, f.id, None).await;
                assert_eq!(bytes_in(&elsewhere.join("Album")), f.before.1);
            }
        }
        assert_eq!(find(&f.engine, f.id).await.location, at(elsewhere.join("Album")), "others {others}");
        f.engine.stop().await;
    }
}

/// ⌘Z on the found Rename while it's held, or once its files didn't match: it's pointed back, Missing
/// files there, held no more, and its files stay as they are.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_found_rename_while_held_points_it_back_untouched() {
    for checked in [false, true] {
        let f = found_and_held(false, other_bytes).await;
        if checked {
            f.gate.hold(false);
            until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
        }
        let (undone, ()) = tokio::join!(f.engine.undo(), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            f.gate.hold(false);
        });
        undone.unwrap();
        moved(&f.engine, f.id).await;
        wait_for(&f.engine, f.id, Status::MissingFiles).await;
        let t = find(&f.engine, f.id).await;
        assert_eq!((t.held, t.location), (None, at(f.dir.path().join("downloads/pack"))), "checked {checked}");
        assert!(!f.dir.path().join("downloads/pack").exists(), "checked {checked}");
        assert!(f.untouched(), "checked {checked}");
        f.engine.stop().await;
    }
}

/// A crash before the record names where it's found: it's where it was, Missing files, not held there.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_rename_a_crash_put_back_is_not_held_where_it_was() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = crashed.clone();
        engine.inspect(move |a| a.move_hold.crash_before_take_out = Some(hold)).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), engine.command(id, rename("Album"))).await;
        assert!(crashed.load(Ordering::SeqCst), "it crashed there");
        id
    };
    let before = tree(&downloads.join("Album"));
    let engine = engine(dir.path()).await;
    moved(&engine, id).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let t = find(&engine, id).await;
    assert_eq!((t.held, t.location), (None, at(downloads.join("pack"))));
    assert_eq!(tree(&downloads.join("Album")), before);
    engine.stop().await;
}

/// Its check over while a file of it is on its way to the Trash: the hold waits for that, then lifts.
#[tokio::test(flavor = "multi_thread")]
async fn the_hold_is_not_lifted_under_a_move_to_trash() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let gate = Arc::new(super::super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let trash: super::super::trash::Gate = Default::default();
    let _opens_trash = OpensTrash(trash.clone());
    let guard = super::super::files::Guard::default().with_gate(super::super::files::At::Read, gate.clone());
    let engine = super::letgo::engine_guarded(dir.path(), guard, trash.clone()).await;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
    // One it gets isn't there, which a size check under the Move to Trash mustn't take for Missing files.
    let c = file_index(&engine, id, "c.bin").await;
    std::fs::remove_file(downloads.join("Album/c.bin")).unwrap();
    let a = file_index(&engine, id, "a.bin").await;
    wait_for(&engine, id, Status::MissingFiles).await;
    gate.hold(true);
    engine.command(id, rename("Album")).await.unwrap();
    until(&engine, id, |s| matches!(s, Status::Checking)).await;
    super::super::trash::hold(&trash, true);
    let trashing = engine.sent(id, TorrentCommand::TrashFiles { files: vec![a] }).await;
    held_at_the_gate(&trash).await;
    gate.hold(false);
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert!(!downloads.join("Album/c.bin").exists(), "file {c} isn't made meanwhile");
    assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking), "under the Move to Trash");
    let started = engine
        .inspect(move |a| {
            let handle = a.torrents[&id].lib.torrent().expect("in librqbit");
            matches!(handle.stats().state, librqbit::TorrentStatsState::Live)
        })
        .await;
    assert!(!started, "never started while held");
    super::super::trash::hold(&trash, false);
    trashing.await.unwrap();
    until_held(&engine, id, None).await;
    assert_eq!(bytes_in(&downloads.join("Album")).len(), 2, "a.bin in the Trash, not made again; c.bin made");
    engine.stop().await;
}

// The lift: only what the check read, unchanged and the torrent's own, is opened to write (#361's review).

async fn file_index(engine: &Engine, id: u64, name: &str) -> usize {
    engine.details(id).await.unwrap().files.iter().find(|f| f.path.ends_with(name)).unwrap().index
}

async fn set(engine: &Engine, id: u64, names: &[&str], priority: proto::Priority) {
    let mut files = Vec::new();
    for name in names {
        files.push(file_index(engine, id, name).await);
    }
    engine.command(id, TorrentCommand::SetPriority { files, priority }).await.unwrap();
}

/// Puts `bytes` at `path` as another file, in one step.
fn swap(f: &Found, name: &str, bytes: &[u8]) {
    std::fs::write(f.dir.path().join("mine"), bytes).unwrap();
    std::fs::rename(f.dir.path().join("mine"), f.album.join(name)).unwrap();
}

/// Appends 500 bytes to c.bin, which makes it longer than the torrent's.
fn append(f: &Found) {
    let mut c = std::fs::OpenOptions::new().append(true).open(f.album.join("c.bin")).unwrap();
    std::io::Write::write_all(&mut c, &[7; 500]).unwrap();
}

/// What's made of c.bin while it's checked that isn't its own to write: Missing files, held, nothing
/// there changed. That put right, it's checked again and back.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_is_not_its_own_once_checked_is_missing_files_untouched() {
    for how in ["longer", "appended", "linked", "link", "folder linked"] {
        let f = found_and_held(false, |_| {}).await;
        let c = f.album.join("c.bin");
        let other = f.dir.path().join("other");
        match how {
            "longer" => swap(&f, "c.bin", &[7; 2 * PIECE + 500]),
            "appended" => append(&f),
            "linked" => std::fs::hard_link(&c, &other).unwrap(),
            "link" => {
                std::fs::rename(&c, &other).unwrap();
                std::os::unix::fs::symlink(&other, &c).unwrap();
            }
            _ => {
                std::fs::rename(&f.album, &other).unwrap();
                std::os::unix::fs::symlink(&other, &f.album).unwrap();
            }
        }
        let root = if how == "folder linked" { &other } else { &f.album };
        let before = (tree(root), bytes_in(root));
        f.gate.hold(false);
        wait_for(&f.engine, f.id, Status::MissingFiles).await;
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
        let t = find(&f.engine, f.id).await;
        assert_eq!((t.status, t.held), (Status::MissingFiles, Some(proto::Held::Checking)), "{how}");
        assert_eq!((tree(root), bytes_in(root)), before, "{how}: nothing changed");
        // Put right.
        match how {
            "longer" => swap(&f, "c.bin", &f.before.1[Path::new("c.bin")]),
            "appended" => std::fs::OpenOptions::new().write(true).open(&c).unwrap().set_len(2 * PIECE as u64).unwrap(),
            "linked" => std::fs::remove_file(&other).unwrap(),
            "link" => std::fs::rename(&other, &c).unwrap(),
            _ => {
                std::fs::remove_file(&f.album).unwrap();
                std::fs::rename(&other, &f.album).unwrap();
            }
        }
        f.engine.command(f.id, TorrentCommand::Check).await.unwrap();
        wait_for(&f.engine, f.id, Status::Seeding).await;
        assert_eq!(find(&f.engine, f.id).await.held, None, "{how}");
        assert_eq!(bytes_in(&f.album), f.before.1, "{how}");
        f.engine.stop().await;
    }
}

/// Another file of c.bin's length put in its place while it's checked: checked again, so nothing that
/// isn't the torrent's is taken for its pieces.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_swapped_while_checked_is_checked_again() {
    let f = found_and_held(false, |_| {}).await;
    swap(&f, "c.bin", &[7; 2 * PIECE]);
    f.gate.hold(false);
    wait_for(&f.engine, f.id, Status::Downloading).await;
    until_held(&f.engine, f.id, None).await;
    assert_eq!(find(&f.engine, f.id).await.content.unwrap().progress_bytes, 2 * FULL, "none of c.bin");
    f.engine.stop().await;
}

/// Changed as its files are opened to write, after the look before: the add fails, and it's checked
/// again, held, as it finds them. Nothing of the person's is cut.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_changed_as_the_hold_is_lifted_is_left_as_it_is() {
    for how in ["longer", "same", "linked", "gone", "link", "there", "there as made"] {
        let dir = temp();
        let downloads = dir.path().join("downloads");
        let album = downloads.join("Album");
        let once = std::sync::atomic::AtomicBool::new(false);
        let (folder, root) = (album.clone(), dir.path().to_owned());
        let guard = super::super::files::Guard::default().with_hook(Arc::new(move |at| {
            let when = if how == "there as made" { super::super::files::At::Make } else { super::super::files::At::Lift };
            if at != when || once.swap(true, Ordering::SeqCst) {
                return;
            }
            let (c, other) = (folder.join("c.bin"), root.join("other"));
            match how {
                "longer" | "same" => {
                    std::fs::write(&other, vec![7; 2 * PIECE + if how == "longer" { 500 } else { 0 }]).unwrap();
                    std::fs::rename(&other, &c).unwrap();
                }
                "linked" => std::fs::hard_link(&c, &other).unwrap(),
                "gone" => std::fs::rename(&c, &other).unwrap(),
                "there" | "there as made" => std::fs::write(&c, [7; 10]).unwrap(),
                _ => {
                    std::fs::rename(&c, &other).unwrap();
                    std::os::unix::fs::symlink(&other, &c).unwrap();
                }
            }
        }));
        let engine = super::letgo::engine_guarded(dir.path(), guard, Default::default()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::rename(downloads.join("pack"), &album).unwrap();
        if how.starts_with("there") {
            std::fs::remove_file(album.join("c.bin")).unwrap();
        }
        wait_for(&engine, id, Status::MissingFiles).await;
        engine.command(id, rename("Album")).await.unwrap();
        match how {
            "gone" => {
                wait_for(&engine, id, Status::MissingFiles).await;
                tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
                assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking), "{how}");
                assert!(!album.join("c.bin").exists(), "{how}: not made again");
            }
            "longer" | "linked" | "link" => {
                wait_for(&engine, id, Status::MissingFiles).await;
                assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking), "{how}");
                let at = if how == "link" { dir.path().join("other") } else { album.join("c.bin") };
                let len = if how == "longer" { 2 * PIECE + 500 } else { 2 * PIECE };
                assert_eq!(std::fs::metadata(at).unwrap().len(), len as u64, "{how}: not cut");
            }
            // Other bytes, or none: its pieces aren't taken for had.
            _ => {
                wait_for(&engine, id, Status::Downloading).await;
                until_held(&engine, id, None).await;
                assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 2 * FULL, "{how}");
                // One put there is checked before anything is made of it: added once more than else.
                if how == "there as made" {
                    let added = engine.inspect(move |a| a.torrents[&id].added).await;
                    assert!(added >= 3, "{how}: held again, then lifted: {added}");
                }
            }
        }
        engine.stop().await;
    }
}

/// Its folder renamed in Finder while it's checked: Missing files, nothing made where it was. Put back,
/// it's checked again and back as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_folder_renamed_while_checked_is_missing_files_and_not_made_again() {
    let f = found_and_held(false, |_| {}).await;
    let other = f.dir.path().join("downloads/Other");
    std::fs::rename(&f.album, &other).unwrap();
    f.gate.hold(false);
    wait_for(&f.engine, f.id, Status::MissingFiles).await;
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert!(!f.album.exists(), "not made again");
    assert_eq!((tree(&other), bytes_in(&other)), f.before);
    assert_eq!(find(&f.engine, f.id).await.status, Status::MissingFiles);
    std::fs::rename(&other, &f.album).unwrap();
    wait_for(&f.engine, f.id, Status::Seeding).await;
    until_held(&f.engine, f.id, None).await;
    f.engine.stop().await;
}

/// The same across a crash: gone by the next start, it's Missing files, not "Files don't match".
#[tokio::test(flavor = "multi_thread")]
async fn a_found_folder_gone_across_a_crash_is_missing_files() {
    let f = found_and_held(false, |_| {}).await;
    f.engine.kill().await;
    f.gate.hold(false);
    std::fs::rename(&f.album, f.dir.path().join("downloads/Other")).unwrap();
    let engine = engine(f.dir.path()).await;
    wait_for(&engine, f.id, Status::MissingFiles).await;
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert_eq!(find(&engine, f.id).await.held, Some(proto::Held::Checking));
    assert!(!f.album.exists());
    engine.stop().await;
}

/// No race: its check cancelled, or a crash, then bytes appended to a file (and another hard-linked):
/// checked again, none of that is cut off.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_grown_while_held_and_not_checked_is_not_cut() {
    for crash in [false, true] {
        let f = found_and_held(true, |_| {}).await;
        if !crash {
            f.engine.command(f.id, TorrentCommand::CancelCheck).await.unwrap();
        } else {
            f.engine.kill().await;
        }
        f.gate.hold(false);
        append(&f);
        let after;
        let engine = match crash {
            false => {
                std::fs::hard_link(f.album.join("b.bin"), f.dir.path().join("b-linked")).unwrap();
                f.engine.command(f.id, TorrentCommand::Check).await.unwrap();
                &f.engine
            }
            true => {
                after = engine(f.dir.path()).await;
                &after
            }
        };
        let before = (tree(&f.album), bytes_in(&f.album));
        wait_for(engine, f.id, Status::MissingFiles).await;
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
        assert_eq!((tree(&f.album), bytes_in(&f.album)), before, "crash {crash}");
        engine.stop().await;
    }
}

/// A symbolic link at a file's path at a start, while it's held: what it leads to is never read as the
/// torrent's, nor written.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_torrent_reads_nothing_through_a_link() {
    let f = found_and_held(true, |album| {
        for name in ["a.bin", "b.bin"] {
            std::fs::write(album.join(name), vec![9; 2 * PIECE]).unwrap();
        }
    })
    .await;
    f.engine.kill().await;
    f.gate.hold(false);
    let other = f.dir.path().join("other");
    std::fs::rename(f.album.join("c.bin"), &other).unwrap();
    std::os::unix::fs::symlink(&other, f.album.join("c.bin")).unwrap();
    let engine = engine(f.dir.path()).await;
    // None of the torrent's pieces is there but through the link.
    until_held(&engine, f.id, Some(proto::Held::DontMatch)).await;
    assert_eq!(find(&engine, f.id).await.content.unwrap().progress_bytes, 0);
    engine.stop().await;
}

/// The look before Resume lifts "Files don't match" is the same: a file grown, linked or gone since is
/// left as it is.
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_of_files_that_do_not_match_writes_over_none_that_is_not_its_own() {
    for how in ["longer", "linked"] {
        let f = found_and_held(false, other_bytes).await;
        f.gate.hold(false);
        until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
        match how {
            "longer" => append(&f),
            _ => std::fs::hard_link(f.album.join("c.bin"), f.dir.path().join("linked")).unwrap(),
        }
        let before = (tree(&f.album), bytes_in(&f.album));
        let _ = f.engine.command(f.id, TorrentCommand::Resume).await;
        wait_for(&f.engine, f.id, Status::MissingFiles).await;
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
        assert_eq!((tree(&f.album), bytes_in(&f.album)), before, "{how}");
        f.engine.stop().await;
    }
    // Other bytes since: checked again first, and the person asked again.
    let f = found_and_held(false, other_bytes).await;
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    swap(&f, "c.bin", &[8; 2 * PIECE]);
    let before = (tree(&f.album), bytes_in(&f.album));
    f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    wait_for(&f.engine, f.id, Status::Paused).await;
    assert_eq!((tree(&f.album), bytes_in(&f.album)), before);
    f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
    until_held(&f.engine, f.id, None).await;
    f.engine.stop().await;
}

/// A file that changes at every check: Unchecked after a few, held, rather than checked for good.
#[tokio::test(flavor = "multi_thread")]
async fn files_that_never_stay_as_they_are_are_left_unchecked() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let album = downloads.join("Album");
    let (on, c) = (Arc::new(std::sync::atomic::AtomicBool::new(false)), album.join("c.bin"));
    let going = on.clone();
    let guard = super::super::files::Guard::default().with_hook(Arc::new(move |at| {
        if at == super::super::files::At::Read && going.load(Ordering::SeqCst) {
            let _ = std::fs::OpenOptions::new().append(true).open(&c).map(|f| f.set_modified(std::time::SystemTime::now()));
        }
    }));
    let engine = super::letgo::engine_guarded(dir.path(), guard, Default::default()).await;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::rename(downloads.join("pack"), &album).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    on.store(true, Ordering::SeqCst);
    engine.command(id, rename("Album")).await.unwrap();
    wait_for(&engine, id, Status::Unchecked).await;
    assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking));
    on.store(false, Ordering::SeqCst);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// Pieces in files set Off count, all of them Off or only the Normal one not matching: the hold lifts.
#[tokio::test(flavor = "multi_thread")]
async fn pieces_in_files_set_off_match_too() {
    for all in [true, false] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let off: &[&str] = if all { &["a.bin", "b.bin", "c.bin"] } else { &["a.bin", "b.bin"] };
        set(&engine, id, off, proto::Priority::Off).await;
        let album = downloads.join("Album");
        std::fs::rename(downloads.join("pack"), &album).unwrap();
        if !all {
            std::fs::write(album.join("c.bin"), vec![9u8; 2 * PIECE]).unwrap();
            wait_for(&engine, id, Status::MissingFiles).await;
        }
        let answer = engine.commanded(vec![id], rename("Album")).await.unwrap();
        assert_eq!(answer.found, vec![id], "all {all}");
        until_held(&engine, id, None).await;
        wait_for(&engine, id, if all { Status::Seeding } else { Status::Downloading }).await;
        engine.stop().await;
    }
}

/// A file set Off that isn't its own to write (longer, or with another hard link), the rest its pieces:
/// Missing files, held, as a wanted file in that state is: no "Files don't match", nothing written,
/// and Resume doesn't get past it; put right, it's checked again and lifts by itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_that_is_not_its_own_is_missing_files_held() {
    for how in ["longer", "linked"] {
        let f = found_and_held(false, |_| {}).await;
        set(&f.engine, f.id, &["c.bin"], proto::Priority::Off).await;
        let other = f.dir.path().join("other");
        match how {
            "longer" => append(&f),
            _ => std::fs::hard_link(f.album.join("c.bin"), &other).unwrap(),
        }
        let before = (tree(&f.album), bytes_in(&f.album));
        f.gate.hold(false);
        wait_for(&f.engine, f.id, Status::MissingFiles).await;
        f.engine.command(f.id, TorrentCommand::Resume).await.unwrap();
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
        let t = find(&f.engine, f.id).await;
        assert_eq!((t.status, t.held), (Status::MissingFiles, Some(proto::Held::Checking)), "{how}");
        assert_eq!((tree(&f.album), bytes_in(&f.album)), before, "{how}: nothing written");
        match how {
            "longer" => std::fs::OpenOptions::new().write(true).open(f.album.join("c.bin")).unwrap().set_len(2 * PIECE as u64).unwrap(),
            _ => std::fs::remove_file(&other).unwrap(),
        }
        until_held(&f.engine, f.id, None).await;
        f.engine.stop().await;
    }
}

/// The same file, with no piece found at all: it's "Files don't match", as ever.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_that_is_not_its_own_with_no_piece_found_is_files_that_do_not_match() {
    let f = found_and_held(false, other_bytes).await;
    set(&f.engine, f.id, &["c.bin"], proto::Priority::Off).await;
    append(&f);
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    wait_for(&f.engine, f.id, Status::Paused).await;
    f.engine.stop().await;
}

/// A file that isn't there, set Normal while it's held (or once its files didn't match), isn't made
/// there; the lift makes it.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_normal_while_held_is_made_only_once_the_hold_is_lifted() {
    for (others, dont_match) in [(true, true), (true, false), (false, false)] {
        let dir = temp();
        let (engine, gate, _reads, _opens) = reading_engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        set(&engine, id, &["c.bin"], proto::Priority::Off).await;
        let album = downloads.join("Album");
        std::fs::rename(downloads.join("pack"), &album).unwrap();
        std::fs::remove_file(album.join("c.bin")).unwrap();
        for name in ["a.bin", "b.bin"].into_iter().filter(|_| others) {
            std::fs::write(album.join(name), vec![9; 2 * PIECE]).unwrap();
        }
        wait_for(&engine, id, Status::MissingFiles).await;
        let before = tree(&album);
        gate.hold(true);
        engine.command(id, rename("Album")).await.unwrap();
        until(&engine, id, |s| matches!(s, Status::Checking)).await;
        if dont_match {
            gate.hold(false);
            until_held(&engine, id, Some(proto::Held::DontMatch)).await;
        }
        set(&engine, id, &["c.bin"], proto::Priority::Normal).await;
        assert_eq!(tree(&album), before, "others {others}, dont_match {dont_match}");
        gate.hold(false);
        match others {
            true => until_held(&engine, id, Some(proto::Held::DontMatch)).await,
            false => until_held(&engine, id, None).await,
        }
        if others {
            tokio::time::sleep(SIZE_CHECK_EVERY * 2).await;
            assert_eq!(tree(&album), before, "others {others}, dont_match {dont_match}: checked");
        } else {
            wait_for(&engine, id, Status::Downloading).await;
            assert_eq!(std::fs::metadata(album.join("c.bin")).unwrap().len(), 2 * PIECE as u64, "made, to its length");
        }
        engine.stop().await;
    }
}

/// A file moved to the Trash while it's held is read where it went, after a restart too: its undo
/// brings back all of it, nothing downloaded again.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_trashed_while_held_is_read_where_it_went_after_a_restart() {
    let f = found_and_held(false, |_| {}).await;
    let c = file_index(&f.engine, f.id, "c.bin").await;
    f.engine.command(f.id, TorrentCommand::TrashFiles { files: vec![c] }).await.unwrap();
    f.engine.kill().await;
    f.gate.hold(false);
    let engine = engine(f.dir.path()).await;
    until_held(&engine, f.id, None).await;
    wait_for(&engine, f.id, Status::Seeding).await;
    assert!(!f.album.join("c.bin").exists());
    engine.undo().await.unwrap();
    wait_for(&engine, f.id, Status::Seeding).await;
    assert_eq!(find(&engine, f.id).await.content.unwrap().progress_bytes, 3 * FULL);
    assert_eq!(bytes_in(&f.album), f.before.1);
    engine.stop().await;
}

/// Bytes appended once the lift looked at the file, before librqbit sets its length: never cut off.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_grown_as_its_length_is_set_is_not_cut() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let album = downloads.join("Album");
    let lifting = std::sync::atomic::AtomicBool::new(false);
    let c = album.join("c.bin");
    let guard = super::super::files::Guard::default().with_hook(Arc::new(move |at| match at {
        super::super::files::At::Lift => lifting.store(true, Ordering::SeqCst),
        super::super::files::At::Change if lifting.swap(false, Ordering::SeqCst) => {
            let mut c = std::fs::OpenOptions::new().append(true).open(&c).unwrap();
            std::io::Write::write_all(&mut c, &[7; 500]).unwrap();
        }
        _ => {}
    }));
    let engine = super::letgo::engine_guarded(dir.path(), guard, Default::default()).await;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, true).await.unwrap().id;
    wait_for(&engine, id, Status::Done { reached: None }).await;
    std::fs::rename(downloads.join("pack"), &album).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.command(id, rename("Album")).await.unwrap();
    until_held(&engine, id, None).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(std::fs::metadata(album.join("c.bin")).unwrap().len(), 2 * PIECE as u64 + 500);
    engine.stop().await;
}

/// Its check over and not judged yet (a Move to Trash runs on it), c.bin's bytes change: in place, its
/// modification time put back, so only its change time tells; or, across a crash, as another file. The
/// pieces found aren't taken for those bytes: it's checked again.
#[tokio::test(flavor = "multi_thread")]
async fn pieces_found_are_not_taken_for_bytes_changed_before_the_hold_is_judged() {
    for crash in [false, true] {
        let dir = temp();
        let downloads = dir.path().join("downloads");
        let gate = Arc::new(super::super::files::Gate::default());
        let _opens = Opens(gate.clone());
        let trash: super::super::trash::Gate = Default::default();
        let opens_trash = OpensTrash(trash.clone());
        let guard = super::super::files::Guard::default().with_gate(super::super::files::At::Read, gate.clone());
        let engine_then = super::letgo::engine_guarded(dir.path(), guard, trash.clone()).await;
        let id = add(&engine_then, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine_then, id, Status::Seeding).await;
        let hash = find(&engine_then, id).await.info_hash;
        let a = file_index(&engine_then, id, "a.bin").await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
        wait_for(&engine_then, id, Status::MissingFiles).await;
        gate.hold(true);
        engine_then.command(id, rename("Album")).await.unwrap();
        until(&engine_then, id, |s| matches!(s, Status::Checking)).await;
        super::super::trash::hold(&trash, true);
        let _trashing = engine_then.sent(id, TorrentCommand::TrashFiles { files: vec![a] }).await;
        held_at_the_gate(&trash).await;
        gate.hold(false);
        for _ in 0..500 {
            if super::letgo::saved_pieces(dir.path(), &hash) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(super::letgo::saved_pieces(dir.path(), &hash) > 0, "its check is over, and saved");
        let c = downloads.join("Album/c.bin");
        let engine = match crash {
            true => {
                engine_then.kill().await;
                drop(opens_trash);
                std::fs::write(dir.path().join("mine"), vec![7; 2 * PIECE]).unwrap();
                std::fs::rename(dir.path().join("mine"), &c).unwrap();
                engine(dir.path()).await
            }
            false => {
                let file = std::fs::OpenOptions::new().write(true).open(&c).unwrap();
                let was = file.metadata().unwrap().modified().unwrap();
                std::os::unix::fs::FileExt::write_all_at(&file, &[7; 2 * PIECE], 0).unwrap();
                file.set_modified(was).unwrap();
                super::super::trash::hold(&trash, false);
                engine_then
            }
        };
        until_held(&engine, id, None).await;
        wait_for(&engine, id, Status::Downloading).await;
        let c = file_index(&engine, id, "c.bin").await;
        assert_eq!(engine.details(id).await.unwrap().files[c].progress_bytes, 0, "none of it is the torrent's");
        engine.stop().await;
    }
}

// A piece of zeros proves nothing: only a matching piece with real (non-zero) content lifts the hold.

/// A .torrent of `files` (name, bytes) under `dir/downloads/pack`, which it writes there.
async fn torrent_of(dir: &Path, files: &[(&str, &[u8])]) -> AddSource {
    let pack = dir.join("downloads/pack");
    std::fs::create_dir_all(&pack).unwrap();
    for (name, bytes) in files {
        std::fs::write(pack.join(name), bytes).unwrap();
    }
    let options = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec!["http://127.0.0.1:1/announce".into()],
        piece_length: Some(PIECE as u32),
    };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    AddSource::TorrentFile(librqbit::create_torrent(&pack, options, &spawner).await.unwrap().as_bytes().unwrap().to_vec())
}

/// A torrent of `files`, complete, its folder renamed in Finder to "Album" with `there` in it instead
/// (a file of the same name, other bytes), then found by Rename…. Returns once it's judged.
async fn found_with(files: &[(&str, &[u8])], there: &[(&str, &[u8])]) -> (TempDir, Engine, u64, PathBuf) {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, torrent_of(dir.path(), files).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let album = downloads.join("Album");
    std::fs::rename(downloads.join("pack"), &album).unwrap();
    for (name, bytes) in there {
        std::fs::write(album.join(name), bytes).unwrap();
    }
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(engine.commanded(vec![id], rename("Album")).await.unwrap().found, vec![id]);
    (dir, engine, id, album)
}

/// Another file that shares only a zero piece with the torrent: no real piece matches, so it's "Files
/// don't match", stopped, and nothing there changed (bytes, lengths, times). Across a restart too: the
/// check is made again, held, and says the same.
#[tokio::test(flavor = "multi_thread")]
async fn another_file_that_shares_only_a_zero_piece_does_not_match() {
    let (zero, real, other) = (vec![0u8; PIECE], vec![7u8; PIECE], vec![9u8; PIECE]);
    let theirs = [zero.clone(), other.clone()].concat();
    let ours = [zero, real.clone()].concat();
    let (dir, engine, id, album) = found_with(&[("a.bin", &ours), ("b.bin", &real)], &[("a.bin", &theirs), ("b.bin", &other)]).await;
    let before = (tree(&album), bytes_in(&album));
    until_held(&engine, id, Some(proto::Held::DontMatch)).await;
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    let t = find(&engine, id).await;
    assert_eq!((t.status, t.held), (Status::Paused, Some(proto::Held::DontMatch)));
    assert_eq!((tree(&album), bytes_in(&album)), before, "nothing changed");
    engine.kill().await;
    let engine = super::engine(dir.path()).await;
    until(&engine, id, |s| matches!(s, Status::Paused)).await;
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert_eq!(find(&engine, id).await.held, Some(proto::Held::DontMatch), "across a crash");
    assert_eq!((tree(&album), bytes_in(&album)), before);
    // Resume is the way out, and the zero piece it matched is kept as good as any.
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until_held(&engine, id, None).await;
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, PIECE as u64);
    engine.stop().await;
}

/// The torrent's own content, with zero pieces and one real one: it lifts, with all of them.
#[tokio::test(flavor = "multi_thread")]
async fn the_torrents_own_content_with_zero_pieces_and_a_real_one_lifts_with_all_of_them() {
    let (a, b) = ([vec![0u8; PIECE], vec![7u8; PIECE]].concat(), vec![0u8; PIECE]);
    let (_dir, engine, id, _album) = found_with(&[("a.bin", &a), ("b.bin", &b)], &[]).await;
    until_held(&engine, id, None).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 3 * PIECE as u64);
    engine.stop().await;
}

/// A piece across two files counts if either part is real; the short last piece counts as any.
#[tokio::test(flavor = "multi_thread")]
async fn a_piece_across_files_and_the_short_last_piece_count_by_their_own_bytes() {
    let half = PIECE / 2;
    // a.bin | b.bin: piece 0 is half zeros, half real; piece 1 is the short last one: zeros.
    let (a, b) = (vec![0u8; half], [vec![7u8; half], vec![0u8; 100]].concat());
    let (_dir, engine, id, _album) = found_with(&[("a.bin", &a), ("b.bin", &b)], &[]).await;
    until_held(&engine, id, None).await;
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;

    // The same where the person's file has zeros in the same pieces and others elsewhere: only zeros match.
    let (a, b) = (vec![0u8; half], vec![0u8; half + 100]);
    let torn = [vec![7u8; half], vec![1u8; 100]].concat();
    let (_dir, engine, id, album) = found_with(&[("a.bin", &a), ("b.bin", &torn)], &[("a.bin", &a), ("b.bin", &b)]).await;
    let before = (tree(&album), bytes_in(&album));
    until_held(&engine, id, Some(proto::Held::DontMatch)).await;
    assert_eq!((tree(&album), bytes_in(&album)), before);
    engine.stop().await;
}
