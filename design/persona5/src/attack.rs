//! Direction C, "All-Out": maximal and animated. A red ground with a slowly turning starburst
//! and black halftone, the list on a tilted sheet of white paper with a fat black outline, black
//! cut-out slabs for buttons and the selection. Everything moves: rows slam in on appear,
//! buttons jump off their shadow on hover and slam into it on press, progress bars animate by
//! status, the ghost bobs.
//!
//! Active things (a hovered or pressed button, the selected row) throw out a blade, a spiky
//! non-rectangular shape, at a random tilt: every activation lands differently.
//!
//! Lettering: ransom letters for the title, buttons, column headings and the connection state;
//! a plain face for the torrents themselves.

use rune::{Color, El, Family, LineCap, Shape, Spring, TextStyle, col, el, row, text};

use crate::common::*;
use crate::{BAR_H, Parts, ROW_H, button_w, cells};

pub static PARTS: Parts = Parts {
    background,
    brand: Some(brand),
    field,
    button,
    panel,
    header,
    row: torrent_row,
    progress,
    indeterminate: true,
    list_font: Some(list_font),
    cell: Some(cell),
    status,
    errors,
    caption: Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
};

/// Torrent names and statuses: Optima, the macOS cousin of the Optima Nova Black that Persona 5
/// Royal sets its English lists in.
fn list_font(size: f32, weight: f32) -> TextStyle {
    if jose4() {
        return TextStyle::new(size * 0.95, 600.0).family(Family::Named(jose_family()));
    }
    TextStyle::new(size, weight.max(800.0)).family(Family::Named("Optima"))
}

fn kick() -> Spring {
    Spring::new(0.28, 0.45)
}

/// A's jump: short and hard.
fn jump() -> Spring {
    Spring::new(0.18, 0.35)
}

/// The seed of the current activation of `key`.
fn active_seed(s: &S, key: &str) -> u32 {
    key_seed(key) ^ s.activations.wrapping_mul(2654435761)
}

fn background(s: &S, w: f32, h: f32) -> El {
    if palette() >= 3 {
        return night_sky(s, w, h);
    }
    let mut sh = Shape::new().fill(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], red());
    // Rays from behind the title, turning slowly.
    let (cx, cy) = (w - 110.0, 18.0);
    let n = 18;
    for i in 0..n {
        let a0 = s.t * 0.12 + i as f32 * std::f32::consts::TAU / n as f32;
        let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.5;
        let r = 1400.0;
        sh = sh.fill(vec![[cx, cy], [cx + a0.cos() * r, cy + a0.sin() * r], [cx + a1.cos() * r, cy + a1.sin() * r]], deep());
    }
    // Black halftone rising from the bottom-left.
    sh = halftone(sh, w, h, 10.0, black(), |x, y| {
        let d = (x * 0.6).hypot(h - y);
        (5.6 - d / 55.0).max(0.0)
    });
    layer(sh, w, h)
}

/// The title alone: the ghost lives by the connection state.
fn brand(_: &S) -> El {
    ransom_mix("PHANTOM", 19.0, 5, &[(paper(), black()), (black(), paper()), (paper(), red()), (black(), red())], &[paper(), black()], 0.7)
        .mt(2.0)
}

fn field(_: &S, w: f32, hint: &str) -> El {
    let h = BAR_H - 6.0;
    let slab = if tape() { tape_strip(w - 8.0, h, 41) } else { cut(w - 8.0, h, 7.0, 41) };
    let s = if tape() {
        // Jose's tape: no black edge, a soft shadow, the paper a touch see-through.
        Shape::new().fill(moved(&slab, 3.0, 4.0), black().alpha(0.45)).fill(slab, paper().alpha(0.93))
    } else {
        Shape::new().fill(moved(&slab, 6.0, 5.0), black()).fill(inflate(&slab, 2.5), black()).fill(slab, paper())
    };
    el().size(w, BAR_H).child(layer(s, w, BAR_H)).rotate(-0.006).child(
        row().pin(Some(12.0), Some(0.0), Some(12.0), Some(6.0)).items_center().child(
            text(hint.to_owned()).font(sans(12.5, 600.0)).color(if jose3() { hex("#2b3f8c").alpha(0.8) } else { hex("#555") }).truncate(),
        ),
    )
}

fn button(s: &S, key: &str, label: &str, primary: bool, look: Look) -> El {
    button_lettered(s, key, label, primary, look, s.lettering)
}

fn button_lettered(s: &S, key: &str, label: &str, primary: bool, look: Look, lettering: u8) -> El {
    button_styled(s, key, label, primary, look, lettering, s.hover_style, s.t - s.hovered_at)
}

/// The ways a button can show it's active (hovered or pressed), for the comparison sheet:
/// (name, what it tries). The idle button is the same in all of them.
/// Round 1 (styles 0–6: before, A–F) is in `out/v/hovers-options.png`, round 2 (B, D, F and the
/// mixes G, H, J, K) in `out/v/e5f9fd0-hovers.png`; round 3 tweaks B, G and D, each beside its
/// round-2 self; round 4 (G tight morphing, D keeping its colours) sets the user's two picks from
/// round 3 beside their tweaks: (style, name, what it tries).
pub const HOVERS: [(u8, &str, &str); 2] =
    [(25, "G  Own colours", "the window's default"), (28, "D  Black press", "kept: a setting, or the Jose palette's")];

/// `since` is how long the pointer has been over the button, for the hover's entrance.
#[allow(clippy::too_many_arguments)]
fn button_styled(s: &S, key: &str, label: &str, primary: bool, look: Look, lettering: u8, style: u8, since: f32) -> El {
    if jose4() {
        return signboard(s, key, label, primary, look, since);
    }
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let seed = active_seed(s, key);
    let slab = cut(w, h, 14.0, key_seed(key));
    let active = matches!(look, Look::Hover | Look::Press);
    let press = look == Look::Press;
    let (mut fill, mut ink, mut edge, flash) = match (look, primary) {
        (Look::Disabled, _) => (deep(), if palette() == 0 { hex("#d2505a") } else { hex("#6f8ce8") }, deep(), None),
        (Look::Press, _) => (red(), paper(), black(), Some(black())),
        (Look::Hover, true) => (black(), paper(), paper(), Some(paper())),
        (Look::Hover, false) => (paper(), black(), black(), Some(black())),
        (Look::Idle, true) => (paper(), red(), black(), None),
        (Look::Idle, false) => (black(), paper(), paper(), None),
    };
    let grow = snap(since, 0.2).max(if press { 1.0 } else { 0.0 });
    let scaled = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> {
        pts.into_iter().map(|p| [w / 2.0 + (p[0] - w / 2.0) * grow, h / 2.0 + (p[1] - h / 2.0) * grow]).collect()
    };
    // The face's shape, what's drawn behind it, and whether the black shadow shows.
    let mut face_pts = slab.clone();
    let mut back = Shape::new();
    let mut shadow = look != Look::Disabled;
    let mut face_sh: Option<Shape> = None;
    let (mut scale, mut turn) = match look {
        Look::Hover => (1.08, tilt(seed, 0.04, 0.11)),
        Look::Press => (0.92, tilt(seed, 0.02, 0.05)),
        _ => (1.0, 0.0),
    };
    // The copies' entrance: they slide out from behind the face over 0.2 s (1 when pressed).
    // 21 enters 15% slower.
    let e = if press { 1.0 } else { snap(since, if matches!(style, 21 | 25..=27) { 0.23 } else { 0.2 }) };
    // Loose copies grow out from the face's centre.
    let grown = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> {
        let k = 0.75 + 0.25 * e;
        pts.into_iter().map(|p| [w / 2.0 + (p[0] - w / 2.0) * k, h / 2.0 + (p[1] - h / 2.0) * k]).collect()
    };
    // Round 3's tight living slab (14, 15): 4 pt in from the box at the sides and 3 at the top and
    // bottom, drifting at most 2 pt, so its edges stay clear of the letters.
    let tight = || moved(&living(w - 8.0, h - 6.0, 2.0, s.t, seed), 4.0, 3.0);
    if active {
        match style {
            // Only the living quads: cyan under paper (or under red on press).
            1 => {
                let top = if press { red() } else { paper() };
                face_pts = scaled(living(w, h, 4.0, s.t * 0.7, seed.wrapping_add(13)));
                back = back.fill(scaled(living(w, h, 9.0, s.t, seed)), cyan());
                (fill, ink, edge) = (top, if press { paper() } else { black() }, top);
                shadow = false;
            }
            // The slab itself drifts; a cyan copy of it is the shadow.
            2 | 14 => {
                face_pts = if style == 14 { tight() } else { living(w, h, 3.0, s.t, seed) };
                back = back.fill(moved(&face_pts, e * if press { 2.0 } else { 6.0 }, e * if press { 2.0 } else { 5.0 }), cyan());
                if !press {
                    (fill, ink, edge) = (paper(), black(), black());
                }
                shadow = false;
            }
            // The slab fills red from the left with a slanted edge, leaving a wedge unfilled.
            3 => {
                let f = if press { 1.0 } else { snap(since, 0.12).min(1.0) };
                let reach = (w + 10.0) * f * 0.88;
                let mut sh = Shape::new().fill(inflate(&slab, 2.0), black()).fill(slab.clone(), paper());
                sh = sh.fill(vec![[-2.0, -2.0], [reach + 8.0, -2.0], [reach - 8.0, h + 2.0], [-2.0, h + 2.0]], red());
                face_sh = Some(sh);
                ink = paper();
                scale = if press { 0.94 } else { 1.06 };
                turn = if press { 0.0 } else { tilt(seed, 0.05, 0.08) };
            }
            // The slab stays; colour-split copies of it jitter around its edges.
            4 | 13 | 22 | 23 | 28 => {
                // Drifting at the living corners' pace (round 1 jittered 30 times a second).
                let j = |n: u32, a: f32| a + a * wave(s.t, seed.wrapping_add(n));
                let amp = e * if press { 2.0 } else { 4.0 };
                back = back
                    .fill(moved(&slab, -j(1, amp), -j(2, amp) * 0.6), hex("#ff2bd6"))
                    .fill(moved(&slab, j(3, amp), j(4, amp) * 0.6), cyan())
                    .fill(moved(&slab, -j(5, amp) * 0.5, j(6, amp)), hex("#ffe600"));
                // 22 keeps the idle colours on hover (press still turns red); 23 keeps them on
                // press too.
                let idle = if primary { (paper(), red(), black()) } else { (black(), paper(), paper()) };
                if style == 23 || (style == 22 && !press) {
                    (fill, ink, edge) = idle;
                } else if style == 28 {
                    // Black on hover and on press: the press shows by sinking in, the split tighter.
                    (fill, ink, edge) = (black(), paper(), black());
                } else if !press {
                    (fill, ink, edge) = (black(), paper(), black());
                }
                shadow = false;
                // 13, 22, 23 and 28 keep the hover's swell and tilt, as F does.
                if style == 4 {
                    (scale, turn) = (1.0, 0.0);
                }
            }
            // The slab swells and kicks; cyan and red spikes fly out of its right end and swap.
            5 => {
                let swap = ((s.t * 12.0) as u32) % 2 == 0;
                let (l1, l2) = if swap { (1.0, 0.6) } else { (0.6, 1.0) };
                let spike = |len: f32, lift: f32| vec![[w - 10.0, h * 0.35], [w + 40.0 * len, -10.0 * len - lift], [w - 4.0, h * 0.7]];
                back = back.fill(spike(l1, 0.0), cyan()).fill(spike(l2, 8.0), paper());
                scale = if press { 0.92 } else { 1.0 + 0.14 * (1.0 - snap(since, 0.2)).max(0.0) + 0.1 };
            }
            // B and D mixed: the slab drifts, and colour-split copies of its living shape drift
            // round it.
            9 => {
                face_pts = living(w, h, 3.0, s.t, seed);
                let j = |n: u32, a: f32| a + a * wave(s.t, seed.wrapping_add(n));
                let amp = e * if press { 2.0 } else { 4.0 };
                back = back
                    .fill(moved(&face_pts, -j(1, amp), -j(2, amp) * 0.6), hex("#ff2bd6"))
                    .fill(moved(&face_pts, j(3, amp), j(4, amp) * 0.6), cyan())
                    .fill(moved(&face_pts, -j(5, amp) * 0.5, j(6, amp)), hex("#ffe600"));
                if !press {
                    (fill, ink, edge) = (black(), paper(), black());
                }
                shadow = false;
                (scale, turn) = (1.0, 0.0);
            }
            // F and its mixes: red and cyan copies peek out on either side. 6: the slab is still
            // and the copies follow it; 7: the slab drifts and the copies follow it; 8: the slab
            // is still and the copies drift on their own; 10: both drift, each its own way.
            _ => {
                let d = |n: u32| e * (3.0 + 3.0 * (0.5 + 0.5 * (s.t * (2.0 + n as f32) + n as f32).sin()));
                if matches!(style, 7 | 10) {
                    face_pts = living(w, h, 3.0, s.t, seed);
                }
                if style == 15 {
                    face_pts = tight();
                }
                if matches!(style, 21 | 25..=27) {
                    // The idle slab turns into the living one as the hover enters, so the shape
                    // doesn't jump.
                    let k = e.clamp(0.0, 1.0);
                    face_pts = slab.iter().zip(tight()).map(|(a, b)| [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]).collect();
                }
                back = if matches!(style, 8 | 10) {
                    back.fill(moved(&grown(living(w, h, 4.0, s.t * 0.9, seed.wrapping_add(101))), -5.0 * e, -4.0 * e), cyan())
                        .fill(moved(&grown(living(w, h, 4.0, s.t * 1.1, seed.wrapping_add(202))), 5.0 * e, 4.0 * e), red())
                } else {
                    back.fill(moved(&face_pts, -d(1), -d(2) * 0.7), cyan()).fill(moved(&face_pts, d(3), d(4) * 0.7), red())
                };
                // G morphing's colours: 21 turns paper on hover; 25 keeps the idle colours on
                // hover (red on press), 26 keeps them on press too, 27 inverts on hover.
                let idle = if primary { (paper(), red(), black()) } else { (black(), paper(), paper()) };
                let inverted = if primary { (black(), paper(), black()) } else { (hex("#fbf6e8"), black(), black()) };
                match style {
                    25 if !press => (fill, ink, edge) = idle,
                    26 => (fill, ink, edge) = idle,
                    27 if !press => (fill, ink, edge) = inverted,
                    25 | 27 => {}
                    _ if !press => (fill, ink, edge) = (hex("#fbf6e8"), black(), black()),
                    _ => {}
                }
                shadow = false;
            }
        }
        if style == 0 {
            if let Some(c) = flash {
                let (a, b) = if press { (black(), cyan()) } else { (cyan(), paper()) };
                back = back
                    .fill(scaled(living(w, h, 13.0, s.t, seed)), a)
                    .fill(scaled(living(w, h, 9.0, s.t, seed.wrapping_add(7))), b)
                    .fill(scaled(living(w, h, 4.0, s.t * 0.7, seed.wrapping_add(13))), c);
                shadow = true;
            }
        }
    }
    // The face swells and tilts with the copies' entrance.
    if look == Look::Hover {
        (scale, turn) = (1.0 + (scale - 1.0) * e, turn * e);
    }
    // The shadow stays put; the face jumps off it on hover and slams into it on press.
    let (dx, dy, sd) = match look {
        Look::Hover if shadow => (-2.0, -3.0, 6.0),
        Look::Press if shadow => (3.0, 3.0, 2.0),
        _ => (0.0, 0.0, 4.0),
    };
    let mut face_sh = face_sh.unwrap_or_else(|| Shape::new().fill(inflate(&face_pts, 2.0), edge).fill(face_pts.clone(), fill));
    if look == Look::Disabled {
        face_sh = hatch(face_sh, w, h, 5.0, 1.2, if palette() == 0 { hex("#8a0a12") } else { hex("#081a4d") }, 0.0);
    }
    let label_el = if look == Look::Disabled {
        text(label.to_owned()).font(caps(15.0, 900.0).tracking(1.0)).color(ink)
    } else {
        lettered(lettering, label, ink, fill, key_seed(label))
    };
    let face = el().size(w, h).center().child(layer(face_sh, w, h)).child(label_el).offset(dx, dy).offset_motion(jump());
    let mut b = el().size(w + 6.0, BAR_H).pt(3.0).child(layer(back, w, h).pin(Some(0.0), Some(3.0), None, None));
    if shadow {
        b = b.child(layer(Shape::new().fill(moved(&slab, sd, sd), black()), w, h).pin(Some(0.0), Some(3.0), None, None));
    }
    b.child(face.pin(Some(0.0), Some(3.0), None, None)).scale(scale).scale_motion(kick()).rotate(turn).rotate_motion(kick())
}

/// Every hover style on the same buttons: idle, hovered, pressed.
pub fn hovers_sheet(s: &S, w: f32, h: f32) -> El {
    // The last column loops: idle for 0.8 s, hovered for 1.6 s.
    let cycle = s.t % 2.4;
    let entering = cycle >= 0.8;
    let col_w = 124.0;
    let mut list = col().gap(4.0).padding(20.0).pt(10.0);
    for (style, name, what) in HOVERS {
        let mut cols = row().gap(10.0).items_center();
        for (n, look) in [Look::Idle, Look::Hover, Look::Press, Look::Disabled, Look::Idle].into_iter().enumerate() {
            let looped = n == 4;
            let look = if looped && entering { Look::Hover } else { look };
            let since = if looped { cycle - 0.8 } else { 1.0 };
            let mut c = col().w(col_w).gap(4.0);
            for (key, label, primary) in [("add", "ADD", true), ("open", "OPEN…", false)] {
                let k = format!("{key}-{style}-{n}");
                c = c.child(button_styled(s, &k, label, primary, look, s.lettering, style, since));
            }
            cols = cols.child(c);
        }
        let caption = col()
            .w(200.0)
            .child(text(name.to_owned()).font(caps(14.0, 900.0).tracking(0.6)).color(paper()))
            .child(text(what.to_owned()).font(sans(10.5, 600.0)).color(paper().alpha(0.85)));
        list = list.child(row().gap(14.0).items_center().h(2.0 * BAR_H + 10.0).child(caption).child(cols));
    }
    let mut heads = row().pl(20.0 + 200.0 + 14.0).gap(10.0);
    for h in ["IDLE", "HOVER", "PRESSED", "DISABLED", "IDLE → HOVER"] {
        heads = heads.child(text(h.to_owned()).font(caps(12.0, 900.0).tracking(1.5)).color(black()).w(col_w));
    }
    el().w(w).h(h).child(background(s, w, h)).child(col().pt(12.0).child(heads).child(list).pin(Some(0.0), Some(0.0), None, None))
}

