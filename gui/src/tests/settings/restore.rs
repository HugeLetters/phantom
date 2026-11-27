//! Restore Defaults on the Look sheet (#417): a footer button that sets each half's settings back
//! to a fresh install's, keeping its Theme and which rows are split; a pull-down of Light, Dark
//! and Both while the halves show different themes. Previewed, kept by Done, dropped by Cancel.

use std::path::{Path, PathBuf};

use rune::{Key, KeyEvent, Mods};

use super::super::Desk;
use super::super::focus_path;
use super::{appearance_tab, cmd, press, tab_to};
use crate::settings::file;
use crate::theme::ThemeLook;
use crate::theme::preset::{self, Appearance, LookPreset, Looks};
use crate::theme::setting::{Rgb, Value};

const BLACK: Value = Value::Color(Some(Rgb([0, 0, 0])));

fn look(d: &Desk, a: Appearance) -> ThemeLook {
    d.main.state.looks.borrow().preset().get(a).clone()
}

fn accent(d: &Desk, a: Appearance) -> Option<Value> {
    look(d, a).settings.get("accent")
}

/// The Accent of `a` set to black, as a person's edit leaves it.
fn blacken(d: &mut Desk, a: Appearance) {
    let mut look = look(d, a);
    let _ = look.settings.set("accent", BLACK);
    d.main.state.looks.borrow_mut().set(a, look);
    d.settle();
}

/// The Active look's sheet open over a fresh look kept in `path` (if any) as it changes.
fn sheet(path: Option<&Path>) -> Desk {
    let mut d = appearance_tab();
    if let Some(path) = path {
        let at = path.to_owned();
        let fresh = LookPreset::fresh();
        file::keep_at(path, preset::SETTING, Some(&fresh.stored())).unwrap();
        *d.main.state.looks.borrow_mut() =
            Looks::new(fresh, move |p: &LookPreset| file::keep_at(&at, preset::SETTING, Some(&p.stored())).unwrap());
    }
    d.settle();
    press(&mut d, "active-edit");
    d
}

/// Dark's theme the tests' second one, in its own column.
fn apart(d: &mut Desk) {
    let sample = crate::theme::themes().iter().position(|t| t.id() == "sample").expect("the tests' second theme");
    press(d, "edit-theme-split");
    press(d, "edit-theme-dark-trigger");
    press(d, &format!("option{sample}"));
}

fn enabled(d: &Desk, key: &str) -> bool {
    let elements = d.settings.elements();
    let own = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).expect(key);
    elements.iter().any(|e| !e.exiting && e.path.starts_with(&own.path) && e.focusable)
}

fn gui_json(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("gui.json")
}

#[test]
fn one_button_restores_both_halves_while_the_theme_is_joined() {
    let mut d = sheet(None);
    for a in Appearance::BOTH {
        blacken(&mut d, a);
    }
    assert!(enabled(&d, "edit-restore"));
    let themes = Appearance::BOTH.map(|a| look(&d, a).theme.id());
    press(&mut d, "edit-restore");
    for a in Appearance::BOTH {
        assert_eq!(accent(&d, a), Some(Value::Color(None)), "{a:?} back to its default");
    }
    assert_eq!(Appearance::BOTH.map(|a| look(&d, a).theme.id()), themes, "themes kept");
    assert!(!enabled(&d, "edit-restore"), "greyed at the defaults");
}

#[test]
fn restore_keeps_split_and_each_halfs_tone() {
    let mut d = sheet(None);
    let tones = Appearance::BOTH.map(|a| look(&d, a).settings.get("tone"));
    assert_ne!(tones[0], tones[1], "a fresh install's Tone follows the Appearance");
    blacken(&mut d, Appearance::Light);
    assert!(d.settings.find("edit-tone-join").is_some(), "Tone split");
    press(&mut d, "edit-restore");
    assert!(d.settings.find("edit-tone-join").is_some(), "still split");
    assert_eq!(Appearance::BOTH.map(|a| look(&d, a).settings.get("tone")), tones, "Light in Light, Dark in Dark");
}

#[test]
fn a_theme_in_each_half_restores_light_dark_or_both() {
    for (pick, light, dark) in [(0, true, false), (1, false, true), (2, true, true)] {
        let mut d = sheet(None);
        apart(&mut d);
        for a in Appearance::BOTH {
            blacken(&mut d, a);
        }
        let themes = Appearance::BOTH.map(|a| look(&d, a).theme.id());
        press(&mut d, "edit-restore");
        let (x, y) = {
            let e = d.settings.elements();
            let item = e.iter().filter(|e| !e.exiting && e.role.starts_with("menuitem")).nth(pick).expect("the entry");
            (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0)
        };
        d.settings.pointer_down(x, y);
        d.settings.pointer_up(x, y);
        d.settle();
        let reset = |a| accent(&d, a) == Some(Value::Color(None));
        assert_eq!((reset(Appearance::Light), reset(Appearance::Dark)), (light, dark), "entry {pick}");
        assert_eq!(Appearance::BOTH.map(|a| look(&d, a).theme.id()), themes, "themes kept");
    }
}

