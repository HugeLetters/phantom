//! ⌘P in the Settings window (#434): the same palette as the main window's, listing what applies
//! there, acting in place. Driven as a person would, with both windows; the registry tests walk
//! every setting and command, so a new one can't be left out of a palette it belongs in.

use rune::{Key, KeyEvent, Mods, Runtime};

use super::*;
use crate::changes::Target;
use crate::commands::{Command, OnSheet, Sheet};
use crate::model::palette::{self, Group, Host, Kind, Place, SettingRef};
use crate::model::settings::{SERVICE, Side, Tab};
use crate::theme::ThemeLook;
use crate::theme::preset::Appearance;
use crate::theme::setting::Value;

fn plain(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods::default() }
}

fn cmd_key(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods { cmd: true, ..Default::default() } }
}

/// Settings open in front, over a fresh install, on General.
fn in_settings() -> Desk {
    in_settings_with(MemoryService::new())
}

fn in_settings_with(service: MemoryService) -> Desk {
    let mut d = desk(service);
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    d.main.update(|_, tx| tx.blur());
    d.key(cmd_key(Key::Command(',')));
    d.wait(0.5);
    d.settle();
    assert_eq!(d.shown().front, Some(Which::Settings));
    d
}

fn press(d: &mut Desk, key: Key) {
    d.key(plain(key));
    d.settle();
}

fn type_in(d: &mut Desk, text: &str) {
    for c in text.chars() {
        d.key(plain(Key::Text(c.to_string())));
    }
    d.settle();
}

/// ⌘P in Settings.
fn open(d: &mut Desk) {
    d.front(Which::Settings);
    d.key(cmd_key(Key::Command('p')));
    d.settle();
}

fn search(d: &mut Desk, text: &str) {
    open(d);
    type_in(d, text);
}

fn is_open(d: &Desk) -> bool {
    d.settings.state.palette.is_open()
}

/// The undo, ⌘Z in Settings, once it's been through the main window.
fn undo(d: &mut Desk) {
    d.key(cmd_key(Key::Command('z')));
    d.wait(0.5);
    d.settle();
}

/// The panel's rows as drawn, with the headings between them: `(heading, [rows])`.
fn drawn<S: 'static>(rt: &Runtime<S>) -> Vec<(String, Vec<String>)> {
    let elements = rt.elements();
    let Some(panel) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")) else { return Vec::new() };
    let inside = format!("{}/", panel.path);
    let mut shown: Vec<&rune::ElementInfo> =
        elements.iter().filter(|e| !e.exiting && e.path.starts_with(&inside) && (e.role == "option" || e.role == "heading")).collect();
    shown.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for e in shown {
        if e.role == "heading" {
            groups.push((e.label.clone().expect("a heading has its text"), Vec::new()));
        } else {
            let below = format!("{}/", e.path);
            let label = elements.iter().find(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("label"));
            groups.last_mut().expect("a row under a heading").1.push(label.and_then(|c| c.text.clone()).expect("a label"));
        }
    }
    groups
}

fn headings(d: &Desk) -> Vec<String> {
    drawn(&d.settings).into_iter().map(|(h, _)| h).collect()
}

fn names(d: &Desk) -> Vec<String> {
    drawn(&d.settings).into_iter().flat_map(|(_, rows)| rows).collect()
}

/// The rows of the group under `heading`.
fn under(d: &Desk, heading: &str) -> Vec<String> {
    drawn(&d.settings).into_iter().find(|(h, _)| h == heading).map(|(_, rows)| rows).unwrap_or_default()
}

fn focused(d: &Desk) -> String {
    d.settings.elements().iter().find(|e| e.focused && !e.exiting).map(|e| e.path.clone()).unwrap_or_default()
}

fn tab(d: &Desk) -> Option<Tab> {
    d.settings.state.sheet.as_ref().map(|s| s.tab)
}

fn show_tab(d: &mut Desk, tab: Tab) {
    crate::model::settings::show_tab(&mut d.settings.state, tab);
    d.settle();
}

fn look(d: &Desk, half: Appearance, key: &str) -> Option<Value> {
    d.main.state.looks.borrow().preset().get(half).settings.get(key)
}

fn set_look(d: &mut Desk, half: Appearance, key: &'static str, value: Value) {
    let mut look = d.main.state.looks.borrow().preset().get(half).clone();
    let _ = look.settings.set(key, value);
    d.main.state.looks.borrow_mut().set(half, look);
    d.settle();
}

