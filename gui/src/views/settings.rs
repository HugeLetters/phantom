//! The Settings window (#257), in tabs, which sit in its title bar: General (the default
//! destination, keeping the Mac awake), Network (the listening port, UPnP and the speed limits)
//! and Appearance (the Look Presets: a row for the Active look and one for each saved preset, whose
//! Edit… opens the one generic editor of a Look Preset's themes and settings in a sheet; then the
//! App icon's row, whose Edit… opens a sheet of every icon, each with ◐ ☀ ☾ chips). A tab shows at once: nothing on it
//! animates into place (#342).
//! Each setting's row is keyed by its name in the settings file, and is on exactly one tab
//! ([`tab_of`]).

use client::display;
use proto::{Direction, ListenPort, SeedingLimit, Settings};
use rune::{Cx, El, col, el, row};

use crate::changes::Target;
use crate::model::settings::presets;
use crate::model::settings::{
    AppIcon, Chip, Editor, Row, SettingsSheet, SettingsWindow, Side, Switch, Tab, add_image, chip_in_use, choose_chip, choose_folder,
    choose_image, choose_pair, choose_theme, clear_folder, close, close_icons, close_theme_menu, edit_icons, escape, icon_full_name,
    icon_name, pair_in_use, pick_port_automatically, remove_image, set_theme, show_tab, submit_colour, submit_limit, submit_port,
    submit_seeding, toggle, toggle_theme_menu,
};
use crate::model::settings::{Held, THEME, ThemeRow, ThemeRows, focused_side, join_row, split_row, theme_rows};
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::setting::{Kind, Value};
use crate::ui::{Halves, Icon, LookChip, Tone, Ui};

/// Every setting the sheet shows, by its name in its settings file.
pub const SETTINGS: [&str; 11] = [
    "default_destination",
    "keep_awake",
    "seed_ratio",
    "seed_time",
    "listen_port",
    "upnp",
    "download_limit",
    "upload_limit",
    "look_preset",
    "saved_presets",
    "app_icon",
];

/// Every field in the sheet: its key, how wide it is, and its tab. A field takes its width from
/// here ([`width_of`]); the layout check finds every field the sheet draws and fails on one not
/// listed here, so none goes unchecked.
pub const FIELDS: [(&str, f32, Tab); 5] = [
    ("seed-ratio-field", 84.0, Tab::General),
    ("seed-time-field", 100.0, Tab::General),
    ("port-field", 84.0, Tab::Network),
    ("download-limit-field", 120.0, Tab::Network),
    ("upload-limit-field", 120.0, Tab::Network),
];

/// How wide the field keyed `key` is.
fn width_of(key: &str) -> f32 {
    FIELDS.iter().find(|(k, ..)| *k == key).map(|(_, width, _)| *width).expect("every field is in FIELDS")
}

/// The tab a setting is on; a name of something in one (`look_preset.light.tone`), its tab.
pub fn tab_of(setting: &str) -> Option<Tab> {
    match setting_of(setting) {
        "default_destination" | "keep_awake" | "seed_ratio" | "seed_time" => Some(Tab::General),
        "listen_port" | "upnp" | "download_limit" | "upload_limit" => Some(Tab::Network),
        "look_preset" | "saved_presets" | "app_icon" => Some(Tab::Appearance),
        _ => None,
    }
}

