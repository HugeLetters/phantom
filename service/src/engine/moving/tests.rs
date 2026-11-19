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
        let planned = plan(&from, &to, "h", &files(), &BTreeSet::new(), c.clash, c.renames);
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
        // What a move with no way back named the person's, at the paths of the files it left out.
        let named = std::cell::RefCell::new(BTreeSet::new());
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
                    *named.borrow_mut() = left_out(plan);
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
        prop_assert!(!plan.staging.exists(), "something's left staged");
        prop_assert!(all_bytes(&elsewhere).is_empty(), "something went through the link");
        let trashed = all_bytes(&trash_dir);
        for (n, p) in files() {
            let (old, new) = (holds(&from.join(&p), &bytes(n)), holds(&to.join(&p), &bytes(n)));
            prop_assert!(!(old && new), "{p:?} in both destinations");
            match (kept(n) && moved, destroyed[n]) {
                // Its own copy went to the Trash: the person's is the torrent's now.
                (true, _) => prop_assert!(!old && (trashed.contains(&bytes(n)) || c.gone == Some(n) || source_gone.get()), "{p:?}: its own copy"),
                (false, false) if partial && moved => prop_assert!(!old && holds_partly(&to.join(&p)) || blocks, "{p:?}, cut short"),
                // Left out of a move with no way back (blocked from its place, or a copy cut short):
                // its own copy is back where it was.
                (false, false) if moved && plan.missing().contains(&n) => prop_assert!(old && !new, "{:?}, left out", p),
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
        if let Some(Appears::File(n)) = appeared {
            prop_assert!(holds(&to.join(path(n)), b"appeared"), "what appeared is overwritten");
        }
        // Gone forward: the person's paths are exactly those of the files missing there with
        // something at them.
        if moved && plan.forward {
            let named = named.borrow();
            for n in 0..3 {
                let theirs = plan.missing().contains(&n) && meta(&to.join(path(n))).is_some();
                prop_assert_eq!(named.contains(&n), theirs, "file {} named the person's", n);
            }
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
    assert_eq!(plan(&from, &to, "h", &files(), &BTreeSet::new(), None, true), Err(Refused::NotAFolder(to.join("t"))));
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

/// The three files of [`files`] in `from`, `to` made: the plan of their move, with `theirs` at
/// file 0's new path if given, and the Trash of the test's own.
fn set_up(dir: &Path, clash: Option<Clash>, theirs: Option<&[u8]>, renames: bool) -> (Plan, Trash, PathBuf) {
    let (from, to, trash_dir) = (dir.join("from"), dir.join("to"), dir.join("trash"));
    std::fs::create_dir_all(&to).unwrap();
    for (n, path) in files() {
        std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
        std::fs::write(from.join(path), bytes(n)).unwrap();
    }
    if let Some(theirs) = theirs {
        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), theirs).unwrap();
    }
    let plan = plan(&from, &to, "h", &files(), &BTreeSet::new(), clash, renames).unwrap();
    (plan, trash::into(trash_dir.clone(), Arc::default(), Arc::default()), trash_dir)
}

fn staged(plan: &mut Plan) {
    let staged = stage(plan, |_, _| {}, &AtomicBool::new(false), &|| true).unwrap();
    plan.staged_as(&staged);
}

/// The re-review's blocking finding on #264: with no way back (an original deleted by hand), a
/// file that can't take its place because the person put one there is left out of the move. The
/// person's file stays as it is, and the torrent won't claim it: the file is missing, and what's at
/// its path is named as the person's. The torrent's own copy goes back to its old path, where the
/// person can find it: renamed back, or, copied, its original left there and the copy removed. The
/// rest go forward.
#[test]
fn with_no_way_back_a_file_that_cant_take_its_place_is_left_out_and_the_persons_stays() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, trash_dir) = set_up(dir.path(), None, None, renames);
        let (from, to) = (plan.from.clone(), plan.to.clone());
        staged(&mut plan);
        // No way back: copied, c.bin's original is deleted by hand; renamed, its folder is.
        match renames {
            false => std::fs::remove_file(from.join("t/c.bin")).unwrap(),
            true => std::fs::remove_dir_all(from.join("t")).unwrap(),
        }
        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), b"mine").unwrap();
        let blocked = finish(&plan, &trash, &|_| true);
        assert!(matches!(blocked, Err(NotMoved::Taken { .. })), "renames {renames}: {blocked:?}");
        assert_eq!(restore(&plan), Restored::NoWayBack, "renames {renames}");
        forward(&mut plan);
        assert!(finish(&plan, &trash, &|_| true).is_ok(), "renames {renames}");
        let theirs = left_out(&mut plan);

        assert_eq!(std::fs::read(to.join("t/a.bin")).unwrap(), b"mine", "renames {renames}: the person's file, untouched");
        assert_eq!(theirs, BTreeSet::from([0]), "renames {renames}: named the person's");
        assert_eq!(plan.missing(), vec![0], "renames {renames}: a.bin is missing");
        assert_eq!(std::fs::read(from.join("t/a.bin")).unwrap(), bytes(0), "renames {renames}: its own copy, back where it was");
        for (n, path) in files().into_iter().skip(1) {
            assert!(holds(&to.join(&path), &bytes(n)), "renames {renames}: {path:?} moved");
            assert!(!from.join(&path).exists(), "renames {renames}: {path:?} not left behind");
        }
        assert!(!plan.staging.exists(), "renames {renames}: nothing staged");
        assert!(all_bytes(&trash_dir).is_empty(), "renames {renames}: nothing in the Trash");
    }
}

