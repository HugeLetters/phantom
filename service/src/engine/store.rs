//! Phantom's record of each torrent (#45): the source of truth, from which the engine adds it to
//! librqbit itself, at a start too. `engine.json` in the data dir: per torrent, by info hash, its
//! id, name, destination, trackers, the files it gets (those Normal), whether it's wanted
//! running, and what's under way on it; plus the next id to give. Each torrent's .torrent is kept
//! beside it, in `torrents/<hash>.torrent`, written once, when its info arrives. Written whole on
//! every change, through a temporary file and a rename, so a crash never leaves half a file.
//!
//! [`Saved`] is a record as `engine.json` and `undo.json` keep it; [`Record`] one parsed from it,
//! with its info, which the engine works with.
//!
//! What came before is taken over once: `ids.json` (the ids alone), and librqbit's own
//! `session.json` with its .torrent copies (where each torrent's files went, which it got), which
//! [`Store::load`] moves into the records and puts aside (`session_db.rs`), so librqbit restores
//! nothing. A record's .torrent is looked for here, then in librqbit's copy, then in what Undo or a
//! let-go kept of it; one that can't be read keeps its record, as it was, and is listed as an
//! error until a check reads it. Nothing that can't be read is deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use proto::{AbsolutePath, TorrentId};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::files::Gone;
use super::machine::{Wants, letgo};
use super::session_db;
use super::trash::Trashed;
use super::undo::Stash;

const FILE: &str = "engine.json";
/// The store before `engine.json`: just `next` and `ids` (info hash to id).
const OLD_IDS_FILE: &str = "ids.json";
/// Each torrent's .torrent, by info hash.
const INFOS: &str = "torrents";

/// The ids given so far, and where the records are kept.
#[derive(Debug, PartialEq)]
pub struct Store {
    dir: PathBuf,
    /// librqbit's session folder.
    session: PathBuf,
    next_id: TorrentId,
}

/// `engine.json` as it's written.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
struct File {
    next_id: TorrentId,
    /// By info hash (hex).
    torrents: BTreeMap<String, Saved>,
}

/// A torrent's record as it's kept, in `engine.json` and in `undo.json` (an entry that brings a
/// torrent back). What older builds wrote reads too.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Saved {
    pub id: TorrentId,
    /// From the magnet's `dn=`, the .torrent, or its info once it arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The folder the torrent's files go into; a torrent of several files has its own folder
    /// inside it. Given by the add. Older builds may not have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<AbsolutePath>,
    /// The trackers it names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trackers: Vec<String>,
    /// The files it gets (those Normal), by index; `None`: every one (as before #45).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<BTreeSet<usize>>,
    /// Before #45: priorities asked for during a check, applied once it was done. Read once, as
    /// `normal`.
    #[serde(default, skip_serializing)]
    held: Option<BTreeSet<usize>>,
    /// Run or stop, as the user last said (or stopped at a seeding limit): kept whenever it changes.
    #[serde(default)]
    pub wants: Wants,
    /// Its check was cancelled and nothing has checked it since ([`Phase::Unchecked`](super::machine::Phase::Unchecked)):
    /// it comes back so at start, not Ready with progress nobody checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unchecked: bool,
    /// What it did in its seeding round ([`lifecycle::seeding`]): librqbit forgets what it
    /// uploaded at every stop. Kept at the regular size check and when the service stops.
    #[serde(default)]
    pub round: lifecycle::seeding::Round,
    /// Before `wants`: it was running when the service stopped. Read once.
    #[serde(default, skip_serializing)]
    running_at_shutdown: bool,
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

