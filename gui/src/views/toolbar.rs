//! The toolbar: the magnet field, Add and Add file…, and Resume and Pause for the selection. Remove
//! and Delete have no button: ⌫ removes the selection and ⌘⌫ deletes it, and the row menu has both.

use client::display;
use proto::{Action, TorrentCommand};
use rune::{El, FileDialogOptions, Tx, el, row};

use crate::model::{AddNote, Phantom, add_files, link_problem, selected_for, selected_torrents, submit_url};
use crate::notice::Notice;
use crate::shortcuts::{self, Command};
use crate::ui::Ui;

pub fn toolbar(s: &Phantom, ui: &Ui, overlays: &mut Vec<El>) -> El {
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
        .child(
            ui.button("add-file", "Add file…")
                .on_press_tx(add_torrent_file)
                .build()
                .help(display::add_file_help(&shortcuts::hint(Command::AddFile))),
        )
        .child(el().w(12.0))
        .child(selection_button(s, ui, Action::Resume, overlays))
        .child(selection_button(s, ui, Action::Pause, overlays))
}

/// Resume or Pause for the selection: on while it does something to a torrent selected; clicked
/// while off, it says why in a note under it (#89), as Add does, until a click elsewhere or the
/// button turns on (the poll closes it then).
fn selection_button(s: &Phantom, ui: &Ui, action: Action, overlays: &mut Vec<El>) -> El {
    let (key, label, note, command) = match action {
        Action::Pause => ("pause", "Pause", "pause-note", TorrentCommand::Pause),
        _ => ("resume", "Resume", "resume-note", TorrentCommand::Resume),
    };
    let why = why_off(s, action);
    let button = ui
        .button(key, label)
        .disabled(why.is_some())
        .on_press(move |s: &mut Phantom| command_selected(s, action, command.clone()))
        .on_press_disabled(move |s: &mut Phantom| s.why_off = Some(action))
        .build();
    let open = s.why_off == Some(action) && why.is_some();
    ui.note(note, button, why.as_deref().unwrap_or(""), open, |s: &mut Phantom| s.why_off = None, overlays)
}

fn command_selected(s: &mut Phantom, action: Action, command: TorrentCommand) {
    crate::model::command(&mut *s.service, &selected_for(&s.table, &s.torrents, action), command)
}

/// Why `action` is off for the selection, worded by `client`; `None` while it's on.
pub fn why_off(s: &Phantom, action: Action) -> Option<String> {
    let selected = selected_torrents(&s.table);
    let answers: Vec<&proto::Answer> = s.torrents.iter().filter(|t| selected.contains(&t.id)).map(|t| t.actions.get(action)).collect();
    client::display::why_off(&answers)
}

/// Add file…: picks a .torrent file and adds it.
pub fn add_torrent_file(_: &mut Phantom, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Add torrent file".into(), ..Default::default() }.filter("Torrent", &["torrent"]);
    tx.open_file(options, |s: &mut Phantom, result, _| match result {
        Ok(Some(path)) => add_files(s, &[path]),
        Ok(None) => {}
        Err(e) => s.errors.report(Notice::error(client::display::sentence(&format!("{e:#}")))),
    });
}
