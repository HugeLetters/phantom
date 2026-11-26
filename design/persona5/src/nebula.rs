//! The Jose's shop nebulae: the generated noise texture and the metaball clouds.

use rune::{El, Shape, el};

use crate::common::*;
use crate::noise::*;

thread_local! {
    /// Nebula textures by size and seed, made once.
    static NEB_TEX: std::cell::RefCell<std::collections::HashMap<(u32, u32), rune::Pixels>> = Default::default();
}

/// The nebulae as a texture: domain-warped fractal noise, so the clouds billow at every scale
/// like the photographs, in the sky's blues only, with dark dust lanes and bright filaments.
/// Made once, a little larger than the window, and drifted slowly. Two skies from different
/// seeds cross-fade very slowly (90 s each way), both overlapping throughout, so the nebula
/// seems to change shape without being seen to unless watched for. Dimmed to 55%.
pub(crate) fn nebula_texture(s: &S, w: f32, h: f32) -> El {
    let pad = 60.0;
    let (tw, th) = ((w + 2.0 * pad) as u32, (h + 2.0 * pad) as u32);
    let tex = |seed: u32| {
        NEB_TEX.with(|c| {
            c.borrow_mut()
                .entry((tw, th ^ seed))
                .or_insert_with(|| {
                    let p = rune::Pixels::new();
                    p.set(tw, th, nebula_rgba(tw, th, seed));
                    p
                })
                .clone()
        })
    };
    // A slow drift along the comets' sweep, back and forth within the margin.
    let (dx, dy) = ((s.t * 0.05).sin() * pad * 0.8, (s.t * 0.035).cos() * pad * 0.5);
    let one = |seed: u32| el().size(tw as f32, th as f32).pixels(&tex(seed)).pin(Some(-pad + dx), Some(-pad + dy), None, None);
    let dim = 0.55;
    let mix = 0.5 - 0.5 * (std::f32::consts::TAU * s.t / 180.0).cos();
    el().size(w, h).child(one(0).opacity(dim * (1.0 - mix))).child(one(977).opacity(dim * mix))
}