/// The setting a reset name is in: `look_preset` for `look_preset.light.tone`.
pub fn setting_of(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// The window, in the look of the system's Appearance; empty while it's closed.
pub fn view(s: &SettingsWindow, cx: &mut Cx<SettingsWindow>) -> El {
    let appearance = Appearance::of(cx.system_dark());
    let look = s.looks().shown(appearance).clone();
    let ui = &look.ui;
    cx.help_look(ui.help_look());
    let Some(sheet) = &s.sheet else {
        return ui.root(ui.window_page("settings", "Settings", el(), None, |_: &mut SettingsWindow| {}), Vec::new());
    };
    // Read, it's keyed anew, a new modal to Rune, which then focuses the control asked for
    // (`first_focus`): before, the rows that hold it aren't there.
    let key = if sheet.service.is_some() || sheet.refused(Row::All).is_some() { "settings" } else { "settings-reading" };
    let labels = Tab::ALL.map(Tab::label);
    let selected = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap_or(0);
    let tabs = ui.tabs("settings-tabs", &labels, selected, |s: &mut SettingsWindow, i| show_tab(s, Tab::ALL[i]));
    // Rune dismisses a modal on Escape before the app sees the key, so the revert is tried here.
    let body = settings(s, sheet, ui, appearance);
    let page = ui.window_page(key, "Settings", tabs, Some(body), |s: &mut SettingsWindow| {
        if !escape(s) {
            close(s)
        }
    });
    let mut overlays = edit_sheet(s, sheet, ui);
    overlays.extend(icon_sheet(&s.app_icon, sheet, ui, appearance));
    ui.root(page, overlays)
}

fn settings(s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui, appearance: Appearance) -> El {
    let page = match (sheet.tab, &sheet.service) {
        (Tab::Appearance, _) => col()
            .w_full()
            .gap(12.0)
            .child(section(ui, display::setting_label(presets::SETTING), look_presets(s, sheet, ui, appearance)))
            .child(ui.card([app_icon(&s.app_icon, sheet, ui)])),
        (tab, Some(settings)) => ui.card(service_rows(tab, settings, s, sheet, ui)),
        (_, None) => match sheet.refused(Row::All) {
            Some(why) => ui.error_caption(why).key("settings-unread"),
            None => ui.caption("Reading the settings…").key("settings-reading"),
        },
    };
    // A tab lands at once: its rows and their dividers don't spring from where the last tab's
    // were, and, keyed by its tab, nothing of the last tab's is reused, so no colour eases from
    // what it was there either (#342).
    let page = col().key(format!("tab-page-{}", sheet.tab.label())).w_full().instant_changes().child(page);
    page.transition(rune::Transition::none())
}

fn service_rows(tab: Tab, settings: &Settings, s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
    let rows = SETTINGS.iter().filter(|name| tab_of(name) == Some(tab));
    rows.filter_map(|name| match *name {
        "default_destination" => Some(default_destination(settings, s.default_unavailable.as_ref(), sheet, ui)),
        "keep_awake" => Some(switch(Switch::KeepAwake, sheet, ui)),
        "listen_port" => Some(listen_port(settings, sheet, ui)),
        "upnp" => Some(switch(Switch::Upnp, sheet, ui)),
        "seed_ratio" => Some(seeding(SeedingLimit::Ratio, sheet, ui)),
        "seed_time" => Some(seeding(SeedingLimit::Time, sheet, ui)),
        "download_limit" => Some(limit(Direction::Download, sheet, ui)),
        "upload_limit" => Some(limit(Direction::Upload, sheet, ui)),
        _ => None,
    })
    .collect()
}

/// What the value means, under `row`; or, while it's refused, only why.
fn notes(sheet: &SettingsSheet, row: Row, meaning: Option<String>) -> Vec<(String, Tone)> {
    match sheet.refused(row) {
        Some(why) => vec![(why.to_owned(), Tone::Error)],
        None => meaning.map(|m| (m, Tone::Muted)).into_iter().collect(),
    }
}

/// The default folder, and under it why it can't be reached now, if it can't, worded as the torrent
/// rows word it (#209).
fn default_destination(settings: &Settings, unavailable: Option<&proto::Unavailable>, sheet: &SettingsSheet, ui: &Ui) -> El {
    let folder = match &settings.default_destination {
        Some(folder) => folder.display().to_string(),
        None => "None: every add asks where".into(),
    };
    let controls = row()
        .gap(6.0)
        .child(ui.button("choose-folder", "Choose…").first_focus(first(sheet, "default_destination")).on_press_tx(choose_folder).build())
        .child(ui.button("clear-folder", "Clear").disabled(settings.default_destination.is_none()).on_press(clear_folder).build());
    let mut notes = notes(sheet, Row::Folder, Some(folder));
    notes.extend(unavailable.filter(|_| settings.default_destination.is_some()).map(|why| (display::unavailable(why), Tone::Warning)));
    ui.setting_row("default_destination", display::setting_label("default_destination"), controls, notes)
}

fn listen_port(settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let field = ui
        .text_field("port-field", &sheet.port, display::setting_label("listen_port"))
        .on_edit(|s: &mut SettingsWindow| &mut s.sheet.as_mut().expect("the sheet is open").port)
        .on_submit(|s: &mut SettingsWindow, _| submit_port(s))
        .on_key(escape_key)
        .width(width_of("port-field"))
        .build();
    let mut controls = row().gap(6.0).center_y().child(field);
    if matches!(settings.listen_port, ListenPort::Pinned(_)) {
        controls = controls.child(ui.button("pick-port", "Pick automatically").on_press(pick_port_automatically).build());
    }
    let meaning = display::sentence(&display::listen_port(settings.listen_port, settings.listening_on));
    ui.setting_row("listen_port", display::setting_label("listen_port"), controls, notes(sheet, Row::Port, Some(meaning)))
}

/// Escape in a text setting's field being edited puts the stored value back, before the sheet's own
/// Escape (which closes it) is tried.
fn escape_key(s: &mut SettingsWindow, ev: &rune::KeyEvent, _: &mut rune::Tx) -> bool {
    ev.key == rune::Key::Escape && escape(s)
}

/// A speed limit's field: what's set, or empty for unlimited; Return or leaving it sends it.
fn limit(direction: Direction, sheet: &SettingsSheet, ui: &Ui) -> El {
    let name = crate::model::settings::limit_name(direction);
    let key = match direction {
        Direction::Download => "download-limit-field",
        Direction::Upload => "upload-limit-field",
    };
    let field = ui
        .text_field(key, sheet.limit(direction), display::setting_label(name))
        .placeholder("Unlimited")
        .on_edit(move |s: &mut SettingsWindow| s.sheet.as_mut().expect("the sheet is open").limit_mut(direction))
        .on_submit(move |s: &mut SettingsWindow, _| submit_limit(s, direction))
        .on_key(escape_key)
        .width(width_of(key))
        .build();
    let meaning = display::speed_limit_rule().to_owned();
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Limit(direction), Some(meaning)))
}

