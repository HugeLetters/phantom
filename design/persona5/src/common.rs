//! What the three directions share: the palette, the demo torrents, the sketch's state, and the
//! drawing helpers (slanted and jagged polygons, halftone, the masked ghost, ransom-note letters).

use std::f32::consts::PI;

use rune::{Color, El, Family, LineCap, Path, Phase, Shape, TextStyle, canvas, el, row, text};

pub fn hex(s: &str) -> Color {
    rune::color::hex(s).expect("a hex colour")
}

/// The palette the theme draws in: 0 is the red theme; 1–4 are steps towards a Jose palette
/// (blue and yellow): 1 swaps red for blue, 2 adds yellow selections, 3 a night sky, 4 cream
/// paper. Global, so every colour function follows it; a scene sets it.
static PALETTE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn palette() -> u8 {
    PALETTE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_palette(p: u8) {
    PALETTE.store(p, std::sync::atomic::Ordering::Relaxed);
}

/// The theme's main colour: red, or Jose's blue.
pub fn red() -> Color {
    if palette() == 0 { hex("#e60012") } else { hex("#1f4fe0") }
}
/// The main colour's dark shade: deep red, or navy.
pub fn deep() -> Color {
    if palette() == 0 { hex("#a8000c") } else { hex("#0c2466") }
}
/// The selection band's colour: the main colour, or Jose's yellow flag.
pub fn select() -> Color {
    if jose6() {
        return jose_yellow();
    }
    if palette() >= 2 { hex("#ffd60a") } else { red() }
}
/// Text on the selection band.
pub fn on_select() -> Color {
    if jose6() && jose_mark_opt() == 8 {
        return hex("#0b1f5c");
    }
    if jose6() && jose_mark_opt() == 9 {
        return hex("#1f4fd1");
    }
    if palette() >= 2 { black() } else { paper() }
}
pub fn black() -> Color {
    hex("#0b0b0b")
}
pub fn paper() -> Color {
    if matches!(palette(), 4 | 6 | 8) || palette() >= 9 { hex("#fff3c4") } else { hex("#f4f2ec") }
}

// Jose's round 2 (palettes 5–8) builds on 3 (night sky) and 4 (cream paper) with his shapes:
// 5 is 3 with a flag for the selection and his slab colours; 6 is 4 with the flag, slabs and torn
// tape; 7 is 3 with the flag, slabs and a see-through card for the list; 8 is 4 with all of it.

/// The selection is a swaying flag rather than a band.
pub fn flag() -> bool {
    palette() >= 5
}
/// The field and the column headings are strips of torn tape.
pub fn tape() -> bool {
    matches!(palette(), 6 | 8) || jose3()
}

/// Jose's round 3 (palettes 9–11): round 2's 8 with glow, moons, a bowed translucent flag,
/// counter-plate speeds, cream for white, a glowing yellow header and coral errors. The three
/// differ in the list card's colour.
pub fn jose3() -> bool {
    palette() >= 9
}

/// Jose's round 4 (palettes 12–14): real blurred glows, comets along the arcs, drifting moons,
/// a rounded font, signboard buttons. The three differ in the navy card: 12 vivid, 13 frosted,
/// 14 frosted with a violet glow.
pub fn jose4() -> bool {
    palette() >= 12
}

/// Jose's round 5 (palettes 15–17): a clearer card, fainter glows, SF Compact Rounded, a crowded
/// drift of C-moons and rounded stars, signboards after the shop's. The three differ in the card:
/// 15 clear navy, 16 clearer vivid blue, 17 dark smoke.
pub fn jose5() -> bool {
    palette() >= 15
}

/// Jose's round 6 (palette 18): split by element, each with its own options in `jose_opt`.
pub fn jose6() -> bool {
    palette() >= 18
}

/// Round 6's options, one per element: the yellow, the card, the sky, the flag's text inset,
/// the signboards' hover.
#[derive(Clone, Copy)]
pub enum Part {
    Yellow = 0,
    Card = 1,
    Sky = 2,
    Inset = 3,
    Hover = 4,
    /// Which plate the signboards drop: 0 none, 1 the yellow board on hover, 2 the yellow board
    /// and its askew frame, 3 the gold glass at rest.
    Plate = 5,
    /// The hover sheet's size and place: 0 the big sheet, 1 the board's size up and left, 2 the
    /// board's size down and right, 3 a narrower sheet tilted across the label.
    Fit = 6,
    /// Round 9's morphing sheet: 0 off, 1 up and left, 2 down and right, 3 up and left with the
    /// askew frame.
    Morph = 7,
    /// The hover sheet's pace: 0 now (1.5× round 10), 1 +20%, 2 +40%.
    Pace = 8,
    /// Round 11's sheet: 0 as round 10, 1 10% smaller, 2 smaller with its centre wandering at
    /// random, 3 smaller with its centre circling slowly.
    Center = 9,
    /// The sheet's corner wander: 0 as before, 1 20% less, 2 40% less, 3 60% less.
    Amp = 10,
    /// The heap's spread: 0 at random, 1 spaced with some jitter, 2 spaced with some jitter and
    /// one shared speed, 3 spaced with less jitter, speeds a touch apart.
    Heap = 11,
    /// The night stars: 0 as before, 1 varied sizes and glows, slower twinkle, 2 slower still,
    /// 3 1 with a few bright stars in wide halos; 4–7 3 with 500, 900, 1400, 2400 stars, mostly
    /// faint. Settled on 7 (also 0); 8 is the old 0.
    Stars = 12,
    /// The hovered board's golden glow: off (settled; 0 or 1), 2 on.
    Glow = 13,
    /// Telling overlapping moons and stars apart: 0 none, 1 a thin dark outline, 2 each a
    /// slightly different shade, 3 a small dark offset shadow, 4 outline and shades; 5–7 are 2
    /// with a smaller spread of shades (70% of it, then 50%, then about 30%). Settled on 6
    /// (also 0); 8 is none.
    Sep = 14,
    /// The heap's glow: settled on the tightest and faintest (0 or 3); 1 tighter, 2 tighter and
    /// fainter, 4 the old wide glow.
    HeapGlow = 15,
    /// White glints in the heap: settled on 90 with four-point sparkles on every sixth (0);
    /// 1 90 without, 2 150, 3 150 with sparkles, 4 the old 45; 5 sparkles on every tenth, 6 any
    /// dot flashing a sparkle every 6–10 s, 7 every 12–20 s, 8 every twentieth always and the
    /// rest as 7, 9 8 with those in magenta. Settled on 6 (also 0); 10 is the old every sixth.
    Glints = 16,
    /// How soft the night stars are: 0 as before (a hard white core in a halo disc), 1 halos
    /// fading out in rings, 2 a softened core, 3 both, 4 both with a wider halo, 5 4 with the
    /// core softer still; 6–8 3 with the small stars softened too, each step more; 9–11
    /// between 3 and 6. Settled on 10 (also 0); 12 is the old 0.
    StarBlur = 17,
    /// The selected torrent's mark: 0 the inverted letter (P5's), 1 a small twinkling star over
    /// one letter, 2 a crescent moon hooked over the name's start, 3 a sparkle running along the
    /// name once, then resting at its end, 4 an underline of tiny stars filling in, 5 one letter
    /// in lemon on a navy tile; 6–9 no mark, the name in blue: 6 navy, 7 royal blue, 8 and 9
    /// the whole row's text in them; 10 none, the name plain. Settled on 10 (also 0); 11 is the
    /// inverted letter.
    Mark = 18,
    /// Structure inside the nebulae, drawn sharper over the blurred clouds: 0 none, 1 thin
    /// bright filaments, 2 dark dust lanes, 3 mottling (light and dark knots), 4 bright cores
    /// with brighter shell rims, 5 all four, 6 5 with violet and pink accents; 7–10 2's lanes
    /// alone, sharper: 7 crisp, 8 more and thinner, 9 and 10 7 and 8 on crisper clouds; 11–14
    /// a noise texture instead of shapes: 11 billows, 12 with dust lanes, 13 12 with filaments,
    /// 14 13 covering more of the sky; 15 13 with less detail, 16 15 covering less, 17 16 over the
    /// shape clouds, 18 17 covering less again, 19 17 from another seed; 20–24 18 with more
    /// detail back (20), softer but peaks kept (21), a little sharper (22), at full detail (23),
    /// 22 cross-fading to another sky (24); 25 18 dimmed, 26 21 dimmed, 27 21 dimmer, 28 26
    /// cross-fading very slowly (90 s each way); 29 18 at 55%, 30 29 cross-fading slowly, 31 27
    /// cross-fading slowly. Settled on 30 (also 0); 32 is none.
    Neb = 19,
    /// The heap's vibrancy: 0 as before, 1 shades towards gold and white instead of olive,
    /// 2 1 on a more saturated lemon, 3 2 with the card's smoke clearing over the heap, 4 3 with
    /// a stronger glow, 5 2 with 4's glow, the card's smoke left as it is. Settled on 5 (also 0);
    /// 6 is the old 0.
    HeapPop = 20,
    /// The list header's lettering: 0 ransom letters (C's), 1 rounded caps in navy, 2 1 with a
    /// small star before the first, 3 hand-set (each letter nudged and tilted), 4 cream caps
    /// outlined in navy, 5 title case in the rows' black. Settled on 3 (also 0); 6 is the ransom.
    Head = 21,
    /// The top-right logo: 0 ransom letters (C's), 1 rounded lemon letters in a navy outline,
    /// 2 1 with a crescent moon for the O, 3 lemon on a small tilted navy signboard, 4 spaced
    /// letters with twinkling stars between some, 5 hand-set lemon letters, each nudged and tilted,
    /// 6 2 and 5 together, 7 the shop's glowing pale lowercase, 8 7 bouncing (sizes, heights,
    /// tilts), 9 8 with a crescent for the o, 10 8 in lemon; 11 and 12 8 and 10 with a capital P,
    /// 13 and 14 11 and 12 larger, bouncing more; 15–18 13 see-through and a little blue, higher
    /// (15; 16 more see-through, 17 bluer, 18 at 85%); 19–21 15 with green: 19 mint, 20 blue
    /// turning mint along the word, 21 a mint inner glow in a blue outer one; 22–24 the
    /// reference's green, sampled (core #e6f8d4): 22 a pale green glow, 23 greener, 24 23 less
    /// see-through; 25 24 with the core a little see-through, 26 25 with the glow too; 27–29
    /// more see-through again (the core at 62%, 50%, 40%); 30–32 27 more vibrant: 30 a more
    /// saturated green, 31 more again, 32 a bright core in a deep green glow. Settled on 32 (also
    /// 0); 33 is the ransom letters.
    Logo = 22,
    /// Progress bars: 0 C's (a different fill per state), 1 a rounded navy track with a lemon
    /// fill and a small state mark at its tip, 2 a row of stars filling in, 3 1 with a waxing
    /// moon at the tip, 4 1 glowing, sparkles drifting along it while it downloads; 5–7 C's
    /// pattern per state in Jose's motifs (racing stars, marching crescents, a scanned starry
    /// halftone, still crescents, a torn red end, a sliding moon): 5 rounded, 6 5 glowing, 7 6
    /// on C's slanted track; 8–11 round two (sparkles fading in and out, slow white crescents, a
    /// twinkle wave, breathing sleepy crescents): 8 rounded and glowing, 9 8 with z's when paused,
    /// 10 8 on C's slanted track, 11 10 with C's arrowhead while downloading; 12–14 round three:
    /// 8's downloading and checking, still crescents and z's in the track when paused, and three
    /// takes each on seeding (rocking, rising, flowing out), fetching (a shooting star, stars
    /// lighting in turn, a moon turning its phase) and error (an eclipse, a fallen star, a dying
    /// flicker); 15–17 mix them, with 13's fetching: seeding 13's rising as stars twice the size
    /// (15, 17) or 14's flowing out (16), and error as a snapped track (15), a "!" badge (16),
    /// or a bigger, plainer fallen star (17); 18–20 15 with smaller rising stars and new errors:
    /// a star falling from the bar's end (18), a cracked crescent (19), 17 pulsing (20); 21–26
    /// errors started over: a red rim (21), hazard stripes (22), clouds drifting over (23), a
    /// no-entry sign (24), the bar breathing red (25), C's twitch (26); 27 22's hazard stripes
    /// with 26's twitch, 28 the same with a gentler twitch, 29 28 twitching up and down too; 30
    /// and 31 a twitch you can see (up to 2.5 and 4 points, about a fifth of a second). Settled on
    /// 29 (also 0); 32 is C's bars.
    Bar = 23,
    /// Percentage pills: 0 C's slanted slips, 1 a rounded lemon capsule with navy digits, 2 a
    /// navy capsule with lemon digits and a lemon rim, 3 no pill, the number alone, 4 3 over a
    /// soft dark halo, 5 3 in cream with a navy outline, 6 the number inside the bar's own track.
    /// Settled on 3 (also 0); 7 is C's slips.
    Pill = 24,
    /// The footer's status: 0 the ghost and ransom "CONNECTED" (C's), 1 "Connected" in the
    /// rounded face with a twinkling star as its light, 2 lemon, with a glowing full moon (it
    /// would wax while connecting), 3 the ghost kept, the text rounded, 4 the text alone; 5–7 2
    /// with the moon's phase moving: 5 a slow full cycle, 6 connecting (waxing, holding, waning
    /// to reconnect), 7 full, breathing a little. Settled on 6 (also 0), dropping to new in 0.4 s;
    /// 8 is the ghost and ransom.
    FootText = 25,
    /// The footer's shape: 0 C's black slanted strip, 1 a navy strip with rounded ends and a thin
    /// lemon rim, floating, 2 a lemon ribbon with navy text, gently curved, 3 no strip, the text
    /// on the heap over a soft dark shadow, 4 two small signboards, status left and totals right;
    /// 5–7 the selected row's flag, swaying gently: 5 lemon, 6 still, 7 a swallowtail at both
    /// ends; 8 C's strip; 9–12 the flag told from the heap without an outline: 9 cream, 10 navy,
    /// 11 frosted, 12 cream and frosted. Settled on 12 (also 0).
    FootShape = 26,
    /// Rows over the heap: 0 as now, 1 the card's smoke growing denser towards the bottom, 2 a
    /// dark band behind each row, 3 the smoke dense only over the heap's height, 4 a soft,
    /// blurred dark band behind each row's text, 5 between 1 and 3. Settled on 1 (also 0); 6 is
    /// none; 7 1 denser at the very bottom (about 78%). Settled on 7.
    RowRead = 27,
    /// The rows' secondary text (sizes, dashes), grey in C: 0 grey, 1 the rows' cream, 2 pale
    /// blue, 3 pale lemon, 4 grey. Settled on 3, pale lemon (also 0).
    Muted = 28,
    /// For comparing error designs only: 0 the data's progress, otherwise errored torrents drawn
    /// at that many tenths (6 = 60%), so the error's fill has room to show.
    ErrAt = 29,
}
static JOSE_OPTS: [std::sync::atomic::AtomicU8; 30] = [const { std::sync::atomic::AtomicU8::new(0) }; 30];
/// The Jose sky: settled on 60 (also 0); 62 is the old 0.
pub fn jose_sky_opt() -> u8 {
    match jose_opt(Part::Sky) {
        0 => 60,
        62 => 0,
        v => v,
    }
}

/// The heap's vibrancy, with the settled 5 as 0 and the old look at 6.
pub fn jose_heap_pop() -> u8 {
    match jose_opt(Part::HeapPop) {
        0 => 5,
        6 => 0,
        v => v,
    }
}

/// Rows over the heap, with the settled 7 (the smoke thickening to about 78% at the bottom) as
/// 0 and none at 6.
pub fn jose_rows_opt() -> u8 {
    match jose_opt(Part::RowRead) {
        0 => 7,
        6 => 0,
        v => v,
    }
}

/// The list header's lettering, with the settled hand-set caps (3) as 0 and the ransom at 6.
pub fn jose_head_opt() -> u8 {
    match jose_opt(Part::Head) {
        0 => 3,
        6 => 0,
        v => v,
    }
}

/// The nebulae, with the settled 30 (18 at 55%, cross-fading slowly) as 0, and none (the shape
/// clouds alone) at 32.
pub fn jose_neb_opt() -> u8 {
    match jose_opt(Part::Neb) {
        0 => 30,
        32 => 0,
        v => v,
    }
}

/// The logo, with the settled 32 (the reference's glowing green, see-through, over the
/// toolbar) as 0 and the ransom letters at 33.
pub fn jose_logo_opt() -> u8 {
    match jose_opt(Part::Logo) {
        0 => 32,
        33 => 0,
        v => v,
    }
}

/// The buttons' morphing sheet, with the approved 7 (round 12) as 0 and none at 8. Before, the
/// approved look was only set by the comparison scenes, so the default showed round 5's.
pub fn jose_morph_opt() -> u8 {
    match jose_opt(Part::Morph) {
        0 => 7,
        8 => 0,
        v => v,
    }
}

/// The selected torrent's mark, with the settled one (10) as 0 and P5's inverted letter at 11.
pub fn jose_mark_opt() -> u8 {
    match jose_opt(Part::Mark) {
        0 => 10,
        11 => 0,
        v => v,
    }
}

pub fn set_jose_opt(p: Part, v: u8) {
    JOSE_OPTS[p as usize].store(v, std::sync::atomic::Ordering::Relaxed);
}
pub fn jose_opt(p: Part) -> u8 {
    JOSE_OPTS[p as usize].load(std::sync::atomic::Ordering::Relaxed)
}

/// Jose's yellow, greener than before, after the shop's lemon (measured near `#fcf664`):
/// 0 lemon, 1 deeper lemon, 2 between lemon and the old gold.
pub fn jose_yellow() -> Color {
    match jose_opt(Part::Yellow) {
        1 => hex("#f4e83a"),
        2 => hex("#ffe84a"),
        _ => hex("#fbf25a"),
    }
}
/// A pale tint of the lemon, for ghosts, sheens and comets.
pub fn jose_pale() -> Color {
    hex("#fbf7a6")
}
/// The yellow's glow: a little deeper than the body.
pub fn jose_yellow_glow() -> Color {
    match jose_opt(Part::Yellow) {
        1 => hex("#e6d418"),
        2 => hex("#f5cc12"),
        _ => hex("#efe02a"),
    }
}

/// Jose's font, chosen for the comparison: 0 Hiragino Maru Gothic, 1 SF Pro Rounded, 2 SF
/// Compact Rounded, 3 Arial Rounded.
static JOSE_FONT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);
pub fn set_jose_font(f: u8) {
    JOSE_FONT.store(f, std::sync::atomic::Ordering::Relaxed);
}
pub fn jose_family() -> &'static str {
    match JOSE_FONT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ".SF NS Rounded",
        2 => ".SF Compact Rounded",
        3 => "Arial Rounded MT Bold",
        // Unset: round 5 settled on SF Compact Rounded.
        255 if jose5() => ".SF Compact Rounded",
        _ => "Hiragino Maru Gothic ProN",
    }
}

