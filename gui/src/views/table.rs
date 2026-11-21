//! The torrent list, with sortable columns. A status with a reason (Destination unavailable) shows
//! it on hover; one moving, what it copied, and its progress is the copy's. A right-click anywhere in a row opens its menu
//! ([`row_menu`]); a double-click opens the torrent. A torrent being renamed (#331) has a text field
//! for its Name cell, and a note under it while the name typed isn't one.

use std::borrow::Cow;

use client::display;
use proto::Status;
use rune::{Align, El, Side, col, el, row};

use super::{row_menu, space, speed, status_reason};
use crate::model::{Phantom, follow_selection, renaming, sort};
use crate::platform::open_torrent;
use crate::ui::{TEXT_INSET, Table, TableColumn, Tone, Ui};

/// `room`: where the Name cell being renamed was at the last frame, and the list's right edge.
pub fn torrent_table(s: &Phantom, ui: &Ui, room: Room, overlays: &mut Vec<El>) -> El {
    // The list offers the torrent commands, for its selection.
    s.mounted.offer(crate::commands::OnTorrents::ALL);
    rename_field(s, ui, room, overlays);
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
            // The name hides under the field, which is drawn over the list from the foot of this
            // cell (`rename_field`), as wide as the name.
            ("name", _) if s.rename.editing(id) => {
                let foot = el().key("rename-anchor").w_full().h(0.0).ml(-TEXT_INSET);
                col().key("rename-cell").w_full().h_full().child(el().grow(1.0)).child(foot)
            }
            ("name", _) => cui.cell_text(t.display_name(), Tone::Normal, false),
            // The free-space warning, while its disk is short for the torrents that need it (#127,
            // #209): its status as ever, in the warning colour with a warning icon, and the
            // disk's picture on hover.
            ("status", _) => match (space::short_on_space(s, id), status_reason(t)) {
                (Some(why), _) => cui.warned_text(client::display::status(&t.status), why, format!("short-on-space/{id}")),
                (None, Some(reason)) => cui.text_with_help(client::display::status(&t.status), status_tone(&t.status), reason),
                // Moving, what's copied of what it copies, on hover (#24).
                (None, None) => match display::move_progress(&t.status) {
                    Some(copied) => cui.text_with_help(client::display::status(&t.status), status_tone(&t.status), copied),
                    None => cui.cell_text(client::display::status(&t.status), status_tone(&t.status), false),
                },
            },
            // Nothing to show until the torrent's info arrives. Not an indeterminate bar: a
            // magnet can fetch for minutes, and a looping animation would keep the window
            // drawing the whole time.
            (_, None) => cui.cell_text("–", Tone::Muted, true),
            // Moving, what's copied of what it copies (#24); its status says how much on hover.
            ("progress", _) if matches!(t.status, Status::Moving { total, .. } if total > 0) => {
                let Status::Moving { done, total } = t.status else { unreachable!() };
                cui.progress_bar((done as f64 / total as f64) as f32)
            }
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
            open_torrent(s, id);
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

/// Where the Name cell being renamed was drawn at the last frame (x, y, width, height), and the
/// list's right edge: how wide its field may get.
#[derive(Clone, Copy, Default)]
pub struct Room {
    pub cell: Option<[f32; 4]>,
    /// The list's x, y, width and height.
    pub list: Option<[f32; 4]>,
}

/// The rename field (#331), over the list at the Name cell's place, its text exactly where the
/// name was; as wide as the whole name (its extension too) and never narrower than the cell, up
/// to the list's edge, over the cells beside it. A name refused for what it is (Return or a click
/// away) is said in a tooltip under it, until the next keystroke.
fn rename_field(s: &Phantom, ui: &Ui, room: Room, overlays: &mut Vec<El>) {
    if s.rename.id.is_none() {
        return;
    }
    let (font, selected) = ui.table_cell_look();
    let cell = room.cell.unwrap_or([0.0, 0.0, 180.0, ROW_HEIGHT]);
    let widest = room.list.map_or(f32::MAX, |l| l[0] + l[2] - cell[0] + TEXT_INSET);
    let width = ui.in_place_width(&s.rename.field.text).max(cell[2] + TEXT_INSET).min(widest);
    let field = ui
        .text_field("rename-field", &s.rename.field, "Name")
        .plain()
        .text_style(font)
        .height(ROW_HEIGHT)
        .on_edit(|s: &mut Phantom| &mut s.rename.field)
        .on_submit(|s: &mut Phantom, _| renaming::submit(s))
        .on_key(|s: &mut Phantom, ev: &rune::KeyEvent, _: &mut rune::Tx| match ev.key {
            rune::Key::Escape => {
                renaming::close(s);
                true
            }
            _ => false,
        })
        .build()
        .grow(1.0)
        .min_w(0.0);
    let field = ui.in_place(row().w_full().h(ROW_HEIGHT).child(field), selected).w(width).h(ROW_HEIGHT);
    let over = row().key("rename-over").child(field).anchored("rename-anchor", Side::Top, Align::Start, 0.0);
    // Inside the list's rows, under its header: scrolled out of them, it's out of sight as its row is.
    overlays.push(match room.list {
        Some([x, y, w, h]) => el().absolute(x, y + HEADER).size(w, (h - HEADER).max(0.0)).clip_rounded(0.0).child(over),
        None => over,
    });
    // Its tooltip, while the field is in sight.
    let in_sight = match (room.cell, room.list) {
        (Some(c), Some(l)) => c[1] >= l[1] + HEADER - 0.5 && c[1] + c[3] <= l[1] + l[3] + 0.5,
        _ => true,
    };
    if let Some(why) = s.rename.note().filter(|_| in_sight) {
        ui.tooltip_under("rename-note", "rename-over", &display::name_problem(why), overlays);
    }
}

/// A row's height in the list, the field's too, and the list's header's.
const ROW_HEIGHT: f32 = 28.0;
const HEADER: f32 = 30.0;