/// A seeding limit's field: what's set, or empty for never; Return or leaving it sends it.
fn seeding(at: SeedingLimit, sheet: &SettingsSheet, ui: &Ui) -> El {
    let name = crate::model::settings::seeding_name(at);
    let (key, meaning) = match at {
        SeedingLimit::Ratio => ("seed-ratio-field", display::seed_ratio_rule()),
        SeedingLimit::Time => ("seed-time-field", display::seed_time_rule()),
    };
    let field = ui
        .text_field(key, sheet.seeding(at), display::setting_label(name))
        .placeholder("Never")
        .on_edit(move |s: &mut SettingsWindow| s.sheet.as_mut().expect("the sheet is open").seeding_mut(at))
        .on_submit(move |s: &mut SettingsWindow, _| submit_seeding(s, at))
        .on_key(escape_key)
        .width(width_of(key))
        .build();
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Seeding(at), Some(meaning.to_owned())))
}

/// A setting that's on or off, and what it does.
fn switch(which: Switch, sheet: &SettingsSheet, ui: &Ui) -> El {
    let (name, label) = (which.name(), display::setting_label(which.name()));
    let on = sheet.switch(which).unwrap_or_default();
    let control = ui.switch(which.key(), label, on, first(sheet, name), move |s: &mut SettingsWindow| toggle(s, which));
    ui.setting_row(name, label, control, notes(sheet, Row::Switch(which), display::setting_note(name).map(str::to_owned)))
}

/// A section of a tab: `title` over `content`.
fn section(ui: &Ui, title: &str, content: El) -> El {
    col().w_full().gap(8.0).child(ui.heading(title)).child(content)
}

