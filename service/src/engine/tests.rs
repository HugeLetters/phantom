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
pub(super) fn multi_file_torrent(paths: &[&str]) -> AddSource {
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
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.join("trash"), refusing, gate),
        speed_limits: Default::default(),
        seeding: Default::default(),
    })
    .await
    .unwrap()
}

/// Adds into the engine's download dir, made first: the engine adds only into a destination there
/// is. A .torrent file's add is answered while librqbit takes it, a moment: this waits for that.
async fn add(engine: &Engine, download_dir: &Path, source: AddSource, paused: bool) -> Result<Added, Failure> {
    std::fs::create_dir_all(download_dir).unwrap();
    add_to(engine, source, paused, download_dir).await
}

/// [`add`], its files on disk as once it has started (#374): one added stopped has none until then.
async fn add_made(engine: &Engine, download_dir: &Path, source: AddSource, paused: bool) -> Result<Added, Failure> {
    let added = add(engine, download_dir, source, paused).await?;
    granted(engine, added.id).await;
    Ok(added)
}

/// Torrent `id`'s files made and let write, as its start does (#374).
async fn granted(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if engine.inspect(move |actor| actor.grant_now(id)).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("torrent {id}'s files were never granted");
}

async fn add_to(engine: &Engine, source: AddSource, paused: bool, destination: &Path) -> Result<Added, Failure> {
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

/// Waits up to 10 seconds for librqbit to be done reading the pieces it saved of torrent `id`, as
/// it resumes (at a start, after an undo): its row is its record's until then.
async fn sampled(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if engine.inspect(move |a| !a.torrents.get(&id).is_some_and(|t| matches!(t.lib, super::machine::lib::Lib::Resuming(_)))).await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("torrent {id} is still resuming");
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

/// Waits for the engine's next `n` size checks, each whole after this call: what one sees, and what
/// it starts, has been seen and started by then, however loaded the machine. Wait on this, or on a
/// status ([`until`], [`wait_for`]), never for a time.
async fn size_checks(engine: &Engine, n: u64) {
    let done = engine.inspect(|a| a.size_checks).await;
    for _ in 0..2000 {
        if engine.inspect(move |a| a.size_checks >= done + n).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine did no {n} size checks in 10 s");
}

/// Waits for the engine's next `n` polls of librqbit, each whole after this call: what librqbit
/// did by then has been seen, and what the polls start has started.
async fn polls(engine: &Engine, n: u64) {
    let done = engine.inspect(|a| a.polls).await;
    for _ in 0..2000 {
        if engine.inspect(move |a| a.polls >= done + n).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine did no {n} polls of librqbit in 10 s");
}

/// Waits for the next `n` reports of the engine's looks at disks and the default folder, each
/// handled after this call.
async fn looks(engine: &Engine, n: u64) {
    looks_since(engine, engine.inspect(|a| a.looks_reported).await, n).await;
}

/// Waits until the engine has handled `n` reports of its looks since it had handled `done`
/// (`actor.looks_reported`).
async fn looks_since(engine: &Engine, done: u64, n: u64) {
    for _ in 0..2000 {
        if engine.inspect(move |a| a.looks_reported >= done + n).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine had no {n} reports of its looks in 10 s");
}

/// Waits until what the engine does by itself has come round twice since this call: two polls of
/// librqbit and two size checks, each whole. What a test that something *doesn't* happen waits
/// for before it looks, instead of a time: whatever would have happened has had its turn.
async fn settled(engine: &Engine) {
    tokio::join!(polls(engine, 2), size_checks(engine, 2));
}

/// Waits until the engine's file watcher reports a change under `folder`: FSEvents starts
/// watching a moment after it's asked to. A file is made and removed there until a report comes.
#[cfg(target_os = "macos")]
async fn watching(engine: &Engine, folder: &Path) {
    let probe = folder.join(".watched");
    for _ in 0..100 {
        let seen = engine.inspect(|a| a.changes_seen).await;
        std::fs::write(&probe, b"").unwrap();
        std::fs::remove_file(&probe).unwrap();
        // The watcher gathers events for `files::DEBOUNCE` before it reports them.
        for _ in 0..100 {
            if engine.inspect(move |a| a.changes_seen > seen).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    panic!("the watcher never reported a change under {}", folder.display());
}

/// Waits until the engine's file watcher reports a change under `folder`. Elsewhere than macOS
/// (inotify) a file made and removed at once is no change the watcher reports (its folder's own
/// isn't one): the file is made, and removed once the watcher has gathered that.
#[cfg(not(target_os = "macos"))]
async fn watching(engine: &Engine, folder: &Path) {
    let probe = folder.join(".watched");
    for _ in 0..20 {
        let seen = engine.inspect(|a| a.changes_seen).await;
        std::fs::write(&probe, b"").unwrap();
        // Past the watcher's `files::DEBOUNCE` (250 ms): the file made is gathered alone.
        tokio::time::sleep(Duration::from_millis(500)).await;
        std::fs::remove_file(&probe).unwrap();
        for _ in 0..100 {
            if engine.inspect(move |a| a.changes_seen > seen).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    panic!("the watcher never reported a change under {}", folder.display());
}

/// Runs `run` of each of `cases` side by side, `width` at a time: each in its own temp dir with its
/// own engine, most of its time waiting (every engine's stop alone takes librqbit's 1 s). The
/// first case to panic fails the test with its panic.
async fn side_by_side<C, F>(width: usize, cases: impl IntoIterator<Item = C>, run: impl Fn(C) -> F)
where
    C: Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let width = Arc::new(tokio::sync::Semaphore::new(width));
    let mut running = tokio::task::JoinSet::new();
    for case in cases {
        let (width, case) = (width.clone(), run(case));
        running.spawn(async move {
            let _turn = width.acquire().await.unwrap();
            case.await
        });
    }
    while let Some(done) = running.join_next().await {
        if let Err(failed) = done {
            running.shutdown().await;
            std::panic::resume_unwind(failed.into_panic());
        }
    }
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
    let live = engine.inspect(move |actor| actor.torrents[&id].lib.torrent().map(|h| h.is_paused())).await;
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
    let added = add_made(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap();
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
    let added = add_made(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
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
    // Its folder is made with its files as it starts, after the add (#374, #439).
    assert!(eventually(10, || a.exists() && b.exists()).await, "its files are made as it starts");
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
    size_checks(&engine, 3).await;
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
    let added = add_made(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
    // Added paused, librqbit checks it first (nothing on disk): how it was is what's left once
    // that's over, not the Checking a loaded machine may still be showing.
    until(&engine, added.id, |s| *s != Status::Checking).await;
    let before = find(&engine, added.id).await.status;
    super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(added.id, TorrentCommand::Delete));
    held_at_the_gate(&gate).await;
    let stopping = engine.stop();
    tokio::pin!(stopping);
    assert!(tokio::time::timeout(Duration::from_millis(500), &mut stopping).await.is_err(), "the stop waits for the trashing");
    super::trash::hold(&gate, false);
    stopping.await;
    let (_, why) = refusal(deleting.await.unwrap());
    assert!(matches!(why, TorrentRefusal::NotTrashed { .. }), "{why:?}");

    let engine = start().await;
    // librqbit may check it as it starts (nothing saved of it), which shows as Checking (#244).
    until(&engine, added.id, |s| *s != Status::Checking).await;
    assert_eq!(find(&engine, added.id).await.status, before, "nothing moved: as it was");
    assert!(downloads.join("pack/a.bin").exists() && downloads.join("pack/b.bin").exists());
    engine.stop().await;
}

/// While a Delete's trashing is held, the torrent is gone from the list already (#190), and a
/// command for it is refused as for one not in the list; the engine goes on: another torrent's
/// command is answered, and the snapshot follows.
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
    let slow = add_made(&engine, &downloads, torrent_file("slow.bin"), true).await.unwrap();
    let other = add_made(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap();
    super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(slow.id, TorrentCommand::Delete));
    held_at_the_gate(&gate).await;
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [other.id], "gone from the list at once");
    let answered = tokio::time::timeout(Duration::from_secs(5), engine.command(other.id, TorrentCommand::Resume)).await;
    assert!(answered.expect("answered while the Delete waits").is_ok());
    assert_ne!(find(&engine, other.id).await.status, Status::Paused, "and the snapshot shows it");
    assert_eq!(refusal(engine.command(slow.id, TorrentCommand::Pause).await), (slow.id, TorrentRefusal::NotInList));
    assert!(!deleting.is_finished(), "the Delete is answered once it's done");
    super::trash::hold(&gate, false);
    deleting.await.unwrap().unwrap();
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [other.id]);
    assert!(!downloads.join("slow.bin").exists());
    engine.stop().await;
}

/// Move to Trash (#151), for every choice of files, with the trash held at its gate after the first
/// file it moved: the files are Off already while the move is half done, so the torrent never says
/// Missing files (a size check every few milliseconds looks meanwhile); the files not chosen are
/// untouched and still Normal; the torrent's other commands wait for the move rather than meet half
/// of it; and once it ends the answer comes, the chosen files are in the Trash, and the Undo puts
/// them back and each file's priority. For some choices the Undo comes while the move is held: it
/// aborts it (#190), the files show their priorities at once, the move ends after the file in
/// flight and is answered as undone, and what it moved comes back.
#[tokio::test(flavor = "multi_thread")]
async fn trashing_files_sets_them_off_before_any_moves_and_holds_the_torrents_other_commands_meanwhile() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let gate = super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), Duration::from_millis(20), Default::default(), gate.clone()).await;
    let id = add_made(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin", "c.bin"]), true).await.unwrap().id;
    let priorities = || async { engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect::<Vec<Priority>>() };
    for chosen in 1u8..8 {
        let files: Vec<usize> = (0..3).filter(|f| chosen & (1 << f) != 0).collect();
        // The first file moves, then the trash waits.
        super::trash::hold(&gate, true);
        let trashing = tokio::spawn(engine.command_later(id, TorrentCommand::TrashFiles { files: files.clone() }));
        held_at_the_gate(&gate).await;
        let parked_pause = tokio::spawn(engine.command_later(id, TorrentCommand::Pause));
        // Several size checks pass while it's half done.
        for _ in 0..8 {
            size_checks(&engine, 1).await;
            assert_ne!(find(&engine, id).await.status, Status::MissingFiles, "{files:?}: Off before anything goes");
            let want: Vec<Priority> = (0..3).map(|f| if files.contains(&f) { Priority::Off } else { Priority::Normal }).collect();
            assert_eq!(priorities().await, want, "{files:?}: the chosen files are Off at once, the others untouched");
        }
        assert!(!trashing.is_finished() && !parked_pause.is_finished(), "{files:?}: nothing is answered while it's held");
        let this = undoable(proto::UndoAction::TrashFiles { count: files.len() }, id, "pack");
        let aborted = chosen % 2 == 1;
        let undo = aborted.then(|| tokio::spawn(engine.undo_later()));
        if aborted {
            // Still held at the gate: "at once" is before the file in flight ends.
            for _ in 0..500 {
                if priorities().await == [Priority::Normal; 3] {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(super::trash::waiting(&gate), "{files:?}: the file in flight is still held");
            assert_eq!(priorities().await, [Priority::Normal; 3], "{files:?}: their priorities show back at once");
            assert!(!undo.as_ref().unwrap().is_finished(), "{files:?}: the file in flight ends first");
        }
        super::trash::hold(&gate, false);
        let moved = trashing.await.unwrap();
        parked_pause.await.unwrap().unwrap();
        let undone = match undo {
            Some(undo) => {
                assert!(matches!(moved, Err(Failure::Refused(proto::Refusal::Undone))), "{files:?}: {moved:?}");
                undo.await.unwrap().unwrap()
            }
            None => {
                moved.unwrap();
                for (f, name) in ["a.bin", "b.bin", "c.bin"].into_iter().enumerate() {
                    assert_eq!(
                        downloads.join("pack").join(name).exists(),
                        !files.contains(&f),
                        "{files:?}: {name} is in the Trash, or untouched"
                    );
                }
                assert_eq!(top(&engine).await, Some(this.clone()));
                engine.undo().await.unwrap()
            }
        };
        assert_eq!(unnumbered(undone), this);
        for name in ["a.bin", "b.bin", "c.bin"] {
            assert!(downloads.join("pack").join(name).exists(), "{files:?}: {name} is back");
        }
        assert_eq!(priorities().await, [Priority::Normal; 3], "{files:?}: each file's priority is back");
        assert_eq!(top(&engine).await, None);
    }
    engine.stop().await;
}

/// Waits until what the size checks make of torrent `id`'s files is settled: the size checks that
/// see what's on disk now, a check that starts for a file that can be the one that went, and what
/// librqbit says once it ends.
async fn judged(engine: &Engine, id: u64) {
    size_checks(engine, 2).await;
    until(engine, id, |s| *s != Status::Checking).await;
    settled(engine).await;
}

/// Waits until the engine has begun the stop just asked of it (`stop`, `stop_to_restart`).
async fn stop_begun(engine: &Engine) {
    for _ in 0..1000 {
        if engine.stopping() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the engine never began to stop");
}

/// Whether `answer` is still unanswered, polled once now. After `engine.inspect(|_| ()).await`, the
/// actor has handled everything sent before it, so an answer it gave as it handled one is in.
async fn unanswered<F: Future + Unpin>(answer: &mut F) -> bool {
    tokio::time::timeout(Duration::ZERO, answer).await.is_err()
}

/// Opens a test trash's gate when the test ends, however it ends.
struct OpensTrash(super::trash::Gate);

impl Drop for OpensTrash {
    fn drop(&mut self) {
        super::trash::hold(&self.0, false);
    }
}

/// Waits until the trash's thread waits at the held gate, after an item.
async fn held_at_the_gate(gate: &super::trash::Gate) {
    // Long, for a loaded machine running every test at once.
    for _ in 0..1000 {
        if super::trash::waiting(gate) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the trash never waited at its gate");
}

/// The undo stack's actions, oldest first.
async fn stack(engine: &Engine) -> Vec<proto::UndoAction> {
    engine.subscribe().borrow().snapshot.undo.iter().map(|u| u.action).collect()
}

/// The pack of three files, with the trash gated, ready for a Move to Trash.
async fn gated_pack(dir: &Path) -> (Engine, super::trash::Gate, u64) {
    let gate = super::trash::Gate::default();
    let engine = engine_with(dir, Duration::from_millis(20), Default::default(), gate.clone()).await;
    let id = add_made(&engine, &dir.join("downloads"), multi_file_torrent(&["a.bin", "b.bin", "c.bin"]), true).await.unwrap().id;
    (engine, gate, id)
}

/// A Move to Trash under way holds up no other torrent (#190): a Remove of another is done at once,
/// both are on the stack in the order they came, the move's step running, and an Undo takes back
/// the last of them, the Remove, without waiting for the move.
#[tokio::test(flavor = "multi_thread")]
async fn another_torrents_remove_during_a_move_to_trash_is_done_at_once_and_stacked_in_order() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let other = add_made(&engine, &dir.path().join("downloads"), torrent_file("other.bin"), true).await.unwrap().id;
    super::trash::hold(&gate, true);
    let trashing = tokio::spawn(engine.command_later(id, TorrentCommand::TrashFiles { files: vec![0] }));
    held_at_the_gate(&gate).await;
    use proto::UndoAction::{Remove, TrashFiles};
    assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }], "on the stack from the moment it came");
    tokio::time::timeout(Duration::from_secs(5), engine.command(other, TorrentCommand::Remove)).await.expect("not held").unwrap();
    assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }, Remove]);
    let undone = tokio::time::timeout(Duration::from_secs(5), engine.undo()).await.expect("not held").unwrap();
    assert_eq!(unnumbered(undone), undoable(Remove, other, "other.bin"));
    assert!(!trashing.is_finished(), "the move is still held");
    super::trash::hold(&gate, false);
    trashing.await.unwrap().unwrap();
    assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }]);
    engine.stop().await;
}

/// More of them behind a move of the same torrent run in the order they came, after it: another
/// Move to Trash of it, then a Delete. Each is on the stack at once, in that order, and the torrent
/// leaves the list as the Delete comes.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_move_to_trash_and_a_delete_wait_behind_a_move_in_order() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    super::trash::hold(&gate, true);
    let first = tokio::spawn(engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await);
    held_at_the_gate(&gate).await;
    // Sent in this order, each handled before the test looks (#222).
    let second = tokio::spawn(engine.sent(id, TorrentCommand::TrashFiles { files: vec![1] }).await);
    engine.inspect(|_| ()).await;
    let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Off, Priority::Normal], "the second's file shows Off at once");
    let delete = tokio::spawn(engine.sent(id, TorrentCommand::Delete).await);
    engine.inspect(|_| ()).await;
    assert!(!second.is_finished() && !delete.is_finished());
    use proto::UndoAction::{Delete, TrashFiles};
    assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }, TrashFiles { count: 1 }, Delete], "in the order they came");
    assert!(engine.listing().await.is_empty(), "gone from the list as the Delete came");
    super::trash::hold(&gate, false);
    for done in [first, second, delete] {
        done.await.unwrap().unwrap();
    }
    assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }, TrashFiles { count: 1 }, Delete]);
    engine.stop().await;
}

/// A stop doesn't start what waits on a lane: a Delete behind a move is refused (the service is
/// stopping) and leaves the stack. An Undo never waits: one asked during the move aborts it, and the
/// stop lets that finish, so what the move took is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_ends_a_move_to_trash_but_runs_nothing_that_waited_for_it() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let engine = Arc::new(engine);
    super::trash::hold(&gate, true);
    let trashing = tokio::spawn(engine.command_later(id, TorrentCommand::TrashFiles { files: vec![0, 1] }));
    held_at_the_gate(&gate).await;
    // Sent in this order, each handled before the next (#222).
    let undo = tokio::spawn(engine.undo_sent().await);
    engine.inspect(|_| ()).await;
    let delete = tokio::spawn(engine.sent(id, TorrentCommand::Delete).await);
    engine.inspect(|_| ()).await;
    let stopping = tokio::spawn({
        let engine = engine.clone();
        async move { engine.stop().await }
    });
    // The stop has begun once it answers what waited: then the move goes on.
    for _ in 0..1000 {
        if delete.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(delete.is_finished(), "the stop never answered what waited");
    super::trash::hold(&gate, false);
    stopping.await.unwrap();
    assert!(matches!(trashing.await.unwrap(), Err(Failure::Refused(proto::Refusal::Undone))));
    assert!(matches!(delete.await.unwrap(), Err(Failure::Refused(proto::Refusal::Stopping))), "refused, not run");
    assert_eq!(unnumbered(undo.await.unwrap().unwrap()), undoable(proto::UndoAction::TrashFiles { count: 2 }, id, "pack"));
    drop(engine);
    let downloads = dir.path().join("downloads/pack");
    assert!(["a.bin", "b.bin", "c.bin"].iter().all(|f| downloads.join(f).exists()), "the move was undone, the Delete didn't run");
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
    assert_eq!(stack(&engine).await, [], "nothing is left on the stack");
    engine.stop().await;
}

/// The torrent stays in the list while an Undo of its Move to Trash puts files back, so what's done
/// to it waits for the put-back, then runs on the torrent as it is once it ends: ⌘⌫ on a file about to
/// come back moves it again (and isn't lost), a priority set meanwhile isn't overwritten, and a Delete
/// takes the whole torrent, put-back files included.
#[tokio::test(flavor = "multi_thread")]
async fn commands_for_a_torrent_wait_while_its_move_to_trash_is_being_undone() {
    for during in ["trash the file again", "set another file Off", "delete the torrent"] {
        let dir = temp();
        let (engine, gate, id) = gated_pack(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
        let pack = dir.path().join("downloads/pack");
        assert!(!pack.join("a.bin").exists() && !pack.join("b.bin").exists());
        // The put-back waits after its first file.
        super::trash::hold(&gate, true);
        let undoing = tokio::spawn(engine.undo_later());
        held_at_the_gate(&gate).await;
        let command = match during {
            "trash the file again" => TorrentCommand::TrashFiles { files: vec![0, 1] },
            "set another file Off" => TorrentCommand::SetPriority { files: vec![2], priority: Priority::Off },
            _ => TorrentCommand::Delete,
        };
        let mut sent = Box::pin(engine.sent(id, command).await);
        engine.inspect(|_| ()).await;
        assert!(unanswered(&mut sent).await, "{during}: it waits for the put-back");
        super::trash::hold(&gate, false);
        undoing.await.unwrap().unwrap();
        sent.await.unwrap();
        use proto::UndoAction::{Delete, TrashFiles};
        match during {
            "trash the file again" => {
                assert!(!pack.join("a.bin").exists() && !pack.join("b.bin").exists() && pack.join("c.bin").exists(), "{during}");
                assert_eq!(stack(&engine).await, [TrashFiles { count: 2 }], "{during}: moved again, and on the stack");
                let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
                assert_eq!(priorities, [Priority::Off, Priority::Off, Priority::Normal], "{during}");
            }
            "set another file Off" => {
                let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
                assert_eq!(priorities, [Priority::Normal, Priority::Normal, Priority::Off], "{during}: the file's priority is the user's");
                assert_eq!(stack(&engine).await, [], "{during}");
            }
            _ => {
                assert!(engine.listing().await.is_empty() && !pack.exists(), "{during}: all of it went");
                assert_eq!(stack(&engine).await, [Delete], "{during}: its undo is the Delete's alone");
            }
        }
        engine.stop().await;
    }
}

/// A Move to Trash of two files, one of which is taken out of the Trash by hand: its Undo puts back
/// the other with its priority, and the file that's gone stays Off, so the torrent is never missing
/// a file it gets.
#[tokio::test(flavor = "multi_thread")]
async fn undoing_a_move_to_trash_with_one_file_emptied_from_the_trash_keeps_that_file_off() {
    let dir = temp();
    let (engine, _gate, id) = gated_pack(dir.path()).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    let emptied = std::fs::read_dir(dir.path().join("trash"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with("%a.bin"))
        .unwrap();
    std::fs::remove_file(emptied.path()).unwrap();
    engine.undo().await.unwrap();
    let pack = dir.path().join("downloads/pack");
    assert!(!pack.join("a.bin").exists() && pack.join("b.bin").exists(), "b is back; a is gone for good");
    let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Normal, Priority::Normal], "a stays Off, b has the priority it had");
    for _ in 0..3 {
        size_checks(&engine, 1).await;
        assert_ne!(find(&engine, id).await.status, Status::MissingFiles);
    }
    assert_eq!(stack(&engine).await, []);
    engine.stop().await;
}

