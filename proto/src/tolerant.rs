//! Reading a settings file that may be wrong. Settings are the defaults plus whatever the file
//! overrides; a value that doesn't decode counts as missing, and its field's name is reported so a
//! client can say so. Shared by `settings.json` (service) and `gui.json` (GUI).

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// What a file said: the settings, and the fields that were reset to their defaults.
#[derive(Debug, PartialEq)]
pub struct Decoded<T> {
    pub value: T,
    pub reset: Vec<String>,
}

/// `bytes` as a `T` (every field of which must serialize, `None` as null, and decode alone), field
/// by field: a field that doesn't decode is skipped and named in `reset`, unknown fields are
/// ignored. Something that isn't a JSON object resets every field.
pub fn decode<T: Default + Serialize + DeserializeOwned>(bytes: &[u8]) -> Decoded<T> {
    let mut fields = match serde_json::to_value(T::default()) {
        Ok(Value::Object(fields)) => fields,
        _ => Map::new(),
    };
    let Ok(Value::Object(file)) = serde_json::from_slice(bytes) else {
        return Decoded { value: T::default(), reset: fields.keys().cloned().collect() };
    };
    let mut reset = Vec::new();
    for (name, value) in file {
        let before = fields.insert(name.clone(), value);
        if serde_json::from_value::<T>(Value::Object(fields.clone())).is_err() {
            match before {
                Some(before) => fields.insert(name.clone(), before),
                None => fields.remove(&name),
            };
            reset.push(name);
        }
    }
    Decoded { value: serde_json::from_value(Value::Object(fields)).unwrap_or_default(), reset }
}

/// The file at `path` as a `T`; a file that isn't there is the defaults. When fields were reset,
/// a copy of the file is kept beside it (`settings.json` → `settings.invalid.json`) before the
/// caller writes the repaired settings over it.
pub fn load<T: Default + Serialize + DeserializeOwned>(path: &Path) -> io::Result<Decoded<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Decoded { value: T::default(), reset: vec![] }),
        Err(e) => return Err(e),
    };
    let decoded = decode(&bytes);
    if !decoded.reset.is_empty() {
        std::fs::write(invalid_copy(path), &bytes)?;
    }
    Ok(decoded)
}

/// Where the copy of a bad `settings.json` is kept: `settings.invalid.json`.
pub fn invalid_copy(path: &Path) -> PathBuf {
    path.with_extension("invalid.json")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::Deserialize;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    #[serde(default)]
    struct Sample {
        port: u16,
        upnp: bool,
        name: String,
        folder: Option<String>,
    }

    impl Default for Sample {
        fn default() -> Self {
            Self { port: 7, upnp: true, name: "x".into(), folder: None }
        }
    }

    const FIELDS: [&str; 4] = ["port", "upnp", "name", "folder"];

    fn sample() -> impl Strategy<Value = Sample> {
        (any::<u16>(), any::<bool>(), ".*", proptest::option::of(".*")).prop_map(|(port, upnp, name, folder)| Sample {
            port,
            upnp,
            name,
            folder,
        })
    }

    /// JSON no `Sample` field accepts: not a number, bool, string or null.
    fn invalid() -> impl Strategy<Value = Value> {
        prop_oneof![
            Just(Value::Array(vec![])),
            Just(Value::Object(Map::new())),
            any::<Vec<u8>>().prop_map(|v| Value::Array(v.into_iter().map(Into::into).collect())),
        ]
    }

    fn json(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    proptest! {
        #[test]
        fn encoding_then_decoding_gives_the_value_back(s in sample()) {
            let d = decode::<Sample>(&serde_json::to_vec(&s).unwrap());
            prop_assert_eq!(d, Decoded { value: s, reset: vec![] });
        }

        #[test]
        fn invalid_fields_are_reset_and_named_and_the_rest_kept(
            s in sample(),
            bad in proptest::collection::btree_map(proptest::sample::select(FIELDS.to_vec()), invalid(), 0..=4),
        ) {
            let mut file = serde_json::to_value(&s).unwrap();
            for (name, value) in &bad {
                file[*name] = value.clone();
            }
            let d = decode::<Sample>(&json(&file));
            let mut expected = s.clone();
            let defaults = Sample::default();
            for name in bad.keys() {
                match *name {
                    "port" => expected.port = defaults.port,
                    "upnp" => expected.upnp = defaults.upnp,
                    "name" => expected.name = defaults.name.clone(),
                    _ => expected.folder = defaults.folder.clone(),
                }
            }
            let mut reset = d.reset.clone();
            reset.sort();
            let mut names: Vec<String> = bad.keys().map(|n| n.to_string()).collect();
            names.sort();
            prop_assert_eq!(reset, names);
            prop_assert_eq!(d.value, expected);
        }

        #[test]
        fn unknown_fields_are_ignored(s in sample(), extra in "[a-z]{8,12}", value in any::<i64>()) {
            let mut file = serde_json::to_value(&s).unwrap();
            file[extra] = value.into();
            prop_assert_eq!(decode::<Sample>(&json(&file)), Decoded { value: s, reset: vec![] });
        }

        #[test]
        fn a_bad_file_is_kept_as_it_was_and_a_good_one_isnt_copied(
            s in sample(),
            garbage in "[^{]*".prop_filter("not json", |g| serde_json::from_str::<Value>(g).is_err()),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            prop_assert_eq!(load::<Sample>(&path).unwrap(), Decoded { value: Sample::default(), reset: vec![] });
            std::fs::write(&path, serde_json::to_vec(&s).unwrap()).unwrap();
            prop_assert!(load::<Sample>(&path).unwrap().reset.is_empty());
            prop_assert!(!invalid_copy(&path).exists());
            std::fs::write(&path, &garbage).unwrap();
            let d = load::<Sample>(&path).unwrap();
            prop_assert_eq!((d.value, d.reset.len()), (Sample::default(), 4));
            prop_assert_eq!(std::fs::read_to_string(invalid_copy(&path)).unwrap(), garbage);
            prop_assert_eq!(invalid_copy(&path), dir.path().join("settings.invalid.json"));
        }

        #[test]
        fn what_isnt_a_json_object_gives_the_defaults_and_resets_everything(
            bytes in any::<Vec<u8>>().prop_filter("not an object", |b| !matches!(serde_json::from_slice(b), Ok(Value::Object(_)))),
        ) {
            let d = decode::<Sample>(&bytes);
            prop_assert_eq!(d.value, Sample::default());
            prop_assert_eq!(d.reset, vec!["folder", "name", "port", "upnp"]);
        }
    }
}
