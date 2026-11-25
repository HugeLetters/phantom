//! #414: what Phantom says a torrent has of each file is the pieces librqbit has of it, whatever
//! librqbit's own count per file says. Both tests hold librqbit where that count goes wrong, and
//! check that it did: once librqbit counts right, they say so.

use super::super::files::Gate;
use super::*;

const PIECE_LENGTH: u32 = 32 << 10;
const A: u64 = 1 << 20;
const B: u64 = 256 << 10;

/// Holds librqbit, on the threads it's set on, between having a piece and counting it in its files:
/// it says then, at `trace`, that the piece is verified.
struct BeforeCounted(Arc<Gate>);

impl tracing::Subscriber for BeforeCounted {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.is_event() && metadata.target() == "librqbit::torrent_state::live" && *metadata.level() == tracing::Level::TRACE
    }

    fn event(&self, event: &tracing::Event<'_>) {
        struct Verified(bool);
        impl tracing::field::Visit for Verified {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0 |= field.name() == "message" && format!("{value:?}") == "successfully downloaded and verified";
            }
        }
        let mut verified = Verified(false);
        event.record(&mut verified);
        if verified.0 {
            self.0.wait();
        }
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A pack of `a.bin` and `b.bin`, seeded.
async fn seeded(dir: &Path) -> (Arc<librqbit::Session>, Vec<u8>) {
    let pack = dir.join("seed/pack");
    std::fs::create_dir_all(&pack).unwrap();
    let bytes = |len: u64, by: u32| (0..len as u32).map(|n| (n.wrapping_mul(by) >> 13) as u8).collect::<Vec<u8>>();
    std::fs::write(pack.join("a.bin"), bytes(A, 2_654_435_761)).unwrap();
    std::fs::write(pack.join("b.bin"), bytes(B, 2_246_822_519)).unwrap();
    let (seeder, torrent, _, _) = seed(dir, &pack, PIECE_LENGTH).await;
    (seeder, torrent)
}

/// What librqbit's own count says it has of each file, by name.
async fn counted(engine: &Engine, id: u64) -> Vec<u64> {
    engine.inspect(move |a| (**a.torrents[&id].lib.torrent().unwrap()).stats().file_progress).await
}

/// What clients see it has of each file, and what its record keeps.
async fn had(engine: &Engine, id: u64) -> (Vec<u64>, Vec<u64>) {
    let details = engine.details(id).await.unwrap();
    let kept = engine.inspect(move |a| super::super::actor::progress(&a.torrents[&id])).await;
    (details.files.iter().map(|f| f.progress_bytes).collect(), kept)
}

