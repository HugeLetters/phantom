//! Undo end to end through `client` (#150), as `phantom undo` asks: a Remove is undone after the
//! service stopped and started again (the stack is kept in the data dir), the snapshot says what
//! would be undone, and an empty stack is refused. A Remove, not a Delete: the real service moves
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

    let removed = Undoable { action: UndoAction::Remove, id: added.id, name: "pack".into() };
    client.command(added.id, proto::TorrentCommand::Remove).unwrap();
    let snapshot = Client::open().unwrap().snapshot(Vec::new()).unwrap();
    assert_eq!(snapshot.undo.as_ref(), Some(&removed), "the snapshot says what Undo would undo");

    client.shutdown().unwrap();
    client.wait_closed().unwrap();
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();
    assert_eq!(client.undo().unwrap(), removed);
    let back = || Client::open().unwrap().snapshot(Vec::new()).unwrap();
    until(|| back().torrents.iter().any(checked));
    let listed = back();
    let back = listed.torrents.iter().find(|t| t.id == added.id).expect("back under its id");
    assert_eq!(back.content.map(|c| c.progress_bytes), Some(60_000), "with what it had");
    assert_eq!(listed.undo, None);
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
