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

/// How round the corners are: Square, Small or Round.
pub const CORNERS: &str = "corners";

static SETTINGS: [Setting; 2] = [
    Setting {
        key: TONE,
        label: "Tone",
        kind: Kind::Choice { choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }], default: "light" },
    },
    Setting {
        key: CORNERS,
        label: "Corners",
        kind: Kind::Choice {
            choices: &[
                Choice { id: "square", label: "Square" },
                Choice { id: "small", label: "Small" },
                Choice { id: "round", label: "Round" },
            ],
            default: "small",
        },
    },
];

/// The radius tokens `corners` (one of [`CORNERS`]' choices) gives: Small is the kit's own.
fn radius(corners: Option<Value>) -> rune_kit::Radius {
    match corners {
        Some(Value::Choice("square")) => rune_kit::Radius { xs: 0.0, sm: 0.0, md: 0.0, lg: 0.0, xl: 0.0 },
        Some(Value::Choice("round")) => rune_kit::Radius { xs: 4.0, sm: 7.0, md: 10.0, lg: 14.0, xl: 18.0 },
        _ => Kit::neutral(false).radius,
    }
}

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
        let mut tokens = Kit::neutral(dark);
        tokens.radius = radius(settings.get(CORNERS));
        Built {
            colours: Colours::of(&tokens),
            spacing: Spacing::phantom(),
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

    fn radii(corners: &str, dark: bool) -> [f32; 5] {
        let mut look = theme_look(dark);
        assert!(look.settings.set(
            CORNERS,
            Value::Choice(match corners {
                "square" => "square",
                "round" => "round",
                _ => "small",
            })
        ));
        let r = Look::build(&look).ui.tokens().radius;
        [r.xs, r.sm, r.md, r.lg, r.xl]
    }

    #[test]
    fn small_is_the_kits_own_radii_and_the_default() {
        for dark in [false, true] {
            let kit = Kit::neutral(dark).radius;
            assert_eq!(radii("small", dark), [kit.xs, kit.sm, kit.md, kit.lg, kit.xl]);
            assert_eq!(Look::build(&theme_look(dark)).ui.tokens().radius.md, kit.md);
        }
        assert_eq!(radii("small", false), [2.0, 4.0, 6.0, 8.0, 10.0]);
    }

    #[test]
    fn square_and_round_change_every_radius_token() {
        for dark in [false, true] {
            assert_eq!(radii("square", dark), [0.0; 5]);
            assert_eq!(radii("round", dark), [4.0, 7.0, 10.0, 14.0, 18.0]);
        }
    }
}
