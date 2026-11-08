//! A theme's look, as far as it exists so far: how each [`Icon`] is drawn. A theme overrides any
//! icon or none; what it leaves out comes from Phantom, the default theme, which draws them all
//! ([ADR 0004]).
//!
//! [ADR 0004]: ../../../../docs/adr/0004-themes-override-building-blocks.md

mod icons;

use super::Icon;

#[derive(Clone, Copy)]
pub struct Theme {
    /// The icons this theme draws itself.
    icons: fn(Icon) -> Option<&'static rune::Icon>,
}

impl Theme {
    /// The default theme: complete.
    pub const PHANTOM: Theme = Theme { icons: icons::phantom };

    /// A theme that draws the icons `icons` returns, and Phantom's for the rest.
    #[cfg(test)]
    pub const fn overriding(icons: fn(Icon) -> Option<&'static rune::Icon>) -> Theme {
        Theme { icons }
    }

    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        (self.icons)(icon).unwrap_or_else(|| icons::phantom(icon).expect("Phantom draws every icon"))
    }
}
