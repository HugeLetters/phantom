//! Phantom's colors: eight base colors (Background, Surface, Text, Accent, Border, Danger, Warning,
//! Success), and every other color worked out from them by a rule ([`DERIVED`]), each until it's
//! set by hand. A color not set by hand is today's own while what it follows isn't set either, so
//! a fresh install looks as Phantom always has; the rules come within a hair of it (its tests).
//!
//! A translucent color (a hairline, a hover) is picked as it looks on the Surface, and drawn as the
//! ink that looks like that there ([`oklab::ink_over`]), so it still tints whatever it's over.

use rune::Color;
use rune_kit::{Kit, Palette, StateColors};

use super::super::oklab::{self, contrast, ink_over, lerp_oklab, over};
use super::super::setting::{Kind, Resolved, Rgb, Section, Setting, Value, Values};

/// The base colors' section.
pub static COLORS: Section = Section { key: "colors", title: "Colors", folds: false };

/// Every other color, in groups, under a header that opens it.
pub static EVERY_COLOR: Section = Section { key: "every-color", title: "Every color", folds: true };

/// A base color: the rest follow these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Background,
    Surface,
    Text,
    Accent,
    Border,
    Danger,
    Warning,
    Success,
}

impl Base {
    pub const ALL: [Base; 8] =
        [Base::Background, Base::Surface, Base::Text, Base::Accent, Base::Border, Base::Danger, Base::Warning, Base::Success];

    /// Its key in the Theme Look's settings.
    pub const fn key(self) -> &'static str {
        match self {
            Base::Background => "background",
            Base::Surface => "surface",
            Base::Text => "text",
            Base::Accent => "accent",
            Base::Border => "border",
            Base::Danger => "danger",
            Base::Warning => "warning",
            Base::Success => "success",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Base::Background => "Background",
            Base::Surface => "Surface",
            Base::Text => "Text",
            Base::Accent => "Accent",
            Base::Border => "Border",
            Base::Danger => "Danger",
            Base::Warning => "Warning",
            Base::Success => "Success",
        }
    }

    /// Today's color, as `Kit::neutral` has it: the Border a translucent ink.
    fn today(self, t: &Palette) -> Color {
        match self {
            Base::Background => t.page,
            Base::Surface => t.bg_base,
            Base::Text => t.text_base,
            Base::Accent => t.bg_accent,
            Base::Border => t.border_base,
            Base::Danger => t.danger.fg,
            Base::Warning => t.warning.fg,
            Base::Success => t.success.fg,
        }
    }
}

/// How a derived color is picked: as itself, or, translucent, as it looks on the Surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    Itself,
    OnSurface,
}

/// A color worked out by a rule, which "Every color" lists: its key and name, its group there,
/// the key of the color its rule works it out from (a base, or Muted text for Faint text), which
/// its swatch and Follow button name, how it's picked, and the kit's colors it sets (`set`) and is
/// read from (`get`).
pub struct Derived {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub follows: &'static str,
    seen: Seen,
    get: fn(&Palette) -> Color,
    set: fn(&mut Palette, Color),
}

macro_rules! derived {
    ($key:literal, $label:literal, $group:literal, $follows:expr, $seen:ident, $first:ident $(. $sub:ident)? $(, $more:ident)*) => {
        Derived {
            key: $key,
            label: $label,
            group: $group,
            follows: $follows,
            seen: Seen::$seen,
            get: |p| p.$first $(. $sub)?,
            set: |p, c| {
                p.$first $(. $sub)? = c;
                $(p.$more = c;)*
            },
        }
    };
}

