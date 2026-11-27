//! Themes ([ADR 0004], [ADR 0011]): what a theme is ([`Theme`]), the look it resolves to
//! ([`Look`]), the Active Look Preset with its two resolved looks (`preset`), and the saved Look
//! Presets (`saved`).
//!
//! Given a Theme Look's settings, a theme builds its tokens, the blocks and screens it draws
//! itself, and its App icon ([`Built`]). [`Look::build`] resolves that into a look the views draw
//! with: each block and screen whole from the first theme along theme → parent → Phantom that
//! draws it, so a partial theme still draws everything. A theme declares nothing about the
//! Appearance; which Theme Look each Appearance shows is the Look Preset's.
//!
//! Themes are built in: [`themes`] lists them, and `gui.json` names them by [`Theme::id`].
//!
//! [ADR 0004]: ../../../docs/adr/0004-themes-override-building-blocks.md
//! [ADR 0011]: ../../../docs/adr/0011-look-presets.md

mod colors;
pub mod icon;
pub(crate) mod oklab;
pub mod phantom;
pub mod preset;
pub mod saved;
pub mod setting;
mod spacing;
#[cfg(all(debug_assertions, not(test)))]
pub mod standin;

use std::fmt;
use std::rc::Rc;

use proto::TorrentInfo;
use rune::El;
use rune_kit::Kit;

use crate::model::Phantom;
use crate::ui::{BlockOverrides, Blocks, Ui};
pub use colors::Colors;
use setting::{Resolved, Setting, Values};
pub use spacing::Spacing;

/// A theme: built into the app, with its assets in its own folder.
pub trait Theme: Sync {
    /// Its name in `gui.json`; never changes.
    fn id(&self) -> &'static str;
    /// What Settings calls it.
    fn name(&self) -> &'static str;
    /// The theme whose blocks and screens it falls back to; only Phantom has none.
    fn parent(&self) -> Option<&'static dyn Theme>;
    /// What a person can set, which the generic settings screen shows.
    fn settings(&self) -> &'static [Setting];
    /// Its look with `settings`, which hold a value of its kind for each of
    /// [`settings`](Self::settings). The blocks and screens it draws are closures, which keep
    /// whatever they need of these settings and of what the theme resolves from them.
    fn build(&self, settings: &Values) -> Built;
    /// The App icons it draws from settings of their own (#417); most draw none.
    fn icons(&self) -> &'static [icon::ThemeIcon] {
        &[]
    }
}

/// What a theme builds from a Theme Look's settings.
pub struct Built {
    /// Every color, font, radius and timing its blocks draw with.
    pub tokens: Kit,
    /// The colors its blocks draw that `tokens` has no field for.
    pub colors: Colors,
    /// The room between and around things, and the fixed heights.
    pub spacing: Spacing,
    /// The blocks it draws itself; the rest are its parent's.
    pub blocks: BlockOverrides,
    /// The screens it lays out itself, as a last resort; the rest are its parent's.
    pub screens: ScreenOverrides,
    /// The id of its App icon in the icons' manifest, which "Follow theme" shows.
    pub app_icon: &'static str,
    /// Whether the theme moves, or follows the Mac's Reduce Motion.
    pub motion: Motion,
    /// What its color settings come to, and its warnings about its settings, for Settings.
    pub resolved: Resolved,
}

/// Whether a theme's motion plays: always, never, or as the Mac's Reduce Motion says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    /// Moves unless the Mac reduces motion: Rune's own behaviour.
    #[default]
    Follow,
    /// Moves even when the Mac reduces motion.
    On,
    /// Everything lands at once, whatever the Mac says.
    Off,
}

impl Motion {
    /// The reduced motion state a window draws with, given the Mac's own.
    pub fn reduced(self, system: bool) -> bool {
        match self {
            Motion::Follow => system,
            Motion::On => false,
            Motion::Off => true,
        }
    }
}

/// The details pane of the torrent shown. It takes the app: its files' table reaches its state
/// and handlers through it, and the shape the window leaves it (its header's lines, its width).
pub type DetailsScreen = Rc<dyn Fn(&Phantom, &Ui, &TorrentInfo, crate::views::PaneShape) -> El>;

/// The command palette (#33), while it's open: what it draws goes into the overlays' layer. Its
/// third argument is the window's size, when known.
pub type PaletteScreen = Rc<dyn Fn(&Phantom, &Ui, Option<(f32, f32)>, &mut Vec<El>)>;