fn panel(st: &S, w: f32, h: f32, content: El) -> El {
    let mut sheet = jagged(0.0, 0.0, w - 10.0, h - 6.0, 1.2, 40.0, 77);
    if st.step_style == 5 {
        // E: the rows stay straight; the sheet's left edge steps with them instead.
        let top = 6.0 + 30.0;
        let mut pts = vec![[0.0, 0.0], [w - 10.0, 0.0], [w - 10.0, h - 6.0]];
        let n = ((h - 6.0 - top) / ROW_H).ceil() as usize;
        let x = |r: usize| ((r % 8) as f32) * STEP;
        pts.push([x(n.saturating_sub(1)), h - 6.0]);
        for r in (0..n).rev() {
            let y = top + r as f32 * ROW_H;
            pts.push([x(r), y]);
            if r > 0 {
                pts.push([x(r - 1), y]);
            }
        }
        pts.push([0.0, top]);
        sheet = pts;
    }
    // A black sheet with a white outline, as Persona 5 sets its lists.
    if jose6() {
        // Round 6: half see-through. 0 navy at half, 1 navy a little denser, 2 deep navy smoke.
        // 3–6 iterate on 2, the smoke: 3 fading (denser behind the header, clearing towards the
        // heap), 4 frosted, 5 a glass sheen, 6 a lemon rim. Chosen: 3 with 6's rim, now 0, the
        // default.
        let opt = match jose_opt(Part::Card) {
            0 => 7,
            o => o,
        };
        let smoke = hex("#020a2c");
        let base = match opt {
            1 => hex("#0a1a6e").alpha(0.62),
            2 | 5 | 6 => smoke.alpha(0.55),
            3 | 7 => smoke.alpha(0.18),
            4 => smoke.alpha(0.45),
            _ => hex("#0a1a6e").alpha(0.5),
        };
        let ring = sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>();
        let (edge, line) = if matches!(opt, 6 | 7) { (jose_yellow_glow(), jose_pale()) } else { (hex("#5fe8ff"), hex("#bff6ff")) };
        let glow = Shape::new().stroke(ring.clone(), 4.0, edge.alpha(0.7));
        let rim = Shape::new().stroke(ring, 1.4, line);
        let (cw, ch) = (w - 10.0, h - 6.0);
        let mut card = el().size(cw, ch).fill(base);
        if matches!(opt, 3 | 5 | 7) {
            card = card.clip_rounded(0.0);
        }
        match opt {
            3 | 7 => {
                // Denser at the top, gone by two thirds of the way down; blurred into a gradient.
                let mut g = Shape::new();
                for k in 0..8 {
                    let (y0, y1) = (ch * 0.6 * k as f32 / 8.0, ch * 0.6 * (k + 1) as f32 / 8.0 + 1.0);
                    g = g.fill(
                        vec![
                            [-40.0, y0 - 40.0 * (k == 0) as i32 as f32],
                            [cw + 40.0, y0 - 40.0 * (k == 0) as i32 as f32],
                            [cw + 40.0, y1],
                            [-40.0, y1],
                        ],
                        smoke.alpha(0.55 * (1.0 - k as f32 / 8.0)),
                    );
                }
                card = card.child(layer(g, cw + 80.0, ch).blur(20.0).pin(Some(-40.0), Some(0.0), None, None));
            }
            4 => card = card.backdrop_blur(7.0),
            5 => {
                // Two pale diagonal bands across the glass, drifting slowly.
                let x = (st.t * 12.0 + cw * 0.55).rem_euclid(cw + 400.0) - 200.0;
                let band = |x0: f32, wd: f32| vec![[x0, 0.0], [x0 + wd, 0.0], [x0 + wd - 160.0, ch], [x0 - 160.0, ch]];
                let g =
                    Shape::new().fill(band(x, 110.0), hex("#bff6ff").alpha(0.12)).fill(band(x + 150.0, 30.0), hex("#bff6ff").alpha(0.16));
                card = card.child(layer(g, cw, ch));
            }
            _ => {}
        }
        let clip =
            el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
        return el()
            .size(w, h)
            .child(layer(glow, w, h).blur(5.0))
            .child(card.pin(Some(0.0), Some(0.0), None, None))
            .child(layer(rim, w, h))
            .child(clip)
            .rotate(-0.008);
    }
    if jose5() {
        // Round 5: clear enough to see the sky through. 15 clear navy, 16 clearer vivid blue, 17
        // a dark smoke that keeps the moons gold.
        let (base, frost) = match palette() {
            16 => (hex("#1238c8").alpha(0.2), 0.0),
            // Dark, nearly hueless: dims the sky without turning its yellows green.
            17 => (hex("#030818").alpha(0.36), 0.0),
            _ => (hex("#0a1a6e").alpha(0.45), 0.0),
        };
        // The glow is a ring round the edge: a filled glow would show through the clear card.
        let glow =
            Shape::new().stroke(sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>(), 4.0, hex("#5fe8ff").alpha(0.7));
        let rim = Shape::new().stroke(sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>(), 1.4, hex("#bff6ff"));
        let mut card = el().size(w - 10.0, h - 6.0).fill(base);
        if frost > 0.0 {
            card = card.backdrop_blur(frost);
        }
        let clip =
            el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
        return el()
            .size(w, h)
            .child(layer(glow, w, h).blur(5.0))
            .child(card.pin(Some(0.0), Some(0.0), None, None))
            .child(layer(rim, w, h))
            .child(clip)
            .rotate(-0.008);
    }
    if jose4() {
        // Round 4: navy again, livelier. 12 vivid, 13 frosted (the sky blurred through it), 14
        // frosted with a violet glow beyond the cyan rim.
        let (base, frost) = match palette() {
            12 => (hex("#0b1f7a").alpha(0.86), 0.0),
            14 => (hex("#0c1a6a").alpha(0.74), 8.0),
            _ => (hex("#0a1a62").alpha(0.72), 10.0),
        };
        let mut glow = Shape::new().fill(inflate(&sheet, 3.0), hex("#5fe8ff").alpha(0.9));
        if palette() == 14 {
            glow = Shape::new().fill(inflate(&sheet, 7.0), hex("#a98bff").alpha(0.7)).fill(inflate(&sheet, 3.0), hex("#5fe8ff").alpha(0.9));
        }
        let rim = Shape::new().stroke(sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>(), 1.6, hex("#bff6ff"));
        let mut card = el().size(w - 10.0, h - 6.0).fill(base);
        if frost > 0.0 {
            card = card.backdrop_blur(frost);
        }
        let clip =
            el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
        return el()
            .size(w, h)
            .child(layer(glow, w, h).blur(6.0))
            .child(card.pin(Some(0.0), Some(0.0), None, None))
            .child(layer(rim, w, h))
            .child(clip)
            .rotate(-0.008);
    }
    let mut s = if jose3() {
        // Round 3's livelier cards: 9 electric blue, 10 blue with a teal sheen, 11 violet-blue.
        let (base, rim) = match palette() {
            10 => (hex("#05267f").alpha(0.72), hex("#5ff0ff")),
            11 => (hex("#22137a").alpha(0.72), hex("#b8a8ff")),
            _ => (hex("#0a1f86").alpha(0.66), hex("#7fe7ff")),
        };
        let mut c = Shape::new()
            .fill(moved(&sheet, 6.0, 6.0), black().alpha(0.3))
            .fill(inflate(&sheet, 7.0), rim.alpha(0.1))
            .fill(inflate(&sheet, 4.0), rim.alpha(0.2))
            .fill(inflate(&sheet, 1.8), rim.alpha(0.85))
            .fill(sheet.clone(), base);
        if palette() == 10 {
            // A teal sheen across the lower right.
            c = c.fill(vec![[w * 0.35, h], [w - 10.0, h * 0.25], [w - 10.0, h - 6.0]], hex("#22c3e6").alpha(0.2));
        }
        c
    } else if glass() {
        // Jose's card: see-through navy, a cyan glow for a rim, a soft shadow.
        Shape::new()
            .fill(moved(&sheet, 6.0, 6.0), black().alpha(0.35))
            .fill(inflate(&sheet, 5.0), hex("#7fe7ff").alpha(0.18))
            .fill(inflate(&sheet, 2.0), hex("#7fe7ff").alpha(0.7))
            .fill(sheet.clone(), hex("#061235").alpha(0.82))
    } else {
        Shape::new().fill(moved(&sheet, 10.0, 9.0), black()).fill(inflate(&sheet, 3.5), paper()).fill(sheet.clone(), black())
    };
    // A faint screentone in the sheet's bottom corner.
    if !glass() {
        s = halftone(s, w - 14.0, h - 10.0, 6.0, hex("#1e1e1e"), |x, y| ((x / (w - 14.0)) * (y / (h - 10.0)) * 3.4 - 0.6).max(0.0));
    }
    // The paper clips the rows sliding in, but leaves room for the selection's overhang.
    let clip =
        el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
    el().size(w, h).child(layer(s, w, h)).child(clip).rotate(-0.008)
}

/// A column heading in ransom letters, mostly bare: black on the paper strip.
fn heading(word: &str, seed: u32) -> El {
    ransom_mix(word, 11.5, seed, &[(red(), paper()), (black(), paper())], &[black()], 0.18)
}

fn header(_: &S, w: f32) -> El {
    let w = w - 10.0;
    let strip = if tape() { moved(&tape_strip(w, 24.0, 5), -2.0, 2.0) } else { moved(&cut(w, 24.0, 12.0, 5), -2.0, 2.0) };
    let fixed: f32 = crate::common::COLS.iter().map(|c| c.1).sum();
    // As wide as the rows' cells, which leave room to step right.
    let cells_w = w - STEP * 7.0;
    let mut r = row().w(cells_w).h(26.0).mt(1.0).items_center().px(8.0);
    r = r.child(row().w(cells_w - fixed - 16.0).child(heading("NAME", 101)));
    for (k, (col, cw)) in crate::common::COLS.iter().enumerate() {
        let right = !matches!(*col, "Progress" | "Status");
        let mut c = row().w(*cw).pl(if *col == "Progress" { 4.0 } else { 6.0 });
        if right {
            c = c.justify_end();
        }
        r = r.child(c.child(heading(&col.to_uppercase(), 211 + k as u32 * 37)));
    }
    if jose4() {
        // A saturated yellow strip in a soft, blurred glow.
        let (g, a) = if jose6() {
            (0.3, 0.94)
        } else if jose5() {
            (0.3, 0.8)
        } else {
            (0.7, 0.94)
        };
        let (body, halo) = if jose6() { (jose_yellow(), jose_yellow_glow()) } else { (hex("#ffd21f"), hex("#ffd60a")) };
        let glow = layer(Shape::new().fill(inflate(&strip, 3.0), halo.alpha(g)), w, 30.0).blur(5.0);
        let sh = Shape::new().fill(strip, body.alpha(a));
        return el().w(w + 10.0).h(30.0).child(glow).child(layer(sh, w, 30.0)).child(r);
    }
    let sh = if jose3() {
        // A glowing, see-through yellow strip.
        Shape::new()
            .fill(inflate(&strip, 6.0), hex("#ffe14a").alpha(0.12))
            .fill(inflate(&strip, 3.0), hex("#ffe14a").alpha(0.22))
            .fill(strip, hex("#ffe55c").alpha(0.86))
    } else {
        Shape::new().fill(strip, paper())
    };
    el().w(w + 10.0).h(30.0).child(layer(sh, w, 30.0)).child(r)
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &torrent(s, i);
    let w = w - 10.0;
    let key = format!("row-{i}");
    let mut bg = Shape::new();
    let (name, muted, mut status_c) =
        if selected { (on_select(), on_select(), on_select()) } else { (paper(), hex("#a8a8a8"), hex("#e2e2e2")) };
    if t.kind == Kind::Error && !selected {
        status_c = error_c();
    }
    let mut turn = 0.0;
    // Round 4's flag glows through a blur rather than stacked rings.
    let mut glow_sh: Option<Shape> = None;
    if selected {
        // A red band with a wedge out past the left edge, cut fresh for each selection, slamming
        // in from the left.
        let seed = selection_seed(s, t);
        let f = snap(s.t - s.selected_at, 0.26);
        let lean = 4.0 + 6.0 * noise(seed);
        let h = ROW_H;
        let band =
            vec![[-26.0, h / 2.0 + 2.0], [-10.0 + lean, 2.0], [w + 10.0 + lean, 0.0], [w + 4.0 - lean, h - 3.0], [-12.0 - lean, h - 1.0]];
        let band = if flag() {
            // Jose's flag: a swallowtail at the left, the top and bottom edges swaying.
            let sway = |x: f32, k: f32| (s.t * 2.2 + x * 0.012 + k).sin() * 3.0;
            let n = 12;
            let mut pts = vec![[-4.0, sway(-4.0, 0.0)]];
            for i in 1..=n {
                let x = -4.0 + (w + 12.0) * i as f32 / n as f32;
                pts.push([x, 1.0 + sway(x, 0.0)]);
            }
            pts.push([w + 2.0, h * 0.5]);
            for i in (0..=n).rev() {
                let x = -4.0 + (w + 12.0) * i as f32 / n as f32;
                pts.push([x - 8.0, h - 1.0 + sway(x, 1.3)]);
            }
            // The swallowtail's notch.
            pts.push([12.0, h * 0.5]);
            pts
        } else {
            band
        };
        let band: Vec<[f32; 2]> = band.into_iter().map(|p| [-26.0 + (p[0] + 26.0) * f, p[1]]).collect();
        // Under it, a cyan sliver that never rests, as on the game's highlighted rows.
        let sliver: Vec<[f32; 2]> =
            living(w * 0.4, h - 10.0, 4.0, s.t, seed).into_iter().map(|p| [w * 0.6 + p[0] * f, p[1] + 5.0]).collect();
        if jose4() {
            let bow = |p: [f32; 2]| {
                let u = ((p[0] + 4.0) / (w + 12.0)).clamp(0.0, 1.0);
                [p[0], p[1] - 4.0 * (1.0 - (2.0 * u - 1.0).powi(2)) + 2.0]
            };
            let band: Vec<[f32; 2]> = band.into_iter().map(bow).collect();
            let (g, a) = if jose6() {
                (0.32, 0.94)
            } else if jose5() {
                (0.32, 0.8)
            } else {
                (0.8, 0.92)
            };
            let (body, halo) = if jose6() { (jose_yellow(), jose_yellow_glow()) } else { (hex("#ffd60a"), hex("#ffd60a")) };
            glow_sh = Some(Shape::new().fill(inflate(&band, 2.0), halo.alpha(g)));
            let ghost = if jose6() { jose_pale() } else { hex("#fff3a0") };
            bg = bg.fill(sliver, ghost.alpha(0.5)).fill(band, body.alpha(a));
        } else if jose3() {
            // Jose's ribbon: bowed, glowing, a touch see-through, over a pale-yellow ghost.
            let bow = |p: [f32; 2]| {
                let u = ((p[0] + 4.0) / (w + 12.0)).clamp(0.0, 1.0);
                [p[0], p[1] - 4.0 * (1.0 - (2.0 * u - 1.0).powi(2)) + 2.0]
            };
            let band: Vec<[f32; 2]> = band.into_iter().map(bow).collect();
            bg = bg
                .fill(sliver, hex("#fff3a0").alpha(0.5))
                .fill(inflate(&band, 7.0), hex("#ffd60a").alpha(0.1))
                .fill(inflate(&band, 3.5), hex("#ffd60a").alpha(0.22))
                .fill(band, hex("#ffd60a").alpha(0.9));
        } else {
            bg = bg.fill(sliver, cyan()).fill(band, select());
        }
        turn = tilt(seed, 0.004, 0.012);
    } else {
        bg = bg.line([0.0, ROW_H - 0.5], [w, ROW_H - 0.5], 1.0, LineCap::Butt, hex("#333333"));
        if look == Look::Hover {
            // A red shard points at the row, and a zigzag runs under it.
            let shard = blade(10.0, ROW_H - 16.0, 4.0, active_seed(s, &key));
            bg = bg.fill(moved(&shard, -6.0, 8.0), red());
            let zig: Vec<[f32; 2]> =
                (0..=(w / 8.0) as usize).map(|k| [k as f32 * 8.0, ROW_H - 4.0 + if k % 2 == 0 { 0.0 } else { 3.0 }]).collect();
            bg = bg.stroke(zig, 2.2, red());
        }
    }
    // Rows step right one after another, as the game's lists do, every eight rows over.
    let step = step_offset(s.step_style, i, &|k| torrent(s, k).name);
    let nudge = if look == Look::Hover && !selected {
        8.0
    } else if selected && jose6() {
        // The flag's row moves in off its swallowtail, so the name clears the notch.
        [8.0, 14.0, 20.0][jose_opt(Part::Inset) as usize % 3]
    } else {
        0.0
    };
    let cells_el = cells(s, &PARTS, t, w - STEP * 7.0, name, muted, status_c).offset(step + nudge, 0.0).offset_motion(kick());
    // Rows slam in from the right, one after another, when the window appears.
    let enter = snap(s.t - 0.15 - i as f32 * 0.07, 0.38);
    let mut row_el = el().w(w).h(ROW_H);
    if let Some(g) = glow_sh {
        row_el = row_el.child(layer(g, w, ROW_H).blur(6.0));
    }
    row_el.child(layer(bg, w, ROW_H)).child(cells_el).offset((1.0 - enter) * 900.0, 0.0).rotate(turn).rotate_motion(kick())
}

/// How far each row steps right of the one above.
const STEP: f32 = 3.0;

fn is_selected(s: &S, t: &Torrent) -> bool {
    // By name: `TORRENTS` is a const, so each use is its own copy and addresses don't match.
    s.selected.is_some_and(|i| torrent(s, i).name == t.name)
}

/// The seed of the current selection: it changes when the selection does, not on every hover.
fn selection_seed(s: &S, t: &Torrent) -> u32 {
    key_seed(t.name) ^ ((s.selected_at * 1000.0) as i32 as u32).wrapping_mul(2654435761)
}

/// The cells C draws its own way: the selected name with one letter inverted, and the speeds
/// underlined in colour.
fn cell(s: &S, t: &Torrent, col: &str, w: f32, color: Color) -> Option<El> {
    match col {
        "Name" if is_selected(s, t) => {
            let chars: Vec<char> = t.name.chars().collect();
            let letters: Vec<usize> = (0..chars.len().min(14)).filter(|&k| chars[k].is_alphabetic()).collect();
            let k = letters[(noise(selection_seed(s, t)) * letters.len() as f32) as usize % letters.len()];
            let font = list_font(14.0, 800.0);
            let before: String = chars[..k].iter().collect();
            let after: String = chars[k + 1..].iter().collect();
            Some(
                row()
                    .w(w)
                    .items_center()
                    .child(text(before).font(font).color(on_select()))
                    .child(el().fill(on_select()).px(1.5).child(text(chars[k].to_string()).font(font).color(select())))
                    .child(text(after).font(font).color(on_select()).truncate()),
            )
        }
        "Down" | "Up" => {
            let v = if col == "Down" { t.down } else { t.up };
            (v != "–").then(|| speed_cell(s.speed_style, v, col == "Down", color, s.t))
        }
        _ => None,
    }
}

