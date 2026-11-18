//! Free space (#127, #209), generated: three torrents downloading to one disk with 5.5 GiB free, of
//! 3, 4 and 6 GiB, so no sum is near the edge (and a few MiB downloaded meanwhile change none).
//! Steps pause and resume them, set their files Off and back, remove them, and free space on the
//! disk or fill it again. After every step:
//! - the disk is short exactly while the torrents wanted running need more than is free together,
//!   files set Off not counting;
//! - then every torrent that still needs space has the ⚠ in its status cell, in the warning
//!   colour, and on hover the disk's whole picture (how much is free, this torrent and how many
//!   others still need how much), and no other row has either; free the space, and they all go;
//! - nothing is ever refused for space.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{Priority, Status, TorrentCommand};
use rune::Runtime;

use super::{app, settle};
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
    /// Space is freed on the disk (more than all three need), or filled again (back to what it was).
    Room(bool),
}

#[derive(Debug, Clone, Default)]
struct Model {
    running: [bool; 3],
    off: [bool; 3],
    removed: [bool; 3],
    room: bool,
}

/// What's free once space is freed: more than the torrents need together.
const ROOMY: u64 = 20 * GIB;

impl Model {
    fn needs(&self, i: usize) -> u64 {
        if self.running[i] && !self.off[i] && !self.removed[i] { SIZES[i] } else { 0 }
    }

    fn free(&self) -> u64 {
        if self.room { ROOMY } else { FREE }
    }

    fn short(&self) -> bool {
        (0..3).map(|i| self.needs(i)).sum::<u64>() > self.free()
    }

    /// The torrents the disk's warning is on: every one that still needs space, while it's short.
    fn warned(&self) -> Vec<usize> {
        if self.short() { (0..3).filter(|&i| self.needs(i) > 0).collect() } else { Vec::new() }
    }

    fn apply(&mut self, step: &Step) {
        match *step {
            Step::Pause(i) => self.running[i] = false,
            Step::Resume(i) => self.running[i] = true,
            Step::Off(i, off) => self.off[i] = off,
            Step::Remove(i) => self.removed[i] = true,
            Step::Room(room) => self.room = room,
        }
    }

    /// What can be done: a removed torrent is gone.
    fn can(&self, step: &Step) -> bool {
        match *step {
            Step::Pause(i) | Step::Resume(i) | Step::Off(i, _) | Step::Remove(i) => !self.removed[i],
            Step::Room(_) => true,
        }
    }
}

fn step() -> impl Strategy<Value = Step> {
    let i = 0..3usize;
    prop_oneof![
        2 => i.clone().prop_map(Step::Pause),
        3 => i.clone().prop_map(Step::Resume),
        2 => (i.clone(), any::<bool>()).prop_map(|(i, off)| Step::Off(i, off)),
        1 => i.prop_map(Step::Remove),
        2 => any::<bool>().prop_map(Step::Room),
    ]
}

fn start() -> (Runtime<Phantom>, crate::service::Disk) {
    let torrents = (0..3).map(|i| crate::t(i as u64, &format!("T{i}"), Status::Downloading, 0, SIZES[i], 0, 0, 0)).collect();
    let service = MemoryService::new().with_torrents(torrents).with_free_space(FREE);
    let disk = service.disk();
    let mut rt = app(service);
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    (rt, disk)
}

fn run(rt: &mut Runtime<Phantom>, disk: &crate::service::Disk, step: &Step) {
    let id = |i: usize| i as u64;
    match *step {
        Step::Pause(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Pause)),
        Step::Resume(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Resume)),
        Step::Off(i, off) => {
            let priority = if off { Priority::Off } else { Priority::Normal };
            rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::SetPriority { files: vec![0], priority }));
        }
        Step::Remove(i) => rt.update(move |s, _| command(&mut *s.service, &[id(i)], TorrentCommand::Remove)),
        Step::Room(room) => disk.change(crate::service::DiskChange::FreeBytes(if room { ROOMY } else { FREE })),
    }
    settle(rt);
    settle(rt);
}

fn check(rt: &Runtime<Phantom>, m: &Model) {
    let warned = m.warned();
    let elements = rt.elements();
    let icons: Vec<usize> = elements
        .iter()
        .filter(|e| e.role == "icon")
        .filter_map(|e| e.key.as_deref()?.strip_prefix("short-on-space/")?.parse().ok())
        .collect();
    assert_eq!(icons, warned, "the ⚠ on exactly the torrents that still need space, while the disk is short");
    // Every warned row says the disk's picture on hover (or its own need); the others say nothing of it.
    let mut hovers: Vec<&str> = elements.iter().filter_map(|e| e.help.as_deref()).filter(|h| h.contains(" is short on space: ")).collect();
    hovers.sort();
    let free = client::display::bytes(m.free());
    let needs = client::display::bytes((0..3).map(|i| m.needs(i)).sum());
    let others = match warned.len() {
        0 | 1 => "this torrent still needs".to_owned(),
        2 => "this and 1 other torrent still need".to_owned(),
        n => format!("this and {} other torrents still need", n - 1),
    };
    // One that alone needs more than is free says so of itself; the rest, the disk's picture.
    let mut expected: Vec<String> = warned
        .iter()
        .map(|&i| match m.needs(i) > m.free() {
            true => {
                format!("Macintosh HD is short on space: {free} free; this torrent still needs {}.", client::display::bytes(m.needs(i)))
            }
            false => format!("Macintosh HD is short on space: {free} free; {others} {needs}."),
        })
        .collect();
    expected.sort();
    assert_eq!(hovers, expected, "the hover on each warned row");
    // The warning icon and the status text beside it are in the warning colour, for those rows only.
    let warning = crate::ui::Ui::new(true).tone_color(crate::ui::Tone::Warning);
    assert!(
        elements
            .iter()
            .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("short-on-space/")))
            .all(|e| e.text_color == Some(warning)),
        "the icon is in the warning colour"
    );
    let statuses = ["Downloading", "Paused"];
    let coloured =
        elements.iter().filter(|e| e.text.as_deref().is_some_and(|t| statuses.contains(&t)) && e.text_color == Some(warning)).count();
    assert_eq!(coloured, warned.len(), "the status text is in the warning colour exactly for those");
    assert!(!elements.iter().any(|e| e.text.as_deref().is_some_and(|t| t.starts_with("Won't fit"))), "the status keeps its own words");
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
        let (mut rt, disk) = start();
        let mut m = Model { running: [true; 3], ..Model::default() };
        check(&rt, &m);
        for step in &steps {
            if !m.can(step) {
                continue;
            }
            run(&mut rt, &disk, step);
            m.apply(step);
            check(&rt, &m);
        }
    }
}

/// An example, not a property (the generated steps above cover several torrents, the wording a
/// property of `display`): a torrent alone on a disk too small for it says "this torrent".
#[test]
fn a_torrent_alone_on_a_disk_too_small_for_it_says_this_torrent() {
    let torrents = vec![crate::t(0, "T0", Status::Downloading, 0, 6 * GIB, 0, 0, 0)];
    let mut rt = app(MemoryService::new().with_torrents(torrents).with_free_space(FREE));
    rt.set_viewport((960.0, 600.0), 2.0);
    settle(&mut rt);
    let hover = |rt: &Runtime<Phantom>| {
        rt.elements().into_iter().filter_map(|e| e.help).filter(|h| h.contains("short on space")).collect::<Vec<_>>()
    };
    assert_eq!(hover(&rt), ["Macintosh HD is short on space: 5.5 GiB free; this torrent still needs 6.0 GiB."]);
}
