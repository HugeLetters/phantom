//! Persona 5 theme sketches (#11): the Phantom Thieves and Jose's shop themes drawn on throwaway
//! components, headless. Not app code: nothing here is the app's `Theme` interface. See
//! README.md for scenes and commands.
//!
//! `themes` fills in [`Parts`]; this file lays the parts out the way Phantom's window is laid out
//! (toolbar, list, status bar), and as a component sheet.

mod common;
mod jose_shop;
mod motion;
mod nebula;
mod noise;
mod paint;
mod phantom_thieves;
mod shapes;
mod themes;

use rune::{AppSpec, Color, Cx, El, Tx, col, el, row, text};

use common::{COLS, Look, S, TORRENTS, Theme, Torrent, caps, figures, interactive};
use motion::ProgressDemo;

pub const W: f32 = 960.0;
pub const H: f32 = 600.0;
/// The component sheet is taller than the window.
pub const SHEET_H: f32 = 700.0;
/// Below the transparent titlebar, as Phantom's page.
pub const TOP: f32 = 40.0;
pub const PAD: f32 = 12.0;
pub const BAR_H: f32 = 34.0;
pub const ROW_H: f32 = 36.0;
pub const STATUS_H: f32 = 26.0;

/// How the themes draw each piece. Sizes are in points; the layout hands each piece its box.
pub struct Parts {
    /// Under everything, the whole window.
    pub background: fn(&S, f32, f32) -> El,
    /// Drawn in the titlebar strip, right-aligned.
    pub brand: fn(&S) -> El,
    pub field: fn(&S, f32, &str) -> El,
    /// key, label, primary, look.
    pub button: fn(&S, &str, &str, bool, Look) -> El,
    /// The list's frame, around `content`.
    pub panel: fn(&S, f32, f32, El) -> El,
    pub header: fn(&S, f32) -> El,
    /// The torrent at `i`, in a box `w` wide; `look` is its hover, `selected` the selection.
    pub row: fn(&S, usize, Look, bool, f32) -> El,
    /// Also draws torrents still fetching their info.
    pub progress: fn(&S, &Torrent, f32) -> El,
    /// The face of torrent names and statuses (size, weight).
    pub list_font: fn(f32, f32) -> rune::TextStyle,
    /// Draws a cell its own way: given the column ("Name", "Down", …) and its width, `Some`
    /// replaces the default cell.
    pub cell: fn(&S, &Torrent, &str, f32) -> Option<El>,
    pub status: fn(&S, f32) -> El,
    pub errors: fn(&S, f32) -> El,
}

/// How wide a toolbar button is for `label`.
pub fn button_w(label: &str) -> f32 {
    label.chars().count() as f32 * 9.2 + 30.0
}

/// The cells of a torrent row, in `w`: the name takes what the fixed columns leave.
pub fn cells(s: &S, p: &Parts, t: &Torrent, w: f32, name: Color, muted: Color, status: Color) -> El {
    let fixed: f32 = COLS.iter().map(|c| c.1).sum();
    let name_w = w - fixed - 16.0;
    let mut r = row().w(w).h(ROW_H).items_center().px(8.0);
    let name_font = (p.list_font)(14.0, 800.0);
    let status_font = (p.list_font)(12.5, 800.0);
    let own = |col: &str, cw: f32| (p.cell)(s, t, col, cw);
    r = r.child(own("Name", name_w).unwrap_or_else(|| text(t.name).font(name_font).color(name).truncate().w(name_w)));
    for (col, cw) in COLS {
        let cell = match col {
            _ if let Some(e) = own(col, cw - 6.0) => e,
            "Progress" => (p.progress)(s, t, cw - 12.0),
            "Status" => text(t.status.to_uppercase()).font(status_font).color(status),
            "Size" => text(t.size).font(figures(12.5)).color(muted),
            "Down" => text(t.down).font(figures(12.5)).color(muted),
            "Up" => text(t.up).font(figures(12.5)).color(muted),
            "Peers" => text(t.peers).font(figures(12.5)).color(muted),
            _ => text(t.eta).font(figures(12.5)).color(muted),
        };
        let right = !matches!(col, "Progress" | "Status");
        let mut c = row().w(cw).items_center().pl(if col == "Progress" { 4.0 } else { 6.0 });
        if right {
            c = c.justify_end();
        }
        r = r.child(c.child(cell));
    }
    r
}

