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
    // Jose's logo 15–18 sits over the page, so a hovered button's sheet passes behind it.
    let over = common::jose6() && common::jose_logo_opt() >= 15;
    let brand_el = p.brand.map(|brand| row().pin(None, Some(2.0), Some(PAD + 6.0), None).child(brand(s)));
    if let Some(b) = brand_el.clone().filter(|_| !over) {
        root = root.child(b);
    }
    root = root.child(page.pin(Some(0.0), Some(0.0), None, None));
    if let Some(b) = brand_el.filter(|_| over) {
        root = root.child(b);
    }
    root
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
            ("jose-sky-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 6);
            }),
            ("jose-sky-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 7);
            }),
            ("jose-sky-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 8);
            }),
            ("jose-sky-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 9);
            }),
            ("jose-sky-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 10);
            }),
            ("jose-sky-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 11);
            }),
            ("jose-sky-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 12);
            }),
            ("jose-sky-13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 13);
            }),
            ("jose-sky-14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 14);
            }),
            ("jose-sky-15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 15);
            }),
            ("jose-sky-12+13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 19);
            }),
            ("jose-sky-12+14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 21);
            }),
            ("jose-sky-13+14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 22);
            }),
            ("jose-sky-12+13+14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 23);
            }),
            ("jose-sky-12+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 25);
            }),
            ("jose-sky-13+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 26);
            }),
            ("jose-sky-12+13+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 27);
            }),
            ("jose-sky-14+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 28);
            }),
            ("jose-sky-12+14+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 29);
            }),
            ("jose-sky-13+14+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 30);
            }),
            ("jose-sky-12+13+14+15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 31);
            }),
            ("jose-sky-32", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 32);
            }),
            ("jose-sky-33", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 33);
            }),
            ("jose-sky-34", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 34);
            }),
            ("jose-sky-35", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 35);
            }),
            ("jose-sky-36", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 36);
            }),
            ("jose-sky-37", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 37);
            }),
            ("jose-sky-38", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 38);
            }),
            ("jose-sky-39", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 39);
            }),
            ("jose-sky-40", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 40);
            }),
            ("jose-sky-41", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
            }),
            ("jose-sky-42", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 42);
            }),
            ("jose-sky-43", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 43);
            }),
            ("jose-heap-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Heap, 4);
            }),
            ("jose-heap-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Heap, 1);
            }),
            ("jose-heap-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Heap, 2);
            }),
            ("jose-heap-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Heap, 3);
            }),
            ("jose-stars-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 8);
            }),
            ("jose-stars-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 1);
            }),
            ("jose-stars-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 2);
            }),
            ("jose-stars-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 3);
            }),
            ("jose-stars-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 4);
            }),
            ("jose-stars-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 5);
            }),
            ("jose-stars-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 6);
            }),
            ("jose-stars-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Stars, 7);
            }),
            ("jose-sky-44", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 44);
            }),
            ("jose-sep-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 8);
            }),
            ("jose-sep-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 1);
            }),
            ("jose-sep-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 2);
            }),
            ("jose-sep-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 3);
            }),
            ("jose-sep-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 4);
            }),
            ("jose-sep-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 5);
            }),
            ("jose-sep-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 6);
            }),
            ("jose-sep-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Sep, 7);
            }),
            ("jose-hglow-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::HeapGlow, 4);
            }),
            ("jose-hglow-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::HeapGlow, 1);
            }),
            ("jose-hglow-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::HeapGlow, 2);
            }),
            ("jose-hglow-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::HeapGlow, 3);
            }),
            ("jose-glints-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 4);
            }),
            ("jose-glints-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 1);
            }),
            ("jose-glints-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 2);
            }),
            ("jose-glints-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 3);
            }),
            ("jose-starblur-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 12);
            }),
            ("jose-starblur-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 1);
            }),
            ("jose-starblur-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 2);
            }),
            ("jose-starblur-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 3);
            }),
            ("jose-starblur-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 4);
            }),
            ("jose-starblur-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 5);
            }),
            ("jose-sky-45", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 45);
            }),
            ("jose-sky-46", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 46);
            }),
            ("jose-sky-47", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 47);
            }),
            ("jose-sky-48", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 48);
            }),
            ("jose-glints-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 5);
            }),
            ("jose-glints-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 6);
            }),
            ("jose-glints-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 7);
            }),
            ("jose-glints-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 8);
            }),
            ("jose-starblur-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 6);
            }),
            ("jose-starblur-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 7);
            }),
            ("jose-starblur-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 8);
            }),
            ("jose-glints-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 9);
            }),
            ("jose-glints-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Glints, 10);
            }),
            ("jose-starblur-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 9);
            }),
            ("jose-starblur-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 10);
            }),
            ("jose-starblur-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::StarBlur, 11);
            }),
            ("jose-sky-49", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 49);
            }),
            ("jose-sky-50", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 50);
            }),
            ("jose-sky-51", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 51);
            }),
            ("jose-sky-52", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 52);
            }),
            ("jose-sky-53", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 53);
            }),
            ("jose-sky-54", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 54);
            }),
            ("jose-sky-55", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 55);
            }),
            ("jose-sky-56", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 56);
            }),
            ("jose-mark-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 11);
            }),
            ("jose-mark-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 1);
            }),
            ("jose-mark-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 2);
            }),
            ("jose-mark-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 3);
            }),
            ("jose-mark-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 4);
            }),
            ("jose-mark-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 5);
            }),
            ("jose-sky-57", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 57);
            }),
            ("jose-mark-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 6);
            }),
            ("jose-mark-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 7);
            }),
            ("jose-mark-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 8);
            }),
            ("jose-mark-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 41);
                common::set_jose_opt(common::Part::Mark, 9);
            }),
            ("jose-sky-58", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 58);
            }),
            ("jose-sky-59", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 59);
            }),
            ("jose-sky-60", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 60);
            }),
            ("jose-sky-61", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Sky, 61);
            }),
            ("jose-neb-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 32);
            }),
            ("jose-neb-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 1);
            }),
            ("jose-neb-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 2);
            }),
            ("jose-neb-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 3);
            }),
            ("jose-neb-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 4);
            }),
            ("jose-neb-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 5);
            }),
            ("jose-neb-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 6);
            }),
            ("jose-pop-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 6);
            }),
            ("jose-pop-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 1);
            }),
            ("jose-pop-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 2);
            }),
            ("jose-pop-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 3);
            }),
            ("jose-pop-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 4);
            }),
            ("jose-head-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 6);
            }),
            ("jose-head-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 1);
            }),
            ("jose-head-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 2);
            }),
            ("jose-head-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 3);
            }),
            ("jose-head-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 4);
            }),
            ("jose-head-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Head, 5);
            }),
            ("jose-logo-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 33);
            }),
            ("jose-logo-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 1);
            }),
            ("jose-logo-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 2);
            }),
            ("jose-logo-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 3);
            }),
            ("jose-logo-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 4);
            }),
            ("jose-logo-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 5);
            }),
            ("jose-bar-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 0);
            }),
            ("jose-bar-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 1);
            }),
            ("jose-bar-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 2);
            }),
            ("jose-bar-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 3);
            }),
            ("jose-bar-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 4);
            }),
            ("jose-pill-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 1);
                common::set_jose_opt(common::Part::Pill, 7);
            }),
            ("jose-pill-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 1);
                common::set_jose_opt(common::Part::Pill, 1);
            }),
            ("jose-pill-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 1);
                common::set_jose_opt(common::Part::Pill, 2);
            }),
            ("jose-pill-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 1);
                common::set_jose_opt(common::Part::Pill, 3);
            }),
            ("jose-foottext-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 8);
            }),
            ("jose-foottext-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
            }),
            ("jose-foottext-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 2);
            }),
            ("jose-foottext-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 3);
            }),
            ("jose-foottext-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 4);
            }),
            ("jose-footshape-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
                common::set_jose_opt(common::Part::FootShape, 8);
            }),
            ("jose-footshape-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
                common::set_jose_opt(common::Part::FootShape, 1);
            }),
            ("jose-footshape-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
                common::set_jose_opt(common::Part::FootShape, 2);
            }),
            ("jose-footshape-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
                common::set_jose_opt(common::Part::FootShape, 3);
            }),
            ("jose-footshape-4", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 1);
                common::set_jose_opt(common::Part::FootShape, 4);
            }),
            ("jose-neb-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 7);
            }),
            ("jose-neb-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 8);
            }),
            ("jose-neb-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 9);
            }),
            ("jose-neb-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 10);
            }),
            ("jose-pop-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::HeapPop, 5);
            }),
            ("jose-logo-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 6);
            }),
            ("jose-logo-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 7);
            }),
            ("jose-logo-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 8);
            }),
            ("jose-logo-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 9);
            }),
            ("jose-logo-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 10);
            }),
            ("jose-bar-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 5);
            }),
            ("jose-bar-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 6);
            }),
            ("jose-bar-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 7);
            }),
            ("jose-pillm-3", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 4);
                common::set_jose_opt(common::Part::Pill, 3);
            }),
            ("jose-pillm-4", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 4);
                common::set_jose_opt(common::Part::Pill, 4);
            }),
            ("jose-pillm-5", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 4);
                common::set_jose_opt(common::Part::Pill, 5);
            }),
            ("jose-pillm-6", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 4);
                common::set_jose_opt(common::Part::Pill, 6);
            }),
            ("jose-foottext-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 5);
            }),
            ("jose-foottext-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 6);
            }),
            ("jose-foottext-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 7);
            }),
            ("jose-footflag-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 6);
                common::set_jose_opt(common::Part::FootShape, 2);
            }),
            ("jose-footflag-5", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 6);
                common::set_jose_opt(common::Part::FootShape, 5);
            }),
            ("jose-footflag-6", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 6);
                common::set_jose_opt(common::Part::FootShape, 6);
            }),
            ("jose-footflag-7", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootText, 6);
                common::set_jose_opt(common::Part::FootShape, 7);
            }),
            ("jose-neb-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 11);
            }),
            ("jose-neb-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 12);
            }),
            ("jose-neb-13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 13);
            }),
            ("jose-neb-14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 14);
            }),
            ("jose-logo-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 11);
            }),
            ("jose-logo-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 12);
            }),
            ("jose-logo-13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 13);
            }),
            ("jose-logo-14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 14);
            }),
            ("jose-bar-8", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 8);
            }),
            ("jose-bar-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 9);
            }),
            ("jose-bar-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 10);
            }),
            ("jose-bar-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 11);
            }),
            ("jose-rows-0", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 6);
            }),
            ("jose-rows-1", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 1);
            }),
            ("jose-rows-2", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 2);
            }),
            ("jose-rows-3", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 3);
            }),
            ("jose-rows-4", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 4);
            }),
            ("jose-neb-15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 15);
            }),
            ("jose-neb-16", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 16);
            }),
            ("jose-neb-17", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 17);
            }),
            ("jose-neb-18", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 18);
            }),
            ("jose-neb-19", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 19);
            }),
            ("jose-logo-15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 15);
            }),
            ("jose-logo-16", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 16);
            }),
            ("jose-logo-17", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 17);
            }),
            ("jose-logo-18", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 18);
            }),
            ("jose-bar-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 12);
            }),
            ("jose-bar-13", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 13);
            }),
            ("jose-bar-14", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 14);
            }),
            ("jose-rows-5", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 5);
            }),
            ("jose-neb-20", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 20);
            }),
            ("jose-neb-21", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 21);
            }),
            ("jose-neb-22", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 22);
            }),
            ("jose-neb-23", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 23);
            }),
            ("jose-neb-24", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 24);
            }),
            ("jose-logo-19", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 19);
            }),
            ("jose-logo-20", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 20);
            }),
            ("jose-logo-21", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 21);
            }),
            ("jose-footflag-9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootShape, 9);
            }),
            ("jose-footflag-10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootShape, 10);
            }),
            ("jose-footflag-11", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootShape, 11);
            }),
            ("jose-bar-15", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 15);
            }),
            ("jose-bar-16", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 16);
            }),
            ("jose-bar-17", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 17);
            }),
            ("jose-logo-22", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 22);
            }),
            ("jose-logo-23", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 23);
            }),
            ("jose-logo-24", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 24);
            }),
            ("jose-neb-25", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 25);
            }),
            ("jose-neb-26", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 26);
            }),
            ("jose-neb-27", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 27);
            }),
            ("jose-neb-28", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 28);
            }),
            ("jose-bar-18", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 18);
            }),
            ("jose-bar-19", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 19);
            }),
            ("jose-bar-20", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 20);
            }),
            ("jose-footflag-12", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::FootShape, 12);
            }),
            ("jose-rowsx-1-0", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 1);
                common::set_jose_opt(common::Part::Muted, 4);
            }),
            ("jose-rowsx-7-0", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 7);
                common::set_jose_opt(common::Part::Muted, 4);
            }),
            ("jose-rowsx-7-1", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 7);
                common::set_jose_opt(common::Part::Muted, 1);
            }),
            ("jose-rowsx-7-2", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 7);
                common::set_jose_opt(common::Part::Muted, 2);
            }),
            ("jose-rowsx-7-3", |s| {
                s.dir = Dir::Attack;
                s.many = true;
                s.selected = Some(4);
                common::set_palette(18);
                common::set_jose_opt(common::Part::RowRead, 7);
                common::set_jose_opt(common::Part::Muted, 3);
            }),
            ("jose-neb-29", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 29);
            }),
            ("jose-neb-30", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 30);
            }),
            ("jose-neb-31", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Neb, 31);
            }),
            ("jose-bar-21", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 21);
            }),
            ("jose-bar-22", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 22);
            }),
            ("jose-bar-23", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 23);
            }),
            ("jose-bar-24", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 24);
            }),
            ("jose-bar-25", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 25);
            }),
            ("jose-bar-26", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 26);
            }),
            ("jose-logo-25", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 25);
            }),
            ("jose-logo-26", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 26);
            }),
            ("jose-bar60-21", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 21);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-22", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 22);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-23", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 23);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-24", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 24);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-25", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 25);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-26", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 26);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-27", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 27);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-bar60-28", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 28);
                common::set_jose_opt(common::Part::ErrAt, 6);
            }),
            ("jose-logo-27", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 27);
            }),
            ("jose-logo-28", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 28);
            }),
            ("jose-logo-29", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 29);
            }),
            ("jose-logo-30", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 30);
            }),
            ("jose-logo-31", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 31);
            }),
            ("jose-logo-32", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Logo, 32);
            }),
            ("jose-bar60-29", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Bar, 29);
                common::set_jose_opt(common::Part::ErrAt, 6);
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
            ("jose-19", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
            }),
            ("jose-19-pace-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Pace, 1);
            }),
            ("jose-19-pace-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Pace, 2);
            }),
            ("jose-19-center-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 1);
            }),
            ("jose-19-center-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 2);
            }),
            ("jose-19-center-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 3);
            }),
            ("jose-20-amp-0", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 2);
                common::set_jose_opt(common::Part::Amp, 0);
            }),
            ("jose-20-amp-1", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 2);
                common::set_jose_opt(common::Part::Amp, 1);
            }),
            ("jose-20-amp-2", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 2);
                common::set_jose_opt(common::Part::Amp, 2);
            }),
            ("jose-20-amp-3", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Center, 2);
                common::set_jose_opt(common::Part::Amp, 3);
            }),
            ("jose-21", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Glow, 2);
            }),
            ("jose-21-noglow", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                common::set_jose_opt(common::Part::Morph, 7);
                common::set_jose_opt(common::Part::Glow, 1);
            }),
            ("jose-signs10", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                s.signs_sheet = true;
                s.morph_sheet = true;
                s.tilt_sheet = true;
            }),
            ("jose-signs9", |s| {
                s.dir = Dir::Attack;
                common::set_palette(18);
                s.signs_sheet = true;
                s.morph_sheet = true;
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
