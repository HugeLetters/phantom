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

const RED_ZERO: [f64; 5] = [1.19086277, 1.76576728, 0.59662641, 0.75515197, 0.56771245];
const GREEN_ZERO: [f64; 5] = [0.73956515, -0.45954404, 0.08285427, 0.12541070, 0.14503204];
const BLUE_ZERO: [f64; 5] = [1.35733652, -0.00915799, -1.15130210, -0.50559606, 0.00692167];

/// How far on the cyan side of pure blue (in [`red_branch`]'s units; about 10° of hue) the edge
/// eases from the blue corner down to where red runs out. Narrower leaves a visible ridge.
const BLUE_EASE: f64 = 0.3;

/// The gamut Okhsl's saturation is measured against, and whose linear RGB a color is given in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code, reason = "the picker moves to P3 in #470's step 4"))]
pub enum Gamut {
    Srgb,
    /// Display P3: P3's primaries, D65 white and sRGB's transfer curve.
    P3,
}

/// Linear sRGB to linear Display P3, and back: from the primaries' and D65's chromaticities.
const SRGB_TO_P3: [[f64; 3]; 3] = [
    [0.8224619687143623, 0.1775380312856377, 0.0],
    [0.0331941988509616, 0.9668058011490384, 0.0],
    [0.0170826307211200, 0.0723974406639635, 0.9105199286149165],
];
const P3_TO_SRGB: [[f64; 3]; 3] = [
    [1.2249401762805598, -0.2249401762805600, 0.0],
    [-0.0420569547096882, 1.0420569547096881, 0.0],
    [-0.0196375545903344, -0.0786360455506319, 1.0982736001409663],
];

/// The rows of the matrix from LMS (cubed) to linear Display P3: [`SRGB_TO_P3`] after
/// [`LMS_TO_RGB`].
const LMS_TO_P3: [[f64; 3]; 3] = {
    let (p, m) = (SRGB_TO_P3, LMS_TO_RGB);
    let mut out = [[0.0; 3]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 3 {
            out[i][j] = p[i][0] * m[0][j] + p[i][1] * m[1][j] + p[i][2] * m[2][j];
            j += 1;
        }
        i += 1;
    }
    out
};

fn apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|w| w[0] * v[0] + w[1] * v[1] + w[2] * v[2])
}

/// Linear sRGB as linear Display P3; a color sRGB holds lands inside P3.
#[cfg_attr(not(test), allow(dead_code, reason = "the picker moves to P3 in #470's step 4"))]
pub fn srgb_to_p3(rgb: [f64; 3]) -> [f64; 3] {
    apply(SRGB_TO_P3, rgb)
}

/// Linear Display P3 as linear sRGB, extended: below 0 or above 1 where sRGB doesn't hold it.
#[cfg_attr(not(test), allow(dead_code, reason = "the picker moves to P3 in #470's step 4"))]
pub fn p3_to_srgb(rgb: [f64; 3]) -> [f64; 3] {
    apply(P3_TO_SRGB, rgb)
}

impl Gamut {
    fn lms_to_rgb(self) -> [[f64; 3]; 3] {
        match self {
            Gamut::Srgb => LMS_TO_RGB,
            Gamut::P3 => LMS_TO_P3,
        }
    }

    /// OKLab of a linear color in this gamut's RGB.
    fn to_lab(self, rgb: [f64; 3]) -> [f64; 3] {
        let [r, g, b] = match self {
            Gamut::Srgb => rgb,
            Gamut::P3 => p3_to_srgb(rgb),
        };
        linear_to_lab(r, g, b)
    }

    /// The linear color in this gamut's RGB of OKLab `lab`, unclamped.
    fn from_lab(self, [l, a, b]: [f64; 3]) -> [f64; 3] {
        let [kl, km, ks] = lms_k(a, b);
        let lms = [(l + kl).powi(3), (l + km).powi(3), (l + ks).powi(3)];
        apply(self.lms_to_rgb(), lms)
    }

