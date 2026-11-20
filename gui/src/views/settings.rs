//! The Settings window (#257), in tabs: General (the default destination, keeping the Mac awake), Network
//! (the listening port, UPnP and the speed limits) and Appearance (the Look Presets: a row for the
//! Active look and one for each saved preset, whose Edit… opens the one generic editor of a Look
//! Preset's themes and settings in a sheet; then the App icon section).
//! Each setting's row is keyed by its name in the settings file, and is on exactly one tab
//! ([`tab_of`]).

use client::display;
use proto::{Direction, ListenPort, SeedingLimit, Settings};
use rune::{Cx, El, col, el, row};

use crate::changes::Target;
use crate::model::settings::presets;
use crate::model::settings::{
    AppIcon, Chip, Editor, Row, SettingsSheet, SettingsWindow, Side, Switch, Tab, ThemeField, choose_folder, choose_icon, choose_side,
    clear_folder, close, escape, pick_port_automatically, set_theme, show_tab, split_theme, submit_colour, submit_limit, submit_port,
    submit_seeding, theme_groups, toggle, toggle_custom_pair,
};
use crate::platform::app_icon::Icons;
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::setting::{Kind, Value};
use crate::ui::{Icon, LookChip, Tone, Ui};

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
    let Some(sheet) = &s.sheet else {
        return ui.root(ui.window_page("settings", "Settings", 0.0, None, |_: &mut SettingsWindow| {}), Vec::new());
    };
    // Read, it's keyed anew, a new modal to Rune, which then focuses the control asked for
    // (`first_focus`): before, the rows that hold it aren't there.
    let key = if sheet.service.is_some() || sheet.refused(Row::All).is_some() { "settings" } else { "settings-reading" };
    // Rune dismisses a modal on Escape before the app sees the key, so the revert is tried here.
    let page = ui.window_page(key, "Settings", cx.titlebar_inset(), Some(settings(s, sheet, ui, appearance)), |s: &mut SettingsWindow| {
        if !escape(s) {
            close(s)
        }
    });
    ui.root(page, edit_sheet(s, sheet, ui))
}

fn settings(s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui, appearance: Appearance) -> El {
    let labels = Tab::ALL.map(Tab::label);
    let selected = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap_or(0);
    let body =
        col().w_full().gap(12.0).child(ui.tabs("settings-tabs", &labels, selected, |s: &mut SettingsWindow, i| show_tab(s, Tab::ALL[i])));
    let page = match (sheet.tab, &sheet.service) {
        (Tab::Appearance, _) => col()
            .w_full()
            .gap(12.0)
            .child(section(ui, display::setting_label(presets::SETTING), look_presets(s, sheet, ui, appearance)))
            .child(section(ui, "App icon", app_icon(&s.app_icon, sheet, ui))),
        (tab, Some(settings)) => ui.card(service_rows(tab, settings, s, sheet, ui)),
        (_, None) => match sheet.refused(Row::All) {
            Some(why) => ui.error_caption(why).key("settings-unread"),
            None => ui.caption("Reading the settings…").key("settings-reading"),
        },
    };
    body.child(page)
}

fn service_rows(tab: Tab, settings: &Settings, s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
    let rows = SETTINGS.iter().filter(|name| tab_of(name) == Some(tab));
    rows.filter_map(|name| match *name {
        "default_destination" => Some(default_destination(settings, s.default_unavailable.as_ref(), sheet, ui)),
        "keep_awake" => Some(switch(Switch::KeepAwake, sheet, ui)),
        "listen_port" => Some(listen_port(settings, sheet, ui)),
        "upnp" => Some(switch(Switch::Upnp, sheet, ui)),
        "seed_ratio" => Some(seeding(SeedingLimit::Ratio, settings, sheet, ui)),
        "seed_time" => Some(seeding(SeedingLimit::Time, settings, sheet, ui)),
        "download_limit" => Some(limit(Direction::Download, settings, sheet, ui)),
        "upload_limit" => Some(limit(Direction::Upload, settings, sheet, ui)),
        _ => None,
    })
    .collect()
}

