//! The engine against a real librqbit session in a temp dir, offline. Magnets name a tracker on
//! a closed local port, so their info is never found and they stay fetching info.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use proto::{AbsolutePath, Action, AddSource, Added, Priority, Status, TorrentCommand, TorrentInfo, TorrentRefusal, Unavailable};
use tempfile::TempDir;

use super::{Config, Destination, Engine, Failure};

const HASH: &str = "00000000000000000000000000000000000000aa";
/// Short, so tests see missing files quickly.
const SIZE_CHECK_EVERY: Duration = Duration::from_millis(200);

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
    let mut file = format!("d4:infod5:filesl{files}e4:name4:pack12:piece lengthi16384e6:pieces{}:", 20 * paths.len()).into_bytes();
    file.extend(vec![7; 20 * paths.len()]);
    file.extend(b"ee");
    AddSource::TorrentFile(file)
}

async fn engine(dir: &Path) -> Engine {
    engine_size_checking_every(dir, SIZE_CHECK_EVERY).await
}

async fn engine_size_checking_every(dir: &Path, size_check_every: Duration) -> Engine {
    Engine::start(Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every,
    })
    .await
    .unwrap()
}

/// Adds into the engine's download dir, made first: the engine adds only into a destination there
/// is. A .torrent file's add is answered while librqbit takes it, a moment: this waits for that.
async fn add(engine: &Engine, download_dir: &Path, source: AddSource, paused: bool) -> anyhow::Result<Added> {
    std::fs::create_dir_all(download_dir).unwrap();
    add_to(engine, source, paused, download_dir).await
}

