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
//! [`Store::load`] moves into the records and then deletes, so librqbit restores nothing.

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
    /// The files it gets (those Normal), by index; every one until a priority changes.
    pub normal: BTreeSet<usize>,
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
        } = saved;
        if running_at_shutdown {
            wants = Wants::Run;
        }
        let normal = saved_normal
            .or(held)
            .or_else(|| normal.map(|n| n.into_iter().collect()))
            .or_else(|| info.as_ref().map(Info::all))
            .unwrap_or_default();
        Some(Self {
            info_hash: info_hash.to_owned(),
            id,
            name: name.or_else(|| info.as_ref().and_then(|i| i.name.clone())),
            destination: destination?,
            info,
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
            normal: Some(self.normal.clone()),
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
            .filter(|(i, _)| self.normal.contains(i))
            .filter_map(|(&i, &progress_bytes)| {
                let f = info.files.get(i)?;
                Some((i, Gone { path: paths[i].clone(), size_bytes: f.len, progress_bytes, seen: lifecycle::disk::Seen::NotYet }))
            })
            .collect()
    }
}

impl Store {
    /// Loads the records in `dir`, taking over what older builds kept (`ids.json`, librqbit's
    /// `session.json` in `session`): each torrent librqbit had gets a record from it. Then librqbit's
    /// own record of them goes, so it restores nothing; the pieces it saved stay, and the engine
    /// adds each torrent with them. Records whose info never arrived go: librqbit never had them.
    pub fn load(dir: &Path, session: &Path) -> Result<(Self, Vec<Record>)> {
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
        let mut store = Self { dir: dir.to_owned(), next_id: file.next_id };
        let mut librqbit = session_db::restored(session);
        let mut records = Vec::new();
        for (hash, saved) in file.torrents {
            let old = librqbit.remove(&hash);
            let info = match store.info(&hash, old.as_ref()) {
                Ok(info) => info,
                Err(e) => {
                    warn!(hash, error = %e, "can't read the torrent's .torrent: it's dropped");
                    continue;
                }
            };
            // Fetching info when the service stopped: librqbit never had it.
            if info.is_none() {
                continue;
            }
            let destination = saved.destination.clone().or_else(|| old.as_ref().and_then(|o| o.destination(info.as_ref()?)));
            let normal = old.and_then(|o| o.only_files);
            match Record::parse(&hash, Saved { destination, ..saved }, info, normal) {
                Some(record) => records.push(record),
                None => warn!(hash, "the torrent's record has no destination: it's dropped"),
            }
        }
        // librqbit had it, and Phantom no record of it: a fresh id.
        for (hash, old) in librqbit {
            let Ok(Some(info)) = store.info(&hash, Some(&old)) else { continue };
            let Some(destination) = old.destination(&info) else { continue };
            let id = store.next_id();
            let saved = Saved { id, destination: Some(destination), trackers: old.trackers.clone(), ..Default::default() };
            records.extend(Record::parse(&hash, saved, Some(info), old.only_files));
        }
        store.save(records.iter())?;
        session_db::forget(session);
        Ok((store, records))
    }

