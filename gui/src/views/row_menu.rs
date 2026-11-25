//! A torrent row's context menu: the torrent commands available for the selection (`commands`),
//! in the list's order, each with its icon and key. What it offers is the list's call; this groups
//! the items and runs the one picked. Move to ▸ opens a submenu (#87): the default folder, the
//! recent ones (`model::recent`), Other… (the folder picker). It opens on a click or Return, or
//! when the pointer rests on it a moment, as macOS's do; the pointer on another item closes it.

use std::path::Path;

use proto::{AbsolutePath, TorrentId};
use rune::{Align, El, PointerPress, Side, Tx, el};

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

/// How long the pointer rests on Move to ▸ before it opens, as long as macOS waits.
const OPEN_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

/// The open row menu, in `layer`.
/// `window` is the window's size, when known: the menu stays inside it. `layout` is where an
/// element was drawn at the last frame, by key: the menu's rows, for the pointer over them.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layout: impl Fn(&str) -> Option<[f32; 4]>, layer: &mut Vec<El>) {
    let Menu::Open(open @ Open::Row { at, .. }) = s.menu else { return };
    let items = on_torrents(s);
    let sub = s.move_sub == Some(open);
    let entries = entries(&items, selected_torrents(&s.table).len(), &recent::offered(s), sub);
    // Which entries are rows the pointer can rest on, by their place: the kit keys each
    // "row-menu-item<place>". Move to ▸ is the one that opens.
    let rows: Vec<(usize, bool)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Entry::Choice { .. } | Entry::Disabled { .. } => Some((i, false)),
            Entry::Sub { .. } => Some((i, true)),
            Entry::Title(_) | Entry::Separator => None,
        })
        .collect();
    ui.context_menu(
        "row-menu",
        Some(at),
        window,
        entries,
        move |s: &mut Phantom, picked, tx| pick(s, picked, tx),
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
    // The kit's items take no hover handler, so Phantom follows the pointer over them itself: a
    // zone over each row that takes no presses and no hover of its own. It's anchored to its row
    // (under it, pulled up by its height), which makes it part of the open menu: Rune leaves the
    // pointer nothing else while a modal is up.
    for (i, opens) in rows {
        let Some([_, _, w, h]) = layout(&format!("row-menu-item{i}")) else { continue };
        let zone = el()
            .key(format!("row-menu-zone{i}"))
            .size(w, h)
            .anchored(format!("row-menu-item{i}"), Side::Bottom, Align::Start, -h)
            .a11y_hidden();
        layer.push(match opens {
            true => zone
                .on_hover_change(move |s: &mut Phantom, inside, _: &mut Tx| {
                    if inside && s.menu == Menu::Open(open) {
                        s.move_sub = Some(open);
                    }
                })
                .hover_delays(OPEN_AFTER, std::time::Duration::ZERO),
            false => zone.on_hover_change(move |s: &mut Phantom, inside, _: &mut Tx| {
                if inside && s.move_sub == Some(open) {
                    s.move_sub = None;
                }
            }),
        });
    }
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
    // The folder is in the id, as the palette's entry carries it: the menu may have been drawn
    // with other folders than are offered now.
    if let Some(folder) = picked.strip_prefix(FOLDER).and_then(|p| AbsolutePath::new(p).ok()) {
        crate::model::moving::move_selection_to(s, folder);
        return;
    }
    if let Some(item) = on_torrents(s).into_iter().find(|i| i.spec().id == picked) {
        run_on_torrents(s, item, tx);
    }
}

/// The start of the id of a folder of Move to ▸, which ends with the folder's path, and the id of Other….
const FOLDER: &str = "move-to:";
const OTHER: &str = "move-other";

/// Move to ▸'s entries: the default folder (its key hint "Default"), the recent ones, then Other….
fn move_to_entries(folders: &[recent::Offered]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = folders
        .iter()
        .zip(recent::names(folders))
        .map(|(f, label)| Entry::Choice {
            id: format!("{FOLDER}{}", f.folder.display()).into(),
            label,
            hint: f.default.then(|| client::display::default_folder().to_owned()),
            icon: None,
            checked: None,
        })
        .collect();
    if !entries.is_empty() {
        entries.push(Entry::Separator);
    }
    entries.push(Entry::Choice { id: OTHER.into(), label: client::display::other_folder().into(), hint: None, icon: None, checked: None });
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
            entries.push(Entry::Sub { id: spec.id.into(), label, icon: spec.icon, entries: move_to_entries(folders), open: sub });
            continue;
        }
        entries.push(Entry::Choice {
            id: spec.id.into(),
            label: item.wording(n, can_delete),
            hint: spec.hint(),
            icon: spec.icon,
            checked: None,
        });
    }
    entries
}
