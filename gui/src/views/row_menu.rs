//! A torrent row's context menu: the actions the service lists for the selection (Open and Reveal
//! in Finder for a single torrent, the toolbar's others), plus Copy magnet link. What it offers is
//! [`row_items`]'s call; this words the items and runs the one picked.

use proto::{Action, TorrentCommand, TorrentId};
use rune::{PointerPress, PopupItem, PopupMenu, Tx};

use crate::model::{Phantom, RowItem, command, copy_magnet, remove_selected, row_items, select_for_menu, selected_for, selected_torrents};
use crate::platform::{Show, show_torrent};

/// Opens the menu at `press` for a right-click on torrent `id`'s row.
pub fn open(s: &mut Phantom, id: TorrentId, press: PointerPress, tx: &mut Tx) {
    select_for_menu(&mut *s.service, &mut s.table, &s.torrents, &mut s.pane, id);
    let items = row_items(&s.table, &s.torrents);
    if items.is_empty() {
        return;
    }
    let n = selected_torrents(&s.table).len();
    let can_delete = items.contains(&RowItem::Do(Action::Delete));
    let mut menu = PopupMenu::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && group(*item) != group(items[i - 1]) {
            menu = menu.separator();
        }
        menu = menu.item(PopupItem::new(item_id(*item), label(*item, n, can_delete)));
    }
    tx.popup_menu(menu, press, move |s: &mut Phantom, picked, tx| {
        if let Some(item) = items.into_iter().find(|i| Some(item_id(*i)) == picked) {
            run(s, id, item, tx);
        }
    });
}

/// Items of a group sit together, apart from the others' by a separator.
fn group(item: RowItem) -> u8 {
    match item {
        RowItem::Do(Action::Open | Action::Reveal) => 0,
        RowItem::Do(Action::Pause | Action::Resume | Action::Check | Action::SetPriority) => 1,
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
    let what = if n == 1 { "torrent".to_owned() } else { format!("{n} torrents") };
    match item {
        RowItem::Do(Action::Pause) => "Pause".into(),
        RowItem::Do(Action::Resume) => "Resume".into(),
        RowItem::Do(Action::Check) => "Check".into(),
        RowItem::Do(Action::Remove) if can_delete => format!("Remove {what} (keep files)"),
        RowItem::Do(Action::Remove) => format!("Remove {what}"),
        RowItem::Do(Action::Delete) => format!("Delete {what} and {} files", if n == 1 { "its" } else { "their" }),
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
