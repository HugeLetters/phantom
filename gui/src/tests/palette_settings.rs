//! The palette's settings and places (#434), driven as a person would with both windows: ⌘P, a few
//! letters, Return or ⌘Return. Each kind of entry does what the design says: an action runs, a place
//! opens Settings there, an on/off setting flips with an Undo, a Choice lists its options, a typed
//! value shows in Settings with its control focused.

use rune::{Key, KeyEvent, Mods};

use super::*;
use crate::model::palette::{Group, Kind, Place, SettingRef, strict};
use crate::model::settings::{Side, Tab};
use crate::theme::preset::Appearance;
use crate::theme::setting::Value;

fn desk_with_palette() -> Desk {
    let mut d = desk(MemoryService::new());
    d.main.set_viewport((960.0, 600.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    d
}

fn press(d: &mut Desk, key: Key) {
    d.key(KeyEvent { key, mods: Mods::default() });
    d.settle();
}

fn cmd(d: &mut Desk, key: Key) {
    d.key(KeyEvent { key, mods: Mods { cmd: true, ..Default::default() } });
    d.settle();
}

fn open(d: &mut Desk) {
    d.front(Which::Main);
    d.key(shortcuts::keys(InApp::Palette)[0].event());
    d.settle();
}

fn type_in(d: &mut Desk, text: &str) {
    for c in text.chars() {
        d.key(KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() });
    }
    d.settle();
}

/// ⌘P and `text`.
fn search(d: &mut Desk, text: &str) {
    open(d);
    type_in(d, text);
}

/// The names the open palette's rows show, top to bottom.
fn names(d: &Desk) -> Vec<String> {
    let elements = d.main.elements();
    let Some(panel) = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")) else { return Vec::new() };
    let inside = format!("{}/", panel.path);
    let mut rows: Vec<&rune::ElementInfo> =
        elements.iter().filter(|e| !e.exiting && e.role == "option" && e.path.starts_with(&inside)).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.iter()
        .map(|row| {
            let below = format!("{}/", row.path);
            let label = elements.iter().find(|c| c.path.starts_with(&below) && c.key.as_deref() == Some("label"));
            label.and_then(|c| c.text.clone()).expect("a row has its name")
        })
        .collect()
}

/// What the open palette's row `name` reads aloud.
fn read(d: &Desk, name: &str) -> String {
    let at = names(d).iter().position(|n| n == name).unwrap_or_else(|| panic!("no row {name:?}"));
    super::palette::labels(&d.main)[at].clone()
}

fn headings(d: &Desk) -> Vec<String> {
    super::palette::headings(&d.main)
}

fn highlighted(d: &Desk) -> String {
    names(d)[crate::model::palette::highlighted(&d.main.state)].clone()
}

fn palette_open(d: &Desk) -> bool {
    d.main.state.palette.is_open()
}

/// The key of what has the focus in Settings.
fn focused_in_settings(d: &Desk) -> Option<String> {
    d.settings.elements().iter().find(|e| e.focused && !e.exiting).map(|e| e.path.clone())
}

fn look(d: &Desk, half: Appearance, key: &str) -> Option<Value> {
    d.main.state.looks.borrow().preset().get(half).settings.get(key)
}

fn keep_awake(d: &Desk) -> Option<bool> {
    d.main.state.service_settings.as_ref().map(|s| s.keep_awake)
}

fn notice(d: &Desk) -> Option<(String, bool)> {
    d.main.state.bar.newest().map(|n| (n.short.clone(), n.undo().is_some()))
}

/// Settings are matched strictly: each letter starts a word or follows the one before it, so "tone"
/// finds Tone and not "Stop seeding after"; a synonym ranks under a name.
#[test]
fn a_setting_matches_strictly_and_a_synonym_ranks_under_a_name() {
    assert!(strict("tone", "Tone").is_some());
    assert_eq!(strict("tone", "Stop seeding after"), None);
    assert!(strict("tone", "Stop seeding at ratio").is_none());
    assert!(strict("ka", "Keep awake").is_some(), "word starts");
    assert!(strict("keepa", "Keep awake").is_some(), "a run, then a word start");
    assert_eq!(strict("kw", "Keep awake"), None, "w is neither a word start nor after k");
    assert!(strict("ab", "a ab").is_some(), "the later a, which ab follows");
    assert!(strict("", "Anything").is_some());
    assert!(strict("keep", "Keep awake") > strict("keep", "Keep awake").map(|s| s - 3), "a synonym is a little under");
}

/// "tone" lists Tone in each half, found by its name; "radius" finds Corners by a synonym; not
/// "Stop seeding after". Typing puts the Settings group first, over the selection's commands.
#[test]
fn typing_finds_settings_by_name_and_synonym_and_puts_the_best_group_first() {
    let mut d = desk(with_an_album());
    d.settle();
    search(&mut d, "tone");
    assert_eq!(names(&d), ["Tone ☀", "Tone ☾"]);
    assert_eq!(headings(&d), ["Settings"]);
    type_in(&mut d, "x");
    assert!(names(&d).is_empty());
    press(&mut d, Key::Backspace);
    press(&mut d, Key::Escape);
    search(&mut d, "radius");
    assert_eq!(names(&d), ["Corners"], "by a synonym");
    press(&mut d, Key::Escape);
    search(&mut d, "seed");
    assert_eq!(names(&d), ["Stop seeding at ratio", "Stop seeding after"]);
    press(&mut d, Key::Escape);
    // Every setting has an entry: the service's from the table, the theme's from what it declares.
    open(&mut d);
    let all: Vec<String> = crate::model::palette::settings_entries(&d.main.state).into_iter().map(|e| e.label).collect();
    for setting in &crate::model::settings::SERVICE {
        assert!(all.contains(&setting.label().to_owned()), "{}", setting.key);
    }
    for (setting, _) in d.main.state.looks.borrow().preset().light.settings.iter() {
        assert!(all.iter().any(|l| l.starts_with(setting.label)), "{}", setting.key);
    }
    assert!(Tab::ALL.iter().all(|t| all.contains(&t.label().to_owned())));
}

/// A setting's row reads its name, value and place; a Choice adds "has choices"; its list is
/// labelled "Corners, 3 choices"; the field is "Commands and settings".
#[test]
fn rows_read_their_name_value_and_place_and_the_field_is_labelled() {
    let mut d = desk_with_palette();
    search(&mut d, "corners");
    assert_eq!(read(&d, "Corners"), "Corners, Small, Appearance, Edit…, Look, has choices");
    let elements = d.main.elements();
    let field = elements.iter().find(|e| e.path.contains("palette-field") && e.label.is_some()).and_then(|e| e.label.clone());
    assert_eq!(field.as_deref(), Some("Commands and settings"));
    press(&mut d, Key::Enter);
    let list = d.main.elements().into_iter().find(|e| !e.exiting && e.role == "listbox").and_then(|e| e.label);
    assert_eq!(list.as_deref(), Some("Corners, 3 choices"));
    press(&mut d, Key::Escape);
    type_in(&mut d, "");
    press(&mut d, Key::Escape);
    search(&mut d, "keep awake");
    assert!(d.main.state.service_settings.is_some());
    assert_eq!(read(&d, "Keep awake"), "Keep awake, Off, General");
}

/// A place opens Settings there: a tab, the editor sheet on the Active look.
#[test]
fn a_place_opens_settings_there() {
    let mut d = desk_with_palette();
    search(&mut d, "network");
    press(&mut d, Key::Enter);
    assert!(!palette_open(&d));
    assert!(d.shown().settings);
    assert_eq!(d.settings.state.sheet.as_ref().map(|s| s.tab), Some(Tab::Network));
    search(&mut d, "edit");
    assert_eq!(names(&d), ["Edit…"]);
    press(&mut d, Key::Enter);
    let sheet = d.settings.state.sheet.as_ref().expect("open");
    assert_eq!(sheet.tab, Tab::Appearance);
    assert_eq!(sheet.edit.as_ref().map(|e| e.target.clone()), Some(crate::changes::Target::Active));
}

/// An action still runs as before: its entry is the selection's, and Return runs it.
#[test]
fn an_action_runs_as_before() {
    let mut d = desk(with_an_album());
    d.settle();
    tap(&mut d.main, "rows/1");
    d.settle();
    search(&mut d, "pause");
    assert_eq!(headings(&d)[0], "Album");
    assert!(matches!(
        crate::model::palette::listed(&d.main.state)[0].kind,
        Kind::Action(crate::commands::Command::Torrents(OnTorrents::Pause))
    ));
}

/// Return on an on/off service setting flips it at once; the status bar says so, with Undo, and ⌘Z
/// takes it back.
#[test]
fn an_on_off_service_setting_flips_with_undo_and_cmd_z() {
    let mut d = desk_with_palette();
    for (typed, name) in [("keep awake", "Keep awake"), ("upnp", "UPnP")] {
        search(&mut d, typed);
        let before = d.main.state.service_settings.clone().expect("told at open");
        press(&mut d, Key::Enter);
        assert!(!palette_open(&d));
        let now = d.main.state.service_settings.clone().unwrap();
        assert_ne!(now, before, "{name} flipped");
        let word = if name == "Keep awake" {
            if now.keep_awake { "On" } else { "Off" }
        } else if now.upnp {
            "On"
        } else {
            "Off"
        };
        assert_eq!(notice(&d), Some((format!("{name}: {word}"), true)), "the bar says so, with Undo");
        d.front(Which::Main);
        cmd(&mut d, Key::Command('z'));
        d.wait(0.5);
        d.settle();
        assert_eq!(d.main.state.service_settings.clone().unwrap(), before, "⌘Z took {name} back");
    }
}

/// The notice's Undo takes the flip back too.
#[test]
fn the_notices_undo_takes_the_flip_back() {
    let mut d = desk_with_palette();
    search(&mut d, "keep awake");
    press(&mut d, Key::Enter);
    assert_eq!(keep_awake(&d), Some(true));
    let undo = d.main.state.bar.newest_undo().cloned().expect("an Undo");
    d.main.update(move |s, tx| (undo.run)(s, tx));
    d.wait(0.5);
    d.settle();
    assert_eq!(keep_awake(&d), Some(false));
}

/// Tone is Light in ☀ and Dark in ☾ by default: one entry for each half while split, one while
/// joined. Setting a half changes that half only, and an undo step.
#[test]
fn a_split_setting_has_an_entry_for_each_half_until_it_is_joined() {
    let mut d = desk_with_palette();
    search(&mut d, "tone");
    assert_eq!(names(&d), ["Tone ☀", "Tone ☾"]);
    assert_eq!(read(&d, "Tone ☀"), "Tone ☀, Light, Appearance, Edit…, has choices");
    assert_eq!(read(&d, "Tone ☾"), "Tone ☾, Dark, Appearance, Edit…, has choices");
    press(&mut d, Key::Down);
    press(&mut d, Key::Enter);
    assert_eq!(names(&d), ["Light", "Dark", "Show in Settings"], "☾'s choices");
    assert_eq!(highlighted(&d), "Dark", "on the current one");
    press(&mut d, Key::Up);
    press(&mut d, Key::Enter);
    assert_eq!(
        (look(&d, Appearance::Light, "tone"), look(&d, Appearance::Dark, "tone")),
        (Some(Value::Choice("light")), Some(Value::Choice("light")))
    );
    assert_eq!(notice(&d), Some(("Tone ☾: Light".into(), true)));
    // Now both are Light: one entry.
    search(&mut d, "tone");
    assert_eq!(names(&d)[0], "Tone", "one entry for both halves (and the Undo of the last change after it)");
    assert!(!names(&d).iter().any(|n| n.starts_with("Tone ")));
    press(&mut d, Key::Enter);
    press(&mut d, Key::Down);
    press(&mut d, Key::Enter);
    assert_eq!(
        (look(&d, Appearance::Light, "tone"), look(&d, Appearance::Dark, "tone")),
        (Some(Value::Choice("dark")), Some(Value::Choice("dark")))
    );
    // Each was one undo step.
    d.front(Which::Main);
    cmd(&mut d, Key::Command('z'));
    d.wait(0.5);
    d.settle();
    assert_eq!(look(&d, Appearance::Light, "tone"), Some(Value::Choice("light")));
}

/// Return on a Choice lists its options, the current one ticked and highlighted; typing filters;
/// Return sets one, for both halves while joined, as one undo step; Escape goes back with the typed
/// text, and from the list closes.
#[test]
fn a_choice_lists_its_options_and_escape_goes_back_a_level() {
    let mut d = desk_with_palette();
    search(&mut d, "corn");
    press(&mut d, Key::Enter);
    assert!(palette_open(&d));
    assert_eq!(names(&d), ["Square", "Small", "Round", "Show in Settings"]);
    assert_eq!(headings(&d), ["Corners"]);
    assert_eq!(highlighted(&d), "Small");
    let ticked = crate::model::palette::listed(&d.main.state).into_iter().filter(|e| e.selected).map(|e| e.label).collect::<Vec<_>>();
    assert_eq!(ticked, ["Small"]);
    type_in(&mut d, "ro");
    assert_eq!(names(&d), ["Round"]);
    press(&mut d, Key::Escape);
    assert!(palette_open(&d), "back to the list, not closed");
    assert_eq!(d.main.state.palette.query.text, "corn", "with the typed text back");
    assert_eq!(names(&d), ["Corners"]);
    press(&mut d, Key::Enter);
    press(&mut d, Key::Down);
    press(&mut d, Key::Enter);
    assert!(!palette_open(&d));
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")));
    assert_eq!(look(&d, Appearance::Dark, "corners"), Some(Value::Choice("round")));
    assert_eq!(notice(&d), Some(("Corners: Round".into(), true)));
    d.front(Which::Main);
    cmd(&mut d, Key::Command('z'));
    d.wait(0.5);
    d.settle();
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "an undo step");
    search(&mut d, "corn");
    press(&mut d, Key::Escape);
    assert!(!palette_open(&d), "Escape from the list closes");
}

