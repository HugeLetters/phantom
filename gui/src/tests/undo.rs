//! Undo in the status bar's notices (#150, #151), generated: Removes and Deletes of one row or of all
//! of them, ⌘⌫ on a torrent's file with the file list focused (which moves it to the Trash and sets
//! it Off), ⌘Z, and a person opening the notice of a past Remove, Delete or Move to Trash and
//! pressing its Undo. A reference model knows the rows, which files are in the Trash, the undo
//! stack (each action, newest last: one press, one entry, its rows all of it, #235) and the notices
//! told. After every step:
//! - the list is the model's rows;
//! - the bar's notices are the model's, in order: a Remove or Delete tells "Removed <name>",
//!   "Deleted <name>" or "Removed 3 torrents"; an undo, what came back;
//!   A timed notice (a receipt, 5 s; an act's, a minute) leaves once the list has been closed that
//!   long, and the steps' frames pass the app's clock, so the model ages each notice by the time
//!   the steps take (a Press, which opens and closes the list, starts every clock again; one whose
//!   Undo takes the list's only notice closes it right then, before what came back is told);
//! - one ⌘Z brings back all of the newest action's rows, as its own Undo does, and its notice leaves;
//!   the "is back" notices stay;
//! - a notice's Undo is on exactly while its Remove or Delete is the newest on the stack. Pressed
//!   then, its rows come back and the notice gives way to what came back. Pressed while off,
//!   nothing changes and the notice says why. It shows ⌘Z beside it exactly while ⌘Z would take
//!   the same action: another client's settings change (`phantom settings set`) is newer, and ⌘Z
//!   takes that first (#255), opening Settings at it, which the person then closes.
//!
//! 64 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::collections::BTreeSet;

use client::display::{self, UndoOff};
use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{Named, TorrentId, UndoAction, Undoable};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{Desk, Showing, app, desk, settle, tap, torrent, wait};
use crate::bar::machine::{MAX_TOLD, Way};
use crate::model::{Phantom, selected_torrents};
use crate::service::MemoryService;
use crate::shortcuts::{self, Command};
use crate::windows::Which;

const ROWS: u64 = 4;

#[derive(Debug, Clone)]
enum Step {
    /// ⌫ or ⌘⌫ on the `n`-th row there is, or on every one (⌘A).
    Go { n: usize, all: bool, delete: bool },
    /// ⌘⌫ with the file list focused, on the only file of the `n`-th row there is.
    Trash { n: usize },
    /// ⌘Z.
    Z,
    /// The Undo in the `n`-th notice of a Remove or Delete still told.
    Press { n: usize },
    /// Another client switches keeping the Mac awake.
    Awake,
}

/// A change ⌘Z takes back: an act, or another client's switch of keeping the Mac awake, which was
/// on or off before.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    Act(usize),
    Awake(bool),
}

/// A Remove or Delete done: its rows, and whether its notice is still in the bar.
#[derive(Debug, Clone)]
struct Act {
    action: UndoAction,
    ids: Vec<TorrentId>,
}

/// A notice the bar holds: a Remove or Delete's, by act, or another, by what it says.
#[derive(Debug, Clone, PartialEq)]
enum Told {
    Act(usize),
    Said(String),
}

/// A notice the bar holds, and how long its clock may have run. A timed notice leaves by itself
/// once the list has been closed for its time (a receipt 5 s, an act's 60 s: `crate::notice`), and
/// the test's frames pass the app's clock, so the model keeps each notice's age: the time since it
/// was told or the list last closed, as at least `lo` and at most `hi` (a step tells its notice
/// somewhere in its frames).
#[derive(Debug, Clone)]
struct Held {
    told: Told,
    lo: f64,
    hi: f64,
    /// Told in the step now running: its age is set when the step ends.
    new: bool,
}

/// When a step closed the list, and whether what it told came after: an Undo that takes the only
/// notice the list has closes it with that notice, before the undo tells what came back.
#[derive(Debug, Clone, Copy)]
struct Closed {
    at: f64,
    told_after: bool,
}

#[derive(Default)]
struct Model {
    rows: BTreeSet<TorrentId>,
    acts: Vec<Act>,
    /// The undo stack, newest last: each entry's act.
    stack: Vec<usize>,
    told: Vec<Held>,
    /// The torrents whose file is in the Trash, and Off.
    gone: BTreeSet<TorrentId>,
    /// Every change ⌘Z takes back, newest last, and whether the Mac is kept awake.
    changes: Vec<Change>,
    awake: bool,
}

fn name(id: TorrentId) -> String {
    format!("Torrent {id}")
}

impl Model {
    fn tell(&mut self, told: Told) {
        self.told.push(Held { told, lo: 0.0, hi: 0.0, new: true });
        if self.told.len() > MAX_TOLD {
            self.told.remove(0);
        }
    }

    /// How long a notice's clock runs once the list is closed, as `Lasts` says: a receipt's, or an
    /// act's.
    fn secs(told: &Told) -> f64 {
        match told {
            Told::Act(_) => crate::notice::LONG_SECS,
            Told::Said(_) => crate::notice::BRIEF_SECS,
        }
    }

    /// A step ran from `t0` to `t1` on the app's clock. A notice told in it is between 0 and that
    /// old; the others are older by it. When the step closed the list, closing gave every notice
    /// its full time again: each is as old as the time since, but one told after it closed, which
    /// is between 0 and that old.
    fn pass(&mut self, t0: f64, t1: f64, closed: Option<Closed>) {
        for h in &mut self.told {
            match closed {
                Some(Closed { at, told_after: true }) if h.new => (h.lo, h.hi) = (0.0, t1 - at),
                Some(Closed { at, .. }) => (h.lo, h.hi) = (t1 - at, t1 - at),
                None if h.new => (h.lo, h.hi) = (0.0, t1 - t0),
                None => (h.lo, h.hi) = (h.lo + (t1 - t0), h.hi + (t1 - t0)),
            }
            h.new = false;
        }
    }

    /// The notices the bar shows now, `shorts`, against the model's. One whose clock must have
    /// run out is gone, one whose clock can't have is there, in order. A clock starts at the first
    /// frame after its notice was told or the list closed, so one that's within two frames of
    /// running out may be either; then the bar says which, and the model follows it.
    fn reconcile(&mut self, shorts: &[String], step: &Step) {
        let sure = |h: &Held| h.hi < Self::secs(&h.told);
        let gone = |h: &Held| h.lo >= Self::secs(&h.told) + 2.0 * super::FRAME;
        let held: Vec<Held> = std::mem::take(&mut self.told).into_iter().filter(|h| !gone(h)).collect();
        let mut kept = Vec::new();
        let mut at = 0;
        for (i, h) in held.iter().enumerate() {
            let short = self.short(&h.told);
            let sure_after = held[i + 1..].iter().filter(|h| sure(h)).count();
            let there = shorts.get(at) == Some(&short);
            if sure(h) || (there && shorts.len() - at > sure_after) {
                assert!(there, "the notices after {step:?}: {short:?} should be there, the bar has {shorts:?}");
                at += 1;
                kept.push(h.clone());
            }
        }
        assert_eq!(at, shorts.len(), "the notices after {step:?}: the bar has {shorts:?}, more than the model's {:?}", kept);
        self.told = kept;
    }

