//! The toolbar: the magnet field, Add and Open…, and Resume, Pause and Remove… (which offers
//! Remove and Delete) for the selection.

use proto::{Action, TorrentCommand};
use rune::{El, FileDialogOptions, Tx, el, row};

use crate::menu::{Event, Menu, Open};
use crate::model::{AddNote, Phantom, add_files, command, link_problem, remove_selected, selected_for, selected_torrents, submit_url};
use crate::notice::Notice;
use crate::ui::{At, Entry, Ui};

/// `menus` are the toolbar's open menus, which the page mounts after every other overlay: a menu
/// is modal, and takes what is drawn after it into its own focus.
pub fn toolbar(s: &Phantom, ui: &Ui, overlays: &mut Vec<El>, menus: &mut Vec<El>) -> El {
    let add = ui
        .button("add", "Add")
        .primary()
        .disabled(link_problem(&s.magnet.text).is_some())
        .alert(s.add_note.as_ref().is_some_and(|n| n.alerting))
        .on_press(|s: &mut Phantom| submit_url(&mut *s.service, &s.magnet, &mut s.add_note))
        .on_press_disabled(|s: &mut Phantom| {
            s.add_note = link_problem(&s.magnet.text).map(|p| AddNote::about(&s.magnet, client::display::link_problem(p)));
        })
        .build();
    // The message about the text in the field, until the text changes.
    let note = s.add_note.as_ref().filter(|n| n.text == s.magnet.text);
    let close = |s: &mut Phantom| s.add_note = None;
    let add = match note {
        Some(n) if n.error => ui.error_note("add-note", add, &n.message, true, close, overlays),
        _ => ui.note("add-note", add, note.map_or("", |n| &n.message), note.is_some(), close, overlays),
    };
    let remove = ui.button("remove", "Remove…").disabled(s.table.selected_len() == 0).on_press(open_remove).build();
    let at = matches!(s.menu, Menu::Open(Open::Remove)).then_some(At::Under);
    let remove = ui.menu(
        "remove-menu",
        remove,
        at,
        remove_entries(s),
        |s: &mut Phantom, picked, _| {
            s.menu = s.menu.then(Event::Close);
            remove_selected(&mut *s.service, &mut s.table, picked == "delete");
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        menus,
    );
    let can = |action| !selected_for(&s.table, &s.torrents, action).is_empty();
    row()
        .key("toolbar")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            ui.text_field("magnet-field", &s.magnet, "Magnet link")
                .placeholder("Paste a magnet link, .torrent URL or info hash")
                .on_edit(|s: &mut Phantom| &mut s.magnet)
                .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.service, &s.magnet, &mut s.add_note))
                .build()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(add)
        .child(ui.button("open", "Open…").on_press_tx(open_torrent_file).build())
        .child(el().w(12.0))
        .child(
            ui.button("resume", "Resume")
                .disabled(!can(Action::Resume))
                .on_press(|s: &mut Phantom| {
                    command(&mut *s.service, &selected_for(&s.table, &s.torrents, Action::Resume), TorrentCommand::Resume)
                })
                .build(),
        )
        .child(
            ui.button("pause", "Pause")
                .disabled(!can(Action::Pause))
                .on_press(|s: &mut Phantom| {
                    command(&mut *s.service, &selected_for(&s.table, &s.torrents, Action::Pause), TorrentCommand::Pause)
                })
                .build(),
        )
        .child(remove)
}

fn open_remove(s: &mut Phantom) {
    s.menu = s.menu.then(Event::Open(Open::Remove));
}

fn open_torrent_file(_: &mut Phantom, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Open torrent".into(), ..Default::default() }.filter("Torrent", &["torrent"]);
    tx.open_file(options, |s: &mut Phantom, result, _| match result {
        Ok(Some(path)) => add_files(s, &[path]),
        Ok(None) => {}
        Err(e) => s.errors.report(Notice::error(client::display::sentence(&format!("{e:#}")))),
    });
}

/// What Remove… offers for the selection.
fn remove_entries(s: &Phantom) -> Vec<Entry> {
    let selected = selected_torrents(&s.table);
    let n = selected.len();
    let what = if n == 1 { "torrent".to_owned() } else { format!("{n} torrents") };
    // Torrents fetching info have no files yet, so they only offer Remove, which stops them.
    let selection = || s.torrents.iter().filter(|t| selected.contains(&t.id));
    let choice = |id, label| Entry::Choice { id, label };
    if selection().all(|t| t.content.is_none()) {
        vec![choice("remove", format!("Remove {what}"))]
    } else if !selection().all(|t| t.actions.contains(&Action::Delete)) {
        // e.g. files where the destination can't be reached.
        vec![choice("remove", format!("Remove {what} (keep files)"))]
    } else {
        let their = if n == 1 { "its" } else { "their" };
        vec![choice("remove", format!("Remove {what} (keep files)")), choice("delete", format!("Delete {what} and {their} files"))]
    }
}
