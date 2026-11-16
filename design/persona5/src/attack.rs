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
    brand,
    field,
    button,
    panel,
    header,
    row: torrent_row,
    progress,
    list_font,
    cell,
    status,
    errors,
};

/// Torrent names and statuses: Optima, the macOS cousin of the Optima Nova Black that Persona 5
/// Royal sets its English lists in.
fn list_font(size: f32, weight: f32) -> TextStyle {
    if jose_shop() {
        return TextStyle::new(size * 0.95, 600.0).family(Family::Named(JOSE_FAMILY));
    }
    TextStyle::new(size, weight.max(800.0)).family(Family::Named("Optima"))
}

fn kick() -> Spring {
    Spring::new(0.28, 0.45)
}

/// The seed of the current activation of `key`.
fn active_seed(s: &S, key: &str) -> u32 {
    key_seed(key) ^ s.activations.wrapping_mul(2654435761)
}

fn background(s: &S, w: f32, h: f32) -> El {
    if jose_shop() {
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

/// Jose's logo, after the shop's titles: "Phantom" in pale, glowing letters, like light through
/// a tube, each letter its own size, height and tilt. A bright core in a deep green glow, the
/// core a little see-through.
fn jose_brand() -> El {
    let tint = hex("#6fd27c");
    let core = hex("#f0ffe2").alpha(0.66);
    let mut r = row().items_end();
    for (i, ch) in "Phantom".chars().enumerate() {
        let n = 5003 + i as u32 * 11;
        let size = 23.0 + 12.0 * noise(n + 2) + if i == 0 { 6.0 } else { 0.0 };
        let f = list_font(size, 700.0);
        let glyph = |c: Color| text(ch.to_string()).font(f).color(c);
        let letter = el()
            .child(glyph(tint).blur(7.0).opacity(0.55).pin(Some(0.0), Some(0.0), None, None))
            .child(glyph(tint).blur(2.0).opacity(0.8).pin(Some(0.0), Some(0.0), None, None))
            .child(glyph(core));
        let k = 1.4;
        r = r.child(letter.offset(0.0, (noise(n) - 0.5) * 9.0 * k).rotate((noise(n + 1) - 0.5) * 0.35 * k));
    }
    r
}

/// The title alone: the ghost lives by the connection state.
fn brand(_: &S) -> El {
    if jose_shop() {
        // 5 points higher: clear of the buttons at rest (a hovered one may pass over), without
        // clipping the tallest letters at the window's top.
        return jose_brand().mt(2.0).offset(0.0, -5.0);
    }
    ransom_mix("PHANTOM", 19.0, 5, &[(paper(), black()), (black(), paper()), (paper(), red()), (black(), red())], &[paper(), black()], 0.7)
        .mt(2.0)
}

fn field(_: &S, w: f32, hint: &str) -> El {
    // Jose's tape shares the footer's cream.
    let (paper_c, hint_c) = if jose_shop() { (hex("#fff6c8").alpha(0.93), hex("#2b3f8c").alpha(0.8)) } else { (paper().alpha(0.93), hex("#555")) };
    let h = BAR_H - 6.0;
    let slab = if jose_shop() { tape_strip(w - 8.0, h, 41) } else { cut(w - 8.0, h, 7.0, 41) };
    let s = if jose_shop() {
        // Jose's tape: no black edge, a soft shadow, the paper a touch see-through.
        Shape::new().fill(moved(&slab, 3.0, 4.0), black().alpha(0.45)).fill(slab, paper_c)
    } else {
        Shape::new().fill(moved(&slab, 6.0, 5.0), black()).fill(inflate(&slab, 2.5), black()).fill(slab, paper())
    };
    el().size(w, BAR_H).child(layer(s, w, BAR_H)).rotate(-0.006).child(
        row()
            .pin(Some(12.0), Some(0.0), Some(12.0), Some(6.0))
            .items_center()
            .child(text(hint.to_owned()).font(sans(12.5, 600.0)).color(hint_c).truncate()),
    )
}

fn button(s: &S, key: &str, label: &str, primary: bool, look: Look) -> El {
    if jose_shop() {
        return jose_shop_button(s, key, label, primary, look);
    }
    let [hov, prs, dis] = button_amounts(s, key, look);
    phantom_thieves_button(s, key, label, primary, hov, prs, dis.clamp(0.0, 1.0))
}

/// A button's state as three amounts, 0–1, each a spring easing toward what the current look
/// wants: hovered, pressed, disabled. The Phantom Thieves button is drawn from these alone, so
/// any change of state (pressed to idle, hovered to disabled, a press let go halfway) is the
/// amounts easing from wherever they are, with no transition written per pair of states.
fn button_amounts(s: &S, key: &str, look: Look) -> [f32; 3] {
    thread_local! {
        static STATE: std::cell::RefCell<std::collections::HashMap<String, ([f32; 6], f32)>> = Default::default();
    }
    let target = match look {
        Look::Idle => [0.0, 0.0, 0.0],
        Look::Hover => [1.0, 0.0, 0.0],
        Look::Press => [1.0, 1.0, 0.0],
        Look::Disabled => [0.0, 0.0, 1.0],
    };
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let (x, at) = st.entry(key.to_owned()).or_insert(([target[0], 0.0, target[1], 0.0, target[2], 0.0], s.t));
        let mut left = (s.t - *at).clamp(0.0, 1.0);
        *at = s.t;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            left -= dt;
            for (k, &goal) in target.iter().enumerate() {
                // Hover comes in with a touch of overshoot, as the snap did, and leaves without;
                // the press is quicker; disabled the calmest.
                let (w, z) = match k {
                    0 if goal > x[0] => (17.0, 0.62),
                    0 => (16.0, 1.0),
                    1 => (24.0, 1.0),
                    _ => (13.0, 1.0),
                };
                let (pos, vel) = (x[2 * k], x[2 * k + 1]);
                let a = w * w * (goal - pos) - 2.0 * z * w * vel;
                x[2 * k + 1] = vel + a * dt;
                x[2 * k] = pos + x[2 * k + 1] * dt;
            }
        }
        [x[0], x[2], x[4]]
    })
}

