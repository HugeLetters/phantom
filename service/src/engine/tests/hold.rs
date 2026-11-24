//! Generated test of the hold (#358): a torrent found where it is now, then anything a person may do
//! to it and to what's at its place, by hand and through Phantom, across restarts, crashes and its
//! destination going and coming back. Not a model of when the engine checks: an oracle of what must
//! never happen, whenever it does.
//!
//! - **Held, or off the list, nothing at its place changes**: every file is, byte for byte, what
//!   was last put there by hand, none is made, and no folder.
//! - **A hold is lifted only on what was read**: every file there is the torrent's own to write (none
//!   longer, none with another hard link), and either a file there holds a piece of the torrent's own
//!   content, or the person chose "Download over them" (Resume, "Files don't match") after every
//!   piece was read since the files were last changed by hand or the service started, and nothing
//!   was changed since. Lifted, its bytes are still there (a short file grown, one not there made),
//!   and it has just the pieces that are truly there.
//! - **In the end a Check settles it**: lifted if a piece of its own content is there and every file
//!   is its own to write, held if not; and then "Download over them" lifts it, unless a file isn't its
//!   own to write.
//!
//! Each change by hand is made on the actor, between two of its turns, so what the engine decided
//! before it and after it can be told apart. 16 cases, or `PROPTEST_CASES`.

use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use proto::{FileName, Held};

use super::*;

const NAMES: [&str; 3] = ["a.bin", "b.bin", "c.bin"];
const EVERY: Duration = Duration::from_millis(40);

/// What's at a file's path, put there by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The torrent's own content.
    Own,
    /// Other bytes of the same length.
    Other,
    /// Zeros of the same length.
    Zeros,
    /// Its first piece only.
    Short,
    /// Its own content and more after it.
    Longer,
    Gone,
}

#[derive(Debug, Clone)]
enum Step {
    /// The file made `kind` by hand: another file renamed over it (as Finder replaces one), or
    /// written in place.
    Hand {
        file: usize,
        kind: Kind,
        replace: bool,
    },
    /// A hard link to the file made elsewhere, or removed.
    Link {
        file: usize,
        on: bool,
    },
    Check,
    CancelCheck,
    Resume,
    Pause,
    Restart,
    Crash,
    DestinationGone,
    DestinationBack,
    Priority {
        file: usize,
        off: bool,
    },
    /// Remove (or Delete), then Undo or not.
    Remove {
        delete: bool,
        undo: bool,
    },
    /// Checks wait as they read, or go on.
    HoldChecks(bool),
}

fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![Just(Kind::Own), Just(Kind::Other), Just(Kind::Zeros), Just(Kind::Short), Just(Kind::Longer), Just(Kind::Gone)]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => (0..3usize, kind(), any::<bool>()).prop_map(|(file, kind, replace)| Step::Hand { file, kind, replace }),
        2 => (0..3usize, any::<bool>()).prop_map(|(file, on)| Step::Link { file, on }),
        3 => Just(Step::Check),
        1 => Just(Step::CancelCheck),
        4 => Just(Step::Resume),
        1 => Just(Step::Pause),
        2 => Just(Step::Restart),
        2 => Just(Step::Crash),
        1 => Just(Step::DestinationGone),
        1 => Just(Step::DestinationBack),
        2 => (0..3usize, any::<bool>()).prop_map(|(file, off)| Step::Priority { file, off }),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(delete, undo)| Step::Remove { delete, undo }),
        2 => any::<bool>().prop_map(Step::HoldChecks),
    ]
}

/// What's found: each file as it is there (none longer than its own, and one there at least).
fn found() -> impl Strategy<Value = [Kind; 3]> {
    let there = || prop_oneof![Just(Kind::Own), Just(Kind::Other), Just(Kind::Zeros), Just(Kind::Short), Just(Kind::Gone)];
    [there(), there(), there()].prop_filter("a file of it is there", |kinds| kinds.iter().any(|k| *k != Kind::Gone))
}

struct World {
    dir: TempDir,
    engine: Option<Engine>,
    gate: Arc<super::super::files::Gate>,
    /// Reads of the torrent's files since they were last changed by hand, or the engine started.
    reads: Arc<AtomicUsize>,
    id: u64,
    own: [Vec<u8>; 3],
    kinds: [Kind; 3],
    linked: [bool; 3],
    off: [bool; 3],
    /// The index librqbit gives each of `NAMES`.
    index: [usize; 3],
    away: bool,
    /// "Download over them" was chosen of the files as they are, every piece read: it may lift.
    chosen: bool,
}