/// #162's rule for an undo cut short by a crash, for a Move to Trash's: killed while the put-back
/// waits after its first file, the service finishes the undo at start (the rest of the files back, and
/// every priority as it was); or, with something of the user's where one still in the Trash goes back,
/// puts back nothing: what had gone back before the crash returns to the Trash, the undo is refused
/// and dropped, the files stay Off, and the user's file is untouched. Either way it's off the stack.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_undo_cut_short_by_a_crash_is_finished_at_start_or_puts_back_nothing() {
    for blocked in [false, true] {
        let dir = temp();
        let pack = dir.path().join("downloads/pack");
        let id = {
            let (engine, gate, id) = gated_pack(dir.path()).await;
            engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1, 2] }).await.unwrap();
            super::trash::hold(&gate, true);
            let undoing = tokio::spawn(engine.undo_later());
            held_at_the_gate(&gate).await;
            // Past the first, which went back (the last moved goes first).
            assert!(pack.join("c.bin").exists() && !pack.join("a.bin").exists() && !pack.join("b.bin").exists());
            engine.kill().await;
            super::trash::abandon(&gate);
            let _ = undoing.await;
            id
        };
        if blocked {
            std::fs::write(pack.join("a.bin"), b"mine").unwrap();
        }
        let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
        // The undo under way is finished (or refused) as the engine starts.
        for _ in 0..500 {
            if engine.inspect(|actor| super::undo::undoing(actor.steps.machine()).next().is_none()).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(stack(&engine).await, [], "blocked: {blocked}: off the stack either way");
        let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
        if blocked {
            assert!(!pack.join("b.bin").exists() && !pack.join("c.bin").exists(), "nothing is put back, c.bin returns to the Trash");
            assert_eq!(std::fs::read(pack.join("a.bin")).unwrap(), b"mine", "the user's file is untouched");
            assert_eq!(priorities, [Priority::Off; 3], "the files stay Off");
        } else {
            assert!(pack.join("a.bin").exists() && pack.join("b.bin").exists() && pack.join("c.bin").exists(), "all back");
            assert_eq!(priorities, [Priority::Normal; 3], "each priority as it was");
        }
        assert_ne!(find(&engine, id).await.status, Status::MissingFiles);
        engine.stop().await;
    }
}

/// A Move to Trash undone with one of its files emptied from the Trash, and a stub where it was (as
/// a shared piece makes after a restart: it's Off, and librqbit writes into it): the others come
/// back with their priority, the stub is untouched, that file stays Off, and nothing is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_at_an_emptied_files_path_doesnt_stop_the_rest_of_a_move_to_trash_coming_back() {
    let dir = temp();
    let (engine, _gate, id) = gated_pack(dir.path()).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    let emptied = std::fs::read_dir(dir.path().join("trash"))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with("%a.bin"))
        .unwrap();
    std::fs::remove_file(emptied.path()).unwrap();
    let pack = dir.path().join("downloads/pack");
    std::fs::write(pack.join("a.bin"), b"stub").unwrap();
    engine.undo().await.unwrap();
    assert_eq!(std::fs::read(pack.join("a.bin")).unwrap(), b"stub", "what's at its path is untouched");
    assert!(pack.join("b.bin").exists(), "the other file is back");
    let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Normal, Priority::Normal], "the stub's file stays Off");
    assert_eq!(stack(&engine).await, []);
    engine.stop().await;
}

/// A Move to Trash asked while another torrent's Delete is trashing runs at once, beside it (#190),
/// and its step goes on the stack after the Delete's, as it came: ⌘Z then undoes the move.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_asked_during_another_torrents_delete_runs_beside_it_and_is_stacked_after() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let downloads = dir.path().join("downloads");
    let other = add_made(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap().id;
    super::trash::hold_under(&gate, &downloads.join("other.bin"));
    let delete = tokio::spawn(engine.command_later(other, TorrentCommand::Delete));
    held_at_the_gate(&gate).await;
    tokio::time::timeout(Duration::from_secs(5), engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }))
        .await
        .expect("it doesn't wait for the Delete")
        .unwrap();
    assert!(!downloads.join("pack/a.bin").exists() && !delete.is_finished());
    use proto::UndoAction::{Delete, TrashFiles};
    assert_eq!(stack(&engine).await, [Delete, TrashFiles { count: 1 }]);
    assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(TrashFiles { count: 1 }, id, "pack"));
    super::trash::hold(&gate, false);
    delete.await.unwrap().unwrap();
    assert_eq!(stack(&engine).await, [Delete]);
    engine.stop().await;
}

/// A stop while a Move to Trash's undo puts files back: the put-back ends first, and what waited for
/// it (a Delete and a Move to Trash of its torrent) isn't run, but refused as the service stopping.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_during_the_put_back_of_a_move_to_trash_runs_nothing_that_waited_for_it() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let engine = Arc::new(engine);
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    super::trash::hold(&gate, true);
    let undoing = tokio::spawn(engine.undo_later());
    held_at_the_gate(&gate).await;
    // Sent in this order, each handled before the next.
    let delete = tokio::spawn(engine.sent(id, TorrentCommand::Delete).await);
    let moving = tokio::spawn(engine.sent(id, TorrentCommand::TrashFiles { files: vec![2] }).await);
    engine.inspect(|_| ()).await;
    let stopping = tokio::spawn({
        let engine = engine.clone();
        async move { engine.stop().await }
    });
    // The stop has begun once it answers what waited: then the put-back goes on.
    for _ in 0..1000 {
        if delete.is_finished() && moving.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(super::trash::waiting(&gate), "the put-back is still held");
    super::trash::hold(&gate, false);
    stopping.await.unwrap();
    undoing.await.unwrap().unwrap();
    assert!(delete.await.unwrap().is_err() && moving.await.unwrap().is_err(), "refused, not run");
    let pack = dir.path().join("downloads/pack");
    assert!(["a.bin", "b.bin", "c.bin"].iter().all(|f| pack.join(f).exists()), "the undo finished and nothing went to the Trash after it");
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

/// A .torrent file of two sparse 64 MiB files already in the download dir, added and checking: of
/// an engine whose checks wait at a gate ([`checking_engine`]), held until the test opens it, so the
/// check is under way however fast the disk, or slow the commands (#222).
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

/// An engine whose checks wait at the gate it returns, held: [`checking_torrent`]'s. The guard opens
/// it when the test ends.
async fn checking_engine(dir: &Path) -> (Engine, Arc<super::files::Gate>, Opens) {
    let gate = Arc::new(super::files::Gate::default());
    gate.hold(true);
    let engine = engine_gated(dir, &gate).await;
    (engine, gate.clone(), Opens(gate))
}

async fn priorities(engine: &Engine, id: u64) -> Vec<Priority> {
    engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect()
}

/// The files librqbit itself gets, not the priorities the record says: `None` while librqbit
/// doesn't have the torrent.
async fn librqbit_only_files(engine: &Engine, id: u64) -> Option<BTreeSet<usize>> {
    engine.inspect(move |actor| actor.torrents[&id].lib.torrent().map(super::machine::lib::Librqbit::gets)).await
}

/// The priorities the record says that librqbit doesn't have yet, held until it can take them.
async fn held(engine: &Engine, id: u64) -> Option<BTreeSet<usize>> {
    engine
        .inspect(move |actor| {
            let torrent = &actor.torrents[&id];
            let gets = torrent.lib.torrent().map(super::machine::lib::Librqbit::gets);
            (gets.as_ref() != Some(&torrent.record.normal())).then(|| torrent.record.normal())
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_priority_set_while_checking_shows_at_once_and_applies_when_the_check_ends() {
    let dir = temp();
    let (engine, gate, _opens) = checking_engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;

    engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "still checking");
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "shown as asked");
    assert_eq!(find(&engine, id).await.content.unwrap().size_bytes, 64 << 20, "and out of the Size");

    // Changes during the check build on each other.
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
    assert_eq!(priorities(&engine, id).await, [Priority::Off, Priority::Off]);

    gate.hold(false);
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(priorities(&engine, id).await, [Priority::Off, Priority::Off], "applied: librqbit agrees");
    assert_eq!(librqbit_only_files(&engine, id).await, Some(BTreeSet::new()), "in librqbit itself, not only held here");
    assert_eq!(held(&engine, id).await, None, "nothing held any more");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_priority_held_during_a_check_survives_a_restart() {
    let dir = temp();
    let id = {
        // The check waits at the gate before its first read, so it's under way when the stop
        // comes, however fast the disk (#193).
        let gate = Arc::new(super::files::Gate::default());
        let _opens = Opens(gate.clone());
        gate.hold(true);
        let engine = engine_gated(dir.path(), &gate).await;
        let id = checking_torrent(&engine, dir.path()).await;
        engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
        assert_eq!(find(&engine, id).await.status, Status::Checking, "stopped while still checking");
        // About 3 s: the check held at the gate keeps librqbit's session alive, so the stop
        // waits out its own timeout. The old session writes nothing once the gate opens.
        engine.stop().await;
        id
    };

    let engine = engine(dir.path()).await;
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "still held, and shown");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(librqbit_only_files(&engine, id).await, Some([0].into()), "applied after the check");
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
    let saved = super::store::saved_in(&dir.path().join("data"));
    assert_eq!(saved.values().filter(|s| s.wants == super::machine::Wants::Run).count(), 1, "{saved:?}");
    let destination = AbsolutePath::new(dir.path().join("downloads")).unwrap();
    assert_eq!(saved.values().filter(|s| s.destination == destination).count(), 3, "each torrent's destination: {saved:?}");

    let engine = engine(dir.path()).await;
    wait_for(&engine, running, Status::Downloading).await;
    settled(&engine).await;
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
    let (engine, gate, _opens) = checking_engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "the check goes on");
    gate.hold(false);
    wait_for(&engine, id, Status::Paused).await;
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "stays paused");
    assert!(!librqbit_live(&engine, id).await);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// Cancelling a check (#88) leans on how librqbit ends one: a pause stops it between pieces and leaves
/// the torrent Initializing, so only a real session says whether the torrent stays stopped, shows
/// nothing checked, and comes back so after a restart. The lifecycle's own rows are the table's.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_check_leaves_the_torrent_not_checked_until_it_is_checked_again_even_after_a_restart() {
    let dir = temp();
    let id = {
        let (engine, gate, _opens) = checking_engine(dir.path()).await;
        let id = checking_torrent(&engine, dir.path()).await;
        assert!(find(&engine, id).await.actions.does(proto::Action::CancelCheck), "offered while checking");
        engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
        // The read in flight ends, and librqbit's check with it.
        gate.hold(false);
        let t = find(&engine, id).await;
        assert_eq!((t.status, t.content.unwrap().progress_bytes), (Status::Unchecked, 0));
        assert!(!t.actions.does(proto::Action::CancelCheck), "nothing left to cancel");
        assert_eq!(refusal(engine.command(id, TorrentCommand::CancelCheck).await), (id, TorrentRefusal::NotChecking));
        settled(&engine).await;
        assert_eq!(find(&engine, id).await.status, Status::Unchecked, "the check doesn't start again, nor end as a finished one");
        assert!(!librqbit_live(&engine, id).await);
        // Pause is remembered; it stays not checked.
        engine.command(id, TorrentCommand::Pause).await.unwrap();
        assert_eq!(find(&engine, id).await.status, Status::Unchecked);
        // A priority is held until the next check, and shows meanwhile.
        engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
        settled(&engine).await;
        assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "shown as asked");
        assert_eq!(held(&engine, id).await, Some([0].into()), "held: nothing applies it to a torrent not checked");
        assert_eq!(librqbit_only_files(&engine, id).await, None, "out of librqbit until it's checked, which gets it added with them");
        // Nothing of it is read when the service stops: no piece count to wait for.
        let pieces = engine
            .inspect(move |actor| {
                let hash = actor.torrents[&id].record.info_hash.clone();
                actor.pieces_kept().into_iter().find(|(h, _)| *h == hash)
            })
            .await;
        assert_eq!(pieces, None, "no pieces to wait for librqbit to save");
        engine.stop().await;
        id
    };
    let engine = engine(dir.path()).await;
    assert_eq!(find(&engine, id).await.status, Status::Unchecked, "restored not checked, not Ready");
    assert_eq!(priorities(&engine, id).await, [Priority::Normal, Priority::Off], "the held priority too");
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::Unchecked, "and it stays so");
    assert!(!librqbit_live(&engine, id).await);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Checking, "Resume checks it");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(librqbit_only_files(&engine, id).await, Some([0].into()), "the check applied it");
    engine.stop().await;
}

/// Opens the gate when the test ends, however it ends: a check held would keep the runtime from stopping.
struct Opens(Arc<super::files::Gate>);

impl Drop for Opens {
    fn drop(&mut self) {
        self.0.hold(false);
    }
}

/// An engine whose checks wait at `gate` before each read, while it's held.
async fn engine_gated(dir: &Path, gate: &Arc<super::files::Gate>) -> Engine {
    engine_gated_at(dir, super::files::At::Read, gate).await
}

/// An engine whose storage waits at `gate` at `at` while it's held.
async fn engine_gated_at(dir: &Path, at: super::files::At, gate: &Arc<super::files::Gate>) -> Engine {
    engine_guarded(dir, super::files::Guard::default().with_gate(at, gate.clone())).await
}

/// An engine whose storage is `guard`'s.
async fn engine_guarded(dir: &Path, guard: super::files::Guard) -> Engine {
    let config = Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    Engine::start_with(config, add_into, guard, super::Looks::real()).await.unwrap()
}

/// A torrent of two 64 KiB files, `pack/a.bin` and `pack/b.bin`, already in `downloads`, so adding it
/// checks to everything valid.
async fn valid_pack(dir: &Path) -> AddSource {
    let folder = dir.join("downloads/pack");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, byte) in [("a.bin", 1u8), ("b.bin", 2)] {
        std::fs::write(folder.join(name), (0..65536usize).map(|n| (n % 251) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let options = librqbit::CreateTorrentOptions { name: None, trackers: vec![], piece_length: Some(16384) };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let created = librqbit::create_torrent(&folder, options, &spawner).await.unwrap();
    AddSource::TorrentFile(created.as_bytes().unwrap().to_vec())
}

/// A torrent that had everything, checked and cancelled: nothing is known of any file, however
/// much it had before, in the pane too.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_not_checked_shows_no_progress_of_files_it_had_before() {
    let dir = temp();
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir.path(), &gate).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let had: Vec<u64> = engine.details(id).await.unwrap().files.iter().map(|f| f.progress_bytes).collect();
    assert_eq!(had, [65536, 65536]);

    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
    gate.hold(false);
    settled(&engine).await;
    let shown: Vec<u64> = engine.details(id).await.unwrap().files.iter().map(|f| f.progress_bytes).collect();
    assert_eq!((find(&engine, id).await.status, shown), (Status::Unchecked, vec![0, 0]));
    // Checked again, what it has shows again.
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    let shown: Vec<u64> = engine.details(id).await.unwrap().files.iter().map(|f| f.progress_bytes).collect();
    assert_eq!(shown, [65536, 65536]);
    engine.stop().await;
}

/// A torrent not checked whose destination goes and comes back is checked then, as any is: librqbit
/// calls it Initializing all along, which mustn't keep its files from being looked at.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_not_checked_whose_destination_comes_back_is_checked() {
    let dir = temp();
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir.path(), &gate).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
    gate.hold(false);
    wait_for(&engine, id, Status::Unchecked).await;

    let (here, away) = (dir.path().join("downloads"), dir.path().join("away"));
    std::fs::rename(&here, &away).unwrap();
    until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
    std::fs::rename(&away, &here).unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// A torrent whose file went while the service was stopped (#88's review, #45): Missing files at the
/// start, with the progress of the files still there, and out of librqbit, which never makes the
/// file again: it's added only once a check is asked for.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_missing_files_at_start_shows_its_progress_and_isnt_added() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let source = valid_pack(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    std::fs::remove_file(dir.path().join("downloads/pack/b.bin")).unwrap();

    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    gate.hold(true);
    let engine = engine_gated(dir.path(), &gate).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    settled(&engine).await;
    assert_eq!(librqbit_torrent(&engine, id).await, 0, "never added");
    assert!(!dir.path().join("downloads/pack/b.bin").exists(), "nothing made it again");
    gate.hold(false);
    assert!(
        eventually(10, || engine
            .snapshot
            .borrow()
            .snapshot
            .torrents
            .iter()
            .any(|t| t.id == id && t.content.is_some_and(|c| c.progress_bytes == 65536)))
        .await,
        "the file that's there is had: {:?}",
        find(&engine, id).await
    );
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_command_is_answered_in_every_phase_it_can_be_in() {
    let dir = temp();
    let (engine, gate, _opens) = checking_engine(dir.path()).await;
    // The others first, their checks let through; then one held checking, for every command.
    gate.hold(false);
    let fetching = add(&engine, &dir.path().join("downloads"), magnet(HASH), false).await.unwrap().id;
    let downloading = add(&engine, &dir.path().join("downloads"), torrent_file("a.bin"), false).await.unwrap().id;
    let paused = add(&engine, &dir.path().join("downloads"), torrent_file("b.bin"), true).await.unwrap().id;
    wait_for(&engine, downloading, Status::Downloading).await;
    wait_for(&engine, paused, Status::Paused).await;
    gate.hold(true);
    let checking = checking_torrent(&engine, dir.path()).await;

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
    gate.hold(false);

    engine.stop().await;
    let after = tokio::time::timeout(std::time::Duration::from_secs(1), engine.command(downloading, TorrentCommand::Pause)).await;
    match after.expect("answered after stopping too").unwrap_err() {
        Failure::Refused(refusal) => assert_eq!(refusal, proto::Refusal::Stopping),
        failed => panic!("not refused, failed: {failed:?}"),
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

/// How many times `id` was added to librqbit, to tell an add again (a check) from the same torrent.
async fn librqbit_torrent(engine: &Engine, id: u64) -> u32 {
    engine.inspect(move |actor| actor.torrents[&id].added).await
}

async fn librqbit_live(engine: &Engine, id: u64) -> bool {
    engine.inspect(move |actor| actor.torrents[&id].lib.torrent().is_some_and(|h| h.live().is_some())).await
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
    settled(&engine).await;
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
    watching(&engine, &dir.path().join("downloads")).await;

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
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "an Off file is never missing");

    // Set Normal again, it's downloaded from nothing (#276): Missing files is for a file it gets
    // that vanished.
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    assert!(file(dir.path(), "b.bin").exists(), "made at its path");
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

/// #366's review: a file deleted by hand, its destination gone, the service stopped, the destination
/// back before the next start. Another file put at the missing file's path then isn't the torrent's
/// own: it's left as it is, and the torrent stays Missing files. The count shown for the missing
/// file is not what's saved or decided from.
#[tokio::test(flavor = "multi_thread")]
async fn another_file_put_where_one_was_missing_across_a_stop_while_its_destination_was_gone_isnt_downloaded_over() {
    let dir = temp();
    let (here, away) = (dir.path().join("downloads"), dir.path().join("away"));
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &here, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
        wait_for(&engine, id, Status::MissingFiles).await;
        std::fs::rename(&here, &away).unwrap();
        until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
        assert_eq!(files_on(&engine, id).await[1].1, 0, "the missing file isn't counted while away");
        engine.stop().await;
        id
    };
    std::fs::rename(&away, &here).unwrap();
    let engine = engine(dir.path()).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let other = vec![0u8; 2 * PIECE];
    std::fs::write(file(dir.path(), "b.bin"), &other).unwrap();

    // Its size check sees the file, and a check ends.
    judged(&engine, id).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "not the file that went");
    assert_eq!(files_on(&engine, id).await[1], ("b.bin".into(), 0, true));
    assert_eq!(std::fs::read(file(dir.path(), "b.bin")).unwrap(), other, "not written over");
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
        settled(&engine).await;
        assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "though librqbit made the file again");
        assert!(!librqbit_live(&engine, id).await);
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
    settled(&engine).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "not missing");

    let before = librqbit_torrent(&engine, id).await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    assert_ne!(librqbit_torrent(&engine, id).await, before, "added to librqbit again");
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(files_on(&engine, id).await, [("a.bin".into(), FULL, false), ("b.bin".into(), 0, false), ("c.bin".into(), FULL, false)]);

    engine.command(id, TorrentCommand::Pause).await.unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Paused).await;
    settled(&engine).await;
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
    let (engine, gate, _opens) = checking_engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    gate.hold(false);
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
    settled(&engine).await;
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
    settled(&engine).await;
    assert_eq!(librqbit_torrent(&engine, id).await, checked, "not checked over and over");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_restart_a_missing_file_isnt_made_again_and_counts_once_put_back() {
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
    settled(&engine).await;
    assert!(!file(dir.path(), "b.bin").exists(), "out of librqbit (#45): nothing makes it again");
    assert_eq!(librqbit_torrent(&engine, id).await, 0, "not added until it's checked");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);

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
    size_checks(&engine, 3).await;
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
    size_checks(&engine, 3).await;
    assert!(!destination.exists(), "listed without making its folder");
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
    assert!(matches!(missing, Failure::Error(proto::ServiceError::TorrentUrlStatus { status: 404, .. })), "{missing:?}");
    assert!(engine.listing().await.is_empty(), "a failed download adds nothing");
    engine.stop().await;
}

/// A seeder of one 48 MiB file in `dir` with a limited upload speed, on a local port: the .torrent,
/// its info hash and the port. The session is kept alive by the caller.
async fn seeder(dir: &Path) -> (std::sync::Arc<librqbit::Session>, Vec<u8>, String, u16) {
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
    seed(dir, &files.join("big.bin"), 256 * 1024).await
}

/// A seeder of what's at `path` (a file, or a folder of them), in pieces of `piece_length`, with
/// a limited upload speed, on a local port: as [`seeder`].
async fn seed(dir: &Path, path: &Path, piece_length: u32) -> (std::sync::Arc<librqbit::Session>, Vec<u8>, String, u16) {
    use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, SessionOptions, create_torrent};
    // librqbit picks the port: one let go here could still be held by a child spawned meanwhile (#166).
    let session = librqbit::Session::new_with_opts(
        dir.join("seed-session"),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
            ratelimits: librqbit::limits::LimitsConfig { upload_bps: std::num::NonZeroU32::new(8 << 20), download_bps: None },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let port = session.listen_addr().expect("the seeder listens").port();
    let announce = tracker(port);
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(2);
    let options = CreateTorrentOptions { piece_length: Some(piece_length), trackers: vec![announce], ..Default::default() };
    let created = create_torrent(path, options, &spawner).await.unwrap();
    let bytes = created.as_bytes().unwrap().to_vec();
    // librqbit puts a torrent of several files straight into the folder it's given.
    let folder = if path.is_dir() { path } else { path.parent().unwrap() };
    let opts = AddTorrentOptions { overwrite: true, output_folder: Some(folder.to_str().unwrap().into()), ..Default::default() };
    let added = session.add_torrent(AddTorrent::from_bytes(bytes.clone()), Some(opts)).await.unwrap();
    let hash = added.into_handle().unwrap().info_hash().as_string();
    (session, bytes, hash, port)
}

