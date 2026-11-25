//! #374: a file at a torrent's path that's longer than the torrent's is never cut short, nor made
//! longer if it's shorter than what was had: it's Missing files, its bytes as they were.

use super::*;

/// What leads the engine to the longer file.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Way {
    Check,
    Resume,
    DestinationBack,
    /// As `DestinationBack`, the engine started while it was away.
    DestinationBackSinceStart,
    Start,
    Add,
    UndoRemove,
    UndoDelete,
    OffToNormal,
    MoveUsingExisting,
}

/// `b.bin`'s own bytes, with more after them.
fn longer(own: &[u8]) -> Vec<u8> {
    [own, b"and what the person wrote"].concat()
}

async fn longer_file(way: Way) {
    let dir = temp();
    let dir = dir.path();
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    let source = complete_torrent(dir).await;
    let theirs = longer(&std::fs::read(file(dir, "b.bin")).unwrap());
    let put = || std::fs::write(file(dir, "b.bin"), &theirs).unwrap();
    if way == Way::Add {
        put();
    }
    let mut engine = super::engine(dir).await;
    let id = add(&engine, &here, source, false).await.unwrap().id;
    let mut path = file(dir, "b.bin");
    if way != Way::Add {
        wait_for(&engine, id, Status::Seeding).await;
    }
    let is_away = |s: &Status| matches!(s, Status::DestinationUnavailable { .. });
    match way {
        Way::Add => {}
        Way::Check | Way::Resume => {
            put();
            wait_for(&engine, id, Status::MissingFiles).await;
            let before = librqbit_torrent(&engine, id).await;
            let press = if way == Way::Check { TorrentCommand::Check } else { TorrentCommand::Resume };
            engine.command(id, press).await.unwrap();
            assert_ne!(librqbit_torrent(&engine, id).await, before, "checked");
        }
        Way::DestinationBack | Way::DestinationBackSinceStart => {
            std::fs::rename(&here, &away).unwrap();
            until(&engine, id, is_away).await;
            if way == Way::DestinationBackSinceStart {
                engine.stop().await;
                engine = super::engine(dir).await;
                until(&engine, id, is_away).await;
            }
            std::fs::write(away.join("pack/b.bin"), &theirs).unwrap();
            std::fs::rename(&away, &here).unwrap();
        }
        Way::Start => {
            engine.stop().await;
            put();
            engine = super::engine(dir).await;
        }
        Way::UndoRemove => {
            engine.command(id, TorrentCommand::Remove).await.unwrap();
            put();
            engine.undo().await.unwrap();
        }
        Way::UndoDelete => {
            engine.command(id, TorrentCommand::Delete).await.unwrap();
            let trashed = std::fs::read_dir(dir.join("trash")).unwrap().flatten().map(|e| e.path());
            for trashed in trashed {
                let b = if trashed.is_dir() { trashed.join("b.bin") } else { trashed };
                if b.ends_with("b.bin") && b.exists() {
                    std::fs::write(b, &theirs).unwrap();
                }
            }
            if engine.undo().await.is_err() {
                // It refuses a file that isn't as it went: nothing of it is touched.
                engine.stop().await;
                return;
            }
        }
        Way::OffToNormal => {
            let b = index_of(&engine, id, "b.bin").await;
            engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
            put();
            engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
        }
        Way::MoveUsingExisting => {
            let elsewhere = dir.join("elsewhere");
            path = elsewhere.join("pack/b.bin");
            std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
            std::fs::write(&path, &theirs).unwrap();
            let to = AbsolutePath::new(&elsewhere).unwrap();
            let refused = engine.command(id, TorrentCommand::Move { to: to.clone(), clash: None, name: None }).await;
            let TorrentRefusal::Clash { files: found } = refusal(refused).1 else { panic!("a clash") };
            let answered = proto::Answered { choice: proto::Clash::UseExisting, found };
            engine.command(id, TorrentCommand::Move { to, clash: Some(answered), name: None }).await.unwrap();
        }
    }
    // It may be Missing files before what was asked too: long enough for that to have begun.
    tokio::time::sleep(Duration::from_secs(1)).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let now = std::fs::read(&path).unwrap();
    assert!(now == theirs, "{way:?}: {} bytes, not the {} put there", now.len(), theirs.len());
    engine.stop().await;
}

