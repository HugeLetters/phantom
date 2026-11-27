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
    /// It's given the name `u` there (#331): its own folder `t` is `u` in the new destination.
    name: bool,
    /// The new destination is the old one: a Rename (only with a name).
    same: bool,
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
    /// Once everything's staged, the person puts a file of their own at file `n`'s old path, if
    /// nothing is there (it was renamed).
    old_taken: Option<usize>,
}

fn case() -> impl Strategy<Value = Case> {
    let during = prop_oneof![(0..3usize).prop_map(During::DeleteStaged), Just(During::SourceGone)];
    let appears = prop_oneof![(0..3usize).prop_map(Appears::File), Just(Appears::Link)];
    (
        (any::<bool>(), any::<bool>(), any::<bool>()),
        prop_oneof![Just(None), Just(Some(Clash::UseExisting)), Just(Some(Clash::Replace))],
        proptest::option::of(0..3usize),
        proptest::option::of(0..3usize),
        proptest::option::weighted(0.3, 1..7usize),
        proptest::option::weighted(0.4, (1..7usize, during)),
        proptest::option::weighted(0.3, appears),
        proptest::option::weighted(0.3, 0..3usize),
        proptest::option::weighted(0.3, 0..3usize),
    )
        .prop_map(|((renames, name, same), clash, theirs, gone, stop_after, during, appears, crash_after, old_taken)| Case {
            renames,
            name,
            same: same && name,
            clash,
            theirs,
            gone,
            stop_after,
            during,
            appears,
            crash_after,
            old_taken,
        })
}

fn files() -> Vec<(usize, PathBuf)> {
    vec![(0, "t/a.bin".into()), (1, "t/sub/b.bin".into()), (2, "t/c.bin".into())]
}