/// A tracker that names the peer on local `port` to whoever asks: its announce URL.
fn tracker(port: u16) -> String {
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
    announce
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

/// A disk image, mounted in `dir` out of Finder's sight, and detached when dropped. macOS only:
/// `hdiutil` makes it.
#[cfg(target_os = "macos")]
struct SmallDisk {
    mount: PathBuf,
}

#[cfg(target_os = "macos")]
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
            let _spawning = crate::spawn_lock::spawning();
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

#[cfg(target_os = "macos")]
impl Drop for SmallDisk {
    fn drop(&mut self) {
        let _spawning = crate::spawn_lock::spawning();
        let _ = std::process::Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
    }
}

/// A real disk too small for a torrent, which can't be generated: before anything downloads, the
/// snapshot warns of both the disk and the torrent (it alone needs more than is free); the add
/// isn't refused; and once the disk is full, the torrent is an Error saying so, the path a full
/// disk takes (#127).
// macOS only: the disk is an image `hdiutil` makes.
#[cfg(target_os = "macos")]
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
    assert_eq!(status, Status::Error { failure: proto::TorrentFailure::DiskFull });
    engine.stop().await;
}

/// A .torrent file of one file, `name`, of `len` bytes in 1 MiB pieces, that no one seeds. For the
/// disk images' tests, macOS only.
#[cfg(target_os = "macos")]
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
// macOS only: the disk is an image `hdiutil` makes.
#[cfg(target_os = "macos")]
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
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.path().join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = std::sync::Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    let look = stalling.look(folder.canonicalize().unwrap());
    let engine = Engine::start_with(config, add_into, super::files::Guard::default(), super::Looks { disks: look, ..super::Looks::real() })
        .await
        .unwrap();
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
    // Each tick while the look is stuck reports it stalled, and starts no other.
    looks(&engine, 3).await;
    assert_eq!(stalling.looks.load(std::sync::atomic::Ordering::SeqCst), 1, "one look at a time, after {answered} answers");

    stalling.set(false);
    assert!(eventually(10, || matches!(free_for(&engine, id), Some(Some(_)))).await, "its free space, back");
    engine.stop().await;
}

/// Free space follows the disk (#127): generated writes and deletes on a real disk (APFS, so the
/// torrent's file is sparse and it needs space there all along), and after each the snapshot's
/// free space for that disk comes to what statfs says.
// macOS only: the disk is an image `hdiutil` makes.
#[cfg(target_os = "macos")]
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
                    // Free space is kept per disk for 3 s: the write's effect is asked for at once.
                    super::space::cache().refresh();
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
            poll_every: super::super::POLL_EVERY,
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

    cases::proptest! {
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
/// downloads for real.
///
/// Measured against the limit, never against a clock's speed: the bytes that arrive after it is set
/// stay within what was already in flight plus the limit times the time that really passed (read
/// back after each sample, so a stalled test thread only loosens the bound), and after it is
/// lifted the torrent gets further than the limit could carry it.
#[tokio::test(flavor = "multi_thread")]
async fn a_limit_set_while_downloading_applies_at_once() {
    const LIMIT: u64 = 256 << 10;
    const PIECE: u64 = 256 << 10;
    // Asked for before the limit and still arriving after it: librqbit's requests in flight to the
    // peer, about 2.6 MiB seen. Unlimited, the seeder alone sends more than this in half a second.
    const IN_FLIGHT: u64 = 5 << 20;
    let allowed = |elapsed: Duration| IN_FLIGHT + PIECE + (LIMIT as f64 * elapsed.as_secs_f64()) as u64;

    let dir = temp();
    let (_seeder, torrent, _, _) = seeder(dir.path()).await;
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    let progress = || async { find(&engine, id).await.content.map_or(0, |c| c.progress_bytes) };
    let started = std::time::Instant::now();
    while !(find(&engine, id).await.status == Status::Downloading && progress().await > 1 << 20) {
        assert!(started.elapsed() < Duration::from_secs(60), "the download never got going");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let limit = proto::SpeedLimit::new(LIMIT).unwrap();
    engine.set_limits(proto::SpeedLimits { download: Some(limit), upload: None });
    let (set_at, before) = (std::time::Instant::now(), progress().await);
    let mut elapsed = Duration::ZERO;
    while elapsed < Duration::from_secs(4) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let gained = progress().await - before;
        elapsed = set_at.elapsed();
        assert!(
            gained <= allowed(elapsed),
            "{gained} bytes in {elapsed:?} after a limit of {LIMIT} B/s, at most {} let through",
            allowed(elapsed)
        );
    }

    engine.set_limits(proto::SpeedLimits::default());
    let (lifted_at, before) = (std::time::Instant::now(), progress().await);
    loop {
        let gained = progress().await - before;
        let elapsed = lifted_at.elapsed();
        // More than the limit could have carried it, in-flight bytes and all.
        if gained > allowed(elapsed) + (1 << 20) {
            break;
        }
        assert!(elapsed < Duration::from_secs(30), "{gained} bytes in {elapsed:?} after the limit was lifted");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
            poll_every: super::super::POLL_EVERY,
            trash: super::super::trash::into(dir.join("trash"), Default::default(), Default::default()),
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
        // Three polls of it seeding, where the limit is applied: 1 to 1.5 s of the new round.
        polls(&engine, 3).await;
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "a Resume isn't undone by the same limit straight away");

        // A second or so into the new round, as the engine counted it: kept across the restart,
        // where it goes on until it reaches the limit.
        let round = async |engine: &Engine| engine.inspect(move |a| a.torrents[&id].meter.round).await;
        let kept = round(&engine).await;
        assert!(kept.seeded > Duration::ZERO && kept.seeded < Duration::from_secs(2), "a new round: {kept:?}");
        engine.stop().await;
        let engine = start(dir.path(), limits).await;
        let restored = round(&engine).await;
        assert!(restored.seeded >= kept.seeded, "the round goes on after the restart: {restored:?}, of {kept:?}");
        until(&engine, id, &done).await;
        assert!(round(&engine).await.seeded >= Duration::from_secs(2), "it reached the limit");
        engine.stop().await;
    }

    /// A leecher on this machine, listening, that a local tracker names to whoever announces:
    /// the engine (which doesn't listen) connects to it and uploads. Its session, the .torrent of
    /// one file, complete in the engine's download dir, and the tracker's gate: it holds the first
    /// announce until that's sent (or dropped), so the engine meets the leecher only then.
    async fn leecher(dir: &Path) -> (std::sync::Arc<librqbit::Session>, Vec<u8>, std::sync::mpsc::Sender<()>) {
        use librqbit::{AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, SessionOptions, create_torrent};
        // librqbit picks the port: one let go here could still be held by a child spawned meanwhile (#166).
        let session = librqbit::Session::new_with_opts(
            dir.join("leecher"),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let port = session.listen_addr().expect("the leecher listens").port();
        let tracker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let announce = format!("http://{}/announce", tracker.local_addr().unwrap());
        let (open, opened) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut body = b"d8:intervali60e5:peers6:\x7f\x00\x00\x01".to_vec();
            body.extend(port.to_be_bytes());
            body.push(b'e');
            let mut held = true;
            for stream in tracker.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.read(&mut [0; 2048]);
                if std::mem::take(&mut held) {
                    let _ = opened.recv();
                }
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
        let opts = AddTorrentOptions { output_folder: Some(dir.join("leeched").to_str().unwrap().into()), ..Default::default() };
        session.add_torrent(AddTorrent::from_bytes(torrent.clone()), Some(opts)).await.unwrap();
        (session, torrent, open)
    }

    /// The ratio limit on a real engine uploading to a real leecher, which can't be generated:
    /// librqbit's upload counts, and the torrent stops at the ratio, Done saying so.
    #[tokio::test(flavor = "multi_thread")]
    async fn uploads_reach_the_ratio_limit_and_stop_seeding() {
        let dir = temp();
        let (_leecher, torrent, open) = leecher(dir.path()).await;
        let ratio = Ratio::new(50).unwrap();
        let engine = start(dir.path(), Limits { ratio: Some(ratio), time: None }).await;
        let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
        // The leecher could take the ratio's worth between two polls, before Seeding is ever
        // published, and the torrent go straight to Done (#168): it's named only once seen Seeding.
        until(&engine, id, &Status::Seeding).await;
        open.send(()).unwrap();
        until(&engine, id, &Status::Done { reached: Some(Reached::Ratio(ratio)) }).await;
        let round = engine.inspect(move |actor| actor.torrents[&id].meter.round).await;
        assert!(round.uploaded >= 512 << 10, "stopped at ratio 0.5 having uploaded {} of 1 MiB", round.uploaded);
        engine.stop().await;
    }
}

/// What can't be generated: the model's files fill whole pieces and nothing downloads there. A
/// piece shared by a file set Off and one the torrent gets is downloaded, and librqbit writes it
/// into both: the file set Off, deleted, comes back as a stub of its length holding only that
/// piece's part (#67).
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_and_deleted_comes_back_only_as_the_stub_of_a_piece_it_shares() {
    const LEN: usize = PIECE + PIECE / 2;
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    let downloads = dir.path().join("downloads/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    for (i, name) in ["a.bin", "b.bin"].iter().enumerate() {
        let bytes: Vec<u8> = (0..LEN).map(|n| (n * 7 + i * 31) as u8 | 1).collect();
        std::fs::write(seeded.join(name), &bytes).unwrap();
        std::fs::write(downloads.join(name), &bytes).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, PIECE as u32).await;
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // In the .torrent's order: the first's second piece is the second's first.
    let order = engine.details(id).await.unwrap().files;
    let (off, getting) = (&order[0].path, &order[1].path);
    let off_bytes = std::fs::read(seeded.join(off)).unwrap();

    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
    std::fs::remove_file(downloads.join(off)).unwrap();
    std::fs::remove_file(downloads.join(getting)).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;

    assert_eq!(std::fs::read(downloads.join(getting)).unwrap(), std::fs::read(seeded.join(getting)).unwrap(), "downloaded");
    let stub = std::fs::read(downloads.join(off)).unwrap();
    assert_eq!(stub.len(), LEN, "of its length");
    assert!(stub[..PIECE].iter().all(|&b| b == 0), "nothing of its own piece");
    assert_eq!(stub[PIECE..], off_bytes[PIECE..], "the part of the piece it shares");
}

/// #376: librqbit writes 0 bytes into the file just before a chunk. With the file before it Off and
/// absent (here whole pieces, so no stub of a shared piece is wanted), that makes no file.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_and_absent_is_not_made_by_a_write_of_nothing() {
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    let downloads = dir.path().join("downloads/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    for (i, name) in ["a.bin", "b.bin"].iter().enumerate() {
        let bytes: Vec<u8> = (0..PIECE * 2).map(|n| (n * 7 + i * 31) as u8 | 1).collect();
        std::fs::write(seeded.join(name), &bytes).unwrap();
        std::fs::write(downloads.join(name), &bytes).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, PIECE as u32).await;
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let order = engine.details(id).await.unwrap().files;
    let (off, getting) = (&order[0].path, &order[1].path);

    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
    std::fs::remove_file(downloads.join(off)).unwrap();
    std::fs::remove_file(downloads.join(getting)).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;

    assert_eq!(std::fs::read(downloads.join(getting)).unwrap(), std::fs::read(seeded.join(getting)).unwrap(), "downloaded");
    assert!(!downloads.join(off).exists(), "no file for the one set Off: nothing of it was written");
    engine.stop().await;
}

// Undo (#150).

/// `downloads/pack`'s files as they are now: each file's bytes, by name.
fn pack_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<_> = std::fs::read_dir(dir.join("downloads/pack"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect();
    all.sort();
    all
}

/// An entry as the tests name it: its seq is the service's.
fn undoable(action: proto::UndoAction, id: u64, name: &str) -> proto::Undoable {
    proto::Undoable { seq: 0, action, torrents: vec![proto::Named { id, name: name.into() }] }
}

fn unnumbered(undo: impl Undid) -> proto::Undoable {
    proto::Undoable { seq: 0, ..undo.undoable() }
}

/// What an undo answer says, for tests: what came back, of one torrent.
pub(super) trait Undid {
    fn undoable(self) -> proto::Undoable;
    /// Its one torrent's id.
    fn id(&self) -> u64;
}

impl Undid for proto::Undoable {
    fn undoable(self) -> proto::Undoable {
        self
    }
    fn id(&self) -> u64 {
        self.torrents[0].id
    }
}

impl Undid for proto::Undone {
    fn undoable(self) -> proto::Undoable {
        assert!(self.refused.is_empty(), "all of it came back: {self:?}");
        self.undo
    }
    fn id(&self) -> u64 {
        self.undo.id()
    }
}

/// The undo refusal `result` is.
fn cant_undo<T: std::fmt::Debug>(result: Result<T, Failure>) -> (proto::Undoable, proto::CantUndo) {
    match result.unwrap_err() {
        Failure::Refused(proto::Refusal::Undo { undo, why, .. }) => (unnumbered(undo), why),
        other => panic!("not an undo refusal: {other:?}"),
    }
}

async fn top(engine: &Engine) -> Option<proto::Undoable> {
    engine.subscribe().borrow().snapshot.undo.last().cloned().map(unnumbered)
}

/// librqbit keeps a torrent's .torrent and the pieces it has in its session folder as
/// `<info hash>.torrent` and `<info hash>.bitv`, and deletes both with the torrent; Undo keeps them
/// aside first, and puts the pieces back while the files are as they were. The pieces kept here
/// leave out c.bin's, though it's whole on disk: the torrent comes back without them, so librqbit
/// took the kept pieces rather than checking. A librqbit that keeps them elsewhere, or otherwise,
/// fails here. Then c.bin written by hand while the torrent is removed (the same bytes even) makes
/// it check them all.
///
/// An example, not generated: the full check it tells apart from librqbit taking the pieces kept is
/// librqbit's, which the model takes as given.
#[tokio::test(flavor = "multi_thread")]
async fn undo_remove_brings_the_torrent_back_with_its_pieces_and_priorities_without_a_full_check() {
    let dir = temp();
    let source = complete_torrent(dir.path()).await;
    let c = file(dir.path(), "c.bin");
    let valid_c = std::fs::read(&c).unwrap();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, true).await.unwrap().id;
    wait_for(&engine, id, Status::Done { reached: None }).await;
    let hash = find(&engine, id).await.info_hash;
    let session = dir.path().join("data/session");
    assert!(session.join(format!("{hash}.torrent")).exists(), "librqbit keeps the .torrent as <hash>.torrent");
    let bits = std::fs::read(super::bitv::path(&session, &hash)).expect("librqbit keeps the pieces as <hash>.bitv");
    assert_eq!(super::bitv::pieces(&bits), 6, "two pieces of each file");
    let b = index_of(&engine, id, "b.bin").await;
    let c_index = index_of(&engine, id, "c.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
    let before = engine.details(id).await.unwrap();
    assert_eq!(top(&engine).await, None);

    engine.command(id, TorrentCommand::Remove).await.unwrap();
    assert!(!session.join(format!("{hash}.bitv")).exists(), "librqbit deletes the pieces with the torrent");
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Remove, id, "pack")));
    // The kept pieces without c.bin's: each file is two pieces, in the .torrent's order.
    let kept = std::fs::read_dir(dir.path().join("data/undo")).unwrap().flatten().map(|e| e.path().join("bitv")).find(|p| p.exists());
    let kept = kept.expect("the pieces are kept aside");
    let mut bits = std::fs::read(&kept).unwrap();
    for piece in [2 * c_index, 2 * c_index + 1] {
        bits[piece / 8] &= !(0x80 >> (piece % 8));
    }
    // A new file put in its place: librqbit's last flush of the pieces, on dropping the torrent,
    // goes through the file it holds open, the stash's hard link, and may land after Remove is
    // answered (#193). It then writes the old file, and the edit stays.
    let edited = kept.with_extension("edited");
    std::fs::write(&edited, &bits).unwrap();
    std::fs::rename(&edited, &kept).unwrap();

    assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(proto::UndoAction::Remove, id, "pack"));
    sampled(&engine, id).await;
    until(&engine, id, |s| *s != Status::Checking).await;
    let after = find(&engine, id).await;
    assert_eq!((after.id, after.status), (id, Status::Paused), "the same id, stopped as it was, without c.bin's pieces");
    let files = |d: &proto::Details| d.files.iter().map(|f| (f.path.clone(), f.priority, f.progress_bytes)).collect::<Vec<_>>();
    let without_c: Vec<_> = files(&before)
        .into_iter()
        .map(|(path, p, progress)| (path.clone(), p, if path.ends_with("c.bin") { 0 } else { progress }))
        .collect();
    assert_eq!(files(&engine.details(id).await.unwrap()), without_c, "the same priorities, and the pieces kept: c.bin unchecked");
    assert_eq!(top(&engine).await, None);
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))));

    // c.bin written by hand while it's removed: the pieces kept don't hold, it's checked in full.
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    std::fs::write(&c, &valid_c).unwrap();
    engine.undo().await.unwrap();
    until(&engine, id, |s| *s != Status::Checking).await;
    assert_eq!(files(&engine.details(id).await.unwrap()), files(&before), "checked: c.bin found whole");
    engine.stop().await;
}

/// Delete, then Undo: the same bytes at the same paths, and the torrent as it was, seeding.
#[tokio::test(flavor = "multi_thread")]
async fn undo_delete_puts_the_files_back_and_the_torrent_with_them() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = pack_bytes(dir.path());
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    assert!(!dir.path().join("downloads/pack").exists(), "in the trash");
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Delete, id, "pack")));

    assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(proto::UndoAction::Delete, id, "pack"));
    assert_eq!(pack_bytes(dir.path()), before);
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 3 * FULL);
    assert!(std::fs::read_dir(dir.path().join("trash")).unwrap().next().is_none(), "nothing left in the trash");
    engine.stop().await;
}

/// A complete torrent of three files of its own, `k` telling their bytes apart, in `dir/<folder>/pack`.
async fn complete_torrent_in(dir: &Path, folder: &str, k: u8) -> AddSource {
    let pack = dir.join(folder).join("pack");
    std::fs::create_dir_all(&pack).unwrap();
    for (i, name) in ["a.bin", "b.bin", "c.bin"].iter().enumerate() {
        let bytes: Vec<u8> = (0..2 * PIECE).map(|n| (n * 7 + i * 31) as u8 ^ k).collect();
        std::fs::write(pack.join(name), bytes).unwrap();
    }
    let options = librqbit::CreateTorrentOptions {
        name: None,
        trackers: vec!["http://127.0.0.1:1/announce".into()],
        piece_length: Some(PIECE as u32),
    };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    AddSource::TorrentFile(librqbit::create_torrent(&pack, options, &spawner).await.unwrap().as_bytes().unwrap().to_vec())
}

/// Remove three torrents in one press (#235), then one ⌘Z: one entry on the stack, and all three
/// back; the same for Delete, their files back from the Trash. An example, as the model's
/// selections are compared through the whole state: this one names the user's sequence.
#[tokio::test(flavor = "multi_thread")]
async fn remove_or_delete_three_then_one_undo_brings_all_three_back() {
    for (command, action) in [(TorrentCommand::Remove, proto::UndoAction::Remove), (TorrentCommand::Delete, proto::UndoAction::Delete)] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let mut ids = Vec::new();
        for k in 0..3u8 {
            let folder = format!("downloads{k}");
            let source = complete_torrent_in(dir.path(), &folder, k + 1).await;
            ids.push(add_to(&engine, source, false, &dir.path().join(&folder)).await.unwrap().id);
        }
        for &id in &ids {
            wait_for(&engine, id, Status::Seeding).await;
        }
        let before: Vec<Vec<u8>> = (0..3).map(|k| std::fs::read(dir.path().join(format!("downloads{k}/pack/a.bin"))).unwrap()).collect();
        assert!(engine.commands(ids.clone(), command.clone()).await.unwrap().is_empty(), "{action:?}: each did as asked");
        assert!(listed_now(&engine).await.is_empty(), "{action:?}: none listed");
        let one = proto::Undoable { seq: 0, action, torrents: ids.iter().map(|&id| proto::Named { id, name: "pack".into() }).collect() };
        let stack = engine.subscribe().borrow().snapshot.undo.clone();
        assert_eq!(stack.into_iter().map(unnumbered).collect::<Vec<_>>(), [one.clone()], "{action:?}: one press, one entry");

        // The answer lists them as each came back, which no order rules: by id.
        let mut undone = unnumbered(engine.undo().await.unwrap());
        undone.torrents.sort_by_key(|t| t.id);
        assert_eq!(undone, one, "{action:?}: one ⌘Z, all three");
        for &id in &ids {
            wait_for(&engine, id, Status::Seeding).await;
        }
        let after: Vec<Vec<u8>> = (0..3).map(|k| std::fs::read(dir.path().join(format!("downloads{k}/pack/a.bin"))).unwrap()).collect();
        assert_eq!(after, before, "{action:?}: the files as they were");
        assert_eq!(top(&engine).await, None, "{action:?}: nothing left to undo");
        engine.stop().await;
    }
}

/// The stack is kept in the data dir: a Delete before a restart is undone after it.
#[tokio::test(flavor = "multi_thread")]
async fn undo_works_across_a_restart() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let before = pack_bytes(dir.path());
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    engine.stop().await;

    let engine = self::engine(dir.path()).await;
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Delete, id, "pack")));
    engine.undo().await.unwrap();
    assert_eq!(pack_bytes(dir.path()), before);
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// The records set aside as damaged start ids again, but `undo.json` still names the old ones: an
/// Undo never brings a torrent back under the id of one added since.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_never_brings_a_torrent_back_under_an_id_a_newer_one_has() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    let engine = engine(dir.path()).await;
    let removed = add(&engine, &downloads, magnet(HASH), false).await.unwrap().id;
    engine.command(removed, TorrentCommand::Remove).await.unwrap();
    engine.stop().await;
    let data = dir.path().join("data");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(data.join(format!("records.db{suffix}")));
    }
    std::fs::write(data.join("records.db"), b"not a database").unwrap();

    let engine = self::engine(dir.path()).await;
    let newer = add(&engine, &downloads, magnet("00000000000000000000000000000000000000bb"), false).await.unwrap().id;
    assert_ne!(newer, removed, "an id undo.json names isn't given again");
    engine.undo().await.unwrap();
    let ids: Vec<(u64, String)> = engine.listing().await.into_iter().map(|t| (t.id, t.info_hash)).collect();
    assert_eq!(ids.len(), 2, "both are listed: {ids:?}");
    assert_eq!(saved_ids(&data), [(removed, HASH.to_owned()), (newer, "00000000000000000000000000000000000000bb".to_owned())]);
    engine.stop().await;
}

