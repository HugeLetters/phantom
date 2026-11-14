//! The window's contents: what's shown and which actions exist, drawn through `ui`. Handlers
//! reach the service through `s.service`; the model helpers they call take only the pieces they
//! use.

mod details;
mod empty;
mod errors;
mod row_menu;
pub mod settings;
pub mod status_bar;
mod table;
pub(crate) mod toolbar;

use client::display;
use proto::TorrentInfo;
use rune::{Cx, El};

use crate::model::{Phantom, add, add_files, shown_torrent};
use crate::service::AddRequest;
use crate::ui::Ui;

pub fn view(s: &Phantom, cx: &mut Cx<Phantom>) -> El {
    let ui = Ui::new(cx.system_dark());
    let mut overlays = Vec::new();
    let mut page = ui.page(cx.titlebar_inset()).key("app").gap(10.0).child(toolbar::toolbar(s, &ui, &mut overlays)).child(
        ui.panel().key("list").w_full().grow(1.0).min_h(0.0).child(if s.torrents.is_empty() {
            empty::empty(&ui)
        } else {
            table::torrent_table(s, &ui)
        }),
    );
    if let Some(torrent) = shown_torrent(&s.table, &s.torrents, &s.pane) {
        // A little more than the list, so a file shows under the header at the smallest window.
        page = page.child(details::details_pane(s, &ui, torrent).grow(1.25).min_h(0.0));
    }
    let page = page.child(status_bar::status_bar(s, &ui));
    if s.errors.is_open() {
        overlays.push(errors::error_layer(s, &ui, cx.titlebar_inset()));
    }
    if let Some(sheet) = &s.settings {
        settings::settings(s, sheet, &ui, &mut overlays);
    }
    // Last: a menu is modal, and takes what's drawn after it into its own focus.
    row_menu::overlay(s, &ui, &mut overlays);
    // On the root, where Rune routes window-level pastes and drops: drop .torrent files
    // anywhere, or paste a magnet link with no field focused.
    ui.root(page, overlays)
        .on_drop_files(|s: &mut Phantom, files, _| add_files(s, files))
        .accepts_extensions(&["torrent"])
        .on_paste(|s: &mut Phantom, pasted, _| add(s, vec![AddRequest::Url(pasted.trim().to_owned())], Vec::new()))
}

/// Why the torrent's status needs the user, if it says: "Drive MyDisk isn't connected", or the
/// error. The list shows it on hover, the details pane in its header.
fn status_reason(t: &TorrentInfo) -> Option<String> {
    client::display::status_reason(&t.status)
}

fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 { "–".into() } else { format!("{}/s", display::bytes(bytes_per_sec)) }
}
