//! What a size check finds of a torrent's files, as plain data: the engine reads it from disk, the
//! GUI's fake service makes it up. What it means for the torrent is the machine's to decide
//! (`lib.rs`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A file a torrent is getting, and what must be on disk for it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Expected {
    pub index: usize,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// What librqbit has of it.
    pub progress_bytes: u64,
}

/// A missing file, by what librqbit had of it before it went and what was at its path since.
#[derive(Debug, Clone, PartialEq)]
pub struct Gone {
    pub path: PathBuf,
    pub size_bytes: u64,
    /// What librqbit had of it before it went: a file put back must hold at least as much.
    pub progress_bytes: u64,
    pub seen: Seen,
}

/// What was at a missing file's path when it was last judged.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Seen {
    /// Not size-checked yet: the next size check records what's there (the file librqbit made again
    /// when it restored the torrent, or the one a check just found isn't the file that went)
    /// without taking it for the file put back.
    NotYet,
    /// This file (`None`: none).
    Was(Option<Stamp>),
}

/// Which file is at a path, and its size and when it was last changed: another file, or the same
/// one changed, differs in one of them.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    pub inode: u64,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

/// What a size check found: each file the torrent is getting (those Normal), by index, and what's
/// at its path. Facts only: what they mean for the torrent is the machine's to decide.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Disk {
    pub files: BTreeMap<usize, Found>,
}

/// A file a torrent is getting, and what a size check found at its path (`None`: no file).
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub file: Expected,
    pub at: Option<Stamp>,
    /// How many names the file there has: more than one, it's another file's too (a hard link).
    pub links: u64,
    /// Something other than a file is at its path (a folder, a symbolic link).
    pub taken: bool,
    /// Phantom has a file of it open for writing (it was let run): one that isn't at its path then went.
    pub open: bool,
    /// Its bytes in pieces it shares with other files: librqbit can't have those without them.
    pub shared: u64,
}

impl Found {
    /// What's at `file`'s path: a file with one name, or nothing.
    pub fn of(file: Expected, at: Option<Stamp>) -> Self {
        Self { file, at, links: 1, taken: false, open: false, shared: 0 }
    }

    /// It isn't on disk as it must be: nothing is at its path though some of it was had or it was
    /// open (one never got nor made is made as the torrent starts, #374), something other than a file
    /// is, or a file it can't be.
    pub fn is_missing(&self) -> bool {
        self.taken || (self.at.is_none() && (self.file.progress_bytes > 0 || self.open)) || self.is_impossible()
    }

    /// A file is there that can't be the torrent's: longer than the torrent's, shorter than what was
    /// had of it, or another file's too while it's incomplete (written, the other would change). It
    /// isn't the torrent's file as it was, whatever a check finds in it, and nothing writes into it.
    pub fn is_impossible(&self) -> bool {
        self.at.is_some_and(|at| {
            at.len > self.file.size_bytes || at.len < self.file.progress_bytes || (self.links > 1 && !self.whole_but_shared())
        })
    }

    /// librqbit has every piece of it but those it shares with other files, which only they complete.
    pub fn whole_but_shared(&self) -> bool {
        self.file.progress_bytes + self.shared >= self.file.size_bytes
    }
}

impl Disk {
    /// Whether a file there [is of a size it can't have](Found::is_impossible): nothing may be
    /// written into it, so the torrent isn't run, nor checked unasked (#374).
    pub fn any_impossible(&self) -> bool {
        self.files.values().any(Found::is_impossible)
    }
}

impl Gone {
    /// Gone as found, with what's at its path.
    pub fn found(found: &Found) -> Self {
        let Expected { path, size_bytes, progress_bytes, .. } = found.file.clone();
        Self { path, size_bytes, progress_bytes, seen: Seen::Was(found.at) }
    }

    /// Whether a file that can be the missing one is at its path `now`: of a size it can have, and
    /// not what was there when it was last judged.
    pub fn is_back(&self, now: Option<Stamp>) -> bool {
        let (Seen::Was(seen), Some(now)) = (self.seen, now) else { return false };
        now.len <= self.size_bytes && now.len >= self.progress_bytes && Some(now) != seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u64 = 100;
    const PROGRESS: u64 = 40;
    const THEN: u64 = 1;
    const NOW: u64 = 2;

    fn stamp(inode: u64, len: u64) -> Stamp {
        Stamp { inode, len, modified: None }
    }

    fn found(len: u64) -> Found {
        let file = Expected { index: 0, path: "a".into(), size_bytes: SIZE, progress_bytes: PROGRESS };
        Found::of(file, Some(stamp(NOW, len)))
    }

    fn gone() -> Gone {
        Gone { path: "a".into(), size_bytes: SIZE, progress_bytes: PROGRESS, seen: Seen::Was(Some(stamp(THEN, 0))) }
    }

    /// A file holding exactly what's counted has every counted byte: one byte less has lost some.
    #[test]
    fn a_file_as_long_as_its_progress_is_on_disk_and_one_byte_shorter_is_missing() {
        assert!(!found(PROGRESS).is_missing());
        assert!(found(PROGRESS - 1).is_missing());
    }

    /// A file as long as its torrent is the torrent's: one byte longer is not as it was.
    #[test]
    fn a_file_as_long_as_its_torrent_is_on_disk_and_one_byte_longer_is_missing() {
        assert!(!found(SIZE).is_missing());
        assert!(found(SIZE + 1).is_missing());
    }

    /// A hard link to another file: while incomplete it can't be the torrent's (writing it would write
    /// the other), complete it can (only read).
    #[test]
    fn a_hard_link_is_impossible_until_complete() {
        let linked = |progress, shared| Found {
            links: 2,
            open: true,
            shared,
            file: Expected { progress_bytes: progress, ..found(SIZE).file },
            ..found(SIZE)
        };
        assert!(linked(PROGRESS, 0).is_impossible());
        assert!(!linked(SIZE, 0).is_missing());
        assert!(!linked(PROGRESS, SIZE - PROGRESS).is_missing(), "but for what it shares, which only the other files complete");
    }

    /// Nothing at its path: missing once some of it was had or it was made, or when something else is
    /// there.
    #[test]
    fn nothing_at_its_path_is_missing_only_once_some_of_it_was_had() {
        let none = |progress, taken, open| Found {
            at: None,
            taken,
            open,
            file: Expected { progress_bytes: progress, ..found(0).file },
            links: 1,
            shared: 0,
        };
        assert!(!none(0, false, false).is_missing(), "never had nor made: made as it starts");
        assert!(none(1, false, false).is_missing());
        assert!(none(0, false, true).is_missing(), "made, and gone since");
        assert!(none(0, true, false).is_missing(), "a folder or a link there");
    }

    #[test]
    fn a_file_as_long_as_its_progress_is_back_and_one_byte_shorter_is_not() {
        assert!(gone().is_back(Some(stamp(NOW, PROGRESS))));
        assert!(!gone().is_back(Some(stamp(NOW, PROGRESS - 1))));
    }

    #[test]
    fn a_file_as_long_as_its_torrent_is_back_and_one_byte_longer_is_not() {
        assert!(gone().is_back(Some(stamp(NOW, SIZE))));
        assert!(!gone().is_back(Some(stamp(NOW, SIZE + 1))));
    }
}
