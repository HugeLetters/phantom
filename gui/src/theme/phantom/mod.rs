//! Phantom, the default theme: complete, and the end of every theme's parent chain. Its tokens
//! are rune-kit's neutral kit, light or dark as its Tone says; its blocks are the building blocks
//! as `ui` writes them, with its own icons; its App icon is the bundle's own.

mod icons;

use rune_kit::Kit;

#[cfg(test)]
use super::Look;
use super::setting::{Choice, Kind, Setting, Value, Values};
use super::{Built, Theme, ThemeLook};
use crate::ui::Icon;

pub struct Phantom;

pub static PHANTOM: Phantom = Phantom;

/// Light or Dark.
pub const TONE: &str = "tone";

static SETTINGS: [Setting; 1] = [Setting {
    key: TONE,
    label: "Tone",
    kind: Kind::Choice { choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }], default: "light" },
}];

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
        Built { tokens: Kit::neutral(dark), blocks: Default::default(), screens: Default::default(), app_icon: "phantom" }
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