/// What the actor says of the torrent and what's at its place, at one instant.
struct Seen {
    listed: bool,
    held: Option<Held>,
    disk: std::collections::BTreeMap<PathBuf, Vec<u8>>,
}

impl World {
    fn downloads(&self) -> PathBuf {
        self.dir.path().join(if self.away { "away" } else { "downloads" })
    }

    fn album(&self) -> PathBuf {
        self.downloads().join("Album")
    }

    fn link(&self, file: usize) -> PathBuf {
        self.dir.path().join("links").join(NAMES[file])
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("running")
    }

    async fn start(&mut self) {
        self.reads.store(0, Ordering::SeqCst);
        let guard = super::super::files::Guard::default()
            .counting_reads(self.reads.clone())
            .with_gate(super::super::files::At::Read, self.gate.clone());
        let dir = self.dir.path();
        let config = Config {
            data_dir: dir.join("data"),
            download_dir: dir.join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: EVERY,
            poll_every: EVERY,
            trash: super::super::trash::into(dir.join("trash"), Default::default(), Default::default()),
            speed_limits: Default::default(),
            seeding: Default::default(),
        };
        let add_into: super::super::actor::AddInto = Arc::new(|session, id, add, destination, download_dir, guard| {
            Box::pin(super::super::actor::add_into(session, id, add, destination, download_dir, guard))
        });
        self.engine = Some(Engine::start_with(config, add_into, guard, super::super::Looks::real()).await.unwrap());
    }

    /// What `kind` puts at `file`'s path.
    fn bytes(&self, file: usize, kind: Kind) -> Option<Vec<u8>> {
        let own = &self.own[file];
        match kind {
            Kind::Own => Some(own.clone()),
            Kind::Other => Some(vec![9; own.len()]),
            Kind::Zeros => Some(vec![0; own.len()]),
            Kind::Short => Some(own[..PIECE].to_vec()),
            Kind::Longer => Some([own.as_slice(), &[7; 500]].concat()),
            Kind::Gone => None,
        }
    }

