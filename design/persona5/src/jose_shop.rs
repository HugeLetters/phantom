//! Jose's shop's own components: the brand, button, progress bar and its patterns, the percent pill, the footer with its moon, and the night sky.

use rune::{Color, El, Family, Shape, TextStyle, el, row, text};

use crate::common::*;
use crate::motion::*;
use crate::nebula::*;
use crate::noise::*;
use crate::paint::*;
use crate::shapes::*;
use crate::themes::*;
use crate::{BAR_H, button_w};

/// Jose's logo, after the shop's titles: "Phantom" in pale, glowing letters, like light through
/// a tube, each letter its own size, height and tilt. A bright core in a deep green glow, the
/// core a little see-through.
pub(crate) fn jose_shop_brand() -> El {
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

/// The fill colour of Jose's bars by state: lemon, a cooler cyan once seeding, pale when paused,
/// red on error.
pub(crate) fn jose_shop_bar_fill(t: &Torrent) -> Color {
    match t.kind {
        Kind::Seeding => hex("#7fe3ff"),
        Kind::Paused => jose_shop_yellow().alpha(0.45),
        Kind::Error => hex("#ff5a6a"),
        _ => jose_shop_yellow(),
    }
}

/// Jose's bars: a rounded navy track with a thin pale rim, the fill glowing, and a pattern per
/// state in Jose's motifs (see `jose_shop_pattern`). An errored bar twitches now and then; a paused
/// one has z's rising from its sleeping crescents.
pub(crate) fn jose_shop_bar(s: &S, t: &Torrent, bw: f32, h: f32) -> El {
    let navy = hex("#0b1f5c");
    let sel = is_selected(s, t);
    let p = t.progress.clamp(0.0, 1.0);
    // Fetching has no fill until it's given progress (as a general bar).
    let fw = (bw * p).max(if p > 0.0 { 8.0 } else { 0.0 });
    let (th, ty) = (10.0, (h - 10.0) / 2.0);
    let quad = move |x0: f32, x1: f32| -> Vec<[f32; 2]> {
        rounded((x1 - x0).max(th), th, th / 2.0).into_iter().map(|q| [q[0] + x0, q[1] + ty]).collect()
    };
    let track = quad(0.0, bw);
    let track_c = if sel { hex("#fffbe6").alpha(0.75) } else { navy.alpha(0.85) };
    let mut sh = Shape::new().fill(track.clone(), track_c);
    sh = jose_shop_pattern(s, t, sh, bw, h, fw, sel, &quad);
    let rim: Vec<[f32; 2]> = track.iter().copied().chain(std::iter::once(track[0])).collect();
    let rim_c = if sel { navy.alpha(0.8) } else { jose_shop_pale().alpha(0.7) };
    sh = sh.stroke(rim, 1.0, rim_c);
    let mut e = el().size(bw, h);
    if fw > 0.0 {
        let c = if t.kind == Kind::Seeding { hex("#7fe3ff") } else { jose_shop_bar_fill(t) };
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
                e = e.child(text("z".to_owned()).font(list_font(6.5, 900.0)).color(jose_shop_pale().alpha(a)).pin(
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
pub(crate) fn jose_shop_pattern(
    s: &S,
    t: &Torrent,
    mut sh: Shape,
    bw: f32,
    h: f32,
    fw: f32,
    sel: bool,
    quad: &dyn Fn(f32, f32) -> Vec<[f32; 2]>,
) -> Shape {
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
            let c = if sel { navy } else { jose_shop_yellow() };
            sh = sh.fill(quad(0.0, fw), c);
            let pitch = 14.0;
            let mut x = -pitch + (s.t * 18.0).rem_euclid(pitch);
            while x < fw {
                let a = fade(x);
                if a > 0.01 {
                    let l = 3.4 * (0.7 + 0.3 * (s.t * 3.0 + x * 0.3).sin());
                    let c = if sel { jose_shop_yellow().alpha(a) } else { hex("#ffffff").alpha(0.95 * a) };
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
            sh = sh.fill(quad(0.0, fw), jose_shop_pale().alpha(if sel { 0.7 } else { 0.9 }));
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
                    sh = sh.fill(q, jose_shop_yellow().alpha(0.5 * g));
                }
                x += 10.0;
            }
        }
        Kind::Fetching => {
            // As a general bar with progress, a soft lemon fill grows under the looking stars.
            if fw > 0.0 {
                sh = sh.fill(quad(0.0, fw), jose_shop_yellow().alpha(0.4));
            }
            let n = 9;
            let step = bw / n as f32;
            let head = (s.t * 6.0).rem_euclid(n as f32 + 3.0);
            for i in 0..n {
                let d = head - i as f32;
                let a = if (0.0..3.0).contains(&d) { 1.0 - d / 3.0 } else { 0.0 };
                let x = step * (i as f32 + 0.5);
                sh = sh.fill(star_soft(x, cy, 2.4, 0.0), jose_shop_yellow().alpha(0.18 + 0.82 * a));
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

/// Jose's percent: the number alone, in the row's colours (red on error), now that the card's
/// smoke keeps it legible.
pub(crate) fn jose_shop_pill(s: &S, t: &Torrent, pct: &str, pw: f32, ph: f32) -> El {
    let ink = if t.kind == Kind::Error {
        hex("#ff5a6a")
    } else if is_selected(s, t) {
        on_select()
    } else {
        paper()
    };
    el().size(pw, ph).center().child(text(pct.to_owned()).font(list_font(13.5, 800.0)).color(ink))
}

/// The footer moon's phase over time, 0 new to 1 full, as a connection goes: waxing while
/// connecting, holding full, and dropping to new when the connection drops.
pub(crate) fn foot_moon_lit(t: f32) -> f32 {
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
pub(crate) fn foot_flag(s: &S, w: f32, h: f32) -> Vec<[f32; 2]> {
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
pub(crate) fn jose_shop_status(s: &S, w: f32) -> El {
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
            m = m.fill(q, jose_shop_yellow());
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

/// Jose's button, after the shop's boards, drawn from the hovered, pressed and disabled amounts
/// (see `button_amounts`), so it's one drawing at every state. At rest a cream rim round a dark
/// navy glass (a secondary button shows its glass alone, the rim coming in with the hover). On
/// hover a lemon sheet comes in over the board, tilted back or forward, its corners wandering
/// so it changes shape as it drifts, overlapping the board but not all of it. The rim keeps its
/// shape throughout: black over the sheet and cream over the sky, turning black only as fast as
/// the sheet appears; the glass fades out as the sheet fades in; the label goes from cream to
/// black with it.
pub(crate) fn jose_shop_button(s: &S, key: &str, label: &str, primary: bool, look: Look) -> El {
    let w = button_w(label) + 6.0;
    let h = BAR_H - 6.0;
    let seed = key_seed(key);
    let [hov, prs, dis] = button_amounts(s, key, look);
    let e = hov.clamp(0.0, 1.1);
    let yellow = jose_shop_yellow();
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
    let font = TextStyle::new(15.0, 600.0).family(Family::Named(JOSE_SHOP_FAMILY));
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

/// Jose's night sky: deep blue bands, lit by nebulae; comets sweeping along arcs; a dense field
/// of soft stars, most of them faint, twinkling slowly; and a heap of lemon moons and stars
/// drifting along the bottom, white glints flashing between them. The heap is drawn in a fixed
/// order, so overlapping moons never swap places as they drift.
pub(crate) fn jose_shop_sky(s: &S, w: f32, h: f32) -> El {
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
        let c = if k % 2 == 0 { hex("#8ff4ff") } else { jose_shop_pale() };
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
    let (body, halo) = (hex("#ffec1f"), jose_shop_yellow_glow());
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
