//! The torrent list, with sortable columns in the person's order: those that fit the window
//! ([`layout::fit`], #33), a drag on a header moving its column, a right-click on the header
//! listing them all ([`columns_menu`]). A status with a reason (Destination unavailable) shows
//! it on hover; one moving, what it copied, and its progress is the copy's. A right-click anywhere in a row opens its menu
//! ([`row_menu`]); a double-click opens the torrent. A torrent is renamed in the details pane's
//! header, not here (#331).

use std::borrow::Cow;

use client::display;
use proto::{Action, Status, TorrentInfo};
use rune::{Align, Cursor, Drag, El, Phase, Tx, el, row};

use super::{columns_menu, row_menu, space, speed, status_reason};
use crate::model::layout::{self, Column, Fitted};
use crate::model::{Phantom, follow_selection, sort};
use crate::platform::open_torrent;
use crate::ui::{Table, TableColumn, Tone, Ui};

/// The room the space warning's icon takes in a Status cell.
const SPACE_ICON: f32 = 18.0;

/// The columns that fit `width` points: each at the width it was dragged to, else its own; Status
/// as wide as its longest current word, Name taking the rest until it was sized.
pub fn fitted(s: &Phantom, ui: &Ui, width: f32) -> Fitted {
    layout::fit(width - layout::TOGGLE, s.prefs.order(), s.prefs.hidden(), s.prefs.width(Column::Name).is_none(), |c| dragged(s, ui, c))
}

/// How wide Status is as its longest current word.
fn longest_status(s: &Phantom, ui: &Ui) -> f32 {
    s.torrents
        .iter()
        .map(|t| ui.cell_width(display::status(&t.status)) + if space::short_on_space(s, t.id).is_some() { SPACE_ICON } else { 0.0 })
        .fold(ui.header_width(Column::Status.title()), f32::max)
        .ceil()
}

/// A column's width: the one its edge was dragged to (within its bounds), else its own (Status's
/// longest word, whatever). Name's, unsized, is the least it takes before columns hide.
fn dragged(s: &Phantom, ui: &Ui, c: Column) -> f32 {
    if c == Column::Status {
        return longest_status(s, ui);
    }
    s.prefs.width(c).map_or(c.width(), |w| declared(c).clamp(w))
}

/// The narrowest a dragged Name gets.
const NAME_LEAST: f32 = 120.0;

/// A column as the table declares it, its width aside.
fn declared(c: Column) -> TableColumn {
    // The table's own resize handles are off: the edges are the grips' ([`grips`]), which keep the
    // widths in the person's choices and know Name takes the rest.
    let column = TableColumn::new(c.id(), c.title()).sortable().resizable(false);
    match c {
        Column::Name => column.min(NAME_LEAST),
        Column::Progress => column.min(110.0).max(260.0),
        Column::Status => column,
        Column::Size | Column::Down | Column::Up | Column::Peers | Column::Eta => column.min(50.0).align(Align::End),
    }
}

