//! Persona 5 theme sketches (#11): three directions drawn on throwaway components, headless.
//! Not app code: nothing here is the `Theme` interface. See README.md for scenes and commands.
//!
//! Each direction (`card`, `accent`, `attack`) fills in [`Parts`]; this file lays the parts out
//! the way Phantom's window is laid out (toolbar, list, status bar), and as a component sheet.

mod accent;
mod attack;
mod card;
mod common;

use rune::{AppSpec, Color, Cx, El, Tx, col, el, row, text};

use common::{COLS, Dir, Look, S, TORRENTS, Torrent, caps, figures, interactive, sans};

pub const W: f32 = 960.0;
pub const H: f32 = 600.0;
/// The angled spike's window: the usual one, plus its slant on both sides and a margin.
pub const ANGLED_W: f32 = W + 2.0 * attack::SLANT + 16.0;
/// The component sheet is taller than the window.
pub const SHEET_H: f32 = 700.0;
/// The lettering comparison sheet.
pub const LETTERS_H: f32 = 760.0;
/// The hover comparison sheet.
pub const HOVERS_H: f32 = 240.0;
/// Below the transparent titlebar, as Phantom's page.
pub const TOP: f32 = 40.0;
pub const PAD: f32 = 12.0;
pub const BAR_H: f32 = 34.0;
pub const ROW_H: f32 = 36.0;
pub const STATUS_H: f32 = 26.0;

/// How one direction draws each piece. Sizes are in points; the layout hands each piece its box.
pub struct Parts {
    /// Under everything, the whole window.
    pub background: fn(&S, f32, f32) -> El,
    /// Drawn in the titlebar strip, right-aligned, if the direction brands the window.
    pub brand: Option<fn(&S) -> El>,
    pub field: fn(&S, f32, &str) -> El,
    /// key, label, primary, look.
    pub button: fn(&S, &str, &str, bool, Look) -> El,
    /// The list's frame, around `content`.
    pub panel: fn(&S, f32, f32, El) -> El,
    pub header: fn(&S, f32) -> El,
    /// The torrent at `i`, in a box `w` wide; `look` is its hover, `selected` the selection.
    pub row: fn(&S, usize, Look, bool, f32) -> El,
    pub progress: fn(&S, &Torrent, f32) -> El,
    /// `progress` also draws torrents still fetching their info; otherwise they show a dash.
    pub indeterminate: bool,
    /// The face of torrent names and statuses (size, weight), if not the default.
    pub list_font: Option<fn(f32, f32) -> rune::TextStyle>,
    /// Draws a cell its own way: given the column ("Name", "Down", …), its width and the text
    /// colour, `Some` replaces the default cell.
    pub cell: Option<fn(&S, &Torrent, &str, f32, Color) -> Option<El>>,
    pub status: fn(&S, f32) -> El,
    pub errors: fn(&S, f32) -> El,
    /// Captions on the component sheet.
    pub caption: Color,
}

