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

/// The saved bits without the pieces in `ranges` (each `[start, end)`): those past the end are none.
pub fn cleared(mut saved: Vec<u8>, ranges: impl IntoIterator<Item = (u32, u32)>) -> Vec<u8> {
    for (start, end) in ranges {
        for piece in start..end {
            if let Some(byte) = saved.get_mut(piece as usize / 8) {
                *byte &= !(0x80 >> (piece % 8));
            }
        }
    }
    saved
}

/// What `saved` says librqbit has of each file of `lens` bytes, laid end to end in pieces of
/// `piece_length`: the bytes of each piece it has that fall in the file, as librqbit counts them.
pub fn file_progress(saved: &[u8], lens: &[u64], piece_length: u32) -> Vec<u64> {
    let piece_length = u64::from(piece_length.max(1));
    let has = |piece: u64| saved.get((piece / 8) as usize).is_some_and(|byte| byte & (0x80 >> (piece % 8)) != 0);
    let mut start = 0;
    lens.iter()
        .map(|&len| {
            let end = start + len;
            let pieces = (start / piece_length)..end.div_ceil(piece_length);
            let had = pieces.filter(|&p| has(p)).map(|p| end.min((p + 1) * piece_length) - start.max(p * piece_length)).sum();
            start = end;
            had
        })
        .collect()
}

/// Clears the pieces in `ranges` in the file at `path`, librqbit's or one kept aside, through a
/// temporary file and a rename: a crash leaves it before or after, never half. None saved: nothing.
pub fn clear(path: &Path, ranges: impl IntoIterator<Item = (u32, u32)>) -> std::io::Result<()> {
    let saved = match std::fs::read(path) {
        Ok(saved) => saved,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let tmp = path.with_extension("bitv.tmp");
    std::fs::write(&tmp, cleared(saved, ranges))?;
    std::fs::rename(tmp, path)
}

/// Drops what's saved at `path`, so librqbit checks the torrent in full when it's added. None
/// saved: nothing.
pub fn forget(path: &Path) {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => warn!(?path, error = %e, "failed to drop the pieces librqbit saved"),
        _ => {}
    }
}

/// How many pieces a torrent of `piece_length`-byte pieces holds, at least, with `progress_bytes`
/// of it had: every piece but the last is full length.
pub fn pieces_in(progress_bytes: u64, piece_length: u32) -> u64 {
    progress_bytes.div_ceil(u64::from(piece_length.max(1)))
}

/// Waits until each torrent's file (by info hash) holds at least the given number of pieces,
/// up to two seconds, then logs those that don't and goes on.
pub async fn wait_for_saves(session_folder: &Path, wanted: &[(String, u64)]) {
    let wanted: Vec<(PathBuf, u64)> = wanted.iter().map(|(hash, n)| (path(session_folder, hash), *n)).collect();
    wait_for(&wanted).await;
}

/// Waits until each file holds at least the given number of pieces, up to two seconds, then logs
/// those that don't and goes on: librqbit's save as it drops a torrent, into a file of its or a
/// link to one (an undo's stash).
pub async fn wait_for(wanted: &[(PathBuf, u64)]) {
    let wanted: Vec<_> = wanted.iter().filter(|(_, pieces)| *pieces > 0).collect();
    let saved = |file: &Path| std::fs::read(file).map(|bits| pieces(&bits)).unwrap_or(0);
    let behind = || wanted.iter().filter(|(file, pieces)| saved(file) < *pieces).collect::<Vec<_>>();
    let mut waited = Duration::ZERO;
    while !behind().is_empty() && waited < GIVE_UP_AFTER {
        tokio::time::sleep(POLL_EVERY).await;
        waited += POLL_EVERY;
    }
    for (file, pieces) in behind() {
        warn!(file = %file.display(), wanted = pieces, saved = saved(file), "librqbit's save of the pieces didn't land in time");
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
    fn clears_only_the_pieces_asked_and_those_past_the_end_are_none() {
        assert_eq!(cleared(vec![0xff, 0xff], [(1, 3), (9, 10), (20, 30)]), vec![0b1001_1111, 0b1011_1111]);
        assert_eq!(cleared(vec![0xff], []), vec![0xff]);
    }

    #[test]
    fn clearing_a_file_keeps_the_rest_and_a_missing_one_is_left_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.bitv");
        clear(&file, [(0, 8)]).unwrap();
        assert!(!file.exists());
        std::fs::write(&file, [0xff, 0x0f]).unwrap();
        clear(&file, [(4, 12)]).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), [0xf0, 0x0f]);
    }

    #[test]
    fn a_files_progress_is_what_the_pieces_it_has_hold_of_it() {
        // Pieces of 10 bytes; files of 15, 5 and 12 bytes: piece 1 is shared by the first two, piece
        // 3 is the short last one.
        let lens = [15, 5, 12];
        assert_eq!(file_progress(&[0b1111_0000], &lens, 10), vec![15, 5, 12]);
        assert_eq!(file_progress(&[0b1000_0000], &lens, 10), vec![10, 0, 0]);
        assert_eq!(file_progress(&[0b0100_0000], &lens, 10), vec![5, 5, 0]);
        assert_eq!(file_progress(&[0b0001_0000], &lens, 10), vec![0, 0, 2]);
        assert_eq!(file_progress(&[], &lens, 10), vec![0, 0, 0]);
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
