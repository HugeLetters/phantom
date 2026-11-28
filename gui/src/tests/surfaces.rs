//! Every surface that offers commands, in every context, as one table (#486): the menu bar's menus
//! (each front window), the row menu, the file menu, the keys, the main window's palette, and
//! whether ⌘P opens Settings' palette and what it lists there. It's compared with `surfaces.txt`, written
//! from the build before commands were declared once, so the declarations change nothing a person
//! sees. `PHANTOM_WRITE_SURFACES=1` writes the file anew instead, for a change that means to.

use rune::{Key, KeyEvent, Mods};

use super::*;
use crate::commands::{Command, on_files, on_torrents};
use crate::menu_bar::{Around, Item, Keys, Menu, Own, Pick, build};
use crate::model::palette::{self, Entry};
use crate::model::settings::Tab;
use crate::model::{common_priority, recent, selected_torrents};
use crate::shortcuts::command_of;
use crate::theme::preset::Appearance;
use crate::theme::setting::Value;
use crate::ui;

const AROUNDS: [(&str, Around); 4] = [
    ("main", Around { front: Some(Which::Main), panel: false }),
    ("settings", Around { front: Some(Which::Settings), panel: false }),
    ("no window", Around { front: None, panel: false }),
    ("panel", Around { front: Some(Which::Main), panel: true }),
];

fn own_line(own: &Own, indent: &str) -> String {
    let keys = match own.keys {
        Keys::None => String::new(),
        Keys::Matched(k) => format!("  [{}]", k.hint()),
        Keys::Shown(k) => format!("  ({})", k.hint()),
    };
    let tick = if own.checked { "✓ " } else { "" };
    format!("{indent}{tick}{}{keys}{}  #{}", own.title, if own.enabled { "" } else { "  (greyed)" }, own.id)
}

fn menu_lines(menu: &Menu) -> Vec<String> {
    let mut lines = vec![format!("  {}:", menu.name)];
    for item in &menu.items {
        match item {
            Item::Separator => lines.push("    —".into()),
            Item::Own(own) => lines.push(own_line(own, "    ")),
            Item::Submenu { title, enabled, items } => {
                lines.push(format!("    {title} ▸{}", if *enabled { "" } else { "  (greyed)" }));
                lines.extend(items.iter().map(|i| own_line(i, "        ")));
            }
            Item::Standard { action, title, keys, enabled, shown } => lines.push(format!(
                "    {title}  {}{}{}  @{action}",
                keys.hint(),
                if *enabled { "" } else { "  (greyed)" },
                if *shown { "" } else { "  (hidden)" }
            )),
            Item::System { title, keys } => lines.push(format!("    {title}{}", keys.map_or(String::new(), |k| format!("  {}", k.hint())))),
        }
    }
    lines
}

fn ui_entries(entries: &[ui::Entry], indent: &str, out: &mut Vec<String>) {
    for e in entries {
        match e {
            ui::Entry::Choice { id, label, hint, icon, checked } => {
                out.push(format!("{indent}{label}  {hint:?} {icon:?} {checked:?}  #{id}"))
            }
            ui::Entry::Disabled { id, label, icon, checked } => out.push(format!("{indent}{label} (disabled) {icon:?} {checked:?}  #{id}")),
            ui::Entry::Sub { id, label, icon, entries, open } => {
                out.push(format!("{indent}{label} ▸ {icon:?} open: {open}  #{id}"));
                ui_entries(entries, &format!("{indent}    "), out);
            }
            ui::Entry::Title(t) => out.push(format!("{indent}[{t}]")),
            ui::Entry::Separator => out.push(format!("{indent}—")),
        }
    }
}

fn palette_lines(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| {
            format!(
                "    {:?} | {} | {:?} | {:?} | {:?} | {:?} | choices: {} | {:?}",
                e.group, e.label, e.hint, e.icon, e.place, e.value, e.has_choices, e.kind
            )
        })
        .collect()
}

/// Every chord any command has, and the toggle's.
fn chords() -> Vec<crate::shortcuts::Chord> {
    let mut all: Vec<crate::shortcuts::Chord> = vec![crate::commands::TOGGLE_KEY];
    for c in Command::all() {
        for k in c.spec().keys {
            if !all.contains(k) {
                all.push(*k);
            }
        }
    }
    all
}

