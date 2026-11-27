//! Settings' rows fit the window (#425, #457): a label or a segment is never cut; a row's
//! controls sit beside its label where they fit whole, else on a line of their own under it; a
//! dropdown's trigger cuts its value with "…" (its menu and tooltip show it whole).

use super::*;

/// The widths checked: the widest the Look sheet gets, two in between, and Settings' minimum.
const WIDTHS: [f32; 4] = [960.0, 640.0, 560.0, 480.0];

/// Every text Settings shows (the Look sheet's, while it's open) is whole: not cut or shortened,
/// as wide as it needs, inside the window and, in the Look sheet, inside its half. `at` names
/// the case.
fn texts_whole(d: &Desk, width: f32, at: &str) {
    let elements = d.settings.elements();
    let sheet = elements.iter().any(|e| e.key.as_deref() == Some("edit-sheet") && !e.exiting);
    let half_of = |path: &str| {
        elements.iter().filter(|e| !e.exiting && path.starts_with(&e.path)).find(|e| {
            let key = e.key.as_deref().unwrap_or("");
            key.starts_with("edit-") && (key.ends_with("-left") || key.ends_with("-right"))
        })
    };
    let mut seen = 0;
    for e in elements.iter().filter(|e| !e.exiting && e.opacity > 0.0) {
        let Some(text) = e.text.as_deref().filter(|t| !t.trim().is_empty()) else { continue };
        if sheet && !e.path.contains("edit-sheet") {
            continue;
        }
        seen += 1;
        let [x, _, w, _] = e.rect;
        // Rune keeps the whole text here; `text_cut` says whether it drew it shortened. A
        // dropdown's value may be: its trigger's tooltip then reads it whole.
        if e.key.as_deref() == Some("value") && e.path.contains("-trigger") {
            if e.text_cut {
                let trigger = elements.iter().find(|t| !t.exiting && e.path.starts_with(&t.path) && t.role == "combobox");
                assert_eq!(trigger.and_then(|t| t.help.as_deref()), Some(text), "{at}: the cut {text:?} is whole in its trigger's tooltip");
            }
            continue;
        }
        assert!(!e.text_cut, "{at}: {text:?} is cut");
        if let Some([needs, _]) = e.text_size {
            assert!(needs <= w + 0.5, "{at}: {text:?} needs {needs} but is {w} wide");
        }
        assert!(x >= -0.5 && x + w <= width + 0.5, "{at}: {text:?} at {:?} spills out of the window", e.rect);
        if let Some(half) = half_of(&e.path) {
            let [hx, _, hw, _] = half.rect;
            assert!(x >= hx - 0.5 && x + w <= hx + hw + 0.5, "{at}: {text:?} at {:?} spills out of its half {:?}", e.rect, half.rect);
        }
    }
    assert!(seen >= 6, "{at}: the texts were found");
}

/// The Look sheet's rows, one theme in both halves: each row's control cell and its half.
/// Whether a row is stacked: its control is on a line under its label, not beside it.
fn stacked_rows(d: &Desk) -> Vec<(String, bool)> {
    let elements = d.settings.elements();
    let rect = |key: &str| elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).map(|e| e.rect);
    let mut rows = Vec::new();
    for e in elements.iter().filter(|e| !e.exiting) {
        let Some(key) = e.key.as_deref() else { continue };
        let Some(row) = key.strip_suffix("-both-cell").or_else(|| key.strip_suffix("-light-cell")) else { continue };
        let half = rect(&format!("{row}-left")).unwrap_or_else(|| panic!("{row}'s left half"));
        let dark = rect(&format!("{row}-dark-cell")).unwrap_or_else(|| panic!("{row}'s dark cell"));
        let stacked = e.rect[1] - half[1] > 1.0;
        assert_eq!(stacked, dark[1] - half[1] > 1.0, "{row}: both halves stack together");
        assert!((e.rect[1] - dark[1]).abs() < 0.5, "{row}: the halves' controls are on one line");
        rows.push((row.to_owned(), stacked));
    }
    rows
}