/// The Phantom Thieves button, drawn from its hovered, pressed and disabled amounts (see
/// `button_amounts`). One drawing at every state: a black-rimmed slab on a black shadow. As the
/// hover comes in, the slab swells, tilts and turns into a drifting living shape, red and cyan
/// copies of it peek out on either side, and the shadow fades. A press wipes a red face in from
/// the right; becoming disabled fades the face to deep red and brings in a hatch.
fn phantom_thieves_button(s: &S, key: &str, label: &str, primary: bool, hov: f32, prs: f32, dis: f32) -> El {
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
    // The face is drawn in its rest colours; the red comes in as the wipe below.
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
    // On press the face turns red and the red letters paper. Rune tweens text colour for at
    // least 0.1 s (even with `Spring::NONE`), passing through red on red, so the label would
    // vanish for a moment. Instead both labels are always there, in rest and press colours, each
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

fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    let sheet = jagged(0.0, 0.0, w - 10.0, h - 6.0, 1.2, 40.0, 77);
    // The paper clips the rows sliding in, but leaves room for the selection's overhang.
    let clip = el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
    if jose_shop() {
        // A half see-through card of deep navy smoke, denser behind the header and clearing
        // towards the heap, thickening again at the bottom so the rows over the heap read; a
        // lemon rim in a soft glow.
        let smoke = hex("#020a2c");
        let ring = sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>();
        let glow = Shape::new().stroke(ring.clone(), 4.0, jose_yellow_glow().alpha(0.7));
        let rim = Shape::new().stroke(ring, 1.4, jose_pale());
        let (cw, ch) = (w - 10.0, h - 6.0);
        // Denser at the top, gone by 60% of the way down; blurred into a gradient.
        let mut g = Shape::new();
        for k in 0..8 {
            let (y0, y1) = (ch * 0.6 * k as f32 / 8.0, ch * 0.6 * (k + 1) as f32 / 8.0 + 1.0);
            g = g.fill(
                vec![[-40.0, y0 - 40.0 * (k == 0) as i32 as f32], [cw + 40.0, y0 - 40.0 * (k == 0) as i32 as f32], [cw + 40.0, y1], [-40.0, y1]],
                smoke.alpha(0.55 * (1.0 - k as f32 / 8.0)),
            );
        }
        // From 60% down, the smoke thickens again, steepening to about 78% at the very bottom.
        for k in 0..10 {
            let (y0, y1) = (ch * (0.6 + 0.04 * k as f32), ch * (0.6 + 0.04 * (k + 1) as f32) + 1.0);
            let a = 0.18 + 0.6 * ((k as f32 + 1.0) / 10.0).powf(1.3);
            g = g.fill(vec![[-40.0, y0], [cw + 40.0, y0], [cw + 40.0, y1], [-40.0, y1]], smoke.alpha(a));
        }
        let card = el()
            .size(cw, ch)
            .fill(smoke.alpha(0.18))
            .clip_rounded(0.0)
            .child(layer(g, cw + 80.0, ch).blur(20.0).pin(Some(-40.0), Some(0.0), None, None));
        return el()
            .size(w, h)
            .child(layer(glow, w, h).blur(5.0))
            .child(card.pin(Some(0.0), Some(0.0), None, None))
            .child(layer(rim, w, h))
            .child(clip)
            .rotate(-0.008);
    }
    // A black sheet with a white outline, as Persona 5 sets its lists.
    let s = Shape::new().fill(moved(&sheet, 10.0, 9.0), black()).fill(inflate(&sheet, 3.5), paper()).fill(sheet.clone(), black());
    // A faint screentone in the sheet's bottom corner.
    let s = halftone(s, w - 14.0, h - 10.0, 6.0, hex("#1e1e1e"), |x, y| ((x / (w - 14.0)) * (y / (h - 10.0)) * 3.4 - 0.6).max(0.0));
    el().size(w, h).child(layer(s, w, h)).child(clip).rotate(-0.008)
}

/// A column heading: ransom letters, mostly bare, black on the paper strip; on Jose's shop, hand
/// set in the rounded face, each letter nudged up or down and tilted a little, its own way.
fn heading(word: &str, seed: u32) -> El {
    if !jose_shop() {
        return ransom_mix(word, 11.5, seed, &[(red(), paper()), (black(), paper())], &[black()], 0.18);
    }
    let navy = hex("#0b1f5c");
    let mut r = row().items_center();
    for (i, ch) in word.chars().enumerate() {
        let n = seed.wrapping_mul(31).wrapping_add(i as u32 * 7);
        r = r.child(text(ch.to_string()).font(list_font(12.5, 800.0)).color(navy).offset(0.0, (noise(n) - 0.5) * 3.0).rotate((noise(n + 1) - 0.5) * 0.18));
    }
    r
}

fn header(_: &S, w: f32) -> El {
    let w = w - 10.0;
    let strip = if jose_shop() { moved(&tape_strip(w, 24.0, 5), -2.0, 2.0) } else { moved(&cut(w, 24.0, 12.0, 5), -2.0, 2.0) };
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
    if jose_shop() {
        // A saturated yellow strip in a soft, blurred glow.
        let glow = layer(Shape::new().fill(inflate(&strip, 3.0), jose_yellow_glow().alpha(0.3)), w, 30.0).blur(5.0);
        let sh = Shape::new().fill(strip, jose_yellow().alpha(0.94));
        return el().w(w + 10.0).h(30.0).child(glow).child(layer(sh, w, 30.0)).child(r);
    }
    el().w(w + 10.0).h(30.0).child(layer(Shape::new().fill(strip, paper()), w, 30.0)).child(r)
}

