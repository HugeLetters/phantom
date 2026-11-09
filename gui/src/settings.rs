//! The GUI's own settings, `gui.json` in the data dir: what [`GuiSettings`] names. Settings are
//! the defaults plus whatever the file overrides ([`proto::tolerant`]): at startup [`repair`]
//! keeps a copy of a bad file as `gui.invalid.json` and writes the
//! valid values back, so the stores that read it afterwards (Rune's `Prefs`) see a good file and
//! no valid value is lost.

use std::io;
use std::path::Path;

use proto::tolerant;
use serde::{Deserialize, Serialize};

use crate::platform::app_icon::{Icons, Written};

/// Every key of `gui.json`: [`crate::platform::app_icon::SETTING`] and
/// [`crate::platform::app_icon::WRITTEN`].
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(default)]
pub struct GuiSettings {
    pub app_icon: Option<Icons>,
    pub app_icon_written: Option<Written>,
}

/// Reads `path` and, if some of it was invalid, keeps a copy and writes the repaired file. Returns
/// the names reset; the file not being there is none.
pub fn repair(path: &Path) -> io::Result<Vec<String>> {
    let decoded = tolerant::load::<GuiSettings>(path)?;
    if !decoded.reset.is_empty() {
        std::fs::write(path, serde_json::to_vec_pretty(&decoded.value)?)?;
    }
    Ok(decoded.reset)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::*;
    use crate::platform::app_icon::{SETTING, WRITTEN};

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

    proptest! {
        #[test]
        fn the_fields_are_the_keys_the_stores_use(app_icon in icons(), written in written()) {
            let value = serde_json::to_value(GuiSettings { app_icon, app_icon_written: written }).unwrap();
            let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
            prop_assert_eq!(keys, vec![SETTING, WRITTEN]);
        }

        #[test]
        fn settings_round_trip_and_a_bad_field_is_reset_with_the_other_kept(app_icon in icons(), written in written()) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("gui.json");
            let good = GuiSettings { app_icon: app_icon.clone(), app_icon_written: written.clone() };
            std::fs::write(&path, serde_json::to_vec(&good).unwrap()).unwrap();
            prop_assert!(repair(&path).unwrap().is_empty());
            prop_assert!(!tolerant::invalid_copy(&path).exists());

            let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            file[SETTING] = json!({"icon": 7});
            let bad = serde_json::to_vec(&file).unwrap();
            std::fs::write(&path, &bad).unwrap();
            prop_assert_eq!(repair(&path).unwrap(), vec![SETTING]);
            prop_assert_eq!(std::fs::read(tolerant::invalid_copy(&path)).unwrap(), bad);
            let repaired: GuiSettings = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            prop_assert_eq!(repaired, GuiSettings { app_icon: None, app_icon_written: written });
        }
    }
}