/// Jose's signboard button style: 0 yellow plates, 1 translucent plates with a cream rim, 2
/// counter plates.
static JOSE_SIGN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub fn set_jose_sign(v: u8) {
    JOSE_SIGN.store(v, std::sync::atomic::Ordering::Relaxed);
}
pub fn jose_sign() -> u8 {
    JOSE_SIGN.load(std::sync::atomic::Ordering::Relaxed)
}

/// The colour an error is drawn in: the main colour, or coral where blue would vanish on navy.
pub fn error_c() -> Color {
    // Round 6 uses the bars' red, so an error's status, number and bar all match.
    if jose6() {
        hex("#ff5a6a")
    } else if jose3() {
        hex("#ff7a59")
    } else {
        red()
    }
}
/// The list sheet is a see-through navy card with a cyan glow for a rim.
pub fn glass() -> bool {
    matches!(palette(), 7 | 8) || jose3()
}
/// The download and upload slabs: cyan and pink, or Jose's teal and white.
pub fn speed_colours() -> (Color, Color) {
    if jose3() {
        (hex("#7fe7ff"), hex("#fff3c4"))
    } else if palette() >= 5 {
        (hex("#7fe7ff"), hex("#ffffff"))
    } else {
        (hex("#4fe3e3"), hex("#ff5ec4"))
    }
}