    /// The info of torrent `hash`: from its .torrent kept here, else from librqbit's copy (`old`),
    /// which is kept here from now on.
    fn info(&self, hash: &str, old: Option<&session_db::Restored>) -> Result<Option<Info>> {
        if let Some(bytes) = read(&self.info_path(hash))? {
            return Info::parse(&bytes).map(Some);
        }
        let Some(bytes) = old.and_then(|o| o.torrent.as_ref()) else { return Ok(None) };
        let info = Info::parse(bytes)?;
        self.keep_info(hash, bytes)?;
        Ok(Some(info))
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

    /// A .torrent of two files.
    fn torrent() -> Vec<u8> {
        match super::super::tests::multi_file_torrent(&["a.bin", "b.bin"]) {
            proto::AddSource::TorrentFile(bytes) => bytes,
            _ => unreachable!("a .torrent"),
        }
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

    #[test]
    fn ids_json_is_taken_over_for_the_torrents_librqbit_had() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        std::fs::write(data.join("ids.json"), format!(r#"{{"next": 5, "ids": {{"{hash}": 3}}}}"#)).unwrap();
        std::fs::write(session.join(format!("{hash}.torrent")), &bytes).unwrap();
        let db = format!(
            r#"{{"torrents": {{"0": {{"info_hash": "{hash}", "trackers": [], "output_folder": "/tmp/x/pack", "only_files": null, "is_paused": true}}}}}}"#
        );
        std::fs::write(session.join("session.json"), db).unwrap();
        let (mut store, records) = Store::load(&data, &session).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 3);
        assert_eq!(records[0].destination, AbsolutePath::new("/tmp/x").unwrap(), "librqbit's folder, less the torrent's own");
        assert_eq!(records[0].output_folder(), Some(PathBuf::from("/tmp/x/pack")));
        assert_eq!(store.next_id(), 5);
        assert!(!data.join("ids.json").exists(), "replaced");
        assert!(!session.join("session.json").exists() && !session.join(format!("{hash}.torrent")).exists(), "librqbit restores nothing");
        let (_, again) = Store::load(&data, &session).unwrap();
        assert_eq!(again, records, "kept as Phantom's own");
    }

    #[test]
    fn a_saved_store_loads_back() {
        let (_dir, data, session) = dirs();
        let bytes = torrent();
        let hash = hash_of(&bytes);
        let (mut store, _) = Store::load(&data, &session).unwrap();
        let id = store.next_id();
        let mut record =
            Record::new(hash.clone(), id, Some("pack".into()), AbsolutePath::new("/tmp/x").unwrap(), vec!["http://t/a".into()], Wants::Run);
        store.keep_info(&hash, &bytes).unwrap();
        record.info = Some(Info::parse(&bytes).unwrap());
        record.normal = [1].into();
        record.unchecked = true;
        record.missing_at_shutdown = [(1, 5)].into();
        record.had = vec![9, 3];
        store.save([&record]).unwrap();
        let (loaded_store, loaded) = Store::load(&data, &session).unwrap();
        assert_eq!(loaded, vec![record]);
        assert_eq!(loaded_store, store);
        assert_eq!(std::fs::read_dir(&data).unwrap().filter(|e| e.as_ref().unwrap().path().is_file()).count(), 1, "no temporary file left");
    }

    #[test]
    fn a_record_without_info_goes_at_start() {
        let (_dir, data, session) = dirs();
        let (mut store, _) = Store::load(&data, &session).unwrap();
        let record = Record::new("aa".into(), store.next_id(), None, AbsolutePath::new("/tmp/x").unwrap(), Vec::new(), Wants::Run);
        store.save([&record]).unwrap();
        assert_eq!(Store::load(&data, &session).unwrap().1, Vec::new());
    }

    proptest::proptest! {
        /// What's wanted, stopped at a seeding limit too, and the seeding round load back as saved.
        #[test]
        fn wants_and_the_seeding_round_load_back(wants in proptest::sample::select(lifecycle::samples::wants()), uploaded in 0..u64::MAX / 2, secs in 0..u64::MAX / 4) {
            let (_dir, data, session) = dirs();
            let bytes = torrent();
            let hash = hash_of(&bytes);
            let (mut store, _) = Store::load(&data, &session).unwrap();
            let mut record = Record::new(hash.clone(), store.next_id(), Some("pack".into()), AbsolutePath::new("/tmp/x").unwrap(), Vec::new(), wants);
            store.keep_info(&hash, &bytes).unwrap();
            record.info = Some(Info::parse(&bytes).unwrap());
            record.normal = record.info.as_ref().unwrap().all();
            record.round = lifecycle::seeding::Round { uploaded, seeded: Duration::from_secs(secs) };
            store.save([&record]).unwrap();
            proptest::prop_assert_eq!(Store::load(&data, &session).unwrap().1, vec![record]);
        }
    }

    #[test]
    fn fresh_ids_stay_above_ids_taken_elsewhere() {
        let (_dir, data, session) = dirs();
        let (mut store, _) = Store::load(&data, &session).unwrap();
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
        let (_, records) = Store::load(&data, &session).unwrap();
        assert_eq!(records[0].wants, Wants::Run);
        assert_eq!(records[0].normal, [1].into());
    }
}
