//! PROTOTYPE (#417), throwaway: Phantom's tokens from eight base colours, with Corners, Density
//! and Motion. `PHANTOM_PROTO="set=warm;corners=round;density=compact;motion=off"` overrides a
//! headless run's settings, so the main window can be rendered in each.

use std::sync::atomic::{AtomicU8, Ordering};

use rune::Color;
use rune_kit::{Kit, Palette, Radius, StateColors};

use crate::theme::setting::Rgb;

/// A derived colour that follows its base: stands in for `Option<Rgb>`'s `None`.
pub const FOLLOW: Rgb = Rgb([1, 2, 3]);

/// The base colours, in the order Settings lists them.
pub const BASES: [(&str, &str); 8] = [
    ("c_background", "Background"),
    ("c_surface", "Surface"),
    ("c_text", "Text"),
    ("c_accent", "Accent"),
    ("c_border", "Border"),
    ("c_danger", "Danger"),
    ("c_warning", "Warning"),
    ("c_success", "Success"),
];

#[derive(Clone, Copy)]
pub struct Bases(pub [Rgb; 8]);

const fn h(x: u32) -> Rgb {
    Rgb([(x >> 16) as u8, (x >> 8) as u8, x as u8])
}

/// The three sets the stills compare, Light then Dark.
pub fn set(name: &str, dark: bool) -> Bases {
    let v = match (name, dark) {
        ("warm", false) => [0xf5f0e8, 0xfffcf7, 0x2b2118, 0xb4501a, 0xe8ddd0, 0xb3261e, 0x875400, 0x27702d],
        ("warm", true) => [0x16120e, 0x211b16, 0xf3eadf, 0xf0904f, 0x352b22, 0xff8f84, 0xe9a640, 0x7fc78c],
        ("bold", false) => [0xf2f0fa, 0xffffff, 0x15112a, 0x6526d4, 0xe2ddf3, 0xbe1e3c, 0x8a5600, 0x0f7a45],
        ("bold", true) => [0x0d0a18, 0x17122a, 0xf2effc, 0xa98bff, 0x2d2549, 0xff7a8e, 0xf2b33d, 0x4fd69a],
        (_, false) => [0xf0f1f4, 0xffffff, 0x1a1c1f, 0x2f7fe3, 0xe5e5e5, 0xa83634, 0x8e5500, 0x00792f],
        (_, true) => [0x121314, 0x1c1d20, 0xeff0f2, 0x4392f7, 0x28292b, 0xff847d, 0xdfa11a, 0x5dc879],
    };
    Bases(v.map(h))
}

fn c(rgb: Rgb) -> Color {
    rgb.color()
}

fn enc(x: f32) -> f32 {
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

fn dec(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

/// `a` moved `t` of the way to `b`, in sRGB as Rune blends; `t` past 1 goes beyond `b`.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| dec((enc(x) + (enc(y) - enc(x)) * t).clamp(0.0, 1.0));
    Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: 1.0 }
}

fn over(top: Color, under: Color) -> Color {
    mix(under, Color { a: 1.0, ..top }, top.a)
}

fn lum(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// WCAG 2 contrast of `a` on `b` (`a` blended over `b` when translucent).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (lum(over(a, b)), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The furthest from `text` towards `surface`, up to `t`, that keeps 4.5:1 on both `grounds`.
fn readable(text: Color, surface: Color, t: f32, grounds: &[Color]) -> Color {
    (0..=40)
        .map(|i| t * (1.0 - i as f32 / 40.0))
        .map(|t| mix(text, surface, t))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 4.5))
        .unwrap_or(text)
}

/// Tone factors: overlays, tracks and borders are the text at an alpha, a quarter or so in Dark.
struct Factors {
    hover: f32,
    pressed: f32,
    track: f32,
    off: f32,
}

