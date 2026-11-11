//! What the three directions share: the palette, the demo torrents, the sketch's state, and the
//! drawing helpers (slanted and jagged polygons, halftone, the masked ghost, ransom-note letters).

use std::f32::consts::PI;

use rune::{Color, El, Family, LineCap, Path, Phase, Shape, TextStyle, canvas, el, row, text};

pub fn hex(s: &str) -> Color {
    rune::color::hex(s).expect("a hex colour")
}

pub fn red() -> Color {
    hex("#e60012")
}
pub fn deep() -> Color {
    hex("#a8000c")
}
pub fn black() -> Color {
    hex("#0b0b0b")
}
pub fn paper() -> Color {
    hex("#f4f2ec")
}

// ------------------------------------------------------------------ fonts (macOS system fonts)

/// Condensed caps for labels: Avenir Next Condensed ships with macOS, so a theme can name it
/// without bundling it.
pub fn caps(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Avenir Next Condensed")).tracking(0.4)
}
pub fn sans(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Avenir Next"))
}
pub fn figures(size: f32) -> TextStyle {
    TextStyle::new(size, 600.0).family(Family::Named("Avenir Next Condensed")).features("'tnum' 1")
}

// ------------------------------------------------------------------ state

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Card,
    Accent,
    Attack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Idle,
    Hover,
    Press,
    Disabled,
}

pub struct S {
    pub dir: Dir,
    /// The angled-window spike: the window is a slanted four-sided shape, its corners clear.
    pub angled: bool,
    /// The window's size in points, as last laid out (the angled frame follows it).
    pub win: [f32; 2],
    /// Presses the window has seen, and where the last one was: tells whether a click on a
    /// clear corner reached the window or the app behind it.
    pub presses: u32,
    pub last_press: [f32; 2],
    /// The component sheet instead of the window.
    pub sheet: bool,
    pub hovered: Option<String>,
    pub pressed: Option<String>,
    pub selected: Option<usize>,
    /// Seconds since the sketch started, for the time-driven motion.
    pub t: f32,
    /// When the selection last changed, for the selection's snap-in.
    pub selected_at: f32,
    /// When the pointer last entered something, for the active shape's burst.
    pub hovered_at: f32,
    /// How many times anything was hovered, pressed or selected: each activation seeds its own
    /// random tilt and shape, so a button never lands the same way twice.
    pub activations: u32,
    /// How the toolbar buttons letter their labels (see `attack::label`).
    pub lettering: u8,
    /// The lettering comparison sheet instead of the window.
    pub letters_sheet: bool,
    /// How an active button shows it (see `attack::HOVERS`).
    pub hover_style: u8,
    /// The hover comparison sheet instead of the window.
    pub hovers_sheet: bool,
    /// How rows step right (see `attack::STEPS`), and the sheet comparing them.
    pub step_style: u8,
    pub steps_sheet: bool,
    /// How speeds stand out (see `attack::SPEEDS`), and the sheet comparing them.
    pub speed_style: u8,
    pub speeds_sheet: bool,
    /// A long list (16 torrents) instead of the six, to see the row steps over many rows.
    pub many: bool,
    /// How progress bars sit on the sheet (see `attack::TRACKS`), and the sheet comparing them.
    pub track_style: u8,
    pub tracks_sheet: bool,
}

impl S {
    pub fn new() -> S {
        S {
            dir: Dir::Card,
            sheet: false,
            hovered: None,
            pressed: None,
            selected: Some(0),
            t: 0.0,
            selected_at: -10.0,
            hovered_at: -10.0,
            activations: 0,
            // P2 + one box, as the user chose: at least one lowercase letter, at most half.
            lettering: 21,
            letters_sheet: false,
            // G own colours (tight, morphing, keeps its colours, red on press); D black press (28)
            // is kept beside it, as a setting or for the Jose palette.
            hover_style: 25,
            hovers_sheet: false,
            // B2 (random per row, never close to the row above) at 7 pt, as the user chose.
            step_style: 10,
            steps_sheet: false,
            // F: black figures on a living cyan / pink slab, as the user chose.
            speed_style: 6,
            speeds_sheet: false,
            many: false,
            angled: false,
            win: [0.0, 0.0],
            presses: 0,
            last_press: [0.0, 0.0],
            // B, level top and bottom with leaning sides and front edge, its patterns straight; a
            // red shadow on error and none while seeding (the user's picks).
            track_style: 19,
            tracks_sheet: false,
        }
    }

