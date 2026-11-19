use std::sync::Arc;

use proptest::prelude::*;

use super::*;

/// What the person does to a move's files after the stage's `k`th chunk (or file, renamed).
#[derive(Debug, Clone, Copy, PartialEq)]
enum During {
    /// Deletes file `n`'s staged copy, if it's staged.
    DeleteStaged(usize),
    /// Deletes the torrent's folder in the old destination, whatever's left of it.
    SourceGone,
}

/// What the person puts in the new destination once everything is staged, before the files take
/// their places.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Appears {
    /// A file at file `n`'s new path (over the person's own there, if it's theirs: another file).
    File(usize),
    /// `t/sub` becomes a link to a folder elsewhere.
    Link,
}

/// Three files of a torrent's own folder, `t/`, each of a size of its own, in `from`; and what's
/// done around the move: a file of the person's at a path one takes in `to`, one deleted from the
/// old destination before it moves, a stop, what happens during the stage, what appears before
/// the end, and a crash in the end, after which a start runs the end again.
#[derive(Debug, Clone)]
struct Case {
    renames: bool,
    clash: Option<Clash>,
    /// A file of the person's at the new path of file `n`.
    theirs: Option<usize>,
    /// File `n` is gone from the old destination before it moves.
    gone: Option<usize>,
    /// It's stopped after this many chunks (then restored).
    stop_after: Option<usize>,
    during: Option<(usize, During)>,
    appears: Option<Appears>,
    /// The end is cut short after this many files took their places, and run again.
    crash_after: Option<usize>,
}

fn case() -> impl Strategy<Value = Case> {
    let during = prop_oneof![(0..3usize).prop_map(During::DeleteStaged), Just(During::SourceGone)];
    let appears = prop_oneof![(0..3usize).prop_map(Appears::File), Just(Appears::Link)];
    (
        any::<bool>(),
        prop_oneof![Just(None), Just(Some(Clash::UseExisting)), Just(Some(Clash::Replace))],
        proptest::option::of(0..3usize),
        proptest::option::of(0..3usize),
        proptest::option::weighted(0.3, 1..7usize),
        proptest::option::weighted(0.4, (1..7usize, during)),
        proptest::option::weighted(0.3, appears),
        proptest::option::weighted(0.3, 0..3usize),
    )
        .prop_map(|(renames, clash, theirs, gone, stop_after, during, appears, crash_after)| Case {
            renames,
            clash,
            theirs,
            gone,
            stop_after,
            during,
            appears,
            crash_after,
        })
}

fn files() -> Vec<(usize, PathBuf)> {
    vec![(0, "t/a.bin".into()), (1, "t/sub/b.bin".into()), (2, "t/c.bin".into())]
}

fn bytes(n: usize) -> Vec<u8> {
    // A chunk and a half, and a chunk of zeros in the middle of the second.
    let mut b: Vec<u8> = (0..CHUNK + CHUNK / 2 + n * 7).map(|i| (i * 13 + n) as u8 | 1).collect();
    if n == 1 {
        b[CHUNK / 4..CHUNK / 4 + CHUNK].fill(0);
    }
    b
}

fn holds(path: &Path, want: &[u8]) -> bool {
    meta(path).is_some_and(|m| m.file_type().is_file()) && std::fs::read(path).is_ok_and(|b| b == want)
}