    fn short(&self, told: &Told) -> String {
        match told {
            Told::Act(a) => display::happened(self.acts[*a].action, self.acts[*a].ids.len()),
            Told::Said(said) => said.clone(),
        }
    }

    /// Whether act `a` is the newest on the stack.
    fn on_top(&self, a: usize) -> bool {
        self.stack.last() == Some(&a)
    }

    /// The newest act comes back, all of it: its rows, or its file; its notice leaves (#200), and
    /// what came back is told.
    fn undo(&mut self) {
        let Some(a) = self.stack.pop() else { return };
        self.changes.retain(|c| *c != Change::Act(a));
        for &id in &self.acts[a].ids {
            match self.acts[a].action {
                UndoAction::TrashFiles { .. } => drop(self.gone.remove(&id)),
                UndoAction::Remove | UndoAction::Delete => drop(self.rows.insert(id)),
                // No act renames: the Rename's own tests are `tests/renaming.rs`.
                UndoAction::Rename => {}
            }
        }
        self.told.retain(|h| h.told != Told::Act(a));
        let mut ids = self.acts[a].ids.clone();
        ids.sort();
        let undone =
            Undoable { seq: 0, action: self.acts[a].action, torrents: ids.into_iter().map(|id| Named { id, name: name(id) }).collect() };
        self.tell(Told::Said(display::undone_line(&undone)));
    }

    /// ⌘Z: the newest change, an act or a setting's.
    fn z(&mut self) {
        match self.changes.last().copied() {
            Some(Change::Act(_)) => self.undo(),
            Some(Change::Awake(was)) => {
                self.changes.pop();
                self.awake = was;
                self.tell(Told::Said(display::setting_undone_short(display::setting_short("keep_awake"))));
            }
            None => {}
        }
    }
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let step = prop_oneof![
        4 => (0..ROWS as usize, prop::bool::weighted(0.2), any::<bool>()).prop_map(|(n, all, delete)| Step::Go { n, all, delete }),
        3 => (0..ROWS as usize).prop_map(|n| Step::Trash { n }),
        2 => Just(Step::Z),
        4 => (0..MAX_TOLD).prop_map(|n| Step::Press { n }),
        1 => Just(Step::Awake),
    ];
    prop::collection::vec(step, 1..14)
}

/// Frames until the fake has answered everything asked.
fn quiet(rt: &mut Runtime<Phantom>) {
    for _ in 0..ROWS + 2 {
        settle(rt);
    }
}

fn key(rt: &mut Runtime<Phantom>, ev: KeyEvent) {
    rt.update(|_, tx| tx.blur());
    rt.key(ev);
    quiet(rt);
}

fn run(steps: &[Step]) {
    let service = MemoryService::new().with_torrents((0..ROWS).map(|id| torrent(id, &name(id))).collect());
    let disk = service.disk();
    let mut d = desk(service);
    desk_quiet(&mut d);
    let mut model = Model { rows: (0..ROWS).collect(), ..Model::default() };
    for step in steps {
        let t0 = d.main.clock;
        let mut closed = None;
        match *step {
            Step::Go { n, all, delete } => {
                let Some(&id) = model.rows.iter().nth(n % model.rows.len().max(1)) else { continue };
                let ids: Vec<TorrentId> = if all { model.rows.iter().copied().collect() } else { vec![id] };
                tap(&mut d.main, &format!("rows/{id}"));
                if all {
                    d.main.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                    settle(&mut d.main);
                }
                // In the order the app sends them, which is the stack's.
                let sent = selected_torrents(&d.main.state.table);
                assert_eq!(sent.iter().copied().collect::<BTreeSet<_>>(), ids.iter().copied().collect());
                let command = if delete { Command::Delete } else { Command::Remove };
                key_in(&mut d, shortcuts::keys(command)[0].event());
                let action = if delete { UndoAction::Delete } else { UndoAction::Remove };
                let ids = sent;
                for id in &ids {
                    model.rows.remove(id);
                }
                model.acts.push(Act { action, ids: ids.clone() });
                let a = model.acts.len() - 1;
                model.stack.push(a);
                model.changes.push(Change::Act(a));
                model.tell(Told::Act(a));
            }
            Step::Trash { n } => {
                let Some(&id) = model.rows.iter().nth(n % model.rows.len().max(1)) else { continue };
                tap(&mut d.main, &format!("rows/{id}"));
                // The file list takes the focus, so ⌘⌫ is its own: the file goes, never the torrent.
                tap(&mut d.main, "files/viewport/rows/0");
                d.main.key(shortcuts::keys(Command::Delete)[0].event());
                desk_quiet(&mut d);
                // Nothing is moved that's gone already: no step on the stack, nothing told.
                if model.gone.insert(id) {
                    model.acts.push(Act { action: UndoAction::TrashFiles { count: 1 }, ids: vec![id] });
                    let a = model.acts.len() - 1;
                    model.stack.push(a);
                    model.changes.push(Change::Act(a));
                    model.tell(Told::Act(a));
                }
            }
            Step::Z => {
                d.main.update(|_, tx| tx.blur());
                d.key(shortcuts::keys(Command::Undo)[0].event());
                desk_quiet(&mut d);
                // A setting's undo opens Settings at it, in front: closed again, as a person would.
                let opens = matches!(model.changes.last(), Some(Change::Awake(_)));
                assert_eq!(d.settings.state.sheet.is_some(), opens, "Settings opens for a setting's undo");
                assert_eq!(
                    d.shown(),
                    Showing { main: true, settings: opens, front: Some(if opens { Which::Settings } else { Which::Main }) }
                );
                if opens {
                    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                    desk_quiet(&mut d);
                    assert_eq!(d.shown(), Showing { main: true, settings: false, front: Some(Which::Main) }, "Escape closed it");
                }
                model.z();
            }
            Step::Awake => {
                let disk = disk.clone();
                disk.change(crate::service::DiskChange::SetByAnother(proto::Setting::KeepAwake(Some(!model.awake))));
                desk_quiet(&mut d);
                model.changes.push(Change::Awake(model.awake));
                model.awake = !model.awake;
            }
            Step::Press { n } => {
                let acts: Vec<(usize, usize)> = model
                    .told
                    .iter()
                    .enumerate()
                    .filter_map(|(at, h)| if let Told::Act(a) = h.told { Some((at, a)) } else { None })
                    .collect();
                let Some(&(at, a)) = acts.get(n % acts.len().max(1)) else { continue };
                let target = d.main.state.bar.entries()[at].id;
                d.main.update(move |s, _| {
                    if !s.bar.is_open() {
                        s.bar.press();
                    }
                    while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Older) {}
                    while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Newer) {}
                });
                settle(&mut d.main);
                assert_eq!(d.main.state.bar.shown().map(|(e, ..)| e.id), Some(target), "the popover shows the act's notice");
                let on = model.on_top(a);
                let hint = d.main.find("notice-action-hint").map(|_| ());
                assert_eq!(
                    hint.is_some(),
                    model.changes.last() == Some(&Change::Act(a)),
                    "⌘Z shows beside its Undo exactly while ⌘Z takes it"
                );
                tap(&mut d.main, "notice-action");
                // Its notice the only one, it went at once and the list closed with it (#200).
                let closed_itself = !d.main.state.bar.is_open();
                let tapped = d.main.clock;
                desk_quiet(&mut d);
                if on {
                    model.undo();
                } else {
                    let why = d.main.state.why_off_note.clone();
                    assert!(why.is_some() && d.main.find("notice-action-why").is_some(), "an Undo that's off says why when pressed");
                }
                d.main.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                closed = Some(match closed_itself {
                    true => Closed { at: tapped, told_after: true },
                    false => Closed { at: d.main.clock, told_after: false },
                });
                settle(&mut d.main);
            }
        }
        model.pass(t0, d.main.clock, closed);
        let listed: BTreeSet<TorrentId> = d.main.state.torrents.iter().map(|t| t.id).collect();
        assert_eq!(listed, model.rows, "the list after {step:?}");
        // Each torrent's one file is Off, and so out of its Size, exactly while it's in the Trash, and
        // moving it never leaves the torrent missing a file it gets.
        for t in &d.main.state.torrents {
            assert_eq!(t.content.is_some_and(|c| c.size_bytes == 0), model.gone.contains(&t.id), "torrent {} after {step:?}", t.id);
            assert_ne!(t.status, proto::Status::MissingFiles, "torrent {} after {step:?}", t.id);
        }
        let shorts: Vec<String> = d.main.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect();
        model.reconcile(&shorts, step);
    }
}