/// #264's fourth review: a file whose path in the old destination holds the person's moves its
/// shadow, the torrent's own, to its path in the new one, the person's left where it is and the
/// shadows' folder gone; what's at its new path is a clash, as for any file that moves. With no
/// shadow nothing of it moves, so its new path is no clash, whatever is there.
#[test]
fn a_file_at_the_persons_path_moves_its_shadow_and_without_one_is_no_clash() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        let (plan_of, _, _) = set_up(dir.path(), None, None, renames);
        std::fs::write(from.join("t/a.bin"), b"mine").unwrap();
        let shadow = from.join(".phantom-theirs-h/0");
        std::fs::create_dir_all(shadow.parent().unwrap()).unwrap();
        std::fs::write(&shadow, bytes(0)).unwrap();
        drop(plan_of);
        let theirs = BTreeSet::from([0]);
        let trash = trash::into(dir.path().join("trash"), Arc::default(), Arc::default());

        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), b"also mine").unwrap();
        let refused = plan(&from, &to, "h", &files(), &theirs, None, renames);
        assert_eq!(refused, Err(Refused::Clash(vec![to.join("t/a.bin")])), "renames {renames}: its shadow would go there");
        std::fs::remove_file(to.join("t/a.bin")).unwrap();

        let mut moving = plan(&from, &to, "h", &files(), &theirs, None, renames).unwrap();
        staged(&mut moving);
        assert!(finish(&moving, &trash, &|_| true).is_ok(), "renames {renames}");
        assert_eq!(left_out(&mut moving), BTreeSet::new(), "renames {renames}: nothing of the person's in the new one");
        assert!(holds(&to.join("t/a.bin"), &bytes(0)), "renames {renames}: the shadow, at the file's path");
        assert_eq!(std::fs::read(from.join("t/a.bin")).unwrap(), b"mine", "renames {renames}: the person's, where it was");
        assert!(!shadow.parent().unwrap().exists(), "renames {renames}: no shadows left");

        // Without a shadow: nothing of it moves, and its new path is no clash.
        let (from, to) = (to, dir.path().join("third"));
        std::fs::rename(from.join("t/a.bin"), dir.path().join("kept")).unwrap();
        std::fs::write(from.join("t/a.bin"), b"mine").unwrap();
        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), b"also mine").unwrap();
        let moving = plan(&from, &to, "h", &files(), &theirs, None, renames).expect("no clash");
        assert_eq!(moving.items[0].source, None, "renames {renames}: nothing of it moves");
    }
}

/// The fifth review's should-fix 3 on #264: with no way back, a file whose shadow can't take its
/// place in the new destination (the person put a file there too) keeps a shadow there: the
/// torrent's own goes with the torrent, a shadow in the new destination until that path is free,
/// and nothing of it is left hidden in the old one. Renamed and copied.
#[test]
fn with_no_way_back_a_blocked_shadow_goes_to_the_new_destinations_shadows() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        drop(set_up(dir.path(), None, None, renames));
        std::fs::write(from.join("t/a.bin"), b"mine").unwrap();
        let shadow = from.join(".phantom-theirs-h/0");
        std::fs::create_dir_all(shadow.parent().unwrap()).unwrap();
        std::fs::write(&shadow, bytes(0)).unwrap();
        let trash = trash::into(dir.path().join("trash"), Arc::default(), Arc::default());
        let mut moving = plan(&from, &to, "h", &files(), &BTreeSet::from([0]), None, renames).unwrap();
        staged(&mut moving);
        // No way back: copied, c.bin's original is deleted by hand; renamed, its folder is.
        match renames {
            false => std::fs::remove_file(from.join("t/c.bin")).unwrap(),
            true => std::fs::remove_dir_all(from.join("t")).unwrap(),
        }
        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), b"also mine").unwrap();
        assert!(finish(&moving, &trash, &|_| true).is_err(), "renames {renames}: blocked");
        assert_eq!(restore(&moving), Restored::NoWayBack, "renames {renames}");
        forward(&mut moving);
        assert!(finish(&moving, &trash, &|_| true).is_ok(), "renames {renames}");

        assert_eq!(left_out(&mut moving), BTreeSet::from([0]), "renames {renames}: the person's path in the new one");
        assert_eq!(std::fs::read(to.join("t/a.bin")).unwrap(), b"also mine", "renames {renames}: the person's, untouched");
        assert!(holds(&to.join(".phantom-theirs-h/0"), &bytes(0)), "renames {renames}: the shadow, in the new destination");
        assert!(!shadow.parent().unwrap().exists(), "renames {renames}: no shadow left in the old one");
        assert!(!moving.staging.exists(), "renames {renames}: nothing staged");
    }
}

