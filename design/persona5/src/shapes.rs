//! Geometry: polygons as point lists, rounded corners, jagged and bent outlines, stars, moons and comets, and clipping against convex shapes.

use std::f32::consts::PI;

use crate::noise::*;

/// A strip `w` × `h` of tape, its short ends torn.
pub fn tape_strip(w: f32, h: f32, seed: u32) -> Vec<[f32; 2]> {
    let n = 5;
    let tear = |k: u32| 2.0 + 4.0 * noise(seed.wrapping_mul(31).wrapping_add(k));
    let mut pts = vec![[6.0, 0.0], [w - 6.0, 0.0]];
    for i in 1..n {
        pts.push([w - 6.0 + if i % 2 == 1 { tear(i) } else { -tear(i) * 0.6 }, h * i as f32 / n as f32]);
    }
    pts.push([w - 6.0, h]);
    pts.push([6.0, h]);
    for i in (1..n).rev() {
        pts.push([6.0 - if i % 2 == 1 { tear(i + 20) } else { -tear(i + 20) * 0.6 }, h * i as f32 / n as f32]);
    }
    pts
}

/// A cut-out: a box whose corners and edges are knocked off at random, as if cut with scissors.
/// `amp` is how far an edge wanders; `step` how often it turns.
pub fn jagged(x: f32, y: f32, w: f32, h: f32, amp: f32, step: f32, seed: u32) -> Vec<[f32; 2]> {
    let mut pts = Vec::new();
    let mut k = seed.wrapping_mul(97);
    let mut j = || {
        k = k.wrapping_add(1);
        (noise(k) - 0.5) * 2.0 * amp
    };
    let n = |len: f32| ((len / step).round() as usize).max(1);
    let (nx, ny) = (n(w), n(h));
    for i in 0..nx {
        pts.push([x + w * i as f32 / nx as f32 + j() * 0.5, y + j().abs() * -1.0 + amp * 0.5]);
    }
    for i in 0..ny {
        pts.push([x + w - j().abs() + amp * 0.5, y + h * i as f32 / ny as f32 + j() * 0.5]);
    }
    for i in 0..nx {
        pts.push([x + w - w * i as f32 / nx as f32 + j() * 0.5, y + h + j().abs() - amp * 0.5]);
    }
    for i in 0..ny {
        pts.push([x + j().abs() - amp * 0.5, y + h - h * i as f32 / ny as f32 + j() * 0.5]);
    }
    pts
}

/// A slab cut on the slant: a quad over the `w` × `h` box with no square corner. The top and
/// bottom edges lean by up to `slant` points each way and the sides tilt, as Persona 5 cuts its
/// menu slabs. `seed` picks the cut.
pub fn cut(w: f32, h: f32, slant: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(53).wrapping_add(k));
    // Every corner moves in; the sides lean the same way, so the slab reads as one cut.
    let lean = slant * (0.5 + 0.5 * r(1));
    vec![
        [lean * (0.6 + 0.4 * r(2)), -slant * 0.25 * r(3)],
        [w + slant * 0.2 * r(4), slant * (0.15 + 0.25 * r(5))],
        [w - lean * (0.6 + 0.4 * r(6)), h + slant * 0.2 * r(7)],
        [-slant * 0.2 * r(8), h - slant * (0.1 + 0.25 * r(9))],
    ]
}

/// `pts` moved by (`dx`, `dy`).
pub fn moved(pts: &[[f32; 2]], dx: f32, dy: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|p| [p[0] + dx, p[1] + dy]).collect()
}

/// `pts` turned `a` radians about (`cx`, `cy`).
pub fn turned(pts: &[[f32; 2]], a: f32, cx: f32, cy: f32) -> Vec<[f32; 2]> {
    let (s, c) = a.sin_cos();
    pts.iter().map(|p| [cx + (p[0] - cx) * c - (p[1] - cy) * s, cy + (p[0] - cx) * s + (p[1] - cy) * c]).collect()
}