    /// What's at its place as last put there by hand.
    fn mine(&self) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        (0..3).filter_map(|i| Some((PathBuf::from(NAMES[i]), self.bytes(i, self.kinds[i])?))).collect()
    }

    /// The torrent and its place as the actor has them now, then `change` made to the disk at that
    /// instant if it's still held or off the list: the engine does nothing meanwhile.
    async fn seen_with(&self, change: impl FnOnce() + Send + 'static) -> Seen {
        let (id, album) = (self.id, self.album());
        self.engine()
            .inspect(move |actor| {
                let torrent = actor.torrents.get(&id);
                let (listed, held) = (torrent.is_some(), torrent.and_then(|t| t.record.held()));
                // As it is before the change: what the engine decided on.
                let disk = super::moving::bytes_in(&album);
                if held.is_some() || !listed {
                    change();
                }
                Seen { listed, held, disk }
            })
            .await
    }

    async fn seen(&self) -> Seen {
        self.seen_with(|| {}).await
    }

    /// Waits until the engine has nothing more to do to the torrent, as far as can be told: two size
    /// checks in a row find it not being checked, moved or added, and as it was. Not while checks
    /// wait at the gate: a few size checks then.
    async fn settle(&self) {
        let id = self.id;
        let look = || async move {
            self.engine()
                .inspect(move |actor| {
                    let quiet = actor.torrents.get(&id).is_none_or(|t| {
                        let checking =
                            t.lib.torrent().is_some_and(|h| matches!(h.stats().state, librqbit::TorrentStatsState::Initializing { .. }));
                        let waits = actor.steps.machine().lanes.get(&id).is_some_and(|lane| !lane.is_empty());
                        matches!(
                            t.lifecycle.phase,
                            lifecycle::Phase::Ready
                                | lifecycle::Phase::MissingFiles { .. }
                                | lifecycle::Phase::Unchecked { .. }
                                | lifecycle::Phase::DestinationUnavailable { .. }
                                | lifecycle::Phase::Error { .. }
                        ) && matches!(t.lib, super::super::machine::lib::Lib::In(_) | super::super::machine::lib::Lib::Out)
                            && !checking
                            && !waits
                    });
                    let state = actor.torrents.get(&id).map(|t| (format!("{:?}", t.lifecycle), t.record.held()));
                    (quiet, state, actor.size_checks)
                })
                .await
        };
        let mut last = None;
        let mut same = 0;
        let mut at = look().await.2;
        for round in 0..400 {
            let (quiet, state, checks) = look().await;
            if checks < at + 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
                continue;
            }
            at = checks;
            same = if quiet && last.as_ref() == Some(&state) { same + 1 } else { 0 };
            last = Some(state);
            if same >= 2 || (self.gate.is_held() && round > 60) {
                return;
            }
        }
        panic!("never settled: {last:?}");
    }

    /// What a file there truly has of the torrent's own pieces, in bytes.
    fn has(&self, file: usize) -> u64 {
        match self.kinds[file] {
            Kind::Own | Kind::Longer => 2 * PIECE as u64,
            Kind::Short => PIECE as u64,
            Kind::Other | Kind::Zeros | Kind::Gone => 0,
        }
    }

    /// A file there holds a piece of the torrent's own content.
    fn real(&self) -> bool {
        (0..3).any(|i| self.has(i) > 0)
    }

    /// A file there isn't the torrent's own to write.
    fn not_its(&self) -> bool {
        (0..3).any(|i| self.kinds[i] == Kind::Longer || (self.linked[i] && self.kinds[i] != Kind::Gone))
    }

    /// Held or off the list: nothing at its place changed. Lifted: only as it may be, and `true`.
    async fn judge(&self, seen: &Seen, after: &str) -> Result<bool, TestCaseError> {
        if seen.held.is_some() || !seen.listed {
            prop_assert_eq!(&seen.disk, &self.mine(), "after {}: changed while held ({:?}) or off the list", after, seen.held);
            return Ok(false);
        }
        prop_assert!(
            !self.not_its(),
            "after {}: lifted with a file that isn't its own to write: {:?} {:?}",
            after,
            self.kinds,
            self.linked
        );
        prop_assert!(
            self.real() || self.chosen,
            "after {}: lifted with none of its pieces there, nothing chosen of what was read: {:?}",
            after,
            self.kinds
        );
        self.settle().await;
        let disk = super::moving::bytes_in(&self.album());
        for i in 0..3 {
            let (mine, now) = (self.bytes(i, self.kinds[i]), disk.get(Path::new(NAMES[i])));
            match (mine, now) {
                (None, None) => {}
                (None, Some(now)) => prop_assert!(!self.off[i] && now.len() == 2 * PIECE, "after {}: {} made wrongly", after, NAMES[i]),
                (Some(_), None) => prop_assert!(false, "after {}: {} is gone", after, NAMES[i]),
                (Some(mine), Some(now)) => {
                    prop_assert!(now.starts_with(&mine), "after {}: {}'s bytes changed", after, NAMES[i]);
                    prop_assert!(
                        now.len() == mine.len() || now.len() == 2 * PIECE,
                        "after {}: {}'s length is {}",
                        after,
                        NAMES[i],
                        now.len()
                    );
                }
            }
        }
        let t = find(self.engine(), self.id).await;
        let has: u64 = (0..3).filter(|i| !self.off[*i]).map(|i| self.has(i)).sum();
        prop_assert_eq!(
            t.content.map(|c| c.progress_bytes),
            Some(has),
            "after {}: has what isn't there: {:?}, {:?}",
            after,
            self.kinds,
            t.status
        );
        Ok(true)
    }

    async fn command(&self, command: TorrentCommand) {
        // Refused or not: what must hold does either way.
        let _ = tokio::time::timeout(Duration::from_secs(20), self.engine().command(self.id, command)).await.expect("answered");
    }

    /// Makes `step`; whether the case is over (its hold was lifted).
    async fn take(&mut self, step: &Step) -> Result<bool, TestCaseError> {
        let before = self.seen().await;
        if self.judge(&before, "the step before").await? {
            return Ok(true);
        }
        match step.clone() {
            Step::Hand { file, kind, replace } => {
                let (path, bytes, new, link) =
                    (self.album().join(NAMES[file]), self.bytes(file, kind), self.dir.path().join("new.bin"), self.link(file));
                let exists = self.album().exists();
                let was_linked = self.linked[file];
                let change = move || match bytes {
                    None => drop(std::fs::remove_file(&path)),
                    Some(bytes) if replace || !path.exists() => {
                        std::fs::write(&new, bytes).unwrap();
                        std::fs::rename(&new, &path).unwrap();
                    }
                    Some(bytes) => std::fs::write(&path, bytes).unwrap(),
                };
                if !exists {
                    return Ok(false);
                }
                let had = self.kinds[file];
                let seen = self.seen_with(change).await;
                if self.judge(&seen, "a change by hand").await? {
                    return Ok(true);
                }
                // Another file at the path, or none: what was linked is the old one.
                if was_linked && (kind == Kind::Gone || replace || had == Kind::Gone) {
                    let _ = std::fs::remove_file(link);
                    self.linked[file] = false;
                }
                self.kinds[file] = kind;
                self.reads.store(0, Ordering::SeqCst);
                self.chosen = false;
            }
            Step::Link { file, on } => {
                let (path, link) = (self.album().join(NAMES[file]), self.link(file));
                if on == self.linked[file] || (on && !path.exists()) {
                    return Ok(false);
                }
                let change = move || match on {
                    true => std::fs::hard_link(&path, &link).unwrap(),
                    false => std::fs::remove_file(&link).unwrap(),
                };
                let seen = self.seen_with(change).await;
                if !(seen.held.is_some() || !seen.listed) {
                    return self.judge(&seen, "a link").await;
                }
                self.linked[file] = on;
                self.reads.store(0, Ordering::SeqCst);
                self.chosen = false;
            }
            Step::Check => self.command(TorrentCommand::Check).await,
            Step::CancelCheck => self.command(TorrentCommand::CancelCheck).await,
            Step::Resume => {
                let dont_match = before.listed && before.held == Some(Held::DontMatch);
                // Every piece of the files there was read: two each.
                let there = 2 * self.kinds.iter().filter(|k| **k != Kind::Gone).count();
                self.chosen = dont_match && self.reads.load(Ordering::SeqCst) >= there && !self.gate.is_held();
                self.command(TorrentCommand::Resume).await;
            }
            Step::Pause => {
                self.chosen = false;
                self.command(TorrentCommand::Pause).await;
            }
            Step::Restart | Step::Crash => {
                self.gate.hold(false);
                let engine = self.engine.take().expect("running");
                match step {
                    Step::Crash => engine.kill().await,
                    _ => engine.stop().await,
                }
                drop(engine);
                self.chosen = false;
                self.start().await;
            }
            Step::DestinationGone | Step::DestinationBack => {
                let gone = matches!(step, Step::DestinationGone);
                if gone == self.away {
                    return Ok(false);
                }
                let (downloads, away) = (self.dir.path().join("downloads"), self.dir.path().join("away"));
                let seen = self
                    .seen_with(move || match gone {
                        true => std::fs::rename(&downloads, &away).unwrap(),
                        false => std::fs::rename(&away, &downloads).unwrap(),
                    })
                    .await;
                if !(seen.held.is_some() || !seen.listed) {
                    return self.judge(&seen, "its destination").await;
                }
                self.away = gone;
            }
            Step::Priority { file, off } => {
                let priority = if off { Priority::Off } else { Priority::Normal };
                let done = tokio::time::timeout(
                    Duration::from_secs(20),
                    self.engine().command(self.id, TorrentCommand::SetPriority { files: vec![self.index[file]], priority }),
                )
                .await
                .expect("answered");
                if done.is_ok() {
                    self.off[file] = off;
                }
            }
            Step::Remove { delete, undo } => {
                if !before.listed {
                    return Ok(false);
                }
                self.chosen = false;
                let command = if delete { TorrentCommand::Delete } else { TorrentCommand::Remove };
                let removed =
                    tokio::time::timeout(Duration::from_secs(20), self.engine().command(self.id, command)).await.expect("answered");
                if removed.is_ok() && undo {
                    self.settle().await;
                    let _ = tokio::time::timeout(Duration::from_secs(20), self.engine().undo()).await.expect("answered");
                }
            }
            Step::HoldChecks(held) => self.gate.hold(held),
        }
        self.settle().await;
        let seen = self.seen().await;
        self.judge(&seen, &format!("{step:?}")).await
    }

    /// In the end: its destination back, checks going on, a Check says whether its pieces are there,
    /// and "Download over them" lifts it once they aren't, unless a file isn't its own to write.
    async fn end(&mut self) -> Result<(), TestCaseError> {
        self.gate.hold(false);
        if self.take(&Step::DestinationBack).await? {
            return Ok(());
        }
        self.settle().await;
        let seen = self.seen().await;
        if !seen.listed || self.judge(&seen, "the end").await? {
            return Ok(());
        }
        self.chosen = false;
        self.command(TorrentCommand::Check).await;
        self.settle().await;
        let seen = self.seen().await;
        let lifted = self.judge(&seen, "the last Check").await?;
        prop_assert_eq!(lifted, self.real() && !self.not_its(), "a Check of {:?} {:?}: {:?}", self.kinds, self.linked, seen.held);
        if lifted {
            return Ok(());
        }
        // "Download over them", of what that Check read: twice, as one of a torrent Missing files
        // only checks it.
        for _ in 0..2 {
            let seen = self.seen().await;
            if seen.held != Some(Held::DontMatch) {
                break;
            }
            self.chosen = true;
            self.command(TorrentCommand::Resume).await;
            self.settle().await;
            let seen = self.seen().await;
            if self.judge(&seen, "the last Resume").await? {
                return Ok(());
            }
        }
        let seen = self.seen().await;
        prop_assert!(
            seen.held != Some(Held::DontMatch) || self.not_its(),
            "Download over them didn't lift {:?} {:?}: {:?}",
            self.kinds,
            self.linked,
            find(self.engine(), self.id).await.status
        );
        Ok(())
    }
}