/// Frames until the fake has answered everything asked, and each window ran what the other posted.
fn desk_quiet(d: &mut Desk) {
    for _ in 0..ROWS + 2 {
        d.settle();
    }
}

/// A key in the window in front, the focus off any field.
fn key_in(d: &mut Desk, ev: KeyEvent) {
    d.main.update(|_, tx| tx.blur());
    d.key(ev);
    desk_quiet(d);
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(64);
    Config { cases, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_notices_undo_takes_back_its_remove_or_delete_while_it_is_the_newest(steps in steps()) {
        run(&steps);
    }
}

// Undo's odd cases (the audit of #159), as examples.

fn two_rows() -> Runtime<Phantom> {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]));
    quiet(&mut rt);
    rt
}

fn listed(rt: &Runtime<Phantom>) -> Vec<TorrentId> {
    let mut ids: Vec<TorrentId> = rt.state.torrents.iter().map(|t| t.id).collect();
    ids.sort();
    ids
}

fn remove_row(rt: &mut Runtime<Phantom>, id: TorrentId) {
    tap(rt, &format!("rows/{id}"));
    key(rt, shortcuts::keys(Command::Remove)[0].event());
}

fn shorts(rt: &Runtime<Phantom>) -> Vec<String> {
    rt.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect()
}

/// 6. ⌘Z with the link field focused and holding text: the field has the key, so the Remove stays;
/// the text is the field's own to undo. With the field left, ⌘Z undoes the Remove.
#[test]
fn cmd_z_in_the_link_field_is_the_fields_and_leaves_the_remove() {
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    assert_eq!(listed(&rt), [1]);

    tap(&mut rt, "magnet-field");
    rt.paste("magnet:?xt=urn:btih:00000000000000000000000000000000000000aa");
    settle(&mut rt);
    let typed = rt.state.magnet.text.clone();
    assert!(!typed.is_empty());
    let before = shorts(&rt);
    rt.key(shortcuts::keys(Command::Undo)[0].event());
    quiet(&mut rt);
    assert_eq!(listed(&rt), [1], "the focused field took the key: the Remove isn't undone");
    assert_eq!(shorts(&rt), before, "and nothing is told");

    key(&mut rt, shortcuts::keys(Command::Undo)[0].event());
    assert_eq!(listed(&rt), [0, 1], "with no field focused ⌘Z undoes the Remove");
}

/// 7. The Undo in an old notice, after a newer Remove: off, and pressed it says why, in
/// `display`'s words; nothing comes back. The newer notice's Undo is on, and after it the older's.
#[test]
fn an_old_notices_undo_is_off_after_a_newer_remove_and_says_why() {
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    assert_eq!(shorts(&rt), ["Removed", "Removed"]);
    let open = |rt: &mut Runtime<Phantom>, at: usize| {
        let target = rt.state.bar.entries()[at].id;
        rt.update(move |s, _| {
            if !s.bar.is_open() {
                s.bar.press();
            }
            while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Older) {}
            while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Newer) {}
        });
        settle(rt);
    };
    open(&mut rt, 0);
    tap(&mut rt, "notice-action");
    quiet(&mut rt);
    assert!(listed(&rt).is_empty(), "nothing came back");
    assert_eq!(rt.state.why_off_note.as_deref(), Some(display::undo_off(&display::UndoOff::Newer).as_str()));
    assert!(rt.find("notice-action-why").is_some(), "the reason shows under the button");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);

    open(&mut rt, 1);
    tap(&mut rt, "notice-action");
    quiet(&mut rt);
    assert_eq!(listed(&rt), [1]);
}

/// 1 (decided). An undo of a torrent added again by hand does nothing to the list and says so, short:
/// "Already in the list", the name in its list; its entry is dropped, so the next ⌘Z is the one under it.
#[test]
fn an_undo_of_a_torrent_added_again_says_already_in_the_list() {
    let refused = |why| {
        client::Failure::Refused(proto::Refusal::Undo {
            undo: Undoable { seq: 1, action: UndoAction::Remove, torrents: vec![Named { id: 3, name: "Album".into() }] },
            why,
            stranded: Vec::new(),
        })
    };
    let notice = crate::model::undo_notices(&Err(refused(proto::CantUndo::InList))).pop().expect("a notice");
    assert_eq!((notice.short.as_str(), notice.severity), (display::already_in_list(), crate::notice::Severity::Info));
    assert_eq!(display::already_in_list(), "Already in the list");
    assert!(notice.more.is_some(), "the torrent's name is the list's");
    // Others still say why they couldn't.
    let other = crate::model::undo_notices(&Err(refused(proto::CantUndo::NotKept))).pop().expect("a notice");
    assert_eq!(other.short, "Couldn't undo the Remove");
}

