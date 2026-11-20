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
//! `records.invalid.db`, and the service starts with none, which the log says. A row whose record
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

use super::files::Gone;
use super::machine::{Wants, letgo};
use super::trash::Trashed;
use super::undo::Stash;

const FILE: &str = "records.db";
/// A database that couldn't be read, kept as it was (with a number when one is there already).
const INVALID: &str = "records.invalid";

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
    /// What it did in its seeding round ([`lifecycle::seeding`]): librqbit forgets what it
    /// uploaded at every stop. Kept at the regular size check and when the service stops.
    pub round: lifecycle::seeding::Round,
    /// The files that were missing (in Missing files) when the service stopped, by index, with
    /// what librqbit had of each before it went. Taken on start.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub missing_at_shutdown: BTreeMap<usize, u64>,
    /// What librqbit had of each file, by index, when the service stopped: the first size check at
    /// the next start, before librqbit has it again, judges its files by it. Taken on start, so a
    /// crash, which skips the stop, judges them by their paths alone.
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
    pub destination: AbsolutePath,
    /// Its info, once it arrived.
    pub info: Option<Info>,
    pub trackers: Vec<String>,
    /// The files it gets (those Normal), by index.
    pub gets: Gets,
    pub wants: Wants,
    pub unchecked: bool,
    pub round: lifecycle::seeding::Round,
    pub missing_at_shutdown: BTreeMap<usize, u64>,
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
            destination,
            trackers,
            normal,
            wants,
            unchecked,
            round,
            missing_at_shutdown,
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
            destination,
            info,
            trackers,
            gets,
            wants,
            unchecked,
            round,
            missing_at_shutdown,
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
            destination,
            info: None,
            trackers,
            gets: Gets::Every,
            wants,
            unchecked: false,
            round: Default::default(),
            missing_at_shutdown: BTreeMap::new(),
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
            destination: self.destination.clone(),
            trackers: self.trackers.clone(),
            normal: match &self.gets {
                Gets::Every => None,
                Gets::Only(only) => Some(only.clone()),
            },
            wants: self.wants,
            unchecked: self.unchecked,
            round: self.round,
            missing_at_shutdown: self.missing_at_shutdown.clone(),
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

    /// The folder librqbit puts its files in: its destination, or its own folder there.
    pub fn output_folder(&self) -> Option<PathBuf> {
        let info = self.info.as_ref()?;
        Some(match info.own_folder(self.name.as_deref()) {
            Some(own) => self.destination.join(own),
            None => self.destination.to_path_buf(),
        })
    }

    /// Where it is on disk: the file itself for a torrent of one file, its own folder otherwise.
    pub fn location(&self) -> Option<PathBuf> {
        let info = self.info.as_ref()?;
        let folder = self.output_folder()?;
        Some(match &info.files[..] {
            [only] => folder.join(&only.path),
            _ => folder,
        })
    }

    /// Each file's path, by index.
    pub fn paths(&self) -> Vec<PathBuf> {
        let (Some(info), Some(folder)) = (&self.info, self.output_folder()) else { return Vec::new() };
        info.files.iter().map(|f| folder.join(&f.path)).collect()
    }

    /// The files missing at start, with what librqbit had of each, not judged yet: what's at their
    /// paths then is taken for what was there when they went.
    pub fn gone_at_start(&self) -> BTreeMap<usize, Gone> {
        let (Some(info), paths) = (&self.info, self.paths()) else { return BTreeMap::new() };
        self.missing_at_shutdown
            .iter()
            .filter(|(i, _)| self.gets(**i))
            .filter_map(|(&i, &progress_bytes)| {
                let f = info.files.get(i)?;
                Some((i, Gone { path: paths[i].clone(), size_bytes: f.len, progress_bytes, seen: lifecycle::disk::Seen::NotYet }))
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
        // SQLITE_CORRUPT, SQLITE_NOTADB; and SQLITE_ERROR, which a statement of the store's meets
        // only in tables of another shape.
        match e {
            libsql::Error::SqliteFailure(code, _) if [1, 11, 26].contains(&(code & 0xff)) => Self::Damaged(e.into()),
            e => Self::Failed(e.into()),
        }
    }
}

impl Store {
    /// Loads the records in `dir`. A record without its .torrent takes librqbit's copy in its
    /// `session` folder: librqbit had the torrent, and the engine hadn't heard yet when it
    /// stopped. Others whose info never arrived go: they were fetching info, and librqbit never
    /// had them. A database that can't be read is kept aside as it was, and the store starts with
    /// none; one that can't be reached fails.
    pub fn load(dir: &Path, session: &Path) -> Result<Loaded> {
        let path = dir.join(FILE);
        let loaded = match Self::open(&path, session) {
            Ok(loaded) => loaded,
            Err(NotOpened::Damaged(e)) => {
                let aside = keep_aside(&path)?;
                warn!(error = %format!("{e:#}"), kept = %aside.display(), "the torrents' records can't be read: kept aside as they were, starting with none");
                Self::open(&path, session).map_err(|e| match e {
                    NotOpened::Damaged(e) | NotOpened::Failed(e) => e,
                })?
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
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
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
            conn.execute_transactional_batch(SCHEMA).await?;
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
        Ok(Loaded { store, records, unreadable })
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
        #[cfg(test)]
        let mut statements = self.fails_after.lock().unwrap().take();
        wait(async {
            #[cfg(test)]
            let mut statement = || match statements.as_mut() {
                Some(0) => Err(anyhow::anyhow!("the disk is full")),
                Some(n) => Ok(*n -= 1),
                None => Ok(()),
            };
            let tx = self.conn.transaction_with_behavior(libsql::TransactionBehavior::Immediate).await?;
            for (hash, json, torrent) in &changed {
                #[cfg(test)]
                statement()?;
                let torrent = torrent.as_ref().map_or(libsql::Value::Null, |t| libsql::Value::Blob(t.to_vec()));
                tx.execute(
                    "INSERT INTO torrents (info_hash, record, torrent) VALUES (?1, ?2, ?3)
                     ON CONFLICT (info_hash) DO UPDATE SET record = excluded.record, torrent = coalesce(excluded.torrent, torrent)",
                    libsql::params![hash.as_str(), json.as_str(), torrent],
                )
                .await?;
            }
            for hash in &gone {
                #[cfg(test)]
                statement()?;
                tx.execute("DELETE FROM torrents WHERE info_hash = ?1", [hash.as_str()]).await?;
            }
            if next.next_id != written.next_id {
                #[cfg(test)]
                statement()?;
                tx.execute("UPDATE ids SET next_id = ?1", [next.next_id]).await?;
            }
            tx.commit().await?;
            anyhow::Ok(())
        })
        .with_context(|| format!("failed to save the records in {}", self.path.display()))?;
        *written = next;
        Ok(())
    }

    /// Makes the next save fail after `statements` of its statements, and roll back.
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
    let path = session.join(format!("{hash}.torrent"));
    match std::fs::read(&path) {
        Ok(bytes) => Some(Ok(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!("failed to read {}: {e}", path.display()))),
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
    let dir = path.parent().context("the database is in a folder")?;
    let aside = (1..)
        .map(|n: u32| dir.join(if n == 1 { format!("{INVALID}.db") } else { format!("{INVALID}-{n}.db") }))
        .find(|p| !p.exists() && !beside(p, "-wal").exists())
        .expect("a number is free");
    for suffix in ["", "-wal", "-shm"] {
        let (from, to) = (beside(path, suffix), beside(&aside, suffix));
        match std::fs::rename(&from, &to) {
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
        record.missing_at_shutdown = [(1, 5)].into();
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
            let Loaded { store, records, unreadable } = load(dir.path());
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
        assert_eq!(load(dir.path()).records, Vec::new());
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
        /// After any sequence of saves, some failing partway, the database holds what the last save
        /// that didn't fail was given, whole: never part of a failed one, never less of one that
        /// followed it (a failed save's changes are written by the next).
        #[test]
        fn the_database_holds_the_last_save_whole(steps in proptest::collection::vec(step(), 1..8)) {
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
                if store.save(now.iter()).is_ok() {
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