/// A row's state as eased amounts (see `button_amounts`): how hovered (pressed counts) and how
/// selected it is, 0–1, and the seed its selection band was cut with when it was selected, so a
/// band fading out keeps its shape after another row is picked.
fn row_amounts(s: &S, key: &str, hovered: bool, selected: bool, seed_now: u32) -> (f32, f32, u32) {
    thread_local! {
        static STATE: std::cell::RefCell<std::collections::HashMap<String, ([f32; 4], f32, u32, bool)>> = Default::default();
    }
    let target = [if hovered { 1.0 } else { 0.0 }, if selected { 1.0 } else { 0.0 }];
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let (x, at, seed, was) = st.entry(key.to_owned()).or_insert(([target[0], 0.0, target[1], 0.0], s.t, seed_now, selected));
        if selected && (!*was || *seed != seed_now) {
            *seed = seed_now;
        }
        *was = selected;
        let mut left = (s.t - *at).clamp(0.0, 1.0);
        *at = s.t;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            left -= dt;
            for (k, &goal) in target.iter().enumerate() {
                // Selecting comes in with C's overshoot (the slam); everything else settles.
                let (w, z) = match k {
                    1 if goal > x[2] => (20.0, 0.55),
                    // Deselecting is quick, so a name already back in its pale ink spends as
                    // little time as possible over a fading flag.
                    1 => (30.0, 1.0),
                    _ => (22.0, 1.0),
                };
                let (pos, vel) = (x[2 * k], x[2 * k + 1]);
                let a = w * w * (goal - pos) - 2.0 * z * w * vel;
                x[2 * k + 1] = vel + a * dt;
                x[2 * k] = pos + x[2 * k + 1] * dt;
            }
        }
        (x[0].max(0.0), x[2].max(0.0), *seed)
    })
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &torrent(s, i);
    let w = w - 10.0;
    let key = format!("row-{i}");
    let mut bg = Shape::new();
    // Hovered and selected as eased amounts: the band, the hover flag or shard, the nudge and
    // the tilt are all drawn from these, so selecting, deselecting and hovering in any order
    // animate from wherever the row is.
    let (ha, sa, band_seed) = row_amounts(s, &key, matches!(look, Look::Hover | Look::Press), selected, selection_seed(s, t));
    // The text turns to the selection's colours once the band is half in (or half out).
    let shown_sel = sa > 0.5;
    let (name, muted, mut status_c) = if shown_sel { (on_select(), on_select(), on_select()) } else { (paper(), muted(), hex("#e2e2e2")) };
    if t.kind == Kind::Error && !shown_sel {
        status_c = error_c();
    }
    let mut turn = 0.0;
    // Jose's flag glows through a blur.
    let mut glow_sh: Option<Shape> = None;
    // On Jose's shop, hovering draws the selection's own flag, the same shape, at a quarter
    // strength, so clicking only fills its colour in. Pressed counts as hovered here, so the
    // faint flag stays through the click until the selection fades in over it.
    let ghost_flag = jose_shop() && ha > 0.001 && !selected;
    let come = ha.min(1.0);
    if sa > 0.001 || ghost_flag {
        // A band with a wedge out past the left edge, cut fresh for each selection.
        let seed = band_seed;
        // `f` how far it has stretched in, `fade_in` its strength. Jose's flag fades with the
        // selected amount, over the hover's quarter-strength flag; the Phantom Thieves band
        // stretches in with it, overshooting as a slam, and draws back the same way when
        // deselected.
        let (f, fade_in) = if jose_shop() { (1.0, sa.min(1.0)) } else { (sa, 1.0) };
        let lean = 4.0 + 6.0 * noise(seed);
        let h = ROW_H;
        let ghost = if ghost_flag { 0.25 * come } else { 0.0 };
        let k = fade_in + (1.0 - fade_in) * ghost;
        let band = if jose_shop() {
            // Jose's flag: a swallowtail at the left, the top and bottom edges swaying.
            let sway = |x: f32, k: f32| (s.t * 2.2 + x * 0.012 + k).sin() * 3.0;
            let n = 12;
            let top = |x: f32| 1.0 + sway(x, 0.0);
            let bottom = |x: f32| h - 1.0 + sway(x, 1.3);
            let mut pts = vec![[-4.0, sway(-4.0, 0.0)]];
            for k in 1..=n {
                let x = -4.0 + (w + 12.0) * k as f32 / n as f32;
                pts.push([x, top(x)]);
            }
            pts.push([w + 2.0, h * 0.5]);
            for k in (0..=n).rev() {
                let x = -4.0 + (w + 12.0) * k as f32 / n as f32;
                pts.push([x - 8.0, bottom(x)]);
            }
            // The swallowtail's notch.
            pts.push([12.0, h * 0.5]);
            pts
        } else {
            vec![[-26.0, h / 2.0 + 2.0], [-10.0 + lean, 2.0], [w + 10.0 + lean, 0.0], [w + 4.0 - lean, h - 3.0], [-12.0 - lean, h - 1.0]]
        };
        let band: Vec<[f32; 2]> = band.into_iter().map(|p| [-26.0 + (p[0] + 26.0) * f, p[1]]).collect();
        // Under it, a sliver that never rests, as on the game's highlighted rows.
        let sliver = living(w * 0.4, h - 10.0, 4.0, s.t, seed).into_iter();
        if jose_shop() {
            let sliver: Vec<[f32; 2]> = sliver.map(|p| [w * 0.6 + p[0] * f, p[1] + 5.0]).collect();
            // Bowed, glowing, a touch see-through, over a pale ghost.
            let bow = |p: [f32; 2]| {
                let u = ((p[0] + 4.0) / (w + 12.0)).clamp(0.0, 1.0);
                [p[0], p[1] - 4.0 * (1.0 - (2.0 * u - 1.0).powi(2)) + 2.0]
            };
            let band: Vec<[f32; 2]> = band.into_iter().map(bow).collect();
            glow_sh = Some(Shape::new().fill(inflate(&band, 2.0), jose_yellow_glow().alpha(0.32 * k)));
            // The pale sliver under the flag stays off on hover: through the faint flag it read
            // as a second, smaller flag.
            let sliver_a = 0.5 * fade_in * sa.min(1.0);
            bg = bg.fill(sliver, jose_pale().alpha(sliver_a)).fill(band, jose_yellow().alpha(0.94 * k));
        } else {
            // Cyan, stretched with the band from the same left point, so it slides in and back
            // with it.
            let sliver: Vec<[f32; 2]> = sliver.map(|p| [-26.0 + (w * 0.6 + p[0] + 26.0) * f, p[1] + 5.0]).collect();
            bg = bg.fill(sliver, cyan().alpha(k)).fill(band, select().alpha(k));
        }
        turn = tilt(seed, 0.004, 0.012) * sa.min(1.0);
    }
    if sa < 0.999 {
        bg = bg.line([0.0, ROW_H - 0.5], [w, ROW_H - 0.5], 1.0, LineCap::Butt, hex("#333333"));
        if !jose_shop() && ha > 0.001 && !selected {
            // A red shard points at the row, and a zigzag runs under it, fading with the hover.
            let shard = blade(10.0, ROW_H - 16.0, 4.0, active_seed(s, &key));
            bg = bg.fill(moved(&shard, -6.0, 8.0), red().alpha(ha.min(1.0)));
            let zig: Vec<[f32; 2]> =
                (0..=(w / 8.0) as usize).map(|k| [k as f32 * 8.0, ROW_H - 4.0 + if k % 2 == 0 { 0.0 } else { 3.0 }]).collect();
            bg = bg.stroke(zig, 2.2, red().alpha(ha.min(1.0)));
        }
    }
    // Rows step right one after another, as the game's lists do, every eight rows over.
    let step = step_offset(i);
    // Hovered rows step in 8 pt; Jose's selected row moves in off its swallowtail, so the name
    // clears the notch. Both follow the amounts.
    let inset = if jose_shop() { 8.0 } else { 0.0 };
    let sk = sa.min(1.0);
    let nudge = 8.0 * ha.min(1.0) * (1.0 - sk) + inset * sk;
    let cells_el = cells(s, &PARTS, t, w - STEP * 7.0, name, muted, status_c).offset(step + nudge, 0.0).offset_motion(kick());
    // The list is simply there on open. A row added or removed slides in or out: the Phantom
    // Thieves' slams from the right, Jose's fades with a short drift.
    let (enter, _) = s.row_life(i);
    // The glow layer is always there (empty when nothing glows), so a row's children never
    // change as it's hovered, selected or deselected.
    let row_el = el().w(w).h(ROW_H).child(layer(glow_sh.unwrap_or_default(), w, ROW_H).blur(6.0)).child(layer(bg, w, ROW_H)).child(cells_el);
    let row_el = if jose_shop() {
        row_el.offset((1.0 - enter.clamp(0.0, 1.0)) * 28.0, 0.0).opacity(enter.clamp(0.0, 1.0))
    } else {
        row_el.offset((1.0 - enter) * 900.0, 0.0)
    };
    row_el.rotate(turn).rotate_motion(kick())
}

/// How far each row steps right of the one above.
const STEP: f32 = 3.0;

fn is_selected(s: &S, t: &Torrent) -> bool {
    // By name: `TORRENTS` is a const, so each use is its own copy and addresses don't match.
    (0..count(s)).any(|i| row_selected(s, i) && torrent(s, i).name == t.name)
}

/// The seed of the current selection: it changes when the selection does, not on every hover.
fn selection_seed(s: &S, t: &Torrent) -> u32 {
    key_seed(t.name) ^ ((s.selected_at * 1000.0) as i32 as u32).wrapping_mul(2654435761)
}

/// The cells drawn their own way: the selected name (with one letter inverted on the Phantom
/// Thieves; plain in the row's black on Jose's shop), and the speeds on slabs.
fn cell(s: &S, t: &Torrent, col: &str, w: f32) -> Option<El> {
    match col {
        "Name" if is_selected(s, t) && jose_shop() => Some(text(t.name.to_owned()).font(list_font(14.0, 800.0)).color(on_select()).truncate().w(w)),
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
            (v != "–").then(|| speed_cell(v, col == "Down", s.t))
        }
        _ => None,
    }
}

/// The rows' secondary text (sizes, dashes): grey, or Jose's pale lemon.
fn muted() -> Color {
    if jose_shop() { hex("#f3eeb6") } else { hex("#a8a8a8") }
}

