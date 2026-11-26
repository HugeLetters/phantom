//! The Phantom Thieves' own components: the press-wipe button, ransom-note lettering, the masked ghost and the progress fill.

use rune::{Color, El, Family, LineCap, Path, Shape, TextStyle, canvas, el, row, text};

use crate::common::*;
use crate::motion::*;
use crate::noise::*;
use crate::paint::*;
use crate::shapes::*;
use crate::{BAR_H, button_w};

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

/// The faces a ransom-note heading cuts its letters from: all ship with macOS.
pub(crate) const FACES: [(&str, f32); 6] = [
    ("Futura", 800.0),
    ("Didot", 700.0),
    ("American Typewriter", 600.0),
    ("Avenir Next Condensed", 900.0),
    ("Georgia", 700.0),
    ("Rockwell", 700.0),
];

/// [`ransom_mix`]'s faces: a few, swapped letter by letter, as the Persona 5 menus are made
/// (Futura, a Times, a Clarendon, a Helvetica, an Avant Garde). All macOS system fonts.
pub(crate) const MIX_FACES: [(&str, f32); 5] =
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

/// The Phantom Thieves button, drawn from its hovered, pressed and disabled amounts (see
/// `button_amounts`). One drawing at every state: a black-rimmed slab on a black shadow. As the
/// hover comes in, the slab swells, tilts and turns into a drifting living shape, red and cyan
/// copies of it peek out on either side, and the shadow fades. A press wipes a red face in from
/// the right; becoming disabled fades the face to deep red and brings in a hatch.
pub(crate) fn phantom_thieves_button(s: &S, key: &str, label: &str, primary: bool, hov: f32, prs: f32, dis: f32) -> El {
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let seed = active_seed(s, key);
    let slab = cut(w, h, 14.0, key_seed(key));
    // The button draws as hovered whenever it's at all hovered or pressed; the press shows as
    // the wipe and the shrink below.
    let hovered = hov > 0.001 || prs > 0.001;
    // The face's shape, and what's drawn behind it.
    let mut face_pts = slab.clone();
    let mut back = Shape::new();
    // The copies' entrance: they slide out from behind the face as the hover comes in.
    let e = hov.max(0.0);
    let (mut scale, mut turn) = if hovered { (1.08, tilt(seed, 0.04, 0.11)) } else { (1.0, 0.0) };
    if hovered {
        let d = |n: u32| e * (3.0 + 3.0 * (0.5 + 0.5 * (s.t * (2.0 + n as f32) + n as f32).sin()));
        // The tight living slab: 4 pt in from the box at the sides and 3 at the top and bottom,
        // drifting at most 2 pt, so its edges stay clear of the letters. The idle slab turns
        // into it as the hover enters, so the shape doesn't jump.
        let tight = moved(&living(w - 8.0, h - 6.0, 2.0, s.t, seed), 4.0, 3.0);
        let k = e.clamp(0.0, 1.0);
        face_pts = slab.iter().zip(tight).map(|(a, b)| [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]).collect();
        back = back.fill(moved(&face_pts, -d(1), -d(2) * 0.7), cyan()).fill(moved(&face_pts, d(3), d(4) * 0.7), red());
        // The face swells and tilts with the copies' entrance.
        (scale, turn) = (1.0 + (scale - 1.0) * e, turn * e);
    }
    // Pressed, the button shrinks and tilts less; let go, it swells back up and re-tilts.
    let pressed_k = prs.clamp(0.0, 1.0);
    if pressed_k > 0.0 {
        scale += (0.92 - scale) * pressed_k;
        turn += (tilt(seed, 0.02, 0.05) - turn) * pressed_k;
    }
    // The face is drawn in its rest colors; the red comes in as the wipe below.
    let idle = if primary { (paper(), black()) } else { (black(), paper()) };
    let mut fill = idle.0;
    let mut edge = idle.1.mix(black(), pressed_k);
    // Fading to disabled: the face and rim to the deep red.
    if dis > 0.0 {
        fill = fill.mix(deep(), dis);
        edge = edge.mix(deep(), dis);
    }
    let mut face_sh = Shape::new().fill(inflate(&face_pts, 2.0), edge).fill(face_pts.clone(), fill);
    // Where the wipe has reached: the rest face left of it, the pressed (red) face right. The red
    // slab slides in over the face from the right, and slides back out to the right on release.
    let wipe_x = w * (1.0 - pressed_k);
    if wipe_x < w {
        let rest = vec![[wipe_x, -5.0], [w + 5.0, -5.0], [w + 5.0, h + 5.0], [wipe_x, h + 5.0]];
        let r = clip_convex(&rest, &inflate(&face_pts, 2.0));
        if r.len() >= 3 {
            face_sh = face_sh.fill(r, black());
        }
        let r = clip_convex(&rest, &face_pts);
        if r.len() >= 3 {
            face_sh = face_sh.fill(r, red());
        }
    }
    if dis > 0.001 {
        face_sh = hatch(face_sh, w, h, 5.0, 1.2, hex("#8a0a12").alpha(dis), 0.0);
    }
    // On press the face turns red and the red letters paper. Rune tweens text color for at
    // least 0.1 s (even with `Spring::NONE`), passing through red on red, so the label would
    // vanish for a moment. Instead both labels are always there, in rest and press colors, each
    // clipped to its side of the wipe; opacity in an `instant_changes` element lands at once.
    let rest = if primary { (red(), paper()) } else { (paper(), black()) };
    // The letters dim to the disabled ink, keeping their lettering.
    let rest = (rest.0.mix(hex("#d2505a"), dis), rest.1.mix(deep(), dis));
    let rest_el = lettered(label, rest.0, rest.1, key_seed(label));
    let press_el = lettered(label, paper(), red(), key_seed(label));
    let left = el().pin(Some(0.0), Some(0.0), None, None).size(wipe_x, h).clip_rounded(0.0).child(el().size(w, h).center().child(rest_el));
    let right = el()
        .pin(Some(wipe_x), Some(0.0), None, None)
        .size(w - wipe_x, h)
        .clip_rounded(0.0)
        .child(el().size(w, h).pin(Some(-wipe_x), Some(0.0), None, None).center().child(press_el));
    let label_el = el().size(w, h).pin(Some(0.0), Some(0.0), None, None).child(left).child(right).instant_changes();
    let face = el().size(w, h).center().child(layer(face_sh, w, h)).child(label_el);
    let mut b = el().size(w + 6.0, BAR_H).pt(3.0).child(layer(back, w, h).pin(Some(0.0), Some(3.0), None, None));
    // The shadow is always there, so the face keeps its place among the children (dropping the
    // shadow on hover made the hovered face a new element, cross-fading over the idle one). It
    // fades out as the hover comes in, and with the disabled amount.
    let shadow_a = if hovered { (1.0 - e).clamp(0.0, 1.0) } else { 1.0 };
    let shadow_a = shadow_a * (1.0 - dis);
    b = b.child(layer(Shape::new().fill(moved(&slab, 4.0, 4.0), black().alpha(shadow_a)), w, h).pin(Some(0.0), Some(3.0), None, None));
    b.child(face.pin(Some(0.0), Some(3.0), None, None)).scale(scale).scale_motion(kick()).rotate(turn).rotate_motion(kick())
}

