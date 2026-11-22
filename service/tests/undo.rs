//! Undo end to end through `client` (#150), as `phantom undo` asks: a Remove is undone after the
//! service stopped and started again (the stack is kept in the data dir), the snapshot says what
//! would be undone, and an empty stack is refused. A Remove of several in one request, as `phantom
//! remove a b` sends it, is one entry, and one Undo brings all of it back (#235). A Remove, not a Delete: the real service moves
//! files to the user's Trash, which tests leave alone (the engine's tests undo Deletes across a
//! restart, with a trash of their own). An example rather than generated: it's the real service and
//! its socket; the engine's generated tests check what an undo does.

mod fake;

use std::time::{Duration, Instant};

use client::{Client, Failure};
use proto::{AddSource, Refusal, UndoAction, Undoable};

#[test]
fn a_remove_is_undone_across_a_restart_of_the_service() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    let service = env!("CARGO_BIN_EXE_phantom-service");
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();

    // A complete torrent of one folder, made here.
    let folder = dir.path().join("pack");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.bin"), vec![7; 40_000]).unwrap();
    std::fs::write(folder.join("b.bin"), vec![9; 20_000]).unwrap();
    let options =
        librqbit::CreateTorrentOptions { name: None, trackers: vec!["http://127.0.0.1:1/announce".into()], piece_length: Some(16384) };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let torrent = rt.block_on(async {
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        librqbit::create_torrent(&folder, options, &spawner).await.unwrap()
    });
    let destination = client::absolute(dir.path()).unwrap();
    let added = client.add(AddSource::TorrentFile(torrent.as_bytes().unwrap().to_vec()), true, Some(destination)).unwrap();
    let checked = |t: &proto::TorrentInfo| t.id == added.id && t.status == proto::Status::Done { reached: None };
    until(|| Client::open().unwrap().snapshot(Vec::new()).unwrap().torrents.iter().any(checked));
    assert!(matches!(client.undo(), Err(Failure::Refused(Refusal::NothingToUndo))));

    let removed = Undoable { seq: 0, action: UndoAction::Remove, torrents: vec![proto::Named { id: added.id, name: "pack".into() }] };
    let unnumbered = |u: &Undoable| Undoable { seq: 0, ..u.clone() };
    client.command(added.id, proto::TorrentCommand::Remove).unwrap();
    let snapshot = Client::open().unwrap().snapshot(Vec::new()).unwrap();
    assert_eq!(snapshot.undo.iter().map(unnumbered).collect::<Vec<_>>(), [removed.clone()], "the snapshot says what Undo would undo");

    client.shutdown().unwrap();
    client.wait_closed().unwrap();
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();
    assert_eq!(unnumbered(&client.undo().unwrap().undo), removed);
    let back = || Client::open().unwrap().snapshot(Vec::new()).unwrap();
    until(|| back().torrents.iter().any(checked));
    let listed = back();
    let back = listed.torrents.iter().find(|t| t.id == added.id).expect("back under its id");
    assert_eq!(back.content.map(|c| c.progress_bytes), Some(60_000), "with what it had");
    assert!(listed.undo.is_empty());

    // Two more, removed with it in one request: one entry, one Undo for the three.
    let magnet = |n: u8| AddSource::Magnet(format!("magnet:?xt=urn:btih:{:040x}&dn=m{n}", 0xb0 + n as u64));
    let destination = client::absolute(dir.path()).unwrap();
    let ids: Vec<u64> =
        [added.id].into_iter().chain((0..2).map(|n| client.add(magnet(n), true, Some(destination.clone())).unwrap().id)).collect();
    assert!(client.commands(&ids, proto::TorrentCommand::Remove).unwrap().failed.is_empty(), "each removed");
    let snapshot = Client::open().unwrap().snapshot(Vec::new()).unwrap();
    assert!(snapshot.torrents.is_empty());
    assert_eq!(snapshot.undo.len(), 1, "one request, one entry: {:?}", snapshot.undo);
    let back: Vec<u64> = client.undo().unwrap().undo.torrents.iter().map(|t| t.id).collect();
    assert_eq!(back, ids, "one Undo, all three");
    until(|| Client::open().unwrap().snapshot(Vec::new()).unwrap().torrents.len() == 3);
    client.shutdown().unwrap();
}

fn until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        let listed = Client::open().unwrap().snapshot(Vec::new()).unwrap().torrents;
        assert!(Instant::now() < deadline, "never happened: {listed:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