fn edit_active(d: &mut Desk) {
    show_tab(d, Tab::Appearance);
    d.settings.update(|s, _| crate::model::settings::presets::edit_active(s));
    d.settle();
}

/// Both halves show the second theme.
fn sample_in_both(d: &mut Desk) {
    for half in Appearance::BOTH {
        d.main.state.looks.borrow_mut().set(half, ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    }
    d.settle();
}

fn labels_of(entries: Vec<palette::Entry>) -> Vec<String> {
    entries.into_iter().map(|e| e.label).collect()
}

/// ⌘P opens it in Settings, over the page, in the main window's panel; Escape closes it. The main
/// window's own palette stays closed.
#[test]
fn cmd_p_opens_the_palette_in_settings_and_escape_closes_it() {
    let mut d = in_settings();
    assert!(!is_open(&d));
    open(&mut d);
    assert!(is_open(&d), "⌘P in Settings");
    assert!(!d.main.state.palette.is_open(), "not in the main window");
    assert!(!drawn(&d.settings).is_empty(), "its panel is drawn in Settings");
    let panel = d.settings.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")).expect("the panel");
    assert_eq!(panel.label.as_deref(), Some("Commands"));
    let field = d.settings.elements().into_iter().find(|e| !e.exiting && e.path.contains("palette-field") && e.label.is_some());
    assert_eq!(field.and_then(|e| e.label).as_deref(), Some("Commands and settings"));
    press(&mut d, Key::Escape);
    assert!(!is_open(&d), "Escape closes it");
    assert!(drawn(&d.settings).is_empty());
    assert!(d.shown().settings, "Settings stays");
}

/// With nothing typed: this tab's settings first, then everything else; the tabs are entries; no
/// "Settings…" (it's here), nothing about the torrents.
#[test]
fn the_entries_follow_the_tab_shown() {
    let mut d = in_settings();
    open(&mut d);
    assert_eq!(headings(&d), ["General", "Settings"]);
    let general: Vec<String> = SERVICE.iter().filter(|s| s.tab == Tab::General).map(|s| s.label().to_owned()).collect();
    assert_eq!(under(&d, "General"), general);
    let rest = under(&d, "Settings");
    for tab in Tab::ALL {
        assert!(rest.contains(&tab.label().to_owned()), "{tab:?} is an entry");
    }
    assert!(!rest.contains(&"Settings…".to_owned()), "Settings is where it is");
    assert!(
        !names(&d).iter().any(|n| n == "Add file…" || n.starts_with("Undo") || n == "Pause"),
        "no main-window command: {:?}",
        names(&d)
    );
    assert!(rest.contains(&"Listening port".to_owned()), "the other tabs' settings come after: {rest:?}");
    assert!(!under(&d, "General").contains(&"Listening port".to_owned()));
    press(&mut d, Key::Escape);
    show_tab(&mut d, Tab::Network);
    open(&mut d);
    assert_eq!(headings(&d), ["Network", "Settings"]);
    assert!(under(&d, "Network").contains(&"Listening port".to_owned()));
    assert!(under(&d, "Network").contains(&"UPnP".to_owned()));
    press(&mut d, Key::Escape);
    show_tab(&mut d, Tab::Appearance);
    open(&mut d);
    assert_eq!(headings(&d), ["Appearance", "Settings"]);
    let here = under(&d, "Appearance");
    for name in ["App icon…", "Look Presets", "Edit…", "Theme", "Corners"] {
        assert!(here.contains(&name.to_owned()), "{name} is Appearance's: {here:?}");
    }
}

/// A tab entry switches the tab in place, and goes into Recent.
#[test]
fn a_tab_entry_switches_the_tab_in_place() {
    let mut d = in_settings();
    search(&mut d, "network");
    assert_eq!(names(&d), ["Network"]);
    press(&mut d, Key::Enter);
    assert!(!is_open(&d));
    assert_eq!(tab(&d), Some(Tab::Network));
    open(&mut d);
    assert_eq!(headings(&d)[0], "Recent");
    assert_eq!(names(&d)[0], "Network", "Recent: the last place picked");
    assert_eq!(headings(&d), ["Recent", "Network", "Settings"]);
}

/// With the Look sheet open: its own commands first, then the Look's settings; nothing behind the
/// sheet (the tabs, the service's settings, the Look Presets).
#[test]
fn the_look_sheets_commands_come_first_and_what_is_behind_it_is_left_out() {
    let mut d = in_settings();
    edit_active(&mut d);
    open(&mut d);
    assert_eq!(headings(&d), ["Active look", "Settings"]);
    let sheet = under(&d, "Active look");
    assert_eq!(sheet[..2], ["Done", "Cancel"]);
    assert!(
        sheet.contains(&"Split Theme".to_owned()) && sheet.contains(&"Split Corners".to_owned()),
        "a Split for each joined row: {sheet:?}"
    );
    assert!(!sheet.iter().any(|n| n.starts_with("Restore")), "nothing to restore on a fresh look: {sheet:?}");
    assert!(sheet.contains(&"Join Tone".to_owned()), "Tone's halves differ by default: it's a split row");
    let rest = under(&d, "Settings");
    assert!(rest.contains(&"Corners".to_owned()) && rest.contains(&"Theme".to_owned()));
    for behind in ["General", "Network", "Keep awake", "UPnP", "Look Presets", "App icon…", "Edit…"] {
        assert!(!rest.contains(&behind.to_owned()), "{behind} is behind the sheet");
    }
    // Typing finds a sheet command first.
    type_in(&mut d, "done");
    assert_eq!(names(&d), ["Done"]);
}

/// Split and Join name the row, and each is offered where it applies: Tone is split by default
/// (Join), Corners joined (Split); one theme in each half has no Split or Join.
#[test]
fn split_and_join_are_offered_where_they_apply() {
    let mut d = in_settings();
    edit_active(&mut d);
    open(&mut d);
    let sheet = under(&d, "Active look");
    assert!(sheet.contains(&"Join Tone".to_owned()), "{sheet:?}");
    assert!(sheet.contains(&"Split Corners".to_owned()));
    type_in(&mut d, "split corners");
    press(&mut d, Key::Enter);
    assert!(!is_open(&d));
    let editor = d.settings.state.sheet.as_ref().and_then(|s| s.editor()).expect("the sheet");
    assert!(editor.split.contains("corners"), "Corners is split now");
    open(&mut d);
    assert!(under(&d, "Active look").contains(&"Join Corners".to_owned()));
    type_in(&mut d, "join corners");
    press(&mut d, Key::Enter);
    let editor = d.settings.state.sheet.as_ref().and_then(|s| s.editor()).expect("the sheet");
    assert!(!editor.split.contains("corners"), "and joined again");
    // A theme in each half: no row is joined or split.
    d.main.state.looks.borrow_mut().set(Appearance::Dark, ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    d.settle();
    open(&mut d);
    let sheet = under(&d, "Active look");
    assert!(!sheet.iter().any(|n| n.starts_with("Split") || n.starts_with("Join")), "{sheet:?}");
}

/// Restore Defaults is one entry while both halves show one theme, one for each half and both when
/// they differ; only where there is something to restore.
#[test]
fn restore_defaults_is_offered_for_the_halves_that_differ_from_their_defaults() {
    let mut d = in_settings();
    set_look(&mut d, Appearance::Light, "corners", Value::Choice("round"));
    edit_active(&mut d);
    open(&mut d);
    let sheet = under(&d, "Active look");
    assert_eq!(sheet.iter().filter(|n| n.starts_with("Restore")).collect::<Vec<_>>(), ["Restore Defaults"], "one theme in both halves");
    press(&mut d, Key::Escape);
    // A theme in each half: Light's Corners is off its default, Dark's Pulse too.
    d.main.state.looks.borrow_mut().set(Appearance::Dark, ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    set_look(&mut d, Appearance::Dark, "pulse", Value::Toggle(false));
    open(&mut d);
    let restore: Vec<String> = under(&d, "Active look").into_iter().filter(|n| n.starts_with("Restore")).collect();
    assert_eq!(restore, ["Restore Defaults ☀", "Restore Defaults ☾", "Restore Defaults"], "Light, Dark and both");
    type_in(&mut d, "restore defaults ☾");
    press(&mut d, Key::Enter);
    assert_eq!(look(&d, Appearance::Dark, "pulse"), Some(Value::Toggle(true)), "Dark restored");
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")), "Light kept");
    open(&mut d);
    let restore: Vec<String> = under(&d, "Active look").into_iter().filter(|n| n.starts_with("Restore")).collect();
    assert_eq!(restore, ["Restore Defaults ☀", "Restore Defaults"], "Dark has nothing left to restore");
}

/// Restore Defaults from the palette is the sheet's own: previewed, kept by Done as one step of
/// Undo, put back by Cancel.
#[test]
fn restore_defaults_from_the_palette_follows_done_and_cancel() {
    // Cancel puts back what the sheet opened on.
    let mut d = in_settings();
    set_look(&mut d, Appearance::Light, "corners", Value::Choice("round"));
    set_look(&mut d, Appearance::Dark, "corners", Value::Choice("round"));
    edit_active(&mut d);
    search(&mut d, "restore");
    press(&mut d, Key::Enter);
    assert!(!is_open(&d));
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "previewed at once");
    open(&mut d);
    type_in(&mut d, "cancel");
    press(&mut d, Key::Enter);
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.edit.is_none()), "Cancel closed the sheet");
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")), "Cancel put it back");
    assert_eq!(look(&d, Appearance::Dark, "corners"), Some(Value::Choice("round")));

    // Done keeps it, as one undo step.
    let steps = d.main.state.changes.state.list.len();
    edit_active(&mut d);
    search(&mut d, "restore");
    press(&mut d, Key::Enter);
    open(&mut d);
    type_in(&mut d, "done");
    press(&mut d, Key::Enter);
    d.settle();
    assert!(d.settings.state.sheet.as_ref().is_some_and(|s| s.edit.is_none()), "Done closed the sheet");
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "Done kept it");
    assert_eq!(d.main.state.changes.state.list.len(), steps + 1, "one step");
    undo(&mut d);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")), "⌘Z took the whole restore back");
}

