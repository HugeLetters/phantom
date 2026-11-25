//! Phantom's record of each torrent (#45): the source of truth, from which the engine adds it to
//! librqbit itself, at a start too. Kept in a database in the data dir (`records.db`, #243): a row
//! per torrent, by info hash, with its record (id, name, destination, trackers, the files it gets,
//! whether it's wanted running, and what's under way on it) and its .torrent, once its info arrived;
//! and the next id to give.
//!
//! [`Saved`] is a record as its row and `undo.json` keep it; [`Record`] one parsed from it, with its
//! info, which the engine works with.
//!
//! [`Store::save`] is given every record, as the engine has them, and writes what changed since
//! the last save, as one transaction: each record that changed, its .torrent with it the first
//! time it has one, and a row for each that went deleted. So a write-ahead step (a Delete's
//! `deleting` and each item it moved, a let-go's, a move's) rewrites only its torrent's row, and a
//! crash leaves the database as one save or the one before, never between: as the whole-file
//! rewrite through a rename did before.
//!
//! A database that can't be read (not one, damaged, or of another shape) is kept aside as it was,
//! `records.invalid.db`, and the service starts with none, which the log says and the app is told ([`Loaded::set_aside`]). A row whose record
//! can't be read is left out of the list and stays in the database as it is; one whose .torrent
//! can't be read keeps its record, as it was, listed as an error until a check reads it. Nothing
//! that can't be read is deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use proto::{AbsolutePath, TorrentId};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::files::{Gone, Seen, Stamp};
use super::machine::{Wants, letgo};
use super::trash::Trashed;
use super::undo::Stash;

const FILE: &str = "records.db";
/// A database that couldn't be read, kept as it was (with a number when one is there already).
const INVALID: &str = "records.invalid";

/// How long a statement waits for another connection's lock. Short in tests, which hold one longer.
const BUSY_TIMEOUT: std::time::Duration =
    if cfg!(test) { std::time::Duration::from_millis(300) } else { std::time::Duration::from_secs(10) };

/// The tables, made in one transaction with the first id to give.
const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS torrents (
        info_hash TEXT PRIMARY KEY NOT NULL,
        record TEXT NOT NULL,
        torrent BLOB
    ) STRICT;
    CREATE TABLE IF NOT EXISTS ids (
        one INTEGER PRIMARY KEY CHECK (one = 1),
        next_id INTEGER NOT NULL
    ) STRICT;
    INSERT OR IGNORE INTO ids (one, next_id) VALUES (1, 0);
";

/// The ids given so far, and the database the records are kept in.
pub struct Store {
    path: PathBuf,
    /// librqbit's session folder.
    session: PathBuf,
    conn: libsql::Connection,
    next_id: TorrentId,
    /// What the database holds, as last written: what [`Store::save`] compares the records with.
    written: Mutex<Written>,
    /// The next save fails after this many of its statements, as on a full disk, and rolls back.
    #[cfg(test)]
    fails_after: Mutex<Option<usize>>,
}

/// What the database holds of the records this store loaded or wrote.
#[derive(Debug, Default, Clone, PartialEq)]
struct Written {
    next_id: TorrentId,
    /// By info hash: each record as written, and whether its row has its .torrent.
    rows: BTreeMap<String, Row>,
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    record: String,
    torrent: bool,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.path).field("next_id", &self.next_id).finish_non_exhaustive()
    }
}

/// A torrent's record as it's kept, in its row of the database and in `undo.json` (an entry that
/// brings a torrent back).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: TorrentId,
    /// From the magnet's `dn=`, the .torrent, or its info once it arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The name it has on disk, once a Rename gave it one other than its info's (#331): a torrent
    /// of one file's file, a torrent of several's own folder. None: as its info names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<String>,
    /// The folder the torrent's files go into; a torrent of several files has its own folder
    /// inside it. Given by the add.
    pub destination: AbsolutePath,
    /// The trackers it names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trackers: Vec<String>,
    /// The files it gets (those Normal), by index; `None`: every one, until a priority changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<BTreeSet<usize>>,
    /// Run or stop, as the user last said (or stopped at a seeding limit): kept whenever it changes.
    pub wants: Wants,
    /// Its check was cancelled and nothing has checked it since ([`Phase::Unchecked`](super::machine::Phase::Unchecked)):
    /// it comes back so at start, not Ready with progress nobody checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unchecked: bool,
    /// Its files were looked at on a reachable destination once: before, a file not there may never
    /// have been made (added, or its info arrived, while the destination was away).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reached: bool,
    /// What it did in its seeding round ([`lifecycle::seeding`]): librqbit forgets what it
    /// uploaded at every stop. Kept at the regular size check and when the service stops.
    pub round: lifecycle::seeding::Round,
    /// The files known missing, by index, kept as that's learnt, so a crash loses none (#370): those
    /// of Missing files, and those a check is to tell are back; as they were while it's away, or an
    /// error. A start judges them as they're here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missing: BTreeMap<usize, Missing>,
    /// What librqbit had of each file, by index, as its phase last changed or the service stopped:
    /// the first size check at the next start, before librqbit has it again, judges its files by it
    /// where librqbit's saved pieces are gone (they go as it's taken out). None: nothing is known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub had: Vec<u64>,
    /// What Delete couldn't move to the Trash, while the torrent waits for it to be tried again
    /// ([`Phase::DeleteFailed`](super::machine::Phase::DeleteFailed)): it comes back so at start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delete_failed: Vec<proto::Kept>,
    /// The files a Delete under way is moving to the Trash, saved before it starts: a crash
    /// leaves the torrent with what's still on disk, or deleted if nothing is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleting: Vec<PathBuf>,
    /// What Delete moved to the Trash so far, and where each went there, recorded as each goes:
    /// while it's Deleting, and kept while it waits with the rest ([`Phase::DeleteFailed`]), for
    /// Undo to put back.
    ///
    /// [`Phase::DeleteFailed`]: super::machine::Phase::DeleteFailed
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trashed: Vec<Trashed>,
    /// The torrent kept aside for Undo, from before its first Delete: what librqbit had of it while
    /// it was whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stash: Option<Stash>,
    /// Letting go of its files gone from the Trash (#234, [`letgo`]): each step written ahead, so
    /// a start finishes one a crash cut short.
    #[serde(default, skip_serializing_if = "is_holding")]
    pub letting_go: letgo::State,
    /// Its files moving to a new destination (#24), written ahead of each step: a start finishes
    /// one past the point where `destination` names the new one, and restores one before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<Moving>,
}

/// A missing file, as the record keeps it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Missing {
    /// What librqbit had of it before it went.
    pub had: u64,
    /// What was at its path when it was last judged.
    pub seen: Seen,
}

