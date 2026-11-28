//! #385: a file a torrent had, or Phantom noted (#374), that's absent when its destination comes
//! back makes it Missing files, as a file that goes while the destination is there does. The check
//! isn't to make it again, nor the torrent to download it, until Resume. A file never had nor noted
//! (the torrent was never let run where its drive is) is made as before. Ported from #389, which
//! told the two apart with a flag in the record; the notes tell them now.

use super::*;

/// When the engine starts, if it does, while the destination is away.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Started {
    No,
    /// Away as it stopped: librqbit's pieces of it went as it was taken out.
    AwayAsItStopped,
    /// Away only since it stopped: librqbit's pieces of it are still saved.
    AwaySinceItStopped,
}

const STARTED: [Started; 3] = [Started::No, Started::AwayAsItStopped, Started::AwaySinceItStopped];

fn is_away(s: &Status) -> bool {
    matches!(s, Status::DestinationUnavailable { .. })
}

/// The destination goes, `edit` changes what's in it (`pack` is the torrent's folder there), and it
/// comes back. The engine, and how many times librqbit had the torrent while it was away.
async fn away_and_back(dir: &Path, mut engine: Engine, id: u64, started: Started, edit: impl FnOnce(&Path)) -> (Engine, u32) {
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    if started == Started::AwaySinceItStopped {
        engine.stop().await;
    }
    std::fs::rename(&here, &away).unwrap();
    if started == Started::AwayAsItStopped {
        until(&engine, id, is_away).await;
        engine.stop().await;
    }
    if started != Started::No {
        engine = super::engine(dir).await;
    }
    until(&engine, id, is_away).await;
    let added = librqbit_torrent(&engine, id).await;
    edit(&away.join("pack"));
    std::fs::rename(&away, &here).unwrap();
    until(&engine, id, |s| !is_away(s)).await;
    // What the size check that saw it back decided, and what that started, has run.
    settled(&engine).await;
    (engine, added)
}

fn whole(seed: usize) -> Vec<u8> {
    (0..4 * PIECE).map(|n| (n % 241) as u8 ^ seed as u8).collect()
}

const NAMES: [&str; 3] = ["a.bin", "b.bin", "c.bin"];

/// A torrent of three files, whole in the download dir, and a peer that has them too.
async fn peered(dir: &Path) -> (std::sync::Arc<librqbit::Session>, AddSource) {
    let (seeded, pack) = (dir.join("seed/pack"), dir.join("downloads/pack"));
    std::fs::create_dir_all(&seeded).unwrap();
    std::fs::create_dir_all(&pack).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        std::fs::write(seeded.join(name), whole(i + 1)).unwrap();
        std::fs::write(pack.join(name), whole(i + 1)).unwrap();
    }
    let (seeder, torrent, _, _) = seed(dir, &seeded, PIECE as u32).await;
    (seeder, AddSource::TorrentFile(torrent))
}

/// Missing files, listing exactly the absent ones, with nothing made again; each of them put back is
/// what lets it go on.
#[tokio::test(flavor = "multi_thread")]
async fn files_absent_as_its_destination_is_back_are_missing_files_and_nothing_makes_them() {
    let cases = STARTED.into_iter().flat_map(|s| [(s, false, vec![1]), (s, true, vec![1]), (s, false, vec![0, 2])]);
    side_by_side(4, cases, |(started, paused, absent)| async move {
        let case = format!("{started:?}, paused {paused}, absent {absent:?}");
        let temp = temp();
        let dir = temp.path();
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        if paused {
            engine.command(id, TorrentCommand::Pause).await.unwrap();
        }
        let own: Vec<Vec<u8>> = NAMES.iter().map(|n| std::fs::read(file(dir, n)).unwrap()).collect();
        let (engine, added) = away_and_back(dir, engine, id, started, |pack| {
            for &f in &absent {
                std::fs::remove_file(pack.join(NAMES[f])).unwrap();
            }
        })
        .await;
        assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
        let files = files_on(&engine, id).await;
        let missing: Vec<usize> = (0..3).filter(|&f| files[f].2).collect();
        assert_eq!(missing, absent, "{case}: exactly the absent ones are listed");
        for f in 0..3 {
            assert_eq!(file(dir, NAMES[f]).exists(), !absent.contains(&f), "{case}: {} isn't made", NAMES[f]);
        }
        assert_eq!(librqbit_torrent(&engine, id).await, added, "{case}: not added, so not checked and nothing made");
        for &f in &absent {
            std::fs::write(file(dir, NAMES[f]), &own[f]).unwrap();
        }
        let done = if paused { Status::Done { reached: None } } else { Status::Seeding };
        wait_for(&engine, id, done).await;
        engine.stop().await;
    })
    .await;
}