/// ⌘Return on any setting opens Settings at it with its control focused itself, not first in the
/// Tab order; a field takes it with its text selected.
#[test]
fn cmd_return_shows_a_setting_in_settings_with_its_control_focused() {
    let mut d = desk_with_palette();
    for (typed, tab, control) in [
        ("keep awake", Tab::General, "keep-awake-switch"),
        ("upnp", Tab::Network, "upnp-switch"),
        ("tone ☀", Tab::Appearance, "edit-tone-light"),
        ("corners", Tab::Appearance, "edit-corners-both"),
    ] {
        search(&mut d, typed);
        cmd(&mut d, Key::Enter);
        d.wait(0.5);
        d.settle();
        assert!(!palette_open(&d), "{typed}");
        assert!(d.shown().settings, "{typed}");
        assert_eq!(d.settings.state.sheet.as_ref().map(|s| s.tab), Some(tab), "{typed}");
        let focused = focused_in_settings(&d).unwrap_or_default();
        assert!(focused.contains(control), "{typed}: {focused:?} has the focus");
    }
    // A typed value: Return shows it too, its field focused with the text selected.
    d.settings.update(|s, _| crate::model::settings::close(s));
    d.settle();
    search(&mut d, "port");
    press(&mut d, Key::Enter);
    d.wait(0.5);
    d.settle();
    let sheet = d.settings.state.sheet.as_ref().expect("open");
    assert_eq!(sheet.tab, Tab::Network);
    assert!(sheet.port.focused, "its field has the focus");
    assert_eq!((sheet.port.anchor, sheet.port.focus), (0, sheet.port.text.len()), "its text selected");
}