/// `pts` grown outward by `d` with mitred (sharp) corners: draw it under `pts` for a thick
/// outline that keeps the cut-out's points.
pub fn inflate(pts: &[[f32; 2]], d: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    // Which way is out: the sign of the polygon's area.
    let area: f32 = (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum();
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    let norm = |a: [f32; 2], b: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-4);
        [s * dy / l, -s * dx / l]
    };
    (0..n)
        .map(|i| {
            let (p, c, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (n1, n2) = (norm(p, c), norm(c, q));
            let m = [n1[0] + n2[0], n1[1] + n2[1]];
            let dot = (m[0] * n1[0] + m[1] * n1[1]).max(0.35);
            [c[0] + m[0] * d / dot, c[1] + m[1] * d / dot]
        })
        .collect()
}

pub fn circle(x: f32, y: f32, r: f32) -> Vec<[f32; 2]> {
    let n = if r < 2.0 {
        6
    } else if r < 5.0 {
        10
    } else {
        18
    };
    (0..n).map(|i| i as f32 * 2.0 * PI / n as f32).map(|a| [x + a.cos() * r, y + a.sin() * r]).collect()
}

/// A starburst of `n` points between radii `r0` and `r1`.
pub fn burst(cx: f32, cy: f32, r0: f32, r1: f32, n: usize, turn: f32, seed: u32) -> Vec<[f32; 2]> {
    (0..n * 2)
        .map(|i| {
            let a = turn + i as f32 * PI / n as f32;
            let r = if i % 2 == 0 { r1 * (0.8 + 0.2 * noise(seed + i as u32)) } else { r0 };
            [cx + a.cos() * r, cy + a.sin() * r]
        })
        .collect()
}

/// The shape an active element takes, as Persona 5 menus highlight a choice: a quad around the
/// `w` × `h` box whose corners are thrown out by up to `reach`, with one corner drawn out into
/// a long spike. `seed` picks the throw, so each activation lands differently.
pub fn blade(w: f32, h: f32, reach: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(31).wrapping_add(k));
    // Sheared one way or the other, so no corner comes out square.
    let shear = reach * (0.8 + 0.8 * r(11)) * if r(12) < 0.5 { -1.0 } else { 1.0 };
    let mut pts = vec![
        [-reach * (0.3 + r(1)) + shear, -reach * (0.2 + 0.6 * r(2))],
        [w + reach * (0.2 + 0.8 * r(3)) + shear, -reach * 0.5 * r(4)],
        [w + reach * (0.1 + 0.5 * r(5)) - shear, h + reach * (0.2 + 0.6 * r(6))],
        [-reach * (0.1 + 0.6 * r(7)) - shear, h + reach * 0.4 * r(8)],
    ];
    // The spike: out of one corner, along the box's diagonal, then half again.
    let c = (r(9) * 4.0) as usize % 4;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let p = pts[c];
    let (dx, dy) = (p[0] - cx, p[1] - cy);
    let l = (dx * dx + dy * dy).sqrt().max(1e-3);
    let len = reach * (1.4 + 1.2 * r(10));
    pts[c] = [p[0] + dx / l * len, p[1] + dy / l * len * 0.6];
    pts
}

/// A *living* quad, as the game draws a highlighted choice: a quad around the `w` × `h` box
/// whose corners never rest. Each corner drifts on its own slow wave, so the shape keeps
/// reshaping while it's active. `reach` is how far the corners go out; `seed` gives each layer
/// its own rhythm and lean.
pub fn living(w: f32, h: f32, reach: f32, t: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(29).wrapping_add(k));
    let wave = |k: u32| {
        let speed = 2.2 + 2.4 * r(k);
        let phase = r(k + 40) * std::f32::consts::TAU;
        0.5 + 0.5 * (t * speed + phase).sin()
    };
    // Leaning one way, as a cut, and the far corners reaching the furthest.
    let lean = reach * (0.6 + 0.6 * r(90)) * if r(91) < 0.5 { -1.0 } else { 1.0 };
    vec![
        [-reach * (0.2 + 0.8 * wave(1)) + lean, -reach * (0.3 + 0.9 * wave(2))],
        [w + reach * (0.4 + 1.4 * wave(3)) + lean, -reach * (0.0 + 0.5 * wave(4))],
        [w + reach * (0.1 + 0.6 * wave(5)) - lean, h + reach * (0.3 + 0.9 * wave(6))],
        [-reach * (0.3 + 1.2 * wave(7)) - lean, h + reach * (0.0 + 0.5 * wave(8))],
    ]
}