async fn wait_until(what: &str, mut met: impl AsyncFnMut() -> bool) {
    for _ in 0..3000 {
        if met().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never: {what}");
}

fn all_had(mut had: (Vec<u64>, Vec<u64>)) -> bool {
    had.0.sort();
    had.1.sort();
    had == (vec![B, A], vec![B, A])
}

/// Paused between librqbit having a piece and counting it, librqbit never counts it.
#[test]
fn a_piece_had_as_the_torrent_is_paused_is_counted() {
    let gate = Arc::new(Gate::default());
    let dispatch = tracing::Dispatch::new(BeforeCounted(gate.clone()));
    // With one only, tracing asks just the thread that gets to an event first, maybe another test's,
    // whether anyone wants it.
    let _second = tracing::Dispatch::new(BeforeCounted(gate.clone()));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        // Only this test's threads.
        .on_thread_start(move || std::mem::forget(tracing::dispatcher::set_default(&dispatch)))
        .build()
        .unwrap();
    runtime.block_on(async move {
        let dir = temp();
        let (_seeder, torrent) = seeded(dir.path()).await;
        let engine = engine(dir.path()).await;
        let _opens = Opens(gate.clone());
        let downloads = dir.path().join("downloads");
        let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
        wait_for(&engine, id, Status::Paused).await;

        gate.hold(true);
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        wait_until("a piece is had and not yet counted", async || gate.waiting()).await;
        engine.command(id, TorrentCommand::Pause).await.unwrap();
        gate.hold(false);
        wait_until("it's not counted", async || !gate.waiting()).await;
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        until(&engine, id, |s| *s == Status::Seeding).await;

        let own: u64 = counted(&engine, id).await.iter().sum();
        assert_eq!(own, A + B - u64::from(PIECE_LENGTH), "librqbit's own count misses it: if not, ours needn't be made");
        wait_until("all of it is had", async || all_had(had(&engine, id).await)).await;
        let content = find(&engine, id).await.content.unwrap();
        assert_eq!((content.progress_bytes, content.size_bytes), (A + B, A + B));
        engine.stop().await;
    });
    runtime.shutdown_background();
}

/// A piece being hashed as its file is set Off and Normal again is got from another peer too, and
/// librqbit counts it twice.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_piece_got_twice_is_counted_once() {
    let dir = temp();
    let (seeder, one) = seeded(dir.path()).await;
    let pack = dir.path().join("seed/pack");
    let other = librqbit::SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: Some(librqbit::ListenerOptions { listen_addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }),
        ..Default::default()
    };
    let other = librqbit::Session::new_with_opts(dir.path().join("other-session"), other).await.unwrap();
    let opts = librqbit::AddTorrentOptions { overwrite: true, output_folder: Some(pack.to_str().unwrap().into()), ..Default::default() };
    other.add_torrent(librqbit::AddTorrent::from_bytes(one), Some(opts)).await.unwrap();
    let trackers = [&seeder, &other].map(|s| tracker(s.listen_addr().unwrap().port())).to_vec();
    let opts = librqbit::CreateTorrentOptions { piece_length: Some(PIECE_LENGTH), trackers, ..Default::default() };
    let torrent = librqbit::create_torrent(&pack, opts, &librqbit::spawn_utils::BlockingSpawner::new(2)).await.unwrap();
    let torrent = torrent.as_bytes().unwrap().to_vec();
    let gate = Arc::new(Gate::default());
    let first = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Only the first read once armed: the hash of the first piece got.
    let guard = super::super::files::Guard::default().with_hook(Arc::new({
        let (gate, first) = (gate.clone(), first.clone());
        move |at| {
            if at == super::super::files::At::Read && first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                gate.wait();
            }
        }
    }));
    let _opens = Opens(gate.clone());
    let engine = engine_guarded(dir.path(), guard).await;
    let downloads = dir.path().join("downloads");
    let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;

    gate.hold(true);
    first.store(true, std::sync::atomic::Ordering::SeqCst);
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_until("a piece is being hashed", async || gate.waiting()).await;
    // Its seeder's connection waits with the hash: the other sends all of it, that piece too.
    for files in [vec![0], vec![1]] {
        for priority in [Priority::Off, Priority::Normal] {
            engine.command(id, TorrentCommand::SetPriority { files: files.clone(), priority }).await.unwrap();
        }
    }
    until(&engine, id, |s| *s == Status::Seeding).await;
    gate.hold(false);

    wait_until("librqbit's own count has it twice: if not, ours needn't be made", async || {
        counted(&engine, id).await.iter().sum::<u64>() == A + B + u64::from(PIECE_LENGTH)
    })
    .await;
    // As kept, then as clients see it, at the polls since.
    let now = had(&engine, id).await;
    assert!(all_had((now.1.clone(), now.1.clone())), "{now:?}");
    wait_until("all of it is had, once", async || all_had(had(&engine, id).await)).await;
    // Nor is a whole file, as of a size it can't have, Missing files at the size checks.
    tokio::time::sleep(3 * SIZE_CHECK_EVERY).await;
    assert_eq!(find(&engine, id).await.status, Status::Seeding);
    engine.stop().await;
}
