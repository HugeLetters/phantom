//! The icons screens can ask for. A screen names an [`Icon`]; the theme decides how it's drawn
//! (each icon is a block of its own, see `blocks`), so a theme can swap any icon without touching
//! a screen. Add a variant when a screen first needs it, with its drawing in the Phantom theme.

use lifecycle::every;

use super::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Two overlapping sheets: copy something.
    Copy,
    /// A triangle with an exclamation mark: a warning.
    Warning,
    /// A check mark: the current one of several values, in a menu's icon gutter.
    Check,
}

every!(Icon { Icon::Copy => [Icon::Copy], Icon::Warning => [Icon::Warning], Icon::Check => [Icon::Check] });

impl Ui {
    /// How the theme draws `icon`; Rune's icon, for anything that draws one.
    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        self.blocks.icon(icon)
    }
}
