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
    // As it's added back (#273): none of the file, though librqbit's last word had all of it.
    for _ in 0..1000 {
        if engine.inspect(move |a| matches!(a.torrents[&id].lib, Lib::Resuming(_))).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(progress(&engine, id).await, [0, 65536], "while it's sampled, what it's added back with");
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
        until_added_back(&engine, id).await;
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

/// Waits up to 10 seconds for [`let_go`]: librqbit is done sampling the pieces it saved of torrent
/// `id`. Its row is its record's until then.
async fn until_added_back(engine: &Engine, id: u64) {
    for _ in 0..1000 {
        if let_go(engine, id).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("torrent {id} never added back");
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
    // In place: cut short even for a moment, a size check could find it missing.
    let file = std::fs::File::options().write(true).open(pack.join(&middle)).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&file, &[0; 20000 - 12768], 12768).unwrap();
    drop(file);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| *s == Status::Paused).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    let kept = super::super::parts::dir(&dir.path().join("data")).join(&hash).join("1");
    assert!(kept.is_file(), "the middle file's part of piece 2 is kept");
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
    assert!(kept.is_file(), "still kept");
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
/// set Off, cut short by hand meanwhile: librqbit's sampling fails only on a read, not on a wrong
/// hash; one deleted is let go of too, #276) is checked in full, and its row says so: Checking (held while librqbit reads with nothing
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
    std::fs::File::options()
        .write(true)
        .open(dir.path().join("downloads/pack").join(name(&engine, id, 1).await))
        .unwrap()
        .set_len(0)
        .unwrap();
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
    until_added_back(&engine, id).await;
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
    until_added_back(&engine, id).await;
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

/// What librqbit itself has of each file of torrent `id`, the pieces it offers peers: none while it
/// doesn't have it.
async fn librqbit_has(engine: &Engine, id: u64) -> Vec<u64> {
    engine.inspect(move |a| a.torrents[&id].lib.torrent().map(|h| h.stats().file_progress).unwrap_or_default()).await
}

/// Waits up to 10 seconds for librqbit to have torrent `id` (a start adds it after its first
/// snapshot), then what it has of each file.
async fn librqbit_has_once_added(engine: &Engine, id: u64) -> Vec<u64> {
    for _ in 0..1000 {
        let has = librqbit_has(engine, id).await;
        if !has.is_empty() {
            return has;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("librqbit never had torrent {id}");
}

/// #276: a file set Off, then moved out of its folder by hand (Finder's Move to Trash, emptied or
/// not) or deleted, is let go of: librqbit has none of its pieces, so it neither counts nor offers
/// them, and its row shows none of it. librqbit had it open, and read it through that.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_then_moved_away_by_hand_is_let_go_of() {
    for deleted in [false, true] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let source = valid_pack(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let hash = find(&engine, id).await.info_hash;
        engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
        assert_eq!(librqbit_has(&engine, id).await[0], 65536, "librqbit has it all, set Off");
        let path = dir.path().join("downloads/pack").join(name(&engine, id, 0).await);
        let finders = dir.path().join("finders-trash");
        std::fs::create_dir_all(&finders).unwrap();
        // Held at the let-go's gate: found gone, it shows none of it before librqbit lets go.
        engine.inspect(|a| a.letting_go_held = true).await;
        match deleted {
            false => std::fs::rename(&path, finders.join("a.bin")).unwrap(),
            true => std::fs::remove_file(&path).unwrap(),
        }
        for _ in 0..1000 {
            if engine.inspect(move |a| a.torrents[&id].record.letting_go != letgo::State::Holding).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(progress(&engine, id).await, [0, 65536], "found gone, its row shows none of it at once");
        assert_eq!(librqbit_has(&engine, id).await[0], 65536, "librqbit lets go once the gate opens");
        engine.inspect(|a| a.letting_go_held = false).await;
        for _ in 0..1000 {
            if librqbit_has(&engine, id).await.first() == Some(&0) && let_go(&engine, id).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(librqbit_has(&engine, id).await, [0, 65536], "librqbit offers none of it (deleted: {deleted})");
        assert_eq!(progress(&engine, id).await, [0, 65536], "its row shows none of it");
        assert_eq!(saved_pieces(dir.path(), &hash), 4, "the other file's only");
        assert_eq!(find(&engine, id).await.status, Status::Seeding);
        engine.stop().await;
    }
}

/// #276 review: a file set Off and deleted while the service was stopped is let go of at the start,
/// before librqbit is given the pieces it saved, not at the first size check: with that an hour
/// away, librqbit has none of the file, and what it saved doesn't either.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_and_deleted_while_the_service_was_stopped_is_let_go_of_at_the_start() {
    let dir = temp();
    let source = valid_pack(dir.path()).await;
    let (id, hash, path) = {
        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let hash = find(&engine, id).await.info_hash;
        engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
        let path = dir.path().join("downloads/pack").join(name(&engine, id, 0).await);
        engine.stop().await;
        (id, hash, path)
    };
    std::fs::remove_file(&path).unwrap();
    let engine = engine_size_checking_every(dir.path(), Duration::from_secs(3600)).await;
    assert_eq!(saved_pieces(dir.path(), &hash), 4, "the other file's pieces only, before any size check");
    assert_eq!(librqbit_has_once_added(&engine, id).await[0], 0, "librqbit has none of the file it was added with");
    assert_eq!(progress(&engine, id).await, [0, 65536]);
    engine.stop().await;
}

/// #276: a file set Off and not at its path, set Normal again, is made there and downloaded from
/// nothing: the torrent isn't Missing files, which is for a file it gets that vanished. Whether a
/// Move to Trash took it (librqbit read it in the Trash, for its undo) or it was deleted by hand
/// and let go of: librqbit has none of it either way.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_set_off_and_gone_set_normal_again_downloads_from_nothing() {
    for trashed in [true, false] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let source = valid_pack(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let path = dir.path().join("downloads/pack").join(name(&engine, id, 0).await);
        match trashed {
            true => engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap(),
            false => {
                engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Off }).await.unwrap();
                std::fs::remove_file(&path).unwrap();
                for _ in 0..1000 {
                    if librqbit_has(&engine, id).await.first() == Some(&0) && let_go(&engine, id).await {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        engine.command(id, TorrentCommand::SetPriority { files: vec![0], priority: Priority::Normal }).await.unwrap();
        for _ in 0..1000 {
            if librqbit_has(&engine, id).await.first() == Some(&0) && let_go(&engine, id).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(SIZE_CHECK_EVERY * 3).await;
        assert_eq!(find(&engine, id).await.status, Status::Downloading, "trashed: {trashed}");
        assert_eq!(librqbit_has(&engine, id).await, [0, 65536], "librqbit has none of it (trashed: {trashed})");
        let details = engine.details(id).await.unwrap();
        assert!(details.files[0].on_disk && path.exists(), "made at its path (trashed: {trashed})");
        assert_eq!(details.files[0].progress_bytes, 0);
        engine.stop().await;
    }
}

/// What librqbit itself counts of each file of torrent `id` (its stats), not what the row shows: a
/// file Off in the Trash shows 0 whatever librqbit has (#276).
async fn librqbit_progress(engine: &Engine, id: u64) -> Vec<u64> {
    engine.inspect(move |a| a.torrents[&id].lib.torrent().map(|h| h.stats().file_progress).unwrap_or_default()).await
}

/// Waits up to 10 seconds for librqbit's own count of torrent `id`'s file `file` to be `want`, the
/// torrent let go of what it's to let go of: the count it ends with.
async fn until_librqbit_counts(engine: &Engine, id: u64, file: usize, want: u64) -> Vec<u64> {
    let mut counted = Vec::new();
    for _ in 0..1000 {
        counted = librqbit_progress(engine, id).await;
        if counted.get(file) == Some(&want) && let_go(engine, id).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    counted
}

/// Writes other bytes of the same length at `path`, as an editor saving over the file does: a new
/// file renamed onto it, so librqbit's handle is to the one that lost its name.
fn replace_by_hand(path: &Path) {
    let new = path.with_extension("new");
    std::fs::write(&new, vec![9u8; 65536]).unwrap();
    std::fs::rename(&new, path).unwrap();
}

/// #268, found by the engine model: a file overwritten by hand, then sent to the Trash with Move to
/// Trash (the file librqbit has open isn't the one that goes), still had its pieces counted by
/// librqbit, and offered them, after the Move to Trash and after a restart. The row hides it (a file
/// Off in the Trash shows 0 %, #278), so it's librqbit's own count that's compared: none of the
/// replaced file's pieces, saved or counted, the other file's as they were.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_replaced_by_hand_then_moved_to_the_trash_has_none_of_its_pieces_counted_by_librqbit() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let hash = find(&engine, id).await.info_hash;
    let first = name(&engine, id, 0).await;
    assert_eq!(librqbit_progress(&engine, id).await, [65536, 65536]);
    replace_by_hand(&dir.path().join("downloads/pack").join(&first));
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    assert_eq!(until_librqbit_counts(&engine, id, 0, 0).await, [0, 65536], "librqbit counts none of the replaced file");
    assert_eq!(saved_pieces(dir.path(), &hash), 4, "nor has it saved any of it: the other file's only");
    assert_eq!(std::fs::read(in_trash(dir.path(), &first)).unwrap(), vec![9u8; 65536], "what went to the Trash is the file there, whole");

    // As a restart finds it.
    engine.stop().await;
    drop(engine);
    let engine = super::engine(dir.path()).await;
    sampled(&engine, id).await;
    assert_eq!(until_librqbit_counts(&engine, id, 0, 0).await, [0, 65536], "after a restart too");
    assert_eq!(saved_pieces(dir.path(), &hash), 4);
    engine.stop().await;
}

/// #268's question: the undo of that Move to Trash puts the file back with none of its pieces
/// counted (it holds the bytes written by hand, not the torrent's), whether the undo comes before
/// librqbit has let go of them or after; the other file, which it never touched, keeps its own.
/// The put-back file is the one that was written, and the torrent doesn't count it as whole.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_of_a_file_replaced_by_hand_puts_it_back_with_none_of_its_pieces_counted() {
    for at_once in [true, false] {
        let dir = temp();
        let engine = engine(dir.path()).await;
        let source = valid_pack(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let first = name(&engine, id, 0).await;
        let path = dir.path().join("downloads/pack").join(&first);
        replace_by_hand(&path);
        engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
        if !at_once {
            until_librqbit_counts(&engine, id, 0, 0).await;
        }
        engine.undo().await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), vec![9u8; 65536], "{at_once}: the file written by hand is back");
        let counted = until_librqbit_counts(&engine, id, 0, 0).await;
        assert_eq!(counted[0], 0, "{at_once}: none of the file put back is counted: {counted:?}");
        assert_eq!(counted[1], 65536, "{at_once}: the other file, whole where it went and back, is");
        // The listing follows librqbit's count a poll later.
        for _ in 0..100 {
            if progress(&engine, id).await[0] == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(progress(&engine, id).await, [0, 65536], "{at_once}: nor does the row show it");
        engine.stop().await;
    }
}

/// #289 review: a Move to Trash of a file replaced by hand closes librqbit's handle to the file it
/// had open (the one that lost its name) as it goes, not at the next look of the watch or the
/// add-back: both are held back here (the let-go, and the look an hour away).
#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_trash_closes_the_handle_to_a_file_replaced_by_hand_before_the_let_go() {
    let dir = temp();
    // No look of the watch (which closes a file with no name left too) comes meanwhile.
    let engine = engine_size_checking_every(dir.path(), Duration::from_secs(3600)).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let path = dir.path().join("downloads/pack").join(name(&engine, id, 0).await);
    let open = open_on(&path);
    assert!(open() > 0, "librqbit has the file open");
    // The file librqbit has open keeps another name (moved aside by hand), so it isn't one gone for
    // good, which the watch closes on its own.
    std::fs::hard_link(&path, dir.path().join("kept.bin")).unwrap();
    replace_by_hand(&path);
    engine.inspect(|a| a.letting_go_held = true).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0] }).await.unwrap();
    for _ in 0..500 {
        if open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), 0, "the handle to the file replaced is closed while the let-go waits");
    assert_eq!(librqbit_progress(&engine, id).await[0], 65536, "librqbit hasn't been added back yet");
    engine.inspect(|a| a.letting_go_held = false).await;
    engine.stop().await;
}