/// The refusal under `row`, if any, after what the value means.
fn notes(sheet: &SettingsSheet, row: Row, meaning: Option<String>) -> Vec<(String, Tone)> {
    let refused = sheet.refused(row).map(|why| (why.to_owned(), Tone::Error));
    meaning.map(|m| (m, Tone::Muted)).into_iter().chain(refused).collect()
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
fn limit(direction: Direction, settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
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
    let meaning = display::speed_limit_meaning(settings.speed_limits.get(direction));
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Limit(direction), Some(meaning)))
}

/// A seeding limit's field: what's set, or empty for never; Return or leaving it sends it.
fn seeding(at: SeedingLimit, settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let name = crate::model::settings::seeding_name(at);
    let (key, meaning) = match at {
        SeedingLimit::Ratio => ("seed-ratio-field", display::seed_ratio_meaning(settings.seeding.ratio)),
        SeedingLimit::Time => ("seed-time-field", display::seed_time_meaning(settings.seeding.time)),
    };
    let field = ui
        .text_field(key, sheet.seeding(at), display::setting_label(name))
        .placeholder("Never")
        .on_edit(move |s: &mut SettingsWindow| s.sheet.as_mut().expect("the sheet is open").seeding_mut(at))
        .on_submit(move |s: &mut SettingsWindow, _| submit_seeding(s, at))
        .on_key(escape_key)
        .width(width_of(key))
        .build();
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Seeding(at), Some(meaning)))
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

/// The editor of a Look Preset's theme settings: the theme each Appearance shows and its settings,
/// one card with the same theme in both, each field one control for both or, split, one for each
/// Appearance; else a card for each. `first`: its first control takes the focus as it opens;
/// `focus`, the control of a setting an undo shows.
fn editor(preset: &LookPreset, editor: &Editor, sheet: &SettingsSheet, ui: &Ui, first: bool, focus: Option<&str>) -> El {
    let mut first = first;
    let mut page = col().w_full().gap(8.0);
    for group in theme_groups(preset, &editor.split) {
        let side = group.appearance.map_or(Side::Both, Side::Only);
        let mut rows = vec![ui.setting_row(&format!("{EDIT}-theme-{}", side.key()), "Theme", ui.caption(group.theme.name()), Vec::new())];
        for field in &group.fields {
            let focus = std::mem::take(&mut first) || focus == Some(field.setting.key);
            rows.push(theme_field(field, group.appearance.is_none(), editor, sheet, ui, focus));
        }
        if let Some(appearance) = group.appearance {
            page = page.child(ui.caption(format!("In {}", appearance.label())));
        }
        page = page.child(ui.card(rows));
    }
    page
}

/// What the sheet's controls' keys start with: `edit-tone-light`.
const EDIT: &str = "edit";

