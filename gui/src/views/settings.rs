//! The Settings sheet, in tabs: General (the default destination, keeping the Mac awake), Network
//! (the listening port, UPnP and the speed limits), Theme (the Active Look Preset's themes and
//! their settings, on one generic screen) and App icon. Each setting's row is keyed by its name in
//! the settings file, and is on exactly one tab ([`tab_of`]).

use client::display;
use proto::{Direction, ListenPort, SeedingLimit, Settings};
use rune::{El, col, el, row};

use crate::model::Phantom;
use crate::model::settings::{
    AppIcon, NOT_A_COLOUR, Row, SettingsSheet, Side, Switch, Tab, ThemeField, choose_folder, choose_icon, choose_side, clear_folder, close,
    pick_port_automatically, set_theme, show_tab, split_theme, submit_colour, submit_limit, submit_port, submit_seeding, theme_groups,
    toggle, toggle_custom_pair,
};
use crate::platform::app_icon::Icons;
use crate::theme::preset::Looks;
use crate::theme::setting::{Kind, Value};
use crate::ui::{Tone, Ui};

/// Every setting the sheet shows, by its name in its settings file.
pub const SETTINGS: [&str; 10] = [
    "default_destination",
    "keep_awake",
    "seed_ratio",
    "seed_time",
    "listen_port",
    "upnp",
    "download_limit",
    "upload_limit",
    "look_preset",
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
        "look_preset" => Some(Tab::Theme),
        "app_icon" => Some(Tab::AppIcon),
        _ => None,
    }
}

/// The setting a reset name is in: `look_preset` for `look_preset.light.tone`.
pub fn setting_of(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

pub fn settings(s: &Phantom, sheet: &SettingsSheet, ui: &Ui, layer: &mut Vec<El>) {
    let labels = Tab::ALL.map(Tab::label);
    let selected = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap_or(0);
    let mut body =
        col().w_full().gap(12.0).child(ui.tabs("settings-tabs", &labels, selected, |s: &mut Phantom, i| show_tab(s, Tab::ALL[i])));
    let page = match (sheet.tab, &sheet.service) {
        (Tab::AppIcon, _) => app_icon(&s.app_icon, sheet, ui),
        (Tab::Theme, _) => theme(&s.looks, sheet, ui),
        (tab, Some(settings)) => ui.card(service_rows(tab, settings, s, sheet, ui)),
        (_, None) => match sheet.refused(Row::All) {
            Some(why) => ui.error_caption(why).key("settings-unread"),
            None => ui.caption("Reading the settings…").key("settings-reading"),
        },
    };
    body = body.child(page);
    // Read, it's a new modal to Rune, which then focuses the control asked for (`first_focus`):
    // before, the rows that hold it aren't there.
    let key = if sheet.service.is_some() || sheet.refused(Row::All).is_some() { "settings" } else { "settings-reading" };
    ui.sheet(key, "Settings", body, close, layer);
}

fn service_rows(tab: Tab, settings: &Settings, s: &Phantom, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
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
        .on_edit(|s: &mut Phantom| &mut s.settings.as_mut().expect("the sheet is open").port)
        .on_submit(|s: &mut Phantom, _| submit_port(s))
        .width(width_of("port-field"))
        .build();
    let mut controls = row().gap(6.0).center_y().child(field);
    if matches!(settings.listen_port, ListenPort::Pinned(_)) {
        controls = controls.child(ui.button("pick-port", "Pick automatically").on_press(pick_port_automatically).build());
    }
    let meaning = display::sentence(&display::listen_port(settings.listen_port, settings.listening_on));
    ui.setting_row("listen_port", display::setting_label("listen_port"), controls, notes(sheet, Row::Port, Some(meaning)))
}

/// A speed limit's field: what's set, or empty for unlimited; Return sends it.
fn limit(direction: Direction, settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let name = crate::model::settings::limit_name(direction);
    let key = match direction {
        Direction::Download => "download-limit-field",
        Direction::Upload => "upload-limit-field",
    };
    let field = ui
        .text_field(key, sheet.limit(direction), display::setting_label(name))
        .placeholder("Unlimited")
        .on_edit(move |s: &mut Phantom| s.settings.as_mut().expect("the sheet is open").limit_mut(direction))
        .on_submit(move |s: &mut Phantom, _| submit_limit(s, direction))
        .width(width_of(key))
        .build();
    let meaning = display::speed_limit_meaning(settings.speed_limits.get(direction));
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Limit(direction), Some(meaning)))
}

/// A seeding limit's field: what's set, or empty for never; Return sends it.
fn seeding(at: SeedingLimit, settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let name = crate::model::settings::seeding_name(at);
    let (key, meaning) = match at {
        SeedingLimit::Ratio => ("seed-ratio-field", display::seed_ratio_meaning(settings.seeding.ratio)),
        SeedingLimit::Time => ("seed-time-field", display::seed_time_meaning(settings.seeding.time)),
    };
    let field = ui
        .text_field(key, sheet.seeding(at), display::setting_label(name))
        .placeholder("Never")
        .on_edit(move |s: &mut Phantom| s.settings.as_mut().expect("the sheet is open").seeding_mut(at))
        .on_submit(move |s: &mut Phantom, _| submit_seeding(s, at))
        .width(width_of(key))
        .build();
    ui.setting_row(name, display::setting_label(name), field, notes(sheet, Row::Seeding(at), Some(meaning)))
}

