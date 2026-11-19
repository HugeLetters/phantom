//! The saved Look Presets' controls (#228), in the Appearance tab: a half's chip applies it to the
//! Active look, Apply both applies both; Edit… opens the saved preset in a sheet over Settings,
//! which edits it with the Active look's editor, and while it's open both windows show it (a
//! preview, never kept); Delete drops it; Save as… saves the Active look under a new name, or, asked
//! first, over the saved preset that has it (Replace). Each that changes something is one step of
//! Undo, which puts the Active look and the saved presets back as they were.

use rune::{TextEdit, Tx};

use super::{At, PresetSheet, Row, SaveAs, SettingsWindow, Tab, app_changed, follow_focus, leave, looks_changed};
use crate::changes::machine::AppSetting;
use crate::changes::{PresetChange, PresetVerb, Presets, Was};
use crate::theme::preset::Appearance;
use crate::theme::saved::{Name, NameProblem, SavedPreset};
use client::display;

/// The saved presets' name in `gui.json`, and the key of their section.
pub const SETTING: &str = crate::theme::saved::SETTING;

/// The key of the Replace that Save as… asks with, which takes the focus as it's asked.
pub const REPLACE: &str = "save-as-replace";

/// Applies the saved preset `i` to the Active look: both halves, or `half` alone.
pub fn apply(s: &mut SettingsWindow, i: usize, half: Option<Appearance>) {
    follow_focus(s);
    let Some(saved) = s.saved.list().get(i).cloned() else { return };
    let was = Presets::of(&s.looks.borrow(), &s.saved);
    let halves = match half {
        Some(half) => vec![half],
        None => Appearance::BOTH.to_vec(),
    };
    for appearance in halves {
        s.looks.borrow_mut().set(appearance, saved.preset.get(appearance).clone());
    }
    // The Active look's fields show what's applied, whatever was typed in them.
    if let Some(sheet) = s.sheet.as_mut().filter(|_| Presets::of(&s.looks.borrow(), &s.saved) != was) {
        sheet.editor.colours.clear();
        sheet.refused.retain(|r| !matches!(r.row, Row::Colour(At::Active, ..)));
    }
    changed(s, was, PresetVerb::Apply(half), saved.name);
}

/// Drops the saved preset `i`.
pub fn delete(s: &mut SettingsWindow, i: usize) {
    follow_focus(s);
    let was = Presets::of(&s.looks.borrow(), &s.saved);
    let Some(saved) = was.saved.get(i).cloned() else { return };
    let mut list = was.saved.clone();
    list.remove(i);
    s.saved.set(list);
    changed(s, was, PresetVerb::Delete, saved.name);
}

/// Save as…: its field, empty and focused.
pub fn save_as(s: &mut SettingsWindow) {
    follow_focus(s);
    let Some(sheet) = &mut s.sheet else { return };
    let mut name = TextEdit::new("");
    name.focused = true;
    sheet.save_as = Some(SaveAs { name, replace: None });
    sheet.refused.retain(|r| r.row != Row::PresetName);
}

/// Return in Save as…'s field, or its Save: the Active look saved under the name typed, last; a
/// name a saved preset has asks whether to replace it, and a blank one is refused under the field.
pub fn save(s: &mut SettingsWindow, tx: &mut Tx) {
    follow_focus(s);
    let Some(sheet) = &mut s.sheet else { return };
    let Some(text) = sheet.save_as.as_ref().map(|a| a.name.text.clone()) else { return };
    match s.saved.new_name(&text) {
        Err(NameProblem::Blank) => sheet.refuse(Row::PresetName, display::preset_name_blank()),
        // The question's Replace takes the focus.
        Err(NameProblem::Taken(name)) => {
            if let Some(save_as) = &mut sheet.save_as {
                save_as.replace = Some(name);
                save_as.name.focused = false;
            }
            // Its Replace takes the focus, until something else asks for it.
            sheet.focus = Some(REPLACE);
            tx.blur();
        }
        Ok(name) => {
            sheet.save_as = None;
            sheet.refused.retain(|r| r.row != Row::PresetName);
            let was = Presets::of(&s.looks.borrow(), &s.saved);
            let mut list = was.saved.clone();
            list.push(SavedPreset { name: name.clone(), preset: was.active.clone() });
            s.saved.set(list);
            changed(s, was, PresetVerb::Save, name);
        }
    }
}

/// Replace, asked by Save as…: the Active look saved over the saved preset with its name, in its
/// place; or, gone meanwhile (an undo took it), saved under the name, last.
pub fn replace(s: &mut SettingsWindow) {
    follow_focus(s);
    let Some(sheet) = &mut s.sheet else { return };
    let Some(name) = sheet.save_as.take().and_then(|a| a.replace) else { return };
    let was = Presets::of(&s.looks.borrow(), &s.saved);
    let mut list = was.saved.clone();
    let verb = match list.iter_mut().find(|p| p.name == name) {
        Some(saved) => {
            saved.preset = was.active.clone();
            PresetVerb::Replace
        }
        None => {
            list.push(SavedPreset { name: name.clone(), preset: was.active.clone() });
            PresetVerb::Save
        }
    };
    s.saved.set(list);
    changed(s, was, verb, name);
}