/// A strip `w` × `h` of tape, its short ends torn.
pub fn tape_strip(w: f32, h: f32, seed: u32) -> Vec<[f32; 2]> {
    let n = 5;
    let tear = |k: u32| 2.0 + 4.0 * noise(seed.wrapping_mul(31).wrapping_add(k));
    let mut pts = vec![[6.0, 0.0], [w - 6.0, 0.0]];
    for i in 1..n {
        pts.push([w - 6.0 + if i % 2 == 1 { tear(i) } else { -tear(i) * 0.6 }, h * i as f32 / n as f32]);
    }
    pts.push([w - 6.0, h]);
    pts.push([6.0, h]);
    for i in (1..n).rev() {
        pts.push([6.0 - if i % 2 == 1 { tear(i + 20) } else { -tear(i + 20) * 0.6 }, h * i as f32 / n as f32]);
    }
    pts
}

// ------------------------------------------------------------------ fonts (macOS system fonts)

/// Condensed caps for labels: Avenir Next Condensed ships with macOS, so a theme can name it
/// without bundling it.
pub fn caps(size: f32, weight: f32) -> TextStyle {
    if jose4() {
        return TextStyle::new(size * 0.95, weight.min(700.0)).family(Family::Named(jose_family())).tracking(0.3);
    }
    TextStyle::new(size, weight).family(Family::Named("Avenir Next Condensed")).tracking(0.4)
}
pub fn sans(size: f32, weight: f32) -> TextStyle {
    if jose4() {
        return TextStyle::new(size * 0.95, weight.min(600.0)).family(Family::Named(jose_family()));
    }
    TextStyle::new(size, weight).family(Family::Named("Avenir Next"))
}
pub fn figures(size: f32) -> TextStyle {
    if jose4() {
        return TextStyle::new(size * 0.95, 500.0).family(Family::Named(jose_family())).features("'tnum' 1");
    }
    TextStyle::new(size, 600.0).family(Family::Named("Avenir Next Condensed")).features("'tnum' 1")
}

