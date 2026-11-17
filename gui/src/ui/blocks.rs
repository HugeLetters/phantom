//! The building blocks a theme can draw itself ([ADR 0004]): [`BlockOverrides`] is what one
//! theme draws, [`Blocks`] every block resolved, each whole from the first theme along
//! theme → parent → Phantom that draws it ([`Blocks::over`]). Each icon is a block of its own.
//!
//! A block that's here is drawn through its slot, so an override replaces it for every screen.
//! A block becomes overridable by moving its drawing into a `fn(&Ui, ..) -> El` here; blocks
//! that take handlers (a button, a menu) get theirs when a theme first draws one, with what
//! they're given as data and the handlers attached around what the theme draws.
//!
//! [ADR 0004]: ../../../docs/adr/0004-themes-override-building-blocks.md

use lifecycle::machine::Every;
use rune::El;

use super::{Icon, Tone, Ui};

/// Draws a progress bar: `fraction` from 0.0 to 1.0.
pub type ProgressBar = fn(&Ui, f32) -> El;
/// Draws a dot in the colour of `tone`.
pub type StatusDot = fn(&Ui, Tone) -> El;

/// What one theme draws itself; `None` is its parent's.
#[derive(Clone, Copy, Default)]
pub struct BlockOverrides {
    /// The icons it draws; the rest are its parent's.
    pub icons: Option<fn(Icon) -> Option<&'static rune::Icon>>,
    pub progress_bar: Option<ProgressBar>,
    pub status_dot: Option<StatusDot>,
}

/// Every block, as a look draws it.
#[derive(Clone)]
pub struct Blocks {
    pub icons: Vec<(Icon, &'static rune::Icon)>,
    pub progress_bar: ProgressBar,
    pub status_dot: StatusDot,
}

impl Blocks {
    /// The blocks every chain ends at: Phantom's, complete, its icons drawn by `icon`. Phantom's
    /// blocks are the ones written in this module's siblings; its icons are its theme's assets.
    pub fn phantom(icon: fn(Icon) -> &'static rune::Icon) -> Blocks {
        Blocks {
            icons: Icon::every().into_iter().map(|i| (i, icon(i))).collect(),
            progress_bar: super::progress::phantom,
            status_dot: super::status::phantom_dot,
        }
    }

    /// These blocks with what `theme` draws over them, each block whole.
    pub fn over(mut self, theme: &BlockOverrides) -> Blocks {
        if let Some(icons) = theme.icons {
            for (icon, drawing) in &mut self.icons {
                *drawing = icons(*icon).unwrap_or(*drawing);
            }
        }
        Blocks {
            icons: self.icons,
            progress_bar: theme.progress_bar.unwrap_or(self.progress_bar),
            status_dot: theme.status_dot.unwrap_or(self.status_dot),
        }
    }

    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        self.icons.iter().find(|(i, _)| *i == icon).map(|(_, d)| *d).expect("every icon is drawn")
    }
}