/// Heavy and serif faces mixed, as the pause menu mixes them, with cap heights.
pub(crate) const MIXED: [(&str, f32, f32); 6] = [
    ("Futura", 800.0, 0.70),
    ("Superclarendon", 900.0, 0.72),
    ("Arial Rounded MT Bold", 700.0, 0.72),
    ("Times New Roman", 700.0, 0.66),
    ("Arial Black", 900.0, 0.72),
    ("Impact", 400.0, 0.79),
];

pub(crate) fn face(name: &'static str, size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named(name))
}

/// `word` set as `ink` on a button whose face is `bg`: a Superclarendon capital, the rest in
/// Arial Rounded with some letters in other faces, each tilted a little. At least one letter and
/// at most half go lowercase (vowels first), and one letter (never the capital) is boxed in
/// `ink` with its letter in `bg`.
pub(crate) fn lettered(word: &str, ink: Color, bg: Color, seed: u32) -> El {
    let chars: Vec<char> = word.chars().collect();
    let cap = 10.5;
    let boxed = {
        let letters: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
        letters[seed as usize % letters.len()]
    };
    // Which letters go lowercase: some vowels, by chance, and always at least one.
    let after: Vec<usize> = (1..chars.len()).filter(|&k| chars[k].is_alphabetic()).collect();
    let mut lower: Vec<usize> =
        after.iter().copied().filter(|&k| "AEIOU".contains(chars[k]) && noise(seed.wrapping_add(k as u32 * 13) + 9) > 0.35).collect();
    if lower.is_empty() && !after.is_empty() {
        let vowels: Vec<usize> = after.iter().copied().filter(|&k| "AEIOU".contains(chars[k])).collect();
        let pool = if vowels.is_empty() { &after } else { &vowels };
        lower.push(pool[(seed / 7) as usize % pool.len()]);
    }
    lower.truncate((after.len() / 2).max(1));
    let mut r = row().items_end();
    for (k, ch) in chars.iter().enumerate() {
        let n = seed.wrapping_add(k as u32 * 13);
        let (g, f): (String, TextStyle) = if k == 0 {
            (ch.to_string(), face("Superclarendon", 18.0, 900.0))
        } else if lower.contains(&k) {
            (ch.to_lowercase().collect(), face("Superclarendon", 14.5, 900.0))
        } else if noise(n) > 0.6 {
            let (f, wt, ratio) = MIXED[(noise(n + 4) * MIXED.len() as f32) as usize % MIXED.len()];
            (ch.to_string(), face(f, cap / ratio, wt))
        } else {
            (ch.to_string(), face("Arial Rounded MT Bold", 14.0, 700.0))
        };
        let turn = (noise(n + 2) - 0.5) * 0.12;
        let t = text(g).font(f);
        let e = if boxed == k { el().fill(ink).px(1.5).child(t.color(bg)) } else { el().child(t.color(ink)) };
        r = r.child(e.rotate(turn).mb(if k == 0 { -1.0 } else { 0.0 }));
    }
    r
}

