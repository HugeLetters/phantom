//! Undo in the status bar's notices (#150), generated: Removes and Deletes of one row or of all of
//! them, ⌘Z, and a person opening the notice of a past Remove or Delete and pressing its Undo. A
//! reference model knows the rows, the undo stack (each Remove or Delete with its rows, newest last)
//! and the notices told. After every step:
//! - the list is the model's rows;
//! - the bar's notices are the model's, in order: a Remove or Delete tells "Removed <name>",
//!   "Deleted <name>" or "Removed 3 torrents"; an undo, what came back;
//! - a notice's Undo is on exactly while its Remove or Delete is the newest on the stack. Pressed
//!   then, its rows come back and the notice gives way to what came back. Pressed while off,
//!   nothing changes and the notice says why.
//!
//! 64 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::collections::BTreeSet;

use client::display;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proto::{TorrentId, UndoAction, Undoable};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{app, settle, tap, torrent, wait};
use crate::bar::machine::{MAX_TOLD, Way};
use crate::model::{Phantom, selected_torrents};
use crate::service::MemoryService;
use crate::shortcuts::{self, Command};

const ROWS: u64 = 4;

#[derive(Debug, Clone)]
enum Step {
    /// ⌫ or ⌘⌫ on the `n`-th row there is, or on every one (⌘A).
    Go { n: usize, all: bool, delete: bool },
    /// ⌘Z.
    Z,
    /// The Undo in the `n`-th notice of a Remove or Delete still told.
    Press { n: usize },
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

#[derive(Default)]
struct Model {
    rows: BTreeSet<TorrentId>,
    acts: Vec<Act>,
    /// The undo stack, newest last: each entry's act and row.
    stack: Vec<(usize, TorrentId)>,
    told: Vec<Told>,
}

fn name(id: TorrentId) -> String {
    format!("Torrent {id}")
}

impl Model {
    fn tell(&mut self, told: Told) {
        self.told.push(told);
        if self.told.len() > MAX_TOLD {
            self.told.remove(0);
        }
    }

    fn short(&self, told: &Told) -> String {
        match told {
            Told::Act(a) => display::happened(self.acts[*a].action, &self.acts[*a].ids.iter().map(|&id| name(id)).collect::<Vec<_>>()),
            Told::Said(said) => said.clone(),
        }
    }

    /// Whether act `a` is the newest on the stack, all of it.
    fn on_top(&self, a: usize) -> bool {
        let k = self.acts[a].ids.len();
        let mine = self.stack.iter().filter(|(act, _)| *act == a).count();
        mine == k && self.stack.len() >= k && self.stack[self.stack.len() - k..].iter().all(|(act, _)| *act == a)
    }

    fn undone(&self, a: usize, id: TorrentId) -> Undoable {
        Undoable { seq: 0, action: self.acts[a].action, id, name: name(id) }
    }
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let step = prop_oneof![
        4 => (0..ROWS as usize, prop::bool::weighted(0.2), any::<bool>()).prop_map(|(n, all, delete)| Step::Go { n, all, delete }),
        2 => Just(Step::Z),
        4 => (0..MAX_TOLD).prop_map(|n| Step::Press { n }),
    ];
    prop::collection::vec(step, 1..14)
}

/// Frames until the fake has answered everything asked: an undo of several asks one at a time.
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
    let mut rt = app(service);
    quiet(&mut rt);
    let mut model = Model { rows: (0..ROWS).collect(), ..Model::default() };
    for step in steps {
        match *step {
            Step::Go { n, all, delete } => {
                let Some(&id) = model.rows.iter().nth(n % model.rows.len().max(1)) else { continue };
                let ids: Vec<TorrentId> = if all { model.rows.iter().copied().collect() } else { vec![id] };
                tap(&mut rt, &format!("rows/{id}"));
                if all {
                    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                    settle(&mut rt);
                }
                // In the order the app sends them, which is the stack's.
                let sent = selected_torrents(&rt.state.table);
                assert_eq!(sent.iter().copied().collect::<BTreeSet<_>>(), ids.iter().copied().collect());
                let command = if delete { Command::Delete } else { Command::Remove };
                key(&mut rt, shortcuts::keys(command)[0].event());
                let action = if delete { UndoAction::Delete } else { UndoAction::Remove };
                let ids = sent;
                for id in &ids {
                    model.rows.remove(id);
                }
                model.acts.push(Act { action, ids: ids.clone() });
                let a = model.acts.len() - 1;
                model.stack.extend(ids.iter().map(|&id| (a, id)));
                model.tell(Told::Act(a));
            }
            Step::Z => {
                key(&mut rt, shortcuts::keys(Command::Undo)[0].event());
                if let Some((a, id)) = model.stack.pop() {
                    model.rows.insert(id);
                    let said = display::undone(&model.undone(a, id));
                    model.tell(Told::Said(said));
                }
            }
            Step::Press { n } => {
                let acts: Vec<(usize, usize)> =
                    model.told.iter().enumerate().filter_map(|(at, t)| if let Told::Act(a) = t { Some((at, *a)) } else { None }).collect();
                let Some(&(at, a)) = acts.get(n % acts.len().max(1)) else { continue };
                let target = rt.state.bar.entries()[at].id;
                rt.update(move |s, _| {
                    if !s.bar.is_open() {
                        s.bar.press();
                    }
                    while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Older) {}
                    while s.bar.shown().is_some_and(|(e, ..)| e.id != target) && s.bar.step(Way::Newer) {}
                });
                settle(&mut rt);
                assert_eq!(rt.state.bar.shown().map(|(e, ..)| e.id), Some(target), "the popover shows the act's notice");
                let on = model.on_top(a);
                tap(&mut rt, "notice-action");
                quiet(&mut rt);
                if on {
                    let k = model.acts[a].ids.len();
                    let back: Vec<(usize, TorrentId)> = model.stack.split_off(model.stack.len() - k);
                    for (_, id) in &back {
                        model.rows.insert(*id);
                    }
                    model.told.remove(at);
                    let said = match &back[..] {
                        [(a, id)] => display::undone(&model.undone(*a, *id)),
                        many => display::several_undone(many.len()),
                    };
                    model.tell(Told::Said(said));
                } else {
                    let why = rt.state.why_off_note.clone();
                    assert!(why.is_some() && rt.find("notice-action-why").is_some(), "an Undo that's off says why when pressed");
                }
                rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                settle(&mut rt);
            }
        }
        let listed: BTreeSet<TorrentId> = rt.state.torrents.iter().map(|t| t.id).collect();
        assert_eq!(listed, model.rows, "the list after {step:?}");
        let shorts: Vec<String> = rt.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect();
        let want: Vec<String> = model.told.iter().map(|t| model.short(t)).collect();
        assert_eq!(shorts, want, "the notices after {step:?}");
    }
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

