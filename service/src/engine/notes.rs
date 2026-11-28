//! Phantom's note of each of a torrent's files (#374, gate PR 5): which file at its path is the
//! torrent's ([`Identity`]), and why ([`Why`]). Kept in its record ([`Saved::notes`]), so it
//! outlasts a restart and comes back with an Undo.
//!
//! - A file is noted **Made** as Phantom makes it at its path (as the torrent is let run, or a write
//!   into a file set Off that isn't there), and **Matched** as a check that read it ends. Nothing
//!   notes **Chosen** yet: it's for a file the person chose to download over (gate PR 6).
//! - A file let go of (set Off and gone from its path, or moved to the Trash), or set Normal while it
//!   isn't there, has no note any more: it's downloaded from nothing, not missing.
//! - A torrent added back with the pieces librqbit saved (at a start, a let-go, an undo, a move) is
//!   checked in full instead if a file it gets is at its path that isn't the one noted
//!   ([`differs`]): librqbit's own look at a few pieces passes a file replaced by hand (#393).
//! - A size check tells the machine what the note says of each file ([`Noted`]): one noted that
//!   isn't there is missing, whatever was had of it (#385).
//!
//! A note says which file, not what's in it: nothing is noted as Phantom writes, so a crash leaves
//! notes as they were, and the pieces librqbit saved are trusted as at any start.
//!
//! [`Saved::notes`]: super::store::Saved::notes

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lifecycle::disk::Noted;
use serde::{Deserialize, Serialize};

/// Which file at a torrent's path is its own, and why.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub file: Identity,
    pub why: Why,
}

/// Which file it is: its disk and inode, and when it was made where the file system says.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub dev: u64,
    pub ino: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<SystemTime>,
}

/// Why a file is the torrent's.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Phantom made it.
    Made,
    /// A check read it.
    Matched,
    /// The person chose to download over it (gate PR 6).
    Chosen,
}

impl Identity {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        // A time at or before 1970 isn't a real birth time (file systems without one report zero):
        // the file is then told by its disk and inode alone.
        let birth = meta.created().ok().filter(|b| *b > UNIX_EPOCH);
        Self { dev: meta.dev(), ino: meta.ino(), birth }
    }

    /// The file open as `file`.
    pub fn of_file(file: &std::fs::File) -> Option<Self> {
        file.metadata().ok().map(|m| Self::of(&m))
    }

    /// The file at `path`, never through a link; none if something else is there.
    pub fn at(path: &Path) -> Option<Self> {
        std::fs::symlink_metadata(path).ok().filter(std::fs::Metadata::is_file).map(|m| Self::of(&m))
    }

    /// Whether `other` is the same file: its inode, and when it was made. Its disk only where that
    /// isn't known: a drive mounted again can have another number.
    pub fn is(&self, other: &Identity) -> bool {
        self.ino == other.ino
            && match (self.birth, other.birth) {
                (Some(a), Some(b)) => a == b,
                _ => self.dev == other.dev,
            }
    }
}

/// What `note` says of the file at a path, `at`.
pub fn noted(note: Option<&Note>, at: Option<&Identity>) -> Noted {
    match (note, at) {
        (None, _) => Noted::None,
        (Some(note), Some(at)) if note.file.is(at) => Noted::Same,
        (Some(_), _) => Noted::Differs,
    }
}

/// Whether a file of `files` (by index, with its path) is at its path that `notes` doesn't name:
/// another than the one noted, or one where none was. A torrent added back with its saved pieces is
/// checked in full then. One not there doesn't count: it's made as the torrent starts, or missing.
pub fn differs(notes: &BTreeMap<usize, Note>, files: impl IntoIterator<Item = (usize, PathBuf)>) -> bool {
    files.into_iter().any(|(i, path)| Identity::at(&path).is_some_and(|at| noted(notes.get(&i), Some(&at)) != Noted::Same))
}

/// `notes` with the files a check just read, `read` (by index), noted: each one the check found
/// the torrent's (`kept`), Matched unless it was noted as it is already (a file Phantom made stays
/// Made); those it read and didn't keep, and the files it found none at (`absent`), lose theirs.
pub fn checked(notes: &mut BTreeMap<usize, Note>, read: &BTreeMap<usize, Identity>, kept: &BTreeSet<usize>, absent: &BTreeSet<usize>) {
    for (&i, file) in read {
        if !kept.contains(&i) {
            notes.remove(&i);
            continue;
        }
        if !notes.get(&i).is_some_and(|n| n.file.is(file)) {
            notes.insert(i, Note { file: *file, why: Why::Matched });
        }
    }
    for i in absent {
        notes.remove(i);
    }
}

/// `notes` with the files Phantom made, `made` (by index), noted Made.
pub fn made(notes: &mut BTreeMap<usize, Note>, made: BTreeMap<usize, Identity>) {
    for (i, file) in made {
        notes.insert(i, Note { file, why: Why::Made });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(ino: u64, birth: Option<u64>) -> Identity {
        Identity { dev: 1, ino, birth: birth.map(|s| UNIX_EPOCH + std::time::Duration::from_secs(s)) }
    }

    #[test]
    fn a_file_is_the_same_by_its_inode_and_birth_on_any_disk_number() {
        let a = id(5, Some(10));
        assert!(a.is(&Identity { dev: 2, ..a }), "a drive mounted again under another number");
        assert!(!a.is(&id(5, Some(11))), "the inode given to another file");
        assert!(!a.is(&id(6, Some(10))));
        assert!(id(5, None).is(&id(5, None)));
        assert!(!id(5, None).is(&Identity { dev: 2, ..id(5, None) }), "without a birth, the disk tells");
    }

    #[test]
    fn what_a_note_says_of_a_path() {
        let note = Note { file: id(5, Some(10)), why: Why::Made };
        assert_eq!(noted(None, Some(&id(5, Some(10)))), Noted::None);
        assert_eq!(noted(None, None), Noted::None);
        assert_eq!(noted(Some(&note), Some(&id(5, Some(10)))), Noted::Same);
        assert_eq!(noted(Some(&note), Some(&id(7, Some(10)))), Noted::Differs);
        assert_eq!(noted(Some(&note), None), Noted::Differs, "noted, and gone");
    }

    #[test]
    fn a_check_notes_what_it_kept_and_keeps_made_as_made() {
        let mut notes = BTreeMap::from([(0, Note { file: id(1, None), why: Why::Made }), (1, Note { file: id(2, None), why: Why::Made })]);
        let read = BTreeMap::from([(0, id(1, None)), (1, id(9, None)), (2, id(3, None)), (3, id(4, None))]);
        checked(&mut notes, &read, &BTreeSet::from([0, 1, 2]), &BTreeSet::new());
        assert_eq!(notes[&0].why, Why::Made, "the file Phantom made, read");
        assert_eq!(notes[&1], Note { file: id(9, None), why: Why::Matched }, "another file there now, read");
        assert_eq!(notes[&2].why, Why::Matched);
        assert!(!notes.contains_key(&3), "read, not kept: missing again");
        checked(&mut notes, &BTreeMap::new(), &BTreeSet::new(), &BTreeSet::from([0]));
        assert!(!notes.contains_key(&0), "none there as it was checked");
    }

    #[test]
    fn a_note_round_trips() {
        let note = Note { file: id(5, Some(10)), why: Why::Matched };
        let json = serde_json::to_string(&note).unwrap();
        assert_eq!(serde_json::from_str::<Note>(&json).unwrap(), note);
    }
}
