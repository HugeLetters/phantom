//! Human-readable formatting shared by clients.

use std::time::Duration;

use proto::{TorrentInfo, TorrentState};

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, s / 60 % 60),
    }
}

pub fn status(t: &TorrentInfo) -> &'static str {
    match (t.state, t.finished) {
        (TorrentState::Live, true) => "seeding",
        (TorrentState::Live, false) => "downloading",
        (TorrentState::Initializing, _) => "checking",
        (TorrentState::Paused, _) => "paused",
        (TorrentState::Error, _) => "error",
    }
}
