//! Helpers that draw onto a [`Shape`] or wrap one in a layer: layers, outlines, halftone dots and hatching.

use rune::{Color, El, LineCap, Shape, canvas};

use crate::shapes::*;

/// `pts` with an outline `width` thick around it, in `color`.
pub fn outline(shape: Shape, pts: &[[f32; 2]], width: f32, color: Color) -> Shape {
    shape.fill(inflate(pts, width), color)
}

/// Halftone dots on a grid turned 45°, each of radius `r(x, y)` (0 skips it), in the box.
pub fn halftone(shape: Shape, w: f32, h: f32, pitch: f32, color: Color, r: impl Fn(f32, f32) -> f32) -> Shape {
    halftone_in(shape, w, h, pitch, color, r, &|p| p)
}

/// [`halftone`], each dot's centre passed through `warp`.
pub fn halftone_in(
    mut shape: Shape,
    w: f32,
    h: f32,
    pitch: f32,
    color: Color,
    r: impl Fn(f32, f32) -> f32,
    warp: &dyn Fn([f32; 2]) -> [f32; 2],
) -> Shape {
    let rows = (h / (pitch * 0.5)).ceil() as i32 + 1;
    let cols = (w / pitch).ceil() as i32 + 1;
    for j in 0..rows {
        let y = j as f32 * pitch * 0.5;
        let shift = if j % 2 == 0 { 0.0 } else { pitch * 0.5 };
        for i in 0..cols {
            let x = i as f32 * pitch + shift;
            let rad = r(x, y).min(pitch * 0.72);
            if rad < 0.35 {
                continue;
            }
            let [cx, cy] = warp([x, y]);
            shape = shape.fill(circle(cx, cy, rad), color);
        }
    }
    shape
}

/// Screentone: parallel lines at 45°, `pitch` apart, clipped to the box; `phase` (points)
/// slides them along, for moving stripes.
pub fn hatch(shape: Shape, w: f32, h: f32, pitch: f32, width: f32, color: Color, phase: f32) -> Shape {
    hatch_in(shape, w, h, pitch, width, color, phase, &|p| p)
}

/// [`hatch`], each line's ends passed through `warp`.
#[allow(clippy::too_many_arguments)]
pub fn hatch_in(
    mut shape: Shape,
    w: f32,
    h: f32,
    pitch: f32,
    width: f32,
    color: Color,
    phase: f32,
    warp: &dyn Fn([f32; 2]) -> [f32; 2],
) -> Shape {
    let mut x = -h - pitch + phase.rem_euclid(pitch);
    while x < w {
        // The line runs through (x + u, h - u) for u in 0..h; keep the part inside 0..w.
        let (u0, u1) = ((-x).max(0.0), h.min(w - x));
        if u1 > u0 {
            shape = shape.line(warp([x + u0, h - u0]), warp([x + u1, h - u1]), width, LineCap::Butt, color);
        }
        x += pitch;
    }
    shape
}

/// A box of `w` × `h` with `shape` drawn under its children, the shape in the box's points.
pub fn layer(shape: Shape, w: f32, h: f32) -> El {
    canvas(shape).size(w, h).pin(Some(0.0), Some(0.0), None, None).inert()
}
