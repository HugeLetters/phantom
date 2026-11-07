//! The engine against a real librqbit session in a temp dir, offline. Magnets name a tracker on
//! a closed local port, so their info is never found and they stay fetching info.

use std::path::Path;

use proto::{AddSource, Added, Priority, Status, TorrentInfo};
use tempfile::TempDir;

use super::{Config, Engine};

const HASH: &str = "00000000000000000000000000000000000000aa";

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
    Engine::start(Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
    })
    .await
    .unwrap()
}

fn temp() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    dir
}

fn find(engine: &Engine, id: u64) -> TorrentInfo {
    engine.listing().into_iter().find(|t| t.id == id).unwrap_or_else(|| panic!("no torrent {id}"))
}

/// Waits up to 10 seconds for the torrent's status, e.g. while its files are checked.
async fn wait_for(engine: &Engine, id: u64, status: Status) {
    for _ in 0..100 {
        if find(engine, id).status == status {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("torrent {id} is {:?}, not {status:?}", find(engine, id).status);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_added_magnet_is_a_torrent_fetching_info() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    assert_eq!(added, Added { id: 0, name: Some("Big Buck Bunny".into()), already_existed: false });

    let listing = engine.listing();
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
    assert_eq!(engine.listing().len(), 1, "no second row");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_torrent_fetching_info_stops_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    engine.remove(added.id).await.unwrap();
    assert!(engine.listing().is_empty());
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
    assert_ne!(find(&engine, added.id).status, Status::FetchingInfo, "a file brings its info");

    let again = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(again, Added { already_existed: true, ..added.clone() });

    let hash = find(&engine, added.id).info_hash.unwrap();
    let by_magnet = engine.add(magnet(&hash), false).await.unwrap();
    assert_eq!((by_magnet.id, by_magnet.already_existed), (added.id, true), "found by the magnet's hash too");
    assert_eq!(engine.listing().len(), 1);
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
    assert_eq!(find(&engine, added.id).status, Status::Paused, "incomplete, so not Done");
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
    assert!(engine.listing().is_empty());
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
    let listing = engine.listing();
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
    let details = engine.details(added.id).unwrap();
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
    let t = find(&engine, added.id);
    assert_eq!((t.size_bytes, t.full_size_bytes), (3 * 16384, 3 * 16384));

    let single = engine.add(torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(engine.details(single.id).unwrap().location, dir.path().join("downloads/a.bin"), "one file is itself");
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

        let t = find(&engine, added.id);
        assert_eq!(t.size_bytes, 2 * 16384, "Size leaves out the file set Off");
        assert_eq!(t.full_size_bytes, 3 * 16384, "Full size doesn't");
        let priorities: Vec<_> = engine.details(added.id).unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal]);
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    let priorities: Vec<_> = engine.details(id).unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal], "kept across the restart");
    assert_eq!(find(&engine, id).size_bytes, 2 * 16384);

    wait_for(&engine, id, Status::Downloading).await;
    engine.set_priority(id, &[0, 1], Priority::Normal).await.unwrap();
    assert_eq!(find(&engine, id).size_bytes, 3 * 16384, "Normal again, several at once");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_file_off_is_refused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(multi_file_torrent(&["a.bin", "b.bin"]), false).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.set_priority(added.id, &[0], Priority::Off).await.unwrap();
    let refused = engine.set_priority(added.id, &[1], Priority::Off).await.unwrap_err();
    assert!(refused.to_string().contains("remove the torrent"), "says what to do instead: {refused}");
    assert!(engine.set_priority(added.id, &[2], Priority::Off).await.is_err(), "no file 2");
    assert_eq!(find(&engine, added.id).size_bytes, 16384, "unchanged");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_info_has_no_details_yet() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = engine.add(magnet(HASH), false).await.unwrap();
    let refused = engine.details(added.id).unwrap_err();
    assert!(refused.to_string().contains("no info yet"), "{refused}");
    assert!(engine.set_priority(added.id, &[0], Priority::Off).await.is_err());
    engine.stop().await;
}
