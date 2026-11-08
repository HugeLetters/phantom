//! The App icon: which icon the user chose, which icon that means right now, and whether the
//! bundle's own icon needs changing. Everything here is pure; [`appkit`] shows the result in the
//! Dock and on the bundle (Finder, Spotlight, Launchpad, launchers).
//!
//! The icons and their manifest come from `design/icons/app/` (see its README, "App icons"), which
//! `scripts/install-mac.sh` copies into the bundle's Resources. The manifest's `default` (Phantom)
//! is also the bundle's compiled icon, so choosing it touches nothing.
//!
//! The choice is stored in the GUI's `gui.json` in the data dir under `app_icon`, as
//! `{"icon": id}` or `{"dark": id, "light": id}` (a designed pair is stored just like a custom
//! one); without it the icon follows the theme. Until the picker (#42), that's also how to choose
//! one by hand. Anything unreadable or naming an unknown icon means Follow theme.

#[cfg(target_os = "macos")]
pub mod appkit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The theme whose icon Follow theme shows, until themes exist ([ADR 0004]).
///
/// [ADR 0004]: ../../../../docs/adr/0004-themes-override-building-blocks.md
pub const THEME: &str = "plain";

/// The key of the choice in `gui.json`.
pub const SETTING: &str = "app_icon";

/// One icon, or a dark and a light one swapped with the Appearance: what the user chose (the
/// choice is `Option<Icons>`, `None` following the theme) and what a theme names as its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Icons {
    One { icon: String },
    Pair { dark: String, light: String },
}

impl Icons {
    fn side(&self, dark: bool) -> &str {
        match self {
            Icons::One { icon } => icon,
            Icons::Pair { dark: d, light: l } => if dark { d } else { l },
        }
    }

    /// Whether the manifest has every icon this names, so a pair never shows half.
    fn known(&self, manifest: &Manifest) -> bool {
        [true, false].into_iter().all(|dark| manifest.icon(self.side(dark)).is_some())
    }
}

/// `app-icons.json`: the icons that ship and each theme's own. Its `pairs` are for the picker.
#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// The bundle's compiled icon.
    pub default: String,
    pub icons: Vec<Icon>,
    #[serde(default)]
    pub themes: BTreeMap<String, Icons>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Icon {
    pub id: String,
    /// An `.icns`, relative to the manifest.
    pub file: PathBuf,
}

impl Manifest {
    pub fn parse(json: &str) -> Option<Manifest> {
        serde_json::from_str(json).ok()
    }

    fn icon(&self, id: &str) -> Option<&Icon> {
        self.icons.iter().find(|icon| icon.id == id)
    }
}

/// The icon to show now, or `None` for the bundle's own (the manifest's default), which Phantom
/// leaves alone. `dark` is the effective Appearance.
pub fn resolve<'m>(choice: Option<&Icons>, manifest: &'m Manifest, theme: &str, dark: bool) -> Option<&'m Icon> {
    let icons = choice.filter(|icons| icons.known(manifest)).or_else(|| manifest.themes.get(theme));
    let id = icons.map_or(manifest.default.as_str(), |icons| icons.side(dark));
    manifest.icon(id).filter(|icon| icon.id != manifest.default)
}

/// The bundle icon Phantom last wrote, kept in `gui.json` so a reset removes only Phantom's own:
/// a user's Finder icon has a different fingerprint and stays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub id: String,
    /// [`fingerprint`] of the custom icon's data as written.
    pub fingerprint: u64,
}

/// The key of [`Written`] in `gui.json`.
pub const WRITTEN: &str = "app_icon_written";

#[derive(Debug, PartialEq, Eq)]
pub enum BundleStep<'m> {
    Set(&'m Icon),
    /// Remove the custom icon: Phantom wrote it and the choice is now the default.
    Reset,
    Leave,
}

/// What to do to the bundle's custom icon. `current` is the [`fingerprint`] of the one it has,
/// `written` the last one Phantom wrote. An icon still in place isn't written again.
pub fn bundle_step<'m>(target: Option<&'m Icon>, current: Option<u64>, written: Option<&Written>) -> BundleStep<'m> {
    let ours = current.is_some() && current == written.map(|w| w.fingerprint);
    match target {
        Some(icon) if ours && written.is_some_and(|w| w.id == icon.id) => BundleStep::Leave,
        Some(icon) => BundleStep::Set(icon),
        None if ours => BundleStep::Reset,
        None => BundleStep::Leave,
    }
}

/// FNV-1a: stable across builds, which `std`'s hasher isn't promised to be.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The `.app` an executable at `Contents/MacOS/<name>` is in.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let contents = exe.parent().filter(|d| d.ends_with("MacOS"))?.parent().filter(|d| d.ends_with("Contents"))?;
    contents.parent().filter(|app| app.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Where a bundle's custom icon lives: the resource fork of the hidden `Icon\r` in its root.
pub fn custom_icon_data(bundle: &Path) -> PathBuf {
    bundle.join("Icon\r/..namedfork/rsrc")
}

#[cfg(test)]
mod tests;
