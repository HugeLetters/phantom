//! The torrent list, with sortable columns. A status with a reason (Destination unavailable) shows
//! it on hover. A right-click anywhere in a row opens its menu
//! ([`row_menu`]); a double-click opens the torrent.

use std::borrow::Cow;

use client::display;
use proto::Status;
use rune::{Align, El};

use super::{row_menu, space, speed, status_reason};
use crate::model::{Phantom, follow_selection, sort};
use crate::platform::{Show, show_torrent};
use crate::ui::{Table, TableColumn, Tone, Ui};

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
    .column(TableColumn::new("status", "Status").width(180.0).min(90.0).sortable())
    .column(TableColumn::new("size", "Size").width(84.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("down", "Down").width(92.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("up", "Up").width(92.0).min(70.0).align(Align::End).sortable())
    .column(TableColumn::new("peers", "Peers").width(60.0).min(50.0).align(Align::End).sortable())
    .column(TableColumn::new("eta", "ETA").width(72.0).min(60.0).align(Align::End).sortable())
    .cell(move |s: &Phantom, i, column| {
        let t = &s.torrents[i];
        let id = t.id;
        let content = match (column, t.content) {
            ("name", _) => cui.cell_text(t.display_name(), Tone::Normal, false),
            // Its own free-space warning, while it alone needs more than its disk has (#127): its
            // status as ever, in the warning colour with a warning icon, and why on hover.
            ("status", _) => match (space::too_big(s, id), status_reason(t)) {
                (Some(why), _) => cui.warned_text(client::display::status(&t.status), why, format!("too-big/{id}")),
                (None, Some(reason)) => cui.text_with_help(client::display::status(&t.status), status_tone(&t.status), reason),
                (None, None) => cui.cell_text(client::display::status(&t.status), status_tone(&t.status), false),
            },
            // Nothing to show until the torrent's info arrives. Not an indeterminate bar: a
            // magnet can fetch for minutes, and a looping animation would keep the window
            // drawing the whole time.
            (_, None) => cui.cell_text("–", Tone::Muted, true),
            ("progress", Some(c)) => cui.progress_bar(c.progress()),
            ("size", Some(c)) => cui.cell_text(display::bytes(c.size_bytes), Tone::Normal, true),
            ("down", _) => cui.cell_text(speed(t.download_bytes_per_sec), Tone::Normal, true),
            ("up", _) => cui.cell_text(speed(t.upload_bytes_per_sec), Tone::Normal, true),
            ("peers", _) => cui.cell_text(t.live_peers.to_string(), Tone::Normal, true),
            _ => cui.cell_text(t.eta().map(display::duration).unwrap_or_else(|| "–".into()), Tone::Muted, true),
        };
        let align = if matches!(column, "size" | "down" | "up" | "peers" | "eta") { Align::End } else { Align::Start };
        cui.menu_cell(content, align, move |s: &mut Phantom, press, tx| row_menu::open(s, id, press, tx))
    })
    .on_sort(|s: &mut Phantom, column, dir, _| sort(&mut s.torrents, column, dir))
    // Double-click, or Return: Open.
    .on_open(|s: &mut Phantom, id, _| {
        if let Ok(id) = id.parse() {
            show_torrent(s, id, Show::Open);
        }
    })
    .on_select(|s: &mut Phantom, _, _| follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane));
    ui.table(s, table).grow(1.0).min_h(0.0)
}

/// Statuses that need the user look like errors; the rest are secondary.
fn status_tone(status: &Status) -> Tone {
    match status {
        Status::Error { .. } | Status::MissingFiles | Status::DestinationUnavailable { .. } => Tone::Error,
        _ => Tone::Muted,
    }
}
