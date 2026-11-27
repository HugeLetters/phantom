//! The App icon's Customize sheet (#417): Phantom's icon with a Tile color per half.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rune::{Key, KeyEvent, Mods};

use super::super::{Desk, focus_path, wait};
use super::{appearance_tab, file_id, manifest, scroll_to};
use crate::model::settings::{AppIcon, IconChange};
use crate::platform::app_icon::Icons;
use crate::theme::icon::{IconSettings, Tint};
use crate::theme::preset::{Appearance, LookPreset, Looks};
use crate::theme::setting::Rgb;

const TILE: &str = "tile";

/// The Appearance tab at `width`, the App icon following the theme (Phantom's), each change to
/// the icons' settings recorded as it's kept.
fn tab(width: f32) -> (Desk, Rc<RefCell<Vec<IconSettings>>>) {
    let mut d = appearance_tab();
    let kept = Rc::new(RefCell::new(Vec::new()));
    let keep = kept.clone();
    d.settings.state.app_icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), move |change| {
        if let IconChange::Tints(tints) = change {
            keep.borrow_mut().push(tints.clone());
        }
    });
    d.settings.set_viewport((width, 900.0), 2.0);
    d.settle();
    (d, kept)
}

/// The sheet open at `width`.
fn sheet(width: f32) -> (Desk, Rc<RefCell<Vec<IconSettings>>>) {
    let (mut d, kept) = tab(width);
    press(&mut d, "app-icon-customize");
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_some()), "it opens");
    (d, kept)
}

/// Presses `key`; an open picker that `key` isn't in is closed first, as a press outside it does.
fn press(d: &mut Desk, key: &str) {
    let open = d.settings.state.sheet.as_ref().and_then(|s| s.customize.as_ref()).and_then(|c| c.picker.as_ref()).map(|p| p.half);
    if let Some(half) = open
        && !key.starts_with(&format!("customize-tile-{}", half.key()))
    {
        crate::model::settings::customize::close_picker(&mut d.settings.state);
        d.settle();
    }
    super::press(d, key);
}

fn tint(d: &Desk, half: Appearance) -> Tint {
    d.settings.state.app_icon.tint("phantom", half, TILE)
}

fn picker_open(d: &Desk) -> bool {
    d.settings.state.sheet.as_ref().and_then(|s| s.customize.as_ref()).is_some_and(|c| c.picker.is_some())
}

fn key(d: &mut Desk, key: Key) {
    d.settings.key(KeyEvent { key, mods: Mods::default() });
    d.settle();
}

/// Whether the button keyed `key` takes presses: something in it takes the focus.
fn enabled(d: &Desk, key: &str) -> bool {
    let elements = d.settings.elements();
    let button = elements.iter().position(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key:?}"));
    elements.iter().any(|e| (e.parent == Some(button) || e.key.as_deref() == Some(key)) && e.focusable)
}

fn element(d: &Desk, key: &str) -> rune::ElementInfo {
    d.settings.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}"))
}

/// Customize… is on the App icon's row only while the icon shown has settings of its own:
/// Phantom's, following the theme or chosen, in either half; not another icon, nor an image added.
#[test]
fn customize_shows_only_for_an_icon_with_settings() {
    let (mut d, _) = tab(960.0);
    let mut icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), |_| {}).with_added(vec![file_id(0)]);
    icon.load();
    icon.poll();
    d.settings.state.app_icon = icon;
    let shown = |d: &mut Desk, choice: Option<Icons>| {
        d.settings.state.app_icon.choice = choice;
        d.settle();
        d.settings.find("app-icon-customize").is_some()
    };
    assert!(shown(&mut d, None), "following the theme, Phantom's");
    assert!(shown(&mut d, Some(Icons::One { icon: "phantom".into() })), "Phantom's chosen");
    assert!(shown(&mut d, Some(Icons::Pair { dark: "phantom".into(), light: "full-moon".into() })), "Phantom's in Dark");
    assert!(!shown(&mut d, Some(Icons::One { icon: "full-moon".into() })), "an icon without settings");
    assert!(!shown(&mut d, Some(Icons::One { icon: file_id(0) })), "an image added");
}