/// A speed over a coloured underline, as the game sets HP and SP.
fn col_el(v: &str, ink: Color, line: Color) -> El {
    let u = Shape::new().fill(vec![[4.0, 0.0], [58.0, 0.0], [56.0, 2.5], [0.0, 2.5]], line);
    col()
        .items_end()
        .gap(1.0)
        .child(text(v.to_owned()).font(figures(12.5)).color(ink))
        .child(el().size(58.0, 3.0).child(layer(u, 58.0, 3.0)))
}

/// A different bar for each state, so the state reads at a glance.
fn progress(s: &S, t: &Torrent, w: f32) -> El {
    let bw = w - 52.0;
    let h = 12.0;
    let track = jagged(0.0, 0.0, bw, h, 0.8, 30.0, 3);
    let fw = (bw * t.progress).max(if t.progress > 0.0 { 6.0 } else { 0.0 });
    // Round 2: a leaning, not quite parallel quad around the bar, so the fill stays inside it.
    let lean = vec![[0.5, -1.5], [bw + 5.0, -0.5], [bw + 0.5, h + 1.5], [-9.0, h + 0.5]];
    // 11 and 12 bend the bar's contents into the leaning quad too: a point at (x, y) in the
    // bar's rectangle goes to the same place between the quad's corners.
    let bend = matches!(s.track_style, 11 | 12 | 16 | 17);
    // 16 and 17: the top and bottom stay level; only the sides lean.
    let level = vec![[0.5, 0.0], [bw + 5.0, 0.0], [bw + 0.5, h], [-9.0, h]];
    let q = if matches!(s.track_style, 16 | 17) { level.clone() } else { lean.clone() };
    let warp = move |p: [f32; 2]| -> [f32; 2] {
        if !bend {
            return p;
        }
        let (u, v) = (p[0] / bw, p[1] / h);
        let top = [q[0][0] + (q[1][0] - q[0][0]) * u, q[0][1] + (q[1][1] - q[0][1]) * u];
        let bot = [q[3][0] + (q[2][0] - q[3][0]) * u, q[3][1] + (q[2][1] - q[3][1]) * u];
        [top[0] + (bot[0] - top[0]) * v, top[1] + (bot[1] - top[1]) * v]
    };
    let wp = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> { pts.into_iter().map(&warp).collect() };
    let mut sh = match s.track_style {
        // A, leaning.
        // 19, as chosen: the shadow is red for an error, and a seeding bar has none.
        19 if t.kind == Kind::Seeding => Shape::new().fill(level.clone(), black()),
        19 if t.kind == Kind::Error => Shape::new().fill(moved(&level, 2.5, 2.5), error_c()).fill(level.clone(), black()),
        17 | 19 | 20 | 24 => Shape::new().fill(moved(&level, 2.5, 2.5), paper()).fill(level.clone(), black()),
        16 => Shape::new().fill(inflate(&level, 1.5), paper()).fill(level.clone(), black()),
        6 | 11 => Shape::new().fill(inflate(&lean, 1.5), paper()).fill(lean.clone(), black()),
        // B, leaning.
        7 | 12 => Shape::new().fill(moved(&lean, 2.5, 2.5), paper()).fill(lean.clone(), black()),
        // D, leaning.
        8 => Shape::new().fill(inflate(&lean, 3.0), paper()).fill(inflate(&lean, 1.5), black()).fill(lean.clone(), hex("#222222")),
        // D, leaning, grey gap and black track.
        9 => Shape::new().fill(inflate(&lean, 3.0), paper()).fill(inflate(&lean, 1.5), hex("#4a4a4a")).fill(lean.clone(), black()),
        // D, grey gap and black track, on the round-1 shape.
        10 => Shape::new().fill(inflate(&track, 3.0), paper()).fill(inflate(&track, 1.5), hex("#4a4a4a")).fill(track.clone(), black()),
        // A black track with a white rim.
        1 => Shape::new().fill(inflate(&track, 1.5), paper()).fill(track.clone(), black()),
        // A black track over a white offset shadow.
        2 => Shape::new().fill(moved(&track, 2.5, 2.5), paper()).fill(track.clone(), black()),
        // A white rim and a red offset shadow.
        3 => Shape::new()
            .fill(moved(&inflate(&track, 1.5), 3.0, 3.0), red())
            .fill(inflate(&track, 1.5), paper())
            .fill(track.clone(), black()),
        // A sticker: white rim, a black gap, then the track.
        4 => Shape::new().fill(inflate(&track, 3.0), paper()).fill(inflate(&track, 1.5), black()).fill(track.clone(), hex("#222222")),
        // Paper track with a black rim: the bar reads as a white slip on the sheet.
        5 => Shape::new().fill(inflate(&track, 1.5), black()).fill(track.clone(), paper()),
        _ => Shape::new().fill(track, hex("#333333")),
    };
    let mut ink = black();
    let pct = if matches!(s.track_style, 19 | 20 | 24) {
        let (shape, c, label) = straight_fill(s, t, sh, &level, bw, h, fw, s.track_style != 20);
        sh = shape;
        ink = c;
        label
    } else {
        match t.kind {
            Kind::Downloading => {
                // An arrowhead driving right, white stripes racing through it.
                let body = (fw - 8.0).max(0.0);
                sh = sh.fill(wp(vec![[0.0, 0.0], [body, 0.0], [fw + 4.0, h / 2.0], [body, h], [0.0, h]]), red());
                sh = hatch_in(sh, (body - 4.0).max(0.0), h, 9.0, 3.0, paper().alpha(0.85), s.t * 26.0, &warp);
                ink = red();
                None
            }
            Kind::Seeding => {
                // Full; paper with red chevrons marching out, as it gives back.
                sh = sh.fill(wp(vec![[0.0, 0.0], [fw, 0.0], [fw, h], [0.0, h]]), paper());
                let pitch = 12.0;
                let mut x = -pitch + (s.t * 10.0).rem_euclid(pitch);
                while x < fw - 12.0 {
                    if x > 0.0 {
                        sh = sh.stroke(wp(vec![[x, 2.5], [x + 4.5, h / 2.0], [x, h - 2.5]]), 2.4, red());
                    }
                    x += pitch;
                }
                None
            }
            Kind::Checking => {
                // Halftone, with a red slit scanning the checked part.
                sh = sh.fill(wp(vec![[0.0, 0.0], [fw, 0.0], [(fw - 5.0).max(0.0), h], [0.0, h]]), paper());
                sh = halftone_in(sh, (fw - 4.0).max(0.0), h, 4.0, black(), |x, _| 0.4 + 1.2 * (x / fw), &warp);
                let sx = (s.t * 0.9).fract() * fw;
                sh = sh.fill(wp(vec![[sx, -2.0], [sx + 4.0, -2.0], [sx + 1.0, h + 2.0], [sx - 3.0, h + 2.0]]), red());
                None
            }
            Kind::Paused => {
                // Frozen: grey blocks with gaps, and nothing moves.
                let mut x = 0.0;
                while x < fw {
                    let bx = (x + 7.0).min(fw);
                    sh = sh.fill(wp(vec![[x + 2.0, 1.5], [bx, 1.5], [bx - 2.0, h - 1.5], [x, h - 1.5]]), hex("#9a9a9a"));
                    x += 10.0;
                }
                ink = hex("#777777");
                None
            }
            Kind::Error => {
                // Torn off at the progress, cracked, and twitching now and then.
                let mut torn = vec![[0.0, 0.0]];
                for k in 0..5 {
                    torn.push([fw + (noise(50 + k) - 0.3) * 8.0, h * k as f32 / 4.0]);
                }
                torn.push([0.0, h]);
                sh = sh.fill(wp(torn), red());
                sh = sh.stroke(wp(vec![[fw + 6.0, 0.0], [fw + 12.0, 6.0], [fw + 9.0, 8.0], [fw + 16.0, h]]), 1.6, red());
                ink = red();
                None
            }
            Kind::Fetching => {
                // No size yet: a paper shard sliding to and fro.
                let x = (s.t * 2.4).sin() * 0.5 + 0.5;
                let sw = 22.0;
                let sx = x * (bw - sw);
                sh = sh.fill(wp(vec![[sx + 5.0, 1.0], [sx + sw, 1.0], [sx + sw - 5.0, h - 1.0], [sx, h - 1.0]]), paper());
                Some("??".to_owned())
            }
        }
    };
    let pct = pct.unwrap_or_else(|| format!("{:.0}%", (t.progress * 100.0).floor()));
    let twitch = if t.kind == Kind::Error && (s.t * 0.7).fract() < 0.08 { (s.t * 90.0).sin() * 1.5 } else { 0.0 };
    // The percent in a slanted pill, as the game sets item counts; on the selection it turns dark
    // red with a cyan sliver.
    let (pw, ph) = (46.0, 17.0);
    let mut pill = Shape::new();
    if is_selected(s, t) {
        pill = pill.fill(vec![[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], deep());
        // The cyan sliver pulses, as on the game's selected item count.
        let pulse = 3.0 + 3.0 * (0.5 + 0.5 * (s.t * 5.0).sin());
        pill = pill.fill(vec![[pw - 3.0, -1.0], [pw + pulse, ph * 0.3], [pw - 3.0 + pulse * 0.4, ph + 1.0], [pw - 8.0, ph]], cyan());
        ink = paper();
    } else {
        pill = pill.fill(vec![[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], paper());
    }
    row()
        .w(w)
        .items_center()
        .gap(6.0)
        .child(el().size(bw, h).child(layer(sh, bw, h)).offset(twitch, 0.0))
        .child(el().size(pw, ph).center().child(layer(pill, pw, ph)).child(text(pct).font(caps(14.0, 900.0)).color(ink)))
}

fn status(s: &S, w: f32) -> El {
    let ribbon = cut(w, 24.0, 12.0, 19);
    let sh = Shape::new().fill(moved(&ribbon, 0.0, 4.0), paper()).fill(ribbon, black());
    // The ghost is the connection state: bobbing while connected.
    let bob = (s.t * 2.2).sin() * 2.0;
    el().w(w).h(crate::STATUS_H).child(layer(sh, w, 26.0)).rotate(-0.004).child(
        row()
            .pin(Some(8.0), Some(0.0), Some(12.0), Some(2.0))
            .items_center()
            .gap(6.0)
            .child(el().size(30.0, 24.0).child(ghost(30.0, Some(red())).pin(Some(0.0), Some(-8.0), None, None).offset(0.0, bob)))
            .child(ransom_mix("CONNECTED", 13.5, 77, &[(red(), paper()), (paper(), black())], &[paper()], 0.25))
            .child(el().grow(1.0))
            .child(text("6 TORRENTS · ↓ 68 MiB/s · ↑ 1.5 MiB/s").font(caps(14.0, 800.0).tracking(0.6)).color(paper())),
    )
}

fn errors(s: &S, w: f32) -> El {
    let h = 204.0;
    let card = jagged(0.0, 0.0, w, h, 3.0, 16.0, 23);
    let mut sh = Shape::new().fill(moved(&card, 9.0, 9.0), paper()).fill(inflate(&card, 3.0), paper()).fill(card, black());
    sh = halftone(sh, w, h, 7.0, hex("#2a2a2a"), |x, y| ((x / w) + (y / h) - 0.9).max(0.0) * 2.4);
    let mut list = col().gap(10.0).padding(16.0).pt(10.0);
    list = list.child(ransom("ERRORS!", 25.0, 31, &[(paper(), black()), (red(), paper()), (paper(), red()), (red(), black())]).mb(4.0));
    for (k, (name, why)) in ERRORS.iter().enumerate() {
        let star = burst(13.0, 13.0, 7.5, 13.0, 7, 0.3 + s.t * 0.8 * if k % 2 == 0 { 1.0 } else { -1.0 }, k as u32);
        list = list.child(
            row()
                .gap(10.0)
                .items_start()
                .child(
                    el().size(26.0, 26.0)
                        .center()
                        .child(layer(Shape::new().fill(star, red()), 26.0, 26.0))
                        .child(text("!").font(caps(15.0, 900.0)).color(paper())),
                )
                .child(
                    col()
                        .gap(0.0)
                        .w(w - 70.0)
                        .child(text(name.to_uppercase()).font(caps(15.0, 900.0)).color(paper()).truncate())
                        .child(text(why.to_string()).font(sans(12.0, 500.0)).color(hex("#c9c9c9")).truncate()),
                ),
        );
    }
    el().size(w + 10.0, h + 10.0).child(layer(sh, w, h)).child(list.pin(Some(0.0), Some(0.0), None, None)).rotate(0.025)
}

// ------------------------------------------------------------------ lettering options

/// The letterings on the comparison sheet: (style, name, what it tries). Rounds 1 and 2 are in
/// `out/v/letters-round1.png` and `letters-round2.png`; this is round 3: each pick from round 2
/// beside its change; round 4 narrows to the user's three, P2 with and without a box; round 5 is P2
/// with one box, with the lowercase letters bounded.
pub const LETTERINGS: [(u8, &str, &str); 4] = [
    (19, "P2 + one box", "round 4: some vowels lowercase, by chance"),
    (20, "Up to a third", "at least one lowercase, at most 1 in 3"),
    (21, "Up to a half", "at least one lowercase, at most 1 in 2"),
    (22, "Exactly one", "one lowercase letter per word"),
];

/// Heavy faces only, each with its cap height as a share of the em, so a mix can share one
/// cap height.
const HEAVY: [(&str, f32, f32); 5] = [
    ("Futura", 800.0, 0.70),
    ("Impact", 400.0, 0.79),
    ("Arial Black", 900.0, 0.72),
    ("Avenir Next", 900.0, 0.71),
    ("DIN Condensed", 700.0, 0.71),
];

/// Heavy and serif faces mixed, as the pause menu mixes them, with cap heights.
const MIXED: [(&str, f32, f32); 6] = [
    ("Futura", 800.0, 0.70),
    ("Superclarendon", 900.0, 0.72),
    ("Arial Rounded MT Bold", 700.0, 0.72),
    ("Times New Roman", 700.0, 0.66),
    ("Arial Black", 900.0, 0.72),
    ("Impact", 400.0, 0.79),
];

fn face(name: &'static str, size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named(name))
}

/// `word` set as `ink` on a button whose face is `bg`, in lettering `style` (0-based index into
/// [`LETTERINGS`]).
fn lettered(style: u8, word: &str, ink: Color, bg: Color, seed: u32) -> El {
    let chars: Vec<char> = word.chars().collect();
    let cap = 10.5;
    // Outlined letters are light with a dark rim, as the game sets them; the rim is red on a
    // black face, where black wouldn't show.
    let edge = if bg == black() { red() } else { black() };
    let lit = if ink == black() { paper() } else { ink };
    match style {
        1 => text(word.to_owned()).font(caps(15.5, 900.0).tracking(1.0)).color(ink),
        2 => {
            let letters: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
            let pick = letters[seed as usize % letters.len()];
            let mut r = row().items_center();
            for (k, ch) in chars.iter().enumerate() {
                let g = text(ch.to_string()).font(face("Futura", 14.0, 800.0));
                r = r.child(if k == pick { el().fill(ink).px(1.5).child(g.color(bg)) } else { g.color(ink) });
            }
            r
        }
        3 | 4 => {
            let build = |c: Color| {
                let mut r = row().items_end();
                for (k, ch) in chars.iter().enumerate() {
                    let (f, wt, ratio) = HEAVY[(noise(seed.wrapping_add(k as u32 * 13)) * HEAVY.len() as f32) as usize % HEAVY.len()];
                    r = r.child(text(ch.to_string()).font(face(f, cap / ratio, wt)).color(c));
                }
                r
            };
            if style == 4 { outlined(build, lit, edge, 1.6) } else { build(ink) }
        }
        5 => {
            let build = |c: Color| {
                row()
                    .items_end()
                    .child(text(chars[0].to_string()).font(face("Superclarendon", 19.0, 900.0)).color(c).mb(-1.0))
                    .child(text(chars[1..].iter().collect::<String>()).font(face("Futura", 13.0, 800.0)).color(c))
            };
            outlined(build, lit, edge, 1.4)
        }
        6 => {
            let (w, h) = (16.0, 20.0);
            let scrap = jagged(0.0, 0.0, w, h, 1.4, 7.0, seed);
            let tile = el()
                .size(w, h)
                .center()
                .child(layer(Shape::new().fill(moved(&scrap, 1.5, 1.5), black()).fill(scrap, red()), w, h))
                .child(text(chars[0].to_string()).font(face("Superclarendon", 15.0, 900.0)).color(paper()))
                .rotate(-0.08);
            row()
                .gap(1.0)
                .items_center()
                .child(tile)
                .child(text(chars[1..].iter().collect::<String>()).font(caps(15.0, 900.0).tracking(0.8)).color(ink))
        }
        7 => ransom(word, 11.0, seed, &[(paper(), black()), (black(), paper()), (red(), paper())]),
        8 => {
            let build = |c: Color| text(word.to_owned()).font(face("Futura", 14.0, 900.0).tracking(0.6)).color(c);
            // The extrusion: grey copies stepping down-left, under the outline.
            let mut e = el();
            for k in 1..=3 {
                e = e.child(build(hex("#8a8a8a")).pin(Some(0.0), Some(0.0), None, None).offset(-(k as f32), k as f32));
            }
            e.child(outlined(build, lit, black(), 1.8))
        }
        9 | 15 => {
            // One letter in four, and always at least one, in a second face. 15 also lowercases
            // one letter and boxes it.
            let letters: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
            let pick = letters[seed as usize % letters.len()];
            let mut r = row().items_end();
            for (k, ch) in chars.iter().enumerate() {
                let n = seed.wrapping_add(k as u32 * 13);
                let swap = k == pick || (k > 0 && ch.is_alphabetic() && noise(n) < 0.2);
                let f = if swap { face("Superclarendon", 13.5, 900.0) } else { face("Futura", 14.0, 800.0) };
                if style == 15 && k == pick {
                    let g: String = ch.to_lowercase().collect();
                    r = r.child(el().fill(ink).px(1.5).child(text(g).font(f).color(bg)).rotate((noise(n + 2) - 0.5) * 0.2));
                    continue;
                }
                r = r.child(text(ch.to_string()).font(f).color(ink).rotate((noise(n + 2) - 0.5) * 0.06));
            }
            r
        }
        10 | 16 => {
            let letters: Vec<usize> = (0..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
            let a = letters[seed as usize % letters.len()];
            let b = letters[(seed as usize / 7 + 2) % letters.len()];
            let mut r = row().items_center().gap(0.5);
            for (k, ch) in chars.iter().enumerate() {
                let n = seed.wrapping_add(k as u32 * 13);
                let (f, wt, ratio) = MIXED[(noise(n) * MIXED.len() as f32) as usize % MIXED.len()];
                let lower = k > 0 && noise(n + 9) > 0.75;
                let g: String = if lower { ch.to_lowercase().collect() } else { ch.to_string() };
                let size = cap / ratio * (0.92 + 0.16 * noise(n + 1));
                let t = text(g).font(face(f, size, wt));
                let e = if k == a || (style == 10 && k == b && letters.len() > 2) {
                    el().fill(ink).px(1.5).child(t.color(bg))
                } else {
                    el().child(t.color(ink))
                };
                r = r.child(e.rotate((noise(n + 2) - 0.5) * 0.14).offset(0.0, (noise(n + 3) - 0.5) * 2.0));
            }
            r
        }
        11 | 17 => {
            let letters: Vec<usize> = (0..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
            let boxed = if style == 17 { Some(letters[seed as usize % letters.len()]) } else { None };
            let mut r = row().items_center().gap(0.5);
            for (k, ch) in chars.iter().enumerate() {
                let n = seed.wrapping_add(k as u32 * 13);
                let (f, wt, ratio) = MIXED[(noise(n) * MIXED.len() as f32) as usize % MIXED.len()];
                let lower = k > 0 && noise(n + 9) > 0.65;
                let g: String = if lower { ch.to_lowercase().collect() } else { ch.to_string() };
                let size = cap / ratio * (0.9 + 0.25 * noise(n + 1)) * if k == 0 { 1.15 } else { 1.0 };
                let t = text(g).font(face(f, size, wt));
                let e = if boxed == Some(k) { el().fill(ink).px(1.5).child(t.color(bg)) } else { el().child(t.color(ink)) };
                r = r.child(e.rotate((noise(n + 2) - 0.5) * 0.22).offset(0.0, (noise(n + 3) - 0.5) * 2.5));
            }
            r
        }
        12..=14 | 18..=22 => {
            let mixed = matches!(style, 13 | 18..=22);
            // 19–22 box one letter (never the capital).
            let boxed = (style >= 19).then(|| {
                let letters: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
                letters[seed as usize % letters.len()]
            });
            // Which letters go lowercase. Round 4 (19): some vowels, by chance, so some words had
            // none. 20–22: always at least one, and at most a third, a half, or exactly one.
            let after: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
            let mut lower: Vec<usize> = after
                .iter()
                .copied()
                .filter(|&k| "AEIOU".contains(chars[k]) && noise(seed.wrapping_add(k as u32 * 13) + 9) > 0.35)
                .collect();
            if style >= 20 {
                if lower.is_empty() && !after.is_empty() {
                    let vowels: Vec<usize> = after.iter().copied().filter(|&k| "AEIOU".contains(chars[k])).collect();
                    let pool = if vowels.is_empty() { &after } else { &vowels };
                    lower.push(pool[(seed / 7) as usize % pool.len()]);
                }
                let most = match style {
                    20 => (after.len() / 3).max(1),
                    21 => (after.len() / 2).max(1),
                    _ => 1,
                };
                lower.truncate(most);
            }
            let build = |c: Color| {
                let mut r = row().items_end();
                for (k, ch) in chars.iter().enumerate() {
                    let n = seed.wrapping_add(k as u32 * 13);
                    let (g, f): (String, TextStyle) = if k == 0 {
                        (ch.to_string(), face("Superclarendon", 18.0, 900.0))
                    } else if lower.contains(&k) {
                        (ch.to_lowercase().collect(), face("Superclarendon", 14.5, 900.0))
                    } else if mixed && noise(n) > 0.6 {
                        let (f, wt, ratio) = MIXED[(noise(n + 4) * MIXED.len() as f32) as usize % MIXED.len()];
                        (ch.to_string(), face(f, cap / ratio, wt))
                    } else {
                        (ch.to_string(), face("Arial Rounded MT Bold", 14.0, 700.0))
                    };
                    let turn = if mixed { (noise(n + 2) - 0.5) * 0.12 } else { 0.0 };
                    let t = text(g).font(f);
                    let e = if boxed == Some(k) { el().fill(c).px(1.5).child(t.color(bg)) } else { el().child(t.color(c)) };
                    r = r.child(e.rotate(turn).mb(if k == 0 { -1.0 } else { 0.0 }));
                }
                r
            };
            let word = if matches!(style, 18..=22) {
                build(ink)
            } else {
                outlined(build, if ink == black() { paper() } else { ink }, black(), 2.0)
            };
            if style == 14 {
                // A black cut-out hugging the word, as the menu's words sit on.
                let (bw, bh) = (button_w(&chars.iter().collect::<String>()) - 10.0, 24.0);
                let blob = jagged(0.0, 0.0, bw, bh, 2.2, 9.0, seed);
                el().size(bw, bh).center().child(layer(Shape::new().fill(blob, black()), bw, bh)).child(word)
            } else {
                word
            }
        }
        _ => ransom_mix(word, 13.5, seed, &[(red(), paper()), (black(), paper()), (paper(), black())], &[ink], 0.22),
    }
}

/// What `build` draws, in `ink`, inside an outline `width` thick in `edge`: eight copies in the
/// edge colour around it.
fn outlined(build: impl Fn(Color) -> El, ink: Color, edge: Color, width: f32) -> El {
    let mut e = el();
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::TAU / 8.0;
        e = e.child(build(edge).pin(Some(0.0), Some(0.0), None, None).offset(a.cos() * width, a.sin() * width));
    }
    e.child(build(ink))
}

/// Every lettering on the same toolbar: idle, then hovered, so they can be compared side by side.
pub fn letters_sheet(s: &S, w: f32, h: f32) -> El {
    let mut list = col().gap(4.0).padding(24.0).pt(20.0);
    for (i, (style, name, what)) in LETTERINGS.iter().enumerate() {
        let style = *style;
        let mut bar = row().gap(8.0).items_center();
        for (key, label, primary, look) in [
            ("add", "ADD", true, Look::Idle),
            ("open", "OPEN…", false, Look::Idle),
            ("pause", "PAUSE", false, Look::Idle),
            ("remove", "REMOVE…", false, Look::Idle),
            ("resume", "RESUME", false, Look::Idle),
            ("add-h", "ADD", true, Look::Hover),
        ] {
            bar = bar.child(button_lettered(s, &format!("{key}-{i}"), label, primary, look, style));
        }
        let caption = col()
            .w(210.0)
            .child(text(name.to_string()).font(caps(15.0, 900.0).tracking(0.8)).color(paper()))
            .child(text(what.to_string()).font(sans(11.0, 600.0)).color(paper().alpha(0.8)));
        list = list.child(row().gap(16.0).items_center().h(BAR_H + 44.0).child(caption).child(bar));
    }
    el().w(w).h(h).child(background(s, w, h)).child(list.pin(Some(0.0), Some(0.0), None, None))
}

// ------------------------------------------------------------------ row steps

/// The ways rows can step right, for the comparison sheet: (name, what it tries).
pub const STEPS: [(&str, &str); 6] = [
    ("Before", "3 pt a row, reset every 8"),
    ("A  Zigzag", "out 7 rows, back 7"),
    ("B  Random", "0–12 pt, fixed per torrent"),
    ("C  From the top", "steps from the top of the view"),
    ("D  Wave", "a gentle sine, ±6 pt"),
    ("E  Sheet only", "rows straight, edge steps"),
];

/// How far row `i` steps right, in step style `style`; `name` gives the torrent at any row.
fn step_offset(style: u8, i: usize, name: &dyn Fn(usize) -> &'static str) -> f32 {
    // Five levels for the random styles, 0–14 pt; B2's quieter versions (9–11) span less.
    let levels = 5u32;
    let amp = match style {
        9 => 10.0,
        10 => 7.0,
        11 => 4.0,
        _ => 14.0,
    };
    let level = |k: u32| k as f32 * amp / (levels - 1) as f32;
    let roll = |nm: &str| (noise(key_seed(nm)) * levels as f32) as u32 % levels;
    match style {
        1 => {
            let k = i % 14;
            (if k <= 7 { k } else { 14 - k }) as f32 * STEP
        }
        2 => (noise(key_seed(name(i))) * 5.0).floor() * STEP,
        3 => i.min(7) as f32 * STEP,
        4 => 6.0 + (i as f32 * 0.8).sin() * 6.0,
        // E: every row sits past the deepest step, so the sheet's stepped edge never reaches the
        // text.
        5 => 7.0 * STEP,
        // B1: per torrent, any offset in 0–14, not on a grid.
        6 => noise(key_seed(name(i)).wrapping_mul(3)) * 14.0,
        // B2: per row; never the same level or a neighbouring one as the row above.
        7 | 9..=11 => {
            let mut prev = noise(11) as u32 % levels;
            for k in 1..=i {
                let r = (noise(1000 + k as u32) * levels as f32) as u32 % levels;
                prev = if r.abs_diff(prev) <= 1 { (prev + 2 + r % 2) % levels } else { r };
            }
            level(prev)
        }
        // B3: per torrent, but bumped away from the row above when they land too close.
        8 => {
            let mine = roll(name(i));
            if i == 0 {
                return level(mine);
            }
            let above = roll(name(i - 1));
            level(if mine.abs_diff(above) <= 1 { (mine + 2) % levels } else { mine })
        }
        _ => (i % 8) as f32 * STEP,
    }
}

/// Every step style over 22 rows, scrolled part-way (row 6 at the top), side by side.
pub fn steps_sheet(s: &S, w: f32, h: f32) -> El {
    let n = STEPS.len();
    let cw = (w - 24.0 - 8.0 * (n as f32 - 1.0)) / n as f32;
    let rh = 22.0;
    let mut cols = row().gap(8.0).px(12.0).pt(14.0).items_start();
    for (k, (name, what)) in STEPS.iter().enumerate() {
        let style = k as u8;
        let mut list = col();
        let mut sheet = Shape::new();
        for r in 0..24 {
            let i = r + 6;
            let nm = MANY[i % MANY.len()];
            // C counts from the first visible row, the others from the top of the list.
            let off = step_offset(style, if style == 3 { r } else { i }, &|k| MANY[k % MANY.len()]);
            let y = r as f32 * rh;
            let edge = if style == 5 { ((i % 8) as f32) * STEP } else { 0.0 };
            sheet = sheet.fill(vec![[edge, y], [cw, y], [cw, y + rh], [edge, y + rh]], black());
            let selected = r == 4;
            if selected {
                sheet = sheet
                    .fill(vec![[off - 6.0, y + 2.0], [cw + 4.0, y + 1.0], [cw - 2.0, y + rh - 1.0], [off - 10.0, y + rh - 2.0]], red());
            } else {
                sheet = sheet.line([edge, y + rh - 0.5], [cw, y + rh - 0.5], 1.0, LineCap::Butt, hex("#333333"));
            }
            list = list.child(
                row()
                    .h(rh)
                    .w(cw)
                    .items_center()
                    .pl(6.0 + off + edge)
                    .child(text(nm.to_owned()).font(list_font(10.5, 800.0)).color(paper()).truncate()),
            );
        }
        let body = el().w(cw).h(rh * 24.0).child(layer(sheet, cw, rh * 24.0)).child(list.pin(Some(0.0), Some(0.0), None, None));
        cols = cols.child(
            col()
                .w(cw)
                .gap(6.0)
                .child(text(name.to_string()).font(caps(14.0, 900.0).tracking(0.6)).color(paper()))
                .child(text(what.to_string()).font(sans(10.0, 600.0)).color(paper().alpha(0.85)).h(26.0))
                .child(body),
        );
    }
    el().w(w).h(h).child(background(s, w, h)).child(cols.pin(Some(0.0), Some(0.0), None, None))
}

// ------------------------------------------------------------------ speeds

/// The ways a speed can stand out, for the comparison sheet: (name, what it tries).
pub const SPEEDS: [(&str, &str); 7] = [
    ("Before", "a coloured underline"),
    ("A  Coloured figures", "heavy italic, cyan down, pink up"),
    ("B  Arrow chip", "a small coloured ↓ / ↑ box"),
    ("C  Black pill", "a slanted pill with a coloured rim"),
    ("D  Gauge", "the underline's length is the speed"),
    ("E  Size by speed", "faster is bigger"),
    ("F  Coloured slab", "black figures on a cyan / pink slab"),
];

/// A speed in MiB/s, from its label, for the styles that scale with it.
fn mib(v: &str) -> f32 {
    let n: f32 = v.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    if v.contains("KiB") { n / 1024.0 } else { n }
}

/// The speed `v` (down or up) in `style`, in the cell's text colour `ink`.
fn speed_cell(style: u8, v: &str, down: bool, ink: Color, t: f32) -> El {
    let (dc, uc) = speed_colours();
    let c = if down { dc } else { uc };
    let arrow = if down { "↓" } else { "↑" };
    let fig = |size: f32| figures(size);
    match style {
        1 => text(v.to_owned()).font(fig(14.0).italic()).color(c),
        2 => row()
            .gap(3.0)
            .items_center()
            .child(el().fill(c).px(2.5).child(text(arrow.to_owned()).font(caps(11.0, 900.0)).color(black())))
            .child(text(v.to_owned()).font(fig(12.5)).color(ink)),
        3 => {
            let (pw, ph) = (68.0, 18.0);
            let pill = Shape::new()
                .fill(inflate(&[[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], 1.5), c)
                .fill(vec![[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], black());
            el().size(pw, ph).center().child(layer(pill, pw, ph)).child(text(v.to_owned()).font(fig(12.5)).color(paper()))
        }
        4 => {
            let len = (mib(v) / 70.0).clamp(0.06, 1.0) * 58.0;
            let u = Shape::new().fill(vec![[58.0 - len + 3.0, 0.0], [58.0, 0.0], [57.0, 3.0], [58.0 - len, 3.0]], c);
            col()
                .items_end()
                .gap(1.0)
                .child(text(v.to_owned()).font(fig(12.5)).color(ink))
                .child(el().size(58.0, 4.0).child(layer(u, 58.0, 4.0)))
        }
        5 => {
            let size = 11.5 + (mib(v) / 70.0).clamp(0.0, 1.0).sqrt() * 7.0;
            text(v.to_owned()).font(fig(size).italic()).color(ink)
        }
        6 if jose3() => {
            // Jose's counter plate: near-black, see-through, an italic cream figure over a thin
            // underline in the direction's colour; its corners still drift.
            let (pw, ph) = (70.0, 20.0);
            let corners = living(pw - 8.0, ph - 4.0, 1.5, t, key_seed(v) ^ down as u32);
            let plate = Shape::new().fill(moved(&corners, 4.0, 2.0), hex("#05081a").alpha(0.82)).line(
                [10.0, ph - 4.0],
                [pw - 8.0, ph - 4.0],
                1.2,
                LineCap::Butt,
                c.alpha(0.9),
            );
            el().size(pw, ph).center().child(layer(plate, pw, ph)).child(text(v.to_owned()).font(fig(13.5).italic()).color(c).mb(2.0))
        }
        6 => {
            let (pw, ph) = (66.0, 18.0);
            // Its corners never rest, as the game's highlighted shapes.
            let corners = living(pw - 8.0, ph - 4.0, 2.5, t, key_seed(v) ^ down as u32);
            let slab = Shape::new().fill(moved(&corners, 4.0, 2.0), c);
            el().size(pw, ph).center().child(layer(slab, pw, ph)).child(text(v.to_owned()).font(fig(13.0)).color(black()))
        }
        _ => col_el(v, ink, c),
    }
}

/// Every speed style on the same rows, on the black sheet.
pub fn speeds_sheet(s: &S, w: f32, h: f32) -> El {
    let samples = [("68 MiB/s", "–"), ("–", "1.5 MiB/s"), ("4.2 MiB/s", "320 KiB/s")];
    let mut list = col().gap(4.0).padding(20.0).pt(16.0);
    for (k, (name, what)) in SPEEDS.iter().enumerate() {
        let mut r = row().gap(14.0).items_center().h(58.0).px(12.0);
        r = r.child(
            col()
                .w(220.0)
                .child(text(name.to_string()).font(caps(14.0, 900.0).tracking(0.6)).color(paper()))
                .child(text(what.to_string()).font(sans(10.5, 600.0)).color(paper().alpha(0.8))),
        );
        for (d, u) in samples {
            let cell = |v: &str, down: bool| {
                let mut c = row().w(80.0).justify_end();
                c = c.child(if v == "–" {
                    text("–".to_owned()).font(figures(12.5)).color(hex("#a8a8a8"))
                } else {
                    speed_cell(k as u8, v, down, hex("#e2e2e2"), s.t)
                });
                c
            };
            r = r.child(row().gap(6.0).child(cell(d, true)).child(cell(u, false)).px(6.0));
        }
        list = list.child(
            el().child(layer(Shape::new().fill(vec![[0.0, 0.0], [w - 40.0, 0.0], [w - 44.0, 58.0], [4.0, 58.0]], black()), w - 40.0, 58.0))
                .child(r),
        );
    }
    let heads = row()
        .pl(20.0 + 12.0 + 220.0 + 14.0 + 6.0)
        .gap(14.0)
        .pt(10.0)
        .child(text("DOWN  ·  UP, THREE TORRENTS".to_owned()).font(caps(12.0, 900.0).tracking(2.0)).color(black()));
    el().w(w).h(h).child(background(s, w, h)).child(col().child(heads).child(list).pin(Some(0.0), Some(0.0), None, None))
}

// ------------------------------------------------------------------ progress tracks

/// How a progress bar sits on the black sheet, for the comparison sheet: (name, what it tries).
/// Round 1 (styles 0–5: before, A–E) is in `out/v/297ec2c-tracks.png`, round 2 (A, B and D
/// leaning) in `out/v/abbd9f0-tracks.png`, round 3 (the fill bent into the lean) in
/// `out/v/4ca7c13-tracks.png`, round 4 (top and bottom level) in `out/v/ec6ae2e-tracks.png`;
/// round 5 keeps B's patterns straight.
pub const TRACKS: [(u8, &str, &str); 3] = [
    (2, "B  Straight", "for reference"),
    (24, "B  Straight patterns", "round 5: one shadow for all"),
    (19, "B  Straight patterns", "red shadow on error, none seeding"),
];

/// Every track style with all six statuses, on the black sheet.
pub fn tracks_sheet(s: &S, w: f32, h: f32) -> El {
    let mut list = col().gap(6.0).padding(20.0).pt(14.0);
    for (style, name, what) in TRACKS {
        let mut st = S::new();
        st.t = s.t;
        st.track_style = style;
        st.dir = s.dir;
        let mut bars = col().gap(8.0);
        for half in TORRENTS.chunks(3) {
            let mut r = row().gap(18.0).items_center();
            for t in half {
                r = r.child(progress(&st, t, 200.0));
            }
            bars = bars.child(r);
        }
        let r = row()
            .gap(14.0)
            .items_center()
            .h(70.0)
            .px(14.0)
            .child(
                col()
                    .w(160.0)
                    .child(text(name.to_string()).font(caps(14.0, 900.0).tracking(0.6)).color(paper()))
                    .child(text(what.to_string()).font(sans(10.5, 600.0)).color(paper().alpha(0.8))),
            )
            .child(bars);
        list = list.child(
            el().child(layer(Shape::new().fill(vec![[0.0, 0.0], [w - 40.0, 0.0], [w - 44.0, 70.0], [4.0, 70.0]], black()), w - 40.0, 70.0))
                .child(r),
        );
    }
    let heads = row().pl(20.0 + 14.0 + 160.0 + 14.0).pt(10.0).child(
        text("DOWNLOADING · SEEDING · CHECKING · PAUSED · FETCHING · ERROR".to_owned())
            .font(caps(12.0, 900.0).tracking(1.5))
            .color(black()),
    );
    el().w(w).h(h).child(background(s, w, h)).child(col().child(heads).child(list).pin(Some(0.0), Some(0.0), None, None))
}

/// A smooth 0..1 swing at the living corners' pace, its speed and phase from `seed`.
fn wave(t: f32, seed: u32) -> f32 {
    let speed = 2.2 + 2.4 * noise(seed.wrapping_mul(29));
    let phase = noise(seed.wrapping_mul(29).wrapping_add(40)) * std::f32::consts::TAU;
    0.5 + 0.5 * (t * speed + phase).sin()
}

// ------------------------------------------------------------------ straight patterns

/// Whether `p` is inside the convex polygon `poly` (either winding).
fn inside(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
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
fn clip_segment(a: [f32; 2], b: [f32; 2], poly: &[[f32; 2]]) -> Option<([f32; 2], [f32; 2])> {
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

/// A progress fill in the level, leaning track `quad` whose patterns stay straight: stripes,
/// chevrons, dots and blocks are drawn as on the straight bar and cut by the leaning edges, not
/// stretched. With `front_leans` the fill's front edge leans with the track's sides; without, it
/// stands upright. Returns the shape, the percent's ink and the label in place of a percent.
#[allow(clippy::too_many_arguments)]
fn straight_fill(
    s: &S,
    t: &Torrent,
    mut sh: Shape,
    quad: &[[f32; 2]],
    bw: f32,
    h: f32,
    fw: f32,
    front_leans: bool,
) -> (Shape, Color, Option<String>) {
    // Where the track's left edge is at height y, and how a point moves onto the bar.
    let left = |y: f32| quad[0][0] + (quad[3][0] - quad[0][0]) * (y / h);
    let right = |y: f32| quad[1][0] + (quad[2][0] - quad[1][0]) * (y / h);
    let place = |p: [f32; 2]| -> [f32; 2] {
        let (l, r) = (left(p[1]), right(p[1]));
        if front_leans {
            [l + p[0] / bw * (r - l), p[1]]
        } else if p[0] <= 0.01 {
            [l, p[1]]
        } else {
            p
        }
    };
    let region = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> { pts.into_iter().map(place).collect() };
    let mut ink = black();
    let mut label = None;
    match t.kind {
        Kind::Downloading => {
            let body = (fw - 8.0).max(0.0);
            sh = sh.fill(region(vec![[0.0, 0.0], [body, 0.0], [fw + 4.0, h / 2.0], [body, h], [0.0, h]]), red());
            // The stripes, straight, over the body less its last 4 pt.
            let clip = region(vec![[0.0, 0.0], [(body - 4.0).max(0.0), 0.0], [(body - 4.0).max(0.0), h], [0.0, h]]);
            let pitch = 9.0;
            let mut x = -20.0 - h + (s.t * 26.0).rem_euclid(pitch);
            while x < bw + 10.0 {
                if let Some((a, b)) = clip_segment([x, h], [x + h, 0.0], &clip) {
                    sh = sh.line(a, b, 3.0, LineCap::Butt, paper().alpha(0.85));
                }
                x += pitch;
            }
            ink = red();
        }
        Kind::Seeding => {
            let area = region(vec![[0.0, 0.0], [fw, 0.0], [fw, h], [0.0, h]]);
            sh = sh.fill(area.clone(), paper());
            let pitch = 12.0;
            let mut x = -24.0 + (s.t * 10.0).rem_euclid(pitch);
            while x < fw {
                let v = vec![[x, 2.5], [x + 4.5, h / 2.0], [x, h - 2.5]];
                if v.iter().all(|&p| inside(&area, [p[0] - 1.5, p[1]]) && inside(&area, [p[0] + 1.5, p[1]])) {
                    sh = sh.stroke(v, 2.4, red());
                }
                x += pitch;
            }
        }
        Kind::Checking => {
            sh = sh.fill(region(vec![[0.0, 0.0], [fw, 0.0], [(fw - 5.0).max(0.0), h], [0.0, h]]), paper());
            let dots = region(vec![[0.0, 0.0], [(fw - 4.0).max(0.0), 0.0], [(fw - 4.0).max(0.0), h], [0.0, h]]);
            let pitch = 4.0;
            for j in 0..=((h / (pitch * 0.5)) as i32 + 1) {
                let y = j as f32 * pitch * 0.5;
                let shift = if j % 2 == 0 { 0.0 } else { pitch * 0.5 };
                let mut x = -12.0 + shift;
                while x < fw {
                    let r = (0.4 + 1.2 * (x.max(0.0) / fw)).min(pitch * 0.72);
                    if inside(&dots, [x, y]) {
                        sh = sh.fill(circle(x, y, r), black());
                    }
                    x += pitch;
                }
            }
            let sx = place([(s.t * 0.9).fract() * fw, h / 2.0])[0];
            sh = sh.fill(vec![[sx, -2.0], [sx + 4.0, -2.0], [sx + 1.0, h + 2.0], [sx - 3.0, h + 2.0]], red());
        }
        Kind::Paused => {
            let area = region(vec![[0.0, 0.0], [fw, 0.0], [fw, h], [0.0, h]]);
            let mut x = -10.0;
            while x < fw + 10.0 {
                let v = vec![[x + 2.0, 1.5], [x + 7.0, 1.5], [x + 5.0, h - 1.5], [x, h - 1.5]];
                if v.iter().all(|&p| inside(&area, p)) {
                    sh = sh.fill(v, hex("#9a9a9a"));
                }
                x += 10.0;
            }
            ink = hex("#777777");
        }
        Kind::Error => {
            let mut torn = vec![[0.0, 0.0]];
            for k in 0..5 {
                torn.push([fw + (noise(50 + k) - 0.3) * 8.0, h * k as f32 / 4.0]);
            }
            torn.push([0.0, h]);
            let dx = place([fw, h / 2.0])[0] - fw;
            sh = sh.fill(region(torn), error_c());
            sh = sh.stroke(moved(&[[fw + 6.0, 0.0], [fw + 12.0, 6.0], [fw + 9.0, 8.0], [fw + 16.0, h]], dx, 0.0), 1.6, error_c());
            ink = error_c();
        }
        Kind::Fetching => {
            let x = (s.t * 2.4).sin() * 0.5 + 0.5;
            let sw = 22.0;
            let sx = place([x * (bw - sw) + sw / 2.0, h / 2.0])[0] - sw / 2.0;
            sh = sh.fill(vec![[sx + 5.0, 1.0], [sx + sw, 1.0], [sx + sw - 5.0, h - 1.0], [sx, h - 1.0]], paper());
            label = Some("??".to_owned());
        }
    }
    (sh, ink, label)
}

// ------------------------------------------------------------------ the angled window (spike)

/// How far the angled window's left edge leans over its height, in points.
pub const SLANT: f32 = 56.0;

/// The angled window's shape in a `w` × `h` window: a slanted four-sided sheet, 4 pt in from the
/// window's edges, the corners outside it clear.
pub fn angled_quad(w: f32, h: f32) -> Vec<[f32; 2]> {
    vec![[SLANT + 4.0, 4.0], [w - 4.0, 4.0], [w - SLANT - 4.0, h - 4.0], [4.0, h - 4.0]]
}

/// The window as an angled sheet: the theme's background cut to the slanted shape with a paper
/// rim, `content` (the usual window, without its background) centred on it, and a note of the
/// presses the window has seen.
pub fn angled_frame(s: &S, w: f32, h: f32, content: El, cw: f32) -> El {
    if s.edge_style > 0 {
        return edged_frame(s, w, h, content, cw);
    }
    let quad = angled_quad(w, h);
    let mut sh = Shape::new().fill(inflate(&quad, 3.0), paper()).fill(quad.clone(), red());
    let (cx, cy) = (w - 110.0, 18.0);
    let n = 18;
    for i in 0..n {
        let a0 = s.t * 0.12 + i as f32 * std::f32::consts::TAU / n as f32;
        let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.5;
        let r = 1600.0;
        let ray = clip_convex(&[[cx, cy], [cx + a0.cos() * r, cy + a0.sin() * r], [cx + a1.cos() * r, cy + a1.sin() * r]], &quad);
        if ray.len() >= 3 {
            sh = sh.fill(ray, deep());
        }
    }
    let note = format!("PRESSES SEEN: {}  ·  LAST AT {:.0}, {:.0}", s.presses, s.last_press[0], s.last_press[1]);
    el().w(w)
        .h(h)
        .child(layer(sh, w, h))
        .child(content.pin(Some((w - cw) / 2.0), Some(0.0), None, None))
        .child(text(note).font(caps(10.0, 900.0).tracking(1.0)).color(paper()).pin(Some(SLANT + 96.0), Some(16.0), None, None))
}

// ------------------------------------------------------------------ the Jose palette

/// Jose's night sky: navy, a scatter of stars that twinkle, and a few faint arcs sweeping past.
fn night_sky(s: &S, w: f32, h: f32) -> El {
    if jose6() {
        return night_sky_v6(s, w, h);
    }
    if jose5() {
        return night_sky_v5(s, w, h);
    }
    if jose4() {
        return night_sky_v4(s, w, h);
    }
    if jose3() {
        return night_sky_vivid(s, w, h);
    }
    let mut sh = Shape::new().fill(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], hex("#0a1a4a"));
    // A lighter band low across the sky.
    sh = sh.fill(vec![[0.0, h * 0.55], [w, h * 0.35], [w, h * 0.7], [0.0, h * 0.95]], hex("#13306f").alpha(0.7));
    // Arcs, turning slowly.
    for k in 0..4 {
        let r = 500.0 + k as f32 * 140.0;
        let (cx, cy) = (w * 0.15, h + 380.0);
        let a0 = -1.9 + k as f32 * 0.15 + (s.t * 0.05 + k as f32).sin() * 0.05;
        let pts: Vec<[f32; 2]> = (0..=40)
            .map(|i| {
                let a = a0 + i as f32 * 0.025;
                [cx + a.cos() * r, cy + a.sin() * r]
            })
            .collect();
        let c = if k % 2 == 0 { hex("#7fe7ff") } else { hex("#ffe86a") };
        sh = sh.stroke(pts, 1.6, c.alpha(0.45));
    }
    // Stars: small dots, and a few four-point sparkles in yellow.
    for i in 0..160u32 {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h);
        let tw = 0.5 + 0.5 * (s.t * (1.0 + 2.0 * noise(i * 3 + 3)) + i as f32).sin();
        if i % 16 == 0 {
            let r = 5.0 + 3.0 * tw;
            sh = sh.fill(
                vec![
                    [x, y - r],
                    [x + r * 0.22, y - r * 0.22],
                    [x + r, y],
                    [x + r * 0.22, y + r * 0.22],
                    [x, y + r],
                    [x - r * 0.22, y + r * 0.22],
                    [x - r, y],
                    [x - r * 0.22, y - r * 0.22],
                ],
                hex("#ffe14a"),
            );
        } else {
            sh = sh.fill(circle(x, y, 0.6 + 0.8 * tw), hex("#ffffff").alpha(0.5 + 0.5 * tw));
        }
    }
    layer(sh, w, h)
}

// ------------------------------------------------------------------ shaped edges

// The rectangular window's edge shapes, after the slanted spike: 1 cut corners, 2 a slight
// lean, 3 torn paper, 4 a title tab, 5 cut corners with teeth along the bottom. Each stays inside
// the window's rectangle, at most `EDGE` in from it.

/// How far in from the window's rectangle the shaped edge may reach.
pub const EDGE: f32 = 12.0;

/// The window's outline in `w` × `h` for edge style `style` (1–5).
fn edge_outline(style: u8, w: f32, h: f32) -> Vec<[f32; 2]> {
    let (a, b) = (4.0, 4.0);
    let (r, d) = (w - 4.0, h - 4.0);
    match style {
        1 => vec![[a + 28.0, b], [r, b], [r, d - 28.0], [r - 28.0, d], [a, d], [a, b + 28.0]],
        2 => vec![[a + EDGE, b], [r, b], [r - EDGE, d], [a, d]],
        3 => jagged(a + 4.0, b + 4.0, r - a - 8.0, d - b - 8.0, 5.0, 22.0, 911),
        4 => vec![[a, b], [a + 230.0, b], [a + 244.0, b + 10.0], [r, b + 10.0], [r, d], [a, d]],
        _ => {
            let mut pts = vec![[a + 28.0, b], [r, b], [r, d - 28.0], [r - 28.0, d]];
            // Teeth along the bottom, right to left.
            let n = ((r - 28.0 - a) / 16.0) as usize;
            for k in 1..n {
                let x = r - 28.0 - k as f32 * 16.0;
                pts.push([x + 8.0, d - if k % 2 == 1 { 9.0 } else { 0.0 }]);
                pts.push([x, d]);
            }
            pts.push([a, d]);
            pts.push([a, b + 28.0]);
            pts
        }
    }
}

/// The window with a shaped edge: the outline in black over a paper rim, the theme's
/// background inside, and `content` (the window's usual content, `cw` wide) set in by `EDGE`.
fn edged_frame(s: &S, w: f32, h: f32, content: El, cw: f32) -> El {
    let outline = edge_outline(s.edge_style, w, h);
    let mut sh =
        Shape::new().fill(moved(&outline, 4.0, 4.0), black().alpha(0.6)).fill(inflate(&outline, 2.5), paper()).fill(outline.clone(), red());
    // The rays, cut to the rectangle inside the edge so a rough or stepped edge stays solid red.
    let inner = vec![[EDGE, EDGE], [w - EDGE, EDGE], [w - EDGE, h - EDGE], [EDGE, h - EDGE]];
    let (cx, cy) = (w - 110.0, 18.0);
    let n = 18;
    for i in 0..n {
        let a0 = s.t * 0.12 + i as f32 * std::f32::consts::TAU / n as f32;
        let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.5;
        let r = 1600.0;
        let ray = clip_convex(&[[cx, cy], [cx + a0.cos() * r, cy + a0.sin() * r], [cx + a1.cos() * r, cy + a1.sin() * r]], &inner);
        if ray.len() >= 3 {
            sh = sh.fill(ray, deep());
        }
    }
    el().w(w).h(h).child(layer(sh, w, h)).child(content.pin(Some((w - cw) / 2.0), Some(EDGE), None, None))
}

/// A crescent moon of radius `r` at (`cx`, `cy`), its back turned by `turn` radians.
fn crescent(cx: f32, cy: f32, r: f32, turn: f32) -> Vec<[f32; 2]> {
    let n = 18;
    let mut pts = Vec::new();
    for i in 0..=n {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
        pts.push([a.cos() * r, a.sin() * r]);
    }
    for i in (0..=n).rev() {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
        pts.push([a.cos() * r * 0.35, a.sin() * r * 0.92]);
    }
    let (c, sn) = (turn.cos(), turn.sin());
    pts.into_iter().map(|[x, y]| [cx + x * c - y * sn, cy + x * sn + y * c]).collect()
}

/// A glow: rings of `color` fading out from radius `r` at (`x`, `y`).
fn glow(mut sh: Shape, x: f32, y: f32, r: f32, color: Color, strength: f32) -> Shape {
    for k in (1..=4).rev() {
        sh = sh.fill(circle(x, y, r * (1.0 + 0.35 * k as f32)), color.alpha(strength * 0.06 * (5 - k) as f32));
    }
    sh
}

/// Jose's sky as the footage has it: vivid blue to teal, glowing stars, moons and arcs.
fn night_sky_vivid(s: &S, w: f32, h: f32) -> El {
    // Bands from deep blue at the top to a bright blue-teal low down, standing in for a gradient.
    let bands = ["#081a63", "#0b2580", "#0e3299", "#1242b3", "#1657c4", "#1a6fcc", "#1e88cf"];
    let mut sh = Shape::new();
    for (k, c) in bands.iter().enumerate() {
        let (y0, y1) = (h * k as f32 / bands.len() as f32, h * (k + 1) as f32 / bands.len() as f32 + 1.0);
        sh = sh.fill(vec![[0.0, y0], [w, y0 - 30.0], [w, y1 - 30.0], [0.0, y1]], hex(c));
    }
    // Arcs sweeping across, glowing.
    for k in 0..5 {
        let r = 480.0 + k as f32 * 120.0;
        let (cx, cy) = (w * 0.1, h + 360.0);
        let a0 = -1.95 + k as f32 * 0.13 + (s.t * 0.05 + k as f32).sin() * 0.05;
        let pts: Vec<[f32; 2]> = (0..=44)
            .map(|i| {
                let a = a0 + i as f32 * 0.025;
                [cx + a.cos() * r, cy + a.sin() * r]
            })
            .collect();
        let c = if k % 2 == 0 { hex("#8ff4ff") } else { hex("#fff07a") };
        sh = sh.stroke(pts.clone(), 6.0, c.alpha(0.12)).stroke(pts, 1.8, c.alpha(0.7));
    }
    // Stars, every one with a halo; the big ones are yellow sparkles.
    for i in 0..170u32 {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h);
        let tw = 0.5 + 0.5 * (s.t * (1.0 + 2.0 * noise(i * 3 + 3)) + i as f32).sin();
        if i % 14 == 0 {
            let r = 6.0 + 3.0 * tw;
            sh = glow(sh, x, y, r * 0.8, hex("#ffe14a"), 1.0);
            sh = sh.fill(
                vec![
                    [x, y - r],
                    [x + r * 0.22, y - r * 0.22],
                    [x + r, y],
                    [x + r * 0.22, y + r * 0.22],
                    [x, y + r],
                    [x - r * 0.22, y + r * 0.22],
                    [x - r, y],
                    [x - r * 0.22, y - r * 0.22],
                ],
                hex("#fff27a"),
            );
        } else {
            sh = sh.fill(circle(x, y, 2.2 + 1.5 * tw), hex("#bff6ff").alpha(0.12 + 0.12 * tw));
            sh = sh.fill(circle(x, y, 0.7 + 0.8 * tw), hex("#ffffff").alpha(0.6 + 0.4 * tw));
        }
    }
    // Glowing moons along the bottom and the far edges, bobbing.
    let moons =
        [(0.04, 0.92, 46.0, 0.6), (0.17, 1.02, 34.0, -0.3), (0.63, 1.0, 40.0, 0.9), (0.97, 0.78, 52.0, 2.6), (0.88, 1.03, 30.0, 1.8)];
    for (k, (fx, fy, r, turn)) in moons.iter().enumerate() {
        let bob = (s.t * 0.8 + k as f32 * 1.7).sin() * 3.0;
        let (x, y) = (fx * w, fy * h + bob);
        sh = glow(sh, x, y, *r, hex("#ffe14a"), 1.3);
        sh = sh.fill(crescent(x, y, *r, *turn + (s.t * 0.3 + k as f32).sin() * 0.05), hex("#ffe94d"));
    }
    layer(sh, w, h)
}

// ------------------------------------------------------------------ Jose, round 4

/// A five-point star of radius `r` at (`x`, `y`), turned by `turn`.
fn star5(x: f32, y: f32, r: f32, turn: f32) -> Vec<[f32; 2]> {
    (0..10)
        .map(|i| {
            let a = turn - std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            [x + a.cos() * rr, y + a.sin() * rr]
        })
        .collect()
}

/// A comet's body along the circle of radius `r` round (`cx`, `cy`), from angle `a0` (its tail)
/// to `a1` (its head), widening to `width` at the head.
fn comet(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, width: f32) -> Vec<[f32; 2]> {
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

/// Jose's sky, round 4: a smooth blue gradient (blurred bands), comets sweeping along their arcs,
/// and a drift of moons and stars along the bottom, all glowing through a real blur.
fn night_sky_v4(s: &S, w: f32, h: f32) -> El {
    let m = 120.0;
    let (bw, bh) = (w + 2.0 * m, h + 2.0 * m);
    // The gradient: bands, blurred so their steps vanish.
    let bands = ["#06155a", "#081c6e", "#0b2685", "#0f339c", "#1443b4", "#1857c2", "#1c6fc9", "#2088cc"];
    let mut sky = Shape::new();
    for (k, c) in bands.iter().enumerate() {
        let (y0, y1) = (bh * k as f32 / bands.len() as f32, bh * (k + 1) as f32 / bands.len() as f32 + 1.0);
        sky = sky.fill(vec![[0.0, y0], [bw, y0 - 30.0], [bw, y1 - 30.0], [0.0, y1]], hex(c));
    }
    let sky = layer(sky, bw, bh).blur(36.0).pin(Some(-m), Some(-m), None, None);

    let mut glow = Shape::new();
    let mut sharp = Shape::new();
    // Comets: each arc carries one, sweeping left to right along it and round again.
    let (cx, cy) = (w * 0.1, h + 360.0);
    for k in 0..6 {
        let r = 470.0 + k as f32 * 105.0;
        let span = 1.25;
        let start = -2.0 + k as f32 * 0.1;
        let u = (s.t * (0.07 + 0.02 * noise(k * 7 + 1)) + noise(k * 7 + 2)).fract();
        let head = start + u * (span + 0.6);
        let tail = head - 0.55;
        let c = if k % 2 == 0 { hex("#8ff4ff") } else { hex("#fff07a") };
        glow = glow.fill(comet(cx, cy, r, tail, head, 9.0), c.alpha(0.55));
        sharp = sharp.fill(comet(cx, cy, r, tail, head, 2.4), c);
    }
    // Small stars, scattered, with a soft halo.
    for i in 0..150u32 {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h * 0.85);
        let tw = 0.5 + 0.5 * (s.t * (1.0 + 2.0 * noise(i * 3 + 3)) + i as f32).sin();
        glow = glow.fill(circle(x, y, 2.0 + 2.0 * tw), hex("#bff6ff").alpha(0.5 * tw));
        sharp = sharp.fill(circle(x, y, 0.6 + 0.8 * tw), hex("#ffffff").alpha(0.6 + 0.4 * tw));
    }
    // The drift along the bottom: moons and five-point stars, moving the comets' way.
    let drift = s.t * 18.0;
    for i in 0..22u32 {
        let lane = w + 240.0;
        let x = (noise(i * 5 + 1) * lane + drift * (0.7 + 0.6 * noise(i * 5 + 4))).rem_euclid(lane) - 120.0;
        // Lower at the left, rising a little to the right, as the arcs do.
        let y = h * (0.86 + 0.16 * noise(i * 5 + 2)) - (x / w) * 30.0;
        let spin = s.t * 0.4 * (noise(i * 5 + 3) - 0.5);
        if i % 3 == 0 {
            let r = 26.0 + 28.0 * noise(i * 5 + 3);
            glow = glow.fill(circle(x, y, r * 1.1), hex("#ffd60a").alpha(0.75));
            sharp = sharp.fill(crescent(x, y, r, 0.8 + spin + noise(i) * 2.0), hex("#ffe94d"));
        } else {
            let r = 9.0 + 14.0 * noise(i * 5 + 3);
            glow = glow.fill(circle(x, y, r * 1.2), hex("#ffd60a").alpha(0.6));
            sharp = sharp.fill(star5(x, y, r, spin * 2.0 + noise(i) * 3.0), hex("#fff07a"));
        }
    }
    el().w(w).h(h).clip_rounded(0.0).child(sky).child(layer(glow, w, h).blur(9.0)).child(layer(sharp, w, h))
}

/// A rounded rectangle `w` × `h` with corner radius `r`.
fn rounded(w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
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

/// The signboard styles, for the comparison sheet.
pub const SIGNS: [(&str, &str); 3] = [
    ("A  Yellow plates", "Items: yellow, black rim; others navy, yellow rim"),
    ("B  Glass plates", "Cognition: see-through, a cream rim"),
    ("C  Counter plates", "dark plates, an underline; the primary yellow"),
];

/// A button as one of Jose's signboards, in style `jose_sign()`. At rest it sways a little; on
/// hover it glows and lifts; on press it sinks and brightens.
fn signboard(s: &S, key: &str, label: &str, primary: bool, look: Look, since: f32) -> El {
    if jose5() {
        return signboard5(s, key, label, primary, look, since);
    }
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let plate = rounded(w, h, 6.0);
    let seed = key_seed(key);
    let active = matches!(look, Look::Hover | Look::Press);
    let press = look == Look::Press;
    let e = if press {
        1.0
    } else if active {
        snap(since, 0.23)
    } else {
        0.0
    };
    let yellow = hex("#ffd21f");
    let navy = hex("#0a1a62");
    let cream = paper();
    let (fill, rim, ink, under) = match (jose_sign(), primary) {
        (0, true) => (yellow, black(), black(), None),
        (0, false) => (navy.alpha(0.88), yellow, cream, None),
        (1, true) => (yellow.alpha(0.78), cream, navy, None),
        (1, false) => (hex("#1a3aa8").alpha(0.45), cream, cream, None),
        (_, true) => (hex("#05081a").alpha(0.85), yellow.alpha(0.0), yellow, Some(yellow)),
        (_, false) => (hex("#05081a").alpha(0.8), cream.alpha(0.0), cream, Some(cream)),
    };
    let (fill, ink) = if press && !primary { (yellow.alpha(0.9), navy) } else { (fill, ink) };
    let disabled = look == Look::Disabled;
    let mut sh = Shape::new();
    if rim.a > 0.0 {
        sh = sh.fill(inflate(&plate, 2.5), rim);
    }
    sh = sh.fill(plate.clone(), fill);
    if let Some(u) = under {
        sh = sh.line([10.0, h - 5.0], [w - 10.0, h - 5.0], 1.3, LineCap::Butt, u);
    }
    let glow = layer(Shape::new().fill(inflate(&plate, 4.0), yellow.alpha(0.9 * e)), w, h).blur(7.0);
    let font = TextStyle::new(14.5, 600.0).family(Family::Named(jose_family()));
    let font = if jose_sign() == 2 { font.italic() } else { font };
    let face = el()
        .size(w, h)
        .center()
        .child(layer(sh, w, h))
        .child(text(label.replace('…', "...")).font(font).color(ink).mb(if under.is_some() { 3.0 } else { 0.0 }));
    // At rest a slow sway, each sign its own; hover lifts and tilts it, press sinks it.
    let sway = (s.t * 1.3 + noise(seed) * 6.0).sin() * 0.012;
    let (scale, lift) = if press { (0.94, 1.0) } else { (1.0 + 0.06 * e, -2.0 * e) };
    let turn = sway + tilt(seed, 0.02, 0.04) * e;
    let mut b = el().size(w + 6.0, BAR_H).pt(3.0);
    if active {
        b = b.child(glow.pin(Some(0.0), Some(3.0), None, None));
    }
    let b = b.child(face.pin(Some(0.0), Some(3.0), None, None)).offset(0.0, lift).scale(scale).scale_motion(kick()).rotate(turn);
    if disabled { b.opacity(0.4) } else { b }
}

/// Each signboard style on Add and Open: idle, hover, pressed, disabled.
pub fn signs_sheet(s: &S, w: f32, h: f32) -> El {
    let mut list = col().gap(6.0).padding(20.0).pt(10.0);
    let signs = if s.morph_sheet && s.tilt_sheet {
        &SIGNS10
    } else if s.morph_sheet {
        &SIGNS9
    } else if s.fits_sheet {
        &SIGNS8
    } else if s.plates_sheet {
        &SIGNS7
    } else if jose6() {
        &SIGNS6
    } else if jose5() {
        &SIGNS5
    } else {
        &SIGNS
    };
    for (k, (name, what)) in signs.iter().enumerate() {
        if s.morph_sheet {
            set_jose_opt(Part::Morph, k as u8 + if s.tilt_sheet { 4 } else { 1 });
        } else if s.fits_sheet {
            set_jose_opt(Part::Hover, 0);
            set_jose_opt(Part::Plate, 2);
            set_jose_opt(Part::Fit, k as u8 + 1);
        } else if s.plates_sheet {
            set_jose_opt(Part::Hover, 0);
            set_jose_opt(Part::Plate, k as u8 + 1);
        } else if jose6() {
            set_jose_opt(Part::Hover, k as u8);
        } else {
            set_jose_sign(k as u8);
        }
        let mut r = row().gap(16.0).items_center().h(BAR_H + 50.0);
        // On a dark backing, so the heap behind doesn't swallow the labels.
        r = r.child(
            col()
                .w(230.0)
                .padding(6.0)
                .fill(hex("#020a2c").alpha(0.78))
                .child(text(name.to_string()).font(caps(14.0, 700.0)).color(paper()))
                .child(text(what.to_string()).font(sans(10.5, 500.0)).color(paper().alpha(0.85))),
        );
        for (n, look) in [Look::Idle, Look::Hover, Look::Press, Look::Disabled].into_iter().enumerate() {
            let mut c = row().gap(8.0);
            for (key, label, primary) in [("add", "Add", true), ("open", "Open…", false)] {
                c = c.child(signboard(s, &format!("{key}-{k}-{n}"), label, primary, look, 1.0));
            }
            r = r.child(c);
        }
        list = list.child(r);
    }
    set_jose_sign(0);
    set_jose_opt(Part::Plate, 0);
    set_jose_opt(Part::Fit, 0);
    set_jose_opt(Part::Morph, 0);
    let mut heads = row().pl(20.0 + 230.0 + 16.0).gap(16.0).pt(8.0);
    for t in ["IDLE", "HOVER", "PRESSED", "DISABLED"] {
        heads = heads.child(
            text(t.to_owned()).font(caps(12.0, 700.0).tracking(1.5)).color(if jose6() { jose_yellow() } else { hex("#ffd21f") }).w(150.0),
        );
    }
    el().w(w).h(h).child(background(s, w, h)).child(col().child(heads).child(list).pin(Some(0.0), Some(0.0), None, None))
}

// ------------------------------------------------------------------ Jose, round 5

/// `pts` with every corner rounded off by up to `rad`, concave corners as well as convex.
fn round_poly(pts: &[[f32; 2]], rad: f32) -> Vec<[f32; 2]> {
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

/// A plump five-point star, rounded at its tips and its inner corners, wider-angled than
/// [`star5`].
fn star_soft(x: f32, y: f32, r: f32, turn: f32) -> Vec<[f32; 2]> {
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
fn moon_pieces(cx: f32, cy: f32, r: f32, turn: f32) -> Vec<Vec<[f32; 2]>> {
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

/// A deep C-shaped moon, as Jose's: the disc of radius `r` less a disc offset towards its open
/// side, which `turn` points.
fn moon_c(cx: f32, cy: f32, r: f32, turn: f32) -> Vec<[f32; 2]> {
    let (ri, d) = (0.8 * r, 0.4 * r);
    // Where the two circles cross, on the outer circle and the inner.
    let xo = (r * r - ri * ri + d * d) / (2.0 * d);
    let ao = (xo / r).clamp(-1.0, 1.0).acos();
    let ai = ((xo - d) / ri).clamp(-1.0, 1.0).acos();
    let n = 28;
    let mut pts = Vec::new();
    for k in 0..=n {
        let a = ao + (std::f32::consts::TAU - 2.0 * ao) * k as f32 / n as f32;
        pts.push([a.cos() * r, a.sin() * r]);
    }
    // The inner arc skips its two ends: they are the horn tips already on the outer arc, and a
    // repeated point leaves Rune's ear-clipper no ear, so it fans the moon into a disk.
    for k in 1..n {
        let a = std::f32::consts::TAU - ai - (std::f32::consts::TAU - 2.0 * ai) * k as f32 / n as f32;
        pts.push([d + a.cos() * ri, a.sin() * ri]);
    }
    let (c, sn) = (turn.cos(), turn.sin());
    pts.into_iter().map(|[x, y]| [cx + x * c - y * sn, cy + x * sn + y * c]).collect()
}

/// `pts` scaled by `k` about (`x`, `y`).
fn grown(pts: &[[f32; 2]], x: f32, y: f32, k: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|p| [x + (p[0] - x) * k, y + (p[1] - y) * k]).collect()
}

/// Jose's sky, round 5: round 4's gradient and comets, with the bottom and the corners heaped
/// with C-moons and soft stars drifting the comets' way, each glowing in its own shape.
fn night_sky_v5(s: &S, w: f32, h: f32) -> El {
    let m = 120.0;
    let (bw, bh) = (w + 2.0 * m, h + 2.0 * m);
    let bands = ["#06155a", "#081c6e", "#0b2685", "#0f339c", "#1443b4", "#1857c2", "#1c6fc9", "#2088cc"];
    let mut sky = Shape::new();
    for (k, c) in bands.iter().enumerate() {
        let (y0, y1) = (bh * k as f32 / bands.len() as f32, bh * (k + 1) as f32 / bands.len() as f32 + 1.0);
        sky = sky.fill(vec![[0.0, y0], [bw, y0 - 30.0], [bw, y1 - 30.0], [0.0, y1]], hex(c));
    }
    let sky = layer(sky, bw, bh).blur(36.0).pin(Some(-m), Some(-m), None, None);

    let mut glow = Shape::new();
    let mut sharp = Shape::new();
    let (cx, cy) = (w * 0.1, h + 360.0);
    for k in 0..6 {
        let r = 470.0 + k as f32 * 105.0;
        let start = -2.0 + k as f32 * 0.1;
        let u = (s.t * (0.07 + 0.02 * noise(k * 7 + 1)) + noise(k * 7 + 2)).fract();
        let head = start + u * 1.85;
        let tail = head - 0.55;
        let c = if k % 2 == 0 { hex("#8ff4ff") } else { hex("#fff07a") };
        glow = glow.fill(comet(cx, cy, r, tail, head, 8.0), c.alpha(0.5));
        sharp = sharp.fill(comet(cx, cy, r, tail, head, 2.4), c);
    }
    for i in 0..170u32 {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h * 0.85);
        let tw = 0.5 + 0.5 * (s.t * (1.0 + 2.0 * noise(i * 3 + 3)) + i as f32).sin();
        glow = glow.fill(circle(x, y, 1.6 + 1.6 * tw), hex("#bff6ff").alpha(0.5 * tw));
        sharp = sharp.fill(circle(x, y, 0.6 + 0.8 * tw), hex("#ffffff").alpha(0.6 + 0.4 * tw));
    }
    // The heap: a crowded drift along the bottom, piled higher at both corners, moving the
    // comets' way. Far ones first, so the near ones overlap them.
    let drift = s.t * 16.0;
    let lane = w + 260.0;
    let mut items: Vec<(f32, f32, f32, f32, bool)> = Vec::new();
    for i in 0..90u32 {
        let x = (noise(i * 5 + 1) * lane + drift * (0.75 + 0.5 * noise(i * 5 + 4))).rem_euclid(lane) - 130.0;
        // Higher towards the corners: a valley in the middle.
        let edge = ((x / w - 0.5).abs() * 2.0).powf(2.2);
        let depth = noise(i * 5 + 2);
        let y = h - 10.0 - depth * (70.0 + 170.0 * edge) - (x / w) * 20.0;
        let spin = s.t * 0.35 * (noise(i * 5 + 3) - 0.5) + noise(i * 5 + 6) * 6.0;
        let moon = i % 3 == 0;
        let r = if moon { 22.0 + 30.0 * noise(i * 5 + 3) } else { 8.0 + 16.0 * noise(i * 5 + 3) };
        items.push((x, y, r, spin, moon));
    }
    items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let (body, light, halo) = (hex("#ffd60a"), hex("#fff3a0"), hex("#ffb400"));
    for (x, y, r, spin, moon) in items {
        let (shape, hi) = if moon {
            let c = moon_c(x, y, r, spin);
            (c, Vec::new())
        } else {
            (star_soft(x, y, r, spin), star_soft(x, y - r * 0.06, r * 0.6, spin))
        };
        glow = glow.fill(grown(&shape, x, y, 1.12), halo.alpha(0.85));
        sharp = sharp.fill(shape, body);
        if !hi.is_empty() {
            sharp = sharp.fill(hi, light.alpha(0.55));
        }
    }
    el().w(w).h(h).clip_rounded(0.0).child(sky).child(layer(glow, w, h).blur(7.0)).child(layer(sharp, w, h))
}

/// The round 5 signboards, for the comparison sheet.
pub const SIGNS5: [(&str, &str); 3] = [
    ("A  Navy glass", "idle: cream frame, navy tint; hover: Items"),
    ("B  Amber glass", "idle: cream frame, dark gold tint; hover: Items"),
    ("C  A, Add stays yellow", "the primary always an Items board"),
];

/// A button as the shop's boards: at rest a cream frame round a tinted glass (Cognition); on
/// hover or press, the yellow board with a black inner rim and a cream frame askew behind it
/// (Items). Style by `jose_sign()`.
fn signboard5(s: &S, key: &str, label: &str, primary: bool, look: Look, since: f32) -> El {
    signboard5_as(s, key, label, primary, look, since)
}

fn signboard5_as(s: &S, key: &str, label: &str, primary: bool, look: Look, since: f32) -> El {
    let w = button_w(label) + 6.0;
    let h = BAR_H - 6.0;
    let seed = key_seed(key);
    let press = look == Look::Press;
    // Just left: one board winds the hover down over 0.2 s (sheet, rims, glow and lift fading,
    // the label turning back to cream). Round 10 laid a fading copy of the hovered board over
    // the idle one; the two sat at different scales and tilts, so the label doubled, and the
    // swap back to one board at the end showed as a flicker.
    let leaving = if look == Look::Idle && jose6() {
        s.left.as_ref().filter(|(k, _)| k == key).map(|(_, at)| s.t - at).filter(|d| *d < 0.2).map(|d| (1.0 - snap(d, 0.2)).clamp(0.0, 1.0))
    } else {
        None
    };
    let active = matches!(look, Look::Hover | Look::Press);
    let e = if press {
        1.0
    } else if let Some(f) = leaving {
        f
    } else if active {
        snap(since, 0.23)
    } else {
        0.0
    };
    let yellow = if jose6() { jose_yellow() } else { hex("#ffd21f") };
    let cream = paper();
    let closed = |p: Vec<[f32; 2]>| p.iter().copied().chain(std::iter::once(p[0])).collect::<Vec<_>>();
    let plate = rounded(w, h, 7.0);
    let lit = active || leaving.is_some() || (primary && jose_sign() == 2 && !jose6());
    let mut sh = Shape::new();
    let ink;
    let morph = if jose6() { jose_opt(Part::Morph) } else { 0 };
    // Jose's morphing board is one drawing at every state, idle included: `e` runs from 0 (idle)
    // to 1 (hover). The rim keeps its shape throughout (round 11's idle had its own, rounder
    // frame, so the corners jumped on hover); the gold glass fades out as the sheet fades in;
    // the rim turns black over the sheet only as fast as the sheet appears; the label goes from
    // cream to black with it.
    let _ = lit;
    if morph > 0 {
        let frame: Vec<[f32; 2]> = rounded(w - 6.0, h - 6.0, 2.0).into_iter().map(|p| [p[0] + 3.0, p[1] + 3.0]).collect();
        sh = sh.fill(frame, hex("#3a2a05").alpha(0.62 * (1.0 - e)));
        // The shop's Items board: a yellow sheet whose corners wander, so it changes shape as it
        // drifts, overlapping the board but not all of it; the rims invert, black over the sheet
        // and cream over the sky. Corners barely rounded. 1.5× round 10's pace.
        // Round 11: +30% on round 10's pace by default (1.95); the pace options stay for reference.
        let pace = [1.95, 1.8, 2.1][jose_opt(Part::Pace) as usize % 3];
        let t = s.t * pace + noise(seed) * 5.0;
        // 4–6 tilt the sheet against the board, so it sticks out at two opposite corners and
        // leaves the other two bare: 4 top-left and bottom-right, 5 top-right and bottom-left, 6
        // swinging slowly from one pair to the other.
        let (dx, dy) = match morph {
            2 => (7.0, 4.0),
            4..=7 => (0.0, 0.0),
            _ => (-6.0, -4.0),
        };
        let tilt_by = match morph {
            4 => 0.2,
            5 => -0.2,
            6 => 0.2 * (t * 0.6).sin(),
            // Back or forward, at random per button.
            7 => {
                if noise(seed ^ 0x51) < 0.5 {
                    0.2
                } else {
                    -0.2
                }
            }
            _ => 0.0,
        };
        let (tc, ts) = (tilt_by.cos(), tilt_by.sin());
        let spin = |p: [f32; 2]| {
            let (x, y) = (p[0] - w / 2.0, p[1] - h / 2.0);
            [w / 2.0 + x * tc - y * ts, h / 2.0 + x * ts + y * tc]
        };
        // Settled: 40% less wander (index 0); 1–3 keep the round's comparisons (20/40/60% less).
        let amp = 5.5 * [0.6, 0.8, 0.6, 0.4][jose_opt(Part::Amp) as usize % 4];
        let wander = |k: u32| {
            let (sp, ph) = (1.1 + 1.4 * noise(seed + k), noise(seed + k + 9) * 6.0);
            [(t * sp + ph).sin() * amp, (t * sp * 0.8 + ph * 1.7).cos() * amp * 0.7]
        };
        // Settled: narrower, centre wandering (2) unless a scene asks otherwise.
        let center = match jose_opt(Part::Center) {
            0 => 2,
            c => c,
        };
        // 10% narrower about its middle (round 12: the height back to full).
        let k = if center > 0 { 0.9 } else { 1.0 };
        let (bw, bh) = (w * k, h);
        let (ox, oy) = ((w - bw) / 2.0, (h - bh) / 2.0);
        let base = [[ox, oy], [ox + bw, oy], [ox + bw, oy + bh], [ox, oy + bh]].map(spin);
        let drift = match center {
            // Wandering: three sines at unrelated speeds per axis, so it never quite repeats.
            2 => [
                (t * 0.9).sin() * 1.6 + (t * 2.3 + 1.0).sin() * 1.2 + (t * 3.7 + 2.0).sin() * 0.6,
                (t * 1.3).cos() * 1.0 + (t * 2.9 + 0.5).sin() * 0.8 + (t * 4.1 + 1.5).sin() * 0.4,
            ],
            // Circling: a small, slow loop.
            3 => [(t * 1.6).cos() * 2.5, (t * 1.6).sin() * 1.6],
            _ => [(t * 0.9).sin() * 2.5, (t * 1.3).cos() * 1.5],
        };
        let sheet: Vec<[f32; 2]> = base
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let o = wander(k as u32);
                [p[0] + dx + drift[0] + o[0] * e, p[1] + dy + drift[1] + o[1] * e]
            })
            .collect();
        // Very slightly rounded.
        let sheet = if morph == 7 { round_poly(&sheet, 2.5) } else { sheet };
        sh = sh.fill(sheet.clone(), yellow.alpha(e));
        let mut rims = vec![(rounded(w - 6.0, h - 6.0, 2.0).into_iter().map(|p| [p[0] + 3.0, p[1] + 3.0]).collect::<Vec<_>>(), 3.2)];
        if morph == 3 {
            rims.push((rounded(w, h, 2.5).iter().map(|p| [p[0] - 4.0 + p[1] * 0.05, p[1] - 3.5]).collect(), 3.2));
        }
        for (rim, width) in rims {
            for q in rim_pieces(&rim, width) {
                sh = sh.fill(q.clone(), cream);
                let inside = clip_convex(&q, &sheet);
                if inside.len() >= 3 {
                    sh = sh.fill(inside, black().alpha(e));
                }
            }
        }
        ink = cream.mix(black(), e);
    } else if lit {
        // Items: a cream frame askew behind, the yellow board, a black rim inside.
        let back: Vec<[f32; 2]> = plate.iter().map(|p| [p[0] - 3.0 + p[1] * 0.06, p[1] - 2.5]).collect();
        let plate_opt = if jose6() { jose_opt(Part::Plate) } else { 0 };
        if plate_opt != 2 {
            sh = sh.stroke(closed(back), 2.6, cream);
        }
        if !matches!(plate_opt, 1 | 2) {
            sh = sh.fill(plate.clone(), if press && !jose6() { hex("#ffe55c") } else { yellow });
        }
        let inner: Vec<[f32; 2]> = rounded(w - 8.0, h - 8.0, 4.5).into_iter().map(|p| [p[0] + 4.0, p[1] + 4.0]).collect();
        sh = sh.stroke(closed(inner), 2.4, black());
        ink = black();
    } else {
        // Cognition: a cream frame round a tinted glass.
        let tint = if jose6() && jose_opt(Part::Plate) == 3 {
            // No gold: a faint neutral smoke, just enough for the label.
            hex("#020a2c").alpha(0.3)
        } else if jose_sign() == 1 || jose6() {
            hex("#3a2a05").alpha(0.62)
        } else {
            hex("#081a5a").alpha(0.62)
        };
        sh = sh.fill(plate.clone(), tint).stroke(closed(plate.clone()), 2.8, cream);
        ink = cream;
    }
    let glow_a = if jose6() && jose_opt(Part::Glow) == 1 { 0.0 } else { 0.45 * e };
    let glow = layer(Shape::new().fill(inflate(&plate, 3.0), yellow.alpha(glow_a)), w, h).blur(6.0);
    let font = TextStyle::new(15.0, 600.0).family(Family::Named(jose_family()));
    // Title case, as the shop's boards: "OPEN…" reads "Open...".
    let mut word = label.to_lowercase().replace('…', "...");
    if let Some(f) = word.get(0..1) {
        word = f.to_uppercase() + &word[1..];
    }
    let face = el().size(w, h).center().child(layer(sh, w, h)).child(text(word).font(font).color(ink));
    let sway = (s.t * 1.3 + noise(seed) * 6.0).sin() * 0.01;
    // Just released: spring from the pressed pose back up to hover over 0.25 s.
    let unpress = if look == Look::Hover && jose6() {
        s.released.as_ref().filter(|(k, _)| k == key).map(|(_, at)| s.t - at).filter(|d| *d < 0.25).map(|d| snap(d, 0.25))
    } else {
        None
    };
    let (scale, lift) = if press {
        (0.94, 1.0)
    } else if let Some(p) = unpress {
        (0.94 + (1.06 - 0.94) * p, 1.0 - 3.0 * p)
    } else {
        (1.0 + 0.06 * e, -2.0 * e)
    };
    // A board leans a little at rest, as the shop's do, and further on hover.
    let turn = tilt(seed, 0.015, 0.03) + sway + tilt(seed ^ 7, 0.02, 0.03) * e;
    let mut b = el().size(w + 6.0, BAR_H).pt(3.0);
    // Jose keeps the glow in the tree even at rest (at zero), so the board's children never
    // change and its springs carry on across hover, leave and idle.
    if active || (jose6() && morph > 0) {
        b = b.child(glow.pin(Some(0.0), Some(3.0), None, None));
        if active && jose6() && jose_opt(Part::Morph) == 0 {
            b = b.child(hover_sheen(s, seed, w, h, e).pin(Some(0.0), Some(3.0), None, None));
        }
    }
    let b = b.child(face.pin(Some(0.0), Some(3.0), None, None)).offset(0.0, lift).scale(scale).scale_motion(kick()).rotate(turn);
    if look == Look::Disabled { b.opacity(0.4) } else { b }
}

/// A closed outline's stroke as convex pieces, one quad per segment, each run on past its ends
/// by half the width so the joints close, for clipping piece by piece.
fn rim_pieces(pts: &[[f32; 2]], width: f32) -> Vec<Vec<[f32; 2]>> {
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

/// Round 9's morphing sheets.
pub const SIGNS10: [(&str, &str); 3] = [
    ("4  Tilted back", "out at top-left and bottom-right"),
    ("5  Tilted forward", "out at top-right and bottom-left"),
    ("6  Swinging", "from one pair of corners to the other"),
];

pub const SIGNS9: [(&str, &str); 3] = [
    ("1  Up and left", "a wandering sheet; rims invert over it"),
    ("2  Down and right", "the same, shifted down-right"),
    ("3  With the askew frame", "1 plus the shop's offset frame"),
];

/// Round 8's sheet sizes, on plate 2.
pub const SIGNS8: [(&str, &str); 3] = [
    ("1  Up and left", "the board's size, shifted up-left"),
    ("2  Down and right", "the board's size, shifted down-right"),
    ("3  Tilted across", "narrower, tilted across the label"),
];

/// Round 7's plates: which one A's buttons drop.
pub const SIGNS7: [(&str, &str); 3] = [
    ("1  No yellow board", "hover: the sheet, a black rim, the askew frame"),
    ("2  Sheet and rim only", "hover: the sheet and a black rim"),
    ("3  No gold glass", "rest: a cream frame on faint smoke"),
];

/// The hover options for round 6's signboards.
pub const SIGNS6: [(&str, &str); 3] = [
    ("A  Swaying sheet", "a pale sheet behind, rocking"),
    ("B  Sweeping sheet", "the sheet slides across and back"),
    ("C  Two sheets", "two pale sheets turning apart"),
];

/// The shop's hover: pale yellow sheets behind the lit board, never at rest. `e` is the
/// entrance, 0 to 1.
fn hover_sheen(s: &S, seed: u32, w: f32, h: f32, e: f32) -> El {
    let fit = jose_opt(Part::Fit);
    if fit > 0 {
        // The board's own size, as in the shop: it overlaps the board but never covers all of it.
        let t = s.t + noise(seed) * 5.0;
        let (sw, sh_, dx, dy, turn) = match fit {
            2 => (w, h, 8.0, 4.0, 0.06),
            3 => (w * 0.85, h, w * 0.04, 1.0, -0.12),
            _ => (w, h, -8.0, -5.0, -0.07),
        };
        let a = turn + 0.03 * (t * 2.0).sin();
        let (x, y) = (dx + (t * 1.3).cos() * 2.5, dy + (t * 1.7).sin() * 1.5);
        let sheet = layer(Shape::new().fill(rounded(sw, sh_, 5.0), jose_yellow().alpha(0.9 * e)), sw, sh_);
        return el().size(w, h).child(sheet.rotate(a).pin(Some((w - sw) / 2.0 + x), Some((h - sh_) / 2.0 + y), None, None));
    }
    let (sw, sh_) = (w + 28.0, h + 16.0);
    let pale = |a: f32| jose_yellow().alpha(a * e);
    let sheet = |a: f32| layer(Shape::new().fill(rounded(sw, sh_, 4.0), pale(a)), sw, sh_);
    let t = s.t + noise(seed) * 5.0;
    // Up and to the left of the board, as in the shop.
    let at = |x: f32, y: f32| (Some((w - sw) / 2.0 - 6.0 + x), Some((h - sh_) / 2.0 - 4.0 + y));
    let mut b = el().size(w, h);
    match jose_opt(Part::Hover) {
        1 => {
            // Slides across and back under the board.
            let x = (t * 2.2).sin() * 14.0;
            let (l, tp) = at(x, -2.0);
            b = b.child(sheet(0.8).rotate(-0.06).pin(l, tp, None, None));
        }
        2 => {
            for (k, dir) in [(0.0, 1.0), (1.7, -1.0)] {
                let a = dir * (0.05 + 0.04 * (t * 1.8 + k).sin());
                let (l, tp) = at(dir * 4.0, -2.0);
                b = b.child(sheet(0.6).rotate(a).pin(l, tp, None, None));
            }
        }
        _ => {
            // Rocks about its middle, drifting a little.
            let a = -0.07 + 0.05 * (t * 2.0).sin();
            let (l, tp) = at((t * 1.3).cos() * 5.0, -3.0 + (t * 1.7).sin() * 2.0);
            b = b.child(sheet(0.8).rotate(a).pin(l, tp, None, None));
        }
    }
    b
}

// ------------------------------------------------------------------ Jose, round 6

/// The sky option's nebula strength and its deep blue bands.
fn sky6() -> (f32, [&'static str; 7]) {
    match jose_opt(Part::Sky) {
        1 => (0.55, ["#00041a", "#000c2e", "#011640", "#021f52", "#042863", "#073272", "#0b3e80"]),
        2..=43 => (0.8, ["#00031a", "#010a2a", "#02123a", "#031a4a", "#05225a", "#082a66", "#0c3474"]),
        _ => (0.35, ["#00051e", "#000f34", "#011a49", "#022458", "#052e68", "#093a78", "#0e4788"]),
    }
}

/// Jose's sky, round 6: deeper blue, lit by drifting nebulae; a fuller heap of moons and single-
/// colour stars a third faster, white glints between them; drawn in a fixed order, so overlapping
/// moons no longer swap places as they drift.
fn night_sky_v6(s: &S, w: f32, h: f32) -> El {
    let m = 120.0;
    let (bw, bh) = (w + 2.0 * m, h + 2.0 * m);
    let (neb, bands) = sky6();
    let mut sky = Shape::new();
    for (k, c) in bands.iter().enumerate() {
        let (y0, y1) = (bh * k as f32 / bands.len() as f32, bh * (k + 1) as f32 / bands.len() as f32 + 1.0);
        sky = sky.fill(vec![[0.0, y0], [bw, y0 - 30.0], [bw, y1 - 30.0], [0.0, y1]], hex(c));
    }
    // Nebulae: broad, lighter clouds, slowly drifting.
    // About half the window: a band from the upper right down towards the middle, leaving the
    // left and the top-left corner deep.
    let clouds =
        [(0.78, 0.2, 170.0, "#3a5fc0"), (0.58, 0.42, 150.0, "#2f62b0"), (0.9, 0.55, 140.0, "#4a6fd0"), (0.4, 0.68, 120.0, "#2a74b8")];
    let shape = jose_opt(Part::Sky);
    if shape >= 6 {
        sky = merged_nebulae(s, sky, &clouds, m, w, h, neb, shape);
    }
    for (k, (fx, fy, r, c)) in clouds.iter().enumerate().filter(|_| shape < 6) {
        let (x, y) = (m + fx * w + (s.t * 0.05 + k as f32).sin() * 30.0, m + fy * h + (s.t * 0.04 + k as f32 * 2.0).cos() * 20.0);
        let (c, a) = (hex(c), (neb * 1.5).min(1.0));
        let seed = k as u32 * 101;
        match shape {
            3 => {
                // Wisps: long thin ellipses laid along the comets' sweep, overlapping unevenly.
                for i in 0..9u32 {
                    let u = i as f32 / 8.0 - 0.5;
                    let (px, py) = (x + u * r * 2.6, y + u * r * 0.9 + (noise(seed + i) - 0.5) * r * 0.6);
                    let (rx, ry) = (r * (0.35 + 0.4 * noise(seed + i + 20)), r * (0.08 + 0.14 * noise(seed + i + 40)));
                    sky = sky.fill(
                        ellipse(px, py, rx, ry, 0.33 + (noise(seed + i + 60) - 0.5) * 0.5),
                        c.alpha(a * (0.6 + 0.4 * noise(seed + i + 80))),
                    );
                }
            }
            4 => {
                // Lumps: a cluster of uneven blobs, some bright knots.
                for i in 0..10u32 {
                    let (ang, dist) = (noise(seed + i) * std::f32::consts::TAU, r * 0.7 * noise(seed + i + 20).sqrt());
                    let rr = r * (0.18 + 0.32 * noise(seed + i + 40));
                    let (px, py) = (x + ang.cos() * dist * 1.4, y + ang.sin() * dist * 0.8);
                    sky = sky.fill(
                        ellipse(px, py, rr * 1.3, rr * 0.8, noise(seed + i + 60) * 3.0),
                        c.alpha(a * (0.5 + 0.5 * noise(seed + i + 80))),
                    );
                }
                sky = sky.fill(ellipse(x + r * 0.2, y - r * 0.1, r * 0.25, r * 0.15, 0.4), hex("#7fb4ff").alpha(neb * 0.5));
            }
            5 => {
                // Wisps with dark lanes: as 3, then streaks of deep sky cut back through them.
                for i in 0..9u32 {
                    let u = i as f32 / 8.0 - 0.5;
                    let (px, py) = (x + u * r * 2.6, y + u * r * 0.9 + (noise(seed + i) - 0.5) * r * 0.6);
                    let (rx, ry) = (r * (0.35 + 0.4 * noise(seed + i + 20)), r * (0.1 + 0.16 * noise(seed + i + 40)));
                    sky = sky.fill(
                        ellipse(px, py, rx, ry, 0.33 + (noise(seed + i + 60) - 0.5) * 0.5),
                        c.alpha(a * (0.6 + 0.4 * noise(seed + i + 80))),
                    );
                }
                for i in 0..3u32 {
                    let u = i as f32 / 2.0 - 0.5;
                    let (px, py) = (x + u * r * 1.6 + r * 0.1, y + u * r * 0.6 + r * 0.12);
                    sky = sky.fill(ellipse(px, py, r * 0.7, r * 0.05, 0.36), hex("#00031a").alpha(0.8));
                }
            }
            _ => {
                sky =
                    sky.fill(circle(x, y, *r), c.alpha(a)).fill(circle(x + r * 0.3, y - r * 0.2, r * 0.5), hex("#7fb4ff").alpha(neb * 0.4));
            }
        }
    }
    // Shaped clouds need less blur, or they melt back into discs.
    let soften = match shape {
        6..=8 => 14.0,
        9..=43 => 30.0,
        3..=5 => 18.0,
        _ => 48.0,
    };
    let sky = layer(sky, bw, bh).blur(soften).pin(Some(-m), Some(-m), None, None);

    let mut glow = Shape::new();
    let mut sharp = Shape::new();
    let (cx, cy) = (w * 0.1, h + 360.0);
    for k in 0..6 {
        let r = 470.0 + k as f32 * 105.0;
        let start = -2.0 + k as f32 * 0.1;
        let u = (s.t * (0.07 + 0.02 * noise(k * 7 + 1)) + noise(k * 7 + 2)).fract();
        let head = start + u * 1.85;
        let tail = head - 0.55;
        let c = if k % 2 == 0 { hex("#8ff4ff") } else { jose_pale() };
        glow = glow.fill(comet(cx, cy, r, tail, head, 8.0), c.alpha(0.5));
        sharp = sharp.fill(comet(cx, cy, r, tail, head, 2.4), c);
    }
    let stars = jose_opt(Part::Stars);
    // 4–7 are 3 with many more stars, most of them faint: 500, 900, 1400, 2400.
    let count = match stars {
        4 => 500,
        5 => 900,
        6 => 1400,
        7 => 2400,
        _ => 200,
    };
    for i in 0..count {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h * 0.85);
        if stars == 0 {
            let tw = 0.5 + 0.5 * (s.t * (1.0 + 2.0 * noise(i * 3 + 3)) + i as f32).sin();
            glow = glow.fill(circle(x, y, 1.6 + 1.6 * tw), hex("#bff6ff").alpha(0.5 * tw));
            sharp = sharp.fill(circle(x, y, 0.6 + 0.8 * tw), hex("#ffffff").alpha(0.6 + 0.4 * tw));
            continue;
        }
        // Varied: most stars small, a few large (size cubed skews it), each with its own halo
        // reach and strength, twinkling more slowly and only partway, so none blinks out.
        let size = 0.5 + 2.2 * noise(i * 7 + 11).powi(3);
        let reach = 1.5 + 2.5 * noise(i * 7 + 12);
        let rate = if stars == 2 { 0.15 + 0.35 * noise(i * 3 + 3) } else { 0.3 + 0.7 * noise(i * 3 + 3) };
        let tw = 0.65 + 0.35 * (s.t * rate + i as f32 * 1.7).sin();
        // In the dense skies most stars are faint: brightness skewed low, the small ones dimmest.
        let (size, tw) = if stars >= 4 {
            let b = 0.3 + 0.7 * noise(i * 7 + 15).powi(3);
            // At least a visible point: under ~0.6 pt they vanish at 1×.
            ((size * (0.6 + 0.4 * b)).max(0.9), tw * b)
        } else {
            (size, tw)
        };
        let tint = if noise(i * 7 + 13) < 0.25 { hex("#fff6c8") } else { hex("#bff6ff") };
        glow = glow.fill(circle(x, y, size * reach * (0.8 + 0.2 * tw)), tint.alpha((0.25 + 0.35 * noise(i * 7 + 14)) * tw));
        let core = if stars >= 4 { 0.25 + 0.75 * tw } else { 0.55 + 0.45 * tw };
        sharp = sharp.fill(circle(x, y, size * (0.6 + 0.25 * tw)), hex("#ffffff").alpha(core));
        if (stars == 3 && i % 25 == 0) || (stars >= 4 && i % 60 == 0) {
            // A few bright ones, in wide, soft halos.
            glow = glow.fill(circle(x, y, 9.0 + 2.0 * tw), hex("#d8f6ff").alpha(0.35 * tw));
            sharp = sharp.fill(circle(x, y, 2.0 + 0.4 * tw), hex("#ffffff"));
        }
    }
    // The heap, a third faster than round 5 and fuller. Each item keeps its place in the
    // drawing order for good: sorting by height made overlapping moons swap as they drifted.
    let drift = s.t * 21.0;
    let lane = w + 260.0;
    let (body, halo) = (jose_yellow(), jose_yellow_glow());
    let mut order: Vec<u32> = (0..140).collect();
    order.sort_by(|a, b| noise(a * 5 + 2).partial_cmp(&noise(b * 5 + 2)).unwrap().reverse());
    for i in order {
        // Where along the lane it starts and how fast it drifts. At random (0) they clump and
        // leave gaps; spaced (1–3), each item gets its own slot of the lane, nudged within it.
        let heap = jose_opt(Part::Heap);
        let slot = (i as f32 * 0.618034).fract();
        let start = match heap {
            // Nudged up to two slots either way, or one.
            1 | 2 => slot + (noise(i * 5 + 1) - 0.5) * 4.0 / 140.0,
            3 => slot + (noise(i * 5 + 1) - 0.5) * 2.0 / 140.0,
            _ => noise(i * 5 + 1),
        };
        let speed = match heap {
            2 => 1.0,
            3 => 0.94 + 0.12 * noise(i * 5 + 4),
            _ => 0.75 + 0.5 * noise(i * 5 + 4),
        };
        let x = (start * lane + drift * speed).rem_euclid(lane) - 130.0;
        let edge = ((x / w - 0.5).abs() * 2.0).powf(2.0);
        let depth = noise(i * 5 + 2);
        let y = h - 8.0 - depth * (80.0 + 190.0 * edge) - (x / w) * 20.0;
        let spin = s.t * 0.35 * (noise(i * 5 + 3) - 0.5) + noise(i * 5 + 6) * 6.0;
        let moon = i % 3 == 0;
        let r = if moon { 20.0 + 30.0 * noise(i * 5 + 3) } else { 8.0 + 16.0 * noise(i * 5 + 3) };
        if moon {
            // In convex pieces: as one polygon, Rune's ear-clipper sometimes found no ear and
            // fanned the moon into a full disk for a frame.
            for q in moon_pieces(x, y, r * 1.12, spin) {
                glow = glow.fill(q, halo.alpha(0.85));
            }
            for q in moon_pieces(x, y, r, spin) {
                sharp = sharp.fill(q, body);
            }
        } else {
            let shape = star_soft(x, y, r, spin);
            glow = glow.fill(grown(&shape, x, y, 1.12), halo.alpha(0.85));
            sharp = sharp.fill(shape, body);
        }
    }
    // White glints among the heap, twinkling.
    for i in 0..45u32 {
        let x = (noise(i * 11 + 1) * lane + drift * 0.9).rem_euclid(lane) - 130.0;
        let edge = ((x / w - 0.5).abs() * 2.0).powf(2.0);
        let y = h - 6.0 - noise(i * 11 + 2) * (90.0 + 200.0 * edge) - (x / w) * 20.0;
        let tw = 0.5 + 0.5 * (s.t * (2.0 + 3.0 * noise(i * 11 + 3)) + i as f32 * 1.3).sin();
        glow = glow.fill(circle(x, y, 4.0 + 5.0 * tw), hex("#ffffff").alpha(0.9 * tw));
        sharp = sharp.fill(circle(x, y, 1.0 + 1.4 * tw), hex("#ffffff").alpha(0.5 + 0.5 * tw));
    }
    el().w(w).h(h).clip_rounded(0.0).child(sky).child(layer(glow, w, h).blur(7.0)).child(layer(sharp, w, h))
}

/// An ellipse with radii `rx`, `ry` at (`x`, `y`), turned by `turn`. Convex, so it always fills.
fn ellipse(x: f32, y: f32, rx: f32, ry: f32, turn: f32) -> Vec<[f32; 2]> {
    let (c, sn) = (turn.cos(), turn.sin());
    (0..32)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / 32.0;
            let (px, py) = (a.cos() * rx, a.sin() * ry);
            [x + px * c - py * sn, y + px * sn + py * c]
        })
        .collect()
}

/// Nebulae as metaballs: each blob a field falling off with distance, the cloud everything above
/// a threshold of their sum, so blobs that drift close grow a neck and merge like droplets.
/// 6 the clouds with satellites, one level; 7 the same in three nested levels; 8 a drift of
/// smaller droplets merging and parting.
fn merged_nebulae(s: &S, mut sky: Shape, clouds: &[(f32, f32, f32, &str)], m: f32, w: f32, h: f32, neb: f32, opt: u8) -> Shape {
    let t = s.t;
    let mut balls: Vec<(f32, f32, f32)> = Vec::new();
    if opt == 8 {
        // Droplets drifting along the comets' sweep, over the right half and the middle.
        for i in 0..14u32 {
            let lane = w * 0.9;
            let u = (noise(i * 7 + 1) * lane + t * (10.0 + 8.0 * noise(i * 7 + 2))).rem_euclid(lane);
            let (x, y) = (m + w * 0.25 + u, m + h * (0.1 + 0.6 * noise(i * 7 + 3)) - u * 0.25 + (t * 0.4 + i as f32).sin() * 12.0);
            balls.push((x, y, 45.0 + 60.0 * noise(i * 7 + 4)));
        }
    } else {
        for (k, (fx, fy, r, _)) in clouds.iter().enumerate() {
            let (x, y) = (m + fx * w + (t * 0.05 + k as f32).sin() * 30.0, m + fy * h + (t * 0.04 + k as f32 * 2.0).cos() * 20.0);
            balls.push((x, y, r * 0.75));
            // Two satellites circling slowly, touching and leaving the main blob.
            for j in 0..2 {
                let a = t * (0.15 + 0.05 * j as f32) + k as f32 * 1.9 + j as f32 * 3.1;
                let d = r * (0.95 + 0.25 * (t * 0.3 + j as f32).sin());
                balls.push((x + a.cos() * d * 1.3, y + a.sin() * d * 0.7, r * (0.35 + 0.1 * j as f32)));
            }
        }
    }
    if opt >= 9 {
        // Gentler: each cloud merges only with its own satellites, never with the other clouds,
        // and a higher threshold keeps the necks thin until blobs are close. 12–15 each push one
        // thing on 10: 12 more clouds, 13 bigger clouds, 14 a stronger glow, 15 stronger merging.
        let extra = [
            (0.25, 0.3, 130.0, "#2f62b0"),
            (0.68, 0.82, 120.0, "#3a5fc0"),
            (0.97, 0.3, 110.0, "#2a74b8"),
            (0.12, 0.5, 110.0, "#3466bd"),
            (0.5, 0.12, 120.0, "#2f62b0"),
            (0.84, 0.9, 100.0, "#2a74b8"),
        ];
        // 32–37 follow the kept combinations (more or bigger clouds, merging, no extra glow), each
        // as (extra clouds, size, merging: 1 as 15, 2 stronger).
        let tuned: Option<(usize, f32, u8)> = match opt {
            32 => Some((6, 0.85, 1)),
            33 => Some((3, 0.85, 2)),
            34 => Some((0, 1.3, 1)),
            35 => Some((3, 1.0, 1)),
            36 => Some((3, 1.15, 2)),
            37 => Some((6, 1.15, 1)),
            38 | 39 | 41 | 42 | 43 => Some((3, 1.0, 1)),
            40 => Some((3, 1.0, 1)),
            _ => None,
        };
        // 38–41, after 34 and 35: clouds of different sizes, and a faint veil where the clouds'
        // fields meet, so small ones lean towards their neighbours instead of sitting alone.
        // 38 sizes only; 39 sizes and a faint veil; 40 four big and three small, veiled; 41 39
        // with a stronger veil.
        let vary = match opt {
            38 | 39 | 41 | 42 | 43 => 1,
            40 => 2,
            _ => 0,
        };
        let veil = match opt {
            39 | 40 | 42 => 0.22,
            41 | 43 => 0.38,
            _ => 0.0,
        };
        let mut everyone: Vec<(f32, f32, f32)> = Vec::new();
        let mut bodies: Vec<(Vec<(f32, f32, f32)>, &str)> = Vec::new();
        // 16–31 combine them: 16 plus a mask, 1 more, 2 bigger, 4 glow, 8 merging.
        let mask = match opt {
            12 => 1,
            13 => 2,
            14 => 4,
            15 => 8,
            16.. => opt - 16,
            _ => 0,
        };
        let n_extra = tuned.map_or(if mask & 1 != 0 { 3 } else { 0 }, |t| t.0);
        let all: Vec<(f32, f32, f32, &str)> = clouds.iter().copied().chain(extra[..n_extra].iter().copied()).collect();
        let base = if let Some(t) = tuned {
            t.1
        } else if opt == 11 {
            0.7
        } else if mask & 2 != 0 {
            1.15
        } else {
            0.85
        };
        let strength = if mask & 4 != 0 { 1.6 } else { 1.0 };
        let merge = tuned.map_or(if mask & 8 != 0 { 1 } else { 0 }, |t| t.2);
        let (sat, near, iso_k) = match merge {
            1 => (1.45, 0.75, 0.75),
            2 => (1.7, 0.62, 0.65),
            _ => (1.0, 1.0, 1.0),
        };
        for (k, (fx, fy, r, c)) in all.iter().enumerate() {
            let (x, y) = (m + fx * w + (t * 0.05 + k as f32).sin() * 30.0, m + fy * h + (t * 0.04 + k as f32 * 2.0).cos() * 20.0);
            let size = match vary {
                1 => 0.6 + 0.8 * noise(k as u32 * 13 + 5),
                2 => {
                    if k < 4 {
                        1.3
                    } else {
                        0.6
                    }
                }
                _ => 1.0,
            };
            let base = base * size;
            let mut own = vec![(x, y, r * 0.6 * base)];
            for j in 0..2 {
                let a = t * (0.15 + 0.05 * j as f32) + k as f32 * 1.9 + j as f32 * 3.1;
                let d = r * (1.0 + 0.3 * (t * 0.3 + j as f32).sin()) * near;
                own.push((x + a.cos() * d * 1.2, y + a.sin() * d * 0.7, r * (0.22 + 0.06 * j as f32) * base * sat));
            }
            if veil > 0.0 || vary > 0 {
                everyone.extend(own.iter().copied());
                bodies.push((own, c));
                continue;
            }
            let levels: &[(f32, &str, f32)] = match opt {
                9 => &[(1.6, c, 1.0)],
                11 => &[(1.1, c, 0.7), (2.0, "#4a7fe0", 0.6), (3.6, "#9cc4ff", 0.35)],
                _ => &[(1.3, c, 0.8), (2.6, "#7fb4ff", 0.35)],
            };
            for (iso, col, a) in levels {
                for piece in metaball_pieces(&own, w + 2.0 * m, h + 2.0 * m, 12.0, iso * iso_k) {
                    sky = sky.fill(piece, hex(col).alpha((neb * 1.25 * a * strength).min(1.0)));
                }
            }
        }
        if veil > 0.0 {
            // The veil: all the clouds' fields summed, at a low threshold, drawn faintly under
            // the bodies, so it shows only between clouds that are near each other.
            for piece in metaball_pieces(&everyone, w + 2.0 * m, h + 2.0 * m, 12.0, 1.3) {
                // 42 and 43 are 39 and 41 with the veil in magenta, to show where it lies.
                let c = if opt >= 42 { hex("#ff2bd6").alpha(0.55) } else { hex("#2a5aae").alpha((neb * veil).min(1.0)) };
                sky = sky.fill(piece, c);
            }
        }
        for (own, c) in bodies {
            for (iso, col, a) in [(1.3, c, 0.8), (2.6, "#7fb4ff", 0.35)] {
                for piece in metaball_pieces(&own, w + 2.0 * m, h + 2.0 * m, 12.0, iso * iso_k) {
                    sky = sky.fill(piece, hex(col).alpha((neb * 1.25 * a * strength).min(1.0)));
                }
            }
        }
        return sky;
    }
    let levels: &[(f32, &str, f32)] =
        if opt == 7 { &[(0.6, "#2a5aae", 1.0), (1.2, "#3a72d4", 0.8), (2.4, "#7fb4ff", 0.45)] } else { &[(1.0, "#3466bd", 1.0)] };
    for (iso, c, a) in levels {
        for piece in metaball_pieces(&balls, w + 2.0 * m, h + 2.0 * m, 12.0, *iso) {
            sky = sky.fill(piece, hex(c).alpha((neb * 1.25 * a).min(1.0)));
        }
    }
    sky
}

/// The region where Σ r²/d² ≥ `iso`, by marching squares on a `step` grid over `w` × `h`. Every
/// piece is convex (a saddle cell gives two triangles), so each fills exactly as drawn.
fn metaball_pieces(balls: &[(f32, f32, f32)], w: f32, h: f32, step: f32, iso: f32) -> Vec<Vec<[f32; 2]>> {
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
