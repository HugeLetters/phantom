//! The saved Look Presets' controls (#228), in the Appearance tab: pick a saved preset into the
//! Active Look Preset, or one half of it into that half alone; update a saved one from the active
//! one; save the active one as a new one, under the name typed (Return in the field, or Save);
//! delete one. Each that changes something is one step of Undo, which puts the active preset and
//! the saved ones back as they were ([`show`]).

use rune::TextEdit;

use super::{Row, SettingsWindow, app_changed, follow_focus, show_preset};
use crate::changes::machine::AppSetting;
use crate::changes::{PresetChange, PresetVerb, Was};
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::saved::{NameProblem, SavedPreset};
use client::display;

/// The saved presets' name in `gui.json`, and the key of their section.
pub const SETTING: &str = crate::theme::saved::SETTING;

/// Copies the saved preset `i` into the active one: both halves, or `half` alone.
pub fn pick(s: &mut SettingsWindow, i: usize, half: Option<Appearance>) {
    follow_focus(s);
    let Some(saved) = s.saved.list().get(i).cloned() else { return };
    let active = s.looks.borrow().preset().clone();
    let mut picked = active.clone();
    let halves = match half {
        Some(half) => vec![half],
        None => Appearance::BOTH.to_vec(),
    };
    for appearance in halves {
        *picked.get_mut(appearance) = saved.preset.get(appearance).clone();
    }
    if picked == active {
        return;
    }
    let list = s.saved.list().to_vec();
    show_preset(s, picked);
    changed(s, active, list, PresetVerb::Pick(half), &saved);
}

/// Makes the saved preset `i` the active one.
pub fn update(s: &mut SettingsWindow, i: usize) {
    follow_focus(s);
    let active = s.looks.borrow().preset().clone();
    let was = s.saved.list().to_vec();
    let Some(saved) = was.get(i).cloned() else { return };
    if saved.preset == active {
        return;
    }
    let mut list = was.clone();
    list[i].preset = active.clone();
    s.saved.set(list);
    changed(s, active, was, PresetVerb::Update, &saved);
}

/// Drops the saved preset `i`.
pub fn delete(s: &mut SettingsWindow, i: usize) {
    follow_focus(s);
    let was = s.saved.list().to_vec();
    let Some(saved) = was.get(i).cloned() else { return };
    let mut list = was.clone();
    list.remove(i);
    s.saved.set(list);
    let active = s.looks.borrow().preset().clone();
    changed(s, active, was, PresetVerb::Delete, &saved);
}

/// Saves the active preset as a new one, last, under the name typed, and empties the field; or
/// says, under it, why the name can't be one.
pub fn save_new(s: &mut SettingsWindow) {
    follow_focus(s);
    let Some(sheet) = &mut s.sheet else { return };
    let name = match s.saved.new_name(&sheet.preset_name.text) {
        Ok(name) => name,
        Err(problem) => {
            let why = match problem {
                NameProblem::Blank => display::preset_name_blank().to_owned(),
                NameProblem::Taken => display::preset_name_taken(sheet.preset_name.text.trim()),
            };
            return sheet.refuse(Row::PresetName, why);
        }
    };
    let focused = sheet.preset_name.focused;
    sheet.preset_name = TextEdit::new("");
    sheet.preset_name.focused = focused;
    sheet.refused.retain(|r| r.row != Row::PresetName);
    let active = s.looks.borrow().preset().clone();
    let was = s.saved.list().to_vec();
    let saved = SavedPreset { name, preset: active.clone() };
    let mut list = was.clone();
    list.push(saved.clone());
    s.saved.set(list);
    changed(s, active, was, PresetVerb::Save, &saved);
}

/// A change ⌘Z takes back: the active preset and the saved ones were `active` and `saved`.
fn changed(s: &SettingsWindow, active: LookPreset, saved: Vec<SavedPreset>, verb: PresetVerb, of: &SavedPreset) {
    let change = PresetChange { verb, name: of.name.clone() };
    app_changed(s, AppSetting::Presets, Was::Presets(active, saved, change));
}

/// The active preset and the saved ones back to `active` and `saved`, as an undo does.
pub fn show(s: &mut SettingsWindow, active: LookPreset, saved: Vec<SavedPreset>) {
    show_preset(s, active);
    s.saved.set(saved);
}

/// The setting whose control an undo of `change` focuses: a pick's, the Theme section it changed;
/// another's, the saved presets'.
pub fn focus_of(change: &PresetChange) -> &'static str {
    match change.verb {
        PresetVerb::Pick(_) => crate::theme::preset::SETTING,
        PresetVerb::Save | PresetVerb::Update | PresetVerb::Delete => SETTING,
    }
}