/// Within the sheet the Look's settings are the sheet's rows: a choice from the palette is previewed,
/// kept by Done, put back by Cancel.
#[test]
fn a_setting_changed_from_the_palette_over_the_sheet_follows_done_and_cancel() {
    let mut d = in_settings();
    edit_active(&mut d);
    search(&mut d, "corners");
    press(&mut d, Key::Enter);
    press(&mut d, Key::Down);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "highlighted, not kept");
    press(&mut d, Key::Enter);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")), "the sheet's edit, shown at once");
    open(&mut d);
    type_in(&mut d, "cancel");
    press(&mut d, Key::Enter);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "Cancel put it back");
}

/// A jump focuses the control where it is: a switch on another tab, a field with its text
/// selected, a control of the Look sheet (opened), the App icon's first chip.
#[test]
fn a_jump_focuses_the_control_in_place() {
    let mut d = in_settings();
    for (typed, tab_after, control) in [
        ("upnp", Tab::Network, "upnp-switch"),
        ("keep awake", Tab::General, "keep-awake-switch"),
        ("corners", Tab::Appearance, "edit-corners-both"),
    ] {
        search(&mut d, typed);
        d.key(cmd_key(Key::Enter));
        d.wait(0.5);
        d.settle();
        assert!(!is_open(&d), "{typed}");
        assert!(!d.main.state.palette.is_open(), "{typed}");
        assert_eq!(tab(&d), Some(tab_after), "{typed}");
        assert!(focused(&d).contains(control), "{typed}: {:?} has the focus", focused(&d));
        // The sheet, if one opened, closes as the next one is looked for.
        d.settings.update(|s, _| crate::model::settings::close_over(s));
        d.settle();
    }
    // A field: its text selected.
    search(&mut d, "port");
    press(&mut d, Key::Enter);
    d.wait(0.5);
    d.settle();
    let sheet = d.settings.state.sheet.as_ref().expect("open");
    assert_eq!(sheet.tab, Tab::Network);
    assert!(sheet.port.focused, "its field has the focus");
    assert_eq!((sheet.port.anchor, sheet.port.focus), (0, sheet.port.text.len()), "its text selected");
}

