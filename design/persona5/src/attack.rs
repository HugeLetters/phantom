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
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let seed = active_seed(s, key);
    let slab = cut(w, h, 14.0, key_seed(key));
    let (fill, ink, edge, flash) = match (look, primary) {
        (Look::Disabled, _) => (deep(), hex("#d2505a"), deep(), None),
        (Look::Press, _) => (red(), paper(), black(), Some(black())),
        (Look::Hover, true) => (black(), paper(), paper(), Some(paper())),
        (Look::Hover, false) => (paper(), black(), black(), Some(black())),
        (Look::Idle, true) => (paper(), red(), black(), None),
        (Look::Idle, false) => (black(), paper(), paper(), None),
    };
    // The blade bursts out from behind the face when the button becomes active.
    let mut back = Shape::new();
    if let Some(c) = flash {
        let grow = snap(s.t - s.hovered_at, 0.2).max(if look == Look::Press { 1.0 } else { 0.0 });
        let b: Vec<[f32; 2]> =
            blade(w, h, 9.0, seed).into_iter().map(|p| [w / 2.0 + (p[0] - w / 2.0) * grow, h / 2.0 + (p[1] - h / 2.0) * grow]).collect();
        let edge_c = if c == paper() { black() } else { paper() };
        back = back.fill(inflate(&b, 2.0), edge_c).fill(b, c);
    }
    // The shadow stays put; the face jumps off it on hover and slams into it on press.
    let (dx, dy, sd) = match look {
        Look::Hover => (-2.0, -3.0, 6.0),
        Look::Press => (3.0, 3.0, 2.0),
        _ => (0.0, 0.0, 4.0),
    };
    let mut face_sh = Shape::new().fill(inflate(&slab, 2.0), edge).fill(slab.clone(), fill);
    if look == Look::Disabled {
        face_sh = hatch(face_sh, w, h, 5.0, 1.2, hex("#8a0a12"), 0.0);
    }
    let label_el = if look == Look::Disabled {
        text(label.to_owned()).font(caps(15.0, 900.0).tracking(1.0)).color(ink)
    } else {
        ransom_mix(label, 13.5, key_seed(label), &[(red(), paper()), (black(), paper()), (paper(), black())], &[ink], 0.22)
    };
    let face = el().size(w, h).center().child(layer(face_sh, w, h)).child(label_el).offset(dx, dy).offset_motion(jump());
    let (scale, turn) = match look {
        Look::Hover => (1.08, tilt(seed, 0.04, 0.11)),
        Look::Press => (0.92, tilt(seed, 0.02, 0.05)),
        _ => (1.0, 0.0),
    };
    let mut b = el().size(w + 6.0, BAR_H).pt(3.0).child(layer(back, w, h).pin(Some(0.0), Some(3.0), None, None));
    if look != Look::Disabled {
        b = b.child(layer(Shape::new().fill(moved(&slab, sd, sd), black()), w, h).pin(Some(0.0), Some(3.0), None, None));
    }
    b.child(face.pin(Some(0.0), Some(3.0), None, None)).scale(scale).scale_motion(kick()).rotate(turn).rotate_motion(kick())
}

fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    let sheet = jagged(0.0, 0.0, w - 10.0, h - 6.0, 1.2, 40.0, 77);
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
    let t = &TORRENTS[i];
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
        bg = bg.fill(band, red());
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
    let step = (i % 8) as f32 * STEP;
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
    s.selected.is_some_and(|i| TORRENTS[i].name == t.name)
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
            (v != "–").then(|| {
                let line = if col == "Down" { hex("#4fe3e3") } else { hex("#ff5ec4") };
                col_el(v, color, line)
            })
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
    let bw = w - 48.0;
    let h = 12.0;
    let track = jagged(0.0, 0.0, bw, h, 0.8, 30.0, 3);
    let fw = (bw * t.progress).max(if t.progress > 0.0 { 6.0 } else { 0.0 });
    let mut sh = Shape::new().fill(track, hex("#333333"));
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
    let (pw, ph) = (42.0, 17.0);
    let mut pill = Shape::new();
    if is_selected(s, t) {
        pill = pill.fill(vec![[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], deep());
        pill = pill.fill(vec![[pw - 3.0, 0.0], [pw + 2.0, 0.0], [pw - 3.0, ph], [pw - 8.0, ph]], hex("#4fe3e3"));
        ink = paper();
    } else {
        pill = pill.fill(vec![[5.0, 0.0], [pw, 0.0], [pw - 5.0, ph], [0.0, ph]], paper());
    }
    row()
        .w(w)
        .items_center()
        .gap(6.0)
        .child(el().size(bw, h).child(layer(sh, bw, h)).offset(twitch, 0.0))
        .child(el().size(pw, ph).center().child(layer(pill, pw, ph)).child(text(pct).font(figures(13.5).italic()).color(ink)))
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
