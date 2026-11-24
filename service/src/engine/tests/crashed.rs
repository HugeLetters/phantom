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

/// Runs the cases side by side: each takes seconds, most of them waiting.
async fn each(cases: impl IntoIterator<Item = (At, &'static [&'static str], Put, When, bool, bool)>) {
    let mut running = tokio::task::JoinSet::new();
    for (at, gone, put, when, off, crash) in cases {
        running.spawn(interrupted(at, gone, put, when, off, crash));
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