pub fn torrent_table(s: &Phantom, ui: &Ui, fitted: &Fitted, width: f32) -> El {
    // The list offers the torrent commands, for its selection, but Rename…: the details pane's
    // header offers it, as its field is the header's name (ADR 0013).
    s.mounted.offer(crate::commands::OnTorrents::ALL.into_iter().filter(|c| *c != crate::commands::OnTorrents::Rename));
    let cui = ui.clone();
    let mut table = Table::new(
        |s: &Phantom| &s.table,
        |s: &mut Phantom| &mut s.table,
        |s: &Phantom| s.torrents.len(),
        |s: &Phantom, i| Cow::Owned(s.torrents[i].id.to_string()),
    )
    .name("Torrents");
    // The toggle first, whatever the order: no column the person can move or hide.
    table = table.column(TableColumn::new(TOGGLE_ID, "").width(layout::TOGGLE).min(layout::TOGGLE).max(layout::TOGGLE).resizable(false));
    for (c, width) in fitted {
        table = table.column(match width {
            None => declared(*c).flex(1.0).min(dragged(s, ui, *c)),
            Some(w) => declared(*c).width(*w),
        });
    }
    let table = table
        .cell(move |s: &Phantom, i, column| {
            let t = &s.torrents[i];
            let id = t.id;
            let content = match (column, t.content) {
                (TOGGLE_ID, _) => {
                    let toggle = layout_toggle(s, &cui, t);
                    return cui.hover_cell(
                        toggle,
                        Align::Center,
                        move |s: &mut Phantom, press, tx| row_menu::open(s, id, press, tx),
                        hover(id),
                    );
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
            cui.hover_cell(content, align, move |s: &mut Phantom, press, tx| row_menu::open(s, id, press, tx), hover(id))
        })
        .on_sort(|s: &mut Phantom, column, dir, _| sort(&mut s.torrents, column, dir))
        // Double-click, or Return: Open.
        .on_open(|s: &mut Phantom, id, _| {
            if let Ok(id) = id.parse() {
                open_torrent(s, id);
            }
        })
        .on_select(|s: &mut Phantom, _, _| follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane));
    // A right-click on the header (the rows' cells have their own menu) lists the columns.
    ui.table(s, table)
        .grow(1.0)
        .min_h(0.0)
        .on_secondary_press_local(columns_menu::open)
        .on_key_preview(crate::shortcuts::on_list_key)
        .child(grips(s, ui, fitted, width - layout::TOGGLE))
}

/// The toggle's column id.
const TOGGLE_ID: &str = "toggle";

/// Tells the model which row the pointer is on, for its toggle's strength; whether that changed.
fn hover(id: proto::TorrentId) -> impl Fn(&mut Phantom, bool) -> bool + 'static {
    move |s: &mut Phantom, inside| {
        let hot = if inside {
            Some(id)
        } else if s.hot == Some(id) {
            None
        } else {
            s.hot
        };
        std::mem::replace(&mut s.hot, hot) != hot
    }
}

/// A row's toggle: what a click on it does to its torrent alone, strong while its row is selected
/// or hovered. Where the service lists neither Pause nor Resume (a torrent whose info couldn't be fetched) it's shown
/// disabled, the icon of what it would do, its tip the status word.
fn layout_toggle(s: &Phantom, ui: &Ui, t: &TorrentInfo) -> El {
    let id = t.id;
    let strong = s.hot == Some(id) || s.table.is_selected(&id.to_string());
    let next = crate::model::row_toggle(t);
    let button = match next {
        Some(action) => {
            ui.row_toggle(action == Action::Pause, strong, None).on_press(move |s: &mut Phantom| crate::model::toggle_row(s, id))
        }
        // A press on it is taken, so it doesn't reach the row and select it.
        None => ui
            .row_toggle(crate::model::toggle_icon(t) == Action::Pause, strong, Some(display::status(&t.status)))
            .on_press(|_: &mut Phantom| {}),
    };
    row().w_full().h_full().center().child(button)
}

/// How far a press travels before it's a drag of its column, not a click that sorts.
const DRAG_SLOP: f32 = 4.0;

