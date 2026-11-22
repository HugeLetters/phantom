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