/// Resume checks the rest in full (what was saved of it is forgotten) and downloads only what's
/// missing, from a peer.
#[tokio::test(flavor = "multi_thread")]
async fn resume_downloads_the_absent_file_and_checks_the_rest_in_full() {
    side_by_side(3, STARTED, |started| async move {
        let case = format!("{started:?}");
        let temp = temp();
        let dir = temp.path();
        let (_seeder, torrent) = peered(dir).await;
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), torrent, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let (engine, added) = away_and_back(dir, engine, id, started, |pack| {
            std::fs::remove_file(pack.join("b.bin")).unwrap();
            // Another piece differs, its size the same.
            let mut a = whole(1);
            a[0] ^= 1;
            std::fs::write(pack.join("a.bin"), a).unwrap();
        })
        .await;
        assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
        assert!(!file(dir, "b.bin").exists(), "{case}: not made before Resume");
        assert_eq!(librqbit_torrent(&engine, id).await, added, "{case}");
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        wait_for(&engine, id, Status::Seeding).await;
        for (i, name) in NAMES.iter().enumerate() {
            assert!(std::fs::read(file(dir, name)).unwrap() == whole(i + 1), "{case}: {name}");
        }
        engine.stop().await;
    })
    .await;
}

/// Nothing had of it: not stuck at Missing files while its drive was away, as when it wasn't. Let
/// run, Phantom made its files (noted Made), and they're there when it's back; paused, it made none.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_that_had_nothing_goes_on_when_its_destination_is_back() {
    let cases = STARTED.into_iter().flat_map(|s| [(s, false), (s, true)]);
    side_by_side(4, cases, |(started, paused)| async move {
        let case = format!("{started:?}, paused {paused}");
        let temp = temp();
        let dir = temp.path();
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), multi_file_torrent(&["a.bin", "b.bin"]), paused).await.unwrap().id;
        let was = if paused { Status::Paused } else { Status::Downloading };
        wait_for(&engine, id, was.clone()).await;
        let (engine, _) = away_and_back(dir, engine, id, started, |_| {}).await;
        assert_eq!(find(&engine, id).await.status, was, "{case}");
        engine.stop().await;
    })
    .await;
}

/// A file Phantom made, nothing had of it, deleted by hand with its destination there: Missing
/// files, as the note says it was there (#374).
#[tokio::test(flavor = "multi_thread")]
async fn a_file_made_with_nothing_had_and_deleted_is_missing_files_with_its_destination_there() {
    let temp = temp();
    let dir = temp.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), multi_file_torrent(&["a.bin", "b.bin"]), false).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;
    let b = dir.join("downloads/pack/b.bin");
    assert!(b.exists(), "made as it was let run");
    std::fs::remove_file(&b).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.stop().await;
}

/// Still fetching its info, nothing of its files is known: it doesn't matter where its drive is.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_its_info_is_not_missing_files_when_its_destination_is_back() {
    let temp = temp();
    let dir = temp.path();
    let engine = engine(dir).await;
    let downloads = dir.join("downloads");
    let id = add(&engine, &downloads, magnet(HASH), false).await.unwrap().id;
    wait_for(&engine, id, Status::FetchingInfo).await;
    take_away(&downloads);
    size_checks(&engine, 2).await;
    bring_back(&downloads);
    size_checks(&engine, 2).await;
    assert_eq!(find(&engine, id).await.status, Status::FetchingInfo);
    engine.stop().await;
}

/// A file set Off isn't one the torrent gets: absent, with the destination away, it isn't missing,
/// and nothing makes it. Nor is one Phantom moved to the Trash: its note went with it.
#[tokio::test(flavor = "multi_thread")]
async fn an_absent_file_set_off_or_moved_to_the_trash_isnt_missing_when_its_destination_is_back() {
    let cases = STARTED.into_iter().flat_map(|s| [(s, false), (s, true)]);
    side_by_side(4, cases, |(started, trashed)| async move {
        let case = format!("{started:?}, trashed {trashed}");
        let temp = temp();
        let dir = temp.path();
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let b = index_of(&engine, id, "b.bin").await;
        if trashed {
            engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
        } else {
            engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
            std::fs::remove_file(file(dir, "b.bin")).unwrap();
        }
        wait_for(&engine, id, Status::Seeding).await;
        let (engine, _) = away_and_back(dir, engine, id, started, |_| {}).await;
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "{case}");
        assert!(!file(dir, "b.bin").exists(), "{case}: not made");
        engine.stop().await;
    })
    .await;
}