/// The screens one theme lays out itself, built like its blocks; `None` is its parent's. A
/// screen override shows the same data, or less, never something else.
#[derive(Clone, Default)]
pub struct ScreenOverrides {
    pub details: Option<DetailsScreen>,
    pub palette: Option<PaletteScreen>,
}

/// Every overridable screen, as a look lays it out.
#[derive(Clone)]
pub struct Screens {
    pub details: DetailsScreen,
    pub palette: PaletteScreen,
}

impl Screens {
    /// The screens as `views` lays them out.
    fn phantom() -> Screens {
        Screens { details: Rc::new(crate::views::details_pane), palette: Rc::new(crate::views::palette::overlay) }
    }

    fn over(self, theme: &ScreenOverrides) -> Screens {
        Screens { details: theme.details.clone().unwrap_or(self.details), palette: theme.palette.clone().unwrap_or(self.palette) }
    }
}

/// A theme with its settings, as one Appearance shows it.
#[derive(Clone)]
pub struct ThemeLook {
    pub theme: &'static dyn Theme,
    /// Values for `theme`'s settings.
    pub settings: Values,
}

impl PartialEq for ThemeLook {
    fn eq(&self, other: &Self) -> bool {
        self.theme.id() == other.theme.id() && self.settings == other.settings
    }
}

impl fmt::Debug for ThemeLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let settings: Vec<_> = self.settings.iter().map(|(s, v)| (s.key, v)).collect();
        f.debug_struct("ThemeLook").field("theme", &self.theme.id()).field("settings", &settings).finish()
    }
}

impl ThemeLook {
    /// What its theme works its settings out to be, for Settings: each color, and the warnings.
    pub fn resolved(&self) -> Resolved {
        self.theme.build(&self.settings).resolved
    }

    /// `theme` at its defaults.
    #[cfg(test)]
    pub fn defaults(theme: &'static dyn Theme) -> ThemeLook {
        ThemeLook { theme, settings: Values::defaults(theme.settings()) }
    }
}

/// A Theme Look resolved: what the views draw with.
pub struct Look {
    /// The id of the theme it's of.
    pub theme: &'static str,
    pub ui: Ui,
    pub screens: Screens,
    /// As [`Built::app_icon`].
    pub app_icon: &'static str,
    /// As [`Built::resolved`].
    pub resolved: Resolved,
}

impl Look {
    /// `look`, built by its theme, its blocks and screens resolved through its parents to
    /// Phantom's. A parent lends its blocks as it draws them at its defaults.
    pub fn build(look: &ThemeLook) -> Look {
        let built = look.theme.build(&look.settings);
        let mut chain = vec![(built.blocks, built.screens)];
        let mut parent = look.theme.parent();
        while let Some(theme) = parent {
            let lent = theme.build(&Values::defaults(theme.settings()));
            chain.push((lent.blocks, lent.screens));
            parent = theme.parent();
        }
        let base = (Blocks::phantom(phantom::icon), Screens::phantom());
        let (blocks, screens) = chain.iter().rev().fold(base, |(blocks, screens), (b, s)| (blocks.over(b), screens.over(s)));
        let ui = Ui::new(built.tokens, built.colors, built.spacing, blocks, built.motion);
        Look { theme: look.theme.id(), ui, screens, app_icon: built.app_icon, resolved: built.resolved }
    }
}

/// Every theme there is.
pub fn themes() -> &'static [&'static dyn Theme] {
    #[cfg(all(not(test), not(debug_assertions)))]
    static THEMES: [&dyn Theme; 1] = [&phantom::PHANTOM];
    // A second theme to see the editor's two lists with (#87), in debug builds only.
    #[cfg(all(not(test), debug_assertions))]
    static THEMES: [&dyn Theme; 2] = [&phantom::PHANTOM, &standin::STAND_IN];
    // A second theme, so tests meet a look that isn't Phantom's.
    #[cfg(test)]
    static THEMES: [&dyn Theme; 2] = [&phantom::PHANTOM, &tests::SAMPLE];
    &THEMES
}

/// The theme `gui.json` names `id`.
pub fn find(id: &str) -> Option<&'static dyn Theme> {
    themes().iter().copied().find(|theme| theme.id() == id)
}

#[cfg(test)]
pub(crate) mod tests;
