//! The toolbar: the magnet field, Add and Open…, and Resume, Pause and Remove… (which offers
//! Remove and Delete) for the selection.

use proto::Status;
use rune::{El, FileDialogOptions, PopupItem, PopupMenu, Tx, el, row};

use crate::model::{Phantom, LinkProblem, add_files, link_problem, remove_selected, selected_torrents, selected_with_info, submit_url};
use crate::service::Notice;
use crate::ui::Ui;

pub fn toolbar(s: &Phantom, ui: &Ui, overlays: &mut Vec<El>) -> El {
    let problem = link_problem(&s.magnet.text);
    let add = ui
        .button("add", "Add")
        .primary()
        .disabled(problem.is_some())
        .on_press(|s: &mut Phantom| submit_url(&mut *s.service, &mut s.magnet, &mut s.add_hint))
        .on_press_disabled(|s: &mut Phantom| s.add_hint = Some(s.magnet.text.clone()))
        .build();
    // Why Add is off, once it's been pressed so; any edit closes it.
    let why = problem.filter(|_| s.add_hint.as_deref() == Some(&s.magnet.text));
    let add = ui.note(
        "add-note",
        add,
        why.map_or("", LinkProblem::reason),
        why.is_some(),
        |s: &mut Phantom| s.add_hint = None,
        overlays,
    );
    let any_with_info = !selected_with_info(&s.table, &s.torrents).is_empty();
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
                .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.service, &mut s.magnet, &mut s.add_hint))
                .build()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(add)
        .child(ui.button("open", "Open…").on_press_tx(open_torrent_file).build())
        .child(el().w(12.0))
        .child(
            ui.button("resume", "Resume")
                .disabled(!any_with_info)
                .on_press(|s: &mut Phantom| s.service.resume(&selected_with_info(&s.table, &s.torrents)))
                .build(),
        )
        .child(
            ui.button("pause", "Pause")
                .disabled(!any_with_info)
                .on_press(|s: &mut Phantom| s.service.pause(&selected_with_info(&s.table, &s.torrents)))
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
    let fetching = s.torrents.iter().filter(|t| selected.contains(&t.id)).all(|t| t.status == Status::FetchingInfo);
    let menu = if fetching {
        PopupMenu::new().item(PopupItem::new("remove", format!("Remove {what}")))
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