/// A fresh install's icon is Original in both halves: nothing is drawn, the row and tiles show the
/// shipped picture itself, and nothing is kept. A tile color draws it (resvg) and shows that.
#[test]
fn the_default_is_original_and_draws_the_shipped_icon() {
    let mut icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), |_| {});
    icon.load_images(Rc::new(rune::ImageCache::new(Arc::new(|| {}))));
    let looks = Looks::default();
    icon.redraw(&looks);
    for half in Appearance::BOTH {
        assert_eq!(icon.tint("phantom", half, TILE), Tint::Original);
        assert_eq!(icon.drawing("phantom", half), None, "nothing drawn in {half:?}");
        assert_eq!(icon.image_in("phantom", half).map(|i| i.id()), icon.image("phantom").map(|i| i.id()), "the shipped picture");
    }
    assert!(icon.tints.is_empty(), "nothing kept");
    icon.set_tint("phantom", Appearance::Dark, TILE, Tint::Color(Rgb([0x3f, 0x7e, 0xd0])));
    icon.redraw(&looks);
    let drawn = icon.image_in("phantom", Appearance::Dark).expect("drawn").clone();
    assert_ne!(Some(drawn.id()), icon.image("phantom").map(|i| i.id()), "Dark shows the tinted icon");
    assert_eq!(drawn.wait().ok(), Some([1024, 1024]), "rendered at icon size");
    assert_eq!(icon.image_in("phantom", Appearance::Light).map(|i| i.id()), icon.image("phantom").map(|i| i.id()), "Light as shipped");
    // The same inputs again render nothing new; another look's Accent, followed, does.
    icon.redraw(&looks);
    assert_eq!(icon.image_in("phantom", Appearance::Dark).map(|i| i.id()), Some(drawn.id()), "kept while the same");
}

/// A drag in the picker sets the Tile by hand in that half alone: the swatch shows it with the
/// accent ring's word on hover, the icon is drawn with it, and it's kept as `"#rrggbb"`.
#[test]
fn a_pick_sets_the_tile_by_hand_and_it_sticks() {
    let (mut d, kept) = sheet(960.0);
    press(&mut d, "customize-tile-light-swatch");
    assert!(picker_open(&d), "the swatch opens the picker");
    let (x, y) = d.settings.find("customize-tile-light-square").expect("its square");
    d.settings.pointer_down(x, y);
    d.settings.pointer_up(x, y);
    d.settle();
    let Tint::Color(rgb) = tint(&d, Appearance::Light) else { panic!("set by hand: {:?}", tint(&d, Appearance::Light)) };
    assert_eq!(tint(&d, Appearance::Dark), Tint::Original, "Dark untouched");
    assert!(d.settings.state.app_icon.drawing("phantom", Appearance::Light).is_some_and(|svg| svg.contains("stop-color")), "drawn");
    let last = kept.borrow().last().cloned().expect("kept");
    assert_eq!(serde_json::to_value(&last).unwrap(), serde_json::json!({"phantom": {"light": {"tile": rgb.hex()}}}));
    key(&mut d, Key::Escape);
    assert!(!picker_open(&d), "Escape closes the picker");
    assert_eq!(element(&d, "customize-tile-light-swatch").help.as_deref(), Some(format!("{} · set by hand", rgb.hex()).as_str()));
    // Typed in the hex field, Return sets it too.
    press(&mut d, "customize-tile-dark-swatch");
    d.settings.key(super::select_all());
    d.settings.paste("#6933c2");
    key(&mut d, Key::Enter);
    assert_eq!(tint(&d, Appearance::Dark), Tint::Color(Rgb([0x69, 0x33, 0xc2])), "the hex typed");
    press(&mut d, "customize-done");
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_none()), "Done closes it");
    assert_eq!(tint(&d, Appearance::Light), Tint::Color(rgb), "and keeps what was set");
}

/// The picker's Original and Follow the look: each sets its half, the one in use off.
#[test]
fn original_and_follow_the_look() {
    let (mut d, _) = sheet(960.0);
    press(&mut d, "customize-tile-dark-swatch");
    assert!(!enabled(&d, "customize-tile-dark-original"), "Original is in use");
    press(&mut d, "customize-tile-dark-follow");
    assert_eq!(tint(&d, Appearance::Dark), Tint::Follow);
    let accent = d.main.state.looks.borrow().shown(Appearance::Dark).resolved.color("accent").unwrap();
    assert_eq!(element(&d, "customize-tile-dark-swatch").fill, accent.color(), "the swatch shows the look's Accent");
    assert!(!enabled(&d, "customize-tile-dark-follow"), "in use");
    press(&mut d, "customize-tile-dark-original");
    assert_eq!(tint(&d, Appearance::Dark), Tint::Original);
    assert_eq!(d.settings.state.app_icon.drawing("phantom", Appearance::Dark), None, "the shipped picture again");
    // Cancel puts back what the sheet opened on.
    press(&mut d, "customize-tile-light-swatch");
    press(&mut d, "customize-tile-light-follow");
    press(&mut d, "customize-cancel");
    assert_eq!(tint(&d, Appearance::Light), Tint::Original, "Cancel");
}

