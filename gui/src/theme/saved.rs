//! Saved Look Presets ([ADR 0011]): named snapshots of a Look Preset, kept in `gui.json` under
//! [`SETTING`] next to the active one, in the order they were saved ([`Saved`]). Picking one copies
//! it, or one half of it, into the Active Look Preset; the saved one changes only when it's updated
//! from the active one.
//!
//! `gui.json` holds them as a list: `[{"name": "Calm", "light": {…}, "dark": {…}}, …]`, each half
//! as the active one's. Reading it ([`read`]) repairs each preset as the active one is read, and
//! names what it reset under `saved_presets."<name>"`: a half naming no theme there is, or that
//! isn't a half at all, comes back as a fresh install's half (`saved_presets."Calm".dark`); a
//! setting as [`LookPreset::read`] has it (`saved_presets."Calm".dark.tone`). A preset without a
//! name is dropped (`saved_presets`), and so is one named as one before it (`saved_presets."Calm"`).
//!
//! [ADR 0011]: ../../../../docs/adr/0011-look-presets.md

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::preset::{Appearance, LookPreset, Stored, StoredLook, half_label};

/// The key of the saved presets in `gui.json`.
pub const SETTING: &str = "saved_presets";

/// A saved preset's name: not blank, without spaces around it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name(String);

/// Why text can't be a new preset's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameProblem {
    Blank,
    /// A saved preset has it already.
    Taken(Name),
}