// ------------------------------------------------------------------ state

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Card,
    Accent,
    Attack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Idle,
    Hover,
    Press,
    Disabled,
}

pub struct S {
    pub dir: Dir,
    /// The angled-window spike: the window is a slanted four-sided shape, its corners clear.
    pub angled: bool,
    /// The sheet comparing Jose's signboard buttons.
    pub signs_sheet: bool,
    /// The signboard sheet compares plates (round 7) rather than hover sheets.
    pub plates_sheet: bool,
    /// The signboard sheet compares sheet sizes (round 8).
    pub fits_sheet: bool,
    /// The signboard sheet compares morphing sheets (round 9).
    pub morph_sheet: bool,
    /// With `morph_sheet`: the tilted sheets (round 10).
    pub tilt_sheet: bool,
    /// With `angled`, which edge shape the window takes (see `attack::EDGES`); 0 is the
    /// spike's full slant.
    pub edge_style: u8,
    /// The window's size in points, as last laid out (the angled frame follows it).
    pub win: [f32; 2],
    /// Presses the window has seen, and where the last one was: tells whether a click on a
    /// clear corner reached the window or the app behind it.
    pub presses: u32,
    pub last_press: [f32; 2],
    /// The component sheet instead of the window.
    pub sheet: bool,
    pub hovered: Option<String>,
    pub pressed: Option<String>,
    pub selected: Option<usize>,
    /// Seconds since the sketch started, for the time-driven motion.
    pub t: f32,
    /// When the selection last changed, for the selection's snap-in.
    pub selected_at: f32,
    /// When the pointer last entered something, for the active shape's burst.
    pub hovered_at: f32,
    /// The last thing the pointer left, and when: lets a highlight play out instead of snapping
    /// off.
    pub left: Option<(String, f32)>,
    /// The last thing released, and when: lets a press spring back instead of snapping.
    pub released: Option<(String, f32)>,
    /// How many times anything was hovered, pressed or selected: each activation seeds its own
    /// random tilt and shape, so a button never lands the same way twice.
    pub activations: u32,
    /// How the toolbar buttons letter their labels (see `attack::label`).
    pub lettering: u8,
    /// The lettering comparison sheet instead of the window.
    pub letters_sheet: bool,
    /// How an active button shows it (see `attack::HOVERS`).
    pub hover_style: u8,
    /// The hover comparison sheet instead of the window.
    pub hovers_sheet: bool,
    /// How rows step right (see `attack::STEPS`), and the sheet comparing them.
    pub step_style: u8,
    pub steps_sheet: bool,
    /// How speeds stand out (see `attack::SPEEDS`), and the sheet comparing them.
    pub speed_style: u8,
    pub speeds_sheet: bool,
    /// A long list (16 torrents) instead of the six, to see the row steps over many rows.
    pub many: bool,
    /// A very long list (64 torrents, the 16 four times over) in a scrolling viewport, to see
    /// the list scroll.
    pub scrolling: bool,
    /// How progress bars sit on the sheet (see `attack::TRACKS`), and the sheet comparing them.
    pub track_style: u8,
    pub tracks_sheet: bool,
}

