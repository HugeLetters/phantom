//! Direction B, "Accent": restrained. Phantom's calm dark window, square and quiet, with Persona
//! 5 only at the edges: clipped corners, a red slash for the selection, a red offset outline on
//! hover, a faint screentone in one corner, ransom letters only on the error list's heading.
//! Motion is quick and small.

use rune::{Color, El, LineCap, Shape, Spring, col, el, row, text};

use crate::common::*;
use crate::{BAR_H, Parts, ROW_H, button_w, cells, heading_cells};

pub static PARTS: Parts = Parts {
    background,
    brand: None,
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
    caption: Color { r: 0.55, g: 0.55, b: 0.55, a: 1.0 },
};

fn bg() -> Color {
    hex("#111111")
}
fn surface() -> Color {
    hex("#191919")
}
fn line() -> Color {
    hex("#2c2c2c")
}
fn text_c() -> Color {
    hex("#ecebe7")
}
fn muted() -> Color {
    hex("#8d8d8d")
}

fn spring() -> Spring {
    Spring::new(0.14, 0.1)
}

/// A box with its top-left and bottom-right corners clipped by `c`.
fn clipped(w: f32, h: f32, c: f32) -> Vec<[f32; 2]> {
    vec![[c, 0.0], [w, 0.0], [w, h - c], [w - c, h], [0.0, h], [0.0, c]]
}

fn background(_: &S, w: f32, h: f32) -> El {
    let mut s = Shape::new().fill(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]], bg());
    // Screentone fading in from the top-right corner, barely there.
    s = halftone(s, w, h, 7.0, hex("#1f1f1f"), |x, y| {
        let d = ((w - x).powi(2) + y.powi(2)).sqrt();
        (2.6 - d / 120.0).max(0.0)
    });
    // One red cut across the titlebar's corner.
    s = s.fill(vec![[w - 120.0, 0.0], [w - 96.0, 0.0], [w - 150.0, 34.0], [w - 174.0, 34.0]], red());
    layer(s, w, h)
}

fn field(_: &S, w: f32, hint: &str) -> El {
    let h = BAR_H - 6.0;
    let s = Shape::new().fill(inflate(&clipped(w, h, 6.0), 1.0), line()).fill(clipped(w, h, 6.0), surface());
    el().size(w, BAR_H).pt(3.0).child(layer(s, w, h).mt(3.0)).child(
        row()
            .pin(Some(12.0), Some(3.0), Some(12.0), Some(3.0))
            .items_center()
            .child(text(hint.to_owned()).font(sans(12.5, 500.0)).color(hex("#6a6a6a")).truncate()),
    )
}

fn button(_: &S, _key: &str, label: &str, primary: bool, look: Look) -> El {
    let w = button_w(label);
    let h = BAR_H - 6.0;
    let shape = clipped(w, h, 6.0);
    let (fill, ink, edge) = match (look, primary) {
        (Look::Disabled, _) => (hex("#161616"), hex("#4d4d4d"), hex("#232323")),
        (Look::Press, true) => (deep(), text_c(), deep()),
        (Look::Press, false) => (hex("#101010"), text_c(), red()),
        (_, true) => (red(), hex("#fff"), red()),
        (Look::Hover, false) => (hex("#262626"), text_c(), hex("#3a3a3a")),
        (Look::Idle, false) => (hex("#212121"), text_c(), hex("#333")),
    };
    // The red offset outline: hidden behind the button at rest, out on hover, back in on press.
    let off = match look {
        Look::Hover => 3.0,
        _ => 0.0,
    };
    let ring = Shape::new().fill(inflate(&shape, 1.5), if primary { paper() } else { red() }).fill(shape.clone(), bg());
    let face = Shape::new().fill(inflate(&shape, 1.0), edge).fill(shape, fill);
    let label_font = sans(12.5, 600.0);
    el().size(w + 6.0, BAR_H)
        .pt(3.0)
        .child(
            layer(ring, w, h)
                .mt(3.0)
                .offset(off, off)
                .offset_motion(spring())
                .opacity(if off > 0.0 { 1.0 } else { 0.0 })
                .opacity_motion(spring()),
        )
        .child(
            el().size(w, h)
                .center()
                .pin(Some(0.0), Some(3.0), None, None)
                .child(layer(face, w, h))
                .child(text(title(label)).font(label_font).color(ink))
                .offset(if look == Look::Press { 1.0 } else { 0.0 }, if look == Look::Press { 1.0 } else { 0.0 })
                .offset_motion(spring()),
        )
}

