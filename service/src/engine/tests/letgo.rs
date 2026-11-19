//! #234: a file a Move to Trash took, emptied from the Trash while librqbit has it open, is let go of:
//! closed at once, so its space is freed, and its torrent taken out of librqbit and added back
//! without its pieces, its row as it was. Every Trash here is the test's own folder.

use std::sync::atomic::{AtomicBool, Ordering};

use super::super::actor::letgo::CrashAt;
use super::super::machine::letgo;
use super::super::machine::lib::Lib;
use super::super::trashed::open_on;
use super::*;

/// The item the test's Trash holds of the file named `name`.
fn in_trash(dir: &Path, name: &str) -> PathBuf {
    let items = std::fs::read_dir(dir.join("trash")).unwrap().flatten();
    let items: Vec<PathBuf> = items.map(|e| e.path()).collect();
    items
        .iter()
        .find(|p| p.to_string_lossy().ends_with(&format!("%{name}")))
        .cloned()
        .unwrap_or_else(|| panic!("{name} not in the trash: {items:?}"))
}

/// The name of torrent `id`'s file `index`: librqbit orders them its own way.
async fn name(engine: &Engine, id: u64, index: usize) -> String {
    engine.details(id).await.unwrap().files[index].path.file_name().unwrap().to_string_lossy().into_owned()
}

/// How many pieces librqbit has saved of torrent `hash`.
pub(super) fn saved_pieces(dir: &Path, hash: &str) -> u64 {
    super::super::bitv::pieces(&std::fs::read(super::super::bitv::path(&dir.join("data/session"), hash)).unwrap_or_default())
}

/// Whether the engine is done letting go of torrent `id`'s files: nothing to let go of in its record,
/// and librqbit has it, past resuming.
async fn let_go(engine: &Engine, id: u64) -> bool {
    engine
        .inspect(move |a| {
            let Some(t) = a.torrents.get(&id) else { return false };
            t.record.letting_go == letgo::State::Holding && matches!(t.lib, Lib::In(_))
        })
        .await
}

/// [`engine`]'s, counting into `reads` every read librqbit makes of the torrents' files.
pub(super) async fn engine_counting(dir: &Path, reads: Arc<std::sync::atomic::AtomicUsize>) -> Engine {
    engine_reading(dir, reads, Arc::default()).await
}

/// [`engine_counting`]'s, each read waiting while `gate` is held.
async fn engine_reading(dir: &Path, reads: Arc<std::sync::atomic::AtomicUsize>, gate: Arc<super::super::files::Gate>) -> Engine {
    let guard = super::super::files::Guard::default().counting_reads(reads).with_gate(super::super::files::At::Read, gate);
    engine_guarded(dir, guard, Default::default()).await
}

/// [`engine`]'s, librqbit's storage through `guard`, its trash waiting while `trash` is held.
async fn engine_guarded(dir: &Path, guard: super::super::files::Guard, trash: super::super::trash::Gate) -> Engine {
    let config = Config {
        data_dir: dir.join("data"),
        download_dir: dir.join("downloads"),
        listen_port: 0,
        upnp: false,
        offline: true,
        size_check_every: SIZE_CHECK_EVERY,
        poll_every: super::super::POLL_EVERY,
        trash: super::super::trash::into(dir.join("trash"), Default::default(), trash),
        speed_limits: Default::default(),
        seeding: Default::default(),
    };
    let add_into: super::super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
        Box::pin(super::super::actor::add_into(session, id, add, destination, download_dir, guard))
    });
    Engine::start_with(config, add_into, guard, super::super::Looks::real()).await.unwrap()
}

/// librqbit's torrent for torrent `id` now, as how many times the actor added it, if librqbit has it:
/// another once it's taken out and added again.
async fn librqbit_handle(engine: &Engine, id: u64) -> Option<u32> {
    engine.inspect(move |a| a.torrents.get(&id).filter(|t| t.lib.torrent().is_some()).map(|t| t.added)).await
}

/// What librqbit has of each file of torrent `id`, as its details say.
async fn progress(engine: &Engine, id: u64) -> Vec<u64> {
    engine.details(id).await.unwrap().files.iter().map(|f| f.progress_bytes).collect()
}