    /// What an interactive piece keyed `key` looks like now.
    pub fn look(&self, key: &str, disabled: bool) -> Look {
        if disabled {
            Look::Disabled
        } else if self.pressed.as_deref() == Some(key) {
            Look::Press
        } else if self.hovered.as_deref() == Some(key) {
            Look::Hover
        } else {
            Look::Idle
        }
    }
}

/// Tracks hover and press on `e` under `key`; a release inside runs `on_up`.
pub fn interactive(e: El, key: String, on_up: impl Fn(&mut S) + 'static) -> El {
    let (k1, k2) = (key.clone(), key.clone());
    e.key(key)
        .on_hover_change(move |s: &mut S, inside, _| {
            if inside {
                s.hovered = Some(k1.clone());
                s.hovered_at = s.t;
                s.activations += 1;
            } else if s.hovered.as_deref() == Some(&k1) {
                s.hovered = None;
            }
        })
        .on_drag(move |s: &mut S, d, _| match d.phase {
            Phase::Down => {
                s.pressed = Some(k2.clone());
                s.activations += 1;
            }
            Phase::Move => {}
            Phase::Up => {
                s.pressed = None;
                let inside = d.x >= 0.0 && d.y >= 0.0 && d.x <= d.w && d.y <= d.h;
                if inside && !d.cancelled() {
                    on_up(s);
                }
            }
        })
}

// ------------------------------------------------------------------ demo data

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Downloading,
    Seeding,
    Checking,
    Paused,
    Fetching,
    Error,
}

#[derive(Clone, Copy)]
pub struct Torrent {
    pub name: &'static str,
    pub progress: f32,
    pub kind: Kind,
    pub status: &'static str,
    pub size: &'static str,
    pub down: &'static str,
    pub up: &'static str,
    pub peers: &'static str,
    pub eta: &'static str,
}