/// From a setting's choices ⌘Return shows that setting in Settings.
#[test]
fn cmd_return_in_a_settings_choices_shows_the_setting() {
    let mut d = desk_with_palette();
    search(&mut d, "motion");
    press(&mut d, Key::Enter);
    cmd(&mut d, Key::Enter);
    d.wait(0.5);
    d.settle();
    assert!(!palette_open(&d));
    assert!(focused_in_settings(&d).unwrap_or_default().contains("edit-motion-both"));
}

/// Recent: with nothing typed, the last 3 settings or places picked come first, newest first, and
/// no more; actions don't go in. It's in memory only: a new palette state has none.
#[test]
fn recent_lists_the_last_three_settings_and_places_picked() {
    let mut d = desk_with_palette();
    open(&mut d);
    assert_eq!(headings(&d), ["Phantom", "Settings"], "nothing picked yet: the commands, then every setting");
    press(&mut d, Key::Escape);
    for typed in ["general", "keep awake", "upnp", "network"] {
        search(&mut d, typed);
        press(&mut d, Key::Enter);
    }
    open(&mut d);
    assert_eq!(headings(&d), ["Recent", "Phantom", "Settings"]);
    let all = names(&d);
    assert_eq!(all.iter().filter(|n| *n == "Network").count(), 1, "what Recent lists isn't listed again: {all:?}");
    assert_eq!(names(&d)[..3], ["Network", "UPnP", "Keep awake"], "newest first");
    let recent = crate::model::palette::listed(&d.main.state);
    assert!(recent[..3].iter().all(|e| e.group == Group::Recent));
    assert_eq!(recent[0].kind, Kind::Place(Place::Tab(Tab::Network)));
    assert_eq!(recent[1].kind, Kind::Setting(SettingRef::Service("upnp")));
    // Typing lists them in the Settings group as any.
    type_in(&mut d, "network");
    assert_eq!(headings(&d), ["Settings"]);
}