/// Emptying the Trash of a file a Move to Trash took closes it, so nothing holds its space, and its
/// torrent is added back without its pieces: Seeding throughout, with none of the file, the other
/// file whole. librqbit's saved pieces say so too, and it trusts them, sampling a few: no check. Held
/// while it samples them, it's still Seeding.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_emptied_from_the_trash_is_closed_and_its_pieces_let_go_of_while_the_row_stays_as_it_was() {
    let dir = temp();
    let (reads, gate) = (Arc::new(std::sync::atomic::AtomicUsize::new(0)), Arc::new(super::super::files::Gate::default()));
    // Opened however the test ends, so librqbit's reads never wait for good.
    let _opens = Opens(gate.clone());
    let engine = engine_reading(dir.path(), reads.clone(), gate.clone()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let hash = find(&engine, id).await.info_hash;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    let item = in_trash(dir.path(), &name(&engine, id, 0).await);
    let open = open_on(&item);
    assert_eq!(open(), 1, "librqbit has it open where it went");
    assert_eq!(saved_pieces(dir.path(), &hash), 8);
    reads.store(0, Ordering::SeqCst);
    gate.hold(true);
    std::fs::remove_file(&item).unwrap();
    // Added back, librqbit samples the pieces it had: a poll or two meet it at it.
    for _ in 0..1000 {
        if gate.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting(), "added back, sampling");
    assert_eq!(open(), 0, "closed: nothing holds the emptied file's space");
    for _ in 0..120 {
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "no other status while it's sampled");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    gate.hold(false);
    let mut closed = false;
    for _ in 0..1000 {
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "no other status while it's let go of");
        closed |= open() == 0;
        if closed && let_go(&engine, id).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(closed, "nothing holds the emptied file's space");
    assert!(let_go(&engine, id).await, "added back");
    assert_eq!(saved_pieces(dir.path(), &hash), 4, "the other file's only");
    let read = reads.load(Ordering::SeqCst);
    // Sampling: the other file's first piece, and of its 3 others the first and maybe more. A check
    // reads the emptied file once (failing, it's skipped) and each of the other's 4: 6 at least.
    assert!((1..=4).contains(&read), "a few of the other file's 4 pieces sampled, not the torrent checked: {read} reads");
    for _ in 0..100 {
        if progress(&engine, id).await == [0, 65536] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    assert_eq!(find(&engine, id).await.status, Status::Seeding);
    engine.stop().await;
}

/// After the let-go, ⌘Z of the Move to Trash is as ADR 0008 says: the file still in the Trash goes
/// back with its priority and the pieces it had (no check), and the emptied one stays Off. With
/// every file it moved emptied, it has nothing to put back, and is refused.
#[tokio::test(flavor = "multi_thread")]
async fn an_undo_after_the_let_go_puts_back_what_is_still_in_the_trash_and_the_emptied_file_stays_off() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let pack = dir.path().join("downloads/pack");
    let (first, second) = (name(&engine, id, 0).await, name(&engine, id, 1).await);
    // Both moved, one emptied.
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    std::fs::remove_file(in_trash(dir.path(), &first)).unwrap();
    until_let_go(&engine, id, 0).await;
    engine.undo().await.unwrap();
    assert!(!pack.join(&first).exists() && pack.join(&second).exists(), "the second is back; the first is gone for good");
    let priorities: Vec<Priority> = engine.details(id).await.unwrap().files.iter().map(|f| f.priority).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Normal]);
    for _ in 0..30 {
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "the second's pieces kept, the first Off: no check");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    // The other one moved, and emptied: nothing to put back.
    engine.command(id, TorrentCommand::TrashFiles { files: vec![1] }).await.unwrap();
    std::fs::remove_file(in_trash(dir.path(), &second)).unwrap();
    until_let_go(&engine, id, 1).await;
    assert_eq!(progress(&engine, id).await, [0, 0]);
    let refused = engine.undo().await;
    assert!(matches!(&refused, Err(Failure::Refused(proto::Refusal::Undo { why: proto::CantUndo::NotInTrash { .. }, .. }))), "{refused:?}");
    assert_eq!(stack(&engine).await, []);
    engine.stop().await;
}

