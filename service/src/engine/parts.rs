//! Kept-aside parts (#374): what librqbit writes into a file Phantom doesn't let it write (one set
//! Off, one not granted yet) goes into a log of that file's in the data dir, and is read
//! over the file, until it's written into it.
//!
//! ```text
//! <data_dir>/parts/<info_hash>/<file index>     one append-only log per file
//!   "PHPARTS1"                                  written as the log is made
//!   record* = offset: u64 LE | len: u32 LE | bytes[len]     (offset in the torrent's file)
//! ```
//!
//! - A [kept](Parts::keep) write returns only once its record is synced: librqbit counts a piece (and
//!   saves it in `.bitv`) after the write returns, so a counted piece always has its bytes behind it.
//!   One that fails (a full disk) is cut back to the last whole record and fails the write: the torrent
//!   goes to Error, as for any failed write.
//! - In memory each log is only an index of its records (a later one wins), never the bytes.
//! - A log is loaded at its first use: a torn last record is cut off (it was never acknowledged); one
//!   that can't be read is [damaged](damaged), and dealt with at a start, before any torrent is added.
//! - [Written in](Parts::write_into): every range into the file, the file synced, then the log
//!   deleted and its folder synced. A crash in between writes the same bytes again next time.
//!
//! A leaf lock: taken under a file's slot, nothing is taken under it.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const MAGIC: &[u8; 8] = b"PHPARTS1";
/// A record's head: its offset and length.
const HEAD: u64 = 12;
const PARTS: &str = "parts";

/// Where every torrent's logs are: `parts/<info_hash>/<file index>`.
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PARTS)
}

/// A torrent's kept-aside parts, by file.
#[derive(Default)]
pub(super) struct Parts {
    /// `parts/<info_hash>`; `None` keeps nothing (a write aside fails).
    dir: Option<PathBuf>,
    logs: Mutex<Logs>,
    /// Where a test stops [`Parts::write_into`], as a crash would.
    #[cfg(test)]
    pub stop: Option<fn(Step)>,
}

#[derive(Default)]
struct Logs {
    loaded: bool,
    by_file: BTreeMap<usize, Log>,
}

struct Log {
    file: File,
    /// Where the next record goes: the end of the last whole one.
    end: u64,
    /// The bytes it holds of the torrent's file: each range's start, its end and where in the log
    /// its bytes start.
    index: BTreeMap<u64, (u64, u64)>,
}

impl Parts {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir, ..Self::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Logs> {
        let mut logs = self.logs.lock().unwrap_or_else(PoisonError::into_inner);
        if !logs.loaded {
            logs.loaded = true;
            if let Some(dir) = &self.dir {
                logs.by_file = load(dir);
            }
        }
        logs
    }

    /// Keeps `bufs`, one after another, written at `offset` into file `index`, synced before it
    /// returns. A log of more than `cap` bytes of records refuses it: nothing writes a file aside
    /// that much but a loop.
    pub fn keep(&self, index: usize, offset: u64, bufs: &[&[u8]], cap: u64) -> std::io::Result<()> {
        let dir = self.dir.as_ref().ok_or_else(|| std::io::Error::other("nowhere to keep it"))?;
        let mut logs = self.lock();
        let log = match logs.by_file.entry(index) {
            std::collections::btree_map::Entry::Occupied(log) => log.into_mut(),
            std::collections::btree_map::Entry::Vacant(at) => at.insert(Log::make(dir, index)?),
        };
        let len: u64 = bufs.iter().map(|b| b.len() as u64).sum();
        if log.end + HEAD + len > MAGIC.len() as u64 + cap {
            return Err(std::io::Error::other(format!("file {index}'s kept parts would pass {cap} bytes")));
        }
        let mut record = Vec::with_capacity((HEAD + len) as usize);
        record.extend_from_slice(&offset.to_le_bytes());
        record.extend_from_slice(&u32::try_from(len).map_err(std::io::Error::other)?.to_le_bytes());
        bufs.iter().for_each(|b| record.extend_from_slice(b));
        if let Err(e) = log.file.write_all_at(&record, log.end).and_then(|()| log.file.sync_all()) {
            // Back to the last whole record: what failed was never acknowledged.
            let _ = log.file.set_len(log.end);
            return Err(e);
        }
        log.add(offset, offset + len, log.end + HEAD);
        log.end += HEAD + len;
        Ok(())
    }