/// Every derived color, in the order "Every color" lists them.
pub static DERIVED: [Derived; 18] = [
    derived!("muted_text", "Muted text", "Text", Base::Text.key(), Itself, text_muted, icon_muted, text_weak, std_icon, icon_weaker),
    derived!("faint_text", "Faint text", "Text", "muted_text", OnSurface, text_faint),
    derived!("window", "Window", "Surfaces", Base::Background.key(), Itself, page, bg_deep),
    derived!("sheet", "Sheet and dialog", "Surfaces", Base::Background.key(), Itself, layer_01),
    derived!("panel", "Panel", "Surfaces", Base::Surface.key(), Itself, bg_base, bg_button_neutral, surface_raised, surface_raised_base),
    derived!("hover", "Hover", "States", Base::Text.key(), OnSurface, hover, surface_hover, surface_raised_hover),
    derived!("pressed", "Pressed", "States", Base::Text.key(), OnSurface, pressed),
    derived!("track", "Track", "States", Base::Text.key(), OnSurface, layer_02, surface_base, surface_weak, surface_disabled),
    derived!("off", "Off", "States", Base::Text.key(), OnSurface, layer_03),
    derived!("hairline", "Hairline", "Borders", Base::Border.key(), OnSurface, border_weak, border_muted, border_disabled),
    derived!("strong_border", "Strong border", "Borders", Base::Border.key(), OnSurface, border_strong),
    derived!("focus_ring", "Focus ring", "Borders", Base::Accent.key(), Itself, border_focus),
    derived!("danger_fill", "Danger fill", "Status", Base::Danger.key(), Itself, danger.bg),
    derived!("danger_edge", "Danger edge", "Status", Base::Danger.key(), Itself, danger.border),
    derived!("warning_fill", "Warning fill", "Status", Base::Warning.key(), Itself, warning.bg),
    derived!("warning_edge", "Warning edge", "Status", Base::Warning.key(), Itself, warning.border),
    derived!("success_fill", "Success fill", "Status", Base::Success.key(), Itself, success.bg),
    derived!("success_edge", "Success edge", "Status", Base::Success.key(), Itself, success.border),
];

/// The settings Phantom's colors add: the bases, then every derived color.
pub fn settings() -> impl Iterator<Item = Setting> {
    let bases = Base::ALL.into_iter().map(|b| Setting::new(b.key(), b.label(), Kind::Color { follows: None }).in_section(&COLORS));
    let derived = DERIVED
        .iter()
        .map(|d| Setting::new(d.key, d.label, Kind::Color { follows: Some(d.follows) }).in_section(&EVERY_COLOR).in_group(d.group));
    bases.chain(derived)
}

/// The rules' factors, by Tone: how far muted text sits from Text towards Surface; the overlays,
/// tracks and the off state, Text at an opacity; how much stronger a hairline is than the Border;
/// how far a state's fill and edge sit off the Surface in lightness, and how much of the state's
/// chroma they keep.
struct Factors {
    muted: f32,
    hover: f32,
    pressed: f32,
    track: f32,
    off: f32,
    /// Dark only: how far the "unset" tile sits from Surface towards Text.
    tile: Option<f32>,
    hairline: f32,
    fill: (f32, f32),
    edge: (f32, f32),
}

/// Fitted to `Kit::neutral` (lightness exact; see the survey in #417).
const LIGHT: Factors = Factors {
    muted: 0.355,
    hover: 0.07,
    pressed: 0.12,
    track: 0.13,
    off: 0.24,
    tile: None,
    hairline: 1.3,
    fill: (-0.05, 0.04 / 0.15),
    edge: (-0.15, 0.08 / 0.15),
};

const DARK: Factors = Factors {
    muted: 0.325,
    hover: 0.016,
    pressed: 0.03,
    track: 0.05,
    off: 0.085,
    tile: Some(0.274),
    hairline: 1.1,
    fill: (0.068, 0.04 / 0.15),
    edge: (0.168, 0.08 / 0.15),
};

/// A state's fill or edge: `base`'s hue, `chroma` of its chroma, `lightness` off the Surface's.
fn state_tint(base: Color, surface: Color, (lightness, chroma): (f32, f32)) -> Color {
    let (_, c, h) = oklab::oklch(base);
    rune::oklch((oklab::oklch(surface).0 + lightness).clamp(0.0, 1.0), c * chroma, h)
}