/// Waits up to 10 seconds for torrent `id` to have let go of its file `file` gone from the Trash:
/// listed, nothing left to let go of, and none of the file had.
async fn until_let_go(engine: &Engine, id: u64, file: usize) {
    for _ in 0..1000 {
        let listed = engine.listing().await.iter().any(|t| t.id == id);
        if listed && let_go(engine, id).await && progress(engine, id).await[file] == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("torrent {id} never let go: {:?}", progress(engine, id).await);
}

/// An engine in `dir` with a torrent Seeding, whose first file a Move to Trash took and the Trash
/// then lost, killed at `at` as it lets go of it: the torrent's id and info hash.
async fn crashed_letting_go(dir: &Path, at: CrashAt) -> (u64, String) {
    let engine = engine(dir).await;
    let source = valid_pack(dir).await;
    let id = add(&engine, &dir.join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let hash = find(&engine, id).await.info_hash;
    let crashed = Arc::new(AtomicBool::new(false));
    let flag = crashed.clone();
    engine.inspect(move |a| a.crash_letting_go = Some((at, flag))).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    std::fs::remove_file(in_trash(dir, &name(&engine, id, 0).await)).unwrap();
    for _ in 0..1000 {
        if crashed.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(crashed.load(Ordering::SeqCst), "{at:?}: killed there");
    (id, hash)
}

/// A crash once the file is closed, as the torrent is taken out of librqbit, or once it's out
/// before it's added back: the next start finishes the let-go before it adds the torrent. What
/// librqbit saved of its pieces, if it still had it, is cleared of the emptied file's; taken out,
/// they're put back from what was kept, without them. Either way it's listed under its id from the
/// first snapshot, with the other file whole, nothing is left to let go of, and what was kept aside
/// goes.
#[tokio::test(flavor = "multi_thread")]
async fn a_let_go_a_crash_cuts_short_is_finished_at_the_next_start() {
    for at in [CrashAt::Closing, CrashAt::TakingOut, CrashAt::AddingBack] {
        let dir = temp();
        let (id, hash) = crashed_letting_go(dir.path(), at).await;
        // Which way it's finished: librqbit still has its pieces saved, or it's out.
        let saved = super::super::bitv::path(&dir.path().join("data/session"), &hash).exists();
        assert_eq!(saved, at != CrashAt::AddingBack, "{at:?}");
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let engine = engine_counting(dir.path(), reads.clone()).await;
        assert_ne!(find(&engine, id).await.status, Status::FetchingInfo, "{at:?}: listed as it was from the start");
        // Before it handles anything, nothing is left to let go of.
        let left = engine.inspect(move |a| a.torrents.get(&id).map(|t| t.record.letting_go.clone())).await;
        assert_eq!(left, Some(letgo::State::Holding), "{at:?}: finished as it starts");
        until_let_go(&engine, id, 0).await;
        until(&engine, id, |s| *s == Status::Seeding).await;
        assert_eq!(saved_pieces(dir.path(), &hash), 4, "{at:?}: the other file's only");
        for _ in 0..100 {
            if progress(&engine, id).await == [0, 65536] {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(progress(&engine, id).await, [0, 65536], "{at:?}");
        let stashes = std::fs::read_dir(dir.path().join("data/undo")).map_or(0, |d| d.flatten().filter(|e| e.path().is_dir()).count());
        assert_eq!(stashes, 0, "{at:?}: nothing kept aside any more");
        assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }], "{at:?}: the move is still undoable");
        // librqbit trusted what it saved, sampling a few of the other file's 4 pieces: it didn't find
        // one of the emptied file's it can't read, and check the torrent (6 reads at least).
        let reads = reads.load(Ordering::SeqCst);
        assert!((1..=4).contains(&reads), "{at:?}: {reads} reads");
        engine.stop().await;
    }
}

/// A torrent a crash took out of librqbit to let go of a file, whose destination isn't connected at
/// the next start, isn't lost: it's listed from the start, Destination unavailable, out of librqbit
/// as any torrent whose destination is away. Once its folder is back it's checked, and has the
/// other file whole. One that can't be added back at all (librqbit can't read what it saved of its
/// pieces) is listed as an error, its record and what was kept kept, for Resume or Check to try
/// again.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_a_crash_took_out_isnt_lost_when_it_cant_be_added_back_at_the_next_start() {
    let dir = temp();
    let (id, _) = crashed_letting_go(dir.path(), CrashAt::AddingBack).await;
    let (downloads, away) = (dir.path().join("downloads"), dir.path().join("downloads-away"));
    std::fs::rename(&downloads, &away).unwrap();
    let engine = engine(dir.path()).await;
    assert!(matches!(find(&engine, id).await.status, Status::DestinationUnavailable { .. }), "listed from the start");
    until_let_go_of_record(&engine, id).await;
    assert!(matches!(find(&engine, id).await.status, Status::DestinationUnavailable { .. }));
    std::fs::rename(&away, &downloads).unwrap();
    until(&engine, id, |s| *s == Status::Seeding).await;
    for _ in 0..100 {
        if progress(&engine, id).await == [0, 65536] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    assert_eq!(stack(&engine).await, [proto::UndoAction::TrashFiles { count: 1 }]);
    engine.stop().await;
    drop(engine);

    // What librqbit saves of it unreadable: listed as an error, nothing lost.
    let dir = temp();
    let (id, hash) = crashed_letting_go(dir.path(), CrashAt::AddingBack).await;
    std::fs::create_dir_all(super::super::bitv::path(&dir.path().join("data/session"), &hash)).unwrap();
    let engine = super::engine(dir.path()).await;
    until(&engine, id, |s| matches!(s, Status::Error { .. })).await;
    let kept = engine.inspect(move |a| a.torrents.get(&id).map(|t| t.record.letting_go.clone())).await;
    let Some(letgo::State::Stashed { stash, .. }) = kept else { panic!("its record kept: {kept:?}") };
    assert!(stash.dir.is_dir(), "what was kept of it kept");
    engine.stop().await;
}

/// Waits up to 10 seconds for torrent `id`'s record to have nothing left to let go of.
async fn until_let_go_of_record(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if engine.inspect(move |a| a.torrents.get(&id).is_some_and(|t| t.record.letting_go == letgo::State::Holding)).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("torrent {id} never let go");
}

/// S1 of #223 still holds once a file is let go of: the torrent's peers drop once, as it's taken out
/// of librqbit, connect again, and get the rest. Here the engine seeds, and finds a slow leecher
/// through a tracker that names it, again as it's added back and started; the leecher is part way
/// when the Trash is emptied, and gets every byte of the other file. Were the file only closed,
/// the leecher's next request for one of its pieces would drop it (#223's probe: nothing more).
#[tokio::test(flavor = "multi_thread")]
async fn peers_connect_again_once_a_file_gone_from_the_trash_is_let_go_of_and_get_the_rest() {
    use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, CreateTorrentOptions, ListenerOptions, SessionOptions};
    let dir = temp();
    let leecher = librqbit::Session::new_with_opts(
        dir.path().join("leecher-session"),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
            ratelimits: librqbit::limits::LimitsConfig { download_bps: std::num::NonZeroU32::new(256 << 10), upload_bps: None },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let announce = tracker(leecher.listen_addr().expect("the leecher listens").port());
    let pack = dir.path().join("downloads/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let size = 512 * 1024;
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(pack.join(name), (0..size).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let options = CreateTorrentOptions { name: None, trackers: vec![announce], piece_length: Some(16384) };
    let created = librqbit::create_torrent(&pack, options, &librqbit::spawn_utils::BlockingSpawner::new(1)).await.unwrap();
    let bytes = created.as_bytes().unwrap().to_vec();
    let leech = dir.path().join("leech");
    std::fs::create_dir_all(&leech).unwrap();
    let opts = AddTorrentOptions { output_folder: Some(leech.to_str().unwrap().to_owned()), ..Default::default() };
    let Ok(AddTorrentResponse::Added(_, leeching)) = leecher.add_torrent(AddTorrent::from_bytes(bytes.clone()), Some(opts)).await else {
        panic!("added")
    };
    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(bytes), true).await.unwrap().id;
    wait_for(&engine, id, Status::Done { reached: None }).await;
    let (first, second) = (name(&engine, id, 0).await, name(&engine, id, 1).await);
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    // Part way: it's getting both files from the engine, the first read where it went.
    for _ in 0..1000 {
        if leeching.stats().progress_bytes >= 128 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let part = leeching.stats().progress_bytes;
    assert!((128 * 1024..2 * size as u64).contains(&part), "part way: {part}");
    let item = in_trash(dir.path(), &first);
    let open = open_on(&item);
    std::fs::remove_file(&item).unwrap();
    // Let go of, as far as the engine goes: closed, nothing left in its record. What follows is the
    // peer's.
    let holding = || engine.inspect(move |a| a.torrents.get(&id).is_some_and(|t| t.record.letting_go == letgo::State::Holding));
    for _ in 0..1000 {
        if open() == 0 && holding().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(open() == 0 && holding().await, "let go of");
    let whole = |s: &librqbit::TorrentStats| s.file_progress.get(1) == Some(&(size as u64));
    for _ in 0..3000 {
        if whole(&leeching.stats()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(whole(&leeching.stats()), "the peer got the rest: {:?}", leeching.stats());
    assert_eq!(std::fs::read(leech.join(&second)).unwrap(), std::fs::read(pack.join(&second)).unwrap());
    // And stays: nothing it asks for drops it again.
    for _ in 0..20 {
        let live = leeching.stats().live.map_or(0, |l| l.snapshot.peer_stats.live);
        assert_eq!(live, 1, "connected: {:?}", leeching.stats());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    engine.stop().await;
    leecher.stop().await;
}

/// What's kept of a neighbour still in the Trash outlasts the let-go of another file. Three files of
/// 20000 bytes in 16 KiB pieces: the first shares piece 1 with the middle one, the middle one piece 2
/// with the last. The middle one goes to the Trash missing its part of piece 2, which comes while it's
/// there (the last needs it) and is kept aside; then the first, which is emptied from the Trash. Its
/// pieces go, piece 1 too, and librqbit, sampling piece 2 as the middle file's, reads it whole with
/// what's kept: no check, Seeding throughout. ⌘Z then puts the middle one back whole with what was
/// kept, and piece 1 comes again.
#[tokio::test(flavor = "multi_thread")]
async fn what_is_kept_of_a_neighbour_in_the_trash_outlasts_letting_go_of_another_file() {
    let dir = temp();
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5), ("c.bin", 7)] {
        std::fs::write(seeded.join(name), (0..20000usize).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, hash, _) = seed(dir.path(), &seeded, 16384).await;
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let engine = engine_counting(dir.path(), reads.clone()).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let pack = dir.path().join("downloads/pack");
    let (first, middle) = (name(&engine, id, 0).await, name(&engine, id, 1).await);
    // The middle file without its part of piece 2 (from 32768, 12768 bytes into it), found so.
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    let mut bytes = std::fs::read(pack.join(&middle)).unwrap();
    bytes[12768..].fill(0);
    std::fs::write(pack.join(&middle), &bytes).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| *s == Status::Paused).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    let kept = super::super::undo::parts_dir(&dir.path().join("data")).join(&hash).join("1");
    assert!(std::fs::read_dir(&kept).is_ok_and(|mut d| d.next().is_some()), "the middle file's part of piece 2 is kept");
    reads.store(0, Ordering::SeqCst);
    std::fs::remove_file(in_trash(dir.path(), &first)).unwrap();
    for _ in 0..1000 {
        assert_eq!(find(&engine, id).await.status, Status::Seeding, "piece 2 read whole with what's kept: no check");
        if let_go(&engine, id).await && saved_pieces(dir.path(), &hash) == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(saved_pieces(dir.path(), &hash), 2, "pieces 2 and 3: the first file's went, the one it shares too");
    // Sampled: piece 2 (from the middle file, read with what's kept, and the last) and piece 3. Read
    // without what's kept, piece 2 is broken, and librqbit checks the torrent: 7 reads.
    let read = reads.load(Ordering::SeqCst);
    assert!((1..=3).contains(&read), "sampled, not checked: {read} reads");
    for _ in 0..50 {
        assert_eq!(find(&engine, id).await.status, Status::Seeding);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(std::fs::read_dir(&kept).is_ok_and(|mut d| d.next().is_some()), "still kept");
    engine.undo().await.unwrap();
    // Piece 1 comes again from the seeder; the middle file is whole.
    for _ in 0..1000 {
        if find(&engine, id).await.status == Status::Seeding
            && std::fs::read(pack.join(&middle)).ok() == std::fs::read(seeded.join(&middle)).ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(std::fs::read(pack.join(&middle)).unwrap(), std::fs::read(seeded.join(&middle)).unwrap(), "back whole");
    wait_for(&engine, id, Status::Seeding).await;
    engine.stop().await;
}

/// A file emptied from the Trash while its torrent is checked is closed at once, so nothing holds its
/// space, and the torrent stays in librqbit, as the check reads what's there. The check, finding
/// none of it, drops its pieces: that ends the let-go, and the torrent isn't taken out again.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_emptied_during_a_check_is_closed_and_the_check_drops_its_pieces_without_taking_the_torrent_out() {
    let dir = temp();
    let (reads, gate) = (Arc::new(std::sync::atomic::AtomicUsize::new(0)), Arc::new(super::super::files::Gate::default()));
    let _opens = Opens(gate.clone());
    let engine = engine_reading(dir.path(), reads, gate.clone()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    let item = in_trash(dir.path(), &name(&engine, id, 0).await);
    let open = open_on(&item);
    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    for _ in 0..1000 {
        if gate.waiting() && open() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting() && open() == 1, "checking, the file opened where it went");
    let checked = librqbit_handle(&engine, id).await;
    std::fs::remove_file(&item).unwrap();
    for _ in 0..1000 {
        if open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), 0, "closed while it's checked");
    for _ in 0..60 {
        assert_eq!(find(&engine, id).await.status, Status::Checking);
        assert_eq!(librqbit_handle(&engine, id).await, checked, "not taken out of librqbit while it's checked");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    gate.hold(false);
    until(&engine, id, |s| *s == Status::Seeding).await;
    until_let_go_of_record(&engine, id).await;
    assert_eq!(progress(&engine, id).await, [0, 65536], "the check dropped its pieces");
    for _ in 0..60 {
        assert_eq!(librqbit_handle(&engine, id).await, checked, "nor taken out after: nothing was left to let go of");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    engine.stop().await;
}

/// A file emptied from the Trash while a step runs on its torrent's lane (another Move to Trash,
/// held after its move) is closed at once, and the torrent is taken out of librqbit only once the
/// step is over.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_emptied_while_a_step_runs_on_its_lane_is_closed_and_let_go_of_once_the_step_is_over() {
    let dir = temp();
    let trash = super::super::trash::Gate::default();
    let _opens = OpensTrash(trash.clone());
    let engine = engine_guarded(dir.path(), super::super::files::Guard::default(), trash.clone()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    let item = in_trash(dir.path(), &name(&engine, id, 0).await);
    let open = open_on(&item);
    let before = librqbit_handle(&engine, id).await;
    super::super::trash::hold(&trash, true);
    let moving = engine.sent(id, TorrentCommand::TrashFiles { files: vec![1] }).await;
    held_at_the_gate(&trash).await;
    std::fs::remove_file(&item).unwrap();
    for _ in 0..1000 {
        if open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), 0, "closed while the step runs");
    for _ in 0..60 {
        assert_eq!(librqbit_handle(&engine, id).await, before, "not taken out of librqbit while a step runs on its lane");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    super::super::trash::hold(&trash, false);
    moving.await.unwrap();
    until_let_go(&engine, id, 0).await;
    assert_ne!(librqbit_handle(&engine, id).await, before, "taken out and added back once it's over");
    engine.stop().await;
}

/// B1 of the review of #242: letting go of a file never waits on librqbit's storage, which may wait
/// on a disk that doesn't answer. With a read of the file in hand, or a change to another of the
/// torrent's files, held at a gate as a stalled disk holds them, the Trash emptied: the engine goes on
/// answering. The file is closed at once beside a change; one being read, once the read is over.
#[tokio::test(flavor = "multi_thread")]
async fn letting_go_never_waits_on_what_librqbit_is_doing_with_the_files() {
    for at in [super::super::files::At::Change, super::super::files::At::Reading] {
        let dir = temp();
        let gate = Arc::new(super::super::files::Gate::default());
        let _opens = Opens(gate.clone());
        let guard = super::super::files::Guard::default().with_gate(at, gate.clone());
        let engine = engine_guarded(dir.path(), guard, Default::default()).await;
        let source = valid_pack(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
        let item = in_trash(dir.path(), &name(&engine, id, 0).await);
        // A check reads the file where it went, piece 0 first; as it ends, it sets the other's length.
        gate.hold(true);
        engine.command(id, TorrentCommand::Check).await.unwrap();
        for _ in 0..1000 {
            if gate.waiting() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(gate.waiting(), "{at:?}: held");
        let open = open_on(&item);
        std::fs::remove_file(&item).unwrap();
        let other = add(&engine, &dir.path().join("downloads"), magnet(HASH), true).await.unwrap().id;
        for _ in 0..40 {
            let answered = tokio::time::timeout(Duration::from_secs(2), engine.command(other, TorrentCommand::Resume)).await;
            assert!(answered.is_ok(), "{at:?}: the engine answers");
            if at == super::super::files::At::Change && open() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        match at {
            super::super::files::At::Change => assert_eq!(open(), 0, "closed beside the change"),
            _ => assert_eq!(open(), 1, "{at:?}: left open while it's read"),
        }
        gate.hold(false);
        for _ in 0..1000 {
            if open() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(open(), 0, "{at:?}: closed");
        until(&engine, id, |s| *s == Status::Seeding).await;
        until_let_go(&engine, id, 0).await;
        engine.stop().await;
    }
}

/// R2 of the review of #242: a torrent added back with pieces librqbit can't read (the other file,
/// set Off, deleted by hand meanwhile: librqbit's sampling fails only on a read, not on a wrong
/// hash) is checked in full, and its row says so: Checking (held while librqbit reads with nothing
/// saved, as it does then), then as the check found it.
#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_added_back_with_pieces_librqbit_cant_trust_shows_its_check() {
    let dir = temp();
    let gate = Arc::new(super::super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let saved: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let (held, at) = (gate.clone(), saved.clone());
    // Only a check in full waits: librqbit drops what it saved before one.
    let hook: super::super::trashed::Hook = Arc::new(move |now| {
        let unsaved = at.lock().unwrap().as_ref().is_some_and(|p| !p.exists());
        if now == super::super::files::At::Read && unsaved {
            held.wait();
        }
    });
    let engine = engine_guarded(dir.path(), super::super::files::Guard::default().with_hook(hook), Default::default()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let hash = find(&engine, id).await.info_hash;
    *saved.lock().unwrap() = Some(super::super::bitv::path(&dir.path().join("data/session"), &hash));
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    engine.command(id, TorrentCommand::SetPriority { files: vec![1], priority: Priority::Off }).await.unwrap();
    std::fs::remove_file(dir.path().join("downloads/pack").join(name(&engine, id, 1).await)).unwrap();
    gate.hold(true);
    std::fs::remove_file(in_trash(dir.path(), &name(&engine, id, 0).await)).unwrap();
    until(&engine, id, |s| *s == Status::Checking).await;
    gate.hold(false);
    until(&engine, id, |s| *s != Status::Checking).await;
    until_let_go(&engine, id, 0).await;
    assert_eq!(progress(&engine, id).await, [0, 0], "the check found neither");
    engine.stop().await;
}

/// An engine in `dir` started after a crash took a torrent out of librqbit as it let go of its first
/// file, whose add-back then fails (a folder where librqbit's saved pieces go): the torrent, an
/// error, kept aside; the engine; and the torrent's other file, still at its path.
async fn kept_in_error(dir: &Path, reads: Arc<std::sync::atomic::AtomicUsize>) -> (u64, Engine, PathBuf) {
    let (id, hash) = crashed_letting_go(dir, CrashAt::AddingBack).await;
    std::fs::create_dir_all(super::super::bitv::path(&dir.join("data/session"), &hash)).unwrap();
    let engine = engine_counting(dir, reads).await;
    until(&engine, id, |s| matches!(s, Status::Error { .. })).await;
    let pack = dir.join("downloads/pack");
    let other = ["a.bin", "b.bin"].iter().map(|n| pack.join(n)).find(|p| p.exists()).expect("the other file");
    // The trouble goes, for what follows.
    std::fs::remove_dir(super::super::bitv::path(&dir.join("data/session"), &hash)).unwrap();
    (id, engine, other)
}

/// A Delete of a torrent whose add-back failed after a crash moves its files to the Trash as any
/// Delete does, from what was kept of it, and ⌘Z brings it all back: the file, and the torrent with
/// the pieces it had, unchecked (a few read, as librqbit samples them).
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_of_a_torrent_whose_add_back_failed_moves_its_files_and_is_undone() {
    let dir = temp();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (id, engine, other) = kept_in_error(dir.path(), reads.clone()).await;
    engine.command(id, TorrentCommand::Delete).await.unwrap();
    assert!(!other.exists(), "moved");
    // Its folder whole, as it holds nothing else.
    in_trash(dir.path(), "pack");
    assert!(!engine.listing().await.iter().any(|t| t.id == id), "off the list");
    assert_eq!(stack(&engine).await.last(), Some(&proto::UndoAction::Delete));
    reads.store(0, Ordering::SeqCst);
    engine.undo().await.unwrap();
    assert!(other.exists(), "back at its path");
    until(&engine, id, |s| *s == Status::Seeding).await;
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    let read = reads.load(Ordering::SeqCst);
    assert!((1..=4).contains(&read), "back with the pieces it had, not checked: {read} reads");
    until_let_go_of_record(&engine, id).await;
    engine.stop().await;
}

/// A Remove of a torrent whose add-back failed after a crash is undone from what was kept of it,
/// not its magnet: it's back with the pieces it had, nothing downloaded again, unchecked.
#[tokio::test(flavor = "multi_thread")]
async fn a_remove_of_a_torrent_whose_add_back_failed_is_undone_from_what_was_kept() {
    let dir = temp();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (id, engine, other) = kept_in_error(dir.path(), reads.clone()).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    assert!(!engine.listing().await.iter().any(|t| t.id == id), "off the list");
    assert!(other.exists(), "its files stay");
    reads.store(0, Ordering::SeqCst);
    engine.undo().await.unwrap();
    until(&engine, id, |s| *s == Status::Seeding).await;
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    let read = reads.load(Ordering::SeqCst);
    assert!((1..=4).contains(&read), "back with the pieces it had, not checked: {read} reads");
    until_let_go_of_record(&engine, id).await;
    engine.stop().await;
}

/// #244's re-check, as #223's probe: a file deleted by hand is closed at once (its space freed); set
/// Off, its torrent seeds again. librqbit must offer none of its pieces then, or each peer that
/// asks for one fails to read and is dropped: it's let go of, as one gone from the Trash is. Here a
/// leecher finds the engine through a tracker, and gets every byte of the other file, and stays.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_deleted_by_hand_then_set_off_drops_no_peer() {
    use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, CreateTorrentOptions, ListenerOptions, SessionOptions};
    let dir = temp();
    let leecher = librqbit::Session::new_with_opts(
        dir.path().join("leecher-session"),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
            ratelimits: librqbit::limits::LimitsConfig { download_bps: std::num::NonZeroU32::new(256 << 10), upload_bps: None },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let announce = tracker(leecher.listen_addr().expect("the leecher listens").port());
    let pack = dir.path().join("downloads/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let size = 512 * 1024;
    for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
        std::fs::write(pack.join(name), (0..size).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let options = CreateTorrentOptions { name: None, trackers: vec![announce], piece_length: Some(16384) };
    let created = librqbit::create_torrent(&pack, options, &librqbit::spawn_utils::BlockingSpawner::new(1)).await.unwrap();
    let bytes = created.as_bytes().unwrap().to_vec();
    let leech = dir.path().join("leech");
    std::fs::create_dir_all(&leech).unwrap();
    let opts = AddTorrentOptions { output_folder: Some(leech.to_str().unwrap().to_owned()), ..Default::default() };
    let Ok(AddTorrentResponse::Added(_, leeching)) = leecher.add_torrent(AddTorrent::from_bytes(bytes.clone()), Some(opts)).await else {
        panic!("added")
    };
    // Size checks, and the looks with them, as seldom as the service's own: a torrent that ran before
    // its let-go would meet the peer first.
    let engine = super::engine_size_checking_every(dir.path(), Duration::from_secs(3)).await;
    let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(bytes), true).await.unwrap().id;
    wait_for(&engine, id, Status::Done { reached: None }).await;
    let (first, second) = (name(&engine, id, 0).await, name(&engine, id, 1).await);
    let deleted = pack.join(&first);
    let open = open_on(&deleted);
    std::fs::remove_file(&deleted).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    for _ in 0..2000 {
        if open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), 0, "closed, its space freed");
    engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: proto::Priority::Off }).await.unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    let whole = |s: &librqbit::TorrentStats| s.file_progress.get(1) == Some(&(size as u64));
    // Whenever it runs, librqbit offers none of the deleted file's pieces (its bitfield is every
    // piece it has, a file Off or not): a peer that asked for one would be dropped.
    let offered = || {
        engine.inspect(move |a| {
            let stats = a.torrents[&id].lib.torrent().map(|h| h.stats())?;
            matches!(stats.state, librqbit::TorrentStatsState::Live).then(|| stats.file_progress.first().copied().unwrap_or(0))
        })
    };
    for _ in 0..3000 {
        if let Some(had) = offered().await {
            assert_eq!(had, 0, "running, it offers pieces of the deleted file");
        }
        if whole(&leeching.stats()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(whole(&leeching.stats()), "the peer got the other file: {:?}", leeching.stats());
    assert_eq!(std::fs::read(leech.join(&second)).unwrap(), std::fs::read(pack.join(&second)).unwrap());
    // And stays: nothing it asks for drops it again.
    for _ in 0..20 {
        let live = leeching.stats().live.map_or(0, |l| l.snapshot.peer_stats.live);
        assert_eq!(live, 1, "connected: {:?}", leeching.stats());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    engine.stop().await;
}