/// Phantom's kit from its bases: what `Kit::neutral` sets, every field from one of the eight.
pub fn kit(b: Bases, dark: bool) -> Kit {
    let [bg, surface, text, accent, border, danger, warning, success] = b.0.map(c);
    let f = if dark {
        Factors { hover: 0.017, pressed: 0.03, track: 0.05, off: 0.09 }
    } else {
        Factors { hover: 0.07, pressed: 0.12, track: 0.13, off: 0.24 }
    };
    let mut k = Kit::neutral(dark);
    let p = &mut k.c;
    // Muted reads 4.5:1 on the window and on a selected row too (the accent at .16).
    let selected = mix(surface, accent, 0.16);
    let muted = readable(text, surface, 0.33, &[surface, bg, selected]);
    // Today's faint is muted at .8 (3.8:1 in Light); this one stops where it still reads 4.5.
    let faint = readable(text, surface, 0.45, &[surface, bg]);
    let on_accent = if contrast(Color::WHITE, accent) >= contrast(Color::BLACK, accent) { Color::WHITE } else { Color::BLACK };
    let state = |base: Color| StateColors {
        bg: mix(surface, base, if dark { 0.13 } else { 0.12 }),
        fg: base,
        border: mix(surface, base, 0.31),
        icon: base,
    };
    let edge = mix(surface, border, 1.3);
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
    for x in [&mut p.text_muted, &mut p.icon_muted, &mut p.text_weak, &mut p.std_icon, &mut p.icon_weaker] {
        *x = muted;
    }
    p.text_faint = faint;
    p.icon_faint = faint;
    for x in [&mut p.bg_accent, &mut p.text_accent, &mut p.icon_accent, &mut p.border_focus, &mut p.border_active] {
        *x = accent;
    }
    p.submit_fg = on_accent;
    p.submit_icon = on_accent;
    p.hover = text.alpha(f.hover);
    p.surface_hover = p.hover;
    p.surface_raised_hover = p.hover;
    p.pressed = text.alpha(f.pressed);
    for x in [&mut p.layer_02, &mut p.surface_base, &mut p.surface_weak, &mut p.surface_disabled] {
        *x = text.alpha(f.track);
    }
    p.layer_03 = text.alpha(f.off);
    p.layer_04 = if dark { mix(surface, text, 0.16) } else { Color::WHITE };
    for x in [&mut p.border_base, &mut p.std_border, &mut p.border_hover] {
        *x = border;
    }
    for x in [&mut p.border_weak, &mut p.border_muted, &mut p.border_strong, &mut p.border_disabled] {
        *x = edge;
    }
    p.danger = state(danger);
    p.warning = state(warning);
    p.success = state(success);
    p.info = state(accent);
    k.ring = Some(|p: &Palette| p.border_base);
    k
}

/// A derived colour "Show every colour" lists: key, label, group, base it follows, its field.
pub type Field = fn(&mut Palette) -> &mut Color;
pub struct Derived {
    pub key: &'static str,
    pub group: &'static str,
    pub base: &'static str,
    pub field: Field,
}

macro_rules! d {
    ($key:literal, $label:literal, $group:literal, $base:literal, $($f:tt)+) => {
        Derived { key: $key, group: $group, base: $base, field: |p| &mut p.$($f)+ }
    };
}

pub static DERIVED: [Derived; 22] = [
    d!("d_text_muted", "Muted text", "Text", "Text", text_muted),
    d!("d_text_faint", "Faint text", "Text", "Text", text_faint),
    d!("d_text_accent", "Accent text", "Text", "Accent", text_accent),
    d!("d_on_accent", "Text on Accent", "Text", "Accent", submit_fg),
    d!("d_page", "Window", "Surfaces", "Background", page),
    d!("d_sheet", "Sheet and dialog", "Surfaces", "Background", layer_01),
    d!("d_panel", "Panel", "Surfaces", "Surface", bg_base),
    d!("d_raised", "Notice", "Surfaces", "Surface", surface_raised),
    d!("d_tooltip", "Tooltip", "Surfaces", "Text", surface_float),
    d!("d_hover", "Hover", "States", "Text", hover),
    d!("d_pressed", "Pressed", "States", "Text", pressed),
    d!("d_track", "Track", "States", "Text", layer_02),
    d!("d_off", "Off", "States", "Text", layer_03),
    d!("d_border_weak", "Hairline", "Borders", "Border", border_weak),
    d!("d_border_strong", "Strong", "Borders", "Border", border_strong),
    d!("d_focus", "Focus ring", "Borders", "Accent", border_focus),
    d!("d_danger_bg", "Danger fill", "Status", "Danger", danger.bg),
    d!("d_danger_border", "Danger edge", "Status", "Danger", danger.border),
    d!("d_warning_bg", "Warning fill", "Status", "Warning", warning.bg),
    d!("d_warning_border", "Warning edge", "Status", "Warning", warning.border),
    d!("d_success_bg", "Success fill", "Status", "Success", success.bg),
    d!("d_success_border", "Success edge", "Status", "Success", success.border),
];