/// The editor of a Look Preset's theme settings, in two columns, ☀ Light and ☾ Dark
/// ([`ThemeRows`]), always named, each drawn in its half's look (`looks`, Light's then Dark's), as
/// if the system were set to that Appearance. With one theme in both halves, shared rows: its
/// Theme, a pop-up of every theme, then one for each of its settings; joined, one control across
/// both columns (one value, or theme, for both), drawn half in each look; split, one in each; its
/// Split or Join splits or joins it. With a different theme in each half, two lists side by side
/// that share no rows, each its half's Theme pop-up then its own theme's settings; choosing the
/// same theme in both brings the shared rows back. `first`: its first control takes the focus
/// as it opens; `focus`, the control of a setting an undo shows. A Theme pop-up's list, open,
/// goes into `layer`.
#[allow(clippy::too_many_arguments)]
fn editor(
    preset: &LookPreset,
    editor: &Editor,
    sheet: &SettingsSheet,
    ui: &Ui,
    looks: [&Ui; 2],
    first: bool,
    focus: Option<(&str, Side)>,
    layer: &mut Vec<El>,
) -> El {
    let rows = theme_rows(preset, editor);
    let look = |side: Side| match side {
        Side::Only(Appearance::Dark) => looks[1],
        Side::Both | Side::Only(Appearance::Light) => looks[0],
    };
    // Whether the control of `key` on `side`, of those on `sides`, takes the focus an undo gives.
    let wants = |key: &str, side: Side, sides: &[Side]| focus.is_some_and(|(k, s)| k == key && focused_side(sides, s) == Some(side));
    let theme_sides: &[Side] = match rows {
        ThemeRows::Same { theme_joined: true, .. } => &[Side::Both],
        _ => &[Side::Only(Appearance::Light), Side::Only(Appearance::Dark)],
    };
    let themes = crate::theme::themes();
    let names: Vec<&str> = themes.iter().map(|t| t.name()).collect();
    // A Theme pop-up of `side`, in the look `ui`.
    let pop_up = |side: Side, ui: &Ui, layer: &mut Vec<El>| {
        let theme = preset.get(side.appearances()[0]).theme;
        let current = themes.iter().position(|t| t.id() == theme.id()).unwrap_or(0);
        ui.popup_button(
            format!("{EDIT}-theme-{}", side.key()),
            "Theme",
            &names,
            current,
            editor.menu == Some(side),
            (first && side != Side::Only(Appearance::Dark)) || wants(THEME, side, theme_sides),
            move |s: &mut SettingsWindow| toggle_theme_menu(s, side),
            close_theme_menu,
            move |s: &mut SettingsWindow, i| choose_theme(s, side, crate::theme::themes()[i].id()),
            layer,
        )
    };
    let theme_joined = matches!(rows, ThemeRows::Same { theme_joined: true, .. });
    let page = col().w_full().gap(4.0);
    match rows {
        ThemeRows::Same { rows, .. } => {
            let theme = match theme_joined {
                true => Halves::Joined(pop_up(Side::Both, looks[0], layer)),
                false => Halves::Split(
                    Some(pop_up(Side::Only(Appearance::Light), looks[0], layer)),
                    Some(pop_up(Side::Only(Appearance::Dark), looks[1], layer)),
                ),
            };
            let toggle = join_control(looks[1], THEME, "Theme", theme_joined);
            let theme = ui.halves_row(&format!("{EDIT}-theme"), "Theme", theme, looks, Some(toggle), Vec::new());
            let header = ui.halves_header(looks[0].half_heading(false), looks[1].half_heading(true), looks);
            let mut lines = vec![header, theme];
            lines.extend(rows.iter().map(|row| {
                let sides: &[Side] =
                    if row.joined { &[Side::Both] } else { &[Side::Only(Appearance::Light), Side::Only(Appearance::Dark)] };
                same_row(row, editor, sheet, ui, looks, |side| wants(row.setting.key, side, sides))
            }));
            page.child(ui.halves_card(lines, looks))
        }
        ThemeRows::Apart { light, dark } => {
            let mut column = |side: Side, held: &[Held]| {
                let ui = look(side);
                let heading = ui.half_heading(side == Side::Only(Appearance::Dark));
                let theme = ui.setting_row(&format!("{EDIT}-theme-{}-row", side.key()), "Theme", pop_up(side, ui, layer), Vec::new());
                let lines = held.iter().map(|(setting, value)| {
                    let control = theme_control(setting, side, *value, editor, ui, wants(setting.key, side, &[side]));
                    let notes =
                        sheet.refused(Row::Colour(side, setting.key)).map(|why| (why.to_owned(), Tone::Error)).into_iter().collect();
                    ui.setting_row(&format!("{EDIT}-{}-{}-row", setting.key, side.key()), setting.label, control, notes)
                });
                ui.card([heading, theme].into_iter().chain(lines).collect::<Vec<_>>()).grow(1.0).min_w(0.0)
            };
            let light = column(Side::Only(Appearance::Light), &light);
            let dark = column(Side::Only(Appearance::Dark), &dark);
            page.child(row().key("halves-columns").w_full().gap(8.0).items_start().child(light).child(dark))
        }
    }
}

/// A row's Join or Split, keyed `edit-<key>-split` while it's joined and `edit-<key>-join` while
/// it's split.
fn join_control(ui: &Ui, key: &'static str, label: &str, joined: bool) -> El {
    let (what, shown, accessible) = match joined {
        true => ("split", "Split", format!("{label} for each Appearance")),
        false => ("join", "Join", format!("One {label} for both Appearances")),
    };
    ui.join_toggle(format!("{EDIT}-{key}-{what}"), shown, &accessible, move |s: &mut SettingsWindow| match joined {
        true => split_row(s, key),
        false => join_row(s, key),
    })
}