impl From<&Gone> for Missing {
    fn from(gone: &Gone) -> Self {
        let seen = match gone.seen {
            // A time before 1970 can't be kept: the file there then differs, so it's checked.
            Seen::Was(Some(at)) => {
                Seen::Was(Some(Stamp { modified: at.modified.filter(|m| m.duration_since(std::time::UNIX_EPOCH).is_ok()), ..at }))
            }
            seen => seen,
        };
        Self { had: gone.progress_bytes, seen }
    }
}

/// A move under way, as the record keeps it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Moving {
    /// What moves, from where to where.
    pub plan: super::moving::Plan,
    /// The torrent kept aside as it was taken out of librqbit: the pieces it had come back with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stash: Option<Stash>,
    /// Every file is in the new destination, which the record names now: what's left is putting
    /// them in place and removing the old.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub placed: bool,
    /// Every file has taken its place there, written ahead before any original is removed: from
    /// then on the move never goes back, and a start after a crash goes straight on removing the
    /// old. Each move's own: a new one starts without it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub landed: bool,
    /// A file of it wasn't the one librqbit had open as it was taken out (replaced by hand): its
    /// pieces don't come back with it, and it's checked in full once it's added again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replaced: bool,
    /// The name it has on disk before and after, when the move gives it a new one (a Rename,
    /// #331): the record's `on_disk` is `now` from where `destination` names the new one, `was`
    /// before (none: its info's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Names>,
}

/// The names on disk a move takes a torrent from and to ([`Record::on_disk`]'s: none is its info's).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Names {
    pub was: Option<String>,
    pub now: Option<String>,
}

fn is_holding(state: &letgo::State) -> bool {
    *state == letgo::State::Holding
}

/// A torrent's record: what Phantom knows of it, which librqbit gets it from.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub info_hash: String,
    pub id: TorrentId,
    /// Its name: the magnet's `dn=` or the .torrent's, then its info's.
    pub name: Option<String>,
    /// The name a Rename gave it on disk (#331), when it isn't its info's.
    pub on_disk: Option<String>,
    pub destination: AbsolutePath,
    /// Its info, once it arrived.
    pub info: Option<Info>,
    pub trackers: Vec<String>,
    /// The files it gets (those Normal), by index.
    pub gets: Gets,
    pub wants: Wants,
    pub unchecked: bool,
    pub reached: bool,
    pub round: lifecycle::seeding::Round,
    pub missing: BTreeMap<usize, Missing>,
    pub had: Vec<u64>,
    pub delete_failed: Vec<proto::Kept>,
    pub deleting: Vec<PathBuf>,
    pub trashed: Vec<Trashed>,
    pub stash: Option<Stash>,
    pub letting_go: letgo::State,
    pub moving: Option<Moving>,
}

/// The files a torrent gets (those Normal).
#[derive(Debug, Clone, PartialEq)]
pub enum Gets {
    /// Every one, until a priority changes: as many as its info has, once it's known.
    Every,
    Only(BTreeSet<usize>),
}

/// A torrent's info, parsed once from its .torrent: its files, in librqbit's order and as librqbit
/// names them.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub bytes: Arc<[u8]>,
    pub files: Vec<FileOf>,
    pub piece_length: u32,
    /// Its own name, if it has one.
    pub name: Option<String>,
}

/// A file of a torrent, as its info has it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileOf {
    /// Inside the torrent's own folder (or the destination, for a torrent of one file).
    pub path: PathBuf,
    pub len: u64,
    /// BEP 47 padding, which only aligns the others and is never written.
    pub padding: bool,
    /// The pieces it's in.
    pub pieces: Range<u32>,
}

impl Info {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let torrent = librqbit::torrent_from_bytes(bytes)?;
        let name = torrent.info.data.name.as_ref().map(|n| String::from_utf8_lossy(n.as_ref()).into_owned()).filter(|n| !n.is_empty());
        let info = torrent.info.data.validate()?;
        let files = info
            .iter_file_details_ext()
            .map(|f| FileOf {
                path: f.details.filename.to_pathbuf(),
                len: f.details.len,
                padding: f.details.attrs().padding,
                pieces: f.pieces.clone(),
            })
            .collect();
        Ok(Self { bytes: bytes.into(), files, piece_length: info.lengths().default_piece_length(), name })
    }

    pub fn single(&self) -> bool {
        self.files.len() == 1
    }

    /// Every file, by index: what a torrent gets until a priority changes.
    pub fn all(&self) -> BTreeSet<usize> {
        (0..self.files.len()).collect()
    }

    /// A path of it, inside its destination, that takes a name of Phantom's own there
    /// ([`super::trashed::phantoms`]): a move's staging. Such a torrent is never added, so it never
    /// shares one.
    pub fn phantoms(&self, magnet_name: Option<&str>) -> Option<PathBuf> {
        let own = self.own_folder(magnet_name).unwrap_or_default();
        self.files.iter().map(|f| own.join(&f.path)).find(|p| p.components().any(|c| super::trashed::phantoms(c.as_os_str())))
    }

    /// The torrent's own folder in its destination, as librqbit names it: none for one file; its
    /// name, else the magnet's `magnet_name`, else the stem of its longest file.
    fn own_folder(&self, magnet_name: Option<&str>) -> Option<PathBuf> {
        if self.single() {
            return None;
        }
        let longest = || self.files.iter().max_by_key(|f| f.len).and_then(|f| f.path.file_stem()).map(PathBuf::from);
        self.name.as_deref().or(magnet_name).map(PathBuf::from).or_else(longest)
    }
}

#[cfg(test)]
impl Saved {
    /// One of a torrent that's still fetching info.
    pub fn sample() -> Self {
        let destination = AbsolutePath::new("/x").expect("absolute");
        Record::new("00".repeat(20), 0, None, destination, Vec::new(), Wants::Stop).saved()
    }
}

impl Record {
    /// `saved`, of torrent `info_hash`, with its `info`.
    pub fn parse(info_hash: &str, saved: Saved, info: Option<Info>) -> Self {
        let Saved {
            id,
            name,
            on_disk,
            destination,
            trackers,
            normal,
            wants,
            unchecked,
            reached,
            round,
            missing,
            had,
            delete_failed,
            deleting,
            trashed,
            stash,
            letting_go,
            moving,
        } = saved;
        let gets = match normal {
            Some(only) => Gets::Only(only),
            None => Gets::Every,
        };
        Self {
            info_hash: info_hash.to_owned(),
            id,
            name: name.or_else(|| info.as_ref().and_then(|i| i.name.clone())),
            on_disk,
            destination,
            info,
            trackers,
            gets,
            wants,
            unchecked,
            reached,
            round,
            missing,
            had,
            delete_failed,
            deleting,
            trashed,
            stash,
            letting_go,
            moving,
        }
    }