pub const TORRENTS: &[Torrent] = &[
    Torrent {
        name: "debian-13.7.0-amd64-netinst.iso",
        progress: 0.545,
        kind: Kind::Downloading,
        status: "Downloading",
        size: "756 MiB",
        down: "68 MiB/s",
        up: "–",
        peers: "32",
        eta: "5s",
    },
    Torrent {
        name: "ubuntu-24.04.3-desktop-amd64.iso",
        progress: 1.0,
        kind: Kind::Seeding,
        status: "Seeding",
        size: "6.0 GiB",
        down: "–",
        up: "1.5 MiB/s",
        peers: "7",
        eta: "–",
    },
    Torrent {
        name: "Big Buck Bunny (2008)",
        progress: 0.61,
        kind: Kind::Checking,
        status: "Checking files",
        size: "5.9 GiB",
        down: "–",
        up: "–",
        peers: "18",
        eta: "–",
    },
    Torrent {
        name: "archlinux-2026.11.01-x86_64.iso",
        progress: 0.25,
        kind: Kind::Paused,
        status: "Paused",
        size: "1.2 GiB",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
    Torrent {
        name: "Sintel (4K)",
        progress: 0.0,
        kind: Kind::Fetching,
        status: "Fetching info",
        size: "–",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
    Torrent {
        name: "Tears of Steel (2012)",
        progress: 0.06,
        kind: Kind::Error,
        status: "Error",
        size: "3.3 GiB",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
];

/// Names for a long list, to see the row steps over many rows.
pub const MANY: [&str; 16] = [
    "debian-13.7.0-amd64-netinst.iso",
    "ubuntu-24.04.3-desktop-amd64.iso",
    "Big Buck Bunny (2008)",
    "archlinux-2026.11.01-x86_64.iso",
    "Sintel (4K)",
    "Tears of Steel (2012)",
    "fedora-workstation-43.iso",
    "Elephants Dream (2006)",
    "linuxmint-22.2-cinnamon.iso",
    "Cosmos Laundromat (2015)",
    "kali-linux-2026.3-installer.iso",
    "Spring (2019)",
    "freebsd-15.0-amd64-dvd1.iso",
    "Agent 327 (2017)",
    "tails-amd64-7.2.img",
    "Charge (2022)",
];

/// How many torrents the list shows.
pub fn count(s: &S) -> usize {
    if s.many { MANY.len() } else { TORRENTS.len() }
}

/// The torrent at `i`: with a long list, the six repeat under other names.
pub fn torrent(s: &S, i: usize) -> Torrent {
    let t = TORRENTS[i % TORRENTS.len()];
    if s.many { Torrent { name: MANY[i], ..t } } else { t }
}

pub const ERRORS: &[(&str, &str)] = &[
    ("Tears of Steel (2012)", "No space left on device (os error 28)"),
    ("Sintel (4K)", "Drive MyDisk isn't connected"),
    ("magnet:?xt=urn:btih:00ff…", "Not a magnet link or .torrent URL"),
];

/// Column widths of the torrent list (Name takes the rest).
pub const COLS: [(&str, f32); 7] =
    [("Progress", 150.0), ("Status", 116.0), ("Size", 70.0), ("Down", 78.0), ("Up", 78.0), ("Peers", 48.0), ("ETA", 44.0)];

// ------------------------------------------------------------------ geometry

/// A tiny deterministic noise, so jagged edges stay put between frames.
pub fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    (x & 0xffff) as f32 / 65535.0
}

/// A parallelogram `w` × `h` whose top edge leans `skew` points right.
pub fn slant(x: f32, y: f32, w: f32, h: f32, skew: f32) -> Vec<[f32; 2]> {
    vec![[x + skew, y], [x + w, y], [x + w - skew, y + h], [x, y + h]]
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

pub fn cyan() -> Color {
    hex("#3fe0f0")
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

// ------------------------------------------------------------------ the masked ghost

/// Phantom's masked ghost (design/icons, pick `phantom-thief`), in a `size` box: white body,
/// black domino mask with a white rim, white eyes. `shadow` draws an offset copy behind it.
pub fn ghost(size: f32, shadow: Option<Color>) -> El {
    let k = size / 600.0;
    let (ox, oy) = (212.0, 212.0);
    let p = |x: f32, y: f32| {
        let t = turned(&[[x, y]], -9f32.to_radians(), 512.0, 512.0)[0];
        [(t[0] - ox) * k, (t[1] - oy) * k]
    };
    let body = Path::new().move_to(p(292.0, 500.0)).cubic_to(p(292.0, 330.0), p(392.0, 226.0), p(516.0, 226.0)).cubic_to(
        p(648.0, 226.0),
        p(736.0, 330.0),
        p(736.0, 490.0),
    );
    let hem = [
        (742.0, 640.0),
        (812.0, 800.0),
        (700.0, 738.0),
        (664.0, 822.0),
        (594.0, 730.0),
        (532.0, 812.0),
        (480.0, 722.0),
        (408.0, 786.0),
        (384.0, 700.0),
        (318.0, 742.0),
        (300.0, 620.0),
    ];
    let body = hem.iter().fold(body, |b, &(x, y)| b.line_to(p(x, y))).close();
    let pts = |v: &[(f32, f32)]| v.iter().map(|&(x, y)| p(x, y)).collect::<Vec<_>>();
    let mask = pts(&[
        (514.0, 420.0),
        (470.0, 398.0),
        (214.0, 344.0),
        (312.0, 473.0),
        (435.0, 507.0),
        (514.0, 471.0),
        (593.0, 507.0),
        (716.0, 473.0),
        (814.0, 344.0),
        (558.0, 398.0),
    ]);
    let mut s = Shape::new();
    if let Some(c) = shadow {
        let d = 26.0 * k;
        let moved_body = {
            let q = |x: f32, y: f32| {
                let v = p(x, y);
                [v[0] + d, v[1] + d]
            };
            let b = Path::new().move_to(q(292.0, 500.0)).cubic_to(q(292.0, 330.0), q(392.0, 226.0), q(516.0, 226.0)).cubic_to(
                q(648.0, 226.0),
                q(736.0, 330.0),
                q(736.0, 490.0),
            );
            hem.iter().fold(b, |b, &(x, y)| b.line_to(q(x, y))).close()
        };
        s = s.fill_path(&moved_body, c);
    }
    s = s.fill_path(&body, Color::WHITE);
    s = outline(s, &mask, 11.0 * k, Color::WHITE).fill(mask, Color::BLACK);
    s = s.fill(pts(&[(318.0, 418.0), (471.0, 441.0), (422.0, 481.0)]), Color::WHITE);
    s = s.fill(pts(&[(710.0, 418.0), (557.0, 441.0), (606.0, 481.0)]), Color::WHITE);
    canvas(s).size(size, size)
}

// ------------------------------------------------------------------ ransom-note lettering

/// The faces a ransom-note heading cuts its letters from: all ship with macOS.
const FACES: [(&str, f32); 6] = [
    ("Futura", 800.0),
    ("Didot", 700.0),
    ("American Typewriter", 600.0),
    ("Avenir Next Condensed", 900.0),
    ("Georgia", 700.0),
    ("Rockwell", 700.0),
];

/// [`ransom_mix`]'s faces: a few, swapped letter by letter, as the Persona 5 menus are made
/// (Futura, a Times, a Clarendon, a Helvetica, an Avant Garde). All macOS system fonts.
const MIX_FACES: [(&str, f32); 5] =
    [("Futura", 800.0), ("Times New Roman", 700.0), ("Superclarendon", 700.0), ("Helvetica Neue", 800.0), ("Avenir Next", 800.0)];

/// A heading of letters cut from different prints: each letter on its own paper scrap, turned
/// and nudged a little. `palette` is the (paper, ink) pairs the scraps cycle through.
pub fn ransom(word: &str, size: f32, seed: u32, palette: &[(Color, Color)]) -> El {
    ransom_mix(word, size, seed, palette, &[], 1.0)
}

/// Ransom letters as the Persona 5 pause menu sets them: every letter in its own face, size and
/// tilt, but only some (`scraps` of them, 0..1) on a paper scrap; the rest are bare, in one of
/// the `bare` inks. With bare letters, the first letter is set larger, as a capital.
pub fn ransom_mix(word: &str, size: f32, seed: u32, palette: &[(Color, Color)], bare: &[Color], scraps: f32) -> El {
    let mut r = row().gap(1.0).items_center();
    for (i, ch) in word.chars().enumerate() {
        let n = seed.wrapping_add(i as u32 * 13);
        if ch == ' ' {
            r = r.child(el().w(size * 0.35));
            continue;
        }
        let lead = !bare.is_empty() && i == 0;
        let faces: &[(&str, f32)] = if bare.is_empty() { &FACES } else { &MIX_FACES };
        let (face, weight) = faces[(noise(n) * faces.len() as f32) as usize % faces.len()];
        let (bg, fg) = palette[(noise(n + 5) * palette.len() as f32) as usize % palette.len()];
        // Mixed letters are set calmer than a ransom heading, so a label stays readable.
        let (spread, turn, drop) = if bare.is_empty() { (0.3, 0.28, 0.18) } else { (0.14, 0.1, 0.06) };
        let scale = if lead { 1.12 } else { 1.0 - spread / 2.0 + noise(n + 1) * spread };
        let lower = !lead && noise(n + 9) > if bare.is_empty() { 0.7 } else { 0.82 } && ch.is_alphabetic();
        let glyph: String = if lower { ch.to_lowercase().collect() } else { ch.to_uppercase().collect() };
        let fs = size * scale;
        if !bare.is_empty() && noise(n + 7) >= scraps {
            let ink = bare[(noise(n + 11) * bare.len() as f32) as usize % bare.len()];
            r = r.child(
                text(glyph)
                    .font(TextStyle::new(fs, weight).family(Family::Named(face)))
                    .color(ink)
                    .rotate((noise(n + 2) - 0.5) * turn)
                    .offset(0.0, (noise(n + 3) - 0.5) * size * drop),
            );
            continue;
        }
        let (w, h) = (fs * 0.78 + 6.0, fs * 1.18);
        let scrap = jagged(0.0, 0.0, w, h, 1.6, 7.0, n);
        let tile = el()
            .size(w, h)
            .center()
            .child(canvas(Shape::new().fill(moved(&scrap, 1.5, 2.0), Color::BLACK.alpha(0.55)).fill(scrap, bg)).size(w, h).pin(
                Some(0.0),
                Some(0.0),
                None,
                None,
            ))
            .child(text(glyph).font(TextStyle::new(fs, weight).family(Family::Named(face))).color(fg))
            .rotate((noise(n + 2) - 0.5) * turn)
            .offset(0.0, (noise(n + 3) - 0.5) * size * drop);
        r = r.child(tile);
    }
    r
}

/// A box of `w` × `h` with `shape` drawn under its children, the shape in the box's points.
pub fn layer(shape: Shape, w: f32, h: f32) -> El {
    canvas(shape).size(w, h).pin(Some(0.0), Some(0.0), None, None).inert()
}

/// The ease-out-back of a snap: overshoots a touch, then settles, over `dur` seconds.
pub fn snap(t: f32, dur: f32) -> f32 {
    let x = (t / dur).clamp(0.0, 1.0);
    let c = 1.9;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
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