/// Over the table's own header, a grip on each column and a handle on its right edge, both laid
/// out as the table lays its cells out (Name taking the rest, no less than its least). A click on
/// a grip sorts by it, as the header's own cell would; a drag moves the column along the header,
/// the others making way as it passes them, kept when let go, put back on Escape. A drag on a
/// handle (not Status's: as wide as its longest word) moves that edge under the pointer, the
/// columns left of it staying and those right of it shifting; room that frees at the right end
/// stays empty until a hidden column fits whole. Kept when let go; a double-click puts it back
/// (Name: to taking the rest).
fn grips(s: &Phantom, ui: &Ui, fitted: &Fitted, width: f32) -> El {
    let fixed: f32 = fitted.iter().filter_map(|(_, w)| *w).sum();
    let spans: Vec<(Column, f32)> = fitted.iter().map(|(c, w)| (*c, w.unwrap_or((width - fixed).max(dragged(s, ui, *c))))).collect();
    // Name's drawn width while it takes the rest: another column's drag fixes it there.
    let name = fitted
        .iter()
        .find(|(c, w)| *c == Column::Name && w.is_none())
        .and_then(|_| spans.iter().find(|(c, _)| *c == Column::Name))
        .map(|(_, w)| *w);
    let shown: Vec<Column> = spans.iter().map(|(c, _)| *c).collect();
    let mut grips = row().key("grips").pin(Some(0.0), Some(0.0), Some(0.0), None).h(layout::TABLE_HEADER).child(el().w(layout::TOGGLE));
    let mut start = layout::TOGGLE;
    for (c, w) in fitted.iter().copied() {
        let cell = match w {
            Some(w) => row().w(w),
            None => row().w(0.0).grow(1.0).shrink().min_w(dragged(s, ui, c)),
        };
        let active = s.prefs.moving.as_ref().is_some_and(|m| m.column == c);
        let (all, shown, left) = (spans.clone(), shown.clone(), start + GRIP_INSET);
        let grip = ui.column_grip(active).key("grip").on_press(move |s: &mut Phantom| sort_by(s, c)).on_drag(
            move |s: &mut Phantom, d: &mut Drag, _: &mut Tx| match d.phase {
                Phase::Down => {
                    s.prefs.moved = false;
                    s.prefs.start_move(c);
                    d.data = left + d.x;
                }
                Phase::Move if d.dx.abs() > DRAG_SLOP || s.prefs.moved => {
                    s.prefs.moved = true;
                    s.prefs.move_to(&shown, layout::drop_index(&all, c, d.data + d.dx));
                }
                Phase::Move => {}
                Phase::Up => s.prefs.end_move(d.cancelled()),
            },
        );
        let drawn = spans.iter().find(|(s, _)| *s == c).map_or(0.0, |(_, w)| *w);
        let mut cell = cell.key(c.id()).h_full().px(GRIP_INSET).child(grip);
        // Status is as wide as its longest word: no edge to drag.
        if c != Column::Status {
            let bounds = declared(c);
            let bounds = (bounds.clamp(0.0), bounds.clamp(f32::MAX));
            cell = cell.child(resize_handle(c, drawn, width - (start - layout::TOGGLE), bounds, name));
        }
        grips = grips.child(cell);
        start += drawn;
    }
    grips
}

/// The drag target on a column's right edge, in the room the table's own handle took (the grips
/// leave it free): the column is at `drawn` points as the drag starts, and goes where the pointer
/// does, within its `bounds` and the `room` the list has from its left edge (a column never drags
/// itself out of the list). `name` is Name's drawn width if it takes the rest, which another
/// column's drag fixes. Pressed and let go without moving, it changes
/// nothing, so a double-click is no resize.
fn resize_handle(c: Column, drawn: f32, room: f32, (least, most): (f32, f32), name: Option<f32>) -> El {
    el().key("resize")
        .role("separator")
        .label(format!("Resize {}", c.title()))
        .pin(None, Some(6.0), Some(-4.0), Some(6.0))
        .w(9.0)
        .cursor(Cursor::ResizeColumn)
        .on_drag(move |s: &mut Phantom, d: &mut Drag, _: &mut Tx| match d.phase {
            Phase::Down => d.data = drawn,
            _ if d.cancelled() => s.prefs.end_resize(true),
            // Once it moved, every move counts, back to where it began too.
            Phase::Move if d.dx != 0.0 || s.prefs.resizing.is_some_and(|r| r.column == c) => {
                s.prefs.resize_to(c, (d.data + d.dx).min(room).max(least).min(most.max(least)), name)
            }
            Phase::Move => {}
            Phase::Up => s.prefs.end_resize(false),
        })
        .on_double_press(move |s: &mut Phantom| s.prefs.reset_width(c))
}

/// What a grip leaves free at each side of its column: the table's resize handle.
const GRIP_INSET: f32 = 5.0;

/// A click on a column's grip: the list sorts by it, the other way if it already does, as a click
/// on the table's own header cell would. Not after a drag of it.
fn sort_by(s: &mut Phantom, c: Column) {
    if std::mem::take(&mut s.prefs.moved) {
        return;
    }
    crate::model::sort_by(s, c);
}

/// Statuses that need the user look like errors; the rest are secondary.
fn status_tone(status: &Status) -> Tone {
    match status {
        Status::Error { .. } | Status::MissingFiles | Status::DestinationUnavailable { .. } => Tone::Error,
        _ => Tone::Muted,
    }
}