/// Every file's bytes under `dir`, not following links.
fn all_bytes(dir: &Path) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_owned()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                Ok(t) if t.is_file() => found.push(std::fs::read(e.path()).unwrap()),
                _ => {}
            }
        }
    }
    found
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    /// Whatever the case: refused with a clash exactly when something is at a new path and no
    /// choice says what to do. Else, once it's over, nothing is staged, and each file is in at most
    /// one destination with its bytes, in exactly one but for what the person deleted of it: in
    /// the new one if it moved, else in the old. The person's file at a new path is kept (Use
    /// existing, or a move that didn't happen) or in the Trash (Replace), and the torrent's own
    /// copy of one kept is in the Trash. What appears in the new destination before the end blocks
    /// the end and is never overwritten, nor anything reached through a link; a staged file
    /// deleted by hand fails the stage. A source deleted by hand leaves no way back: what was
    /// staged goes forward. A crash in the end, run again, ends as without.
    #[test]
    fn a_move_ends_in_exactly_one_destination(c in case()) {
        let dir = tempfile::tempdir().unwrap();
        let (from, to, elsewhere, trash_dir) =
            (dir.path().join("from"), dir.path().join("to"), dir.path().join("elsewhere"), dir.path().join("trash"));
        let trash = trash::into(trash_dir.clone(), Arc::default(), Arc::default());
        for d in [&to, &elsewhere] {
            std::fs::create_dir_all(d).unwrap();
        }
        for (n, path) in files() {
            std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
            std::fs::write(from.join(path), bytes(n)).unwrap();
        }
        let path = |n: usize| files()[n].1.clone();
        if let Some(n) = c.theirs {
            std::fs::create_dir_all(to.join(path(n)).parent().unwrap()).unwrap();
            std::fs::write(to.join(path(n)), b"theirs").unwrap();
        }
        let planned = plan(&from, &to, "h", &files(), c.clash, c.renames);
        let mut plan = match (planned, c.theirs, c.clash) {
            (Err(Refused::Clash(taken)), Some(n), None) => {
                prop_assert_eq!(taken, vec![to.join(path(n))]);
                return Ok(());
            }
            (Ok(plan), _, Some(_)) | (Ok(plan), None, None) => plan,
            (other, ..) => panic!("{other:?} for {c:?}"),
        };
        let kept = |n: usize| c.theirs == Some(n) && c.clash == Some(Clash::UseExisting);
        // What the person destroyed of each file: none of its bytes are left anywhere.
        let destroyed = std::cell::RefCell::new([false; 3]);
        let alive = |n: usize| holds(&from.join(path(n)), &bytes(n)) || holds(&plan.staging.join(path(n)), &bytes(n));
        if let Some(n) = c.gone {
            std::fs::remove_file(from.join(path(n))).unwrap();
            destroyed.borrow_mut()[n] = true;
        }
        let stop = AtomicBool::new(false);
        let chunks = std::cell::Cell::new(0);
        let deleted_staged = std::cell::Cell::new(false);
        let source_gone = std::cell::Cell::new(false);
        let between = || {
            chunks.set(chunks.get() + 1);
            if c.stop_after == Some(chunks.get()) {
                stop.store(true, Ordering::SeqCst);
            }
            match c.during {
                Some((k, During::DeleteStaged(n))) if k == chunks.get() && holds(&plan.staging.join(path(n)), &bytes(n)) => {
                    std::fs::remove_file(plan.staging.join(path(n))).unwrap();
                    deleted_staged.set(true);
                    destroyed.borrow_mut()[n] |= !alive(n);
                }
                Some((k, During::SourceGone)) if k == chunks.get() => {
                    std::fs::remove_dir_all(from.join("t")).unwrap();
                    source_gone.set(true);
                    // A copy under way goes on from the file it has open.
                    for n in 0..3 {
                        destroyed.borrow_mut()[n] |= !alive(n) && !holds_partly(&plan.staging.join(path(n)));
                    }
                }
                _ => {}
            }
            true
        };
        let staged = stage(&plan, |_, _| {}, &stop, &between);
        let destroyed = destroyed.into_inner();
        let stopped = stop.load(Ordering::SeqCst);
        if deleted_staged.get() && !stopped {
            prop_assert!(matches!(staged, Err(Stop::Failed(NotMoved::CopyGone { .. }))), "a staged file deleted: {staged:?}");
        }
        // How it ends: moved, or back where it was; with no way back (a source deleted), moved
        // without what can't take its place.
        let mut blocks = false;
        // What appeared, if it did: only once everything's staged.
        let mut appeared = None;
        let crashed = std::cell::Cell::new(false);
        let back_or_forward = |plan: &mut Plan, blocks: bool| -> Result<bool, TestCaseError> {
            match restore(plan) {
                Restored::Back(warnings) => {
                    prop_assert_eq!(warnings, Vec::<String>::new());
                    Ok(false)
                }
                Restored::NoWayBack => {
                    prop_assert!(source_gone.get(), "no way back, with every source there");
                    forward(plan);
                    let warnings = finish(plan, &trash, &|_| true);
                    prop_assert!(warnings.as_ref().is_ok_and(|w| blocks || w.is_empty()), "{:?}", warnings);
                    Ok(true)
                }
            }
        };
        let moved = match staged {
            Ok(found) if !stopped => {
                plan.staged_as(&found);
                appeared = c.appears;
                match c.appears {
                    Some(Appears::File(n)) => {
                        let at = to.join(path(n));
                        let _ = std::fs::remove_file(&at);
                        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
                        std::fs::write(&at, b"appeared").unwrap();
                        blocks = plan.items[n].staged.is_some();
                    }
                    Some(Appears::Link) if meta(&to.join("t/sub")).is_none() => {
                        std::fs::create_dir_all(to.join("t")).unwrap();
                        std::os::unix::fs::symlink(&elsewhere, to.join("t/sub")).unwrap();
                        blocks = plan.items[1].staged.is_some();
                    }
                    Some(Appears::Link) | None => {}
                }
                let landed = std::cell::Cell::new(0);
                let crash = |_: &Path| {
                    landed.set(landed.get() + 1);
                    crashed.set(crashed.get() || c.crash_after == Some(landed.get()));
                    c.crash_after != Some(landed.get())
                };
                let mut ended = finish(&plan, &trash, &crash);
                if ended == Err(NotMoved::Interrupted) {
                    ended = finish(&plan, &trash, &|_| true);
                }
                match ended {
                    Ok(warnings) => {
                        prop_assert!(!blocks, "what appeared didn't block it");
                        prop_assert_eq!(warnings, Vec::<String>::new());
                        true
                    }
                    Err(why) => {
                        prop_assert!(blocks, "blocked for nothing: {why:?}");
                        prop_assert!(matches!(why, NotMoved::Taken { .. } | NotMoved::NotAFolder { .. }), "{why:?}");
                        back_or_forward(&mut plan, blocks)?
                    }
                }
            }
            _ => back_or_forward(&mut plan, blocks)?,
        };
        // Copies cut short by a stop after their source went are all there is of them: kept.
        let partial = source_gone.get() && stopped;
        prop_assert!(!plan.staging.exists() || plan.forward && blocks, "something's left staged");
        prop_assert!(all_bytes(&elsewhere).is_empty(), "something went through the link");
        let trashed = all_bytes(&trash_dir);
        for (n, p) in files() {
            let (old, new) = (holds(&from.join(&p), &bytes(n)), holds(&to.join(&p), &bytes(n)));
            prop_assert!(!(old && new), "{p:?} in both destinations");
            match (kept(n) && moved, destroyed[n]) {
                // Its own copy went to the Trash: the person's is the torrent's now.
                (true, _) => prop_assert!(!old && (trashed.contains(&bytes(n)) || c.gone == Some(n) || source_gone.get()), "{p:?}: its own copy"),
                (false, false) if partial && moved => prop_assert!(!old && holds_partly(&to.join(&p)) || blocks, "{p:?}, cut short"),
                (false, false) if moved && blocks && plan.forward => {}
                (false, false) => prop_assert!(if moved { new } else { old }, "{:?} isn't where it should be (moved {})", p, moved),
                (false, true) => {}
            }
        }
        if let Some(n) = c.theirs {
            let at = to.join(path(n));
            let replaced = moved && c.clash == Some(Clash::Replace) && plan.items[n].staged.is_some();
            let over = appeared == Some(Appears::File(n));
            match (replaced, over) {
                (_, true) => {}
                (true, false) => prop_assert!(trashed.iter().any(|b| b == b"theirs") && !holds(&at, b"theirs"), "theirs, replaced"),
                // What a run before a crash sent to the Trash for Replace stays there, if the end is
                // then blocked: where it went is that run's.
                (false, false) if crashed.get() && c.clash == Some(Clash::Replace) => {
                    prop_assert!(holds(&at, b"theirs") || trashed.iter().any(|b| b == b"theirs"), "theirs, lost")
                }
                (false, false) => prop_assert!(holds(&at, b"theirs"), "theirs, kept"),
            }
        }
        if let Some(Appears::File(n)) = appeared
            && plan.items[n].staged.is_some()
        {
            prop_assert!(holds(&to.join(path(n)), b"appeared"), "what appeared is overwritten");
        }
    }
}