/// 1, on the app: the fake's torrent added again by hand (its info hash the same) while its Remove is
/// on the stack: ⌘Z tells "Already in the list", the added one stays, and a second ⌘Z has nothing.
#[test]
fn cmd_z_after_the_same_torrent_was_added_again_tells_so_and_touches_nothing() {
    // The row's info hash is the magnet link's, so adding the link adds it again.
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0xaa, "Big Buck Bunny")]));
    quiet(&mut rt);
    remove_row(&mut rt, 0xaa);
    tap(&mut rt, "magnet-field");
    rt.paste(super::MAGNET);
    settle(&mut rt);
    tap(&mut rt, "add");
    quiet(&mut rt);
    let rows = listed(&rt);
    assert_eq!(rows.len(), 1, "added again, as another torrent");
    key(&mut rt, shortcuts::keys(Command::Undo)[0].event());
    assert_eq!(listed(&rt), rows, "nothing changed");
    assert_eq!(shorts(&rt).last().map(String::as_str), Some("Already in the list"));
    let told = shorts(&rt).len();
    key(&mut rt, shortcuts::keys(Command::Undo)[0].event());
    assert_eq!(shorts(&rt).len(), told, "the entry was dropped: nothing more to undo");
}

/// 5 (#190). Two ⌘Z while slow undos run: each takes the next step down at once, so both torrents
/// are listed again straight away, and both put-backs run side by side: each tells it's back once it
/// is. A third press, with nothing more on the stack, is nothing. Nothing says "Undoing…". The
/// notices come as the put-backs end, side by side, so in no set order: they're compared sorted.
#[test]
fn cmd_z_pressed_twice_while_undos_run_lists_both_at_once() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]).undo_takes(3.0);
    let mut rt = app(service);
    quiet(&mut rt);
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    assert_eq!(shorts(&rt), ["Removed", "Removed"]);
    let cmd_z = shortcuts::keys(Command::Undo)[0].event();
    for _ in 0..3 {
        rt.update(|_, tx| tx.blur());
        rt.key(cmd_z.clone());
        settle(&mut rt);
    }
    assert_eq!(listed(&rt), [0, 1], "both back in the list at once");
    assert!(shorts(&rt).is_empty(), "the Removes' notices went when ⌘Z was pressed, and \"back\" comes when each undo is answered");
    wait(&mut rt, 3.5);
    assert_eq!(listed(&rt), [0, 1]);
    let now = shorts(&rt);
    assert_eq!(now.len(), 2, "a notice for each undo, and nothing for the press past the stack: {now:?}");
    let mut back = now;
    back.sort();
    assert_eq!(back, ["Back in the list", "Back in the list"]);
}

/// The user's sequence (#190) in the app, pressed while both Deletes still run: Delete A, Delete B,
/// ⌘Z, ⌘Z. The list changes at once with each press; neither Delete reports a failure (they were
/// undone); each tells it's back once it is, in the order the put-backs end (compared sorted).
#[test]
fn delete_a_delete_b_cmd_z_cmd_z_changes_the_list_at_once() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]).trash_takes(2.0).undo_takes(1.0);
    let mut rt = app(service);
    quiet(&mut rt);
    let delete = |rt: &mut Runtime<Phantom>, id: TorrentId| {
        tap(rt, &format!("rows/{id}"));
        key(rt, shortcuts::keys(Command::Delete)[0].event());
        settle(rt);
    };
    delete(&mut rt, 0);
    assert_eq!(listed(&rt), [1], "A leaves at once");
    delete(&mut rt, 1);
    assert_eq!(listed(&rt), Vec::<TorrentId>::new(), "and B");
    let cmd_z = shortcuts::keys(Command::Undo)[0].event();
    rt.update(|_, tx| tx.blur());
    rt.key(cmd_z.clone());
    settle(&mut rt);
    assert_eq!(listed(&rt), [1], "B is back at once");
    rt.key(cmd_z);
    settle(&mut rt);
    assert_eq!(listed(&rt), [0, 1], "then A, in its place");
    let told = shorts(&rt).len();
    wait(&mut rt, 4.0);
    assert_eq!(listed(&rt), [0, 1]);
    assert!(
        !rt.state.bar.entries().iter().any(|e| e.notice.severity == crate::notice::Severity::Error),
        "no Delete failed: they were undone"
    );
    let mut back = shorts(&rt)[told..].to_vec();
    back.sort();
    assert_eq!(back, ["Back, with its files", "Back, with its files"]);
}

// An undone action's notice leaves; each reason an Undo is off has its own words (#200).

fn delete_row(rt: &mut Runtime<Phantom>, id: TorrentId) {
    tap(rt, &format!("rows/{id}"));
    key(rt, shortcuts::keys(Command::Delete)[0].event());
}

fn cmd_z(rt: &mut Runtime<Phantom>) {
    key(rt, shortcuts::keys(Command::Undo)[0].event());
}

fn shown_short(rt: &Runtime<Phantom>) -> Option<(String, usize, usize)> {
    rt.state.bar.shown().map(|(e, at, of)| (e.notice.short.clone(), at, of))
}

/// Opens the popover on the notice that says `short`.
fn open_notice(rt: &mut Runtime<Phantom>, short: &str) {
    let target = rt.state.bar.entries().iter().find(|e| e.notice.short == short).expect("the notice is there").id;
    super::generated::show_notice(rt, target);
}

/// ⌘Z takes back a Delete: its notice leaves, "is back" stays, and the popover that showed the
/// Delete's moves on to the next notice, the count with it.
#[test]
fn an_undone_deletes_notice_leaves_and_the_open_popover_moves_on() {
    let mut rt = two_rows();
    remove_row(&mut rt, 1);
    delete_row(&mut rt, 0);
    assert_eq!(shorts(&rt), ["Removed", "Deleted"]);
    open_notice(&mut rt, "Deleted");
    assert_eq!(shown_short(&rt), Some(("Deleted".into(), 0, 2)));
    cmd_z(&mut rt);
    assert_eq!(shorts(&rt), ["Removed", "Back, with its files"]);
    assert_eq!(shown_short(&rt), Some(("Removed".into(), 1, 2)), "the popover moved to the next, and counts what's left");
}

/// The notice open was the only one: the popover closes with it.
#[test]
fn the_popover_closes_when_the_last_notice_it_shows_is_undone() {
    let mut rt = two_rows();
    delete_row(&mut rt, 0);
    open_notice(&mut rt, "Deleted");
    cmd_z(&mut rt);
    assert_eq!(shorts(&rt), ["Back, with its files"]);
    assert!(!rt.state.bar.is_open(), "closed: no notice was left to show");
}

/// An undo from the CLI or another client: the stack lives in the service, so the snapshot says it,
/// and the app's notice goes too, with nothing told in its place (the app didn't undo).
#[test]
fn an_undo_by_another_client_removes_the_notice_in_the_app_too() {
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    // Not through the app's own undo: as a connection of another client would ask.
    rt.update(|s, _| s.service.undo(Box::new(|_, _, _| {})));
    quiet(&mut rt);
    assert_eq!(listed(&rt), [1]);
    assert_eq!(shorts(&rt), ["Removed"], "only the undone Remove's notice left");
}