/// A different bar for each state, so the state reads at a glance, and the percent beside it.
fn progress(s: &S, t: &Torrent, w: f32) -> El {
    let bw = w - 52.0;
    let h = 12.0;
    let (pw, ph) = (46.0, 17.0);
    // The pill reports the client's number as it is; only the bar is smoothed. A torrent still
    // fetching its info shows "??" while the report is 0, not while the smoothed bar is.
    let pct = if t.kind == Kind::Fetching && reported_progress(s, t) <= 0.0 {
        "??".to_owned()
    } else {
        format!("{:.0}%", (reported_progress(s, t) * 100.0).floor())
    };
    if jose_shop() {
        return row().w(w).items_center().gap(6.0).child(jose_bar(s, t, bw, h)).child(jose_pill(s, t, &pct, pw, ph));
    }
    // 0 is drawn as the smallest stub, as 1% looks: drawing nothing there left a stray sliver for
    // a frame. Fetching keeps 0 for "no value yet".
    let stub_at_0 = t.kind != Kind::Fetching;
    let fw = (bw * t.progress).max(if t.progress > 0.0 || stub_at_0 { 6.0 } else { 0.0 });
    // The track: level top and bottom, its sides and front edge leaning, on a paper shadow; the
    // shadow is red for an error, and a seeding bar has none.
    let level = vec![[0.5, 0.0], [bw + 5.0, 0.0], [bw + 0.5, h], [-9.0, h]];
    let sh = match t.kind {
        Kind::Seeding => Shape::new().fill(level.clone(), black()),
        Kind::Error => Shape::new().fill(moved(&level, 2.5, 2.5), error_c()).fill(level.clone(), black()),
        _ => Shape::new().fill(moved(&level, 2.5, 2.5), paper()).fill(level.clone(), black()),
    };
    let (sh, mut ink) = straight_fill(s, t, sh, &level, bw, h, fw);
    let twitch = if t.kind == Kind::Error && (s.t * 0.7).fract() < 0.08 { (s.t * 90.0).sin() * 1.5 } else { 0.0 };
    // The percent in a slanted pill, as the game sets item counts; on the selection it turns dark
    // red with a cyan sliver.
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
    let bar = el().size(bw, h).child(layer(sh, bw, h)).offset(twitch, 0.0);
    let tag = el().size(pw, ph).center().child(layer(pill, pw, ph)).child(text(pct).font(caps(14.0, 900.0)).color(ink));
    row().w(w).items_center().gap(6.0).child(bar).child(tag)
}

/// The fill colour of Jose's bars by state: lemon, a cooler cyan once seeding, pale when paused,
/// red on error.
fn jose_bar_fill(t: &Torrent) -> Color {
    match t.kind {
        Kind::Seeding => hex("#7fe3ff"),
        Kind::Paused => jose_yellow().alpha(0.45),
        Kind::Error => hex("#ff5a6a"),
        _ => jose_yellow(),
    }
}

/// Jose's bars: a rounded navy track with a thin pale rim, the fill glowing, and a pattern per
/// state in Jose's motifs (see `jose_pattern`). An errored bar twitches now and then; a paused
/// one has z's rising from its sleeping crescents.
fn jose_bar(s: &S, t: &Torrent, bw: f32, h: f32) -> El {
    let navy = hex("#0b1f5c");
    let sel = is_selected(s, t);
    let p = t.progress.clamp(0.0, 1.0);
    // Fetching has no fill until it's given progress (as a general bar).
    let fw = (bw * p).max(if p > 0.0 { 8.0 } else { 0.0 });
    let (th, ty) = (10.0, (h - 10.0) / 2.0);
    let quad = move |x0: f32, x1: f32| -> Vec<[f32; 2]> { rounded((x1 - x0).max(th), th, th / 2.0).into_iter().map(|q| [q[0] + x0, q[1] + ty]).collect() };
    let track = quad(0.0, bw);
    let track_c = if sel { hex("#fffbe6").alpha(0.75) } else { navy.alpha(0.85) };
    let mut sh = Shape::new().fill(track.clone(), track_c);
    sh = jose_pattern(s, t, sh, bw, h, fw, sel, &quad);
    let rim: Vec<[f32; 2]> = track.iter().copied().chain(std::iter::once(track[0])).collect();
    let rim_c = if sel { navy.alpha(0.8) } else { jose_pale().alpha(0.7) };
    sh = sh.stroke(rim, 1.0, rim_c);
    let mut e = el().size(bw, h);
    if fw > 0.0 {
        let c = if t.kind == Kind::Seeding { hex("#7fe3ff") } else { jose_bar_fill(t) };
        // Fetching's fill is a soft lemon; its glow is gentler to match.
        let ga = if t.kind == Kind::Fetching { 0.3 } else { 0.7 };
        e = e.child(layer(Shape::new().fill(quad(0.0, fw), c.alpha(ga)), bw, h).blur(4.0));
    }
    e = e.child(layer(sh, bw, h));
    if t.kind == Kind::Error {
        // Now and then the whole bar shakes for a moment, side to side and up and down.
        let (rate, len, amp, vy) = (0.4, 0.05, 0.9, 0.8);
        // Offsets spring by default, which smoothed every twitch away: this bar's land at once.
        e = e.instant_changes();
        let k = (s.t * rate).fract();
        if k < len {
            // Fading out over the moment, so it reads as a jolt that settles.
            let fall = 1.0 - k / len;
            e = e.offset((s.t * 55.0).sin() * amp * fall, (s.t * 43.0).cos() * vy * fall);
        }
    }
    if t.kind == Kind::Paused {
        // Two tiny z's rising from every other sleeping moon, fading, each a little out of step;
        // they grow in with their moon. With no moon yet (near 0) one pair rises at the start.
        let mut moons: Vec<(f32, f32)> = Vec::new();
        let mut x = 6.0;
        while x < fw - 2.0 {
            let u = ((fw - 2.0 - x) / 10.0).clamp(0.0, 1.0);
            moons.push((x, u * u * (3.0 - 2.0 * u)));
            x += 10.0;
        }
        if moons.is_empty() {
            moons.push((-2.0, 1.0));
        }
        for (j, &(mx, g)) in moons.iter().enumerate().filter(|(j, _)| j % 2 == 0) {
            for k in 0..2 {
                let u = (s.t * 0.4 + k as f32 * 0.5 + j as f32 * 0.37).fract();
                let a = (1.0 - u) * smooth_in(u) * g;
                e = e.child(text("z".to_owned()).font(list_font(6.5, 900.0)).color(jose_pale().alpha(a)).pin(
                    Some((mx + 2.0 + 3.0 * k as f32 + 2.0 * u).max(2.0)),
                    Some((h - 10.0) / 2.0 + 1.0 - 3.0 * u),
                    None,
                    None,
                ));
            }
        }
    }
    e
}

