//! The notes of a torrent's files (#374, gate PR 5): which file at each path is the torrent's,
//! kept in its record. A torrent added back with the pieces librqbit saved is checked in full when
//! a file it gets isn't the one noted (#393), and an undo of a Move to Trash tells Phantom's own new
//! copy by its note, after a restart too.

use super::*;

/// Other bytes of `path`'s size, written by hand as a person would: a new file moved into place.
fn overwrite(dir: &Path, path: &Path) {
    let len = std::fs::metadata(path).unwrap().len() as usize;
    let new = dir.join("new.bin");
    std::fs::write(&new, vec![0xee; len]).unwrap();
    std::fs::rename(&new, path).unwrap();
}

async fn notes(engine: &Engine, id: u64) -> std::collections::BTreeMap<usize, super::super::notes::Note> {
    engine.inspect(move |a| a.torrents[&id].record.notes.clone()).await
}

/// #393: a file overwritten by hand, then another let go of (set Off and deleted), which adds the
/// torrent back with its saved pieces. librqbit's look at a few of them passes the overwritten
/// file, so it was still counted and offered; it isn't the one noted, so it's checked in full.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_overwritten_by_hand_is_checked_when_another_is_let_go_of() {
    let temp = temp();
    let dir = temp.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let (a, b) = (index_of(&engine, id, "a.bin").await, index_of(&engine, id, "b.bin").await);
    overwrite(dir, &file(dir, "b.bin"));
    engine.command(id, TorrentCommand::SetPriority { files: vec![a], priority: Priority::Off }).await.unwrap();
    std::fs::remove_file(file(dir, "a.bin")).unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    let details = engine.details(id).await.unwrap();
    assert_eq!(details.files[b].progress_bytes, 0, "checked: none of it is the torrent's");
    assert!(notes(&engine, id).await[&b].file.is(&super::super::notes::Identity::at(&file(dir, "b.bin")).unwrap()), "noted as read");
    engine.stop().await;
}

/// #393 at a start: a file overwritten by hand while the service was stopped is checked in full, not
/// trusted from the pieces librqbit saved. One left as it was is resumed without a check.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_overwritten_while_stopped_is_checked_at_the_start() {
    side_by_side(2, [false, true], |overwritten| async move {
        let temp = temp();
        let dir = temp.path();
        let engine = engine(dir).await;
        let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
        wait_for(&engine, id, Status::Seeding).await;
        let b = index_of(&engine, id, "b.bin").await;
        let noted = notes(&engine, id).await;
        assert_eq!(noted.len(), 3, "each file the check read");
        assert!(noted.values().all(|n| n.why == super::super::notes::Why::Matched));
        engine.stop().await;
        if overwritten {
            overwrite(dir, &file(dir, "b.bin"));
        }
        let engine = super::engine(dir).await;
        if overwritten {
            wait_for(&engine, id, Status::Downloading).await;
            let details = engine.details(id).await.unwrap();
            assert_eq!(details.files[b].progress_bytes, 0, "checked in full");
            assert!(details.files.iter().filter(|f| f.index != b).all(|f| f.progress_bytes == 2 * PIECE as u64), "the rest whole");
        } else {
            sampled(&engine, id).await;
            wait_for(&engine, id, Status::Seeding).await;
            assert_eq!(notes(&engine, id).await, noted, "kept across the restart");
            assert_eq!(engine.inspect(move |a| a.torrents[&id].lifecycle.phase.clone()).await, super::super::machine::Phase::Ready);
        }
        engine.stop().await;
    })
    .await;
}

/// The new copy of a trashed file set Normal again is Phantom's by its note, which outlasts a
/// restart: ⌘Z after one, with the torrent paused (so nothing is granted), still sends the new
/// copy to the Trash and puts the old one back.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_tells_the_new_copy_after_a_restart() {
    let temp = temp();
    let dir = temp.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let path = file(dir, "b.bin");
    let old = std::fs::read(&path).unwrap();
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Normal }).await.unwrap();
    until(&engine, id, |s| *s == Status::Downloading).await;
    assert!(eventually(10, || path.exists()).await, "made again at its path");
    for _ in 0..1000 {
        if notes(&engine, id).await.get(&b).is_some_and(|n| n.why == super::super::notes::Why::Made) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(notes(&engine, id).await[&b].why, super::super::notes::Why::Made);
    engine.command(id, TorrentCommand::Pause).await.unwrap();
    engine.stop().await;
    let engine = super::engine(dir).await;
    until(&engine, id, |s| *s == Status::Paused).await;
    engine.undo().await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), old, "the old copy is back");
    let trashed: Vec<PathBuf> = std::fs::read_dir(dir.join("trash")).unwrap().flatten().map(|e| e.path()).collect();
    assert_eq!(trashed.len(), 1, "the new copy is in the Trash: {trashed:?}");
    until(&engine, id, |s| *s != Status::Checking).await;
    assert_eq!(engine.details(id).await.unwrap().files[b].progress_bytes, 2 * PIECE as u64, "checked: the old copy is whole");
    engine.stop().await;
}

/// A file a person put where a trashed one was, after a restart: not Phantom's by any note, so the
/// undo is refused and the file stays.
#[tokio::test(flavor = "multi_thread")]
async fn the_undo_of_a_move_to_trash_refuses_a_persons_file_after_a_restart() {
    let temp = temp();
    let dir = temp.path();
    let engine = engine(dir).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let b = index_of(&engine, id, "b.bin").await;
    let path = file(dir, "b.bin");
    engine.command(id, TorrentCommand::TrashFiles { files: vec![b] }).await.unwrap();
    engine.stop().await;
    std::fs::write(&path, b"mine").unwrap();
    let engine = super::engine(dir).await;
    wait_for(&engine, id, Status::Seeding).await;
    assert!(engine.undo().await.is_err(), "refused");
    assert_eq!(std::fs::read(&path).unwrap(), b"mine", "the person's file stays");
    engine.stop().await;
}
