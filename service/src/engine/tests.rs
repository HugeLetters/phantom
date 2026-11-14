//! The engine against a real librqbit session in a temp dir, offline. Magnets name a tracker on
//! a closed local port, so their info is never found and they stay fetching info.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::{AbsolutePath, Action, AddSource, Added, Priority, Status, TorrentCommand, TorrentInfo, TorrentRefusal, Unavailable};
use tempfile::TempDir;

use super::{Config, Destination, Engine, Failure};

const HASH: &str = "00000000000000000000000000000000000000aa";
/// Short, so tests see missing files quickly.
const SIZE_CHECK_EVERY: Duration = Duration::from_millis(200);

fn magnet(hash: &str) -> AddSource {
    AddSource::Magnet(format!("magnet:?xt=urn:btih:{hash}&dn=Big+Buck+Bunny&tr=http://127.0.0.1:1/announce"))
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
    engine_with(dir, size_check_every, Default::default(), Default::default()).await
}

/// With a trash that keeps the paths in `refusing`, and waits while `gate` is held.
async fn engine_with(dir: &Path, size_check_every: Duration, refusing: Arc<Mutex<Vec<PathBuf>>>, gate: super::trash::Gate) -> Engine {
    Engine::start(Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every,
        trash: super::trash::into(dir.join("trash"), refusing, gate),
        speed_limits: Default::default(),
        seeding: Default::default(),
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
    let added = engine.add(super::fetched(source).await?, paused, parsed(destination)).await?;
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

/// Waits up to 10 seconds for the torrent's status to be one `want` takes.
async fn until(engine: &Engine, id: u64, want: impl Fn(&Status) -> bool) {
    for _ in 0..1000 {
        if want(&find(engine, id).await.status) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("torrent {id} is {:?}", find(engine, id).await.status);
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

/// A bare info hash, as the client sends it: the magnet link of it, which librqbit takes.
#[tokio::test(flavor = "multi_thread")]
async fn an_info_hash_on_its_own_is_added() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = client::add_source(HASH).unwrap();
    let added = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap();
    assert_eq!(find(&engine, added.id).await.info_hash, HASH);
    engine.stop().await;
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
    assert_eq!(listing[0].trackers, ["http://127.0.0.1:1/announce"], "the magnet's trackers, for its own magnet link");
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

/// The actions that do something to `t` now, in their order.
fn offered(t: &TorrentInfo) -> Vec<Action> {
    use Action::*;
    [Pause, Resume, Check, SetPriority, Remove, Delete, Open, Reveal].into_iter().filter(|a| t.actions.does(*a)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_fetching_info_can_be_paused_and_resumed() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    // Delete would remove it only: it has no files yet.
    assert_eq!(offered(&find(&engine, id).await), [Action::Pause, Action::Remove]);
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    let paused = find(&engine, id).await;
    assert_eq!((paused.status.clone(), offered(&paused)), (Status::FetchingInfo, vec![Action::Resume, Action::Remove]));
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    assert_eq!(offered(&find(&engine, id).await), [Action::Pause, Action::Remove]);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_paused_while_fetching_info_lands_paused_without_downloading() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    // Not waited for: the add is still fetching info when Pause comes.
    let id = engine.add(super::Fetched::local(torrent_file("a.bin")), false, parsed(&downloads)).await.unwrap().id;
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
    let live = engine.inspect(move |actor| actor.torrents[&id].inlib.info().map(|h| h.is_paused())).await;
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

/// A Delete that moved only some files waits with what's left, across a restart: librqbit's
/// restore makes none of the trashed files again. Tried again, it finishes.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_delete_comes_back_after_a_restart_and_can_be_tried_again() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let refusing = Arc::new(Mutex::new(Vec::new()));
    let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default());
    let engine = start().await;
    let added = add(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
    let (a, b, mine) = (downloads.join("pack/a.bin"), downloads.join("pack/b.bin"), downloads.join("pack/mine.txt"));
    assert!(a.exists() && b.exists(), "adding creates the files");
    // So each file goes on its own, and `a` is locked.
    std::fs::write(&mine, b"mine").unwrap();
    *refusing.lock().unwrap() = vec![a.clone()];
    let (_, why) = refusal(engine.command(added.id, TorrentCommand::Delete).await);
    assert!(matches!(&why, TorrentRefusal::NotTrashed { kept, .. } if kept.len() == 1 && kept[0].path.to_path_buf() == a), "{why:?}");
    let failed = |t: &TorrentInfo| matches!(&t.status, Status::Error { failure: proto::TorrentFailure::NotTrashed { kept } } if kept.len() == 1 && kept[0].path.ends_with("a.bin"));
    assert!(failed(&find(&engine, added.id).await), "it waits with what's left");
    assert!(a.exists() && !b.exists());
    engine.stop().await;

    let engine = start().await;
    let after = find(&engine, added.id).await;
    assert!(failed(&after), "it comes back with its error: {:?}", after.status);
    assert!(!b.exists(), "the restore didn't make the trashed file again");
    refusing.lock().unwrap().clear();
    engine.command(added.id, TorrentCommand::Delete).await.unwrap();
    assert!(engine.listing().await.is_empty());
    assert!(!a.exists() && mine.exists(), "the rest goes; the user's file stays");
    engine.stop().await;
}

/// A partial Delete stays so whatever its drive does, and nothing makes its trashed files again:
/// Resume and Check are refused; the drive goes, the service restarts while it's out, the drive
/// comes back.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_delete_outlasts_its_drive_going_and_a_restart_meanwhile() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let away = dir.path().join("away");
    let refusing = Arc::new(Mutex::new(Vec::new()));
    let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default());
    let engine = start().await;
    let added = add(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), false).await.unwrap();
    let (a, b) = (downloads.join("pack/a.bin"), downloads.join("pack/b.bin"));
    std::fs::write(downloads.join("pack/mine.txt"), b"mine").unwrap();
    *refusing.lock().unwrap() = vec![a.clone()];
    refusal(engine.command(added.id, TorrentCommand::Delete).await);
    for command in [TorrentCommand::Resume, TorrentCommand::Check] {
        assert_eq!(refusal(engine.command(added.id, command).await), (added.id, TorrentRefusal::PartlyDeleted));
    }
    let partly = |s: &Status| matches!(s, Status::Error { .. });
    let out = |s: &Status| matches!(s, Status::DestinationUnavailable { .. });
    std::fs::rename(&downloads, &away).unwrap();
    until(&engine, added.id, out).await;
    engine.stop().await;
    let engine = start().await;
    until(&engine, added.id, out).await;
    std::fs::rename(&away, &downloads).unwrap();
    until(&engine, added.id, partly).await;
    // A few size checks more: nothing turns it into a check.
    tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
    assert!(partly(&find(&engine, added.id).await.status));
    assert!(a.exists() && !b.exists(), "what Delete moved isn't made again");
    engine.stop().await;
}

/// A stop waits for a Delete under way and keeps how it went. Here the trash refuses the folder
/// whole, so nothing moved: after a restart the torrent is as it was, not an error.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_waits_for_the_trashing_and_keeps_how_it_went() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let refusing = Arc::new(Mutex::new(vec![downloads.join("pack")]));
    let gate = super::trash::Gate::default();
    struct Opens(super::trash::Gate);
    impl Drop for Opens {
        fn drop(&mut self) {
            super::trash::hold(&self.0, false);
        }
    }
    let _opens = Opens(gate.clone());
    let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), gate.clone());
    let engine = start().await;
    let added = add(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
    let before = find(&engine, added.id).await.status;
    super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(added.id, TorrentCommand::Delete));
    wait_for(&engine, added.id, Status::Deleting).await;
    let stopping = engine.stop();
    tokio::pin!(stopping);
    assert!(tokio::time::timeout(Duration::from_millis(500), &mut stopping).await.is_err(), "the stop waits for the trashing");
    super::trash::hold(&gate, false);
    stopping.await;
    let (_, why) = refusal(deleting.await.unwrap());
    assert!(matches!(why, TorrentRefusal::NotTrashed { .. }), "{why:?}");

    let engine = start().await;
    assert_eq!(find(&engine, added.id).await.status, before, "nothing moved: as it was");
    assert!(downloads.join("pack/a.bin").exists() && downloads.join("pack/b.bin").exists());
    engine.stop().await;
}

