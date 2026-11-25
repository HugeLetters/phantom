//! The menu bar, as data: every menu with its items, built from the command list (`commands`) and
//! the app's state, so names and keys are the list's. `platform::menu_bar` shows it; a pick comes
//! back as its [`Pick`] ([`run`]), done only if the menu built then still offers the very same one,
//! for the same torrents: a menu left open acts on what it was opened for, or not at all.
//!
//! Unlike the row menu and the palette, what doesn't apply is greyed, and titles are in title case.
//! What's behind an open dialog, menu or palette, or behind Settings or a file picker, is greyed.
//! A key runs once: the Torrent menu's are only shown ([`Keys::Shown`]) and run in the window
//! (`shortcuts`), so a field keeps its ⌫ and Space; the others are AppKit's to match
//! ([`Keys::Matched`]), and the window never sees one it matched.

use proto::TorrentId;
use rune::Tx;

use crate::commands::{self, InApp, OnTorrents, Selected, Spec, TOGGLE_KEY};
use crate::model::layout::Column;
use crate::model::{Phantom, selected_torrents};
use crate::overlay::{self, Overlay};
use crate::shortcuts::{Chord, Named, chord};
use crate::windows::Which;

/// What's around the app's own state: the window in front (none: every one is closed or minimized),
/// and whether a panel of macOS's (a file picker, About) has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Around {
    pub front: Option<Which>,
    pub panel: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub name: &'static str,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Separator,
    /// Phantom's own.
    Own(Own),
    Submenu {
        title: &'static str,
        enabled: bool,
        items: Vec<Own>,
    },
    /// One of macOS's standard items Phantom words, greys or leaves out: its AppKit action, which
    /// hands the window its key.
    Standard {
        action: &'static str,
        title: String,
        keys: Chord,
        enabled: bool,
        shown: bool,
    },
    /// One of macOS's that's left as it is.
    System {
        title: &'static str,
        keys: Option<Chord>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Own {
    pub id: String,
    pub title: String,
    pub keys: Keys,
    pub enabled: bool,
    pub checked: bool,
    pub pick: Pick,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Keys {
    None,
    /// Matched by AppKit before the window sees the press: the item runs, or, greyed, nothing does.
    Matched(Chord),
    /// Shown beside the item while its menu is open; AppKit never matches it in a closed menu.
    Shown(Chord),
}

/// What a pick of an item does.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Settings,
    App(InApp),
    CloseWindow,
    /// The command, on these torrents: the selection when the menu was built.
    Torrents(OnTorrents, Vec<TorrentId>),
    Column(Column),
    Sort(Column),
    MainWindow,
}

/// The menu bar for the app as it is.
pub fn build(s: &Phantom, around: Around) -> Vec<Menu> {
    let main = around.front == Some(Which::Main) && !around.panel;
    let overlay = overlay::up(s);
    // The window's own commands: not behind a dialog, a menu or the palette.
    let live = main && matches!(overlay, None | Some(Overlay::Notices));
    let settings = around.front == Some(Which::Settings) && !around.panel;
    let field = around.panel || if settings { s.settings_field } else { main && main_field(s) };
    vec![
        Menu {
            name: "Phantom",
            items: vec![
                Item::System { title: "About Phantom", keys: None },
                own("settings", "Settings…", Keys::Matched(letter(',')), true, Pick::Settings),
                Item::Separator,
                Item::System { title: "Hide Phantom", keys: Some(letter('h')) },
                Item::Separator,
                Item::System { title: "Quit Phantom", keys: Some(letter('q')) },
            ],
        },
        Menu {
            name: "File",
            items: vec![
                // Also with Settings in front, or with no window: the main one comes back for it.
                app(InApp::AddFile, !around.panel && (live || !main)),
                Item::Separator,
                own("close-window", "Close Window", Keys::Matched(letter('w')), around.front.is_some() || around.panel, Pick::CloseWindow),
            ],
        },
        edit(s, field, live, settings),
        torrent(s, live, field),
        view(s, live),
        Menu {
            name: "Window",
            items: vec![
                standard("performMiniaturize:", "Minimize", letter('m'), around.front.is_some(), true),
                Item::System { title: "Zoom", keys: None },
                Item::Separator,
                own("main-window", "Phantom", Keys::None, true, Pick::MainWindow),
            ],
        },
        Menu { name: "Help", items: Vec::new() },
    ]
}

/// A text field of the main window has the focus.
fn main_field(s: &Phantom) -> bool {
    let clash = s.clash.as_ref().and_then(|c| c.renaming.as_ref());
    s.palette.is_open() || (s.rename.id.is_some() && s.rename.field.focused) || s.magnet.focused || clash.is_some_and(|n| n.field.focused)
}

fn edit(s: &Phantom, field: bool, live: bool, settings: bool) -> Menu {
    let selected = !selected_torrents(&s.table).is_empty() && s.mounted.offers(OnTorrents::CopyMagnet);
    let copies = live && !field && selected;
    let list = live && !field;
    // A main window's field has its own undo; Settings' only while it's edited (`menu_title`).
    let undo = if field && !settings { client::display::undo().to_owned() } else { crate::changes::menu_title(s) };
    let undoes = field || (live || settings) && crate::changes::can_undo(s);
    let copy = if copies { title_case(OnTorrents::CopyMagnet.spec().name) } else { "Copy".into() };
    let redo = chord(Named::Letter('z'), true, true);
    Menu {
        name: "Edit",
        items: vec![
            standard("undo:", &undo, InApp::Undo.spec().keys[0], undoes, true),
            standard("redo:", "Redo", redo, field, field),
            Item::Separator,
            standard("cut:", "Cut", letter('x'), field, field),
            standard("copy:", &copy, letter('c'), field || copies, true),
            standard("paste:", if list { "Paste and Add" } else { "Paste" }, letter('v'), field || list, true),
            standard("selectAll:", "Select All", letter('a'), field || list && !s.torrents.is_empty(), true),
        ],
    }
}

fn torrent(s: &Phantom, live: bool, field: bool) -> Menu {
    use OnTorrents::{CancelCheck, CancelMove, Check, Delete, MoveTo, Open, Pause, Remove, Rename, Reveal};
    let ids = selected_torrents(&s.table);
    // The disk is looked at when the item is picked, not while the menu is built.
    let selected = Selected::of(&s.table, &s.torrents, true);
    let item = |c: OnTorrents, keys: Option<Chord>| {
        // ⌫ and ⌘⌫ are a focused field's, so the items showing them are greyed then.
        let fields = field && matches!(c, Remove | Delete);
        let enabled = live && !fields && s.mounted.offers(c) && c.available(&selected);
        let Spec { id, name, .. } = c.spec();
        own(id, &title_case(name), keys.map_or(Keys::None, Keys::Shown), enabled, Pick::Torrents(c, ids.clone()))
    };
    let of = |c: OnTorrents| item(c, c.spec().keys.first().copied());
    Menu {
        name: "Torrent",
        items: vec![
            of(Open),
            of(Reveal),
            Item::Separator,
            item(commands::toggle(s).unwrap_or(Pause), Some(TOGGLE_KEY)),
            of(Check),
            of(CancelCheck),
            of(CancelMove),
            Item::Separator,
            of(MoveTo),
            of(Rename),
            Item::Separator,
            of(Remove),
            of(Delete),
        ],
    }
}

fn view(s: &Phantom, live: bool) -> Menu {
    let shown = s.drawn_columns.borrow();
    let columns = crate::views::columns_menu::wanted(s, &shown);
    let tick = |prefix: &str, c: Column, enabled, checked, pick| Own {
        id: format!("{prefix}-{}", c.id()),
        title: c.title().into(),
        keys: Keys::None,
        enabled,
        checked,
        pick,
    };
    let sorted = s.table.sort().map(|(c, _)| c.to_owned());
    let sorts = columns.iter().map(|&(c, _)| tick("sort", c, live, sorted.as_deref() == Some(c.id()), Pick::Sort(c))).collect();
    let columns = columns.iter().map(|&(c, on)| tick("column", c, live && *shown != [c], on, Pick::Column(c))).collect();
    Menu {
        name: "View",
        items: vec![
            Item::Submenu { title: "Columns", enabled: live, items: columns },
            Item::Submenu { title: "Sort By", enabled: live, items: sorts },
            app(InApp::Palette, live),
        ],
    }
}

/// Does what a picked item does, if the menu bar still offers it ([`build`]).
pub fn run(s: &mut Phantom, pick: &Pick, around: Around, tx: &mut Tx) {
    let offered = build(s, around).iter().flat_map(Menu::own).any(|own| own.enabled && own.pick == *pick);
    if !offered {
        return;
    }
    match pick {
        Pick::Settings => s.settings.post(|w, _| crate::model::settings::open(w)),
        Pick::App(command) => {
            if *command == InApp::AddFile && around.front != Some(Which::Main) {
                s.window.raise();
            }
            commands::run_in_app(s, *command, tx);
        }
        Pick::CloseWindow if around.panel => crate::platform::menu_bar::close_panel(),
        Pick::CloseWindow if around.front == Some(Which::Settings) => s.settings.post(|w, _| crate::model::settings::close_front(w)),
        Pick::CloseWindow => tx.close_window(),
        Pick::Torrents(command, _) => commands::run_on_torrents(s, *command, tx),
        Pick::Column(c) => {
            let shown = s.drawn_columns.borrow().clone();
            s.prefs.toggle(*c, &shown);
        }
        Pick::Sort(c) => crate::model::sort_by(s, *c),
        Pick::MainWindow => s.window.raise(),
    }
}

impl Menu {
    /// Phantom's own items, its submenus' too.
    pub fn own(&self) -> impl Iterator<Item = &Own> {
        self.items.iter().flat_map(|item| match item {
            Item::Own(own) => std::slice::from_ref(own),
            Item::Submenu { items, .. } => items.as_slice(),
            Item::Separator | Item::Standard { .. } | Item::System { .. } => &[],
        })
    }
}

fn letter(c: char) -> Chord {
    chord(Named::Letter(c), true, false)
}

fn own(id: &str, title: &str, keys: Keys, enabled: bool, pick: Pick) -> Item {
    Item::Own(Own { id: id.into(), title: title.into(), keys, enabled, checked: false, pick })
}

fn app(command: InApp, enabled: bool) -> Item {
    let Spec { id, name, keys, .. } = command.spec();
    own(id, &title_case(name), keys.first().map_or(Keys::None, |k| Keys::Matched(*k)), enabled, Pick::App(command))
}

fn standard(action: &'static str, title: &str, keys: Chord, enabled: bool, shown: bool) -> Item {
    Item::Standard { action, title: title.into(), keys, enabled, shown }
}

/// A command's name as menus have it: "Move to…" is "Move To…", "Reveal in Finder" stays.
fn title_case(name: &str) -> String {
    let words: Vec<&str> = name.split(' ').collect();
    let cased = words.iter().enumerate().map(|(i, word)| {
        let small = matches!(*word, "a" | "an" | "and" | "in" | "of" | "or" | "the" | "to");
        let mut letters = word.chars();
        match letters.next() {
            Some(first) if !small || i == 0 => first.to_uppercase().chain(letters).collect(),
            _ => (*word).to_owned(),
        }
    });
    cased.collect::<Vec<String>>().join(" ")
}
