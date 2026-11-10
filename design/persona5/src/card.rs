//! Direction A, "Calling Card": bold and graphic. Black ground, a red slash through the
//! titlebar, white slanted slabs with thick offset outlines, a red slab for the selection,
//! halftone in the corners. Motion is short and hard: things jump off their shadow on hover and
//! slam into it on press.

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
    indeterminate: false,
    list_font: None,
    status,
    errors,
    caption: Color { r: 0.9, g: 0.9, b: 0.9, a: 1.0 },
};

const SKEW: f32 = 9.0;

fn spring() -> Spring {
    Spring::new(0.18, 0.35)
}

fn background(_: &S, w: f32, h: f32) -> El {
    let mut s = Shape::new().fill(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], black());
    // The slash through the titlebar, and a thin echo under it.
    s = s.fill(vec![[w * 0.38, 0.0], [w, 0.0], [w, 22.0], [w * 0.33, 40.0]], red());
    s = s.fill(vec![[w * 0.31, 44.0], [w, 26.0], [w, 30.0], [w * 0.305, 47.0]], paper());
    // Red halftone swelling into the bottom-right corner.
    s = halftone(s, w, h, 9.0, deep(), |x, y| {
        let d = ((w - x).powi(2) + (h - y).powi(2)).sqrt();
        (4.2 - d / 70.0).max(0.0)
    });
    s = halftone(s, w, h, 9.0, hex("#2a2a2a"), |x, y| {
        let d = (x.powi(2) + (h - y).powi(2)).sqrt();
        (3.2 - d / 90.0).max(0.0)
    });
    layer(s, w, h)
}

fn brand(_: &S) -> El {
    row()
        .gap(6.0)
        .items_center()
        .child(ransom("PHANTOM", 17.0, 3, &[(paper(), black()), (black(), paper()), (paper(), red()), (black(), red())]))
        .child(ghost(34.0, Some(black())))
}

fn field(_: &S, w: f32, hint: &str) -> El {
    let h = BAR_H - 4.0;
    let body = slant(0.0, 0.0, w - 6.0, h, SKEW);
    let s = Shape::new().fill(moved(&body, 5.0, 4.0), red()).fill(inflate(&body, 2.0), black()).fill(body, paper());
    el().size(w, BAR_H).child(layer(s, w, BAR_H)).child(
        row()
            .pin(Some(SKEW + 8.0), Some(0.0), Some(SKEW + 8.0), Some(4.0))
            .items_center()
            .child(text(hint.to_owned()).font(sans(12.5, 500.0)).color(hex("#6d6d6d")).truncate()),
    )
}

fn button(_: &S, _key: &str, label: &str, primary: bool, look: Look) -> El {
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let shape = slant(0.0, 0.0, w, h, SKEW);
    let (body, ink, edge, shadow) = match (look, primary) {
        (Look::Disabled, _) => (hex("#1d1d1d"), hex("#5a5a5a"), hex("#3a3a3a"), None),
        (Look::Press, _) => (paper(), black(), black(), Some(red())),
        (Look::Hover, true) => (red(), paper(), paper(), Some(paper())),
        (Look::Hover, false) => (paper(), black(), black(), Some(red())),
        (Look::Idle, true) => (red(), paper(), black(), Some(paper())),
        (Look::Idle, false) => (black(), paper(), paper(), Some(red())),
    };
    let (dx, dy, sd) = match look {
        Look::Hover => (-2.0, -3.0, 6.0),
        Look::Press => (3.0, 3.0, 3.0),
        _ => (0.0, 0.0, 4.0),
    };
    let mut s = Shape::new();
    if look == Look::Disabled {
        s = s.fill(inflate(&shape, 1.5), edge).fill(shape.clone(), body);
        s = hatch(s, w, h, 5.0, 1.0, hex("#262626"), 0.0);
    } else {
        s = s.fill(inflate(&shape, 2.0), edge).fill(shape.clone(), body);
    }
    let face = el()
        .size(w, h)
        .center()
        .child(layer(s, w, h))
        .child(text(label.to_owned()).font(caps(14.0, 800.0).tracking(1.0)).color(ink))
        .offset(dx, dy)
        .offset_motion(spring());
    let mut b = el().size(w + 6.0, BAR_H).pt(1.0);
    if let Some(c) = shadow {
        let back = Shape::new().fill(shape, c);
        b = b.child(layer(back, w, h).offset(sd - 1.0, sd).offset_motion(spring()).mt(1.0));
    }
    b.child(face.pin(Some(0.0), Some(1.0), None, None))
}

fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    // A black slab cut at the top-right corner, a white rule round it, a red slab behind.
    let cut = vec![[0.0, 0.0], [w - 30.0, 0.0], [w, 18.0], [w, h], [0.0, h]];
    let s = Shape::new().fill(moved(&cut, 7.0, 7.0), red()).fill(inflate(&cut, 3.0), paper()).fill(cut, hex("#101010"));
    el().size(w + 8.0, h + 8.0).child(layer(s, w, h)).child(content.pin(Some(8.0), Some(6.0), None, None))
}

