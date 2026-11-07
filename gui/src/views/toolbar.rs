//! The toolbar: the magnet field, Add and Open…, and Resume, Pause and Remove… for the selection.

use rune::{El, FileDialogOptions, PopupItem, PopupMenu, Tx, el, row};

use crate::model::{Phantom, add_files, remove_selected, selected_pending, selected_torrents, submit_url};
use crate::service::Notice;
use crate::ui::Ui;

pub fn toolbar(s: &Phantom, ui: &Ui) -> El {
    let has_magnet = !s.magnet.text.trim().is_empty();
    let any_torrents = !selected_torrents(&s.table).is_empty();
    let any_selected = s.table.selected_len() > 0;
    row()
        .key("toolbar")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            ui.text_field("magnet-field", &s.magnet, "Magnet link")
                .placeholder("Paste a magnet link or .torrent URL")
                .on_edit(|s: &mut Phantom| &mut s.magnet)
                .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.service, &mut s.magnet))
                .build()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            ui.button("add", "Add")
                .primary()
                .disabled(!has_magnet)
                .on_press(|s: &mut Phantom| submit_url(&mut *s.service, &mut s.magnet))
                .build(),
        )
        .child(ui.button("open", "Open…").on_press_tx(open_torrent_file).build())
        .child(el().w(12.0))
        .child(
            ui.button("resume", "Resume")
                .disabled(!any_torrents)
                .on_press(|s: &mut Phantom| s.service.resume(&selected_torrents(&s.table)))
                .build(),
        )
        .child(
            ui.button("pause", "Pause")
                .disabled(!any_torrents)
                .on_press(|s: &mut Phantom| s.service.pause(&selected_torrents(&s.table)))
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
    let plural = |n: usize, one: &str| if n == 1 { one.to_owned() } else { format!("{n} {one}s") };
    let (torrents, pending) = (selected_torrents(&s.table).len(), selected_pending(&s.table).len());
    // Pending adds have no files yet, so they only offer to stop; they're stopped either way.
    let menu = if torrents == 0 {
        PopupMenu::new().item(PopupItem::new("keep", format!("Stop adding {}", plural(pending, "torrent"))))
    } else {
        let what = plural(torrents, "torrent");
        PopupMenu::new()
            .item(PopupItem::new("keep", format!("Remove {what}, keep files")))
            .item(PopupItem::new("delete", format!("Remove {what} and delete files")))
    };
    tx.popup_menu(menu, "remove", |s: &mut Phantom, picked, _| {
        let delete_files = match picked {
            Some("keep") => false,
            Some("delete") => true,
            _ => return,
        };
        remove_selected(&mut *s.service, &mut s.table, delete_files);
    });
}