/// The column headings, matching [`cells`].
pub fn heading_cells(w: f32, color: Color) -> El {
    let fixed: f32 = COLS.iter().map(|c| c.1).sum();
    let mut r = row().w(w).items_center().px(8.0);
    r = r.child(text("NAME").font(caps(11.5, 800.0).tracking(1.2)).color(color).w(w - fixed - 16.0));
    for (col, cw) in COLS {
        let right = !matches!(col, "Progress" | "Status");
        let mut c = row().w(cw).pl(if col == "Progress" { 4.0 } else { 6.0 });
        if right {
            c = c.justify_end();
        }
        r = r.child(c.child(text(col.to_uppercase()).font(caps(11.5, 800.0).tracking(1.2)).color(color)));
    }
    r
}

/// A toolbar or sheet button, hoverable and pressable.
fn button(s: &S, p: &Parts, key: &str, label: &str, primary: bool, disabled: bool) -> El {
    let look = s.look(key, disabled);
    let b = (p.button)(s, key, label, primary, look);
    if disabled { b.key(key.to_owned()) } else { interactive(b, key.to_owned(), |_| {}) }
}

fn window(s: &S, p: &Parts) -> El {
    let inner = W - PAD * 2.0;
    // Resume and Remove act on the selection; Pause only while it's running.
    let selected = s.selected.map(|i| common::torrent(s, i));
    let running = selected.is_some_and(|t| matches!(t.kind, common::Kind::Downloading | common::Kind::Seeding | common::Kind::Checking));
    let buttons = [
        ("add", "ADD", true, false),
        ("open", "OPEN…", false, false),
        ("resume", "RESUME", false, selected.is_none() || running),
        ("pause", "PAUSE", false, !running),
        ("remove", "REMOVE…", false, selected.is_none()),
    ];
    // Each button's box is 6 points wider than its face, for the shadow; 8 between, 6 more
    // before Resume.
    let used: f32 = buttons.iter().map(|b| button_w(b.1) + 6.0 + 8.0).sum::<f32>() + 6.0 + 8.0;
    let mut bar = row().key("toolbar").w(inner).h(BAR_H).items_center().gap(8.0);
    bar = bar.child((p.field)(s, inner - used, "Paste a magnet link, .torrent URL or info hash"));
    for (i, (key, label, primary, disabled)) in buttons.into_iter().enumerate() {
        if i == 2 {
            bar = bar.child(el().w(6.0));
        }
        bar = bar.child(button(s, p, key, label, primary, disabled));
    }

    let list_h = H - TOP - BAR_H - STATUS_H - PAD - 2.0 * 12.0;
    // Where the rows have scrolled to, and the window they show through (see below).
    let scroll_y = ((s.t - 2.0).max(0.0) * 40.0).min(common::count(s) as f32 * ROW_H - (list_h - 38.0));
    let view_h = list_h - 6.0 - 32.0;
    let mut list = col().w(inner - 16.0).child((p.header)(s, inner - 16.0));
    // The scrolling list puts its rows in a viewport, padded 30 points each side so the
    // selected row's flag and wedge, which reach past the list's edge, aren't clipped.
    let mut rows = col().w(inner - 16.0).px(30.0);
    for i in 0..common::count(s) {
        let key = format!("row-{i}");
        let look = s.look(&key, false);
        let r = (p.row)(s, i, look, common::row_selected(s, i), inner - 16.0);
        // Scrolling rows fade out over their last 30 points at the window's top and bottom.
        let r = if s.scrolling {
            let y = i as f32 * ROW_H - scroll_y;
            let f = ((y + ROW_H) / 30.0).clamp(0.0, 1.0) * ((view_h - y) / 30.0).clamp(0.0, 1.0);
            r.opacity(f)
        } else {
            r
        };
        let r = interactive(r, key, move |s: &mut S| {
            s.selected = Some(i);
            s.selected_at = s.t;
            s.activations += 1;
        });
        // A row being added or removed takes only part of its height: the others make room.
        let (_, room) = s.row_life(i);
        let r = if room <= 0.0 {
            continue;
        } else if room < 1.0 {
            el().w(inner - 16.0).h(ROW_H * room).clip_rounded(0.0).child(r)
        } else {
            r
        };
        if s.scrolling {
            rows = rows.child(r);
        } else {
            list = list.child(r);
        }
    }
    if s.scrolling {
        // Scrolled by moving the rows up inside a clipped window, 40 points a second from 2 s,
        // as someone reading down a long list would: the same picture as a scroll viewport,
        // which the headless renderer didn't move. Instant, so the offset doesn't spring.
        let view = el().w(inner - 16.0 + 60.0).h(view_h).ml(-30.0).clip_rounded(0.0).child(rows.offset(0.0, -scroll_y).instant_changes());
        list = list.child(view);
    }
    let page = col()
        .key("app")
        .w(W)
        .h(H)
        .padding(PAD)
        .pt(TOP)
        .gap(12.0)
        .child(bar)
        .child((p.panel)(s, inner, list_h, list))
        .child((p.status)(s, inner));
    let mut root = el().w(W).h(H).child((p.background)(s, W, H));
    // The logo sits under the page, so a hovered button's sheet passes over it.
    root = root.child(row().pin(None, Some(2.0), Some(PAD + 6.0), None).child((p.brand)(s)));
    root.child(page.pin(Some(0.0), Some(0.0), None, None))
}