async fn add_to(engine: &Engine, source: AddSource, paused: bool, destination: &Path) -> anyhow::Result<Added> {
    let file = matches!(source, AddSource::TorrentFile(_));
    let added = engine.add(source, paused, parsed(destination)).await?;
    if file {
        for _ in 0..1000 {
            if engine.listing().await.iter().any(|t| t.id == added.id && t.status != Status::FetchingInfo) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(added)
}

/// The torrent refusal `result` is: which torrent, and why.
fn refusal<T: std::fmt::Debug>(result: Result<T, Failure>) -> (u64, TorrentRefusal) {
    match result.unwrap_err() {
        Failure::Refused(proto::Refusal::Torrent { id, why }) => (id, why),
        other => panic!("not a torrent refusal: {other:?}"),
    }
}

/// `folder`, which the test made, as a destination.
fn parsed(folder: &Path) -> Destination {
    Destination::parse(AbsolutePath::new(folder).unwrap()).unwrap()
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
    for _ in 0..1000 {
        if find(engine, id).await.status == status {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("torrent {id} is {:?}, not {status:?}", find(engine, id).await.status);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_added_magnet_is_a_torrent_fetching_info() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    assert_eq!(added, Added { id: 0, name: Some("Big Buck Bunny".into()), already_existed: false });

    let listing = engine.listing().await;
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].id, 0);
    assert_eq!(listing[0].status, Status::FetchingInfo);
    assert_eq!(listing[0].name.as_deref(), Some("Big Buck Bunny"));
    assert_eq!(listing[0].info_hash, HASH);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_a_magnet_again_answers_with_the_torrent_already_in_the_list() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let first = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    let again = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    assert_eq!(again, Added { already_existed: true, ..first });
    assert_eq!(engine.listing().await.len(), 1, "no second row");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_torrent_fetching_info_stops_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    engine.command(added.id, TorrentCommand::Remove).await.unwrap();
    assert!(engine.listing().await.is_empty());
    assert!(engine.command(added.id, TorrentCommand::Pause).await.is_err(), "it's gone");

    let again = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    assert!(!again.already_existed, "it can be added again");
    assert_ne!(again.id, added.id, "as a new torrent");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_file_is_added_once() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(added.name.as_deref(), Some("a.bin"));
    assert!(!added.already_existed);
    assert_ne!(find(&engine, added.id).await.status, Status::FetchingInfo, "a file brings its info");

    let again = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(again, Added { already_existed: true, ..added.clone() });

    let hash = find(&engine, added.id).await.info_hash;
    let by_magnet = add(&engine, &dir.path().join("downloads"), magnet(&hash), false).await.unwrap();
    assert_eq!((by_magnet.id, by_magnet.already_existed), (added.id, true), "found by the magnet's hash too");
    assert_eq!(engine.listing().await.len(), 1);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_info_can_be_paused_and_resumed() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    assert_eq!(find(&engine, id).await.actions, [Action::Pause, Action::Remove, Action::Delete]);
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    let paused = find(&engine, id).await;
    assert_eq!((paused.status, paused.actions), (Status::FetchingInfo, vec![Action::Resume, Action::Remove, Action::Delete]));
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    assert_eq!(find(&engine, id).await.actions, [Action::Pause, Action::Remove, Action::Delete]);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_paused_while_fetching_info_lands_paused_without_downloading() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    // Not waited for: the add is still fetching info when Pause comes.
    let id = engine.add(torrent_file("a.bin"), false, parsed(&downloads)).await.unwrap().id;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    for _ in 0..1000 {
        let status = find(&engine, id).await.status;
        assert_ne!(status, Status::Downloading, "paused before its info arrived");
        if status == Status::Paused {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(find(&engine, id).await.status, Status::Paused);
    let live = engine.inspect(move |actor| actor.torrents[&id].handle.as_ref().map(|h| h.is_paused())).await;
    assert_eq!(live, Some(true), "librqbit didn't start it");
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_incomplete_torrent_is_paused_not_done() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.command(added.id, TorrentCommand::Pause).await.unwrap();
    assert_eq!(find(&engine, added.id).await.status, Status::Paused, "incomplete, so not Done");
    engine.command(added.id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_the_torrent_and_its_files() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    let file = dir.path().join("downloads/a.bin");
    assert!(file.exists(), "adding creates the file");
    engine.command(added.id, TorrentCommand::Delete).await.unwrap();
    assert!(engine.listing().await.is_empty());
    assert!(!file.exists());
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn torrents_added_at_the_same_time_are_each_in_librqbit_and_survive_a_restart() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let names: Vec<String> = (0..32).map(|n| format!("{n:02}.bin")).collect();
    let ids: Vec<u64> = {
        let engine = std::sync::Arc::new(engine(dir.path()).await);
        let mut adds = tokio::task::JoinSet::new();
        for name in &names {
            let (engine, downloads, source) = (engine.clone(), downloads.clone(), torrent_file(name));
            adds.spawn(async move { add(&engine, &downloads, source, true).await.unwrap().id });
        }
        let mut ids: Vec<u64> = adds.join_all().await;
        ids.sort();
        let in_librqbit = engine.inspect(|actor| actor.session.with_torrents(|ts| ts.count())).await;
        assert_eq!(in_librqbit, names.len(), "each its own torrent in librqbit");
        engine.stop().await;
        ids
    };
    let engine = engine(dir.path()).await;
    let listed: Vec<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, ids, "all of them, after a restart");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ids_survive_a_restart() {
    let dir = temp();
    let (a, b) = {
        let engine = engine(dir.path()).await;
        // librqbit numbers only the torrents it has, from 0; ours count the magnet too.
        let fetching = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
        let a = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
        let b = add(&engine, &dir.path().join("downloads"), torrent_file("b.bin"), false).await.unwrap();
        assert_eq!((fetching.id, a.id, b.id), (0, 1, 2));
        engine.command(a.id, TorrentCommand::Remove).await.unwrap();
        engine.stop().await;
        (a, b)
    };

    let engine = engine(dir.path()).await;
    let listing = engine.listing().await;
    let ids: Vec<_> = listing.iter().map(|t| (t.id, t.name.clone())).collect();
    assert_eq!(ids, [(b.id, b.name.clone())], "b keeps its id; the magnet fetching info is gone");
    let next = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    assert!(next.id > b.id && next.id != a.id, "ids aren't reused");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_lists_its_files_and_where_they_are() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), multi_file_torrent(&["a.bin", "b.bin", "extras/c.bin"]), false).await.unwrap();
    let details = engine.details(added.id).await.unwrap();
    let files: Vec<_> = details.files.iter().map(|f| (f.index, f.path.to_str().unwrap(), f.size_bytes, f.priority)).collect();
    assert_eq!(
        files,
        [(0, "a.bin", 16384, Priority::Normal), (1, "b.bin", 16384, Priority::Normal), (2, "extras/c.bin", 16384, Priority::Normal),]
    );
    assert_eq!(details.location, dir.path().join("downloads/pack"), "a torrent of several files has a folder");
    let t = find(&engine, added.id).await;
    assert_eq!((t.size_bytes, t.full_size_bytes), (3 * 16384, 3 * 16384));

    let single = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    assert_eq!(engine.details(single.id).await.unwrap().location, dir.path().join("downloads/a.bin"), "one file is itself");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_leaves_the_size_and_stays_off_after_a_restart() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let added = add(&engine, &dir.path().join("downloads"), multi_file_torrent(&["a.bin", "b.bin", "c.bin"]), false).await.unwrap();
        wait_for(&engine, added.id, Status::Downloading).await;
        engine.command(added.id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();

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
    engine.command(id, TorrentCommand::SetPriority { files: vec![0, 1], priority: Priority::Normal }).await.unwrap();
    assert_eq!(find(&engine, id).await.size_bytes, 3 * 16384, "Normal again, several at once");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_file_off_finishes_the_torrent() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let added = add(&engine, &dir.path().join("downloads"), multi_file_torrent(&["a.bin", "b.bin"]), false).await.unwrap();
        wait_for(&engine, added.id, Status::Downloading).await;
        engine.command(added.id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
        engine.command(added.id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
        assert!(
            engine.command(added.id, TorrentCommand::SetPriority { files: vec![2], priority: Priority::Off }).await.is_err(),
            "no file 2"
        );

        let t = find(&engine, added.id).await;
        assert_eq!((t.size_bytes, t.full_size_bytes, t.progress_bytes), (0, 2 * 16384, 0), "nothing left to get");
        assert_eq!(t.status, Status::Seeding, "so it's finished");
        let priorities: Vec<_> = engine.details(added.id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Off, Priority::Off]);

        engine.command(added.id, TorrentCommand::Pause).await.unwrap();
        assert_eq!(find(&engine, added.id).await.status, Status::Done);
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    wait_for(&engine, id, Status::Done).await;
    assert_eq!(find(&engine, id).await.size_bytes, 0, "kept across the restart");

    engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Normal }).await.unwrap();
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
    let added = add(&engine, &dir.join("downloads"), AddSource::TorrentFile(file), false).await.unwrap();
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

    engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "still checking");
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "shown as asked");
    assert_eq!(find(&engine, id).await.size_bytes, 64 << 20, "and out of the Size");

    // Changes during the check build on each other.
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
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
        engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
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
        let running = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap().id;
        let paused = add(&engine, &dir.path().join("downloads"), torrent_file("b.bin"), false).await.unwrap().id;
        let added_paused = add(&engine, &dir.path().join("downloads"), torrent_file("c.bin"), true).await.unwrap().id;
        wait_for(&engine, running, Status::Downloading).await;
        wait_for(&engine, paused, Status::Downloading).await;
        engine.command(paused, TorrentCommand::Pause).await.unwrap();
        wait_for(&engine, added_paused, Status::Paused).await;
        engine.stop().await;
        (running, paused, added_paused)
    };
    let saved = std::fs::read_to_string(dir.path().join("data/engine.json")).unwrap();
    assert_eq!(saved.matches("\"wants\": \"run\"").count(), 1, "{saved}");
    let destination = format!("\"destination\": {:?}", dir.path().join("downloads").to_str().unwrap());
    assert_eq!(saved.matches(&destination).count(), 3, "each torrent's destination: {saved}");

    let engine = engine(dir.path()).await;
    wait_for(&engine, running, Status::Downloading).await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(find(&engine, paused).await.status, Status::Paused, "paused by the user");
    assert_eq!(find(&engine, added_paused).await.status, Status::Paused, "added paused");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_added_paused_checks_then_stops() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), true).await.unwrap();
    wait_for(&engine, added.id, Status::Paused).await;
    engine.command(added.id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, added.id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pausing_during_a_check_lets_it_end_then_the_torrent_stays_paused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "the check goes on");
    wait_for(&engine, id, Status::Paused).await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "stays paused");
    assert!(!librqbit_live(&engine, id).await);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_command_is_answered_in_every_phase_it_can_be_in() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let fetching = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    let downloading = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap().id;
    let paused = add(&engine, &dir.path().join("downloads"), torrent_file("b.bin"), true).await.unwrap().id;
    let checking = checking_torrent(&engine, dir.path()).await;
    wait_for(&engine, downloading, Status::Downloading).await;
    wait_for(&engine, paused, Status::Paused).await;

    for id in [fetching, downloading, paused, checking, 99] {
        let answered = async {
            let _ = engine.details(id).await;
            let _ = engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Normal }).await;
            let _ = engine.command(id, TorrentCommand::Pause).await;
            let _ = engine.command(id, TorrentCommand::Pause).await;
            let _ = engine.command(id, TorrentCommand::Resume).await;
            let _ = engine.command(id, TorrentCommand::Resume).await;
            let _ = engine.command(id, TorrentCommand::Remove).await;
            let _ = engine.command(id, TorrentCommand::Delete).await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), answered).await.expect("every command answered");
    }
    assert!(engine.listing().await.is_empty(), "all removed");

    engine.stop().await;
    let after = tokio::time::timeout(std::time::Duration::from_secs(1), engine.command(downloading, TorrentCommand::Pause)).await;
    assert_eq!(after.expect("answered after stopping too").unwrap_err().to_string(), "the service is stopping");
}