/// Save as…'s line under the Active look's row: nothing; open, the name field and Save, a blank
/// name's refusal under them; for a name a saved preset has, the question whether to replace it.
fn save_as(sheet: &SettingsSheet, ui: &Ui) -> Option<El> {
    let save_as = sheet.save_as.as_ref()?;
    let line = row().key("save-as").w_full().gap(6.0).center_y().justify_end().px(8.0).pb(4.0);
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

/// A theme setting's row: its control for both Appearances, with Split, or one for each, with
/// Join once they agree when they're one theme's (`joinable`).
fn theme_field(field: &ThemeField, joinable: bool, editor: &Editor, sheet: &SettingsSheet, ui: &Ui, focus: bool) -> El {
    let setting = field.setting;
    let key = setting.key;
    let mut notes = Vec::new();
    let controls = field.sides.iter().enumerate().map(|(i, &(side, value))| {
        let control = theme_control(setting, side, value, editor, ui, focus && i == 0);
        if let Some(why) = sheet.refused(Row::Colour(side, key)) {
            notes.push((if field.split() { format!("{}: {why}", side_label(side)) } else { why.to_owned() }, Tone::Error));
        }
        match side {
            Side::Both => control,
            Side::Only(_) if !joinable => control,
            Side::Only(_) => row().gap(8.0).center_y().child(ui.caption(format!("In {}", side_label(side)))).child(control),
        }
    });
    let controls: Vec<El> = controls.collect();
    let agree = field.sides.windows(2).all(|w| w[0].1 == w[1].1);
    let button = match (joinable, field.split()) {
        (false, _) => None,
        (true, false) => Some(
            ui.button(format!("{EDIT}-{key}-split"), "Split")
                .accessible(&format!("{} for each Appearance", setting.label))
                .on_press(move |s: &mut SettingsWindow| split_theme(s, key, true)),
        ),
        (true, true) if agree => Some(
            ui.button(format!("{EDIT}-{key}-join"), "Join")
                .accessible(&format!("One {} for both Appearances", setting.label))
                .on_press(move |s: &mut SettingsWindow| split_theme(s, key, false)),
        ),
        (true, true) => None,
    };
    let control = row().gap(6.0).center_y().child(col().gap(4.0).items_end().children(controls)).children(button.map(|b| b.build()));
    ui.setting_row(&format!("{EDIT}-{key}"), setting.label, control, notes)
}

/// "Light" or "Dark".
fn side_label(side: Side) -> &'static str {
    match side {
        Side::Both => "Both",
        Side::Only(appearance) => appearance.label(),
    }
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
/// saved. A row is the name, chips for what its halves look like (a saved preset's ☀ and ☾ apply
/// that half to the Active look, its ◐ both; one that's in use, value for value, is outlined in its
/// accent and a press does nothing) and its buttons: the Active look's chips only show, and it has
/// Edit… and Save as…; a saved preset's, Edit… and Delete. `appearance` is the one the window shows.
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
        let chips = vec![chip(None), chip(Some(Appearance::Light)), chip(Some(Appearance::Dark))];
        let button = |what: &str, label: &str, accessible: String| ui.button(format!("preset-{i}-{what}"), label).accessible(&accessible);
        let buttons = vec![
            button("edit", "Edit…", format!("Edit {name}")).on_press(move |s: &mut SettingsWindow| presets::edit(s, i)).build(),
            button("delete", "Delete", format!("Delete {name}")).on_press(move |s: &mut SettingsWindow| presets::delete(s, i)).build(),
        ];
        ui.preset_row(&format!("preset-{i}"), name, chips, buttons)
    });
    rows.extend(saved);
    let section = col().key(presets::SETTING).w_full().gap(8.0).child(ui.card(rows));
    if s.saved.list().is_empty() { section.child(ui.caption("None saved")) } else { section }
}

/// The Active look's row: its chips, which only show its halves, Edit… and Save as….
fn active_row(
    active: &LookPreset,
    sheet: &SettingsSheet,
    ui: &Ui,
    colours: &dyn Fn(Appearance, &LookPreset) -> (rune::Color, rune::Color, rune::Color),
) -> El {
    let chips = Appearance::BOTH
        .map(|half| {
            let (page, text, accent) = colours(half, active);
            let glyph = if half == Appearance::Light { Icon::Light } else { Icon::Dark };
            let chip =
                LookChip { glyph, pages: vec![page], texts: vec![text], accent, in_use: false, help: display::active_half(half.label()) };
            ui.look_chip::<SettingsWindow>(format!("active-{}", half.key()), chip, false, None)
        })
        .to_vec();
    let edit = ui
        .button("active-edit", "Edit…")
        .accessible("Edit the active look")
        .first_focus(first(sheet, "look_preset"))
        .on_press(presets::edit_active)
        .build();
    let save = ui.button("save-as-open", "Save as…").disabled(sheet.save_as.is_some()).on_press(presets::save_as).build();
    ui.preset_row("look_preset", "Active look", chips, vec![edit, save])
}

/// The sheet over Settings of the Look Preset being edited: its name ("Active look" for the Active
/// one), the editor, and Done.
fn edit_sheet(s: &SettingsWindow, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
    let Some(open) = &sheet.edit else { return Vec::new() };
    let (title, preset) = match &open.target {
        Target::Active => ("Active look".to_owned(), s.looks().preset().clone()),
        Target::Saved(name) => match s.saved.get(name) {
            Some(saved) => (name.as_str().to_owned(), saved.preset.clone()),
            None => return Vec::new(),
        },
    };
    let body = self::editor(&preset, &open.editor, sheet, ui, open.focus.is_none(), open.focus);
    ui.over_sheet(
        EDIT,
        &title,
        body,
        |s: &mut SettingsWindow| {
            escape(s);
        },
        presets::close,
    )
}