/// Missing before its destination went: still missing when it's back, and what's put back while it's
/// away is checked, as before, but one put back before a start (#466).
#[tokio::test(flavor = "multi_thread")]
async fn a_file_missing_before_its_destination_went_stays_missing_until_it_is_put_back() {
    let cases = STARTED.into_iter().flat_map(|s| [(s, false), (s, true)]);
    side_by_side(4, cases, |(started, put_back)| async move {
        let case = format!("{started:?}, put back {put_back}");
        let temp = temp();
        let dir = temp.path();
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let own = std::fs::read(file(dir, "b.bin")).unwrap();
        std::fs::remove_file(file(dir, "b.bin")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        let (engine, added) = away_and_back(dir, engine, id, started, |pack| {
            if put_back {
                std::fs::write(pack.join("b.bin"), &own).unwrap();
            }
        })
        .await;
        if put_back && started == Started::AwaySinceItStopped {
            // Put back before the start that found it missing: what's at a missing file's path at a
            // start isn't taken for the file put back (#466), so it stays missing until it's put
            // back again, or Resume checks it.
            assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
        } else if put_back {
            wait_for(&engine, id, Status::Seeding).await;
        } else {
            assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
            assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true), "{case}");
            assert!(!file(dir, "b.bin").exists(), "{case}");
            assert_eq!(librqbit_torrent(&engine, id).await, added, "{case}");
        }
        engine.stop().await;
    })
    .await;
}

/// A torrent and a peer that has it, nothing downloaded: `seeded` is where the peer's files are.
async fn peer_only(dir: &Path) -> (std::sync::Arc<librqbit::Session>, Vec<u8>, String, u16) {
    let seeded = dir.join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    std::fs::create_dir_all(dir.join("downloads")).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        std::fs::write(seeded.join(name), whole(i + 1)).unwrap();
    }
    seed(dir, &seeded, PIECE as u32).await
}

fn assert_whole(dir: &Path, case: &str) {
    for (i, name) in NAMES.iter().enumerate() {
        assert!(std::fs::read(file(dir, name)).unwrap() == whole(i + 1), "{case}: {name}");
    }
}

/// Waits up to 10 seconds for torrent `id`'s info to arrive.
async fn info_arrived(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if engine.inspect(move |a| a.torrents.get(&id).is_some_and(|t| t.record.info.is_some())).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no info for torrent {id} in 10 s");
}

/// Never let run on a reachable destination, so no file of it was ever made nor noted: not Missing
/// files when the drive is first back, it downloads. Added as the drive goes (the destination was
/// parsed before), or a magnet whose info arrives while it's away.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_never_seen_on_its_destination_downloads_when_the_drive_is_first_back() {
    side_by_side(2, [false, true], |magnet| async move {
        let case = format!("magnet {magnet}");
        let temp = temp();
        let dir = temp.path();
        let (_seeder, torrent, hash, port) = peer_only(dir).await;
        let engine = engine(dir).await;
        let downloads = dir.join("downloads");
        let destination = parsed(&downloads);
        take_away(&downloads);
        let id = if magnet {
            let link = format!("magnet:?xt=urn:btih:{hash}&tr={}", tracker(port));
            let fetched = super::super::fetched(AddSource::Magnet(link)).await.unwrap();
            engine.add(fetched, false, destination).await.unwrap().id
        } else {
            let fetched = super::super::fetched(AddSource::TorrentFile(torrent)).await.unwrap();
            engine.add(fetched, false, destination).await.unwrap().id
        };
        info_arrived(&engine, id).await;
        until(&engine, id, is_away).await;
        bring_back(&downloads);
        wait_for(&engine, id, Status::Seeding).await;
        assert_whole(dir, &case);
        engine.stop().await;
    })
    .await;
}

/// The drive pulled right after the add: nothing had of its files, and none made.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_whose_drive_is_pulled_right_after_the_add_goes_on_when_it_is_back() {
    let temp = temp();
    let dir = temp.path();
    let (_seeder, torrent, _, _) = peer_only(dir).await;
    let engine = engine(dir).await;
    let downloads = dir.join("downloads");
    let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    take_away(&downloads);
    until(&engine, id, is_away).await;
    bring_back(&downloads);
    wait_for(&engine, id, Status::Paused).await;
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert_whole(dir, "pulled after the add");
    engine.stop().await;
}

