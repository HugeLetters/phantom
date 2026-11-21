//! The GUI's own settings, `gui.json` in the data dir: what [`GuiSettings`] names. Settings are
//! the defaults plus whatever the file overrides ([`proto::tolerant`]), and the Active Look
//! Preset and each saved one are read as their themes declare their settings ([`LookPreset::read`],
//! [`saved::read`]): at startup
//! [`repair`] keeps a copy of a bad file as `gui.invalid.json` and writes the valid values back,
//! so the stores that read it afterwards (Rune's `Prefs`) see a good file and no valid value is
//! lost.

pub mod file;

use std::io;
use std::path::Path;

use proto::tolerant;
use serde::{Deserialize, Serialize};

use crate::platform::app_icon::{Icons, Written};
use crate::theme::preset::{LookPreset, Stored};
use crate::theme::saved;

/// Every key of `gui.json`: [`crate::platform::app_icon::SETTING`],
/// [`crate::platform::app_icon::WRITTEN`], [`crate::theme::preset::SETTING`] and
/// [`crate::theme::saved::SETTING`].
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(default)]
pub struct GuiSettings {
    pub app_icon: Option<Icons>,
    pub app_icon_written: Option<Written>,
    pub look_preset: Stored,
    /// Each as [`saved::read`] reads it: one that's off is repaired or dropped alone.
    pub saved_presets: Vec<serde_json::Value>,
    /// The folders Move to ▸ offers ([`crate::model::recent::SETTING`]).
    pub recent_folders: Vec<proto::AbsolutePath>,
}

/// Phantom's own records (the icon it wrote, the folders last moved to), not something a person
/// sets: reset silently.
const INTERNAL: [&str; 2] = ["app_icon_written", "recent_folders"];

/// Reads `path` and, if some of it was invalid, keeps a copy and writes the repaired file. Returns
/// the names reset; the file not being there is none.
pub fn repair(path: &Path) -> io::Result<Vec<String>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let (value, reset) = decode(&bytes);
    if !reset.is_empty() {
        std::fs::write(tolerant::invalid_copy(path), &bytes)?;
        std::fs::write(path, serde_json::to_vec_pretty(&value)?)?;
    }
    Ok(reset)
}

