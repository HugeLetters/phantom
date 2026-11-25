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
mod icon;
mod menu;
mod notice;
mod palette;
mod popover;
mod progress;
mod sheet;
mod status;
mod table;
mod tabs;
mod text;
mod text_field;

use rune::{El, col, el, row};
use rune_kit::Kit;

pub use blocks::{BlockOverrides, Blocks};
#[cfg(test)]
pub use blocks::{ProgressBar, StatusDot};
pub use icon::Icon;
pub use menu::Entry;
pub use notice::NOTICES_REST;
pub use palette::PaletteRow;
#[cfg(test)]
pub(crate) use sheet::NAME_LINE;
pub use sheet::{Halves, IMAGES, LookChip, TRAFFIC_LIGHTS};
pub use status::Tone;
pub use table::{Cell, Table, TableColumn};
pub use tabs::BandSide;
pub use text_field::TEXT_INSET;

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

    /// The look's tokens, as a block of another look's reads them (a Look Preset's editor draws
    /// each half's column on its own card colour).
    pub fn tokens(&self) -> &Kit {
        &self.k
    }

    /// Phantom with the Tone that matches `dark`, as a fresh install shows it: for tests that
    /// draw a block alone.
    #[cfg(test)]
    pub fn phantom(dark: bool) -> Self {
        crate::theme::phantom::look(dark).ui
    }

    /// The window's background, holding everything else, edge to edge: the content runs under a
    /// transparent titlebar, whose band the screen fills itself (the toolbar, #33).
    pub fn page(&self) -> El {
        col().w_full().h_full().fill(self.k.c.page)
    }

    /// The divider between two parts of a page that a drag moves (the list and the details pane):
    /// a grip in its middle.
    pub fn divider(&self, height: f32) -> El {
        row().w_full().h(height).center().cursor(rune::Cursor::ResizeRow).child(el().w(36.0).h(4.0).radius(2.0).fill(self.k.c.border_weak))
    }

    /// A raised, rounded area for the main content.
    pub fn panel(&self) -> El {
        col().radius(self.k.radius.md).fill(self.k.c.bg_base).clip_rounded(self.k.radius.md)
    }

    /// The window's root, around the page: where Rune routes window-level pastes and drops,
    /// and where `overlays` (see [`Ui::note`]) and tooltips draw.
    pub fn root(&self, page: El, overlays: Vec<El>) -> El {
        let nowhere = el()
            .key(NOWHERE)
            .absolute(0.0, 0.0)
            .size(1.0, 1.0)
            .focusable(true)
            .tab_index(-1)
            .focus_ring(0.0, 0.0, rune::Color::TRANSPARENT)
            .a11y_hidden()
            .on_focus_change_any(|focused, tx| {
                if focused {
                    tx.blur();
                }
            });
        rune_kit::root(page, std::iter::once(nowhere).chain(overlays).collect::<Vec<_>>())
    }

    /// A marker for an element whose leaving takes the focus in it nowhere: drawn beside it, going
    /// when it goes. Rune would hand the focus to a neighbour, and the next key (⌘⌫, Return) would
    /// act there: on the files, when a notice was dismissed.
    pub fn focus_goes_nowhere(&self) -> El {
        el().key("focus-goes-nowhere").absolute(0.0, 0.0).w(0.0).h(0.0).focus_return(NOWHERE)
    }
}

/// The element that gives up the focus it's handed ([`Ui::focus_goes_nowhere`]).
const NOWHERE: &str = "nowhere";

/// A kit button with its focus ring 2.5 points out, as Phantom's is (the kit's is 2).
fn ringed(button: El, k: &Kit) -> El {
    button.focus_ring(2.0, 2.5, k.c.border_focus)
}

/// `color` as it looks over the opaque `behind`, in sRGB as Rune blends. For an edge: Rune draws a
/// translucent border over its own element's fill, so one that's to take in what's around it is
/// made opaque first.
fn opaque_over(color: rune::Color, behind: rune::Color) -> rune::Color {
    let encode = |c: f32| if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    let decode = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let mix = |top: f32, under: f32| decode(encode(top) * color.a + encode(under) * (1.0 - color.a));
    rune::Color { r: mix(color.r, behind.r), g: mix(color.g, behind.g), b: mix(color.b, behind.b), a: 1.0 }
}

/// Gives a kit component a key, for tests, scripts and anchoring menus.
fn keyed(key: impl Into<String>, e: El) -> El {
    el().key(key).child(e)
}