impl S {
    pub fn new() -> S {
        S {
            dir: Dir::Card,
            sheet: false,
            hovered: None,
            pressed: None,
            selected: Some(0),
            t: 0.0,
            selected_at: -10.0,
            hovered_at: -10.0,
            left: None,
            released: None,
            activations: 0,
            // P2 + one box, as the user chose: at least one lowercase letter, at most half.
            lettering: 21,
            letters_sheet: false,
            // G own colours (tight, morphing, keeps its colours, red on press); D black press (28)
            // is kept beside it, as a setting or for the Jose palette.
            hover_style: 25,
            hovers_sheet: false,
            // B2 (random per row, never close to the row above) at 7 pt, as the user chose.
            step_style: 10,
            steps_sheet: false,
            // F: black figures on a living cyan / pink slab, as the user chose.
            speed_style: 6,
            speeds_sheet: false,
            many: false,
            scrolling: false,
            angled: false,
            signs_sheet: false,
            plates_sheet: false,
            fits_sheet: false,
            morph_sheet: false,
            tilt_sheet: false,
            edge_style: 0,
            win: [0.0, 0.0],
            presses: 0,
            last_press: [0.0, 0.0],
            // B, level top and bottom with leaning sides and front edge, its patterns straight; a
            // red shadow on error and none while seeding (the user's picks).
            track_style: 19,
            tracks_sheet: false,
        }
    }

    /// What an interactive piece keyed `key` looks like now.
    pub fn look(&self, key: &str, disabled: bool) -> Look {
        if disabled {
            Look::Disabled
        } else if self.pressed.as_deref() == Some(key) {
            Look::Press
        } else if self.hovered.as_deref() == Some(key) {
            Look::Hover
        } else {
            Look::Idle
        }
    }
}

/// Tracks hover and press on `e` under `key`; a release inside runs `on_up`.
pub fn interactive(e: El, key: String, on_up: impl Fn(&mut S) + 'static) -> El {
    let (k1, k2) = (key.clone(), key.clone());
    e.key(key)
        .on_hover_change(move |s: &mut S, inside, _| {
            if inside {
                s.hovered = Some(k1.clone());
                s.hovered_at = s.t;
                s.activations += 1;
            } else if s.hovered.as_deref() == Some(&k1) {
                s.hovered = None;
                s.left = Some((k1.clone(), s.t));
            }
        })
        .on_drag(move |s: &mut S, d, _| match d.phase {
            Phase::Down => {
                s.pressed = Some(k2.clone());
                s.activations += 1;
            }
            Phase::Move => {}
            Phase::Up => {
                s.pressed = None;
                s.released = Some((k2.clone(), s.t));
                let inside = d.x >= 0.0 && d.y >= 0.0 && d.x <= d.w && d.y <= d.h;
                if inside && !d.cancelled() {
                    on_up(s);
                }
            }
        })
}

// ------------------------------------------------------------------ demo data

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Downloading,
    Seeding,
    Checking,
    Paused,
    Fetching,
    Error,
}

#[derive(Clone, Copy)]
pub struct Torrent {
    pub name: &'static str,
    pub progress: f32,
    pub kind: Kind,
    pub status: &'static str,
    pub size: &'static str,
    pub down: &'static str,
    pub up: &'static str,
    pub peers: &'static str,
    pub eta: &'static str,
}

pub const TORRENTS: &[Torrent] = &[
    Torrent {
        name: "debian-13.7.0-amd64-netinst.iso",
        progress: 0.545,
        kind: Kind::Downloading,
        status: "Downloading",
        size: "756 MiB",
        down: "68 MiB/s",
        up: "–",
        peers: "32",
        eta: "5s",
    },
    Torrent {
        name: "ubuntu-24.04.3-desktop-amd64.iso",
        progress: 1.0,
        kind: Kind::Seeding,
        status: "Seeding",
        size: "6.0 GiB",
        down: "–",
        up: "1.5 MiB/s",
        peers: "7",
        eta: "–",
    },
    Torrent {
        name: "Big Buck Bunny (2008)",
        progress: 0.61,
        kind: Kind::Checking,
        status: "Checking files",
        size: "5.9 GiB",
        down: "–",
        up: "–",
        peers: "18",
        eta: "–",
    },
    Torrent {
        name: "archlinux-2026.11.01-x86_64.iso",
        progress: 0.25,
        kind: Kind::Paused,
        status: "Paused",
        size: "1.2 GiB",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
    Torrent {
        name: "Sintel (4K)",
        progress: 0.0,
        kind: Kind::Fetching,
        status: "Fetching info",
        size: "–",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
    Torrent {
        name: "Tears of Steel (2012)",
        progress: 0.06,
        kind: Kind::Error,
        status: "Error",
        size: "3.3 GiB",
        down: "–",
        up: "–",
        peers: "0",
        eta: "–",
    },
];

/// Names for a long list, to see the row steps over many rows.
pub const MANY: [&str; 16] = [
    "debian-13.7.0-amd64-netinst.iso",
    "ubuntu-24.04.3-desktop-amd64.iso",
    "Big Buck Bunny (2008)",
    "archlinux-2026.11.01-x86_64.iso",
    "Sintel (4K)",
    "Tears of Steel (2012)",
    "fedora-workstation-43.iso",
    "Elephants Dream (2006)",
    "linuxmint-22.2-cinnamon.iso",
    "Cosmos Laundromat (2015)",
    "kali-linux-2026.3-installer.iso",
    "Spring (2019)",
    "freebsd-15.0-amd64-dvd1.iso",
    "Agent 327 (2017)",
    "tails-amd64-7.2.img",
    "Charge (2022)",
];

/// How many torrents the list shows.
pub fn count(s: &S) -> usize {
    if s.scrolling {
        MANY.len() * 4
    } else if s.many {
        MANY.len()
    } else {
        TORRENTS.len()
    }
}

/// The torrent at `i`: with a long list, the six repeat under other names.
pub fn torrent(s: &S, i: usize) -> Torrent {
    let t = TORRENTS[i % TORRENTS.len()];
    if s.many || s.scrolling { Torrent { name: MANY[i % MANY.len()], ..t } } else { t }
}

pub const ERRORS: &[(&str, &str)] = &[
    ("Tears of Steel (2012)", "No space left on device (os error 28)"),
    ("Sintel (4K)", "Drive MyDisk isn't connected"),
    ("magnet:?xt=urn:btih:00ff…", "Not a magnet link or .torrent URL"),
];

/// Column widths of the torrent list (Name takes the rest).
pub const COLS: [(&str, f32); 7] =
    [("Progress", 150.0), ("Status", 116.0), ("Size", 70.0), ("Down", 78.0), ("Up", 78.0), ("Peers", 48.0), ("ETA", 44.0)];

// ------------------------------------------------------------------ geometry

/// A tiny deterministic noise, so jagged edges stay put between frames.
pub fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    (x & 0xffff) as f32 / 65535.0
}

/// A parallelogram `w` × `h` whose top edge leans `skew` points right.
pub fn slant(x: f32, y: f32, w: f32, h: f32, skew: f32) -> Vec<[f32; 2]> {
    vec![[x + skew, y], [x + w, y], [x + w - skew, y + h], [x, y + h]]
}

/// A cut-out: a box whose corners and edges are knocked off at random, as if cut with scissors.
/// `amp` is how far an edge wanders; `step` how often it turns.
pub fn jagged(x: f32, y: f32, w: f32, h: f32, amp: f32, step: f32, seed: u32) -> Vec<[f32; 2]> {
    let mut pts = Vec::new();
    let mut k = seed.wrapping_mul(97);
    let mut j = || {
        k = k.wrapping_add(1);
        (noise(k) - 0.5) * 2.0 * amp
    };
    let n = |len: f32| ((len / step).round() as usize).max(1);
    let (nx, ny) = (n(w), n(h));
    for i in 0..nx {
        pts.push([x + w * i as f32 / nx as f32 + j() * 0.5, y + j().abs() * -1.0 + amp * 0.5]);
    }
    for i in 0..ny {
        pts.push([x + w - j().abs() + amp * 0.5, y + h * i as f32 / ny as f32 + j() * 0.5]);
    }
    for i in 0..nx {
        pts.push([x + w - w * i as f32 / nx as f32 + j() * 0.5, y + h + j().abs() - amp * 0.5]);
    }
    for i in 0..ny {
        pts.push([x + j().abs() - amp * 0.5, y + h - h * i as f32 / ny as f32 + j() * 0.5]);
    }
    pts
}

/// A slab cut on the slant: a quad over the `w` × `h` box with no square corner. The top and
/// bottom edges lean by up to `slant` points each way and the sides tilt, as Persona 5 cuts its
/// menu slabs. `seed` picks the cut.
pub fn cut(w: f32, h: f32, slant: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(53).wrapping_add(k));
    // Every corner moves in; the sides lean the same way, so the slab reads as one cut.
    let lean = slant * (0.5 + 0.5 * r(1));
    vec![
        [lean * (0.6 + 0.4 * r(2)), -slant * 0.25 * r(3)],
        [w + slant * 0.2 * r(4), slant * (0.15 + 0.25 * r(5))],
        [w - lean * (0.6 + 0.4 * r(6)), h + slant * 0.2 * r(7)],
        [-slant * 0.2 * r(8), h - slant * (0.1 + 0.25 * r(9))],
    ]
}

