//! When a sheet's edits are kept (#470): they're shown as they're made and written to `gui.json`
//! once, by Done. Cancel, Escape, the window closing and the app quitting with the sheet open
//! keep none of them, and nothing else written to `gui.json` meanwhile carries them along.

use std::path::{Path, PathBuf};

use rune::{Key, KeyEvent, Mods};

use super::super::Desk;
use super::customize;
use super::{accent_sheet_keeping, manifest, press, type_into};
use crate::model::settings::{AppIcon, IconChange};
use crate::settings::file;
use crate::theme::icon::{self, IconSettings, Tint};
use crate::theme::preset::{self, Appearance, LookPreset, Looks};
use crate::theme::setting::Rgb;
use crate::windows::Which;

/// Something else's key in `gui.json`, written as the app writes its own.
const OTHER: &str = "recent_folders";

fn gui_json() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gui.json");
    (dir, path)
}

/// The Look Preset's Accent in each half, as `gui.json` holds it.
fn kept_accents(path: &Path) -> Vec<String> {
    let stored: preset::Stored = file::read_at(path, preset::SETTING).expect("kept");
    let json = serde_json::to_value(stored).unwrap();
    ["light", "dark"].map(|half| json[half]["settings"]["accent"].to_string()).to_vec()
}

/// The Active look's sheet open, its Accent kept in `path` as it changes; black in `gui.json`.
fn look_sheet(path: &Path) -> Desk {
    let at = path.to_owned();
    let d = accent_sheet_keeping(move |preset: &LookPreset| file::keep_at(&at, preset::SETTING, Some(&preset.stored())).unwrap());
    // What the app started with is in the file, as startup leaves it.
    let looks = d.main.state.looks.borrow();
    file::keep_at(path, preset::SETTING, Some(&looks.preset().stored())).unwrap();
    drop(looks);
    d
}

/// A color typed into the sheet and applied.
fn edit_color(d: &mut Desk) {
    type_into(d, "edit-accent-both", "#3366ff");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
}

fn shown_accent(d: &Desk) -> String {
    d.main.state.looks.borrow().preset().light.settings.get("accent").map(|v| format!("{v:?}")).unwrap()
}

#[test]
fn the_look_sheet_shows_a_color_at_once_and_keeps_it_on_done() {
    let (_dir, path) = gui_json();
    let mut d = look_sheet(&path);
    let before = kept_accents(&path);
    let old = shown_accent(&d);
    edit_color(&mut d);
    assert_ne!(shown_accent(&d), old, "shown");
    assert_eq!(kept_accents(&path), before, "not kept while the sheet is open");
    press(&mut d, "edit-done");
    d.settle();
    assert_ne!(kept_accents(&path), before, "Done keeps it");
}

#[test]
fn the_look_sheets_cancel_and_escape_keep_nothing() {
    for escape in [false, true] {
        let (_dir, path) = gui_json();
        let mut d = look_sheet(&path);
        let before = std::fs::read(&path).unwrap();
        edit_color(&mut d);
        if escape {
            d.settings.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            d.settle();
        } else {
            press(&mut d, "edit-cancel");
            d.settle();
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "escape {escape}: nothing written");
    }
}

#[test]
fn quitting_with_the_look_sheet_open_keeps_nothing() {
    let (_dir, path) = gui_json();
    let mut d = look_sheet(&path);
    let before = kept_accents(&path);
    edit_color(&mut d);
    // ⌘Q writes nothing of the sheet: the file is what a start finds.
    assert_eq!(kept_accents(&path), before);
    drop(d);
    assert_eq!(kept_accents(&path), before);
}

#[test]
fn closing_the_settings_window_with_the_look_sheet_open_keeps_nothing() {
    let (_dir, path) = gui_json();
    let mut d = look_sheet(&path);
    let before = std::fs::read(&path).unwrap();
    let old = shown_accent(&d);
    edit_color(&mut d);
    d.close_button(Which::Settings);
    d.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing written");
    assert_eq!(shown_accent(&d), old, "and what it showed is back");
}

#[test]
fn another_write_to_gui_json_while_the_look_sheet_is_open_carries_none_of_its_edits() {
    let (_dir, path) = gui_json();
    let mut d = look_sheet(&path);
    let before = kept_accents(&path);
    edit_color(&mut d);
    file::keep_at(&path, OTHER, Some(&vec!["/tmp"])).unwrap();
    assert_eq!(kept_accents(&path), before);
    press(&mut d, "edit-done");
    d.settle();
    assert_ne!(kept_accents(&path), before, "Done keeps it");
    assert_eq!(file::read_at::<Vec<String>>(&path, OTHER), Some(vec!["/tmp".to_owned()]), "beside the other key");
}

/// The Customize sheet open, the icons' settings kept in `path` as they change.
fn customize_sheet(path: &Path) -> Desk {
    let (mut d, _) = customize::tab(960.0);
    let at = path.to_owned();
    d.settings.state.app_icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), move |change| {
        if let IconChange::Tints(tints) = change {
            file::keep_at(&at, icon::SETTING, Some(tints)).unwrap();
        }
    });
    d.settle();
    press(&mut d, "app-icon-customize");
    d
}

/// A tile color set in Light.
fn pick(d: &mut Desk) {
    press(d, "customize-tile-light-swatch");
    d.settings.update(|_, _| {});
    let (x, y) = d.settings.find("customize-tile-light-square").expect("its square");
    d.settings.pointer_down(x, y);
    d.settings.pointer_up(x, y);
    d.settle();
    assert!(matches!(d.settings.state.app_icon.tint("phantom", Appearance::Light, "tile"), Tint::Color(Rgb(_))), "shown");
}

fn kept_tints(path: &Path) -> Option<IconSettings> {
    file::read_at(path, icon::SETTING)
}

#[test]
fn the_customize_sheet_keeps_on_done_only() {
    // Done keeps; Cancel, Escape, the window closing and quitting keep nothing.
    for end in ["done", "cancel", "escape", "window", "quit"] {
        let (_dir, path) = gui_json();
        let mut d = customize_sheet(&path);
        pick(&mut d);
        file::keep_at(&path, OTHER, Some(&vec!["/tmp"])).unwrap();
        assert_eq!(kept_tints(&path), None, "{end}: not kept while the sheet is open, nor by another write");
        match end {
            "done" => customize::press(&mut d, "customize-done"),
            "cancel" => customize::press(&mut d, "customize-cancel"),
            "escape" => {
                // The picker closes first, then the sheet is cancelled.
                d.settings.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
                d.settle();
                d.settings.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            }
            "window" => d.close_button(Which::Settings),
            _ => {}
        }
        d.settle();
        let kept = kept_tints(&path);
        if end == "done" {
            assert!(kept.is_some_and(|k| !k.is_empty()), "Done keeps it");
        } else {
            assert_eq!(kept.filter(|k| !k.is_empty()), None, "{end}: nothing kept");
        }
    }
}