/// Jose's state patterns inside a bar's fill. `quad` gives the fill's shape from x0 to x1.
/// Downloading: lemon, with sparkles drifting through that fade in and out at the fill's ends.
/// Seeding: cyan, small stars rising through it and fading, as bubbles do. Checking: pale lemon
/// dotted with tiny stars, a wave passing that makes each twinkle. Paused: still, dim crescents.
/// Fetching: small stars along the track lighting one after another, looking. Error: red with
/// navy diagonal stripes, like warning tape.
#[allow(clippy::too_many_arguments)]
fn jose_pattern(s: &S, t: &Torrent, mut sh: Shape, bw: f32, h: f32, fw: f32, sel: bool, quad: &dyn Fn(f32, f32) -> Vec<[f32; 2]>) -> Shape {
    let navy = hex("#0b1f5c");
    let cy = h / 2.0;
    let red = hex("#ff5a6a");
    let smooth = |e0: f32, e1: f32, x: f32| {
        let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    };
    // Motifs fade in over the fill's first 8 points and out over its last 8.
    let fade = |x: f32| smooth(0.0, 8.0, x) * smooth(0.0, 8.0, fw - x);
    match t.kind {
        Kind::Downloading => {
            let c = if sel { navy } else { jose_yellow() };
            sh = sh.fill(quad(0.0, fw), c);
            let pitch = 14.0;
            let mut x = -pitch + (s.t * 18.0).rem_euclid(pitch);
            while x < fw {
                let a = fade(x);
                if a > 0.01 {
                    let l = 3.4 * (0.7 + 0.3 * (s.t * 3.0 + x * 0.3).sin());
                    let c = if sel { jose_yellow().alpha(a) } else { hex("#ffffff").alpha(0.95 * a) };
                    sh = sh
                        .fill(vec![[x - l, cy], [x, cy - 0.7], [x + l, cy], [x, cy + 0.7]], c)
                        .fill(vec![[x, cy - l * 0.8], [x + 0.7, cy], [x, cy + l * 0.8], [x - 0.7, cy]], c);
                }
                x += pitch;
            }
        }
        Kind::Seeding => {
            sh = sh.fill(quad(0.0, fw), hex("#7fe3ff"));
            for k in 0..6 {
                let u = (s.t * (0.35 + 0.2 * noise(k * 5 + 1)) + noise(k * 5 + 2)).fract();
                let x = 5.0 + (fw - 10.0) * noise(k * 5 + 3);
                let y = h - 1.5 - (h - 3.0) * u;
                let a = (u * 4.0).min(1.0) * (1.0 - u) * fade(x);
                sh = sh.fill(star_soft(x, y, 2.6, 0.0), hex("#ffffff").alpha(a));
            }
        }
        Kind::Checking => {
            // Tiny stars growing along the checked part; a wave passes, each star twinkling
            // brighter and a little bigger as it goes by.
            sh = sh.fill(quad(0.0, fw), jose_pale().alpha(if sel { 0.7 } else { 0.9 }));
            let sx = (s.t * 0.9).fract() * (fw + 30.0) - 15.0;
            let mut x = 4.0;
            while x < fw - 3.0 {
                let wave = (-((x - sx) / 7.0).powi(2)).exp();
                // A star the fill has just reached grows in (and one it's leaving shrinks away)
                // over a star's spacing, instead of popping.
                let g = smooth(0.0, 7.0, fw - 3.0 - x);
                let r = (1.2 + 1.6 * (x / fw)) * (1.0 + 0.6 * wave) * g;
                sh = sh.fill(star_soft(x, cy, r, 0.0), navy.alpha((0.7 + 0.3 * wave) * g));
                if wave > 0.05 {
                    sh = sh.fill(star_soft(x, cy, r * 0.55, 0.0), hex("#ffffff").alpha(wave));
                }
                x += 7.0;
            }
        }
        Kind::Paused => {
            // Asleep: dim crescents, still. One the fill has just reached grows in (and one it's
            // leaving shrinks away) over a moon's spacing, instead of popping.
            let mut x = 6.0;
            while x < fw - 2.0 {
                let g = smooth(0.0, 10.0, fw - 2.0 - x);
                for q in moon_pieces(x, cy, 3.5 * g.max(0.05), 0.3) {
                    sh = sh.fill(q, jose_yellow().alpha(0.5 * g));
                }
                x += 10.0;
            }
        }
        Kind::Fetching => {
            // As a general bar with progress, a soft lemon fill grows under the looking stars.
            if fw > 0.0 {
                sh = sh.fill(quad(0.0, fw), jose_yellow().alpha(0.4));
            }
            let n = 9;
            let step = bw / n as f32;
            let head = (s.t * 6.0).rem_euclid(n as f32 + 3.0);
            for i in 0..n {
                let d = head - i as f32;
                let a = if (0.0..3.0).contains(&d) { 1.0 - d / 3.0 } else { 0.0 };
                let x = step * (i as f32 + 0.5);
                sh = sh.fill(star_soft(x, cy, 2.4, 0.0), jose_yellow().alpha(0.18 + 0.82 * a));
            }
        }
        Kind::Error => {
            let fwm = fw.max(10.0);
            sh = sh.fill(quad(0.0, fwm), red);
            let ty = (h - 10.0) / 2.0;
            let mut x = -6.0;
            while x < fwm {
                let (x0, x1) = (x.max(0.0), (x + 4.0).min(fwm));
                if x1 > x0 + 0.5 {
                    sh = sh.fill(vec![[x0 + 3.0, ty + 1.0], [x1 + 3.0, ty + 1.0], [x1, ty + 9.0], [x0, ty + 9.0]], navy.alpha(0.8));
                }
                x += 8.0;
            }
        }
    }
    sh
}

/// 0 to 1 over the first fifth of `u`, for fading something in.
fn smooth_in(u: f32) -> f32 {
    (u / 0.2).clamp(0.0, 1.0)
}

/// Jose's percent: the number alone, in the row's colours (red on error), now that the card's
/// smoke keeps it legible.
fn jose_pill(s: &S, t: &Torrent, pct: &str, pw: f32, ph: f32) -> El {
    let ink = if t.kind == Kind::Error {
        hex("#ff5a6a")
    } else if is_selected(s, t) {
        on_select()
    } else {
        paper()
    };
    el().size(pw, ph).center().child(text(pct.to_owned()).font(list_font(13.5, 800.0)).color(ink))
}

