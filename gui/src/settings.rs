//! The GUI's own settings, `gui.json` in the data dir: what [`GuiSettings`] names. Settings are
//! the defaults plus whatever the file overrides ([`proto::tolerant`]), and the Active Look
//! Preset is read as its themes declare their settings ([`LookPreset::read`]): at startup
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

/// Every key of `gui.json`: [`crate::platform::app_icon::SETTING`],
/// [`crate::platform::app_icon::WRITTEN`] and [`crate::theme::preset::SETTING`].
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(default)]
pub struct GuiSettings {
    pub app_icon: Option<Icons>,
    pub app_icon_written: Option<Written>,
    pub look_preset: Stored,
}

/// Phantom's own record of the icon it wrote, not something a person sets: reset silently.
const INTERNAL: [&str; 1] = ["app_icon_written"];

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
/// the Look Preset as its themes do.
fn decode(bytes: &[u8]) -> (GuiSettings, Vec<String>) {
    let mut decoded = tolerant::decode::<GuiSettings>(bytes, &INTERNAL);
    let (preset, reset) = LookPreset::read(&decoded.value.look_preset);
    decoded.value.look_preset = preset.stored();
    decoded.reset.extend(reset);
    (decoded.value, decoded.reset)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::*;
    use crate::platform::app_icon::{SETTING, WRITTEN};
    use crate::theme::preset::SETTING as LOOK;
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
        (icons(), written(), preset()).prop_map(|(app_icon, app_icon_written, preset)| GuiSettings {
            app_icon,
            app_icon_written,
            look_preset: preset.stored(),
        })
    }

    proptest! {
        #[test]
        fn the_fields_are_the_keys_the_stores_use(settings in settings()) {
            let value = serde_json::to_value(settings).unwrap();
            let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            prop_assert_eq!(keys, vec![SETTING, WRITTEN, LOOK]);
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
        fn settings_round_trip_and_a_bad_field_is_reset_with_the_others_kept(settings in settings(), which in 0..4usize) {
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
                _ => {
                    file[LOOK]["dark"]["theme"] = json!("gone");
                    expected.look_preset.dark = Stored::default().dark;
                    format!("{LOOK}.dark")
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
