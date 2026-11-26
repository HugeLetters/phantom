//! Phantom, the default theme: complete, and the end of every theme's parent chain. Its tokens
//! are rune-kit's neutral kit, light or dark as its Tone says; its blocks are the building blocks
//! as `ui` writes them, with its own icons; its App icon is the bundle's own.

mod icons;
pub mod proto;

#[cfg(test)]
use super::Look;
use super::setting::{Choice, Kind, Setting, Value, Values};
use super::{Built, Theme, ThemeLook};
use crate::ui::Icon;

pub struct Phantom;

pub static PHANTOM: Phantom = Phantom;

/// Light or Dark.
pub const TONE: &str = "tone";

pub const CORNERS: &str = "corners";
pub const DENSITY: &str = "density";
pub const MOTION: &str = "motion";
pub const EVERY: &str = "every_colour";

macro_rules! colour {
    ($key:literal, $label:literal, $hex:literal) => {
        Setting { key: $key, label: $label, kind: Kind::Colour { default: proto::FOLLOW } }
    };
}
macro_rules! derived {
    ($($key:literal $label:literal),*) => { [$(Setting { key: $key, label: $label, kind: Kind::Colour { default: proto::FOLLOW } }),*] };
}

static SETTINGS: [Setting; 35] = {
    let d = derived!(
        "d_text_muted" "Muted text", "d_text_faint" "Faint text", "d_text_accent" "Accent text", "d_on_accent" "Text on Accent",
        "d_page" "Window", "d_sheet" "Sheet and dialog", "d_panel" "Panel", "d_raised" "Notice", "d_tooltip" "Tooltip",
        "d_hover" "Hover", "d_pressed" "Pressed", "d_track" "Track", "d_off" "Off",
        "d_border_weak" "Hairline", "d_border_strong" "Strong", "d_focus" "Focus ring",
        "d_danger_bg" "Danger fill", "d_danger_border" "Danger edge", "d_warning_bg" "Warning fill",
        "d_warning_border" "Warning edge", "d_success_bg" "Success fill", "d_success_border" "Success edge"
    );
    let [d0, d1, d2, d3, d4, d5, d6, d7, d8, d9, d10, d11, d12, d13, d14, d15, d16, d17, d18, d19, d20, d21] = d;
    [
        Setting {
            key: TONE,
            label: "Tone",
            kind: Kind::Choice {
                choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }],
                default: "light",
            },
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
        Setting {
            key: DENSITY,
            label: "Density",
            kind: Kind::Choice {
                choices: &[Choice { id: "compact", label: "Compact" }, Choice { id: "comfortable", label: "Comfortable" }],
                default: "comfortable",
            },
        },
        Setting { key: MOTION, label: "Motion", kind: Kind::Toggle { default: true } },
        colour!("c_background", "Background", 0),
        colour!("c_surface", "Surface", 0),
        colour!("c_text", "Text", 0),
        colour!("c_accent", "Accent", 0),
        colour!("c_border", "Border", 0),
        colour!("c_danger", "Danger", 0),
        colour!("c_warning", "Warning", 0),
        colour!("c_success", "Success", 0),
        Setting { key: EVERY, label: "Show every colour", kind: Kind::Toggle { default: false } },
        d0,
        d1,
        d2,
        d3,
        d4,
        d5,
        d6,
        d7,
        d8,
        d9,
        d10,
        d11,
        d12,
        d13,
        d14,
        d15,
        d16,
        d17,
        d18,
        d19,
        d20,
        d21,
    ]
};

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
        let choice = |key: &str| match settings.get(key) {
            Some(Value::Choice(id)) => id.to_owned(),
            _ => String::new(),
        };
        let set = proto::env("set").unwrap_or_else(|| "today".into());
        let defaults = proto::set(&set, dark);
        // A base left to follow is the set's, per Tone: the stand-in for a default per Tone.
        let bases = proto::Bases(std::array::from_fn(|i| match settings.get(proto::BASES[i].0) {
            Some(Value::Colour(rgb)) if rgb != proto::FOLLOW && proto::env("set").is_none() => rgb,
            _ => defaults.0[i],
        }));
        let mut tokens = proto::kit(bases, dark);
        for d in &proto::DERIVED {
            if let Some(Value::Colour(rgb)) = settings.get(d.key)
                && rgb != proto::FOLLOW
            {
                *(d.field)(&mut tokens.c) = rgb.color();
            }
        }
        let corners = proto::env("corners").unwrap_or_else(|| choice(CORNERS));
        tokens.radius = proto::radius(&corners);
        proto::set_compact(proto::env("density").unwrap_or_else(|| choice(DENSITY)) == "compact");
        let motion = proto::env("motion").map_or(settings.get(MOTION) != Some(Value::Toggle(false)), |m| m != "off");
        proto::set_motion_off(!motion);
        if !motion {
            let none = rune::Spring::NONE;
            tokens.motion = rune_kit::Motion { control: none, overlay: none, disclosure: none };
        }
        Built { tokens, blocks: Default::default(), screens: Default::default(), app_icon: "phantom" }
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