/// Another client's undo still running: its entry is off the stack with no word yet why, so the
/// notice is out of the list at once, as for one pressed here (#200), and never says it's past the
/// last 20. Done, it's gone for good; refused, it comes back saying why.
#[test]
fn another_clients_undo_under_way_takes_the_notice_out_at_once() {
    for refused in [false, true] {
        let service = MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_takes(1.0);
        let service = if refused { service.undo_refusing("Zero") } else { service };
        let mut rt = app(service);
        quiet(&mut rt);
        delete_row(&mut rt, 0);
        rt.update(|s, _| s.service.undo(Box::new(|_, _, _| {})));
        quiet(&mut rt);
        assert!(shorts(&rt).is_empty(), "out while it runs: {:?}", shorts(&rt));
        wait(&mut rt, 1.5);
        match refused {
            false => assert!(shorts(&rt).is_empty(), "undone: gone for good: {:?}", shorts(&rt)),
            true => {
                assert_eq!(shorts(&rt), ["Deleted"], "refused: back");
                let why = press_undo_of(&mut rt, "Deleted").expect("off, with why");
                assert!(why.starts_with("Couldn't be undone: "), "{why}");
            }
        }
    }
}

/// Another client undid a selection, and one of its torrents was added again meanwhile: the app
/// says so too, from the service's word on the entry (#235), though it never asked.
#[test]
fn another_clients_partial_undo_of_a_selection_says_which_torrent_didnt_come_back() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0xaa, "Big Buck Bunny"), torrent(1, "One")]));
    quiet(&mut rt);
    tap(&mut rt, "rows/1");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    key(&mut rt, shortcuts::keys(Command::Remove)[0].event());
    assert_eq!(rt.state.undo.len(), 1, "one press, one entry");
    tap(&mut rt, "magnet-field");
    rt.paste(super::MAGNET);
    settle(&mut rt);
    tap(&mut rt, "add");
    quiet(&mut rt);
    rt.update(|s, _| s.service.undo(Box::new(|_, _, _| {})));
    quiet(&mut rt);
    assert!(listed(&rt).contains(&1), "One is back");
    assert!(shorts(&rt).iter().any(|s| s == "Already in the list"), "{:?}", shorts(&rt));
    assert!(!shorts(&rt).iter().any(|s| s == "Removed 2 torrents"), "undone: its notice gone: {:?}", shorts(&rt));
}

/// A refused undo changes nothing: the notice stays, its Undo off, and says why: "Couldn't be undone: ".
#[test]
fn a_refused_undo_keeps_the_notice_and_says_why_in_its_own_words() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_refusing("Zero"));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    cmd_z(&mut rt);
    assert_eq!(shorts(&rt).first().map(String::as_str), Some("Deleted"), "the Delete's notice stays: {:?}", shorts(&rt));
    open_notice(&mut rt, "Deleted");
    tap(&mut rt, "notice-action");
    settle(&mut rt);
    let why = rt.state.why_off_note.clone().expect("the Undo is off and says why");
    assert!(why.starts_with("Couldn't be undone: "), "{why}");
    assert!(why.contains("isn't in the Trash any more"), "{why}");
}

/// Past the last 20 the oldest leave the stack: their notices stay, saying so, which isn't an undo.
#[test]
fn a_notice_past_the_last_twenty_stays_with_its_own_words() {
    let service = MemoryService::new().with_torrents((0..21).map(|id| torrent(id, &name(id))).collect());
    let mut rt = app(service);
    quiet(&mut rt);
    for id in 0..21 {
        remove_row(&mut rt, id);
    }
    assert_eq!(rt.state.bar.entries().len(), 21, "none was undone: every notice stays");
    open_notice(&mut rt, "Removed");
    tap(&mut rt, "notice-action");
    settle(&mut rt);
    assert_eq!(rt.state.why_off_note.as_deref(), Some("Only the last 20 Removes, Deletes, Moves to Trash and Renames can be undone"));
}

/// Selects every row, then Removes (or Deletes) them: one notice for all.
fn remove_all(rt: &mut Runtime<Phantom>, command: Command) {
    tap(rt, "rows/0");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(rt);
    key(rt, shortcuts::keys(command)[0].event());
}

/// Presses the Undo of the notice that says `short`, and says what the button says of it.
fn press_undo_of(rt: &mut Runtime<Phantom>, short: &str) -> Option<String> {
    open_notice(rt, short);
    rt.update(|s, _| s.why_off_note = None);
    tap(rt, "notice-action");
    settle(rt);
    rt.state.why_off_note.clone()
}

/// Which reason an Undo that's off gives, from where its entry stands: the service's word once it's
/// off the stack, and the stack.
#[test]
fn the_reason_an_undo_is_off_follows_where_its_entry_stands() {
    use crate::model::{Happened, undone::Undos};
    use proto::{CantUndo, Leave, Left};
    let entry = |seq: u64| Undoable { seq, action: UndoAction::Remove, torrents: vec![Named { id: seq, name: format!("t{seq}") }] };
    let said = |left: Vec<(u64, Leave)>| {
        let mut undos = Undos::default();
        undos.observe(&left.into_iter().map(|(seq, why)| Left { seq, why }).collect::<Vec<_>>());
        undos
    };
    let one = Happened { seq: 1 };
    // Pushed past the last 20: the stack moved on.
    assert_eq!(one.off(&[], &said(vec![(1, Leave::Aged)])), Some(UndoOff::PastLimit));
    // Taken by a newer Delete: that one comes first.
    assert_eq!(one.off(&[], &said(vec![(1, Leave::Absorbed { by: 5 })])), Some(UndoOff::Newer));
    // Refused, with why.
    let refused = Leave::Refused { why: CantUndo::NotKept, stranded: Vec::new() };
    assert_eq!(one.off(&[], &said(vec![(1, refused)])), Some(UndoOff::Refused(CantUndo::NotKept, Vec::new())));
    // Its torrent moved (#24): dropped as the Move started, or refused once its undo, pressed while
    // the Move waited, came after it. Either way, moved since.
    assert_eq!(one.off(&[], &said(vec![(1, Leave::Moved)])), Some(UndoOff::Moved));
    let refused = Leave::Refused { why: CantUndo::Moved, stranded: Vec::new() };
    assert_eq!(one.off(&[], &said(vec![(1, refused)])), Some(UndoOff::Moved));
    // A newer entry above it.
    assert_eq!(one.off(&[entry(1), entry(2)], &Undos::default()), Some(UndoOff::Newer));
    assert_eq!(Happened { seq: 2 }.off(&[entry(1), entry(2)], &Undos::default()), None);
}