pub(crate) fn nebula_rgba(tw: u32, th: u32, seed: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    // Covering less of the sky, and calm: fewer, larger octaves.
    let cover = -0.2;
    let (oct, woct, foct, scale) = (4, 3, 3, 0.75);
    let (d0, d1, l0, l1, f0, f1) = (0.32, 0.78, 0.62, 0.9, 0.72, 0.95);
    // The blues of the clouds, from the sky's deep navy up to a pale cyan core.
    let ramp = [
        (0.0, [0x0c, 0x34, 0x74]),
        (0.35, [0x2f, 0x62, 0xb0]),
        (0.65, [0x4a, 0x7f, 0xe0]),
        (0.85, [0x7f, 0xb4, 0xff]),
        (1.0, [0xcf, 0xe6, 0xff]),
    ];
    let color = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let k = ramp.iter().position(|r| r.0 >= v).unwrap_or(ramp.len() - 1).max(1);
        let (a, b) = (ramp[k - 1], ramp[k]);
        let u = ((v - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
        [0, 1, 2].map(|i| a.1[i] as f32 + (b.1[i] as f32 - a.1[i] as f32) * u)
    };
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    for j in 0..th {
        for i in 0..tw {
            let (x, y) = (i as f32 / tw as f32, j as f32 / th as f32);
            // Where the nebulae lie: a broad band from the upper right towards the middle,
            // broken up by large-scale noise so the gaps are uneven and the left stays clear.
            let band = 1.0 - ((y - (0.15 + (1.0 - x) * 0.55)).abs() * 2.2);
            let broad = fbm(x * 2.2, y * 2.2, 3, 11 + seed);
            let mask = smooth(0.35 - cover, 0.75 - cover, band * 0.6 + broad * 0.7 + x * 0.25 - 0.2);
            if mask <= 0.001 {
                continue;
            }
            // Domain warp: the noise sampled through itself, for billows and swirls.
            let (sx, sy) = (x * 5.0 * scale, y * 3.6 * scale);
            let qx = fbm(sx, sy, woct, 21 + seed);
            let qy = fbm(sx + 5.2, sy + 1.3, woct, 22 + seed);
            let d = fbm(sx + 3.0 * qx, sy + 3.0 * qy, oct, 23 + seed);
            let mut v = smooth(d0, d1, d) * mask;
            // Dark dust: thin winding channels cutting through the glow.
            let r = ridged(sx * 0.9 + 2.0 * qy, sy * 0.9 + 2.0 * qx, 3, 31 + seed);
            v *= 1.0 - 0.85 * smooth(l0, l1, r);
            // Bright filaments and knots where the warped noise folds.
            let f = ridged(sx * 1.6 + 1.5 * qx, sy * 1.6 + 1.5 * qy, foct, 41 + seed);
            let glow = (v + 0.45 * smooth(f0, f1, f) * mask * v.sqrt()).min(1.0);
            let c = color(glow);
            let a = (v * 1.1).min(0.92);
            let o = ((j * tw + i) * 4) as usize;
            out[o] = c[0] as u8;
            out[o + 1] = c[1] as u8;
            out[o + 2] = c[2] as u8;
            out[o + 3] = (a * 255.0) as u8;
        }
    }
    out
}

/// Nebulae as metaballs: each blob a field falling off with distance, the cloud everything above
/// a threshold of their sum, so blobs that drift close grow a neck and merge like droplets. Each
/// cloud, of its own size, merges only with its two satellites circling it; a faint veil where
/// the clouds' fields meet lets small ones lean towards their neighbours instead of sitting
/// alone. Two of the seven clouds are left out, so less of the sky is light.
pub(crate) fn merged_nebulae(s: &S, mut sky: Shape, clouds: &[(f32, f32, f32, &str)], m: f32, w: f32, h: f32, neb: f32) -> Shape {
    let t = s.t;
    let extra = [(0.25, 0.3, 130.0, "#2f62b0"), (0.68, 0.82, 120.0, "#3a5fc0"), (0.97, 0.3, 110.0, "#2a74b8")];
    let base = 0.748;
    let strength = 1.12;
    let veil = 0.3;
    let (sat, near, iso_k) = (1.45, 0.75, 0.75);
    let mut everyone: Vec<(f32, f32, f32)> = Vec::new();
    let mut bodies: Vec<(Vec<(f32, f32, f32)>, &str)> = Vec::new();
    let all: Vec<(f32, f32, f32, &str)> = clouds.iter().copied().chain(extra.iter().copied()).collect();
    for (k, (fx, fy, r, c)) in all.iter().enumerate() {
        if matches!(k, 1 | 3) {
            continue;
        }
        let (x, y) = (m + fx * w + (t * 0.05 + k as f32).sin() * 30.0, m + fy * h + (t * 0.04 + k as f32 * 2.0).cos() * 20.0);
        let size = 0.6 + 0.8 * noise(k as u32 * 13 + 5);
        let base = base * size;
        let mut own = vec![(x, y, r * 0.6 * base)];
        for j in 0..2 {
            let a = t * (0.15 + 0.05 * j as f32) + k as f32 * 1.9 + j as f32 * 3.1;
            let d = r * (1.0 + 0.3 * (t * 0.3 + j as f32).sin()) * near;
            own.push((x + a.cos() * d * 1.2, y + a.sin() * d * 0.7, r * (0.22 + 0.06 * j as f32) * base * sat));
        }
        everyone.extend(own.iter().copied());
        bodies.push((own, c));
    }
    // The veil: all the clouds' fields summed, at a low threshold, drawn faintly under the
    // bodies, so it shows only between clouds that are near each other.
    for piece in metaball_pieces(&everyone, w + 2.0 * m, h + 2.0 * m, 12.0, 1.3) {
        sky = sky.fill(piece, hex("#2a5aae").alpha((neb * veil).min(1.0)));
    }
    for (own, c) in bodies {
        for (iso, col, a) in [(1.3, c, 0.8), (2.6, "#7fb4ff", 0.35)] {
            for piece in metaball_pieces(&own, w + 2.0 * m, h + 2.0 * m, 12.0, iso * iso_k) {
                sky = sky.fill(piece, hex(col).alpha((neb * 1.25 * a * strength).min(1.0)));
            }
        }
    }
    sky
}

/// The region where Σ r²/d² ≥ `iso`, by marching squares on a `step` grid over `w` × `h`. Every
/// piece is convex (a saddle cell gives two triangles), so each fills exactly as drawn.
pub(crate) fn metaball_pieces(balls: &[(f32, f32, f32)], w: f32, h: f32, step: f32, iso: f32) -> Vec<Vec<[f32; 2]>> {
    let field = |x: f32, y: f32| balls.iter().map(|&(bx, by, r)| r * r / ((x - bx).powi(2) + (y - by).powi(2) + 1.0)).sum::<f32>();
    let (nx, ny) = ((w / step) as usize + 1, (h / step) as usize + 1);
    let v: Vec<f32> = (0..=ny).flat_map(|j| (0..=nx).map(move |i| (i, j))).map(|(i, j)| field(i as f32 * step, j as f32 * step)).collect();
    let at = |i: usize, j: usize| v[j * (nx + 1) + i];
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let (x0, y0) = (i as f32 * step, j as f32 * step);
            let p = [[x0, y0], [x0 + step, y0], [x0 + step, y0 + step], [x0, y0 + step]];
            let f = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            let inside: Vec<bool> = f.iter().map(|&q| q >= iso).collect();
            let count = inside.iter().filter(|&&b| b).count();
            if count == 0 {
                continue;
            }
            if count == 4 {
                out.push(p.to_vec());
                continue;
            }
            let cut = |a: usize, b: usize| {
                let u = ((iso - f[a]) / (f[b] - f[a])).clamp(0.0, 1.0);
                [p[a][0] + (p[b][0] - p[a][0]) * u, p[a][1] + (p[b][1] - p[a][1]) * u]
            };
            if count == 2 && inside[0] == inside[2] {
                // A saddle: a triangle at each inside corner.
                for k in 0..4 {
                    if inside[k] {
                        out.push(vec![cut(k, (k + 3) % 4), p[k], cut(k, (k + 1) % 4)]);
                    }
                }
                continue;
            }
            let mut poly = Vec::new();
            for k in 0..4 {
                let n = (k + 1) % 4;
                if inside[k] {
                    poly.push(p[k]);
                }
                if inside[k] != inside[n] {
                    poly.push(cut(k, n));
                }
            }
            out.push(poly);
        }
    }
    out
}