/// A setting that's on or off, and what it does.
fn switch(which: Switch, sheet: &SettingsSheet, ui: &Ui) -> El {
    let (name, label) = (which.name(), display::setting_label(which.name()));
    let on = sheet.switch(which).unwrap_or_default();
    let control = ui.switch(which.key(), label, on, first(sheet, name), move |s: &mut Phantom| toggle(s, which));
    ui.setting_row(name, label, control, notes(sheet, Row::Switch(which), display::setting_note(name).map(str::to_owned)))
}

/// The theme each Appearance shows and its settings: one card with the same theme in both, each
/// field one control for both or, split, one for each Appearance; else a card for each.
fn theme(looks: &Looks, sheet: &SettingsSheet, ui: &Ui) -> El {
    let mut focus = first(sheet, "look_preset");
    let mut page = col().key("look_preset").w_full().gap(8.0);
    for group in theme_groups(looks.preset(), &sheet.split) {
        let side = group.appearance.map_or(Side::Both, Side::Only);
        let mut rows = vec![ui.setting_row(&format!("theme-{}", side.key()), "Theme", ui.caption(group.theme.name()), Vec::new())];
        for field in &group.fields {
            rows.push(theme_field(field, group.appearance.is_none(), sheet, ui, std::mem::take(&mut focus)));
        }
        if let Some(appearance) = group.appearance {
            page = page.child(ui.caption(format!("In {}", appearance.label())));
        }
        page = page.child(ui.card(rows));
    }
    page
}

/// A theme setting's row: its control for both Appearances, with Split, or one for each, with
/// Join once they agree when they're one theme's (`joinable`).
fn theme_field(field: &ThemeField, joinable: bool, sheet: &SettingsSheet, ui: &Ui, focus: bool) -> El {
    let setting = field.setting;
    let key = setting.key;
    let mut notes = Vec::new();
    let controls = field.sides.iter().enumerate().map(|(i, &(side, value))| {
        let control = theme_control(setting, side, value, sheet, ui, focus && i == 0);
        if let Some(why) = sheet.colour(side, key).filter(|f| f.refused.as_ref() == Some(&f.text.text)).map(|_| NOT_A_COLOUR) {
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
            ui.button(format!("theme-{key}-split"), "Split")
                .accessible(&format!("{} for each Appearance", setting.label))
                .on_press(move |s: &mut Phantom| split_theme(s, key, true)),
        ),
        (true, true) if agree => Some(
            ui.button(format!("theme-{key}-join"), "Join")
                .accessible(&format!("One {} for both Appearances", setting.label))
                .on_press(move |s: &mut Phantom| split_theme(s, key, false)),
        ),
        (true, true) => None,
    };
    let control = row().gap(6.0).center_y().child(col().gap(4.0).items_end().children(controls)).children(button.map(|b| b.build()));
    ui.setting_row(&format!("theme-{key}"), setting.label, control, notes)
}

/// "Light" or "Dark".
fn side_label(side: Side) -> &'static str {
    match side {
        Side::Both => "Both",
        Side::Only(appearance) => appearance.label(),
    }
}

/// The control that sets `setting` to a value of its kind on `side`, showing `value`, keyed
/// `theme-<key>-<side>`.
fn theme_control(
    setting: &'static crate::theme::setting::Setting,
    side: Side,
    value: Value,
    sheet: &SettingsSheet,
    ui: &Ui,
    focus: bool,
) -> El {
    let key = setting.key;
    let control_key = format!("theme-{key}-{}", side.key());
    match (&setting.kind, value) {
        (Kind::Choice { choices, .. }, Value::Choice(id)) => {
            let labels: Vec<&str> = choices.iter().map(|c| c.label).collect();
            let selected = choices.iter().position(|c| c.id == id).unwrap_or(0);
            // Never first in the Tab order (`Ui::choice`).
            ui.choice(control_key, setting.label, &labels, selected, move |s: &mut Phantom, i| {
                set_theme(s, side, key, Value::Choice(choices[i].id))
            })
        }
        (Kind::Toggle { .. }, Value::Toggle(on)) => {
            ui.switch(control_key, setting.label, on, focus, move |s: &mut Phantom| set_theme(s, side, key, Value::Toggle(!on)))
        }
        (Kind::Colour { .. }, Value::Colour(rgb)) => {
            let Some(field) = sheet.colour(side, key) else { return el() };
            let text = ui
                .text_field(control_key, &field.text, setting.label)
                .on_edit(move |s: &mut Phantom| {
                    &mut s.settings.as_mut().and_then(|sheet| sheet.colour_mut(side, key)).expect("its field").text
                })
                .on_submit(move |s: &mut Phantom, _| submit_colour(s, side, key))
                .width(COLOUR_WIDTH)
                .build();
            row().gap(6.0).center_y().child(ui.swatch(rgb.color())).child(text)
        }
        // A value is always of its setting's kind (`Values`).
        (Kind::Choice { .. } | Kind::Toggle { .. } | Kind::Colour { .. }, _) => el(),
    }
}

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
    let pick = |icons: Option<Icons>| move |s: &mut Phantom| choose_icon(s, icons.clone());
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
                ui.icon_tile(key, &[image(&one.id)], &one.name, *side == one.id, false, move |s: &mut Phantom| choose_side(s, dark, &id))
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