/// With the Dark look another theme's, Dark's Follow the look is off and says why on hover;
/// Light's stays on. A Follow kept for that half draws the icon's own colors.
#[test]
fn follow_is_off_with_why_on_a_half_of_another_theme() {
    let (mut d, _) = tab(960.0);
    let sample = crate::theme::ThemeLook::defaults(crate::theme::find("sample").unwrap());
    let preset = LookPreset { dark: sample, ..LookPreset::fresh() };
    *d.main.state.looks.borrow_mut() = Looks::new(preset, |_: &LookPreset| {});
    d.settings.state.app_icon.set_tint("phantom", Appearance::Dark, TILE, Tint::Follow);
    d.settings.state.app_icon.redraw(&d.main.state.looks.borrow());
    assert_eq!(d.settings.state.app_icon.drawing("phantom", Appearance::Dark), None, "nothing to follow: Original");
    d.settle();
    press(&mut d, "app-icon-customize");
    press(&mut d, "customize-tile-dark-swatch");
    assert!(!enabled(&d, "customize-tile-dark-follow"), "off in Dark");
    let at = super::super::generated::rect_of(&d.settings, "customize-tile-dark-follow-why").expect("what holds it");
    d.settings.pointer_move(at[0] + at[2] / 2.0, at[1] + at[3] / 2.0);
    wait(&mut d.settings, 0.1);
    wait(&mut d.settings, 0.9);
    let tip = d.settings.help_tooltip().map(|(t, _)| t.to_owned());
    assert_eq!(tip.as_deref(), Some("Your Dark look isn\u{2019}t Phantom"), "why, on hover");
    key(&mut d, Key::Escape);
    press(&mut d, "customize-tile-light-swatch");
    assert!(enabled(&d, "customize-tile-light-follow"), "on in Light");
    assert!(d.settings.find("customize-tile-light-follow-why").is_none(), "nothing to say");
}

/// With Full Keyboard Access off, Tab reaches each swatch, Cancel and Done, and in the picker its
/// square, bar, field and buttons; Space on a swatch opens its picker; Escape closes the picker
/// first, then cancels the sheet; Return presses Done. VoiceOver names the swatches by half.
#[test]
fn the_keyboard_reaches_and_presses_everything() {
    let (mut d, _) = sheet(960.0);
    d.settings.set_full_keyboard_access(false);
    let keyed = |path: String| path.split('/').rev().find(|p| p.starts_with("customize-")).map(str::to_owned).unwrap_or(path);
    let round = |d: &mut Desk, n: usize| -> Vec<String> {
        (0..n)
            .map(|_| {
                key(d, Key::Tab);
                keyed(focus_path(&d.settings).unwrap_or_default())
            })
            .collect()
    };
    let mut stops = round(&mut d, 4);
    let first = stops.iter().position(|k| k == "customize-tile-light-swatch").unwrap_or_else(|| panic!("{stops:?}"));
    stops.rotate_left(first);
    assert_eq!(stops, ["customize-tile-light-swatch", "customize-tile-dark-swatch", "customize-cancel", "customize-done"]);
    // To Dark's swatch, and Space.
    while keyed(focus_path(&d.settings).unwrap_or_default()) != "customize-tile-dark-swatch" {
        key(&mut d, Key::Tab);
    }
    key(&mut d, Key::Text(" ".into()));
    assert!(picker_open(&d), "Space opens the picker");
    let mut stops = round(&mut d, 4);
    let first = stops.iter().position(|k| k == "customize-tile-dark-square").unwrap_or_else(|| panic!("{stops:?}"));
    stops.rotate_left(first);
    assert_eq!(stops, ["customize-tile-dark-square", "customize-tile-dark-bar", "customize-tile-dark", "customize-tile-dark-follow"]);
    while keyed(focus_path(&d.settings).unwrap_or_default()) != "customize-tile-dark-follow" {
        key(&mut d, Key::Tab);
    }
    key(&mut d, Key::Text(" ".into()));
    assert_eq!(tint(&d, Appearance::Dark), Tint::Follow, "Space presses Follow the look");
    key(&mut d, Key::Escape);
    assert!(!picker_open(&d), "Escape closes the picker first");
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_some()), "the sheet stays");
    assert_eq!(element(&d, "customize-tile-dark-swatch").label.as_deref(), Some("Dark Tile"));
    key(&mut d, Key::Enter);
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_none()), "Return presses Done");
    assert_eq!(tint(&d, Appearance::Dark), Tint::Follow, "kept");
    press(&mut d, "app-icon-customize");
    key(&mut d, Key::Escape);
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_none()), "Escape cancels it");
}

