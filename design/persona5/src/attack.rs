//! Direction C, "All-Out": maximal and animated. A red ground with a slowly turning starburst
//! and black halftone, the list on a tilted sheet of white paper with a fat black outline, black
//! cut-out slabs for buttons and the selection. Everything moves: rows slam in on appear,
//! buttons kick up and turn on hover and crunch on press, progress stripes march.

use rune::{Color, El, Shape, Spring, col, el, row, text};

use crate::common::*;
use crate::{BAR_H, Parts, ROW_H, button_w, cells, heading_cells};

pub static PARTS: Parts = Parts {
    background,
    brand: Some(brand),
    field,
    button,
    panel,
    header,
    row: torrent_row,
    progress,
    status,
    errors,
    caption: Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
};

fn kick() -> Spring {
    Spring::new(0.28, 0.45)
}

fn background(s: &S, w: f32, h: f32) -> El {
    let mut sh = Shape::new().fill(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], red());
    // Rays from behind the ghost, turning slowly.
    let (cx, cy) = (w - 70.0, 20.0);
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

fn brand(s: &S) -> El {
    let bob = (s.t * 2.2).sin() * 2.0;
    row()
        .gap(4.0)
        .items_center()
        .child(ransom("PHANTOM", 15.0, 5, &[(paper(), black()), (black(), paper()), (paper(), red()), (black(), red())]))
        .child(
            el().size(36.0, 36.0)
                .child(canvas_burst(36.0, s.t))
                .child(ghost(32.0, Some(black())).pin(Some(3.0), Some(1.0), None, None).offset(0.0, bob)),
        )
}

fn canvas_burst(size: f32, t: f32) -> El {
    let c = size / 2.0;
    let star = burst(c, c, c * 0.62, c, 11, t * 0.6, 3);
    layer(Shape::new().fill(inflate(&star, 2.0), black()).fill(star, paper()), size, size)
}

fn field(_: &S, w: f32, hint: &str) -> El {
    let h = BAR_H - 6.0;
    let slab = jagged(0.0, 0.0, w - 8.0, h, 1.4, 22.0, 41);
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
    let seed = key.bytes().map(u32::from).sum::<u32>();
    let slab = jagged(0.0, 0.0, w, h, 1.6, 12.0, seed);
    let (fill, ink, edge) = match (look, primary) {
        (Look::Disabled, _) => (deep(), hex("#d2505a"), deep()),
        (Look::Press, _) => (red(), paper(), black()),
        (Look::Hover, true) => (black(), paper(), paper()),
        (Look::Hover, false) => (paper(), black(), black()),
        (Look::Idle, true) => (paper(), red(), black()),
        (Look::Idle, false) => (black(), paper(), paper()),
    };
    let mut sh = Shape::new();
    if look == Look::Hover {
        // A burst flashes out behind the button.
        // Squashed to the button's shape, so it flares past the edges without covering much.
        let star: Vec<[f32; 2]> = burst(w / 2.0, h / 2.0, w * 0.5, w * 0.66, 12, s.t * 1.5, seed)
            .into_iter()
            .map(|p| [p[0], h / 2.0 + (p[1] - h / 2.0) * (h / w) * 1.5])
            .collect();
        sh = sh.fill(star, if primary { paper() } else { black() });
    }
    if look != Look::Disabled {
        sh = sh.fill(moved(&slab, 4.0, 4.0), black());
    }
    sh = sh.fill(inflate(&slab, 2.0), edge).fill(slab.clone(), fill);
    if look == Look::Disabled {
        sh = hatch(sh, w, h, 5.0, 1.2, hex("#8a0a12"), 0.0);
    }
    let (scale, turn) = match look {
        Look::Hover => (1.12, -0.07),
        Look::Press => (0.9, 0.03),
        _ => (1.0, 0.0),
    };
    el().size(w + 4.0, BAR_H)
        .center()
        .child(layer(sh, w, h).pin(Some(0.0), Some(3.0), None, None))
        .child(text(label.to_owned()).font(caps(15.0, 900.0).tracking(1.0)).color(ink).mr(4.0))
        .scale(scale)
        .scale_motion(kick())
        .rotate(turn)
        .rotate_motion(kick())
}

fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    let sheet = jagged(0.0, 0.0, w - 10.0, h - 6.0, 1.2, 40.0, 77);
    let mut s = Shape::new().fill(moved(&sheet, 10.0, 9.0), black()).fill(inflate(&sheet, 3.5), black()).fill(sheet, paper());
    // A grey screentone in the sheet's bottom corner, as printed paper.
    s = halftone(s, w - 14.0, h - 10.0, 6.0, hex("#d8d4cb"), |x, y| ((x / (w - 14.0)) * (y / (h - 10.0)) * 3.4 - 0.6).max(0.0));
    // The paper clips the rows sliding in, but leaves room for the selection's overhang.
    let clip =
        el().pin(Some(-6.0), Some(0.0), Some(-6.0), Some(4.0)).clip_rounded(0.0).child(content.pin(Some(10.0), Some(6.0), None, None));
    el().size(w, h).child(layer(s, w, h)).child(clip).rotate(-0.008)
}