/// `poly` clipped to the convex polygon `clip` (Sutherland–Hodgman); empty when they don't meet.
pub fn clip_convex(poly: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = clip.len();
    // Which side is inside: the side the clip polygon's centre is on.
    let cx = clip.iter().map(|p| p[0]).sum::<f32>() / n as f32;
    let cy = clip.iter().map(|p| p[1]).sum::<f32>() / n as f32;
    let mut out = poly.to_vec();
    for i in 0..n {
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let side = |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let inner = side([cx, cy]).signum();
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p) * inner, side(q) * inner);
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}

/// A moon `lit` (0 new, 1 full) lit from the right, as horizontal slices between the limb and
/// the terminator: each slice is convex, so it always fills as drawn.
pub(crate) fn moon_phase(cx: f32, cy: f32, r: f32, lit: f32) -> Vec<Vec<[f32; 2]>> {
    let k = 1.0 - 2.0 * lit.clamp(0.0, 1.0);
    let n = 16;
    let at = |i: usize| -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
    (0..n)
        .map(|i| {
            // Each slice runs a quarter point into the next, so no seam shows between them.
            let (a0, a1) = (at(i), at(i + 1));
            let (y0, y1) = (cy + r * a0.sin() - 0.25, cy + r * a1.sin() + 0.25);
            vec![[cx + r * a0.cos(), y0], [cx + r * a1.cos(), y1], [cx + k * r * a1.cos(), y1], [cx + k * r * a0.cos(), y0]]
        })
        .collect()
}

/// Whether `p` is inside the convex polygon `poly` (either winding).
pub(crate) fn inside(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if c.abs() < 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// The part of segment `a`–`b` inside the convex polygon `poly`, if any (Cyrus–Beck).
pub(crate) fn clip_segment(a: [f32; 2], b: [f32; 2], poly: &[[f32; 2]]) -> Option<([f32; 2], [f32; 2])> {
    let n = poly.len();
    let cx = poly.iter().map(|p| p[0]).sum::<f32>() / n as f32;
    let cy = poly.iter().map(|p| p[1]).sum::<f32>() / n as f32;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let d = [b[0] - a[0], b[1] - a[1]];
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        // The edge's inward normal: the side the centre is on.
        let mut nrm = [q[1] - p[1], p[0] - q[0]];
        if nrm[0] * (cx - p[0]) + nrm[1] * (cy - p[1]) < 0.0 {
            nrm = [-nrm[0], -nrm[1]];
        }
        let num = nrm[0] * (a[0] - p[0]) + nrm[1] * (a[1] - p[1]);
        let den = nrm[0] * d[0] + nrm[1] * d[1];
        if den.abs() < 1e-6 {
            if num < 0.0 {
                return None;
            }
        } else {
            let t = -num / den;
            if den > 0.0 { t0 = t0.max(t) } else { t1 = t1.min(t) }
        }
    }
    (t0 < t1).then(|| ([a[0] + d[0] * t0, a[1] + d[1] * t0], [a[0] + d[0] * t1, a[1] + d[1] * t1]))
}

/// A comet's body along the circle of radius `r` round (`cx`, `cy`), from angle `a0` (its tail)
/// to `a1` (its head), widening to `width` at the head.
pub(crate) fn comet(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, width: f32) -> Vec<[f32; 2]> {
    let n = 40;
    let at = |k: usize, side: f32| {
        let u = k as f32 / n as f32;
        let a = a0 + (a1 - a0) * u;
        let half = width * 0.5 * u.powf(1.5) * side;
        [cx + a.cos() * (r + half), cy + a.sin() * (r + half)]
    };
    let mut pts: Vec<[f32; 2]> = (0..=n).map(|k| at(k, 1.0)).collect();
    pts.extend((0..=n).rev().map(|k| at(k, -1.0)));
    pts
}

