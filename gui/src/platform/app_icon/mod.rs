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

/// A theme, as the manifest names it; each has its own icon or pair, which Follow theme shows.
/// Until themes exist ([ADR 0004]) Phantom's is always the default one.
///
/// [ADR 0004]: ../../../../docs/adr/0004-themes-override-building-blocks.md
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Theme {
    #[default]
    #[serde(rename = "phantom")]
    Phantom,
    #[serde(rename = "persona-3-reload")]
    Persona3Reload,
    #[serde(rename = "persona-5")]
    Persona5,
}

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

/// An icon that ships, with its `.icns` already read.
#[derive(Debug, PartialEq, Eq)]
pub struct Icon {
    pub id: String,
    pub bytes: Vec<u8>,
}

/// `app-icons.json` with every icon's file read: the icons that can be shown and each theme's
/// own. Its `pairs` are for the picker.
#[derive(Debug)]
pub struct Manifest {
    icons: Vec<Icon>,
    /// Index into `icons` of the bundle's compiled icon.
    default: usize,
    themes: BTreeMap<Theme, Icons>,
}

/// The file as written; [`Manifest::load`] turns it into a [`Manifest`].
#[derive(Deserialize)]
struct File {
    default: String,
    icons: Vec<FileIcon>,
    #[serde(default)]
    themes: BTreeMap<String, Icons>,
}

#[derive(Deserialize)]
struct FileIcon {
    id: String,
    /// An `.icns`, relative to the manifest.
    file: PathBuf,
}

impl Manifest {
    /// The manifest in `dir` and the icons it lists. An icon whose file can't be read isn't
    /// offered, and a theme naming an unknown icon or theme is left out (both logged); `None`
    /// when the manifest itself or the default icon can't be read.
    pub fn load(dir: &Path) -> Option<Manifest> {
        let json = std::fs::read_to_string(dir.join("app-icons.json"))
            .map_err(|e| eprintln!("phantom: can't read the app icons' manifest: {e}"))
            .ok()?;
        Manifest::parse(&json, |file| std::fs::read(dir.join(file)))
    }

    /// [`load`](Self::load), reading each icon's file with `read`.
    pub fn parse(json: &str, read: impl Fn(&Path) -> std::io::Result<Vec<u8>>) -> Option<Manifest> {
        let file: File = serde_json::from_str(json).map_err(|e| eprintln!("phantom: app-icons.json: {e}")).ok()?;
        let icons: Vec<Icon> = file
            .icons
            .into_iter()
            .filter_map(|icon| match read(&icon.file) {
                Ok(bytes) => Some(Icon { id: icon.id, bytes }),
                Err(e) => {
                    eprintln!("phantom: app icon {} isn't offered: {}: {e}", icon.id, icon.file.display());
                    None
                }
            })
            .collect();
        let default = icons.iter().position(|icon| icon.id == file.default)?;
        let mut manifest = Manifest { icons, default, themes: BTreeMap::new() };
        for (name, icons) in file.themes {
            let theme = serde_json::from_value::<Theme>(serde_json::Value::String(name.clone()));
            match theme {
                Ok(theme) if icons.known(&manifest) => {
                    manifest.themes.insert(theme, icons);
                }
                _ => eprintln!("phantom: app-icons.json: theme {name} is unknown or names an unknown icon"),
            }
        }
        Some(manifest)
    }

    fn icon(&self, id: &str) -> Option<&Icon> {
        self.icons.iter().find(|icon| icon.id == id)
    }

    /// The bundle's compiled icon, which Phantom shows by touching nothing.
    pub fn default(&self) -> &Icon {
        &self.icons[self.default]
    }

    pub fn is_default(&self, icon: &Icon) -> bool {
        std::ptr::eq(icon, self.default())
    }
}

/// The icon to show now: the choice if the manifest has it, else the theme's own, else the
/// default. `dark` is the effective Appearance.
pub fn resolve<'m>(choice: Option<&Icons>, manifest: &'m Manifest, theme: Theme, dark: bool) -> &'m Icon {
    let icons = choice.filter(|icons| icons.known(manifest)).or_else(|| manifest.themes.get(&theme));
    icons.and_then(|icons| manifest.icon(icons.side(dark))).unwrap_or(manifest.default())
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
