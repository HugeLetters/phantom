//! The Settings sheet, in tabs: General (the default destination, keeping the Mac awake), Network
//! (the listening port and UPnP) and App icon. Each setting's row is keyed by its name in the settings file, and is on
//! exactly one tab ([`tab_of`]).

use client::display;
use proto::{ListenPort, Settings};
use rune::{El, col, el, row};

use crate::model::Phantom;
use crate::model::settings::{
    AppIcon, Row, SettingsSheet, Switch, Tab, choose_folder, choose_icon, choose_side, clear_folder, close, pick_port_automatically,
    show_tab, submit_port, toggle, toggle_custom_pair,
};
use crate::platform::app_icon::Icons;
use crate::ui::{Tone, Ui};

/// Every setting the sheet shows, by its name in its settings file.
pub const SETTINGS: [&str; 5] = ["default_destination", "keep_awake", "listen_port", "upnp", "app_icon"];

/// The tab a setting is on.
pub fn tab_of(setting: &str) -> Option<Tab> {
    match setting {
        "default_destination" | "keep_awake" => Some(Tab::General),
        "listen_port" | "upnp" => Some(Tab::Network),
        "app_icon" => Some(Tab::AppIcon),
        _ => None,
    }
}

pub fn settings(s: &Phantom, sheet: &SettingsSheet, ui: &Ui, layer: &mut Vec<El>) {
    let labels = Tab::ALL.map(Tab::label);
    let selected = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap_or(0);
    let mut body =
        col().w_full().gap(12.0).child(ui.tabs("settings-tabs", &labels, selected, |s: &mut Phantom, i| show_tab(s, Tab::ALL[i])));
    let page = match (sheet.tab, &sheet.service) {
        (Tab::AppIcon, _) => app_icon(&s.app_icon, sheet, ui),
        (tab, Some(settings)) => ui.card(service_rows(tab, settings, sheet, ui)),
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

fn service_rows(tab: Tab, settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> Vec<El> {
    let rows = SETTINGS.iter().filter(|name| tab_of(name) == Some(tab));
    rows.filter_map(|name| match *name {
        "default_destination" => Some(default_destination(settings, sheet, ui)),
        "keep_awake" => Some(switch(Switch::KeepAwake, sheet, ui)),
        "listen_port" => Some(listen_port(settings, sheet, ui)),
        "upnp" => Some(switch(Switch::Upnp, sheet, ui)),
        _ => None,
    })
    .collect()
}

/// The refusal under `row`, if any, after what the value means.
fn notes(sheet: &SettingsSheet, row: Row, meaning: Option<String>) -> Vec<(String, Tone)> {
    let refused = sheet.refused(row).map(|why| (why.to_owned(), Tone::Error));
    meaning.map(|m| (m, Tone::Muted)).into_iter().chain(refused).collect()
}

fn default_destination(settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let folder = match &settings.default_destination {
        Some(folder) => folder.display().to_string(),
        None => "None: every add asks where".into(),
    };
    let controls = row()
        .gap(6.0)
        .child(ui.button("choose-folder", "Choose…").first_focus(first(sheet, "default_destination")).on_press_tx(choose_folder).build())
        .child(ui.button("clear-folder", "Clear").disabled(settings.default_destination.is_none()).on_press(clear_folder).build());
    ui.setting_row("default_destination", display::setting_label("default_destination"), controls, notes(sheet, Row::Folder, Some(folder)))
}

fn listen_port(settings: &Settings, sheet: &SettingsSheet, ui: &Ui) -> El {
    let field = ui
        .text_field("port-field", &sheet.port, display::setting_label("listen_port"))
        .on_edit(|s: &mut Phantom| &mut s.settings.as_mut().expect("the sheet is open").port)
        .on_submit(|s: &mut Phantom, _| submit_port(s))
        .build();
    let mut controls = row().gap(6.0).center_y().child(el().w(84.0).child(field));
    if matches!(settings.listen_port, ListenPort::Pinned(_)) {
        controls = controls.child(ui.button("pick-port", "Pick automatically").on_press(pick_port_automatically).build());
    }
    let meaning = display::sentence(&display::listen_port(settings.listen_port, settings.listening_on));
    ui.setting_row("listen_port", display::setting_label("listen_port"), controls, notes(sheet, Row::Port, Some(meaning)))
}

/// A setting that's on or off, and what it does.
fn switch(which: Switch, sheet: &SettingsSheet, ui: &Ui) -> El {
    let (name, label) = (which.name(), display::setting_label(which.name()));
    let on = sheet.switch(which).unwrap_or_default();
    let control = ui.switch(which.key(), label, on, first(sheet, name), move |s: &mut Phantom| toggle(s, which));
    ui.setting_row(name, label, control, notes(sheet, Row::Switch(which), display::setting_note(name).map(str::to_owned)))
}

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