pub fn derived(key: &str) -> Option<&'static Derived> {
    DERIVED.iter().find(|d| d.key == key)
}

/// The colour a look's kit has for `d`.
pub fn read(k: &Kit, d: &Derived) -> Color {
    let mut p = k.c.clone();
    *(d.field)(&mut p)
}

/// A base's colour in `k`, for a base left at its default.
pub fn base_now(key: &str, k: &Kit) -> Option<Color> {
    let p = &k.c;
    Some(match key {
        "c_background" => p.page,
        "c_surface" => p.bg_base,
        "c_text" => p.text_base,
        "c_accent" => p.bg_accent,
        "c_border" => p.border_base,
        "c_danger" => p.danger.fg,
        "c_warning" => p.warning.fg,
        "c_success" => p.success.fg,
        _ => return None,
    })
}

pub fn hex(c: Color) -> String {
    let b = |x: f32| (enc(x.clamp(0.0, 1.0)) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// What the sheet says under the row of `key` when its pair reads under 4.5:1 (3:1 for edges).
pub fn warnings(key: &str, k: &Kit) -> Vec<String> {
    let p = &k.c;
    let page = p.page;
    let pairs: Vec<(&str, Color, Color, f32)> = match key {
        "c_text" => vec![("Text on Surface", p.text_base, p.bg_base, 4.5)],
        "c_background" => vec![("Muted text on Window", p.text_muted, page, 4.5)],
        "c_surface" => vec![("Muted text on Surface", p.text_muted, p.bg_base, 4.5)],
        "c_accent" => vec![("Text on Accent", p.submit_fg, p.bg_accent, 4.5), ("Accent text on Surface", p.text_accent, p.bg_base, 4.5)],
        "c_danger" => vec![("Danger on its fill", p.danger.fg, p.danger.bg, 4.5)],
        "c_warning" => vec![("Warning on its fill", p.warning.fg, p.warning.bg, 4.5)],
        "c_success" => vec![("Success on its fill", p.success.fg, p.success.bg, 4.5)],
        "d_text_faint" => vec![("Faint text on Surface", p.text_faint, p.bg_base, 4.5), ("Faint text on Window", p.text_faint, page, 4.5)],
        "d_text_muted" => vec![("Muted text on Surface", p.text_muted, p.bg_base, 4.5)],
        "d_focus" => vec![("Focus ring on Surface", p.border_focus, p.bg_base, 3.0)],
        _ => vec![],
    };
    pairs
        .into_iter()
        .map(|(what, a, b, need)| (what, contrast(a, b), need))
        .filter(|(_, r, need)| r < need)
        .map(|(what, r, need)| format!("{what} reads {r:.1}:1, under {need}:1"))
        .collect()
}

pub fn radius(corners: &str) -> Radius {
    match corners {
        "square" => Radius { xs: 0.0, sm: 0.0, md: 0.0, lg: 0.0, xl: 0.0 },
        "round" => Radius { xs: 4.0, sm: 7.0, md: 10.0, lg: 14.0, xl: 18.0 },
        _ => Radius { xs: 2.0, sm: 4.0, md: 6.0, lg: 8.0, xl: 10.0 },
    }
}

/// The density the last look was built with, for what can't reach the kit (0 comfortable,
/// 1 compact): a stand-in for a Phantom token struct beside `Kit`.
static DENSITY: AtomicU8 = AtomicU8::new(0);

pub fn set_compact(on: bool) {
    DENSITY.store(on as u8, Ordering::Relaxed);
}

pub fn compact() -> bool {
    DENSITY.load(Ordering::Relaxed) == 1
}

/// `comfortable` as is, or `compact` while Density is Compact.
pub fn dens(comfortable: f32, compact_: f32) -> f32 {
    if compact() { compact_ } else { comfortable }
}

static MOTION_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_motion_off(off: bool) {
    MOTION_OFF.store(off, Ordering::Relaxed);
}

pub fn motion_off() -> bool {
    MOTION_OFF.load(Ordering::Relaxed)
}

/// `PHANTOM_PROTO`'s `name=value`, if set.
pub fn env(name: &str) -> Option<String> {
    let all = std::env::var("PHANTOM_PROTO").ok()?;
    all.split(';').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=').map(str::to_owned))
}
