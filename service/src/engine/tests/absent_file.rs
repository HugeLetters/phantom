//! #385: a file of a torrent's that's absent when its destination comes back makes it Missing
//! files, as a file that goes while the destination is there does. The check isn't to make it
//! again, nor the torrent to download it, until Resume.

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
    // Long enough for a size check, and what follows it, to have run.
    tokio::time::sleep(Duration::from_secs(1)).await;
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
    for started in STARTED {
        for (paused, absent) in [(false, vec![1]), (true, vec![1]), (false, vec![0, 2])] {
            let case = format!("{started:?}, paused {paused}, absent {absent:?}");
            let dir = temp();
            let dir = dir.path();
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
        }
    }
}

/// Resume checks the rest in full (what was saved of it is forgotten) and downloads only what's
/// missing, from a peer.
#[tokio::test(flavor = "multi_thread")]
async fn resume_downloads_the_absent_file_and_checks_the_rest_in_full() {
    for started in STARTED {
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
    }
}

/// Nothing had of it: not stuck at Missing files while its drive was away, as when it wasn't.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_that_had_nothing_goes_on_when_its_destination_is_back() {
    for started in STARTED {
        for paused in [false, true] {
            let case = format!("{started:?}, paused {paused}");
            let dir = temp();
            let dir = dir.path();
            let engine = engine(dir).await;
            let id = add(&engine, &dir.join("downloads"), multi_file_torrent(&["a.bin", "b.bin"]), paused).await.unwrap().id;
            let was = if paused { Status::Paused } else { Status::Downloading };
            wait_for(&engine, id, was.clone()).await;
            let (engine, _) = away_and_back(dir, engine, id, started, |_| {}).await;
            assert_eq!(find(&engine, id).await.status, was, "{case}");
            engine.stop().await;
        }
    }
}

/// The same files never made (the person took them away), with its destination there: what master
/// does of one that had nothing and went missing.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_that_had_nothing_with_its_files_gone_is_missing_files_with_its_destination_there() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    let b = dir.join("downloads/pack/b.bin");
    assert!(b.exists(), "librqbit made it when it added the torrent, though paused");
    std::fs::remove_file(&b).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.stop().await;
}

/// Still fetching its info, nothing of its files is known: it doesn't matter where its drive is.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_its_info_is_not_missing_files_when_its_destination_is_back() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let downloads = dir.join("downloads");
    let id = add(&engine, &downloads, magnet(HASH), false).await.unwrap().id;
    wait_for(&engine, id, Status::FetchingInfo).await;
    take_away(&downloads);
    tokio::time::sleep(Duration::from_millis(600)).await;
    bring_back(&downloads);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(find(&engine, id).await.status, Status::FetchingInfo);
    engine.stop().await;
}

/// A file set Off isn't one the torrent gets: absent, with the destination away, it isn't missing,
/// and nothing makes it. Nor is one Phantom moved to the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn an_absent_file_set_off_or_moved_to_the_trash_isnt_missing_when_its_destination_is_back() {
    for started in STARTED {
        for trashed in [false, true] {
            let case = format!("{started:?}, trashed {trashed}");
            let dir = temp();
            let dir = dir.path();
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
        }
    }
}

/// Missing before its destination went: still missing when it's back, and what's put back while it's
/// away is checked, as before.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_missing_before_its_destination_went_stays_missing_until_it_is_put_back() {
    for started in STARTED {
        for put_back in [false, true] {
            let case = format!("{started:?}, put back {put_back}");
            let dir = temp();
            let dir = dir.path();
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
            if put_back {
                wait_for(&engine, id, Status::Seeding).await;
            } else {
                assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
                assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true), "{case}");
                assert!(!file(dir, "b.bin").exists(), "{case}");
                assert_eq!(librqbit_torrent(&engine, id).await, added, "{case}");
            }
            engine.stop().await;
        }
    }
}