/// The value of color setting `key` set by hand, if it is.
fn picked(values: &Values, key: &str) -> Option<Rgb> {
    match values.get(key) {
        Some(Value::Color(rgb)) => rgb,
        _ => None,
    }
}

/// Phantom's palette in the Tone `dark` says, with `values`' colors, and what each color setting
/// comes to with its warnings. `kit` is `Kit::neutral(dark)`; its colors are replaced.
pub fn apply(kit: &mut Kit, values: &Values) -> Resolved {
    let dark = kit.dark;
    let today = kit.c;
    let f = if dark { &DARK } else { &LIGHT };
    let set = |b: Base| picked(values, b.key());
    let changed = |bases: &[Base]| bases.iter().any(|b| set(*b).is_some());
    let surface = set(Base::Surface).map(Rgb::color).unwrap_or(today.bg_base);
    let base = |b: Base| match set(b) {
        Some(rgb) if b == Base::Border => ink_over(rgb.color(), surface),
        Some(rgb) => rgb.color(),
        None => b.today(&today),
    };
    let [bg, _, text, accent, border, danger, warning, success] = Base::ALL.map(base);
    let p = &mut kit.c;
    for x in [&mut p.page, &mut p.layer_01, &mut p.bg_deep] {
        *x = bg;
    }
    for x in [
        &mut p.bg_base,
        &mut p.bg_button_neutral,
        &mut p.surface_raised,
        &mut p.surface_raised_base,
        &mut p.text_inverse,
        &mut p.text_contrast,
        &mut p.icon_inverse,
        &mut p.icon_contrast,
        &mut p.std_icon_invert,
        &mut p.text_invert_strong,
        &mut p.submit_fg,
        &mut p.submit_icon,
    ] {
        *x = surface;
    }
    for x in [
        &mut p.text_base,
        &mut p.text_strong,
        &mut p.icon_base,
        &mut p.bg_inverse,
        &mut p.bg_contrast,
        &mut p.bg_icon_button_contrast,
        &mut p.std_text,
        &mut p.std_icon_strong,
        &mut p.surface_float,
    ] {
        *x = text;
    }
    for x in [&mut p.bg_accent, &mut p.text_accent, &mut p.icon_accent, &mut p.border_focus, &mut p.border_active] {
        *x = accent;
    }
    for x in [&mut p.border_base, &mut p.std_border, &mut p.border_hover] {
        *x = border;
    }
    // A rule replaces today's color only once something it follows is set.
    let text_or_surface = changed(&[Base::Text, Base::Surface]);
    if text_or_surface {
        (DERIVED[0].set)(p, lerp_oklab(text, surface, f.muted));
        p.layer_04 = match f.tile {
            Some(t) => lerp_oklab(surface, text, t),
            None => surface,
        };
    }
    if changed(&[Base::Text]) {
        p.hover = text.alpha(f.hover);
        p.surface_hover = p.hover;
        p.surface_raised_hover = p.hover;
        p.pressed = text.alpha(f.pressed);
        for x in [&mut p.layer_02, &mut p.surface_base, &mut p.surface_weak, &mut p.surface_disabled] {
            *x = text.alpha(f.track);
        }
        p.layer_03 = text.alpha(f.off);
    }
    if changed(&[Base::Border]) {
        for x in [&mut p.border_weak, &mut p.border_muted, &mut p.border_strong, &mut p.border_disabled] {
            *x = border.alpha((border.a * f.hairline).min(1.0));
        }
        for shadows in
            [&mut kit.elevation.raised, &mut kit.elevation.floating, &mut kit.elevation.overlay, &mut kit.elevation.button_neutral]
        {
            for shadow in shadows.iter_mut().filter(|s| s.color == today.border_base) {
                shadow.color = border;
            }
        }
    }
    let states: [(Base, Color, fn(&mut Palette) -> &mut StateColors); 3] = [
        (Base::Danger, danger, |p| &mut p.danger),
        (Base::Warning, warning, |p| &mut p.warning),
        (Base::Success, success, |p| &mut p.success),
    ];
    for (b, ink, colors) in states {
        let state = colors(p);
        state.fg = ink;
        state.icon = ink;
        if changed(&[b, Base::Surface]) {
            state.bg = state_tint(ink, surface, f.fill);
            state.border = state_tint(ink, surface, f.edge);
        }
    }
    // Then what's set by hand, in "Every color"'s order: a surface before what's picked on it.
    for d in &DERIVED {
        if let Some(rgb) = picked(values, d.key) {
            let color = match d.seen {
                Seen::Itself => rgb.color(),
                Seen::OnSurface => ink_over(rgb.color(), p.bg_base),
            };
            (d.set)(p, color);
        }
    }
    // Faint text is muted text, fainter, as today; unless set by hand, it follows muted text.
    let muted_moved = text_or_surface || picked(values, DERIVED[0].key).is_some();
    if muted_moved && picked(values, DERIVED[1].key).is_none() {
        p.text_faint = p.text_muted.alpha(0.8);
        p.icon_faint = p.text_muted.alpha(0.7);
    }
    resolve(p, &today, values, &Base::ALL.map(base))
}

