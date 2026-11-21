//! The window's contents: what's shown and which actions exist, drawn through `ui`. Handlers
//! reach the service through `s.service`; the model helpers they call take only the pieces they
//! use.

mod clash;
mod details;
mod empty;
pub(crate) mod file_menu;
pub(crate) mod palette;
mod row_menu;
pub mod settings;
mod space;
pub mod status_bar;
mod table;
pub(crate) mod toolbar;

use client::display;
use proto::TorrentInfo;
use rune::{Cx, El};

pub(crate) use details::details_pane;

use crate::model::{AddFailure, Phantom, add, add_files, shown_torrent};
use crate::service::AddRequest;
use crate::theme::preset::Appearance;

/// The window, in the look of the system's Appearance.
pub fn view(s: &Phantom, cx: &mut Cx<Phantom>) -> El {
    // What the views built below offer, registered as they're built (#33).
    s.mounted.begin();
    let look = s.looks.borrow().shown(Appearance::of(cx.system_dark())).clone();
    let ui = &look.ui;
    let mut overlays = Vec::new();
    let mut page = ui.page(cx.titlebar_inset()).key("app").gap(10.0).child(toolbar::toolbar(s, ui, &mut overlays)).child(
        ui.panel().key("list").w_full().grow(1.0).min_h(0.0).child(if s.torrents.is_empty() {
            empty::empty(ui)
        } else {
            table::torrent_table(s, ui, &mut overlays)
        }),
    );
    if let Some(torrent) = shown_torrent(&s.table, &s.torrents, &s.pane) {
        // A little more than the list, so a file shows under the header at the smallest window.
        page = page.child((look.screens.details)(s, ui, torrent).grow(1.25).min_h(0.0));
    }
    // The room above the status bar for its popover, from where it was last drawn: under the
    // toolbar, and under the message by Add while it's shown, which the popover never covers.
    let under = |key: &str| cx.layout(key).map(|at| at[1] + at[3]);
    let top = under("toolbar").into_iter().chain(under("add-note-content")).fold(cx.titlebar_inset(), f32::max) + 8.0;
    let room = cx.layout("totals").map_or(400.0, |totals| totals[1] - top - 4.0);
    let page = page.child(status_bar::status_bar(s, ui, room, &mut overlays));
    // Last: a menu is modal, and takes what's drawn after it into its own focus.
    let window = cx.layout("app").map(|at| (at[2], at[3]));
    row_menu::overlay(s, ui, window, &mut overlays);
    file_menu::overlay(s, ui, window, &mut overlays);
    (look.screens.palette)(s, ui, window, &mut overlays);
    clash::overlay(s, ui, &mut overlays);
    // On the root, where Rune routes window-level pastes and drops: drop .torrent files
    // anywhere, or paste a magnet link with no field focused.
    ui.root(page, overlays).on_drop_files(|s: &mut Phantom, files, _| add_files(s, files)).accepts_extensions(&["torrent"]).on_paste(
        |s: &mut Phantom, pasted, _| match AddRequest::link(&pasted) {
            Ok(request) => add(s, vec![request], Vec::new()),
            Err(problem) => add(s, Vec::new(), vec![AddFailure::not_a_link(&pasted, problem)]),
        },
    )
}

/// Why the torrent's status needs the user, if it says: "Drive MyDisk isn't connected", or the
/// error. The list shows it on hover, the details pane in its header.
fn status_reason(t: &TorrentInfo) -> Option<String> {
    client::display::status_reason(t.display_name(), &t.status)
}

fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 { "–".into() } else { format!("{}/s", display::bytes(bytes_per_sec)) }
}
