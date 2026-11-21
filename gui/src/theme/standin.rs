//! Stand-in, a second theme for the Settings prototype (#87) only, in debug builds: the editor
//! sheet's halves can show two themes, each with its own rows, before a second real theme
//! exists. Never in a release build, nor in tests (they have their own second theme). Three
//! settings of its own: Shade (a choice), Accent (a colour) and Animations (a toggle).

use rune_kit::Kit;

use super::setting::{Choice, Kind, Rgb, Setting, Value, Values};
use super::{Built, Theme, phantom};

pub struct StandIn;

pub static STAND_IN: StandIn = StandIn;

static SETTINGS: [Setting; 3] = [
    Setting {
        key: "shade",
        label: "Shade",
        kind: Kind::Choice { choices: &[Choice { id: "dusk", label: "Dusk" }, Choice { id: "night", label: "Night" }], default: "night" },
    },
    Setting { key: "accent", label: "Accent", kind: Kind::Colour { default: Rgb([0xd9, 0x4f, 0x8a]) } },
    Setting { key: "animations", label: "Animations", kind: Kind::Toggle { default: true } },
];

impl Theme for StandIn {
    fn id(&self) -> &'static str {
        "stand-in"
    }

    fn name(&self) -> &'static str {
        "Stand-in"
    }

    fn parent(&self) -> Option<&'static dyn Theme> {
        Some(&phantom::PHANTOM)
    }

    fn settings(&self) -> &'static [Setting] {
        &SETTINGS
    }

    fn build(&self, settings: &Values) -> Built {
        let mut tokens = Kit::neutral(settings.get("shade") == Some(Value::Choice("night")));
        if let Some(Value::Colour(rgb)) = settings.get("accent") {
            tokens.c.bg_accent = rgb.color();
            tokens.c.border_focus = rgb.color();
        }
        Built { tokens, blocks: Default::default(), screens: Default::default(), app_icon: "dark-hour" }
    }
}