/// The person's `choice` for what's at the new paths of [`files`] in `to` now, as a clash found it
/// (none there: an answer for nothing).
fn answered(to: &Path, name: Option<&str>, choice: Clash) -> Answered {
    let found = match plan(Path::new("/nowhere"), to, name, "h", &files(), None, true) {
        Err(Refused::Clash(found)) => found,
        Ok(_) | Err(_) => Vec::new(),
    };
    Answered { choice, found }
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

cases::proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    /// Whatever the case: refused with a clash exactly when something is at a new path and no
    /// choice says what to do. Else, once it's over, nothing is staged, and each file is in at most
    /// one destination with its bytes, in exactly one but for what the person deleted of it: in
    /// the new one if it moved, else in the old. What the person deleted stays deleted: a copy of
    /// it gone back is in the Trash, never in either destination. The person's file at a new path
    /// is kept (Use existing, or a move that didn't happen) or in the Trash (Replace), and the
    /// torrent's own copy of one kept is in the Trash. What appears in the new destination before
    /// the end blocks the end, which always goes back, and is never overwritten, nor anything
    /// reached through a link; a staged file deleted by hand fails the stage. A crash in the end,
    /// run again, ends as without.
    #[test]
    fn a_move_ends_in_exactly_one_destination(c in case()) {
        let dir = tempfile::tempdir().unwrap();
        let (from, to, elsewhere, trash_dir) =
            (dir.path().join("from"), dir.path().join("to"), dir.path().join("elsewhere"), dir.path().join("trash"));
        let to = if c.same { from.clone() } else { to };
        let name = c.name.then_some("u");
        let trash = trash::into(trash_dir.clone(), Arc::default(), Arc::default());
        for d in [&to, &elsewhere] {
            std::fs::create_dir_all(d).unwrap();
        }
        // Its path in the new destination: under its new name, if it's given one.
        let new_path = |n: usize| if c.name { named(&files()[n].1, "u") } else { files()[n].1.clone() };
        for (n, path) in files() {
            std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
            std::fs::write(from.join(path), bytes(n)).unwrap();
        }
        let path = |n: usize| files()[n].1.clone();
        if let Some(n) = c.theirs {
            std::fs::create_dir_all(to.join(new_path(n)).parent().unwrap()).unwrap();
            std::fs::write(to.join(new_path(n)), b"theirs").unwrap();
        }
        let answer = c.clash.map(|choice| answered(&to, name, choice));
        let planned = plan(&from, &to, name, "h", &files(), answer.as_ref(), c.renames);
        let mut plan = match (planned, c.theirs, c.clash) {
            (Err(Refused::Clash(taken)), Some(n), None) => {
                prop_assert_eq!(Found::paths(&taken), vec![AbsolutePath::new(to.join(new_path(n))).unwrap()]);
                return Ok(());
            }
            (Ok(plan), _, Some(_)) | (Ok(plan), None, None) => plan,
            (other, ..) => panic!("{other:?} for {c:?}"),
        };
        let kept = |n: usize| c.theirs == Some(n) && c.clash == Some(Clash::UseExisting);
        // What the person destroyed of each file: none of its bytes are left anywhere.
        let destroyed = std::cell::RefCell::new([false; 3]);
        let alive = |n: usize| holds(&from.join(path(n)), &bytes(n)) || holds(&plan.staging.join(new_path(n)), &bytes(n));
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
                Some((k, During::DeleteStaged(n))) if k == chunks.get() && holds(&plan.staging.join(new_path(n)), &bytes(n)) => {
                    std::fs::remove_file(plan.staging.join(new_path(n))).unwrap();
                    deleted_staged.set(true);
                    destroyed.borrow_mut()[n] |= !alive(n);
                }
                Some((k, During::SourceGone)) if k == chunks.get() => {
                    std::fs::remove_dir_all(from.join("t")).unwrap();
                    source_gone.set(true);
                    // A copy under way goes on from the file it has open.
                    for n in 0..3 {
                        destroyed.borrow_mut()[n] |= !alive(n) && !holds_partly(&plan.staging.join(new_path(n)));
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
        // How it ends: moved, or back where it was, always, whatever the person deleted meanwhile.
        let mut blocks = false;
        // What appeared, if it did: only once everything's staged.
        let mut appeared = None;
        let mut old_taken = None;
        let crashed = std::cell::Cell::new(false);
        let back = |plan: &Plan| -> Result<bool, TestCaseError> {
            let restored = restore(plan, &trash);
            prop_assert_eq!(restored.warnings, Vec::<String>::new());
            Ok(false)
        };
        let moved = match staged {
            Ok(found) if !stopped => {
                plan.staged_as(&found);
                appeared = c.appears;
                if let Some(n) = c.old_taken
                    && meta(&from.join(path(n))).is_none()
                {
                    std::fs::create_dir_all(from.join(path(n)).parent().unwrap()).unwrap();
                    std::fs::write(from.join(path(n)), b"mine").unwrap();
                    old_taken = Some(n);
                }
                // A move under a new name makes its top folder itself (#331): one made meanwhile blocks it.
                let makes_top = c.name
                    && plan.items.iter().all(|i| !i.kept && i.replaces.is_none())
                    && plan.items.iter().any(|i| i.staged.is_some() && i.renamed.as_ref().is_some_and(|r| r.components().count() > 1));
                match c.appears {
                    Some(Appears::File(n)) => {
                        let at = to.join(new_path(n));
                        let _ = std::fs::remove_file(&at);
                        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
                        std::fs::write(&at, b"appeared").unwrap();
                        blocks = plan.items[n].staged.is_some() || plan.items[n].kept || makes_top;
                    }
                    Some(Appears::Link) if meta(&to.join(new_path(1)).parent().unwrap()).is_none() => {
                        let sub = to.join(new_path(1)).parent().unwrap().to_owned();
                        std::fs::create_dir_all(sub.parent().unwrap()).unwrap();
                        std::os::unix::fs::symlink(&elsewhere, sub).unwrap();
                        blocks = plan.items[1].staged.is_some() || makes_top;
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
                        back(&plan)?
                    }
                }
            }
            _ => back(&plan)?,
        };
        prop_assert!(!plan.staging.exists(), "something's left staged");
        prop_assert!(all_bytes(&elsewhere).is_empty(), "something went through the link");
        let trashed = all_bytes(&trash_dir);
        for (n, p) in files() {
            let (old, new) = (holds(&from.join(&p), &bytes(n)), holds(&to.join(new_path(n)), &bytes(n)));
            prop_assert!(!(old && new), "{p:?} in both destinations");
            // Gone back with its original deleted by hand: deleted it stays, its copy (or what was
            // copied of it) in the Trash.
            let copy_trashed = !moved && source_gone.get() && !c.renames && !destroyed[n] && !kept(n);
            // Its old path the person's now: never overwritten. Gone back, the torrent's own file
            // can't take it again, and goes to the Trash.
            if old_taken == Some(n) {
                prop_assert!(holds(&from.join(&p), b"mine"), "{:?}: the person's file at its old path, overwritten", p);
            }
            match (kept(n) && moved, destroyed[n]) {
                (false, false) if old_taken == Some(n) && !moved && plan.items[n].staged.is_some() => {
                    prop_assert!(!new && trashed.contains(&bytes(n)), "{:?}: its own file, in the Trash", p)
                }
                // Its own copy went to the Trash: the person's is the torrent's now.
                (true, _) => prop_assert!(!old && (trashed.contains(&bytes(n)) || c.gone == Some(n) || source_gone.get()), "{p:?}: its own copy"),
                (false, true) => prop_assert!(!old && !new, "{:?}, deleted by hand, stays deleted", p),
                (false, false) if copy_trashed => {
                    prop_assert!(!old && !new && trashed.iter().any(|b| bytes(n).starts_with(b)), "{:?}: its copy, in the Trash", p)
                }
                (false, false) => prop_assert!(if moved { new } else { old }, "{:?} isn't where it should be (moved {})", p, moved),
            }
        }
        if let Some(n) = c.theirs {
            let at = to.join(new_path(n));
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
            prop_assert!(holds(&to.join(new_path(n)), b"appeared"), "what appeared is overwritten");
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
    assert_eq!(plan(&from, &to, None, "h", &files(), None, true), Err(Refused::NotAFolder(to.join("t"))));
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
    let answer = clash.map(|choice| answered(&to, None, choice));
    let plan = plan(&from, &to, None, "h", &files(), answer.as_ref(), renames).unwrap();
    (plan, trash::into(trash_dir.clone(), Arc::default(), Arc::default()), trash_dir)
}

/// What the actor does once every file is staged: lands them, then (that written ahead) removes
/// the old.
fn finish(plan: &Plan, trash: &Trash, between: &dyn Fn(&Path) -> bool) -> Result<Vec<String>, NotMoved> {
    land(plan, trash, between)?;
    Ok(remove_old(plan, trash))
}

fn staged(plan: &mut Plan) {
    let staged = stage(plan, |_, _| {}, &AtomicBool::new(false), &|| true).unwrap();
    plan.staged_as(&staged);
}

/// The user's rule on #264: a file of the person's put at a new path sends the move back, even with
/// an original deleted by hand meanwhile. The person's file stays as it is. Copied, the copy of the
/// file they deleted goes to the Trash, and it stays deleted; renamed (the torrent's folder deleted
/// by hand, every file of it staged already), each goes back to its path, its folders made again.
#[test]
fn a_file_of_the_persons_at_a_new_path_sends_the_move_back_even_with_an_original_deleted_by_hand() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, trash_dir) = set_up(dir.path(), None, None, renames);
        let (from, to) = (plan.from.clone(), plan.to.clone());
        staged(&mut plan);
        match renames {
            false => std::fs::remove_file(from.join("t/c.bin")).unwrap(),
            true => std::fs::remove_dir_all(from.join("t")).unwrap(),
        }
        std::fs::create_dir_all(to.join("t")).unwrap();
        std::fs::write(to.join("t/a.bin"), b"mine").unwrap();
        let blocked = finish(&plan, &trash, &|_| true);
        assert!(matches!(blocked, Err(NotMoved::Taken { .. })), "renames {renames}: {blocked:?}");
        assert_eq!(restore(&plan, &trash), Restored::default(), "renames {renames}");

        assert_eq!(std::fs::read(to.join("t/a.bin")).unwrap(), b"mine", "renames {renames}: the person's file, untouched");
        for (n, path) in files() {
            let deleted = !renames && n == 2;
            assert_eq!(holds(&from.join(&path), &bytes(n)), !deleted, "renames {renames}: {path:?} back, unless deleted by hand");
        }
        let trashed = all_bytes(&trash_dir);
        assert_eq!(trashed, if renames { Vec::new() } else { vec![bytes(2)] }, "renames {renames}: the copy of the one deleted");
        assert!(!plan.staging.exists(), "renames {renames}: nothing staged");
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

/// Gone back, a copy whose original was deleted by hand goes to the Trash: deleted it stays. One
/// renamed whose old path holds something new (the person's) can't take it again: it goes to the
/// Trash too, and the torrent's pieces there aren't its own any more.
#[test]
fn what_cant_go_back_to_its_old_path_goes_to_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let (mut plan, trash, trash_dir) = set_up(dir.path(), None, None, false);
    staged(&mut plan);
    std::fs::remove_file(plan.from.join("t/c.bin")).unwrap();
    assert_eq!(restore(&plan, &trash), Restored::default());
    assert_eq!(all_bytes(&trash_dir), [bytes(2)], "the copy, in the Trash");
    assert!(!plan.from.join("t/c.bin").exists(), "deleted it stays");

    let dir = tempfile::tempdir().unwrap();
    let (mut plan, trash, trash_dir) = set_up(dir.path(), None, None, true);
    staged(&mut plan);
    std::fs::write(plan.from.join("t/a.bin"), b"mine").unwrap();
    assert_eq!(restore(&plan, &trash), Restored { warnings: Vec::new(), replaced: true });
    assert_eq!(std::fs::read(plan.from.join("t/a.bin")).unwrap(), b"mine", "the person's, untouched");
    assert_eq!(all_bytes(&trash_dir), [bytes(0)], "the torrent's, in the Trash");
    assert!(holds(&plan.from.join("t/c.bin"), &bytes(2)), "the rest, back");
}

/// Once every file has landed (and the record says so), the move never goes back: a start after a
/// crash removes the old, before any of it went or with some gone (should-fix 1a of #264's seventh
/// review), even with something at a new path by then (the person's, put there while Phantom was
/// down): the old files go all the same, and the person's stays.
#[test]
fn once_every_file_has_landed_a_crash_finishes_the_move_whatever_is_at_a_new_path() {
    for (renames, removed_first) in [(false, false), (false, true), (true, false), (true, true)] {
        let at = format!("renames {renames}, removed first {removed_first}");
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, _) = set_up(dir.path(), None, None, renames);
        staged(&mut plan);
        land(&plan, &trash, &|_| true).unwrap();
        if removed_first {
            assert_eq!(remove_old(&plan, &trash), Vec::<String>::new(), "{at}");
        }
        std::fs::remove_file(plan.to.join("t/a.bin")).unwrap();
        std::fs::write(plan.to.join("t/a.bin"), b"mine").unwrap();
        assert_eq!(remove_old(&plan, &trash), Vec::<String>::new(), "{at}");
        assert_eq!(std::fs::read(plan.to.join("t/a.bin")).unwrap(), b"mine", "{at}: the person's, untouched");
        for (n, path) in files() {
            assert!(n == 0 || holds(&plan.to.join(&path), &bytes(n)), "{at}: {path:?} moved");
            assert!(!plan.from.join(&path).exists(), "{at}: {path:?} not left behind");
        }
        assert!(!plan.staging.exists(), "{at}: nothing staged");
    }
}

/// Should-fix 1b of #264's seventh review: a landing that can't be written ahead is taken back
/// before any original goes. Every file is staged again, what Replace sent to the Trash is back at
/// its path, and the move then goes back, every original where it was.
#[test]
fn a_landing_taken_back_leaves_every_original_and_what_replace_was_chosen_for() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut plan, trash, trash_dir) = set_up(dir.path(), Some(Clash::Replace), Some(b"theirs"), renames);
        staged(&mut plan);
        let landed = land(&plan, &trash, &|_| true).unwrap();
        assert!(holds(&plan.to.join("t/a.bin"), &bytes(0)), "renames {renames}: landed");
        unland(&plan, &trash, &landed);
        assert_eq!(restore(&plan, &trash), Restored::default(), "renames {renames}");
        for (n, path) in files() {
            assert!(holds(&plan.from.join(&path), &bytes(n)), "renames {renames}: {path:?} back where it was");
        }
        assert_eq!(std::fs::read(plan.to.join("t/a.bin")).unwrap(), b"theirs", "renames {renames}: theirs, back");
        assert_eq!(all_bytes(&plan.to), [b"theirs".to_vec()], "renames {renames}: nothing else in the new destination");
        assert!(all_bytes(&trash_dir).is_empty(), "renames {renames}: nothing in the Trash");
    }
}