/// A setting's row, one theme in both halves: one control across both columns, drawn in both
/// looks, or one in each, in its half's; its Split or Join; and why a colour typed there was
/// refused.
fn same_row(row: &ThemeRow, editor: &Editor, sheet: &SettingsSheet, ui: &Ui, looks: [&Ui; 2], focus: impl Fn(Side) -> bool) -> El {
    let (setting, key) = (row.setting, row.setting.key);
    let (light, dark) = (Side::Only(Appearance::Light), Side::Only(Appearance::Dark));
    let (cells, sides) = match row.joined {
        true => {
            let cells = Halves::Joined(theme_control(setting, Side::Both, row.light, editor, looks[0], focus(Side::Both)));
            (cells, vec![Side::Both])
        }
        false => {
            let cells = Halves::Split(
                Some(theme_control(setting, light, row.light, editor, looks[0], focus(light))),
                Some(theme_control(setting, dark, row.dark, editor, looks[1], focus(dark))),
            );
            (cells, vec![light, dark])
        }
    };
    let notes = sides
        .iter()
        .filter_map(|&side| {
            let why = sheet.refused(Row::Colour(side, key))?;
            Some((if row.joined { why.to_owned() } else { format!("{}: {why}", side_label(side)) }, Tone::Error))
        })
        .collect();
    ui.halves_row(
        &format!("{EDIT}-{key}"),
        setting.label,
        cells,
        looks,
        Some(join_control(looks[1], key, setting.label, row.joined)),
        notes,
    )
}

/// "Light" or "Dark", for a note under a split row.
fn side_label(side: Side) -> &'static str {
    match side {
        Side::Both => "Both",
        Side::Only(appearance) => appearance.label(),
    }
}

/// What the sheet's controls' keys start with: `edit-tone-light`.
const EDIT: &str = "edit";

/// Save as…'s line under the Active look's row: nothing; open, the name field and Save, a blank
/// name's refusal under them; for a name a saved preset has, the question whether to replace it.
fn save_as(sheet: &SettingsSheet, ui: &Ui) -> Option<El> {
    let save_as = sheet.save_as.as_ref()?;
    // As tall asking Replace? as with its field, so the list under it stays put (#343).
    let line = row().key("save-as").w_full().min_h(SAVE_AS_HEIGHT).gap(6.0).center_y().justify_end().px(8.0).pb(4.0);
    if let Some(name) = &save_as.replace {
        let ask = ui.caption(display::replace_preset(name.as_str())).key("save-as-replace-ask");
        let replace = ui.button(presets::REPLACE, "Replace").first_focus(first(sheet, presets::REPLACE)).on_press(presets::replace).build();
        let cancel = ui.button("save-as-cancel", "Cancel").on_press(presets::cancel_replace).build();
        return Some(line.child(ask).child(replace).child(cancel));
    }
    let field = ui
        .text_field("save-as-field", &save_as.name, "Name of the Look Preset")
        .placeholder("Name")
        .on_edit(|s: &mut SettingsWindow| &mut s.sheet.as_mut().and_then(|sheet| sheet.save_as.as_mut()).expect("Save as… is open").name)
        .on_submit(presets::save)
        .on_key(escape_key)
        .width(SAVE_AS_WIDTH)
        .build();
    let save = ui.button("save-as-save", "Save").on_press_tx(presets::save).build();
    let mut open = col().w_full().gap(4.0).items_end().child(line.child(field).child(save));
    if let Some(why) = sheet.refused(Row::PresetName) {
        open = open.child(ui.error_caption(why).key("save-as-refused"));
    }
    Some(open)
}

/// The control that sets `setting` to a value of its kind on `side`, showing `value`, keyed
/// `edit-<key>-<side>`. `focus` makes a toggle or a choice first in the Tab order; a colour's
/// field takes focus through its text.
fn theme_control(setting: &'static crate::theme::setting::Setting, side: Side, value: Value, editor: &Editor, ui: &Ui, focus: bool) -> El {
    let key = setting.key;
    let control_key = format!("{EDIT}-{key}-{}", side.key());
    match (&setting.kind, value) {
        (Kind::Choice { choices, .. }, Value::Choice(id)) => {
            let labels: Vec<&str> = choices.iter().map(|c| c.label).collect();
            let selected = choices.iter().position(|c| c.id == id).unwrap_or(0);
            ui.choice(control_key, setting.label, &labels, selected, focus, move |s: &mut SettingsWindow, i| {
                set_theme(s, side, key, Value::Choice(choices[i].id))
            })
        }
        (Kind::Toggle { .. }, Value::Toggle(on)) => {
            ui.switch(control_key, setting.label, on, focus, move |s: &mut SettingsWindow| set_theme(s, side, key, Value::Toggle(!on)))
        }
        (Kind::Colour { .. }, Value::Colour(rgb)) => {
            let Some(field) = editor.colour(side, key) else { return el() };
            let text = ui
                .text_field(control_key, &field.text, setting.label)
                .on_edit(move |s: &mut SettingsWindow| s.sheet.as_mut().expect("the sheet is open").colour_edit(side, key))
                .on_submit(move |s: &mut SettingsWindow, _| submit_colour(s, side, key))
                .on_key(escape_key)
                .width(COLOUR_WIDTH)
                .build();
            row().gap(6.0).center_y().child(ui.swatch(rgb.color())).child(text)
        }
        // A value is always of its setting's kind (`Values`).
        (Kind::Choice { .. } | Kind::Toggle { .. } | Kind::Colour { .. }, _) => el(),
    }
}

