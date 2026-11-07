//! The engine against a real librqbit session in a temp dir, offline. Magnets name a tracker on
//! a closed local port, so their info is never found and they stay fetching info.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use proto::{AddSource, Added, Priority, Status, TorrentInfo};
use tempfile::TempDir;

use super::lifecycle::State;
use super::{Config, Engine};

const HASH: &str = "00000000000000000000000000000000000000aa";
/// Short, so tests see missing files quickly.
const CHECK_FILES_EVERY: Duration = Duration::from_millis(200);

fn magnet(hash: &str) -> AddSource {
    AddSource::Url(format!("magnet:?xt=urn:btih:{hash}&dn=Big+Buck+Bunny&tr=http://127.0.0.1:1/announce"))
}

/// A .torrent file of one 16 KiB file, `name`, that no one seeds.
fn torrent_file(name: &str) -> AddSource {
    let mut info = format!("d6:lengthi16384e4:name{}:{name}12:piece lengthi16384e6:pieces20:", name.len()).into_bytes();
    info.extend([7; 20]);
    info.push(b'e');
    let mut file = b"d4:info".to_vec();
    file.extend(info);
    file.push(b'e');
    AddSource::TorrentFile(file)
}

/// A .torrent file named `pack` of 16 KiB files at `paths` (a piece each), that no one seeds.
fn multi_file_torrent(paths: &[&str]) -> AddSource {
    let mut files = String::new();
    for path in paths {
        let parts: String = path.split('/').map(|p| format!("{}:{p}", p.len())).collect();
        files += &format!("d6:lengthi16384e4:pathl{parts}ee");
    }
    let mut file = format!("d4:infod5:filesl{files}e4:name4:pack12:piece lengthi16384e6:pieces{}:", 20 * paths.len())
        .into_bytes();
    file.extend(vec![7; 20 * paths.len()]);
    file.extend(b"ee");
    AddSource::TorrentFile(file)
}

async fn engine(dir: &Path) -> Engine {
    engine_checking_files_every(dir, CHECK_FILES_EVERY).await
}

async fn engine_checking_files_every(dir: &Path, check_files_every: Duration) -> Engine {
    Engine::start(Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        check_files_every,
    })
    .await
    .unwrap()
}

fn temp() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    dir
}

async fn find(engine: &Engine, id: u64) -> TorrentInfo {
    engine.listing().await.into_iter().find(|t| t.id == id).unwrap_or_else(|| panic!("no torrent {id}"))
}

