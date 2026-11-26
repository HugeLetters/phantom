//! The two themes, Phantom Thieves and Jose's shop, as one set of [`Parts`]: each part draws
//! either theme, by [`jose_shop`].
//!
//! Phantom Thieves: maximal and animated. A red ground with a slowly turning starburst and black
//! halftone, the list on a tilted black sheet with a white outline, cut-out slabs for buttons and
//! the selection. Everything moves: rows slam in, buttons swell and tilt on hover with red and
//! cyan copies peeking out, progress bars animate by status, the ghost bobs. Active things (a
//! hovered button, a hovered row) throw out a shape at a random tilt, so every activation lands
//! differently. Ransom letters for the title, column headings and the connection state; mixed
//! faces on the buttons; Optima for the torrents themselves.
//!
//! Jose's shop: a navy night sky with nebulae, comets, stars and a drifting heap of lemon moons;
//! the list on a smoky see-through card; boards for buttons after the shop's signs; a swaying
//! lemon flag for the selection; SF Compact Rounded throughout.

use rune::{Color, El, Family, LineCap, Shape, TextStyle, col, el, row, text};

use crate::common::*;
use crate::jose_shop::*;
use crate::motion::*;
use crate::noise::*;
use crate::paint::*;
use crate::phantom_thieves::*;
use crate::shapes::*;
use crate::{BAR_H, Parts, ROW_H, cells};

pub static PARTS: Parts =
    Parts { background, brand, field, button, panel, header, row: torrent_row, progress, list_font, cell, status, errors };

/// Torrent names and statuses: Optima, the macOS cousin of the Optima Nova Black that Persona 5
/// Royal sets its English lists in.
pub(crate) fn list_font(size: f32, weight: f32) -> TextStyle {
    if jose_shop() {
        return TextStyle::new(size * 0.95, 600.0).family(Family::Named(JOSE_SHOP_FAMILY));
    }
    TextStyle::new(size, weight.max(800.0)).family(Family::Named("Optima"))
}

