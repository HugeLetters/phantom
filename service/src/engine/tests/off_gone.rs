//! #402: a file set Off and gone from its path, none of which was had, set Normal again downloads
//! from nothing into the file at its path, not into the one librqbit has open elsewhere. Every Trash
//! here is the test's own folder.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gone {
    /// Moved out of its folder by hand, whole.
    Hand,
    Trash,
}

struct Case {
    gone: Gone,
    running: bool,
    restart: bool,
    /// `a.bin` is there as the torrent is added, and shares a piece with `b.bin`, whichever of them
    /// comes first in the torrent. Else neither is there, and they share none.
    shared: bool,
    /// A check is under way as it's set Normal, held at its reads.
    checking: bool,
}

const PIECE: usize = 16384;

/// Every item under `dir`, by its bytes.
fn contents(dir: &Path) -> Vec<Vec<u8>> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| std::fs::read(e.path()).ok()).collect()
}

async fn run(case: Case) {
    let dir = temp();
    let dir = dir.path();
    let pack = dir.join("seed/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let bytes = |len: usize, by: u32| (0..len as u32).map(|n| (n.wrapping_mul(by) >> 13) as u8).collect::<Vec<u8>>();
    let half = if case.shared { PIECE / 2 } else { 0 };
    let (a, b) = (bytes(3 * PIECE + half, 2_654_435_761), bytes(4 * PIECE + half, 2_246_822_519));
    std::fs::write(pack.join("a.bin"), &a).unwrap();
    std::fs::write(pack.join("b.bin"), &b).unwrap();
    let (_seeder, torrent, hash, _) = seed(dir, &pack, PIECE as u32).await;
    let downloads = dir.join("downloads");
    let path = downloads.join("pack/b.bin");
    std::fs::create_dir_all(downloads.join("pack")).unwrap();
    if case.shared {
        std::fs::write(downloads.join("pack/a.bin"), &a).unwrap();
    }

    let gate = Arc::new(super::super::files::Gate::default());
    let mut engine = engine_gated(dir, &gate).await;
    let _opens = Opens(gate.clone());
    let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    let (first, file) = (index_of(&engine, id, "a.bin").await, index_of(&engine, id, "b.bin").await);
    let files = vec![file];
    if case.gone == Gone::Hand {
        engine.command(id, TorrentCommand::SetPriority { files: files.clone(), priority: Priority::Off }).await.unwrap();
        std::fs::create_dir_all(dir.join("theirs")).unwrap();
        std::fs::rename(&path, dir.join("theirs/b.bin")).unwrap();
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    } else {
        engine.command(id, TorrentCommand::TrashFiles { files: files.clone() }).await.unwrap();
    }
    if case.restart {
        engine.stop().await;
        engine = engine_gated(dir, &gate).await;
        until(&engine, id, |s| matches!(s, Status::Paused | Status::Done { .. })).await;
    }
    let normal = TorrentCommand::SetPriority { files, priority: Priority::Normal };
    if case.running {
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        until(&engine, id, |s| *s == Status::Seeding || *s == Status::Downloading).await;
    }
    if case.checking {
        gate.hold(true);
        engine.command(id, TorrentCommand::Check).await.unwrap();
        for _ in 0..1000 {
            if gate.waiting() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(gate.waiting(), "its check is under way");
        engine.command(id, normal).await.unwrap();
        gate.hold(false);
    } else {
        engine.command(id, normal).await.unwrap();
    }
    if !case.running {
        // Long enough for what's let go of to be.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let whole = if case.shared { 3 * PIECE as u64 } else { 0 };
        let has = engine.inspect(move |t| t.torrents[&id].lib.torrent().map(|h| h.stats().file_progress).unwrap()).await;
        assert_eq!((has[first], has[file]), (whole, 0), "the neighbour's pieces stay had, the file's aren't");
        assert_eq!(super::letgo::saved_pieces(dir, &hash), whole / PIECE as u64);
    }
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until(&engine, id, |s| *s == Status::Seeding).await;
    for _ in 0..200 {
        if std::fs::read(&path).unwrap() == b {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let now = std::fs::read(&path).unwrap();
    assert!(now == b, "Seeding, its file isn't the torrent's: {} bytes, zeros: {}", now.len(), now.iter().all(|byte| *byte == 0));
    assert_eq!(std::fs::read(downloads.join("pack/a.bin")).unwrap(), a);
    // What was taken away is as it went: the zeros librqbit made it with.
    let went = contents(&dir.join(if case.gone == Gone::Hand { "theirs" } else { "trash" }));
    assert!(!went.is_empty() && went.iter().all(|w| w.iter().all(|byte| *byte == 0)), "what went is as it was");
    engine.stop().await;
}

macro_rules! cases {
    ($($name:ident: $gone:ident, running $running:literal, restart $restart:literal, shared $shared:literal, checking $checking:literal;)*) => { $(
        #[tokio::test(flavor = "multi_thread")]
        async fn $name() {
            run(Case { gone: Gone::$gone, running: $running, restart: $restart, shared: $shared, checking: $checking }).await;
        }
    )* };
}

cases! {
    moved_by_hand_set_normal_while_stopped: Hand, running false, restart false, shared false, checking false;
    in_the_trash_set_normal_while_stopped: Trash, running false, restart false, shared false, checking false;
    moved_by_hand_set_normal_while_stopped_after_a_restart: Hand, running false, restart true, shared false, checking false;
    in_the_trash_set_normal_while_stopped_after_a_restart: Trash, running false, restart true, shared false, checking false;
    moved_by_hand_set_normal_while_running: Hand, running true, restart false, shared false, checking false;
    in_the_trash_set_normal_while_running: Trash, running true, restart false, shared false, checking false;
    in_the_trash_set_normal_while_running_after_a_restart: Trash, running true, restart true, shared false, checking false;
    moved_by_hand_set_normal_while_stopped_its_neighbour_had: Hand, running false, restart false, shared true, checking false;
    in_the_trash_set_normal_while_stopped_its_neighbour_had: Trash, running false, restart false, shared true, checking false;
    moved_by_hand_set_normal_during_a_check: Hand, running false, restart false, shared false, checking true;
    in_the_trash_set_normal_during_a_check: Trash, running false, restart false, shared false, checking true;
}