/// The Look Presets: the Active look's row, then a row for each saved one, in the order they were
/// saved, or, with none, a muted "None saved". A row is the name, chips for what its halves look
/// like (a saved preset's ☀ and ☾ apply that half to the Active look, its ◐ both; one that's in
/// use, value for value, is outlined in its accent and a press does nothing) and its buttons: the
/// Active look's chips open its Edit…, and it has Edit… and Save as…; a saved preset's, Edit… and
/// Delete. `appearance` is the one the window shows.
fn look_presets(s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui, appearance: Appearance) -> El {
    let looks = s.looks();
    let active = looks.preset();
    let colours = |half: Appearance, preset: &LookPreset| {
        let look = preset.get(half);
        let c = look.theme.build(&look.settings).tokens.c;
        (c.page, c.text_base, c.bg_accent)
    };
    let mut rows = vec![active_row(active, sheet, ui, &colours)];
    rows.extend(save_as(sheet, ui));
    // The first chip takes the focus as the section does: an undo's, or the first preset's ◐.
    let wanted = match (&sheet.chip, first(sheet, presets::SETTING)) {
        (Some(chip), true) if s.saved.get(&chip.name).is_some() => Some(chip.clone()),
        (_, true) => s.saved.list().first().map(|p| Chip { name: p.name.clone(), half: None }),
        (_, false) => None,
    };
    let saved = s.saved.list().iter().enumerate().map(|(i, saved)| {
        let name = saved.name.as_str();
        let chip = |half: Option<Appearance>| {
            let halves = half.map_or(Appearance::BOTH.to_vec(), |h| vec![h]);
            let in_use = halves.iter().all(|h| saved.preset.get(*h) == active.get(*h));
            let shown: Vec<_> = halves.iter().map(|h| colours(*h, &saved.preset)).collect();
            let help = match (half, in_use) {
                (None, false) => display::apply_both(name),
                (None, true) => display::both_in_use(name),
                (Some(half), false) => display::apply_half(name, half.label()),
                (Some(half), true) => display::half_in_use(name, half.label()),
            };
            let accent = colours(half.unwrap_or(appearance), &saved.preset).2;
            let glyph = match half {
                None => Icon::Both,
                Some(Appearance::Light) => Icon::Light,
                Some(Appearance::Dark) => Icon::Dark,
            };
            let chip = LookChip {
                glyph,
                pages: shown.iter().map(|c| c.0).collect(),
                texts: shown.iter().map(|c| c.1).collect(),
                accent,
                in_use,
                help,
            };
            let focus = wanted.as_ref().is_some_and(|w| w.name == saved.name && w.half == half);
            let key = format!("preset-{i}-{}", half.map_or("both", Appearance::key));
            let apply: Box<dyn Fn(&mut SettingsWindow)> = Box::new(move |s| presets::apply(s, i, half));
            ui.look_chip(key, chip, focus, Some(apply))
        };
        let button = |what: &str, label: &str, accessible: String| ui.button(format!("preset-{i}-{what}"), label).accessible(&accessible);
        let edit = button("edit", "Edit…", format!("Edit {name}")).on_press(move |s: &mut SettingsWindow| presets::edit(s, i)).build();
        let delete = button("delete", "Delete", format!("Delete {name}"))
            .fill()
            .on_press(move |s: &mut SettingsWindow| presets::delete(s, i))
            .build();
        let (light, dark) = (chip(Some(Appearance::Light)), chip(Some(Appearance::Dark)));
        ui.preset_row(&format!("preset-{i}"), name, Some(chip(None)), light, dark, edit, delete)
    });
    rows.extend(saved);
    if s.saved.list().is_empty() {
        rows.push(ui.muted_row("none-saved", "None saved"));
    }
    col().key(presets::SETTING).w_full().gap(8.0).child(ui.card(rows))
}