pub(crate) fn background(s: &S, w: f32, h: f32) -> El {
    if jose_shop() {
        return jose_shop_sky(s, w, h);
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
pub(crate) fn brand(_: &S) -> El {
    if jose_shop() {
        // 5 points higher: clear of the buttons at rest (a hovered one may pass over), without
        // clipping the tallest letters at the window's top.
        return jose_shop_brand().mt(2.0).offset(0.0, -5.0);
    }
    ransom_mix("PHANTOM", 19.0, 5, &[(paper(), black()), (black(), paper()), (paper(), red()), (black(), red())], &[paper(), black()], 0.7)
        .mt(2.0)
}

pub(crate) fn field(_: &S, w: f32, hint: &str) -> El {
    // Jose's tape shares the footer's cream.
    let (paper_c, hint_c) =
        if jose_shop() { (hex("#fff6c8").alpha(0.93), hex("#2b3f8c").alpha(0.8)) } else { (paper().alpha(0.93), hex("#555")) };
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

pub(crate) fn button(s: &S, key: &str, label: &str, primary: bool, look: Look) -> El {
    if jose_shop() {
        return jose_shop_button(s, key, label, primary, look);
    }
    let [hov, prs, dis] = button_amounts(s, key, look);
    phantom_thieves_button(s, key, label, primary, hov, prs, dis.clamp(0.0, 1.0))
}

pub(crate) fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    let sheet = jagged(0.0, 0.0, w - 10.0, h - 6.0, 1.2, 40.0, 77);
    // The paper clips the rows sliding in, but leaves room for the selection's overhang.
    let clip =
        el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
    if jose_shop() {
        // A half see-through card of deep navy smoke, denser behind the header and clearing
        // towards the heap, thickening again at the bottom so the rows over the heap read; a
        // lemon rim in a soft glow.
        let smoke = hex("#020a2c");
        let ring = sheet.iter().copied().chain(std::iter::once(sheet[0])).collect::<Vec<_>>();
        let glow = Shape::new().stroke(ring.clone(), 4.0, jose_shop_yellow_glow().alpha(0.7));
        let rim = Shape::new().stroke(ring, 1.4, jose_shop_pale());
        let (cw, ch) = (w - 10.0, h - 6.0);
        // Denser at the top, gone by 60% of the way down; blurred into a gradient.
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
        // From 60% down, the smoke thickens again, steepening to about 78% at the very bottom.
        for k in 0..10 {
            let (y0, y1) = (ch * (0.6 + 0.04 * k as f32), ch * (0.6 + 0.04 * (k + 1) as f32) + 1.0);
            let a = 0.18 + 0.6 * ((k as f32 + 1.0) / 10.0).powf(1.3);
            g = g.fill(vec![[-40.0, y0], [cw + 40.0, y0], [cw + 40.0, y1], [-40.0, y1]], smoke.alpha(a));
        }
        let card = el().size(cw, ch).fill(smoke.alpha(0.18)).clip_rounded(0.0).child(layer(g, cw + 80.0, ch).blur(20.0).pin(
            Some(-40.0),
            Some(0.0),
            None,
            None,
        ));
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
pub(crate) fn heading(word: &str, seed: u32) -> El {
    if !jose_shop() {
        return ransom_mix(word, 11.5, seed, &[(red(), paper()), (black(), paper())], &[black()], 0.18);
    }
    let navy = hex("#0b1f5c");
    let mut r = row().items_center();
    for (i, ch) in word.chars().enumerate() {
        let n = seed.wrapping_mul(31).wrapping_add(i as u32 * 7);
        r = r.child(
            text(ch.to_string())
                .font(list_font(12.5, 800.0))
                .color(navy)
                .offset(0.0, (noise(n) - 0.5) * 3.0)
                .rotate((noise(n + 1) - 0.5) * 0.18),
        );
    }
    r
}

pub(crate) fn header(_: &S, w: f32) -> El {
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
        let glow = layer(Shape::new().fill(inflate(&strip, 3.0), jose_shop_yellow_glow().alpha(0.3)), w, 30.0).blur(5.0);
        let sh = Shape::new().fill(strip, jose_shop_yellow().alpha(0.94));
        return el().w(w + 10.0).h(30.0).child(glow).child(layer(sh, w, 30.0)).child(r);
    }
    el().w(w + 10.0).h(30.0).child(layer(Shape::new().fill(strip, paper()), w, 30.0)).child(r)
}

pub(crate) fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &torrent(s, i);
    let w = w - 10.0;
    let key = format!("row-{i}");
    let mut bg = Shape::new();
    // Hovered and selected as eased amounts: the band, the hover flag or shard, the nudge and
    // the tilt are all drawn from these, so selecting, deselecting and hovering in any order
    // animate from wherever the row is.
    let (ha, sa, band_seed) = row_amounts(s, &key, matches!(look, Look::Hover | Look::Press), selected, selection_seed(s, t));
    // The text turns to the selection's colors once the band is half in (or half out).
    let shown_sel = sa > 0.5;
    let (name, muted, mut status_c) = if shown_sel { (on_select(), on_select(), on_select()) } else { (paper(), muted(), hex("#e2e2e2")) };
    if t.kind == Kind::Error && !shown_sel {
        status_c = error_c();
    }
    let mut turn = 0.0;
    // Jose's flag glows through a blur.
    let mut glow_sh: Option<Shape> = None;
    // On Jose's shop, hovering draws the selection's own flag, the same shape, at a quarter
    // strength, so clicking only fills its color in. Pressed counts as hovered here, so the
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
            glow_sh = Some(Shape::new().fill(inflate(&band, 2.0), jose_shop_yellow_glow().alpha(0.32 * k)));
            // The pale sliver under the flag stays off on hover: through the faint flag it read
            // as a second, smaller flag.
            let sliver_a = 0.5 * fade_in * sa.min(1.0);
            bg = bg.fill(sliver, jose_shop_pale().alpha(sliver_a)).fill(band, jose_shop_yellow().alpha(0.94 * k));
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
    let row_el =
        el().w(w).h(ROW_H).child(layer(glow_sh.unwrap_or_default(), w, ROW_H).blur(6.0)).child(layer(bg, w, ROW_H)).child(cells_el);
    let row_el = if jose_shop() {
        row_el.offset((1.0 - enter.clamp(0.0, 1.0)) * 28.0, 0.0).opacity(enter.clamp(0.0, 1.0))
    } else {
        row_el.offset((1.0 - enter) * 900.0, 0.0)
    };
    row_el.rotate(turn).rotate_motion(kick())
}

/// How far each row steps right of the one above.
pub(crate) const STEP: f32 = 3.0;

pub(crate) fn is_selected(s: &S, t: &Torrent) -> bool {
    // By name: `TORRENTS` is a const, so each use is its own copy and addresses don't match.
    (0..count(s)).any(|i| row_selected(s, i) && torrent(s, i).name == t.name)
}

/// The seed of the current selection: it changes when the selection does, not on every hover.
pub(crate) fn selection_seed(s: &S, t: &Torrent) -> u32 {
    key_seed(t.name) ^ ((s.selected_at * 1000.0) as i32 as u32).wrapping_mul(2654435761)
}

/// The cells drawn their own way: the selected name (with one letter inverted on the Phantom
/// Thieves; plain in the row's black on Jose's shop), and the speeds on slabs.
pub(crate) fn cell(s: &S, t: &Torrent, col: &str, w: f32) -> Option<El> {
    match col {
        "Name" if is_selected(s, t) && jose_shop() => {
            Some(text(t.name.to_owned()).font(list_font(14.0, 800.0)).color(on_select()).truncate().w(w))
        }
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
pub(crate) fn muted() -> Color {
    if jose_shop() { hex("#f3eeb6") } else { hex("#a8a8a8") }
}

/// A different bar for each state, so the state reads at a glance, and the percent beside it.
pub(crate) fn progress(s: &S, t: &Torrent, w: f32) -> El {
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
        return row().w(w).items_center().gap(6.0).child(jose_shop_bar(s, t, bw, h)).child(jose_shop_pill(s, t, &pct, pw, ph));
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

pub(crate) fn status(s: &S, w: f32) -> El {
    if jose_shop() {
        return jose_shop_status(s, w);
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

pub(crate) fn errors(s: &S, w: f32) -> El {
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

/// How far row `i` steps right: one of five levels, 0–7 pt, at random per row, but never the
/// same level as the row above or a neighbouring one.
pub(crate) fn step_offset(i: usize) -> f32 {
    let levels = 5u32;
    let mut prev = noise(11) as u32 % levels;
    for k in 1..=i {
        let r = (noise(1000 + k as u32) * levels as f32) as u32 % levels;
        prev = if r.abs_diff(prev) <= 1 { (prev + 2 + r % 2) % levels } else { r };
    }
    prev as f32 * 7.0 / (levels - 1) as f32
}

/// The speed `v` (down or up) on a slab whose corners drift, as the game's highlighted shapes:
/// black figures on cyan or pink, or on Jose's shop an italic figure on a near-black,
/// see-through counter plate, over a thin underline in the speed's color.
pub(crate) fn speed_cell(v: &str, down: bool, t: f32) -> El {
    let (dc, uc) = speed_colors();
    let c = if down { dc } else { uc };
    if jose_shop() {
        let (pw, ph) = (70.0, 20.0);
        let corners = living(pw - 8.0, ph - 4.0, 1.5, t, key_seed(v) ^ down as u32);
        let plate = Shape::new().fill(moved(&corners, 4.0, 2.0), hex("#05081a").alpha(0.82)).line(
            [10.0, ph - 4.0],
            [pw - 8.0, ph - 4.0],
            1.2,
            LineCap::Butt,
            c.alpha(0.9),
        );
        return el()
            .size(pw, ph)
            .center()
            .child(layer(plate, pw, ph))
            .child(text(v.to_owned()).font(figures(13.5).italic()).color(c).mb(2.0));
    }
    let (pw, ph) = (66.0, 18.0);
    let corners = living(pw - 8.0, ph - 4.0, 2.5, t, key_seed(v) ^ down as u32);
    let slab = Shape::new().fill(moved(&corners, 4.0, 2.0), c);
    el().size(pw, ph).center().child(layer(slab, pw, ph)).child(text(v.to_owned()).font(figures(13.0)).color(black()))
}