#[tokio::test(flavor = "multi_thread")]
async fn refusals_say_why_as_data() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let fetching = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    assert_eq!(refusal(engine.command(fetching, TorrentCommand::Check).await), (fetching, TorrentRefusal::NoInfoYet));
    assert_eq!(refusal(engine.command(99, TorrentCommand::Pause).await), (99, TorrentRefusal::NotInList));

    let id = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;
    let off = TorrentCommand::SetPriority { files: vec![5], priority: Priority::Off };
    assert_eq!(refusal(engine.command(id, off).await), (id, TorrentRefusal::NoFile { index: 5 }));
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_info_has_no_details_yet() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let added = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap();
    assert_eq!(refusal(engine.details(added.id).await), (added.id, TorrentRefusal::NoInfoYet));
    assert!(engine.command(added.id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.is_err());
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
    let mut files: Vec<_> = details.files.iter().map(|f| (f.path.to_string_lossy().into_owned(), f.progress_bytes, f.missing)).collect();
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
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
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
    assert_eq!(engine.command(id, TorrentCommand::Pause).await.unwrap(), (), "pausing is nothing to do");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_of_a_size_it_cannot_have_is_missing_too() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Done);

    std::fs::File::options().write(true).open(file(dir.path(), "c.bin")).unwrap().set_len(10).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(files_on(&engine, id).await[2], ("c.bin".into(), 0, true), "cut short");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_watcher_sees_a_torrent_folder_go_without_waiting_for_the_regular_look() {
    let dir = temp();
    let engine = engine_size_checking_every(dir.path(), Duration::from_secs(3600)).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // The first size check, at start, ran before the torrent was added; this one watches its destination.
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
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = librqbit_torrent(&engine, id).await;

    engine.command(id, TorrentCommand::Resume).await.unwrap();
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
async fn setting_the_missing_file_off_runs_the_torrent_again_as_it_was() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    let b = index_of(&engine, id, "b.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "everything it's getting is there");
    assert!(files_on(&engine, id).await.iter().all(|(_, _, missing)| !missing));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "an Off file is never missing");

    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn setting_the_missing_file_off_of_a_paused_torrent_leaves_it_paused() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = complete_torrent(dir.path()).await;
    std::fs::write(file(dir.path(), "c.bin"), vec![0; 2 * PIECE]).unwrap();
    let id = add(&engine, &dir.path().join("downloads"), source, true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    std::fs::remove_file(file(dir.path(), "a.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    let a = index_of(&engine, id, "a.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![a], priority: Priority::Off }).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Paused, "c.bin is still to download");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_while_the_service_was_stopped_is_missing_on_start_and_not_resumed() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
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
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn check_finds_a_changed_file_keeps_what_is_valid_and_keeps_the_torrent_running_or_stopped() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;

    // The same size, other bytes: only a check sees it.
    std::fs::write(file(dir.path(), "b.bin"), vec![0; 2 * PIECE]).unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "not missing");

    let before = librqbit_torrent(&engine, id).await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added to librqbit again");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(files_on(&engine, id).await, [("a.bin".into(), FULL, false), ("b.bin".into(), 0, false), ("c.bin".into(), FULL, false)]);

    engine.command(id, TorrentCommand::Pause).await.unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Paused).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "a stopped torrent stays stopped");

    let fetching = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    assert_eq!(refusal(engine.command(fetching, TorrentCommand::Check).await), (fetching, TorrentRefusal::NoInfoYet));
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn checking_a_torrent_missing_files_runs_it_as_it_was() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;

    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(files_on(&engine, id).await[1].0, "b.bin");
    assert!(!files_on(&engine, id).await[1].2, "no longer missing: to download");
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

    engine.command(id, TorrentCommand::Resume).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "128 MiB take a while to check");
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// Removes or deletes the torrent, adds it again, and checks it's listed and takes commands.
async fn removed_then_added_again(engine: &Engine, dir: &Path, source: impl Fn() -> AddSource, delete: bool) -> Added {
    let first = add(&engine, &dir.join("downloads"), source(), false).await.unwrap();
    if delete {
        engine.command(first.id, TorrentCommand::Delete).await.unwrap();
    } else {
        engine.command(first.id, TorrentCommand::Remove).await.unwrap();
    }
    assert!(engine.listing().await.is_empty());
    let again = add(&engine, &dir.join("downloads"), source(), false).await.unwrap();
    assert!(!again.already_existed);
    assert_ne!(again.id, first.id, "a re-added torrent is a new one");
    assert_eq!(find(engine, again.id).await.id, again.id, "it's listed");
    engine.command(again.id, TorrentCommand::Pause).await.unwrap();
    again
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_torrent_file_can_be_added_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let again = removed_then_added_again(&engine, dir.path(), || torrent_file("a.bin"), false).await;
    engine.command(again.id, TorrentCommand::Remove).await.unwrap();
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_torrent_file_can_be_added_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    removed_then_added_again(&engine, dir.path(), || torrent_file("a.bin"), true).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_torrent_can_be_added_again_by_its_magnet() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let first = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    let hash = find(&engine, first.id).await.info_hash;
    engine.command(first.id, TorrentCommand::Remove).await.unwrap();
    let again = add(&engine, &dir.path().join("downloads"), magnet(&hash), false).await.unwrap();
    assert!(!again.already_existed);
    assert_eq!(find(&engine, again.id).await.id, again.id, "it's listed");
    engine.command(again.id, TorrentCommand::Remove).await.unwrap();
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_removed_before_a_restart_can_be_added_again() {
    let dir = temp();
    let first = {
        let engine = engine(dir.path()).await;
        let first = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
        engine.command(first.id, TorrentCommand::Remove).await.unwrap();
        engine.stop().await;
        first
    };
    let engine = engine(dir.path()).await;
    assert!(engine.listing().await.is_empty());
    let again = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
    assert!(!again.already_existed);
    assert_ne!(again.id, first.id);
    assert_eq!(find(&engine, again.id).await.id, again.id, "it's listed");
    engine.command(again.id, TorrentCommand::Remove).await.unwrap();
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
    let first = add(&engine_a, &dir.path().join("downloads"), again(&source), false).await.unwrap();
    wait_for(&engine_a, first.id, Status::Seeding).await;
    engine_a.command(first.id, TorrentCommand::Remove).await.unwrap();
    assert!(engine_a.listing().await.is_empty());
    let second = add(&engine_a, &dir.path().join("downloads"), again(&source), false).await.unwrap();
    assert!(!second.already_existed);
    wait_for(&engine_a, second.id, Status::Seeding).await;
    engine_a.command(second.id, TorrentCommand::Pause).await.unwrap();
    engine_a.command(second.id, TorrentCommand::Remove).await.unwrap();
    engine_a.stop().await;

    let engine_b = engine(dir.path()).await;
    assert!(engine_b.listing().await.is_empty());
    let third = add(&engine_b, &dir.path().join("downloads"), again(&source), false).await.unwrap();
    wait_for(&engine_b, third.id, Status::Seeding).await;
    engine_b.command(third.id, TorrentCommand::Delete).await.unwrap();
    let fourth = add(&engine_b, &dir.path().join("downloads"), again(&source), false).await.unwrap();
    assert_eq!(find(&engine_b, fourth.id).await.id, fourth.id, "it's listed");
    engine_b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_file_put_back_is_checked_and_the_torrent_runs_again_as_it_was() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let trash = dir.path().join("b.bin");
    std::fs::rename(file(dir.path(), "b.bin"), &trash).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    let before = librqbit_torrent(&engine, id).await;

    std::fs::rename(&trash, file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added again, so its files were checked");
    assert_eq!(files_on(&engine, id).await, [("a.bin".into(), FULL, false), ("b.bin".into(), FULL, false), ("c.bin".into(), FULL, false)]);

    engine.command(id, TorrentCommand::Pause).await.unwrap();
    std::fs::rename(file(dir.path(), "b.bin"), &trash).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    std::fs::rename(&trash, file(dir.path(), "b.bin")).unwrap();
    wait_for(&engine, id, Status::Done).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Done, "a paused one stays paused");
    assert!(!librqbit_live(&engine, id).await);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_put_back_that_is_not_the_one_that_went_leaves_the_torrent_missing_files() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
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
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
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
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), FULL, false));
    engine.stop().await;
}