/// The Active look's row: its chips, which open its Edit…, Edit… and Save as….
fn active_row(
    active: &LookPreset,
    sheet: &SettingsSheet,
    ui: &Ui,
    colours: &dyn Fn(Appearance, &LookPreset) -> (rune::Color, rune::Color, rune::Color),
) -> El {
    let [light, dark] = Appearance::BOTH.map(|half| {
        let (page, text, accent) = colours(half, active);
        let glyph = if half == Appearance::Light { Icon::Light } else { Icon::Dark };
        let help = display::edit_active_half(half.label());
        let chip = LookChip { glyph, pages: vec![page], texts: vec![text], accent, in_use: false, help };
        let edit: Box<dyn Fn(&mut SettingsWindow)> = Box::new(presets::edit_active);
        ui.look_chip::<SettingsWindow>(format!("active-{}", half.key()), chip, false, Some(edit))
    });
    let edit = ui
        .button("active-edit", "Edit…")
        .accessible("Edit the active look")
        .first_focus(first(sheet, "look_preset"))
        .on_press(presets::edit_active)
        .build();
    let save = ui.button("save-as-open", "Save as…").fill().disabled(sheet.save_as.is_some()).on_press(presets::save_as).build();
    ui.preset_row("look_preset", "Active look", None, light, dark, edit, save)
}

/// The sheet over Settings of the Look Preset being edited: its name ("Active look" for the Active
/// one), the editor, and Done; then the Theme pop-up's list, while it's open.
fn edit_sheet(s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
    let Some(open) = &sheet.edit else { return Vec::new() };
    let (title, preset) = match &open.target {
        Target::Active => ("Active look".to_owned(), s.looks().preset().clone()),
        Target::Saved(name) => match s.saved.get(name) {
            Some(saved) => (name.as_str().to_owned(), saved.preset.clone()),
            None => return Vec::new(),
        },
    };
    let mut menu = Vec::new();
    // Both windows show the preset being edited, so each half's look is the one it shows.
    let looks = s.looks();
    let [light, dark] = Appearance::BOTH.map(|a| looks.shown(a).clone());
    let body = self::editor(&preset, &open.editor, sheet, ui, [&light.ui, &dark.ui], open.focus.is_none(), open.focus, &mut menu);
    let mut layers = ui.over_sheet(
        EDIT,
        &title,
        body,
        |s: &mut SettingsWindow| {
            escape(s);
        },
        Some(Box::new(presets::cancel)),
        presets::close,
    );
    layers.extend(menu);
    layers
}

/// How wide Save as…'s field is. It's not in [`FIELDS`]: it shows only while Save as… is open.
const SAVE_AS_WIDTH: f32 = 160.0;

/// How tall Save as…'s line is, its field or its question: a field and the line's padding under it.
const SAVE_AS_HEIGHT: f32 = 32.0;

/// How wide a colour's field is: `#rrggbb`.
const COLOUR_WIDTH: f32 = 84.0;

/// The App icon's row, one line: what it is, the icon each Appearance shows now, small (both,
/// each with a ☀ or ☾ at its corner, while they differ; one while they're the same), and Edit…,
/// which opens its sheet.
fn app_icon(icon: &AppIcon, sheet: &SettingsSheet, ui: &Ui) -> El {
    let label = display::setting_label("app_icon");
    if icon.manifest.is_none() {
        let unread = vec![("The icons can't be read".to_owned(), Tone::Error)];
        return ui.setting_row("app_icon", label, el(), unread);
    }
    let [dark, light] = icon.sides().unwrap_or_default();
    let shown: Vec<(Option<&rune::Image>, Option<bool>)> = match dark == light {
        true => vec![(icon.image(&light), None)],
        false => vec![(icon.image(&light), Some(false)), (icon.image(&dark), Some(true))],
    };
    let preview = ui.icon_preview(&shown, chip_faces());
    let edit = ui
        .button("app-icon-edit", "Edit…")
        .accessible("Edit the App icon")
        .first_focus(first(sheet, "app_icon"))
        .on_press(edit_icons)
        .build();
    // One line: the caption (whole on hover), the previews, Edit….
    let caption = ui.caption(icon_name(icon, icon.choice.as_ref())).key("app_icon-note").help(icon_full_name(icon, icon.choice.as_ref()));
    let controls = row().gap(8.0).center_y().child(caption).child(preview).child(edit);
    ui.setting_row("app_icon", label, controls, Vec::new())
}

/// The faces of the App icon's chips and marks, `(page, text)` of ☀ then ☾: a fresh install's Light
/// look and Dark one, whatever the Active look is.
fn chip_faces() -> [(rune::Color, rune::Color); 2] {
    let fresh = LookPreset::fresh();
    Appearance::BOTH.map(|a| {
        let c = fresh.get(a).theme.build(&fresh.get(a).settings).tokens.c;
        (c.page, c.text_base)
    })
}