/// In Settings Return acts and ⌘Return focuses the control in place; on a Choice's options both
/// still work as in the main window.
#[test]
fn return_acts_and_cmd_return_focuses_the_control() {
    let mut d = in_settings();
    let before = d.settings.state.sheet.as_ref().and_then(|s| s.service.as_ref()).map(|s| s.upnp).expect("read");
    search(&mut d, "upnp");
    press(&mut d, Key::Enter);
    assert!(!is_open(&d));
    let upnp = |d: &Desk| d.settings.state.sheet.as_ref().and_then(|s| s.service.as_ref()).map(|s| s.upnp);
    d.wait(0.5);
    d.settle();
    assert_eq!(upnp(&d), Some(!before), "Return flipped it, and stays on its tab");
    assert_eq!(tab(&d), Some(Tab::General));
    search(&mut d, "motion");
    press(&mut d, Key::Enter);
    d.key(cmd_key(Key::Enter));
    d.wait(0.5);
    d.settle();
    assert!(focused(&d).contains("edit-motion-both"), "⌘Return from the options: {:?}", focused(&d));
}

/// An on/off setting flips in place; ⌘Z, in Settings or in the palette with nothing typed, takes it
/// back.
#[test]
fn a_flip_and_its_undo() {
    let mut d = in_settings();
    let upnp = |d: &Desk| d.main.state.service_settings.clone().map(|s| s.upnp);
    d.main.state.service_settings = d.settings.state.sheet.as_ref().and_then(|s| s.service.clone());
    let before = upnp(&d).expect("settings read");
    search(&mut d, "upnp");
    press(&mut d, Key::Enter);
    d.wait(0.5);
    d.settle();
    assert_eq!(upnp(&d), Some(!before), "flipped");
    undo(&mut d);
    assert_eq!(upnp(&d), Some(before), "⌘Z took it back");
    // A theme toggle, kept open with ⇧Return, then ⌘Z in the palette itself.
    sample_in_both(&mut d);
    show_tab(&mut d, Tab::Appearance);
    search(&mut d, "pulse");
    d.key(KeyEvent { key: Key::Enter, mods: Mods { shift: true, ..Default::default() } });
    d.settle();
    assert!(is_open(&d), "⇧Return keeps it open");
    assert_eq!(look(&d, Appearance::Light, "pulse"), Some(Value::Toggle(false)), "flipped");
    d.key(cmd_key(Key::Command('z')));
    d.wait(0.5);
    d.settle();
    assert_eq!(look(&d, Appearance::Light, "pulse"), Some(Value::Toggle(true)), "⌘Z in the palette took it back");
    assert!(is_open(&d));
}

