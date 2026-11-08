//! Everything clients show people: sizes and durations, and the words for the service's typed
//! reasons (refusals, why a destination is unavailable). Shared by the CLI and the app; the
//! service only states the facts. A status's label is `proto::Status::label`.

use std::time::Duration;

use proto::{Action, Refusal, Unavailable};

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

/// Why a destination can't be reached, as a sentence: "Drive MyDisk isn't connected".
pub fn unavailable(reason: &Unavailable) -> String {
    match reason {
        Unavailable::DriveNotConnected { name } => format!("Drive {name} isn't connected"),
        Unavailable::FolderMissing { path } => format!("Folder {} doesn't exist", path.display()),
        Unavailable::NoPermission { path } => format!("No permission to open folder {}", path.display()),
        Unavailable::NotAFolder { path } => format!("{} isn't a folder", path.display()),
        Unavailable::CantOpen { path } => format!("Can't open folder {}", path.display()),
    }
}

pub fn refusal(refusal: &Refusal) -> String {
    match refusal {
        // Each client adds its own fix: the CLI names its options, the app asks.
        Refusal::NoDestination => "no destination".into(),
        Refusal::Destination { reason } => {
            format!("can't add the torrent: its destination is unavailable. {}", unavailable(reason))
        }
        Refusal::DefaultDestination { reason } => {
            format!("can't use it as the default destination. {}", unavailable(reason))
        }
        Refusal::TorrentDestinationUnavailable { id, action, reason } => {
            let what = match action {
                Action::Pause => "pause",
                Action::Resume => "start",
                Action::Check => "be checked",
                Action::SetPriority => "change its priorities",
                Action::Remove => "be removed",
                Action::Delete => "delete its files",
            };
            format!("torrent {id} can't {what}: its destination is unavailable. {}", unavailable(reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use proto::AbsolutePath;

    use super::*;

    #[test]
    fn reasons_and_refusals_read_as_sentences() {
        let drive = Unavailable::DriveNotConnected { name: "MyDisk".into() };
        assert_eq!(unavailable(&drive), "Drive MyDisk isn't connected");
        assert_eq!(unavailable(&Unavailable::FolderMissing { path: AbsolutePath::new("/Users/me/Films").unwrap() }), "Folder /Users/me/Films doesn't exist");
        let start = Refusal::TorrentDestinationUnavailable { id: 1, action: Action::Resume, reason: drive.clone() };
        assert_eq!(refusal(&start), "torrent 1 can't start: its destination is unavailable. Drive MyDisk isn't connected");
        let default = Refusal::DefaultDestination { reason: drive };
        assert_eq!(refusal(&default), "can't use it as the default destination. Drive MyDisk isn't connected");
    }
}
