//! The Active Look Preset ([ADR 0011]): the Theme Look the Light Appearance shows and the one the
//! Dark Appearance shows, kept in `gui.json` under [`SETTING`], and the two looks resolved from
//! it ([`Looks`]), of which the window draws the one for the system's Appearance.
//!
//! `gui.json` holds each half as its theme's id and its settings by key:
//! `{"light": {"theme": "phantom", "settings": {"tone": "light"}}, "dark": {…}}`. Reading it
//! ([`LookPreset::read`]) repairs each half from what a fresh install has for that half, and
//! names what it reset: a half naming no theme there is comes back as a fresh install's half; a
//! setting of the wrong kind, or missing, as a fresh install's half has it (its theme's default
//! when that half is another theme's); a setting its theme doesn't have is dropped.
//!
//! [ADR 0011]: ../../../../docs/adr/0011-look-presets.md

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use super::{Look, ThemeLook, find, phantom};
use crate::platform::app_icon::Icons;
use crate::theme::setting::Values;

/// The key of the Active Look Preset in `gui.json`.
pub const SETTING: &str = "look_preset";

/// Light or Dark, as the system sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub const BOTH: [Appearance; 2] = [Appearance::Light, Appearance::Dark];

    /// The Appearance whose system flag says `dark`.
    pub fn of(dark: bool) -> Appearance {
        if dark { Appearance::Dark } else { Appearance::Light }
    }

    /// Its key in `gui.json` and in the names of what's reset.
    pub fn key(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }
}

/// What the app shows in each Appearance.
#[derive(Clone, Debug, PartialEq)]
pub struct LookPreset {
    pub light: ThemeLook,
    pub dark: ThemeLook,
}

impl LookPreset {
    /// A fresh install's: Phantom, its Tone Light in Light and Dark in Dark.
    pub fn fresh() -> LookPreset {
        LookPreset { light: phantom::theme_look(false), dark: phantom::theme_look(true) }
    }

    pub fn get(&self, appearance: Appearance) -> &ThemeLook {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }

    pub fn get_mut(&mut self, appearance: Appearance) -> &mut ThemeLook {
        match appearance {
            Appearance::Light => &mut self.light,
            Appearance::Dark => &mut self.dark,
        }
    }

    /// As `gui.json` keeps it.
    pub fn stored(&self) -> Stored {
        let half = |look: &ThemeLook| StoredLook { theme: look.theme.id().to_owned(), settings: look.settings.json() };
        Stored { light: half(&self.light), dark: half(&self.dark) }
    }

    /// The preset `stored` holds, each half repaired from a fresh install's, and the names of
    /// what was reset: `look_preset.<half>` for a half naming no theme there is,
    /// `look_preset.<half>.<key>` for a setting.
    pub fn read(stored: &Stored) -> (LookPreset, Vec<String>) {
        LookPreset::read_as(stored, SETTING)
    }

    /// [`read`](Self::read), naming what's reset under `prefix`: `<prefix>.<half>[.<key>]`.
    pub fn read_as(stored: &Stored, prefix: &str) -> (LookPreset, Vec<String>) {
        let fresh = LookPreset::fresh();
        let mut preset = fresh.clone();
        let mut reset = Vec::new();
        for appearance in Appearance::BOTH {
            let half = match appearance {
                Appearance::Light => &stored.light,
                Appearance::Dark => &stored.dark,
            };
            let name = format!("{prefix}.{}", appearance.key());
            let Some(theme) = find(&half.theme) else {
                reset.push(name);
                continue;
            };
            // What a fresh install has for this half, where it's the same theme.
            let base = match fresh.get(appearance) {
                look if look.theme.id() == theme.id() => look.settings.clone(),
                _ => Values::defaults(theme.settings()),
            };
            let (settings, bad) = Values::read(&base, &half.settings);
            reset.extend(bad.into_iter().map(|key| format!("{name}.{key}")));
            *preset.get_mut(appearance) = ThemeLook { theme, settings };
        }
        (preset, reset)
    }
}

/// What a person calls what `name`, one of [`LookPreset::read`]'s, names, worded by
/// [`client::display::theme_reset`]: a setting by the label of the first theme with that key, else
/// the key; a half by its theme. `None` for a name that isn't one.
pub fn reset_label(name: &str) -> Option<String> {
    match name.strip_prefix(SETTING) {
        Some(rest) => half_label(rest.strip_prefix('.')?),
        None => super::saved::reset_label(name),
    }
}