/// Waits up to 10 seconds for the torrent's status, e.g. while its files are checked.
async fn wait_for(engine: &Engine, id: u64, status: Status) {
    for _ in 0..100 {
        if find(engine, id).await.status == status {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("torrent {id} is {:?}, not {status:?}", find(engine, id).await.status);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_added_magnet_is_a_torrent_fetching_info() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    assert_eq!(added, Added { id: 0, name: Some("Big Buck Bunny".into()), already_existed: false });

    let listing = engine.listing().await;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].id, 0);
    assert_eq!(listing[0].status, Status::FetchingInfo);
    assert_eq!(listing[0].name.as_deref(), Some("Big Buck Bunny"));
    assert_eq!(listing[0].info_hash.as_deref(), Some(HASH));
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_a_magnet_again_answers_with_the_torrent_already_in_the_list() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let first = engine.add(magnet(HASH), false).await.unwrap();
    let again = engine.add(magnet(HASH), false).await.unwrap();
    assert_eq!(again, Added { already_existed: true, ..first });
    assert_eq!(engine.listing().await.len(), 1, "no second row");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_torrent_fetching_info_stops_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    engine.remove(added.id).await.unwrap();
    assert!(engine.listing().await.is_empty());
    assert!(engine.pause(added.id).await.is_err(), "it's gone");

    let again = engine.add(magnet(HASH), false).await.unwrap();
    assert!(!again.already_existed, "it can be added again");
    assert_ne!(again.id, added.id, "as a new torrent");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_file_is_added_once() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(added.name.as_deref(), Some("a.bin"));
    assert!(!added.already_existed);
    assert_ne!(find(&engine, added.id).await.status, Status::FetchingInfo, "a file brings its info");

    let again = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(again, Added { already_existed: true, ..added.clone() });

    let hash = find(&engine, added.id).await.info_hash.unwrap();
    let by_magnet = engine.add(magnet(&hash), false).await.unwrap();
    assert_eq!((by_magnet.id, by_magnet.already_existed), (added.id, true), "found by the magnet's hash too");
    assert_eq!(engine.listing().await.len(), 1);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_incomplete_torrent_is_paused_not_done() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let fetching = engine.add(magnet(HASH), false).await.unwrap();
    assert!(engine.pause(fetching.id).await.is_err(), "a torrent fetching info can't be paused");

    let added = engine.add(torrent_file("a.bin"), false).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.pause(added.id).await.unwrap();
    assert_eq!(find(&engine, added.id).await.status, Status::Paused, "incomplete, so not Done");
    engine.resume(added.id).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_the_torrent_and_its_files() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(torrent_file("a.bin"), false).await.unwrap();
    let file = dir.path().join("downloads/a.bin");
    assert!(file.exists(), "adding creates the file");
    engine.delete(added.id).await.unwrap();
    assert!(engine.listing().await.is_empty());
    assert!(!file.exists());
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ids_survive_a_restart() {
    let dir = temp();
    let (a, b) = {
        let engine = engine(dir.path()).await;
        // librqbit numbers only the torrents it has, from 0; ours count the magnet too.
        let fetching = engine.add(magnet(HASH), false).await.unwrap();
        let a = engine.add(torrent_file("a.bin"), false).await.unwrap();
        let b = engine.add(torrent_file("b.bin"), false).await.unwrap();
        assert_eq!((fetching.id, a.id, b.id), (0, 1, 2));
        engine.remove(a.id).await.unwrap();
        engine.stop().await;
        (a, b)
    };

    let engine = engine(dir.path()).await;
    let listing = engine.listing().await;
    let ids: Vec<_> = listing.iter().map(|t| (t.id, t.name.clone())).collect();
    assert_eq!(ids, [(b.id, b.name.clone())], "b keeps its id; the magnet fetching info is gone");
    let next = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert!(next.id > b.id && next.id != a.id, "ids aren't reused");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_lists_its_files_and_where_they_are() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(multi_file_torrent(&["a.bin", "b.bin", "extras/c.bin"]), false).await.unwrap();
    let details = engine.details(added.id).await.unwrap();
    let files: Vec<_> = details.files.iter().map(|f| (f.index, f.path.to_str().unwrap(), f.size_bytes, f.priority)).collect();
    assert_eq!(
        files,
        [
            (0, "a.bin", 16384, Priority::Normal),
            (1, "b.bin", 16384, Priority::Normal),
            (2, "extras/c.bin", 16384, Priority::Normal),
        ]
    );
    assert_eq!(details.full_size_bytes, 3 * 16384);
    assert_eq!(details.location, dir.path().join("downloads/pack"), "a torrent of several files has a folder");
    let t = find(&engine, added.id).await;
    assert_eq!((t.size_bytes, t.full_size_bytes), (3 * 16384, 3 * 16384));

    let single = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(engine.details(single.id).await.unwrap().location, dir.path().join("downloads/a.bin"), "one file is itself");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_leaves_the_size_and_stays_off_after_a_restart() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let added = engine.add(multi_file_torrent(&["a.bin", "b.bin", "c.bin"]), false).await.unwrap();
        wait_for(&engine, added.id, Status::Downloading).await;
        engine.set_priority(added.id, &[1], Priority::Off).await.unwrap();

        let t = find(&engine, added.id).await;
        assert_eq!(t.size_bytes, 2 * 16384, "Size leaves out the file set Off");
        assert_eq!(t.full_size_bytes, 3 * 16384, "Full size doesn't");
        let priorities: Vec<_> = engine.details(added.id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal]);
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    let priorities: Vec<_> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal], "kept across the restart");
    assert_eq!(find(&engine, id).await.size_bytes, 2 * 16384);

    wait_for(&engine, id, Status::Downloading).await;
    engine.set_priority(id, &[0, 1], Priority::Normal).await.unwrap();
    assert_eq!(find(&engine, id).await.size_bytes, 3 * 16384, "Normal again, several at once");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_file_off_finishes_the_torrent() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let added = engine.add(multi_file_torrent(&["a.bin", "b.bin"]), false).await.unwrap();
        wait_for(&engine, added.id, Status::Downloading).await;
        engine.set_priority(added.id, &[0], Priority::Off).await.unwrap();
        engine.set_priority(added.id, &[1], Priority::Off).await.unwrap();
        assert!(engine.set_priority(added.id, &[2], Priority::Off).await.is_err(), "no file 2");

        let t = find(&engine, added.id).await;
        assert_eq!((t.size_bytes, t.full_size_bytes, t.progress_bytes), (0, 2 * 16384, 0), "nothing left to get");
        assert_eq!(t.status, Status::Seeding, "so it's finished");
        let priorities: Vec<_> = engine.details(added.id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Off, Priority::Off]);

        engine.pause(added.id).await.unwrap();
        assert_eq!(find(&engine, added.id).await.status, Status::Done);
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    wait_for(&engine, id, Status::Done).await;
    assert_eq!(find(&engine, id).await.size_bytes, 0, "kept across the restart");

    engine.set_priority(id, &[1], Priority::Normal).await.unwrap();
    assert_eq!(find(&engine, id).await.size_bytes, 16384, "a file can be Normal again");
    engine.stop().await;
}