/// The Look sheet at `width`, in `dark`; `split`, its Theme, Tone, Corners and Motion each split,
/// else each joined.
fn look_sheet(width: f32, dark: bool, split: bool) -> Desk {
    let mut d = appearance_tab();
    d.settings.set_system_dark(dark);
    d.settings.set_viewport((width, 900.0), 2.0);
    d.settle();
    press(&mut d, "active-edit");
    for key in ["theme", "tone", "corners", "motion"] {
        let (now, want) = if split { ("split", "join") } else { ("join", "split") };
        if d.settings.find(&format!("edit-{key}-{now}")).is_some() && d.settings.find(&format!("edit-{key}-{want}")).is_none() {
            press(&mut d, &format!("edit-{key}-{now}"));
        }
    }
    d
}

/// Every label, segment and menu value in the Look sheet is whole at every width, both Tones,
/// joined and split; a dropdown's trigger stays on its row, its value cut with "…" if need be.
/// At 960 and 640 every row is aligned, as before; no shipped row needs to stack down to 480 pt.
#[test]
fn the_look_sheets_rows_show_every_label_and_value_whole() {
    for dark in [false, true] {
        for width in WIDTHS {
            for split in [false, true] {
                let d = look_sheet(width, dark, split);
                let at = format!("at {width} pt, dark {dark}, split {split}");
                texts_whole(&d, width, &at);
                let rows = stacked_rows(&d);
                assert!(rows.len() >= 6, "{at}: the rows were found: {rows:?}");
                if width >= 640.0 {
                    assert!(rows.iter().all(|(_, s)| !s), "{at}: every row inline, as before: {rows:?}");
                }
                // Dropdowns never stack.
                assert!(!rows.iter().any(|(r, s)| *s && (r == "edit-theme" || r == "edit-motion")), "{at}: a dropdown stacked: {rows:?}");
                assert!(rows.iter().all(|(_, s)| !s), "{at}: every shipped row fits beside its label: {rows:?}");
                let side = if split { "light" } else { "both" };
                assert!(d.settings.find(&format!("edit-tone-{side}/#0/segment1")).is_some(), "{at}: Tone's segments whole, still segments");
            }
        }
    }
}

/// Tone's controls start right after its label where the label column leaves them no room, and in
/// the column (as at 960) where it does: the same line as the label either way.
#[test]
fn tone_is_beside_its_label_wherever_that_fits() {
    let x_of = |d: &Desk, key: &str| d.settings.elements().iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).map(|e| e.rect[0]);
    let wide = look_sheet(960.0, false, false);
    let narrow = look_sheet(480.0, false, false);
    let (wide_tone, wide_motion) = (x_of(&wide, "edit-tone-both-cell").unwrap(), x_of(&wide, "edit-motion-both-cell").unwrap());
    assert_eq!(wide_tone, wide_motion, "at 960 the controls share a column");
    let (tone, left) = (x_of(&narrow, "edit-tone-both-cell").unwrap(), x_of(&narrow, "edit-tone-left").unwrap());
    assert!(tone > left, "the control is after the label");
    assert!(narrow.settings.find("edit-tone-both/#0/segment1").is_some(), "still segments");
}

/// A dropdown's menu shows every value whole at every width, even where its trigger cuts the
/// value: Motion's "Follow Reduce Motion" and every Theme name.
#[test]
fn a_dropdowns_menu_shows_every_value_whole() {
    for width in WIDTHS {
        for trigger in ["edit-motion-both-trigger", "edit-theme-both-trigger"] {
            let mut d = look_sheet(width, false, false);
            press(&mut d, trigger);
            let elements = d.settings.elements();
            let items: Vec<_> = elements.iter().filter(|e| !e.exiting && e.role.starts_with("menuitem") || e.role == "option").collect();
            assert!(items.len() >= 2, "{trigger} at {width}: the menu is open");
            for item in items {
                let texts =
                    elements.iter().filter(|t| t.path.starts_with(&item.path) && t.text.as_deref().is_some_and(|t| !t.trim().is_empty()));
                let mut seen = 0;
                for t in texts {
                    seen += 1;
                    assert!(!t.text_cut, "{trigger} at {width}: {:?} is cut in the menu", t.text);
                    if let Some([needs, _]) = t.text_size {
                        assert!(needs <= t.rect[2] + 0.5, "{trigger} at {width}: {:?} is squeezed", t.text);
                    }
                }
                assert!(seen > 0, "{trigger} at {width}: an item shows its value");
            }
        }
    }
}

