//! `gui.json` itself: every read of a key and every change to one after startup goes through
//! here. The file holds keys several parts of the app keep (the App icon, the icon written, the
//! Look Preset), so a change reads the file as it is now, sets its own key and writes the whole
//! file back through a temporary file and a rename: no store holds the file between changes,
//! and no change drops another's key.
//!
//! A file that isn't a JSON object when a change comes is kept as `gui.invalid.json`, as startup
//! repair keeps one, and the change starts a new file.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Where the file is: in the data dir.
pub fn path() -> PathBuf {
    proto::data_dir().join("gui.json")
}

/// `key` as the file at [`path`] holds it now; `None` when it's missing or doesn't decode.
pub fn read<T: DeserializeOwned>(key: &str) -> Option<T> {
    read_at(&path(), key)
}

/// Keeps `value` under `key` in the file at [`path`], `None` removing it; a failure is logged.
pub fn keep<T: Serialize + ?Sized>(key: &str, value: Option<&T>) {
    if let Err(e) = keep_at(&path(), key, value) {
        eprintln!("phantom: can't keep {key} in gui.json: {e}");
    }
}

/// [`read`], of the file at `path`.
pub fn read_at<T: DeserializeOwned>(path: &Path, key: &str) -> Option<T> {
    let object = object(&std::fs::read(path).ok()?)?;
    serde_json::from_value(object.get(key)?.clone()).ok()
}

/// [`keep`], in the file at `path`.
pub fn keep_at<T: Serialize + ?Sized>(path: &Path, key: &str, value: Option<&T>) -> io::Result<()> {
    let mut file = match std::fs::read(path) {
        Ok(bytes) => object(&bytes).map_or_else(|| std::fs::write(proto::tolerant::invalid_copy(path), &bytes).map(|()| Map::new()), Ok)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(e),
    };
    match value {
        Some(value) => file.insert(key.to_owned(), serde_json::to_value(value)?),
        None => file.remove(key),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&file)?)?;
    std::fs::rename(&temp, path)
}

fn object(bytes: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice(bytes) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    proptest! {
        /// Keeping a key, in any order and from any part of the app, keeps every other key as the
        /// file has it, and reads back what was kept.
        #[test]
        fn a_change_keeps_every_other_key(changes in proptest::collection::vec(("[a-c]", proptest::option::of(any::<i32>())), 1..12)) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gui.json");
            let mut expected = Map::new();
            for (key, value) in changes {
                keep_at(&path, &key, value.as_ref()).unwrap();
                match value {
                    Some(v) => expected.insert(key.clone(), json!(v)),
                    None => expected.remove(&key),
                };
                prop_assert_eq!(read_at::<i32>(&path, &key), value);
                prop_assert_eq!(object(&std::fs::read(&path).unwrap()), Some(expected.clone()));
            }
        }
    }

    /// A file that isn't an object is kept as `gui.invalid.json` before the change starts a new one.
    #[test]
    fn a_broken_file_is_kept_aside_before_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui.json");
        std::fs::write(&path, b"{ not json").unwrap();
        keep_at(&path, "look_preset", Some(&json!(1))).unwrap();
        assert_eq!(std::fs::read(proto::tolerant::invalid_copy(&path)).unwrap(), b"{ not json");
        assert_eq!(read_at::<i32>(&path, "look_preset"), Some(1));
    }
}