fn saved_ids(data: &Path) -> Vec<(u64, String)> {
    let mut ids: Vec<_> = super::store::saved_in(data).into_iter().map(|(hash, saved)| (saved.id, hash)).collect();
    ids.sort();
    ids
}

/// An undo whose files left the Trash is refused, and dropped: the one under it is next.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_after_the_trash_was_emptied_is_refused_and_dropped() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let magnet_id = add(&engine, &downloads, magnet(HASH), false).await.unwrap().id;
    let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(magnet_id, TorrentCommand::Remove).await.unwrap();
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    std::fs::remove_dir_all(dir.path().join("trash")).unwrap();

    let (undo, why) = cant_undo(engine.undo().await);
    assert_eq!(undo, undoable(proto::UndoAction::Delete, id, "pack"));
    assert!(matches!(why, proto::CantUndo::NotInTrash { path } if path.ends_with("pack")), "the folder went whole");
    assert!(!downloads.join("pack").exists() && engine.listing().await.iter().all(|t| t.id != id), "nothing changed");
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Remove, magnet_id, "Big Buck Bunny")), "dropped");
    engine.undo().await.unwrap();
    let back = find(&engine, magnet_id).await;
    assert_eq!((back.status, back.name.as_deref()), (Status::FetchingInfo, Some("Big Buck Bunny")), "a magnet comes back fetching info");
    engine.stop().await;
}

/// Something written where a file goes back stays: the undo is refused before anything moves.
#[tokio::test(flavor = "multi_thread")]
async fn undo_never_puts_a_file_back_over_one_there() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    std::fs::create_dir_all(dir.path().join("downloads/pack")).unwrap();
    std::fs::write(file(dir.path(), "mine.txt"), b"mine").unwrap();
    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(why, proto::CantUndo::PathTaken { path } if path.ends_with("pack")));
    assert_eq!(pack_bytes(dir.path()), [("mine.txt".to_owned(), b"mine".to_vec())]);
    engine.stop().await;
}

/// A partial Delete undoes what it moved: the file that went comes back, and the torrent is whole
/// again, as it was before.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_delete_undoes_what_it_moved() {
    let dir = temp();
    let refusing = Arc::new(Mutex::new(Vec::new()));
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    std::fs::write(file(dir.path(), "mine.txt"), b"mine").unwrap();
    let before = pack_bytes(dir.path());
    *refusing.lock().unwrap() = vec![file(dir.path(), "a.bin")];
    refusal(engine.command(id, TorrentCommand::Delete).await);
    assert!(!file(dir.path(), "b.bin").exists());
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Delete, id, "pack")));

    engine.undo().await.unwrap();
    assert_eq!(pack_bytes(dir.path()), before);
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(find(&engine, id).await.content.unwrap().progress_bytes, 3 * FULL);
    engine.stop().await;
}

/// Why entries left the undo stack, as the snapshot every client gets says (#200).
fn left(engine: &Engine) -> Vec<proto::Left> {
    engine.subscribe().borrow().snapshot.left.clone()
}

fn seqs(engine: &Engine) -> Vec<u64> {
    engine.subscribe().borrow().snapshot.undo.iter().map(|u| u.seq).collect()
}

/// A Remove pushes the oldest past the last 20 and the next ⌘Z takes it back at once: the snapshot
/// the clients get next says the oldest aged and the newest was undone, whenever they look.
#[tokio::test(flavor = "multi_thread")]
async fn an_entry_pushed_past_twenty_is_aged_even_when_an_undo_follows_at_once() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let mut first = None;
    for n in 0..21u64 {
        let id = add(&engine, &downloads, magnet(&format!("{n:040x}")), false).await.unwrap().id;
        engine.command(id, TorrentCommand::Remove).await.unwrap();
        first = first.or_else(|| seqs(&engine).first().copied());
    }
    let first = first.unwrap();
    assert_eq!(left(&engine), [proto::Left { seq: first, why: proto::Leave::Aged }], "the oldest of 21 is past the last 20");
    let id = add(&engine, &downloads, magnet(&format!("{:040x}", 99)), false).await.unwrap().id;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    let newest = *seqs(&engine).last().unwrap();
    engine.undo().await.unwrap();
    let left = left(&engine);
    assert_eq!(left.len(), 3, "{left:?}");
    assert_eq!((left[0].seq, &left[0].why), (first, &proto::Leave::Aged));
    assert_eq!(left[1].why, proto::Leave::Aged, "pushed out by the 22nd");
    assert_eq!(left[2], proto::Left { seq: newest, why: proto::Leave::Undone { refused: Vec::new() } });
    engine.stop().await;
}

/// An undo that was refused, whoever asked, says why in the snapshot, and it outlasts a restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_undo_says_why_to_every_client_and_after_a_restart() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    let seq = seqs(&engine)[0];
    std::fs::remove_dir_all(dir.path().join("trash")).unwrap();
    let (_, why) = cant_undo(engine.undo().await);
    let refused = |why: &proto::CantUndo| proto::Left { seq, why: proto::Leave::Refused { why: why.clone(), stranded: Vec::new() } };
    assert_eq!(left(&engine), [refused(&why)], "{why:?}");
    engine.stop().await;

    let engine = self::engine(dir.path()).await;
    assert_eq!(left(&engine), [refused(&why)], "kept in undo.json");
    engine.stop().await;
}

/// A Delete of a torrent whose partial Delete is on the stack takes its place: the partial one left
/// as absorbed, by it, not undone.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_that_takes_a_partial_ones_place_leaves_it_absorbed() {
    let dir = temp();
    let refusing = Arc::new(Mutex::new(Vec::new()));
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default()).await;
    let downloads = dir.path().join("downloads");
    let added = add_made(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
    std::fs::write(downloads.join("pack/mine.txt"), b"mine").unwrap();
    *refusing.lock().unwrap() = vec![downloads.join("pack/a.bin")];
    refusal(engine.command(added.id, TorrentCommand::Delete).await);
    let partial = seqs(&engine)[0];
    refusing.lock().unwrap().clear();
    engine.command(added.id, TorrentCommand::Delete).await.unwrap();
    let whole = seqs(&engine);
    assert_eq!(whole.len(), 1, "one entry now: {whole:?}");
    assert_eq!(left(&engine), [proto::Left { seq: partial, why: proto::Leave::Absorbed { by: whole[0] } }]);
    engine.stop().await;
}

/// A Delete of a torrent whose partial Delete is on the stack, taken back by ⌘Z while it runs, takes
/// the partial one with it: its rollback puts back what both moved, so the partial one leaves
/// absorbed, and nothing is left to undo, every file back. Before, the partial one stayed on the
/// stack with its items gone from the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_running_delete_takes_back_its_torrents_partial_delete_with_it() {
    let dir = temp();
    let refusing = Arc::new(Mutex::new(Vec::new()));
    let gate = super::trash::Gate::default();
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), gate.clone()).await;
    let downloads = dir.path().join("downloads");
    let added = add_made(&engine, &downloads, multi_file_torrent(&["a.bin", "b.bin"]), true).await.unwrap();
    // Added paused, librqbit checks it first, and creates its files and sets their length as it
    // does: what's on disk, and the Status the Deletes find, are settled once that's over.
    until(&engine, added.id, |s| *s != Status::Checking).await;
    let before = std::fs::read(downloads.join("pack/b.bin")).unwrap();
    std::fs::write(downloads.join("pack/mine.txt"), b"mine").unwrap();
    *refusing.lock().unwrap() = vec![downloads.join("pack/a.bin")];
    refusal(engine.command(added.id, TorrentCommand::Delete).await);
    let partial = seqs(&engine)[0];
    assert!(!downloads.join("pack/b.bin").exists(), "b.bin went");
    refusing.lock().unwrap().clear();
    super::trash::hold(&gate, true);
    let delete = engine.sent(added.id, TorrentCommand::Delete).await;
    held_at_the_gate(&gate).await;
    let undo = engine.undo_sent().await;
    engine.inspect(|_| ()).await;
    super::trash::hold(&gate, false);
    let _ = tokio::time::timeout(Duration::from_secs(20), delete).await.expect("the Delete is answered");
    tokio::time::timeout(Duration::from_secs(20), undo).await.expect("the Undo is answered").unwrap();
    let left = left(&engine);
    assert!(
        left.iter().any(|l| l.seq == partial && matches!(l.why, proto::Leave::Absorbed { .. })),
        "the partial Delete left absorbed: {left:?}"
    );
    assert!(matches!(left.last().map(|l| &l.why), Some(proto::Leave::Undone { .. })), "and the running one undone: {left:?}");
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "nothing stale left to undo");
    assert_eq!(std::fs::read(downloads.join("pack/b.bin")).unwrap(), before, "b.bin back");
    assert!(downloads.join("pack/a.bin").exists());
    engine.stop().await;
}

/// Remove two torrents in one press, add one again by hand, ⌘Z: the other comes back, and the
/// answer and the service's word on the entry (which every client reads) say the one added again
/// didn't, and why (#235).
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_undo_of_a_selection_says_which_torrent_didnt_come_back_to_every_client() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let a = add(&engine, &downloads, magnet(&format!("{:040x}", 0xa1)), true).await.unwrap().id;
    let b = add(&engine, &downloads, magnet(&format!("{:040x}", 0xb1)), true).await.unwrap().id;
    assert!(engine.commands(vec![a, b], TorrentCommand::Remove).await.unwrap().is_empty());
    let seq = seqs(&engine)[0];
    let again = add(&engine, &downloads, magnet(&format!("{:040x}", 0xa1)), true).await.unwrap();
    assert!(!again.already_existed);
    let undone = engine.undo().await.unwrap();
    assert_eq!(undone.undo.torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [b], "the other is back");
    let refused: Vec<(u64, proto::CantUndo)> = undone.refused.iter().map(|r| (r.torrent.id, r.why.clone())).collect();
    assert_eq!(refused, [(a, proto::CantUndo::InList)], "the answer says which didn't, and why");
    let left = left(&engine);
    let Some(proto::Left { seq: last, why: proto::Leave::Undone { refused } }) = left.last() else { panic!("{left:?}") };
    assert_eq!(*last, seq);
    assert_eq!(refused, &undone.refused, "and so does the word every client reads");
    engine.stop().await;
}

/// An undo that finished is undone in the snapshot, and so after a restart.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_that_finished_is_undone_in_the_snapshot_and_after_a_restart() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    let seq = seqs(&engine)[0];
    engine.undo().await.unwrap();
    assert_eq!(left(&engine), [proto::Left { seq, why: proto::Leave::Undone { refused: Vec::new() } }]);
    engine.stop().await;
    let engine = self::engine(dir.path()).await;
    assert_eq!(left(&engine), [proto::Left { seq, why: proto::Leave::Undone { refused: Vec::new() } }], "kept in undo.json");
    engine.stop().await;
}

/// A stack this build can't read never keeps the engine from starting: what reads is kept, the
/// rest dropped, the file kept aside.
#[tokio::test(flavor = "multi_thread")]
async fn the_engine_starts_whatever_undo_json_holds() {
    for bad in ["{not json", r#"{"stack":[{"action":"move"}]}"#, r#"{"stack":7,"undoing":"x"}"#] {
        let dir = temp();
        std::fs::write(dir.path().join("data/undo.json"), bad).unwrap();
        let engine = engine(dir.path()).await;
        assert_eq!(top(&engine).await, None);
        assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))));
        assert_eq!(std::fs::read_to_string(dir.path().join("data/undo.invalid.json")).unwrap(), bad, "kept aside");
        engine.stop().await;
    }
}

/// The torrent's folder replaced by a link to a folder elsewhere after a Delete that went file by
/// file: nothing goes back through it, and the torrent stays off the list.
#[tokio::test(flavor = "multi_thread")]
async fn undo_never_puts_files_back_through_a_link() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // So each file goes on its own, and the folder stays.
    std::fs::write(file(dir.path(), "mine.txt"), b"mine").unwrap();
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    let pack = dir.path().join("downloads/pack");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::rename(&pack, dir.path().join("was-pack")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &pack).unwrap();
    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(&why, proto::CantUndo::NotAFolder { path } if path.ends_with("pack")), "{why:?}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "nothing behind the link");
    assert!(engine.listing().await.is_empty());
    engine.stop().await;
}

/// What was kept of the torrent can't be read: the undo is refused before anything moves, its
/// files still in the trash.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_that_could_not_add_the_torrent_again_moves_nothing() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    for kept in std::fs::read_dir(dir.path().join("data/undo")).unwrap().flatten() {
        std::fs::remove_file(kept.path().join("torrent")).unwrap();
        std::fs::write(kept.path().join("torrent"), b"not a torrent").unwrap();
    }
    let (_, why) = cant_undo(engine.undo().await);
    assert_eq!(why, proto::CantUndo::NotKept);
    assert!(!dir.path().join("downloads/pack").exists() && engine.listing().await.is_empty(), "nothing moved");
    engine.stop().await;
}

/// A file set Off and gone stays gone through an undo, as through a check or a restart
/// (#67, #158): the torrent comes back with its folder and priorities known, so librqbit makes only
/// the files it gets. An example: the model's sequences reach this one rarely.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_never_makes_again_a_file_set_off_and_gone() {
    for delete in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let b = index_of(&engine, id, "b.bin").await;
        engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
        std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
        engine.command(id, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove }).await.unwrap();
        engine.undo().await.unwrap();
        until(&engine, id, |s| *s != Status::Checking).await;
        assert!(!file(dir.path(), "b.bin").exists(), "b.bin, set Off and gone, isn't made again (delete: {delete})");
        assert!(file(dir.path(), "a.bin").exists() && file(dir.path(), "c.bin").exists());
        engine.stop().await;
    }
}

// Undo's odd cases (the audit of #159): each one pinned as the code does it.

/// The one file named `name` somewhere under `root`.
fn find_under(root: &Path, name: &str) -> PathBuf {
    let mut found = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == name) {
                found.push(path.clone());
            }
            if path.is_dir() {
                todo.push(path);
            }
        }
    }
    assert_eq!(found.len(), 1, "one {name} under {}: {found:?}", root.display());
    found.remove(0)
}

/// 1. Remove, then the same torrent added by hand, then Undo: the undo is refused (it's in the
/// list again) and dropped, and the torrent added by hand is left as it is. What was kept for the
/// undo goes.
#[tokio::test(flavor = "multi_thread")]
async fn undo_after_the_same_torrent_was_added_again_by_hand_is_refused_and_leaves_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let source = complete_torrent(dir.path()).await;
    let id = add(&engine, &downloads, source.clone(), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    let again = add(&engine, &downloads, source, false).await.unwrap();
    assert!(!again.already_existed);
    wait_for(&engine, again.id, Status::Seeding).await;

    let (undo, why) = cant_undo(engine.undo().await);
    assert_eq!((undo, why), (undoable(proto::UndoAction::Remove, id, "pack"), proto::CantUndo::InList));
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [again.id], "the one added by hand, alone");
    assert_eq!(top(&engine).await, None, "dropped");
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 0, "nothing kept for it any more");
    engine.stop().await;
}

/// 2. Remove, Undo, Remove again, Undo twice: the second Remove is a new entry; the first Undo
/// brings it back under the same id, the second has nothing to undo.
#[tokio::test(flavor = "multi_thread")]
async fn remove_undo_remove_again_then_two_undos_bring_it_back_once() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    engine.undo().await.unwrap();
    until(&engine, id, |s| *s != Status::Checking).await;
    assert_eq!(top(&engine).await, None);

    engine.command(id, TorrentCommand::Remove).await.unwrap();
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Remove, id, "pack")), "one entry, not two");
    assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(proto::UndoAction::Remove, id, "pack"));
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "the second Undo has nothing");
    assert_eq!(engine.listing().await.iter().map(|t| t.id).collect::<Vec<_>>(), [id], "back once, under its id");
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// 3. Delete, the Trash emptied (its item deleted, the folder kept), then Undo: refused, naming the
/// item, nothing changed, the entry dropped. The second Undo has nothing.
#[tokio::test(flavor = "multi_thread")]
async fn undo_after_the_trashed_item_was_deleted_is_refused_dropped_and_changes_nothing() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    for item in std::fs::read_dir(dir.path().join("trash")).unwrap().flatten() {
        match item.path().is_dir() {
            true => std::fs::remove_dir_all(item.path()).unwrap(),
            false => std::fs::remove_file(item.path()).unwrap(),
        }
    }
    let (undo, why) = cant_undo(engine.undo().await);
    assert_eq!(undo, undoable(proto::UndoAction::Delete, id, "pack"));
    assert!(matches!(why, proto::CantUndo::NotInTrash { path } if path.ends_with("pack")), "names the folder");
    assert!(engine.listing().await.is_empty() && !dir.path().join("downloads/pack").exists(), "nothing changed");
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "dropped");
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 0, "what was kept goes with it");
    engine.stop().await;
}

/// 4. (#162) Delete, then one file taken out of the trashed folder, then Undo. Recorded as it is:
/// the folder is still there and is the same one, so the undo goes ahead; the torrent comes back
/// Downloading, with that file made again empty and to be downloaded, and no word of it.
#[tokio::test(flavor = "multi_thread")]
async fn undo_after_some_files_left_the_trash_goes_ahead_with_the_folder_that_is_there() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    std::fs::remove_file(find_under(&dir.path().join("trash"), "b.bin")).unwrap();

    engine.undo().await.expect("the undo isn't refused: the folder is there");
    assert!(file(dir.path(), "a.bin").exists() && file(dir.path(), "c.bin").exists());
    until(&engine, id, |s| *s != Status::Checking).await;
    let status = find(&engine, id).await.status;
    assert_eq!(status, Status::Downloading, "not Seeding: it wants b.bin again");
    assert_eq!(
        files_on(&engine, id).await,
        [("a.bin".to_owned(), FULL, false), ("b.bin".to_owned(), 0, false), ("c.bin".to_owned(), FULL, false)],
        "the others whole, b.bin made again empty"
    );
    assert!(file(dir.path(), "b.bin").exists(), "an empty b.bin is on disk again");
    engine.stop().await;
}

/// 8. The destination's drive unplugged while the torrent is removed or deleted: Undo is refused,
/// naming it, before anything moves, and dropped; the drive back later changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn undo_while_the_destination_is_away_is_refused_and_dropped() {
    for delete in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, if delete { TorrentCommand::Delete } else { TorrentCommand::Remove }).await.unwrap();
        take_away(&downloads);

        let (_, why) = cant_undo(engine.undo().await);
        let reason = Unavailable::FolderMissing { path: AbsolutePath::new(&downloads).unwrap() };
        assert_eq!(why, proto::CantUndo::Destination { reason }, "delete: {delete}");
        assert!(!downloads.exists(), "never made again");
        bring_back(&downloads);
        assert!(engine.listing().await.is_empty());
        assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "dropped, though the drive is back");
        if delete {
            assert!(find_under(&dir.path().join("trash"), "a.bin").exists(), "its files wait in the Trash");
        }
        engine.stop().await;
    }
}

/// 9. A torrent with only part downloaded, deleted and undone: its progress comes back as it was,
/// file by file, with no check (the pieces kept hold while the files are as they were).
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_partly_downloaded_torrent_brings_its_progress_back() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = complete_torrent(dir.path()).await;
    // a.bin whole, b.bin its first piece, c.bin not there: no one seeds the rest.
    std::fs::OpenOptions::new().write(true).open(file(dir.path(), "b.bin")).unwrap().set_len(PIECE as u64).unwrap();
    std::fs::remove_file(file(dir.path(), "c.bin")).unwrap();
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Downloading).await;
    let before = files_on(&engine, id).await;
    let progress = find(&engine, id).await.content.unwrap().progress_bytes;
    assert_eq!(progress, FULL + PIECE as u64, "a whole and a piece of b");

    engine.command(id, TorrentCommand::Delete).await.unwrap();
    engine.undo().await.unwrap();
    until(&engine, id, |s| *s != Status::Checking).await;
    let after = find(&engine, id).await;
    assert_eq!(after.status, Status::Downloading, "running again, as it was");
    assert_eq!(after.content.unwrap().progress_bytes, progress);
    assert_eq!(files_on(&engine, id).await, before);
    engine.stop().await;
}

/// 10. Remove while the torrent is still Checking, then Undo: it comes back under its id, checked in
/// full (no pieces were saved yet, so nothing is kept of them), and goes on to download.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_remove_during_a_check_brings_it_back_checked_again() {
    let dir = temp();
    let (engine, gate, _opens) = checking_engine(dir.path()).await;
    let id = checking_torrent(&engine, dir.path()).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    assert_eq!(top(&engine).await, Some(undoable(proto::UndoAction::Remove, id, "pack")));
    gate.hold(false);
    engine.undo().await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!(find(&engine, id).await.id, id);
    engine.stop().await;
}

/// 10. Remove or Delete while the torrent is Unchecked (its check cancelled, #88), then Undo: it
/// comes back Unchecked, under its id, showing nothing checked and stopped in librqbit, not Ready
/// with progress nobody checked; and so across a restart. A check then checks it.
#[tokio::test(flavor = "multi_thread")]
async fn undo_of_a_remove_or_delete_while_unchecked_brings_it_back_unchecked() {
    for command in [TorrentCommand::Remove, TorrentCommand::Delete] {
        let dir = temp();
        let id = {
            let (engine, gate, _opens) = checking_engine(dir.path()).await;
            let id = checking_torrent(&engine, dir.path()).await;
            engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
            gate.hold(false);
            assert_eq!(find(&engine, id).await.status, Status::Unchecked);
            engine.command(id, command.clone()).await.unwrap();
            engine.undo().await.unwrap();
            let t = find(&engine, id).await;
            assert_eq!((t.status, t.content.unwrap().progress_bytes), (Status::Unchecked, 0), "{command:?}, undone");
            settled(&engine).await;
            assert_eq!(find(&engine, id).await.status, Status::Unchecked, "{command:?}: and it stays so");
            assert!(!librqbit_live(&engine, id).await);
            engine.stop().await;
            id
        };
        let engine = engine(dir.path()).await;
        assert_eq!(find(&engine, id).await.status, Status::Unchecked, "{command:?}: still Unchecked after a restart");
        engine.command(id, TorrentCommand::Check).await.unwrap();
        assert_eq!(find(&engine, id).await.status, Status::Checking, "{command:?}: a check checks it");
        engine.stop().await;
    }
}