/// A choice lists its options with a live preview, in Settings as in the main window: ↓ shows the
/// option in both windows, Escape puts the look back, Return keeps it.
#[test]
fn a_choice_previews_live_and_return_keeps_it() {
    let mut d = in_settings();
    show_tab(&mut d, Tab::Appearance);
    search(&mut d, "corners");
    press(&mut d, Key::Enter);
    assert_eq!(names(&d), ["Square", "Small", "Round", "Show in Settings"]);
    press(&mut d, Key::Down);
    let shown = d.main.state.looks.borrow().shown(Appearance::Dark).clone();
    let preview = |d: &Desk| d.main.state.looks.borrow().previewed().map(|p| p.dark.settings.get("corners"));
    assert_eq!(preview(&d), Some(Some(Value::Choice("round"))), "previewed in both windows");
    drop(shown);
    press(&mut d, Key::Escape);
    press(&mut d, Key::Escape);
    assert_eq!(preview(&d), None, "put back");
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")));
    search(&mut d, "corners");
    press(&mut d, Key::Enter);
    press(&mut d, Key::Down);
    press(&mut d, Key::Enter);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")));
    undo(&mut d);
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "one undo step");
}

/// One palette at a time: opening the main window's closes Settings' and the other way round.
#[test]
fn one_palette_is_open_at_a_time() {
    let mut d = in_settings();
    open(&mut d);
    d.front(Which::Main);
    d.key(crate::shortcuts::keys(InApp::Palette)[0].event());
    d.settle();
    assert!(d.main.state.palette.is_open());
    assert!(!is_open(&d), "Settings' closed");
    d.front(Which::Settings);
    open(&mut d);
    assert!(is_open(&d));
    assert!(!d.main.state.palette.is_open(), "the main window's closed");
}