/// 6. ⌘Z with nothing to undo: nothing happens, nothing is told.
#[test]
fn cmd_z_with_nothing_to_undo_does_nothing_and_says_nothing() {
    let mut rt = two_rows();
    key(&mut rt, shortcuts::keys(Command::Undo)[0].event());
    assert_eq!(listed(&rt), [0, 1]);
    assert!(shorts(&rt).is_empty(), "no notice: {:?}", shorts(&rt));
}

/// 7. The Undo in an old notice, after a newer Remove: off, and pressed it says why, in
/// `display`'s words; nothing comes back. The newer notice's Undo is on, and after it the older's.
#[test]
fn an_old_notices_undo_is_off_after_a_newer_remove_and_says_why() {
    let mut rt = two_rows();
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    assert_eq!(shorts(&rt), ["Removed Zero", "Removed One"]);
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
    assert_eq!(rt.state.why_off_note.as_deref(), Some(display::undo_off(true)));
    assert!(rt.find("notice-action-why").is_some(), "the reason shows under the button");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);

    open(&mut rt, 1);
    tap(&mut rt, "notice-action");
    quiet(&mut rt);
    assert_eq!(listed(&rt), [1]);
}

/// 1 (decided). An undo of a torrent added again by hand does nothing to the list and says so, short:
/// "Already in the list"; its entry is dropped, so the next ⌘Z is the one under it.
#[test]
fn an_undo_of_a_torrent_added_again_says_already_in_the_list() {
    let refused = |why| {
        client::Failure::Refused(proto::Refusal::Undo {
            undo: Undoable { seq: 1, action: UndoAction::Remove, id: 3, name: "Album".into() },
            why,
            stranded: Vec::new(),
        })
    };
    let notice = crate::model::undo_notice(&Err(refused(proto::CantUndo::InList))).expect("a notice");
    assert_eq!((notice.short.as_str(), notice.severity), (display::already_in_list(), crate::notice::Severity::Info));
    assert_eq!(display::already_in_list(), "Already in the list");
    assert!(notice.more.is_none(), "nothing to explain");
    // Others still say why they couldn't.
    let other = crate::model::undo_notice(&Err(refused(proto::CantUndo::NotKept))).expect("a notice");
    assert_eq!(other.short, "Couldn't undo Remove of Album");
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

/// 5 (decided). Two ⌘Z while the first undo runs: the second waits its turn, and both torrents come
/// back, the newest first, each with a notice of its own; a third press, with nothing more on the
/// stack, is nothing. A slow undo shows "Undoing…" in the status bar after a second, and it goes with
/// the last.
#[test]
fn cmd_z_pressed_twice_while_an_undo_runs_undoes_both_in_turn() {
    let service = MemoryService::new().with_torrents(vec![torrent(0, "Zero"), torrent(1, "One")]).undo_takes(3.0);
    let mut rt = app(service);
    quiet(&mut rt);
    remove_row(&mut rt, 0);
    remove_row(&mut rt, 1);
    let told = shorts(&rt).len();
    let cmd_z = shortcuts::keys(Command::Undo)[0].event();
    for _ in 0..3 {
        rt.update(|_, tx| tx.blur());
        rt.key(cmd_z.clone());
        settle(&mut rt);
    }
    assert!(listed(&rt).is_empty(), "nothing is back yet: the first undo runs");
    assert!(!shorts(&rt).iter().any(|s| s == display::undoing()), "not said before a second");
    wait(&mut rt, 1.5);
    assert_eq!(shorts(&rt).iter().filter(|s| *s == display::undoing()).count(), 1, "Undoing… is in the bar: {:?}", shorts(&rt));
    wait(&mut rt, 2.0);
    assert_eq!(listed(&rt), [1], "the newest is back first");
    assert!(shorts(&rt).iter().any(|s| s == display::undoing()), "still undoing the next");
    wait(&mut rt, 3.5);
    assert_eq!(listed(&rt), [0, 1], "then the older");
    let now = shorts(&rt);
    assert!(!now.iter().any(|s| s == display::undoing()), "gone with the last: {now:?}");
    assert_eq!(now.len(), told + 2, "a notice for each undo, and nothing for the press past the stack: {now:?}");
    assert_eq!(&now[told..], ["One is back in the list", "Zero is back in the list"]);
}
