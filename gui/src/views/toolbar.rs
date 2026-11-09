//! The toolbar: the magnet field, Add and Open…, and Resume, Pause and Remove… (which offers
//! Remove and Delete) for the selection.

use proto::{Action, Status};
use rune::{El, FileDialogOptions, PopupItem, PopupMenu, Tx, el, row};

use crate::model::{AddNote, Phantom, add_files, link_problem, remove_selected, selected_for, selected_torrents, submit_url};
use crate::service::Notice;
use crate::ui::Ui;

pub fn toolbar(s: &Phantom, ui: &Ui, overlays: &mut Vec<El>) -> El {
    let add = ui
        .button("add", "Add")
        .primary()
        .disabled(link_problem(&s.magnet.text).is_some() || s.adding)
        .alert(s.add_note.as_ref().is_some_and(|n| n.alerting))
        .on_press(|s: &mut Phantom| submit_url(&mut *s.service, &s.magnet, &mut s.adding, &mut s.add_note))
        .on_press_disabled(|s: &mut Phantom| {
            s.add_note = link_problem(&s.magnet.text).map(|p| AddNote::about(&s.magnet, p.reason()));
        })
        .build();
    // The message about the text in the field, until the text changes.
    let note = s.add_note.as_ref().filter(|n| n.text == s.magnet.text);
    let close = |s: &mut Phantom| s.add_note = None;
    let add = match note {
        Some(n) if n.error => ui.error_note("add-note", add, &n.message, true, close, overlays),
        _ => ui.note("add-note", add, note.map_or("", |n| &n.message), note.is_some(), close, overlays),
    };
    let can = |action| !selected_for(&s.table, &s.torrents, action).is_empty();
    let any_selected = s.table.selected_len() > 0;
    row()
        .key("toolbar")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            ui.text_field("magnet-field", &s.magnet, "Magnet link")
                .placeholder("Paste a magnet link, .torrent URL or info hash")
                .on_edit(|s: &mut Phantom| &mut s.magnet)
                .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.service, &s.magnet, &mut s.adding, &mut s.add_note))
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
                .on_press(|s: &mut Phantom| s.service.resume(&selected_for(&s.table, &s.torrents, Action::Resume)))
                .build(),
        )
        .child(
            ui.button("pause", "Pause")
                .disabled(!can(Action::Pause))
                .on_press(|s: &mut Phantom| s.service.pause(&selected_for(&s.table, &s.torrents, Action::Pause)))
                .build(),
        )
        .child(ui.button("remove", "Remove…").disabled(!any_selected).on_press_tx(ask_remove).build())
}

fn open_torrent_file(_: &mut Phantom, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Open torrent".into(), ..Default::default() }.filter("Torrent", &["torrent"]);
    tx.open_file(options, |s: &mut Phantom, result, _| match result {
        Ok(Some(path)) => add_files(&mut *s.service, &[path]),
        Ok(None) => {}
        Err(e) => s.notice = Some(Notice::error(format!("{e:#}"))),
    });
}

fn ask_remove(s: &mut Phantom, tx: &mut Tx) {
    let selected = selected_torrents(&s.table);
    let n = selected.len();
    let what = if n == 1 { "torrent".to_owned() } else { format!("{n} torrents") };
    // Torrents fetching info have no files yet, so they only offer Remove, which stops them.
    let selection = || s.torrents.iter().filter(|t| selected.contains(&t.id));
    let fetching = selection().all(|t| t.status == Status::FetchingInfo);
    let menu = if fetching {
        PopupMenu::new().item(PopupItem::new("remove", format!("Remove {what}")))
    } else if !selection().all(|t| t.actions.contains(&Action::Delete)) {
        // e.g. files where the destination can't be reached.
        PopupMenu::new().item(PopupItem::new("remove", format!("Remove {what} (keep files)")))
    } else {
        let their = if n == 1 { "its" } else { "their" };
        PopupMenu::new()
            .item(PopupItem::new("remove", format!("Remove {what} (keep files)")))
            .item(PopupItem::new("delete", format!("Delete {what} and {their} files")))
    };
    tx.popup_menu(menu, "remove", |s: &mut Phantom, picked, _| {
        let delete = match picked {
            Some("remove") => false,
            Some("delete") => true,
            _ => return,
        };
        remove_selected(&mut *s.service, &mut s.table, delete);
    });
}