/// A .torrent file of one sparse 128 MiB file already in the download dir, so adding it means
/// checking 128 MiB: seconds in a debug build.
async fn checking_torrent(engine: &Engine, dir: &Path) -> u64 {
    const PIECES: usize = 128;
    let downloads = dir.join("downloads/pack");
    std::fs::create_dir_all(&downloads).unwrap();
    for name in ["a.bin", "b.bin"] {
        std::fs::File::create(downloads.join(name)).unwrap().set_len((PIECES as u64 / 2) << 20).unwrap();
    }
    let mut file = format!(
        "d4:infod5:filesld6:lengthi{len}e4:pathl5:a.bineed6:lengthi{len}e4:pathl5:b.bineee4:name4:pack12:piece lengthi1048576e6:pieces{}:",
        20 * PIECES,
        len = (PIECES as u64 / 2) << 20,
    )
    .into_bytes();
    file.extend(vec![7; 20 * PIECES]);
    file.extend(b"ee");
    let added = engine.add(AddSource::TorrentFile(file), false).await.unwrap();
    wait_for(engine, added.id, Status::Checking).await;
    added.id
}

async fn priorities(engine: &Engine, id: u64) -> Vec<Priority> {
    engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect()
}

/// The files librqbit itself gets (`None`: all), not the priorities held for after a check.
async fn librqbit_only_files(engine: &Engine, id: u64) -> Option<Vec<usize>> {
    engine.inspect(move |actor| actor.torrents[&id].handle.as_ref().unwrap().only_files()).await
}

