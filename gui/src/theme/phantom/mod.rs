//! Phantom, the default theme: complete, and the end of every theme's parent chain. Its tokens
//! are rune-kit's neutral kit, light or dark as its Tone says, recolored by its Colors (`colors`);
//! its blocks are the building blocks as `ui` writes them, with its own icons; its App icon is the
//! bundle's own, or drawn with a Tile color (`app_icon`).

pub mod app_icon;
pub mod colors;
mod icons;

use std::sync::LazyLock;

use rune_kit::Kit;

#[cfg(test)]
use super::Look;
use super::setting::{Choice, Kind, Section, Setting, Show, Value, Values};
use super::{Built, Colors, Motion, Spacing, Theme, ThemeLook};
use crate::ui::Icon;

pub struct Phantom;

pub static PHANTOM: Phantom = Phantom;

/// Light or Dark.
pub const TONE: &str = "tone";

/// How round the corners are: Square, Small or Round.
pub const CORNERS: &str = "corners";

/// Whether things move: On, Off, or as the Mac's Reduce Motion says.
pub const MOTION: &str = "motion";

/// Corners and Motion's section.
static LOOK: Section = Section { key: "look", title: "Look", folds: false };

/// Tone, then Look (Corners, Motion), then Colors: the bases, then every other color.
static SETTINGS: LazyLock<Vec<Setting>> = LazyLock::new(|| {
    let mut settings = vec![
        Setting::new(
            TONE,
            "Tone",
            Kind::Choice {
                choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }],
                default: "light",
                show: Show::Segments,
            },
        )
        .with_synonyms(&["theme", "dark mode", "light mode", "night"]),
        Setting::new(
            CORNERS,
            "Corners",
            Kind::Choice {
                choices: &[
                    Choice { id: "square", label: "Square" },
                    Choice { id: "small", label: "Small" },
                    Choice { id: "round", label: "Round" },
                ],
                default: "small",
                show: Show::Dropdown,
            },
        )
        .in_section(&LOOK)
        .with_synonyms(&["radius", "rounded", "round", "square"]),
        Setting::new(
            MOTION,
            "Motion",
            Kind::Choice {
                choices: &[
                    Choice { id: "follow", label: "Follow Reduce Motion" },
                    Choice { id: "on", label: "On" },
                    Choice { id: "off", label: "Off" },
                ],
                default: "follow",
                show: Show::Dropdown,
            },
        )
        .in_section(&LOOK)
        .with_synonyms(&["animation", "reduce motion", "animations"]),
    ];
    settings.extend(colors::settings());
    settings
});

/// The radius tokens `corners` (one of [`CORNERS`]' choices) gives: Small is the kit's own.
fn radius(corners: Option<Value>) -> rune_kit::Radius {
    match corners {
        Some(Value::Choice("square")) => rune_kit::Radius { xs: 0.0, sm: 0.0, md: 0.0, lg: 0.0, xl: 0.0 },
        Some(Value::Choice("round")) => rune_kit::Radius { xs: 4.0, sm: 7.0, md: 10.0, lg: 14.0, xl: 18.0 },
        _ => Kit::neutral(false).radius,
    }
}

/// What [`MOTION`] says; Follow, the default, is what an unset or unknown value means.
fn motion(value: Option<Value>) -> Motion {
    match value {
        Some(Value::Choice("on")) => Motion::On,
        Some(Value::Choice("off")) => Motion::Off,
        _ => Motion::Follow,
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
        let resolved = colors::apply(&mut tokens, settings);
        Built {
            colors: Colors::of(&tokens),
            spacing: Spacing::phantom(),
            tokens,
            blocks: Default::default(),
            screens: Default::default(),
            app_icon: "phantom",
            motion: motion(settings.get(MOTION)),
            resolved,
        }
    }

    fn icons(&self) -> &'static [super::icon::ThemeIcon] {
        &app_icon::ICONS
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
    use std::cell::Cell;
    use std::rc::Rc;

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

    /// Whether a window drawn in `motion`'s look reduces motion, when the Mac does or doesn't.
    fn reduced(motion: &str, system: bool) -> bool {
        let mut look = theme_look(false);
        assert!(look.settings.set(
            MOTION,
            Value::Choice(match motion {
                "on" => "on",
                "off" => "off",
                _ => "follow",
            })
        ));
        let seen = Rc::new(Cell::new(None));
        let state = (Rc::new(Look::build(&look)), seen.clone());
        let view = |s: &(Rc<Look>, Rc<Cell<Option<bool>>>), cx: &mut rune::Cx<(Rc<Look>, Rc<Cell<Option<bool>>>)>| {
            s.0.ui.motion(cx);
            s.1.set(Some(cx.is_reduced_motion()));
            rune::el()
        };
        let mut rt = rune::Runtime::new(state, view, (100.0, 100.0), rune::Env { system_dark: false, scale: 1.0 });
        rt.set_system_reduced_motion(system);
        rt.frame(0.0);
        seen.get().expect("the view ran")
    }

    #[test]
    fn each_value_gives_its_rune_motion_state() {
        for system in [false, true] {
            assert!(!reduced("on", system), "On moves even when the Mac reduces motion (system {system})");
            assert!(reduced("off", system), "Off lands at once (system {system})");
        }
    }

    #[test]
    fn follow_is_the_macs_reduce_motion() {
        assert!(reduced("follow", true), "Follow with Reduce Motion on: no motion");
        assert!(!reduced("follow", false));
    }

    /// The default is Follow, and the look it builds touches nothing of Rune's state: as before.
    #[test]
    fn the_default_is_follow_and_the_same_as_before() {
        assert_eq!(motion(Values::defaults(&SETTINGS).get(MOTION)), Motion::Follow);
        assert_eq!(Motion::default(), Motion::Follow);
        for system in [false, true] {
            assert_eq!(Motion::Follow.reduced(system), system);
            let look = theme_look(false);
            let seen = Rc::new(Cell::new(None));
            let state = (Rc::new(Look::build(&look)), seen.clone());
            let view = |s: &(Rc<Look>, Rc<Cell<Option<bool>>>), cx: &mut rune::Cx<(Rc<Look>, Rc<Cell<Option<bool>>>)>| {
                s.0.ui.motion(cx);
                s.1.set(Some(cx.is_reduced_motion()));
                rune::el()
            };
            let mut rt = rune::Runtime::new(state, view, (100.0, 100.0), rune::Env { system_dark: false, scale: 1.0 });
            rt.set_system_reduced_motion(system);
            rt.frame(0.0);
            assert_eq!(seen.get(), Some(system));
        }
    }
}
