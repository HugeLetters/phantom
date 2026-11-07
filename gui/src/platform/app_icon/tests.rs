use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

/// The manifest that ships.
const SHIPPED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../design/icons/app/app-icons.json"));

fn manifest() -> Manifest {
    Manifest::parse(SHIPPED).expect("app-icons.json parses")
}

fn one(id: &str) -> AppIcon {
    AppIcon::One(id.into())
}

fn pair(dark: &str, light: &str) -> AppIcon {
    AppIcon::Pair { dark: dark.into(), light: light.into() }
}

/// The id `resolve` shows in each Appearance, `None` for the bundle's own.
fn shown(choice: &AppIcon, manifest: &Manifest, theme: &str) -> [Option<String>; 2] {
    [true, false].map(|dark| resolve(choice, manifest, theme, dark).map(|icon| icon.id.clone()))
}

fn ids(dark: Option<&str>, light: Option<&str>) -> [Option<String>; 2] {
    [dark.map(Into::into), light.map(Into::into)]
}

#[test]
fn the_shipped_manifest_has_every_icon_and_its_default() {
    let m = manifest();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/icons/app");
    assert_eq!(m.default, "phantom");
    assert!(m.icons.iter().any(|icon| icon.id == m.default));
    for icon in &m.icons {
        assert!(dir.join(&icon.file).is_file(), "{} is missing", icon.file.display());
    }
}

#[test]
fn follow_theme_is_the_default_icon_until_themes_exist() {
    let m = manifest();
    assert_eq!(shown(&AppIcon::FollowTheme, &m, THEME), ids(None, None));
    assert_eq!(shown(&one("phantom"), &m, THEME), ids(None, None), "the default chosen on its own touches nothing too");
}

#[test]
fn a_theme_with_a_pair_swaps_with_the_appearance() {
    let m = manifest();
    assert_eq!(shown(&AppIcon::FollowTheme, &m, "persona-3-reload"), ids(Some("dark-hour"), Some("full-moon")));
    assert_eq!(shown(&AppIcon::FollowTheme, &m, "no-such-theme"), ids(None, None));
}

#[test]
fn one_icon_shows_in_both_appearances() {
    assert_eq!(shown(&one("calling-card"), &manifest(), THEME), ids(Some("calling-card"), Some("calling-card")));
}

#[test]
fn a_pair_swaps_and_its_default_side_is_the_bundles_own() {
    let m = manifest();
    assert_eq!(shown(&pair("sea-of-souls-dark-hour", "sea-of-souls"), &m, THEME), ids(Some("sea-of-souls-dark-hour"), Some("sea-of-souls")));
    assert_eq!(shown(&pair("dark-hour", "phantom"), &m, THEME), ids(Some("dark-hour"), None), "a custom pair with the default");
}

#[test]
fn an_unknown_icon_means_follow_theme() {
    let m = manifest();
    assert_eq!(shown(&one("gone"), &m, "persona-5"), ids(Some("calling-card"), Some("black-mask")));
    assert_eq!(shown(&pair("dark-hour", "gone"), &m, THEME), ids(None, None), "a pair never shows half");
}

#[test]
fn a_stored_choice_reads_back_and_anything_else_is_follow_theme() {
    for choice in [AppIcon::FollowTheme, one("dark-hour"), pair("calling-card", "black-mask")] {
        assert_eq!(AppIcon::decode(&choice.encode()), choice);
    }
    for odd in [json!(null), json!(3), json!("dark-hour"), json!({}), json!({ "dark": "dark-hour" }), json!({ "icon": 1 }), json!({ "icon": "a", "dark": "b", "light": "c" })] {
        assert_eq!(AppIcon::decode(&odd), AppIcon::FollowTheme, "{odd}");
    }
}

#[test]
fn the_environment_names_a_choice() {
    assert_eq!(AppIcon::parse("theme"), AppIcon::FollowTheme);
    assert_eq!(AppIcon::parse(" dark-hour "), one("dark-hour"));
    assert_eq!(AppIcon::parse("dark-hour/full-moon"), pair("dark-hour", "full-moon"));
}

#[test]
fn the_bundle_is_written_once_and_reset_only_when_phantom_wrote_it() {
    let m = manifest();
    let icon = m.icons.iter().find(|i| i.id == "dark-hour");
    let ours = Written { id: "dark-hour".into(), fingerprint: 7 };
    assert_eq!(bundle_step(icon, None, None), BundleStep::Set(icon.unwrap()), "a fresh install");
    assert_eq!(bundle_step(icon, Some(7), Some(&ours)), BundleStep::Leave, "still in place");
    assert_eq!(bundle_step(icon, Some(8), Some(&ours)), BundleStep::Set(icon.unwrap()), "replaced since");
    let other = Written { id: "full-moon".into(), fingerprint: 7 };
    assert_eq!(bundle_step(icon, Some(7), Some(&other)), BundleStep::Set(icon.unwrap()), "the other side of a pair");
    assert_eq!(bundle_step(None, Some(7), Some(&ours)), BundleStep::Reset);
    assert_eq!(bundle_step(None, Some(8), Some(&ours)), BundleStep::Leave, "the user's own Finder icon");
    assert_eq!(bundle_step(None, Some(8), None), BundleStep::Leave);
    assert_eq!(bundle_step(None, None, Some(&ours)), BundleStep::Leave, "the install replaced the bundle");
}

#[test]
fn fingerprints_are_stable() {
    assert_eq!(fingerprint(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fingerprint(b"a"), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn only_an_executable_in_a_bundle_has_one() {
    assert_eq!(bundle_of(Path::new("/A/Phantom.app/Contents/MacOS/Phantom")), Some(PathBuf::from("/A/Phantom.app")));
    assert_eq!(bundle_of(Path::new("/repo/target/debug/phantom-gui")), None);
    assert_eq!(bundle_of(Path::new("/A/Phantom/Contents/MacOS/Phantom")), None);
}