#[test]
fn entries_and_button_are_greyed_where_there_is_nothing_to_restore() {
    let mut d = sheet(None);
    apart(&mut d);
    assert!(!enabled(&d, "edit-restore"), "everything at its defaults");
    blacken(&mut d, Appearance::Dark);
    assert!(enabled(&d, "edit-restore"));
    press(&mut d, "edit-restore");
    let items: Vec<(String, bool)> = d
        .settings
        .elements()
        .iter()
        .filter(|e| !e.exiting && e.role.starts_with("menuitem"))
        .map(|e| (e.label.clone().or(e.text.clone()).unwrap_or_default(), e.focusable))
        .collect();
    assert_eq!(items.len(), 3, "{items:?}");
    let on: Vec<bool> = items.iter().map(|i| i.1).collect();
    assert_eq!(on, [false, true, true], "only Dark and Both: {items:?}");
    let elements = d.settings.elements();
    let widest = elements.iter().filter(|e| !e.exiting && e.role.starts_with("menuitem")).map(|e| e.rect[2]).fold(0.0, f32::max);
    assert!(widest < 160.0, "the pull-down is sized to Light, Dark and Both, not {widest} pt wide");
}

#[test]
fn restoring_is_one_undo_step() {
    let mut d = sheet(None);
    for a in Appearance::BOTH {
        blacken(&mut d, a);
    }
    press(&mut d, "edit-restore");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Restore Defaults Change");
    d.key(cmd('z'));
    d.settle();
    for a in Appearance::BOTH {
        assert_eq!(accent(&d, a), BLACK.into(), "{a:?} back as it was");
    }
}

#[test]
fn cancel_after_restoring_leaves_the_look_as_it_was_and_done_keeps_it() {
    for done in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = gui_json(&dir);
        let mut d = sheet(Some(&path));
        for a in Appearance::BOTH {
            blacken(&mut d, a);
        }
        // What the app held when the sheet opened.
        press(&mut d, "edit-done");
        d.settle();
        press(&mut d, "active-edit");
        let before = std::fs::read(&path).unwrap();
        press(&mut d, "edit-restore");
        assert_eq!(accent(&d, Appearance::Light), Some(Value::Color(None)), "previewed");
        assert_eq!(std::fs::read(&path).unwrap(), before, "nothing written while the sheet is open");
        if done {
            press(&mut d, "edit-done");
            d.settle();
            assert_ne!(std::fs::read(&path).unwrap(), before, "Done writes it");
            assert_eq!(accent(&d, Appearance::Light), Some(Value::Color(None)));
        } else {
            press(&mut d, "edit-cancel");
            d.settle();
            assert_eq!(std::fs::read(&path).unwrap(), before, "Cancel writes nothing");
            for a in Appearance::BOTH {
                assert_eq!(accent(&d, a), BLACK.into(), "{a:?} as it was");
            }
        }
    }
}

#[test]
fn tab_reaches_restore_space_presses_it_and_return_still_presses_done() {
    let mut d = sheet(None);
    d.settings.set_full_keyboard_access(false);
    for a in Appearance::BOTH {
        blacken(&mut d, a);
    }
    tab_to(&mut d, "edit-restore");
    assert!(focus_path(&d.settings).unwrap_or_default().contains("edit-restore"), "Tab reached it");
    d.settings.key(KeyEvent { key: Key::Text(" ".into()), mods: Mods::default() });
    d.settle();
    assert_eq!(accent(&d, Appearance::Light), Some(Value::Color(None)), "Space pressed it");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    assert!(d.settings.find("edit-sheet").is_none(), "Return pressed Done");
}

#[test]
fn the_footer_is_whole_at_every_width() {
    for width in [960.0, 640.0, 480.0] {
        for apart_halves in [false, true] {
            let mut d = sheet(None);
            d.settings.set_viewport((width, 900.0), 2.0);
            d.settle();
            if apart_halves {
                apart(&mut d);
            }
            let e = d.settings.elements();
            let find = |key: &str| {
                e.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).map(|e| e.rect).unwrap_or_else(|| panic!("no {key}"))
            };
            let (restore, cancel, done) = (find("edit-restore"), find("edit-cancel"), find("edit-done"));
            let at = format!("{width} pt, apart {apart_halves}");
            assert!(restore[0] >= 0.0 && done[0] + done[2] <= width + 0.5, "{at}: inside the window");
            assert!(restore[0] + restore[2] <= cancel[0] + 0.5, "{at}: clear of Cancel: {restore:?} {cancel:?}");
            assert!(cancel[0] + cancel[2] <= done[0] + 0.5, "{at}: Cancel clear of Done");
            for e in e.iter().filter(|e| !e.exiting && e.path.contains("edit-restore") && e.text.is_some()) {
                assert!(!e.text_cut, "{at}: {:?} is cut", e.text);
            }
        }
    }
}
