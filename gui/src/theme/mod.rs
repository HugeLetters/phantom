//! Themes ([ADR 0004], [ADR 0011]): what a theme is ([`Theme`]), the look it resolves to
//! ([`Look`]), and the Active Look Preset with its two resolved looks (`preset`).
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

pub mod phantom;
pub mod preset;
pub mod setting;

use std::fmt;
use std::rc::Rc;

use proto::TorrentInfo;
use rune::El;
use rune_kit::Kit;

use crate::model::Phantom;
use crate::ui::{BlockOverrides, Blocks, Ui};
use setting::{Setting, Values};

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
}

/// What a theme builds from a Theme Look's settings.
pub struct Built {
    /// Every colour, font, radius and timing its blocks draw with.
    pub tokens: Kit,
    /// The blocks it draws itself; the rest are its parent's.
    pub blocks: BlockOverrides,
    /// The screens it lays out itself, as a last resort; the rest are its parent's.
    pub screens: ScreenOverrides,
    /// The id of its App icon in the icons' manifest, which "Follow theme" shows.
    pub app_icon: &'static str,
}

/// The details pane of the torrent shown. It takes the app: its files' table reaches its state
/// and handlers through it.
pub type DetailsScreen = Rc<dyn Fn(&Phantom, &Ui, &TorrentInfo) -> El>;

/// The screens one theme lays out itself, built like its blocks; `None` is its parent's. A
/// screen override shows the same data, or less, never something else.
#[derive(Clone, Default)]
pub struct ScreenOverrides {
    pub details: Option<DetailsScreen>,
}

/// Every overridable screen, as a look lays it out.
#[derive(Clone)]
pub struct Screens {
    pub details: DetailsScreen,
}

impl Screens {
    /// The screens as `views` lays them out.
    fn phantom() -> Screens {
        Screens { details: Rc::new(crate::views::details_pane) }
    }

    fn over(self, theme: &ScreenOverrides) -> Screens {
        Screens { details: theme.details.clone().unwrap_or(self.details) }
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
    /// `theme` at its defaults.
    #[cfg(test)]
    pub fn defaults(theme: &'static dyn Theme) -> ThemeLook {
        ThemeLook { theme, settings: Values::defaults(theme.settings()) }
    }
}

/// A Theme Look resolved: what the views draw with.
pub struct Look {
    pub ui: Ui,
    pub screens: Screens,
    /// As [`Built::app_icon`].
    pub app_icon: &'static str,
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
        Look { ui: Ui::new(built.tokens, blocks), screens, app_icon: built.app_icon }
    }
}

/// Every theme there is.
pub fn themes() -> &'static [&'static dyn Theme] {
    #[cfg(not(test))]
    static THEMES: [&dyn Theme; 1] = [&phantom::PHANTOM];
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