/// A theme that declares a new setting shows up with no change to the palette: Sample's toggle
/// flips, its choice lists, its color shows in Settings.
#[test]
fn a_new_theme_setting_appears_without_touching_the_palette() {
    let mut d = desk_with_palette();
    for half in Appearance::BOTH {
        d.main.state.looks.borrow_mut().set(half, crate::theme::ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    }
    d.settle();
    search(&mut d, "pulse");
    assert_eq!(names(&d), ["Pulse"]);
    assert_eq!(read(&d, "Pulse"), "Pulse, On, Appearance, Edit…");
    press(&mut d, Key::Enter);
    assert_eq!(look(&d, Appearance::Light, "pulse"), Some(Value::Toggle(false)), "a toggle flips");
    assert_eq!(notice(&d), Some(("Pulse: Off".into(), true)));
    search(&mut d, "shape");
    press(&mut d, Key::Enter);
    assert_eq!(names(&d), ["Round", "Square", "Cut", "Show in Settings"]);
    press(&mut d, Key::Escape);
    press(&mut d, Key::Escape);
    search(&mut d, "accent");
    press(&mut d, Key::Enter);
    d.wait(0.5);
    d.settle();
    assert!(d.shown().settings, "a color shows in Settings");
}

/// The palette's entries and their place and value show whole: nothing is cut, in the window's
/// widest (960) and at the 640×320 minimum; the panel stays inside the window.
#[test]
fn nothing_is_cut_at_the_widest_and_the_smallest_window() {
    for (w, h) in [(960.0, 600.0), (640.0, 320.0)] {
        let mut d = desk_with_palette();
        d.main.set_viewport((w, h), 2.0);
        d.settle();
        for typed in ["corners", "keep awake", "every", "default"] {
            search(&mut d, typed);
            let elements = d.main.elements();
            let panel = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some("palette-panel")).expect("the panel");
            assert!(panel.rect[0] >= 0.0 && panel.rect[0] + panel.rect[2] <= w + 0.5, "{typed} at {w}: inside the window {:?}", panel.rect);
            let look = d.main.state.looks.borrow().shown(Appearance::Dark).clone();
            let inside = format!("{}/", panel.path);
            for text in
                elements.iter().filter(|e| !e.exiting && e.path.starts_with(&inside) && matches!(e.key.as_deref(), Some("label" | "value")))
            {
                let shown = text.text.as_deref().unwrap_or_default();
                let needs = look.ui.label_width(shown);
                assert!(text.rect[2] + 0.5 >= needs, "{typed} at {w}: {shown:?} is cut: {} of {needs}", text.rect[2]);
            }
            press(&mut d, Key::Escape);
        }
    }
}