/// `bytes` as settings, and the names of those reset: each key as [`tolerant::decode`] reads it,
/// the Look Presets, active and saved, as their themes do.
fn decode(bytes: &[u8]) -> (GuiSettings, Vec<String>) {
    let mut decoded = tolerant::decode::<GuiSettings>(bytes, &INTERNAL);
    let (preset, reset) = LookPreset::read(&decoded.value.look_preset);
    decoded.value.look_preset = preset.stored();
    decoded.reset.extend(reset);
    let (presets, reset) = saved::read(&decoded.value.saved_presets);
    decoded.value.saved_presets = saved::stored(&presets).into_iter().map(|s| serde_json::to_value(s).expect("JSON")).collect();
    decoded.reset.extend(reset);
    (decoded.value, decoded.reset)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::*;
    use crate::model::recent::SETTING as RECENT;
    use crate::platform::app_icon::{SETTING, WRITTEN};
    use crate::theme::preset::SETTING as LOOK;
    use crate::theme::saved::SETTING as SAVED;
    use crate::theme::saved::tests::saved;
    use crate::theme::tests::preset;

    fn icons() -> impl Strategy<Value = Option<Icons>> {
        prop_oneof![
            Just(None),
            "[a-z-]{1,8}".prop_map(|icon| Some(Icons::One { icon })),
            ("[a-z-]{1,8}", "[a-z-]{1,8}").prop_map(|(dark, light)| Some(Icons::Pair { dark, light })),
        ]
    }

    fn written() -> impl Strategy<Value = Option<Written>> {
        proptest::option::of(("[a-z-]{1,8}", any::<u64>()).prop_map(|(id, fingerprint)| Written { id, fingerprint }))
    }

    fn settings() -> impl Strategy<Value = GuiSettings> {
        let recent = proptest::collection::vec("/[a-z]{1,6}(/[a-z]{1,6})?", 0..=5)
            .prop_map(|paths| paths.into_iter().map(|p| proto::AbsolutePath::new(p).unwrap()).collect::<Vec<_>>());
        (icons(), written(), preset(), saved(), recent).prop_map(|(app_icon, app_icon_written, preset, presets, recent_folders)| {
            GuiSettings {
                app_icon,
                app_icon_written,
                look_preset: preset.stored(),
                saved_presets: saved::stored(&presets).into_iter().map(|s| serde_json::to_value(s).unwrap()).collect(),
                recent_folders,
            }
        })
    }

    proptest! {
        #[test]
        fn the_fields_are_the_keys_the_stores_use(settings in settings()) {
            let value = serde_json::to_value(settings).unwrap();
            let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            prop_assert_eq!(keys, vec![SETTING, WRITTEN, LOOK, RECENT, SAVED]);
        }

        /// The folders last moved to are Phantom's record, not a setting: one that isn't a list of
        /// absolute paths is forgotten, silently.
        #[test]
        fn an_invalid_list_of_recent_folders_is_reset_silently(settings in settings(), bad in prop_oneof![Just(json!("Films")), Just(json!(["relative/path"])), Just(json!(7))]) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gui.json");
            let mut file = serde_json::to_value(&settings).unwrap();
            file[RECENT] = bad;
            std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
            prop_assert!(repair(&path).unwrap().is_empty());
            prop_assert_eq!(decode(&std::fs::read(&path).unwrap()).0, GuiSettings { recent_folders: Vec::new(), ..settings });
        }

        #[test]
        fn an_invalid_record_of_the_icon_written_is_reset_silently(settings in settings()) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gui.json");
            let mut file = serde_json::to_value(&settings).unwrap();
            file[WRITTEN] = json!("nonsense");
            std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
            prop_assert!(repair(&path).unwrap().is_empty());
            prop_assert!(!tolerant::invalid_copy(&path).exists());
            prop_assert_eq!(decode(&std::fs::read(&path).unwrap()).0, GuiSettings { app_icon_written: None, ..settings });
        }

        /// Good settings read back as they are and are left alone; a bad field is reset, named and
        /// written back with the others kept, and the file as it was kept as a copy.
        #[test]
        fn settings_round_trip_and_a_bad_field_is_reset_with_the_others_kept(settings in settings(), which in 0..7usize) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gui.json");
            let good = serde_json::to_vec(&settings).unwrap();
            std::fs::write(&path, &good).unwrap();
            prop_assert!(repair(&path).unwrap().is_empty());
            prop_assert!(!tolerant::invalid_copy(&path).exists());
            prop_assert_eq!(&std::fs::read(&path).unwrap(), &good, "left alone");

            let mut file: Value = serde_json::from_slice(&good).unwrap();
            let mut expected = settings.clone();
            let name = match which {
                0 => {
                    file[SETTING] = json!({"icon": 7});
                    expected.app_icon = None;
                    SETTING.to_owned()
                }
                1 => {
                    file[LOOK] = json!("dark");
                    expected.look_preset = Stored::default();
                    LOOK.to_owned()
                }
                // A theme setting its theme doesn't have, and a theme there isn't.
                2 => {
                    file[LOOK]["light"]["settings"]["nonsense"] = json!(1);
                    format!("{LOOK}.light.nonsense")
                }
                3 => {
                    file[LOOK]["dark"]["theme"] = json!("gone");
                    expected.look_preset.dark = Stored::default().dark;
                    format!("{LOOK}.dark")
                }
                // The saved presets: not a list; a saved one's theme there isn't, or setting of
                // the wrong kind.
                4 => {
                    file[SAVED] = json!({"Calm": 1});
                    expected.saved_presets = Vec::new();
                    SAVED.to_owned()
                }
                5 => {
                    file[SAVED] = json!([{"name": "Calm", "light": {"theme": "gone"}, "dark": Stored::default().dark}]);
                    let fresh = Stored::default();
                    expected.saved_presets = vec![json!({"name": "Calm", "light": fresh.light, "dark": fresh.dark})];
                    format!("{SAVED}.\"Calm\".light")
                }
                _ => {
                    let mut dark = Stored::default().dark;
                    dark.settings.insert("tone".into(), json!(7));
                    file[SAVED] = json!([{"name": "Calm", "light": Stored::default().light, "dark": dark}]);
                    let fresh = Stored::default();
                    expected.saved_presets = vec![json!({"name": "Calm", "light": fresh.light, "dark": fresh.dark})];
                    format!("{SAVED}.\"Calm\".dark.tone")
                }
            };
            let bad = serde_json::to_vec(&file).unwrap();
            std::fs::write(&path, &bad).unwrap();
            prop_assert_eq!(repair(&path).unwrap(), vec![name]);
            prop_assert_eq!(std::fs::read(tolerant::invalid_copy(&path)).unwrap(), bad);
            let repaired: GuiSettings = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            prop_assert_eq!(&repaired, &expected);
            prop_assert!(decode(&std::fs::read(&path).unwrap()).1.is_empty(), "the repaired file reads clean");
        }
    }
}