/// What each color setting comes to (a color set by hand, exactly that), and the warnings.
fn resolve(p: &Palette, today: &Palette, values: &Values, bases: &[Color; 8]) -> Resolved {
    let mut resolved = Resolved::default();
    let seen = |c: Color| Rgb::of(over(c, p.bg_base));
    for (b, color) in Base::ALL.iter().zip(bases) {
        let shown = if *b == Base::Border { seen(*color) } else { Rgb::of(*color) };
        resolved.set_color(b.key(), picked(values, b.key()).unwrap_or(shown));
    }
    for d in &DERIVED {
        resolved.set_color(d.key, picked(values, d.key).unwrap_or_else(|| seen((d.get)(p))));
    }
    for (key, warning) in warnings(p, today) {
        resolved.warn(key, warning);
    }
    for (key, line) in room() {
        resolved.make_room(key, line);
    }
    resolved
}

/// The pairs Phantom draws that need contrast, each about the setting whose popup shows it: text
/// (Text, Muted, Faint) at 4.5:1 on the Surface and the Background; the Accent, a shape (the focus
/// ring, a selection, a bar), at 3:1 on both; a state's ink on its fill at 4.5:1. A pair that's
/// as Phantom has it by default isn't warned about, though it may fall short (faint text in
/// Light reads 3.8:1).
fn warnings(p: &Palette, today: &Palette) -> Vec<(&'static str, String)> {
    pairs()
        .into_iter()
        .filter(|pair| (pair.ink)(p) != (pair.ink)(today) || (pair.on)(p) != (pair.on)(today))
        .filter_map(|pair| {
            let ratio = contrast((pair.ink)(p), over((pair.on)(p), p.bg_base));
            // One decimal, rounded down, so a pair just short never reads as enough.
            let shown = (ratio * 10.0).floor() / 10.0;
            (ratio < pair.needs).then(|| (pair.key, pair.line(shown)))
        })
        .collect()
}

/// Every warning there could be, each as long as it can be (its ratio "0.0"), about the setting
/// whose popup shows it: what the popup is placed for.
fn room() -> Vec<(&'static str, String)> {
    pairs().into_iter().map(|pair| (pair.key, pair.line(0.0))).collect()
}

/// A pair that needs contrast: `ink` on `on`, at `needs`:1, warned about in `key`'s popup.
struct Pair {
    key: &'static str,
    verb: &'static str,
    ink: fn(&Palette) -> Color,
    name: &'static str,
    on: fn(&Palette) -> Color,
    needs: f32,
}

impl Pair {
    fn line(&self, shown: f32) -> String {
        let Pair { verb, name, needs, .. } = self;
        format!("Hard to {verb} on {name}: {shown:.1}:1 (needs {needs}:1).")
    }
}