/// A torrent of three files, running or `paused`, renamed in Finder to "Album" with `kinds` there,
/// found by Rename… and held; then `steps`.
async fn run(paused: bool, kinds: [Kind; 3], steps: Vec<Step>) -> Result<(), TestCaseError> {
    let dir = temp();
    let mut world = World {
        dir,
        engine: None,
        gate: Arc::default(),
        reads: Arc::default(),
        id: 0,
        own: Default::default(),
        kinds: [Kind::Own; 3],
        linked: [false; 3],
        off: [false; 3],
        index: [0; 3],
        away: false,
        chosen: false,
    };
    std::fs::create_dir_all(world.dir.path().join("links")).unwrap();
    world.start().await;
    let downloads = world.dir.path().join("downloads");
    let source = complete_torrent(world.dir.path()).await;
    world.id = add(world.engine(), &downloads, source, paused).await.unwrap().id;
    wait_for(world.engine(), world.id, if paused { Status::Done { reached: None } } else { Status::Seeding }).await;
    let details = world.engine().details(world.id).await.unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        world.own[i] = std::fs::read(downloads.join("pack").join(name)).unwrap();
        world.index[i] = details.files.iter().find(|f| f.path.ends_with(name)).unwrap().index;
    }
    std::fs::rename(downloads.join("pack"), world.album()).unwrap();
    for i in 0..3 {
        match world.bytes(i, kinds[i]) {
            Some(bytes) => std::fs::write(world.album().join(NAMES[i]), bytes).unwrap(),
            None => std::fs::remove_file(world.album().join(NAMES[i])).unwrap(),
        }
    }
    world.kinds = kinds;
    wait_for(world.engine(), world.id, Status::MissingFiles).await;
    world.reads.store(0, Ordering::SeqCst);
    let answer = world.engine().commanded(vec![world.id], TorrentCommand::Rename { name: FileName::new("Album").unwrap() }).await.unwrap();
    prop_assert_eq!(answer.found, vec![world.id]);
    world.settle().await;
    let mut over = false;
    for step in &steps {
        over = world.take(step).await?;
        if over {
            break;
        }
    }
    if !over {
        world.end().await?;
    }
    world.gate.hold(false);
    world.engine.take().expect("running").stop().await;
    Ok(())
}