/// 11. More than 20 Removes: the stack keeps the last 20 (and their stashes), the 21st Undo has
/// nothing, and the oldest torrent stays out.
#[tokio::test(flavor = "multi_thread")]
async fn twenty_one_removes_leave_twenty_undos() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let downloads = dir.path().join("downloads");
    let mut ids = Vec::new();
    for n in 0..21 {
        let id = add(&engine, &downloads, torrent_file(&format!("t{n}.bin")), true).await.unwrap().id;
        ids.push(id);
        engine.command(id, TorrentCommand::Remove).await.unwrap();
    }
    assert_eq!(engine.subscribe().borrow().snapshot.undo.len(), 20);
    assert_eq!(std::fs::read_dir(dir.path().join("data/undo")).unwrap().count(), 20, "the oldest one's stash went");
    for n in (1..21).rev() {
        let undone = engine.undo().await.unwrap_or_else(|e| panic!("undo {n}: {e:?}"));
        assert_eq!(undone.id(), ids[n], "newest first");
    }
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "the 21st");
    assert_eq!(engine.listing().await.len(), 20);
    assert!(engine.listing().await.iter().all(|t| t.id != ids[0]), "the first one stays out");
    engine.stop().await;
}

/// 5. (#190) A second Undo while the first is still putting back (held at the trash's gate) takes
/// the next step down at once, and the two put-backs run side by side, each torrent listed again at
/// once; an Undo with nothing left on the stack is nothing to undo at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_undo_while_the_first_is_held_takes_the_next_step_at_once() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let downloads = dir.path().join("downloads");
    let first = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, first, Status::Seeding).await;
    engine.command(first, TorrentCommand::Delete).await.unwrap();
    let second = add(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap().id;
    std::fs::write(downloads.join("other.bin"), vec![0u8; PIECE]).unwrap();
    engine.command(second, TorrentCommand::Delete).await.unwrap();
    assert_eq!(engine.subscribe().borrow().snapshot.undo.len(), 2);

    super::trash::hold(&gate, true);
    let newest = tokio::spawn(engine.undo_later());
    held_at_the_gate(&gate).await;
    let older = tokio::spawn(engine.undo_later());
    for _ in 0..500 {
        if super::trash::waiters(&gate) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(super::trash::waiters(&gate), 2, "both put-backs are under way side by side");
    let listed: BTreeSet<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, [first, second].into(), "both are listed again at once");
    assert!(matches!(engine.undo().await, Err(Failure::Refused(proto::Refusal::NothingToUndo))), "past the stack: nothing");

    super::trash::hold(&gate, false);
    let answered = |task: tokio::task::JoinHandle<Result<proto::Undone, Failure>>| async move {
        tokio::time::timeout(Duration::from_secs(10), task).await.expect("answered").unwrap().unwrap()
    };
    assert_eq!(unnumbered(answered(newest).await), undoable(proto::UndoAction::Delete, second, "other.bin"), "newest first");
    assert_eq!(unnumbered(answered(older).await), undoable(proto::UndoAction::Delete, first, "pack"), "then the older");
    wait_for(&engine, first, Status::Seeding).await;
    assert_eq!(engine.listing().await.len(), 2, "both back");
    assert_eq!(top(&engine).await, None);
    assert!(downloads.join("other.bin").exists() && downloads.join("pack/a.bin").exists());
    engine.stop().await;
}

/// (#190) A slow put-back on one torrent never delays another's: with one torrent's put-back held,
/// the other's Undo, asked after it, is carried out and answered meanwhile.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_put_back_of_one_torrent_never_delays_anothers() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let downloads = dir.path().join("downloads");
    let fast = add(&engine, &downloads, torrent_file("fast.bin"), true).await.unwrap().id;
    let slow = add(&engine, &downloads, torrent_file("slow.bin"), true).await.unwrap().id;
    for (id, name) in [(fast, "fast.bin"), (slow, "slow.bin")] {
        std::fs::write(downloads.join(name), vec![0u8; PIECE]).unwrap();
        engine.command(id, TorrentCommand::Delete).await.unwrap();
    }
    super::trash::hold_under(&gate, &downloads.join("slow.bin"));
    let slow_undo = tokio::spawn(engine.undo_later());
    held_at_the_gate(&gate).await;
    let undone = tokio::time::timeout(Duration::from_secs(10), engine.undo()).await.expect("not delayed by the slow one").unwrap();
    assert_eq!(unnumbered(undone), undoable(proto::UndoAction::Delete, fast, "fast.bin"));
    assert!(downloads.join("fast.bin").exists() && !slow_undo.is_finished());
    super::trash::hold(&gate, false);
    slow_undo.await.unwrap().unwrap();
    assert!(downloads.join("slow.bin").exists());
    engine.stop().await;
}

/// 5. A stop while two undos put back side by side, one held: the stop waits for both, each is
/// answered as back, and both are listed after a restart, with nothing left on the stack.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_finishes_every_undo_under_way() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone());
    let engine = start().await;
    let downloads = dir.path().join("downloads");
    let first = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, first, Status::Seeding).await;
    engine.command(first, TorrentCommand::Delete).await.unwrap();
    let second = add(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap().id;
    std::fs::write(downloads.join("other.bin"), vec![0u8; PIECE]).unwrap();
    engine.command(second, TorrentCommand::Delete).await.unwrap();

    super::trash::hold(&gate, true);
    let newest = tokio::spawn(engine.undo_later());
    held_at_the_gate(&gate).await;
    let older = tokio::spawn(engine.undo_sent().await);
    engine.inspect(|_| ()).await;
    let stopping = engine.stop();
    tokio::pin!(stopping);
    assert!(tokio::time::timeout(Duration::from_millis(300), &mut stopping).await.is_err(), "the stop waits for the undos under way");
    super::trash::hold(&gate, false);
    stopping.await;
    assert_eq!(newest.await.unwrap().unwrap().id(), second);
    assert_eq!(older.await.unwrap().unwrap().id(), first);

    let engine = start().await;
    let listed: BTreeSet<u64> = engine.listing().await.iter().map(|t| t.id).collect();
    assert_eq!(listed, [first, second].into());
    assert_eq!(top(&engine).await, None);
    engine.stop().await;
}

/// 12. The service stopped while a Delete is under way (held after its first item), started again,
/// then Undo: every file is back, the torrent too.
#[tokio::test(flavor = "multi_thread")]
async fn undo_after_the_service_stopped_in_the_middle_of_a_delete() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    struct Opens(super::trash::Gate);
    impl Drop for Opens {
        fn drop(&mut self) {
            super::trash::hold(&self.0, false);
        }
    }
    let _opens = Opens(gate.clone());
    let start = || engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), gate.clone());
    let engine = start().await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    // So each file goes on its own.
    std::fs::write(file(dir.path(), "mine.txt"), b"mine").unwrap();
    let before = pack_bytes(dir.path());
    super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(id, TorrentCommand::Delete));
    for _ in 0..500 {
        if super::trash::waiting(&gate) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(super::trash::waiting(&gate), "one file is in the Trash, the Delete waits");
    let stopping = engine.stop();
    tokio::pin!(stopping);
    assert!(tokio::time::timeout(Duration::from_millis(300), &mut stopping).await.is_err(), "the stop waits for the trashing");
    super::trash::hold(&gate, false);
    stopping.await;
    let _ = deleting.await.unwrap();

    let engine = start().await;
    assert_eq!(top(&engine).await.map(|u| (u.action, u.id())), Some((proto::UndoAction::Delete, id)));
    engine.undo().await.unwrap();
    assert_eq!(pack_bytes(dir.path()), before, "every file back, mine.txt where it was");
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// An engine whose changes to files (writes, lengths set, removals) wait at `gate` while it's
/// held, with a trash of its own.
async fn engine_touches_gated(dir: &Path, gate: &Arc<super::files::Gate>) -> Engine {
    let config = Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    let guard = super::files::Guard::default().with_gate(super::files::At::Change, gate.clone());
    Engine::start_with(config, add_into, guard, super::Looks::real()).await.unwrap()
}

/// A Delete as librqbit sets a file's length at the end of its check, held there as on a stalled
/// drive (#186): the Delete waits for it, off the actor, which answers everything else meanwhile;
/// the file then goes to the Trash as it is, and Undo brings it back.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_waits_off_the_actor_for_a_change_to_its_files_under_way() {
    let dir = temp();
    let touches = Arc::new(super::files::Gate::default());
    let _opens = Opens(touches.clone());
    let engine = engine_touches_gated(dir.path(), &touches).await;
    let downloads = dir.path().join("downloads");
    let other = add(&engine, &downloads, complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, other, Status::Seeding).await;

    touches.hold(true);
    std::fs::write(downloads.join("other.bin"), vec![0u8; PIECE]).unwrap();
    let id = add(&engine, &downloads, torrent_file("other.bin"), true).await.unwrap().id;
    for _ in 0..500 {
        if touches.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(touches.waiting(), "its check sets the file's length: held");
    let soon = Duration::from_secs(2);
    // What it wants saved, which touches it again: it was already.
    let resumed = tokio::time::timeout(soon, engine.command(id, TorrentCommand::Resume)).await;
    assert!(matches!(resumed, Ok(Ok(()))), "its own Resume is answered: {resumed:?}");
    let deleting = tokio::spawn(engine.sent(id, TorrentCommand::Delete).await);
    assert!(tokio::time::timeout(soon, engine.inspect(|_| ())).await.is_ok(), "the actor goes on past the Delete");

    assert!(tokio::time::timeout(soon, engine.listing()).await.is_ok(), "the list is answered");
    let paused = tokio::time::timeout(soon, engine.command(other, TorrentCommand::Pause)).await;
    assert!(matches!(paused, Ok(Ok(()))), "a Pause is answered: {paused:?}");
    let priority = TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off };
    let set = tokio::time::timeout(soon, engine.command(other, priority)).await;
    assert!(matches!(set, Ok(Ok(()))), "a priority is answered: {set:?}");
    assert!(!deleting.is_finished(), "the Delete waits for the change under way");

    touches.hold(false);
    tokio::time::timeout(Duration::from_secs(10), deleting).await.expect("answered").unwrap().unwrap();
    assert_eq!(top(&engine).await.map(|u| u.id()), Some(id));
    let undone = engine.undo().await;
    assert!(undone.is_ok(), "the file in the Trash is the one that went: {undone:?}");
    assert_eq!(std::fs::read(downloads.join("other.bin")).unwrap(), vec![0u8; PIECE]);
    engine.stop().await;
}

/// What the user has of the torrents, to tell whether undos brought back the start (#190): each
/// listed torrent's id, status and file priorities, the bytes under the downloads folder, and what
/// the Trash holds.
#[derive(Debug, PartialEq)]
struct Seen {
    torrents: Vec<(u64, Status, Vec<Priority>)>,
    files: Vec<(PathBuf, Vec<u8>)>,
    in_trash: usize,
}

async fn seen(engine: &Engine, dir: &Path) -> Seen {
    let mut torrents = Vec::new();
    for t in engine.listing().await {
        let priorities = engine.details(t.id).await.map(|d| d.files.iter().map(|f| f.priority).collect()).unwrap_or_default();
        torrents.push((t.id, t.status, priorities));
    }
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(root, &path, out),
                Ok(t) if t.is_file() && e.file_name() != ".DS_Store" => {
                    out.push((path.strip_prefix(root).unwrap().to_owned(), std::fs::read(&path).unwrap()))
                }
                _ => {}
            }
        }
    }
    let mut files = Vec::new();
    walk(&dir.join("downloads"), &dir.join("downloads"), &mut files);
    files.sort();
    Seen { torrents, files, in_trash: std::fs::read_dir(dir.join("trash")).map_or(0, |d| d.count()) }
}

/// Waits up to 10 seconds for what the user has to be `want` (nothing Checking meanwhile).
async fn settles_at(engine: &Engine, dir: &Path, want: &Seen) {
    for _ in 0..1000 {
        if seen(engine, dir).await == *want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(seen(engine, dir).await, *want);
}

/// What the user has once nothing is checked: the start a case's undos must bring back.
async fn at_rest(engine: &Engine, dir: &Path) -> Seen {
    for _ in 0..1000 {
        let now = seen(engine, dir).await;
        if now.torrents.iter().all(|(_, status, _)| *status != Status::Checking) {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("still checking: {:?}", seen(engine, dir).await);
}

/// The torrents listed, by id, once the actor has handled what was sent before.
async fn listed_now(engine: &Engine) -> Vec<u64> {
    engine.inspect(|_| ()).await;
    engine.listing().await.iter().map(|t| t.id).collect()
}

/// A pack of three files, c.bin Off, and a torrent of one file, in a trash whose gate the test holds.
async fn two_torrents(dir: &Path) -> (Engine, super::trash::Gate, u64, u64) {
    let (engine, gate, a) = gated_pack(dir).await;
    engine.command(a, TorrentCommand::SetPriority { files: vec![2], priority: Priority::Off }).await.unwrap();
    let b = add_made(&engine, &dir.join("downloads"), torrent_file("other.bin"), true).await.unwrap().id;
    (engine, gate, a, b)
}

/// The user's sequence (#190): Delete A, Delete B, ⌘Z, ⌘Z brings back B, then A, each as it was. At
/// its own pace, each answered before the next; and pressed quickly while both Deletes still run
/// (held at the trash's gate): the list changes at once with each press, both Deletes are answered
/// as undone, and the Undos as B back, then A.
#[tokio::test(flavor = "multi_thread")]
async fn delete_a_delete_b_undo_undo_brings_back_b_then_a() {
    use proto::UndoAction::Delete;
    for quick in [false, true] {
        let dir = temp();
        let (engine, gate, a, b) = two_torrents(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        let start = at_rest(&engine, dir.path()).await;
        if !quick {
            engine.command(a, TorrentCommand::Delete).await.unwrap();
            engine.command(b, TorrentCommand::Delete).await.unwrap();
            assert_eq!(listed_now(&engine).await, Vec::<u64>::new());
            assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(Delete, b, "other.bin"), "B first");
            assert_eq!(listed_now(&engine).await, [b]);
            assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(Delete, a, "pack"), "then A");
            settles_at(&engine, dir.path(), &start).await;
            engine.stop().await;
            continue;
        }
        super::trash::hold(&gate, true);
        let delete_a = engine.sent(a, TorrentCommand::Delete).await;
        assert_eq!(listed_now(&engine).await, [b], "quick: A leaves the list at once");
        let delete_b = engine.sent(b, TorrentCommand::Delete).await;
        assert_eq!(listed_now(&engine).await, Vec::<u64>::new(), "quick: and B");
        assert_eq!(stack(&engine).await, [Delete, Delete], "quick: both on the stack, running");
        let undo_b = engine.undo_sent().await;
        assert_eq!(listed_now(&engine).await, [b], "quick: B is back at once");
        let undo_a = engine.undo_sent().await;
        assert_eq!(listed_now(&engine).await, [a, b], "quick: and A, in its place");
        assert_eq!(stack(&engine).await, [], "quick: each ⌘Z took the next step down");
        super::trash::hold(&gate, false);
        for deleted in [delete_a.await, delete_b.await] {
            assert!(matches!(deleted, Err(Failure::Refused(proto::Refusal::Undone))), "quick: {deleted:?}");
        }
        assert_eq!(unnumbered(undo_b.await.unwrap()), undoable(Delete, b, "other.bin"));
        assert_eq!(unnumbered(undo_a.await.unwrap()), undoable(Delete, a, "pack"));
        settles_at(&engine, dir.path(), &start).await;
        engine.stop().await;
    }
}

/// The user's sequence (#190): trash file 1 of A, trash file 2 of A, Delete A, ⌘Z ×3 brings back A,
/// then file 2, then file 1, each file with the priority it had (c.bin, never asked about, stays
/// Off). At its own pace; with the ⌘Zs pressed while the Delete runs (each undo waits on A's lane
/// for the one before it: file 2 goes back into A's folder once the Delete's undo brought it back);
/// and with everything pressed while the first Move to Trash runs: the Delete and the second move,
/// not started, are dropped, and the first is aborted.
#[tokio::test(flavor = "multi_thread")]
async fn trash_two_files_then_delete_then_three_undos_bring_back_a_then_file_2_then_file_1() {
    use proto::UndoAction::{Delete, TrashFiles};
    for pace in ["own pace", "during the Delete", "during the first move"] {
        let dir = temp();
        let (engine, gate, a, b) = two_torrents(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        let pack = dir.path().join("downloads/pack");
        let start = at_rest(&engine, dir.path()).await;
        let trash = |file| TorrentCommand::TrashFiles { files: vec![file] };
        let priorities = || async {
            engine.inspect(|_| ()).await;
            engine.details(a).await.map(|d| d.files.iter().map(|f| f.priority).collect::<Vec<_>>()).unwrap_or_default()
        };
        match pace {
            "own pace" => {
                engine.command(a, trash(0)).await.unwrap();
                engine.command(a, trash(1)).await.unwrap();
                engine.command(a, TorrentCommand::Delete).await.unwrap();
                assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(Delete, a, "pack"), "A first");
                assert!(!pack.join("a.bin").exists() && !pack.join("b.bin").exists());
                assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"), "then file 2");
                assert!(!pack.join("a.bin").exists() && pack.join("b.bin").exists());
                assert_eq!(priorities().await, [Priority::Off, Priority::Normal, Priority::Off]);
                assert_eq!(unnumbered(engine.undo().await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"), "then file 1");
            }
            "during the Delete" => {
                engine.command(a, trash(0)).await.unwrap();
                engine.command(a, trash(1)).await.unwrap();
                super::trash::hold(&gate, true);
                let delete = engine.sent(a, TorrentCommand::Delete).await;
                held_at_the_gate(&gate).await;
                let undos = [engine.undo_sent().await, engine.undo_sent().await, engine.undo_sent().await];
                assert_eq!(listed_now(&engine).await, [a, b], "{pace}: A is back at once");
                assert_eq!(stack(&engine).await, [], "{pace}");
                super::trash::hold(&gate, false);
                assert!(matches!(delete.await, Err(Failure::Refused(proto::Refusal::Undone))));
                let [undo_delete, undo_2, undo_1] = undos;
                assert_eq!(unnumbered(undo_delete.await.unwrap()), undoable(Delete, a, "pack"));
                assert_eq!(unnumbered(undo_2.await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"));
                assert_eq!(unnumbered(undo_1.await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"));
            }
            _ => {
                super::trash::hold(&gate, true);
                let first = engine.sent(a, trash(0)).await;
                held_at_the_gate(&gate).await;
                let second = engine.sent(a, trash(1)).await;
                assert_eq!(priorities().await, [Priority::Off, Priority::Off, Priority::Off], "{pace}: both Off at once");
                let delete = engine.sent(a, TorrentCommand::Delete).await;
                assert_eq!(listed_now(&engine).await, [b], "{pace}: A leaves the list at once");
                assert_eq!(stack(&engine).await, [TrashFiles { count: 1 }, TrashFiles { count: 1 }, Delete], "{pace}");
                let undo_delete = engine.undo_sent().await;
                assert_eq!(listed_now(&engine).await, [a, b], "{pace}: A is back at once");
                let undo_2 = engine.undo_sent().await;
                assert_eq!(priorities().await, [Priority::Off, Priority::Normal, Priority::Off], "{pace}: file 2 at once");
                let undo_1 = engine.undo_sent().await;
                assert_eq!(priorities().await, [Priority::Normal, Priority::Normal, Priority::Off], "{pace}: file 1 at once");
                // Dropped before they started: answered at once.
                assert!(matches!(delete.await, Err(Failure::Refused(proto::Refusal::Undone))));
                assert!(matches!(second.await, Err(Failure::Refused(proto::Refusal::Undone))));
                assert_eq!(unnumbered(undo_delete.await.unwrap()), undoable(Delete, a, "pack"));
                assert_eq!(unnumbered(undo_2.await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"));
                super::trash::hold(&gate, false);
                assert!(matches!(first.await, Err(Failure::Refused(proto::Refusal::Undone))));
                assert_eq!(unnumbered(undo_1.await.unwrap()), undoable(TrashFiles { count: 1 }, a, "pack"));
            }
        }
        assert_eq!(stack(&engine).await, [], "{pace}");
        settles_at(&engine, dir.path(), &start).await;
        engine.stop().await;
    }
}

/// A trash that counts the items it takes, over a test trash gated by `gate`.
struct Counting(super::trash::Trash, Arc<std::sync::atomic::AtomicUsize>);

impl super::trash::Bin for Counting {
    fn take(&self, path: &Path) -> Result<Option<PathBuf>, proto::KeptWhy> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.0.take(path)
    }
    fn give_back(&self, from: &Path, to: &Path) -> Result<(), String> {
        self.0.give_back(from, to)
    }
    fn return_to(&self, path: &Path, to: &Path) -> Result<(), String> {
        self.0.return_to(path, to)
    }
    fn after_each(&self, item: &Path) {
        self.0.after_each(item)
    }
}

/// An engine whose trash counts what it takes, gated by `gate`: the count.
async fn engine_counting(dir: &Path, gate: &super::trash::Gate) -> (Engine, Arc<std::sync::atomic::AtomicUsize>) {
    let takes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let trash: super::trash::Trash =
        Arc::new(Counting(super::trash::into(dir.join("trash"), Default::default(), gate.clone()), takes.clone()));
    let engine = Engine::start(Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: Duration::from_millis(20),
        poll_every: super::POLL_EVERY,
        trash,
        speed_limits: Default::default(),
        seeding: Default::default(),
    })
    .await
    .unwrap();
    (engine, takes)
}

/// ⌘Z on a Delete or a Move to Trash under way stops it after the item in flight (#190): with the
/// trash held after its first item, nothing after that one is ever taken, and what went comes back.
#[tokio::test(flavor = "multi_thread")]
async fn cmd_z_stops_a_running_delete_or_move_to_trash_after_the_item_in_flight() {
    for delete in [true, false] {
        let dir = temp();
        let gate = super::trash::Gate::default();
        let _opens = OpensTrash(gate.clone());
        let (engine, takes) = engine_counting(dir.path(), &gate).await;
        let pack = dir.path().join("downloads/pack");
        let id = add_made(&engine, &dir.path().join("downloads"), multi_file_torrent(&["a.bin", "b.bin", "c.bin"]), true).await.unwrap().id;
        // A file of the user's beside them: a Delete takes them one by one.
        std::fs::write(pack.join("mine.txt"), b"mine").unwrap();
        let start = at_rest(&engine, dir.path()).await;
        super::trash::hold(&gate, true);
        let command = if delete { TorrentCommand::Delete } else { TorrentCommand::TrashFiles { files: vec![0, 1, 2] } };
        let step = engine.sent(id, command).await;
        held_at_the_gate(&gate).await;
        let undo = engine.undo_sent().await;
        engine.inspect(|_| ()).await;
        super::trash::hold(&gate, false);
        assert!(matches!(step.await, Err(Failure::Refused(proto::Refusal::Undone))), "delete: {delete}");
        undo.await.unwrap();
        assert_eq!(takes.load(std::sync::atomic::Ordering::SeqCst), 1, "delete: {delete}: only the item in flight was taken");
        settles_at(&engine, dir.path(), &start).await;
        engine.stop().await;
    }
}

/// B2 of #217's review: ⌘Z on a Delete under way lists its torrent again at once, and what's asked
/// of it meanwhile is judged as clients see it, back as it was: a Resume shows at once, waits its turn
/// behind the rollback, and is answered once it has run; a Delete again is taken too.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_after_undo_of_a_running_delete_waits_for_the_rollback() {
    for again in [false, true] {
        let dir = temp();
        let (engine, gate, id) = gated_pack(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        wait_for(&engine, id, Status::Paused).await;
        super::trash::hold(&gate, true);
        let delete = engine.sent(id, TorrentCommand::Delete).await;
        held_at_the_gate(&gate).await;
        let undo = engine.undo_sent().await;
        assert_eq!(listed_now(&engine).await, [id], "back at once");
        assert_eq!(find(&engine, id).await.status, Status::Paused, "as it was");
        let resume = engine.sent(id, TorrentCommand::Resume).await;
        engine.inspect(|_| ()).await;
        let shown = find(&engine, id).await;
        assert_ne!(shown.status, Status::Paused, "the Resume shows at once");
        assert!(matches!(shown.actions.get(Action::Resume), proto::Answer::DoesNothing { .. }), "and its actions follow");
        let second = match again {
            true => Some(engine.sent(id, TorrentCommand::Delete).await),
            false => None,
        };
        super::trash::hold(&gate, false);
        assert!(matches!(delete.await, Err(Failure::Refused(proto::Refusal::Undone))));
        undo.await.unwrap();
        resume.await.expect("answered once the rollback ran, not refused as being deleted");
        match second {
            Some(second) => {
                second.await.expect("the Delete again is carried out after them");
                assert_eq!(listed_now(&engine).await, Vec::<u64>::new());
            }
            None => until(&engine, id, |s| *s != Status::Paused).await,
        }
        engine.stop().await;
    }
}

/// S3 of #217's review: a command that waits behind a Move to Trash under way shows done at once (a
/// Resume: no longer Paused), and runs when its turn comes. One that then fails (a priority for a
/// file there isn't) is answered with why, and the list never showed anything of it.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_waiting_behind_a_move_to_trash_shows_done_at_once() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    wait_for(&engine, id, Status::Paused).await;
    super::trash::hold(&gate, true);
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&gate).await;
    let size = find(&engine, id).await.content.unwrap().size_bytes;
    let resume = engine.sent(id, TorrentCommand::Resume).await;
    let off = engine.sent(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await;
    engine.inspect(|_| ()).await;
    let shown = find(&engine, id).await;
    assert_ne!(shown.status, Status::Paused, "the Resume shows at once");
    assert!(shown.content.unwrap().size_bytes < size, "so does the priority, in its Size");
    let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Off, Priority::Normal], "and its file's priority");
    // One for a file it doesn't have is known now: refused at once, never on the stack.
    let missing = tokio::time::timeout(Duration::from_secs(5), engine.command(id, TorrentCommand::TrashFiles { files: vec![9] })).await;
    assert_eq!(refusal(missing.expect("answered at once")), (id, TorrentRefusal::NoFile { index: 9 }));
    assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }]);
    super::trash::hold(&gate, false);
    moving.await.unwrap();
    resume.await.unwrap();
    off.await.unwrap();
    engine.stop().await;
}

/// S2 of #217's review: an engine restart for new settings (the listening port, UPnP) runs what
/// waits on a lane before it ends, as the service carries on: a Delete and a Pause behind a Move to
/// Trash are carried out, not refused. A shutdown refuses them
/// (`a_stop_ends_a_move_to_trash_but_runs_nothing_that_waited_for_it`).
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_for_new_settings_runs_what_waits_on_a_lane() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let other = add_made(&engine, &dir.path().join("downloads"), torrent_file("other.bin"), false).await.unwrap().id;
    let engine = Arc::new(engine);
    super::trash::hold(&gate, true);
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&gate).await;
    let delete = engine.sent(id, TorrentCommand::Delete).await;
    let pause = engine.sent(other, TorrentCommand::Pause).await;
    engine.inspect(|_| ()).await;
    let restarting = tokio::spawn({
        let engine = engine.clone();
        async move { engine.stop_to_restart().await }
    });
    stop_begun(&engine).await;
    super::trash::hold(&gate, false);
    restarting.await.unwrap();
    moving.await.unwrap();
    delete.await.expect("run, not refused as stopping");
    pause.await.expect("answered");
    drop(engine);
    assert!(!dir.path().join("downloads/pack").exists(), "the Delete ran");
}