    /// What file `index`'s log holds of the bytes from `offset` goes into `buf`, over what's there, the
    /// first `read` of them read already from the file: whether `buf` is whole.
    pub fn read_over(&self, index: usize, offset: u64, buf: &mut [u8], read: usize) -> std::io::Result<bool> {
        let end = offset + buf.len() as u64;
        let mut covered = vec![(offset, offset + read as u64)];
        let logs = self.lock();
        if let Some(log) = logs.by_file.get(&index) {
            for (from, to, at) in log.over(offset, end) {
                log.file.read_exact_at(&mut buf[(from - offset) as usize..(to - offset) as usize], at)?;
                covered.push((from, to));
            }
        }
        covered.sort();
        let mut at = offset;
        for (from, to) in covered {
            if from > at {
                return Ok(false);
            }
            at = at.max(to);
        }
        Ok(at >= end)
    }

    /// Whether anything of file `index` is kept.
    pub fn has(&self, index: usize) -> bool {
        self.lock().by_file.contains_key(&index)
    }

    /// Writes what's kept of file `index` into `file`, synced, then deletes its log: whether anything
    /// went in. A failure leaves the log, to be written in again.
    pub fn write_into(&self, index: usize, file: &File) -> std::io::Result<bool> {
        #[cfg(test)]
        let at = |step: Step| {
            if let Some(stop) = self.stop {
                stop(step);
            }
        };
        let mut logs = self.lock();
        let Some(log) = logs.by_file.get(&index) else { return Ok(false) };
        let mut bytes = Vec::new();
        for (&from, &(to, at)) in &log.index {
            bytes.resize((to - from) as usize, 0);
            log.file.read_exact_at(&mut bytes, at)?;
            file.write_all_at(&bytes, from)?;
        }
        #[cfg(test)]
        at(Step::Written);
        file.sync_all()?;
        #[cfg(test)]
        at(Step::Synced);
        logs.by_file.remove(&index);
        self.delete(index);
        Ok(true)
    }

    /// Deletes what's kept of file `index`: its pieces are let go of.
    pub fn forget(&self, index: usize) {
        if self.lock().by_file.remove(&index).is_some() {
            self.delete(index);
        }
    }

    /// Deletes everything kept of the torrent: it's gone.
    pub fn forget_all(&self) {
        let mut logs = self.lock();
        let indices: Vec<usize> = std::mem::take(&mut logs.by_file).into_keys().collect();
        drop(logs);
        indices.into_iter().for_each(|index| self.delete(index));
    }

    fn delete(&self, index: usize) {
        let Some(dir) = &self.dir else { return };
        let _ = std::fs::remove_file(dir.join(index.to_string()));
        if let Ok(dir) = File::open(dir) {
            let _ = dir.sync_all();
        }
        let _ = std::fs::remove_dir(dir);
    }
}

/// A step of [`Parts::write_into`], where a test stops it, as a crash would.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Written,
    Synced,
}

impl Log {
    /// A new log of file `index` in `dir`.
    fn make(dir: &Path, index: usize) -> std::io::Result<Self> {
        if let Some(root) = dir.parent() {
            std::fs::create_dir_all(root)?;
        }
        match std::fs::create_dir(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
            _ => {}
        }
        let mut file = File::options().read(true).write(true).create(true).truncate(true).open(dir.join(index.to_string()))?;
        file.write_all(MAGIC)?;
        file.sync_all()?;
        File::open(dir)?.sync_all()?;
        Ok(Self { file, end: MAGIC.len() as u64, index: BTreeMap::new() })
    }