/// The last Remove goes past the stack and is undone before the next snapshot: the oldest still aged
/// (its notice stays, saying so), only the one undone leaves.
#[test]
fn a_push_and_an_undo_between_two_snapshots_still_age_the_oldest() {
    let service = MemoryService::new().with_torrents((0..21).map(|id| torrent(id, &name(id))).collect());
    let mut rt = app(service);
    quiet(&mut rt);
    for id in 0..20 {
        remove_row(&mut rt, id);
    }
    rt.update(|s, _| {
        s.service.command(&[20], proto::TorrentCommand::Remove, Box::new(|_, _, _| {}));
        s.service.undo(Box::new(|_, _, _| {}));
    });
    quiet(&mut rt);
    let notices = shorts(&rt);
    assert!(notices.contains(&"Removed".to_owned()), "the oldest aged, and stays: {notices:?}");
    assert_eq!(press_undo_of(&mut rt, "Removed"), Some(display::undo_off(&UndoOff::PastLimit)));
}

/// Refused for another client (the CLI's `phantom undo`): the notice stays, its Undo off with why.
#[test]
fn an_undo_refused_for_another_client_keeps_the_notice_and_says_why() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_refusing("Zero"));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    rt.update(|s, _| s.service.undo(Box::new(|_, _, _| {})));
    quiet(&mut rt);
    assert_eq!(shorts(&rt), ["Deleted"], "no \"is back\", no notice gone");
    let why = press_undo_of(&mut rt, "Deleted").expect("off, with why");
    assert!(why.starts_with("Couldn't be undone: ") && why.contains("isn't in the Trash any more"), "{why}");
}

/// The first Delete of a torrent failed partway; Delete again fails the same way and takes the
/// first's place on the stack: its notice doesn't go as if undone, but follows the new Delete, and
/// leaves when that is undone.
#[test]
fn a_partly_failed_delete_deleted_again_is_absorbed_not_undone() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).trash_partly("Zero"));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    assert_eq!(shorts(&rt), ["Couldn't delete the torrent"]);
    delete_row(&mut rt, 0);
    assert_eq!(shorts(&rt), ["Couldn't delete the torrent", "Couldn't delete the torrent"], "absorbed, not undone: both stay");
    cmd_z(&mut rt);
    assert!(shorts(&rt).iter().all(|s| !s.starts_with("Couldn't delete")), "the new Delete undone: both go: {:?}", shorts(&rt));
}

/// A partly failed Delete stays on the stack, as the engine's: ⌘Z takes back what moved, and the
/// error notice of that Delete leaves with it.
#[test]
fn a_partly_failed_deletes_error_leaves_when_it_is_undone() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).trash_partly("Zero"));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    assert_eq!(shorts(&rt), ["Couldn't delete the torrent"]);
    cmd_z(&mut rt);
    assert_eq!(listed(&rt), [0], "still listed");
    assert_eq!(shorts(&rt), ["Back, with its files"], "the error went with its Delete; what came back is told");
}

/// Move to… goes ahead while files of the torrent are in the Trash for an undo (#24): as it starts,
/// that Move to Trash leaves the stack, its files staying in the Trash, and its notice's Undo turns
/// off, saying why.
#[test]
fn a_move_drops_the_undo_of_a_move_to_trash_of_its_torrent_and_its_notice_says_why() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]));
    quiet(&mut rt);
    tap(&mut rt, "rows/0");
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(shortcuts::keys(Command::Delete)[0].event());
    quiet(&mut rt);
    let trashed = shorts(&rt).into_iter().find(|s| s.starts_with("Moved")).expect("the Move to Trash's notice");
    assert_eq!(rt.state.undo.len(), 1, "on the stack");
    super::right_click_row(&mut rt, 0);
    super::pick_choice(&mut rt, "row-menu", "Move to");
    super::choose(&mut rt, Some("/Films"));
    quiet(&mut rt);
    assert_eq!(rt.state.torrents[0].location.as_deref(), Some(std::path::Path::new("/Films/Zero")), "moved");
    assert!(rt.state.undo.is_empty(), "the Move to Trash left the stack");
    assert_eq!(press_undo_of(&mut rt, &trashed), Some("Moved to another folder since".to_owned()));
}

/// A Move to Trash that failed partway: its "Moved…" notice and its error both go with its undo.
#[test]
fn a_partly_failed_move_to_trash_leaves_both_its_notices_when_undone() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).trash_partly("Zero"));
    quiet(&mut rt);
    tap(&mut rt, "rows/0");
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(shortcuts::keys(Command::Delete)[0].event());
    quiet(&mut rt);
    let told = shorts(&rt);
    assert!(told.iter().any(|s| s.starts_with("Moved")) && told.iter().any(|s| s.contains("Couldn't")), "{told:?}");
    cmd_z(&mut rt);
    assert!(shorts(&rt).iter().all(|s| !s.starts_with("Moved") && !s.contains("Couldn't")), "both gone: {:?}", shorts(&rt));
}

/// A Delete of A and B where A was refused outright (nothing to undo) and B failed partway: the one
/// failure notice stays when B's Delete is undone, since A's wasn't.
#[test]
fn undoing_one_torrents_partial_delete_leaves_the_others_failure_notice_alone() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).trash_refusing("A").trash_partly("B");
    let mut rt = app(service);
    quiet(&mut rt);
    remove_all(&mut rt, Command::Delete);
    let before = shorts(&rt);
    assert!(before.iter().any(|s| s.contains("Couldn't delete")), "{before:?}");
    cmd_z(&mut rt);
    assert!(shorts(&rt).iter().any(|s| s.contains("Couldn't delete")), "A's failure stays: {:?}", shorts(&rt));
}

/// Two torrents each failing partway in one Delete share one entry and one notice: one ⌘Z undoes
/// both, and the notice goes (#235).
#[test]
fn a_failure_notice_of_two_partial_deletes_goes_with_their_one_undo() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).trash_partly("A").trash_partly("B");
    let mut rt = app(service);
    quiet(&mut rt);
    remove_all(&mut rt, Command::Delete);
    assert!(shorts(&rt).iter().any(|s| s.contains("Couldn't delete")), "{:?}", shorts(&rt));
    assert_eq!(rt.state.undo.len(), 1, "one entry, holding what did happen");
    cmd_z(&mut rt);
    assert!(shorts(&rt).iter().all(|s| !s.contains("Couldn't delete")), "both undone: {:?}", shorts(&rt));
}