/// A nit of #264's seventh review: a file the person put in place of an original before its turn
/// to stage (another file at its path, not the one the plan found) stays out of the move. Renamed
/// into staging, a move going back would send the person's file to the Trash, as a copy whose
/// original is gone.
#[test]
fn a_file_put_in_place_of_an_original_before_its_turn_stays_out_of_the_move() {
    for renames in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (plan, trash, trash_dir) = set_up(dir.path(), None, None, renames);
        let old = plan.from.join("t/c.bin");
        // As long as the original: only what it is tells it apart.
        let mine = vec![9; bytes(2).len()];
        std::fs::write(plan.from.join("t/mine"), &mine).unwrap();
        std::fs::rename(plan.from.join("t/mine"), &old).unwrap();
        let mut plan = plan;
        staged(&mut plan);
        assert_eq!(plan.items[2].staged, None, "renames {renames}: not staged");
        assert_eq!(restore(&plan, &trash), Restored { warnings: Vec::new(), replaced: true }, "renames {renames}");
        assert_eq!(std::fs::read(&old).unwrap(), mine, "renames {renames}: the person's, where they put it");
        assert!(all_bytes(&trash_dir).is_empty(), "renames {renames}: nothing in the Trash");
    }
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
        left_out(&mut plan);
        assert_eq!(plan.missing(), vec![0], "renames {renames}: a.bin is missing");
        assert_eq!(std::fs::read(plan.from.join("t/a.bin")).unwrap(), bytes(0), "renames {renames}: its own copy, where it was");
    }
}