    /// Records that `[from, to)` of the torrent's file is at `at` in the log, over what was before.
    fn add(&mut self, from: u64, to: u64, at: u64) {
        if from >= to {
            return;
        }
        let overlapping: Vec<u64> = self.index.range(..to).filter(|(_, (end, _))| *end > from).map(|(s, _)| *s).collect();
        for start in overlapping {
            let (end, pos) = self.index.remove(&start).expect("just found");
            if start < from {
                self.index.insert(start, (from, pos));
            }
            if end > to {
                self.index.insert(to, (end, pos + (to - start)));
            }
        }
        self.index.insert(from, (to, at));
    }

    /// The ranges it holds within `[from, to)`: each one's start, end and place in the log.
    fn over(&self, from: u64, to: u64) -> impl Iterator<Item = (u64, u64, u64)> + '_ {
        self.index.range(..to).filter(move |(_, (end, _))| *end > from).map(move |(&start, &(end, at))| {
            let (a, b) = (start.max(from), end.min(to));
            (a, b, at + (a - start))
        })
    }
}

/// The logs in `dir`, read: a torn last record cut off. One that can't be read is left out (and
/// deleted at the next start, [`damaged`]).
fn load(dir: &Path) -> BTreeMap<usize, Log> {
    let mut logs = BTreeMap::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(index) = entry.file_name().to_str().and_then(|n| n.parse::<usize>().ok()) else { continue };
        if let Ok(Some(log)) = read(&entry.path()) {
            logs.insert(index, log);
        }
    }
    logs
}

/// The log at `path`, its torn tail cut off; `None`: damaged.
fn read(path: &Path) -> std::io::Result<Option<Log>> {
    let mut file = File::options().read(true).write(true).open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Ok(None);
    }
    let mut log = Log { file, end: MAGIC.len() as u64, index: BTreeMap::new() };
    loop {
        let at = log.end as usize;
        let Some(head) = bytes.get(at..at + HEAD as usize) else { break };
        let offset = u64::from_le_bytes(head[..8].try_into().expect("8 bytes"));
        let len = u32::from_le_bytes(head[8..].try_into().expect("4 bytes")) as u64;
        if at as u64 + HEAD + len > bytes.len() as u64 {
            break;
        }
        log.add(offset, offset + len, log.end + HEAD);
        log.end += HEAD + len;
    }
    if log.end < bytes.len() as u64 {
        log.file.set_len(log.end)?;
        log.file.sync_all()?;
    }
    Ok(Some(log))
}