/// An undo that failed without being refused (the service couldn't finish it) reaches clients as a
/// refusal with the failure as its reason: the notice says "Couldn't be undone: it failed (…)", not
/// that the stack moved past the last 20.
#[test]
fn an_undo_that_failed_without_a_refusal_says_it_failed_not_past_the_last_twenty() {
    use crate::model::{Happened, undone::Undos};
    use proto::{CantUndo, Leave, Left};
    let failed = Leave::Refused { why: CantUndo::Failed { detail: "the list couldn't be changed".into() }, stranded: Vec::new() };
    let mut undos = Undos::default();
    undos.observe(&[Left { seq: 1, why: failed }]);
    let off = Happened { seq: 1 }.off(&[], &undos).expect("off");
    assert_ne!(off, UndoOff::PastLimit);
    assert_eq!(display::undo_off(&off), "Couldn't be undone: it failed (the list couldn't be changed)");
}

// Optimistic Undo (#200): a notice leaves the moment Undo is pressed, and comes back if it's refused.

/// ⌘Z with the undo still running: the Delete's notice is gone at once, together with its partial-failure
/// notice, and "is back" comes once the files are back.
#[test]
fn a_notice_leaves_the_moment_cmd_z_is_pressed_and_is_back_comes_after() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_takes(1.0));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    assert_eq!(shorts(&rt), ["Deleted"]);
    cmd_z(&mut rt);
    assert!(shorts(&rt).is_empty(), "gone at once, the undo not done: {:?}", shorts(&rt));
    wait(&mut rt, 1.5);
    assert_eq!(shorts(&rt), ["Back, with its files"]);
}

/// The notice's own Undo does the same, and the popover moves on to the next notice, or closes.
#[test]
fn a_notice_leaves_the_moment_its_own_undo_is_pressed_and_the_popover_moves_on() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]).undo_takes(1.0));
    quiet(&mut rt);
    remove_row(&mut rt, 1);
    delete_row(&mut rt, 0);
    open_notice(&mut rt, "Deleted");
    tap(&mut rt, "notice-action");
    settle(&mut rt);
    assert_eq!(shorts(&rt), ["Removed"], "gone at once");
    assert_eq!(shown_short(&rt), Some(("Removed".into(), 0, 1)), "the popover moved on");
    open_notice(&mut rt, "Removed");
    tap(&mut rt, "notice-action");
    settle(&mut rt);
    assert!(shorts(&rt).is_empty() && !rt.state.bar.is_open(), "the last one gone: the popover closes");
    wait(&mut rt, 2.0);
    let mut back = shorts(&rt);
    back.sort();
    assert_eq!(back, ["Back in the list", "Back, with its files"]);
}

/// ⌘Z refused: the notice was gone, and is back in its place, its Undo off saying why.
#[test]
fn a_notice_comes_back_with_the_reason_when_cmd_z_is_refused() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_refusing("Zero").undo_takes(1.0));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    cmd_z(&mut rt);
    assert!(shorts(&rt).is_empty(), "gone at once");
    wait(&mut rt, 1.5);
    assert_eq!(shorts(&rt).first().map(String::as_str), Some("Deleted"), "back: {:?}", shorts(&rt));
    let why = press_undo_of(&mut rt, "Deleted").expect("off, with why");
    assert!(why.starts_with("Couldn't be undone: "), "{why}");
}

/// The same from the notice's own Undo, for a Delete of several where the first undo is refused: the
/// notice comes back whole, and the ones never asked aren't left out of the list for good.
#[test]
fn a_notice_of_several_comes_back_when_its_own_undo_is_refused() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "A"), torrent(1, "B")]).undo_refusing("A").undo_refusing("B");
    let mut rt = app(service);
    quiet(&mut rt);
    remove_all(&mut rt, Command::Delete);
    assert!(shorts(&rt).contains(&"Deleted 2 torrents".to_owned()), "{:?}", shorts(&rt));
    open_notice(&mut rt, "Deleted 2 torrents");
    tap(&mut rt, "notice-action");
    quiet(&mut rt);
    assert!(shorts(&rt).iter().any(|s| s == "Deleted 2 torrents"), "back: {:?}", shorts(&rt));
}

/// An Undo pressed here that another client's refusal (through the service's word) ends: the notice,
/// out of the list, comes back with the reason, whoever asked.
#[test]
fn a_notice_comes_back_when_the_service_says_its_undo_was_refused() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero")]).undo_refusing("Zero"));
    quiet(&mut rt);
    delete_row(&mut rt, 0);
    let seq = rt.state.bar.entries()[0].notice.of[0];
    rt.update(move |s, _| s.bar.undoing(vec![seq]));
    assert!(shorts(&rt).is_empty(), "out of the list while under way");
    // Another client's undo, refused: this app's own answer isn't involved.
    rt.update(|s, _| s.service.undo(Box::new(|_, _, _| {})));
    quiet(&mut rt);
    assert_eq!(shorts(&rt), ["Deleted"], "back");
    let why = press_undo_of(&mut rt, "Deleted").expect("off");
    assert!(why.starts_with("Couldn't be undone: "), "{why}");
}

/// Two presses before the snapshot shows the first: the second takes the next entry, not the same.
#[test]
fn two_presses_in_one_frame_take_two_entries_and_both_notices_leave() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]).undo_takes(1.0));
    quiet(&mut rt);
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    rt.update(|s, _| {
        crate::changes::press(s);
        crate::changes::press(s);
    });
    settle(&mut rt);
    assert!(shorts(&rt).is_empty(), "both gone: {:?}", shorts(&rt));
}

/// An undo that fails for no reason of its own (the connection went, say): the notice that left comes
/// back.
#[test]
fn a_notice_comes_back_when_its_undo_failed_without_a_refusal() {
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    let seq = rt.state.bar.entries()[0].notice.of[0];
    rt.update(move |s, _| {
        s.bar.undoing(vec![seq]);
        crate::model::refused_asked(s, vec![seq], &client::Failure::Error(anyhow::anyhow!("connection lost")));
    });
    settle(&mut rt);
    // (Its entry is still on the stack here, so its Undo is on; a refused entry the service drops, and
    // its Undo says why, as `an_undo_that_failed_without_a_refusal…` pins.)
    assert_eq!(shorts(&rt), ["Removed"], "back");
}

