//! The status bar: the connection to the service, the latest notice (click to dismiss), and the
//! totals.

use rune::{El, el, row};

use super::speed;
use crate::model::Phantom;
use crate::service::Connection;
use crate::ui::{Tone, Ui};

pub fn status_bar(s: &Phantom, ui: &Ui) -> El {
    let (tone, connection) = match &s.connection {
        Connection::Connecting => (Tone::Neutral, "Starting service…".to_owned()),
        Connection::Connected => (Tone::Good, "Connected".to_owned()),
        Connection::Lost(e) => (Tone::Bad, format!("Service unavailable: {e}")),
    };
    let down: u64 = s.torrents.iter().map(|t| t.download_bytes_per_sec).sum();
    let up: u64 = s.torrents.iter().map(|t| t.upload_bytes_per_sec).sum();
    let mut totals = match s.row_count() {
        0 => String::new(),
        1 => format!("1 torrent · ↓ {} · ↑ {}", speed(down), speed(up)),
        n => format!("{n} torrents · ↓ {} · ↑ {}", speed(down), speed(up)),
    };
    if s.service.in_flight() > 0 {
        totals = if totals.is_empty() { "Working…".into() } else { format!("Working… · {totals}") };
    }

    let mut bar = row()
        .key("status")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(ui.status_dot(tone))
        .child(ui.caption(connection));
    bar = match &s.notice {
        Some(n) => bar.child(
            ui.notice(n.text.clone(), n.error)
                .key("notice")
                .grow(1.0)
                .min_w(0.0)
                .help("Click to dismiss")
                .on_press(|s: &mut Phantom| s.notice = None),
        ),
        None => bar.child(el().grow(1.0)),
    };
    bar.child(ui.caption(totals))
}
