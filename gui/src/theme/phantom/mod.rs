//! Phantom, the default theme: complete, and the end of every theme's parent chain. Its tokens
//! are rune-kit's neutral kit, light or dark as its Tone says; its blocks are the building blocks
//! as `ui` writes them, with its own icons; its App icon is the bundle's own.

mod icons;

use rune_kit::Kit;

#[cfg(test)]
use super::Look;
use super::setting::{Choice, Kind, Setting, Value, Values};
use super::{Built, Colours, Spacing, Theme, ThemeLook};
use crate::ui::Icon;

pub struct Phantom;

pub static PHANTOM: Phantom = Phantom;

/// Light or Dark.
pub const TONE: &str = "tone";

/// How much room things leave: Compact or Comfortable.
pub const DENSITY: &str = "density";

static SETTINGS: [Setting; 2] = [
    Setting {
        key: TONE,
        label: "Tone",
        kind: Kind::Choice { choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }], default: "light" },
    },
    Setting {
        key: DENSITY,
        label: "Density",
        kind: Kind::Choice {
            choices: &[Choice { id: "compact", label: "Compact" }, Choice { id: "comfortable", label: "Comfortable" }],
            default: "comfortable",
        },
    },
];

impl Theme for Phantom {
    fn id(&self) -> &'static str {
        "phantom"
    }

    fn name(&self) -> &'static str {
        "Phantom"
    }

    fn parent(&self) -> Option<&'static dyn Theme> {
        None
    }

    fn settings(&self) -> &'static [Setting] {
        &SETTINGS
    }

    fn build(&self, settings: &Values) -> Built {
        let dark = settings.get(TONE) == Some(Value::Choice("dark"));
        let tokens = Kit::neutral(dark);
        Built {
            colours: Colours::of(&tokens),
            spacing: if settings.get(DENSITY) == Some(Value::Choice("compact")) { Spacing::compact() } else { Spacing::phantom() },
            tokens,
            blocks: Default::default(),
            screens: Default::default(),
            app_icon: "phantom",
        }
    }
}

/// How Phantom draws `icon`: every one.
pub fn icon(icon: Icon) -> &'static rune::Icon {
    icons::icon(icon)
}

/// Phantom in the Tone `dark` says.
pub fn theme_look(dark: bool) -> ThemeLook {
    let mut settings = Values::defaults(&SETTINGS);
    settings.set(TONE, Value::Choice(if dark { "dark" } else { "light" }));
    ThemeLook { theme: &PHANTOM, settings }
}

/// [`theme_look`], resolved.
#[cfg(test)]
pub fn look(dark: bool) -> Look {
    Look::build(&theme_look(dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spacing(density: Option<&'static str>) -> Spacing {
        let mut look = theme_look(false);
        if let Some(d) = density {
            assert!(look.settings.set(DENSITY, Value::Choice(d)));
        }
        *Look::build(&look).ui.spacing()
    }

    #[test]
    fn comfortable_is_todays_spacing_and_the_default() {
        assert_eq!(spacing(None), Spacing::phantom());
        assert_eq!(spacing(Some("comfortable")), Spacing::phantom());
    }

    #[test]
    fn compact_changes_the_sizes() {
        let c = spacing(Some("compact"));
        assert_eq!(c, Spacing::compact());
        assert_eq!((c.row, c.table_header, c.toolbar, c.margin), (22.0, 24.0, 32.0, 8.0));
    }
}