/// A rounded rectangle `w` × `h` with corner radius `r`.
pub(crate) fn rounded(w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
    let mut pts = Vec::new();
    let corners = [(w - r, r, -0.5), (w - r, h - r, 0.0), (r, h - r, 0.5), (r, r, 1.0)];
    for (cx, cy, q) in corners {
        for k in 0..=4 {
            let a = (q + k as f32 * 0.125) * std::f32::consts::PI;
            pts.push([cx + a.cos() * r, cy + a.sin() * r]);
        }
    }
    pts
}

/// `pts` with every corner rounded off by up to `rad`, concave corners as well as convex.
pub(crate) fn round_poly(pts: &[[f32; 2]], rad: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (p, v, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let toward = |o: [f32; 2]| {
            let (dx, dy) = (o[0] - v[0], o[1] - v[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let t = (rad / len).min(0.45);
            [v[0] + dx * t, v[1] + dy * t]
        };
        let (a, b) = (toward(p), toward(q));
        for k in 0..=4 {
            let u = k as f32 / 4.0;
            let (m0, m1) = ((1.0 - u) * (1.0 - u), 2.0 * u * (1.0 - u));
            let m2 = u * u;
            out.push([a[0] * m0 + v[0] * m1 + b[0] * m2, a[1] * m0 + v[1] * m1 + b[1] * m2]);
        }
    }
    out
}

/// A plump five-point star, rounded at its tips and its inner corners.
pub(crate) fn star_soft(x: f32, y: f32, r: f32, turn: f32) -> Vec<[f32; 2]> {
    let pts: Vec<[f32; 2]> = (0..10)
        .map(|i| {
            let a = turn - std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.58 };
            [x + a.cos() * rr, y + a.sin() * rr]
        })
        .collect();
    round_poly(&pts, r * 0.22)
}

/// [`moon_c`] as a strip of small convex quads from horn to horn, each overlapping the next a
/// little so no seam shows. Every piece is convex, so it always fills as drawn.
pub(crate) fn moon_pieces(cx: f32, cy: f32, r: f32, turn: f32) -> Vec<Vec<[f32; 2]>> {
    let (ri, d) = (0.8 * r, 0.4 * r);
    let xo = (r * r - ri * ri + d * d) / (2.0 * d);
    let ao = (xo / r).clamp(-1.0, 1.0).acos();
    let ai = ((xo - d) / ri).clamp(-1.0, 1.0).acos();
    let (c, sn) = (turn.cos(), turn.sin());
    let at = |[x, y]: [f32; 2]| [cx + x * c - y * sn, cy + x * sn + y * c];
    let outer = |u: f32| {
        let a = ao + (std::f32::consts::TAU - 2.0 * ao) * u;
        at([a.cos() * r, a.sin() * r])
    };
    let inner = |u: f32| {
        let a = ai + (std::f32::consts::TAU - 2.0 * ai) * u;
        at([d + a.cos() * ri, a.sin() * ri])
    };
    let n = 24;
    let pad = 0.2 / n as f32;
    (0..n)
        .map(|k| {
            let (u0, u1) = (((k as f32) / n as f32 - pad).max(0.0), ((k + 1) as f32 / n as f32 + pad).min(1.0));
            vec![outer(u0), outer(u1), inner(u1), inner(u0)]
        })
        .collect()
}

/// `pts` scaled by `k` about (`x`, `y`).
pub(crate) fn grown(pts: &[[f32; 2]], x: f32, y: f32, k: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|p| [x + (p[0] - x) * k, y + (p[1] - y) * k]).collect()
}

/// A closed outline's stroke as convex pieces, one quad per segment, each run on past its ends
/// by half the width so the joints close, for clipping piece by piece.
pub(crate) fn rim_pieces(pts: &[[f32; 2]], width: f32) -> Vec<Vec<[f32; 2]>> {
    let n = pts.len();
    let hw = width / 2.0;
    (0..n)
        .filter_map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-3 {
                return None;
            }
            let (ux, uy) = (dx / len * hw, dy / len * hw);
            let (nx, ny) = (-uy, ux);
            let (a, b) = ([a[0] - ux, a[1] - uy], [b[0] + ux, b[1] + uy]);
            Some(vec![[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]])
        })
        .collect()
}
