//! A torrent row's context menu: the torrent commands available for the selection (`commands`),
//! in the list's order, each with its icon and key. What it offers is the list's call; this groups
//! the items and runs the one picked. Move to ▸ opens a submenu (#87): the default folder, the
//! recent ones (`model::recent`), Other… (the folder picker).

use std::path::Path;

use proto::TorrentId;
use rune::{El, PointerPress, Tx};

use crate::commands::{OnTorrents, on_torrents, run_on_torrents};
use crate::menu::{Event, Menu, Open};
use crate::model::recent;
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
    let Menu::Open(open @ Open::Row { at, .. }) = s.menu else { return };
    let items = on_torrents(s);
    let sub = s.move_sub == Some(open);
    ui.context_menu(
        "row-menu",
        Some(at),
        window,
        entries(&items, selected_torrents(&s.table).len(), &recent::offered(s), sub),
        move |s: &mut Phantom, picked, tx| pick(s, picked, tx),
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}

/// What a pick in the menu does: Move to opens or closes its submenu; a folder of it moves the
/// selection there, Other… asks for one; any other item runs its command.
fn pick(s: &mut Phantom, picked: &str, tx: &mut Tx) {
    if picked == OnTorrents::MoveTo.spec().id {
        let Menu::Open(open) = s.menu else { return };
        s.move_sub = (s.move_sub != Some(open)).then_some(open);
        return;
    }
    s.menu = s.menu.then(Event::Close);
    if picked == OTHER {
        return crate::model::moving::move_to(s, tx);
    }
    if let Some(i) = FOLDERS.iter().position(|id| *id == picked) {
        if let Some(folder) = recent::offered(s).into_iter().nth(i) {
            crate::model::moving::move_selection_to(s, folder.folder);
        }
        return;
    }
    if let Some(item) = on_torrents(s).into_iter().find(|i| i.spec().id == picked) {
        run_on_torrents(s, item, tx);
    }
}

/// The ids of Move to ▸'s folders, in its order, and of Other….
const FOLDERS: [&str; 1 + recent::MOST] = ["move-to-0", "move-to-1", "move-to-2", "move-to-3", "move-to-4", "move-to-5"];
const OTHER: &str = "move-other";

/// Move to ▸'s entries: the default folder (its key hint "Default"), the recent ones, then Other….
fn move_to_entries(folders: &[recent::Offered]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = folders
        .iter()
        .zip(FOLDERS)
        .map(|(f, id)| Entry::Choice {
            id,
            label: recent::name(&f.folder),
            hint: f.default.then(|| client::display::default_folder().to_owned()),
            icon: None,
            checked: None,
        })
        .collect();
    if !entries.is_empty() {
        entries.push(Entry::Separator);
    }
    entries.push(Entry::Choice { id: OTHER, label: client::display::other_folder().into(), hint: None, icon: None, checked: None });
    entries
}

/// The menu's entries for `n` torrents selected: the items, a separator between groups; Move to
/// with its submenu of `folders`, open if `sub`.
pub fn entries(items: &[OnTorrents], n: usize, folders: &[recent::Offered], sub: bool) -> Vec<Entry> {
    let can_delete = items.contains(&OnTorrents::Delete);
    let mut entries = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && item.group() != items[i - 1].group() {
            entries.push(Entry::Separator);
        }
        let spec = item.spec();
        if *item == OnTorrents::MoveTo {
            let label = client::display::move_to_submenu().into();
            entries.push(Entry::Sub { id: spec.id, label, icon: Some(spec.icon), entries: move_to_entries(folders), open: sub });
            continue;
        }
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
