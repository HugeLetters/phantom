//! Color math the Colors setting needs: blending as Rune blends (in sRGB), OKLab and OKLCH,
//! Okhsl for the picker, WCAG 2 contrast, and the translucent ink that gives a picked color over a
//! surface.
//!
//! A `rune::Color` holds linear sRGB; [`Rgb`] holds the sRGB bytes a person picks.

use rune::Color;

use super::setting::Rgb;

/// Linear to sRGB-encoded.
pub fn encode(x: f32) -> f32 {
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB-encoded to linear.
pub fn decode(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

impl Rgb {
    /// The bytes nearest `color`, opaque.
    pub fn of(color: Color) -> Rgb {
        Rgb([color.r, color.g, color.b].map(|c| (encode(c.clamp(0.0, 1.0)) * 255.0).round() as u8))
    }
}

/// `color` as it looks over the opaque `under`, blended in sRGB as Rune blends.
pub fn over(color: Color, under: Color) -> Color {
    super::colors::opaque_over(color, under)
}

/// `t` of the way from `a` to `b` in OKLab, opaque.
pub fn lerp_oklab(a: Color, b: Color, t: f32) -> Color {
    let ([la, aa, ba, _], [lb, ab, bb, _]) = (a.to_oklab(), b.to_oklab());
    let m = |x: f32, y: f32| x + (y - x) * t;
    Color::from_oklab([m(la, lb), m(aa, ab), m(ba, bb), 1.0])
}

/// OKLCH of `color`: lightness 0..1, chroma, hue in degrees 0..360.
pub fn oklch(color: Color) -> (f32, f32, f32) {
    let [l, a, b, _] = color.to_oklab();
    (l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0))
}

/// WCAG 2 contrast of `fg` drawn over `bg` (a translucent `fg` blended over it first).
pub fn contrast(fg: Color, bg: Color) -> f32 {
    let bg = Color { a: 1.0, ..bg };
    let (x, y) = (over(fg, bg).luminance(), bg.luminance());
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The translucent ink that, over `surface`, looks like `seen`: the color past `seen` from
/// `surface` as far as sRGB goes, at the least opacity that reaches it. How a translucent token
/// (a hairline, a hover) is picked: as it looks on the Surface, then drawn over whatever it's on.
pub fn ink_over(seen: Color, surface: Color) -> Color {
    let (s, v) = ([surface.r, surface.g, surface.b].map(encode), [seen.r, seen.g, seen.b].map(encode));
    let need = |i: usize| {
        let d = v[i] - s[i];
        let room = if d > 0.0 { 1.0 - s[i] } else { s[i] };
        if room <= 0.0 { 0.0 } else { d.abs() / room }
    };
    let alpha = need(0).max(need(1)).max(need(2));
    if alpha <= 1e-6 {
        return Color { a: 0.0, ..surface };
    }
    let ink = |i: usize| decode((s[i] + (v[i] - s[i]) / alpha).clamp(0.0, 1.0));
    Color { r: ink(0), g: ink(1), b: ink(2), a: alpha.min(1.0) }
}

/// The OKLab distance of two opaque colors, ×100: under about 1 they're the same color, under 2
/// no one sees a difference.
#[cfg(test)]
pub fn delta_e(a: Color, b: Color) -> f32 {
    let ([la, aa, ba, _], [lb, ab, bb, _]) = (a.to_oklab(), b.to_oklab());
    ((la - lb).powi(2) + (aa - ab).powi(2) + (ba - bb).powi(2)).sqrt() * 100.0
}

// Okhsl: hue, saturation and lightness on OKLab, with a lightness corrected to look even (the
// "toe") and a saturation relative to the most the sRGB gamut holds, so every (h, s, l) is
// displayable and a picker built on it has no crease where chroma would clip. Ported from Björn
// Ottosson's reference code (https://bottosson.github.io/posts/colorpicker/), MIT licensed:
// Copyright (c) 2021 Björn Ottosson. Permission is hereby granted, free of charge, to any person
// obtaining a copy of this software and associated documentation files, to deal in the Software
// without restriction, subject to the copyright notice and this permission notice being included
// in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
// WITHOUT WARRANTY OF ANY KIND.

const K1: f64 = 0.206;
const K2: f64 = 0.03;
const K3: f64 = (1.0 + K1) / (1.0 + K2);

fn toe(x: f64) -> f64 {
    0.5 * (K3 * x - K1 + ((K3 * x - K1) * (K3 * x - K1) + 4.0 * K2 * K3 * x).sqrt())
}

fn toe_inv(x: f64) -> f64 {
    (x * x + K1 * x) / (K3 * (x + K2))
}

/// The rows of the matrix from LMS (cubed) to linear sRGB: red, green, blue.
const LMS_TO_RGB: [[f64; 3]; 3] = [
    [4.0767416621, -3.3077115913, 0.2309699292],
    [-1.2684380046, 2.6097574011, -0.3413193965],
    [-0.0041960863, -0.7034186147, 1.7076147010],
];

/// How much each of L, M and S moves per unit of chroma along the hue (a, b).
fn lms_k(a: f64, b: f64) -> [f64; 3] {
    [0.3963377774 * a + 0.2158037573 * b, -0.1055613458 * a - 0.0638541728 * b, -0.0894841775 * a - 1.2914855480 * b]
}

fn lab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let [kl, km, ks] = lms_k(a, b);
    let lms = [(l + kl).powi(3), (l + km).powi(3), (l + ks).powi(3)];
    LMS_TO_RGB.map(|w| w[0] * lms[0] + w[1] * lms[1] + w[2] * lms[2])
}

fn linear_to_lab(r: f64, g: f64, b: f64) -> [f64; 3] {
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// The most saturation (C / L) sRGB holds along the unit hue (a, b).
fn max_saturation(a: f64, b: f64) -> f64 {
    let (k, w): ([f64; 5], [f64; 3]) = if -1.88170328 * a - 0.80936493 * b > 1.0 {
        ([1.19086277, 1.76576728, 0.59662641, 0.75515197, 0.56771245], LMS_TO_RGB[0])
    } else if 1.81444104 * a - 1.19445276 * b > 1.0 {
        ([0.73956515, -0.45954404, 0.08285427, 0.12541070, 0.14503204], LMS_TO_RGB[1])
    } else {
        ([1.35733652, -0.00915799, -1.15130210, -0.50559606, 0.00692167], LMS_TO_RGB[2])
    };
    let s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let [kl, km, ks] = lms_k(a, b);
    let (l_, m_, s_) = (1.0 + s * kl, 1.0 + s * km, 1.0 + s * ks);
    let f = w[0] * l_.powi(3) + w[1] * m_.powi(3) + w[2] * s_.powi(3);
    let f1 = w[0] * 3.0 * kl * l_ * l_ + w[1] * 3.0 * km * m_ * m_ + w[2] * 3.0 * ks * s_ * s_;
    let f2 = w[0] * 6.0 * kl * kl * l_ + w[1] * 6.0 * km * km * m_ + w[2] * 6.0 * ks * ks * s_;
    s - f * f1 / (f1 * f1 - 0.5 * f * f2)
}

/// The lightness and chroma of the most chromatic sRGB color of the hue.
fn cusp(a: f64, b: f64) -> (f64, f64) {
    let s = max_saturation(a, b);
    let rgb = lab_to_linear(1.0, s * a, s * b);
    let l = (1.0 / rgb[0].max(rgb[1]).max(rgb[2])).cbrt();
    (l, l * s)
}

/// Where the line from (l0, 0) to (l1, c1) leaves the gamut, as a fraction of it.
fn gamut_intersection(a: f64, b: f64, l1: f64, c1: f64, l0: f64, cusp: (f64, f64)) -> f64 {
    if (l1 - l0) * cusp.1 - (cusp.0 - l0) * c1 <= 0.0 {
        return cusp.1 * l0 / (c1 * cusp.0 + cusp.1 * (l0 - l1));
    }
    let mut t = cusp.1 * (l0 - 1.0) / (c1 * (cusp.0 - 1.0) + cusp.1 * (l0 - l1));
    let [kl, km, ks] = lms_k(a, b);
    let dl = l1 - l0;
    let (ldt, mdt, sdt) = (dl + c1 * kl, dl + c1 * km, dl + c1 * ks);
    let (ll, cc) = (l0 * (1.0 - t) + t * l1, t * c1);
    let (l_, m_, s_) = (ll + cc * kl, ll + cc * km, ll + cc * ks);
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let (ld, md, sd) = (3.0 * ldt * l_ * l_, 3.0 * mdt * m_ * m_, 3.0 * sdt * s_ * s_);
    let (ld2, md2, sd2) = (6.0 * ldt * ldt * l_, 6.0 * mdt * mdt * m_, 6.0 * sdt * sdt * s_);
    let step = |w: [f64; 3]| {
        let r = w[0] * l + w[1] * m + w[2] * s - 1.0;
        let r1 = w[0] * ld + w[1] * md + w[2] * sd;
        let r2 = w[0] * ld2 + w[1] * md2 + w[2] * sd2;
        let u = r1 / (r1 * r1 - 0.5 * r * r2);
        if u >= 0.0 { -r * u } else { f64::MAX }
    };
    t += step(LMS_TO_RGB[0]).min(step(LMS_TO_RGB[1])).min(step(LMS_TO_RGB[2]));
    t
}

/// The chromas Okhsl's saturation is measured against at lightness `l`: [C_0, C_mid, C_max].
fn chromas(l: f64, a: f64, b: f64) -> [f64; 3] {
    let cu = cusp(a, b);
    let c_max = gamut_intersection(a, b, l, 1.0, l, cu);
    let (s_max, t_max) = (cu.1 / cu.0, cu.1 / (1.0 - cu.0));
    let k = c_max / (l * s_max).min((1.0 - l) * t_max);
    let s_mid = 0.11516993
        + 1.0
            / (7.44778970
                + 4.15901240 * b
                + a * (-2.19557347
                    + 1.75198401 * b
                    + a * (-2.13704948 - 10.02301043 * b + a * (-4.24894561 + 5.38770819 * b + 4.69891013 * a))));
    let t_mid = 0.11239642
        + 1.0
            / (1.61320320 - 0.68124379 * b
                + a * (0.40370612
                    + 0.90148123 * b
                    + a * (-0.27087943 + 0.61223990 * b + a * (0.00299215 - 0.45399568 * b - 0.14661872 * a))));
    let (ca, cb) = (l * s_mid, (1.0 - l) * t_mid);
    let c_mid = 0.9 * k * (1.0 / (1.0 / ca.powi(4) + 1.0 / cb.powi(4))).sqrt().sqrt();
    let (ca, cb) = (l * 0.4, (1.0 - l) * 0.8);
    let c_0 = (1.0 / (1.0 / (ca * ca) + 1.0 / (cb * cb))).sqrt();
    [c_0, c_mid, c_max]
}

/// A color in Okhsl: hue, saturation and lightness, each 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Okhsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

impl Okhsl {
    /// The color, always displayable.
    pub fn color(self) -> Color {
        let (h, s, l) = (self.h as f64, (self.s as f64).clamp(0.0, 1.0), (self.l as f64).clamp(0.0, 1.0));
        if l >= 1.0 {
            return Color::WHITE;
        }
        if l <= 0.0 {
            return Color::BLACK;
        }
        let (a, b) = ((std::f64::consts::TAU * h).cos(), (std::f64::consts::TAU * h).sin());
        let big_l = toe_inv(l);
        let [c0, c_mid, c_max] = chromas(big_l, a, b);
        let c = if s < 0.8 {
            let (t, k1) = (1.25 * s, 0.8 * c0);
            t * k1 / (1.0 - (1.0 - k1 / c_mid) * t)
        } else {
            let (t, k1) = (5.0 * (s - 0.8), 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0);
            c_mid + t * k1 / (1.0 - (1.0 - k1 / (c_max - c_mid)) * t)
        };
        let [r, g, bl] = lab_to_linear(big_l, c * a, c * b).map(|x| x.clamp(0.0, 1.0) as f32);
        Color { r, g, b: bl, a: 1.0 }
    }

    /// `color`'s Okhsl. A gray has no hue: 0.
    pub fn of(color: Color) -> Okhsl {
        let [l, a, b] = linear_to_lab(color.r as f64, color.g as f64, color.b as f64);
        let c = a.hypot(b);
        if c < 1e-6 {
            return Okhsl { h: 0.0, s: 0.0, l: toe(l) as f32 };
        }
        let h = 0.5 + 0.5 * (-b).atan2(-a) / std::f64::consts::PI;
        // The most chroma sRGB holds jumps where the hue crosses from one branch of
        // `max_saturation` to the next, at pure blue, and a hue rounded to f32 can land on the
        // low side of that jump, where this color is out of gamut. Take the neighbouring f32 hue
        // that can carry it.
        let at = |h: f32| {
            let (ha, hb) = ((std::f64::consts::TAU * h as f64).cos(), (std::f64::consts::TAU * h as f64).sin());
            (h, chromas(l, ha, hb))
        };
        let near = h as f32;
        let up = f32::from_bits(if near > 0.0 { near.to_bits() + 1 } else { 1 });
        let down = f32::from_bits(near.to_bits().saturating_sub(1));
        let (h, [c0, c_mid, c_max]) =
            [near, up, down].into_iter().map(at).find(|(_, [_, _, c_max])| c <= *c_max).unwrap_or_else(|| at(near));
        let s = if c < c_mid {
            let k1 = 0.8 * c0;
            0.8 * (c / (k1 + (1.0 - k1 / c_mid) * c))
        } else {
            let k1 = 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0;
            let t = (c - c_mid) / (k1 + (1.0 - k1 / (c_max - c_mid)) * (c - c_mid));
            0.8 + 0.2 * t
        };
        Okhsl { h, s: s.clamp(0.0, 1.0) as f32, l: toe(l).clamp(0.0, 1.0) as f32 }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// `rgb`, to Okhsl and back, is the same bytes give or take one step of 255. The exception is a
    /// color on the edge of the gamut (saturation 1), mostly dark blues: Okhsl draws that edge as
    /// straight lines to the hue's cusp, which the true edge curves away from by up to four steps, so such a color reads back as the nearest one the line holds. That's a limit
    /// of the Okhsl model (Ottosson's reference has it too), not float error: the same in f64.
    fn assert_round_trips(rgb: [u8; 3]) {
        let okhsl = Okhsl::of(Rgb(rgb).color());
        let back = Rgb::of(okhsl.color());
        let allowed = if okhsl.s >= 1.0 - 1e-5 { 4 } else { 1 };
        for (a, b) in rgb.iter().zip(back.0) {
            assert!((*a as i32 - b as i32).abs() <= allowed, "{rgb:?} came back {:?}", back.0);
        }
    }

    /// Seen failing once as a property case (#454): a near-black blue, whose f32 hue fell on the
    /// low side of the jump in the gamut edge at pure blue.
    #[test]
    fn a_near_black_blue_round_trips_through_okhsl() {
        assert_round_trips([0, 0, 7]);
    }

    /// Every sRGB color; a few seconds in release: `cargo test -p gui --release -- --ignored every_srgb`.
    #[test]
    #[ignore]
    fn every_srgb_color_round_trips_through_okhsl() {
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                for b in 0..=255u8 {
                    assert_round_trips([r, g, b]);
                }
            }
        }
    }

    proptest! {
        /// A picked color, to Okhsl and back, is the same bytes, give or take one step of 255.
        #[test]
        fn okhsl_round_trips_within_one_step(rgb in any::<[u8; 3]>()) {
            assert_round_trips(rgb);
        }

        /// Every Okhsl triple is a color sRGB shows, and reads back as itself (but for hue where
        /// there's no saturation to see it by, and the ends of lightness).
        #[test]
        fn every_okhsl_is_displayable_and_reads_back(h in 0.0f32..1.0, s in 0.05f32..0.95, l in 0.05f32..0.95) {
            let c = Okhsl { h, s, l }.color();
            for x in [c.r, c.g, c.b] {
                prop_assert!((0.0..=1.0).contains(&x));
            }
            let back = Okhsl::of(c);
            prop_assert!((back.l - l).abs() < 1e-3, "lightness {l} came back {}", back.l);
            prop_assert!((back.s - s).abs() < 2e-3, "saturation {s} came back {}", back.s);
            let dh = (back.h - h).abs();
            prop_assert!(dh.min(1.0 - dh) < 2e-3, "hue {h} came back {}", back.h);
        }

        /// The ink of a picked color over a surface looks like it there.
        #[test]
        fn an_ink_over_a_surface_looks_like_what_was_picked(seen in any::<[u8; 3]>(), surface in any::<[u8; 3]>()) {
            let ink = ink_over(Rgb(seen).color(), Rgb(surface).color());
            let back = Rgb::of(over(ink, Rgb(surface).color()));
            for (a, b) in seen.iter().zip(back.0) {
                prop_assert!((*a as i32 - b as i32).abs() <= 1, "{seen:?} over {surface:?} came back {:?}", back.0);
            }
        }
    }

    /// Black on white is 21:1, a color on itself 1:1.
    #[test]
    fn contrast_is_wcags() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
    }

    /// Today's hairline in Light, black at .1 over white, comes back as that ink.
    #[test]
    fn the_ink_of_a_gray_over_white_is_black() {
        let ink = ink_over(over(Color::BLACK.alpha(0.1), Color::WHITE), Color::WHITE);
        assert!(ink.r < 1e-4 && ink.g < 1e-4 && ink.b < 1e-4);
        assert!((ink.a - 0.1).abs() < 1e-3);
    }
}