/// How wide Save as…'s field is. It's not in [`FIELDS`]: it shows only while Save as… is open.
const SAVE_AS_WIDTH: f32 = 160.0;

/// How wide a colour's field is: `#rrggbb`.
const COLOUR_WIDTH: f32 = 84.0;

/// Follow theme, each designed pair and each icon alone, in the manifest's order; then the custom
/// pair: one chosen shows as its own tile, and its rows pick each side.
fn app_icon(icon: &AppIcon, sheet: &SettingsSheet, ui: &Ui) -> El {
    let Some(manifest) = &icon.manifest else {
        let unread = vec![("The icons can't be read".to_owned(), Tone::Error)];
        return ui.card([ui.setting_row("app_icon", display::setting_label("app_icon"), el(), unread)]);
    };
    let image = |id: &str| icon.image(id);
    let chosen = |icons: &Option<Icons>| icon.choice == *icons;
    let pick = |icons: Option<Icons>| move |s: &mut SettingsWindow| choose_icon(s, icons.clone());
    let follow = image(&manifest.default().id);
    let focus = first(sheet, "app_icon");
    let mut pairs = vec![ui.icon_tile("icon-follow".into(), &[follow], "Follow theme", chosen(&None), focus, pick(None))];
    for (i, pair) in manifest.pairs().iter().enumerate() {
        let icons = Some(pair.icons());
        let sides = [image(&pair.dark), image(&pair.light)];
        pairs.push(ui.icon_tile(format!("icon-pair-{i}"), &sides, &pair.name, chosen(&icons), focus, pick(icons)));
    }
    let designed = |icons: &Icons| manifest.pairs().iter().any(|p| p.icons() == *icons);
    if let Some(custom @ Icons::Pair { dark, light }) = &icon.choice
        && !designed(custom)
    {
        let name = |id: &str| manifest.icon(id).map_or(id.to_owned(), |i| i.name.clone());
        let label = format!("{} / {}", name(dark), name(light));
        pairs.push(ui.icon_tile("icon-custom".into(), &[image(dark), image(light)], &label, true, focus, pick(icon.choice.clone())));
    }
    let ones = manifest.icons().iter().map(|one| {
        let icons = Some(Icons::One { icon: one.id.clone() });
        ui.icon_tile(format!("icon-{}", one.id), &[image(&one.id)], &one.name, chosen(&icons), focus, pick(icons))
    });
    let custom = ui.button("custom-pair", if sheet.custom_pair { "Done" } else { "Custom pair…" }).on_press(toggle_custom_pair).build();
    let mut page = col()
        .key("app_icon")
        .w_full()
        .gap(8.0)
        .child(ui.caption("Light and dark, swapped with the Appearance"))
        .child(ui.tile_grid(pairs))
        .child(row().child(custom));
    if sheet.custom_pair {
        let sides = icon.sides().unwrap_or_default();
        for (dark, side, label) in [(true, &sides[0], "Dark"), (false, &sides[1], "Light")] {
            let tiles = manifest.icons().iter().map(|one| {
                let key = format!("{}-{}", if dark { "dark" } else { "light" }, one.id);
                let id = one.id.clone();
                ui.icon_tile(key, &[image(&one.id)], &one.name, *side == one.id, false, move |s: &mut SettingsWindow| {
                    choose_side(s, dark, &id)
                })
            });
            page = page.child(ui.caption(label)).child(ui.tile_grid(tiles.collect::<Vec<_>>()));
        }
    }
    page.child(ui.caption("One icon always")).child(ui.tile_grid(ones.collect()))
}

/// Whether `setting`'s control takes focus as the sheet opens.
fn first(sheet: &SettingsSheet, setting: &str) -> bool {
    sheet.focus == Some(setting)
}