/// A clash's answer holds for the files it was given, as they were (#270): a file put at another
/// of the torrent's paths since, another file swapped in at the path, or the file written since,
/// refuses the plan, whatever the choice, naming that path; one of them gone since is no clash.
#[test]
fn an_answer_holds_only_for_the_files_it_was_given_as_they_were() {
    let changes: [(&str, fn(&Path)); 4] = [
        ("a new file at another path", |to| std::fs::write(to.join("t/c.bin"), b"new").unwrap()),
        ("another file swapped in", |to| {
            std::fs::write(to.join("t/swapped"), b"theirs").unwrap();
            std::fs::rename(to.join("t/swapped"), to.join("t/a.bin")).unwrap();
        }),
        ("the file written", |to| std::fs::write(to.join("t/a.bin"), b"theirs, longer").unwrap()),
        ("the file gone", |to| std::fs::remove_file(to.join("t/a.bin")).unwrap()),
    ];
    for choice in [Clash::Replace, Clash::UseExisting] {
        for (change, make) in changes {
            let dir = tempfile::tempdir().unwrap();
            let (from, to) = (dir.path().join("from"), dir.path().join("to"));
            for (n, path) in files() {
                std::fs::create_dir_all(from.join(&path).parent().unwrap()).unwrap();
                std::fs::write(from.join(path), bytes(n)).unwrap();
            }
            std::fs::create_dir_all(to.join("t")).unwrap();
            std::fs::write(to.join("t/a.bin"), b"theirs").unwrap();
            let answer = answered(&to, None, choice);
            assert!(plan(&from, &to, None, "h", &files(), Some(&answer), true).is_ok(), "{choice:?}: as asked, it moves");
            make(&to);
            let planned = plan(&from, &to, None, "h", &files(), Some(&answer), true);
            match change {
                "the file gone" => assert!(planned.is_ok_and(|p| p.adopted().is_empty()), "{choice:?}, {change}"),
                "a new file at another path" => assert_eq!(planned, Err(Refused::Changed(to.join("t/c.bin"))), "{choice:?}, {change}"),
                _ => assert_eq!(planned, Err(Refused::Changed(to.join("t/a.bin"))), "{choice:?}, {change}"),
            }
        }
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
    let mut plan = plan(&from, &to, None, "h", &files(), Some(&answered(&to, None, Clash::Replace)), true).unwrap();
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
    let mut plan = plan(&from, &to, None, "h", &files(), None, false).unwrap();
    let staged = stage(&plan, |_, _| {}, &AtomicBool::new(false), &|| true).unwrap();
    plan.staged_as(&staged);
    std::fs::create_dir_all(to.join("t/sub")).unwrap();
    std::fs::write(to.join("t/sub/b.bin"), b"mine").unwrap();
    let blocked = finish(&plan, &trash, &|_| true);
    assert!(matches!(blocked, Err(NotMoved::Taken { .. })), "{blocked:?}");
    assert_eq!(std::fs::read(to.join("t/sub/b.bin")).unwrap(), b"mine");
    assert!(!to.join("t/a.bin").exists(), "what had taken its place went back");
    plan.staged_as(&[]);
    assert_eq!(restore(&plan, &trash), Restored::default());
    for (n, path) in files() {
        assert_eq!(std::fs::read(from.join(path)).unwrap(), bytes(n));
    }
}
