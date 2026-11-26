//! A color setting's swatch, and the popup picker it opens beside it: an Okhsl square (hue across,
//! lightness down) and a bar of saturation beside it, each drawn as an image generated for what it
//! shows and kept until that changes, so a gradient never bands; under them a hex field and a
//! button, then lines of warnings. Dragging in the square or the bar picks as it goes; arrows nudge
//! a focused one; a press outside or Escape closes it.

use std::cell::RefCell;

use rune::{Drag, El, Key, KeyEvent, Phase, Tx, col, el, row, text};
use rune_kit::{Placement, popover};

use super::{Tone, Ui};
use crate::theme::oklab::{Okhsl, encode};

/// How big a swatch is, and its ring's width and its gap off the swatch.
const SWATCH: f32 = 22.0;
const RING: f32 = 1.5;
const RING_GAP: f32 = 2.0;

/// The square's size and the bar's width, and the room around and between them: a popup 360
/// wide.
const SQUARE: (f32, f32) = (304.0, 168.0);
const BAR: f32 = 16.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
pub const POPUP_WIDTH: f32 = SQUARE.0 + 2.0 * GAP + BAR + 2.0 * PAD;

/// How far an arrow nudges: hue and lightness in the square, saturation in the bar.
const NUDGE: f32 = 0.01;

/// What the picker hears: a color picked, as the pointer goes down, moves and comes up (a key's
/// nudge is a press and a release in one), or a drag called off, which puts back what was there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Down(Okhsl),
    Move(Okhsl),
    Up(Okhsl),
    Cancelled,
}

/// The popup's parts that aren't the picker's own: what's under it.
pub struct PopupParts {
    /// The hex field.
    pub field: El,
    /// Follow <Base> or Reset.
    pub button: El,
    /// Lines under them, each in its tone, and its key: the warnings.
    pub lines: Vec<(String, Tone, String)>,
    /// The most lines there could be, each as long as it could be: the room kept under them, so
    /// the popup's size and place stay the same whatever `lines` holds.
    pub room: Vec<String>,
}

impl Ui {
    /// A color setting's swatch, keyed `key`: `color`, pressed to open its picker. One set by hand
    /// carries a ring in the accent color, a small even gap off it. Screen readers hear `label`;
    /// the pointer resting on it shows `help`.
    pub fn color_swatch<S: 'static>(
        &self,
        key: String,
        color: rune::Color,
        hand_set: bool,
        label: &str,
        help: String,
        on_press: impl Fn(&mut S) + 'static,
    ) -> El {
        let k = &self.k;
        let mut swatch = el()
            .key(key)
            .role("button")
            .label(label.to_owned())
            .description(help.clone())
            .help(help)
            .size(SWATCH, SWATCH)
            .radius(k.radius.sm)
            .fill(color)
            .border(1.0, k.c.border_strong)
            .focusable(true)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .on_press(on_press);
        if hand_set {
            swatch = swatch.outline(RING, RING_GAP, k.c.bg_accent);
        }
        // Room for the ring, set or not, so a row doesn't move when one appears.
        row().size(SWATCH + 2.0 * (RING + RING_GAP), SWATCH + 2.0 * (RING + RING_GAP)).center().child(swatch)
    }

    /// The picker of `at`, in a popup beside the element keyed `anchor` (its swatch), keyed
    /// `<key>-popup`; it goes into `layer`, modal. `on_pick` hears what's picked, `on_close` a press
    /// outside, `on_escape` Escape. The square is keyed `<key>-square`, the bar `<key>-bar`.
    pub fn color_popup<S: 'static>(
        &self,
        key: &str,
        anchor: &str,
        at: Okhsl,
        parts: PopupParts,
        on_pick: impl Fn(&mut S, Pick) + Clone + 'static,
        on_close: impl Fn(&mut S) + 'static,
        on_escape: impl Fn(&mut S) + 'static,
        layer: &mut Vec<El>,
    ) {
        let k = &self.k;
        let (w, h) = SQUARE;
        let square_px = image("square", (w as u32, h as u32), (quantized(at.s), 0, 0), |x, y| Okhsl { h: x, s: at.s, l: 1.0 - y });
        let bar_px =
            image("bar", (BAR as u32, h as u32), (quantized(at.h), quantized(at.l), 1), |_, y| Okhsl { h: at.h, s: 1.0 - y, l: at.l });
        let mark = |e: El| {
            e.border(2.0, rune::Color::WHITE)
                .box_shadow(rune::Shadow { spread: 1.0, color: rune::Color::BLACK.alpha(0.45), ..Default::default() })
                .a11y_hidden()
                .inert()
        };
        let pick = on_pick.clone();
        let square = el()
            .key(format!("{key}-square"))
            .size(w, h)
            .radius(k.radius.sm)
            .clip_rounded(k.radius.sm)
            .pixels(&square_px)
            .role("slider")
            .label("Hue and lightness".to_owned())
            .description(format!("Hue {:.0}°, lightness {:.0}%", at.h * 360.0, at.l * 100.0))
            .focusable(true)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .cursor(rune::Cursor::Crosshair)
            .on_drag(move |s: &mut S, d: &mut Drag, _: &mut Tx| {
                let (x, y) = held(d);
                let picked = Okhsl { h: (x / d.w).clamp(0.0, 0.999), s: at.s, l: (1.0 - y / d.h).clamp(0.0, 1.0) };
                pick(s, phase(d, picked));
            })
            .on_key({
                let pick = on_pick.clone();
                move |s: &mut S, ev: &KeyEvent, _: &mut Tx| {
                    let (dh, dl) = match ev.key {
                        Key::Left => (-NUDGE, 0.0),
                        Key::Right => (NUDGE, 0.0),
                        Key::Up => (0.0, NUDGE),
                        Key::Down => (0.0, -NUDGE),
                        _ => return false,
                    };
                    let picked = Okhsl { h: (at.h + dh).rem_euclid(1.0), s: at.s, l: (at.l + dl).clamp(0.0, 1.0) };
                    pick(s, Pick::Down(picked));
                    pick(s, Pick::Up(picked));
                    true
                }
            })
            .child(mark(el().absolute(at.h * w - 6.0, (1.0 - at.l) * h - 6.0).size(12.0, 12.0).radius(6.0)));
        let pick = on_pick.clone();
        let bar = el()
            .key(format!("{key}-bar"))
            .size(BAR, h)
            .radius(k.radius.sm)
            .clip_rounded(k.radius.sm)
            .pixels(&bar_px)
            .role("slider")
            .label("Saturation".to_owned())
            .description(format!("{:.0}%", at.s * 100.0))
            .focusable(true)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .on_drag(move |s: &mut S, d: &mut Drag, _: &mut Tx| {
                let picked = Okhsl { s: (1.0 - held(d).1 / d.h).clamp(0.0, 1.0), ..at };
                pick(s, phase(d, picked));
            })
            .on_key(move |s: &mut S, ev: &KeyEvent, _: &mut Tx| {
                let ds = match ev.key {
                    Key::Up => NUDGE,
                    Key::Down => -NUDGE,
                    _ => return false,
                };
                let picked = Okhsl { s: (at.s + ds).clamp(0.0, 1.0), ..at };
                on_pick(s, Pick::Down(picked));
                on_pick(s, Pick::Up(picked));
                true
            })
            .child(mark(el().absolute(2.0, (1.0 - at.s) * h - 3.0).size(BAR - 4.0, 6.0).radius(3.0)));
        let PopupParts { field, button, lines, room } = parts;
        let mut body = col()
            .key(format!("{key}-content"))
            .w(POPUP_WIDTH)
            .padding(PAD)
            .gap(GAP + 2.0)
            .child(row().gap(GAP + GAP).child(square).child(bar))
            .child(row().gap(GAP).items_stretch().child(field).child(button));
        if !room.is_empty() || !lines.is_empty() {
            // The room the lines could take, unseen; the lines over it, from its top.
            let ghost = |line: String| text(line).font(k.ty.text_small).color(rune::Color::TRANSPARENT).wrap();
            let kept = col().gap(GAP + 2.0).children(room.into_iter().map(ghost)).a11y_hidden().inert();
            let mut shown = col().absolute(0.0, 0.0).w_full().gap(GAP + 2.0);
            for (line, tone, line_key) in lines {
                shown = shown.child(text(line).key(line_key).font(k.ty.text_small).color(self.tone_color(tone)).wrap());
            }
            body = body.child(col().key(format!("{key}-lines")).w_full().child(kept).child(shown));
        }
        let surface = popover::raised(k, body, k.radius.md)
            .border(1.0, self.colors.border_on_page)
            .key(format!("{key}-popup"))
            .role("dialog")
            .label("Color".to_owned());
        // Modal over the sheet, with its own scrim inside it: a press outside closes it as
        // `on_close` says, Escape as `on_escape` does.
        let scope = el()
            .key(format!("{key}-layer"))
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(rune_kit::scrim(format!("{key}-scrim"), on_close))
            .child(popover::anchored(k, surface, anchor, Placement::RightStart, GAP))
            .modal(false, on_escape);
        layer.push(scope);
    }
}

