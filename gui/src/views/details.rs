//! The details pane, under the list while exactly one torrent is selected: its name, Size (and
//! Full size when some files are Off), where it is (or, while its destination is unavailable, why,
//! with where it is on hover), and its files with their progress (or Missing) and priority, which a
//! click cycles. Everything else that can be done to it is in its row's menu.

use std::borrow::Cow;
use std::path::Path;

use client::display;
use proto::{Action, File, Priority, TorrentInfo};
use rune::{Align, El, Tx, col, el, row};

use super::status_reason;
use crate::model::{Phantom, cycle_priority, reveal_target, size_label, sort_files};
use crate::shortcuts;
use crate::ui::{Table, TableColumn, Tone, Ui};

pub fn details_pane(s: &Phantom, ui: &Ui, t: &TorrentInfo) -> El {
    let fetching = t.content.is_none();
    let name = t.display_name();
    let location = match reveal_target(t) {
        Some(path) => home_relative(path),
        None if fetching => "Not on disk until its info arrives".into(),
        None => String::new(),
    };

    // Another torrent's name is a new title, not this one resizing: the size lands beside it at
    // once instead of sliding over a longer name. What can be done to it is in its row's menu.
    let title = col()
        .instant_changes()
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(ui.heading(name).key("details-name").min_w(0.0).shrink())
                .child(ui.caption(size_label(t)).key("details-size")),
        )
        .child(match status_reason(t) {
            Some(reason) => ui.error_caption(reason).key("details-location").truncate().min_w(0.0).shrink().help(location),
            None => ui.caption(location).key("details-location").truncate().min_w(0.0).shrink(),
        });
    // Inset as far as the table's cells, so the name lines up with the files.
    let header = row().key("details-header").w_full().px(10.0).pt(6.0).gap(6.0).center_y().child(title);

    let files = if fetching {
        ui.empty_state("No files yet", "They show once the torrent's info arrives").key("no-files")
    } else if s.pane.details.is_some() {
        file_table(s, ui, t.actions.does(Action::SetPriority))
    } else {
        el().grow(1.0)
    };

    ui.panel().key("details").w_full().child(col().w_full().h_full().gap(2.0).child(header).child(files.grow(1.0).min_h(0.0)))
}

/// The files; with `settable`, a click on a file's priority cycles it (Normal → Off → Normal):
/// for the selected files if it's one of them, else for it alone.
fn file_table(s: &Phantom, ui: &Ui, settable: bool) -> El {
    let cui = ui.clone();
    let table = Table::new(
        |s: &Phantom| &s.pane.files,
        |s: &mut Phantom| &mut s.pane.files,
        |s: &Phantom| s.pane.details.as_ref().map_or(0, |d| d.files.len()),
        |s: &Phantom, i| Cow::Owned(files(s)[i].index.to_string()),
    )
    .name("Files")
    .column(TableColumn::new("path", "File").flex(3.0).min(160.0).sortable())
    .column(TableColumn::new("progress", "Progress").width(150.0).min(110.0).max(260.0).sortable())
    .column(TableColumn::new("size", "Size").width(84.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("priority", "Priority").width(72.0).min(60.0).sortable())
    .cell(move |s: &Phantom, i, column| {
        let f = &files(s)[i];
        let index = f.index;
        // A right-click anywhere in a row opens the file menu.
        let menu = move |s: &mut Phantom, press, tx: &mut Tx| super::file_menu::open(s, index, press, tx);
        match column {
            "path" => cui.menu_cell(cui.cell_text(f.path.to_string_lossy(), Tone::Normal, false), Align::Start, menu),
            "progress" if f.held_aside => cui.menu_cell(cui.cell_text(display::held_aside(), Tone::Muted, false), Align::Start, menu),
            "progress" if f.missing => cui.menu_cell(cui.cell_text("Missing", Tone::Error, false), Align::Start, menu),
            "progress" => cui.menu_cell(cui.progress_bar(f.progress()), Align::Start, menu),
            "size" => cui.menu_cell(cui.cell_text(display::bytes(f.size_bytes), Tone::Normal, true), Align::End, menu),
            _ => {
                let tone = if f.priority == Priority::Off { Tone::Muted } else { Tone::Normal };
                let label = cui.cell_text(client::display::priority(f.priority), tone, false);
                cui.press_menu_cell(label, settable.then_some(move |s: &mut Phantom| cycle_priority(&mut *s.service, &s.pane, index)), menu)
            }
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| {
        if let Some(details) = &mut s.pane.details {
            sort_files(details, column, dir);
        }
    });
    // Where the file list has focus, the file shortcuts are its own: a key it leaves goes up to
    // here before the app's, so ⌘⌫ moves files to the Trash and never deletes the torrent.
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_key(|s: &mut Phantom, ev: &rune::KeyEvent, _: &mut Tx| shortcuts::on_file_key(s, ev))
        .child(ui.table(s, table).grow(1.0).min_h(0.0))
}

fn files(s: &Phantom) -> &[File] {
    s.pane.details.as_ref().map_or(&[], |d| &d.files)
}

/// `~/Downloads/…` rather than `/Users/name/Downloads/…`.
fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}