macro_rules! ways {
    ($($name:ident: $way:ident,)*) => { $(
        #[tokio::test(flavor = "multi_thread")]
        async fn $name() {
            longer_file(Way::$way).await;
        }
    )* };
}

ways! {
    a_longer_file_isnt_cut_by_a_check: Check,
    a_longer_file_isnt_cut_by_resume: Resume,
    a_longer_file_isnt_cut_as_its_destination_is_back: DestinationBack,
    a_longer_file_isnt_cut_as_its_destination_away_at_the_start_is_back: DestinationBackSinceStart,
    a_longer_file_isnt_cut_by_a_start: Start,
    a_longer_file_isnt_cut_by_an_add: Add,
    a_longer_file_isnt_cut_by_the_undo_of_a_remove: UndoRemove,
    a_longer_file_isnt_cut_by_the_undo_of_a_delete: UndoDelete,
    a_longer_file_isnt_cut_set_normal_again: OffToNormal,
    a_longer_file_isnt_cut_by_a_move_using_it: MoveUsingExisting,
}

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

/// Its destination comes back with `b.bin` as `put`.
async fn back_with(put: &[u8], started: Started) -> (tempfile::TempDir, Engine, u64, u32) {
    let temp = temp();
    let source = complete_torrent(temp.path()).await;
    let (engine, id, added) = source_back_with(temp.path(), source, put, started).await;
    (temp, engine, id, added)
}

/// [`back_with`], the torrent `source`, whose files are in the download dir.
async fn source_back_with(dir: &Path, source: AddSource, put: &[u8], started: Started) -> (Engine, u64, u32) {
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    let mut engine = engine(dir).await;
    let id = add(&engine, &here, source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let is_away = |s: &Status| matches!(s, Status::DestinationUnavailable { .. });
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
    std::fs::write(away.join("pack/b.bin"), put).unwrap();
    std::fs::rename(&away, &here).unwrap();
    until(&engine, id, |s| !is_away(s)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    (engine, id, added)
}

/// Not added to librqbit, so nothing of it is written, until the own file is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_of_an_impossible_size_as_its_destination_is_back_is_missing_until_its_own_is_back() {
    for started in STARTED {
        let own: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + 31) as u8).collect();
        for put in [longer(&own), own[..PIECE + 10].to_vec(), Vec::new()] {
            let case = format!("{} bytes, {started:?}", put.len());
            let (dir, engine, id, added) = back_with(&put, started).await;
            assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
            assert!(std::fs::read(file(dir.path(), "b.bin")).unwrap() == put, "{case}: as it was put there");
            assert_eq!(librqbit_torrent(&engine, id).await, added, "{case}: not added, so not checked");
            std::fs::write(file(dir.path(), "b.bin"), &own).unwrap();
            wait_for(&engine, id, Status::Seeding).await;
            engine.stop().await;
        }
    }
}

/// The person's own file, cut off by a drive pulled as it was written: Resume, or Check and Resume
/// of one paused meanwhile, keeps what's valid of it and downloads the rest.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_cut_off_as_its_drive_went_is_checked_and_downloaded_by_resume() {
    for started in STARTED {
        for paused in [false, true] {
            let case = format!("{started:?}, paused {paused}");
            let temp = temp();
            let dir = temp.path();
            let (seeded, pack) = (dir.join("seed/pack"), dir.join("downloads/pack"));
            std::fs::create_dir_all(&seeded).unwrap();
            std::fs::create_dir_all(&pack).unwrap();
            let whole = |seed: usize| -> Vec<u8> { (0..4 * PIECE).map(|n| (n % 241) as u8 ^ seed as u8).collect() };
            for (i, name) in ["a.bin", "b.bin", "c.bin"].iter().enumerate() {
                std::fs::write(seeded.join(name), whole(i + 1)).unwrap();
                std::fs::write(pack.join(name), whole(i + 1)).unwrap();
            }
            let (_seeder, torrent, _, _) = seed(dir, &seeded, PIECE as u32).await;
            let cut = whole(2)[..PIECE + 10].to_vec();
            let (engine, id, _) = source_back_with(dir, AddSource::TorrentFile(torrent), &cut, started).await;
            assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
            assert!(std::fs::read(file(dir, "b.bin")).unwrap() == cut, "{case}: as it was");
            if paused {
                engine.command(id, TorrentCommand::Pause).await.unwrap();
                engine.command(id, TorrentCommand::Check).await.unwrap();
                wait_for(&engine, id, Status::Paused).await;
                let b = files_on(&engine, id).await[1].clone();
                assert_eq!(b, ("b.bin".into(), PIECE as u64, false), "{case}: its whole piece kept");
            }
            engine.command(id, TorrentCommand::Resume).await.unwrap();
            wait_for(&engine, id, Status::Seeding).await;
            for (i, name) in ["a.bin", "b.bin", "c.bin"].iter().enumerate() {
                assert!(std::fs::read(file(dir, name)).unwrap() == whole(i + 1), "{case}: {name}");
            }
            engine.stop().await;
        }
    }
}