/// `dir/elsewhere`, made: a destination other than the download dir.
fn elsewhere(dir: &Path) -> PathBuf {
    let folder = dir.join("elsewhere");
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn folder_gone(folder: &Path) -> Status {
    Status::DestinationUnavailable { reason: proto::Unavailable::FolderMissing { path: AbsolutePath::new(folder).unwrap() } }
}

/// Moves `folder` out of the way, as a drive taken out does, and back.
fn take_away(folder: &Path) {
    std::fs::rename(folder, folder.with_extension("away")).unwrap();
}

fn bring_back(folder: &Path) {
    std::fs::rename(folder.with_extension("away"), folder).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_goes_into_the_destination_it_is_added_with() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let destination = elsewhere(dir.path());
    let single = add_to(&engine, torrent_file("a.bin"), false, &destination).await.unwrap().id;
    let several = add_to(&engine, multi_file_torrent(&["a.bin", "b.bin"]), false, &destination).await.unwrap().id;
    assert_eq!(engine.details(single).await.unwrap().location, destination.join("a.bin"), "a single file straight in");
    assert_eq!(engine.details(several).await.unwrap().location, destination.join("pack"), "several in their own folder");
    wait_for(&engine, several, Status::Downloading).await;
    assert!(destination.join("pack/b.bin").exists());
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_destination_that_cannot_be_reached_does_not_parse_and_nothing_is_created() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let nowhere = dir.path().join("nowhere");
    let nowhere = AbsolutePath::new(nowhere).unwrap();
    let refused = Destination::parse(nowhere.clone()).unwrap_err();
    assert_eq!(refused, Unavailable::FolderMissing { path: nowhere.clone() });
    assert!(!nowhere.exists());
    assert!(engine.listing().await.is_empty());
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_destination_that_goes_stops_the_torrent_until_it_is_back_then_it_checks_and_runs_again() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let destination = elsewhere(dir.path());
    let id = add_to(&engine, torrent_file("a.bin"), false, &destination).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;

    take_away(&destination);
    wait_for(&engine, id, folder_gone(&destination)).await;
    assert!(!librqbit_live(&engine, id).await, "stopped in librqbit");
    let reason = Unavailable::FolderMissing { path: AbsolutePath::new(&destination).unwrap() };
    let why = TorrentRefusal::DestinationUnavailable { action: proto::Action::Resume, reason };
    assert_eq!(refusal(engine.command(id, TorrentCommand::Resume).await), (id, why));
    assert!(engine.command(id, TorrentCommand::Check).await.is_err());
    let (_, why) = refusal(engine.command(id, TorrentCommand::Delete).await);
    assert!(matches!(why, TorrentRefusal::DestinationUnavailable { action: proto::Action::Delete, .. }));
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert!(!destination.exists(), "never made again");

    let before = librqbit_torrent(&engine, id).await;
    bring_back(&destination);
    wait_for(&engine, id, Status::Downloading).await;
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added again, which checks its files");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_torrent_whose_destination_comes_back_stays_stopped_and_so_does_one_paused_meanwhile() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let destination = elsewhere(dir.path());
    let paused = add_to(&engine, torrent_file("a.bin"), true, &destination).await.unwrap().id;
    let running = add_to(&engine, torrent_file("b.bin"), false, &destination).await.unwrap().id;
    wait_for(&engine, paused, Status::Paused).await;
    wait_for(&engine, running, Status::Downloading).await;

    take_away(&destination);
    wait_for(&engine, paused, folder_gone(&destination)).await;
    wait_for(&engine, running, folder_gone(&destination)).await;
    engine.command(running, TorrentCommand::Pause).await.unwrap();
    bring_back(&destination);
    wait_for(&engine, paused, Status::Paused).await;
    wait_for(&engine, running, Status::Paused).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_destination_away_at_start_is_not_created_and_a_torrent_running_before_runs_once_it_is_back() {
    let dir = temp();
    let destination = elsewhere(dir.path());
    let id = {
        let engine = engine(dir.path()).await;
        let id = add_to(&engine, multi_file_torrent(&["a.bin", "b.bin"]), false, &destination).await.unwrap().id;
        wait_for(&engine, id, Status::Downloading).await;
        engine.stop().await;
        id
    };
    take_away(&destination);

    let engine = engine(dir.path()).await;
    assert_eq!(find(&engine, id).await.status, folder_gone(&destination));
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert!(!destination.exists(), "librqbit restored it without making its folder");
    assert_eq!(find(&engine, id).await.status, folder_gone(&destination), "not Missing files");

    bring_back(&destination);
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;

    // Still away at the next stop and start: still to run once back.
    take_away(&destination);
    let engine = engine_after_stop(dir.path(), id, &destination).await;
    bring_back(&destination);
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// Starts the engine with `destination` away, waits for torrent `id` to be Destination
/// unavailable, and starts it once more, as a service restarted while a drive is out.
async fn engine_after_stop(dir: &Path, id: u64, destination: &Path) -> Engine {
    let first = engine(dir).await;
    wait_for(&first, id, folder_gone(destination)).await;
    first.stop().await;
    let again = engine(dir).await;
    wait_for(&again, id, folder_gone(destination)).await;
    again
}

/// Serves `bytes` at `/a.torrent` on a local port, and 404 for anything else, until the test ends.
fn serve_torrent(bytes: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0; 1024];
            let n = stream.read(&mut request).unwrap_or(0);
            let found = String::from_utf8_lossy(&request[..n]).starts_with("GET /a.torrent ");
            let (status, body): (_, &[u8]) = if found { ("200 OK", &bytes) } else { ("404 Not Found", b"") };
            let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body));
        }
    });
    format!("http://{address}")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_url_is_downloaded_before_the_add_so_it_is_answered_at_once_with_its_hash() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let AddSource::TorrentFile(bytes) = torrent_file("a.bin") else { unreachable!() };
    let server = serve_torrent(bytes);
    let url = || AddSource::Url(format!("{server}/a.torrent"));

    let added = add(&engine, &dir.path().join("downloads"), url(), false).await.unwrap();
    assert_eq!((added.name.as_deref(), added.already_existed), (Some("a.bin"), false));
    let hash = find(&engine, added.id).await.info_hash;
    assert_eq!(add(&engine, &dir.path().join("downloads"), url(), false).await.unwrap(), Added { already_existed: true, ..added.clone() });
    assert_eq!(
        add(&engine, &dir.path().join("downloads"), magnet(&hash), false).await.unwrap().id,
        added.id,
        "the same torrent by its magnet"
    );

    // Removed at once: nothing is left in librqbit (#32).
    engine.command(added.id, TorrentCommand::Remove).await.unwrap();
    let left = engine.inspect(|actor| actor.session.with_torrents(|ts| ts.count())).await;
    assert_eq!(left, 0);

    let missing =
        add(&engine, &dir.path().join("downloads"), AddSource::Url(format!("{server}/nothing.torrent")), false).await.unwrap_err();
    assert!(missing.to_string().contains("404"), "{missing:#}");
    assert!(engine.listing().await.is_empty(), "a failed download adds nothing");
    engine.stop().await;
}