impl Name {
    /// `text`, trimmed; `None` when that's blank.
    pub fn parse(text: &str) -> Option<Name> {
        let name = text.trim();
        (!name.is_empty()).then(|| Name(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Look Preset kept by name.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedPreset {
    pub name: Name,
    pub preset: LookPreset,
}

/// One saved preset as `gui.json` keeps it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoredSaved {
    pub name: String,
    pub light: StoredLook,
    pub dark: StoredLook,
}

/// The saved presets as `gui.json` keeps them.
pub fn stored(saved: &[SavedPreset]) -> Vec<StoredSaved> {
    saved
        .iter()
        .map(|s| {
            let Stored { light, dark } = s.preset.stored();
            StoredSaved { name: s.name.0.clone(), light, dark }
        })
        .collect()
}

/// The saved presets `entries` hold, each repaired as the active one is, and the names of what
/// was reset (the module's doc says which).
pub fn read(entries: &[Json]) -> (Vec<SavedPreset>, Vec<String>) {
    let mut saved: Vec<SavedPreset> = Vec::new();
    let mut reset = Vec::new();
    for entry in entries {
        let Some(name) = entry.get("name").and_then(Json::as_str).and_then(Name::parse) else {
            reset.push(SETTING.to_owned());
            continue;
        };
        let prefix = prefix(&name);
        if saved.iter().any(|s| s.name == name) {
            reset.push(prefix);
            continue;
        }
        // A half that isn't one names no theme: it reads as a fresh install's, named.
        let half = |appearance: Appearance| {
            entry
                .get(appearance.key())
                .and_then(|half| serde_json::from_value::<StoredLook>(half.clone()).ok())
                .unwrap_or(StoredLook { theme: String::new(), settings: Default::default() })
        };
        let (preset, bad) = LookPreset::read_as(&Stored { light: half(Appearance::Light), dark: half(Appearance::Dark) }, &prefix);
        reset.extend(bad);
        saved.push(SavedPreset { name, preset });
    }
    (saved, reset)
}

/// What the names of a preset's resets start with: `saved_presets."Calm"`, its name as JSON, so
/// any name reads back.
fn prefix(name: &Name) -> String {
    format!("{SETTING}.{}", Json::String(name.0.clone()))
}

/// What a person calls what `name`, one of [`read`]'s names, names: "Tone in Dark of Calm",
/// "Look Preset Calm"; `None` for a name that isn't one. This names the preset: the list's wording.
pub(super) fn reset_label(name: &str) -> Option<String> {
    match reset_parts(name)? {
        (None, _) => Some(client::display::setting_label(SETTING).to_owned()),
        (Some(preset), what) => Some(client::display::saved_reset(&preset, what.as_deref())),
    }
}

/// [`reset_label`] without the preset's name, which the user chose: "Tone in Dark of a saved Look
/// Preset", as the status bar's line words it.
pub(super) fn reset_line(name: &str) -> Option<String> {
    match reset_parts(name)? {
        (None, _) => Some(client::display::setting_label(SETTING).to_owned()),
        (Some(_), what) => Some(client::display::saved_reset_line(what.as_deref())),
    }
}

/// The preset a reset name is of (none for the whole list), and what of it.
fn reset_parts(name: &str) -> Option<(Option<String>, Option<String>)> {
    let rest = name.strip_prefix(SETTING)?;
    let Some(rest) = rest.strip_prefix('.') else {
        return rest.is_empty().then(|| (None, None));
    };
    let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<String>();
    let preset = stream.next()?.ok()?;
    let half = &rest[stream.byte_offset()..];
    let what = match half.strip_prefix('.') {
        Some(half) => Some(half_label(half)?),
        None if half.is_empty() => None,
        None => return None,
    };
    Some((Some(preset), what))
}

/// The saved presets, each change kept through `keep`.
pub struct Saved {
    list: Vec<SavedPreset>,
    keep: Box<dyn FnMut(&[SavedPreset])>,
    /// The list as it was last kept, which [`Self::release`] compares with.
    kept: Vec<SavedPreset>,
    /// How many sheets are open over the list: while any is, a change is shown but not kept.
    held: usize,
}

impl Saved {
    pub fn new(list: Vec<SavedPreset>, keep: impl FnMut(&[SavedPreset]) + 'static) -> Saved {
        Saved { kept: list.clone(), list, keep: Box::new(keep), held: 0 }
    }

    /// A sheet opens over the list: what it changes is kept only once [`Self::release`] ends the
    /// last sheet open.
    pub fn hold(&mut self) {
        self.held += 1;
    }

    /// A sheet closed. With none left open, the list is kept if it isn't what was kept last.
    pub fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
        if self.held == 0 && self.list != self.kept {
            self.kept = self.list.clone();
            (self.keep)(&self.list);
        }
    }

    pub fn list(&self) -> &[SavedPreset] {
        &self.list
    }

    /// `text` as the name of a new preset: one no saved preset has.
    pub fn new_name(&self, text: &str) -> Result<Name, NameProblem> {
        let name = Name::parse(text).ok_or(NameProblem::Blank)?;
        match self.get(&name) {
            Some(_) => Err(NameProblem::Taken(name)),
            None => Ok(name),
        }
    }

    /// The saved preset named `name`.
    pub fn get(&self, name: &Name) -> Option<&SavedPreset> {
        self.list.iter().find(|s| s.name == *name)
    }

    /// Makes `list` the saved presets, kept; the same list again keeps nothing.
    pub fn set(&mut self, list: Vec<SavedPreset>) {
        if list != self.list {
            self.list = list;
            if self.held == 0 {
                self.kept = self.list.clone();
                (self.keep)(&self.list);
            }
        }
    }
}

impl Default for Saved {
    /// None, kept nowhere: for tests and headless runs.
    fn default() -> Saved {
        Saved::new(Vec::new(), |_| {})
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::theme::tests::preset;

    /// A name: a few words.
    pub fn name() -> impl Strategy<Value = Name> {
        "[A-Za-z][A-Za-z0-9 .\"]{0,8}[A-Za-z0-9]".prop_map(|n| Name::parse(&n).unwrap())
    }

    /// Saved presets, their names apart.
    pub fn saved() -> impl Strategy<Value = Vec<SavedPreset>> {
        proptest::collection::btree_map(name(), preset(), 0..4)
            .prop_map(|presets| presets.into_iter().map(|(name, preset)| SavedPreset { name, preset }).collect())
    }

    fn entries(saved: &[SavedPreset]) -> Vec<Json> {
        stored(saved).into_iter().map(|s| serde_json::to_value(s).unwrap()).collect()
    }

    proptest! {
        #[test]
        fn saved_presets_read_back_what_they_keep(saved in saved()) {
            let (read, reset) = read(&entries(&saved));
            prop_assert_eq!(read, saved);
            prop_assert!(reset.is_empty());
        }

        /// A preset is read as the active one is, its resets named by it in words; one without a
        /// name, or named as one before it, is dropped, named; the others are as they were.
        #[test]
        fn a_bad_saved_preset_is_repaired_or_dropped_and_named(saved in saved(), which in 0..4usize, at in any::<prop::sample::Index>()) {
            prop_assume!(!saved.is_empty());
            let i = at.index(saved.len());
            let mut entries = entries(&saved);
            let mut expected = saved.clone();
            let name = saved[i].name.as_str().to_owned();
            let (named, words) = match which {
                0 => {
                    entries[i]["dark"]["theme"] = json!("gone");
                    expected[i].preset.dark = LookPreset::fresh().dark;
                    (format!("{SETTING}.{}.dark", Json::String(name.clone())), format!("Theme in Dark of {name}"))
                }
                1 => {
                    entries[i]["light"]["settings"]["nonsense"] = json!(1);
                    (format!("{SETTING}.{}.light.nonsense", Json::String(name.clone())), format!("nonsense in Light of {name}"))
                }
                2 => {
                    entries[i]["name"] = json!(" ");
                    expected.remove(i);
                    (SETTING.to_owned(), "Look Presets".to_owned())
                }
                _ => {
                    entries.push(entries[i].clone());
                    (format!("{SETTING}.{}", Json::String(name.clone())), format!("Look Preset {name}"))
                }
            };
            let (read, reset) = read(&entries);
            prop_assert_eq!(read, expected);
            prop_assert_eq!(&reset, &vec![named]);
            prop_assert_eq!(reset_label(&reset[0]), Some(words));
        }
    }
}