/// Replace sends to the Trash only the file the plan found at the path, which the person chose to
/// replace. One swapped in for it meanwhile (another file at that path) blocks the move and stays.
#[test]
fn replace_sends_only_the_file_it_was_chosen_for_to_the_trash() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, trash_dir) = set_up(dir.path(), Some(Clash::Replace), Some(b"theirs"), renames);
        staged(&mut plan);
        let at = plan.to.join("t/a.bin");
        std::fs::write(plan.to.join("t/swapped"), b"swapped").unwrap();
        std::fs::rename(plan.to.join("t/swapped"), &at).unwrap();
        let blocked = finish(&plan, &trash, &|_| true);
        assert!(matches!(blocked, Err(NotMoved::Taken { .. })), "renames {renames}: {blocked:?}");
        assert_eq!(std::fs::read(&at).unwrap(), b"swapped", "renames {renames}: the file swapped in stays");
        assert!(all_bytes(&trash_dir).is_empty(), "renames {renames}: nothing in the Trash");
    }
}

/// Restored, a copy whose original was deleted by hand is all there is of that file: it's kept,
/// and there's no way back.
#[test]
fn a_copy_whose_original_is_gone_is_kept_when_the_move_goes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut plan, _, _) = set_up(dir.path(), None, None, false);
    staged(&mut plan);
    std::fs::remove_file(plan.from.join("t/c.bin")).unwrap();
    assert_eq!(restore(&plan), Restored::NoWayBack);
    assert!(holds(&plan.staging.join("t/c.bin"), &bytes(2)), "the copy is kept");
}

/// Use existing, and the person's file is gone by the end: the torrent's own copy takes the free
/// path, rather than going to the Trash.
#[test]
fn use_existing_with_the_persons_file_gone_by_the_end_keeps_the_torrents_own_copy() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, trash_dir) = set_up(dir.path(), Some(Clash::UseExisting), Some(b"theirs"), renames);
        staged(&mut plan);
        std::fs::remove_file(plan.to.join("t/a.bin")).unwrap();
        assert_eq!(finish(&plan, &trash, &|_| true), Ok(Vec::new()), "renames {renames}");
        assert_eq!(std::fs::read(plan.to.join("t/a.bin")).unwrap(), bytes(0), "renames {renames}: its own copy, moved");
        assert!(all_bytes(&trash_dir).is_empty(), "renames {renames}: nothing in the Trash");
        assert!(!plan.from.join("t").exists(), "renames {renames}: nothing left behind");
    }
}

/// Use existing, the person's file gone by the end, and the torrent's own copy can't take the free
/// path (here: its folder there made read-only as the last file lands): the copy stays in the old
/// destination, and the file is missing in the new one, so the person is told.
#[test]
fn use_existing_whose_own_copy_cant_take_the_free_path_is_missing() {
    use std::os::unix::fs::PermissionsExt;
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, _) = set_up(dir.path(), Some(Clash::UseExisting), Some(b"theirs"), renames);
        staged(&mut plan);
        let folder = plan.to.join("t");
        let last = plan.new(&plan.items[2]);
        let locked = |at: &Path| {
            if at == last {
                std::fs::remove_file(folder.join("a.bin")).unwrap();
                std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
            true
        };
        let warnings = finish(&plan, &trash, &locked);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(warnings.as_ref().is_ok_and(|w| w.len() == 1), "renames {renames}: {warnings:?}");
        assert_eq!(left_out(&mut plan), BTreeSet::new(), "renames {renames}: nothing of the person's there");
        assert_eq!(plan.missing(), vec![0], "renames {renames}: a.bin is missing");
        assert_eq!(std::fs::read(plan.from.join("t/a.bin")).unwrap(), bytes(0), "renames {renames}: its own copy, where it was");
    }
}

/// Replace onto a hard link of the torrent's own file, on the same drive: renamed, the staged file
/// is the one at the path already. Its staged name goes, so nothing is left staged.
#[test]
fn replace_onto_a_hard_link_of_its_own_file_leaves_nothing_staged() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("to"));
    for (n, path) in files() {
        std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
        std::fs::write(from.join(path), bytes(n)).unwrap();
    }
    std::fs::create_dir_all(to.join("t")).unwrap();
    std::fs::hard_link(from.join("t/a.bin"), to.join("t/a.bin")).unwrap();
    let trash = trash::into(dir.path().join("trash"), Arc::default(), Arc::default());
    let mut plan = plan(&from, &to, "h", &files(), &BTreeSet::new(), Some(Clash::Replace), true).unwrap();
    staged(&mut plan);
    assert_eq!(finish(&plan, &trash, &|_| true), Ok(Vec::new()));
    assert!(!plan.staging.exists(), "nothing staged");
    for (n, path) in files() {
        assert!(holds(&to.join(&path), &bytes(n)), "{path:?} moved");
    }
    assert!(!from.join("t").exists(), "nothing left behind");
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
    let mut plan = plan(&from, &to, "h", &files(), &BTreeSet::new(), None, false).unwrap();
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