/// "RESUME" as "Resume": this direction keeps sentence-case labels.
fn title(label: &str) -> String {
    let lower = label.to_lowercase();
    let mut c = lower.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn panel(_: &S, w: f32, h: f32, content: El) -> El {
    let shape = clipped(w, h, 10.0);
    let s = Shape::new().fill(inflate(&shape, 1.0), line()).fill(shape, surface());
    el().size(w, h).child(layer(s, w, h)).child(content.pin(Some(8.0), Some(4.0), None, None))
}

fn header(_: &S, w: f32) -> El {
    let s = Shape::new().line([0.0, 29.5], [w, 29.5], 1.0, LineCap::Butt, line());
    el().w(w).h(30.0).child(layer(s, w, 30.0)).child(heading_cells(w, muted()).h(28.0))
}

fn torrent_row(s: &S, i: usize, look: Look, selected: bool, w: f32) -> El {
    let t = &TORRENTS[i];
    let mut bg = Shape::new();
    let status_c = if t.kind == Kind::Error { red() } else { muted() };
    if selected {
        bg = bg.fill(vec![[0.0, 1.0], [w, 1.0], [w, ROW_H - 1.0], [0.0, ROW_H - 1.0]], red().alpha(0.16));
        // The slash: a red parallelogram at the row's start, snapping in.
        let f = snap(s.t - s.selected_at, 0.18);
        bg = bg.fill(slant(-2.0, 1.0, 6.0 + 4.0 * f, ROW_H - 2.0, 4.0), red());
    } else if look == Look::Hover {
        bg = bg.fill(vec![[0.0, 1.0], [w, 1.0], [w, ROW_H - 1.0], [0.0, ROW_H - 1.0]], hex("#202020"));
    }
    el().w(w).h(ROW_H).child(layer(bg, w, ROW_H)).child(cells(s, &PARTS, t, w, text_c(), muted(), status_c))
}

fn progress(_: &S, t: &Torrent, w: f32) -> El {
    let bw = w - 42.0;
    let h = 6.0;
    let track = slant(0.0, 0.0, bw, h, 3.0);
    let fw = (bw * t.progress).max(if t.progress > 0.0 { 4.0 } else { 0.0 });
    let fill = slant(0.0, 0.0, fw, h, 3.0);
    let mut s = Shape::new().fill(track, hex("#2b2b2b"));
    s = match t.kind {
        Kind::Downloading => s.fill(fill, text_c()),
        Kind::Seeding => s.fill(fill, red()),
        Kind::Checking => halftone(s.fill(fill, hex("#3a3a3a")), fw, h, 3.0, text_c(), |_, _| 0.9),
        Kind::Error => s.fill(fill, deep()),
        _ => s.fill(fill, hex("#5a5a5a")),
    };
    let pct = format!("{:.0}%", (t.progress * 100.0).floor());
    row()
        .w(w)
        .items_center()
        .gap(6.0)
        .child(el().size(bw, h).child(layer(s, bw, h)))
        .child(row().w(34.0).justify_end().child(text(pct).font(figures(12.0)).color(muted())))
}

fn status(_: &S, w: f32) -> El {
    row()
        .w(w)
        .h(crate::STATUS_H)
        .items_center()
        .gap(8.0)
        .child(ghost(18.0, None))
        .child(text("Connected").font(sans(12.0, 500.0)).color(muted()))
        .child(el().grow(1.0))
        .child(text("6 torrents · ↓ 68 MiB/s · ↑ 1.5 MiB/s").font(figures(12.5)).color(muted()))
}

fn errors(_: &S, w: f32) -> El {
    let h = 190.0;
    let shape = clipped(w, h, 10.0);
    let s = Shape::new().fill(inflate(&shape, 1.0), line()).fill(shape, surface());
    let mut list = col().gap(12.0).padding(16.0).pt(14.0);
    list = list.child(
        row()
            .gap(10.0)
            .items_center()
            .child(ransom("ERRORS", 15.0, 11, &[(paper(), black()), (red(), paper()), (black(), paper())]))
            .child(text("3").font(figures(12.0)).color(muted())),
    );
    for (name, why) in ERRORS {
        list = list.child(
            row().gap(10.0).items_start().child(el().size(4.0, 30.0).fill(red())).child(
                col()
                    .gap(1.0)
                    .w(w - 50.0)
                    .child(text(name.to_string()).font(sans(13.0, 600.0)).color(text_c()).truncate())
                    .child(text(why.to_string()).font(sans(12.0, 500.0)).color(muted()).truncate()),
            ),
        );
    }
    el().size(w, h).child(layer(s, w, h)).child(list.pin(Some(0.0), Some(0.0), None, None))
}
