//! librqbit's `.bitv` files: the pieces a torrent has, saved in the session folder as
//! `<info hash>.bitv`, one bit per piece, most significant bit first. The only code that knows
//! that layout.
//!
//! librqbit writes the file every 16 MiB, on completion and when it drops the torrent, and that
//! last write races the end of the session ([rqbit#735]): [`wait_for_saves`] waits for it.
//!
//! [rqbit#735]: https://github.com/ikatson/rqbit/issues/735

use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::warn;

const POLL_EVERY: Duration = Duration::from_millis(20);
const GIVE_UP_AFTER: Duration = Duration::from_secs(2);

/// Where librqbit keeps the pieces of the torrent with this info hash.
pub fn path(session_folder: &Path, info_hash: &str) -> PathBuf {
    session_folder.join(format!("{info_hash}.bitv"))
}

/// How many pieces the saved bits hold.
pub fn pieces(saved: &[u8]) -> u64 {
    saved.iter().map(|byte| u64::from(byte.count_ones())).sum()
}

/// How many pieces a torrent of `piece_length`-byte pieces holds, at least, with `progress_bytes`
/// of it had: every piece but the last is full length.
pub fn pieces_in(progress_bytes: u64, piece_length: u32) -> u64 {
    progress_bytes.div_ceil(u64::from(piece_length.max(1)))
}

/// Waits until each torrent's file (by info hash) holds at least the given number of pieces,
/// up to two seconds, then logs those that don't and goes on.
pub async fn wait_for_saves(session_folder: &Path, wanted: &[(String, u64)]) {
    let wanted: Vec<_> = wanted.iter().filter(|(_, pieces)| *pieces > 0).collect();
    let saved = |hash: &str| std::fs::read(path(session_folder, hash)).map(|bits| pieces(&bits)).unwrap_or(0);
    let behind = || wanted.iter().filter(|(hash, pieces)| saved(hash) < *pieces).collect::<Vec<_>>();
    let mut waited = Duration::ZERO;
    while !behind().is_empty() && waited < GIVE_UP_AFTER {
        tokio::time::sleep(POLL_EVERY).await;
        waited += POLL_EVERY;
    }
    for (hash, pieces) in behind() {
        warn!(%hash, wanted = pieces, saved = saved(hash), "librqbit's save of the pieces didn't land in time");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_set_bits() {
        assert_eq!(pieces(&[0b1010_0000, 0xff, 0]), 10);
    }

    #[test]
    fn rounds_progress_up_to_whole_pieces() {
        assert_eq!(pieces_in(0, 16), 0);
        assert_eq!(pieces_in(33, 16), 3);
        assert_eq!(pieces_in(32, 16), 2);
    }

    #[tokio::test]
    async fn waits_for_the_file_then_returns() {
        let dir = std::env::temp_dir().join(format!("phantom-bitv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = path(&dir, "abc");
        let later = file.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            std::fs::write(later, [0b1100_0000]).unwrap();
        });
        let started = std::time::Instant::now();
        wait_for_saves(&dir, &[("abc".into(), 2)]).await;
        assert!(started.elapsed() >= Duration::from_millis(100) && started.elapsed() < GIVE_UP_AFTER);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