fn status(s: &S, w: f32) -> El {
    if jose_shop() {
        return jose_status(s, w);
    }
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

/// A moon `lit` (0 new, 1 full) lit from the right, as horizontal slices between the limb and
/// the terminator: each slice is convex, so it always fills as drawn.
fn moon_phase(cx: f32, cy: f32, r: f32, lit: f32) -> Vec<Vec<[f32; 2]>> {
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

/// The footer moon's phase over time, 0 new to 1 full, as a connection goes: waxing while
/// connecting, holding full, and dropping to new when the connection drops.
fn foot_moon_lit(t: f32) -> f32 {
    let u = t.rem_euclid(8.0);
    if u < 2.5 {
        let x = u / 2.5;
        x * x * (3.0 - 2.0 * x)
    } else if u < 6.5 {
        1.0
    } else {
        // The connection drops: straight to a new moon, in 0.52 s.
        (1.0 - (u - 6.5) / 0.52).max(0.0)
    }
}

/// The selected row's flag, footer-sized: a swallowtail at the left, a point at the right, the
/// edges swaying and bowed.
fn foot_flag(s: &S, w: f32, h: f32) -> Vec<[f32; 2]> {
    // Gentler than the row's flag: half the height, a little over half the pace.
    let sway = |x: f32, k: f32| (s.t * 1.3 + x * 0.008 + k).sin();
    let bow = |x: f32| -3.0 * (1.0 - (2.0 * (x / w) - 1.0).powi(2));
    let n = 20;
    let mut pts = vec![[0.0, 1.0 + sway(0.0, 0.0) + bow(0.0)]];
    for i in 1..=n {
        let x = (w - 10.0) * i as f32 / n as f32;
        pts.push([x, 1.0 + sway(x, 0.0) + bow(x)]);
    }
    pts.push([w, h * 0.5]);
    for i in (0..=n).rev() {
        let x = (w - 10.0) * i as f32 / n as f32;
        pts.push([x, h - 1.0 + sway(x, 1.3) + bow(x)]);
    }
    pts.push([14.0, h * 0.5]);
    pts
}

/// Jose's footer: a cream flag, a touch see-through and frosted, swaying gently, with navy text.
/// The connection state is a moon changing phase as it connects.
fn jose_status(s: &S, w: f32) -> El {
    let navy = hex("#0b1f5c");
    let h = 26.0;
    let font = list_font(13.5, 800.0);
    // The moon: a navy disc, the lit part over it in lemon.
    let lit = foot_moon_lit(s.t);
    let mut m = Shape::new().fill(circle(9.0, 9.0, 7.2), navy).fill(circle(9.0, 9.0, 6.0), navy);
    // A new moon draws no lit part at all: the limb's anti-aliased edge alone showed as a thin
    // yellow rim.
    if lit > 0.02 {
        for q in moon_phase(9.0, 9.0, 6.0, lit) {
            m = m.fill(q, jose_yellow());
        }
    }
    let left = row()
        .items_center()
        .gap(6.0)
        .child(el().size(18.0, 18.0).child(layer(m, 18.0, 18.0)))
        .child(text("Connected".to_owned()).font(font).color(navy));
    let totals = text("6 torrents · ↓ 68 MiB/s · ↑ 1.5 MiB/s".to_owned()).font(list_font(13.5, 800.0)).color(navy);
    let band = foot_flag(s, w - 6.0, h);
    let band: Vec<[f32; 2]> = band.into_iter().map(|p| [p[0] + 3.0, p[1] + 1.0]).collect();
    let glow = Shape::new().fill(inflate(&band, 2.0), hex("#fff6c8").alpha(0.25));
    let body = Shape::new().fill(band, hex("#fff6c8").alpha(0.78));
    el().w(w)
        .h(crate::STATUS_H)
        .backdrop_blur(6.0)
        .child(layer(glow, w, h + 6.0).blur(5.0))
        .child(layer(body, w, h + 6.0))
        .child(row().pin(Some(22.0), Some(1.0), Some(18.0), Some(2.0)).items_center().child(left).child(el().grow(1.0)).child(totals))
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

// ------------------------------------------------------------------ button lettering

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

/// `word` set as `ink` on a button whose face is `bg`: a Superclarendon capital, the rest in
/// Arial Rounded with some letters in other faces, each tilted a little. At least one letter and
/// at most half go lowercase (vowels first), and one letter (never the capital) is boxed in
/// `ink` with its letter in `bg`.
fn lettered(word: &str, ink: Color, bg: Color, seed: u32) -> El {
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

// ------------------------------------------------------------------ row steps

/// How far row `i` steps right: one of five levels, 0–7 pt, at random per row, but never the
/// same level as the row above or a neighbouring one.
fn step_offset(i: usize) -> f32 {
    let levels = 5u32;
    let mut prev = noise(11) as u32 % levels;
    for k in 1..=i {
        let r = (noise(1000 + k as u32) * levels as f32) as u32 % levels;
        prev = if r.abs_diff(prev) <= 1 { (prev + 2 + r % 2) % levels } else { r };
    }
    prev as f32 * 7.0 / (levels - 1) as f32
}

// ------------------------------------------------------------------ speeds

/// The speed `v` (down or up) on a slab whose corners drift, as the game's highlighted shapes:
/// black figures on cyan or pink, or on Jose's shop an italic figure on a near-black,
/// see-through counter plate, over a thin underline in the direction's colour.
fn speed_cell(v: &str, down: bool, t: f32) -> El {
    let (dc, uc) = speed_colours();
    let c = if down { dc } else { uc };
    if jose_shop() {
        let (pw, ph) = (70.0, 20.0);
        let corners = living(pw - 8.0, ph - 4.0, 1.5, t, key_seed(v) ^ down as u32);
        let plate =
            Shape::new().fill(moved(&corners, 4.0, 2.0), hex("#05081a").alpha(0.82)).line([10.0, ph - 4.0], [pw - 8.0, ph - 4.0], 1.2, LineCap::Butt, c.alpha(0.9));
        return el().size(pw, ph).center().child(layer(plate, pw, ph)).child(text(v.to_owned()).font(figures(13.5).italic()).color(c).mb(2.0));
    }
    let (pw, ph) = (66.0, 18.0);
    let corners = living(pw - 8.0, ph - 4.0, 2.5, t, key_seed(v) ^ down as u32);
    let slab = Shape::new().fill(moved(&corners, 4.0, 2.0), c);
    el().size(pw, ph).center().child(layer(slab, pw, ph)).child(text(v.to_owned()).font(figures(13.0)).color(black()))
}

// ------------------------------------------------------------------ progress tracks

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
) -> (Shape, Color) {
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

// ------------------------------------------------------------------ the Jose palette

// ------------------------------------------------------------------ shaped edges

// The rectangular window's edge shapes, after the slanted spike: 1 cut corners, 2 a slight
// lean, 3 torn paper, 4 a title tab, 5 cut corners with teeth along the bottom. Each stays inside
// the window's rectangle, at most `EDGE` in from it.

// ------------------------------------------------------------------ Jose, round 4

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

/// `pts` scaled by `k` about (`x`, `y`).
fn grown(pts: &[[f32; 2]], x: f32, y: f32, k: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|p| [x + (p[0] - x) * k, y + (p[1] - y) * k]).collect()
}

/// Jose's button, after the shop's boards, drawn from the hovered, pressed and disabled amounts
/// (see `button_amounts`), so it's one drawing at every state. At rest a cream rim round a dark
/// navy glass (a secondary button shows its glass alone, the rim coming in with the hover). On
/// hover a lemon sheet comes in over the board, tilted back or forward, its corners wandering
/// so it changes shape as it drifts, overlapping the board but not all of it. The rim keeps its
/// shape throughout: black over the sheet and cream over the sky, turning black only as fast as
/// the sheet appears; the glass fades out as the sheet fades in; the label goes from cream to
/// black with it.
fn jose_shop_button(s: &S, key: &str, label: &str, primary: bool, look: Look) -> El {
    let w = button_w(label) + 6.0;
    let h = BAR_H - 6.0;
    let seed = key_seed(key);
    let [hov, prs, dis] = button_amounts(s, key, look);
    let e = hov.clamp(0.0, 1.1);
    let yellow = jose_yellow();
    let cream = paper();
    let plate = rounded(w, h, 7.0);
    let mut sh = Shape::new();
    let frame: Vec<[f32; 2]> = rounded(w - 6.0, h - 6.0, 2.0).into_iter().map(|p| [p[0] + 3.0, p[1] + 3.0]).collect();
    sh = sh.fill(frame, hex("#020a2c").alpha(0.58 * (1.0 - e).clamp(0.0, 1.0)));
    let t = s.t * 1.95 + noise(seed) * 5.0;
    // Tilted back or forward, at random per button.
    let tilt_by = if noise(seed ^ 0x51) < 0.5 { 0.2 } else { -0.2 };
    let (tc, ts) = (f32::cos(tilt_by), f32::sin(tilt_by));
    let spin = |p: [f32; 2]| {
        let (x, y) = (p[0] - w / 2.0, p[1] - h / 2.0);
        [w / 2.0 + x * tc - y * ts, h / 2.0 + x * ts + y * tc]
    };
    let amp = 5.5 * 0.6;
    let wander = |k: u32| {
        let (sp, ph) = (1.1 + 1.4 * noise(seed + k), noise(seed + k + 9) * 6.0);
        [(t * sp + ph).sin() * amp, (t * sp * 0.8 + ph * 1.7).cos() * amp * 0.7]
    };
    // 10% narrower than the board, about its middle.
    let (bw, bh) = (w * 0.9, h);
    let (ox, oy) = ((w - bw) / 2.0, (h - bh) / 2.0);
    let base = [[ox, oy], [ox + bw, oy], [ox + bw, oy + bh], [ox, oy + bh]].map(spin);
    // Its centre wandering: three sines at unrelated speeds per axis, so it never quite repeats.
    let drift = [
        (t * 0.9).sin() * 1.6 + (t * 2.3 + 1.0).sin() * 1.2 + (t * 3.7 + 2.0).sin() * 0.6,
        (t * 1.3).cos() * 1.0 + (t * 2.9 + 0.5).sin() * 0.8 + (t * 4.1 + 1.5).sin() * 0.4,
    ];
    let sheet: Vec<[f32; 2]> = base
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let o = wander(k as u32);
            [p[0] + drift[0] + o[0] * e, p[1] + drift[1] + o[1] * e]
        })
        .collect();
    // Very slightly rounded.
    let sheet = round_poly(&sheet, 2.5);
    sh = sh.fill(sheet.clone(), yellow.alpha(e.min(1.0)));
    let rim: Vec<[f32; 2]> = rounded(w - 6.0, h - 6.0, 2.0).into_iter().map(|p| [p[0] + 3.0, p[1] + 3.0]).collect();
    let rim_a = if primary { 1.0 } else { e.min(1.0) };
    for q in rim_pieces(&rim, 3.2) {
        sh = sh.fill(q.clone(), cream.alpha(rim_a));
        let inside = clip_convex(&q, &sheet);
        if inside.len() >= 3 {
            sh = sh.fill(inside, black().alpha(e.min(1.0)));
        }
    }
    let ink = cream.mix(black(), e.min(1.0));
    // The glow stays in the tree at zero, so the board's children never change and its springs
    // carry on across hover, leave and idle.
    let glow = layer(Shape::new().fill(inflate(&plate, 3.0), yellow.alpha(0.0)), w, h).blur(6.0);
    let font = TextStyle::new(15.0, 600.0).family(Family::Named(JOSE_FAMILY));
    // Title case, as the shop's boards: "OPEN…" reads "Open...".
    let mut word = label.to_lowercase().replace('…', "...");
    if let Some(f) = word.get(0..1) {
        word = f.to_uppercase() + &word[1..];
    }
    let face = el().size(w, h).center().child(layer(sh, w, h)).child(text(word).font(font).color(ink));
    let sway = (s.t * 1.3 + noise(seed) * 6.0).sin() * 0.01;
    // Pressed eases the board down from its hovered pose: smaller, pressed in.
    let p = prs.clamp(0.0, 1.0);
    let (hs, hl) = (1.0 + 0.06 * e, -2.0 * e);
    let (scale, lift) = (hs + (0.94 - hs) * p, hl + (1.0 - hl) * p);
    // A board leans a little at rest, as the shop's do, and further on hover.
    let turn = tilt(seed, 0.015, 0.03) + sway + tilt(seed ^ 7, 0.02, 0.03) * e;
    // The box is the board's own width: the toolbar counts each button as its face plus 6, and
    // `w` already has the 6. A box 6 wider pushed each button further right, so Remove ran past
    // the window's edge.
    let b = el()
        .size(w, BAR_H)
        .pt(3.0)
        .child(glow.pin(Some(0.0), Some(3.0), None, None))
        .child(face.pin(Some(0.0), Some(3.0), None, None))
        .offset(0.0, lift)
        .scale(scale)
        .scale_motion(kick())
        .rotate(turn);
    // Disabled fades the board to 40%, from wherever it was (hovered included).
    b.opacity(1.0 - 0.6 * dis.clamp(0.0, 1.0)).instant_changes()
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

// ------------------------------------------------------------------ Jose, round 6

/// Jose's night sky: deep blue bands, lit by nebulae; comets sweeping along arcs; a dense field
/// of soft stars, most of them faint, twinkling slowly; and a heap of lemon moons and stars
/// drifting along the bottom, white glints flashing between them. The heap is drawn in a fixed
/// order, so overlapping moons never swap places as they drift.
fn night_sky(s: &S, w: f32, h: f32) -> El {
    let m = 120.0;
    let (bw, bh) = (w + 2.0 * m, h + 2.0 * m);
    let bands = ["#00031a", "#010a2a", "#02123a", "#031a4a", "#05225a", "#082a66", "#0c3474"];
    let mut sky = Shape::new();
    for (k, c) in bands.iter().enumerate() {
        let (y0, y1) = (bh * k as f32 / bands.len() as f32, bh * (k + 1) as f32 / bands.len() as f32 + 1.0);
        sky = sky.fill(vec![[0.0, y0], [bw, y0 - 30.0], [bw, y1 - 30.0], [0.0, y1]], hex(c));
    }
    // Nebulae: broad, lighter clouds, slowly drifting, about half the window: a band from the
    // upper right down towards the middle, leaving the left and the top-left corner deep. Drawn
    // as merging shapes, blurred, with a noise texture over them (`nebula_texture`), both dimmed
    // to 55%.
    let clouds =
        [(0.78, 0.2, 170.0, "#3a5fc0"), (0.58, 0.42, 150.0, "#2f62b0"), (0.9, 0.55, 140.0, "#4a6fd0"), (0.4, 0.68, 120.0, "#2a74b8")];
    let neb: f32 = 0.8;
    let dim: f32 = 0.55;
    sky = merged_nebulae(s, sky, &clouds, m, w, h, neb * dim);
    let sky = layer(sky, bw, bh).blur(30.0).pin(Some(-m), Some(-m), None, None);
    let detail = nebula_texture(s, w, h);

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
    // The stars' halos and softened cores get their own layers, blurred apart from the comets.
    let (mut sg, mut sc, mut ss) = (Shape::new(), Shape::new(), Shape::new());
    // The small stars: a sharp point inside a soft surround of (blur, size, strength, point).
    let small = (0.8, 1.25, 0.25, 0.78);
    let (sg_blur, sg_gain) = (9.0, 1.2);
    // The softened core: blurred a little round a smaller sharp point.
    let (core_blur, point) = (1.5, 0.45);
    // A halo in rings fading outwards, so no edge shows.
    let halo_at = |gl: &mut Shape, x: f32, y: f32, r: f32, c: Color, a: f32| {
        for k in 0..5 {
            let f = 1.0 - k as f32 / 5.0;
            *gl = std::mem::take(gl).fill(circle(x, y, r * (0.3 + 0.7 * f)), c.alpha(a * sg_gain * 0.36));
        }
    };
    for i in 0..2400 {
        let (x, y) = (noise(i * 3 + 1) * w, noise(i * 3 + 2) * h * 0.85);
        // Varied: most stars small, a few large (size cubed skews it), each with its own halo
        // reach and strength, twinkling slowly and only partway, so none blinks out.
        let size = 0.5 + 2.2 * noise(i * 7 + 11).powi(3);
        let reach = 1.5 + 2.5 * noise(i * 7 + 12);
        let rate = 0.3 + 0.7 * noise(i * 3 + 3);
        let tw = 0.65 + 0.35 * (s.t * rate + i as f32 * 1.7).sin();
        // Most stars are faint: brightness skewed low, the small ones dimmest.
        let b = 0.3 + 0.7 * noise(i * 7 + 15).powi(3);
        // At least a visible point: under ~0.6 pt they vanish at 1×.
        let (size, tw) = ((size * (0.6 + 0.4 * b)).max(0.9), tw * b);
        let tint = if noise(i * 7 + 13) < 0.25 { hex("#fff6c8") } else { hex("#bff6ff") };
        halo_at(&mut sg, x, y, size * reach * (0.8 + 0.2 * tw), tint, (0.25 + 0.35 * noise(i * 7 + 14)) * tw);
        let core = 0.25 + 0.75 * tw;
        let cr = size * (0.6 + 0.25 * tw);
        if cr > 1.2 {
            // The bigger cores, softened.
            sc = sc.fill(circle(x, y, cr), hex("#ffffff").alpha(core * 0.8));
            sharp = sharp.fill(circle(x, y, cr * point), hex("#ffffff").alpha(core));
        } else {
            let (_, grow, gain, point) = small;
            ss = ss.fill(circle(x, y, cr * grow), hex("#ffffff").alpha(core * gain));
            sharp = sharp.fill(circle(x, y, cr * point), hex("#ffffff").alpha(core));
        }
        if i % 60 == 0 {
            // A few bright ones, in wide, soft halos.
            halo_at(&mut sg, x, y, 9.0 + 2.0 * tw, hex("#d8f6ff"), 0.35 * tw);
            sc = sc.fill(circle(x, y, 2.0 + 0.4 * tw), hex("#ffffff"));
            sharp = sharp.fill(circle(x, y, (2.0 + 0.4 * tw) * point), hex("#ffffff"));
        }
    }
    // The heap. Each item keeps its place in the drawing order for good: sorting by height made
    // overlapping moons swap as they drifted.
    let drift = s.t * 21.0;
    let lane = w + 260.0;
    // A saturated lemon, its shades running to gold and near-white, so the deeper ones stay
    // warm instead of going muddy.
    let (body, halo) = (hex("#ffec1f"), jose_yellow_glow());
    let (deep, light) = (hex("#ffc21a"), hex("#fffef0"));
    // The heap glows in its own layer, tight and faint.
    let mut hg = Shape::new();
    let (reach, ha, hblur) = (1.06, 0.62, 4.0);
    let mut order: Vec<u32> = (0..140).collect();
    order.sort_by(|a, b| noise(a * 5 + 2).partial_cmp(&noise(b * 5 + 2)).unwrap().reverse());
    for i in order {
        // Each item gets its own slot of the lane, nudged within it, and drifts at about the
        // same speed as the others, a touch apart.
        let slot = (i as f32 * 0.618034).fract();
        let start = slot + (noise(i * 5 + 1) - 0.5) * 2.0 / 140.0;
        let speed = 0.94 + 0.12 * noise(i * 5 + 4);
        let x = (start * lane + drift * speed).rem_euclid(lane) - 130.0;
        let edge = ((x / w - 0.5).abs() * 2.0).powf(2.0);
        let depth = noise(i * 5 + 2);
        let y = h - 8.0 - depth * (80.0 + 190.0 * edge) - (x / w) * 20.0;
        let spin = s.t * 0.35 * (noise(i * 5 + 3) - 0.5) + noise(i * 5 + 6) * 6.0;
        let moon = i % 3 == 0;
        let r = if moon { 20.0 + 30.0 * noise(i * 5 + 3) } else { 8.0 + 16.0 * noise(i * 5 + 3) };
        // Each item's own shade, a little lighter or deeper than the lemon, by depth, so the
        // nearer ones read lighter and overlapping ones are told apart.
        let amp = 0.35;
        let v = noise(i * 5 + 9) - 0.5 + (0.5 - depth) * 0.6;
        let fill = if v > 0.0 { body.mix(light, v * amp) } else { body.mix(deep, -v * amp) };
        // Pieces of the shape, as convex polygons (moons) or one (stars).
        let shape_at = |rr: f32| -> Vec<Vec<[f32; 2]>> {
            if moon {
                moon_pieces(x, y, rr, spin)
            } else {
                let sh = star_soft(x, y, r, spin);
                vec![grown(&sh, x, y, rr / r)]
            }
        };
        for q in shape_at(r * reach) {
            hg = hg.fill(q, halo.alpha(ha));
        }
        // In convex pieces: as one polygon, Rune's ear-clipper sometimes found no ear and
        // fanned the moon into a full disk for a frame.
        for q in shape_at(r) {
            sharp = sharp.fill(q, fill);
        }
    }
    // White glints among the heap, twinkling; any of them flashes a four-point sparkle now and
    // then, each on its own beat of 6–10 s.
    for i in 0..90 {
        let x = (noise(i * 11 + 1) * lane + drift * 0.9).rem_euclid(lane) - 130.0;
        let edge = ((x / w - 0.5).abs() * 2.0).powf(2.0);
        let y = h - 6.0 - noise(i * 11 + 2) * (90.0 + 200.0 * edge) - (x / w) * 20.0;
        let tw = 0.5 + 0.5 * (s.t * (2.0 + 3.0 * noise(i * 11 + 3)) + i as f32 * 1.3).sin();
        glow = glow.fill(circle(x, y, 4.0 + 5.0 * tw), hex("#ffffff").alpha(0.9 * tw));
        sharp = sharp.fill(circle(x, y, 1.0 + 1.4 * tw), hex("#ffffff").alpha(0.5 + 0.5 * tw));
        // How strongly this dot sparkles now, 0–1: a brief flash.
        let period = 6.0 + (10.0 - 6.0) * noise(i * 11 + 5);
        let u = (s.t / period + noise(i * 11 + 6)).fract() * period;
        let d = 0.8;
        let spark = if u < d { (std::f32::consts::PI * u / d).sin() } else { 0.0 };
        if spark > 0.01 {
            // Two thin crossed diamonds.
            let l = (5.0 + 6.0 * tw) * spark.max(0.4);
            let c = hex("#ffffff").alpha(0.9 * spark);
            sharp = sharp
                .fill(vec![[x - l, y], [x, y - 0.9], [x + l, y], [x, y + 0.9]], c)
                .fill(vec![[x, y - l], [x + 0.9, y], [x, y + l], [x - 0.9, y]], c);
        }
    }
    el().w(w)
        .h(h)
        .clip_rounded(0.0)
        .child(sky)
        .child(detail)
        .child(layer(sg, w, h).blur(sg_blur))
        .child(layer(sc, w, h).blur(core_blur))
        .child(layer(ss, w, h).blur(small.0))
        .child(layer(glow, w, h).blur(7.0))
        .child(layer(hg, w, h).blur(hblur))
        .child(layer(sharp, w, h))
}

/// Value noise at `(x, y)`: random values on a unit grid, smoothly interpolated. 0–1.
fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |i: i32, j: i32| noise((i as u32).wrapping_mul(73856093) ^ (j as u32).wrapping_mul(19349663) ^ seed);
    let (i, j) = (xi as i32, yi as i32);
    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

/// Fractal noise: `oct` octaves of value noise, each twice as fine and half as strong. 0–1.
fn fbm(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
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
fn ridged(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
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

thread_local! {
    /// Nebula textures by size and seed, made once.
    static NEB_TEX: std::cell::RefCell<std::collections::HashMap<(u32, u32), rune::Pixels>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The nebulae as a texture: domain-warped fractal noise, so the clouds billow at every scale
/// like the photographs, in the sky's blues only, with dark dust lanes and bright filaments.
/// Made once, a little larger than the window, and drifted slowly. Two skies from different
/// seeds cross-fade very slowly (90 s each way), both overlapping throughout, so the nebula
/// seems to change shape without being seen to unless watched for. Dimmed to 55%.
fn nebula_texture(s: &S, w: f32, h: f32) -> El {
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

fn nebula_rgba(tw: u32, th: u32, seed: u32) -> Vec<u8> {
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
    let colour = |v: f32| {
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
            let c = colour(glow);
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
fn merged_nebulae(s: &S, mut sky: Shape, clouds: &[(f32, f32, f32, &str)], m: f32, w: f32, h: f32, neb: f32) -> Shape {
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