    /// A new torrent's, before its info arrives.
    pub fn new(
        info_hash: String,
        id: TorrentId,
        name: Option<String>,
        destination: AbsolutePath,
        trackers: Vec<String>,
        wants: Wants,
    ) -> Self {
        Self {
            info_hash,
            id,
            name,
            on_disk: None,
            destination,
            info: None,
            trackers,
            gets: Gets::Every,
            wants,
            unchecked: false,
            reached: false,
            round: Default::default(),
            missing: BTreeMap::new(),
            had: Vec::new(),
            delete_failed: Vec::new(),
            deleting: Vec::new(),
            trashed: Vec::new(),
            stash: None,
            letting_go: letgo::State::Holding,
            moving: None,
        }
    }

    /// As it's kept.
    pub fn saved(&self) -> Saved {
        Saved {
            id: self.id,
            name: self.name.clone(),
            on_disk: self.on_disk.clone(),
            destination: self.destination.clone(),
            trackers: self.trackers.clone(),
            normal: match &self.gets {
                Gets::Every => None,
                Gets::Only(only) => Some(only.clone()),
            },
            wants: self.wants,
            unchecked: self.unchecked,
            reached: self.reached,
            round: self.round,
            missing: self.missing.clone(),
            had: self.had.clone(),
            delete_failed: self.delete_failed.clone(),
            deleting: self.deleting.clone(),
            trashed: self.trashed.clone(),
            stash: self.stash.clone(),
            letting_go: self.letting_go.clone(),
            moving: self.moving.clone(),
        }
    }

    /// The files it gets (those Normal), by index; none known before its info.
    pub fn normal(&self) -> BTreeSet<usize> {
        match &self.gets {
            Gets::Only(only) => only.clone(),
            Gets::Every => self.info.as_ref().map(Info::all).unwrap_or_default(),
        }
    }

    /// Whether it gets file `index`.
    pub fn gets(&self, index: usize) -> bool {
        match &self.gets {
            Gets::Only(only) => only.contains(&index),
            Gets::Every => true,
        }
    }

    /// The folder librqbit puts its files in: its destination, or its own folder there, under the
    /// name it has on disk.
    pub fn output_folder(&self) -> Option<PathBuf> {
        let info = self.info.as_ref()?;
        Some(match (info.own_folder(self.name.as_deref()), &self.on_disk) {
            (Some(_), Some(renamed)) => self.destination.join(renamed),
            (Some(own), None) => self.destination.join(own),
            (None, _) => self.destination.to_path_buf(),
        })
    }

    /// File `f`'s path in its folder ([`Record::output_folder`]): as its info names it, but the
    /// file of a torrent of one file, which is the torrent, under the name it has on disk.
    pub fn file_path(&self, f: &FileOf) -> PathBuf {
        match (&self.on_disk, self.info.as_ref().is_some_and(Info::single)) {
            (Some(renamed), true) => PathBuf::from(renamed),
            (Some(_) | None, _) => f.path.clone(),
        }
    }

    /// Where it is on disk: the file itself for a torrent of one file, its own folder otherwise.
    pub fn location(&self) -> Option<PathBuf> {
        let info = self.info.as_ref()?;
        let folder = self.output_folder()?;
        Some(match &info.files[..] {
            [only] => folder.join(self.file_path(only)),
            _ => folder,
        })
    }

    /// Each file's path, by index.
    pub fn paths(&self) -> Vec<PathBuf> {
        let (Some(info), Some(folder)) = (&self.info, self.output_folder()) else { return Vec::new() };
        info.files.iter().map(|f| folder.join(self.file_path(f))).collect()
    }

    /// The name it has on disk: the one a Rename gave it, else its info's.
    pub fn disk_name(&self) -> Option<String> {
        self.on_disk.clone().or_else(|| self.info_name())
    }

    /// Its name as clients see it: the one it has on disk.
    pub fn shown_name(&self) -> Option<String> {
        self.on_disk.clone().or_else(|| self.name.clone())
    }

    /// The name its info gives it on disk, which a Rename back to gives it again: its own folder's,
    /// or its one file's.
    pub fn info_name(&self) -> Option<String> {
        let info = self.info.as_ref()?;
        let name = match (&info.files[..], info.own_folder(self.name.as_deref())) {
            ([only], _) => only.path.clone(),
            (_, own) => own?,
        };
        Some(name.to_string_lossy().into_owned())
    }

    /// The files known missing at start: what's at their paths then is judged against what was
    /// there before, as it would be had the service not gone.
    pub fn gone_at_start(&self) -> BTreeMap<usize, Gone> {
        let (Some(info), paths) = (&self.info, self.paths()) else { return BTreeMap::new() };
        self.missing
            .iter()
            .filter(|(i, _)| self.gets(**i))
            .filter_map(|(&i, &Missing { had: progress_bytes, seen })| {
                let f = info.files.get(i)?;
                Some((i, Gone { path: paths[i].clone(), size_bytes: f.len, progress_bytes, seen }))
            })
            .collect()
    }
}

/// What a start finds: the store, the records, and those of them whose .torrent can't be read,
/// with why.
pub struct Loaded {
    pub store: Store,
    pub records: Vec<Record>,
    pub unreadable: BTreeMap<String, String>,
    /// Where the database went, when it couldn't be read and was kept aside.
    pub set_aside: Option<PathBuf>,
}

/// Why a database didn't open.
enum NotOpened {
    /// It isn't one Phantom can read: not a database, damaged, or of another shape.
    Damaged(anyhow::Error),
    /// It couldn't be reached (permissions, a full disk): it's left as it is.
    Failed(anyhow::Error),
}

impl From<libsql::Error> for NotOpened {
    fn from(e: libsql::Error) -> Self {
        match e {
            libsql::Error::SqliteFailure(code, _) if damaged(code) => Self::Damaged(e.into()),
            e => Self::Failed(e.into()),
        }
    }
}

/// Whether SQLite's result `code` says the database itself is bad: SQLITE_CORRUPT, SQLITE_NOTADB;
/// and SQLITE_ERROR, which a statement of the store's meets only in tables of another shape. Every
/// other (busy, locked, full, no permission, an I/O error) is the database being out of reach
/// now: it's kept as it is, never set aside.
fn damaged(code: i32) -> bool {
    [1, 11, 26].contains(&(code & 0xff))
}

impl Store {
    /// Loads the records in `dir`. A record without its .torrent takes librqbit's copy in its
    /// `session` folder: librqbit had the torrent, and the engine hadn't heard yet when it
    /// stopped. Others whose info never arrived go: they were fetching info, and librqbit never
    /// had them. A copy of librqbit's no row names is dropped: a crash left it behind as its
    /// torrent left the list. A database that can't be read is kept aside as it was, and the store
    /// starts with none; one that can't be reached fails.
    pub fn load(dir: &Path, session: &Path) -> Result<Loaded> {
        let path = dir.join(FILE);
        let loaded = match Self::open(&path, session) {
            Ok(loaded) => loaded,
            Err(NotOpened::Damaged(e)) => {
                let aside = keep_aside(&path)?;
                warn!(error = %format!("{e:#}"), kept = %aside.display(), "the torrents' records can't be read: kept aside as they were, starting with none");
                let loaded = Self::open(&path, session).map_err(|e| match e {
                    NotOpened::Damaged(e) | NotOpened::Failed(e) => e,
                })?;
                Loaded { set_aside: Some(aside), ..loaded }
            }
            Err(NotOpened::Failed(e)) => return Err(e.context(format!("failed to open {}", path.display()))),
        };
        loaded.store.save(loaded.records.iter())?;
        Ok(loaded)
    }