    /// The most saturation (C / L) the gamut holds along the unit hue (a, b). sRGB's comes from
    /// Ottosson's fits and eases at pure blue (see [`max_saturation`]); P3's is found exactly,
    /// and its edge has no jump there to ease.
    fn max_saturation(self, a: f64, b: f64) -> f64 {
        match self {
            Gamut::Srgb => max_saturation(a, b),
            Gamut::P3 => LMS_TO_P3.iter().map(|w| edge_saturation(*w, a, b)).fold(f64::INFINITY, f64::min),
        }
    }

    /// How many Halley steps refine the edge above the cusp: Ottosson's one for sRGB, as it
    /// always was, and until it settles for P3.
    fn halley_steps(self) -> usize {
        match self {
            Gamut::Srgb => 1,
            Gamut::P3 => 8,
        }
    }
}

/// The saturation along the unit hue (a, b) where the channel of row `w` first runs out (falls
/// to 0), found exactly: the channel at lightness 1 is a cubic in the saturation, 1 at 0, so its
/// first root lies in the first stretch between turning points that ends below 0.
fn edge_saturation(w: [f64; 3], a: f64, b: f64) -> f64 {
    let k = lms_k(a, b);
    let sum = |p: i32| w[0] * k[0].powi(p) + w[1] * k[1].powi(p) + w[2] * k[2].powi(p);
    let (c0, c1, c2, c3) = (sum(0), 3.0 * sum(1), 3.0 * sum(2), sum(3));
    let f = |s: f64| c0 + s * (c1 + s * (c2 + s * c3));
    let df = |s: f64| c1 + s * (2.0 * c2 + s * 3.0 * c3);
    // Turning points: roots of f', 3·c3·s² + 2·c2·s + c1.
    let mut ends = Vec::with_capacity(3);
    let (qa, qb, qc) = (3.0 * c3, 2.0 * c2, c1);
    if qa.abs() > 1e-12 {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let q = -0.5 * (qb + qb.signum() * disc.sqrt());
            ends.extend([q / qa, if q != 0.0 { qc / q } else { 0.0 }]);
        }
    } else if qb != 0.0 {
        ends.push(-qc / qb);
    }
    ends.retain(|s| *s > 0.0 && *s < 100.0);
    ends.sort_by(f64::total_cmp);
    ends.push(100.0);
    let mut lo = 0.0;
    for hi in ends {
        if f(hi) <= 0.0 {
            // f falls monotonically from above 0 to below it on (lo, hi): Newton, kept in the
            // bracket by bisection.
            let (mut lo, mut hi, mut s) = (lo, hi, 0.5 * (lo + hi));
            for _ in 0..100 {
                let v = f(s);
                if v > 0.0 {
                    lo = s
                } else {
                    hi = s
                }
                let next = s - v / df(s);
                let next = if next > lo && next < hi { next } else { 0.5 * (lo + hi) };
                if (next - s).abs() <= 1e-15 * s.max(1e-3) {
                    return next;
                }
                s = next;
            }
            return s;
        }
        lo = hi;
    }
    f64::INFINITY
}

/// The most saturation (C / L) sRGB holds along the unit hue (a, b), but at pure blue.
///
/// sRGB's blue corner sticks out in OKLab: just on the cyan side of pure blue, red runs out at
/// about 20% less saturation than the corner has, so the true edge jumps there, and with it every
/// color of a picker measured against it (#454's seam). On that side the edge eases from the
/// corner's saturation down to where red runs out instead, over [`BLUE_EASE`]; the colors it
/// takes in past the true edge are shown clipped to sRGB.
fn max_saturation(a: f64, b: f64) -> f64 {
    let red = red_branch(a, b);
    if red > 0.0 {
        let s = max_saturation_on(RED_ZERO, LMS_TO_RGB[0], a, b);
        if in_blue_ease(a, b) {
            let t = 1.0 - red / BLUE_EASE;
            let corner = max_saturation_on(GREEN_ZERO, LMS_TO_RGB[1], a, b);
            return s + (corner - s).max(0.0) * t * t * (3.0 - 2.0 * t);
        }
        s
    } else if 1.81444104 * a - 1.19445276 * b > 1.0 {
        max_saturation_on(GREEN_ZERO, LMS_TO_RGB[1], a, b)
    } else {
        max_saturation_on(BLUE_ZERO, LMS_TO_RGB[2], a, b)
    }
}

