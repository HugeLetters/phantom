//! Phantom's building blocks: every screen draws with these, never with rune-kit or raw styling,
//! so a theme can override them one at a time ([ADR 0004]). Every colour, font, radius and
//! timing a block uses is a token of the look (a rune-kit [`Kit`]); the blocks a theme can draw
//! itself are in [`blocks`].
//!
//! [`Ui`] carries one resolved look (`crate::theme`). Blocks return Rune elements, which screens
//! lay out (grow, gaps, keys) and give behaviour (handlers, help).
//!
//! [ADR 0004]: ../../../docs/adr/0004-themes-override-building-blocks.md

pub mod blocks;
mod button;
mod dialog;
mod empty;
mod failures;
mod icon;
mod menu;
mod popover;
mod progress;
mod sheet;
mod status;
mod table;
mod text;
mod text_field;

use rune::{El, col, el};
use rune_kit::Kit;

pub use blocks::{BlockOverrides, Blocks};
#[cfg(test)]
pub use blocks::{ProgressBar, StatusDot};
pub use failures::Failure;
pub use icon::Icon;
pub use menu::Entry;
pub use status::Tone;
pub use table::{Table, TableColumn};

#[derive(Clone)]
pub struct Ui {
    /// The look's tokens: palette, type, radii, elevation, motion, glyphs.
    k: Kit,
    blocks: Blocks,
}

impl Ui {
    /// The look of `tokens` and `blocks`, as a theme resolved them.
    pub fn new(tokens: Kit, blocks: Blocks) -> Self {
        Self { k: tokens, blocks }
    }

    #[cfg(test)]
    pub fn blocks(&self) -> &Blocks {
        &self.blocks
    }

    #[cfg(test)]
    pub fn tokens(&self) -> &Kit {
        &self.k
    }

    /// Phantom with the Tone that matches `dark`, as a fresh install shows it: for tests that
    /// draw a block alone.
    #[cfg(test)]
    pub fn phantom(dark: bool) -> Self {
        crate::theme::phantom::look(dark).ui
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
fn keyed(key: impl Into<String>, e: El) -> El {
    el().key(key).child(e)
}
