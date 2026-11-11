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
    let slab = cut(w - 8.0, h, 7.0, 41);
    let s = Shape::new().fill(moved(&slab, 6.0, 5.0), black()).fill(inflate(&slab, 2.5), black()).fill(slab, paper());
    el().size(w, BAR_H).child(layer(s, w, BAR_H)).rotate(-0.006).child(
        row()
            .pin(Some(12.0), Some(0.0), Some(12.0), Some(6.0))
            .items_center()
            .child(text(hint.to_owned()).font(sans(12.5, 600.0)).color(hex("#555")).truncate()),
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
/// Round 1 (styles 0–6: before, A–F) is in `out/v/ad34af2-hovers.png`; round 2 keeps the user's
/// three and mixes them: (style, name, what it tries).
pub const HOVERS: [(u8, &str, &str); 7] = [
    (2, "B  Slab comes alive", "round 1: the slab drifts, cyan copy behind"),
    (4, "D  Colour split", "round 1, slowed to B's pace"),
    (6, "F  Its own copies", "round 1: still slab, copies follow it"),
    (7, "G  Living + copies", "slab drifts, copies follow its shape"),
    (8, "H  Loose copies", "still slab, copies drift on their own"),
    (10, "J  All loose", "slab drifts, copies drift their own way"),
    (9, "K  Living split", "slab drifts, colour split follows it"),
];

/// `since` is how long the pointer has been over the button, for the hover's entrance.
#[allow(clippy::too_many_arguments)]
fn button_styled(s: &S, key: &str, label: &str, primary: bool, look: Look, lettering: u8, style: u8, since: f32) -> El {
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let seed = active_seed(s, key);
    let slab = cut(w, h, 14.0, key_seed(key));
    let active = matches!(look, Look::Hover | Look::Press);
    let press = look == Look::Press;
    let (mut fill, mut ink, mut edge, flash) = match (look, primary) {
        (Look::Disabled, _) => (deep(), hex("#d2505a"), deep(), None),
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
    let e = if press { 1.0 } else { snap(since, 0.2) };
    // Loose copies grow out from the face's centre.
    let grown = |pts: Vec<[f32; 2]>| -> Vec<[f32; 2]> {
        let k = 0.75 + 0.25 * e;
        pts.into_iter().map(|p| [w / 2.0 + (p[0] - w / 2.0) * k, h / 2.0 + (p[1] - h / 2.0) * k]).collect()
    };
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
            2 => {
                face_pts = living(w, h, 3.0, s.t, seed);
                back = back.fill(
                    moved(&living(w, h, 3.0, s.t, seed), e * if press { 2.0 } else { 6.0 }, e * if press { 2.0 } else { 5.0 }),
                    cyan(),
                );
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
            4 => {
                // Drifting at the living corners' pace (round 1 jittered 30 times a second).
                let j = |n: u32, a: f32| a + a * wave(s.t, seed.wrapping_add(n));
                let amp = e * if press { 2.0 } else { 4.0 };
                back = back
                    .fill(moved(&slab, -j(1, amp), -j(2, amp) * 0.6), hex("#ff2bd6"))
                    .fill(moved(&slab, j(3, amp), j(4, amp) * 0.6), cyan())
                    .fill(moved(&slab, -j(5, amp) * 0.5, j(6, amp)), hex("#ffe600"));
                if !press {
                    (fill, ink, edge) = (black(), paper(), black());
                }
                shadow = false;
                (scale, turn) = (1.0, 0.0);
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
                back = if matches!(style, 8 | 10) {
                    back.fill(moved(&grown(living(w, h, 4.0, s.t * 0.9, seed.wrapping_add(101))), -5.0 * e, -4.0 * e), cyan())
                        .fill(moved(&grown(living(w, h, 4.0, s.t * 1.1, seed.wrapping_add(202))), 5.0 * e, 4.0 * e), red())
                } else {
                    back.fill(moved(&face_pts, -d(1), -d(2) * 0.7), cyan()).fill(moved(&face_pts, d(3), d(4) * 0.7), red())
                };
                if !press {
                    (fill, ink, edge) = (hex("#fbf6e8"), black(), black());
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
        face_sh = hatch(face_sh, w, h, 5.0, 1.2, hex("#8a0a12"), 0.0);
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
    let mut s = Shape::new().fill(moved(&sheet, 10.0, 9.0), black()).fill(inflate(&sheet, 3.5), paper()).fill(sheet, black());
    // A faint screentone in the sheet's bottom corner.
    s = halftone(s, w - 14.0, h - 10.0, 6.0, hex("#1e1e1e"), |x, y| ((x / (w - 14.0)) * (y / (h - 10.0)) * 3.4 - 0.6).max(0.0));
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
    let strip = moved(&cut(w, 24.0, 12.0, 5), -2.0, 2.0);
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
    el().w(w + 10.0).h(30.0).child(layer(Shape::new().fill(strip, paper()), w, 30.0)).child(r)
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &torrent(s, i);
    let w = w - 10.0;
    let key = format!("row-{i}");
    let mut bg = Shape::new();
    let (name, muted, mut status_c) = if selected { (paper(), paper(), paper()) } else { (paper(), hex("#a8a8a8"), hex("#e2e2e2")) };
    if t.kind == Kind::Error && !selected {
        status_c = red();
    }
    let mut turn = 0.0;
    if selected {
        // A red band with a wedge out past the left edge, cut fresh for each selection, slamming
        // in from the left.
        let seed = selection_seed(s, t);
        let f = snap(s.t - s.selected_at, 0.26);
        let lean = 4.0 + 6.0 * noise(seed);
        let h = ROW_H;
        let band =
            vec![[-26.0, h / 2.0 + 2.0], [-10.0 + lean, 2.0], [w + 10.0 + lean, 0.0], [w + 4.0 - lean, h - 3.0], [-12.0 - lean, h - 1.0]];
        let band: Vec<[f32; 2]> = band.into_iter().map(|p| [-26.0 + (p[0] + 26.0) * f, p[1]]).collect();
        // Under it, a cyan sliver that never rests, as on the game's highlighted rows.
        let sliver: Vec<[f32; 2]> =
            living(w * 0.4, h - 10.0, 4.0, s.t, seed).into_iter().map(|p| [w * 0.6 + p[0] * f, p[1] + 5.0]).collect();
        bg = bg.fill(sliver, cyan()).fill(band, red());
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
    let nudge = if look == Look::Hover && !selected { 8.0 } else { 0.0 };
    let cells_el = cells(s, &PARTS, t, w - STEP * 7.0, name, muted, status_c).offset(step + nudge, 0.0).offset_motion(kick());
    // Rows slam in from the right, one after another, when the window appears.
    let enter = snap(s.t - 0.15 - i as f32 * 0.07, 0.38);
    el().w(w).h(ROW_H).child(layer(bg, w, ROW_H)).child(cells_el).offset((1.0 - enter) * 900.0, 0.0).rotate(turn).rotate_motion(kick())
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
                    .child(text(before).font(font).color(paper()))
                    .child(el().fill(paper()).px(1.5).child(text(chars[k].to_string()).font(font).color(black())))
                    .child(text(after).font(font).color(paper()).truncate()),
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
    let mut sh = match s.track_style {
        // A, leaning.
        6 => Shape::new().fill(inflate(&lean, 1.5), paper()).fill(lean.clone(), black()),
        // B, leaning.
        7 => Shape::new().fill(moved(&lean, 2.5, 2.5), paper()).fill(lean.clone(), black()),
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
    let pct = match t.kind {
        Kind::Downloading => {
            // An arrowhead driving right, white stripes racing through it.
            let body = (fw - 8.0).max(0.0);
            sh = sh.fill(vec![[0.0, 0.0], [body, 0.0], [fw + 4.0, h / 2.0], [body, h], [0.0, h]], red());
            sh = hatch(sh, (body - 4.0).max(0.0), h, 9.0, 3.0, paper().alpha(0.85), s.t * 26.0);
            ink = red();
            None
        }
        Kind::Seeding => {
            // Full; paper with red chevrons marching out, as it gives back.
            sh = sh.fill(vec![[0.0, 0.0], [fw, 0.0], [fw, h], [0.0, h]], paper());
            let pitch = 12.0;
            let mut x = -pitch + (s.t * 10.0).rem_euclid(pitch);
            while x < fw - 12.0 {
                if x > 0.0 {
                    sh = sh.stroke(vec![[x, 2.5], [x + 4.5, h / 2.0], [x, h - 2.5]], 2.4, red());
                }
                x += pitch;
            }
            None
        }
        Kind::Checking => {
            // Halftone, with a red slit scanning the checked part.
            sh = sh.fill(vec![[0.0, 0.0], [fw, 0.0], [(fw - 5.0).max(0.0), h], [0.0, h]], paper());
            sh = halftone(sh, (fw - 4.0).max(0.0), h, 4.0, black(), |x, _| 0.4 + 1.2 * (x / fw));
            let sx = (s.t * 0.9).fract() * fw;
            sh = sh.fill(vec![[sx, -2.0], [sx + 4.0, -2.0], [sx + 1.0, h + 2.0], [sx - 3.0, h + 2.0]], red());
            None
        }
        Kind::Paused => {
            // Frozen: grey blocks with gaps, and nothing moves.
            let mut x = 0.0;
            while x < fw {
                let bx = (x + 7.0).min(fw);
                sh = sh.fill(vec![[x + 2.0, 1.5], [bx, 1.5], [bx - 2.0, h - 1.5], [x, h - 1.5]], hex("#9a9a9a"));
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
            sh = sh.fill(torn, red());
            sh = sh.stroke(vec![[fw + 6.0, 0.0], [fw + 12.0, 6.0], [fw + 9.0, 8.0], [fw + 16.0, h]], 1.6, red());
            ink = red();
            None
        }
        Kind::Fetching => {
            // No size yet: a paper shard sliding to and fro.
            let x = (s.t * 2.4).sin() * 0.5 + 0.5;
            let sw = 22.0;
            let sx = x * (bw - sw);
            sh = sh.fill(vec![[sx + 5.0, 1.0], [sx + sw, 1.0], [sx + sw - 5.0, h - 1.0], [sx, h - 1.0]], paper());
            Some("??".to_owned())
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
    let c = if down { hex("#4fe3e3") } else { hex("#ff5ec4") };
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
/// Round 1 (styles 0–5: before, A–E) is in `out/v/297ec2c-tracks.png`; round 2 sets A, B and D
/// beside their leaning versions.
pub const TRACKS: [(u8, &str, &str); 8] = [
    (1, "A  White rim", "round 1"),
    (6, "A  Leaning", "a leaning, uneven quad"),
    (2, "B  White shadow", "round 1"),
    (7, "B  Leaning", "a leaning, uneven quad"),
    (4, "D  Sticker", "round 1"),
    (8, "D  Leaning", "a leaning, uneven quad"),
    (9, "D  Leaning, inverted", "grey gap, black track"),
    (10, "D  Inverted", "grey gap, black track, straight"),
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
