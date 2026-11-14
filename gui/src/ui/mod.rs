//! Phantom's building blocks: every screen draws with these, never with rune-kit or raw styling,
//! so a theme can later override them one at a time ([ADR 0004]). Today there is one look,
//! rune-kit's neutral kit; the `Theme` interface comes with the first other theme.
//!
//! [`Ui`] carries the look for one frame. Blocks return Rune elements, which screens lay out
//! (grow, gaps, keys) and give behaviour (handlers, help).
//!
//! [ADR 0004]: ../../../docs/adr/0004-themes-override-building-blocks.md

mod button;
mod empty;
mod errors;
mod icon;
mod menu;
mod popover;
mod progress;
mod sheet;
mod status;
mod table;
mod text;
mod text_field;
mod theme;

use rune::{El, col, el};
use rune_kit::Kit;

pub use errors::Failure;
#[cfg(test)]
pub use errors::{BOTTOM_ROOM, LEAVING, MAX_HEIGHT};
pub use icon::Icon;
pub use menu::Entry;
pub use status::Tone;
pub use table::{Cell, Table, TableColumn};
pub use theme::Theme;

#[derive(Clone)]
pub struct Ui {
    k: Kit,
    theme: Theme,
}

impl Ui {
    pub fn new(dark: bool) -> Self {
        Self { k: Kit::neutral(dark), theme: Theme::PHANTOM }
    }

    /// The same look with another theme's drawings.
    #[cfg(test)]
    pub fn themed(self, theme: Theme) -> Self {
        Self { theme, ..self }
    }

    /// The window's background, holding everything else. `titlebar_inset` is Rune's: the content
    /// runs under a transparent titlebar, so clear the traffic lights (the inset is 0 in full
    /// screen, where there are none).
    pub fn page(&self, titlebar_inset: f32) -> El {
        col().w_full().h_full().fill(self.k.c.page).padding(12.0).pt(self.page_top(titlebar_inset))
    }

    /// How far down the page's content starts.
    pub fn page_top(&self, titlebar_inset: f32) -> f32 {
        if titlebar_inset > 0.0 { 40.0 } else { 12.0 }
    }

    /// A raised, rounded area for the main content.
    pub fn panel(&self) -> El {
        col().radius(self.k.radius.md).fill(self.k.c.bg_base).clip_rounded(self.k.radius.md)
    }

    /// The window's root, around the page: where Rune routes window-level pastes and drops,
    /// and where `overlays` (see [`Ui::note`]) and tooltips draw.
    pub fn root(&self, page: El, overlays: Vec<El>) -> El {
        rune_kit::root(page, overlays)
    }
}

/// Gives a kit component a key, for tests, scripts and anchoring menus.
fn keyed(key: &'static str, e: El) -> El {
    el().key(key).child(e)
}
