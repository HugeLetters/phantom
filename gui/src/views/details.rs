//! The details pane, under the list while exactly one torrent is selected: its name, Size (and
//! Full size when some files are Off), where it is with Reveal in Finder (or, while its destination
//! is unavailable, why, with where it is on hover), Check, and its files with their progress (or
//! Missing) and priority. Normal and Off set the priority of the selected files.

use std::borrow::Cow;
use std::path::Path;

use client::display;
use proto::{File, Priority, Status, TorrentInfo};
use rune::{Align, El, col, el, row};

use super::status_reason;
use crate::model::{Phantom, can_check, is_away, reveal_target, selected_files, set_selected_priority, size_label, sort_files};
use crate::platform;
use crate::ui::{Cell, Table, TableColumn, Tone, Ui};

pub fn details_pane(s: &Phantom, ui: &Ui, t: &TorrentInfo) -> El {
    let fetching = t.status == Status::FetchingInfo;
    let reveal = reveal_target(t, &s.pane);
    let any_files = !selected_files(&s.pane).is_empty();
    let name = t.name.as_deref().or(t.info_hash.as_deref()).unwrap_or("Unnamed torrent");
    let location = match reveal {
        Some(path) => home_relative(path),
        None if fetching => "Not on disk until its info arrives".into(),
        None => String::new(),
    };

    let title = col()
        .grow(1.0)
        .min_w(0.0)
        .child(
            // On a switch the size slides to its new place. The name's width springs with it, and
            // the name is cut to that width, so the size moves along its end rather than over it.
            row()
                .animate_layout()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(ui.heading(name).key("details-name").min_w(0.0).shrink().clip_rounded(0.0))
                .child(ui.caption(if fetching { String::new() } else { size_label(t) }).key("details-size")),
        )
        .child(match status_reason(t) {
            Some(reason) => ui.error_caption(reason).key("details-location").truncate().w_full().help(location),
            None => ui.caption(location).key("details-location").truncate().w_full(),
        });
    // Inset as far as the table's cells, so the name lines up with the files.
    let header = row()
        .key("details-header")
        .w_full()
        .px(10.0)
        .pt(6.0)
        .gap(6.0)
        .center_y()
        .child(title)
        .child(el().w(6.0))
        .child(ui.caption("Priority"))
        .child(
            ui.button("file-normal", Priority::Normal.label())
                .disabled(!any_files)
                .on_press(|s: &mut Phantom| set_selected_priority(&mut *s.service, &s.pane, Priority::Normal))
                .build()
                .help("Download the selected files"),
        )
        .child(
            ui.button("file-off", Priority::Off.label())
                .disabled(!any_files)
                .on_press(|s: &mut Phantom| set_selected_priority(&mut *s.service, &s.pane, Priority::Off))
                .build()
                .help("Don't download the selected files"),
        )
        .child(el().w(12.0))
        .child({
            let id = t.id;
            ui.button("check", "Check")
                .disabled(!can_check(t, &s.pane))
                .on_press(move |s: &mut Phantom| s.service.check(&[id]))
                .build()
                .help("Verify the files on disk, keeping what's valid and downloading the rest")
        })
        .child(
            ui.button("reveal", "Reveal in Finder")
                .disabled(reveal.is_none() || is_away(t))
                .on_press(platform::reveal)
                .build(),
        );

    let files = if fetching {
        ui.empty_state("No files yet", "They show once the torrent's info arrives").key("no-files")
    } else if s.pane.details.is_some() {
        file_table(s, ui)
    } else {
        el().grow(1.0)
    };

    ui.panel()
        .key("details")
        .w_full()
        .child(
            col()
                .w_full()
                .h_full()
                .gap(2.0)
                .child(header)
                .child(files.grow(1.0).min_h(0.0)),
        )
}

fn file_table(s: &Phantom, ui: &Ui) -> El {
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
        match column {
            "path" => Cell::text(f.path.to_string_lossy()),
            "progress" if f.missing => cui.text_cell("Missing", Tone::Error),
            "progress" => cui.progress_cell(f.progress()),
            "size" => Cell::text(display::bytes(f.size_bytes)).mono(),
            _ if f.priority == Priority::Off => cui.text_cell(f.priority.label(), Tone::Muted),
            _ => cui.text_cell(f.priority.label(), Tone::Normal),
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| {
        if let Some(details) = &mut s.pane.details {
            sort_files(details, column, dir);
        }
    });
    ui.table(s, table)
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