/// A seeder of one 48 MiB file in `dir` with a limited upload speed, on a local port: the .torrent,
/// its info hash and the port. The session is kept alive by the caller.
async fn seeder(dir: &Path) -> (std::sync::Arc<librqbit::Session>, Vec<u8>, String, u16) {
    use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, SessionOptions, create_torrent};
    use std::io::Write;
    let files = dir.join("seed");
    std::fs::create_dir_all(&files).unwrap();
    let mut out = std::fs::File::create(files.join("big.bin")).unwrap();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut chunk = vec![0u8; 1 << 20];
    for _ in 0..48 {
        for b in chunk.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        out.write_all(&chunk).unwrap();
    }
    drop(out);
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    // A tracker that names the seeder to whoever asks.
    let tracker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut peers = vec![127, 0, 0, 1];
        peers.extend(port.to_be_bytes());
        let mut body = b"d8:intervali60e5:peers6:".to_vec();
        body.extend(peers);
        body.push(b'e');
        for stream in tracker.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.read(&mut [0; 2048]);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body));
        }
    });
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(2);
    let options = CreateTorrentOptions { piece_length: Some(256 * 1024), trackers: vec![announce], ..Default::default() };
    let created = create_torrent(&files.join("big.bin"), options, &spawner).await.unwrap();
    let bytes = created.as_bytes().unwrap().to_vec();
    let session = librqbit::Session::new_with_opts(
        dir.join("seed-session"),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], port).into(), ..Default::default() }),
            ratelimits: librqbit::limits::LimitsConfig { upload_bps: std::num::NonZeroU32::new(8 << 20), download_bps: None },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let opts = AddTorrentOptions { overwrite: true, output_folder: Some(files.to_str().unwrap().into()), ..Default::default() };
    let added = session.add_torrent(AddTorrent::from_bytes(bytes.clone()), Some(opts)).await.unwrap();
    let hash = added.into_handle().unwrap().info_hash().as_string();
    (session, bytes, hash, port)
}