/// The main window's surfaces, as they are now.
fn main_surfaces(d: &Desk, out: &mut Vec<String>) {
    let s = &d.main.state;
    // Settings in front only while it shows.
    for (name, around) in AROUNDS.into_iter().filter(|(_, a)| d.shown().settings || a.front != Some(Which::Settings)) {
        out.push(format!(" menu bar, {name} in front:"));
        for menu in build(s, around) {
            out.extend(menu_lines(&menu));
        }
    }
    out.push(" row menu:".into());
    let rows = crate::views::row_menu::entries(&on_torrents(s), selected_torrents(&s.table).len(), &recent::offered(s), false);
    ui_entries(&rows, "    ", out);
    out.push(" file menu:".into());
    let files = crate::views::file_menu::entries(&on_files(s), common_priority(&s.table, &s.torrents, &s.pane));
    ui_entries(&files, "    ", out);
    out.push(" keys (torrent list):".into());
    for chord in chords() {
        let command = command_of(&chord.event());
        let offered = command.map(|c| s.mounted.offers(c));
        out.push(format!("    {} → {:?}, offered: {offered:?}", chord.hint(), command.map(|c| c.spec().id)));
    }
    out.push(" keys (file list):".into());
    for chord in chords() {
        let ev = chord.event();
        let file = crate::commands::OnFiles::ALL.into_iter().find(|c| c.spec().keys.iter().any(|k| k.matches(&ev)));
        out.push(format!("    {} → {:?}, offered: {:?}", chord.hint(), file.map(|c| c.spec().id), file.map(|c| s.mounted.offers(c))));
    }
    out.push(" main palette:".into());
    out.extend(palette_lines(&palette::entries(s)));
}

/// Settings' surfaces: its palette, and whether ⌘P opens it, by the menu bar's item and by the key.
fn settings_surfaces(d: &mut Desk, out: &mut Vec<String>) {
    if let Some(sheet) = &d.settings.state.sheet {
        out.push(format!(
            " over Settings: icons {}, customize {}, edit {}, save as {}",
            sheet.icons.is_some(),
            sheet.customize.is_some(),
            sheet.edit.is_some(),
            sheet.save_as.is_some()
        ));
    }
    if !d.shown().settings {
        out.push(" Settings isn't shown".into());
        return;
    }
    let settings = AROUNDS[1].1;
    let front = d.shown().front;
    for by in ["menu", "key"] {
        d.front(Which::Settings);
        if by == "menu" {
            d.main.update(move |s, tx| crate::menu_bar::run(s, &Pick::App(crate::commands::InApp::Palette), settings, tx));
        } else {
            d.key(KeyEvent { key: Key::Command('p'), mods: Mods { cmd: true, ..Mods::default() } });
        }
        d.settle();
        let opened = d.settings.state.palette.is_open();
        out.push(format!(" ⌘P in Settings by the {by}: {}", if opened { "opens" } else { "nothing" }));
        // What it lists, where it opens.
        if opened && by == "key" {
            out.push(" settings palette:".into());
            out.extend(palette_lines(&palette::entries(&d.settings.state)));
        }
        if opened {
            palette::close(&mut d.settings.state);
            d.settle();
        }
    }
    if let Some(front) = front {
        d.front(front);
        d.settle();
    }
}

fn step(d: &mut Desk, name: &str, out: &mut Vec<String>) {
    d.settle();
    out.push(format!("== {name}"));
    main_surfaces(d, out);
    settings_surfaces(d, out);
}

fn key(d: &mut Desk, key: Key, cmd: bool) {
    d.key(KeyEvent { key, mods: Mods { cmd, ..Mods::default() } });
    d.settle();
}