/// The main window's palette lists what it always did: its commands, the places and settings, the
/// Settings… entry, no sheet command, and a pick acts through Settings.
#[test]
fn the_main_windows_palette_is_unchanged() {
    let mut d = desk(with_an_album());
    d.settle();
    d.main.update(|s, _| palette::open(s));
    d.settle();
    let all = labels_of(palette::listed(&d.main.state));
    for name in ["Settings…", "General", "Network", "Appearance", "App icon…", "Look Presets", "Edit…", "Keep awake", "Corners"] {
        assert!(all.contains(&name.to_owned()), "{name}: {all:?}");
    }
    assert!(!all.iter().any(|n| matches!(n.as_str(), "Done" | "Cancel") || n.starts_with("Split") || n.starts_with("Restore")));
    let groups: Vec<Group> = palette::listed(&d.main.state).iter().map(|e| e.group).collect();
    assert!(!groups.contains(&Group::Here) && !groups.contains(&Group::Sheet));
    d.main.update(|s, tx| palette::dismiss(s, tx));
    d.settle();
    // A place opens Settings there.
    d.main.update(|s, _| palette::open(s));
    d.settle();
    for c in "network".chars() {
        d.main.key(plain(Key::Text(c.to_string())));
    }
    d.main.key(plain(Key::Enter));
    d.settle();
    assert!(d.shown().settings);
    assert_eq!(tab(&d), Some(Tab::Network));
}

// ---- The registry ----

/// Every service setting, and every setting each theme declares, is in the main window's palette and
/// in Settings' (on any tab, and in the Look sheet for a theme's); a new one can't be missed.
#[test]
fn every_setting_is_in_both_palettes() {
    let mut d = in_settings();
    let in_main = |d: &Desk| labels_of(palette::entries(&d.main.state));
    let in_settings = |d: &Desk| labels_of(palette::entries(&d.settings.state));
    for tab in Tab::ALL {
        show_tab(&mut d, tab);
        for setting in &SERVICE {
            let label = setting.label().to_owned();
            assert!(in_main(&d).contains(&label), "{} in the main window", setting.key);
            assert!(in_settings(&d).contains(&label), "{} in Settings on {tab:?}", setting.key);
        }
    }
    for theme in crate::theme::themes() {
        for half in Appearance::BOTH {
            d.main.state.looks.borrow_mut().set(half, ThemeLook::defaults(*theme));
        }
        d.settle();
        let theme_settings: Vec<String> = theme.settings().iter().map(|s| s.label.to_owned()).collect();
        for with_sheet in [false, true] {
            if with_sheet {
                edit_active(&mut d);
            }
            for label in &theme_settings {
                let has = |names: Vec<String>| names.iter().any(|n| n == label || n.starts_with(&format!("{label} ")));
                assert!(has(in_main(&d)), "{label} of {} in the main window (sheet: {with_sheet})", theme.id());
                assert!(has(in_settings(&d)), "{label} of {} in Settings (sheet: {with_sheet})", theme.id());
            }
        }
        d.settings.update(|s, _| crate::model::settings::close_over(s));
        d.settle();
    }
}