/// Set Normal on a running torrent, a longer file isn't handed to librqbit while it runs: it would
/// be downloaded into until the size check after it.
#[tokio::test(flavor = "multi_thread")]
async fn a_longer_file_set_normal_isnt_given_to_librqbit_while_it_runs() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let set = async |priority| {
        engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority }).await.unwrap();
        engine.inspect(move |a| a.torrents[&id].chosen_running).await
    };
    let off = set(Priority::Off).await;
    let normal = set(Priority::Normal).await;
    assert_eq!(normal, off + 1, "its own file is, as it runs");
    let off = set(Priority::Off).await;
    let theirs = longer(&std::fs::read(file(dir, "b.bin")).unwrap());
    std::fs::write(file(dir, "b.bin"), &theirs).unwrap();
    let longer = set(Priority::Normal).await;
    assert_eq!(longer, off, "not given while it ran");
    wait_for(&engine, id, Status::MissingFiles).await;
    // The way out: set Off, the rest runs.
    set(Priority::Off).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert!(std::fs::read(file(dir, "b.bin")).unwrap() == theirs);
    engine.stop().await;
}

/// Found of an impossible size, then set Off: the rest is checked in full all the same, as after any
/// return, not trusted from the pieces saved before it went.
#[tokio::test(flavor = "multi_thread")]
async fn the_rest_of_a_returned_torrent_is_checked_in_full_once_the_file_is_set_off() {
    let own: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + 31) as u8).collect();
    for started in STARTED {
        let (dir, engine, id, _) = back_with(&longer(&own), started).await;
        // A piece of another file differs, its size the same.
        let mut a = std::fs::read(file(dir.path(), "a.bin")).unwrap();
        a[0] ^= 1;
        std::fs::write(file(dir.path(), "a.bin"), &a).unwrap();
        let b = index_of(&engine, id, "b.bin").await;
        engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
        wait_for(&engine, id, Status::Downloading).await;
        assert_eq!(files_on(&engine, id).await[0], ("a.bin".into(), PIECE as u64, false), "{started:?}");
        engine.stop().await;
    }
}

/// A check pressed for a longer file ends without librqbit running the torrent, not even until the
/// size check after it: it would download into that file.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_of_a_torrent_with_a_longer_file_ends_without_starting_it() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let theirs = longer(&std::fs::read(file(dir, "b.bin")).unwrap());
    std::fs::write(file(dir, "b.bin"), &theirs).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let counts = || engine.inspect(move |a| (a.torrents[&id].added, a.torrents[&id].started));
    let (added, started) = counts().await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(counts().await, (added + 1, started), "checked, not started");
    engine.stop().await;
}

/// #373, the gate's (#374) to close: another file of the same size, put there while the destination
/// was away, is taken as the torrent's, to be downloaded over once there's a peer.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "#373: not fixed yet"]
async fn another_file_of_the_same_size_as_its_destination_is_back_is_missing() {
    let (dir, engine, id, _) = back_with(&[9; 2 * PIECE], Started::No).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    assert!(std::fs::read(file(dir.path(), "b.bin")).unwrap() == [9; 2 * PIECE]);
    engine.stop().await;
}
