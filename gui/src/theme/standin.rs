//! Stand-in, a second theme for debug builds only (#87): the Look Preset editor's halves can show
//! two themes, each with its own rows, before a second real theme exists. Never in a release build
//! (`cfg(debug_assertions)`), nor in tests (they have their own second theme). Three
//! settings of its own: Shade (a choice), Accent (a color) and Animations (a toggle).

use rune_kit::Kit;

use super::setting::{Choice, Kind, Resolved, Rgb, Setting, Show, Value, Values};
use super::{Built, Colors, Spacing, Theme, phantom};

pub struct StandIn;

pub static STAND_IN: StandIn = StandIn;

static SETTINGS: [Setting; 3] = [
    Setting::new(
        "shade",
        "Shade",
        Kind::Choice {
            choices: &[Choice { id: "dusk", label: "Dusk" }, Choice { id: "night", label: "Night" }],
            default: "night",
            show: Show::Dropdown,
        },
    ),
    Setting::new("accent", "Accent", Kind::Color { follows: None }),
    Setting::new("animations", "Animations", Kind::Toggle { default: true }),
];

/// Its Accent until one is picked.
const ACCENT: Rgb = Rgb([0xd9, 0x4f, 0x8a]);

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
        let accent = match settings.get("accent") {
            Some(Value::Color(Some(rgb))) => rgb,
            _ => ACCENT,
        };
        tokens.c.bg_accent = accent.color();
        tokens.c.border_focus = accent.color();
        let mut resolved = Resolved::default();
        resolved.set_color("accent", accent);
        Built {
            colors: Colors::of(&tokens),
            spacing: Spacing::phantom(),
            tokens,
            blocks: Default::default(),
            screens: Default::default(),
            app_icon: "dark-hour",
            motion: Default::default(),
            resolved,
        }
    }
}
