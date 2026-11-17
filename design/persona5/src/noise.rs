//! Deterministic noise (a seeded hash, value noise, fractal and ridged noise) and the seeds drawn from keys and activations.

use crate::common::*;

/// A tiny deterministic noise, so jagged edges stay put between frames.
pub fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    (x & 0xffff) as f32 / 65535.0
}

/// A tilt for the activation `seed`: between `min` and `max` radians, either way.
pub fn tilt(seed: u32, min: f32, max: f32) -> f32 {
    let a = min + (max - min) * noise(seed.wrapping_mul(7).wrapping_add(3));
    if noise(seed.wrapping_add(17)) < 0.5 { -a } else { a }
}

/// A key's own number, to seed its shapes.
pub fn key_seed(key: &str) -> u32 {
    key.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619))
}

/// The seed of `key`'s latest activation (hover or press). Another element's activation doesn't
/// change it, so a shape fading out as the pointer moves on keeps its shape.
pub(crate) fn active_seed(s: &S, key: &str) -> u32 {
    key_seed(key) ^ s.activated.get(key).copied().unwrap_or(s.activations).wrapping_mul(2654435761)
}

/// Value noise at `(x, y)`: random values on a unit grid, smoothly interpolated. 0–1.
pub(crate) fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |i: i32, j: i32| noise((i as u32).wrapping_mul(73856093) ^ (j as u32).wrapping_mul(19349663) ^ seed);
    let (i, j) = (xi as i32, yi as i32);
    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

/// Fractal noise: `oct` octaves of value noise, each twice as fine and half as strong. 0–1.
pub(crate) fn fbm(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..oct {
        sum += amp * vnoise(x * f, y * f, seed.wrapping_add(o * 1013));
        norm += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    sum / norm
}

/// Ridged noise: bright where the noise crosses its middle, so it draws thin filaments. 0–1.
pub(crate) fn ridged(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..oct {
        let n = 1.0 - (vnoise(x * f, y * f, seed.wrapping_add(o * 7919)) * 2.0 - 1.0).abs();
        sum += amp * n * n;
        norm += amp;
        amp *= 0.5;
        f *= 2.1;
    }
    sum / norm
}