/// Nothing in the sheet or its picker is cut, at the widest and at the 640 pt minimum, and the
/// picker holds its buttons, in either half.
#[test]
fn nothing_is_cut_at_960_and_640() {
    for width in [960.0, 640.0] {
        for half in ["light", "dark"] {
            let (mut d, _) = sheet(width);
            press(&mut d, &format!("customize-tile-{half}-swatch"));
            scroll_to(&mut d.settings, "customize-tile");
            for e in d.settings.elements().into_iter().filter(|e| !e.exiting && e.text.is_some()) {
                assert!(!e.text_cut, "at {width} pt {:?} is cut: {:?}", e.text, e.rect);
            }
            let popup = element(&d, &format!("customize-tile-{half}-popup")).rect;
            assert!(popup[0] >= 0.0 && popup[0] + popup[2] <= width + 0.5, "at {width} pt the picker {popup:?} is inside");
            super::popup_holds_its_parts(&d, &format!("customize-tile-{half}-popup"), width);
            for part in ["original", "follow"] {
                let r = element(&d, &format!("customize-tile-{half}-{part}")).rect;
                let inside = r[0] >= popup[0] - 0.5 && r[0] + r[2] <= popup[0] + popup[2] + 0.5;
                assert!(inside, "at {width} pt {half}'s {part} {r:?} is in the picker {popup:?}");
            }
        }
    }
}

/// The Customize sheet's steps are Original and Follow the look on the Light Tile.
fn the_customize_sheet() -> super::SheetKind {
    super::SheetKind {
        before: |d| {
            press(d, "preset-1-edit");
            press(d, "edit-tone-join");
            press(d, "edit-done");
        },
        before_value: |d| format!("{:?}", super::night_tone(d)),
        open: |d| press(d, "app-icon-customize"),
        step: |d, i| {
            if !picker_open(d) {
                press(d, "customize-tile-light-swatch");
            }
            press(d, if i % 2 == 0 { "customize-tile-light-follow" } else { "customize-tile-light-original" });
        },
        done: |d| press(d, "customize-done"),
        value: |d| format!("{:?}", d.settings.state.app_icon.tints),
        key: "customize-done",
    }
}

/// Customize follows a sheet's undo rules (#87): ⌘Z takes only its own steps and it stays open;
/// Done makes the visit one step, which one ⌘Z in the main window takes back.
#[test]
fn the_customize_sheets_undo_follows_a_sheets_rules() {
    super::a_sheets_undo_follows_the_rules(the_customize_sheet());
}

/// A drag in the square is one step, Cancel leaves none, and Edit ▸ Undo names the visit.
#[test]
fn a_drag_is_one_step_and_cancel_leaves_none() {
    let (mut d, _) = sheet(960.0);
    press(&mut d, "customize-tile-light-swatch");
    let (x, y) = d.settings.find("customize-tile-light-square").expect("its square");
    d.settings.pointer_down(x, y);
    d.settings.pointer_move(x + 8.0, y + 8.0);
    d.settings.pointer_move(x + 16.0, y + 4.0);
    d.settings.pointer_up(x + 16.0, y + 4.0);
    d.settle();
    assert!(matches!(tint(&d, Appearance::Light), Tint::Color(_)));
    d.settings.key(super::cmd('z'));
    d.settle();
    assert_eq!(tint(&d, Appearance::Light), Tint::Original, "one ⌘Z takes the whole drag");
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.customize.is_some()), "the sheet stays open");
    // Cancel: put back, none of its steps left.
    press(&mut d, "customize-tile-dark-swatch");
    press(&mut d, "customize-tile-dark-follow");
    press(&mut d, "customize-cancel");
    d.settle();
    assert_eq!(tint(&d, Appearance::Dark), Tint::Original);
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "no step");
    // Done after a change: named.
    press(&mut d, "app-icon-customize");
    press(&mut d, "customize-tile-dark-swatch");
    press(&mut d, "customize-tile-dark-follow");
    press(&mut d, "customize-done");
    d.settle();
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo App Icon Colors Change");
    d.settings.key(super::cmd('z'));
    d.settle();
    assert_eq!(tint(&d, Appearance::Dark), Tint::Original, "one ⌘Z takes the visit");
}

/// The icon Tile's picker at the 640x320 minimum, in either half.
#[test]
fn the_tile_picker_fits_the_minimum_window() {
    for half in ["light", "dark"] {
        super::minimum_window_holds_the_popup(|y| {
            let (mut d, _) = sheet(640.0);
            d.settings.set_viewport((640.0, 320.0), 2.0);
            d.settle();
            super::press_at(&mut d, &format!("customize-tile-{half}-swatch"), y);
            (d, format!("customize-tile-{half}-popup"))
        });
    }
}