/// A zero-length wanted file absent: the size check says the same with its destination there as
/// when it comes back.
#[tokio::test(flavor = "multi_thread")]
async fn an_absent_zero_length_file_is_judged_alike_with_the_drive_there_and_as_it_is_back() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    side_by_side(2, [false, true], |away| {
        let seen = seen.clone();
        async move {
            let temp = temp();
            let dir = temp.path();
            let pack = dir.join("downloads/pack");
            std::fs::create_dir_all(&pack).unwrap();
            std::fs::write(pack.join("a.bin"), whole(1)).unwrap();
            std::fs::write(pack.join("z.bin"), b"").unwrap();
            let (_seeder, torrent, _, _) = seed(dir, &pack, PIECE as u32).await;
            let engine = engine(dir).await;
            let id = add(&engine, &dir.join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
            wait_for(&engine, id, Status::Seeding).await;
            let engine = if away {
                away_and_back(dir, engine, id, Started::No, |p| std::fs::remove_file(p.join("z.bin")).unwrap()).await.0
            } else {
                std::fs::remove_file(pack.join("z.bin")).unwrap();
                settled(&engine).await;
                engine
            };
            let status = find(&engine, id).await.status;
            seen.lock().unwrap().push((away, status));
            engine.stop().await;
        }
    })
    .await;
    let mut seen = seen.lock().unwrap().clone();
    seen.sort_by_key(|(away, _)| *away);
    assert_eq!(seen, [(false, Status::MissingFiles), (true, Status::MissingFiles)], "there / back");
}

/// Not checked (a check cancelled) when the drive goes, and a file absent when it's back: Missing
/// files, as for any torrent.
#[tokio::test(flavor = "multi_thread")]
async fn an_unchecked_torrent_with_a_file_absent_as_its_destination_is_back_is_missing_files() {
    let temp = temp();
    let dir = temp.path();
    let gate = Arc::new(super::super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir, &gate).await;
    let id = add(&engine, &dir.join("downloads"), valid_pack(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
    gate.hold(false);
    wait_for(&engine, id, Status::Unchecked).await;
    let (engine, added) = away_and_back(dir, engine, id, Started::No, |p| std::fs::remove_file(p.join("b.bin")).unwrap()).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    assert!(!file(dir, "b.bin").exists(), "not made");
    assert_eq!(librqbit_torrent(&engine, id).await, added, "not added again");
    engine.stop().await;
}

/// A check under way as the drive goes, and a file absent when it's back: Missing files.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_interrupted_by_the_drive_leaving_with_a_file_absent_as_it_is_back_is_missing_files() {
    let temp = temp();
    let dir = temp.path();
    let gate = Arc::new(super::super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir, &gate).await;
    let id = add(&engine, &dir.join("downloads"), valid_pack(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    std::fs::rename(&here, &away).unwrap();
    gate.hold(false);
    until(&engine, id, is_away).await;
    std::fs::remove_file(away.join("pack/b.bin")).unwrap();
    std::fs::rename(&away, &here).unwrap();
    until(&engine, id, |s| !is_away(s)).await;
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    assert!(!file(dir, "b.bin").exists(), "not made");
    engine.stop().await;
}

/// A Move under way as the drive goes, and a file absent when it's back: Missing files, as it is
/// without the drive's coming back.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_interrupted_by_the_drive_leaving_with_a_file_absent_as_it_is_back_is_missing_files() {
    let temp = temp();
    let dir = temp.path();
    let (here, away, elsewhere) = (dir.join("downloads"), dir.join("away"), dir.join("elsewhere"));
    std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
    let engine = engine(dir).await;
    let id = add(&engine, &here, complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let gate = super::super::trash::Gate::default();
    engine
        .inspect({
            let gate = gate.clone();
            move |a| a.move_hold = super::super::actor::moving::Hold { gate: Some(gate), copies: true, ..Default::default() }
        })
        .await;
    super::super::trash::hold(&gate, true);
    let _opens = OpensTrash(gate.clone());
    let to = AbsolutePath::new(&elsewhere).unwrap();
    let answer = tokio::spawn(engine.command_later(id, TorrentCommand::Move { to, clash: None, name: None }));
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    std::fs::rename(&here, &away).unwrap();
    super::super::trash::hold(&gate, false);
    answer.await.unwrap().unwrap_err();
    until(&engine, id, |s| !matches!(s, Status::Moving { .. })).await;
    // Cut short with the drive gone, it ends Missing files (what's left of it isn't where it was).
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    std::fs::remove_file(away.join("pack/a.bin")).unwrap();
    std::fs::rename(&away, &here).unwrap();
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    assert!(!file(dir, "a.bin").exists(), "not made");
    engine.stop().await;
}