async fn held(engine: &Engine, id: u64) -> Option<BTreeSet<usize>> {
    engine
        .inspect(move |actor| {
            let hash = actor.torrents[&id].handle.as_ref().unwrap().info_hash().as_string();
            actor.store.get(&hash).unwrap().held.clone()
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_priority_set_while_checking_shows_at_once_and_applies_when_the_check_ends() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;

    engine.set_priority(id, &[1], Priority::Off).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "still checking");
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "shown as asked");
    assert_eq!(find(&engine, id).await.size_bytes, 64 << 20, "and out of the Size");

    // Changes during the check build on each other.
    engine.set_priority(id, &[0], Priority::Off).await.unwrap();
    assert_eq!(priorities(&engine, id).await, [Priority::Off, Priority::Off]);

    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(priorities(&engine, id).await, [Priority::Off, Priority::Off], "applied: librqbit agrees");
    assert_eq!(librqbit_only_files(&engine, id).await, Some(vec![]), "in librqbit itself, not only held here");
    assert_eq!(held(&engine, id).await, None, "nothing held any more");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_priority_held_during_a_check_survives_a_restart() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = checking_torrent(&engine, dir.path()).await;
        engine.set_priority(id, &[1], Priority::Off).await.unwrap();
        assert_eq!(find(&engine, id).await.status, Status::Checking, "stopped while still checking");
        engine.stop().await;
        id
    };

    let engine = engine(dir.path()).await;
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "still held, and shown");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(librqbit_only_files(&engine, id).await, Some(vec![0]), "applied after the check");
    assert_eq!(held(&engine, id).await, None);
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off]);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn running_torrents_resume_after_a_restart_and_paused_ones_stay_paused() {
    let dir = temp();
    let (running, paused, added_paused) = {
        let engine = engine(dir.path()).await;
        let running = engine.add(torrent_file("a.bin"), false).await.unwrap().id;
        let paused = engine.add(torrent_file("b.bin"), false).await.unwrap().id;
        let added_paused = engine.add(torrent_file("c.bin"), true).await.unwrap().id;
        wait_for(&engine, running, Status::Downloading).await;
        wait_for(&engine, paused, Status::Downloading).await;
        engine.pause(paused).await.unwrap();
        wait_for(&engine, added_paused, Status::Paused).await;
        engine.stop().await;
        (running, paused, added_paused)
    };
    let saved = std::fs::read_to_string(dir.path().join("data/engine.json")).unwrap();
    assert_eq!(saved.matches("\"running_at_shutdown\": true").count(), 1, "{saved}");
    let destination = format!("\"destination\": {:?}", dir.path().join("downloads").to_str().unwrap());
    assert_eq!(saved.matches(&destination).count(), 3, "each torrent's destination: {saved}");

    let engine = engine(dir.path()).await;
    wait_for(&engine, running, Status::Downloading).await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(find(&engine, paused).await.status, Status::Paused, "paused by the user");
    assert_eq!(find(&engine, added_paused).await.status, Status::Paused, "added paused");
    let saved = std::fs::read_to_string(dir.path().join("data/engine.json")).unwrap();
    assert!(!saved.contains("running_at_shutdown"), "cleared once resumed: {saved}");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_added_paused_checks_then_stops() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(torrent_file("a.bin"), true).await.unwrap();
    wait_for(&engine, added.id, Status::Paused).await;
    engine.resume(added.id).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pausing_during_a_check_pauses_and_resuming_checks_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    engine.pause(id).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Paused);
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "stays paused");
    engine.pause(id).await.unwrap();
    engine.resume(id).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_command_is_answered_in_every_state_it_can_be_in() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let fetching = engine.add(magnet(HASH), false).await.unwrap().id;
    let downloading = engine.add(torrent_file("a.bin"), false).await.unwrap().id;
    let paused = engine.add(torrent_file("b.bin"), true).await.unwrap().id;
    let checking = checking_torrent(&engine, dir.path()).await;
    wait_for(&engine, downloading, Status::Downloading).await;
    wait_for(&engine, paused, Status::Paused).await;

    for id in [fetching, downloading, paused, checking, 99] {
        let answered = async {
            let _ = engine.details(id).await;
            let _ = engine.set_priority(id, &[0], Priority::Normal).await;
            let _ = engine.pause(id).await;
            let _ = engine.pause(id).await;
            let _ = engine.resume(id).await;
            let _ = engine.resume(id).await;
            let _ = engine.remove(id).await;
            let _ = engine.delete(id).await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), answered).await.expect("every command answered");
    }
    assert!(engine.listing().await.is_empty(), "all removed");

    engine.stop().await;
    let after = tokio::time::timeout(std::time::Duration::from_secs(1), engine.pause(downloading)).await;
    assert_eq!(after.expect("answered after stopping too").unwrap_err().to_string(), "the service is stopping");
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_say_why() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let fetching = engine.add(magnet(HASH), false).await.unwrap().id;
    let message = |r: anyhow::Result<()>| r.unwrap_err().to_string();
    assert_eq!(message(engine.pause(fetching).await), format!("torrent {fetching} has no info yet"));
    assert_eq!(message(engine.resume(fetching).await), format!("torrent {fetching} has no info yet"));
    assert_eq!(message(engine.pause(99).await), "no torrent with id 99");

    // States nothing enters yet (#23, #24), set by hand: the reasons get to the caller.
    let id = engine.add(torrent_file("a.bin"), false).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;
    let reason = "drive not connected".to_owned();
    engine
        .inspect(move |actor| actor.torrents.get_mut(&id).unwrap().state = State::DestinationUnavailable { reason })
        .await;
    assert_eq!(
        message(engine.resume(id).await),
        format!("torrent {id} can't start: its destination is unavailable (drive not connected)")
    );
    let status = find(&engine, id).await.status;
    assert_eq!(status, Status::DestinationUnavailable { reason: "drive not connected".into() });
    engine.pause(id).await.unwrap();
    engine.inspect(move |actor| actor.torrents.get_mut(&id).unwrap().state = State::Moving { paused: false }).await;
    assert_eq!(message(engine.remove(id).await), format!("torrent {id} is moving"));
    assert_eq!(find(&engine, id).await.status, Status::Moving);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_info_has_no_details_yet() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    let refused = engine.details(added.id).await.unwrap_err();
    assert!(refused.to_string().contains("no info yet"), "{refused}");
    assert!(engine.set_priority(added.id, &[0], Priority::Off).await.is_err());
    engine.stop().await;
}