fn header(_: &S, w: f32) -> El {
    let strip = slant(-4.0, 3.0, 230.0, 20.0, 8.0);
    let s = Shape::new().fill(strip, red());
    el().w(w).h(30.0).child(layer(s, w, 30.0)).child(heading_cells(w, paper()).h(26.0).mt(2.0))
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &TORRENTS[i];
    let mut bg = Shape::new();
    let (name, muted, mut status_c) = if selected { (paper(), paper(), paper()) } else { (paper(), hex("#9a9a9a"), hex("#b5b5b5")) };
    if t.kind == Kind::Error && !selected {
        status_c = red();
    }
    if selected {
        // The red slab snaps across from the left when the selection lands.
        let f = snap(s.t - s.selected_at, 0.22);
        let sw = (w + 10.0) * f;
        let slab = slant(-6.0, 2.0, sw, ROW_H - 3.0, 10.0);
        bg = bg.fill(moved(&slab, 4.0, 4.0), black()).fill(inflate(&slab, 1.5), paper()).fill(slab, red());
    } else if look == Look::Hover {
        bg = bg.fill(slant(-4.0, 3.0, w + 4.0, ROW_H - 6.0, 10.0), hex("#232323"));
        bg = bg.fill(slant(-4.0, 3.0, 9.0, ROW_H - 6.0, 6.0), red());
    } else {
        bg = bg.line([0.0, ROW_H - 0.5], [w, ROW_H - 0.5], 1.0, rune::LineCap::Butt, hex("#262626"));
    }
    // On appear, rows cut in from the left one after another.
    let enter = snap(s.t - 0.1 - i as f32 * 0.05, 0.26);
    let mut r = el()
        .w(w)
        .h(ROW_H)
        .child(layer(bg, w, ROW_H))
        .child(cells(s, &PARTS, t, w, name, muted, status_c))
        .offset((enter - 1.0) * 80.0, 0.0)
        .opacity(enter.clamp(0.0, 1.0));
    if selected {
        r = r.rotate(-0.006);
    }
    r
}

fn progress(_: &S, t: &Torrent, w: f32) -> El {
    let bw = w - 42.0;
    let h = 12.0;
    let track = slant(0.0, 0.0, bw, h, 6.0);
    let fw = (bw * t.progress).max(if t.progress > 0.0 { 8.0 } else { 0.0 });
    let fill = slant(0.0, 0.0, fw, h, 6.0);
    let mut s = Shape::new().fill(inflate(&track, 1.5), hex("#6a6a6a")).fill(track.clone(), black());
    match t.kind {
        Kind::Downloading => {
            s = s.fill(fill, paper());
            // A red notch at the leading edge.
            s = s.fill(slant(fw - 9.0, -3.0, 7.0, h + 6.0, 6.0), red());
        }
        Kind::Seeding => {
            s = s.fill(fill, red());
            s = hatch(s, fw, h, 6.0, 1.6, deep(), 0.0);
        }
        Kind::Checking => {
            s = s.fill(fill, paper());
            s = halftone(s, fw, h, 4.0, black(), |x, _| 0.3 + 1.4 * (x / fw));
        }
        Kind::Error => s = s.fill(fill, deep()),
        _ => s = s.fill(fill, hex("#5d5d5d")),
    }
    let pct = format!("{:.0}%", (t.progress * 100.0).floor());
    row()
        .w(w)
        .items_center()
        .gap(6.0)
        .child(el().size(bw, h).child(layer(s, bw, h)))
        .child(row().w(34.0).justify_end().child(text(pct).font(figures(12.0)).color(paper())))
}

fn status(_: &S, w: f32) -> El {
    let tag = slant(0.0, 0.0, 128.0, 22.0, 8.0);
    let s = Shape::new().fill(moved(&tag, 3.0, 3.0), black()).fill(tag, red());
    row()
        .w(w)
        .h(crate::STATUS_H)
        .items_center()
        .child(
            el().size(136.0, 26.0).child(layer(s, 136.0, 26.0)).child(
                row()
                    .pin(Some(14.0), Some(0.0), None, Some(4.0))
                    .gap(6.0)
                    .items_center()
                    .child(el().size(7.0, 7.0).fill(paper()).rotate(0.785))
                    .child(text("CONNECTED").font(caps(13.0, 800.0).tracking(1.4)).color(paper())),
            ),
        )
        .child(el().grow(1.0))
        .child(text("6 torrents · ↓ 68 MiB/s · ↑ 1.5 MiB/s").font(figures(12.5)).color(paper()))
}

fn errors(_: &S, w: f32) -> El {
    let h = 196.0;
    let card = jagged(0.0, 0.0, w, h, 2.4, 14.0, 7);
    let s = Shape::new().fill(moved(&card, 7.0, 7.0), red()).fill(card, paper());
    let mut list = col().gap(10.0).padding(16.0).pt(12.0);
    list = list.child(ransom("ERRORS", 22.0, 11, &[(black(), paper()), (red(), paper()), (paper(), black()), (black(), red())]).mb(4.0));
    for (name, why) in ERRORS {
        let badge = Shape::new().fill(slant(0.0, 0.0, 22.0, 22.0, 5.0), red());
        list = list.child(
            row()
                .gap(10.0)
                .items_start()
                .child(
                    el().size(22.0, 22.0).center().child(layer(badge, 22.0, 22.0)).child(text("!").font(caps(16.0, 900.0)).color(paper())),
                )
                .child(
                    col()
                        .gap(1.0)
                        .w(w - 70.0)
                        .child(text(name.to_string()).font(sans(13.0, 700.0)).color(black()).truncate())
                        .child(text(why.to_string()).font(sans(12.0, 500.0)).color(hex("#3a3a3a")).truncate()),
                ),
        );
    }
    el().size(w + 8.0, h + 8.0).child(layer(s, w, h)).child(list.pin(Some(0.0), Some(0.0), None, None)).rotate(-0.02)
}