/// Where the pointer is in the element as the press found it: where it went down plus how far
/// it's travelled, so the pick never jumps should the popup move under it.
fn held(d: &Drag) -> (f32, f32) {
    (d.start_x + d.dx, d.start_y + d.dy)
}

/// What a drag's phase picks.
fn phase(d: &Drag, picked: Okhsl) -> Pick {
    match d.phase {
        Phase::Down => Pick::Down(picked),
        Phase::Move => Pick::Move(picked),
        Phase::Up if d.cancelled() => Pick::Cancelled,
        Phase::Up => Pick::Up(picked),
    }
}

/// `x` to a thousandth, as an image's key holds it: finer than a pixel of the square.
fn quantized(x: f32) -> u32 {
    (x.clamp(0.0, 1.0) * 1000.0).round() as u32
}

/// An image of `size` pixels whose pixel at fraction (x, y) from the top left is `at(x, y)`,
/// generated once for `inputs` and kept while they're the same: the last few of each `what`.
fn image(what: &'static str, size: (u32, u32), inputs: (u32, u32, u8), at: impl Fn(f32, f32) -> Okhsl) -> rune::Pixels {
    type Kept = Vec<((&'static str, (u32, u32, u8)), rune::Pixels)>;
    thread_local! {
        static KEPT: RefCell<Kept> = const { RefCell::new(Vec::new()) };
    }
    let id = (what, inputs);
    if let Some(p) = KEPT.with(|k| k.borrow().iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone())) {
        return p;
    }
    let (w, h) = size;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = at((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32).color();
            let byte = |v: f32| (encode(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
            rgba.extend_from_slice(&[byte(c.r), byte(c.g), byte(c.b), 255]);
        }
    }
    let pixels = rune::Pixels::new();
    pixels.set(w, h, rgba);
    KEPT.with(|k| {
        let mut k = k.borrow_mut();
        k.push((id, pixels.clone()));
        let mine = k.iter().filter(|(i, _)| i.0 == what).count();
        if mine > 3 {
            let first = k.iter().position(|(i, _)| i.0 == what).expect("one of its own");
            k.remove(first);
        }
    });
    pixels
}