/// Whether something of a copy is at `path`: the start of a copy under way.
fn holds_partly(path: &Path) -> bool {
    meta(path).is_some_and(|m| m.file_type().is_file())
}

#[test]
fn nothing_moves_through_a_link_on_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to, elsewhere) = (dir.path().join("from"), dir.path().join("to"), dir.path().join("elsewhere"));
    for d in [&from, &to, &elsewhere] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::os::unix::fs::symlink(&elsewhere, to.join("t")).unwrap();
    assert_eq!(plan(&from, &to, "h", &files(), None, true), Err(Refused::NotAFolder(to.join("t"))));
}

/// The same folder, however it's spelled: through a link, with `..`, in another case (the temp
/// dir's drive ignores case, as macOS's does by default); and inside the torrent's own folder so too.
#[test]
fn a_folder_is_told_by_what_it_is_not_by_its_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(downloads.join("t/sub")).unwrap();
    std::fs::create_dir_all(dir.path().join("x")).unwrap();
    std::os::unix::fs::symlink(&downloads, dir.path().join("alias")).unwrap();
    let ignores_case = dir.path().join("DOWNLOADS").exists();
    let mut spellings = vec![dir.path().join("alias"), dir.path().join("x/../downloads"), downloads.clone()];
    if ignores_case {
        spellings.push(dir.path().join("DOWNLOADS"));
    }
    for to in &spellings {
        assert!(same_folder(to, &downloads), "{} is the same folder", to.display());
        assert!(!inside(to, &downloads.join("t")), "{} isn't inside the torrent's", to.display());
        assert!(inside(&to.join("t/sub"), &downloads.join("t")), "{} is inside the torrent's", to.join("t/sub").display());
        assert!(inside(&to.join("t"), &downloads.join("t")), "{} is the torrent's own", to.join("t").display());
    }
    assert!(!same_folder(&dir.path().join("x"), &downloads));
}