/// `pts` moved by (`dx`, `dy`).
pub fn moved(pts: &[[f32; 2]], dx: f32, dy: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|p| [p[0] + dx, p[1] + dy]).collect()
}

/// `pts` turned `a` radians about (`cx`, `cy`).
pub fn turned(pts: &[[f32; 2]], a: f32, cx: f32, cy: f32) -> Vec<[f32; 2]> {
    let (s, c) = a.sin_cos();
    pts.iter().map(|p| [cx + (p[0] - cx) * c - (p[1] - cy) * s, cy + (p[0] - cx) * s + (p[1] - cy) * c]).collect()
}

/// `pts` grown outward by `d` with mitred (sharp) corners: draw it under `pts` for a thick
/// outline that keeps the cut-out's points.
pub fn inflate(pts: &[[f32; 2]], d: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    // Which way is out: the sign of the polygon's area.
    let area: f32 = (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum();
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    let norm = |a: [f32; 2], b: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-4);
        [s * dy / l, -s * dx / l]
    };
    (0..n)
        .map(|i| {
            let (p, c, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (n1, n2) = (norm(p, c), norm(c, q));
            let m = [n1[0] + n2[0], n1[1] + n2[1]];
            let dot = (m[0] * n1[0] + m[1] * n1[1]).max(0.35);
            [c[0] + m[0] * d / dot, c[1] + m[1] * d / dot]
        })
        .collect()
}

/// `pts` with an outline `width` thick around it, in `color`.
pub fn outline(shape: Shape, pts: &[[f32; 2]], width: f32, color: Color) -> Shape {
    shape.fill(inflate(pts, width), color)
}

/// Halftone dots on a grid turned 45°, each of radius `r(x, y)` (0 skips it), in the box.
pub fn halftone(shape: Shape, w: f32, h: f32, pitch: f32, color: Color, r: impl Fn(f32, f32) -> f32) -> Shape {
    halftone_in(shape, w, h, pitch, color, r, &|p| p)
}

/// [`halftone`], each dot's centre passed through `warp`.
pub fn halftone_in(
    mut shape: Shape,
    w: f32,
    h: f32,
    pitch: f32,
    color: Color,
    r: impl Fn(f32, f32) -> f32,
    warp: &dyn Fn([f32; 2]) -> [f32; 2],
) -> Shape {
    let rows = (h / (pitch * 0.5)).ceil() as i32 + 1;
    let cols = (w / pitch).ceil() as i32 + 1;
    for j in 0..rows {
        let y = j as f32 * pitch * 0.5;
        let shift = if j % 2 == 0 { 0.0 } else { pitch * 0.5 };
        for i in 0..cols {
            let x = i as f32 * pitch + shift;
            let rad = r(x, y).min(pitch * 0.72);
            if rad < 0.35 {
                continue;
            }
            let [cx, cy] = warp([x, y]);
            shape = shape.fill(circle(cx, cy, rad), color);
        }
    }
    shape
}

pub fn circle(x: f32, y: f32, r: f32) -> Vec<[f32; 2]> {
    let n = if r < 2.0 {
        6
    } else if r < 5.0 {
        10
    } else {
        18
    };
    (0..n).map(|i| i as f32 * 2.0 * PI / n as f32).map(|a| [x + a.cos() * r, y + a.sin() * r]).collect()
}

/// Screentone: parallel lines at 45°, `pitch` apart, clipped to the box; `phase` (points)
/// slides them along, for moving stripes.
pub fn hatch(shape: Shape, w: f32, h: f32, pitch: f32, width: f32, color: Color, phase: f32) -> Shape {
    hatch_in(shape, w, h, pitch, width, color, phase, &|p| p)
}