/// Pieces librqbit has saved of the torrent, in bytes of progress (256 KiB each).
fn saved_bytes(data: &Path, hash: &str) -> u64 {
    let bits = std::fs::read(super::bitv::path(&data.join("session"), hash)).unwrap_or_default();
    super::bitv::pieces(&bits) * 256 * 1024
}

#[tokio::test(flavor = "multi_thread")]
async fn what_was_downloaded_is_kept_when_the_service_stops() {
    let dir = temp();
    let (_seeder, torrent, hash, _) = seeder(dir.path()).await;
    let data = dir.path().join("data");
    let engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    // Past the first save librqbit makes by itself (16 MiB), short of the whole torrent. Past
    // Checking too: until then progress is how much has been checked.
    let mut before = 0;
    for _ in 0..600 {
        let t = find(&engine, id).await;
        before = t.progress_bytes;
        if t.status == Status::Downloading && before > 20 << 20 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(before > 20 << 20 && before < 48 << 20, "{before} is mid-download");
    engine.stop().await;
    assert!(saved_bytes(&data, &hash) >= before, "librqbit saved {} of {before}", saved_bytes(&data, &hash));

    let restarted = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    // librqbit checks the files at start; the progress kept is what's there after.
    let mut after = 0;
    for _ in 0..600 {
        let t = restarted.listing().await.remove(0);
        after = t.progress_bytes;
        if t.status != Status::Checking {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(after >= before, "{after} after the restart, {before} before the stop");
    restarted.stop().await;
}