const PIECE: usize = 16384;

/// Writes `pack/a.bin`, `pack/b.bin` and `pack/c.bin` (two pieces each, all different) into the
/// download dir and returns a .torrent of them, so the torrent is complete once added. Its
/// tracker is a closed local port, so nothing else is ever downloaded.
async fn complete_torrent(dir: &Path) -> AddSource {
    let pack = dir.join("downloads/pack");
    std::fs::create_dir_all(&pack).unwrap();
    for (i, name) in ["a.bin", "b.bin", "c.bin"].iter().enumerate() {
        let bytes: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + i * 31) as u8).collect();
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

fn file(dir: &Path, name: &str) -> PathBuf {
    dir.join("downloads/pack").join(name)
}

/// Each file's (path, progress, missing), by path.
async fn files_on(engine: &Engine, id: u64) -> Vec<(String, u64, bool)> {
    let details = engine.details(id).await.unwrap();
    let mut files: Vec<_> =
        details.files.iter().map(|f| (f.path.to_string_lossy().into_owned(), f.progress_bytes, f.missing)).collect();
    files.sort();
    files
}

/// The index of the file at `path`: librqbit's .torrent lists them in its own order.
async fn index_of(engine: &Engine, id: u64, path: &str) -> usize {
    engine.details(id).await.unwrap().files.iter().find(|f| f.path == Path::new(path)).unwrap().index
}

/// The librqbit torrent behind `id`, to tell an add again from the same torrent (librqbit's
/// own ids are reused).
async fn librqbit_torrent(engine: &Engine, id: u64) -> usize {
    engine.inspect(move |actor| std::sync::Arc::as_ptr(actor.torrents[&id].handle.as_ref().unwrap()) as usize).await
}

