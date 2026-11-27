//! The App icons a theme draws from settings of their own (#417): Phantom's ghost on a tile of any
//! color. A theme owns its icons and their settings ([`Theme::icons`]); the App icon's
//! Customize… sheet shows them, a Light and a Dark value each, and they're kept in `gui.json`
//! under [`SETTING`], outside any Look Preset, as the App icon is. An icon whose settings are all
//! their defaults is the shipped picture, untouched.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::preset::Appearance;
use super::setting::Rgb;
use super::{Look, Theme};

/// The key of every icon's settings in `gui.json`.
pub const SETTING: &str = "app_icon_settings";

/// What an icon's color setting holds in one half: the icon's own colors, the colors of the look
/// that half shows, or one set by hand. Kept as `"original"`, `"follow"` or `"#rrggbb"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tint {
    #[default]
    Original,
    Follow,
    Color(Rgb),
}

impl Serialize for Tint {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Tint::Original => s.serialize_str("original"),
            Tint::Follow => s.serialize_str("follow"),
            Tint::Color(rgb) => s.serialize_str(&rgb.hex()),
        }
    }
}

impl<'de> Deserialize<'de> for Tint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Tint, D::Error> {
        let text = String::deserialize(d)?;
        match text.as_str() {
            "original" => Ok(Tint::Original),
            "follow" => Ok(Tint::Follow),
            hex => Rgb::parse(hex)
                .filter(|_| hex.len() == 7)
                .map(Tint::Color)
                .ok_or_else(|| serde::de::Error::custom(format!("{text:?} is no tint: \"original\", \"follow\" or \"#rrggbb\""))),
        }
    }
}

/// A color setting of an icon.
#[derive(Debug)]
pub struct IconSetting {
    /// Its name in `gui.json`; never changes.
    pub key: &'static str,
    /// What the Customize sheet calls it.
    pub label: &'static str,
}

/// One half's values of an icon's settings, by key; a key not there holds [`Tint::Original`].
pub type IconValues = BTreeMap<String, Tint>;

/// An icon's values, a Light and a Dark half, as `gui.json` keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IconHalves {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub light: IconValues,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dark: IconValues,
}

impl IconHalves {
    pub fn get(&self, half: Appearance) -> &IconValues {
        match half {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }

    pub fn get_mut(&mut self, half: Appearance) -> &mut IconValues {
        match half {
            Appearance::Light => &mut self.light,
            Appearance::Dark => &mut self.dark,
        }
    }
}

/// Every icon's settings, by the icon's id.
pub type IconSettings = BTreeMap<String, IconHalves>;

/// The value of `key` in `half` of `icon`'s settings: Original unless set.
pub fn tint(settings: &IconSettings, icon: &str, half: Appearance, key: &str) -> Tint {
    settings.get(icon).and_then(|h| h.get(half).get(key)).copied().unwrap_or_default()
}

/// `settings` as `gui.json` keeps them: only what isn't Original, of the icons and keys a theme
/// declares; nothing at all for an icon left at its defaults.
pub fn stored(settings: &IconSettings) -> IconSettings {
    let mut kept = IconSettings::new();
    for (id, halves) in settings {
        let Some((_, icon)) = find(id) else { continue };
        let keep = |values: &IconValues| -> IconValues {
            values
                .iter()
                .filter(|(k, v)| **v != Tint::Original && icon.settings.iter().any(|s| s.key == k.as_str()))
                .map(|(k, v)| (k.clone(), *v))
                .collect()
        };
        let halves = IconHalves { light: keep(&halves.light), dark: keep(&halves.dark) };
        if halves != IconHalves::default() {
            kept.insert(id.clone(), halves);
        }
    }
    kept
}

/// An App icon a theme draws from its settings.
pub struct ThemeIcon {
    /// Its id in the icons' manifest.
    pub id: &'static str,
    pub settings: &'static [IconSetting],
    /// The icon drawn with `values` in `half`, as an SVG; `None` for the shipped picture as it is.
    /// `look` is the look `half` shows when it's this icon's theme's: what Follow the look follows.
    pub draw: fn(values: &IconValues, look: Option<&Look>, half: Appearance) -> Option<String>,
    /// The color the setting `key` comes to with `values`, as its swatch shows it.
    pub color: fn(key: &str, values: &IconValues, look: Option<&Look>, half: Appearance) -> Rgb,
}

/// The icon with settings that's `id`, and the theme it's of.
pub fn find(id: &str) -> Option<(&'static dyn Theme, &'static ThemeIcon)> {
    super::themes().iter().find_map(|theme| theme.icons().iter().find(|icon| icon.id == id).map(|icon| (*theme, icon)))
}

/// `look` if it's `theme`'s: what an icon of `theme` follows in that half.
pub fn of_theme<'l>(theme: &dyn Theme, look: &'l Look) -> Option<&'l Look> {
    (look.theme == theme.id()).then_some(look)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tint_is_kept_as_a_word_or_a_hex() {
        for (tint, json) in
            [(Tint::Original, "\"original\""), (Tint::Follow, "\"follow\""), (Tint::Color(Rgb([0x3f, 0x7e, 0xd0])), "\"#3f7ed0\"")]
        {
            assert_eq!(serde_json::to_string(&tint).unwrap(), json);
            assert_eq!(serde_json::from_str::<Tint>(json).unwrap(), tint);
        }
        for bad in ["\"blue\"", "\"#3f7ed\"", "\"#3f7ed0ff\"", "7", "\" #3f7ed0\""] {
            assert!(serde_json::from_str::<Tint>(bad).is_err(), "{bad} is refused");
        }
    }

    #[test]
    fn only_what_is_not_original_is_kept() {
        let mut settings = IconSettings::new();
        let mut halves = IconHalves::default();
        halves.light.insert("tile".into(), Tint::Original);
        halves.dark.insert("tile".into(), Tint::Follow);
        halves.dark.insert("nonsense".into(), Tint::Follow);
        settings.insert("phantom".into(), halves);
        settings.insert("dark-hour".into(), IconHalves { light: [("tile".into(), Tint::Follow)].into(), ..Default::default() });
        let kept = stored(&settings);
        assert_eq!(serde_json::to_value(&kept).unwrap(), serde_json::json!({"phantom": {"dark": {"tile": "follow"}}}));
        let mut original = kept.clone();
        original.get_mut("phantom").unwrap().dark.insert("tile".into(), Tint::Original);
        assert!(stored(&original).is_empty(), "an icon at its defaults is kept as nothing");
    }
}