    fn open(path: &Path, session: &Path) -> Result<Loaded, NotOpened> {
        let conn = wait(async {
            let db = libsql::Builder::new_local(path).build().await?;
            db.connect()
        })?;
        // Another connection to it may be closing, or saving: an engine that stopped, or crashed
        // in a test. It's waited for, as SQLite would otherwise refuse at once.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let (next_id, rows) = wait(async {
            // Each commit is on the disk before the engine acts on it (F_FULLFSYNC, as the file
            // writes before did): a write-ahead step must outlast a power cut too.
            let wal = text(first(&conn, "PRAGMA journal_mode = WAL").await?);
            if wal.as_deref() != Some("wal") {
                return Err(NotOpened::Failed(anyhow::anyhow!("its journal can't be a write-ahead log: {wal:?}")));
            }
            conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;").await?;
            let check = text(first(&conn, "PRAGMA quick_check").await?);
            if check.as_deref() != Some("ok") {
                return Err(NotOpened::Damaged(anyhow::anyhow!("it's damaged: {}", check.unwrap_or_default())));
            }
            // `BEGIN IMMEDIATE`, so the write lock is asked for first and waited for: a deferred
            // transaction's upgrade to a write refuses at once, past the busy timeout.
            begin(&conn).await?;
            if let Err(e) = conn.execute_batch(SCHEMA).await {
                rollback(&conn).await;
                return Err(e.into());
            }
            commit(&conn).await?;
            let next_id = match first(&conn, "SELECT next_id FROM ids").await? {
                Some(libsql::Value::Integer(next_id)) if next_id >= 0 => next_id as u64,
                _ => return Err(NotOpened::Damaged(anyhow::anyhow!("it has no next id"))),
            };
            let mut rows = conn.query("SELECT info_hash, record, torrent FROM torrents", ()).await?;
            let mut read = Vec::new();
            while let Some(row) = rows.next().await? {
                let torrent = match row.get_value(2)? {
                    libsql::Value::Blob(bytes) => Some(bytes),
                    _ => None,
                };
                read.push((row.get::<String>(0)?, row.get::<String>(1)?, torrent));
            }
            Ok((next_id, read))
        })?;
        // librqbit's .torrent of a hash no row names is one a crash left behind as the torrent left
        // the list (#334): taken for librqbit's, it would bring a torrent added since back with info.
        let named: BTreeSet<&str> = rows.iter().map(|(hash, ..)| hash.as_str()).collect();
        for entry in std::fs::read_dir(session).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let Some(hash) = name.to_str().and_then(|n| n.strip_suffix(".torrent")) else { continue };
            if !named.contains(hash) {
                forget_librqbits(session, hash);
            }
        }
        let mut written = Written { next_id, rows: BTreeMap::new() };
        let (mut records, mut unreadable) = (Vec::new(), BTreeMap::new());
        for (hash, json, torrent) in rows {
            let saved: Saved = match serde_json::from_str(&json) {
                Ok(saved) => saved,
                Err(e) => {
                    warn!(hash, error = %e, "a torrent's record can't be read: left out of the list, and kept as it is");
                    continue;
                }
            };
            written.rows.insert(hash.clone(), Row { record: json, torrent: torrent.is_some() });
            let info = match torrent.map(Ok).or_else(|| librqbits(session, &hash)) {
                Some(Ok(bytes)) => Info::parse(&bytes).map_err(|e| format!("its .torrent isn't one: {e:#}")),
                Some(Err(why)) => Err(why),
                // Fetching info when the service stopped: librqbit never had it.
                None => continue,
            };
            let info = match info {
                Ok(info) => Some(info),
                Err(why) => {
                    warn!(hash, why, "can't read the torrent's .torrent: listed as an error, until a check reads it");
                    unreadable.insert(hash.clone(), why);
                    None
                }
            };
            records.push(Record::parse(&hash, saved, info));
        }
        let store = Self {
            path: path.to_owned(),
            session: session.to_owned(),
            conn,
            next_id,
            written: Mutex::new(written),
            #[cfg(test)]
            fails_after: Mutex::default(),
        };
        Ok(Loaded { store, records, unreadable, set_aside: None })
    }

    /// Torrent `record`'s info, read again from where [`Store::load`] looks, for a record whose
    /// .torrent couldn't be read. Why not, if it still can't.
    pub fn read_info(&self, record: &Record) -> Result<Info, String> {
        let bytes = wait(async {
            let mut rows = self.conn.query("SELECT torrent FROM torrents WHERE info_hash = ?1", [record.info_hash.as_str()]).await?;
            Ok::<_, libsql::Error>(match rows.next().await?.map(|r| r.get_value(0)).transpose()? {
                Some(libsql::Value::Blob(bytes)) => Some(bytes),
                _ => None,
            })
        });
        match bytes.map_err(|e| format!("{e:#}"))?.map(Ok).or_else(|| librqbits(&self.session, &record.info_hash)) {
            Some(bytes) => Info::parse(&bytes?).map_err(|e| format!("its .torrent isn't one: {e:#}")),
            None => Err("its .torrent is gone".into()),
        }
    }

    /// Keeps `records`, every torrent's, and the next id, as one transaction: each record that
    /// changed since the last save (with its info's .torrent, the first time it has one), and the
    /// row of each that's gone deleted, with its .torrent. A save that fails changes nothing, and
    /// the next one writes what it didn't.
    pub fn save<'a>(&self, records: impl IntoIterator<Item = &'a Record>) -> Result<()> {
        let mut written = self.written.lock().expect("nothing panics holding it");
        // This save's own, even one with nothing to write: never left for the next (#309).
        #[cfg(test)]
        let mut statements = self.fails_after.lock().unwrap().take();
        let mut next = Written { next_id: self.next_id, rows: BTreeMap::new() };
        let mut changed = Vec::new();
        for record in records {
            let json = serde_json::to_string(&record.saved())?;
            let before = written.rows.get(&record.info_hash);
            let has_torrent = before.is_some_and(|b| b.torrent);
            let torrent = record.info.as_ref().filter(|_| !has_torrent).map(|i| i.bytes.clone());
            if torrent.is_some() || before.is_none_or(|b| b.record != json) {
                changed.push((record.info_hash.clone(), json.clone(), torrent));
            }
            next.rows.insert(record.info_hash.clone(), Row { record: json, torrent: has_torrent || record.info.is_some() });
        }
        let gone: Vec<&String> = written.rows.keys().filter(|hash| !next.rows.contains_key(*hash)).collect();
        if changed.is_empty() && gone.is_empty() && next.next_id == written.next_id {
            return Ok(());
        }
        wait(async {
            #[cfg(test)]
            let mut statement = || match statements.as_mut() {
                Some(0) => Err(anyhow::anyhow!("the disk is full")),
                Some(n) => Ok(*n -= 1),
                None => Ok(()),
            };
            begin(&self.conn).await?;
            let done = async {
                for (hash, json, torrent) in &changed {
                    #[cfg(test)]
                    statement()?;
                    let torrent = torrent.as_ref().map_or(libsql::Value::Null, |t| libsql::Value::Blob(t.to_vec()));
                    self.conn
                        .execute(
                            "INSERT INTO torrents (info_hash, record, torrent) VALUES (?1, ?2, ?3)
                     ON CONFLICT (info_hash) DO UPDATE SET record = excluded.record, torrent = coalesce(excluded.torrent, torrent)",
                            libsql::params![hash.as_str(), json.as_str(), torrent],
                        )
                        .await?;
                }
                for hash in &gone {
                    #[cfg(test)]
                    statement()?;
                    self.conn.execute("DELETE FROM torrents WHERE info_hash = ?1", [hash.as_str()]).await?;
                }
                if next.next_id != written.next_id {
                    #[cfg(test)]
                    statement()?;
                    self.conn.execute("UPDATE ids SET next_id = ?1", [next.next_id]).await?;
                }
                anyhow::Ok(())
            }
            .await;
            match done {
                Ok(()) => commit(&self.conn).await.map_err(Into::into),
                Err(e) => {
                    rollback(&self.conn).await;
                    Err(e)
                }
            }
        })
        .with_context(|| format!("failed to save the records in {}", self.path.display()))?;
        *written = next;
        Ok(())
    }

    /// Makes the next save fail after `statements` of its statements, and roll back; one with no
    /// more to write succeeds.
    #[cfg(test)]
    pub fn fail_next_save_after(&self, statements: usize) {
        *self.fails_after.lock().unwrap() = Some(statements);
    }

    /// Fresh ids from now on are above `id`, which is taken elsewhere.
    pub fn ids_above(&mut self, id: TorrentId) {
        self.next_id = self.next_id.max(id + 1);
    }

    /// A fresh id, for a torrent whose info hash isn't known yet.
    pub fn next_id(&mut self) -> TorrentId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// librqbit's copy of torrent `hash`'s .torrent in its `session` folder, which it writes as it