fn parts(dir: Dir) -> &'static Parts {
    match dir {
        Dir::Card => &card::PARTS,
        Dir::Accent => &accent::PARTS,
        Dir::Attack => &attack::PARTS,
    }
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
    let name_font = p.list_font.map_or(sans(13.0, 500.0), |f| f(14.0, 800.0));
    let status_font = p.list_font.map_or(caps(12.0, 700.0), |f| f(12.5, 800.0));
    let own = |col: &str, cw: f32, color: Color| p.cell.and_then(|f| f(s, t, col, cw, color));
    r = r.child(own("Name", name_w, name).unwrap_or_else(|| text(t.name).font(name_font).color(name).truncate().w(name_w)));
    for (col, cw) in COLS {
        let cell = match col {
            _ if let Some(e) = own(col, cw - 6.0, muted) => e,
            "Progress" if t.kind == common::Kind::Fetching && !p.indeterminate => text("–").font(figures(12.0)).color(muted),
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
    let mut list = col().w(inner - 16.0).child((p.header)(s, inner - 16.0));
    for i in 0..common::count(s) {
        let key = format!("row-{i}");
        let look = s.look(&key, false);
        let r = (p.row)(s, i, look, s.selected == Some(i), inner - 16.0);
        list = list.child(interactive(r, key, move |s: &mut S| {
            s.selected = Some(i);
            s.selected_at = s.t;
            s.activations += 1;
        }));
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
    // The angled spike draws its own background, cut to the window's shape.
    let mut root = el().w(W).h(H);
    if !s.angled {
        root = root.child((p.background)(s, W, H));
    }
    if let Some(brand) = p.brand {
        root = root.child(row().pin(None, Some(2.0), Some(PAD + 6.0), None).child(brand(s)));
    }
    root.child(page.pin(Some(0.0), Some(0.0), None, None))
}

/// Every component in each state it has, at a size to look at closely.
fn sheet(s: &S, p: &Parts) -> El {
    let cap = |t: &str| text(t.to_owned()).font(caps(12.0, 800.0).tracking(2.0)).color(p.caption);
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
    let mut bars = col().gap(12.0).child(cap("PROGRESS"));
    for (i, name) in [(0, "DOWNLOADING"), (1, "SEEDING"), (2, "CHECKING"), (3, "PAUSED"), (4, "FETCHING"), (5, "ERROR")] {
        bars = bars.child(row().gap(12.0).items_center().child(cap(name).w(110.0)).child((p.progress)(s, &TORRENTS[i], 260.0)));
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
    let p = parts(s.dir);
    if s.angled {
        let full = if s.edge_style > 0 { [W + 2.0 * attack::EDGE, H + 2.0 * attack::EDGE] } else { [ANGLED_W, H] };
        let [w, h] = if s.win[0] > 0.0 { s.win } else { full };
        return el()
            .w_full()
            .h_full()
            .on_layout(|s: &mut S, r| s.win = [r[2], r[3]])
            .on_drag(|s: &mut S, d, _| {
                if d.phase == rune::Phase::Down {
                    s.presses += 1;
                    s.last_press = [d.start_x, d.start_y];
                }
            })
            .child(attack::angled_frame(s, w, h, window(s, p), W));
    }
    if s.signs_sheet {
        return attack::signs_sheet(s, W, 300.0);
    }
    if s.tracks_sheet {
        return attack::tracks_sheet(s, W, 330.0);
    }
    if s.steps_sheet {
        return attack::steps_sheet(s, W, 640.0);
    }
    if s.speeds_sheet {
        return attack::speeds_sheet(s, W, 520.0);
    }
    if s.hovers_sheet {
        return attack::hovers_sheet(s, W, HOVERS_H);
    }
    if s.letters_sheet {
        return attack::letters_sheet(s, W, LETTERS_H);
    }
    if s.sheet { sheet(s, p) } else { window(s, p) }
}

fn poll(s: &mut S, t: f64, tx: &mut Tx) -> bool {
    s.t = t as f32;
    // The hover sheet's loop shows the drawn entrance, not Rune's colour and mount fades.
    if s.hovers_sheet {
        tx.instant();
    }
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
    // The angled spike needs a clear window, so it's chosen before the window opens.
    let edges = std::env::args().any(|a| a.starts_with("edges-"));
    let angled = edges || std::env::args().any(|a| a == "angled");
    let spec = AppSpec {
        poll: Some(poll),
        fonts: system_fonts(),
        families: ("Avenir Next", "Avenir Next"),
        scenes: &[
            ("card", |s| s.dir = Dir::Card),
            ("card-parts", |s| {
                s.dir = Dir::Card;
                s.sheet = true;
            }),
            ("accent", |s| s.dir = Dir::Accent),
            ("accent-parts", |s| {
                s.dir = Dir::Accent;
                s.sheet = true;
            }),
            ("attack", |s| s.dir = Dir::Attack),
            ("attack-parts", |s| {
                s.dir = Dir::Attack;
                s.sheet = true;
            }),
            ("attack-tracks", |s| {
                s.dir = Dir::Attack;
                s.tracks_sheet = true;
            }),
            ("attack-many", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
            }),
            ("attack-many-b", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 2;
            }),
            ("attack-many-b1", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 6;
            }),
            ("attack-many-b2", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 7;
            }),
            ("attack-many-b2-10", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 9;
            }),
            ("attack-many-b2-7", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 10;
            }),
            ("attack-many-b2-4", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 11;
            }),
            ("attack-many-b3", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 8;
            }),
            ("attack-many-e", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                s.step_style = 5;
            }),
            ("attack-steps", |s| {
                s.dir = Dir::Attack;
                s.steps_sheet = true;
            }),
            ("attack-speeds", |s| {
                s.dir = Dir::Attack;
                s.speeds_sheet = true;
            }),
            ("jose-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(1);
            }),
            ("jose-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(2);
            }),
            ("jose-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(3);
            }),
            ("jose-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(4);
            }),
            ("edges-1", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
                s.edge_style = 1;
            }),
            ("edges-2", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
                s.edge_style = 2;
            }),
            ("edges-3", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
                s.edge_style = 3;
            }),
            ("edges-4", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
                s.edge_style = 4;
            }),
            ("edges-5", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
                s.edge_style = 5;
            }),
            ("jose-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(5);
            }),
            ("jose-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(6);
            }),
            ("jose-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(7);
            }),
            ("jose-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(8);
            }),
            ("jose-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(9);
            }),
            ("jose-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(10);
            }),
            ("jose-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(11);
            }),
            ("jose-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(12);
            }),
            ("jose-13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
            }),
            ("jose-14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(14);
            }),
            ("jose-font-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
                common::set_jose_font(0);
            }),
            ("jose-font-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
                common::set_jose_font(1);
            }),
            ("jose-font-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
                common::set_jose_font(2);
            }),
            ("jose-font-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
                common::set_jose_font(3);
            }),
            ("jose-signs", |s| {
                s.dir = Dir::Attack;
                common::set_palette(13);
                s.signs_sheet = true;
            }),
            ("jose-15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(15);
            }),
            ("jose-16", |s| {
                s.dir = Dir::Attack;
                common::set_palette(16);
            }),
            ("jose-17", |s| {
                s.dir = Dir::Attack;
                common::set_palette(17);
            }),
            ("jose-signs5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(15);
                s.signs_sheet = true;
            }),
            ("jose-18", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
            }),
            ("jose-yellow-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Yellow, 0);
            }),
            ("jose-yellow-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Yellow, 1);
            }),
            ("jose-yellow-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Yellow, 2);
            }),
            ("jose-card-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 0);
            }),
            ("jose-card-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 1);
            }),
            ("jose-card-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 2);
            }),
            ("jose-card-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 3);
            }),
            ("jose-card-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 4);
            }),
            ("jose-card-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 5);
            }),
            ("jose-card-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Card, 6);
            }),
            ("jose-sky-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 3);
            }),
            ("jose-sky-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 4);
            }),
            ("jose-sky-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 5);
            }),
            ("jose-sky-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 0);
            }),
            ("jose-sky-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 1);
            }),
            ("jose-sky-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 2);
            }),
            ("jose-inset-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Inset, 0);
            }),
            ("jose-inset-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Inset, 1);
            }),
            ("jose-inset-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Inset, 2);
            }),
            ("jose-signs8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                s.signs_sheet = true;
                s.fits_sheet = true;
            }),
            ("jose-signs7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                s.signs_sheet = true;
                s.plates_sheet = true;
            }),
            ("jose-signs6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                s.signs_sheet = true;
            }),
            ("angled", |s| {
                s.dir = Dir::Attack;
                s.angled = true;
            }),
            ("attack-d", |s| {
                s.dir = Dir::Attack;
                s.hover_style = 28;
            }),
            ("attack-hovers", |s| {
                s.dir = Dir::Attack;
                s.hovers_sheet = true;
            }),
            ("attack-letters", |s| {
                s.dir = Dir::Attack;
                s.letters_sheet = true;
            }),
        ],
        ..AppSpec::new(
            "Phantom · Persona 5 sketches",
            if edges {
                (W + 2.0 * attack::EDGE, H + 2.0 * attack::EDGE)
            } else if angled {
                (ANGLED_W, H)
            } else {
                (W, H)
            },
            S::new,
            view,
        )
    };
    let mut app = rune::App::new(spec);
    if angled {
        // A titled window kept for its native resizing, traffic lights and shadow: clear, with
        // its content under a see-through titlebar, the traffic lights moved onto the sheet.
        app = app
            .window_options(rune::WindowOptions { transparent: true, ..Default::default() })
            .chrome(rune::Chrome { traffic_lights: Some((attack::SLANT + 18.0, 14.0)), ..rune::Chrome::DEFAULT });
    }
    app.run();
}
