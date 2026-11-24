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
    let init_in: super::super::files::InitIn = Arc::new(move |destination, init| {
        kept.lock().unwrap().push(tree(&folder));
        super::super::files::init_in(destination, init)
    });
    let engine = super::letgo::engine_guarded(dir, super::super::files::Guard::new(init_in), Default::default()).await;
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
/// with nothing touched meanwhile, then runs as it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_found_torrent_held_for_its_check_comes_back_held_after_a_crash() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let (id, before) = {
        let (engine, gate, _reads, _opens) = reading_engine(dir.path()).await;
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::rename(downloads.join("pack"), downloads.join("Album")).unwrap();
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
    wait_for(&engine, id, Status::Seeding).await;
    let t = find(&engine, id).await;
    assert_eq!((t.held, t.content.unwrap().progress_bytes), (None, 3 * FULL));
    assert!(reads.load(Ordering::SeqCst) >= 6, "checked in full");
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
    // As the check waits to read: what's asked of it waits for that.
    let cancelling = f.engine.sent(f.id, TorrentCommand::CancelCheck).await;
    f.gate.hold(false);
    cancelling.await.unwrap();
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
        let f = found_and_held(false, |album| if others { other_bytes(album) }).await;
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

/// Deleted while it's held, as the person asked: its files go to the Trash as they are, what's the
/// person's stays, and ⌘Z brings them back, held.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_while_held_moves_the_files_as_they_are_and_its_undo_brings_it_back_held() {
    let f = found_and_held(false, |album| {
        other_bytes(album);
        std::fs::write(album.join("mine.txt"), b"mine").unwrap();
    })
    .await;
    let deleting = f.engine.sent(f.id, TorrentCommand::Delete).await;
    deleting.await.unwrap();
    assert_eq!(bytes_in(&f.album).into_keys().collect::<Vec<_>>(), [PathBuf::from("mine.txt")]);
    f.engine.undo().await.unwrap();
    until(&f.engine, f.id, |s| matches!(s, Status::Checking)).await;
    assert_eq!(find(&f.engine, f.id).await.held, Some(proto::Held::Checking));
    f.gate.hold(false);
    until_held(&f.engine, f.id, Some(proto::Held::DontMatch)).await;
    wait_for(&f.engine, f.id, Status::Paused).await;
    assert!(f.untouched(), "back as they were");
    f.engine.stop().await;
}

/// Moved to another folder while it's held, the files go there as they are and it's still held there:
/// checked, then stopped or back as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_while_held_takes_the_files_as_they_are_and_it_is_held_there() {
    for others in [false, true] {
        let f = found_and_held(false, |album| if others { other_bytes(album) }).await;
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
    wait_for(&engine, id, Status::MissingFiles).await;
    gate.hold(true);
    engine.command(id, rename("Album")).await.unwrap();
    until(&engine, id, |s| matches!(s, Status::Checking)).await;
    super::super::trash::hold(&trash, true);
    let trashing = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&trash).await;
    gate.hold(false);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(find(&engine, id).await.held, Some(proto::Held::Checking), "under the Move to Trash");
    super::super::trash::hold(&trash, false);
    trashing.await.unwrap();
    until_held(&engine, id, None).await;
    assert_eq!(bytes_in(&downloads.join("Album")).len(), 2, "one file in the Trash, not made again");
    engine.stop().await;
}
