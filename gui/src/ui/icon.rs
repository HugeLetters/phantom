//! The icons screens can ask for. A screen names an [`Icon`]; the theme decides how it's drawn
//! (see `theme`), so a theme can swap any icon without touching a screen. Add a variant when a
//! screen first needs it, with its drawing in the Phantom theme.

use super::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Two overlapping sheets: copy something.
    Copy,
    /// A tick: done.
    Check,
}

impl Ui {
    /// How the theme draws `icon`; Rune's icon, for anything that draws one.
    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        self.theme.icon(icon)
    }
}
