//! #374: a write that comes late, through a storage librqbit already took (at a pause), into a file
//! in the Trash for an undo is refused, as for every other file. librqbit then has the piece no
//! more than before: it gets it again once resumed, and what's kept for the undo is what that write
//! of the storage it has now brought.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::super::files::{At, Gate};
use super::*;

const PIECE_LENGTH: u32 = 32 << 10;
/// Each file: half of the torrent's one piece, a chunk.
const HALF: usize = 16 << 10;

/// The torrent's one piece, written late into its first file, in the Trash, as the torrent is
/// paused: refused, so nothing is kept through the storage taken, and it isn't had until resumed.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_late_write_into_a_file_in_the_trash_leaves_its_piece_to_be_got_again() {
    let dir = temp();
    let pack = dir.path().join("seed/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let bytes = |by: u32| (0..HALF as u32).map(|n| (n.wrapping_mul(by) >> 13) as u8).collect::<Vec<u8>>();
    std::fs::write(pack.join("a.bin"), bytes(2_654_435_761)).unwrap();
    std::fs::write(pack.join("b.bin"), bytes(2_246_822_519)).unwrap();
    let (_seeder, torrent, _, _) = seed(dir.path(), &pack, PIECE_LENGTH).await;
    // The person's own files, none of them the torrent's.
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(downloads.join("pack")).unwrap();
    for name in ["a.bin", "b.bin"] {
        std::fs::write(downloads.join("pack").join(name), vec![7; HALF]).unwrap();
    }

    let gate = Arc::new(Gate::default());
    let (armed, kept) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
    let guard = super::super::files::Guard::default().with_hook(Arc::new({
        let (gate, armed, kept) = (gate.clone(), armed.clone(), kept.clone());
        move |at| match at {
            At::Change if armed.swap(false, Ordering::SeqCst) => gate.wait(),
            At::Keep => drop(kept.fetch_add(1, Ordering::SeqCst)),
            _ => {}
        }
    }));
    let _opens = Opens(gate.clone());
    let engine = engine_guarded(dir.path(), guard).await;
    let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    // In the torrent's order, which is the folder's as librqbit read it.
    let names: Vec<_> = engine.details(id).await.unwrap().files.iter().map(|f| f.path.file_name().unwrap().to_owned()).collect();
    let [first, second] = [0, 1].map(|n| (downloads.join("pack").join(&names[n]), std::fs::read(pack.join(&names[n])).unwrap()));
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    assert!(!first.0.exists(), "in the Trash");
    let parts = || super::moving::bytes_in(&dir.path().join("data/undo-parts"));
    let had = async || engine.details(id).await.unwrap().files[1].progress_bytes;

    // The first chunk, the first file's, is the first write.
    gate.hold(true);
    armed.store(true, Ordering::SeqCst);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for_that("a write into the first file is held", || gate.waiting()).await;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    wait_for(&engine, id, Status::Paused).await;
    gate.hold(false);
    wait_for_that("the write goes on", || !gate.waiting()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(kept.load(Ordering::SeqCst), 0, "nothing kept through the storage taken");
    assert!(parts().is_empty(), "{:?}", parts().keys());
    assert_eq!(had().await, 0, "the piece isn't had");

    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(std::fs::read(&second.0).unwrap(), second.1);
    assert_eq!(had().await, HALF as u64);
    let content = find(&engine, id).await.content.unwrap();
    assert_eq!((content.progress_bytes, content.size_bytes), (HALF as u64, HALF as u64));
    // File 0's, at 0.
    let kept_now: Vec<_> = parts().into_iter().map(|(path, bytes)| (path.ends_with("0/0"), bytes)).collect();
    assert!(kept_now == [(true, first.1.clone())], "kept for the undo: the first file's part, as got");

    engine.undo().await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert!(std::fs::read(&first.0).unwrap() == first.1, "back, as got");
    engine.stop().await;
}

async fn wait_for_that(what: &str, met: impl Fn() -> bool) {
    for _ in 0..3000 {
        if met() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never: {what}");
}
