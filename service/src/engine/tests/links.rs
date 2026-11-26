//! #267: a symbolic link at a torrent file's path is never the torrent's file. Phantom never
//! follows it to make, write, resize or read a file: the file is missing, and what the link points
//! to (always inside the test's temp dir) stays as it was, or not there.

use std::os::unix::fs::MetadataExt;

use super::*;

/// What's at `path` itself, not following a link: its bytes and modification time, or `None`.
fn as_it_is(path: &Path) -> Option<(Vec<u8>, i64, i64)> {
    let m = std::fs::symlink_metadata(path).ok()?;
    Some((std::fs::read(path).ok()?, m.mtime(), m.mtime_nsec()))
}

/// Puts a link at `b.bin`'s path, to `target`, in place of the file.
fn link_b(dir: &Path, target: &Path) {
    std::fs::remove_file(file(dir, "b.bin")).unwrap();
    std::os::unix::fs::symlink(target, file(dir, "b.bin")).unwrap();
}

fn still_a_link(dir: &Path, target: &Path) {
    let at = file(dir, "b.bin");
    assert!(std::fs::symlink_metadata(&at).unwrap().file_type().is_symlink(), "the link stays");
    assert_eq!(std::fs::read_link(&at).unwrap(), target, "pointing where it did");
}

/// The torrent's status once librqbit is done with what it was asked (not checking, not adding)
/// and two size checks have run since, the second one starting after the first was done: one that
/// found it missing, if it was to be.
async fn settled(engine: &Engine, id: u64) -> Status {
    until(engine, id, |s| !matches!(s, Status::Checking { .. } | Status::FetchingInfo)).await;
    let checks = engine.inspect(|a| a.size_checks).await;
    while engine.inspect(|a| a.size_checks).await < checks + 2 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    find(engine, id).await.status
}

/// A dangling link at a file's path: the file is missing, as with nothing there, through an add
/// (librqbit makes the files it gets), a check, a Resume and a restart (the start's size check);
/// nothing is ever made where the link points.
#[tokio::test(flavor = "multi_thread")]
async fn a_dangling_link_at_a_files_path_has_nothing_made_where_it_points() {
    let dir = temp();
    let source = complete_torrent(dir.path()).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("target.bin");
    link_b(dir.path(), &target);

    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    assert_eq!(settled(&engine, id).await, Status::MissingFiles, "added");
    assert!(std::fs::symlink_metadata(&target).is_err(), "added: nothing made where the link points");
    let b = index_of(&engine, id, "b.bin").await;
    assert!(engine.details(id).await.unwrap().files[b].missing, "b.bin is missing");
    for command in [TorrentCommand::Check, TorrentCommand::Resume] {
        let _ = engine.command(id, command.clone()).await;
        assert_eq!(settled(&engine, id).await, Status::MissingFiles, "{command:?}");
        assert!(std::fs::symlink_metadata(&target).is_err(), "{command:?}: nothing made where the link points");
    }
    engine.stop().await;

    let engine = super::engine(dir.path()).await;
    assert_eq!(settled(&engine, id).await, Status::MissingFiles, "after a restart");
    assert!(std::fs::symlink_metadata(&target).is_err(), "after a restart: nothing made where the link points");
    still_a_link(dir.path(), &target);
    // The link gone, a check makes the file at its path, as for any file missing.
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    engine.command(id, TorrentCommand::Check).await.unwrap();
    until(&engine, id, |s| matches!(s, Status::Downloading)).await;
    assert!(std::fs::symlink_metadata(file(dir.path(), "b.bin")).unwrap().file_type().is_file(), "made at its path");
    engine.stop().await;
}