/// S4 of #217's review: a Move to Trash cut short by a crash after its first file ends at the next
/// start with what it moved, written ahead, which ⌘Z puts back; one an Undo had aborted is rolled
/// back at start. Either way every file and priority is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_cut_short_by_a_crash_ends_with_what_it_moved_or_is_rolled_back() {
    for aborted in [false, true] {
        let dir = temp();
        let pack = dir.path().join("downloads/pack");
        let id = {
            let (engine, gate, id) = gated_pack(dir.path()).await;
            super::trash::hold(&gate, true);
            let trashing = tokio::spawn(engine.command_later(id, TorrentCommand::TrashFiles { files: vec![0, 1, 2] }));
            held_at_the_gate(&gate).await;
            let undo = match aborted {
                true => Some(tokio::spawn(engine.undo_sent().await)),
                false => None,
            };
            engine.inspect(|_| ()).await;
            engine.kill().await;
            super::trash::abandon(&gate);
            let _ = trashing.await;
            if let Some(u) = undo {
                let _ = u.await;
            }
            id
        };
        let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
        for _ in 0..300 {
            if engine.inspect(|actor| actor.steps.machine().steps.is_empty()).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !aborted {
            assert_eq!(stack(&engine).await.len(), 1, "what it moved is on the stack");
            engine.undo().await.unwrap();
        }
        assert!(["a.bin", "b.bin", "c.bin"].iter().all(|n| pack.join(n).exists()));
        let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert_eq!(priorities, [Priority::Normal; 3]);
        assert_eq!(stack(&engine).await, []);
        engine.stop().await;
    }
}

/// A Move to Trash of two torrents in one request (one step, a part each: #235), cut short by a crash
/// with both parts under way, ends at the next start with what each moved: one entry, which one ⌘Z
/// puts back whole, every file and priority.
#[tokio::test(flavor = "multi_thread")]
async fn a_selections_move_to_trash_cut_short_by_a_crash_ends_each_part_with_what_it_moved() {
    let dir = temp();
    let (a_pack, b_pack) = (dir.path().join("downloads/pack"), dir.path().join("downloads2/pack"));
    let (a, b) = {
        let (engine, gate, a) = gated_pack(dir.path()).await;
        std::fs::create_dir_all(dir.path().join("downloads2")).unwrap();
        let b = add_made(&engine, &dir.path().join("downloads2"), multi_file_torrent(&["d.bin", "e.bin"]), true).await.unwrap().id;
        super::trash::hold(&gate, true);
        let trashing = tokio::spawn(engine.sent_all(vec![a, b], TorrentCommand::TrashFiles { files: vec![0, 1] }).await);
        // Both parts at the gate, each with its first file moved.
        for _ in 0..300 {
            let moved = |pack: &Path, names: [&str; 2]| names.iter().any(|n| !pack.join(n).exists());
            if moved(&a_pack, ["a.bin", "b.bin"]) && moved(&b_pack, ["d.bin", "e.bin"]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        held_at_the_gate(&gate).await;
        engine.kill().await;
        super::trash::abandon(&gate);
        let _ = trashing.await;
        (a, b)
    };
    let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, Default::default(), Default::default()).await;
    for _ in 0..300 {
        if engine.inspect(|actor| actor.steps.machine().steps.is_empty()).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let stack = engine.subscribe().borrow().snapshot.undo.clone();
    assert_eq!(stack.len(), 1, "one press, one entry: {stack:?}");
    assert_eq!(stack[0].torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [a, b], "holding both");
    engine.undo().await.unwrap();
    assert!(["a.bin", "b.bin", "c.bin"].iter().all(|n| a_pack.join(n).exists()));
    assert!(["d.bin", "e.bin"].iter().all(|n| b_pack.join(n).exists()));
    for id in [a, b] {
        let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
        assert!(priorities.iter().all(|p| *p == Priority::Normal), "torrent {id}: {priorities:?}");
    }
    assert_eq!(super::tests::stack(&engine).await, []);
    engine.stop().await;
}

/// N1 of #217's re-review: a Move to Trash that waits behind an undo bringing its torrent back runs
/// once the torrent is really back, librqbit settled with it: then nothing writes to the file it moves,
/// and its own undo finds it in the Trash as it went. After a whole-folder Delete, one going file by
/// file, and a partial Delete aborted by ⌘Z.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_queued_behind_an_undo_can_be_undone() {
    for case in ["whole folder", "file by file", "aborted partial"] {
        let dir = temp();
        let (engine, gate, id) = gated_pack(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        wait_for(&engine, id, Status::Paused).await;
        if case != "whole folder" {
            std::fs::write(dir.path().join("downloads/pack/mine.txt"), b"mine").unwrap();
        }
        let start = at_rest(&engine, dir.path()).await;
        let (undo, delete) = match case {
            "aborted partial" => {
                super::trash::hold(&gate, true);
                let delete = engine.sent(id, TorrentCommand::Delete).await;
                held_at_the_gate(&gate).await;
                (engine.undo_sent().await, Some(delete))
            }
            _ => {
                engine.command(id, TorrentCommand::Delete).await.unwrap();
                super::trash::hold(&gate, true);
                let undo = engine.undo_sent().await;
                engine.inspect(|_| ()).await;
                (undo, None)
            }
        };
        let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![1] }).await;
        super::trash::hold(&gate, false);
        if let Some(delete) = delete {
            let _ = delete.await;
        }
        undo.await.unwrap();
        moving.await.unwrap();
        engine.undo().await.unwrap_or_else(|e| panic!("{case}: the Move to Trash is undone: {e:?}"));
        settles_at(&engine, dir.path(), &start).await;
        engine.stop().await;
    }
}

/// S1 of #217's re-review: after a crash, a command waiting behind the undo resumed at the start
/// shows done at once too, its torrent's lifecycle rebuilt from the undo's record.
#[tokio::test(flavor = "multi_thread")]
async fn after_a_crash_a_command_behind_a_resumed_undo_shows_at_once() {
    let dir = temp();
    let pack = dir.path().join("downloads/pack");
    let id = {
        let (engine, gate, id) = gated_pack(dir.path()).await;
        wait_for(&engine, id, Status::Paused).await;
        std::fs::write(pack.join("mine.txt"), b"mine").unwrap();
        engine.command(id, TorrentCommand::Delete).await.unwrap();
        super::trash::hold(&gate, true);
        let undoing = tokio::spawn(engine.undo_later());
        held_at_the_gate(&gate).await;
        engine.kill().await;
        super::trash::abandon(&gate);
        let _ = undoing.await;
        id
    };
    let gate = super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    super::trash::hold(&gate, true);
    let engine = engine_with(dir.path(), Duration::from_millis(20), Default::default(), gate.clone()).await;
    held_at_the_gate(&gate).await;
    assert_eq!(find(&engine, id).await.status, Status::Paused, "back as it was, while its files go back");
    let resume = engine.sent(id, TorrentCommand::Resume).await;
    engine.inspect(|_| ()).await;
    let shown = find(&engine, id).await;
    assert_ne!(shown.status, Status::Paused, "the Resume shows at once");
    assert!(matches!(shown.actions.get(Action::Resume), proto::Answer::DoesNothing { .. }));
    super::trash::hold(&gate, false);
    resume.await.unwrap();
    engine.stop().await;
}

/// S2 of #217's re-review: an undo that can't bring its torrent back (something of the user's where
/// a file goes back) is refused with why; the commands that waited behind it are answered as undone,
/// not each as one for a torrent not in the list.
#[tokio::test(flavor = "multi_thread")]
async fn commands_behind_an_undo_that_fails_are_answered_as_undone() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let pack = dir.path().join("downloads/pack");
    wait_for(&engine, id, Status::Paused).await;
    std::fs::write(pack.join("mine.txt"), b"mine").unwrap();
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    super::trash::hold(&gate, true);
    let undo = engine.undo_sent().await;
    held_at_the_gate(&gate).await;
    for name in ["a.bin", "b.bin", "c.bin"] {
        if !pack.join(name).exists() {
            std::fs::write(pack.join(name), b"the user's").unwrap();
        }
    }
    let waiting = [
        engine.sent(id, TorrentCommand::Resume).await,
        engine.sent(id, TorrentCommand::Pause).await,
        engine.sent(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await,
    ];
    // Each came, behind the undo, before it goes on.
    engine.inspect(|_| ()).await;
    super::trash::hold(&gate, false);
    assert!(matches!(undo.await, Err(Failure::Refused(proto::Refusal::Undo { .. }))), "refused, with why");
    for answer in waiting {
        let answer = answer.await;
        assert!(matches!(answer, Err(Failure::Refused(proto::Refusal::Undone))), "{answer:?}");
    }
    engine.stop().await;
}

/// #217's re-review, S3: an action asked of a torrent with a step under way is judged at once on what
/// clients see: one refused there (a Delete while its destination is away) is answered so at once
/// and never goes on the stack, so ⌘Z still takes the step before it.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_refused_on_a_busy_torrent_never_goes_on_the_stack() {
    let dir = temp();
    let (engine, gate, id) = gated_pack(dir.path()).await;
    let _opens = OpensTrash(gate.clone());
    let downloads = dir.path().join("downloads");
    wait_for(&engine, id, Status::Paused).await;
    super::trash::hold(&gate, true);
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&gate).await;
    take_away(&downloads);
    until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
    let delete = tokio::time::timeout(Duration::from_secs(5), engine.command(id, TorrentCommand::Delete)).await.expect("answered at once");
    assert!(matches!(refusal(delete).1, TorrentRefusal::DestinationUnavailable { .. }));
    assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }], "never on the stack");
    bring_back(&downloads);
    super::trash::hold(&gate, false);
    let _ = moving.await;
    engine.stop().await;
}

/// B1 of #217's third review: an undo whose torrent librqbit checks in full as it's back (a file
/// rewritten while it was away) ends once it's handed to librqbit, as librqbit's check goes on. It
/// shows that check, and what comes after it runs in its turn: Cancel check stops it, a Pause runs,
/// all while the check is held.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_ends_as_its_torrent_is_back_while_librqbit_checks_it() {
    let dir = temp();
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir.path(), &gate).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    let a = dir.path().join("downloads/pack/a.bin");
    let bytes = std::fs::read(&a).unwrap();
    std::fs::remove_file(&a).unwrap();
    std::fs::write(&a, bytes).unwrap();
    gate.hold(true);
    let undone = tokio::time::timeout(Duration::from_secs(5), engine.undo()).await.expect("answered while librqbit checks");
    assert!(undone.is_ok(), "{undone:?}");
    wait_for(&engine, id, Status::Checking).await;
    let soon = |command| tokio::time::timeout(Duration::from_secs(5), engine.command(id, command));
    soon(TorrentCommand::CancelCheck).await.expect("in its turn").unwrap();
    soon(TorrentCommand::Pause).await.expect("in its turn").unwrap();
    wait_for(&engine, id, Status::Unchecked).await;
    assert!(gate.is_held(), "all of it with the check held");
    gate.hold(false);
    engine.stop().await;
}

/// S1 of #217's third review, and B1 of its fourth: a Move to Trash during librqbit's check moves at
/// once, and its undo finds what it moved as it went. librqbit sets each file's length as its check
/// ends, a check paused by Cancel check too (its fast-resume ignores the pause), through the file it
/// opened; Phantom's storage leaves a file moved from its path as it went. During a check then a
/// crash (the model's replay of the same move, with and without a restart, is
/// `a_move_to_trash_during_a_check_is_undone_once_the_check_ends`), during the first check of a
/// torrent just added, and right after a Cancel check of one an undo started.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_during_a_check_can_be_undone() {
    let cases = ["during a check, then a crash", "during its first check", "after a cancel check"];
    for case in cases {
        let dir = temp();
        let gate = Arc::new(super::files::Gate::default());
        let _opens = Opens(gate.clone());
        let mut engine = engine_gated(dir.path(), &gate).await;
        let source = valid_pack(dir.path()).await;
        if case == "during its first check" {
            gate.hold(true);
        }
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        if case != "during its first check" {
            wait_for(&engine, id, Status::Seeding).await;
        }
        let file = &engine.details(id).await.unwrap().files[0];
        let moved = dir.path().join("downloads/pack").join(file.path.file_name().unwrap());
        let before = std::fs::read(&moved).unwrap();
        match case {
            "during its first check" => {}
            // Back by an undo, sampled, then checked: the check is cancelled below.
            "after a cancel check" => {
                engine.command(id, TorrentCommand::Remove).await.unwrap();
                engine.undo().await.unwrap();
                sampled(&engine, id).await;
                gate.hold(true);
                engine.command(id, TorrentCommand::Check).await.unwrap();
            }
            _ => {
                gate.hold(true);
                engine.command(id, TorrentCommand::Check).await.unwrap();
            }
        }
        wait_for(&engine, id, Status::Checking).await;
        if case == "after a cancel check" {
            engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
            wait_for(&engine, id, Status::Unchecked).await;
        }
        let moving = tokio::time::timeout(Duration::from_secs(5), engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }));
        moving.await.expect("{case}: never waits for the check").unwrap_or_else(|e| panic!("{case}: {e:?}"));
        assert!(!moved.exists(), "{case}: moved at once");
        assert!(gate.is_held(), "{case}: with librqbit's reads still held");
        gate.hold(false);
        // librqbit's check ends, and sets each file's length.
        until(&engine, id, |s| !matches!(s, Status::Checking)).await;
        polls(&engine, 1).await;
        // What the move recorded is in the undo log: a crash doesn't lose it.
        if case.ends_with("a crash") {
            engine.kill().await;
            engine = engine_gated(dir.path(), &gate).await;
            until(&engine, id, |s| !matches!(s, Status::Checking)).await;
        }
        let undone = tokio::time::timeout(Duration::from_secs(5), engine.undo()).await.expect("answered");
        assert!(undone.is_ok(), "{case}: the Move to Trash is undone: {undone:?}");
        assert_eq!(std::fs::read(&moved).unwrap(), before, "{case}: back as it was");
        assert_eq!(stack(&engine).await, [], "{case}");
        // And the torrent as before the move (B1 of the fifth review): what the check found missing
        // while the file was in the Trash is checked again. One Unchecked stays so.
        match case {
            "after a cancel check" => wait_for(&engine, id, Status::Unchecked).await,
            _ => {
                wait_for(&engine, id, Status::Seeding).await;
                let content = find(&engine, id).await.content.unwrap();
                assert_eq!(content.progress_bytes, content.size_bytes, "{case}: all of it");
            }
        }
        engine.stop().await;
    }
}

/// ADR 0014: a file a Move to Trash took that shares a piece with one still downloading is made
/// anew at its path by that piece, a stub of Phantom's own, as for any file set Off that isn't
/// there: the piece checks and the other file completes. Its undo sends the stub to the Trash, never
/// erased, puts the old file back and checks the torrent. With a restart before it resumes too.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_sends_phantoms_own_stub_to_the_trash_then_checks() {
    for restart in [false, true] {
        let dir = temp();
        let seeded = dir.path().join("seed/pack");
        std::fs::create_dir_all(&seeded).unwrap();
        for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
            std::fs::write(seeded.join(name), (0..24576usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
        }
        let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
        let mut engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
        let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let files = engine.details(id).await.unwrap().files;
        let pack = dir.path().join("downloads/pack");
        let (first, second) = (pack.join(files[0].path.file_name().unwrap()), pack.join(files[1].path.file_name().unwrap()));
        let before = std::fs::read(&first).unwrap();
        // The second file goes, by hand: getting it again gets the piece it shares with the first.
        // Paused, so it's got only once the first is in the Trash.
        engine.command(id, TorrentCommand::Pause).await.unwrap();
        std::fs::remove_file(&second).unwrap();
        engine.command(id, TorrentCommand::Check).await.unwrap();
        until(&engine, id, |s| !matches!(s, Status::Checking | Status::Seeding | Status::Done { .. })).await;
        let content = find(&engine, id).await.content.unwrap();
        assert!(content.progress_bytes < content.size_bytes, "the second file is to get again");
        engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
        assert!(!first.exists());
        if restart {
            engine.stop().await;
            engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
        }
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        wait_for(&engine, id, Status::Seeding).await;
        assert!(first.exists(), "restart {restart}: a stub of the piece it shares");
        assert_ne!(std::fs::read(&first).unwrap(), before);
        assert_eq!(std::fs::read(&second).unwrap().len(), 24576, "the other file is complete");
        let undone = engine.undo().await;
        assert!(undone.is_ok(), "restart {restart}: {undone:?}");
        assert_eq!(std::fs::read(&first).unwrap(), before, "back as it went");
        let trashed: Vec<PathBuf> = std::fs::read_dir(dir.path().join("trash")).unwrap().flatten().map(|e| e.path()).collect();
        assert_eq!(trashed.len(), 1, "restart {restart}: the stub is in the Trash: {trashed:?}");
        until(&engine, id, |s| *s != Status::Checking).await;
        wait_for(&engine, id, Status::Seeding).await;
        let content = find(&engine, id).await.content.unwrap();
        assert_eq!(content.progress_bytes, content.size_bytes, "restart {restart}: checked, whole");
        engine.stop().await;
    }
}