fn replay(paused: bool, kinds: [Kind; 3], steps: Vec<Step>) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(run(paused, kinds, steps)).unwrap();
}

fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(16);
    ProptestConfig { cases, max_shrink_iters: 64, failure_persistence: None, ..ProptestConfig::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn nothing_changes_while_held_and_a_hold_is_lifted_only_on_what_was_read(
        paused in any::<bool>(),
        kinds in found(),
        steps in prop::collection::vec(step(), 0..10),
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(run(paused, kinds, steps))?;
    }
}

/// The third review's first three, as cases of the generated test.
#[test]
fn files_swapped_across_a_crash_or_a_restart_or_after_a_refused_resume_stay_held() {
    use Kind::*;
    let other = [Other, Other, Other];
    let swap = |kind| (0..3).map(move |file| Step::Hand { file, kind, replace: true });
    // 1: put right but one linked, checked, a crash, all replaced.
    let mut steps: Vec<Step> = swap(Own).collect();
    steps.extend([Step::Link { file: 2, on: true }, Step::Check, Step::Crash, Step::Link { file: 2, on: false }]);
    steps.extend(swap(Other));
    replay(false, other, steps);
    // 2: replaced while Phantom is closed is as replaced before it starts; then Resume.
    let mut steps: Vec<Step> = vec![Step::Restart];
    steps.extend(swap(Zeros));
    steps.extend([Step::Restart, Step::Resume]);
    replay(false, other, steps);
    // 3: a Resume refused for a longer file, then other files.
    for restart in [false, true] {
        let mut steps = vec![Step::Hand { file: 2, kind: Longer, replace: false }, Step::Check, Step::Resume];
        steps.extend(restart.then_some(Step::Restart));
        steps.extend(swap(Zeros));
        replay(false, other, steps);
    }
}
