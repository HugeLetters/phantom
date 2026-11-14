//! A torrent row's context menu: the actions the service lists for the selection (Open and Reveal
//! in Finder for a single torrent, the toolbar's others), plus Copy magnet link. What it offers is
//! [`row_items`]'s call; this words the items and runs the one picked.

use proto::{Action, TorrentCommand, TorrentId};
use rune::{El, PointerPress, Tx};

use crate::menu::{Event, Menu, Open};
use crate::model::{Phantom, RowItem, command, copy_magnet, remove_selected, row_items, select_for_menu, selected_for, selected_torrents};
use crate::platform::{Show, show_torrent};
use crate::shortcuts::{self, Command};
use crate::ui::{Entry, Icon, Ui};

/// Opens the menu at `press` for a right-click on torrent `id`'s row.
pub fn open(s: &mut Phantom, id: TorrentId, press: PointerPress, _: &mut Tx) {
    select_for_menu(&mut *s.service, &mut s.table, &s.torrents, &mut s.pane, id);
    s.menu = s.menu.then(Event::Open(Open::Row { id, at: (press.x, press.y) }));
}

/// The open row menu, in `layer`.
pub fn overlay(s: &Phantom, ui: &Ui, layer: &mut Vec<El>) {
    let Menu::Open(Open::Row { id, at }) = s.menu else { return };
    let items = row_items(&s.table, &s.torrents);
    ui.context_menu(
        "row-menu",
        Some(at),
        entries(&items, selected_torrents(&s.table).len()),
        move |s: &mut Phantom, picked, tx| {
            s.menu = s.menu.then(Event::Close);
            if let Some(item) = row_items(&s.table, &s.torrents).into_iter().find(|i| item_id(*i) == picked) {
                run(s, id, item, tx);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}

/// The menu's entries: the items, a separator between groups.
pub fn entries(items: &[RowItem], n: usize) -> Vec<Entry> {
    let can_delete = items.contains(&RowItem::Do(Action::Delete));
    let mut entries = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && group(*item) != group(items[i - 1]) {
            entries.push(Entry::Separator);
        }
        entries.push(Entry::Choice {
            id: item_id(*item),
            label: label(*item, n, can_delete),
            hint: shortcut(*item).map(shortcuts::hint),
            icon: (*item == RowItem::CopyMagnet).then_some(Icon::Copy),
        });
    }
    entries
}

/// The shortcut that does what the item does, if one does.
pub fn shortcut(item: RowItem) -> Option<Command> {
    match item {
        RowItem::Do(Action::Open) => Some(Command::Open),
        RowItem::Do(Action::Reveal) => Some(Command::Reveal),
        // ⌫ removes at once, ⌘⌫ deletes at once.
        RowItem::Do(Action::Remove) => Some(Command::Remove),
        RowItem::Do(Action::Delete) => Some(Command::Delete),
        RowItem::Do(Action::Pause | Action::Resume | Action::Check | Action::SetPriority) | RowItem::CopyMagnet => None,
    }
}

/// Items of a group sit together, apart from the others' by a separator.
fn group(item: RowItem) -> u8 {
    match item {
        RowItem::Do(Action::Pause | Action::Resume | Action::Check | Action::SetPriority) => 0,
        RowItem::Do(Action::Open | Action::Reveal) => 1,
        RowItem::CopyMagnet => 2,
        RowItem::Do(Action::Remove | Action::Delete) => 3,
    }
}

/// The menu item's id: what a test or script picks.
pub fn item_id(item: RowItem) -> &'static str {
    match item {
        RowItem::Do(Action::Pause) => "pause",
        RowItem::Do(Action::Resume) => "resume",
        RowItem::Do(Action::Check) => "check",
        RowItem::Do(Action::Remove) => "remove",
        RowItem::Do(Action::Delete) => "delete",
        RowItem::Do(Action::SetPriority) => "priority",
        RowItem::Do(Action::Open) => "open",
        RowItem::Do(Action::Reveal) => "reveal",
        RowItem::CopyMagnet => "copy-magnet",
    }
}

/// Worded as the toolbar's Remove… is: with the count for several torrents, and Remove saying it
/// keeps the files when Delete is beside it.
fn label(item: RowItem, n: usize, can_delete: bool) -> String {
    match item {
        RowItem::Do(Action::Pause) => "Pause".into(),
        RowItem::Do(Action::Resume) => "Resume".into(),
        RowItem::Do(Action::Check) => "Check".into(),
        RowItem::Do(Action::Remove) => client::display::remove_choice(n, can_delete),
        RowItem::Do(Action::Delete) => client::display::delete_choice(n),
        RowItem::Do(Action::SetPriority) => "Set priority".into(),
        RowItem::Do(Action::Open) => "Open".into(),
        RowItem::Do(Action::Reveal) => "Reveal in Finder".into(),
        RowItem::CopyMagnet => "Copy magnet link".into(),
    }
}

fn run(s: &mut Phantom, id: TorrentId, item: RowItem, tx: &mut Tx) {
    let send = |s: &mut Phantom, action: Action, what: TorrentCommand| {
        command(&mut *s.service, &selected_for(&s.table, &s.torrents, action), what)
    };
    match item {
        RowItem::Do(Action::Pause) => send(s, Action::Pause, TorrentCommand::Pause),
        RowItem::Do(Action::Resume) => send(s, Action::Resume, TorrentCommand::Resume),
        RowItem::Do(Action::Check) => send(s, Action::Check, TorrentCommand::Check),
        RowItem::Do(Action::Remove) => remove_selected(&mut *s.service, &mut s.table, false),
        RowItem::Do(Action::Delete) => remove_selected(&mut *s.service, &mut s.table, true),
        // Per file, from the details pane.
        RowItem::Do(Action::SetPriority) => {}
        RowItem::Do(Action::Open) => show_torrent(s, id, Show::Open),
        RowItem::Do(Action::Reveal) => show_torrent(s, id, Show::Reveal),
        RowItem::CopyMagnet => copy_magnet(s, tx),
    }
}