/// Something at a file's new path as the move ends that isn't what the plan found there, nor what
/// Replace was chosen for: the files that had taken their places go back into staging, and the
/// move is restored, the person's file untouched.
#[test]
fn a_file_put_at_a_new_path_during_the_copy_blocks_the_end_and_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("to"));
    let trash = trash::into(dir.path().join("trash"), Arc::default(), Arc::default());
    std::fs::create_dir_all(&to).unwrap();
    for (n, path) in files() {
        std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
        std::fs::write(from.join(path), bytes(n)).unwrap();
    }
    let mut plan = plan(&from, &to, "h", &files(), None, false).unwrap();
    let staged = stage(&plan, |_, _| {}, &AtomicBool::new(false), &|| true).unwrap();
    plan.staged_as(&staged);
    std::fs::create_dir_all(to.join("t/sub")).unwrap();
    std::fs::write(to.join("t/sub/b.bin"), b"mine").unwrap();
    let blocked = finish(&plan, &trash, &|_| true);
    assert!(matches!(blocked, Err(NotMoved::Taken { .. })), "{blocked:?}");
    assert_eq!(std::fs::read(to.join("t/sub/b.bin")).unwrap(), b"mine");
    assert!(!to.join("t/a.bin").exists(), "what had taken its place went back");
    plan.staged_as(&[]);
    assert_eq!(restore(&plan), Restored::Back(Vec::new()));
    for (n, path) in files() {
        assert_eq!(std::fs::read(from.join(path)).unwrap(), bytes(n));
    }
}
