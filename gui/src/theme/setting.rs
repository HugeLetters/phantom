//! Theme settings: what a theme declares ([`Setting`], a key, a label and a [`Kind`] with its
//! default), the values a Theme Look holds for them ([`Values`]), and reading those from the JSON
//! `gui.json` keeps by key ([`Values::read`]). A [`Values`] always holds exactly one value of the
//! right kind for each setting its theme declares: an unknown key is dropped and a value of the
//! wrong kind is reset as it's read (to the value the reader starts from), each named, so nothing
//! past here checks again ([ADR 0011]).
//!
//! A color setting holds `None` until it's set by hand: the theme works it out then, from its
//! default or from the setting it follows ([`Kind::Color`]), and tells Settings what it came to and
//! any warning about it ([`Resolved`]). Only a color set by hand is stored.
//!
//! [ADR 0011]: ../../../../docs/adr/0011-look-presets.md

use serde_json::{Map, Value as Json};

/// A setting a theme declares, which the generic settings screen shows.
#[derive(Debug, PartialEq)]
pub struct Setting {
    /// Its key in the Theme Look's settings in `gui.json`.
    pub key: &'static str,
    /// What Settings calls it: "Tone".
    pub label: &'static str,
    pub kind: Kind,
    /// The section Settings shows it in, under the section's title; none, above every section.
    pub section: Option<&'static Section>,
    /// The group of its section it's in, under the group's name ("Text"); none, ungrouped.
    pub group: Option<&'static str>,
}

impl Setting {
    /// A setting outside every section.
    pub const fn new(key: &'static str, label: &'static str, kind: Kind) -> Setting {
        Setting { key, label, kind, section: None, group: None }
    }

    /// In `section`.
    pub const fn in_section(mut self, section: &'static Section) -> Setting {
        self.section = Some(section);
        self
    }

    /// In the group `group` of its section.
    pub const fn in_group(mut self, group: &'static str) -> Setting {
        self.group = Some(group);
        self
    }
}

/// A run of a theme's settings that Settings shows under one title: "Colors", or "Every color",
/// which opens and closes.
#[derive(Debug, PartialEq)]
pub struct Section {
    /// What its header is keyed by: `<key>-section`.
    pub key: &'static str,
    pub title: &'static str,
    /// Whether it starts closed, its header opening it.
    pub folds: bool,
}

/// What a setting holds, from a small closed set, with its default. A new kind is added when a
/// theme needs one.
#[derive(Debug, PartialEq)]
#[cfg_attr(not(test), allow(dead_code, reason = "Phantom's only setting is a choice; the tests' theme has the others"))]
pub enum Kind {
    Toggle {
        default: bool,
    },
    /// One of a fixed list. `default` is one of `choices`' ids (every theme's are checked).
    Choice {
        choices: &'static [Choice],
        default: &'static str,
        /// How Settings draws it.
        show: Show,
    },
    /// A color, `None` until set by hand: then the theme's own, worked out from the setting keyed
    /// `follows` (a base color), or, with none, its default (which may differ by its Tone).
    Color {
        follows: Option<&'static str>,
    },
}

/// How Settings draws a [`Kind::Choice`]: a dropdown, which suits any labels, or the options side
/// by side, which suits a couple of short ones (Light | Dark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Dropdown,
    Segments,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    /// As `gui.json` holds it.
    pub id: &'static str,
    pub label: &'static str,
}

/// A color a person picks, stored as `"#rrggbb"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// `#rrggbb`, either case; anything else is `None`.
    pub fn parse(text: &str) -> Option<Rgb> {
        let hex = text.trim().strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii())?;
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb([byte(0)?, byte(2)?, byte(4)?]))
    }

    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// The color as Rune holds it: linear, decoded from the sRGB bytes by Rune's own `hex`.
    pub fn color(self) -> rune::Color {
        rune::color::hex(&self.hex()).expect("`hex` is always `#rrggbb`")
    }
}

/// A value of a setting, of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Toggle(bool),
    /// The id of one of the setting's choices.
    Choice(&'static str),
    /// Set by hand, or `None`: what the theme works out ([`Resolved::color`]).
    Color(Option<Rgb>),
}

