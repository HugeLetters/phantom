//! A torrent row's context menu: the torrent commands available for the selection (`commands`),
//! in the list's order, each with its icon and key. What it offers is the list's call; this groups
//! the items and runs the one picked.

use std::path::Path;

use proto::TorrentId;
use rune::{El, PointerPress, Tx};

use crate::commands::{OnTorrents, on_torrents, run_on_torrents};
use crate::menu::{Event, Menu, Open};
use crate::model::{Phantom, look_at_disk, reveal_target, select_for_menu, selected_torrents};
use crate::ui::{Entry, Ui};

/// Opens the menu at `press` for a right-click on torrent `id`'s row.
pub fn open(s: &mut Phantom, id: TorrentId, press: PointerPress, _: &mut Tx) {
    select_for_menu(&mut *s.service, &mut s.table, &s.torrents, &mut s.pane, id);
    let target = crate::model::details_torrent(&s.table, &s.torrents).and_then(|t| reveal_target(t).map(Path::to_owned));
    look_at_disk(s, target);
    crate::overlay::open_menu(s, Open::Row { id, at: (press.x, press.y) });
}

/// The open row menu, in `layer`.
/// `window` is the window's size, when known: the menu stays inside it.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    let Menu::Open(Open::Row { at, .. }) = s.menu else { return };
    let items = on_torrents(s);
    ui.context_menu(
        "row-menu",
        Some(at),
        window,
        entries(&items, selected_torrents(&s.table).len()),
        move |s: &mut Phantom, picked, tx| {
            s.menu = s.menu.then(Event::Close);
            if let Some(item) = on_torrents(s).into_iter().find(|i| i.spec().id == picked) {
                run_on_torrents(s, item, tx);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}

/// The menu's entries for `n` torrents selected: the items, a separator between groups.
pub fn entries(items: &[OnTorrents], n: usize) -> Vec<Entry> {
    let can_delete = items.contains(&OnTorrents::Delete);
    let mut entries = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && item.group() != items[i - 1].group() {
            entries.push(Entry::Separator);
        }
        let spec = item.spec();
        entries.push(Entry::Choice {
            id: spec.id,
            label: item.wording(n, can_delete),
            hint: spec.hint(),
            icon: Some(spec.icon),
            checked: None,
        });
    }
    entries
}
