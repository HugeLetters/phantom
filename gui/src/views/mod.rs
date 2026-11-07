//! The window's contents: what's shown and which actions exist, drawn through `ui`. Handlers
//! reach the service through `s.service`; the model helpers they call take only the pieces they
//! use.

mod empty;
mod status_bar;
mod table;
mod toolbar;

use client::format;
use rune::{Cx, El};

use crate::model::{Phantom, add_files};
use crate::service::AddRequest;
use crate::ui::Ui;

pub fn view(s: &Phantom, cx: &mut Cx<Phantom>) -> El {
    let ui = Ui::new(cx.system_dark());
    let page = ui
        .page(cx.titlebar_inset())
        .key("app")
        .gap(10.0)
        .child(toolbar::toolbar(s, &ui))
        .child(
            ui.panel()
                .key("list")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(if s.row_count() == 0 { empty::empty(&ui) } else { table::torrent_table(s, &ui) }),
        )
        .child(status_bar::status_bar(s, &ui));
    // On the root, where Rune routes window-level pastes and drops: drop .torrent files
    // anywhere, or paste a magnet link with no field focused.
    ui.root(page)
        .on_drop_files(|s: &mut Phantom, files, _| add_files(&mut *s.service, files))
        .accepts_extensions(&["torrent"])
        .on_paste(|s: &mut Phantom, pasted, _| s.service.add(AddRequest::Url(pasted.trim().to_owned())))
}

fn speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 { "–".into() } else { format!("{}/s", format::bytes(bytes_per_sec)) }
}