/// The pairs Phantom draws that need contrast, as [`warnings`] has them.
fn pairs() -> Vec<Pair> {
    type Get = fn(&Palette) -> Color;
    let (surface, background): (Get, Get) = (|p| p.bg_base, |p| p.page);
    let pair = |key, verb, ink: Get, name, on: Get, needs| Pair { key, verb, ink, name, on, needs };
    let mut pairs = Vec::new();
    let texts: [(&'static str, Get); 3] =
        [(Base::Text.key(), |p| p.text_base), ("muted_text", |p| p.text_muted), ("faint_text", |p| p.text_faint)];
    for (key, ink) in texts {
        pairs.push(pair(key, "read", ink, "Surface", surface, 4.5));
        pairs.push(pair(key, "read", ink, "Background", background, 4.5));
    }
    pairs.push(pair(Base::Accent.key(), "see", |p| p.bg_accent, "Surface", surface, 3.0));
    pairs.push(pair(Base::Accent.key(), "see", |p| p.bg_accent, "Background", background, 3.0));
    pairs.push(pair(Base::Danger.key(), "read", |p| p.danger.fg, "Danger fill", |p| p.danger.bg, 4.5));
    pairs.push(pair(Base::Warning.key(), "read", |p| p.warning.fg, "Warning fill", |p| p.warning.bg, 4.5));
    pairs.push(pair(Base::Success.key(), "read", |p| p.success.fg, "Success fill", |p| p.success.bg, 4.5));
    pairs
}

#[cfg(test)]
mod tests {
    use super::super::{SETTINGS, theme_look};
    use super::*;
    use crate::theme::oklab::delta_e;

    /// The kit with `values` in the Tone `dark` says.
    fn kit(values: &Values, dark: bool) -> (Kit, Resolved) {
        let mut kit = Kit::neutral(dark);
        let resolved = apply(&mut kit, values);
        (kit, resolved)
    }

    fn values(dark: bool) -> Values {
        theme_look(dark).settings
    }

    fn with(dark: bool, set: &[(&str, Rgb)]) -> Values {
        let mut v = values(dark);
        for (key, rgb) in set {
            assert!(v.set(key, Value::Color(Some(*rgb))), "{key} is a color setting");
        }
        v
    }

    /// Every color the kit holds that Phantom draws or the kit's controls read.
    fn every(p: &Palette) -> Vec<(&'static str, Color)> {
        let mut all = vec![
            ("page", p.page),
            ("layer_01", p.layer_01),
            ("layer_02", p.layer_02),
            ("layer_03", p.layer_03),
            ("layer_04", p.layer_04),
            ("bg_base", p.bg_base),
            ("text_base", p.text_base),
            ("text_muted", p.text_muted),
            ("text_faint", p.text_faint),
            ("icon_faint", p.icon_faint),
            ("hover", p.hover),
            ("pressed", p.pressed),
            ("border_base", p.border_base),
            ("border_weak", p.border_weak),
            ("border_strong", p.border_strong),
            ("border_muted", p.border_muted),
            ("border_focus", p.border_focus),
            ("bg_accent", p.bg_accent),
            ("submit_fg", p.submit_fg),
            ("surface_float", p.surface_float),
            ("surface_base", p.surface_base),
        ];
        for (name, s) in [("danger", p.danger), ("warning", p.warning), ("success", p.success)] {
            let named: [&'static str; 4] = match name {
                "danger" => ["danger.bg", "danger.fg", "danger.border", "danger.icon"],
                "warning" => ["warning.bg", "warning.fg", "warning.border", "warning.icon"],
                _ => ["success.bg", "success.fg", "success.border", "success.icon"],
            };
            all.extend(named.into_iter().zip([s.bg, s.fg, s.border, s.icon]));
        }
        all
    }

    /// With nothing set, the palette is `Kit::neutral`'s, color for color, and its shadows too:
    /// a fresh install looks as Phantom always has.
    #[test]
    fn with_nothing_set_every_color_is_todays() {
        for dark in [false, true] {
            let today = Kit::neutral(dark);
            let (k, resolved) = kit(&values(dark), dark);
            for ((name, a), (_, b)) in every(&k.c).into_iter().zip(every(&today.c)) {
                assert_eq!(a, b, "{name} (dark {dark})");
            }
            assert_eq!(format!("{:?}", k.elevation), format!("{:?}", today.elevation));
            assert!(resolved.every_warning().is_empty(), "no warning by default: {:?}", resolved.every_warning());
        }
    }

    /// Each base set by hand to the color it is by default works out every color through the
    /// rules: within the survey's distances of today's (ΔE ×100 in OKLab, over the Surface).
    #[test]
    fn the_rules_come_within_the_surveys_distance_of_today() {
        for dark in [false, true] {
            let today = Kit::neutral(dark);
            let t = &today.c;
            let bases: Vec<(&str, Rgb)> = Base::ALL
                .iter()
                .map(|b| (b.key(), if *b == Base::Border { Rgb::of(over(t.border_base, t.bg_base)) } else { Rgb::of(b.today(t)) }))
                .collect();
            let (k, _) = kit(&with(dark, &bases), dark);
            for ((name, a), (_, b)) in every(&k.c).into_iter().zip(every(&t.clone())) {
                // A state's fill and edge keep the ink's hue and chroma in proportion, and today's
                // inks are out of sRGB's gamut, so a picked one is a little off: under 2, where the
                // survey's mixes were up to 4. The rest are under 1: the same color.
                let limit = if name.ends_with(".bg") || name.ends_with(".border") { 3.0 } else { 1.0 };
                let d = delta_e(over(a, t.bg_base), over(b, t.bg_base));
                assert!(d < limit, "{name}: ΔE {d:.2} (dark {dark})");
            }
        }
    }

    #[test]
    fn a_color_set_by_hand_sticks_and_following_returns_it_to_the_rule() {
        let pink = Rgb([0xff, 0x33, 0x99]);
        let mut v = with(false, &[("text", Rgb([0x20, 0x10, 0x40])), ("muted_text", pink)]);
        let (k, resolved) = kit(&v, false);
        assert_eq!(k.c.text_muted, pink.color());
        assert_eq!(resolved.color("muted_text"), Some(pink));
        assert!(v.set("muted_text", Value::Color(None)));
        let (k, resolved) = kit(&v, false);
        let rule = lerp_oklab(Rgb([0x20, 0x10, 0x40]).color(), Kit::neutral(false).c.bg_base, LIGHT.muted);
        assert_eq!(k.c.text_muted, rule, "Follow Text: the rule's again");
        assert_eq!(resolved.color("muted_text"), Some(Rgb::of(rule)));
    }

    /// Each derived color is worked out from the color its swatch and Follow button name: with
    /// every other color set by hand, setting that one moves it (#445).
    #[test]
    fn each_derived_color_follows_the_color_it_names() {
        let keys: Vec<&str> = Base::ALL.iter().map(|b| b.key()).chain(DERIVED.iter().map(|d| d.key)).collect();
        for dark in [false, true] {
            for d in &DERIVED {
                let shown = |rgb: Rgb| {
                    let set: Vec<(&str, Rgb)> = keys
                        .iter()
                        .filter(|k| **k != d.key)
                        .map(|k| (*k, if *k == d.follows { rgb } else { Rgb([0x80, 0x80, 0x80]) }))
                        .collect();
                    kit(&with(dark, &set), dark).1.color(d.key)
                };
                assert_ne!(shown(Rgb([0x20, 0x40, 0xc0])), shown(Rgb([0xe0, 0x90, 0x30])), "{} follows {} (dark {dark})", d.key, d.follows);
            }
        }
    }

    /// Reset on a base: back to the theme's default, and everything that follows it with it.
    #[test]
    fn reset_on_a_base_brings_back_todays_colors() {
        let mut v = with(true, &[("accent", Rgb([0xff, 0x80, 0x00]))]);
        assert_eq!(kit(&v, true).0.c.border_focus, Rgb([0xff, 0x80, 0x00]).color());
        assert!(v.set("accent", Value::Color(None)));
        let (k, resolved) = kit(&v, true);
        assert_eq!(k.c.border_focus, Kit::neutral(true).c.bg_accent);
        assert_eq!(resolved.color("accent"), Some(Rgb::of(Kit::neutral(true).c.bg_accent)));
    }

    /// A hairline picked as it looks on the Surface is drawn as an ink that looks like it there.
    #[test]
    fn a_translucent_color_is_picked_as_it_looks_on_the_surface() {
        let seen = Rgb([0xc0, 0xd0, 0xe0]);
        let (k, resolved) = kit(&with(false, &[("hairline", seen)]), false);
        assert!(k.c.border_weak.a < 1.0, "still translucent");
        assert_eq!(Rgb::of(over(k.c.border_weak, k.c.bg_base)), seen);
        assert_eq!(resolved.color("hairline"), Some(seen));
    }

    /// The warnings: exactly the pairs Phantom draws, each under the setting it's about, worded
    /// with the ratio rounded down.
    #[test]
    fn warnings_are_for_the_pairs_drawn() {
        let gray = Rgb([0xb0, 0xb0, 0xb0]);
        let (_, r) = kit(&with(false, &[("text", gray)]), false);
        assert_eq!(
            r.warnings("text"),
            ["Hard to read on Surface: 2.1:1 (needs 4.5:1).", "Hard to read on Background: 1.9:1 (needs 4.5:1)."]
        );
        // Muted and faint text follow Text, so they warn too; nothing else does.
        assert_eq!(r.warnings("muted_text").len(), 2);
        assert_eq!(r.warnings("faint_text").len(), 2);
        let keys: std::collections::BTreeSet<_> = r.every_warning().iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["faint_text", "muted_text", "text"].into());

        let (_, r) = kit(&with(false, &[("accent", Rgb([0xd0, 0xe0, 0xff]))]), false);
        assert_eq!(r.warnings("accent"), ["Hard to see on Surface: 1.3:1 (needs 3:1).", "Hard to see on Background: 1.1:1 (needs 3:1)."]);

        let (_, r) = kit(&with(false, &[("danger_fill", Rgb([0xa0, 0x30, 0x30]))]), false);
        assert_eq!(r.warnings("danger"), ["Hard to read on Danger fill: 1.0:1 (needs 4.5:1)."]);
        assert_eq!(r.every_warning().len(), 1);

        // A faint text set by hand to a light gray: under 4.5 on both.
        let (_, r) = kit(&with(false, &[("faint_text", Rgb([0x9a, 0x9c, 0xa0]))]), false);
        assert_eq!(r.warnings("faint_text").len(), 2);

        // Contrast that's enough says nothing.
        let (_, r) = kit(&with(true, &[("accent", Rgb([0x40, 0x90, 0xf0]))]), true);
        assert!(r.every_warning().is_empty(), "{:?}", r.every_warning());
    }

    /// Every setting Phantom's colors add is a color, in its section, the derived ones each
    /// following a base and in a group.
    #[test]
    fn the_colors_settings_are_generated_from_the_list() {
        let colors: Vec<&Setting> = SETTINGS.iter().filter(|s| matches!(s.kind, Kind::Color { .. })).collect();
        assert_eq!(colors.len(), 8 + DERIVED.len());
        for (s, b) in colors.iter().zip(Base::ALL) {
            assert_eq!((s.key, s.section, s.group), (b.key(), Some(&COLORS), None));
        }
        for (s, d) in colors[8..].iter().zip(&DERIVED) {
            assert_eq!((s.key, s.kind == Kind::Color { follows: Some(d.follows) }, s.section), (d.key, true, Some(&EVERY_COLOR)));
            assert_eq!(s.group, Some(d.group));
        }
    }
}