/// #374: a shared piece got for a file set Off (at its path, so read only) that can't be kept in the
/// data dir (here a folder it can't write into; a full disk alike) fails librqbit's write: the
/// torrent stops with Error, as for any failed write, and nothing is counted that wasn't kept.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_that_cannot_be_kept_stops_the_torrent_with_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(seeded.join(name), (0..24576usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
    let engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let files = engine.details(id).await.unwrap().files;
    let second = dir.path().join("downloads/pack").join(files[1].path.file_name().unwrap());
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    std::fs::remove_file(&second).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| !matches!(s, Status::Checking | Status::Seeding | Status::Done { .. })).await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
    // The data dir takes no part.
    let parts = super::parts::dir(&dir.path().join("data"));
    std::fs::create_dir_all(&parts).unwrap();
    let mode = std::fs::metadata(&parts).unwrap().permissions();
    std::fs::set_permissions(&parts, std::fs::Permissions::from_mode(0o500)).unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    until(&engine, id, |s| matches!(s, Status::Error { .. } | Status::Seeding)).await;
    let status = find(&engine, id).await.status;
    std::fs::set_permissions(&parts, mode).unwrap();
    assert!(matches!(status, Status::Error { .. }), "{status:?}");
    engine.stop().await;
}

/// An older build's `undo-parts/` is deleted at the first start, read by nothing; a log of kept parts
/// that can't be read goes too.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_builds_parts_and_a_damaged_log_are_deleted_at_a_start() {
    let dir = temp();
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("undo-parts/abc/0")).unwrap();
    std::fs::write(data.join("undo-parts/abc/0/0"), [1; 4]).unwrap();
    let damaged = super::parts::dir(&data).join("abc");
    std::fs::create_dir_all(&damaged).unwrap();
    std::fs::write(damaged.join("0"), b"not a log").unwrap();
    let engine = engine(dir.path()).await;
    assert!(!data.join("undo-parts").exists());
    assert!(!damaged.join("0").exists());
    engine.stop().await;
}

/// Once its undo is over, a file a Move to Trash took is the torrent's again: deleted by hand
/// later (one set Off), a piece it shares with a file the torrent gets makes a stub of it at its
/// path, as for any file set Off that isn't there, not a part held in memory.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_back_from_the_trash_is_the_torrents_again() {
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(seeded.join(name), (0..24576usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
    let engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let files = engine.details(id).await.unwrap().files;
    let pack = dir.path().join("downloads/pack");
    let (first, second) = (pack.join(files[0].path.file_name().unwrap()), pack.join(files[1].path.file_name().unwrap()));
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    engine.undo().await.unwrap();
    assert!(first.exists(), "back");
    // Both gone by hand: getting the second again gets the piece it shares with the first.
    std::fs::remove_file(&first).unwrap();
    std::fs::remove_file(&second).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| !matches!(s, Status::Checking | Status::Done { .. })).await;
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!(std::fs::read(&second).unwrap().len(), 24576);
    assert!(first.exists(), "a stub of the first, for the piece it shares");
    engine.stop().await;
}

/// A file a Move to Trash took and then set Normal again is the torrent's again: downloaded from
/// nothing at its path (#276), not Missing files, nor kept in memory as one in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_in_the_trash_set_normal_again_is_got_again_at_its_path() {
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(seeded.join(name), (0..24576usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
    let engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let files = engine.details(id).await.unwrap().files;
    let first = dir.path().join("downloads/pack").join(files[0].path.file_name().unwrap());
    let before = std::fs::read(&first).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    assert!(!first.exists());
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Normal }).await.unwrap();
    assert_ne!(find(&engine, id).await.status, Status::MissingFiles);
    for _ in 0..500 {
        if std::fs::read(&first).is_ok_and(|b| b == before) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(std::fs::read(&first).ok(), Some(before), "got again at its path");
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// #217's fourth review: every storage librqbit gets for a torrent is Phantom's own (`Files`), which
/// never writes to a file moved from its path: at a first add, an add again (a check), and a restore
/// at a start. librqbit's own would write to it where it went.
#[tokio::test(flavor = "multi_thread")]
async fn every_storage_librqbit_gets_is_phantoms_own() {
    let dir = temp();
    let config = || Config {
        data_dir: dir.path().join("data"),
        download_dir: dir.path().join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.path().join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    let guard = super::files::Guard::default();
    let engine = Engine::start_with(config(), add_into.clone(), guard.clone(), super::Looks::real()).await.unwrap();
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let other = add(&engine, &dir.path().join("downloads"), multi_file_torrent(&["x.bin", "y.bin"]), true).await.unwrap().id;
    wait_for(&engine, other, Status::Paused).await;
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
    let engine = Engine::start_with(config(), add_into, guard.clone(), super::Looks::real()).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    wait_for(&engine, other, Status::Paused).await;
    assert_eq!(guard.unguarded(), 0, "librqbit's own storage");
    engine.stop().await;
}

/// S4 of #217's third review: a Cancel check that finds, at its turn, the check it was asked of
/// over (a Move to Trash before it took that long) is done: no check runs, as asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_check_that_finds_the_check_over_at_its_turn_is_done() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    // The check waits at its read gate too: unheld, librqbit's check of the pack ends within
    // milliseconds, and on a loaded machine before the Cancel check is even sent.
    let reads = Arc::new(super::files::Gate::default());
    let _reads_open = Opens(reads.clone());
    let config = Config {
        data_dir: dir.path().join("data"),
        download_dir: dir.path().join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: Duration::from_millis(20),
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.path().join("trash"), Default::default(), gate.clone()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    let guard = super::files::Guard::default().with_gate(super::files::At::Read, reads.clone());
    let engine = Engine::start_with(config, add_into, guard, super::Looks::real()).await.unwrap();
    let _opens = OpensTrash(gate.clone());
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    reads.hold(true);
    super::trash::hold(&gate, true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&gate).await;
    let cancel = engine.sent(id, TorrentCommand::CancelCheck).await;
    engine.inspect(|_| ()).await;
    assert_eq!(find(&engine, id).await.status, Status::Unchecked, "shown at once");
    // Only now does librqbit's check go on, to its end, while the Cancel check waits behind the move.
    reads.hold(false);
    for _ in 0..500 {
        if !engine.inspect(move |actor| actor.checking(id)).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!engine.inspect(move |actor| actor.checking(id)).await, "the check ended while the move waited");
    super::trash::hold(&gate, false);
    moving.await.unwrap();
    cancel.await.expect("done: no check runs");
    assert_ne!(find(&engine, id).await.status, Status::Checking);
    engine.stop().await;
}

/// S4 of #217's third review: a Check, a Cancel check and a Resume that checks first, behind the
/// rollback of an aborted Delete, show at once (a Check with nothing checked yet) and run once the
/// torrent is back (a Cancel check that finds the check over then:
/// `a_cancel_check_that_finds_the_check_over_at_its_turn_is_done`).
#[tokio::test(flavor = "multi_thread")]
async fn checks_behind_an_aborted_delete_run_once_it_is_back() {
    for case in ["check", "check, cancel", "resume"] {
        let dir = temp();
        let (engine, gate, id) = gated_pack(dir.path()).await;
        let _opens = OpensTrash(gate.clone());
        let pack = dir.path().join("downloads/pack");
        wait_for(&engine, id, Status::Paused).await;
        std::fs::write(pack.join("mine.txt"), b"mine").unwrap();
        if case == "resume" {
            // Missing a file: a Resume checks first.
            std::fs::remove_file(pack.join("c.bin")).unwrap();
            until(&engine, id, |s| matches!(s, Status::MissingFiles { .. })).await;
        }
        super::trash::hold(&gate, true);
        let delete = engine.sent(id, TorrentCommand::Delete).await;
        held_at_the_gate(&gate).await;
        let undo = engine.undo_sent().await;
        let mut asked = Vec::new();
        match case {
            "resume" => {
                asked.push(engine.sent(id, TorrentCommand::Resume).await);
                engine.inspect(|_| ()).await;
                assert_ne!(find(&engine, id).await.status, Status::Paused, "{case}: shown at once");
            }
            _ => {
                asked.push(engine.sent(id, TorrentCommand::Check).await);
                engine.inspect(|_| ()).await;
                let shown = find(&engine, id).await;
                assert_eq!(shown.status, Status::Checking, "{case}: shown at once");
                assert_eq!(shown.content.map(|c| c.progress_bytes), Some(0), "{case}: nothing checked yet");
                if case != "check" {
                    asked.push(engine.sent(id, TorrentCommand::CancelCheck).await);
                }
            }
        }
        engine.inspect(|_| ()).await;
        super::trash::hold(&gate, false);
        let _ = delete.await;
        undo.await.unwrap_or_else(|e| panic!("{case}: {e:?}"));
        for answer in asked {
            answer.await.unwrap_or_else(|e| panic!("{case}: no error: {e:?}"));
        }
        match case {
            "resume" => until(&engine, id, |s| !matches!(s, Status::Paused | Status::Checking)).await,
            _ => until(&engine, id, |s| !matches!(s, Status::Checking)).await,
        }
        assert_eq!(stack(&engine).await, [], "{case}");
        engine.stop().await;
    }
}

/// #217's second review, nits: a status shown before librqbit is in it (a Pause waiting behind a
/// Move to Trash) has no rates or peers, never "Paused · 2 MB/s"; a Check waiting there no progress.
#[tokio::test(flavor = "multi_thread")]
async fn a_status_librqbit_isnt_in_yet_shows_no_rates() {
    let dir = temp();
    let gate = super::trash::Gate::default();
    let engine = engine_with(dir.path(), Duration::from_millis(20), Default::default(), gate.clone()).await;
    let _opens = OpensTrash(gate.clone());
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    super::trash::hold(&gate, true);
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![0] }).await;
    held_at_the_gate(&gate).await;
    let rates = move |actor: &mut super::actor::Actor| actor.fake_rates(id, 2_000_000);
    assert_eq!(engine.inspect(rates).await, Some((2_000_000, 2_000_000, 1)), "librqbit's own status shows its rates");
    let pause = engine.sent(id, TorrentCommand::Pause).await;
    engine.inspect(|_| ()).await;
    assert_eq!(engine.inspect(rates).await, Some((0, 0, 0)), "Paused, before librqbit is");
    // A Check waiting there shows nothing checked yet, not what librqbit has.
    let check = engine.sent(id, TorrentCommand::Check).await;
    engine.inspect(|_| ()).await;
    let shown = find(&engine, id).await;
    assert_eq!((shown.status, shown.content.map(|c| c.progress_bytes)), (Status::Checking, Some(0)));
    super::trash::hold(&gate, false);
    moving.await.unwrap();
    pause.await.unwrap();
    check.await.unwrap();
    engine.stop().await;
}

/// The generated property of #190's undos, on a real engine.
mod restore {
    use proptest::prelude::*;

    use super::*;

    /// An action of [`any_undoable_actions_then_as_many_undos_restore_the_start`]'s.
    #[derive(Debug, Clone)]
    enum Act {
        /// Of the pack (`true`), the other, or both in one press (`None`: one step, #235).
        Remove(Option<bool>),
        Delete(Option<bool>),
        /// Of the pack, these files.
        TrashFiles(Vec<usize>),
    }

    cases::proptest! {
        #![proptest_config(ProptestConfig {
            cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).map_or(16, |c: u32| c / 4).max(4),
            ..Default::default()
        })]

        /// Any sequence of Removes, Deletes and Moves to Trash of two torrents, each Remove or Delete of
        /// one or of both in one press, then as many ⌘Z, brings back the start (#190, #235): the same torrents, statuses and priorities, the same bytes at the same
        /// paths, nothing in the Trash, nothing on the stack. Each is sent at once, in order; with the
        /// trash held from `held_from` on, the steps from there wait in flight, so ⌘Z meets them running
        /// (aborted after the item in flight, or dropped before they start), and the gate opens after
        /// the `open_after`th ⌘Z. A command refused (for a torrent no longer listed, say) has no step,
        /// and the ⌘Z past the stack has nothing to undo.
        #[test]
        fn any_undoable_actions_then_as_many_undos_restore_the_start(
            acts in prop::collection::vec(
                prop_oneof![
                    proptest::option::of(any::<bool>()).prop_map(Act::Remove),
                    proptest::option::of(any::<bool>()).prop_map(Act::Delete),
                    prop::collection::vec(0usize..3, 1..3).prop_map(Act::TrashFiles),
                ],
                1..5,
            ),
            held_from in 0usize..6,
            open_after in 0usize..6,
        ) {
            restores(&acts, held_from, open_after);
        }
    }

    /// A case of the property: `acts`, the trash held from the `held_from`th, opened after the
    /// `open_after`th ⌘Z.
    fn restores(acts: &[Act], held_from: usize, open_after: usize) {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let dir = temp();
            let (engine, gate, a, b) = two_torrents(dir.path()).await;
            let _opens = OpensTrash(gate.clone());
            let start = at_rest(&engine, dir.path()).await;
            let mut answers = Vec::new();
            for (n, act) in acts.iter().enumerate() {
                if n == held_from {
                    crate::engine::trash::hold(&gate, true);
                }
                let of = |pack: &Option<bool>| match pack {
                    Some(true) => vec![a],
                    Some(false) => vec![b],
                    None => vec![a, b],
                };
                let (ids, command) = match act {
                    Act::Remove(pack) => (of(pack), TorrentCommand::Remove),
                    Act::Delete(pack) => (of(pack), TorrentCommand::Delete),
                    Act::TrashFiles(files) => (vec![a], TorrentCommand::TrashFiles { files: files.clone() }),
                };
                answers.push(engine.sent_all(ids, command).await);
                // Its trashing started, so the next meets it running.
                engine.inspect(|_| ()).await;
            }
            let mut undos = Vec::new();
            for n in 0..acts.len() {
                if n == open_after {
                    crate::engine::trash::hold(&gate, false);
                }
                undos.push(engine.undo_sent().await);
                // Handled before the gate opens, as the case says.
                engine.inspect(|_| ()).await;
            }
            crate::engine::trash::hold(&gate, false);
            for answer in answers {
                let _ = tokio::time::timeout(Duration::from_secs(20), answer).await.expect("every command is answered");
            }
            for undone in undos {
                let undone = tokio::time::timeout(Duration::from_secs(20), undone).await.expect("every Undo is answered");
                assert!(
                    matches!(undone, Ok(_) | Err(Failure::Refused(proto::Refusal::NothingToUndo))),
                    "{acts:?}, held from {held_from}, open after {open_after}: {undone:?}"
                );
            }
            assert_eq!(stack(&engine).await, [], "{acts:?}");
            settles_at(&engine, dir.path(), &start).await;
            engine.stop().await;
        });
    }

    /// A case the property found: every file moved to the Trash by two moves, then a Delete with
    /// nothing left to move, which takes the torrent off the list all the same; the ⌘Zs bring back the
    /// torrent, then each move's files.
    #[test]
    fn two_moves_that_take_every_file_then_a_delete_then_as_many_undos_restore_the_start() {
        // Nothing held: each runs to its end before the ⌘Zs.
        restores(&[Act::TrashFiles(vec![0]), Act::TrashFiles(vec![2, 1]), Act::Delete(Some(true)), Act::Remove(Some(false))], 9, 9);
    }
}

/// ADR 0014: anything at a trashed file's path that Phantom didn't make there (here the person's
/// own file) refuses the undo as something at its path: nothing moves, the person's file stays as
/// it is, and the old copy stays in the Trash, for Finder's Put Back.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_is_refused_over_a_file_phantom_didnt_make() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let path = file(dir.path(), "b.bin");
    let old = std::fs::read(&path).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    std::fs::write(&path, b"mine").unwrap();
    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(&why, proto::CantUndo::PathTaken { path: p } if p.ends_with("b.bin")), "{why:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"mine", "the person's file stays");
    let trashed: Vec<Vec<u8>> =
        std::fs::read_dir(dir.path().join("trash")).unwrap().flatten().map(|e| std::fs::read(e.path()).unwrap()).collect();
    assert_eq!(trashed, [old], "the old copy stays in the Trash");
    assert!(stack(&engine).await.is_empty(), "a refused undo is gone");
    engine.stop().await;
}