async fn librqbit_live(engine: &Engine, id: u64) -> bool {
    engine.inspect(move |actor| actor.torrents[&id].handle.as_ref().unwrap().live().is_some()).await
}

const FULL: u64 = 2 * PIECE as u64;

#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_outside_phantom_stops_the_torrent_as_missing_files() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;

    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(
        files_on(&engine, id).await,
        [("a.bin".into(), FULL, false), ("b.bin".into(), 0, true), ("c.bin".into(), FULL, false)],
        "the missing file is marked, with nothing downloaded"
    );
    assert_eq!(find(&engine, id).await.progress_bytes, 2 * FULL, "nor counted in the torrent's progress");
    assert!(!librqbit_live(&engine, id).await, "stopped in librqbit");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "and stays so");
    assert!(!file(dir.path(), "b.bin").exists(), "nothing put back");
    assert_eq!(engine.pause(id).await.unwrap(), (), "pausing is nothing to do");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_of_a_size_it_cannot_have_is_missing_too() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.pause(id).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Done);

    std::fs::File::options().write(true).open(file(dir.path(), "c.bin")).unwrap().set_len(10).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(files_on(&engine, id).await[2], ("c.bin".into(), 0, true), "cut short");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_watcher_sees_a_torrent_folder_go_without_waiting_for_the_regular_look() {
    let dir = temp();
    let engine = engine_checking_files_every(dir.path(), Duration::from_secs(3600)).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // The first look, at start, ran before the torrent was added; this one watches its destination.
    engine.inspect(|actor| actor.watch_destinations()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    std::fs::remove_dir_all(dir.path().join("downloads/pack")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let missing: Vec<bool> = files_on(&engine, id).await.into_iter().map(|(_, _, missing)| missing).collect();
    assert_eq!(missing, [true, true, true]);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_a_torrent_missing_files_checks_it_and_downloads_them_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = librqbit_torrent(&engine, id).await;

    engine.resume(id).await.unwrap();
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added to librqbit again");
    let status = find(&engine, id).await.status;
    assert!(matches!(status, Status::Checking | Status::Downloading), "checked first: {status:?}");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(
        files_on(&engine, id).await,
        [("a.bin".into(), FULL, false), ("b.bin".into(), 0, false), ("c.bin".into(), FULL, false)],
        "the check kept what's there; the missing file is to download"
    );
    assert!(file(dir.path(), "b.bin").exists(), "into a new file");
    let t = find(&engine, id).await;
    assert_eq!((t.id, t.size_bytes, t.progress_bytes), (id, 3 * FULL, 2 * FULL), "the same torrent and row");
    assert_eq!(engine.listing().await.len(), 1);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn setting_the_missing_file_off_leaves_the_torrent_done() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    let b = index_of(&engine, id, "b.bin").await;
    engine.set_priority(id, &[b], Priority::Off).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Done, "everything it's getting is there");
    assert!(files_on(&engine, id).await.iter().all(|(_, _, missing)| !missing));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Done, "an Off file is never missing");

    engine.set_priority(id, &[b], Priority::Normal).await.unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn setting_the_missing_file_off_leaves_an_incomplete_torrent_paused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = complete_torrent(dir.path()).await;
    std::fs::write(file(dir.path(), "c.bin"), vec![0; 2 * PIECE]).unwrap();
    let id = engine.add(source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;
    std::fs::remove_file(file(dir.path(), "a.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    let a = index_of(&engine, id, "a.bin").await;
    engine.set_priority(id, &[a], Priority::Off).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Paused, "c.bin is still to download");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_while_the_service_was_stopped_is_missing_on_start_and_not_resumed() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();

    let id = {
        let engine = engine(dir.path()).await;
        assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "instead of resuming");
        assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "though librqbit made the file again");
        assert!(!librqbit_live(&engine, id).await);
        let saved = std::fs::read_to_string(dir.path().join("data/engine.json")).unwrap();
        assert!(!saved.contains("running_at_shutdown") && !saved.contains("files_at_shutdown"), "taken: {saved}");
        engine.stop().await;
        id
    };

    let engine = engine(dir.path()).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "across another restart");
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true));
    engine.resume(id).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn check_finds_a_changed_file_keeps_what_is_valid_and_keeps_the_torrent_running_or_stopped() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;

    // The same size, other bytes: only a check sees it.
    std::fs::write(file(dir.path(), "b.bin"), vec![0; 2 * PIECE]).unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "not missing");

    let before = librqbit_torrent(&engine, id).await;
    engine.check(id).await.unwrap();
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added to librqbit again");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(
        files_on(&engine, id).await,
        [("a.bin".into(), FULL, false), ("b.bin".into(), 0, false), ("c.bin".into(), FULL, false)]
    );

    engine.pause(id).await.unwrap();
    engine.check(id).await.unwrap();
    wait_for(&engine, id, Status::Paused).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "a stopped torrent stays stopped");

    let fetching = engine.add(magnet(HASH), false).await.unwrap().id;
    let refused = engine.check(fetching).await.unwrap_err().to_string();
    assert_eq!(refused, format!("torrent {fetching} has no info yet"));
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn checking_a_torrent_missing_files_leaves_it_stopped() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    engine.check(id).await.unwrap();
    wait_for(&engine, id, Status::Paused).await;
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, false), "no longer missing: to download");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_missing_files_is_checking_after_resume_before_it_downloads() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    wait_for(&engine, id, Status::Downloading).await;
    std::fs::remove_file(dir.path().join("downloads/pack/b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    engine.resume(id).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "128 MiB take a while to check");
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// Removes or deletes the torrent, adds it again, and checks it's listed and takes commands.
async fn removed_then_added_again(engine: &Engine, source: impl Fn() -> AddSource, delete: bool) -> Added {
    let first = engine.add(source(), false).await.unwrap();
    if delete {
        engine.delete(first.id).await.unwrap();
    } else {
        engine.remove(first.id).await.unwrap();
    }
    assert!(engine.listing().await.is_empty());
    let again = engine.add(source(), false).await.unwrap();
    assert!(!again.already_existed);
    assert_ne!(again.id, first.id, "a re-added torrent is a new one");
    assert_eq!(find(engine, again.id).await.id, again.id, "it's listed");
    engine.pause(again.id).await.unwrap();
    again
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_torrent_file_can_be_added_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let again = removed_then_added_again(&engine, || torrent_file("a.bin"), false).await;
    engine.remove(again.id).await.unwrap();
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_torrent_file_can_be_added_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    removed_then_added_again(&engine, || torrent_file("a.bin"), true).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_torrent_can_be_added_again_by_its_magnet() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let first = engine.add(torrent_file("a.bin"), false).await.unwrap();
    let hash = find(&engine, first.id).await.info_hash.unwrap();
    engine.remove(first.id).await.unwrap();
    let again = engine.add(magnet(&hash), false).await.unwrap();
    assert!(!again.already_existed);
    assert_eq!(find(&engine, again.id).await.id, again.id, "it's listed");
    engine.remove(again.id).await.unwrap();
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_removed_before_a_restart_can_be_added_again() {
    let dir = temp();
    let first = {
        let engine = engine(dir.path()).await;
        let first = engine.add(torrent_file("a.bin"), false).await.unwrap();
        engine.remove(first.id).await.unwrap();
        engine.stop().await;
        first
    };
    let engine = engine(dir.path()).await;
    assert!(engine.listing().await.is_empty());
    let again = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert!(!again.already_existed);
    assert_ne!(again.id, first.id);
    assert_eq!(find(&engine, again.id).await.id, again.id, "it's listed");
    engine.remove(again.id).await.unwrap();
    engine.stop().await;
}

/// A .torrent of `done.bin`: 16 KiB of zeros, placed in the downloads folder, so it's complete.
fn complete_zeros_torrent(dir: &Path) -> AddSource {
    std::fs::create_dir_all(dir.join("downloads")).unwrap();
    std::fs::write(dir.join("downloads/done.bin"), vec![0u8; 16384]).unwrap();
    let mut file = b"d4:infod6:lengthi16384e4:name8:done.bin12:piece lengthi16384e6:pieces20:".to_vec();
    // The SHA-1 of the zeros.
    file.extend([137, 114, 86, 182, 112, 158, 26, 77, 169, 218, 186, 146, 182, 189, 227, 156, 207, 204, 216, 193]);
    file.extend(b"ee");
    AddSource::TorrentFile(file)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_complete_torrent_can_be_added_again_with_its_file_kept() {
    let dir = temp();
    let source = complete_zeros_torrent(dir.path());
    let again = |source: &AddSource| match source {
        AddSource::TorrentFile(bytes) => AddSource::TorrentFile(bytes.clone()),
        AddSource::Url(url) => AddSource::Url(url.clone()),
    };
    let engine_a = engine(dir.path()).await;
    let first = engine_a.add(again(&source), false).await.unwrap();
    wait_for(&engine_a, first.id, Status::Seeding).await;
    engine_a.remove(first.id).await.unwrap();
    assert!(engine_a.listing().await.is_empty());
    let second = engine_a.add(again(&source), false).await.unwrap();
    assert!(!second.already_existed);
    wait_for(&engine_a, second.id, Status::Seeding).await;
    engine_a.pause(second.id).await.unwrap();
    engine_a.remove(second.id).await.unwrap();
    engine_a.stop().await;

    let engine_b = engine(dir.path()).await;
    assert!(engine_b.listing().await.is_empty());
    let third = engine_b.add(again(&source), false).await.unwrap();
    wait_for(&engine_b, third.id, Status::Seeding).await;
    engine_b.delete(third.id).await.unwrap();
    let fourth = engine_b.add(again(&source), false).await.unwrap();
    assert_eq!(find(&engine_b, fourth.id).await.id, fourth.id, "it's listed");
    engine_b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_file_put_back_is_checked_and_the_torrent_is_done_without_resuming() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let trash = dir.path().join("b.bin");
    std::fs::rename(file(dir.path(), "b.bin"), &trash).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = librqbit_torrent(&engine, id).await;

    std::fs::rename(&trash, file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::Done).await;
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added again, so its files were checked");
    assert_eq!(
        files_on(&engine, id).await,
        [("a.bin".into(), FULL, false), ("b.bin".into(), FULL, false), ("c.bin".into(), FULL, false)]
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Done, "never resumed by itself");
    assert!(!librqbit_live(&engine, id).await);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_put_back_that_is_not_the_one_that_went_leaves_the_torrent_missing_files() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = librqbit_torrent(&engine, id).await;

    std::fs::write(file(dir.path(), "b.bin"), vec![0; 2 * PIECE]).unwrap();
    // Checked (added again), and found not to be it.
    for _ in 0..100 {
        if librqbit_torrent(&engine, id).await != before && find(&engine, id).await.status == Status::MissingFiles {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_ne!(librqbit_torrent(&engine, id).await, before, "checked");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true));
    let checked = librqbit_torrent(&engine, id).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(librqbit_torrent(&engine, id).await, checked, "not checked over and over");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_restart_only_the_file_put_back_counts_not_the_one_librqbit_makes_again() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = engine.add(complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    let trash = dir.path().join("b.bin");
    std::fs::rename(file(dir.path(), "b.bin"), &trash).unwrap();

    let engine = engine(dir.path()).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(file(dir.path(), "b.bin").exists(), "librqbit made it again");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "which isn't the file put back");

    std::fs::rename(&trash, file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::Done).await;
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), FULL, false));
    engine.stop().await;
}
