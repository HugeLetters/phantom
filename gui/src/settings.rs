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

use crate::model::layout::{Column, ColumnOrder, PaneHeight, PaneTab};
use crate::platform::app_icon::{Icons, Written};
use crate::theme::preset::{LookPreset, Stored};
use crate::theme::saved;

/// Every key of `gui.json`: [`crate::platform::app_icon::SETTING`],
/// [`crate::platform::app_icon::WRITTEN`], [`crate::theme::preset::SETTING`],
/// [`crate::theme::saved::SETTING`], and the main window's [`crate::model::layout::COLUMN_ORDER`],
/// [`crate::model::layout::HIDDEN_COLUMNS`], [`crate::model::layout::PANE_HEIGHT`] and
/// [`crate::model::layout::PANE_TAB`].
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(default)]
pub struct GuiSettings {
    pub app_icon: Option<Icons>,
    pub app_icon_written: Option<Written>,
    pub look_preset: Stored,
    /// Each as [`saved::read`] reads it: one that's off is repaired or dropped alone.
    pub saved_presets: Vec<serde_json::Value>,
    /// The list's columns in the person's order (#33).
    pub column_order: ColumnOrder,
    /// The list's columns the person hid (#33).
    pub hidden_columns: Vec<Column>,
    /// The height the details pane's divider was let go at (#33).
    pub pane_height: Option<PaneHeight>,
    /// The details pane's tab last shown (#33).
    pub details_tab: PaneTab,
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
    use crate::model::layout::{COLUMN_ORDER, HIDDEN_COLUMNS, PANE_HEIGHT, PANE_TAB};
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

    fn layout() -> impl Strategy<Value = (ColumnOrder, Vec<Column>, Option<PaneHeight>, PaneTab)> {
        let order = Just(Column::ALL.to_vec()).prop_shuffle().prop_map(|o| ColumnOrder::try_from(o).unwrap());
        let hidden = proptest::sample::subsequence(Column::ALL.to_vec(), 0..Column::ALL.len());
        let tab = proptest::sample::select(PaneTab::ALL.to_vec());
        (order, hidden, proptest::option::of((60u16..1200).prop_map(|h| PaneHeight::try_from(f32::from(h)).unwrap())), tab)
    }

    fn settings() -> impl Strategy<Value = GuiSettings> {
        (icons(), written(), preset(), saved(), layout()).prop_map(
            |(app_icon, app_icon_written, preset, presets, (order, hidden, height, tab))| GuiSettings {
                app_icon,
                app_icon_written,
                look_preset: preset.stored(),
                saved_presets: saved::stored(&presets).into_iter().map(|s| serde_json::to_value(s).unwrap()).collect(),
                column_order: order,
                hidden_columns: hidden,
                pane_height: height,
                details_tab: tab,
            },
        )
    }

    proptest! {
        #[test]
        fn the_fields_are_the_keys_the_stores_use(settings in settings()) {
            let value = serde_json::to_value(settings).unwrap();
            let keys: std::collections::BTreeSet<_> = value.as_object().unwrap().keys().cloned().collect();
            let expected = [SETTING, WRITTEN, LOOK, SAVED, COLUMN_ORDER, HIDDEN_COLUMNS, PANE_HEIGHT, PANE_TAB].map(String::from).into_iter().collect();
            prop_assert_eq!(keys, expected);
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
        fn settings_round_trip_and_a_bad_field_is_reset_with_the_others_kept(settings in settings(), which in 0..12usize) {
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
                // A column there isn't; a height that isn't a number.
                7 => {
                    file[HIDDEN_COLUMNS] = json!(["nonsense"]);
                    expected.hidden_columns = Vec::new();
                    HIDDEN_COLUMNS.to_owned()
                }
                8 => {
                    file[PANE_HEIGHT] = json!("tall");
                    expected.pane_height = None;
                    PANE_HEIGHT.to_owned()
                }
                // An order that leaves columns out; a tab there isn't.
                9 => {
                    file[COLUMN_ORDER] = json!(["name", "eta"]);
                    expected.column_order = ColumnOrder::default();
                    COLUMN_ORDER.to_owned()
                }
                10 => {
                    file[PANE_TAB] = json!("peers");
                    expected.details_tab = PaneTab::default();
                    PANE_TAB.to_owned()
                }
                // A height that's no height: zero or less.
                11 => {
                    file[PANE_HEIGHT] = json!(-400.0);
                    expected.pane_height = None;
                    PANE_HEIGHT.to_owned()
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