/// At a start, before any torrent is added: the logs under `root` that can't be read (no magic,
/// unreadable), by torrent and file, deleted. Their pieces are counted with nothing behind them, so
/// the caller clears them.
pub fn damaged(root: &Path) -> Vec<(String, usize)> {
    let mut damaged = Vec::new();
    for torrent in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let hash = torrent.file_name().to_string_lossy().into_owned();
        for entry in std::fs::read_dir(torrent.path()).into_iter().flatten().flatten() {
            let index = entry.file_name().to_str().and_then(|n| n.parse::<usize>().ok());
            let readable = index.is_some() && matches!(read(&entry.path()), Ok(Some(_)));
            if !readable {
                tracing::warn!(hash, file = ?entry.file_name(), "a log of kept parts can't be read: deleted, its pieces cleared");
                let _ = std::fs::remove_file(entry.path());
                damaged.extend(index.map(|i| (hash.clone(), i)));
            }
        }
        let _ = std::fs::remove_dir(torrent.path());
    }
    damaged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(dir: &Path) -> Parts {
        Parts::new(Some(dir.join("parts/t")))
    }

    fn read(parts: &Parts, offset: u64, len: usize) -> (bool, Vec<u8>) {
        let mut buf = vec![0; len];
        let whole = parts.read_over(0, offset, &mut buf, 0).unwrap();
        (whole, buf)
    }

    /// A later write wins over an earlier one where they overlap, and what's kept outlasts a restart
    /// (a new `Parts` of the same folder).
    #[test]
    fn what_is_kept_reads_back_the_latest_first_and_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let first = parts(dir.path());
        first.keep(0, 2, &[&[1; 6]], 100).unwrap();
        first.keep(0, 4, &[&[2; 2], &[3; 4]], 100).unwrap();
        for p in [&first, &parts(dir.path())] {
            assert_eq!(read(p, 2, 8), (true, vec![1, 1, 2, 2, 3, 3, 3, 3]));
            assert_eq!(read(p, 0, 4), (false, vec![0, 0, 1, 1]), "a hole at 0");
        }
    }

    /// A record cut short (a crash as it was written) is cut off; what came before stays.
    #[test]
    fn a_torn_last_record_is_cut_off() {
        let dir = tempfile::tempdir().unwrap();
        let p = parts(dir.path());
        p.keep(0, 0, &[&[1; 4]], 100).unwrap();
        let path = dir.path().join("parts/t/0");
        let whole = std::fs::metadata(&path).unwrap().len();
        let mut f = File::options().append(true).open(&path).unwrap();
        f.write_all(&[9; 15]).unwrap();
        let again = parts(dir.path());
        assert_eq!(read(&again, 0, 4), (true, vec![1; 4]));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), whole, "cut back");
        again.keep(0, 4, &[&[2; 2]], 100).unwrap();
        assert_eq!(read(&parts(dir.path()), 0, 6), (true, vec![1, 1, 1, 1, 2, 2]));
    }

    /// A log without its magic is damaged: deleted at a start, and named so its pieces are cleared.
    #[test]
    fn a_damaged_log_is_deleted_and_named_at_a_start() {
        let dir = tempfile::tempdir().unwrap();
        parts(dir.path()).keep(0, 0, &[&[1; 4]], 100).unwrap();
        std::fs::write(dir.path().join("parts/t/1"), b"garbage").unwrap();
        assert_eq!(damaged(&dir.path().join("parts")), vec![("t".to_owned(), 1)]);
        assert!(dir.path().join("parts/t/0").exists() && !dir.path().join("parts/t/1").exists());
    }

    /// A write that can't be kept (here, the log full to its cap; a full disk alike) fails, and the log
    /// keeps its last whole record.
    #[test]
    fn a_write_that_cannot_be_kept_fails_and_leaves_the_last_whole_record() {
        let dir = tempfile::tempdir().unwrap();
        let p = parts(dir.path());
        p.keep(0, 0, &[&[1; 4]], 20).unwrap();
        assert!(p.keep(0, 4, &[&[2; 8]], 20).is_err(), "past its cap");
        assert_eq!(read(&parts(dir.path()), 0, 4), (true, vec![1; 4]));
        assert!(Parts::new(None).keep(0, 0, &[&[1]], 20).is_err(), "nowhere to keep it");
    }

    /// Written in, the file has every byte kept, and the log is gone; a crash before the file was synced
    /// leaves the log, written in again next time.
    #[test]
    fn written_in_it_is_in_the_file_and_kept_no_more() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, [0; 10]).unwrap();
        let file = File::options().read(true).write(true).open(&path).unwrap();
        let p = parts(dir.path());
        p.keep(0, 2, &[&[1; 4]], 100).unwrap();
        p.keep(0, 4, &[&[2; 4]], 100).unwrap();
        drop(p);
        let crashing = Parts { stop: Some(|s| assert_ne!(s, Step::Written, "crashed")), ..parts(dir.path()) };
        let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crashing.write_into(0, &file)));
        assert!(crash.is_err());
        let again = parts(dir.path());
        assert!(again.has(0), "still kept");
        assert!(again.write_into(0, &file).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), [0, 0, 1, 1, 2, 2, 2, 2, 0, 0]);
        assert!(!again.has(0) && !dir.path().join("parts/t").exists(), "kept no more");
        assert!(!again.write_into(0, &file).unwrap(), "nothing to write");
    }
}
