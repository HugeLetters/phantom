//! The building blocks a theme can draw itself ([ADR 0004]): [`BlockOverrides`] is what one
//! theme draws, [`Blocks`] every block resolved, each whole from the first theme along
//! theme → parent → Phantom that draws it ([`Blocks::over`]). Each icon is a block of its own.
//!
//! A slot is a closure the theme builds from its Theme Look's settings, so a block draws with
//! whatever the theme resolved for that look (a setting, fonts it loaded, a texture it cached),
//! captured, not only with the tokens. A block that's here is drawn through its slot, so an
//! override replaces it for every screen. A block becomes overridable by moving its drawing into
//! a slot here.
//!
//! Persona 5's blocks need more than these slots give, to be decided with the first P5 block:
//! - memory between frames and the frame time: a component's eased amounts (hovered, pressed,
//!   selected) spring toward their targets across frames ([ADR 0010]);
//! - slots for blocks that take handlers (a button, a list row), the handlers attached around
//!   what the theme draws;
//! - a wider progress input than a fraction: P5 draws its bar by the torrent's status and by
//!   selection.
//!
//! [ADR 0004]: ../../../docs/adr/0004-themes-override-building-blocks.md
//! [ADR 0010]: ../../../docs/adr/0010-component-state-as-eased-amounts.md

use std::rc::Rc;

use machine::Every;
use rune::El;

use super::{Icon, Tone, Ui};

/// Draws a progress bar: `fraction` from 0.0 to 1.0.
pub type ProgressBar = Rc<dyn Fn(&Ui, f32) -> El>;
/// Draws a dot in the colour of `tone`.
pub type StatusDot = Rc<dyn Fn(&Ui, Tone) -> El>;
/// The icons a theme draws: `None` for one it leaves to its parent.
pub type Icons = Rc<dyn Fn(Icon) -> Option<&'static rune::Icon>>;

/// What one theme draws itself; `None` is its parent's.
#[derive(Clone, Default)]
pub struct BlockOverrides {
    pub icons: Option<Icons>,
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
            progress_bar: Rc::new(super::progress::phantom),
            status_dot: Rc::new(super::status::phantom_dot),
        }
    }

    /// These blocks with what `theme` draws over them, each block whole.
    pub fn over(mut self, theme: &BlockOverrides) -> Blocks {
        if let Some(icons) = &theme.icons {
            for (icon, drawing) in &mut self.icons {
                *drawing = icons(*icon).unwrap_or(*drawing);
            }
        }
        Blocks {
            icons: self.icons,
            progress_bar: theme.progress_bar.clone().unwrap_or(self.progress_bar),
            status_dot: theme.status_dot.clone().unwrap_or(self.status_dot),
        }
    }

    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        self.icons.iter().find(|(i, _)| *i == icon).map(|(_, d)| *d).expect("every icon is drawn")
    }
}