/// The table, built by walking through the contexts as a person would.
fn table() -> String {
    let mut cut = torrent(2, "Cut");
    cut.status = Status::Paused;
    // Album's first file is on disk, so its Open, Reveal and Move to Trash are offered.
    let file = |index, path: &str, on_disk| File {
        index,
        path: path.into(),
        size_bytes: MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
        on_disk,
    };
    let service = MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), torrent(1, "Album"), cut])
        .with_files(1, vec![file(0, "01 Intro.flac", true), file(1, "02 Song.flac", false), file(2, "Cover/front.jpg", false)]);
    let mut d = desk(service);
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    let mut out = Vec::new();
    step(&mut d, "nothing selected", &mut out);
    tap(&mut d.main, "rows/1");
    step(&mut d, "Album selected", &mut out);
    tap(&mut d.main, "files/viewport/rows/0");
    step(&mut d, "Album's first file selected, the file list focused", &mut out);
    tap(&mut d.main, "rows/2");
    step(&mut d, "Cut (paused) selected", &mut out);
    key(&mut d, Key::Command('a'), true);
    step(&mut d, "every torrent selected", &mut out);
    tap(&mut d.main, "rows/0");
    key(&mut d, Key::Backspace, false);
    step(&mut d, "a removed: something to undo", &mut out);
    tap(&mut d.main, "rows/1");
    right_click_row(&mut d.main, 1);
    step(&mut d, "the row menu open", &mut out);
    key(&mut d, Key::Escape, false);
    d.main.update(|s, _| palette::open(s));
    step(&mut d, "the main palette open", &mut out);
    d.main.update(|s, tx| palette::dismiss(s, tx));
    d.main.state.clash = Some(crate::model::moving::Clash {
        id: 1,
        name: "Album".into(),
        to: proto::AbsolutePath::new("/Films").unwrap(),
        files: vec![proto::AbsolutePath::new("/Films/Album/a").unwrap()],
        remaining: 0,
        at: 1,
        of: 1,
        apply_to_rest: false,
        other: None,
        renaming: None,
        explained: None,
    });
    step(&mut d, "a clash dialog", &mut out);
    d.main.state.clash = None;
    tap(&mut d.main, "magnet-field");
    step(&mut d, "the link field focused", &mut out);
    d.main.update(|_, tx| tx.blur());
    // Settings.
    d.key(KeyEvent { key: Key::Command(','), mods: Mods { cmd: true, ..Mods::default() } });
    d.wait(0.5);
    for tab in Tab::ALL {
        crate::model::settings::show_tab(&mut d.settings.state, tab);
        step(&mut d, &format!("Settings on {}", tab.label()), &mut out);
    }
    d.settings.update(crate::model::settings::show_icons);
    step(&mut d, "the App icon sheet", &mut out);
    d.settings.update(|s, _| {
        let opened = s.app_icon.tints.clone();
        let sheet = s.sheet.as_mut().expect("Settings shows");
        sheet.customize = Some(crate::model::settings::customize::Customize { icon: "phantom", opened, picker: None });
    });
    step(&mut d, "Customize, over the App icon sheet", &mut out);
    d.settings.update(|s, _| crate::model::settings::close_over(s));
    d.settings.update(|s, _| crate::model::settings::presets::edit_active(s));
    step(&mut d, "the Look sheet, of the Active look", &mut out);
    let set = |d: &mut Desk, half, key: &'static str, value| {
        let mut look = d.main.state.looks.borrow().preset().get(half).clone();
        let _ = look.settings.set(key, value);
        d.main.state.looks.borrow_mut().set(half, look);
        d.settle();
    };
    set(&mut d, Appearance::Light, "corners", Value::Choice("round"));
    set(&mut d, Appearance::Dark, "tone", Value::Choice("light"));
    step(&mut d, "the Look sheet, the halves differing", &mut out);
    d.main.state.looks.borrow_mut().set(Appearance::Dark, crate::theme::ThemeLook::defaults(&crate::theme::tests::SAMPLE));
    step(&mut d, "the Look sheet, a theme in each half", &mut out);
    d.settings.update(|s, _| crate::model::settings::close_over(s));
    d.settings.update(|s, _| crate::model::settings::presets::save_as(s));
    step(&mut d, "Save as…", &mut out);
    d.settings.update(|s, tx| {
        if let Some(save_as) = s.sheet.as_mut().and_then(|sheet| sheet.save_as.as_mut()) {
            save_as.name = rune::TextEdit::new("Night");
        }
        crate::model::settings::presets::save(s, tx);
    });
    step(&mut d, "Settings with a saved Look Preset", &mut out);
    d.settings.update(|s, _| crate::model::settings::presets::edit(s, 0));
    step(&mut d, "the Look sheet, of the saved Look Preset", &mut out);
    d.settings.update(|s, _| crate::model::settings::close_over(s));
    step(&mut d, "Settings again, nothing over it", &mut out);
    d.close_button(Which::Settings);
    step(&mut d, "Settings closed", &mut out);
    out.join("\n") + "\n"
}

/// Every menu, row menu, key and palette list, in every context, is as `surfaces.txt` has it.
#[test]
fn every_surface_in_every_context_is_as_it_was() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/surfaces.txt");
    let now = table();
    if std::env::var_os("PHANTOM_WRITE_SURFACES").is_some() {
        std::fs::write(&path, &now).expect("written");
        return;
    }
    let was = std::fs::read_to_string(&path).expect("surfaces.txt");
    if now != was {
        // Each line that differs, under the context and the surface it's in.
        let (mut step, mut surface, mut diff) = (String::new(), String::new(), Vec::new());
        for (i, (a, b)) in was.lines().zip(now.lines()).enumerate() {
            if a.starts_with("==") {
                step = a.to_owned();
            } else if a.starts_with(' ') && !a.starts_with("  ") {
                surface = a.to_owned();
            }
            if a != b && diff.len() < 40 {
                diff.push(format!("line {}, {step} /{surface}\n  was: {a}\n  now: {b}", i + 1));
            }
        }
        let lines = (was.lines().count(), now.lines().count());
        panic!("the surfaces changed ({} lines, now {}):\n{}", lines.0, lines.1, diff.join("\n"));
    }
}