/// An engine whose default folder is looked at with `look`.
async fn engine_looking_at_the_default_folder(dir: &Path, look: super::default_folder::Look) -> Engine {
    let config = Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: Duration::from_millis(100),
        trash: super::trash::into(dir.join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    let looks = super::Looks { default_folder: look, ..super::Looks::real() };
    Engine::start_with(config, add_into, super::files::Guard::default(), looks).await.unwrap()
}

fn default_unavailable(engine: &Engine) -> Option<proto::Unavailable> {
    // What a client gets, which is what the protocol carries.
    engine.snapshot.borrow().for_client(&[], &[], None).default_destination_unavailable
}

/// The default destination is watched as a torrent's is (#209): every snapshot says why it can't
/// be reached, as it changes: gone when its folder is (the stand-in for a drive that's unplugged),
/// not once it's back, and none for a folder that's never set or no longer the default.
#[tokio::test(flavor = "multi_thread")]
async fn the_default_folder_going_away_and_coming_back_shows_in_the_snapshots() {
    let dir = temp();
    let folder = dir.path().join("films");
    std::fs::create_dir_all(&folder).unwrap();
    let path = AbsolutePath::new(&folder).unwrap();
    let engine = engine_looking_at_the_default_folder(dir.path(), super::default_folder::look()).await;
    assert_eq!(default_unavailable(&engine), None, "no default destination, nothing to reach");

    engine.set_default_destination(Some(path.clone())).await;
    looks(&engine, 1).await;
    assert_eq!(default_unavailable(&engine), None, "it's there");

    std::fs::remove_dir(&folder).unwrap();
    let missing = Some(proto::Unavailable::FolderMissing { path: path.clone() });
    assert!(eventually(10, || default_unavailable(&engine) == missing).await, "gone: {:?}", default_unavailable(&engine));

    std::fs::create_dir(&folder).unwrap();
    assert!(eventually(10, || default_unavailable(&engine).is_none()).await, "back");

    std::fs::remove_dir(&folder).unwrap();
    assert!(eventually(10, || default_unavailable(&engine) == missing).await, "gone again");
    // Another folder is the default now (or none): what was found of the last one isn't said.
    engine.set_default_destination(None).await;
    assert!(eventually(10, || default_unavailable(&engine).is_none()).await, "no default, no reason");
    engine.stop().await;
}

/// A default folder on a disk that doesn't answer (a network volume, which can't be generated):
/// the snapshots say it can't be opened once the look has taken `STALLED_AFTER`, the engine goes
/// on answering, with one look at a time, and once the disk answers they say what it found.
#[tokio::test(flavor = "multi_thread")]
async fn a_default_folder_that_doesnt_answer_is_unreachable_without_holding_up_the_engine() {
    let dir = temp();
    let folder = AbsolutePath::new(dir.path().join("nas")).unwrap();
    let held = Arc::new((Mutex::new(true), std::sync::Condvar::new()));
    let looks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let look: super::default_folder::Look = {
        let (held, looks) = (held.clone(), looks.clone());
        Arc::new(move |_| {
            looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (on, changed) = &*held;
            let mut on = on.lock().unwrap();
            while *on {
                on = changed.wait(on).unwrap();
            }
            None
        })
    };
    let engine = engine_looking_at_the_default_folder(dir.path(), look).await;
    engine.set_default_destination(Some(folder.clone())).await;
    let started = std::time::Instant::now();
    let cant_open = Some(proto::Unavailable::CantOpen { path: folder });
    while default_unavailable(&engine) != cant_open {
        assert_eq!(default_unavailable(&engine), None, "a look that's only taken {:?} isn't stalled", started.elapsed());
        assert!(started.elapsed() < super::space::STALLED_AFTER + Duration::from_secs(5), "never said it can't be opened");
        let answer = tokio::time::timeout(Duration::from_secs(1), engine.listing()).await;
        assert!(answer.is_ok(), "the engine answers while the disk is stalled");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Not before the look has taken `STALLED_AFTER` (a slow disk isn't a stalled one), give or take a poll.
    let said = started.elapsed();
    assert!(said >= super::space::STALLED_AFTER - Duration::from_millis(500), "a look that's only taken {said:?} isn't stalled");
    // Each tick while it's stuck reports it, and starts no other look.
    self::looks(&engine, 3).await;
    assert_eq!(looks.load(std::sync::atomic::Ordering::SeqCst), 1, "one look at a time, however long it's stuck");

    *held.0.lock().unwrap() = false;
    held.1.notify_all();
    assert!(eventually(10, || default_unavailable(&engine).is_none()).await, "once the disk answers, what it found");
    engine.stop().await;
}

/// A look that finishes after the folder has changed says nothing: folder A's look is stuck, B is
/// made the default and answers at once, and when A's look ends (finding it missing) the snapshots
/// still say B is fine. Deterministic, where the same race in a real run is a matter of timing.
#[tokio::test(flavor = "multi_thread")]
async fn what_a_look_finds_of_a_folder_that_is_no_longer_the_default_is_not_said() {
    let dir = temp();
    let (a, b) = (AbsolutePath::new(dir.path().join("a")).unwrap(), AbsolutePath::new(dir.path().join("b")).unwrap());
    // A's look is stuck until `held` is released, then finds the folder missing. B's answers at once,
    // once; its later looks (a tick each) are stuck too, so none comes to correct a wrong answer.
    let held = Arc::new((Mutex::new(true), std::sync::Condvar::new()));
    let held_b = Arc::new((Mutex::new(true), std::sync::Condvar::new()));
    let (a_looks, b_looks) = (Arc::new(std::sync::atomic::AtomicUsize::new(0)), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let wait = |gate: &Arc<(Mutex<bool>, std::sync::Condvar)>| {
        let (on, changed) = &**gate;
        let mut on = on.lock().unwrap();
        while *on {
            on = changed.wait(on).unwrap();
        }
    };
    let look: super::default_folder::Look = {
        let (held, held_b, a, a_looks, b_looks) = (held.clone(), held_b.clone(), a.clone(), a_looks.clone(), b_looks.clone());
        Arc::new(move |folder| {
            if *folder == a {
                a_looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                wait(&held);
                return Some(proto::Unavailable::FolderMissing { path: folder.clone() });
            }
            if b_looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                wait(&held_b);
            }
            None
        })
    };
    let engine = engine_looking_at_the_default_folder(dir.path(), look).await;
    let seen = |looks: &Arc<std::sync::atomic::AtomicUsize>| looks.load(std::sync::atomic::Ordering::SeqCst) > 0;
    engine.set_default_destination(Some(a)).await;
    assert!(eventually(10, || seen(&a_looks)).await, "A is looked at");
    let before = engine.inspect(|a| a.looks_reported).await;
    engine.set_default_destination(Some(b)).await;
    // B's answer, the one report until A's look ends.
    looks_since(&engine, before, 1).await;
    assert!(seen(&b_looks));
    let before = engine.inspect(|a| a.looks_reported).await;
    *held.0.lock().unwrap() = false;
    held.1.notify_all();
    // A's look's end, reported.
    looks_since(&engine, before, 1).await;
    assert_eq!(default_unavailable(&engine), None, "what A's look found isn't B's");
    *held_b.0.lock().unwrap() = false;
    held_b.1.notify_all();
    engine.stop().await;
}

/// What a parked add task holds: the arguments of librqbit's add, and the task's own id.
type Parked = (Arc<librqbit::Session>, u64, librqbit::AddTorrent<'static>, AbsolutePath, PathBuf, super::files::Guard, tokio::task::Id);

/// An engine whose add tasks park in `add_into`, forever, handing what they hold to the test.
async fn parking_engine(dir: &Path) -> (Engine, Arc<Mutex<Vec<Parked>>>) {
    let parked: Arc<Mutex<Vec<Parked>>> = Arc::default();
    let parking = parked.clone();
    let add_into: super::actor::AddInto = Arc::new(move |session, id, add, destination, download_dir, guard| {
        let parking = parking.clone();
        Box::pin(async move {
            parking.lock().unwrap().push((session, id, add, destination, download_dir, guard, tokio::task::id()));
            std::future::pending().await
        })
    });
    let config = Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: super::POLL_EVERY,
        trash: super::trash::into(dir.join("trash"), Default::default(), Default::default()),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let engine = Engine::start_with(config, add_into, Default::default(), super::Looks::real()).await.unwrap();
    (engine, parked)
}

/// Waits up to 10 seconds for the add tasks parked to be `count`, and takes the newest.
async fn newest_parked(parked: &Arc<Mutex<Vec<Parked>>>, count: usize) -> Parked {
    for _ in 0..1000 {
        if parked.lock().unwrap().len() == count {
            return parked.lock().unwrap().pop().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{} add tasks parked, not {count}", parked.lock().unwrap().len());
}

/// #322: the end of an add task that a Remove stopped, arriving after an Undo brought the torrent
/// back under the same id and its new add. It's the old task's, not the new one's: the torrent is
/// still fetching, and what the old task added is gone from librqbit (the Remove took it out).
#[tokio::test(flavor = "multi_thread")]
async fn the_late_end_of_a_removed_adds_task_isnt_the_end_of_its_undos() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let (engine, parked) = parking_engine(dir.path()).await;
    let id = engine.add(super::fetched(torrent_file("a.bin")).await.unwrap(), false, parsed(&downloads)).await.unwrap().id;
    // The task got as far as librqbit having the torrent, before the Remove reached it.
    let (session, at, add, destination, download_dir, guard, old) = newest_parked(&parked, 1).await;
    assert_eq!(at, id);
    let added = super::actor::add_into(session, id, add, destination, download_dir, guard).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    engine.undo().await.unwrap();
    let new = newest_parked(&parked, 1).await.6;
    assert_ne!(old, new);

    send_end(&engine, id, old, added).await;
    let fetching = engine.inspect(move |actor| matches!(actor.torrents[&id].lib, super::machine::lib::Lib::Adding(_))).await;
    assert!(fetching, "the torrent is still fetching: it's the new task's to end");
    engine.stop().await;
}

/// #322's `Orphan`: an add's task that ends with librqbit holding the torrent after a Remove took
/// it off the list. Its end is sent by hand: a fetch held in `add_into` is aborted by the Remove, so
/// only a task that got past its last await before the abort ends this way.
#[tokio::test(flavor = "multi_thread")]
async fn an_add_that_ends_after_its_remove_leaves_nothing_in_librqbit() {
    let dir = temp();
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let (engine, parked) = parking_engine(dir.path()).await;
    let id = engine.add(super::fetched(torrent_file("a.bin")).await.unwrap(), false, parsed(&downloads)).await.unwrap().id;
    let (session, at, add, destination, download_dir, guard, task) = newest_parked(&parked, 1).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();

    // The add into librqbit happens after the Remove's own delete: nothing else would take it out.
    assert_eq!(at, id);
    let added = super::actor::add_into(session.clone(), id, add, destination, download_dir, guard).await;
    let in_librqbit = librqbit::api::TorrentIdOrHash::Id(id as usize);
    assert!(session.get(in_librqbit).is_some(), "librqbit has a torrent nobody lists");

    send_end(&engine, id, task, added).await;
    assert!(engine.listing().await.is_empty());
    assert!(session.get(in_librqbit).is_none(), "taken back out of librqbit");
    engine.stop().await;
}

/// Hands the actor the end of add task `task` of torrent `id`, as the task itself would.
async fn send_end(engine: &Engine, id: u64, task: tokio::task::Id, added: anyhow::Result<librqbit::AddTorrentResponse>) {
    engine.actor.send(super::actor::Message::AddFinished { id, task, added }).await.unwrap();
    engine.inspect(|_| ()).await;
}

mod checks;
mod counted;
mod crashed;
mod letgo;
mod links;
mod moving;
mod never_cut;
mod off_gone;
mod open_files;
mod oracle;
mod pausing;
mod renaming;
mod undo_place;

/// A .torrent that can't be read at a start keeps its torrent (#244's review): listed under its id,
/// an error, which a Check tries again (still an error while it can't be read, Seeding once it can);
/// and Remove takes it off the list. Nothing that can't be read is deleted.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_whose_torrent_file_cant_be_read_is_an_error_until_a_check_reads_it() {
    let dir = temp();
    let (id, hash) = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let hash = find(&engine, id).await.info_hash;
        engine.stop().await;
        (id, hash)
    };
    let data = dir.path().join("data");
    let bytes = super::store::saved_torrent(&data, &hash).expect("kept with its record");
    super::store::set_torrent(&data, &hash, b"d4:info");
    let restarted = engine(dir.path()).await;
    let engine = restarted;
    let t = find(&engine, id).await;
    assert!(matches!(t.status, Status::Error { .. }), "{t:?}");
    assert!(t.actions.does(proto::Action::Check) && t.actions.does(proto::Action::Remove), "{:?}", t.actions);
    assert!(engine.command(id, TorrentCommand::Check).await.is_err(), "still can't be read");
    assert!(matches!(find(&engine, id).await.status, Status::Error { .. }));
    assert_eq!(super::store::saved_torrent(&data, &hash).as_deref(), Some(&b"d4:info"[..]), "not deleted");
    super::store::set_torrent(&data, &hash, &bytes);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;

    super::store::set_torrent(&data, &hash, b"d4:info");
    let again = self::engine(dir.path()).await;
    let engine = again;
    assert!(matches!(find(&engine, id).await.status, Status::Error { .. }));
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    assert!(!engine.listing().await.iter().any(|t| t.id == id), "off the list");
    engine.stop().await;
}

/// Decided on #244: at a start each torrent is listed at once as its record says, and librqbit's
/// sampling of the pieces it saved shows nothing. It counts as running on the torrent's lane: a
/// Pause pressed meanwhile shows done at once (Done, as the torrent is complete), waits, and runs
/// once the sampling is over.
#[tokio::test(flavor = "multi_thread")]
async fn a_pause_during_the_start_sampling_shows_at_once_and_runs_after_it() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    gate.hold(true);
    let engine = engine_gated(dir.path(), &gate).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "as its record says, at once");
    for _ in 0..1000 {
        if gate.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting(), "librqbit's sampling is held");
    let mut paused = Box::pin(engine.sent(id, TorrentCommand::Pause).await);
    // Shown done while the sampling is still held: as soon as the engine has it.
    until(&engine, id, |s| *s == Status::Done { reached: None }).await;
    assert!(gate.waiting(), "still sampling");
    // Not carried out at the polls of librqbit meanwhile either.
    polls(&engine, 2).await;
    assert!(unanswered(&mut paused).await, "it waits for the sampling");
    let wants = engine.inspect(move |a| a.torrents[&id].lifecycle.wants).await;
    assert_eq!(wants, super::machine::Wants::Run, "not carried out yet");
    assert_eq!(find(&engine, id).await.status, Status::Done { reached: None }, "still shown done");
    gate.hold(false);
    tokio::time::timeout(Duration::from_secs(10), paused).await.expect("runs once the sampling is over").unwrap();
    assert_eq!(find(&engine, id).await.status, Status::Done { reached: None });
    assert!(!librqbit_live(&engine, id).await, "librqbit has it paused");
    engine.stop().await;
}

/// Decided on #244: a file deleted by hand while Phantom runs has librqbit's handle on it closed at
/// once, so its space is freed (no handle left on a file with no name), by the look the size checks
/// make, off the actor: with that close held, the engine answers all the same. The torrent stays in
/// librqbit, stopped, Missing files, and Resume brings it back as before: checked, getting it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_by_hand_is_closed_at_once_off_the_actor() {
    let dir = temp();
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated_at(dir.path(), super::files::At::Close, &gate).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let deleted = file(dir.path(), "b.bin");
    let open = super::trashed::open_on(&deleted);
    assert!(open() > 0, "librqbit has it open");
    // Another moved elsewhere by hand: missing too, but its space is where it went.
    let (moved, away) = (file(dir.path(), "c.bin"), dir.path().join("c-moved.bin"));
    let open_moved = super::trashed::open_on(&moved);
    gate.hold(true);
    std::fs::rename(&moved, &away).unwrap();
    std::fs::remove_file(&deleted).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    for _ in 0..1000 {
        if gate.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting(), "the look closes it");
    let other = add(&engine, &dir.path().join("downloads"), magnet(HASH), true).await.unwrap().id;
    // Across several size checks, each of which would close it on the actor if it were the actor's.
    let until = std::time::Instant::now() + SIZE_CHECK_EVERY * 5;
    while std::time::Instant::now() < until {
        let command = if engine.listing().await.iter().any(|t| t.id == other && t.status == Status::Paused) {
            TorrentCommand::Resume
        } else {
            TorrentCommand::Pause
        };
        let answered = tokio::time::timeout(Duration::from_secs(2), engine.command(other, command)).await;
        assert!(answered.is_ok(), "the engine answers while the close is held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(open() > 0, "held");
    gate.hold(false);
    for _ in 0..1000 {
        if open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), 0, "closed: its space is freed");
    size_checks(&engine, 3).await;
    assert!(open_moved() > 0, "one moved elsewhere stays open");
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    let kept = engine.inspect(move |a| matches!(a.torrents[&id].lib, super::machine::lib::Lib::In(_))).await;
    assert!(kept, "still in librqbit");
    assert!(!librqbit_live(&engine, id).await, "stopped");
    let before = librqbit_torrent(&engine, id).await;
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    assert_ne!(librqbit_torrent(&engine, id).await, before, "checked: added again");
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

/// #244's re-check: ⌘Z on a Delete held while librqbit samples its torrent's pieces (at a start)
/// drops it as one that waited its turn: nothing moved, to the Trash or back. The Delete is answered
/// undone and the Undo done while the sampling is still held.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_of_a_delete_held_during_the_sampling_moves_nothing() {
    let dir = temp();
    let id = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    gate.hold(true);
    let engine = engine_gated(dir.path(), &gate).await;
    for _ in 0..1000 {
        if gate.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting(), "librqbit's sampling is held");
    let deleting = engine.sent(id, TorrentCommand::Delete).await;
    let undoing = engine.undo_sent().await;
    let undone = tokio::time::timeout(Duration::from_secs(5), undoing).await.expect("answered while the sampling is held");
    assert!(undone.is_ok(), "{undone:?}");
    let deleted = tokio::time::timeout(Duration::from_secs(5), deleting).await.expect("answered while the sampling is held");
    assert!(matches!(deleted, Err(Failure::Refused(proto::Refusal::Undone))), "{deleted:?}");
    assert!(gate.waiting(), "still sampling");
    gate.hold(false);
    sampled(&engine, id).await;
    size_checks(&engine, 3).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding, "as it was");
    for name in ["a.bin", "b.bin", "c.bin"] {
        assert!(file(dir.path(), name).exists(), "{name} where it was");
    }
    let trashed = std::fs::read_dir(dir.path().join("trash")).map_or(0, |d| d.count());
    assert_eq!(trashed, 0, "nothing ever went to the Trash");
    engine.stop().await;
}

/// #244's re-check: a step held while librqbit samples its torrent's pieces runs once the torrent
/// leaves librqbit as its destination goes, the sampling with it, not only once librqbit trusts them.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_held_during_the_sampling_runs_once_its_destination_goes() {
    let dir = temp();
    let destination = elsewhere(dir.path());
    let id = {
        let engine = engine(dir.path()).await;
        let source = complete_torrent_in(dir.path(), "elsewhere", 0).await;
        let id = add_to(&engine, source, false, &destination).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.stop().await;
        id
    };
    let gate = Arc::new(super::files::Gate::default());
    let _opens = Opens(gate.clone());
    gate.hold(true);
    let engine = engine_gated(dir.path(), &gate).await;
    let resuming = engine.inspect(move |a| matches!(a.torrents[&id].lib, super::machine::lib::Lib::Resuming(_))).await;
    assert!(resuming, "librqbit samples what it saved");
    let mut pausing = Box::pin(engine.sent(id, TorrentCommand::Pause).await);
    polls(&engine, 1).await;
    assert!(unanswered(&mut pausing).await, "held");
    take_away(&destination);
    let paused = tokio::time::timeout(Duration::from_secs(10), pausing).await.expect("runs once it's out of librqbit");
    paused.unwrap();
    assert!(gate.is_held(), "the sampling never ended");
    until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
    engine.stop().await;
}

/// #244's second re-check (M1): a file deleted by hand and set Off is to be let go of; put back by
/// hand and checked before the next look, the check finds it whole, so nothing is let go of: it
/// keeps its pieces, and the torrent isn't taken out and added back again.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_clears_the_let_go_of_a_missing_file_set_off_and_put_back() {
    let dir = temp();
    let every = Duration::from_secs(1);
    let engine = engine_size_checking_every(dir.path(), every).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let path = file(dir.path(), "b.bin");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    // Right after a size check, so the next look comes only after the check below starts.
    size_checks(&engine, 1).await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
    let letting = engine.inspect(move |a| a.torrents[&id].record.letting_go.clone()).await;
    assert!(matches!(letting, super::machine::letgo::State::Gone { .. }), "to be let go of: {letting:?}");
    std::fs::write(&path, &bytes).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| *s != Status::Checking).await;
    let letting = engine.inspect(move |a| a.torrents[&id].record.letting_go.clone()).await;
    assert_eq!(letting, super::machine::letgo::State::Holding, "the check found what's on disk");
    let added = librqbit_torrent(&engine, id).await;
    size_checks(&engine, 3).await;
    assert_eq!(engine.details(id).await.unwrap().files[b].progress_bytes, 2 * PIECE as u64, "it keeps its pieces");
    assert_eq!(librqbit_torrent(&engine, id).await, added, "not taken out and added back");
    engine.stop().await;
}

/// #276: a Move to Trash does nothing for a file that isn't at its path (one in the Trash already,
/// here): with only that one, nothing changes and nothing is on the stack; with another that's
/// there, only that one moves, and the entry counts it alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_does_nothing_for_a_file_not_on_disk() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let (a, b) = (index_of(&engine, id, "a.bin").await, index_of(&engine, id, "b.bin").await);
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    let one = vec![proto::UndoAction::TrashFiles { count: 1 }];
    assert_eq!(stack(&engine).await, one);
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    assert_eq!(stack(&engine).await, one, "no step for a file not on disk");
    engine.command(id, TorrentCommand::TrashFiles { files: vec![a, b] }).await.unwrap();
    assert_eq!(stack(&engine).await, [one.clone(), one].concat(), "only the one on disk");
    let details = engine.details(id).await.unwrap();
    assert!(!details.files[a].on_disk && !file(dir.path(), "a.bin").exists(), "a.bin went");
    assert_eq!(details.files[b].priority, Priority::Off);
    let trashed = std::fs::read_dir(dir.path().join("trash")).unwrap().count();
    assert_eq!(trashed, 2, "b.bin once, a.bin once");
    engine.stop().await;
}

/// #276: a file a Move to Trash took, set Normal again, is made at its path and downloaded there;
/// the Move to Trash can still be undone. ⌘Z sends that new copy to the Trash (never erased), puts
/// the old one back, whatever the sizes, and checks the torrent: it has the old one whole, with the
/// priority it had before the move. Set Off before the move or not.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_puts_the_old_copy_back_over_a_new_download() {
    for off in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let b = index_of(&engine, id, "b.bin").await;
        let path = file(dir.path(), "b.bin");
        let old = std::fs::read(&path).unwrap();
        if off {
            engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
        }
        engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
        engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
        // The new copy, as far as it got: made as librqbit gets it, once it's added back without
        // the pieces it let go of (#374, #436), Downloading meanwhile.
        until(&engine, id, |s| *s == Status::Downloading).await;
        assert!(eventually(10, || path.exists()).await, "made again at its path");
        assert_ne!(std::fs::read(&path).unwrap(), old);
        assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }], "still undoable");
        engine.undo().await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), old, "the old copy is back (off: {off})");
        let trashed: Vec<PathBuf> = std::fs::read_dir(dir.path().join("trash")).unwrap().flatten().map(|e| e.path()).collect();
        assert_eq!(trashed.len(), 1, "the new copy is in the Trash: {trashed:?}");
        until(&engine, id, |s| *s != Status::Checking).await;
        let details = engine.details(id).await.unwrap();
        assert_eq!(details.files[b].progress_bytes, 2 * PIECE as u64, "checked: the old copy is whole");
        assert_eq!(details.files[b].priority, if off { Priority::Off } else { Priority::Normal });
        assert_eq!(find(&engine, id).await.status, Status::Seeding);
        assert!(stack(&engine).await.is_empty());
        engine.stop().await;
    }
}

/// #278 review: an undo over new copies, cut short by a crash after it put back one old copy
/// (c.bin) and before another (a.bin), goes on at the next start. The old c.bin it put back is
/// no new copy: it stays, and is never sent to the Trash with the copies.
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_undo_keeps_an_old_copy_it_put_back_before_the_crash() {
    use std::os::unix::fs::MetadataExt;
    let dir = temp();
    let pack = dir.path().join("downloads/pack");
    let (old_a, old_c) = {
        let (engine, gate, id) = gated_pack(dir.path()).await;
        let (old_a, old_c) = (std::fs::metadata(pack.join("a.bin")).unwrap().ino(), std::fs::metadata(pack.join("c.bin")).unwrap().ino());
        engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1, 2] }).await.unwrap();
        engine.command(id, TorrentCommand::SetPriority { files: vec![0, 2], priority: Priority::Normal }).await.unwrap();
        super::trash::hold_under(&gate, &pack.join("b.bin"));
        let undoing = tokio::spawn(engine.undo_later());
        held_at_the_gate(&gate).await;
        assert_eq!(std::fs::metadata(pack.join("c.bin")).unwrap().ino(), old_c, "the old c.bin is back before the crash");
        engine.kill().await;
        super::trash::abandon(&gate);
        let _ = undoing.await;
        (old_a, old_c)
    };
    let engine = engine_with(dir.path(), Duration::from_millis(20), Default::default(), Default::default()).await;
    for _ in 0..500 {
        if engine.inspect(|actor| super::undo::undoing(actor.steps.machine()).next().is_none()).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let at = |name: &str| std::fs::metadata(pack.join(name)).map(|m| m.ino()).ok();
    assert_eq!(at("c.bin"), Some(old_c), "the old c.bin stays back");
    assert_eq!(at("a.bin"), Some(old_a), "the old a.bin is put back");
    engine.stop().await;
}

/// Phantom's stub at a trashed file's path, replaced by the person's own file of the
/// same length (an editor's save: a new file renamed onto it), isn't Phantom's: ⌘Z is refused, the
/// person's file stays, and the old copy stays in the Trash.
#[tokio::test(flavor = "multi_thread")]
async fn a_stub_replaced_by_the_persons_file_refuses_the_undo() {
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(seeded.join(name), (0..24576usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
    let engine = engine_size_checking_every(dir.path(), SIZE_CHECK_EVERY).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let files = engine.details(id).await.unwrap().files;
    let pack = dir.path().join("downloads/pack");
    let (first, second) = (pack.join(files[0].path.file_name().unwrap()), pack.join(files[1].path.file_name().unwrap()));
    let before = std::fs::read(&first).unwrap();
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    std::fs::remove_file(&second).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| !matches!(s, Status::Checking | Status::Seeding | Status::Done { .. })).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert!(first.exists(), "a stub of the piece it shares");
    let mine = vec![7u8; std::fs::metadata(&first).unwrap().len() as usize];
    let new = first.with_extension("new");
    std::fs::write(&new, &mine).unwrap();
    std::fs::rename(&new, &first).unwrap();
    let (_, why) = cant_undo(engine.undo().await);
    assert!(matches!(&why, proto::CantUndo::PathTaken { .. }), "{why:?}");
    assert_eq!(std::fs::read(&first).unwrap(), mine, "the person's file stays");
    let trashed: Vec<Vec<u8>> =
        std::fs::read_dir(dir.path().join("trash")).unwrap().flatten().map(|e| std::fs::read(e.path()).unwrap()).collect();
    assert_eq!(trashed, [before], "only the old copy is in the Trash");
    settled(&engine).await;
    assert_eq!(std::fs::read(&first).unwrap(), mine, "nothing writes into the person's file");
    engine.stop().await;
}

/// (ADR 0014: "anything else at the path refuses the undo"): a trashed file set Normal
/// again while paused, so nothing of Phantom's is made at its path, then the person's own file
/// put there: ⌘Z is refused, and the person's file isn't sent to the Trash as a new copy.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_file_at_the_path_of_a_file_set_normal_again_refuses_the_undo() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let path = file(dir.path(), "b.bin");
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
    std::fs::write(&path, b"mine").unwrap();
    let undone = engine.undo().await;
    assert_eq!(std::fs::read(&path).ok().as_deref(), Some(&b"mine"[..]), "the person's file stays: {undone:?}");
    let (_, why) = cant_undo(undone);
    assert!(matches!(&why, proto::CantUndo::PathTaken { .. }), "{why:?}");
    engine.stop().await;
}