/// Cancel, asked by Save as…: back to the name, to change it.
pub fn cancel_replace(s: &mut SettingsWindow) {
    follow_focus(s);
    if let Some(save_as) = s.sheet.as_mut().and_then(|sheet| sheet.save_as.as_mut()) {
        save_as.replace = None;
        save_as.name.focused = true;
    }
}

/// A change ⌘Z takes back, if it changed something: the Look Presets were `was`.
fn changed(s: &mut SettingsWindow, was: Presets, verb: PresetVerb, name: Name) {
    looks_changed(s);
    if Presets::of(&s.looks.borrow(), &s.saved) != was {
        app_changed(s, AppSetting::Presets, Was::Presets(was, PresetChange { verb, name }));
    }
}

/// Edit…: the saved preset `i` in a sheet over Settings, shown in both windows while it's open.
pub fn edit(s: &mut SettingsWindow, i: usize) {
    follow_focus(s);
    let Some(name) = s.saved.list().get(i).map(|p| p.name.clone()) else { return };
    open_sheet(s, name);
}

/// The sheet of the saved preset `name`, unless it's open on it already.
fn open_sheet(s: &mut SettingsWindow, name: Name) {
    let Some(sheet) = &mut s.sheet else { return };
    if sheet.preset.as_ref().is_some_and(|p| p.name == name) {
        return;
    }
    sheet.preset = Some(PresetSheet { name, editor: Default::default(), focus: None });
    looks_changed(s);
}

/// The saved preset's sheet closes (Done, Escape, ⌘W): the field being edited in it applied first,
/// then the preview goes.
pub fn close(s: &mut SettingsWindow) {
    let Some(sheet) = &mut s.sheet else { return };
    if sheet.preset.is_none() {
        return;
    }
    let rows: Vec<Row> = sheet
        .text_rows()
        .into_iter()
        .filter(|row| matches!(row, Row::Colour(At::Sheet, ..)))
        .filter(|&row| sheet.had_focus.contains(&row) || sheet.field(row).is_some_and(|f| f.focused))
        .collect();
    sheet.had_focus.retain(|row| !matches!(row, Row::Colour(At::Sheet, ..)));
    for row in rows {
        if let Some(field) = s.sheet.as_mut().and_then(|sheet| sheet.field_mut(row)) {
            field.focused = false;
        }
        leave(s, row);
    }
    if let Some(sheet) = &mut s.sheet {
        sheet.preset = None;
        sheet.refused.retain(|r| !matches!(r.row, Row::Colour(At::Sheet, ..)));
    }
    preview(s);
}

/// Both windows show the saved preset the sheet's open on, or, without one, the Active look.
pub fn preview(s: &mut SettingsWindow) {
    let shown = s.sheet.as_ref().and_then(|sheet| sheet.preset.as_ref()).and_then(|p| s.saved.get(&p.name)).map(|p| p.preset.clone());
    if s.looks.borrow().previewed() == shown.as_ref() {
        return;
    }
    s.looks.borrow_mut().preview(shown.as_ref());
    // The main window draws it once it runs what's posted.
    s.main.post(|_, _| {});
}

/// After an undo, the theme setting `key` of the saved preset `name`, in its sheet (opened, and
/// Settings with it, if closed), its control focused: a colour's field through its text, any other
/// as the sheet's first in the Tab order.
pub fn show_setting(s: &mut SettingsWindow, name: Name, key: &'static str, tx: &mut Tx) {
    if s.sheet.is_none() {
        super::open(s);
    }
    s.window.raise();
    let Some(sheet) = &mut s.sheet else { return };
    sheet.tab = Tab::Appearance;
    open_sheet(s, name);
    let Some(preset) = s.sheet.as_mut().and_then(|sheet| sheet.preset.as_mut()) else { return };
    preset.focus = None;
    for field in &mut preset.editor.colours {
        field.text.focused = false;
    }
    match preset.editor.colours.iter_mut().find(|f| f.key == key) {
        Some(field) => field.text.focused = true,
        None => {
            preset.focus = Some(key);
            tx.blur();
        }
    }
}

/// The setting whose control an undo of `change` focuses: an Apply's, the Active look it changed;
/// another's, the saved presets'.
pub fn focus_of(change: &PresetChange) -> &'static str {
    match change.verb {
        PresetVerb::Apply(_) => crate::theme::preset::SETTING,
        PresetVerb::Save | PresetVerb::Replace | PresetVerb::Delete => SETTING,
    }
}