/// Side is how the palette tells a half: the joined entry sets both.
#[test]
fn a_joined_entry_is_for_both_halves() {
    let mut d = desk_with_palette();
    for half in Appearance::BOTH {
        let mut l = d.main.state.looks.borrow().preset().get(half).clone();
        l.settings.set("tone", Value::Choice("light"));
        d.main.state.looks.borrow_mut().set(half, l);
    }
    open(&mut d);
    type_in(&mut d, "tone");
    let list = crate::model::palette::listed(&d.main.state);
    assert_eq!(list[0].kind, Kind::Setting(SettingRef::Theme { key: "tone", side: Side::Both }));
}

/// The Settings screen draws from the same table the palette lists: every row of it is a setting
/// of the sheet, on its tab.
#[test]
fn the_service_table_is_what_the_screen_and_the_palette_share() {
    use crate::model::settings::SERVICE;
    use crate::views::settings::{SETTINGS, tab_of};
    for setting in &SERVICE {
        assert!(SETTINGS.contains(&setting.key), "{}", setting.key);
        assert_eq!(tab_of(setting.key), Some(setting.tab), "{}", setting.key);
    }
    assert_eq!(SETTINGS.len(), SERVICE.len() + 3, "the rest are Appearance's: Look Presets and the App icon");
}