fn header(_: &S, w: f32) -> El {
    let strip = jagged(-2.0, 2.0, w - 10.0, 24.0, 1.2, 30.0, 5);
    let s = Shape::new().fill(strip, black());
    el().w(w).h(30.0).child(layer(s, w, 30.0)).child(heading_cells(w - 10.0, paper()).h(26.0).mt(1.0))
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &TORRENTS[i];
    let w = w - 10.0;
    let mut bg = Shape::new();
    let (name, muted, mut status_c) = if selected { (paper(), paper(), red()) } else { (black(), hex("#4a4a4a"), black()) };
    if t.kind == Kind::Error && !selected {
        status_c = red();
    }
    if selected {
        let f = snap(s.t - s.selected_at, 0.26);
        let slab = jagged(-8.0, 2.0, (w + 16.0) * f, ROW_H - 4.0, 1.6, 30.0, 9 + i as u32);
        bg = bg.fill(moved(&slab, 5.0, 4.0), red()).fill(slab, black());
    } else if look == Look::Hover {
        // A red zigzag under the row.
        let zig: Vec<[f32; 2]> =
            (0..=(w / 8.0) as usize).map(|k| [k as f32 * 8.0, ROW_H - 4.0 + if k % 2 == 0 { 0.0 } else { 3.0 }]).collect();
        bg = bg.stroke(zig, 2.2, red());
    }
    let mut cells_el = cells(s, &PARTS, t, w, name, muted, status_c);
    if look == Look::Hover && !selected {
        cells_el = cells_el.offset(6.0, 0.0).offset_motion(kick());
    } else {
        cells_el = cells_el.offset(0.0, 0.0).offset_motion(kick());
    }
    // Rows slam in from the right, one after another, when the window appears.
    let enter = snap(s.t - 0.15 - i as f32 * 0.07, 0.38);
    let r = el().w(w).h(ROW_H).child(layer(bg, w, ROW_H)).child(cells_el).offset((1.0 - enter) * 900.0, 0.0);
    if selected { r.rotate(-0.012) } else { r }
}

fn progress(s: &S, t: &Torrent, w: f32) -> El {
    let bw = w - 42.0;
    let h = 12.0;
    let track = jagged(0.0, 0.0, bw, h, 0.8, 30.0, 3);
    let fw = (bw * t.progress).max(if t.progress > 0.0 { 6.0 } else { 0.0 });
    let fill = vec![[0.0, 0.0], [fw, 0.0], [(fw - 5.0).max(0.0), h], [0.0, h]];
    let mut sh = Shape::new().fill(track, black());
    match t.kind {
        Kind::Downloading => {
            sh = sh.fill(fill, red());
            sh = hatch(sh, (fw - 5.0).max(0.0), h, 9.0, 3.0, paper().alpha(0.85), s.t * 26.0);
        }
        Kind::Seeding => {
            sh = sh.fill(fill, paper());
            sh = hatch(sh, fw - 5.0, h, 9.0, 3.0, red(), s.t * 8.0);
        }
        Kind::Checking => {
            sh = sh.fill(fill, paper());
            sh = hatch(sh, (fw - 5.0).max(0.0), h, 6.0, 3.0, black(), -s.t * 40.0);
        }
        Kind::Error => sh = sh.fill(fill, red()),
        _ => sh = sh.fill(fill, hex("#8a8a8a")),
    }
    let pct = format!("{:.0}%", (t.progress * 100.0).floor());
    let ink = if t.kind == Kind::Downloading { red() } else { black() };
    row()
        .w(w)
        .items_center()
        .gap(6.0)
        .child(el().size(bw, h).child(layer(sh, bw, h)))
        .child(row().w(34.0).justify_end().child(text(pct).font(caps(14.0, 900.0)).color(ink)))
}

fn status(_: &S, w: f32) -> El {
    let ribbon = jagged(0.0, 0.0, w, 24.0, 1.4, 40.0, 19);
    let s = Shape::new().fill(moved(&ribbon, 0.0, 4.0), paper()).fill(ribbon, black());
    el().w(w).h(crate::STATUS_H).child(layer(s, w, 26.0)).rotate(-0.004).child(
        row()
            .pin(Some(12.0), Some(0.0), Some(12.0), Some(2.0))
            .items_center()
            .gap(8.0)
            .child(el().size(8.0, 8.0).fill(red()).rotate(0.785))
            .child(text("CONNECTED").font(caps(14.0, 900.0).tracking(1.6)).color(paper()))
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