impl Record {
    /// `saved`, of torrent `info_hash`, with its `info`: none without a destination, which only a
    /// record older than every add naming one lacks. `normal`: the files it gets if `saved` doesn't
    /// say (librqbit's, from before #45); else every one.
    pub fn parse(info_hash: &str, saved: Saved, info: Option<Info>, normal: Option<Vec<usize>>) -> Option<Self> {
        let Saved {
            id,
            name,
            destination,
            trackers,
            normal: saved_normal,
            held,
            mut wants,
            unchecked,
            round,
            running_at_shutdown,
            missing_at_shutdown,
            had,
            delete_failed,
            deleting,
            trashed,
            stash,
            letting_go,
            moving,
        } = saved;
        if running_at_shutdown {
            wants = Wants::Run;
        }
        let gets = match saved_normal.or(held).or_else(|| normal.map(|n| n.into_iter().collect())) {
            Some(only) => Gets::Only(only),
            None => Gets::Every,
        };
        Some(Self {
            info_hash: info_hash.to_owned(),
            id,
            name: name.or_else(|| info.as_ref().and_then(|i| i.name.clone())),
            destination: destination?,
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
        })
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
        Self::parse(&info_hash, Saved { id, name, destination: Some(destination), trackers, wants, ..Default::default() }, None, None)
            .expect("it has a destination")
    }