/// adds one, if it's there; why it can't be read, if it can't.
fn librqbits(session: &Path, hash: &str) -> Option<Result<Vec<u8>, String>> {
    let path = librqbits_path(session, hash);
    match std::fs::read(&path) {
        Ok(bytes) => Some(Ok(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!("failed to read {}: {e}", path.display()))),
    }
}

fn librqbits_path(session: &Path, hash: &str) -> PathBuf {
    session.join(format!("{hash}.torrent"))
}

/// Drops librqbit's copy of torrent `hash`'s .torrent in its `session` folder, if it's there, as
/// the torrent leaves the list. librqbit's delete drops it only of a torrent it has: one a crash
/// left behind would pass, at a start, for librqbit having had the next torrent of that hash added
/// (#334).
pub fn forget_librqbits(session: &Path, hash: &str) {
    let path = librqbits_path(session, hash);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(error = %e, path = %path.display(), "failed to drop librqbit's .torrent");
    }
}

/// Starts a transaction that holds the write lock from its first statement, waiting for it as long
/// as the busy timeout. One a failed rollback left open (see [`rollback`]) is rolled back first.
async fn begin(conn: &libsql::Connection) -> libsql::Result<()> {
    if !conn.is_autocommit() {
        conn.execute("ROLLBACK", ()).await?;
    }
    conn.execute("BEGIN IMMEDIATE", ()).await?;
    Ok(())
}

/// Ends the transaction [`begin`] started, durably. One that can't commit is rolled back.
async fn commit(conn: &libsql::Connection) -> libsql::Result<()> {
    let committed = conn.execute("COMMIT", ()).await.map(drop);
    if committed.is_err() {
        rollback(conn).await;
    }
    committed
}

/// Undoes the transaction [`begin`] started. libsql's own `Transaction` panics when this fails,
/// inside the actor; here it's told, and the next [`begin`] tries again: the save that asked for
/// this has failed already.
async fn rollback(conn: &libsql::Connection) {
    if !conn.is_autocommit() {
        if let Err(e) = conn.execute("ROLLBACK", ()).await {
            warn!(error = %e, "the records' transaction can't be rolled back: tried again by the next save");
        }
    }
}

/// The first column of the first row `sql` gives, if any. Its statement ends with it, as a
/// transaction can't commit while one is under way.
async fn first(conn: &libsql::Connection, sql: &str) -> libsql::Result<Option<libsql::Value>> {
    let mut rows = conn.query(sql, ()).await?;
    rows.next().await?.map(|row| row.get_value(0)).transpose()
}

/// `value` as text, if it's text.
fn text(value: Option<libsql::Value>) -> Option<String> {
    match value? {
        libsql::Value::Text(text) => Some(text),
        _ => None,
    }
}

/// Moves the database at `path` aside, with its write-ahead log: under a name of its own, never
/// over one kept before. Where it went.
fn keep_aside(path: &Path) -> Result<PathBuf> {
    keep_aside_with(path, |from, to| std::fs::rename(from, to))
}

/// [`keep_aside`], moving each file with `rename`. The log and the shared memory go first, the
/// database last: a crash between two leaves no old log beside a new database, which SQLite
/// would replay into it.
fn keep_aside_with(path: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<PathBuf> {
    let dir = path.parent().context("the database is in a folder")?;
    let aside = (1..)
        .map(|n: u32| dir.join(if n == 1 { format!("{INVALID}.db") } else { format!("{INVALID}-{n}.db") }))
        .find(|p| ["", "-wal", "-shm"].iter().all(|suffix| !beside(p, suffix).exists()))
        .expect("a number is free");
    for suffix in ["-shm", "-wal", ""] {
        let (from, to) = (beside(path, suffix), beside(&aside, suffix));
        match rename(&from, &to) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("failed to keep {} aside", from.display()));
            }
            _ => {}
        }
    }
    Ok(aside)
}

/// `path` with `suffix` after its name, as SQLite names a database's log.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// Runs `future` to its end on this thread. libsql's API is async, but its local database does
/// its work on the calling thread, as each future is polled, as the file writes before it did; a
/// save is a step of the actor's that what follows relies on, so it's made where it's asked.
fn wait<F: Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Arc::new(Unpark(std::thread::current())).into();
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(out) => return out,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

