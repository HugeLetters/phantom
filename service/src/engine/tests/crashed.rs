//! #370: a service that crashes (`Engine::kill`: no stop, so nothing a stop saves) knows at its next
//! start which files were missing and what librqbit had of each, so another file at a missing
//! file's path isn't taken for the torrent's own and downloaded over, and the own one put back is.

use super::*;

/// What the engine was at as it ended.
#[derive(Debug, Clone, Copy, PartialEq)]
enum At {
    /// Seeding, every file there; the file goes while the service isn't running.
    Ready,
    /// As `Ready`, and what librqbit saved of its pieces is lost, as a power cut can.
    PiecesLost,
    /// As `Ready`, downloaded since its phase last changed: its record says it had nothing.
    Downloaded,
    /// Missing files.
    Missing,
    /// Its destination went, the file missing already.
    Away,
    /// As `Away`, and a start (and crash) with the destination away since.
    AwayTwice,
    /// The destination went with every file there; the file goes before it's back.
    AwayWhole,
    /// Checking a file put back.
    Checking,
    /// Its check cancelled, a file put back since it went.
    Unchecked,
    /// librqbit can't be given the torrent again for the check of a file put back.
    Error,
}

/// What's at the missing files' paths after the interruption, or during the check it cut short.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Put {
    Own,
    Other,
}

/// When it's put there: before the next start, or once it runs.
#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Before,
    After,
}

fn other() -> Vec<u8> {
    vec![0u8; 2 * PIECE]
}