/// #289 review: an undo that comes before librqbit has let go of the replaced file's pieces still
/// counts them, so the undo asks a check (the put-back file isn't the one they were of); it reads
/// the file written by hand and finds none. Never counts a piece whose bytes it can't read: the
/// check is harmless, and with none asked librqbit would count the torrent's bytes as the file's.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_of_a_file_replaced_by_hand_before_the_let_go_checks_it() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = valid_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let path = dir.path().join("downloads/pack").join(name(&engine, id, 0).await);
    replace_by_hand(&path);
    engine.inspect(|a| a.letting_go_held = true).await;
    engine.command(id, TorrentCommand::TrashFiles { files: vec![0, 1] }).await.unwrap();
    engine.undo().await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![9u8; 65536], "the file written by hand is back");
    // The let-go is still held: only the undo's own check can have dropped the pieces.
    let mut counted = Vec::new();
    for _ in 0..1000 {
        counted = librqbit_progress(&engine, id).await;
        if counted.first() == Some(&0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(counted[0], 0, "the check found none of the file put back: {counted:?}");
    assert_eq!(counted[1], 65536, "the other file, never touched, keeps its own");
    engine.inspect(|a| a.letting_go_held = false).await;
    engine.stop().await;
}

/// A torrent of two files, 24576 and 40960 bytes, already in `downloads`, so adding it checks to
/// everything valid: piece 1 (16384..32768 with pieces of 16384) is shared by both.
async fn uneven_pack(dir: &Path) -> AddSource {
    let folder = dir.join("downloads/pack");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, len, byte) in [("a.bin", 24576usize, 1u8), ("b.bin", 40960, 2)] {
        std::fs::write(folder.join(name), (0..len).map(|n| (n % 251) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let options = librqbit::CreateTorrentOptions { name: None, trackers: vec![], piece_length: Some(16384) };
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
    let created = librqbit::create_torrent(&folder, options, &spawner).await.unwrap();
    AddSource::TorrentFile(created.as_bytes().unwrap().to_vec())
}

/// #289 review: a piece across the boundary of a replaced file and one that wasn't is let go of for
/// both (what librqbit has of it was read from the replaced file), so the neighbour loses just that
/// piece's bytes in it and keeps the rest.
#[tokio::test(flavor = "multi_thread")]
async fn a_piece_shared_with_a_file_replaced_by_hand_is_let_go_of_for_the_neighbour_too() {
    let dir = temp();
    let engine = engine(dir.path()).await;
    let source = uneven_pack(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let hash = find(&engine, id).await.info_hash;
    // librqbit lists them in its own order: the file of 24576 bytes is the one replaced.
    let counted = librqbit_progress(&engine, id).await;
    let small = counted.iter().position(|&n| n == 24576).expect("the small file");
    let big = 1 - small;
    assert_eq!(counted[big], 40960);
    let first = name(&engine, id, small).await;
    let new = dir.path().join("downloads/pack/new");
    std::fs::write(&new, vec![9u8; 24576]).unwrap();
    std::fs::rename(&new, dir.path().join("downloads/pack").join(&first)).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![small] }).await.unwrap();
    // The replaced file's pieces go with piece 1, which the neighbour shares: it loses the 8192
    // bytes of it that are its own, and keeps the rest.
    let counted = until_librqbit_counts(&engine, id, small, 0).await;
    assert_eq!(counted[big], 40960 - 8192, "{counted:?}");
    assert_eq!(saved_pieces(dir.path(), &hash), 2, "pieces 2 and 3 only");
    engine.stop().await;
}

/// #369: what a peer is sent of a file written by hand, then moved to the Trash: after its undo, only
/// what the undo's check read at its path (all of one written back as it was, though librqbit had none of it
/// before the move; none of one overwritten); after a move the trash refused, none, whatever is there.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_is_sent_of_a_file_written_by_hand_only_what_the_undo_of_its_move_to_trash_checked() {
    use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, CreateTorrentOptions, ListenerOptions, SessionOptions};
    const LEN: u64 = 65536;
    // (The bytes written are the torrent's, the trash refuses the file, what's had of it then.)
    for (valid, refused, want) in [(true, false, LEN), (false, false, 0), (true, true, 0)] {
        let case = format!("valid {valid}, refused {refused}");
        let dir = temp();
        let options = SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
            ..Default::default()
        };
        let leecher = librqbit::Session::new_with_opts(dir.path().join("leecher-session"), options).await.unwrap();
        let announce = tracker(leecher.listen_addr().expect("the leecher listens").port());
        let pack = dir.path().join("downloads/pack");
        std::fs::create_dir_all(&pack).unwrap();
        for (name, byte) in [("a.bin", 3u8), ("b.bin", 5)] {
            std::fs::write(pack.join(name), (0..LEN).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
        }
        let options = CreateTorrentOptions { name: None, trackers: vec![announce], piece_length: Some(16384) };
        let created = librqbit::create_torrent(&pack, options, &librqbit::spawn_utils::BlockingSpawner::new(1)).await.unwrap();
        let bytes = created.as_bytes().unwrap().to_vec();
        let leech = dir.path().join("leech");
        std::fs::create_dir_all(&leech).unwrap();
        let opts = AddTorrentOptions { output_folder: Some(leech.to_str().unwrap().to_owned()), ..Default::default() };
        let Ok(AddTorrentResponse::Added(_, leeching)) = leecher.add_torrent(AddTorrent::from_bytes(bytes.clone()), Some(opts)).await
        else {
            panic!("added")
        };
        // Added stopped with the file not there, so librqbit has none of it, then written by hand.
        let refusing = Arc::new(Mutex::new(Vec::new()));
        let engine = engine_with(dir.path(), SIZE_CHECK_EVERY, refusing.clone(), Default::default()).await;
        let (first, second) = (pack.join("a.bin"), pack.join("b.bin"));
        let original = std::fs::read(&first).unwrap();
        std::fs::remove_file(&first).unwrap();
        let id = add(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(bytes), true).await.unwrap().id;
        wait_for(&engine, id, Status::Paused).await;
        let file = if name(&engine, id, 0).await == "a.bin" { 0 } else { 1 };
        assert_eq!(librqbit_progress(&engine, id).await[file], 0, "{case}");
        let written = if valid { original } else { vec![9u8; LEN as usize] };
        let new = dir.path().join("new.bin");
        std::fs::write(&new, &written).unwrap();
        std::fs::rename(&new, &first).unwrap();
        if refused {
            refusing.lock().unwrap().push(first.clone());
            engine.command(id, TorrentCommand::TrashFiles { files: vec![file] }).await.unwrap_err();
        } else {
            engine.command(id, TorrentCommand::TrashFiles { files: vec![file] }).await.unwrap();
            engine.undo().await.unwrap();
        }
        assert_eq!(std::fs::read(&first).unwrap(), written, "{case}: what was written is at the path");
        let counted = until_librqbit_counts(&engine, id, file, want).await;
        assert_eq!((counted[file], counted[1 - file]), (want, LEN), "{case}");
        // The engine finds the leecher only now, as it starts.
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        for _ in 0..1500 {
            if leeching.stats().progress_bytes == want + LEN {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let stats = leeching.stats();
        assert_eq!((stats.file_progress[file], stats.file_progress[1 - file]), (want, LEN), "{case}");
        // Counted as received whatever its hash: nothing else was sent.
        assert_eq!(stats.live.expect("live").snapshot.fetched_bytes, want + LEN, "{case}");
        assert_eq!(std::fs::read(leech.join("b.bin")).unwrap(), std::fs::read(&second).unwrap(), "{case}");
        if want > 0 {
            assert_eq!(std::fs::read(leech.join("a.bin")).unwrap(), written, "{case}");
        }
        engine.stop().await;
        leecher.stop().await;
    }
}