impl Kind {
    pub fn default(&self) -> Value {
        match *self {
            Kind::Toggle { default } => Value::Toggle(default),
            Kind::Choice { default, .. } => Value::Choice(default),
            Kind::Color { .. } => Value::Color(None),
        }
    }

    /// `json` as a value of this kind; `None` if it's of another kind, or not one of the choices.
    pub(crate) fn read(&self, json: &Json) -> Option<Value> {
        match self {
            Kind::Toggle { .. } => json.as_bool().map(Value::Toggle),
            Kind::Choice { choices, .. } => json.as_str().and_then(|id| choices.iter().find(|c| c.id == id)).map(|c| Value::Choice(c.id)),
            Kind::Color { .. } => json.as_str().and_then(Rgb::parse).map(|rgb| Value::Color(Some(rgb))),
        }
    }

    /// Whether `value` is one this kind holds.
    fn holds(&self, value: &Value) -> bool {
        match (self, value) {
            (Kind::Toggle { .. }, Value::Toggle(_)) | (Kind::Color { .. }, Value::Color(_)) => true,
            (Kind::Choice { choices, .. }, Value::Choice(id)) => choices.iter().any(|c| c.id == *id),
            (Kind::Toggle { .. } | Kind::Choice { .. } | Kind::Color { .. }, _) => false,
        }
    }
}

impl Value {
    /// As `gui.json` keeps it; nothing for a color that isn't set by hand.
    fn json(self) -> Option<Json> {
        match self {
            Value::Toggle(on) => Some(Json::Bool(on)),
            Value::Choice(id) => Some(Json::String(id.to_owned())),
            Value::Color(rgb) => rgb.map(|rgb| Json::String(rgb.hex())),
        }
    }

    /// What Settings compares and copies of it (a Join, the split rows): a color as it resolves
    /// (`resolved`, the setting `key` is in a look whose theme worked it out), else itself.
    pub fn shown(self, key: &str, resolved: &Resolved) -> Value {
        match self {
            Value::Color(_) => Value::Color(resolved.color(key).or(self.color())),
            Value::Toggle(_) | Value::Choice(_) => self,
        }
    }

    fn color(self) -> Option<Rgb> {
        match self {
            Value::Color(rgb) => rgb,
            Value::Toggle(_) | Value::Choice(_) => None,
        }
    }
}