/// [`hatch`], each line's ends passed through `warp`.
#[allow(clippy::too_many_arguments)]
pub fn hatch_in(
    mut shape: Shape,
    w: f32,
    h: f32,
    pitch: f32,
    width: f32,
    color: Color,
    phase: f32,
    warp: &dyn Fn([f32; 2]) -> [f32; 2],
) -> Shape {
    let mut x = -h - pitch + phase.rem_euclid(pitch);
    while x < w {
        // The line runs through (x + u, h - u) for u in 0..h; keep the part inside 0..w.
        let (u0, u1) = ((-x).max(0.0), h.min(w - x));
        if u1 > u0 {
            shape = shape.line(warp([x + u0, h - u0]), warp([x + u1, h - u1]), width, LineCap::Butt, color);
        }
        x += pitch;
    }
    shape
}

/// A starburst of `n` points between radii `r0` and `r1`.
pub fn burst(cx: f32, cy: f32, r0: f32, r1: f32, n: usize, turn: f32, seed: u32) -> Vec<[f32; 2]> {
    (0..n * 2)
        .map(|i| {
            let a = turn + i as f32 * PI / n as f32;
            let r = if i % 2 == 0 { r1 * (0.8 + 0.2 * noise(seed + i as u32)) } else { r0 };
            [cx + a.cos() * r, cy + a.sin() * r]
        })
        .collect()
}

/// The shape an active element takes, as Persona 5 menus highlight a choice: a quad around the
/// `w` × `h` box whose corners are thrown out by up to `reach`, with one corner drawn out into
/// a long spike. `seed` picks the throw, so each activation lands differently.
pub fn blade(w: f32, h: f32, reach: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(31).wrapping_add(k));
    // Sheared one way or the other, so no corner comes out square.
    let shear = reach * (0.8 + 0.8 * r(11)) * if r(12) < 0.5 { -1.0 } else { 1.0 };
    let mut pts = vec![
        [-reach * (0.3 + r(1)) + shear, -reach * (0.2 + 0.6 * r(2))],
        [w + reach * (0.2 + 0.8 * r(3)) + shear, -reach * 0.5 * r(4)],
        [w + reach * (0.1 + 0.5 * r(5)) - shear, h + reach * (0.2 + 0.6 * r(6))],
        [-reach * (0.1 + 0.6 * r(7)) - shear, h + reach * 0.4 * r(8)],
    ];
    // The spike: out of one corner, along the box's diagonal, then half again.
    let c = (r(9) * 4.0) as usize % 4;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let p = pts[c];
    let (dx, dy) = (p[0] - cx, p[1] - cy);
    let l = (dx * dx + dy * dy).sqrt().max(1e-3);
    let len = reach * (1.4 + 1.2 * r(10));
    pts[c] = [p[0] + dx / l * len, p[1] + dy / l * len * 0.6];
    pts
}

/// A *living* quad, as the game draws a highlighted choice: a quad around the `w` × `h` box
/// whose corners never rest. Each corner drifts on its own slow wave, so the shape keeps
/// reshaping while it's active. `reach` is how far the corners go out; `seed` gives each layer
/// its own rhythm and lean.
pub fn living(w: f32, h: f32, reach: f32, t: f32, seed: u32) -> Vec<[f32; 2]> {
    let r = |k: u32| noise(seed.wrapping_mul(29).wrapping_add(k));
    let wave = |k: u32| {
        let speed = 2.2 + 2.4 * r(k);
        let phase = r(k + 40) * std::f32::consts::TAU;
        0.5 + 0.5 * (t * speed + phase).sin()
    };
    // Leaning one way, as a cut, and the far corners reaching the furthest.
    let lean = reach * (0.6 + 0.6 * r(90)) * if r(91) < 0.5 { -1.0 } else { 1.0 };
    vec![
        [-reach * (0.2 + 0.8 * wave(1)) + lean, -reach * (0.3 + 0.9 * wave(2))],
        [w + reach * (0.4 + 1.4 * wave(3)) + lean, -reach * (0.0 + 0.5 * wave(4))],
        [w + reach * (0.1 + 0.6 * wave(5)) - lean, h + reach * (0.3 + 0.9 * wave(6))],
        [-reach * (0.3 + 1.2 * wave(7)) - lean, h + reach * (0.0 + 0.5 * wave(8))],
    ]
}

pub fn cyan() -> Color {
    hex("#3fe0f0")
}

/// A tilt for the activation `seed`: between `min` and `max` radians, either way.
pub fn tilt(seed: u32, min: f32, max: f32) -> f32 {
    let a = min + (max - min) * noise(seed.wrapping_mul(7).wrapping_add(3));
    if noise(seed.wrapping_add(17)) < 0.5 { -a } else { a }
}

/// A key's own number, to seed its shapes.
pub fn key_seed(key: &str) -> u32 {
    key.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619))
}

// ------------------------------------------------------------------ the masked ghost

/// Phantom's masked ghost (design/icons, pick `phantom-thief`), in a `size` box: white body,
/// black domino mask with a white rim, white eyes. `shadow` draws an offset copy behind it.
pub fn ghost(size: f32, shadow: Option<Color>) -> El {
    let k = size / 600.0;
    let (ox, oy) = (212.0, 212.0);
    let p = |x: f32, y: f32| {
        let t = turned(&[[x, y]], -9f32.to_radians(), 512.0, 512.0)[0];
        [(t[0] - ox) * k, (t[1] - oy) * k]
    };
    let body = Path::new().move_to(p(292.0, 500.0)).cubic_to(p(292.0, 330.0), p(392.0, 226.0), p(516.0, 226.0)).cubic_to(
        p(648.0, 226.0),
        p(736.0, 330.0),
        p(736.0, 490.0),
    );
    let hem = [
        (742.0, 640.0),
        (812.0, 800.0),
        (700.0, 738.0),
        (664.0, 822.0),
        (594.0, 730.0),
        (532.0, 812.0),
        (480.0, 722.0),
        (408.0, 786.0),
        (384.0, 700.0),
        (318.0, 742.0),
        (300.0, 620.0),
    ];
    let body = hem.iter().fold(body, |b, &(x, y)| b.line_to(p(x, y))).close();
    let pts = |v: &[(f32, f32)]| v.iter().map(|&(x, y)| p(x, y)).collect::<Vec<_>>();
    let mask = pts(&[
        (514.0, 420.0),
        (470.0, 398.0),
        (214.0, 344.0),
        (312.0, 473.0),
        (435.0, 507.0),
        (514.0, 471.0),
        (593.0, 507.0),
        (716.0, 473.0),
        (814.0, 344.0),
        (558.0, 398.0),
    ]);
    let mut s = Shape::new();
    if let Some(c) = shadow {
        let d = 26.0 * k;
        let moved_body = {
            let q = |x: f32, y: f32| {
                let v = p(x, y);
                [v[0] + d, v[1] + d]
            };
            let b = Path::new().move_to(q(292.0, 500.0)).cubic_to(q(292.0, 330.0), q(392.0, 226.0), q(516.0, 226.0)).cubic_to(
                q(648.0, 226.0),
                q(736.0, 330.0),
                q(736.0, 490.0),
            );
            hem.iter().fold(b, |b, &(x, y)| b.line_to(q(x, y))).close()
        };
        s = s.fill_path(&moved_body, c);
    }
    s = s.fill_path(&body, Color::WHITE);
    s = outline(s, &mask, 11.0 * k, Color::WHITE).fill(mask, Color::BLACK);
    s = s.fill(pts(&[(318.0, 418.0), (471.0, 441.0), (422.0, 481.0)]), Color::WHITE);
    s = s.fill(pts(&[(710.0, 418.0), (557.0, 441.0), (606.0, 481.0)]), Color::WHITE);
    canvas(s).size(size, size)
}