/// The records kept in `data_dir`'s database, by info hash, as tests look at them.
#[cfg(test)]
pub fn saved_in(data_dir: &Path) -> BTreeMap<String, Saved> {
    let conn = wait(async { libsql::Builder::new_local(data_dir.join(FILE)).build().await?.connect() }).unwrap();
    wait(async {
        let mut rows = conn.query("SELECT info_hash, record FROM torrents", ()).await.unwrap();
        let mut saved = BTreeMap::new();
        while let Some(row) = rows.next().await.unwrap() {
            saved.insert(row.get::<String>(0).unwrap(), serde_json::from_str(&row.get::<String>(1).unwrap()).unwrap());
        }
        saved
    })
}

/// Writes `bytes` as torrent `hash`'s .torrent in `data_dir`'s database, as a test damages it.
#[cfg(test)]
pub fn set_torrent(data_dir: &Path, hash: &str, bytes: &[u8]) {
    let conn = wait(async { libsql::Builder::new_local(data_dir.join(FILE)).build().await?.connect() }).unwrap();
    let changed =
        wait(conn.execute("UPDATE torrents SET torrent = ?1 WHERE info_hash = ?2", libsql::params![bytes.to_vec(), hash])).unwrap();
    assert_eq!(changed, 1, "{hash} has a row");
}