/// A torrent of three files, `gone` of them missing, interrupted at `at`, then started with its
/// destination there and `put` at each missing path. Own files end Seeding; others are left as they
/// are, Missing files.
async fn interrupted(at: At, gone: &[&str], put: Put, when: When, off: bool, crash: bool) {
    let case = format!("{at:?}, {gone:?}, {put:?} {when:?}, c.bin Off {off}, crash {crash}");
    let dir = temp();
    let dir = dir.path();
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    let gate = Arc::new(super::super::files::Gate::default());
    let _opens = Opens(gate.clone());
    let engine = engine_gated(dir, &gate).await;
    let id = add(&engine, &here, complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    if off {
        let c = index_of(&engine, id, "c.bin").await;
        engine.command(id, TorrentCommand::SetPriority { files: vec![c], priority: Priority::Off }).await.unwrap();
    }
    let own: Vec<Vec<u8>> = gone.iter().map(|name| std::fs::read(file(dir, name)).unwrap()).collect();
    let remove = |base: &Path| gone.iter().for_each(|name| std::fs::remove_file(base.join("pack").join(name)).unwrap());
    let put_there = || {
        for (name, own) in gone.iter().zip(&own) {
            std::fs::write(file(dir, name), if put == Put::Own { own.clone() } else { other() }).unwrap();
        }
    };
    let is_away = |s: &Status| matches!(s, Status::DestinationUnavailable { .. });
    if !matches!(at, At::Ready | At::PiecesLost | At::Downloaded | At::AwayWhole) {
        remove(&here);
        wait_for(&engine, id, Status::MissingFiles).await;
    }
    match at {
        At::Ready | At::PiecesLost | At::Missing => {}
        At::Downloaded => {
            engine
                .inspect(move |a| {
                    a.torrents.get_mut(&id).unwrap().record.had = vec![0; 3];
                    a.save();
                })
                .await;
        }
        At::Away | At::AwayTwice | At::AwayWhole => {
            std::fs::rename(&here, &away).unwrap();
            until(&engine, id, is_away).await;
        }
        At::Checking | At::Unchecked => {
            gate.hold(true);
            put_there();
            wait_for(&engine, id, Status::Checking).await;
            if at == At::Unchecked {
                engine.command(id, TorrentCommand::CancelCheck).await.unwrap();
                wait_for(&engine, id, Status::MissingFiles).await;
            }
        }
        At::Error => {
            let hash = find(&engine, id).await.info_hash;
            // A folder where librqbit saves the pieces: the add fails.
            let pieces = super::super::bitv::path(&dir.join("data/session"), &hash);
            let _ = std::fs::remove_file(&pieces);
            std::fs::create_dir_all(pieces.join("x")).unwrap();
            put_there();
            until(&engine, id, |s| matches!(s, Status::Error { .. })).await;
            std::fs::remove_dir_all(&pieces).unwrap();
        }
    }
    let end = async |engine: Engine| match crash {
        // The session's stop waits for a check's read.
        true => {
            let killed = engine.kill();
            let open = async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                gate.hold(false);
            };
            tokio::join!(killed, open);
        }
        false => {
            gate.hold(false);
            engine.stop().await;
        }
    };
    end(engine).await;
    if at == At::AwayTwice {
        let engine = super::engine(dir).await;
        until(&engine, id, is_away).await;
        end(engine).await;
    }
    match at {
        At::Ready | At::Downloaded => remove(&here),
        At::PiecesLost => {
            remove(&here);
            for saved in std::fs::read_dir(dir.join("data/session")).unwrap().flatten() {
                if saved.path().extension().is_some_and(|e| e == "bitv") {
                    std::fs::remove_file(saved.path()).unwrap();
                }
            }
        }
        At::AwayWhole => remove(&away),
        _ => {}
    }
    if away.exists() {
        std::fs::rename(&away, &here).unwrap();
    }
    let placed = matches!(at, At::Checking | At::Unchecked | At::Error);
    if when == When::Before && !placed {
        put_there();
    }
    let engine = super::engine(dir).await;
    if when == When::After && !placed {
        wait_for(&engine, id, Status::MissingFiles).await;
        put_there();
    }
    match put {
        Put::Own => {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while find(&engine, id).await.status != Status::Seeding {
                assert!(std::time::Instant::now() < deadline, "{case}: own files refused, {:?}", find(&engine, id).await.status);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Put::Other => {
            // Long enough for a size check to see the files and a check to end.
            tokio::time::sleep(Duration::from_secs(3)).await;
            // A check a loaded machine is late with is let end: it's what's left of it that's judged.
            until(&engine, id, |s| *s != Status::Checking).await;
            assert_eq!(find(&engine, id).await.status, Status::MissingFiles, "{case}");
            for name in gone {
                assert_eq!(std::fs::read(file(dir, name)).unwrap(), other(), "{case}: {name} as it was put there");
            }
        }
    }
    engine.stop().await;
}

const EVERY: [At; 10] = [
    At::Ready,
    At::PiecesLost,
    At::Downloaded,
    At::Missing,
    At::Away,
    At::AwayTwice,
    At::AwayWhole,
    At::Checking,
    At::Unchecked,
    At::Error,
];
const B: &[&str] = &["b.bin"];
const TWO: &[&str] = &["a.bin", "b.bin"];

/// How many cases run at once. All of them (30) started as many engines together, and on a loaded
/// machine their checks took 6-18 s, past the 10 s waits and the 3 s a check had to end in. They
/// get there, they were only slow.
const WIDTH: usize = 6;

/// Runs the cases side by side, `WIDTH` at a time: each takes seconds, most of them waiting.
async fn each(cases: impl IntoIterator<Item = (At, &'static [&'static str], Put, When, bool, bool)>) {
    let mut running = tokio::task::JoinSet::new();
    let width = Arc::new(tokio::sync::Semaphore::new(WIDTH));
    for (at, gone, put, when, off, crash) in cases {
        let width = width.clone();
        running.spawn(async move {
            let _turn = width.acquire().await.unwrap();
            interrupted(at, gone, put, when, off, crash).await
        });
    }
    while let Some(done) = running.join_next().await {
        done.unwrap();
    }
}

/// The issue's steps.
#[tokio::test(flavor = "multi_thread")]
async fn another_file_put_where_one_was_missing_across_a_crash_while_its_destination_was_gone_isnt_downloaded_over() {
    interrupted(At::Away, B, Put::Other, When::After, false, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_file_put_where_one_was_missing_isnt_downloaded_over_whatever_a_crash_cut_short() {
    each(
        EVERY
            .into_iter()
            .flat_map(|at| [(B, false), (B, true), (TWO, false)].map(|(gone, off)| (at, gone, Put::Other, When::After, off, true))),
    )
    .await;
}

/// Those known missing before it: put there while the service wasn't running too.
#[tokio::test(flavor = "multi_thread")]
async fn another_file_put_there_before_the_start_isnt_downloaded_over() {
    each([At::Missing, At::Away, At::AwayTwice].map(|at| (at, B, Put::Other, When::Before, false, true))).await;
}

/// A clean stop that saved no more than a crash did.
#[tokio::test(flavor = "multi_thread")]
async fn another_file_put_where_one_was_missing_isnt_downloaded_over_after_a_stop() {
    let after = [At::Away, At::AwayTwice, At::AwayWhole, At::Checking, At::Error].map(|at| (at, B, Put::Other, When::After, false, false));
    each(after.into_iter().chain([(At::Away, B, Put::Other, When::Before, false, false)])).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persons_own_file_put_back_is_the_torrents_again_whatever_a_crash_cut_short() {
    each(EVERY.into_iter().flat_map(|at| {
        [(B, When::Before), (B, When::After), (TWO, When::After)].map(|(gone, when)| (at, gone, Put::Own, when, false, true))
    }))
    .await;
    interrupted(At::Away, B, Put::Own, When::Before, false, false).await;
}

/// A save that fails as a file goes missing is made up for at the next phase change: taking the
/// torrent out of librqbit then deletes its pieces, the only other place that's known.
#[tokio::test(flavor = "multi_thread")]
async fn a_save_that_failed_as_a_file_went_missing_is_made_up_for_before_its_pieces_go() {
    let dir = temp();
    let dir = dir.path();
    let (here, away) = (dir.join("downloads"), dir.join("away"));
    let engine = engine(dir).await;
    let id = add(&engine, &here, complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let path = file(dir, "b.bin");
    engine
        .inspect(move |a| {
            a.store.fail_next_save_after(0);
            std::fs::remove_file(&path).unwrap();
        })
        .await;
    wait_for(&engine, id, Status::MissingFiles).await;
    std::fs::rename(&here, &away).unwrap();
    until(&engine, id, |s| matches!(s, Status::DestinationUnavailable { .. })).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    engine.kill().await;
    drop(engine);
    std::fs::rename(&away, &here).unwrap();
    std::fs::write(file(dir, "b.bin"), other()).unwrap();
    let engine = super::engine(dir).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(find(&engine, id).await.status, Status::MissingFiles);
    engine.stop().await;
}

/// What the saved record of the only torrent says is missing, by file index.
fn saved_missing(dir: &Path) -> std::collections::BTreeMap<usize, super::super::store::Missing> {
    super::super::store::saved_in(&dir.join("data")).into_values().next().unwrap().missing
}

/// Seeding, then `b.bin` goes: Missing files, saved as that. The torrent's id and `b.bin`'s index.
async fn missing_b(engine: &Engine, dir: &Path) -> (u64, usize) {
    let id = add(engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(engine, id, Status::Seeding).await;
    std::fs::remove_file(file(dir, "b.bin")).unwrap();
    wait_for(engine, id, Status::MissingFiles).await;
    let b = index_of(engine, id, "b.bin").await;
    assert_eq!(saved_missing(dir).keys().copied().collect::<Vec<_>>(), [b], "saved as it was learnt");
    (id, b)
}

/// A Delete under way keeps what was missing: a crash then, or an Undo, brings the torrent back
/// knowing it.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_under_way_keeps_what_was_missing() {
    let dir = temp();
    let dir = dir.path();
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    let engine = engine_with(dir, SIZE_CHECK_EVERY, Default::default(), gate.clone()).await;
    let (id, b) = missing_b(&engine, dir).await;
    super::super::trash::hold(&gate, true);
    let deleting = tokio::spawn(engine.command_later(id, TorrentCommand::Delete));
    held_at_the_gate(&gate).await;
    assert_eq!(saved_missing(dir).keys().copied().collect::<Vec<_>>(), [b]);
    super::super::trash::hold(&gate, false);
    let _ = deleting.await;
    engine.stop().await;
}

/// A Move under way keeps what was missing, as a crash in it is the start's to judge.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_under_way_keeps_what_was_missing() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let (id, b) = missing_b(&engine, dir).await;
    let gate = super::super::trash::Gate::default();
    let _opens = OpensTrash(gate.clone());
    engine
        .inspect({
            let gate = gate.clone();
            move |a| a.move_hold = super::super::actor::moving::Hold { gate: Some(gate), copies: true, ..Default::default() }
        })
        .await;
    super::super::trash::hold(&gate, true);
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let answer = tokio::spawn(
        engine.command_later(id, TorrentCommand::Move { to: AbsolutePath::new(&elsewhere).unwrap(), clash: None, name: None }),
    );
    until(&engine, id, |s| matches!(s, Status::Moving { done, .. } if *done > 0)).await;
    assert_eq!(saved_missing(dir).keys().copied().collect::<Vec<_>>(), [b]);
    super::super::trash::hold(&gate, false);
    let _ = answer.await;
    engine.stop().await;
}

/// A Move that takes the file put at the missing one's new path ends with its files to be checked
/// there, none missing. That's saved as it ends, not only once librqbit has the torrent again: a
/// crash between would find it still missing the file.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_takes_the_file_put_at_the_new_path_saves_nothing_missing_as_it_ends() {
    let dir = temp();
    let dir = dir.path();
    let gate = Arc::new(super::super::files::Gate::default());
    let _held = Opens(gate.clone());
    let engine = engine_gated_at(dir, super::super::files::At::Read, &gate).await;
    let (id, _) = missing_b(&engine, dir).await;
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("pack")).unwrap();
    std::fs::write(elsewhere.join("pack/b.bin"), other()).unwrap();
    let to = AbsolutePath::new(&elsewhere).unwrap();
    let TorrentRefusal::Clash { files: found } =
        refusal(engine.command(id, TorrentCommand::Move { to: to.clone(), clash: None, name: None }).await).1
    else {
        panic!("a clash")
    };
    gate.hold(true);
    let answered = proto::Answered { choice: proto::Clash::UseExisting, found };
    let answer = tokio::spawn(engine.command_later(id, TorrentCommand::Move { to, clash: Some(answered), name: None }));
    for _ in 0..1000 {
        if gate.waiting() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(gate.waiting(), "librqbit's add waits at the gate: {:?}", find(&engine, id).await.status);
    assert_eq!(saved_missing(dir), Default::default());
    gate.hold(false);
    let _ = answer.await;
    engine.stop().await;
}

/// A clean stop in Missing files saves nothing of what was at the missing path: the next start
/// takes what's there then for what was there.
#[tokio::test(flavor = "multi_thread")]
async fn a_clean_stop_forgets_what_was_seen_at_a_missing_path() {
    let dir = temp();
    let dir = dir.path();
    let engine = engine(dir).await;
    let (_, b) = missing_b(&engine, dir).await;
    size_checks(&engine, 2).await;
    engine
        .inspect(move |a| assert_ne!(a.torrents.values().next().unwrap().record.missing[&b].seen, super::super::files::Seen::NotYet))
        .await;
    engine.stop().await;
    assert_eq!(saved_missing(dir)[&b].seen, super::super::files::Seen::NotYet);
}

/// How a Move into `dir`'s `elsewhere`, which has a file at `a.bin`'s path, uses it.
async fn move_using(engine: &Engine, dir: &Path, id: u64, choice: proto::Clash) -> Result<(), Failure> {
    let to = AbsolutePath::new(dir.join("elsewhere")).unwrap();
    let TorrentRefusal::Clash { files: found } =
        refusal(engine.command(id, TorrentCommand::Move { to: to.clone(), clash: None, name: None }).await).1
    else {
        panic!("a clash")
    };
    let answered = proto::Answered { choice, found };
    engine.command(id, TorrentCommand::Move { to, clash: Some(answered), name: None }).await
}

/// What each file of torrent `id` shows it has, by name.
async fn shown_had(engine: &Engine, id: u64) -> std::collections::BTreeMap<String, u64> {
    let files = engine.details(id).await.unwrap().files;
    files.into_iter().map(|f| (f.path.to_string_lossy().into_owned(), f.progress_bytes)).collect()
}

/// How the engine goes on from a Move that adopts `a.bin`, `b.bin` still missing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MovedThen {
    /// To another drive: every inode changes.
    Copied,
    Restart,
    Crash,
    /// A crash once the old files are gone, before the record says the Move is over.
    CrashInIt,
    /// Moved again, nothing there.
    MovedAgain,
    /// The torrent's own file, which a person took away, put back.
    OwnBack,
    /// `b.bin` set Off: nothing is missing.
    SetOff,
}

/// #406: `b.bin` is missing, a person's file at its path, as a Move uses another file at `a.bin`'s
/// new one. It's still Missing files, with nothing of `a.bin` had till it's checked, and the
/// person's file is as it was, whatever comes that isn't Resume or Check.
async fn moved_missing(then: MovedThen) {
    let dir = temp();
    let dir = dir.path();
    let gate = Arc::new(super::super::files::Gate::default());
    let _held = Opens(gate.clone());
    let mut engine = engine_gated_at(dir, super::super::files::At::Read, &gate).await;
    let id = add(&engine, &dir.join("downloads"), complete_torrent(dir).await, false).await.unwrap().id;
    wait_for(&engine, id, Status::Seeding).await;
    let (b, own) = (file(dir, "b.bin"), dir.join("own"));
    std::fs::rename(&b, &own).unwrap();
    wait_for(&engine, id, Status::MissingFiles).await;
    std::fs::write(&b, other()).unwrap();
    // Looked at, and told not to be the one that went.
    size_checks(&engine, 2).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let elsewhere = dir.join("elsewhere/pack");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("a.bin"), other()).unwrap();
    if then == MovedThen::Copied {
        super::moving::holding(&engine, true, None).await;
    }
    if then == MovedThen::CrashInIt {
        let crashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = crashed.clone();
        engine.inspect(move |a| a.move_hold.crash_once_moved = Some(hold)).await;
        let _ = tokio::time::timeout(Duration::from_secs(20), move_using(&engine, dir, id, proto::Clash::UseExisting)).await;
        assert!(crashed.load(std::sync::atomic::Ordering::SeqCst), "it crashed there");
        engine = super::engine(dir).await;
    } else {
        move_using(&engine, dir, id, proto::Clash::UseExisting).await.unwrap();
    }
    super::moving::moved(&engine, id).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    let mut now = elsewhere.join("b.bin");
    if then != MovedThen::Copied {
        assert_eq!(shown_had(&engine, id).await["a.bin"], 0, "not checked yet");
    }
    match then {
        MovedThen::Copied | MovedThen::CrashInIt => {}
        MovedThen::Restart | MovedThen::Crash => {
            if then == MovedThen::Crash {
                engine.kill().await
            } else {
                engine.stop().await
            }
            drop(engine);
            engine = super::engine(dir).await;
        }
        MovedThen::MovedAgain => {
            let again = dir.join("again");
            std::fs::create_dir_all(&again).unwrap();
            engine.command(id, super::moving::move_to(&again)).await.unwrap();
            super::moving::moved(&engine, id).await;
            now = again.join("pack/b.bin");
        }
        MovedThen::OwnBack => {
            std::fs::rename(&now, dir.join("theirs")).unwrap();
            std::fs::rename(&own, &now).unwrap();
            until(&engine, id, |s| !matches!(s, Status::MissingFiles | Status::Checking)).await;
            let had = shown_had(&engine, id).await;
            assert_eq!((had["a.bin"], had["b.bin"], had["c.bin"]), (0, 2 * PIECE as u64, 2 * PIECE as u64));
            assert_eq!(std::fs::read(dir.join("theirs")).unwrap(), other());
            return engine.stop().await;
        }
        MovedThen::SetOff => {
            let index = index_of(&engine, id, "b.bin").await;
            engine.command(id, TorrentCommand::SetPriority { files: vec![index], priority: Priority::Off }).await.unwrap();
            until(&engine, id, |s| !matches!(s, Status::MissingFiles | Status::Checking)).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            until(&engine, id, |s| !matches!(s, Status::MissingFiles | Status::Checking)).await;
            assert_eq!(shown_had(&engine, id).await["a.bin"], 0, "checked, nothing of it matched");
            assert_eq!(std::fs::read(&now).unwrap(), other());
            return engine.stop().await;
        }
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    wait_for(&engine, id, Status::MissingFiles).await;
    assert_eq!(shown_had(&engine, id).await["a.bin"], 0, "{then:?}");
    assert_eq!(std::fs::read(&now).unwrap(), other(), "{then:?}");
    // Resume is the choice of it, as it was.
    engine.command(id, TorrentCommand::Resume).await.unwrap();
    wait_for(&engine, id, Status::Downloading).await;
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_that_uses_a_file_there_is_still_missing_another() {
    let thens = [
        MovedThen::Copied,
        MovedThen::Restart,
        MovedThen::Crash,
        MovedThen::CrashInIt,
        MovedThen::MovedAgain,
        MovedThen::OwnBack,
        MovedThen::SetOff,
    ];
    side_by_side(WIDTH, thens, moved_missing).await;
}