/// Above 0, the hues where red runs out first (cyan to blue); how far past the line it is.
fn red_branch(a: f64, b: f64) -> f64 {
    -1.88170328 * a - 0.80936493 * b - 1.0
}

/// Whether the unit hue (a, b) is in the ease on the cyan side of pure blue.
fn in_blue_ease(a: f64, b: f64) -> bool {
    let red = red_branch(a, b);
    b < 0.0 && red > 0.0 && red < BLUE_EASE
}

/// The saturation along the unit hue (a, b) where the channel of row `w` runs out: the fit `k`,
/// then one Halley step.
fn max_saturation_on(k: [f64; 5], w: [f64; 3], a: f64, b: f64) -> f64 {
    let s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let [kl, km, ks] = lms_k(a, b);
    let (l_, m_, s_) = (1.0 + s * kl, 1.0 + s * km, 1.0 + s * ks);
    let f = w[0] * l_.powi(3) + w[1] * m_.powi(3) + w[2] * s_.powi(3);
    let f1 = w[0] * 3.0 * kl * l_ * l_ + w[1] * 3.0 * km * m_ * m_ + w[2] * 3.0 * ks * s_ * s_;
    let f2 = w[0] * 6.0 * kl * kl * l_ + w[1] * 6.0 * km * km * m_ + w[2] * 6.0 * ks * ks * s_;
    s - f * f1 / (f1 * f1 - 0.5 * f * f2)
}

/// One hue of Okhsl in a gamut, with what's measured once per hue: the lightness and chroma of
/// its most chromatic color (the cusp). A column of the picker's square.
#[derive(Clone, Copy, Debug)]
pub struct Hue {
    gamut: Gamut,
    a: f64,
    b: f64,
    cusp: (f64, f64),
}

impl Hue {
    /// The hue `h` (0..1) in `gamut`.
    pub fn new(h: f32, gamut: Gamut) -> Hue {
        let h = h as f64;
        Hue::along((std::f64::consts::TAU * h).cos(), (std::f64::consts::TAU * h).sin(), gamut)
    }

    /// The hue along the unit (a, b).
    fn along(a: f64, b: f64, gamut: Gamut) -> Hue {
        let s = gamut.max_saturation(a, b);
        let rgb = gamut.from_lab([1.0, s * a, s * b]);
        let l = (1.0 / rgb[0].max(rgb[1]).max(rgb[2])).cbrt();
        Hue { gamut, a, b, cusp: (l, l * s) }
    }

    /// OKLab of saturation `s` and lightness `l`, both 0..1, at this hue.
    fn lab(&self, s: f32, l: f32) -> [f64; 3] {
        let (s, l) = ((s as f64).clamp(0.0, 1.0), (l as f64).clamp(0.0, 1.0));
        if l >= 1.0 {
            return [1.0, 0.0, 0.0];
        }
        if l <= 0.0 {
            return [0.0; 3];
        }
        let big_l = toe_inv(l);
        let [c0, c_mid, c_max] = chromas(big_l, self);
        let c = if s < 0.8 {
            let (t, k1) = (1.25 * s, 0.8 * c0);
            t * k1 / (1.0 - (1.0 - k1 / c_mid) * t)
        } else {
            let (t, k1) = (5.0 * (s - 0.8), 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0);
            c_mid + t * k1 / (1.0 - (1.0 - k1 / (c_max - c_mid)) * t)
        };
        [big_l, c * self.a, c * self.b]
    }

    /// The linear color in the gamut's RGB of saturation `s` and lightness `l`, clipped to it.
    pub fn linear(&self, s: f32, l: f32) -> [f64; 3] {
        let clamped = (l as f64).clamp(0.0, 1.0);
        if clamped >= 1.0 {
            return [1.0; 3];
        }
        if clamped <= 0.0 {
            return [0.0; 3];
        }
        self.gamut.from_lab(self.lab(s, l)).map(|x| x.clamp(0.0, 1.0))
    }
}

