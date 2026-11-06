//! The window's contents. Handlers reach the daemon through `s.daemon`; the helpers they call
//! take only the pieces they use.

use std::borrow::Cow;

use proto::{PendingAdd, TorrentState, format};
use rune::{Align, Cx, El, FileDialogOptions, PopupItem, PopupMenu, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Cell, Kit, Progress, Table, TableColumn, TextInput};

use crate::daemon::{AddRequest, Connection, Notice};
use crate::{Phantom, add_files, remove_selected, row_id, selected_pending, selected_torrents, sort, submit_url};

pub fn view(s: &Phantom, cx: &mut Cx<Phantom>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let body = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(k.c.page)
        .padding(12.0)
        // The window's content runs under a transparent titlebar: clear the traffic lights
        // (the inset is 0 in full screen, where there are none).
        .pt(if cx.titlebar_inset() > 0.0 { 40.0 } else { 12.0 })
        .gap(10.0)
        .child(toolbar(s, &k))
        .child(
            col()
                .key("list")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .radius(k.radius.md)
                .fill(k.c.bg_base)
                .clip_rounded(k.radius.md)
                .child(if s.row_count() == 0 { empty_state(&k) } else { torrent_table(s, &k) }),
        )
        .child(status_bar(s, &k));
    // On the root, where Rune routes window-level pastes and drops: drop .torrent files
    // anywhere, or paste a magnet link with no field focused.
    rune_kit::root(body, [])
        .on_drop_files(|s: &mut Phantom, files, _| add_files(&mut *s.daemon, files))
        .accepts_extensions(&["torrent"])
        .on_paste(|s: &mut Phantom, pasted, _| s.daemon.add(AddRequest::Url(pasted.trim().to_owned())))
}

fn toolbar(s: &Phantom, k: &Kit) -> El {
    let has_magnet = !s.magnet.text.trim().is_empty();
    let any_torrents = !selected_torrents(&s.table).is_empty();
    let any_selected = s.table.selected_len() > 0;
    row()
        .key("toolbar")
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            el().key("magnet-field").grow(1.0).min_w(0.0).child(
                TextInput::new(&s.magnet)
                    .placeholder("Paste a magnet link or .torrent URL")
                    .label("Magnet link")
                    .width(None)
                    .on_edit(|s: &mut Phantom| &mut s.magnet)
                    .on_submit(|s: &mut Phantom, _| submit_url(&mut *s.daemon, &mut s.magnet))
                    .build(k),
            ),
        )
        .child(keyed(
            "add",
            Button::new("Add")
                .variant(ButtonVariant::Contrast)
                .disabled(!has_magnet)
                .on_press(|s: &mut Phantom| submit_url(&mut *s.daemon, &mut s.magnet))
                .build(k),
        ))
        .child(keyed("open", Button::new("Open…").on_press_tx(open_torrent_file).build(k)))
        .child(el().w(12.0))
        .child(keyed(
            "resume",
            Button::new("Resume")
                .disabled(!any_torrents)
                .on_press(|s: &mut Phantom| s.daemon.resume(&selected_torrents(&s.table)))
                .build(k),
        ))
        .child(keyed(
            "pause",
            Button::new("Pause")
                .disabled(!any_torrents)
                .on_press(|s: &mut Phantom| s.daemon.pause(&selected_torrents(&s.table)))
                .build(k),
        ))
        .child(keyed("remove", Button::new("Remove…").disabled(!any_selected).on_press_tx(ask_remove).build(k)))
}

fn open_torrent_file(_: &mut Phantom, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Open torrent".into(), ..Default::default() }.filter("Torrent", &["torrent"]);
    tx.open_file(options, |s: &mut Phantom, result, _| match result {
        Ok(Some(path)) => add_files(&mut *s.daemon, &[path]),
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
        remove_selected(&mut *s.daemon, &mut s.table, delete_files);
    });
}

fn empty_state(k: &Kit) -> El {
    col()
        .key("empty")
        .w_full()
        .h_full()
        .center()
        .gap(6.0)
        .child(text("No torrents").font(k.ty.body_medium).color(k.c.text_base))
        .child(
            text("Paste a magnet link, drop a .torrent file here, or click Open…")
                .font(k.ty.text_small)
                .color(k.c.text_muted),
        )
}

fn torrent_table(s: &Phantom, k: &Kit) -> El {
    let ck = k.clone();
    Table::new(
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
            "progress" => Cell::el(
                row()
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(el().grow(1.0).child(Progress::new(Some(t.progress())).build(&ck)))
                    .child(
                        text(format!("{:.0}%", (t.progress() * 100.0).floor()))
                            .font(ck.ty.text_small)
                            .color(ck.c.text_muted)
                            .w(34.0)
                            .align(Align::End),
                    ),
            ),
            "status" => {
                let cell = Cell::text(format::status(t));
                if t.state == TorrentState::Error { cell.color(ck.c.danger.icon) } else { cell.muted() }
            }
            // Unknown until a magnet's metadata arrives.
            "size" if t.total_bytes == 0 => Cell::text("–").mono().muted(),
            "size" => Cell::text(format::bytes(t.total_bytes)).mono(),
            "down" => Cell::text(speed(t.download_bytes_per_sec)).mono(),
            "up" => Cell::text(speed(t.upload_bytes_per_sec)).mono(),
            "peers" => Cell::text(t.live_peers.to_string()).mono(),
            _ => Cell::text(t.eta().map(format::duration).unwrap_or_else(|| "–".into())).mono().muted(),
        }
    })
    .on_sort(|s: &mut Phantom, column, dir, _| sort(&mut s.torrents, column, dir))
    .build(s, k)
    .grow(1.0)
    .min_h(0.0)
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

fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 { "–".into() } else { format!("{}/s", format::bytes(bytes_per_sec)) }
}

fn status_bar(s: &Phantom, k: &Kit) -> El {
    let (dot, connection) = match &s.connection {
        Connection::Connecting => (k.c.text_muted, "Starting daemon…".to_owned()),
        Connection::Connected => (k.c.success.icon, "Connected".to_owned()),
        Connection::Lost(e) => (k.c.danger.icon, format!("Daemon unavailable: {e}")),
    };
    let down: u64 = s.torrents.iter().map(|t| t.download_bytes_per_sec).sum();
    let up: u64 = s.torrents.iter().map(|t| t.upload_bytes_per_sec).sum();
    let mut totals = match s.row_count() {
        0 => String::new(),
        1 => format!("1 torrent · ↓ {} · ↑ {}", speed(down), speed(up)),
        n => format!("{n} torrents · ↓ {} · ↑ {}", speed(down), speed(up)),
    };
    if s.daemon.in_flight() > 0 {
        totals = if totals.is_empty() { "Working…".into() } else { format!("Working… · {totals}") };
    }

    let mut bar = row()
        .key("status")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
        .child(text(connection).font(k.ty.text_small).color(k.c.text_muted));
    bar = match &s.notice {
        Some(n) => bar.child(
            text(n.text.clone())
                .key("notice")
                .font(k.ty.text_small)
                .color(if n.error { k.c.danger.icon } else { k.c.text_base })
                .truncate()
                .grow(1.0)
                .min_w(0.0)
                .help("Click to dismiss")
                .on_press(|s: &mut Phantom| s.notice = None),
        ),
        None => bar.child(el().grow(1.0)),
    };
    bar.child(text(totals).font(k.ty.text_small).color(k.c.text_muted))
}


/// Gives a kit component a key, for tests, scripts and anchoring menus.
fn keyed(key: &'static str, e: El) -> El {
    el().key(key).child(e)
}