/// What an undo's answer says the service took off its stack (#258's review, X3): an undone entry,
/// a refused one too (a refused undo is gone), and none when it names none. So a ⌘Z right after a
/// refusal takes the next change, here a setting newer than the action under the refused one, never
/// the refused entry again (which would have the service undo the action under it). An example: the
/// fake sends the snapshot that drops the refused entry before the answer, as the service does over
/// one connection, so no generated sequence reaches a press between the two.
#[test]
fn an_undos_answer_names_the_entry_it_took_refused_or_not() {
    use crate::changes::machine::{Change, Command, Entry, Event, Observed, Press, State, decide};
    use crate::model::taken_entry;
    let undo = |seq| Undoable { seq, action: UndoAction::Delete, torrents: vec![Named { id: 7, name: "Seven".into() }] };
    let path = proto::AbsolutePath::new("/Users/me/Downloads/Seven").unwrap();
    let refused =
        client::Failure::Refused(proto::Refusal::Undo { undo: undo(3), why: proto::CantUndo::NotInTrash { path }, stranded: Vec::new() });
    let seven = |seq| Some(Entry { seq, torrents: vec![7] });
    assert_eq!(taken_entry(&Ok(proto::Undone { undo: undo(2), refused: Vec::new() })), seven(2), "undone");
    assert_eq!(taken_entry(&Err(refused)), seven(3), "refused: gone too");
    assert_eq!(taken_entry(&Err(client::Failure::Refused(proto::Refusal::NothingToUndo))), None);
    assert_eq!(taken_entry(&Err(client::Failure::Error(anyhow::anyhow!("connection lost")))), None);
    // B, the port, A; ⌘Z took A, whose undo was refused: the next ⌘Z takes the port.
    let port = Change::Listed { seq: 9, name: proto::SettingName::ListenPort, press: Press::No };
    let state = State {
        list: vec![Change::Action { seq: 1, press: Press::No }, port.clone(), Change::Action { seq: 3, press: Press::Asked }],
        ..State::default()
    };
    let path = proto::AbsolutePath::new("/Users/me/Downloads/Seven").unwrap();
    let refused =
        client::Failure::Refused(proto::Refusal::Undo { undo: undo(3), why: proto::CantUndo::NotInTrash { path }, stranded: Vec::new() });
    let state = decide(&state, &Event::Observed(Observed::ActionAnswered { asked: 3, took: taken_entry(&Err(refused)) })).unwrap();
    let state = decide(&state, &Event::Command(Command::Press)).unwrap();
    assert_eq!(state.list[1], port.with(Press::Taken), "the port, the newest left");
}

/// Another client's settings change and torrent action that one snapshot lists: the snapshot doesn't
/// say which came first, so the setting counts as the older (ADR 0008): ⌘Z takes the action first,
/// and Edit ▸ Undo names it (#258's review, X2). An example: the generated sequences never put two
/// of another client's changes in one snapshot.
#[test]
fn of_another_clients_setting_and_action_in_one_snapshot_the_setting_is_older() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]);
    let disk = service.disk();
    let mut d = desk(service);
    desk_quiet(&mut d);
    // Both before the next poll: one snapshot lists both.
    disk.change(crate::service::DiskChange::RemovedByAnother(1));
    disk.change(crate::service::DiskChange::SetByAnother(proto::Setting::KeepAwake(Some(true))));
    desk_quiet(&mut d);
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Remove of One");
    key_in(&mut d, shortcuts::keys(Command::Undo)[0].event());
    assert!(d.main.state.torrents.iter().any(|t| t.id == 1), "One is back");
    assert!(d.settings.state.sheet.is_none() && !d.shown().settings, "the setting is still to undo");
    assert_eq!(crate::changes::menu_title(&d.main.state), display::undo_change(display::setting_short("keep_awake")));
}

/// How long a notice stays, by kind (5 s for a receipt, 1 minute for an action's with its Undo,
/// never for a warning or an error): told straight to the bar, one frame on, so the times are exact.
fn told_one_frame_ago(notice: crate::notice::Notice) -> Runtime<Phantom> {
    let mut rt = two_rows();
    rt.update(move |s, _| s.bar.tell(notice.clone()));
    rt.frame(super::FRAME);
    rt
}

#[test]
fn a_receipt_goes_at_5_s_and_an_actions_notice_at_60_s() {
    let mut rt = told_one_frame_ago(crate::notice::Notice::new("Added X").brief());
    wait(&mut rt, 4.9);
    assert_eq!(rt.state.bar.len(), 1, "a receipt stays at 4.9 s");
    wait(&mut rt, 0.12);
    assert_eq!(rt.state.bar.len(), 0, "and goes at 5 s");

    let mut rt = told_one_frame_ago(crate::notice::Notice::new("Removed X").long());
    wait(&mut rt, 59.9);
    assert_eq!(rt.state.bar.len(), 1, "an action's notice stays at 59.9 s");
    wait(&mut rt, 0.12);
    assert_eq!(rt.state.bar.len(), 0, "and goes at 60 s");
}

#[test]
fn a_warning_and_an_error_stay_until_dismissed() {
    let mut rt = told_one_frame_ago(crate::notice::Notice::warning("Careful"));
    rt.update(|s, _| s.bar.tell(crate::notice::Notice::error("Broke")));
    wait(&mut rt, 120.0);
    assert_eq!(shorts(&rt), ["Careful", "Broke"]);
}

/// What the app tells, as it tells it: a Remove's notice is long, what an undo of it tells is brief.
#[test]
fn the_apps_notices_last_as_they_should() {
    use crate::notice::Lasts;
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    let lasts = |rt: &Runtime<Phantom>| rt.state.bar.entries().iter().map(|e| e.notice.lasts).collect::<Vec<_>>();
    assert_eq!(lasts(&rt), [Lasts::Long], "a Remove's, with its Undo");
    wait(&mut rt, 30.0);
    assert_eq!(rt.state.bar.len(), 1, "still there at 30 s");
    cmd_z(&mut rt);
    assert_eq!(lasts(&rt), [Lasts::Brief], "an undo's receipt");
    wait(&mut rt, 5.5);
    assert_eq!(rt.state.bar.len(), 0);
}

#[test]
fn an_adds_receipt_is_brief_and_leaves() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste(super::MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    quiet(&mut rt);
    assert_eq!(rt.state.bar.entries().iter().map(|e| e.notice.lasts).collect::<Vec<_>>(), [crate::notice::Lasts::Brief]);
    wait(&mut rt, 6.0);
    assert_eq!(rt.state.bar.len(), 0);
}

/// The generated test's model follows the bar's timed notices (#266): a receipt leaves 5 s after the
/// list closed, while the act's notice (a minute) stays. Each step here takes about half a second
/// of the app's clock.
#[test]
fn the_model_lets_a_receipt_leave_with_the_time_the_steps_take() {
    use Step::*;
    let waits = || (0..16).map(|_| Trash { n: 0 });
    let mut keep_awake = vec![Awake, Z];
    keep_awake.extend(waits());
    run(&keep_awake);
    let mut is_back = vec![Go { n: 0, all: false, delete: false }, Z];
    is_back.extend(waits());
    run(&is_back);
}

/// The model times a receipt from when the list really closed (#319): a notice's Undo that takes the
/// only notice closes the list then, and "Back in the list", told after, starts its clock at once,
/// not at the Escape that follows (which closes nothing). Here it leaves 5 s later, during the last
/// Remove, while a model timing it from the Escape held it a few frames longer.
#[test]
fn the_model_times_a_receipt_from_when_its_undo_closed_the_list() {
    use Step::*;
    let go = Go { n: 0, all: false, delete: false };
    run(&[go.clone(), Press { n: 0 }, go.clone(), go.clone(), go.clone(), go.clone(), Z, Trash { n: 0 }, go.clone(), Z, go.clone(), Z, go]);
}