/// [`reset_label`] as the status bar's line has it: the same, but a saved preset isn't named.
pub fn reset_line(name: &str) -> Option<String> {
    match name.strip_prefix(SETTING) {
        Some(rest) => half_label(rest.strip_prefix('.')?),
        None => super::saved::reset_line(name),
    }
}

/// What a person calls `<half>[.<key>]` of a preset: "Tone in Light", "Theme in Dark".
pub(super) fn half_label(name: &str) -> Option<String> {
    let mut parts = name.splitn(2, '.');
    let half = parts.next()?;
    let appearance = Appearance::BOTH.into_iter().find(|a| a.key() == half)?;
    let setting = parts.next().map(|key| super::themes().iter().flat_map(|t| t.settings()).find(|s| s.key == key).map_or(key, |s| s.label));
    Some(client::display::theme_reset(setting, appearance.label()))
}

/// The Active Look Preset as `gui.json` holds it, before its themes and settings are checked.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stored {
    pub light: StoredLook,
    pub dark: StoredLook,
}

impl Default for Stored {
    /// A fresh install's.
    fn default() -> Stored {
        LookPreset::fresh().stored()
    }
}

/// One half: a theme's id and its settings by key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StoredLook {
    pub theme: String,
    #[serde(default)]
    pub settings: Map<String, Json>,
}

/// The Active Look Preset and the look each Appearance shows. A change rebuilds only the look of
/// the half it changes, and is kept through `keep`. A preview (a saved preset being edited) is
/// shown over it while it's set, and is never kept.
pub struct Looks {
    preset: LookPreset,
    light: Rc<Look>,
    dark: Rc<Look>,
    keep: Box<dyn FnMut(&LookPreset)>,
    preview: Option<Preview>,
}

/// A Look Preset shown over the active one, and its two looks.
struct Preview {
    preset: LookPreset,
    light: Rc<Look>,
    dark: Rc<Look>,
}

impl Looks {
    /// `preset`'s looks, with `keep` to keep each change.
    pub fn new(preset: LookPreset, keep: impl FnMut(&LookPreset) + 'static) -> Looks {
        let light = Rc::new(Look::build(&preset.light));
        let dark = Rc::new(Look::build(&preset.dark));
        Looks { preset, light, dark, keep: Box::new(keep), preview: None }
    }

    /// Shows `preset` over the active one, or, `None`, the active one again. Nothing is kept.
    pub fn preview(&mut self, preset: Option<&LookPreset>) {
        self.preview = match (preset, self.preview.take()) {
            (None, _) => None,
            (Some(preset), Some(shown)) if shown.preset == *preset => Some(shown),
            (Some(preset), _) => Some(Preview {
                preset: preset.clone(),
                light: Rc::new(Look::build(&preset.light)),
                dark: Rc::new(Look::build(&preset.dark)),
            }),
        };
    }

    /// The preset shown over the active one, if any.
    pub fn previewed(&self) -> Option<&LookPreset> {
        self.preview.as_ref().map(|p| &p.preset)
    }

    pub fn preset(&self) -> &LookPreset {
        &self.preset
    }

    /// The look `appearance` shows: the preview's, while there's one.
    pub fn shown(&self, appearance: Appearance) -> &Rc<Look> {
        match (&self.preview, appearance) {
            (Some(preview), Appearance::Light) => &preview.light,
            (Some(preview), Appearance::Dark) => &preview.dark,
            (None, Appearance::Light) => &self.light,
            (None, Appearance::Dark) => &self.dark,
        }
    }

    /// Makes `look` what `appearance` shows: its look is rebuilt, the other half's isn't, and
    /// the preset is kept. The same look again changes nothing.
    pub fn set(&mut self, appearance: Appearance, look: ThemeLook) {
        if *self.preset.get(appearance) == look {
            return;
        }
        let built = Rc::new(Look::build(&look));
        *self.preset.get_mut(appearance) = look;
        match appearance {
            Appearance::Light => self.light = built,
            Appearance::Dark => self.dark = built,
        }
        (self.keep)(&self.preset);
    }

    /// The App icon "Follow theme" shows: each Appearance's look's, one icon when they agree.
    pub fn app_icons(&self) -> Icons {
        match (self.light.app_icon, self.dark.app_icon) {
            (light, dark) if light == dark => Icons::One { icon: light.to_owned() },
            (light, dark) => Icons::Pair { dark: dark.to_owned(), light: light.to_owned() },
        }
    }
}

impl Default for Looks {
    /// A fresh install's, kept nowhere: for tests and headless runs.
    fn default() -> Looks {
        Looks::new(LookPreset::fresh(), |_| {})
    }
}