/// Every component in each state it has, at a size to look at closely.
fn sheet(s: &S, p: &Parts) -> El {
    // Black captions, or pale lemon on Jose's navy, where black would vanish.
    let cap_c = if common::jose_shop() { common::jose_shop_pale() } else { Color::BLACK };
    let cap = |t: &str| text(t.to_owned()).font(caps(12.0, 800.0).tracking(2.0)).color(cap_c);
    let looks = [(Look::Idle, "IDLE"), (Look::Hover, "HOVER"), (Look::Press, "PRESSED"), (Look::Disabled, "DISABLED")];
    let mut buttons = col().gap(14.0).child(cap("BUTTON"));
    for (primary, label) in [(true, "ADD"), (false, "RESUME")] {
        let mut r = row().gap(26.0).items_center();
        for (look, name) in looks {
            let key = format!("sheet-{label}-{name}");
            r = r.child(col().gap(8.0).child((p.button)(s, &key, label, primary, look)).child(cap(name).opacity(0.7)));
        }
        buttons = buttons.child(r);
    }
    // The same button hovered four times: each activation lands with its own tilt and shape.
    let mut again = row().gap(26.0).items_center();
    for k in 0..4 {
        again = again.child((p.button)(s, &format!("sheet-again-{k}"), "OPEN…", false, Look::Hover));
    }
    buttons = buttons.child(col().gap(8.0).child(again).child(cap("HOVER, FOUR TIMES").opacity(0.7)));
    // The bars as unselected rows draw them: the selected row's colours are for its lemon flag
    // and read dark on the sheet's ground.
    let unsel = {
        let mut u = s.clone();
        u.selected = None;
        u
    };
    let mut bars = col().gap(12.0).child(cap("PROGRESS"));
    for (i, name) in [(0, "DOWNLOADING"), (1, "SEEDING"), (2, "CHECKING"), (3, "PAUSED"), (4, "FETCHING"), (5, "ERROR")] {
        bars = bars.child(row().gap(12.0).items_center().child(cap(name).w(110.0)).child((p.progress)(&unsel, &TORRENTS[i], 260.0)));
    }
    let rows_w = W - 2.0 * 28.0 - 16.0;
    let mut rows = col().gap(0.0).child(cap("ROWS · IDLE · SELECTED · HOVER").mb(8.0));
    let mut list = col().w(rows_w).child((p.header)(s, rows_w));
    for (i, look, sel) in [(3, Look::Idle, false), (0, Look::Idle, true), (1, Look::Hover, false)] {
        list = list.child((p.row)(s, i, look, sel, rows_w));
    }
    rows = rows.child((p.panel)(s, rows_w + 16.0, ROW_H * 3.0 + 30.0 + 16.0, list));
    let errors = col().gap(12.0).child(cap("ERROR LIST")).child((p.errors)(s, 340.0));
    let page = col()
        .w(W)
        .h(SHEET_H)
        .padding(28.0)
        .pt(TOP)
        .gap(26.0)
        .child(row().gap(40.0).items_start().child(col().gap(30.0).child(buttons).child(bars)).child(el().grow(1.0)).child(errors))
        .child(rows);
    el().w(W).h(SHEET_H).child((p.background)(s, W, SHEET_H)).child(page.pin(Some(0.0), Some(0.0), None, None))
}