/// With a theme in each half, each column's rows show their labels and values whole at every
/// width: a control that doesn't fit beside its label goes under it.
#[test]
fn a_theme_in_each_half_shows_every_label_and_value_whole() {
    for dark in [false, true] {
        for width in WIDTHS {
            let mut d = look_sheet(width, dark, true);
            let sample = crate::theme::themes().iter().position(|t| t.id() == "sample").expect("the tests' second theme");
            press(&mut d, "edit-theme-dark-trigger");
            press(&mut d, &format!("option{sample}"));
            assert!(d.settings.find("halves-columns").is_some(), "a column for each theme");
            texts_whole(&d, width, &format!("apart at {width} pt, dark {dark}"));
        }
    }
}

/// General's and Network's rows show their labels and values whole at every width.
#[test]
fn the_service_tabs_show_every_label_and_value_whole() {
    for tab in [Tab::General, Tab::Network] {
        for width in WIDTHS {
            let mut d = appearance_tab();
            d.settings.set_viewport((width, 900.0), 2.0);
            d.settle();
            press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap()));
            texts_whole(&d, width, &format!("{} at {width} pt", tab.label()));
        }
    }
}

/// Dragging the window from 960 to 480 and back, the rows follow the width and settle at each
/// step: the same at a width on the way down as on the way up, and every text whole.
#[test]
fn dragging_the_window_moves_rows_without_jitter() {
    let mut d = look_sheet(960.0, false, true);
    let widths: Vec<f32> = (0..=24).map(|i| 960.0 - 20.0 * i as f32).collect();
    let mut down = Vec::new();
    for &width in &widths {
        d.settings.set_viewport((width, 900.0), 2.0);
        d.settle();
        texts_whole(&d, width, &format!("down at {width} pt"));
        down.push(stacked_rows(&d));
    }
    for (&width, down) in widths.iter().zip(down).rev() {
        d.settings.set_viewport((width, 900.0), 2.0);
        d.settle();
        let rows = stacked_rows(&d);
        d.settle();
        assert_eq!(rows, stacked_rows(&d), "settled at {width} pt");
        assert_eq!(rows, down, "the same at {width} pt both ways");
    }
}

/// A choice shown as segments whose labels don't fit whole on a line of their own is a dropdown
/// instead, its widest value whole there (#425); Tone's Light | Dark fit and stay segments. On the
/// stacked line of a half at Settings' 480 pt minimum, both Tones.
#[test]
fn segments_too_long_for_their_line_are_a_dropdown() {
    use crate::theme::setting::{Choice, Kind, Setting, Show};
    use crate::views::settings::{control_width, segments_fit};
    const LONG: Setting = Setting::new(
        "motion",
        "Motion",
        Kind::Choice {
            choices: &[
                Choice { id: "follow", label: "Follow Reduce Motion" },
                Choice { id: "on", label: "On" },
                Choice { id: "off", label: "Off" },
            ],
            default: "follow",
            show: Show::Segments,
        },
    );
    const SHORT: Setting = Setting::new(
        "tone",
        "Tone",
        Kind::Choice {
            choices: &[Choice { id: "light", label: "Light" }, Choice { id: "dark", label: "Dark" }],
            default: "light",
            show: Show::Segments,
        },
    );
    let d = look_sheet(480.0, false, true);
    let elements = d.settings.elements();
    let half = elements.iter().find(|e| e.key.as_deref() == Some("edit-theme-left")).expect("the Theme row's left half").rect[2];
    for dark in [false, true] {
        let ui = crate::ui::Ui::phantom(dark);
        let room = ui.stacked_room(Some(half));
        assert!(!segments_fit(&LONG, &ui, room), "dark {dark}: the long labels are a dropdown");
        assert!(control_width(&LONG, &ui, false) <= room.unwrap(), "dark {dark}: its widest value whole there");
        assert!(segments_fit(&SHORT, &ui, room), "dark {dark}: Light | Dark stay segments");
        assert!(control_width(&SHORT, &ui, true) <= room.unwrap(), "dark {dark}: their labels whole there");
    }
}
