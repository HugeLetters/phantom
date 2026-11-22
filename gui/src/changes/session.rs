//! A sheet's undo session (#87): the rules every sheet over Settings that edits things shares, the
//! Look Preset editor and the App icon's sheet.
//!
//! - While the sheet is open, its steps are its own. ⌘Z (and Edit ▸ Undo) take only them, newest
//!   first; the sheet stays open, showing what each went back to.
//! - When they've run out, ⌘Z does nothing: nothing older is taken while the sheet is open.
//! - Its steps don't count against the last 20 changes the list keeps, so a long visit pushes no
//!   older change off.
//! - Once it closes, the steps leave the list and, if the visit changed anything, become one step
//!   (the editor's Done, the icon sheet's Done, Escape, ⌘W or the window closing). The editor's
//!   Cancel gives none: it puts everything back as it was when the sheet opened.
//!
//! What a sheet makes of a step, and what it folds into, is its own: it says which settings are
//! its ([`Sheet::owns`]) and what the folded step undoes to ([`super::sheet_closing`]).

use super::machine::{self, AppSetting};

/// The sheets that have a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    /// A Look Preset's editor: its theme settings.
    Editor,
    /// The App icon's sheet: its picks.
    Icons,
}

impl Sheet {
    /// Whether a change of `of` made while the sheet is open is a step of it.
    fn owns(self, of: &AppSetting) -> bool {
        matches!((self, of), (Sheet::Editor, AppSetting::Theme { .. }) | (Sheet::Icons, AppSetting::Icon))
    }
}

/// An open sheet's steps, oldest first, by the id of each change.
pub struct Session {
    sheet: Sheet,
    ids: Vec<u64>,
}

impl Session {
    pub fn new(sheet: Sheet) -> Self {
        Self { sheet, ids: Vec::new() }
    }

    /// Whether a change of `of` is a step of this session.
    pub fn owns(&self, of: &AppSetting) -> bool {
        self.sheet.owns(of)
    }

    /// A step was made: its change `id`.
    pub fn push(&mut self, id: u64) {
        self.ids.push(id);
    }

    /// The newest step ⌘Z hasn't taken: with none, ⌘Z does nothing.
    pub fn top(&self, state: &machine::State) -> Option<u64> {
        state.list.iter().rev().find_map(|ch| match ch {
            machine::Change::App { id, press: machine::Press::No, .. } if self.ids.contains(id) => Some(*id),
            _ => None,
        })
    }

    /// The steps, to leave the list when the sheet closes.
    pub fn into_ids(self) -> Vec<u64> {
        self.ids
    }
}