/// A link to a file at a file's path: the file is missing, the one the link points to never the
/// torrent's, whatever it holds: neither read as it (the torrent's own bytes count for nothing),
/// nor resized or written (bytes and modification time as they were), through an add, a check, a
/// restart, and a link put there by hand while the torrent seeds.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_to_a_file_at_a_files_path_never_has_that_file_read_or_written() {
    for valid in [true, false] {
        let dir = temp();
        let source = complete_torrent(dir.path()).await;
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = elsewhere.join("target.bin");
        // The torrent's own bytes, or a few of someone else's, which a length set would change.
        let b_bytes = std::fs::read(file(dir.path(), "b.bin")).unwrap();
        let bytes = if valid { b_bytes.clone() } else { b"someone else's".to_vec() };
        std::fs::write(&target, &bytes).unwrap();
        let was = as_it_is(&target);
        link_b(dir.path(), &target);

        let engine = engine(dir.path()).await;
        let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
        assert_eq!(settled(&engine, id).await, Status::MissingFiles, "valid {valid}: added");
        let b = index_of(&engine, id, "b.bin").await;
        let shown = &engine.details(id).await.unwrap().files[b];
        assert_eq!((shown.missing, shown.progress_bytes), (true, 0), "valid {valid}: b.bin is missing, none of it had");
        let _ = engine.command(id, TorrentCommand::Check).await;
        assert_eq!(settled(&engine, id).await, Status::MissingFiles, "valid {valid}: checked");
        assert_eq!(as_it_is(&target), was, "valid {valid}: the file the link points to, as it was");
        engine.stop().await;

        let engine = super::engine(dir.path()).await;
        assert_eq!(settled(&engine, id).await, Status::MissingFiles, "valid {valid}: after a restart");
        assert_eq!(as_it_is(&target), was, "valid {valid}: after a restart, the file the link points to as it was");
        still_a_link(dir.path(), &target);

        // Put back, it seeds; a link put there by hand while it does is missing too.
        std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
        std::fs::write(file(dir.path(), "b.bin"), &b_bytes).unwrap();
        engine.command(id, TorrentCommand::Check).await.unwrap();
        wait_for(&engine, id, Status::Seeding).await;
        link_b(dir.path(), &target);
        wait_for(&engine, id, Status::MissingFiles).await;
        let _ = engine.command(id, TorrentCommand::Check).await;
        assert_eq!(settled(&engine, id).await, Status::MissingFiles, "valid {valid}: linked while seeding, checked");
        assert_eq!(as_it_is(&target), was, "valid {valid}: linked while seeding, the file the link points to as it was");
        engine.stop().await;
    }
}

/// A dangling link in place of the torrent's own folder: a link to a folder on the way is
/// followed (where the files go is the person's choice), but a folder isn't made through one, so
/// nothing is made where it points, through an add and a check.
#[tokio::test(flavor = "multi_thread")]
async fn a_dangling_link_in_place_of_a_torrents_folder_has_nothing_made_where_it_points() {
    let dir = temp();
    let source = complete_torrent(dir.path()).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("pack");
    std::fs::remove_dir_all(dir.path().join("downloads/pack")).unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("downloads/pack")).unwrap();

    let engine = engine(dir.path()).await;
    let id = add(&engine, &dir.path().join("downloads"), source, false).await.unwrap().id;
    settled(&engine, id).await;
    let _ = engine.command(id, TorrentCommand::Check).await;
    settled(&engine, id).await;
    assert!(std::fs::symlink_metadata(&target).is_err(), "nothing made where the link points: {:?}", find(&engine, id).await.status);
    assert!(std::fs::symlink_metadata(dir.path().join("downloads/pack")).unwrap().file_type().is_symlink(), "the link stays");
    engine.stop().await;
}

/// An Off file with a link at its path, sharing a piece with a wanted file: the piece's write
/// can't go through the link, so the torrent is in Error (Resume tries again, as for a folder at
/// the path), and what the link points to stays as it was (bytes and modification time).
#[tokio::test(flavor = "multi_thread")]
async fn an_off_file_with_a_link_at_its_path_is_never_written_through_it() {
    let dir = temp();
    // 24576 and 40960 bytes, in pieces of 16384: piece 1 is a.bin's end and b.bin's start.
    let seeded = dir.path().join("seed/pack");
    std::fs::create_dir_all(&seeded).unwrap();
    for (name, len, byte) in [("a.bin", 24576usize, 3u8), ("b.bin", 40960, 5)] {
        std::fs::write(seeded.join(name), (0..len).map(|n| (n % 241) as u8 ^ byte).collect::<Vec<u8>>()).unwrap();
    }
    let (_seeder, torrent, _, _) = seed(dir.path(), &seeded, 16384).await;
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("target.bin");
    std::fs::write(&target, vec![7u8; 40960]).unwrap();
    let was = as_it_is(&target);

    let engine = engine(dir.path()).await;
    let id = add_made(&engine, &dir.path().join("downloads"), AddSource::TorrentFile(torrent), true).await.unwrap().id;
    let b = index_of(&engine, id, "b.bin").await;
    engine.command(id, TorrentCommand::SetPriority { files: vec![b], priority: Priority::Off }).await.unwrap();
    // Made empty at the add, as any file a torrent gets: a link in its place.
    std::fs::remove_file(file(dir.path(), "b.bin")).unwrap();
    std::os::unix::fs::symlink(&target, file(dir.path(), "b.bin")).unwrap();

    for resume in ["Resume", "Resume again"] {
        engine.command(id, TorrentCommand::Resume).await.unwrap();
        until(&engine, id, |s| matches!(s, Status::Error { .. })).await;
        // A write that must not happen is nothing to wait on: a short fixed wait for it to.
        tokio::time::sleep(SIZE_CHECK_EVERY).await;
        assert!(matches!(find(&engine, id).await.status, Status::Error { .. }), "{resume}: still Error");
        assert_eq!(as_it_is(&target), was, "{resume}: the file the link points to, as it was");
        still_a_link(dir.path(), &target);
    }
    engine.stop().await;
}
