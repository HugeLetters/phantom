//! Which of a torrent's files Phantom has open (#374): nothing of a torrent librqbit doesn't have, so
//! its drive can be ejected; the same ones across a pause; those at their paths once it's added again.

use super::super::trashed::open_on;
use super::*;

const NAMES: [&str; 3] = ["a.bin", "b.bin", "c.bin"];

/// How often each of the files is open, at its path in `pack` now.
fn opens(pack: &Path) -> impl Fn() -> [usize; 3] + use<> {
    let open = NAMES.map(|name| open_on(&pack.join(name)));
    move || open.each_ref().map(|open| open())
}

/// Waits for librqbit, which lets go of a torrent a little after it's asked to.
async fn become_open(open: impl Fn() -> [usize; 3], want: usize, what: &str) {
    for _ in 0..1000 {
        if open() == [want; 3] {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(open(), [want; 3], "{what}");
}

async fn seeding(dir: &Path) -> (Engine, u64) {
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    (engine, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn each_file_is_open_once_across_pauses_and_checks_and_not_after_a_remove() {
    let dir = temp();
    let (engine, id) = seeding(dir.path()).await;
    let open = opens(&dir.path().join("downloads/pack"));
    become_open(&open, 1, "seeding").await;
    for _ in 0..3 {
        engine.command(id, TorrentCommand::Pause).await.unwrap();
        until(&engine, id, |s| matches!(s, Status::Done { .. })).await;
        become_open(&open, 1, "paused").await;
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        wait_for(&engine, id, Status::Seeding).await;
        become_open(&open, 1, "resumed").await;
    }
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    become_open(&open, 1, "checked").await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    become_open(&open, 0, "removed").await;
    engine.stop().await;
}

/// Taken out while a check reads it: the check ends later, and leaves nothing open either.
#[tokio::test(flavor = "multi_thread")]
async fn no_file_is_open_after_a_remove_during_a_check() {
    let dir = temp();
    let gate = Arc::new(super::super::files::Gate::default());
    let engine = engine_gated(dir.path(), &gate).await;
    let _opens = Opens(gate.clone());
    let id = add(&engine, &dir.path().join("downloads"), complete_torrent(dir.path()).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let open = opens(&dir.path().join("downloads/pack"));
    gate.hold(true);
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Checking).await;
    engine.command(id, TorrentCommand::Remove).await.unwrap();
    gate.hold(false);
    become_open(&open, 0, "removed").await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_moved_torrents_files_are_open_once_where_they_went() {
    let dir = temp();
    let (engine, id) = seeding(dir.path()).await;
    let to = dir.path().join("elsewhere");
    std::fs::create_dir(&to).unwrap();
    let before = opens(&dir.path().join("downloads/pack"));
    let to = AbsolutePath::new(to).unwrap();
    engine.command(id, TorrentCommand::Move { to: to.clone(), clash: None, name: None }).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    // A rename on one volume: the same files.
    let after = opens(&to.join("pack"));
    become_open(&after, 1, "moved").await;
    assert_eq!(before(), [1; 3]);
    engine.stop().await;
}

/// As a drive that's ejected: its folder goes, and nothing keeps a file on it open; back, the files are
/// opened again.
#[tokio::test(flavor = "multi_thread")]
async fn no_file_is_open_while_the_destination_is_away() {
    for paused in [false, true] {
        let dir = temp();
        let (engine, id) = seeding(dir.path()).await;
        if paused {
            engine.command(id, TorrentCommand::Pause).await.unwrap();
            until(&engine, id, |s| matches!(s, Status::Done { .. })).await;
        }
        let (here, away) = (dir.path().join("downloads"), dir.path().join("away"));
        let open = opens(&here.join("pack"));
        std::fs::rename(&here, &away).unwrap();
        until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
        become_open(&open, 0, "away").await;
        std::fs::rename(&away, &here).unwrap();
        until(&engine, id, |s| if paused { matches!(s, Status::Done { .. }) } else { *s == Status::Seeding }).await;
        become_open(&open, 1, "back").await;
        engine.stop().await;
    }
}

/// A file replaced by hand while the torrent is paused, by another of its size: the one open stays so,
/// across a Resume; a Check opens, reads and judges the one there, and has none of its pieces.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_replaced_by_hand_while_paused_is_opened_at_the_next_check() {
    let dir = temp();
    let (engine, id) = seeding(dir.path()).await;
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    until(&engine, id, |s| matches!(s, Status::Done { .. })).await;
    let was = open_on(&file(dir.path(), "b.bin"));
    let theirs = vec![9u8; FULL as usize];
    std::fs::write(dir.path().join("theirs"), &theirs).unwrap();
    std::fs::rename(dir.path().join("theirs"), file(dir.path(), "b.bin")).unwrap();
    let now = open_on(&file(dir.path(), "b.bin"));
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert_eq!((was(), now()), (1, 0), "resumed");
    engine.command(id, TorrentCommand::Check).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    assert_eq!((was(), now()), (0, 1), "checked");
    let had: Vec<u64> = files_on(&engine, id).await.into_iter().map(|f| f.1).collect();
    assert_eq!(had, [FULL, 0, FULL]);
    assert_eq!(std::fs::read(file(dir.path(), "b.bin")).unwrap(), theirs, "a check changes none of it");
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_file_is_open_after_a_stop_and_each_once_after_the_next_start() {
    let dir = temp();
    let (engine, id) = seeding(dir.path()).await;
    let open = opens(&dir.path().join("downloads/pack"));
    engine.stop().await;
    drop(engine);
    become_open(&open, 0, "stopped").await;
    let engine = super::engine(dir.path()).await;
    wait_for(&engine, id, Status::Seeding).await;
    become_open(&open, 1, "started").await;
    engine.stop().await;
}