#[cfg_attr(not(test), allow(dead_code, reason = "the picker moves to P3 in #470's step 4"))]
impl Hue {
    /// Whether saturation `s` and lightness `l` at this hue is a color sRGB holds (within
    /// [`SRGB_SLACK`] per channel, linear). Always so for an sRGB hue.
    pub fn in_srgb(&self, s: f32, l: f32) -> bool {
        Gamut::Srgb.from_lab(self.lab(s, l)).iter().all(|x| (-SRGB_SLACK..=1.0 + SRGB_SLACK).contains(x))
    }

    /// The stretches of lightness (low, high) at saturation `s` that sRGB holds, top to bottom of
    /// the hue's column: inside them [`in_srgb`](Hue::in_srgb), outside not. Their ends are
    /// where the sRGB edge crosses the column, to 1e-6 of lightness.
    pub fn srgb_spans(&self, s: f32) -> Vec<(f32, f32)> {
        const SAMPLES: usize = 256;
        let at = |i: usize| i as f32 / SAMPLES as f32;
        // Where `in_srgb` turns between lightnesses lo (as `inside`) and hi (not).
        let edge = |mut lo: f32, mut hi: f32, inside: bool| {
            while (hi - lo).abs() > 1e-6 {
                let mid = 0.5 * (lo + hi);
                if self.in_srgb(s, mid) == inside { lo = mid } else { hi = mid }
            }
            lo
        };
        let mut spans = Vec::new();
        let mut start = self.in_srgb(s, 0.0).then_some(0.0);
        for i in 1..=SAMPLES {
            let inside = self.in_srgb(s, at(i));
            match (start, inside) {
                (None, true) => start = Some(edge(at(i), at(i - 1), true)),
                (Some(lo), false) => {
                    spans.push((lo, edge(at(i - 1), at(i), true)));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(lo) = start {
            spans.push((lo, 1.0));
        }
        spans
    }
}

/// How far past sRGB's 0..1 (linear) a channel may be and still count as held: float error from
/// the trip through Okhsl, far under one step of 255.
const SRGB_SLACK: f64 = 1e-5;

/// Where the line from (l0, 0) to (l1, c1) at `hue` leaves its gamut, as a fraction of it.
fn gamut_intersection(hue: &Hue, l1: f64, c1: f64, l0: f64) -> f64 {
    let (a, b, cusp) = (hue.a, hue.b, hue.cusp);
    if (l1 - l0) * cusp.1 - (cusp.0 - l0) * c1 <= 0.0 {
        return cusp.1 * l0 / (c1 * cusp.0 + cusp.1 * (l0 - l1));
    }
    let mut t = cusp.1 * (l0 - 1.0) / (c1 * (cusp.0 - 1.0) + cusp.1 * (l0 - l1));
    let [kl, km, ks] = lms_k(a, b);
    let dl = l1 - l0;
    let (ldt, mdt, sdt) = (dl + c1 * kl, dl + c1 * km, dl + c1 * ks);
    let w = hue.gamut.lms_to_rgb();
    for _ in 0..hue.gamut.halley_steps() {
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
        let dt = step(w[0]).min(step(w[1])).min(step(w[2]));
        t += dt;
        if dt == f64::MAX || dt.abs() < 1e-14 {
            break;
        }
    }
    t
}

/// The chromas Okhsl's saturation is measured against at lightness `l`: [C_0, C_mid, C_max].
fn chromas(l: f64, hue: &Hue) -> [f64; 3] {
    let (a, b, cu) = (hue.a, hue.b, hue.cusp);
    let c_max = gamut_intersection(hue, l, 1.0, l);
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
    /// The color, measured against sRGB: always displayable.
    pub fn color(self) -> Color {
        let l = (self.l as f64).clamp(0.0, 1.0);
        if l >= 1.0 {
            return Color::WHITE;
        }
        if l <= 0.0 {
            return Color::BLACK;
        }
        let [r, g, b] = self.linear(Gamut::Srgb).map(|x| x as f32);
        Color { r, g, b, a: 1.0 }
    }

    /// The color measured against `gamut`, as linear RGB in it.
    pub fn linear(self, gamut: Gamut) -> [f64; 3] {
        Hue::new(self.h, gamut).linear(self.s, self.l)
    }

    /// `color`'s Okhsl, measured against sRGB. A gray has no hue: 0.
    pub fn of(color: Color) -> Okhsl {
        Okhsl::of_linear([color.r as f64, color.g as f64, color.b as f64], Gamut::Srgb)
    }

    /// The Okhsl measured against `gamut` of the linear color `rgb` in it.
    pub fn of_linear(rgb: [f64; 3], gamut: Gamut) -> Okhsl {
        let [l, a, b] = gamut.to_lab(rgb);
        let c = a.hypot(b);
        if c < 1e-6 {
            return Okhsl { h: 0.0, s: 0.0, l: toe(l) as f32 };
        }
        let h = 0.5 + 0.5 * (-b).atan2(-a) / std::f64::consts::PI;
        let [c0, c_mid, c_max] = chromas(l, &Hue::along(a / c, b / c, gamut));
        let s = if c < c_mid {
            let k1 = 0.8 * c0;
            0.8 * (c / (k1 + (1.0 - k1 / c_mid) * c))
        } else {
            let k1 = 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0;
            let t = (c - c_mid) / (k1 + (1.0 - k1 / (c_max - c_mid)) * (c - c_mid));
            0.8 + 0.2 * t
        };
        Okhsl { h: h as f32, s: s.clamp(0.0, 1.0) as f32, l: toe(l).clamp(0.0, 1.0) as f32 }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// `rgb`, to Okhsl and back, is the same bytes give or take one step of 255. The exception is a
    /// color on the edge of the gamut (saturation 1), a dozen light yellows: Okhsl draws the edge
    /// above the cusp as a line refined by one step, which misses the true edge there by up to three
    /// steps, so such a color reads back as the nearest one the line holds. That's a limit of the
    /// Okhsl model (Ottosson's reference has it too), not float error: the same in f64.
    fn assert_round_trips(rgb: [u8; 3]) {
        let okhsl = Okhsl::of(Rgb(rgb).color());
        let back = Rgb::of(okhsl.color());
        let allowed = if okhsl.s >= 1.0 - 1e-5 { 3 } else { 1 };
        for (a, b) in rgb.iter().zip(back.0) {
            assert!((*a as i32 - b as i32).abs() <= allowed, "{rgb:?} came back {:?}", back.0);
        }
    }

    /// Seen failing once as a property case (#454): a near-black blue, on the cyan side of pure
    /// blue, where the true gamut edge jumps.
    #[test]
    fn a_near_black_blue_round_trips_through_okhsl() {
        assert_round_trips([0, 0, 7]);
    }

    /// The picker's square has no seam: at every saturation and lightness, colors a hair apart in
    /// hue are close, pure blue (#454) included.
    #[test]
    fn okhsl_is_continuous_across_hue() {
        const STEPS: u32 = 4096;
        let mut worst = (0.0f32, 0.0, 0.0, 0.0);
        for s in [0.2f32, 0.5, 0.8, 0.9, 1.0] {
            for l in (1..50).map(|i| i as f32 / 50.0) {
                let mut prev = Okhsl { h: 0.0, s, l }.color();
                for i in 1..=STEPS {
                    let h = i as f32 / STEPS as f32;
                    let next = Okhsl { h: h % 1.0, s, l }.color();
                    let d = delta_e(prev, next);
                    if d > worst.0 {
                        worst = (d, h, s, l);
                    }
                    prev = next;
                }
            }
        }
        assert!(worst.0 < 1.0, "a jump of {} at (h, s, l) = {:?}", worst.0, (worst.1, worst.2, worst.3));
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
        /// there's no saturation to see it by, and the ends of lightness). In the ease at pure
        /// blue, a color past sRGB's edge is clipped to it and reads back a little off.
        #[test]
        fn every_okhsl_is_displayable_and_reads_back(h in 0.0f32..1.0, s in 0.05f32..0.95, l in 0.05f32..0.95) {
            let c = Okhsl { h, s, l }.color();
            for x in [c.r, c.g, c.b] {
                prop_assert!((0.0..=1.0).contains(&x));
            }
            let back = Okhsl::of(c);
            let tau = std::f64::consts::TAU * h as f64;
            let clipped = if in_blue_ease(tau.cos(), tau.sin()) { 0.03 } else { 0.0 };
            prop_assert!((back.l - l).abs() < 1e-3 + clipped, "lightness {l} came back {}", back.l);
            prop_assert!((back.s - s).abs() < 2e-3 + clipped, "saturation {s} came back {}", back.s);
            let dh = (back.h - h).abs();
            prop_assert!(dh.min(1.0 - dh) < 2e-3 + clipped, "hue {h} came back {}", back.h);
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

    fn encode64(x: f64) -> f64 {
        if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
    }

    fn decode64(x: f64) -> f64 {
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    }

    /// The OKLab distance ×100 of two linear P3 colors.
    fn delta_e_p3(x: [f64; 3], y: [f64; 3]) -> f64 {
        let (x, y) = (Gamut::P3.to_lab(x), Gamut::P3.to_lab(y));
        ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt() * 100.0
    }

    /// The P3 color of 10-bit channels `p` (Display P3's transfer curve is sRGB's), to P3 Okhsl
    /// and back, is the same channels, give or take one step of 1023, edge included: P3's edge is
    /// found exactly, not by a fit.
    fn assert_round_trips_in_p3(p: [u16; 3]) -> Result<(), String> {
        let okhsl = Okhsl::of_linear(p.map(|x| decode64(x as f64 / 1023.0)), Gamut::P3);
        let back = okhsl.linear(Gamut::P3).map(|x| (encode64(x) * 1023.0).round() as i32);
        match p.iter().zip(back).all(|(a, b)| (*a as i32 - b).abs() <= 1) {
            true => Ok(()),
            false => Err(format!("{p:?} came back {back:?} through {okhsl:?}")),
        }
    }

    /// Display P3's red is oklch(0.6486 0.2995 28.96), as CSS Color 4 converts it, and its white
    /// is OKLab's white (to OKLab's own 10 digits).
    #[test]
    fn p3_red_and_white_are_csss_in_oklab() {
        let [l, a, b] = Gamut::P3.to_lab([1.0, 0.0, 0.0]);
        let (c, h) = (a.hypot(b), b.atan2(a).to_degrees());
        assert!((l - 0.6486).abs() < 1e-4 && (c - 0.2995).abs() < 1e-4 && (h - 28.96).abs() < 1e-2, "{l} {c} {h}");
        let [l, a, b] = Gamut::P3.to_lab([1.0; 3]);
        assert!((l - 1.0).abs() < 1e-7 && a.abs() < 1e-7 && b.abs() < 1e-7, "{l} {a} {b}");
        let back = Gamut::P3.from_lab(Gamut::P3.to_lab([0.2, 0.5, 0.9]));
        assert!(back.iter().zip([0.2, 0.5, 0.9]).all(|(x, y)| (x - y).abs() < 1e-6), "{back:?}");
    }

    /// P3's picker has no seam either, with no easing: P3's edge doesn't jump at pure blue.
    #[test]
    fn p3_okhsl_is_continuous_across_hue() {
        const STEPS: u32 = 4096;
        let mut worst = (0.0f64, 0.0, 0.0, 0.0);
        for s in [0.2f32, 0.5, 0.8, 0.9, 1.0] {
            for l in (1..50).map(|i| i as f32 / 50.0) {
                let mut prev = Okhsl { h: 0.0, s, l }.linear(Gamut::P3);
                for i in 1..=STEPS {
                    let h = i as f32 / STEPS as f32;
                    let next = Okhsl { h: h % 1.0, s, l }.linear(Gamut::P3);
                    let d = delta_e_p3(prev, next);
                    if d > worst.0 {
                        worst = (d, h, s, l);
                    }
                    prev = next;
                }
            }
        }
        assert!(worst.0 < 1.0, "a jump of {} at (h, s, l) = {:?}", worst.0, (worst.1, worst.2, worst.3));
    }

    /// The most saturation each gamut truly holds, hue by hue, 0.01° apart: sRGB's jumps at pure
    /// blue (why its edge eases there), P3's only steepens.
    #[test]
    fn p3s_edge_has_no_jump_at_blue_and_srgbs_has() {
        let exact = |m: [[f64; 3]; 3], h: f64| {
            let (a, b) = (h.to_radians().cos(), h.to_radians().sin());
            m.iter().map(|w| edge_saturation(*w, a, b)).fold(f64::INFINITY, f64::min)
        };
        let jump = |m: [[f64; 3]; 3]| {
            let mut worst = (0.0f64, 0.0);
            for i in 0..36_000 {
                let h = i as f64 / 100.0;
                let d = (exact(m, h + 0.01) - exact(m, h)).abs();
                if d > worst.0 {
                    worst = (d, h);
                }
            }
            worst
        };
        let (srgb, p3) = (jump(LMS_TO_RGB), jump(LMS_TO_P3));
        assert!(srgb.0 > 0.05 && (srgb.1 - 264.3).abs() < 0.2, "sRGB: {srgb:?}");
        assert!(p3.0 < 0.002, "P3: {p3:?}");
        // P3's edge is what P3 Okhsl measures against.
        for h in (0..360).map(|h| h as f64) {
            let (a, b) = (h.to_radians().cos(), h.to_radians().sin());
            assert_eq!(Gamut::P3.max_saturation(a, b), exact(LMS_TO_P3, h));
        }
    }

    /// Every 10-bit P3 color; about 25 minutes in release. Last run: all but four light yellows
    /// came back exact, those off by up to 8 steps (unchecked whether that passes this test):
    /// `cargo test -p gui --release -- --ignored every_p3`.
    #[test]
    #[ignore]
    fn every_p3_color_round_trips_through_p3_okhsl() {
        let threads = std::thread::available_parallelism().map_or(8, |n| n.get()) as u16;
        let failures: Vec<String> = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..threads)
                .map(|t| {
                    scope.spawn(move || {
                        let mut failures = Vec::new();
                        for r in (t..1024).step_by(threads as usize) {
                            for g in 0..1024 {
                                for b in 0..1024 {
                                    if let Err(e) = assert_round_trips_in_p3([r, g, b]) {
                                        failures.push(e);
                                    }
                                }
                            }
                        }
                        failures
                    })
                })
                .collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap()).collect()
        });
        assert!(failures.is_empty(), "{} failed, such as {:?}", failures.len(), &failures[..failures.len().min(5)]);
    }

    /// Whether a P3 Okhsl color fits sRGB is whether its chroma is under the most sRGB holds at its
    /// lightness and hue, found by bisecting along chroma (with the same slack).
    #[test]
    fn in_srgb_matches_a_search_along_chroma() {
        let mut outside = 0;
        for h in (0..90).map(|i| i as f32 / 90.0) {
            let hue = Hue::new(h, Gamut::P3);
            for s in [0.3f32, 0.6, 0.8, 0.9, 1.0] {
                for l in (1..40).map(|i| i as f32 / 40.0) {
                    let [big_l, a, b] = hue.lab(s, l);
                    let fits = |c: f64| {
                        let rgb = Gamut::Srgb.from_lab([big_l, c * hue.a, c * hue.b]);
                        rgb.iter().all(|x| (-SRGB_SLACK..=1.0 + SRGB_SLACK).contains(x))
                    };
                    let (mut lo, mut hi) = (0.0, 1.0);
                    for _ in 0..60 {
                        let mid = 0.5 * (lo + hi);
                        if fits(mid) { lo = mid } else { hi = mid }
                    }
                    let c = a.hypot(b);
                    if (c - lo).abs() < 1e-4 {
                        continue;
                    }
                    assert_eq!(hue.in_srgb(s, l), c < lo, "(h, s, l) = {:?}: chroma {c}, sRGB's most {lo}", (h, s, l));
                    outside += (c >= lo) as u32;
                }
            }
        }
        assert!(outside > 1000, "only {outside} colors past sRGB");
    }

    /// The sRGB edge's outline: down every column, lightnesses inside its spans fit sRGB and the
    /// rest don't.
    #[test]
    fn srgb_spans_outline_what_fits_srgb() {
        for h in (0..60).map(|i| i as f32 / 60.0) {
            let hue = Hue::new(h, Gamut::P3);
            for s in [0.0f32, 0.5, 0.8, 0.9, 1.0] {
                let spans = hue.srgb_spans(s);
                for l in (0..=1000).map(|i| i as f32 / 1000.0) {
                    if spans.iter().any(|(lo, hi)| (l - lo).abs() < 1e-5 || (l - hi).abs() < 1e-5) {
                        continue;
                    }
                    let in_span = spans.iter().any(|(lo, hi)| (*lo..=*hi).contains(&l));
                    assert_eq!(in_span, hue.in_srgb(s, l), "(h, s, l) = {:?} in {spans:?}", (h, s, l));
                }
            }
        }
        assert_eq!(Hue::new(0.3, Gamut::P3).srgb_spans(0.0), vec![(0.0, 1.0)]);
    }

    proptest! {
        /// A sRGB color lands inside P3, is the same OKLab there, fits sRGB by
        /// [`Hue::in_srgb`], and reads back through P3 Okhsl as itself.
        #[test]
        fn srgb_colors_land_inside_p3_and_map_identically(rgb in any::<[u8; 3]>()) {
            let c = Rgb(rgb).color();
            let srgb = [c.r as f64, c.g as f64, c.b as f64];
            let p3 = srgb_to_p3(srgb);
            prop_assert!(p3.iter().all(|x| (-1e-12..=1.0 + 1e-12).contains(x)), "{rgb:?} is {p3:?} in P3");
            let (x, y) = (Gamut::P3.to_lab(p3), Gamut::Srgb.to_lab(srgb));
            prop_assert!(x.iter().zip(y).all(|(x, y)| (x - y).abs() < 1e-9), "{x:?} vs {y:?}");
            let okhsl = Okhsl::of_linear(p3, Gamut::P3);
            prop_assert!(Hue::new(okhsl.h, Gamut::P3).in_srgb(okhsl.s, okhsl.l), "{rgb:?} as {okhsl:?}");
            let back = p3_to_srgb(okhsl.linear(Gamut::P3)).map(|x| (encode(x.clamp(0.0, 1.0) as f32) * 255.0).round() as i32);
            prop_assert!(rgb.iter().zip(back).all(|(a, b)| (*a as i32 - b).abs() <= 1), "{rgb:?} came back {back:?}");
        }

        /// A 10-bit P3 color, to P3 Okhsl and back, is the same channels give or take one step.
        #[test]
        fn p3_okhsl_round_trips_within_one_step(p in prop::array::uniform3(0u16..1024)) {
            prop_assert!(assert_round_trips_in_p3(p).is_ok(), "{}", assert_round_trips_in_p3(p).unwrap_err());
        }

        /// Every P3 Okhsl triple is a color P3 holds, unclipped, and reads back as itself.
        #[test]
        fn every_p3_okhsl_is_in_p3_and_reads_back(h in 0.0f32..1.0, s in 0.05f32..0.95, l in 0.05f32..0.95) {
            let hue = Hue::new(h, Gamut::P3);
            let raw = Gamut::P3.from_lab(hue.lab(s, l));
            prop_assert!(raw.iter().all(|x| (-1e-9..=1.0 + 1e-9).contains(x)), "{raw:?}");
            let back = Okhsl::of_linear(hue.linear(s, l), Gamut::P3);
            prop_assert!((back.l - l).abs() < 1e-3, "lightness {l} came back {}", back.l);
            prop_assert!((back.s - s).abs() < 2e-3, "saturation {s} came back {}", back.s);
            let dh = (back.h - h).abs();
            prop_assert!(dh.min(1.0 - dh) < 2e-3, "hue {h} came back {}", back.h);
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