/// What a theme works out of its settings for Settings to show: the color each color setting
/// comes to, and the warnings about a setting's value ("Hard to read on Surface: 3.8:1 (needs
/// 4.5:1)."), and the most warnings a setting could have, each as long as it could be.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    colors: Vec<(&'static str, Rgb)>,
    warnings: Vec<(&'static str, String)>,
    room: Vec<(&'static str, String)>,
}

impl Resolved {
    /// The color setting `key` comes to.
    pub fn color(&self, key: &str) -> Option<Rgb> {
        self.colors.iter().find(|(k, _)| *k == key).map(|(_, rgb)| *rgb)
    }

    /// The warnings about setting `key`, one line each.
    pub fn warnings(&self, key: &str) -> Vec<&str> {
        self.warnings.iter().filter(|(k, _)| *k == key).map(|(_, w)| w.as_str()).collect()
    }

    pub fn set_color(&mut self, key: &'static str, rgb: Rgb) {
        self.colors.retain(|(k, _)| *k != key);
        self.colors.push((key, rgb));
    }

    /// Every warning, by the setting it's about.
    #[cfg(test)]
    pub fn every_warning(&self) -> &[(&'static str, String)] {
        &self.warnings
    }

    pub fn warn(&mut self, key: &'static str, warning: String) {
        self.warnings.push((key, warning));
    }

    /// The warnings setting `key` could have at most, each as long as it could be, whatever it's
    /// set to: the room its picker keeps for them, so it never grows while a drag goes.
    pub fn room(&self, key: &str) -> Vec<&str> {
        self.room.iter().filter(|(k, _)| *k == key).map(|(_, w)| w.as_str()).collect()
    }

    pub fn make_room(&mut self, key: &'static str, line: String) {
        self.room.push((key, line));
    }
}

/// One value for each setting of a theme, in the order it declares them.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    of: &'static [Setting],
    values: Vec<Value>,
}

impl Values {
    /// Every setting at its default.
    pub fn defaults(of: &'static [Setting]) -> Values {
        Values { of, values: of.iter().map(|s| s.kind.default()).collect() }
    }

    /// The settings `json` holds, by key, over `base`, and the keys reset: one no setting has
    /// (dropped), and one whose value isn't of its kind (back to `base`'s). A missing one is
    /// `base`'s, and not named: nothing was there to reset.
    pub fn read(base: &Values, json: &Map<String, Json>) -> (Values, Vec<String>) {
        let mut values = base.clone();
        let of = base.of;
        let mut reset = Vec::new();
        for (key, value) in json {
            match of.iter().position(|s| s.key == key) {
                Some(i) => match of[i].kind.read(value) {
                    Some(value) => values.values[i] = value,
                    None => reset.push(key.clone()),
                },
                None => reset.push(key.clone()),
            }
        }
        (values, reset)
    }

    /// As `gui.json` keeps them: every setting, by key, but a color not set by hand.
    pub fn json(&self) -> Map<String, Json> {
        self.iter().filter_map(|(setting, value)| Some((setting.key.to_owned(), value.json()?))).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static Setting, Value)> + '_ {
        self.of.iter().zip(self.values.iter().copied())
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.iter().find(|(s, _)| s.key == key).map(|(_, v)| v)
    }

    /// Sets `key` to `value`; `false`, changing nothing, when there's no such setting or the
    /// value isn't of its kind.
    pub fn set(&mut self, key: &str, value: Value) -> bool {
        let Some(i) = self.of.iter().position(|s| s.key == key && s.kind.holds(&value)) else { return false };
        self.values[i] = value;
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    /// A picked color is sRGB; `rune::Color` is linear, so it is decoded as Rune decodes a hex.
    #[test]
    fn a_picked_color_is_decoded_from_srgb() {
        let picked = Rgb::parse("#2f7fe3").unwrap().color();
        assert_eq!(picked, rune::color::hex("#2f7fe3").unwrap());
        assert!(picked.r < 0.1, "0x2f as linear is 0.18, as sRGB 0.03");
    }

    /// One setting of every kind.
    pub static SAMPLE: [Setting; 3] = [
        Setting::new("pulse", "Pulse", Kind::Toggle { default: true }),
        Setting::new(
            "shape",
            "Shape",
            Kind::Choice {
                choices: &[
                    Choice { id: "round", label: "Round" },
                    Choice { id: "square", label: "Square" },
                    Choice { id: "cut", label: "Cut" },
                ],
                default: "square",
                show: Show::Dropdown,
            },
        ),
        Setting::new("accent", "Accent", Kind::Color { follows: None }),
    ];

    /// Any value of `setting`'s kind.
    pub fn value_of(setting: &'static Setting) -> BoxedStrategy<Value> {
        match &setting.kind {
            Kind::Toggle { .. } => any::<bool>().prop_map(Value::Toggle).boxed(),
            Kind::Choice { choices, .. } => {
                proptest::sample::select(choices.iter().map(|c| c.id).collect::<Vec<_>>()).prop_map(Value::Choice).boxed()
            }
            Kind::Color { .. } => any::<Option<[u8; 3]>>().prop_map(|c| Value::Color(c.map(Rgb))).boxed(),
        }
    }

    /// `values` with every color following: what a fresh install's half has, the only base
    /// settings are read over (a color not set by hand isn't stored, so it reads as the base's).
    fn following(mut values: Values) -> Values {
        for v in &mut values.values {
            if let Value::Color(_) = v {
                *v = Value::Color(None);
            }
        }
        values
    }

    /// Values for `of`, each any of its kind.
    pub fn values_of(of: &'static [Setting]) -> impl Strategy<Value = Values> {
        of.iter().map(value_of).collect::<Vec<_>>().prop_map(move |values| Values { of, values })
    }

    /// JSON that's of no kind `setting` holds: of another kind, or a choice that isn't one.
    fn wrong_for(setting: &Setting) -> BoxedStrategy<Json> {
        let other = prop_oneof![Just(json!(null)), Just(json!(3)), Just(json!([])), Just(json!({})), "[a-z]{1,6}".prop_map(Json::String)];
        match setting.kind {
            Kind::Toggle { .. } => other.boxed(),
            // Strings that aren't a color or a choice: none of these is "#" and six hex digits, or a choice's id.
            Kind::Choice { .. } | Kind::Color { .. } => {
                prop_oneof![Just(json!(true)), Just(json!(1.5)), "x[a-z]{0,6}".prop_map(Json::String), Just(json!("#12345"))].boxed()
            }
        }
    }

    proptest! {
        #[test]
        fn values_round_trip_through_json(values in values_of(&SAMPLE), base in values_of(&SAMPLE).prop_map(following)) {
            let (read, reset) = Values::read(&base, &values.json());
            prop_assert_eq!(read, values);
            prop_assert!(reset.is_empty());
        }

        /// Each key that's unknown or of the wrong kind is named, back to the base's value (an
        /// unknown one dropped); a missing one is the base's, unnamed; the rest are kept.
        #[test]
        fn an_unknown_key_or_a_wrong_kind_is_reset_to_the_base_and_named(
            values in values_of(&SAMPLE),
            base in values_of(&SAMPLE).prop_map(following),
            wrong in proptest::collection::btree_set(0..SAMPLE.len(), 0..=3),
            missing in proptest::collection::btree_set(0..SAMPLE.len(), 0..=3),
            unknown in proptest::collection::btree_set("[a-z]{1,4}_x", 0..3),
            junk in (wrong_for(&SAMPLE[0]), wrong_for(&SAMPLE[1]), wrong_for(&SAMPLE[2])),
        ) {
            let mut json = values.json();
            let mut expected = values.clone();
            let mut named: Vec<String> = Vec::new();
            let junk = [junk.0, junk.1, junk.2];
            for i in &missing {
                json.remove(SAMPLE[*i].key);
                expected.values[*i] = base.values[*i];
            }
            // A color not set by hand isn't stored: it reads as the base's, which follows too.
            for (i, v) in values.values.iter().enumerate() {
                if *v == Value::Color(None) {
                    expected.values[i] = base.values[i];
                }
            }
            for i in wrong.difference(&missing) {
                json.insert(SAMPLE[*i].key.into(), junk[*i].clone());
                expected.values[*i] = base.values[*i];
                named.push(SAMPLE[*i].key.into());
            }
            for key in &unknown {
                json.insert(key.clone(), json!(true));
                named.push(key.clone());
            }
            let (read, mut reset) = Values::read(&base, &json);
            reset.sort();
            named.sort();
            prop_assert_eq!(read, expected);
            prop_assert_eq!(reset, named);
        }

        /// A setting takes a value of its kind and nothing else.
        #[test]
        fn set_takes_only_a_value_of_the_settings_kind(values in values_of(&SAMPLE), i in 0..SAMPLE.len(), value in prop_oneof![
            any::<bool>().prop_map(Value::Toggle),
            prop_oneof![Just("round"), Just("square"), Just("cut"), Just("oval")].prop_map(Value::Choice),
            any::<Option<[u8; 3]>>().prop_map(|c| Value::Color(c.map(Rgb))),
        ]) {
            let mut set = values.clone();
            let took = set.set(SAMPLE[i].key, value);
            prop_assert_eq!(took, SAMPLE[i].kind.holds(&value));
            let mut expected = values.clone();
            if took {
                expected.values[i] = value;
            }
            prop_assert_eq!(set, expected);
        }

        #[test]
        fn a_color_reads_back_from_its_hex(rgb in any::<[u8; 3]>()) {
            prop_assert_eq!(Rgb::parse(&Rgb(rgb).hex()), Some(Rgb(rgb)));
            prop_assert_eq!(Rgb::parse(&Rgb(rgb).hex().to_uppercase()), Some(Rgb(rgb)));
        }
    }
}