fn view(s: &S, _: &mut Cx<S>) -> El {
    let p = &themes::PARTS;
    if s.sheet { sheet(s, p) } else { window(s, p) }
}

fn poll(s: &mut S, t: f64, _: &mut Tx) -> bool {
    s.t = t as f32;
    // A sketch: the motion is driven by time, so draw every frame.
    true
}

/// The macOS fonts the sketches name. Rune doesn't look system fonts up by name (only its own,
/// the CJK fallbacks and symbols), so a named face that isn't loaded falls back to Inter: each
/// file is read at start and handed to Rune, not bundled.
const SYSTEM_FONTS: &[&str] = &[
    "/System/Library/Fonts/Avenir Next.ttc",
    "/System/Library/Fonts/Avenir Next Condensed.ttc",
    "/System/Library/Fonts/Optima.ttc",
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf",
    "/System/Library/Fonts/Supplemental/SuperClarendon.ttc",
    "/System/Library/Fonts/Supplemental/Futura.ttc",
    "/System/Library/Fonts/Supplemental/Didot.ttc",
    "/System/Library/Fonts/Supplemental/AmericanTypewriter.ttc",
    "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
    "/System/Library/Fonts/Supplemental/Rockwell.ttc",
    "/System/Library/Fonts/Supplemental/Impact.ttf",
    "/System/Library/Fonts/Supplemental/Bodoni 72.ttc",
    "/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Black.ttf",
    "/System/Library/Fonts/Supplemental/Arial Rounded Bold.ttf",
    // Jose's rounded faces.
    "/System/Library/Fonts/ヒラギノ丸ゴ ProN W4.ttc",
    "/System/Library/Fonts/SFNSRounded.ttf",
    "/System/Library/Fonts/SFCompactRounded.ttf",
];

fn system_fonts() -> &'static [&'static [u8]] {
    let files: Vec<&'static [u8]> = SYSTEM_FONTS
        .iter()
        .map(|path| &*Box::leak(std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}")).into_boxed_slice()))
        .collect();
    Box::leak(files.into_boxed_slice())
}

fn main() {
    let spec = AppSpec {
        poll: Some(poll),
        fonts: system_fonts(),
        families: ("Avenir Next", "Avenir Next"),
        scenes: &[
            ("phantom-thieves", |_| common::set_theme(Theme::PhantomThieves)),
            ("phantom-thieves-parts", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.sheet = true;
            }),
            ("phantom-thieves-scroll", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.scrolling = true;
                s.selected = Some(4);
            }),
            ("phantom-thieves-rows-add", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.list_event = 1;
            }),
            ("phantom-thieves-rows-remove", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.list_event = 2;
            }),
            ("phantom-thieves-progress", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.progress_demo = Some(ProgressDemo::Reports);
            }),
            ("phantom-thieves-progress-sweep", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.progress_demo = Some(ProgressDemo::Sweep);
            }),
            ("phantom-thieves-status-flip", |s| {
                common::set_theme(Theme::PhantomThieves);
                s.status_flip = true;
            }),
            ("jose-shop", |_| common::set_theme(Theme::JoseShop)),
            ("jose-shop-parts", |s| {
                common::set_theme(Theme::JoseShop);
                s.sheet = true;
            }),
            ("jose-shop-scroll", |s| {
                common::set_theme(Theme::JoseShop);
                s.scrolling = true;
                s.selected = Some(4);
            }),
            ("jose-shop-rows-add", |s| {
                common::set_theme(Theme::JoseShop);
                s.list_event = 1;
            }),
            ("jose-shop-rows-remove", |s| {
                common::set_theme(Theme::JoseShop);
                s.list_event = 2;
            }),
            ("jose-shop-progress", |s| {
                common::set_theme(Theme::JoseShop);
                s.progress_demo = Some(ProgressDemo::Reports);
            }),
            ("jose-shop-progress-sweep", |s| {
                common::set_theme(Theme::JoseShop);
                s.progress_demo = Some(ProgressDemo::Sweep);
            }),
            ("jose-shop-status-flip", |s| {
                common::set_theme(Theme::JoseShop);
                s.status_flip = true;
            }),
        ],
        ..AppSpec::new("Phantom · Persona 5 sketches", (W, H), S::new, view)
    };
    rune::App::new(spec).run();
}
