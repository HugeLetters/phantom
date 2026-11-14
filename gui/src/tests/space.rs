//! Free space (#127), generated: three torrents downloading to one disk with 5.5 GiB free, of 3, 4
//! and 6 GiB, so no sum is near the edge (and a few MiB downloaded meanwhile change none). Steps
//! pause and resume them, set their files Off and back, remove them, and open the disk's warning,
//! close it and Pause from it. After every step:
//! - the disk's warning shows exactly while the torrents wanted running need more than is free
//!   together, files set Off not counting;
//! - a torrent's own warning (its status in the warning colour, and a warning icon) shows exactly
//!   while it alone needs more;
//! - the warning's list is open only while it's asked for and its disk is short;
//! - nothing is ever refused for space.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{Priority, Status, TorrentCommand};
use rune::Runtime;

use super::{app, settle, tap};
use crate::model::{Phantom, command};
use crate::service::MemoryService;

const GIB: u64 = 1 << 30;
const SIZES: [u64; 3] = [3 * GIB, 4 * GIB, 6 * GIB];
const FREE: u64 = 11 * GIB / 2;

#[derive(Debug, Clone)]
enum Step {
    Pause(usize),
    Resume(usize),
    /// Its one file Off, or back to Normal.
    Off(usize, bool),
    Remove(usize),
    /// A click on the disk's warning.
    Open,
    Close,
    /// Pause in the open list.
    PauseInList(usize),
}

#[derive(Debug, Clone, Default)]
struct Model {
    running: [bool; 3],
    off: [bool; 3],
    removed: [bool; 3],
    open: bool,
}

impl Model {
    fn needs(&self, i: usize) -> u64 {
        if self.running[i] && !self.off[i] && !self.removed[i] { SIZES[i] } else { 0 }
    }

    fn short(&self) -> bool {
        (0..3).map(|i| self.needs(i)).sum::<u64>() > FREE
    }

    fn apply(&mut self, step: &Step) {
        match *step {
            Step::Pause(i) | Step::PauseInList(i) => self.running[i] = false,
            Step::Resume(i) => self.running[i] = true,
            Step::Off(i, off) => self.off[i] = off,
            Step::Remove(i) => self.removed[i] = true,
            // Open, a click on the warning opens its list; open, it lands outside the list.
            Step::Open => self.open = !self.open,
            Step::Close => self.open = false,
        }
        self.open &= self.short();
    }

    /// What can be done: a removed torrent is gone; the warning and its list are there only
    /// while drawn.
    fn can(&self, step: &Step) -> bool {
        match *step {
            Step::Pause(i) | Step::Resume(i) | Step::Off(i, _) | Step::Remove(i) => !self.removed[i],
            Step::Open => self.short(),
            Step::Close => true,
            Step::PauseInList(i) => self.open && self.needs(i) > 0,
        }
    }
}

fn step() -> impl Strategy<Value = Step> {
    let i = 0..3usize;
    prop_oneof![
        2 => i.clone().prop_map(Step::Pause),
        3 => i.clone().prop_map(Step::Resume),
        2 => (i.clone(), any::<bool>()).prop_map(|(i, off)| Step::Off(i, off)),
        1 => i.clone().prop_map(Step::Remove),
        3 => Just(Step::Open),
        1 => Just(Step::Close),
        2 => i.prop_map(Step::PauseInList),
    ]
}

fn start() -> Runtime<Phantom> {
    let torrents = (0..3).map(|i| crate::t(i as u64, &format!("T{i}"), Status::Downloading, 0, SIZES[i], 0, 0, 0)).collect();
    let mut rt = app(MemoryService::new().with_torrents(torrents).with_free_space(FREE));
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    rt
}

fn run(rt: &mut Runtime<Phantom>, step: &Step) {
    let id = |i: usize| i as u64;
    match *step {
        Step::Pause(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Pause)),
        Step::Resume(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Resume)),
        Step::Off(i, off) => {
            let priority = if off { Priority::Off } else { Priority::Normal };
            rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::SetPriority { files: vec![0], priority }));
        }
        Step::Remove(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Remove)),
        Step::Open => tap(rt, "notice"),
        Step::Close => rt.update(|s, _| {
            s.bar.close();
        }),
        Step::PauseInList(i) => tap(rt, &format!("space-pause/{}", id(i))),
    }
    settle(rt);
    settle(rt);
}

fn check(rt: &Runtime<Phantom>, m: &Model) {
    assert_eq!(rt.find("notice").is_some(), m.short(), "the disk's warning shows exactly while short");
    assert!(rt.state.bar.entries().len() <= 1, "one disk, one warning");
    // A torrent that alone needs more keeps its status text, drawn in the warning colour with a
    // warning icon (the same colour) after it; no other row has either.
    let warning = crate::ui::Ui::new(true).tone_color(crate::ui::Tone::Warning);
    let elements = rt.elements();
    let icons: Vec<usize> =
        elements.iter().filter(|e| e.role == "icon").filter_map(|e| e.key.as_deref()?.strip_prefix("too-big/")?.parse().ok()).collect();
    let too_big: Vec<usize> = (0..3).filter(|&i| m.needs(i) > FREE).collect();
    assert_eq!(icons, too_big, "a torrent's own warning icon exactly while it alone needs more");
    assert!(
        elements.iter().filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("too-big/"))).all(|e| e.text_color == Some(warning)),
        "the icon is in the warning colour"
    );
    let statuses = ["Downloading", "Paused"];
    let warned =
        elements.iter().filter(|e| e.text.as_deref().is_some_and(|t| statuses.contains(&t)) && e.text_color == Some(warning)).count();
    assert_eq!(warned, too_big.len(), "the status text is in the warning colour exactly for those");
    assert!(!elements.iter().any(|e| e.text.as_deref().is_some_and(|t| t.starts_with("Won't fit"))), "the status keeps its own words");
    assert_eq!(rt.find("space-list").is_some(), m.open, "the list is open while asked for and short");
    let needed: Vec<u64> = rt.state.volumes.iter().flat_map(|v| v.needs.iter().map(|n| n.id)).collect();
    let expected: Vec<u64> = (0..3).filter(|&i| m.needs(i) > 0).map(|i| i as u64).collect();
    assert_eq!(needed, expected, "which torrents need space: wanted running, files Off not counting");
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32);
    Config { cases, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn free_space_warnings_follow_what_the_torrents_need(steps in proptest::collection::vec(step(), 1..16)) {
        let mut rt = start();
        let mut m = Model { running: [true; 3], ..Model::default() };
        check(&rt, &m);
        for step in &steps {
            if !m.can(step) {
                continue;
            }
            run(&mut rt, step);
            m.apply(step);
            check(&rt, &m);
        }
    }
}
