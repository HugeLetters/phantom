//! The torrent list, with sortable columns.

use std::borrow::Cow;

use client::format;
use proto::Status;
use rune::{Align, El};

use super::speed;
use crate::model::{Phantom, follow_selection, sort};
use crate::ui::{Cell, Table, TableColumn, Tone, Ui};

pub fn torrent_table(s: &Phantom, ui: &Ui) -> El {
    let cui = ui.clone();
    let table = Table::new(
        |s: &Phantom| &s.table,
        |s: &mut Phantom| &mut s.table,
        |s: &Phantom| s.torrents.len(),
        |s: &Phantom, i| Cow::Owned(s.torrents[i].id.to_string()),
    )
    .name("Torrents")
    .column(TableColumn::new("name", "Name").flex(3.0).min(180.0).sortable())
    .column(TableColumn::new("progress", "Progress").width(150.0).min(110.0).max(260.0).sortable())
    .column(TableColumn::new("status", "Status").width(112.0).min(90.0).sortable())
    .column(TableColumn::new("size", "Size").width(84.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("down", "Down").width(92.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("up", "Up").width(92.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("peers", "Peers").width(60.0).min(50.0).align(Align::End).sortable())
    .column(TableColumn::new("eta", "ETA").width(72.0).min(60.0).align(Align::End).sortable())
    .cell(move |s: &Phantom, i, column| {
        let t = &s.torrents[i];
        let fetching = t.status == Status::FetchingInfo;
        match column {
            "name" => Cell::text(t.name.as_deref().or(t.info_hash.as_deref()).unwrap_or("Unnamed torrent")),
            "status" => cui.text_cell(t.status.label(), status_tone(&t.status)),
            // Nothing to show until the torrent's info arrives. Not an indeterminate bar: a
            // magnet can fetch for minutes, and a looping animation would keep the window
            // drawing the whole time.
            _ if fetching => cui.text_cell("–", Tone::Muted).mono(),
            "progress" => cui.progress_cell(t.progress()),
            "size" => Cell::text(format::bytes(t.size_bytes)).mono(),
            "down" => Cell::text(speed(t.download_bytes_per_sec)).mono(),
            "up" => Cell::text(speed(t.upload_bytes_per_sec)).mono(),
            "peers" => Cell::text(t.live_peers.to_string()).mono(),
            _ => cui.text_cell(t.eta().map(format::duration).unwrap_or_else(|| "–".into()), Tone::Muted).mono(),
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| sort(&mut s.torrents, column, dir))
    .on_select(|s: &mut Phantom, _, _| follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane));
    ui.table(s, table).grow(1.0).min_h(0.0)
}

/// Statuses that need the user look like errors; the rest are secondary.
fn status_tone(status: &Status) -> Tone {
    match status {
        Status::Error | Status::MissingFiles => Tone::Error,
        _ => Tone::Muted,
    }
}