/// The App icon's sheet (#87): Pairs, a tile for each designed pair, its two icons split on the
/// diagonal, which a press applies (both halves), outlined while both halves are its icons; then
/// Icons, a tile for Follow theme, every icon there is and each image added, each with ◐ ☀ ☾
/// under it ([`chip_choice`](crate::model::settings::chip_choice), [`chip_in_use`]), ☀ drawn light and ☾ dark; then "+ Add image…". An
/// added image's tile has an × on hover. An image file dropped on the sheet is added too. Done
/// closes it.
fn icon_sheet(icon: &AppIcon, sheet: &SettingsSheet, ui: &Ui, appearance: Appearance) -> Vec<El> {
    if sheet.icons.is_none() {
        return Vec::new();
    }
    let Some(manifest) = &icon.manifest else { return Vec::new() };
    let name = |id: &str| icon.name(id);
    let faces = chip_faces();
    let pairs = manifest.pairs().iter().enumerate().map(|(i, pair)| {
        let help = display::use_icon_pair(&name(&pair.light), &name(&pair.dark));
        ui.pair_tile(
            format!("icon-pair-{i}"),
            icon.image(&pair.light),
            icon.image(&pair.dark),
            &pair.name,
            &help,
            pair_in_use(icon, i),
            move |s: &mut SettingsWindow| choose_pair(s, i),
        )
    });
    let tile = |key: String, id: Option<&str>, image: Option<&rune::Image>, label: &str, help: &str, remove: Option<El>| {
        let shown = id.map_or_else(|| "the theme's icon".to_owned(), name);
        let chips = [None, Some(Appearance::Light), Some(Appearance::Dark)].map(|half| {
            let in_use = chip_in_use(icon, id, half);
            let help = match (half, id) {
                (None, None) => display::follow_theme_help().to_owned(),
                (None, Some(_)) => display::use_icon(&shown, "Light and Dark"),
                (Some(half), _) => display::use_icon(&shown, half.label()),
            };
            let id = id.map(str::to_owned);
            ui.icon_chip(
                format!("{key}-{}", half.map_or("both", Appearance::key)),
                half.map(|h| h == Appearance::Dark),
                faces,
                in_use,
                help,
                move |s: &mut SettingsWindow| choose_chip(s, id.as_deref(), half),
            )
        });
        ui.icon_tile(key, image, label, help, chips.to_vec(), remove)
    };
    let mut tiles = vec![tile(
        "icon-follow".into(),
        None,
        icon.image(&icon.theme_icon(appearance == Appearance::Dark).unwrap_or_else(|| manifest.default().id.clone())),
        "Follow theme",
        "The icon of the look each Appearance shows",
        None,
    )];
    for one in manifest.icons() {
        tiles.push(tile(format!("icon-{}", one.id), Some(&one.id), icon.image(&one.id), &one.name, &one.name, None));
    }
    for (i, id) in icon.added.iter().enumerate() {
        let gone = id.clone();
        let remove = ui
            .button(format!("icon-image-{i}-remove"), "×")
            .ghost()
            .accessible(&format!("Remove {}", name(id)))
            .on_press(move |s: &mut SettingsWindow| remove_image(s, &gone))
            .build();
        tiles.push(tile(format!("icon-image-{i}"), Some(id), icon.image(id), &name(id), &name(id), Some(remove)));
    }
    tiles.push(ui.add_tile("icon-add", choose_image));
    let mut body = col()
        .key("app_icon_tiles")
        .w_full()
        .gap(8.0)
        .child(ui.heading("Pairs"))
        .child(ui.tile_grid(pairs.collect()))
        .child(ui.heading("Icons"))
        .child(ui.tile_grid(tiles))
        .on_drop_files(|s: &mut SettingsWindow, files, _| files.iter().for_each(|f| add_image(s, f)))
        .accepts_extensions(&crate::ui::IMAGES);
    if let Some(why) = sheet.refused(Row::Image) {
        body = body.child(ui.error_caption(why).key("image-refused"));
    }
    ui.over_sheet(
        "icons",
        display::setting_label("app_icon"),
        body,
        |s: &mut SettingsWindow| {
            escape(s);
        },
        None,
        close_icons,
    )
}

/// Whether `setting`'s control takes focus as the sheet opens.
fn first(sheet: &SettingsSheet, setting: &str) -> bool {
    sheet.focus == Some(setting)
}