/// While a Delete's trashing is held, the torrent is Deleting and refuses commands, and the
/// engine goes on: another torrent's command is answered, and the snapshot follows.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_delete_holds_up_nothing_else() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let gate = super::trash::Gate::default();
    // Opened however the test ends: the runtime waits for the trash's thread before it ends.
    struct Opens(super::trash::Gate);
    impl Drop for Opens {
        fn drop(&mut self) {
            super::trash::hold(&self.0, false);
        }
    }
    let _opens = Opens(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let slow = add(&engine, &downloads, torrent_file("slow.bin"), true).await.unwrap();
    let other = add(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap();
    super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(slow.id, TorrentCommand::Delete));
    wait_for(&engine, slow.id, Status::Deleting).await;
    let answered = tokio::time::timeout(Duration::from_secs(5), engine.command(other.id, TorrentCommand::Resume)).await;
    assert!(answered.expect("answered while the Delete waits").is_ok());
    assert_ne!(find(&engine, other.id).await.status, Status::Paused, "and the snapshot shows it");
    assert_eq!(refusal(engine.command(slow.id, TorrentCommand::Pause).await), (slow.id, TorrentRefusal::Deleting));
    assert!(!deleting.is_finished(), "the Delete is answered once it's done");
    super::trash::hold(&gate, false);
    deleting.await.unwrap().unwrap();
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [other.id]);
    assert!(!downloads.join("slow.bin").exists());
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
    assert_eq!((t.content.unwrap().size_bytes, t.content.unwrap().full_size_bytes), (3 * 16384, 3 * 16384));

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
        assert_eq!(t.content.unwrap().size_bytes, 2 * 16384, "Size leaves out the file set Off");
        assert_eq!(t.content.unwrap().full_size_bytes, 3 * 16384, "Full size doesn't");
        let priorities: Vec<_> = engine.details(added.id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal]);
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    let priorities: Vec<_> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Normal, Priority::Off, Priority::Normal], "kept across the restart");
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 2 * 16384);

    wait_for(&engine, id, Status::Downloading).await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![0, 1], priority: Priority::Normal }).await.unwrap();
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 3 * 16384, "Normal again, several at once");
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
        assert_eq!(
            (t.content.unwrap().size_bytes, t.content.unwrap().full_size_bytes, t.content.unwrap().progress_bytes),
            (0, 2 * 16384, 0),
            "nothing left to get"
        );
        assert_eq!(t.status, Status::Seeding, "so it's finished");
        let priorities: Vec<_> = engine.details(added.id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Off, Priority::Off]);

        engine.command(added.id, TorrentCommand::Pause).await.unwrap();
        assert_eq!(find(&engine, added.id).await.status, Status::Done { reached: None });
        engine.stop().await;
        added.id
    };

    let engine = engine(dir.path()).await;
    wait_for(&engine, id, Status::Done { reached: None }).await;
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 0, "kept across the restart");

    engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Normal }).await.unwrap();
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 16384, "a file can be Normal again");
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
    engine.inspect(move |actor| actor.torrents[&id].inlib.info().unwrap().only_files()).await
}

