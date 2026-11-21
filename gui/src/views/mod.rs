//! The window's contents: what's shown and which actions exist, drawn through `ui`. Handlers
//! reach the service through `s.service`; the model helpers they call take only the pieces they
//! use.

mod clash;
mod columns_menu;
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
use rune::{Cx, Drag, El, Phase, Tx, col, row};

pub(crate) use details::{Shape as PaneShape, details_pane};

use crate::model::layout::{self, Column};
use crate::model::{AddFailure, Phantom, add, add_files, shown_torrent};
use crate::service::AddRequest;
use crate::theme::preset::Appearance;
use crate::ui::Ui;

/// The window, in the look of the system's Appearance: the toolbar in the titlebar band, then the
/// list, the divider and the details pane, which share the room (#33), then the status bar.
pub fn view(s: &Phantom, cx: &mut Cx<Phantom>) -> El {
    // What the views built below offer, registered as they're built (#33).
    s.mounted.begin();
    let look = s.looks.borrow().shown(Appearance::of(cx.system_dark())).clone();
    let ui = &look.ui;
    let mut overlays = Vec::new();
    // The room the list and the pane share, as last laid out; the window's default size before
    // the first layout.
    let [width, room] = cx.layout_size("body").map_or([936.0, 420.0], |[w, h]| [w - 2.0 * MARGIN, h]);
    let fitted = table::fitted(s, ui, width);
    let shown: Vec<Column> = fitted.iter().map(|(c, _)| *c).collect();
    let list = ui.panel().key("list").w_full().grow(1.0).min_h(0.0).on_layout_tx(keep_selection_in_view).child(if s.torrents.is_empty() {
        empty::empty(ui)
    } else {
        table::torrent_table(s, ui, &fitted, width)
    });
    let mut body = col().key("body").w_full().grow(1.0).min_h(0.0).px(MARGIN).child(list);
    if let Some(torrent) = shown_torrent(&s.table, &s.torrents, &s.pane) {
        let files = torrent.content.map(|_| s.pane.details.as_ref().filter(|d| d.id == torrent.id).map_or(1, |d| d.files.len()));
        let pane = layout::pane(room, details::content(torrent, files), s.prefs.dragged(), s.torrents.len());
        let shape = PaneShape { header: pane.header, width };
        body = body.child(divider(ui, room, pane.height)).child((look.screens.details)(s, ui, torrent, shape).h(pane.height));
        // Under the rename field, while what was typed in it isn't a name, until the next keystroke.
        if let Some(why) = s.rename.note().filter(|_| s.rename.editing(torrent.id)) {
            ui.tooltip_under("rename-note", details::RENAME_FIELD, &client::display::name_problem(why), &mut overlays);
        }
    }
    let inset = cx.titlebar_inset();
    // The band's room, as last laid out (the default window's before the first layout).
    let band = toolbar::Band {
        width: cx.layout_size("app").map_or(960.0, |[w, _]| w),
        inset,
        add: cx.layout_size("add-focus").map_or(50.0, |[w, _]| w),
        add_file: cx.layout_size("add-file").map_or(84.0, |[w, _]| w),
    };
    let toolbar = toolbar::toolbar(s, ui, band, &mut overlays);
    let page = ui.page().key("app").child(toolbar);
    let mut page = page.child(body);
    // The room above the status bar for its popover, from where it was last drawn: under the
    // toolbar, and under the message by Add while it's shown, which the popover never covers.
    let under = |key: &str| cx.layout(key).map(|at| at[1] + at[3]);
    let top = under("toolbar").into_iter().chain(under("add-note-content")).fold(inset, f32::max) + 8.0;
    let room = cx.layout("totals").map_or(400.0, |totals| totals[1] - top - 4.0);
    page = page.child(row().w_full().px(MARGIN).pt(8.0).pb(10.0).child(status_bar::status_bar(s, ui, room, &mut overlays)));
    // Last: a menu is modal, and takes what's drawn after it into its own focus.
    let window = cx.layout("app").map(|at| (at[2], at[3]));
    row_menu::overlay(s, ui, window, |key| cx.layout(key), &mut overlays);
    file_menu::overlay(s, ui, window, &mut overlays);
    columns_menu::overlay(s, ui, window, &shown, &mut overlays);
    (look.screens.palette)(s, ui, window, &mut overlays);
    clash::overlay(s, ui, window, &mut overlays);
    // On the root, where Rune routes window-level pastes and drops: drop .torrent files
    // anywhere, or paste a magnet link with no field focused.
    ui.root(page, overlays).on_drop_files(|s: &mut Phantom, files, _| add_files(s, files)).accepts_extensions(&["torrent"]).on_paste(
        |s: &mut Phantom, pasted, _| match AddRequest::link(&pasted) {
            Ok(request) => add(s, vec![request], Vec::new()),
            Err(problem) => add(s, Vec::new(), vec![AddFailure::not_a_link(&pasted, problem)]),
        },
    )
}

/// The window's margin, either side of the list, the pane and the status bar.
const MARGIN: f32 = 12.0;

/// The divider between the list and the pane, `height` tall now in `room`: a drag moves it, the
/// pane never under a one-line header over one file nor into the list's header and three rows; let
/// go, its height is kept (`gui.json`).
fn divider(ui: &Ui, room: f32, height: f32) -> El {
    let clamp = move |h: f32| h.clamp(layout::PANE_LEAST, layout::pane_most(room).max(layout::PANE_LEAST));
    ui.divider(layout::DIVIDER).key("divider").on_drag(move |s: &mut Phantom, d: &mut Drag, _: &mut Tx| match d.phase {
        Phase::Down => d.data = height,
        Phase::Move => s.prefs.dragging = Some(clamp(d.data - d.dy)),
        Phase::Up if d.cancelled() => s.prefs.dragging = None,
        Phase::Up => s.prefs.set_pane_height(clamp(d.data - d.dy)),
    })
}

/// The list's height changed (the pane opened or was dragged, the window was resized, or it's the
/// first layout): the selected row is scrolled into view, if it isn't.
fn keep_selection_in_view(s: &mut Phantom, at: [f32; 4], tx: &mut Tx) {
    let height = at[3];
    if (height - s.list_height).abs() < 0.5 {
        tx.live_only();
        return;
    }
    s.list_height = height;
    if let Some(id) = s.table.cursor().filter(|id| s.table.is_selected(id)) {
        tx.scroll_into_view_in("table", id.to_owned());
    }
}

/// Why the torrent's status needs the user, if it says: "Drive MyDisk isn't connected", or the
/// error. The list shows it on hover, the details pane in its header.
fn status_reason(t: &TorrentInfo) -> Option<String> {
    client::display::status_reason(t.display_name(), &t.status)
}

fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 { "–".into() } else { format!("{}/s", display::bytes(bytes_per_sec)) }
}
