//! The torrent list: pending adds first, then the torrents, with sortable columns.

use std::borrow::Cow;

use client::format;
use proto::{PendingAdd, TorrentState};
use rune::{Align, El};

use super::speed;
use crate::model::{Phantom, row_id, sort};
use crate::ui::{Cell, Table, TableColumn, Ui};

pub fn torrent_table(s: &Phantom, ui: &Ui) -> El {
    let cui = ui.clone();
    let table = Table::new(
        |s: &Phantom| &s.table,
        |s: &mut Phantom| &mut s.table,
        |s: &Phantom| s.row_count(),
        |s: &Phantom, i| Cow::Owned(row_id(&s.pending, &s.torrents, i)),
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
        if let Some(p) = s.pending.get(i) {
            return pending_cell(p, column);
        }
        let t = &s.torrents[i - s.pending.len()];
        match column {
            "name" => Cell::text(t.name.as_deref().unwrap_or("Resolving metadata…")),
            "progress" => cui.progress_cell(t.progress()),
            "status" if t.state == TorrentState::Error => cui.error_cell(format::status(t)),
            "status" => Cell::text(format::status(t)).muted(),
            // Unknown until a magnet's metadata arrives.
            "size" if t.total_bytes == 0 => Cell::text("–").mono().muted(),
            "size" => Cell::text(format::bytes(t.total_bytes)).mono(),
            "down" => Cell::text(speed(t.download_bytes_per_sec)).mono(),
            "up" => Cell::text(speed(t.upload_bytes_per_sec)).mono(),
            "peers" => Cell::text(t.live_peers.to_string()).mono(),
            _ => Cell::text(t.eta().map(format::duration).unwrap_or_else(|| "–".into())).mono().muted(),
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| sort(&mut s.torrents, column, dir));
    ui.table(s, table).grow(1.0).min_h(0.0)
}

fn pending_cell(p: &PendingAdd, column: &str) -> Cell {
    match column {
        "name" => Cell::text(p.name.as_deref().unwrap_or(&p.source)),
        // Static text rather than an indeterminate bar: a magnet can wait for minutes, and a
        // looping animation would keep the window drawing the whole time.
        "progress" => Cell::text(format!("waiting {}", format::duration(std::time::Duration::from_secs(p.elapsed_secs)))).muted(),
        "status" => Cell::text("resolving").muted(),
        _ => Cell::text("–").mono().muted(),
    }
}