/// Every command is declared somewhere, and appears in each context it declares (with something to
/// act on); in no other. The windows' contexts are the main window, Settings on each tab, and
/// Settings with the Look sheet open.
#[test]
fn every_command_appears_in_each_context_it_declares() {
    let main = (Which::Main, Sheet::Closed);
    let settings = [(Which::Settings, Sheet::Closed), (Which::Settings, Sheet::Look)];
    for c in Command::all() {
        let contexts = std::iter::once(main).chain(settings).filter(|(window, sheet)| c.scope().holds(*window, *sheet)).count();
        assert!(contexts > 0, "{c:?} applies nowhere");
    }
    // The main window: with an album and two of its files selected, every command it lists is one it declares.
    let mut d = in_settings_with(with_an_album());
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settle();
    super::tap(&mut d.main, "rows/1");
    d.settle();
    let listed = palette::entries(&d.main.state);
    let actions: Vec<Command> = listed.iter().filter_map(|e| if let Kind::Action(c) = e.kind { Some(c) } else { None }).collect();
    assert!(actions.len() > 5, "the selection's commands are listed: {actions:?}");
    assert!(actions.iter().all(|c| c.scope().holds(main.0, main.1)), "every command in the main window declares it");
    // Settings: no main-window command, wherever.
    for sheet in [false, true] {
        if sheet {
            edit_active(&mut d);
        }
        let in_settings = palette::entries(&d.settings.state);
        for e in &in_settings {
            if let Kind::Action(c) = e.kind {
                let ctx = d.settings.state.context();
                assert!(c.scope().holds(ctx.window, ctx.sheet), "{c:?} in Settings (sheet: {sheet})");
                assert!(matches!(c, Command::Sheet(_)));
            }
        }
    }
    // Each sheet command is offered with the sheet open, given what it needs: Done and Cancel, a
    // Split (and a Join) for the rows of one theme, Restore Defaults for each half that differs.
    set_look(&mut d, Appearance::Light, "corners", Value::Choice("round"));
    set_look(&mut d, Appearance::Dark, "tone", Value::Choice("light"));
    let sheet_commands = |d: &Desk| -> Vec<Command> {
        palette::entries(&d.settings.state).into_iter().filter_map(|e| if let Kind::Action(c) = e.kind { Some(c) } else { None }).collect()
    };
    let has = |commands: &[Command], f: &dyn Fn(&OnSheet) -> bool| commands.iter().any(|c| matches!(c, Command::Sheet(s) if f(s)));
    let one_theme = sheet_commands(&d);
    assert!(has(&one_theme, &|s| *s == OnSheet::Done));
    assert!(has(&one_theme, &|s| *s == OnSheet::Cancel));
    assert!(has(&one_theme, &|s| matches!(s, OnSheet::Split(_))));
    assert!(has(&one_theme, &|s| matches!(s, OnSheet::Join(_))));
    assert!(has(&one_theme, &|s| *s == OnSheet::Restore(Side::Both)));
    d.main.state.looks.borrow_mut().set(Appearance::Dark, ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    set_look(&mut d, Appearance::Dark, "pulse", Value::Toggle(false));
    let two_themes = sheet_commands(&d);
    for side in [Side::Only(Appearance::Light), Side::Only(Appearance::Dark), Side::Both] {
        assert!(has(&two_themes, &move |s| *s == OnSheet::Restore(side)), "{side:?}");
    }
    // Every kind of sheet command was met.
    for c in OnSheet::ALL {
        let met = one_theme.iter().chain(&two_themes).any(|m| match (m, c) {
            (Command::Sheet(a), b) => std::mem::discriminant(a) == std::mem::discriminant(&b),
            _ => false,
        });
        assert!(met, "{c:?} is never offered");
    }
}

/// Where one entry acts differently per window it's the window's way of acting, not another entry:
/// the same setting entries in both, Show opening Settings from the main window and focusing in place
/// from Settings.
#[test]
fn an_entry_is_one_whichever_window_picks_it() {
    let d = in_settings();
    let kind = |entries: Vec<palette::Entry>, label: &str| entries.into_iter().find(|e| e.label == label).map(|e| e.kind);
    let main = palette::entries(&d.main.state);
    let settings = palette::entries(&d.settings.state);
    for label in ["Corners", "Keep awake", "Look Presets", "Network", "App icon…", "Edit…"] {
        assert_eq!(kind(main.clone(), label), kind(settings.clone(), label), "{label}");
    }
    assert_eq!(kind(main, "Settings…"), Some(Kind::Place(Place::Settings)));
    assert_eq!(kind(settings.clone(), "Settings…"), None, "Settings is where it is");
    assert_eq!(kind(settings, "Corners"), Some(Kind::Setting(SettingRef::Theme { key: "corners", side: Side::Both })));
    assert_eq!(d.settings.state.editing().0, Target::Active);
}

/// The palette is drawn in the Settings window's own look, over a tab and over the Look sheet
/// alike: its field's ring is the window's Focus ring (the accent), and a change in the sheet that
/// isn't the accent leaves it as it was. The highlighted row is the look's highlight.
#[test]
fn the_palette_in_settings_takes_the_windows_look_over_a_tab_and_over_the_sheet() {
    let ring = |d: &Desk| d.main.state.looks.borrow().shown(Appearance::Light).ui.tokens().c.border_focus;
    let mut d = in_settings();
    let tab_ring = ring(&d);
    open(&mut d);
    press(&mut d, Key::Escape);
    set_look(&mut d, Appearance::Light, "text", Value::Color(crate::theme::setting::Rgb::parse("#2a2a40")));
    edit_active(&mut d);
    open(&mut d);
    assert_eq!(ring(&d), tab_ring, "over the sheet, the same ring as over a tab");
    let first = d.settings.elements().iter().find(|e| !e.exiting && e.role == "option").map(|e| e.fill);
    assert_ne!(first, Some(rune::Color::TRANSPARENT), "the highlighted row is filled");
}