    /// As it's kept.
    pub fn saved(&self) -> Saved {
        Saved {
            id: self.id,
            name: self.name.clone(),
            destination: Some(self.destination.clone()),
            trackers: self.trackers.clone(),
            normal: match &self.gets {
                Gets::Every => None,
                Gets::Only(only) => Some(only.clone()),
            },
            held: None,
            wants: self.wants,
            unchecked: self.unchecked,
            round: self.round,
            running_at_shutdown: false,
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

/// Where a record's .torrent was looked for.
enum Found {
    Info(Info),
    /// Nowhere: its info never arrived.
    Nowhere,
    /// Somewhere, but it can't be read there: why.
    Unreadable(String),
}

impl Store {
    /// Loads the records in `dir`, taking over what older builds kept (`ids.json`, and, once,
    /// librqbit's `session.json` in `session`): each torrent librqbit had gets a record from it, but
    /// those `gone` names (a Remove or a Delete whose undo brings it back, which a crash cut short
    /// before librqbit forgot it). Then librqbit's own record of them goes, so it restores nothing;
    /// the pieces it saved stay, and the engine adds each torrent with them. Records whose info never
    /// arrived go: librqbit never had them.
    pub fn load(dir: &Path, session: &Path, gone: impl Fn(&str) -> bool) -> Result<Loaded> {
        let file = match read(&dir.join(FILE))? {
            Some(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("invalid {FILE}"))?,
            None => match read(&dir.join(OLD_IDS_FILE))? {
                Some(bytes) => {
                    #[derive(Deserialize, Default)]
                    #[serde(default)]
                    struct OldIds {
                        next: TorrentId,
                        ids: BTreeMap<String, TorrentId>,
                    }
                    let old: OldIds = serde_json::from_slice(&bytes).with_context(|| format!("invalid {OLD_IDS_FILE}"))?;
                    let torrents = old.ids.into_iter().map(|(hash, id)| (hash, Saved { id, ..Default::default() })).collect();
                    File { next_id: old.next, torrents }
                }
                None => File::default(),
            },
        };
        let mut store = Self { dir: dir.to_owned(), session: session.to_owned(), next_id: file.next_id };
        let taken_over = session_db::taken_over(session);
        let mut librqbit = if taken_over { session_db::Session::default() } else { session_db::restored(session)? };
        let (mut records, mut unreadable) = (Vec::new(), BTreeMap::new());
        for (hash, saved) in file.torrents {
            let old = librqbit.torrents.remove(&hash);
            let info = match store.find_info(&hash, &saved) {
                Found::Info(info) => Some(info),
                // Fetching info when the service stopped: librqbit never had it.
                Found::Nowhere => continue,
                Found::Unreadable(why) => {
                    warn!(hash, why, "can't read the torrent's .torrent: listed as an error, until a check reads it");
                    unreadable.insert(hash.clone(), why);
                    None
                }
            };
            let destination = saved.destination.clone().or_else(|| old.as_ref().and_then(|o| o.destination(info.as_ref()?)));
            let normal = old.and_then(|o| o.only_files);
            match Record::parse(&hash, Saved { destination, ..saved }, info, normal) {
                Some(record) => records.push(record),
                None => warn!(hash, "the torrent's record has no destination: left out, with what librqbit kept of it"),
            }
        }
        // librqbit had it, and Phantom no record of it: a fresh id. Not one that left the list, which
        // only a crash left with librqbit.
        for (hash, old) in librqbit.torrents.into_iter().filter(|(hash, _)| !gone(hash)) {
            let saved = Saved { id: store.next_id, trackers: old.trackers.clone(), ..Default::default() };
            let Found::Info(info) = store.find_info(&hash, &saved) else {
                warn!(hash, "a torrent of librqbit's has no .torrent that can be read: left out, with what librqbit kept of it");
                continue;
            };
            let Some(destination) = old.destination(&info) else { continue };
            store.next_id += 1;
            records.extend(Record::parse(&hash, Saved { destination: Some(destination), ..saved }, Some(info), old.only_files));
        }
        store.save(records.iter())?;
        if !taken_over {
            session_db::put_aside(session, librqbit.invalid)?;
        }
        // Its copies of the .torrents read here go; one only it has, or that can't be read here,
        // stays. Nothing goes of one some of which can't be read.
        if taken_over || !librqbit.invalid {
            let read: std::collections::BTreeSet<&str> =
                records.iter().filter(|r| r.info.is_some()).map(|r| r.info_hash.as_str()).collect();
            session_db::forget(session, |hash| read.contains(hash));
        }
        Ok(Loaded { store, records, unreadable })
    }

    /// Torrent `hash`'s info: from its .torrent kept here, else from librqbit's copy, else from what
    /// Undo or a let-go kept of it (`saved`); one found elsewhere is kept here from now on.
    fn find_info(&self, hash: &str, saved: &Saved) -> Found {
        let kept = saved.stash.iter().chain(match &saved.letting_go {
            super::machine::letgo::State::Stashed { stash, .. } => Some(stash),
            super::machine::letgo::State::Holding | super::machine::letgo::State::Gone { .. } => None,
        });
        let places = std::iter::once(self.info_path(hash))
            .chain(std::iter::once(session_db::torrent_path(&self.session, hash)))
            .chain(kept.map(|stash| stash.dir.join("torrent")));
        let mut why = None;
        for (n, place) in places.enumerate() {
            let bytes = match read(&place) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    why.get_or_insert_with(|| format!("{e:#}"));
                    continue;
                }
            };
            match Info::parse(&bytes) {
                Ok(info) => {
                    if n > 0
                        && let Err(e) = self.keep_info(hash, &bytes)
                    {
                        warn!(hash, error = %e, "failed to keep the torrent's .torrent");
                    }
                    return Found::Info(info);
                }
                Err(e) => drop(why.get_or_insert_with(|| format!("{} isn't a .torrent: {e:#}", place.display()))),
            }
        }
        match why {
            Some(why) => Found::Unreadable(why),
            None => Found::Nowhere,
        }
    }

    /// Torrent `record`'s info, read again: from the places [`Store::load`] looks, for a record
    /// whose .torrent couldn't be read. Why not, if it still can't.
    pub fn read_info(&self, record: &Record) -> Result<Info, String> {
        match self.find_info(&record.info_hash, &record.saved()) {
            Found::Info(info) => Ok(info),
            Found::Unreadable(why) => Err(why),
            Found::Nowhere => Err("its .torrent is gone".into()),
        }
    }

    fn info_path(&self, hash: &str) -> PathBuf {
        self.dir.join(INFOS).join(format!("{hash}.torrent"))
    }

    /// Keeps torrent `hash`'s .torrent, before a record says it has info.
    pub fn keep_info(&self, hash: &str, bytes: &[u8]) -> Result<()> {
        let folder = self.dir.join(INFOS);
        std::fs::create_dir_all(&folder).with_context(|| format!("failed to make {}", folder.display()))?;
        let mut tmp = tempfile::NamedTempFile::new_in(&folder)?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(self.info_path(hash))?;
        Ok(())
    }

    /// Lets go of torrent `hash`'s .torrent, once its record has gone.
    pub fn forget_info(&self, hash: &str) {
        match std::fs::remove_file(self.info_path(hash)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => warn!(hash, error = %e, "failed to remove a torrent's .torrent"),
            _ => {}
        }
    }

    /// Writes `engine.json` with `records` (and drops `ids.json`, which it replaces).
    pub fn save<'a>(&self, records: impl IntoIterator<Item = &'a Record>) -> Result<()> {
        let file = File { next_id: self.next_id, torrents: records.into_iter().map(|r| (r.info_hash.clone(), r.saved())).collect() };
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        tmp.write_all(&serde_json::to_vec_pretty(&file)?)?;
        tmp.as_file().sync_all()?;
        tmp.persist(self.dir.join(FILE))?;
        match std::fs::remove_file(self.dir.join(OLD_IDS_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
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

impl session_db::Restored {
    /// The destination of a torrent librqbit had, from its output folder: the folder itself for
    /// a torrent of one file, the one it's in for several.
    fn destination(&self, info: &Info) -> Option<AbsolutePath> {
        // librqbit joins an empty folder name for a single file, which leaves a trailing slash.
        let folder: PathBuf = self.output_folder.components().collect();
        let folder = if info.single() { folder } else { folder.parent()?.to_owned() };
        AbsolutePath::new(folder).ok()
    }
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A .torrent of two files, `pack/a.bin` and `pack/b.bin`.
    fn torrent() -> Vec<u8> {
        match super::super::tests::multi_file_torrent(&["a.bin", "b.bin"]) {
            proto::AddSource::TorrentFile(bytes) => bytes,
            _ => unreachable!("a .torrent"),
        }
    }

    /// A .torrent of one file, `one.bin`.
    fn single() -> Vec<u8> {
        let mut file = b"d4:infod6:lengthi16384e4:name7:one.bin12:piece lengthi16384e6:pieces20:".to_vec();
        file.extend([7; 20]);
        file.extend(b"ee");
        file
    }

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (data, session) = (dir.path().join("data"), dir.path().join("data/session"));
        std::fs::create_dir_all(&session).unwrap();
        (dir, data, session)
    }

    fn hash_of(bytes: &[u8]) -> String {
        librqbit::torrent_from_bytes(bytes).unwrap().info_hash.as_string()
    }

    /// [`Store::load`], with no torrent that left the list.
    fn load(data: &Path, session: &Path) -> Loaded {
        Store::load(data, session, |_| false).unwrap()
    }

    /// librqbit's `session.json`, of these torrents: info hash, output folder, the files it gets.
    fn session_json(session: &Path, torrents: &[(&str, &str, &str)]) {
        let entries: Vec<String> = torrents
            .iter()
            .enumerate()
            .map(|(n, (hash, folder, only))| {
                format!(r#""{n}": {{"info_hash": "{hash}", "trackers": ["http://t/a"], "output_folder": "{folder}", "only_files": {only}, "is_paused": true}}"#)
            })
            .collect();
        std::fs::write(session.join("session.json"), format!(r#"{{"torrents": {{{}}}}}"#, entries.join(", "))).unwrap();
    }

    #[test]
    fn ids_json_is_taken_over_for_the_torrents_librqbit_had() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::write(data.join("ids.json"), format!(r#"{{"next": 5, "ids": {{"{hash}": 3}}}}"#)).unwrap();
        std::fs::write(session.join(format!("{hash}.torrent")), &bytes).unwrap();
        session_json(&session, &[(&hash, "/tmp/x/pack", "null")]);
        let Loaded { mut store, records, .. } = load(&data, &session);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 3);
        assert_eq!(records[0].destination, AbsolutePath::new("/tmp/x").unwrap(), "librqbit's folder, less the torrent's own");
        assert_eq!(records[0].output_folder(), Some(PathBuf::from("/tmp/x/pack")));
        assert_eq!(store.next_id(), 5);
        assert!(!data.join("ids.json").exists(), "replaced");
        assert!(!session.join("session.json").exists() && !session.join(format!("{hash}.torrent")).exists(), "librqbit restores nothing");
        assert!(session.join("session.migrated.json").exists(), "put aside, taken over");
        assert_eq!(load(&data, &session).records, records, "kept as Phantom's own");
    }

    /// The files master had Off (librqbit's `only_files`, the record saying nothing) stay Off.
    #[test]
    fn masters_off_files_stay_off() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::write(session.join(format!("{hash}.torrent")), &bytes).unwrap();
        session_json(&session, &[(&hash, "/tmp/x/pack", "[0]")]);
        let old = format!(r#"{{"next_id": 1, "torrents": {{"{hash}": {{"id": 0, "destination": "/tmp/x"}}}}}}"#);
        std::fs::write(data.join("engine.json"), old).unwrap();
        let records = load(&data, &session).records;
        assert_eq!(records[0].normal(), [0].into());
        assert_eq!(load(&data, &session).records[0].normal(), [0].into(), "kept so");
    }

    /// A torrent only librqbit knew gets a record: a fresh id, stopped, its destination from
    /// librqbit's folder, of one file or several; one a crash left with librqbit as it left the list
    /// (which its undo brings back) doesn't.
    #[test]
    fn torrents_only_librqbit_knew_are_taken_over() {
        let (_dir, data, session) = dirs();
        let (several, one, removed) = (torrent(), single(), super::super::tests::multi_file_torrent(&["c.bin"]));
        let proto::AddSource::TorrentFile(removed) = removed else { unreachable!() };
        let hashes = [hash_of(&several), hash_of(&one), hash_of(&removed)];
        for (hash, bytes) in hashes.iter().zip([&several, &one, &removed]) {
            std::fs::write(session.join(format!("{hash}.torrent")), bytes).unwrap();
        }
        session_json(
            &session,
            &[(&hashes[0], "/tmp/x/pack", "null"), (&hashes[1], "/tmp/y/", "null"), (&hashes[2], "/tmp/z/pack", "null")],
        );
        std::fs::write(data.join("engine.json"), r#"{"next_id": 7, "torrents": {}}"#).unwrap();
        let Loaded { mut store, records, .. } = Store::load(&data, &session, |hash| hash == hashes[2]).unwrap();
        let by = |hash: &str| records.iter().find(|r| r.info_hash == hash).cloned();
        let (several, one) = (by(&hashes[0]).expect("taken over"), by(&hashes[1]).expect("taken over"));
        assert!(by(&hashes[2]).is_none(), "left the list: not taken over");
        assert!(several.id >= 7 && one.id >= 7 && several.id != one.id, "fresh ids: {} {}", several.id, one.id);
        assert!(store.next_id() > several.id.max(one.id));
        assert_eq!((several.wants, one.wants), (Wants::Stop, Wants::Stop));
        assert_eq!(several.destination, AbsolutePath::new("/tmp/x").unwrap());
        assert_eq!(one.destination, AbsolutePath::new("/tmp/y").unwrap());
        assert_eq!(several.trackers, ["http://t/a"]);
    }

    /// librqbit's `session.json` is taken over once: one it writes as it runs is never read, so
    /// a torrent a crash left with it as it left the list doesn't come back.
    #[test]
    fn session_json_is_taken_over_once() {
        let (_dir, data, session) = dirs();
        load(&data, &session);
        assert!(session.join("session.migrated.json").exists(), "nothing to take over, marked all the same");
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::write(session.join(format!("{hash}.torrent")), &bytes).unwrap();
        session_json(&session, &[(&hash, "/tmp/x/pack", "null")]);
        assert_eq!(load(&data, &session).records, Vec::new(), "librqbit's own isn't read");
        assert!(!session.join("session.json").exists(), "and librqbit restores nothing");
    }

    /// A `session.json` some of which can't be read is kept aside as it was, and nothing librqbit
    /// kept is deleted; what can be read is taken over.
    #[test]
    fn a_session_json_that_cant_be_read_is_kept() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::write(session.join(format!("{hash}.torrent")), &bytes).unwrap();
        let db = format!(
            r#"{{"torrents": {{"0": {{"info_hash": "{hash}", "trackers": [], "output_folder": "/tmp/x/pack", "only_files": null, "is_paused": true}}, "1": {{"info_hash": "bb"}}}}}}"#
        );
        std::fs::write(session.join("session.json"), &db).unwrap();
        let records = load(&data, &session).records;
        assert_eq!(records.len(), 1, "what can be read");
        assert_eq!(std::fs::read_to_string(session.join("session.invalid.json")).unwrap(), db, "kept as it was");
        assert!(session.join(format!("{hash}.torrent")).exists(), "nothing deleted");
        // Not at all: kept so too.
        let (_dir, data, session) = dirs();
        std::fs::write(session.join("session.json"), b"{not json").unwrap();
        assert_eq!(load(&data, &session).records, Vec::new());
        assert_eq!(std::fs::read(session.join("session.invalid.json")).unwrap(), b"{not json");
    }

    /// Reviewer's repro on #244: master's let-go a crash cut short once librqbit had let go of the
    /// torrent left its .torrent only in the let-go's stash, and nothing in librqbit's session. It's
    /// taken over from there.
    #[test]
    fn a_torrent_masters_let_go_kept_aside_is_taken_over_from_the_stash() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        let stash = data.join("undo/kept");
        std::fs::create_dir_all(&stash).unwrap();
        std::fs::write(stash.join("torrent"), &bytes).unwrap();
        let old = format!(
            r#"{{"next_id": 1, "torrents": {{"{hash}": {{"id": 0, "destination": "/tmp/x", "letting_go": {{"stashed": {{"pieces": {{"0": [0, 1]}}, "stash": {{"dir": "{}", "output_folder": "/tmp/x/pack", "only_files": null, "librqbit_id": 0}}}}}}}}}}}}"#,
            stash.display()
        );
        std::fs::write(data.join("engine.json"), old).unwrap();
        let records = load(&data, &session).records;
        assert_eq!(records.len(), 1, "listed from the stash");
        assert!(records[0].info.is_some());
        assert!(data.join(format!("torrents/{hash}.torrent")).exists(), "kept as Phantom's own from now on");
        assert!(matches!(records[0].letting_go, letgo::State::Stashed { .. }), "its let-go left for the start to finish");
    }

    /// A .torrent that can't be read keeps its record as it was, listed as unreadable, and the file
    /// stays: nothing that can't be read is deleted. Once it reads again, the record has its info.
    #[test]
    fn a_torrent_that_cant_be_read_keeps_its_record() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::create_dir_all(data.join("torrents")).unwrap();
        let kept = data.join(format!("torrents/{hash}.torrent"));
        std::fs::write(&kept, b"d4:info").unwrap();
        std::fs::write(session.join(format!("{hash}.torrent")), b"d4:inf").unwrap();
        let old =
            format!(r#"{{"next_id": 1, "torrents": {{"{hash}": {{"id": 0, "destination": "/tmp/x", "wants": "run", "normal": [1]}}}}}}"#);
        std::fs::write(data.join("engine.json"), &old).unwrap();
        for _ in 0..2 {
            let Loaded { store, records, unreadable } = load(&data, &session);
            assert_eq!(records.len(), 1, "kept");
            assert!(records[0].info.is_none());
            assert!(unreadable.contains_key(&hash), "{unreadable:?}");
            assert_eq!((records[0].wants, records[0].gets.clone()), (Wants::Run, Gets::Only([1].into())), "as it was");
            assert_eq!(std::fs::read(&kept).unwrap(), b"d4:info", "not deleted");
            assert!(session.join(format!("{hash}.torrent")).exists(), "nor librqbit's copy");
            assert!(store.read_info(&records[0]).is_err());
        }
        std::fs::write(&kept, &bytes).unwrap();
        assert!(load(&data, &session).records[0].info.is_some(), "read again");
    }

    #[test]
    fn a_saved_store_loads_back() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        let Loaded { mut store, .. } = load(&data, &session);
        let id = store.next_id();
        let mut record =
            Record::new(hash.clone(), id, Some("pack".into()), AbsolutePath::new("/tmp/x").unwrap(), vec!["http://t/a".into()], Wants::Run);
        store.keep_info(&hash, &bytes).unwrap();
        record.info = Some(Info::parse(&bytes).unwrap());
        record.gets = Gets::Only([1].into());
        record.unchecked = true;
        record.missing_at_shutdown = [(1, 5)].into();
        record.had = vec![9, 3];
        store.save([&record]).unwrap();
        let loaded = load(&data, &session);
        assert_eq!(loaded.records, vec![record]);
        assert_eq!(loaded.store, store);
        assert_eq!(std::fs::read_dir(&data).unwrap().filter(|e| e.as_ref().unwrap().path().is_file()).count(), 1, "no temporary file left");
    }

    #[test]
    fn a_record_without_info_goes_at_start() {
        let (_dir, data, session) = dirs();
        let Loaded { mut store, .. } = load(&data, &session);
        let record = Record::new("aa".into(), store.next_id(), None, AbsolutePath::new("/tmp/x").unwrap(), Vec::new(), Wants::Run);
        store.save([&record]).unwrap();
        assert_eq!(load(&data, &session).records, Vec::new());
    }

    proptest::proptest! {
        /// What's wanted, stopped at a seeding limit too, and the seeding round load back as saved.
        #[test]
        fn wants_and_the_seeding_round_load_back(wants in proptest::sample::select(lifecycle::samples::wants()), uploaded in 0..u64::MAX / 2, secs in 0..u64::MAX / 4) {
            let (_dir, data, session) = dirs();
            let bytes = torrent();
            let hash = hash_of(&bytes);
            let Loaded { mut store, .. } = load(&data, &session);
            let mut record = Record::new(hash.clone(), store.next_id(), Some("pack".into()), AbsolutePath::new("/tmp/x").unwrap(), Vec::new(), wants);
            store.keep_info(&hash, &bytes).unwrap();
            record.info = Some(Info::parse(&bytes).unwrap());
            record.round = lifecycle::seeding::Round { uploaded, seeded: Duration::from_secs(secs) };
            store.save([&record]).unwrap();
            proptest::prop_assert_eq!(load(&data, &session).records, vec![record]);
        }
    }

    #[test]
    fn fresh_ids_stay_above_ids_taken_elsewhere() {
        let (_dir, data, session) = dirs();
        let Loaded { mut store, .. } = load(&data, &session);
        store.ids_above(4);
        assert_eq!(store.next_id(), 5);
        store.ids_above(2);
        assert_eq!(store.next_id(), 6, "never back down");
    }

    #[test]
    fn running_at_shutdown_and_held_priorities_are_taken_over() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::create_dir_all(data.join("torrents")).unwrap();
        std::fs::write(data.join(format!("torrents/{hash}.torrent")), &bytes).unwrap();
        let old = format!(
            r#"{{"next_id": 2, "torrents": {{"{hash}": {{"id": 0, "destination": "/tmp/x", "running_at_shutdown": true, "held": [1]}}}}}}"#
        );
        std::fs::write(data.join("engine.json"), old).unwrap();
        let records = load(&data, &session).records;
        assert_eq!(records[0].wants, Wants::Run);
        assert_eq!(records[0].normal(), [1].into());
    }
}
