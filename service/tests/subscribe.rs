//! Snapshots pushed to a subscriber, end to end through `client` (#112): one at once, one after a
//! change, the details of what it watches once it asks, on through a restart of the engine (a new
//! epoch, on the same stream), and the end of the stream when the service stops. An example rather than generated: it's the real service and its socket; the
//! engine's generated tests check what the snapshots hold.

mod fake;

use std::time::{Duration, Instant};

use client::{Client, Failure};
use proto::{AddSource, Refusal, Setting};

#[test]
fn a_subscriber_gets_a_snapshot_at_once_then_one_per_change_with_what_it_watches() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    let service = env!("CARGO_BIN_EXE_phantom-service");
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();

    let mut snapshots = Client::open().unwrap().subscribe(Vec::new()).unwrap();
    // A stream that stops sending fails the test rather than hanging it.
    snapshots.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let first = snapshots.receive().unwrap();
    assert!(first.torrents.is_empty(), "at once, before any change: {first:?}");

    // A torrent of one file made here, stopped, with a tracker nobody runs.
    let folder = dir.path().join("pack");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.bin"), vec![7; 40_000]).unwrap();
    let options =
        librqbit::CreateTorrentOptions { name: None, trackers: vec!["http://127.0.0.1:1/announce".into()], piece_length: Some(16384) };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let torrent = rt.block_on(async {
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        librqbit::create_torrent(&folder, options, &spawner).await.unwrap()
    });
    let destination = client::absolute(dir.path()).unwrap();
    let added = client.add(AddSource::TorrentFile(torrent.as_bytes().unwrap().to_vec()), true, Some(destination)).unwrap();

    let listed = until(&mut snapshots, |s| s.torrents.iter().any(|t| t.id == added.id));
    assert!(listed.version > first.version, "a change counts up");
    assert!(listed.details.is_empty(), "no details of what it doesn't watch");

    let mut watcher = snapshots.watcher().unwrap();
    watcher.watch(vec![added.id]).unwrap();
    let watched = until(&mut snapshots, |s| s.details.iter().any(|d| d.id == added.id));
    assert_eq!(watched.details.len(), 1, "only what it watches");
    assert_eq!(watched.watching, [added.id], "built for what it watches");
    assert_eq!(watched.epoch, first.epoch, "one run of the engine");

    // The same again is answered at once (within the read timeout), built for it.
    watcher.watch(vec![added.id]).unwrap();
    let again = snapshots.receive().unwrap();
    assert!((again.epoch, again.version) >= (watched.epoch, watched.version) && again.watching == [added.id], "{again:?}");

    // A new UPnP setting restarts the engine: the stream goes on, a new epoch, as it was.
    let upnp = client.settings().unwrap().upnp;
    client.set_setting(Setting::Upnp(Some(!upnp))).unwrap();
    let restarted = until(&mut snapshots, |s| s.epoch != watched.epoch);
    assert!(restarted.epoch > watched.epoch, "a later run's epoch is larger: {} after {}", restarted.epoch, watched.epoch);
    assert!(restarted.torrents.iter().any(|t| t.id == added.id), "with the torrent");
    let watched_again = until(&mut snapshots, |s| s.details.iter().any(|d| d.id == added.id));
    assert_eq!(watched_again.watching, [added.id], "and what it watches");

    client.shutdown().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match snapshots.receive() {
            Ok(_) => assert!(Instant::now() < deadline, "the stream never ended"),
            Err(Failure::Refused(Refusal::Stopping)) | Err(Failure::Error(_)) => break,
            Err(other) => panic!("ended with {other:?}"),
        }
    }
}

/// The first snapshot that's `wanted`, within a few seconds.
fn until(snapshots: &mut client::Subscription, wanted: impl Fn(&proto::Snapshot) -> bool) -> proto::Snapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = snapshots.receive().unwrap();
        if wanted(&snapshot) {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "no such snapshot in time: {snapshot:?}");
    }
}
