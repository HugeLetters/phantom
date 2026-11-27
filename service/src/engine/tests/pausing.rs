//! #375: a torrent paused while a piece it just got is being hashed still gets that piece once it's
//! resumed.

use super::*;

/// Paused and resumed four times, each time with a piece's hash under way, which ends before the
/// Resume (`ends_first`) or only after it: the torrent finishes by itself.
async fn finishes(ends_first: bool) {
    let dir = temp();
    let pack = dir.path().join("seed/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let bytes = |len: u32, by: u32| (0..len).map(|n| (n.wrapping_mul(by) >> 13) as u8).collect::<Vec<u8>>();
    let a = bytes(4 << 20, 2_654_435_761);
    std::fs::write(pack.join("a.bin"), &a).unwrap();
    std::fs::write(pack.join("b.bin"), bytes(1 << 20, 2_246_822_519)).unwrap();
    let (seeder, torrent, _, _) = seed(dir.path(), &pack, 32 << 10).await;
    // So it isn't all had before the last pause.
    seeder.ratelimits.set_upload_bps(std::num::NonZeroU32::new(1 << 20));
    let gate = Arc::new(super::super::files::Gate::default());
    let engine = engine_gated(dir.path(), &gate).await;
    let _opens = Opens(gate.clone());
    let downloads = dir.path().join("downloads");
    let id = add_made(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    let off = index_of(&engine, id, "b.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![off], priority: Priority::Off }).await.unwrap();
    let added = librqbit_torrent(&engine, id).await;

    // Every status and progress a client could see from here on.
    let mut snapshots = engine.subscribe();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seeing = tokio::spawn({
        let seen = seen.clone();
        async move {
            while snapshots.changed().await.is_ok() {
                let row = snapshots.borrow_and_update().for_client(&[], &[], None).torrents.iter().find(|t| t.id == id).cloned();
                let row = row.expect("it's listed throughout");
                seen.lock().unwrap().push((row.status, row.content.unwrap().progress_bytes));
            }
        }
    });

    for round in 0..4 {
        gate.hold(true);
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        for _ in 0..1000 {
            if gate.waiting() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(gate.waiting(), "a piece is being hashed ({round}): {:?}", find(&engine, id).await);
        engine.command(id, TorrentCommand::Pause).await.unwrap();
        if ends_first {
            gate.hold(false);
            while gate.waiting() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        let had = engine.inspect(move |a| a.torrents[&id].lib.torrent().map_or(0, |h| h.stats().progress_bytes)).await;
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        gate.hold(false);
        // Some pieces are had before the next one.
        for _ in 0..1000 {
            if engine.inspect(move |a| a.torrents[&id].lib.torrent().is_some_and(|h| h.stats().progress_bytes > had)).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        engine.command(id, TorrentCommand::Pause).await.unwrap();
    }
    seeder.ratelimits.set_upload_bps(None);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until(&engine, id, |s| *s == Status::Seeding).await;

    assert_eq!(std::fs::read(downloads.join("pack/a.bin")).unwrap(), a);
    // Made as it was added, before it was set Off.
    let b = std::fs::read(downloads.join("pack/b.bin")).unwrap();
    assert!(b.iter().all(|byte| *byte == 0), "nothing of the file set Off is got");
    assert_eq!(librqbit_torrent(&engine, id).await, added, "neither checked nor added again");
    assert_eq!(librqbit_only_files(&engine, id).await, Some(BTreeSet::from([1 - off])));
    seeing.abort();
    let seen = seen.lock().unwrap().clone();
    let last = seen.len() - 1;
    for (i, (status, progress)) in seen.iter().enumerate() {
        let expected =
            matches!(status, Status::Downloading | Status::Paused) || (*status == Status::Seeding && *progress == a.len() as u64);
        assert!(expected, "{status:?} at {progress} ({i} of {last})");
    }
    assert!(seen.windows(2).all(|w| w[0].1 <= w[1].1), "what's had is kept: {seen:?}");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_piece_whose_hash_ends_while_the_torrent_is_paused_is_got_after_it() {
    finishes(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_piece_whose_hash_ends_after_the_torrent_is_resumed_is_got_again() {
    finishes(false).await;
}