/// A progress fill in the level, leaning track `quad` whose patterns stay straight: stripes,
/// chevrons, dots and blocks are drawn as on a straight bar and cut by the leaning edges, not
/// stretched. The fill's front edge leans with the track's sides. Returns the shape and the
/// percent's ink.
#[allow(clippy::too_many_arguments)]
pub(crate) fn straight_fill(s: &S, t: &Torrent, mut sh: Shape, quad: &[[f32; 2]], bw: f32, h: f32, fw: f32) -> (Shape, Color) {
    // Where the track's left edge is at height y, and how a point moves onto the bar.
    let left = |y: f32| quad[0][0] + (quad[3][0] - quad[0][0]) * (y / h);
    let right = |y: f32| quad[1][0] + (quad[2][0] - quad[1][0]) * (y / h);
    let place = |p: [f32; 2]| -> [f32; 2] {
        let (l, r) = (left(p[1]), right(p[1]));
        [l + p[0] / bw * (r - l), p[1]]
    };
    let region = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> { pts.into_iter().map(place).collect() };
    // A shape placed on the bar and cut by the track's edges, so a fill can run past the end and
    // land flush with the track (and its shadow) at 100.
    let in_track = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> { clip_convex(&region(pts), quad) };
    let p = t.progress.clamp(0.0, 1.0);
    // Fills whose front edge slants by 5 pt reach 5 pt further at 100, so the slant's foot meets
    // the track's end too.
    let reach = fw + 5.0 * p;
    let mut ink = black();
    match t.kind {
        Kind::Downloading => {
            // Never smaller than at 15%, so the arrowhead always carries a stripe; at 100 the
            // arrowhead runs past the end and the track's edge cuts it, filling the bar.
            let q = p.max(0.15);
            let body = q * (bw + 12.0) - 8.0;
            let arrow = in_track(vec![[0.0, 0.0], [body, 0.0], [body + 12.0, h / 2.0], [body, h], [0.0, h]]);
            if arrow.len() >= 3 {
                sh = sh.fill(arrow, red());
            }
            // The stripes, straight, over the body and into the arrowhead, short of its tip, so
            // one is always passing even at the smallest.
            let clip = in_track(vec![[0.0, 0.0], [body, 0.0], [body + 6.0, h / 2.0], [body, h], [0.0, h]]);
            let pitch = 9.0;
            let mut x = -20.0 - h + (s.t * 26.0).rem_euclid(pitch);
            while x < bw + 10.0 && clip.len() >= 3 {
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
            while x < fw + 8.0 {
                // Every chevron is the same filled outline (a 2.4 pt stroke with a mitred tip and
                // round ends), cut by the fill's edge: one at the end is simply partly hidden,
                // not drawn smaller.
                let (ax, ay, bx, by, cy2) = (x, 2.5, x + 4.5, h / 2.0, h - 2.5);
                let hw = 1.2;
                let norm = |dx: f32, dy: f32| {
                    let l = (dx * dx + dy * dy).sqrt();
                    [dy / l, -dx / l]
                };
                let n1 = norm(bx - ax, by - ay);
                let n2 = norm(ax - bx, cy2 - by);
                let m = {
                    let s2 = [n1[0] + n2[0], n1[1] + n2[1]];
                    let l = (s2[0] * s2[0] + s2[1] * s2[1]).sqrt();
                    let u = [s2[0] / l, s2[1] / l];
                    let k = hw / (u[0] * n1[0] + u[1] * n1[1]);
                    [u[0] * k, u[1] * k]
                };
                // In convex pieces (each arm, and the round ends): clipping the whole, concave
                // outline joined its cut ends along the edge and filled the notch.
                let tip_out = [bx + m[0], by + m[1]];
                let tip_in = [bx - m[0], by - m[1]];
                let upper = vec![[ax + n1[0] * hw, ay + n1[1] * hw], tip_out, tip_in, [ax - n1[0] * hw, ay - n1[1] * hw]];
                let lower = vec![tip_out, [ax + n2[0] * hw, cy2 + n2[1] * hw], [ax - n2[0] * hw, cy2 - n2[1] * hw], tip_in];
                for piece in [upper, lower, circle(ax, ay, hw), circle(ax, cy2, hw)] {
                    let r = clip_convex(&piece, &area);
                    if r.len() >= 3 {
                        sh = sh.fill(r, red());
                    }
                }
                x += pitch;
            }
        }
        Kind::Checking => {
            let fill = in_track(vec![[0.0, 0.0], [reach, 0.0], [(reach - 5.0).max(0.0), h], [0.0, h]]);
            if fill.len() >= 3 {
                sh = sh.fill(fill, paper());
            }
            let dots = in_track(vec![[0.0, 0.0], [(reach - 4.0).max(0.0), 0.0], [(reach - 9.0).max(0.0), h], [0.0, h]]);
            let pitch = 4.0;
            for j in 0..=((h / (pitch * 0.5)) as i32 + 1) {
                let y = j as f32 * pitch * 0.5;
                let shift = if j % 2 == 0 { 0.0 } else { pitch * 0.5 };
                let mut x = -12.0 + shift;
                while x < fw {
                    let r = (0.4 + 1.2 * (x.max(0.0) / fw)).min(pitch * 0.72);
                    if dots.len() >= 3 && inside(&dots, [x, y]) {
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
            // The torn edge and its bolt stop short of the end; past them the bolt stretches
            // right, filling to the end at 100.
            let xt = fw.min(bw - 20.0);
            let mut torn = vec![[0.0, 0.0]];
            for k in 0..5 {
                torn.push([xt + (noise(50 + k) - 0.3) * 8.0, h * k as f32 / 4.0]);
            }
            torn.push([0.0, h]);
            let dx = place([xt, h / 2.0])[0] - xt;
            sh = sh.fill(region(torn), error_c());
            sh = sh.stroke(moved(&[[xt + 6.0, 0.0], [xt + 12.0, 6.0], [xt + 9.0, 8.0], [xt + 16.0, h]], dx, 0.0), 1.6, error_c());
            // Past the stop the bolt itself stretches right, its zigzag both edges of a
            // widening band, until the track's end cuts it at 100. Over the bolt's own stroke,
            // so they're one shape with no seam. 1.25 so its top reaches the end at 100.
            let stretch = 1.25 * (fw - xt).max(0.0);
            if stretch > 0.0 {
                let bolt = [[6.0, 0.0], [12.0, 6.0], [9.0, 8.0], [16.0, h]].map(|q| [xt + dx + q[0], q[1]]);
                for k in 0..3 {
                    let (a, b) = (bolt[k], bolt[k + 1]);
                    // Each leg as a convex quad, from the stroke's left side to the stretched
                    // right one.
                    let leg = vec![[a[0] - 0.8, a[1]], [b[0] - 0.8, b[1]], [b[0] + stretch + 0.8, b[1]], [a[0] + stretch + 0.8, a[1]]];
                    let r = clip_convex(&leg, quad);
                    if r.len() >= 3 {
                        sh = sh.fill(r, error_c());
                    }
                }
            }
            ink = error_c();
        }
        Kind::Fetching => {
            // As a general bar with progress, a pale fill grows and the shard slides over the rest.
            let x = (s.t * 2.4).sin() * 0.5 + 0.5;
            let sw = 22.0;
            // The shard floats over the whole bar, fill or not; the fill lands flush at 100.
            if t.progress > 0.0 {
                let fill = in_track(vec![[0.0, 0.0], [reach, 0.0], [(reach - 5.0).max(0.0), h], [0.0, h]]);
                if fill.len() >= 3 {
                    sh = sh.fill(fill, paper().alpha(0.55));
                }
            }
            let sx = place([x * (bw - sw) + sw / 2.0, h / 2.0])[0] - sw / 2.0;
            sh = sh.fill(vec![[sx + 5.0, 1.0], [sx + sw, 1.0], [sx + sw - 5.0, h - 1.0], [sx, h - 1.0]], paper());
        }
    }
    (sh, ink)
}