// ------------------------------------------------------------------ ransom-note lettering

/// The faces a ransom-note heading cuts its letters from: all ship with macOS.
const FACES: [(&str, f32); 6] = [
    ("Futura", 800.0),
    ("Didot", 700.0),
    ("American Typewriter", 600.0),
    ("Avenir Next Condensed", 900.0),
    ("Georgia", 700.0),
    ("Rockwell", 700.0),
];

/// [`ransom_mix`]'s faces: a few, swapped letter by letter, as the Persona 5 menus are made
/// (Futura, a Times, a Clarendon, a Helvetica, an Avant Garde). All macOS system fonts.
const MIX_FACES: [(&str, f32); 5] =
    [("Futura", 800.0), ("Times New Roman", 700.0), ("Superclarendon", 700.0), ("Helvetica Neue", 800.0), ("Avenir Next", 800.0)];

/// A heading of letters cut from different prints: each letter on its own paper scrap, turned
/// and nudged a little. `palette` is the (paper, ink) pairs the scraps cycle through.
pub fn ransom(word: &str, size: f32, seed: u32, palette: &[(Color, Color)]) -> El {
    ransom_mix(word, size, seed, palette, &[], 1.0)
}

/// Ransom letters as the Persona 5 pause menu sets them: every letter in its own face, size and
/// tilt, but only some (`scraps` of them, 0..1) on a paper scrap; the rest are bare, in one of
/// the `bare` inks. With bare letters, the first letter is set larger, as a capital.
pub fn ransom_mix(word: &str, size: f32, seed: u32, palette: &[(Color, Color)], bare: &[Color], scraps: f32) -> El {
    let mut r = row().gap(1.0).items_center();
    for (i, ch) in word.chars().enumerate() {
        let n = seed.wrapping_add(i as u32 * 13);
        if ch == ' ' {
            r = r.child(el().w(size * 0.35));
            continue;
        }
        let lead = !bare.is_empty() && i == 0;
        let faces: &[(&str, f32)] = if bare.is_empty() { &FACES } else { &MIX_FACES };
        let (face, weight) = faces[(noise(n) * faces.len() as f32) as usize % faces.len()];
        let (bg, fg) = palette[(noise(n + 5) * palette.len() as f32) as usize % palette.len()];
        // Mixed letters are set calmer than a ransom heading, so a label stays readable.
        let (spread, turn, drop) = if bare.is_empty() { (0.3, 0.28, 0.18) } else { (0.14, 0.1, 0.06) };
        let scale = if lead { 1.12 } else { 1.0 - spread / 2.0 + noise(n + 1) * spread };
        let lower = !lead && noise(n + 9) > if bare.is_empty() { 0.7 } else { 0.82 } && ch.is_alphabetic();
        let glyph: String = if lower { ch.to_lowercase().collect() } else { ch.to_uppercase().collect() };
        let fs = size * scale;
        if !bare.is_empty() && noise(n + 7) >= scraps {
            let ink = bare[(noise(n + 11) * bare.len() as f32) as usize % bare.len()];
            r = r.child(
                text(glyph)
                    .font(TextStyle::new(fs, weight).family(Family::Named(face)))
                    .color(ink)
                    .rotate((noise(n + 2) - 0.5) * turn)
                    .offset(0.0, (noise(n + 3) - 0.5) * size * drop),
            );
            continue;
        }
        let (w, h) = (fs * 0.78 + 6.0, fs * 1.18);
        let scrap = jagged(0.0, 0.0, w, h, 1.6, 7.0, n);
        let tile = el()
            .size(w, h)
            .center()
            .child(canvas(Shape::new().fill(moved(&scrap, 1.5, 2.0), Color::BLACK.alpha(0.55)).fill(scrap, bg)).size(w, h).pin(
                Some(0.0),
                Some(0.0),
                None,
                None,
            ))
            .child(text(glyph).font(TextStyle::new(fs, weight).family(Family::Named(face))).color(fg))
            .rotate((noise(n + 2) - 0.5) * turn)
            .offset(0.0, (noise(n + 3) - 0.5) * size * drop);
        r = r.child(tile);
    }
    r
}

/// A box of `w` × `h` with `shape` drawn under its children, the shape in the box's points.
pub fn layer(shape: Shape, w: f32, h: f32) -> El {
    canvas(shape).size(w, h).pin(Some(0.0), Some(0.0), None, None).inert()
}

/// The ease-out-back of a snap: overshoots a touch, then settles, over `dur` seconds.
pub fn snap(t: f32, dur: f32) -> f32 {
    let x = (t / dur).clamp(0.0, 1.0);
    let c = 1.9;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

/// `poly` clipped to the convex polygon `clip` (Sutherland–Hodgman); empty when they don't meet.
pub fn clip_convex(poly: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = clip.len();
    // Which side is inside: the side the clip polygon's centre is on.
    let cx = clip.iter().map(|p| p[0]).sum::<f32>() / n as f32;
    let cy = clip.iter().map(|p| p[1]).sum::<f32>() / n as f32;
    let mut out = poly.to_vec();
    for i in 0..n {
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let side = |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let inner = side([cx, cy]).signum();
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p) * inner, side(q) * inner);
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}
