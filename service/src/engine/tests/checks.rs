//! #374: a check writes nothing. Each add opens the files closed; only a start opens them for
//! writing, and only those still the ones the check read, the torrent's own to write.

use std::os::unix::fs::MetadataExt;

use super::*;

const PIECE: usize = 16384;

fn bytes(len: usize, by: u8) -> Vec<u8> {
    (0..len).map(|n| (n % 241) as u8 ^ by).collect()
}

/// What's at `path`: its inode, length, bytes and modification time; `None`: nothing.
fn as_it_is(path: &Path) -> Option<(u64, u64, Vec<u8>, i64, i64)> {
    let m = std::fs::symlink_metadata(path).ok()?;
    Some((m.ino(), m.len(), std::fs::read(path).ok()?, m.mtime(), m.mtime_nsec()))
}

/// A seeded pack of `a.bin` and `b.bin`, three pieces each.
async fn pack(dir: &Path) -> (Arc<librqbit::Session>, AddSource, Vec<u8>, Vec<u8>) {
    let seeded = dir.join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    let (a, b) = (bytes(3 * PIECE, 3), bytes(3 * PIECE, 5));
    std::fs::write(seeded.join("a.bin"), &a).unwrap();
    std::fs::write(seeded.join("b.bin"), &b).unwrap();
    let (seeder, torrent, _, _) = seed(dir, &seeded, PIECE as u32).await;
    (seeder, AddSource::TorrentFile(torrent), a, b)
}

/// Added stopped, then checked: a short file isn't grown and one not there isn't made, with its
/// folder; nothing is touched. Both come once it starts.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_of_a_stopped_torrent_changes_nothing_on_disk() {
    let dir = temp();
    let (_seeder, source, a, b) = pack(dir.path()).await;
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(downloads.join("pack")).unwrap();
    std::fs::write(downloads.join("pack/a.bin"), &a[..PIECE]).unwrap();
    let before = as_it_is(&downloads.join("pack/a.bin"));
    let engine = engine(dir.path()).await;
    let id = add(&engine, &downloads, source, true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    for step in ["added", "checked"] {
        assert_eq!(as_it_is(&downloads.join("pack/a.bin")), before, "{step}: the short one as it was");
        assert!(!downloads.join("pack/b.bin").exists(), "{step}: nothing made");
        assert_eq!(engine.details(id).await.unwrap().files.iter().map(|f| f.progress_bytes).sum::<u64>(), PIECE as u64);
        engine.command(id, TorrentCommand::Check).await.unwrap();
        until(&engine, id, |s| *s == Status::Paused).await;
    }
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert!(std::fs::read(downloads.join("pack/a.bin")).unwrap() == a);
    assert!(std::fs::read(downloads.join("pack/b.bin")).unwrap() == b);
    engine.stop().await;
}

/// A file longer than the torrent's, and one with another name too (a hard link) that isn't whole:
/// Missing files, after Check and Resume too, and neither is written. One whole with another name
/// seeds.
#[tokio::test(flavor = "multi_thread")]
async fn a_longer_file_and_an_incomplete_hard_link_are_missing_files_and_never_written() {
    let dir = temp();
    let (_seeder, source, a, b) = pack(dir.path()).await;
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(downloads.join("pack")).unwrap();
    let (theirs, mine) = (dir.path().join("theirs.bin"), dir.path().join("mine.bin"));
    std::fs::write(&theirs, [a.as_slice(), &[7; 10]].concat()).unwrap();
    std::fs::write(&mine, vec![9; b.len()]).unwrap();
    std::fs::hard_link(&theirs, downloads.join("pack/a.bin")).unwrap();
    std::fs::hard_link(&mine, downloads.join("pack/b.bin")).unwrap();
    let was = (as_it_is(&theirs), as_it_is(&mine));
    let engine = engine(dir.path()).await;
    let id = add(&engine, &downloads, source.clone(), false).await.unwrap().id;
    wait_for(&engine, id, Status::MissingFiles).await;
    for command in [TorrentCommand::Check, TorrentCommand::Resume] {
        engine.command(id, command.clone()).await.unwrap();
        tokio::time::sleep(SIZE_CHECK_EVERY * 4).await;
        wait_for(&engine, id, Status::MissingFiles).await;
        assert_eq!((as_it_is(&theirs), as_it_is(&mine)), was, "{command:?}: neither written");
    }
    engine.command(id, TorrentCommand::Remove).await.unwrap();

    // Whole, with another name: only read, it seeds.
    std::fs::remove_file(downloads.join("pack/a.bin")).unwrap();
    std::fs::remove_file(downloads.join("pack/b.bin")).unwrap();
    std::fs::write(&theirs, &a).unwrap();
    std::fs::write(&mine, &b).unwrap();
    std::fs::hard_link(&theirs, downloads.join("pack/a.bin")).unwrap();
    std::fs::hard_link(&mine, downloads.join("pack/b.bin")).unwrap();
    let id = add(&engine, &downloads, source, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    assert!(std::fs::read(&theirs).unwrap() == a && std::fs::read(&mine).unwrap() == b);
    engine.stop().await;
}

/// A person's file at the path of a file set Off: the piece it shares with a file the torrent gets
/// isn't written into it, but kept aside, and the other file completes.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_piece_isnt_written_into_a_persons_file_at_an_off_files_path() {
    let dir = temp();
    // 24576 and 40960 bytes: piece 1 is a.bin's end and b.bin's start.
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    let (a, b) = (bytes(24576, 3), bytes(40960, 5));
    std::fs::write(seeded.join("a.bin"), &a).unwrap();
    std::fs::write(seeded.join("b.bin"), &b).unwrap();
    let (_seeder, torrent, hash, _) = seed(dir.path(), &seeded, PIECE as u32).await;
    let downloads = dir.path().join("downloads");
    let engine = engine(dir.path()).await;
    let id = add(&engine, &downloads, AddSource::TorrentFile(torrent), true).await.unwrap().id;
    wait_for(&engine, id, Status::Paused).await;
    let off = index_of(&engine, id, "b.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![off], priority: Priority::Off }).await.unwrap();
    std::fs::create_dir_all(downloads.join("pack")).unwrap();
    std::fs::write(downloads.join("pack/b.bin"), vec![9; b.len()]).unwrap();
    let was = as_it_is(&downloads.join("pack/b.bin"));
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Seeding).await;
    assert!(std::fs::read(downloads.join("pack/a.bin")).unwrap() == a, "complete");
    assert_eq!(as_it_is(&downloads.join("pack/b.bin")), was, "the person's file as it was");
    assert!(super::super::parts::dir(&dir.path().join("data")).join(&hash).join(off.to_string()).is_file(), "kept aside");
    engine.stop().await;
}