async fn held(engine: &Engine, id: u64) -> Option<BTreeSet<usize>> {
    engine
        .inspect(move |actor| {
            let hash = actor.torrents[&id].inlib.info().unwrap().info_hash().as_string();
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
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 64 << 20, "and out of the Size");

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
    match after.expect("answered after stopping too").unwrap_err() {
        Failure::Error(e) => assert_eq!(e.to_string(), "the service is stopping"),
        refused => panic!("not refused, failed: {refused:?}"),
    }
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
    engine.inspect(move |actor| std::sync::Arc::as_ptr(actor.torrents[&id].inlib.info().unwrap()) as usize).await
}

async fn librqbit_live(engine: &Engine, id: u64) -> bool {
    engine.inspect(move |actor| actor.torrents[&id].inlib.info().unwrap().live().is_some()).await
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
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 2 * FULL, "nor counted in the torrent's progress");
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
    assert_eq!(find(&engine, id).await.status, Status::Done { reached: None });

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
    assert_eq!(
        (t.id, t.content.unwrap().size_bytes, t.content.unwrap().progress_bytes),
        (id, 3 * FULL, 2 * FULL),
        "the same torrent and row"
    );
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
    let again = |source: &AddSource| source.clone();
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
    wait_for(&engine, id, Status::Done { reached: None }).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(find(&engine, id).await.status, Status::Done { reached: None }, "a paused one stays paused");
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
    let url = || AddSource::TorrentUrl(format!("{server}/a.torrent"));

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
        add(&engine, &dir.path().join("downloads"), AddSource::TorrentUrl(format!("{server}/nothing.torrent")), false).await.unwrap_err();
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
        before = t.content.unwrap().progress_bytes;
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
        after = t.content.unwrap().progress_bytes;
        if t.status != Status::Checking {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(after >= before, "{after} after the restart, {before} before the stop");
    restarted.stop().await;
}

mod add_source_is_accepted {
    //! One parser decides for both sides: whatever the client's Add accepts, the service's add
    //! path accepts too, as the same torrent.
    use proptest::prelude::*;

    use super::*;
    use crate::engine::{Fetched, prepare};

    fn texts() -> impl Strategy<Value = String> {
        prop_oneof![
            "[0-9a-fA-F]{40}",
            "[A-Za-z2-7]{32}",
            "magnet:\\?xt=urn:btih:[0-9a-fA-F]{40}(&dn=[a-z%0-9+]{1,8})?(&tr=[a-z%0-9]{1,8})*",
            "magnet:\\?xt=urn:btih:[A-Za-z2-7]{32}(&dn=[a-z]{1,8})?(&tr=[a-z]{1,8})*",
            "magnet:\\?xt=urn:(ed2k|btmh):[a-z0-9]{4,8}&xt=urn:btih:[A-Za-z2-7]{32}",
            "magnet:\\?xt=urn:btih:[0-9a-f]{40}&xt=urn:btih:[A-Za-z2-7]{32}&so=[0-9]&x.pe=[a-z]{3}",
        ]
    }

    proptest! {
        #[test]
        fn what_the_client_accepts_the_service_adds(text in texts()) {
            let folder = tempfile::tempdir().unwrap();
            let destination = AbsolutePath::new(folder.path()).unwrap();
            let Ok(proto::AddSource::Magnet(magnet)) = client::add_source(&text) else {
                return Err(TestCaseError::fail(format!("{text} isn't a magnet source")));
            };
            let prepared = prepare(Fetched::Magnet(magnet.clone()), destination);
            prop_assert!(prepared.is_ok(), "the service refuses {magnet} (from {text}): {:?}", prepared.err());
            let (client::Link::Magnet { info_hash, .. } | client::Link::InfoHash(info_hash)) = client::parse_link(&text).unwrap() else {
                unreachable!("a magnet source comes from a magnet or a hash")
            };
            let expected = librqbit::Magnet::parse(&format!("magnet:?xt=urn:btih:{info_hash}")).unwrap().as_id20().unwrap().as_string();
            prop_assert_eq!(prepared.unwrap().info_hash, expected);
        }
    }
}

/// A disk image, mounted in `dir` out of Finder's sight, and detached when dropped.
struct SmallDisk {
    mount: PathBuf,
}

impl SmallDisk {
    /// 4 MB of HFS+.
    fn new(dir: &Path) -> Self {
        Self::of(dir, "4m", "HFS+")
    }

    /// `size` (as hdiutil takes it) of file system `fs`.
    fn of(dir: &Path, size: &str, fs: &str) -> Self {
        let name: String = fs.chars().filter(char::is_ascii_alphanumeric).collect();
        let image = dir.join(format!("{name}.dmg"));
        let mount = dir.join(&name);
        let run = |args: &[&str]| {
            let out = std::process::Command::new("hdiutil").args(args).output().unwrap();
            assert!(out.status.success(), "hdiutil {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["create", "-quiet", "-size", size, "-fs", fs, "-volname", "PhantomTest", image.to_str().unwrap()]);
        run(&["attach", "-quiet", "-nobrowse", "-mountpoint", mount.to_str().unwrap(), image.to_str().unwrap()]);
        Self { mount }
    }

    /// What the service reads: Finder's figure (`space::disk`), which for an image volume may be
    /// `statfs`'s own.
    fn free(&self) -> u64 {
        super::space::disk(&self.mount).expect("mounted").free_bytes
    }
}

impl Drop for SmallDisk {
    fn drop(&mut self) {
        let _ = std::process::Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
    }
}

/// A real disk too small for a torrent, which can't be generated: before anything downloads, the
/// snapshot warns of both the disk and the torrent (it alone needs more than is free); the add
/// isn't refused; and once the disk is full, the torrent is an Error saying so, the path a full
/// disk takes (#127).
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_bigger_than_its_disk_is_warned_of_then_fails_when_the_disk_is_full() {
    let dir = temp();
    let small = SmallDisk::new(dir.path());
    let (_seeder, torrent, _, _) = seeder(dir.path()).await;
    let engine = engine(dir.path()).await;
    let id = add(&engine, &small.mount, AddSource::TorrentFile(torrent), false).await.expect("an add is never refused for space").id;
    let mut volume = None;
    for _ in 0..200 {
        volume = engine.snapshot.borrow().snapshot.volumes.first().cloned();
        if volume.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let volume = volume.expect("the small disk's volume");
    assert_eq!(&*volume.mount, small.mount.canonicalize().unwrap());
    assert!(volume.free_bytes.is_some_and(|free| free < 4 << 20), "{volume:?}");
    assert!(volume.short() && volume.too_big(id).is_some(), "{volume:?}");
    let mut status = Status::Checking;
    for _ in 0..500 {
        status = find(&engine, id).await.status;
        if matches!(status, Status::Error { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let Status::Error { failure: proto::TorrentFailure::Librqbit { detail } } = &status else { panic!("{status:?}, not a librqbit Error") };
    assert!(detail.contains("No space left on device"), "{detail}");
    engine.stop().await;
}

/// A .torrent file of one file, `name`, of `len` bytes in 1 MiB pieces, that no one seeds.
fn torrent_file_of(name: &str, len: u64) -> AddSource {
    let pieces = len.div_ceil(1 << 20) as usize;
    let mut info = format!("d6:lengthi{len}e4:name{}:{name}12:piece lengthi1048576e6:pieces{}:", name.len(), 20 * pieces).into_bytes();
    info.extend(vec![7; 20 * pieces]);
    info.push(b'e');
    let mut file = b"d4:info".to_vec();
    file.extend(info);
    file.push(b'e');
    AddSource::TorrentFile(file)
}

/// What torrent `id` needs in the engine's last snapshot, if a disk lists it.
fn needs_of(engine: &Engine, id: u64) -> Option<u64> {
    engine.snapshot.borrow().snapshot.volumes.iter().flat_map(|v| &v.needs).find(|n| n.id == id).map(|n| n.bytes)
}

/// The free space the snapshot says of the disk torrent `id` needs space on, if one lists it.
fn free_for(engine: &Engine, id: u64) -> Option<Option<u64>> {
    engine.snapshot.borrow().snapshot.volumes.iter().find(|v| v.needs.iter().any(|n| n.id == id)).map(|v| v.free_bytes)
}

/// Waits up to `secs` for `done`, polling.
async fn eventually(secs: u64, mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..secs * 50 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    done()
}

/// Real HFS+ and exFAT disks, which can't be generated: librqbit sets a file's length when it
/// initialises a torrent, which takes the whole of it there (on APFS, nothing). Once it has, the
/// torrent needs nothing more and the disk's free space is down by the file: what's free after it
/// downloads (free less needs) is what it was less the file once, not twice (#127).
#[tokio::test(flavor = "multi_thread")]
async fn a_preallocated_file_is_counted_once_on_hfs_and_exfat() {
    const SIZE: u64 = 16 << 20;
    const SLACK: u64 = 1 << 20;
    for fs in ["HFS+", "ExFAT"] {
        let dir = temp();
        let disk = SmallDisk::of(dir.path(), "64m", fs);
        let before = disk.free();
        let engine = engine(dir.path()).await;
        let id = add_to(&engine, torrent_file_of("big.bin", SIZE), false, &disk.mount).await.unwrap().id;
        let file = disk.mount.join("big.bin");
        let allocated = eventually(20, || super::space::allocated(&file) >= SIZE).await;
        assert!(allocated, "{fs}: librqbit preallocates the file");
        let counted_once = eventually(20, || needs_of(&engine, id).unwrap_or(0) <= SLACK).await;
        let needs = needs_of(&engine, id).unwrap_or(0);
        assert!(counted_once, "{fs}: it needs {needs} once its file takes {SIZE}");
        let after = disk.free() - needs;
        assert!(after + SLACK >= before - SIZE, "{fs}: {after} free once it's done, from {before}, for {SIZE}");
        engine.stop().await;
    }
}

/// A destination its disk's statfs reaches but Phantom can't use (here a folder it may not open;
/// a folder left at `/Volumes/<name>` of a drive that's gone is the same, but making one needs
/// root): its torrent is Destination unavailable, so it counts on no disk (#127).
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_whose_destination_is_unavailable_counts_on_no_disk_though_statfs_reaches_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp();
    let folder = dir.path().join("films");
    std::fs::create_dir_all(&folder).unwrap();
    let engine = engine(dir.path()).await;
    let id = add_to(&engine, torrent_file("a.bin"), false, &folder).await.unwrap().id;
    assert!(eventually(10, || needs_of(&engine, id).is_some()).await, "counted on its disk while it can be reached");
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
    let unavailable = || {
        let snapshot = engine.snapshot.borrow();
        snapshot.snapshot.torrents.iter().any(|t| t.id == id && matches!(t.status, Status::DestinationUnavailable { .. }))
    };
    assert!(eventually(10, unavailable).await, "Destination unavailable");
    assert!(super::space::disk(&folder).is_some(), "statfs still reaches it");
    let left_out = eventually(5, || needs_of(&engine, id).is_none()).await;
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(left_out, "counted while its destination is unavailable: {:?}", engine.snapshot.borrow().snapshot.volumes);
    engine.stop().await;
}

/// Looks at disks as the engine does, but stalls, without answering, every look at
/// `stalls` while `stalled` is on, counting them.
#[derive(Clone, Default)]
struct Stalling {
    stalled: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    looks: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Stalling {
    fn look(&self, stalls: PathBuf) -> super::space::Look {
        let (this, real) = (self.clone(), super::space::look());
        std::sync::Arc::new(move |destination, files| {
            if destination.canonicalize().is_ok_and(|d| d == stalls) {
                let (on, changed) = &*this.stalled;
                let mut on = on.lock().unwrap();
                if *on {
                    this.looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                while *on {
                    on = changed.wait(on).unwrap();
                }
            }
            real(destination, files)
        })
    }

    fn set(&self, on: bool) {
        *self.stalled.0.lock().unwrap() = on;
        self.stalled.1.notify_all();
    }
}

/// A disk that stops answering (a network volume, say), which can't be generated: its free space
/// shows as unknown, nothing on it is short, and the engine goes on answering, with one look at
/// it at a time. Once it answers, its free space is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_disk_shows_its_free_space_unknown_and_never_holds_up_the_engine() {
    let dir = temp();
    let folder = dir.path().join("films");
    std::fs::create_dir_all(&folder).unwrap();
    let stalling = Stalling::default();
    let config = Config {
        data_dir: dir.path().join("data"),
        download_dir: dir.path().join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        trash: super::trash::into(dir.path().join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
    };
    let add_into: super::actor::AddInto = std::sync::Arc::new(|session, id, add, destination, download_dir| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir))
    });
    let look = stalling.look(folder.canonicalize().unwrap());
    let engine = Engine::start_with(config, add_into, super::files::Guard::default(), look).await.unwrap();
    let id = add_to(&engine, torrent_file("a.bin"), false, &folder).await.unwrap().id;
    assert!(eventually(10, || matches!(free_for(&engine, id), Some(Some(_)))).await, "its free space, read");

    stalling.set(true);
    let started = std::time::Instant::now();
    let mut answered = 0;
    while free_for(&engine, id) != Some(None) {
        assert!(started.elapsed() < super::space::STALLED_AFTER + Duration::from_secs(5), "never shown as unknown");
        let answer = tokio::time::timeout(Duration::from_secs(1), engine.command(id, TorrentCommand::Resume)).await;
        assert!(answer.is_ok(), "the engine answers while the disk is stalled");
        answered += 1;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let volume = engine.snapshot.borrow().snapshot.volumes[0].clone();
    assert!(!volume.short() && volume.too_big(id).is_none(), "nothing short of what isn't known: {volume:?}");
    let paused = tokio::time::timeout(Duration::from_secs(1), engine.command(id, TorrentCommand::Pause)).await;
    assert!(matches!(paused, Ok(Ok(()))), "{paused:?}");
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    tokio::time::sleep(super::space::STALLED_AFTER).await;
    assert_eq!(stalling.looks.load(std::sync::atomic::Ordering::SeqCst), 1, "one look at a time, after {answered} answers");

    stalling.set(false);
    assert!(eventually(10, || matches!(free_for(&engine, id), Some(Some(_)))).await, "its free space, back");
    engine.stop().await;
}

/// Free space follows the disk (#127): generated writes and deletes on a real disk (APFS, so the
/// torrent's file is sparse and it needs space there all along), and after each the snapshot's
/// free space for that disk comes to what statfs says.
#[test]
fn the_free_space_shown_follows_writes_and_deletes_on_the_disk() {
    use proptest::prelude::*;
    let config = proptest::test_runner::Config { cases: 6, max_shrink_iters: 16, ..Default::default() };
    let writes = proptest::collection::vec(prop_oneof![(1..6u64).prop_map(Some), Just(None)], 1..6);
    proptest::test_runner::TestRunner::new(config)
        .run(&writes, |writes| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let dir = temp();
                let disk = SmallDisk::of(dir.path(), "40m", "APFS");
                let engine = engine(dir.path()).await;
                let id = add_to(&engine, torrent_file_of("big.bin", 8 << 20), false, &disk.mount).await.unwrap().id;
                let mut written = Vec::new();
                for (n, write) in writes.iter().enumerate() {
                    match write {
                        Some(mib) => {
                            let file = disk.mount.join(format!("filler{n}"));
                            let bytes: Vec<u8> = (0..mib << 20).map(|i| (i * 2_654_435_761 >> 13) as u8).collect();
                            std::fs::write(&file, bytes).unwrap();
                            std::fs::File::open(&file).unwrap().sync_all().unwrap();
                            written.push(file);
                        }
                        None => {
                            if let Some(file) = written.pop() {
                                std::fs::remove_file(file).unwrap();
                            }
                        }
                    }
                    let close = |shown: Option<Option<u64>>| shown.flatten().is_some_and(|free| free.abs_diff(disk.free()) < 512 << 10);
                    let followed = eventually(5, || close(free_for(&engine, id))).await;
                    prop_assert!(followed, "after {:?}: shown {:?}, statfs {}", write, free_for(&engine, id), disk.free());
                }
                engine.stop().await;
                Ok(())
            })
        })
        .unwrap();
}

mod limits {
    use proptest::prelude::*;
    use proto::{SpeedLimit, SpeedLimits};

    use super::*;

    #[derive(Debug, Clone)]
    enum Step {
        Set(SpeedLimits),
        /// Stopped, and started again with the limits as stored, as the service does.
        Restart,
    }

    fn limits() -> impl Strategy<Value = SpeedLimits> {
        let limit = || proptest::option::of((SpeedLimit::MIN..=SpeedLimit::MAX).prop_map(|b| SpeedLimit::new(b).unwrap()));
        (limit(), limit()).prop_map(|(download, upload)| SpeedLimits { download, upload })
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![3 => limits().prop_map(Step::Set), 1 => Just(Step::Restart)]
    }

    async fn start(dir: &Path, speed_limits: SpeedLimits) -> Engine {
        Engine::start(Config {
            data_dir: dir.join("data"),
            download_dir: dir.join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: SIZE_CHECK_EVERY,
            trash: super::super::trash::into(dir.join("trash"), Default::default(), Default::default()),
            speed_limits,
            seeding: Default::default(),
        })
        .await
        .unwrap()
    }

    /// librqbit's own terms for `limits`.
    fn session(limits: SpeedLimits) -> librqbit::limits::LimitsConfig {
        let bps = |l: Option<SpeedLimit>| l.map(SpeedLimit::bytes_per_sec);
        librqbit::limits::LimitsConfig { download_bps: bps(limits.download), upload_bps: bps(limits.upload) }
    }

    fn cases() -> u32 {
        std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(16)
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

        /// The running session's limits are the ones last set, from the start and across restarts.
        #[test]
        fn the_session_runs_with_the_limits_last_set(start_with in limits(), steps in proptest::collection::vec(step(), 1..10)) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let dir = temp();
            let mut stored = start_with;
            let mut engine = rt.block_on(start(dir.path(), stored));
            prop_assert_eq!(engine.limits(), session(stored));
            for step in steps {
                match step {
                    Step::Set(limits) => {
                        engine.set_limits(limits);
                        stored = limits;
                    }
                    Step::Restart => {
                        rt.block_on(engine.stop());
                        engine = rt.block_on(start(dir.path(), stored));
                    }
                }
                prop_assert_eq!(engine.limits(), session(stored), "after {:?}", step);
            }
            rt.block_on(engine.stop());
        }
    }
}

/// A limit set while a torrent downloads slows it at once, and unset lets it go fast again: librqbit
/// swaps its limiter under running torrents, the reason a limit restarts nothing. An example, as it
/// times a real download.
#[tokio::test(flavor = "multi_thread")]
async fn a_limit_set_while_downloading_applies_at_once() {
    let dir = temp();
    let (_seeder, torrent, _, _) = seeder(dir.path()).await;
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    let progress = || async { find(&engine, id).await.content.map_or(0, |c| c.progress_bytes) };
    for _ in 0..600 {
        if find(&engine, id).await.status == Status::Downloading && progress().await > 1 << 20 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let limit = proto::SpeedLimit::new(256 << 10).unwrap();
    engine.set_limits(proto::SpeedLimits { download: Some(limit), upload: None });
    // What was asked for before arrives first.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let before = progress().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let limited = progress().await - before;
    // Two seconds at the limit, a second's burst, and a piece (256 KiB) counted whole.
    assert!(limited <= 4 * (256 << 10), "{limited} bytes in 2 s at 256 KiB/s");
    engine.set_limits(proto::SpeedLimits::default());
    let before = progress().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let unlimited = progress().await - before;
    assert!(unlimited > 4 << 20, "{unlimited} bytes in 2 s unlimited");
    engine.stop().await;
}

mod seeding_limits {
    use std::time::Instant;

    use proto::{Ratio, Reached, SeedTime, TorrentCommand};

    use super::super::machine::seeding::Limits;
    use super::*;

    async fn start(dir: &Path, seeding: Limits) -> Engine {
        Engine::start(Config {
            data_dir: dir.join("data"),
            download_dir: dir.join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: SIZE_CHECK_EVERY,
            speed_limits: Default::default(),
            seeding,
        })
        .await
        .unwrap()
    }

    /// Waits until torrent `id` is `status`; how long that took.
    async fn until(engine: &Engine, id: u64, status: &Status) -> Duration {
        let started = Instant::now();
        while find(engine, id).await.status != *status {
            assert!(started.elapsed() < Duration::from_secs(10), "torrent {id} is {:?}, not {status:?}", find(engine, id).await.status);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        started.elapsed()
    }

    /// The time limit on a real engine, which times can't be generated for: a seeding torrent
    /// stops at it, Done saying so, not before; Resume starts a new round, so it seeds on rather
    /// than stopping straight away, and stops at the limit again; the round survives a restart,
    /// so it stops sooner than a fresh round would.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_time_limit_stops_seeding_resume_seeds_a_new_round_and_a_restart_keeps_it() {
        let dir = temp();
        let source = complete_torrent(dir.path()).await;
        let at = SeedTime::new(1).unwrap();
        // Two seconds, said as the minute it stands for.
        let limits = Limits { ratio: None, time: Some((Duration::from_secs(2), at)) };
        let done = Status::Done { reached: Some(Reached::Time(at)) };
        let engine = start(dir.path(), limits).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        until(&engine, id, &Status::Seeding).await;
        let took = until(&engine, id, &done).await;
        assert!(took >= Duration::from_millis(1900), "stopped after {took:?}, before the limit");

        engine.command(id, TorrentCommand::Resume).await.unwrap();
        until(&engine, id, &Status::Seeding).await;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "a Resume isn't undone by the same limit straight away");

        // About a second and a half into the new round: kept across the restart.
        engine.stop().await;
        let engine = start(dir.path(), limits).await;
        until(&engine, id, &Status::Seeding).await;
        let took = until(&engine, id, &done).await;
        assert!(took < Duration::from_millis(1800), "a fresh round takes two seconds; this one took {took:?}");
        engine.stop().await;
    }

    /// A leecher on this machine, listening, that a local tracker names to whoever announces:
    /// the engine (which doesn't listen) connects to it and uploads. Its session, and the .torrent
    /// of one file, complete in the engine's download dir.
    async fn leecher(dir: &Path) -> (std::sync::Arc<librqbit::Session>, Vec<u8>) {
        use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, SessionOptions, create_torrent};
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let tracker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut body = b"d8:intervali60e5:peers6:\x7f\x00\x00\x01".to_vec();
            body.extend(port.to_be_bytes());
            body.push(b'e');
            for stream in tracker.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.read(&mut [0; 2048]);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body));
            }
        });
        let file = dir.join("downloads/shared.bin");
        std::fs::create_dir_all(dir.join("downloads")).unwrap();
        // 1 MiB, in pieces of 64 KiB.
        let bytes: Vec<u8> = (0..1 << 20).map(|n: usize| (n * 13 % 251) as u8).collect();
        std::fs::write(&file, bytes).unwrap();
        let options = CreateTorrentOptions { name: None, trackers: vec![announce], piece_length: Some(64 << 10) };
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let torrent = create_torrent(&file, options, &spawner).await.unwrap().as_bytes().unwrap().to_vec();
        let session = librqbit::Session::new_with_opts(
            dir.join("leecher"),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], port).into(), ..Default::default() }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let opts = AddTorrentOptions { output_folder: Some(dir.join("leeched").to_str().unwrap().into()), ..Default::default() };
        session.add_torrent(AddTorrent::from_bytes(torrent.clone()), Some(opts)).await.unwrap();
        (session, torrent)
    }

    /// The ratio limit on a real engine uploading to a real leecher, which can't be generated:
    /// librqbit's upload counts, and the torrent stops at the ratio, Done saying so.
    #[tokio::test(flavor = "multi_thread")]
    async fn uploads_reach_the_ratio_limit_and_stop_seeding() {
        let dir = temp();
        let (_leecher, torrent) = leecher(dir.path()).await;
        let ratio = Ratio::new(50).unwrap();
        let engine = start(dir.path(), Limits { ratio: Some(ratio), time: None }).await;
        let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
        until(&engine, id, &Status::Seeding).await;
        until(&engine, id, &Status::Done { reached: Some(Reached::Ratio(ratio)) }).await;
        let round = engine.inspect(move |actor| actor.torrents[&id].meter.round).await;
        assert!(round.uploaded >= 512 << 10, "stopped at ratio 0.5 having uploaded {} of 1 MiB", round.uploaded);
        engine.stop().await;
    }
}
