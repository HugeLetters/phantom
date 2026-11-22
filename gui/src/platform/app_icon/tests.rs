use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

/// The manifest that ships.
const SHIPPED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../design/icons/app/app-icons.json"));

/// The shipped manifest, with each icon's "bytes" its file name, unless `unreadable` names it.
fn manifest_without(unreadable: &[&str]) -> Option<Manifest> {
    Manifest::parse(SHIPPED, |file| match file.to_str() {
        Some(name) if !unreadable.contains(&name) => Ok(name.as_bytes().to_vec()),
        _ => Err(io::ErrorKind::NotFound.into()),
    })
}

fn manifest() -> Manifest {
    manifest_without(&[]).expect("app-icons.json parses")
}

fn one(id: &str) -> Option<Icons> {
    Some(Icons::One { icon: id.into() })
}

fn pair(dark: &str, light: &str) -> Option<Icons> {
    Some(Icons::Pair { dark: dark.into(), light: light.into() })
}

/// The choice as `gui.json` holds it, read as the app reads it.
fn stored(value: serde_json::Value) -> Option<Icons> {
    serde_json::from_value(value).ok()
}

/// The ids `resolve` shows when dark and when light, following `theme` without a choice.
fn shown(choice: &Option<Icons>, manifest: &Manifest, theme: &Option<Icons>) -> [String; 2] {
    let theme = theme.clone().expect("a theme's icons");
    [true, false].map(|dark| resolve(choice.as_ref(), manifest, &theme, dark).id.clone())
}

/// Phantom's own, as Follow theme shows it.
fn phantoms() -> Option<Icons> {
    one("phantom")
}

fn ids(dark: &str, light: &str) -> [String; 2] {
    [dark.into(), light.into()]
}

/// Each theme's own App icon ships: `crate::theme`'s tests.
#[test]
fn the_shipped_manifest_reads_every_icon() {
    let m = Manifest::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/icons/app")).expect("loads");
    assert_eq!(m.icons.len(), 14, "every .icns read");
    assert!(m.icons.iter().all(|icon| !icon.bytes.is_empty()));
    assert_eq!(m.default().id, "phantom");
}

#[test]
fn an_unreadable_icon_isnt_offered() {
    let m = manifest_without(&["dark-hour.icns"]).expect("still loads");
    assert!(m.icon("dark-hour").is_none());
    let p5 = pair("calling-card", "black-mask");
    assert_eq!(shown(&one("dark-hour"), &m, &p5), ids("calling-card", "black-mask"), "the choice falls back to the theme");
    assert_eq!(shown(&None, &m, &pair("dark-hour", "full-moon")), ids("phantom", "phantom"), "the theme's pair would show half");
    assert!(manifest_without(&["phantom.icns"]).is_none(), "the default must be there");
}

#[test]
fn follow_theme_shows_the_looks_own_and_phantoms_is_the_default() {
    let m = manifest();
    assert_eq!(shown(&None, &m, &phantoms()), ids("phantom", "phantom"));
    assert!(m.is_default(resolve(None, &m, &phantoms().unwrap(), true)), "touches nothing");
    assert!(
        m.is_default(resolve(one("phantom").as_ref(), &m, &pair("calling-card", "black-mask").unwrap(), true)),
        "the default chosen on its own too"
    );
    assert_eq!(shown(&None, &m, &pair("dark-hour", "full-moon")), ids("dark-hour", "full-moon"), "each Appearance's look's");
}

#[test]
fn one_icon_shows_in_both_appearances() {
    assert_eq!(shown(&one("calling-card"), &manifest(), &phantoms()), ids("calling-card", "calling-card"));
}

#[test]
fn a_pair_swaps_with_the_appearance() {
    let m = manifest();
    assert_eq!(shown(&pair("sea-of-souls-dark-hour", "sea-of-souls"), &m, &phantoms()), ids("sea-of-souls-dark-hour", "sea-of-souls"));
    assert_eq!(shown(&pair("dark-hour", "phantom"), &m, &phantoms()), ids("dark-hour", "phantom"), "a custom pair with the default");
}

/// A half naming an icon there isn't (an image moved or removed) follows the theme, never the
/// other half's icon; so does a half chosen to follow it (#87).
#[test]
fn an_unknown_icon_means_that_half_follows_the_theme() {
    let m = manifest();
    let theme = || pair("calling-card", "black-mask");
    assert_eq!(shown(&one("gone"), &m, &theme()), ids("calling-card", "black-mask"));
    assert_eq!(shown(&pair("dark-hour", "gone"), &m, &theme()), ids("dark-hour", "black-mask"), "the Light half follows");
    assert_eq!(shown(&pair(FOLLOW, "full-moon"), &m, &theme()), ids("calling-card", "full-moon"), "the Dark half follows");
}

#[test]
fn a_stored_choice_reads_back_and_anything_else_is_follow_theme() {
    assert_eq!(stored(json!({ "icon": "dark-hour" })), one("dark-hour"));
    assert_eq!(stored(json!({ "dark": "calling-card", "light": "black-mask" })), pair("calling-card", "black-mask"));
    for choice in [one("dark-hour"), pair("calling-card", "black-mask")] {
        assert_eq!(stored(serde_json::to_value(&choice).unwrap()), choice);
    }
    for odd in [
        json!(null),
        json!(3),
        json!("theme"),
        json!("dark-hour"),
        json!({}),
        json!({ "dark": "dark-hour" }),
        json!({ "icon": 1 }),
        json!({ "icon": "a", "dark": "b", "light": "c" }),
    ] {
        assert_eq!(stored(odd.clone()), None, "{odd}");
    }
}

#[test]
fn the_bundle_is_written_once_and_reset_only_when_phantom_wrote_it() {
    let m = manifest();
    let icon = m.icon("dark-hour");
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
