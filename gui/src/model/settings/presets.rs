//! The Look Presets' controls (#228), in the Appearance tab: a saved preset's ◐ chip applies both
//! halves to the Active look, its ☀ or ☾ chip one; Edit… opens a Look Preset, the Active look's or
//! a saved one, in a sheet over Settings. The sheet on a saved preset edits it, and while it's open
//! both windows show it (a preview, never kept); on the Active look, it edits the real settings.
//! Delete drops a saved preset; Save as… saves the Active look under a new name, or, asked first,
//! over the saved preset that has it (Replace). Each that changes something is one step of Undo,
//! which puts the Active look and the saved presets back as they were.

use rune::{TextEdit, Tx};

use super::{Chip, EditSheet, Row, SaveAs, SettingsWindow, Side, Tab, app_changed, follow_focus, leave, looks_changed};
use crate::changes::machine::AppSetting;
use crate::changes::{PresetChange, PresetVerb, Presets, Target, Was};
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

/// Edit… on the saved preset `i`: its sheet over Settings, shown in both windows while it's open.
pub fn edit(s: &mut SettingsWindow, i: usize) {
    follow_focus(s);
    let Some(name) = s.saved.list().get(i).map(|p| p.name.clone()) else { return };
    open_sheet(s, Target::Saved(name));
}

/// Edit… on the Active look: its sheet over Settings; its edits are the real settings.
pub fn edit_active(s: &mut SettingsWindow) {
    follow_focus(s);
    open_sheet(s, Target::Active);
}

/// The sheet of `target`, unless it's open on it already (one open on another is done first):
/// its changes are its own from now on, for ⌘Z, until it closes.
fn open_sheet(s: &mut SettingsWindow, target: Target) {
    let Some(sheet) = &mut s.sheet else { return };
    if sheet.target() == Some(&target) {
        return;
    }
    close(s);
    let opened = Presets::of(&s.looks.borrow(), &s.saved);
    let Some(sheet) = &mut s.sheet else { return };
    sheet.edit = Some(EditSheet { target, editor: Default::default(), opened, focus: None });
    s.main.post(|s, _| crate::changes::sheet_opened(s, crate::changes::session::Sheet::Editor));
    looks_changed(s);
}

/// Cancel, or Escape: the sheet closes, the Look Presets back as they were when it opened, what's
/// typed in it dropped, and none of its changes left for ⌘Z.
pub fn cancel(s: &mut SettingsWindow) {
    let Some(sheet) = &mut s.sheet else { return };
    let Some(edit) = sheet.edit.take() else { return };
    sheet.had_focus.retain(|row| !matches!(row, Row::Colour(..)));
    sheet.refused.retain(|r| !matches!(r.row, Row::Colour(..)));
    if Presets::of(&s.looks.borrow(), &s.saved) != edit.opened {
        super::restore(s, edit.opened);
    }
    s.main.post(|s, _| crate::changes::sheet_closed(s, None));
    preview(s);
}

/// The sheet closes (Done, ⌘W, the window closing): the field being edited in it applied first,
/// then the preview goes. What changed in it since it opened is one step of Undo.
pub fn close(s: &mut SettingsWindow) {
    let Some(sheet) = &mut s.sheet else { return };
    if sheet.edit.is_none() {
        return;
    }
    let rows: Vec<Row> = sheet
        .text_rows()
        .into_iter()
        .filter(|row| matches!(row, Row::Colour(..)))
        .filter(|&row| sheet.had_focus.contains(&row) || sheet.field(row).is_some_and(|f| f.focused))
        .collect();
    sheet.had_focus.retain(|row| !matches!(row, Row::Colour(..)));
    for row in rows {
        if let Some(field) = s.sheet.as_mut().and_then(|sheet| sheet.field_mut(row)) {
            field.focused = false;
        }
        leave(s, row);
    }
    let Some(edit) = s.sheet.as_mut().and_then(|sheet| sheet.edit.take()) else { return };
    if let Some(sheet) = &mut s.sheet {
        sheet.refused.retain(|r| !matches!(r.row, Row::Colour(..)));
    }
    let into = (Presets::of(&s.looks.borrow(), &s.saved) != edit.opened)
        .then(|| (crate::changes::machine::AppSetting::Edit, Was::Edit(edit.opened, edit.target)));
    s.main.post(move |s, _| crate::changes::sheet_closed(s, into));
    preview(s);
}

/// Both windows show the saved preset the sheet's open on, or, without one (closed, or on the
/// Active look, which is shown as it is), the Active look.
pub fn preview(s: &mut SettingsWindow) {
    let target = s.sheet.as_ref().and_then(|sheet| sheet.target().cloned());
    let shown = match target {
        Some(Target::Saved(name)) => s.saved.get(&name).map(|p| p.preset.clone()),
        Some(Target::Active) | None => None,
    };
    if s.looks.borrow().previewed() == shown.as_ref() {
        return;
    }
    s.looks.borrow_mut().preview(shown.as_ref());
    // The main window draws it once it runs what's posted.
    s.main.post(|_, _| {});
}

/// After an undo, the theme setting `key` of `target` set on `side`, in its sheet (opened, and
/// Settings with it, if closed), its row joined or split as it was set, its control focused: a colour's
/// field through its text, any other as the sheet's first in the Tab order.
pub fn show_setting(s: &mut SettingsWindow, target: Target, key: &'static str, side: Side, tx: &mut Tx) {
    if s.sheet.is_none() {
        super::open(s);
    }
    s.window.raise();
    let Some(sheet) = &mut s.sheet else { return };
    sheet.tab = Tab::Appearance;
    super::close_icons(s);
    open_sheet(s, target);
    let Some(edit) = s.sheet.as_mut().and_then(|sheet| sheet.edit.as_mut()) else { return };
    edit.focus = None;
    edit.editor.menu = None;
    // Its row as it was set: joined, or split.
    match side {
        Side::Both => edit.editor.split.remove(key),
        Side::Only(_) => edit.editor.split.insert(key),
    };
    looks_changed(s);
    let Some(edit) = s.sheet.as_mut().and_then(|sheet| sheet.edit.as_mut()) else { return };
    for field in &mut edit.editor.colours {
        field.text.focused = false;
    }
    let at = edit.editor.colours.iter().position(|f| f.key == key && f.side == side);
    let at = at.or_else(|| edit.editor.colours.iter().position(|f| f.key == key));
    match at.map(|i| &mut edit.editor.colours[i]) {
        Some(field) => field.text.focused = true,
        None => {
            edit.focus = Some((key, side));
            tx.blur();
        }
    }
}

/// After an undo of `change` of the Look Presets, Settings at them with the control that did it
/// focused: the chip an Apply used (◐, ☀ or ☾), or, for a save, a replace or a delete, that
/// preset's ◐; the Active look's Edit… when the preset is gone.
pub fn show_change(s: &mut SettingsWindow, change: &PresetChange, tx: &mut Tx) {
    let half = match change.verb {
        PresetVerb::Apply(half) => half,
        PresetVerb::Save | PresetVerb::Replace | PresetVerb::Delete => None,
    };
    let key = if s.saved.get(&change.name).is_some() { SETTING } else { crate::theme::preset::SETTING };
    super::show(s, key, Some(Chip { name: change.name.clone(), half }), Some(tx));
}