/// Torrent `hash`'s .torrent as `data_dir`'s database keeps it, as tests look at it.
#[cfg(test)]
pub fn saved_torrent(data_dir: &Path, hash: &str) -> Option<Vec<u8>> {
    let conn = wait(async { libsql::Builder::new_local(data_dir.join(FILE)).build().await?.connect() }).unwrap();
    wait(async {
        let mut rows = conn.query("SELECT torrent FROM torrents WHERE info_hash = ?1", [hash]).await.unwrap();
        match rows.next().await.unwrap()?.get_value(0).unwrap() {
            libsql::Value::Blob(bytes) => Some(bytes),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::*;

    /// A .torrent of two files, `pack/a.bin` and `pack/b.bin`, or of `names`.
    fn torrent_of(names: &[&str]) -> Vec<u8> {
        match super::super::tests::multi_file_torrent(names) {
            proto::AddSource::TorrentFile(bytes) => bytes,
            _ => unreachable!("a .torrent"),
        }
    }

    fn torrent() -> Vec<u8> {
        torrent_of(&["a.bin", "b.bin"])
    }

    fn hash_of(bytes: &[u8]) -> String {
        librqbit::torrent_from_bytes(bytes).unwrap().info_hash.as_string()
    }

    fn load(data: &Path) -> Loaded {
        Store::load(data, &data.join("session")).unwrap()
    }

    /// A record of the torrent in `bytes`, with its info.
    fn record(store: &mut Store, bytes: &[u8], wants: Wants) -> Record {
        let destination = AbsolutePath::new("/tmp/x").unwrap();
        let mut record = Record::new(hash_of(bytes), store.next_id(), Some("pack".into()), destination, vec!["http://t/a".into()], wants);
        record.info = Some(Info::parse(bytes).unwrap());
        record
    }

    /// The files in `dir`, by name.
    fn names(dir: &Path) -> BTreeSet<String> {
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn a_saved_store_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, records, .. } = load(dir.path());
        assert_eq!(records, Vec::new(), "nothing saved yet");
        let mut record = record(&mut store, &torrent(), Wants::Run);
        record.gets = Gets::Only([1].into());
        record.unchecked = true;
        record.missing = [(1, Missing { had: 5, seen: Seen::NotYet })].into();
        record.had = vec![9, 3];
        store.save([&record]).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.records, vec![record.clone()]);
        assert_eq!(loaded.store.next_id, store.next_id);
        assert_eq!(saved_torrent(dir.path(), &record.info_hash), Some(torrent()), "its .torrent, as it was");
        // Gone, its row goes, its .torrent with it.
        store.save([]).unwrap();
        assert_eq!(load(dir.path()).records, Vec::new());
        assert_eq!(saved_torrent(dir.path(), &record.info_hash), None);
    }

    /// A record without its .torrent goes at a start (it was fetching info), but for one librqbit
    /// has a copy of: it had the torrent, and the engine hadn't heard when it stopped. That one
    /// has its info, kept in its row from then on.
    #[test]
    fn a_record_without_info_goes_at_start_unless_librqbit_had_it() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        let destination = AbsolutePath::new("/tmp/x").unwrap();
        let fetching = Record::new("aa".into(), store.next_id(), None, destination.clone(), Vec::new(), Wants::Run);
        let bytes = torrent();
        let heard = Record::new(hash_of(&bytes), store.next_id(), Some("pack".into()), destination, Vec::new(), Wants::Run);
        store.save([&fetching, &heard]).unwrap();
        assert_eq!(saved_in(dir.path()).len(), 2, "kept while they run");
        let session = dir.path().join("session");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join(format!("{}.torrent", heard.info_hash)), &bytes).unwrap();
        let records = load(dir.path()).records;
        assert_eq!(records.iter().map(|r| r.info_hash.as_str()).collect::<Vec<_>>(), [heard.info_hash.as_str()]);
        assert_eq!(records[0].info, Some(Info::parse(&bytes).unwrap()));
        assert_eq!(saved_in(dir.path()).len(), 1, "the other gone from the database");
        std::fs::remove_dir_all(&session).unwrap();
        assert_eq!(saved_torrent(dir.path(), &heard.info_hash), Some(bytes), "kept as Phantom's own");
        assert_eq!(load(dir.path()).records, records);
    }

    /// #334: a crash after a torrent's row went and before librqbit dropped it leaves librqbit's
    /// .torrent behind. The start drops it, so the next torrent of that hash, added and fetching
    /// info when the service stops, isn't taken for one librqbit had.
    #[test]
    fn a_torrent_added_again_after_a_crash_left_librqbits_copy_is_still_fetching_info() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session");
        std::fs::create_dir_all(&session).unwrap();
        let bytes = torrent();
        std::fs::write(session.join(format!("{}.torrent", hash_of(&bytes))), &bytes).unwrap();
        let Loaded { mut store, records, .. } = load(dir.path());
        assert_eq!(records, Vec::new());
        let destination = AbsolutePath::new("/tmp/x").unwrap();
        let again = Record::new(hash_of(&bytes), store.next_id(), None, destination, Vec::new(), Wants::Run);
        store.save([&again]).unwrap();
        assert_eq!(load(dir.path()).records, Vec::new(), "fetching info, it doesn't survive a restart");
    }

    #[test]
    fn fresh_ids_stay_above_ids_taken_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        store.ids_above(4);
        assert_eq!(store.next_id(), 5);
        store.ids_above(2);
        assert_eq!(store.next_id(), 6, "never back down");
        store.save([]).unwrap();
        assert_eq!(load(dir.path()).store.next_id(), 7, "kept");
    }

    /// A .torrent that can't be read keeps its record as it was, listed as unreadable, and its
    /// bytes stay: nothing that can't be read is deleted. Once it reads again, the record has its
    /// info.
    #[test]
    fn a_torrent_that_cant_be_read_keeps_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = torrent();
        let Loaded { mut store, .. } = load(dir.path());
        let mut kept = record(&mut store, &bytes, Wants::Run);
        kept.gets = Gets::Only([1].into());
        store.save([&kept]).unwrap();
        set_torrent(dir.path(), &kept.info_hash, b"d4:info");
        for _ in 0..2 {
            let Loaded { store, records, unreadable, .. } = load(dir.path());
            assert_eq!(records.len(), 1, "kept");
            assert!(records[0].info.is_none());
            assert!(unreadable.contains_key(&kept.info_hash), "{unreadable:?}");
            assert_eq!((records[0].wants, records[0].gets.clone()), (Wants::Run, Gets::Only([1].into())), "as it was");
            store.save(records.iter()).unwrap();
            assert_eq!(saved_torrent(dir.path(), &kept.info_hash).as_deref(), Some(&b"d4:info"[..]), "not deleted");
            assert!(store.read_info(&records[0]).is_err());
        }
        set_torrent(dir.path(), &kept.info_hash, &bytes);
        let Loaded { store, records, .. } = load(dir.path());
        assert_eq!(records[0].info, kept.info, "read again");
        assert_eq!(store.read_info(&records[0]).ok(), kept.info);
    }

    /// A database that can't be read is kept aside as it was, never over one kept before, and the
    /// store starts with none: whatever is at its path (not a database, a database of another
    /// shape).
    #[test]
    fn a_database_that_cant_be_read_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, b"not a database").unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.records, Vec::new());
        assert_eq!(loaded.set_aside, Some(dir.path().join("records.invalid.db")), "and says where");
        assert_eq!(std::fs::read(dir.path().join("records.invalid.db")).unwrap(), b"not a database", "kept as it was");
        // Of another shape: kept aside too, with its log, beside the one before.
        std::fs::remove_file(&path).unwrap();
        let other = wait(async { libsql::Builder::new_local(&path).build().await?.connect() }).unwrap();
        wait(other.execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE torrents (x INTEGER); INSERT INTO torrents VALUES (1);"))
            .unwrap();
        let Loaded { mut store, records, .. } = load(dir.path());
        drop(other);
        assert_eq!(records, Vec::new());
        assert!(names(dir.path()).contains("records.invalid-2.db"), "{:?}", names(dir.path()));
        let aside = wait(async { libsql::Builder::new_local(dir.path().join("records.invalid-2.db")).build().await?.connect() }).unwrap();
        let mut rows = wait(aside.query("SELECT x FROM torrents", ())).unwrap();
        assert_eq!(wait(rows.next()).unwrap().unwrap().get::<i64>(0).unwrap(), 1, "with what it held");
        // The new one works.
        let record = record(&mut store, &torrent(), Wants::Run);
        store.save([&record]).unwrap();
        assert_eq!(load(dir.path()).records, vec![record]);
    }

    /// A row whose record can't be read is left out of the list and stays in the database as it is,
    /// through every save; the others load.
    #[test]
    fn a_record_that_cant_be_read_stays_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        let good = record(&mut store, &torrent(), Wants::Run);
        store.save([&good]).unwrap();
        let conn = wait(async { libsql::Builder::new_local(dir.path().join(FILE)).build().await?.connect() }).unwrap();
        wait(conn.execute("INSERT INTO torrents (info_hash, record) VALUES ('bb', '{\"id\": \"one\"}')", ())).unwrap();
        let Loaded { store, records, .. } = load(dir.path());
        assert_eq!(records, vec![good]);
        store.save([]).unwrap();
        let mut rows = wait(conn.query("SELECT record FROM torrents WHERE info_hash = 'bb'", ())).unwrap();
        assert_eq!(wait(rows.next()).unwrap().unwrap().get::<String>(0).unwrap(), "{\"id\": \"one\"}", "as it was");
    }

    /// Another connection holds a write lock for `hold`, from when this returns: the join handle
    /// ends once it's released.
    fn lock_for(data: &Path, hold: Duration) -> std::thread::JoinHandle<()> {
        let (locked, held) = std::sync::mpsc::channel();
        let path = data.join(FILE);
        let thread = std::thread::spawn(move || {
            let conn = wait(async { libsql::Builder::new_local(path).build().await?.connect() }).unwrap();
            wait(conn.execute_batch("BEGIN IMMEDIATE")).unwrap();
            locked.send(()).unwrap();
            std::thread::sleep(hold);
            wait(conn.execute_batch("COMMIT")).unwrap();
        });
        held.recv().unwrap();
        thread
    }

    /// Another connection saving or closing at a start is waited for, not refused at once.
    #[test]
    fn a_start_waits_for_a_write_lock_held_for_a_moment() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        let kept = record(&mut store, &torrent(), Wants::Run);
        store.save([&kept]).unwrap();
        drop(store);
        let holder = lock_for(dir.path(), Duration::from_millis(150));
        let started = std::time::Instant::now();
        let loaded = load(dir.path());
        assert!(started.elapsed() >= Duration::from_millis(100), "it waited: {:?}", started.elapsed());
        assert_eq!(loaded.records, vec![kept]);
        holder.join().unwrap();
    }

    /// Every file in `dir`, with its bytes.
    fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        names(dir).into_iter().map(|name| (name.clone(), std::fs::read(dir.join(&name)).unwrap_or_default())).collect()
    }

    /// A database another connection holds the write lock of for longer than the wait, and one
    /// that can't be read, aren't damaged: they're left as they are, with no `records.invalid*`,
    /// and load once they can be reached.
    #[test]
    fn a_busy_or_unreachable_database_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        let kept = record(&mut store, &torrent(), Wants::Run);
        store.save([&kept]).unwrap();
        drop(store);
        let before = files(dir.path());

        let holder = lock_for(dir.path(), BUSY_TIMEOUT * 4);
        assert!(Store::load(dir.path(), &dir.path().join("session")).is_err(), "busy");
        assert!(!names(dir.path()).iter().any(|n| n.starts_with("records.invalid")), "{:?}", names(dir.path()));
        assert_eq!(files(dir.path())[FILE], before[FILE], "as it was");
        holder.join().unwrap();
        assert_eq!(load(dir.path()).records, vec![kept.clone()], "once it's free");

        use std::os::unix::fs::PermissionsExt;
        let path = dir.path().join(FILE);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreachable = Store::load(dir.path(), &dir.path().join("session"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(unreachable.is_err(), "unreachable");
        assert!(!names(dir.path()).iter().any(|n| n.starts_with("records.invalid")), "{:?}", names(dir.path()));
        assert_eq!(load(dir.path()).records, vec![kept], "once it's readable");
    }

    /// Only a database that is bad itself is judged damaged, by SQLite's result code, extended
    /// ones too.
    #[test]
    fn only_corrupt_or_foreign_results_judge_a_database_damaged() {
        let (busy, locked, readonly, ioerr, full, cantopen) = (5, 6, 8, 10, 13, 14);
        for code in [busy, locked, readonly, ioerr, full, cantopen, 3 /* perm */, 5 | 2 << 8, 13 | 1 << 8, 10 | 3 << 8] {
            assert!(!damaged(code), "{code} leaves the database as it is");
            assert!(matches!(NotOpened::from(libsql::Error::SqliteFailure(code, String::new())), NotOpened::Failed(_)));
        }
        for code in [1, 11, 26, 11 | 1 << 8] {
            assert!(damaged(code), "{code}");
            assert!(matches!(NotOpened::from(libsql::Error::SqliteFailure(code, String::new())), NotOpened::Damaged(_)));
        }
    }

    /// A rollback that failed leaves its transaction open: the next save ends it and goes on,
    /// as does one for a save that failed. (libsql's own `Transaction` would panic the actor.)
    #[test]
    fn a_transaction_left_open_is_ended_by_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        let kept = record(&mut store, &torrent(), Wants::Run);
        wait(store.conn.execute("BEGIN IMMEDIATE", ())).unwrap();
        store.save([&kept]).unwrap();
        assert!(store.conn.is_autocommit());
        assert_eq!(load(dir.path()).records, vec![kept]);
    }

    /// A crash between moving the files aside never leaves the old log beside a new database.
    #[test]
    fn a_crash_while_a_database_is_set_aside_leaves_no_old_log_beside_it() {
        for crash_at in 0..4 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(FILE);
            for suffix in ["", "-wal", "-shm"] {
                std::fs::write(beside(&path, suffix), suffix).unwrap();
            }
            let moved = std::cell::Cell::new(0);
            let result = keep_aside_with(&path, |from, to| {
                if moved.replace(moved.get() + 1) == crash_at {
                    return Err(std::io::Error::other("crashed"));
                }
                std::fs::rename(from, to)
            });
            assert_eq!(result.is_ok(), crash_at == 3, "{crash_at}");
            assert!(path.exists() || !beside(&path, "-wal").exists(), "after {crash_at} renames: {:?}", names(dir.path()));
        }
    }

    /// A failure is made for the next save only (#309): one with nothing to write has no statement
    /// to fail at and succeeds, and the save after it, raising the next id, writes it.
    #[test]
    fn a_failure_made_for_a_save_with_nothing_to_write_is_not_left_for_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let Loaded { mut store, .. } = load(dir.path());
        store.save([]).unwrap();
        store.fail_next_save_after(0);
        store.save([]).unwrap();
        store.next_id();
        store.save([]).unwrap();
        assert_eq!(load(dir.path()).store.next_id, 1);
    }

    /// What one save of a generated sequence does: which of three torrents are in the list, each
    /// with what's wanted and its seeding round; ids given; and whether the save fails partway (as
    /// on a full disk, after so many statements).
    #[derive(Debug, Clone)]
    struct Step {
        torrents: Vec<Option<(Wants, u64)>>,
        ids: u64,
        fails_after: Option<usize>,
    }

    fn step() -> impl Strategy<Value = Step> {
        let torrent = proptest::option::of((proptest::sample::select(lifecycle::samples::wants()), 0..3u64));
        (proptest::collection::vec(torrent, 3), 0..2u64, proptest::option::of(0..4usize)).prop_map(|(torrents, ids, fails_after)| Step {
            torrents,
            ids,
            fails_after,
        })
    }

    proptest! {
        // Every save syncs to the disk: 32 cases per `cargo test`, or `PROPTEST_CASES`.
        #![proptest_config(ProptestConfig {
            cases: std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32),
            ..ProptestConfig::default()
        })]

        /// Rollback, not a crash: after any sequence of saves, some failing partway (and rolled
        /// back, never left open), the database holds what the last save
        /// that didn't fail was given, whole: never part of a failed one, never less of one that
        /// followed it (a failed save's changes are written by the next).
        #[test]
        fn a_failed_save_rolls_back_whole_and_the_next_one_writes_what_it_missed(steps in proptest::collection::vec(step(), 1..8)) {
            let dir = tempfile::tempdir().unwrap();
            let Loaded { mut store, .. } = load(dir.path());
            let bytes: Vec<Vec<u8>> = (0..3).map(|n| torrent_of(&[&format!("{n}.bin")])).collect();
            let mut records: Vec<Record> = bytes.iter().map(|b| record(&mut store, b, Wants::Run)).collect();
            let mut kept = (Vec::new(), 0);
            for step in steps {
                let mut now = Vec::new();
                for (record, torrent) in records.iter_mut().zip(&step.torrents) {
                    if let Some((wants, uploaded)) = torrent {
                        record.wants = *wants;
                        record.round = lifecycle::seeding::Round { uploaded: *uploaded, seeded: Duration::from_secs(*uploaded) };
                        now.push(record.clone());
                    }
                }
                for _ in 0..step.ids {
                    store.next_id();
                }
                if let Some(n) = step.fails_after {
                    store.fail_next_save_after(n);
                }
                let saved = store.save(now.iter());
                prop_assert!(saved.is_ok() || step.fails_after.is_some(), "a save that wasn't made to fail did: {saved:?}");
                prop_assert!(store.conn.is_autocommit(), "no transaction is left open");
                if saved.is_ok() {
                    kept = (now, store.next_id);
                }
                let loaded = Store::open(&dir.path().join(FILE), &dir.path().join("session")).map_err(|_| "opens").unwrap();
                let sorted = |mut r: Vec<Record>| { r.sort_by(|a, b| a.info_hash.cmp(&b.info_hash)); r };
                prop_assert_eq!(sorted(loaded.records), sorted(kept.0.clone()));
                prop_assert_eq!(loaded.store.next_id, kept.1);
            }
        }

        /// What's wanted, stopped at a seeding limit too, and the seeding round load back as saved.
        #[test]
        fn wants_and_the_seeding_round_load_back(wants in proptest::sample::select(lifecycle::samples::wants()), uploaded in 0..u64::MAX / 2, secs in 0..u64::MAX / 4) {
            let dir = tempfile::tempdir().unwrap();
            let Loaded { mut store, .. } = load(dir.path());
            let mut record = record(&mut store, &torrent(), wants);
            record.round = lifecycle::seeding::Round { uploaded, seeded: Duration::from_secs(secs) };
            store.save([&record]).unwrap();
            prop_assert_eq!(load(dir.path()).records, vec![record]);
        }
    }
}