fn shift_return(d: &mut Desk) {
    d.key(KeyEvent { key: Key::Enter, mods: Mods { shift: true, ..Default::default() } });
    d.settle();
}

/// ⇧Return runs the entry and keeps the palette open, its field empty, at the top level (Recent
/// first), so the next can be typed at once: twice in a row runs two commands, each with its notice,
/// and ⌘Z takes each back, newest first.
#[test]
fn shift_return_runs_and_stays_open_for_the_next() {
    let mut d = desk_with_palette();
    search(&mut d, "keep awake");
    shift_return(&mut d);
    assert!(palette_open(&d), "it stays open");
    assert_eq!(d.main.state.palette.query.text, "", "its field empty");
    assert!(d.main.state.palette.sub.is_none(), "at the top level");
    assert_eq!(headings(&d)[0], "Recent", "Recent first");
    assert_eq!(names(&d)[0], "Keep awake");
    assert_eq!(keep_awake(&d), Some(true));
    assert_eq!(notice(&d), Some(("Keep awake: On".into(), true)), "its notice shows");
    // The next, typed at once: a Choice's option, from its list.
    type_in(&mut d, "corn");
    press(&mut d, Key::Enter);
    assert_eq!(highlighted(&d), "Small");
    press(&mut d, Key::Down);
    shift_return(&mut d);
    assert!(palette_open(&d) && d.main.state.palette.sub.is_none() && d.main.state.palette.query.text.is_empty());
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")));
    assert_eq!(notice(&d), Some(("Corners: Round".into(), true)));
    // And a third, on/off again.
    type_in(&mut d, "upnp");
    shift_return(&mut d);
    assert!(palette_open(&d));
    let upnp = d.main.state.service_settings.as_ref().map(|s| s.upnp);
    assert_eq!(upnp, Some(false), "UPnP starts On");
    // ⌘Z takes each back, the newest first.
    cmd(&mut d, Key::Command('z'));
    d.wait(0.5);
    d.settle();
    assert_ne!(d.main.state.service_settings.as_ref().map(|s| s.upnp), upnp, "UPnP is back");
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("round")), "Corners wasn't the newest");
    d.front(Which::Main);
    cmd(&mut d, Key::Command('z'));
    d.wait(0.5);
    d.settle();
    assert_eq!(look(&d, Appearance::Light, "corners"), Some(Value::Choice("small")), "then Corners");
    // The undo showed Corners in the editor sheet, whose steps are its own while it's open.
    d.close_button(Which::Settings);
    d.settle();
    d.front(Which::Main);
    cmd(&mut d, Key::Command('z'));
    d.wait(0.5);
    d.settle();
    assert_eq!(keep_awake(&d), Some(false), "then Keep awake");
}

/// ⇧Return runs an action without leaving the palette, as it does a setting.
#[test]
fn shift_return_runs_an_action_and_stays() {
    let mut d = desk(with_an_album());
    d.settle();
    tap(&mut d.main, "rows/1");
    d.settle();
    search(&mut d, "pause");
    assert!(matches!(
        crate::model::palette::listed(&d.main.state)[0].kind,
        Kind::Action(crate::commands::Command::Torrents(OnTorrents::Pause))
    ));
    shift_return(&mut d);
    d.wait(0.5);
    assert!(palette_open(&d) && d.main.state.palette.query.text.is_empty());
    let paused = d.main.state.torrents.iter().find(|t| t.id == 1).map(|t| t.status.clone());
    assert_eq!(paused, Some(proto::Status::Paused), "it ran");
}

/// A pick that opens something leaves the palette on ⇧Return as on Return: a place, a typed value's
/// control, a command that asks for more.
#[test]
fn shift_return_on_what_opens_something_leaves_the_palette() {
    let mut d = desk_with_palette();
    search(&mut d, "network");
    shift_return(&mut d);
    assert!(!palette_open(&d));
    assert!(d.shown().settings);
    search(&mut d, "port");
    shift_return(&mut d);
    assert!(!palette_open(&d));
    search(&mut d, "corn");
    shift_return(&mut d);
    assert!(palette_open(&d), "a Choice setting lists its choices, as Return does");
    assert!(d.main.state.palette.sub.is_some());
}
