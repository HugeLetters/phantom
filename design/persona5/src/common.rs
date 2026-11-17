//! What both themes share: the theme switch and its colours, the fonts, the sketch's state, and
//! the demo torrents.

use rune::{Color, El, Family, Phase, TextStyle};

use crate::motion::*;

pub fn hex(s: &str) -> Color {
    rune::color::hex(s).expect("a hex colour")
}

/// The two themes the sketch draws. Global, so every colour function follows it; a scene sets it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Persona 5's Phantom Thieves: red, black and white paper.
    PhantomThieves,
    /// Persona 5's Jose's shop: a navy night sky, lemon and cream.
    JoseShop,
}

static JOSE_SHOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_theme(t: Theme) {
    JOSE_SHOP.store(t == Theme::JoseShop, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the theme is Jose's shop.
pub fn jose_shop() -> bool {
    JOSE_SHOP.load(std::sync::atomic::Ordering::Relaxed)
}

/// The theme's main colour: red, or Jose's blue.
pub fn red() -> Color {
    if jose_shop() { hex("#1f4fe0") } else { hex("#e60012") }
}
/// The main colour's dark shade: deep red, or navy.
pub fn deep() -> Color {
    if jose_shop() { hex("#0c2466") } else { hex("#a8000c") }
}
/// The selection band's colour: the main colour, or Jose's yellow flag.
pub fn select() -> Color {
    if jose_shop() { jose_shop_yellow() } else { red() }
}
/// Text on the selection band.
pub fn on_select() -> Color {
    if jose_shop() { black() } else { paper() }
}
pub fn black() -> Color {
    hex("#0b0b0b")
}
pub fn paper() -> Color {
    if jose_shop() { hex("#fff3c4") } else { hex("#f4f2ec") }
}

/// Jose's yellow, after the shop's lemon (measured near `#fcf664`).
pub fn jose_shop_yellow() -> Color {
    hex("#fbf25a")
}
/// A pale tint of the lemon, for ghosts, sheens and comets.
pub fn jose_shop_pale() -> Color {
    hex("#fbf7a6")
}
/// The yellow's glow: a little deeper than the body.
pub fn jose_shop_yellow_glow() -> Color {
    hex("#efe02a")
}

/// Jose's rounded face.
pub const JOSE_SHOP_FAMILY: &str = ".SF Compact Rounded";

/// The colour an error is drawn in: the main colour, or on Jose's navy the bars' red, so an
/// error's status, number and bar all match.
pub fn error_c() -> Color {
    if jose_shop() { hex("#ff5a6a") } else { red() }
}
/// The download and upload slabs: cyan and pink, or Jose's teal and cream.
pub fn speed_colours() -> (Color, Color) {
    if jose_shop() { (hex("#7fe7ff"), hex("#fff3c4")) } else { (hex("#4fe3e3"), hex("#ff5ec4")) }
}

pub fn cyan() -> Color {
    hex("#3fe0f0")
}

// ------------------------------------------------------------------ fonts (macOS system fonts)

/// Condensed caps for labels: Avenir Next Condensed ships with macOS, so a theme can name it
/// without bundling it.
pub fn caps(size: f32, weight: f32) -> TextStyle {
    if jose_shop() {
        return TextStyle::new(size * 0.95, weight.min(700.0)).family(Family::Named(JOSE_SHOP_FAMILY)).tracking(0.3);
    }
    TextStyle::new(size, weight).family(Family::Named("Avenir Next Condensed")).tracking(0.4)
}
pub fn sans(size: f32, weight: f32) -> TextStyle {
    if jose_shop() {
        return TextStyle::new(size * 0.95, weight.min(600.0)).family(Family::Named(JOSE_SHOP_FAMILY));
    }
    TextStyle::new(size, weight).family(Family::Named("Avenir Next"))
}
pub fn figures(size: f32) -> TextStyle {
    if jose_shop() {
        return TextStyle::new(size * 0.95, 500.0).family(Family::Named(JOSE_SHOP_FAMILY)).features("'tnum' 1");
    }
    TextStyle::new(size, 600.0).family(Family::Named("Avenir Next Condensed")).features("'tnum' 1")
}

// ------------------------------------------------------------------ state

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Idle,
    Hover,
    Press,
    Disabled,
}

#[derive(Clone)]
pub struct S {
    /// The component sheet instead of the window.
    pub sheet: bool,
    pub hovered: Option<String>,
    pub pressed: Option<String>,
    pub selected: Option<usize>,
    /// Seconds since the sketch started, for the time-driven motion.
    pub t: f32,
    /// When the selection last changed, for the selection's snap-in.
    pub selected_at: f32,
    /// How many times anything was hovered, pressed or selected: each activation seeds its own
    /// random tilt and shape, so a button never lands the same way twice.
    pub activations: u32,
    /// A very long list (64 torrents, the 16 four times over) in a scrolling viewport, to see
    /// the list scroll.
    pub scrolling: bool,
    /// A torrent added (1: the last row slides in) or removed (2: the third row slides out and
    /// the rows below close the gap) at `event_at`; 0 for a list that is simply there.
    pub list_event: u8,
    pub event_at: f32,
    /// Every row's progress changes over time (see `demo_progress`).
    pub progress_demo: Option<ProgressDemo>,
    /// The first torrent pauses by itself from 4 to 6.5 s, as a client might: Pause turns
    /// disabled and Resume enabled while the pointer may be resting on either.
    pub status_flip: bool,
    /// The `play` scene: the primary button reads Switch and swaps the two themes.
    pub play: bool,
}

impl S {
    pub fn new() -> S {
        S {
            sheet: false,
            hovered: None,
            pressed: None,
            selected: Some(0),
            t: 0.0,
            selected_at: -10.0,
            activations: 0,
            scrolling: false,
            list_event: 0,
            event_at: 3.0,
            progress_demo: None,
            status_flip: false,
            play: false,
        }
    }

    /// Row `i`'s life in the list: how far it has slid in (0 out, 1 in place; the Phantom
    /// Thieves' slam overshoots) and how much of its height it takes (the rows below make room or close up).
    pub fn row_life(&self, i: usize) -> (f32, f32) {
        let u = self.t - self.event_at;
        let ease = |x: f32| 1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3);
        match self.list_event {
            1 if i + 1 == count(self) => {
                if u < 0.0 {
                    (0.0, 0.0)
                } else {
                    (snap(u, 0.38), ease(u / 0.25))
                }
            }
            2 if i == 2 => {
                let out = (u / 0.3).clamp(0.0, 1.0);
                if u < 0.0 { (1.0, 1.0) } else { (1.0 - out.powi(3), 1.0 - ease((u - 0.3) / 0.25)) }
            }
            _ => (1.0, 1.0),
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
                s.activations += 1;
            } else if s.hovered.as_deref() == Some(&k1) {
                s.hovered = None;
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

/// Whether row `i` is selected: the primary selection or one of the others.
pub fn row_selected(s: &S, i: usize) -> bool {
    s.selected == Some(i)
}

/// How many torrents the list shows.
pub fn count(s: &S) -> usize {
    if s.scrolling { MANY.len() * 4 } else { TORRENTS.len() }
}

/// The torrent at `i`: with a long list, the six repeat under other names.
pub fn torrent(s: &S, i: usize) -> Torrent {
    let t = TORRENTS[i % TORRENTS.len()];
    let t = if s.scrolling { Torrent { name: MANY[i % MANY.len()], ..t } } else { t };
    let t = if let Some(demo) = s.progress_demo { Torrent { progress: demo_progress(demo, s.t), ..t } } else { t };
    if s.status_flip && i == 0 && (4.0..6.5).contains(&s.t) {
        let paused = TORRENTS[3];
        Torrent { kind: paused.kind, status: paused.status, down: paused.down, eta: paused.eta, ..t }
    } else {
        t
    }
}

pub const ERRORS: &[(&str, &str)] = &[
    ("Tears of Steel (2012)", "No space left on device (os error 28)"),
    ("Sintel (4K)", "Drive MyDisk isn't connected"),
    ("magnet:?xt=urn:btih:00ff…", "Not a magnet link or .torrent URL"),
];

/// Column widths of the torrent list (Name takes the rest).
pub const COLS: [(&str, f32); 7] =
    [("Progress", 150.0), ("Status", 116.0), ("Size", 70.0), ("Down", 78.0), ("Up", 78.0), ("Peers", 48.0), ("ETA", 44.0)];
