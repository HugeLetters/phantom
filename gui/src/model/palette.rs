//! The command palette (#33, #434): ⌘P opens a panel over the window listing the **entries** that
//! can be picked now, in the main window and in Settings alike. There is one registry
//! ([`registry`]): every entry says where it applies ([`Applies`]: the window, and what's open over
//! Settings), and [`entries`] filters it by the [`Context`] it's opened in. A window gives the
//! palette only a [`Host`]: its context, its commands, how acting is done there. An entry is one of
//! three kinds ([`Kind`]):
//!
//! - an *action*, from the command list (`commands`): the commands for the selected torrents, those
//!   for the selected files (while the details pane's Files list shows some selected), then the
//!   app's (Add file…, and Undo, named after what it takes back, while there's something to);
//! - a *setting*, a service setting (one row of the table `model::settings::SERVICE`, which the
//!   Settings screen draws from too) or a theme setting (declared by the Active look's theme), with
//!   its value on the right and where it lives muted after its name;
//! - a *place*: Settings, one of its tabs, the editor sheet of the Active look, the App icon's
//!   sheet of every icon (App icon…).
//!
//! The Theme, Tone and the other Choice settings list their options in the palette, and so do the
//! saved **Look Presets** (each as ◐ for both halves, ☀ or ☾). In a list of choices, the highlighted
//! option, or the one under the pointer, is *previewed* in both windows (`Looks::preview`, nothing
//! kept); Return keeps it as one undo step, Escape or leaving the list puts the look back.
//!
//! Each group is under a muted heading naming what it acts on ([`heading`]); the headings aren't
//! rows: ↑ and ↓ and Return only know the entries. Its field has the focus; typing filters by name
//! (and by a setting's synonyms), best matches first, ↑ and ↓ move (round the ends). Escape goes
//! back one level and a click outside closes it. With nothing typed, **Recent** (the last 3
//! settings or places picked, kept in memory only) comes first, then the actions, then every
//! setting and place (scrolling), each group under its heading. Typing filters them and puts first
//! the group with the best match.
//!
//! | Entry | Return | ⌘Return |
//! |---|---|---|
//! | Action | runs it; Rename… opens the Name cell's field, Move to… the folder picker | the same |
//! | Place | opens Settings there | the same |
//! | Setting, on or off | flips it, the status bar telling so with Undo | Show in Settings |
//! | Setting, a Choice | lists its choices ([`Sub`]), the current one ticked; Return sets it | Show in Settings |
//! | Setting, a typed value, a color | Show in Settings | the same |
//!
//! ⇧Return runs an action, a flip or a Choice's option like Return, but the palette stays open, its
//! field empty and the list at the top level, so the next one can be typed at once; for the rest
//! (a place, Rename…, a setting's choices, Show in Settings) it is Return.
//!
//! A change from the palette is the same Settings change as one made in Settings: one undo step.
//! Show in Settings opens Settings at the setting with its control focused. With nothing matching,
//! Return does nothing. It's one of the overlays (`overlay`): opening it closes the others, and
//! theirs close it.
//!
//! What it offers isn't kept: it's read from the selection, the service's settings and the Active
//! look every frame it's drawn, so it can't go stale. Open and Reveal rest on a look at the disk,
//! taken as it opens, as the menus' do (#307).
//!
//! | state ↓ / event → | `Open`      | `Up`             | `Down { n }`           | `Close`  |
//! |-------------------|-------------|------------------|------------------------|----------|
//! | `Closed`          | `Open { 0 }`| `Closed`         | `Closed`               | `Closed` |
//! | `Open { at }`     | `Open { 0 }`| `Open { at − 1 }`, the first goes to the last (n − 1) | `Open { at + 1 }`, the last goes to the first | `Closed` |
//!
//! Typing leaves the state as it is: the highlight is on the first match again whenever the text
//! changed since an arrow last moved it ([`Palette::at`]).

use rune::{TextEdit, Tx};

use proto::Settings;

use super::settings::{self as st, Control, SERVICE, Side, Switch, Tab};
use super::{Phantom, details_torrent, look_at_disk, look_at_file, reveal_target, selected_in_pane};
use crate::changes::Target;
use crate::commands::{Applies, Command, Context, InApp, OnFiles, OnTorrents, Selected, Sheet, on_files, on_torrents};
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::saved::{Name, SavedPreset};
use crate::theme::setting::{Kind as ThemeKind, Value};
use crate::ui::Icon;
use crate::windows::Which;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Closed,
    /// Open, the `at`-th command highlighted.
    Open { at: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    /// Up a list of `n` commands.
    Up {
        n: usize,
    },
    /// Down a list of `n` commands.
    Down {
        n: usize,
    },
    /// Return ran a command, Escape, or a click outside.
    Close,
}

pub fn decide(state: State, event: Event) -> State {
    match (state, event) {
        (_, Event::Open) => State::Open { at: 0 },
        (State::Closed, Event::Up { .. } | Event::Down { .. } | Event::Close) | (State::Open { .. }, Event::Close) => State::Closed,
        (State::Open { at }, Event::Up { n }) => State::Open { at: if n == 0 { 0 } else { (at + n - 1) % n } },
        (State::Open { at }, Event::Down { n }) => State::Open { at: if n == 0 { 0 } else { (at + 1) % n } },
    }
}

/// A setting's choices, while the palette lists them in place of everything else (Return on a
/// Choice setting): its field then filters them.
#[derive(Clone, Debug)]
pub struct Sub {
    pub of: SettingRef,
    /// The setting's name as its entry has it: "Corners", "Tone ☀".
    pub name: String,
    /// What was typed before, put back by Escape.
    before: String,
}

/// The palette: its state, and its field's text.
#[derive(Default)]
pub struct Palette {
    state: State,
    pub query: TextEdit,
    /// The text as it was when an arrow last moved the highlight: once it changes, the highlight is
    /// on the first match again.
    seen: String,
    /// The command highlighted, as last arrowed to, clicked or opened on: the highlight follows it
    /// when rows come and go under it ([`highlighted`]). Meaningful only while `seen` is the text.
    on: Option<Pin>,
    /// The setting whose choices are listed, if any.
    pub sub: Option<Sub>,
    /// The settings and places last picked, newest first, at most [`RECENT`]: in memory only.
    recent: Vec<Kind>,
    /// The saved Look Presets, as Settings had them when the palette opened (it owns them).
    saved: Vec<SavedPreset>,
    /// The choice the pointer rests on, if any: previewed in place of the highlighted one.
    hover: Option<usize>,
}

/// How many settings and places Recent lists.
const RECENT: usize = 3;

/// What identifies a listed entry: its kind, and for "Move to Films" its folder.
type Pin = (Kind, Option<proto::AbsolutePath>);

impl Palette {
    pub fn is_open(&self) -> bool {
        self.state != State::Closed
    }

    /// Which command is highlighted, of those listed: the first since the text last changed.
    fn at(&self) -> usize {
        match self.state {
            State::Open { at } if self.query.text == self.seen => at,
            State::Open { .. } | State::Closed => 0,
        }
    }

    /// The command the highlight is pinned to, unless the text changed since.
    fn pinned(&self) -> Option<&Pin> {
        if self.query.text == self.seen { self.on.as_ref() } else { None }
    }

    fn then(&mut self, event: Event) {
        let state = match self.state {
            State::Open { .. } => State::Open { at: self.at() },
            State::Closed => State::Closed,
        };
        self.state = decide(state, event);
        self.seen = self.query.text.clone();
    }

    /// `kind`, a setting or a place, was picked: first in Recent.
    fn remember(&mut self, kind: &Kind) {
        self.recent.retain(|k| k != kind);
        self.recent.insert(0, kind.clone());
        self.recent.truncate(RECENT);
    }
}

/// What an entry acts on: the palette lists them in this order, each under a heading naming it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// The settings and places last picked, with nothing typed.
    Recent,
    /// The choices of one setting, in place of every other group.
    Choices,
    /// The Look sheet's own commands, in Settings.
    Sheet,
    Torrents,
    Files,
    App,
    /// In Settings, with no sheet open: the settings of the tab shown.
    Here,
    Settings,
}

/// The heading over `group`'s entries, naming the actual selection: the torrent's name or "3
/// torrents", the file's name or "2 files", "Phantom" for the app's, "Settings", "Recent", over
/// a setting's choices the setting's name, and in Settings the sheet's title or the tab's name.
pub fn heading<H: Host>(s: &H, group: Group) -> String {
    match group {
        Group::Sheet | Group::Here | Group::Torrents | Group::Files => s.heading_of(group),
        Group::App => "Phantom".to_owned(),
        Group::Recent => "Recent".to_owned(),
        Group::Settings => "Settings".to_owned(),
        Group::Choices => s.palette().sub.as_ref().map(|sub| sub.name.clone()).unwrap_or_default(),
    }
}

/// The main window's headings: the selection's.
fn main_heading(s: &Phantom, group: Group) -> String {
    match group {
        Group::Torrents => {
            let selected = Selected::of(&s.table, &s.torrents, s.target_there);
            match selected.torrents.as_slice() {
                [t] => t.display_name().to_owned(),
                many => format!("{} torrents", many.len()),
            }
        }
        Group::Files => match selected_in_pane(&s.table, &s.torrents, &s.pane).map(|(_, files)| files) {
            Some(files) if files.len() == 1 => {
                files[0].path.file_name().unwrap_or(files[0].path.as_os_str()).to_string_lossy().into_owned()
            }
            Some(files) => format!("{} files", files.len()),
            None => String::new(),
        },
        _ => String::new(),
    }
}

/// A setting, by what finds it again: a service setting by its key, a theme setting of the Active
/// look by its key and the half it's changed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingRef {
    Service(&'static str),
    /// `Side::Both` while the setting is the same in both halves (one entry); else one entry for
    /// each half.
    Theme {
        key: &'static str,
        side: Side,
    },
    /// The saved Look Presets: its choices apply one, its ☀ or ☾ half or both.
    Presets,
}

/// Somewhere in Settings. Picked in the main window, it opens Settings there; in Settings, it goes
/// there in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The window, at the tab it was on.
    Settings,
    Tab(Tab),
    /// The editor sheet of the Active look, on Appearance.
    EditLook,
    /// The App icon's Edit…, with the sheet of every icon open.
    AppIcon,
}

// `fixed()` lists the places by hand, in the order the palette shows them. A new `Place` stops this
// from compiling until it's named here: add it to `fixed()` too.
const _: fn(Place) = |place| match place {
    Place::Settings | Place::Tab(_) | Place::EditLook | Place::AppIcon => {}
};

/// What an entry is, and so what picking it does.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Action(Command),
    Setting(SettingRef),
    Place(Place),
    /// One of the choices of the setting whose choices are listed.
    Choice(SettingRef, &'static str),
    /// One of the saved Look Presets' choices: the preset applied, to both halves (`None`) or one.
    Apply(Name, Option<Appearance>),
    /// Last in a setting's choices: the setting in Settings.
    Show(SettingRef),
}

/// One entry as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub group: Group,
    /// As the selection words it: "Delete 3 torrents (files to the Trash)", "Undo Remove of Album"; a
    /// setting's name.
    pub label: String,
    /// Other names a setting is found by, never shown.
    pub synonyms: &'static [&'static str],
    pub hint: Option<String>,
    pub icon: Option<Icon>,
    /// A folder Move to ▸ offers (#87), for "Move to Films": it moves there with no picker.
    pub to: Option<proto::AbsolutePath>,
    /// Where a setting or place lives: "Appearance ▸ Edit… ▸ Look".
    pub place: Option<String>,
    /// A setting's value, as it shows on the right: "On", "Small", "6881".
    pub value: Option<String>,
    /// A setting of a few choices: picking it lists them.
    pub has_choices: bool,
    /// The current one of a setting's choices.
    pub selected: bool,
    /// Where it's offered: the windows, and what's open over Settings ([`entries`] filters by it).
    pub applies: Applies,
    /// The tab of Settings it belongs to, for the settings of the tab shown to come first there.
    pub tab: Option<Tab>,
}

impl Entry {
    fn pin(&self) -> Pin {
        (self.kind.clone(), self.to.clone())
    }

    /// What assistive technology reads: the name, the value and where it lives, "Corners, Small,
    /// Appearance, Edit…, Look", then "has choices".
    pub fn read(&self) -> Option<String> {
        if self.value.is_none() && self.place.is_none() && !self.has_choices {
            return None;
        }
        let place = self.place.as_deref().map(|p| p.replace(" ▸ ", ", "));
        let mut parts = vec![self.label.clone()];
        parts.extend(self.value.clone());
        parts.extend(place);
        if self.has_choices {
            parts.push("has choices".to_owned());
        }
        Some(parts.join(", "))
    }
}

/// What a window gives the palette, so that one body of code lists, ranks, navigates and picks for
/// both: Phantom is the main window's, `SettingsWindow` Settings'. What differs between the windows
/// is here and nowhere else: the context the palette is opened in, the commands the window has, the
/// Look its theme settings are of, and how acting is done (the main window asks Settings; Settings
/// does it in place).
pub trait Host: Sized + 'static {
    fn palette(&self) -> &Palette;
    fn palette_mut(&mut self) -> &mut Palette;
    /// The window, Settings' tab and what's open over it: what the entries are filtered by.
    fn context(&self) -> Context;
    /// The commands (`commands`) this window offers now, worded for what they act on.
    fn commands(&self) -> Vec<Entry>;
    /// The service's settings, as this window last had them.
    fn service_settings(&self) -> Option<&Settings>;
    fn saved_presets(&self) -> Vec<SavedPreset>;
    /// The Look the theme settings listed are of, and which it is: the Active look, or the one the
    /// Look sheet over Settings edits.
    fn editing(&self) -> (Target, LookPreset);
    /// The heading of a group only this window has: the selection's, the sheet's, the tab's.
    fn heading_of(&self, group: Group) -> String;
    /// Shows `shown`, or, with none, the look as it stands, in both windows.
    fn show_preview(&self, shown: Option<&LookPreset>);
    /// Just before the palette opens: what it lists depends on a look at the world, and the other
    /// window's palette (if it's open) goes.
    fn opening(&mut self);
    /// The palette is open: asks for what it shows, the service's settings and the saved presets.
    fn opened(&mut self);
    /// Runs a command; `to` is the folder of a "Move to <folder>".
    fn run_command(&mut self, command: Command, to: Option<proto::AbsolutePath>, tx: &mut Tx);
    /// A place: Settings there. From Settings itself, it goes there in place.
    fn open_place(&mut self, place: Place, tx: &mut Tx);
    /// The setting's control, focused, in Settings: the main window opens Settings there, Settings
    /// focuses it where it is.
    fn show(&mut self, of: SettingRef, tx: &mut Tx);
    /// Flips a service switch.
    fn flip_switch(&mut self, switch: Switch, name: &str);
    /// Sets a theme setting of [`Host::editing`]'s Look on `side`: one undo step.
    fn set_theme(&mut self, side: Side, key: &'static str, value: Value);
    /// Chooses the theme `id` for `side` of [`Host::editing`]'s Look.
    fn choose_theme(&mut self, side: Side, id: &'static str);
    fn apply_preset(&mut self, name: Name, half: Option<Appearance>);
    /// Says what was changed, with Undo where the window has a status bar.
    fn tell(&mut self, words: String);
    /// ⌘Z: the app's undo.
    fn undo(&mut self);
}

/// The actions available now, in the list's order: the selected torrents', the selected files'
/// (while the Files list shows some selected), then the app's. Move to… (which asks for a folder)
/// is followed by "Move to <folder>" for each folder Move to ▸ offers.
pub fn offered(s: &Phantom) -> Vec<Entry> {
    let selected = Selected::of(&s.table, &s.torrents, s.target_there);
    let offered = on_torrents(s);
    let can_delete = offered.contains(&OnTorrents::Delete);
    let n = selected.torrents.len();
    let entry = |command: Command, group: Group, label: String| Entry {
        kind: Kind::Action(command),
        group,
        label,
        synonyms: &[],
        hint: command.spec().hint(),
        icon: command.spec().icon,
        to: None,
        place: None,
        value: None,
        has_choices: false,
        selected: false,
        applies: command.applies(),
        tab: None,
    };
    let mut entries = Vec::new();
    for c in offered {
        entries.push(entry(Command::Torrents(c), Group::Torrents, c.wording(n, can_delete)));
        if c == OnTorrents::MoveTo {
            let folders = super::recent::offered(s);
            let names = super::recent::names(&folders);
            entries.extend(folders.into_iter().zip(names).map(|(f, name)| Entry {
                label: client::display::move_to_folder(&name),
                hint: None,
                to: Some(f.folder),
                ..entry(Command::Torrents(c), Group::Torrents, String::new())
            }));
        }
    }
    // Worded as the file menu words them, but for the priorities ([`label_of`]).
    entries.extend(on_files(s).into_iter().map(|c| entry(Command::Files(c), Group::Files, label_of(c))));
    entries.extend(InApp::ALL.into_iter().filter(|c| c.available(s)).map(|c| entry(Command::App(c), Group::App, c.wording(s))));
    entries
}

/// A file command's label in the palette: the priorities say what they set, as the file menu's
/// title does over them.
fn label_of(c: OnFiles) -> String {
    match c {
        OnFiles::Priority(p) => client::display::priority_command(p),
        _ => c.spec().name.to_owned(),
    }
}

// ---- Settings and places ----

fn setting_entry(label: String, kind: Kind, place: String, value: Option<String>, synonyms: &'static [&'static str]) -> Entry {
    Entry {
        kind,
        group: Group::Settings,
        label,
        synonyms,
        hint: None,
        icon: None,
        to: None,
        place: Some(place),
        value,
        has_choices: false,
        selected: false,
        applies: Applies::CLOSED,
        tab: None,
    }
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_owned()
}

/// A service setting's value as the palette shows it: "On", "6881", "Unlimited", a folder's name.
fn service_value(key: &str, settings: &proto::Settings) -> String {
    let name = proto::SettingName::ALL.into_iter().find(|n| n.key() == key);
    let Some(name) = name else { return String::new() };
    match name {
        proto::SettingName::DefaultDestination => settings
            .default_destination
            .as_ref()
            .and_then(|f| f.file_name())
            .map_or("None".to_owned(), |n| n.to_string_lossy().into_owned()),
        proto::SettingName::Upnp => on_off(settings.upnp),
        proto::SettingName::KeepAwake => on_off(settings.keep_awake),
        name => {
            let mut value = client::display::setting_value(name, settings);
            if let Some(first) = value.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            value
        }
    }
}

/// A place, or the Look Presets, which have no registry of their own: each is declared here once,
/// with where it applies.
struct Fixed {
    kind: Kind,
    label: &'static str,
    /// Where it lives, muted after its name.
    under: Option<&'static str>,
    hint: Option<&'static str>,
    synonyms: &'static [&'static str],
    choices: bool,
    applies: Applies,
    tab: Option<Tab>,
}

/// The places, the tabs (one for each of [`Tab::ALL`]) and the Look Presets, in the order the palette
/// lists them.
fn fixed() -> Vec<Fixed> {
    let place =
        |kind, label, under, hint, synonyms, applies, tab| Fixed { kind, label, under, hint, synonyms, choices: false, applies, tab };
    let mut out = vec![place(
        Kind::Place(Place::Settings),
        "Settings…",
        None,
        Some("⌘,"),
        &["preferences", "options"] as &'static [&'static str],
        // The window it opens is the one you're in.
        Applies::MAIN,
        None,
    )];
    out.extend(
        Tab::ALL
            .into_iter()
            .map(|tab| place(Kind::Place(Place::Tab(tab)), tab.label(), Some("Settings"), None, &[], Applies::CLOSED, None)),
    );
    out.push(place(
        Kind::Place(Place::AppIcon),
        "App icon…",
        Some("Appearance"),
        None,
        &["icon", "dock icon"],
        Applies::CLOSED,
        Some(Tab::Appearance),
    ));
    out.push(Fixed {
        choices: true,
        ..place(
            Kind::Setting(SettingRef::Presets),
            "Look Presets",
            Some("Appearance"),
            None,
            &["saved looks", "preset", "apply look"],
            Applies::CLOSED,
            Some(Tab::Appearance),
        )
    });
    out.push(place(
        Kind::Place(Place::EditLook),
        "Edit…",
        Some("Appearance ▸ Active look"),
        None,
        &["active look", "look preset", "customize"],
        Applies::CLOSED,
        Some(Tab::Appearance),
    ));
    out
}

/// Every entry the palette can list anywhere, whatever the context: the window's commands
/// ([`Host::commands`], from `commands`), the places, then every setting, the service's from its
/// table ([`SERVICE`]) and the Look's from what its theme declares. Each says where it applies.
fn registry<H: Host>(s: &H) -> Vec<Entry> {
    let mut out = s.commands();
    out.extend(fixed().into_iter().map(|f| Entry {
        place: f.under.map(str::to_owned),
        hint: f.hint.map(str::to_owned),
        has_choices: f.choices,
        applies: f.applies,
        tab: f.tab,
        ..setting_entry(f.label.to_owned(), f.kind, String::new(), None, f.synonyms)
    }));
    for setting in &SERVICE {
        let value = s.service_settings().map(|settings| service_value(setting.key, settings));
        out.push(Entry {
            tab: Some(setting.tab),
            ..setting_entry(
                setting.label().to_owned(),
                Kind::Setting(SettingRef::Service(setting.key)),
                setting.tab.label().to_owned(),
                value,
                setting.synonyms,
            )
        });
    }
    out.extend(theme_entries(s));
    out
}

/// What the palette lists in this context: the [`registry`], by what each entry says of where it
/// applies. The same function for every window, tab and sheet.
pub fn entries<H: Host>(s: &H) -> Vec<Entry> {
    let ctx = s.context();
    let at_tab = ctx.window == Which::Settings && ctx.sheet == Sheet::Closed;
    registry(s)
        .into_iter()
        .filter(|e| e.applies.holds(&ctx))
        .map(|e| match e.group == Group::Settings && at_tab && e.tab == Some(ctx.tab) {
            // In Settings, the settings of the tab shown come before the rest, under its name: the
            // place a row names is then said once.
            true => Entry { group: Group::Here, place: e.place.filter(|p| *p != ctx.tab.label()), ..e },
            false => e,
        })
        .collect()
}

/// The settings and places of this context: [`entries`], but the commands.
#[cfg(test)]
pub fn settings_entries<H: Host>(s: &H) -> Vec<Entry> {
    entries(s).into_iter().filter(|e| !matches!(e.kind, Kind::Action(_))).collect()
}

/// The Look's theme settings: one entry for each, or, where its halves differ (Tone is Light in ☀
/// and Dark in ☾ by default), one for each half ("Tone ☀", "Tone ☾"), as the editor sheet shows the
/// row split.
fn theme_entries<H: Host>(s: &H) -> Vec<Entry> {
    let preset = s.editing().1;
    let (light, dark) = (&preset.light, &preset.dark);
    let (lr, dr) = (light.resolved(), dark.resolved());
    let mut out = Vec::new();
    let in_look = |entry: Entry| Entry { applies: Applies::EVERYWHERE, tab: Some(Tab::Appearance), ..entry };
    // The Theme itself: one entry while both halves show the same, else one for each.
    let place = "Appearance ▸ Edit…".to_owned();
    let theme_entry = |side: Side, name: &str, theme: &str| {
        let mut entry = setting_entry(
            name.to_owned(),
            Kind::Setting(SettingRef::Theme { key: st::THEME, side }),
            place.clone(),
            Some(theme.to_owned()),
            &[],
        );
        entry.has_choices = true;
        in_look(entry)
    };
    if light.theme.id() == dark.theme.id() {
        out.push(theme_entry(Side::Both, "Theme", light.theme.name()));
    } else {
        out.push(theme_entry(Side::Only(Appearance::Light), "Theme ☀", light.theme.name()));
        out.push(theme_entry(Side::Only(Appearance::Dark), "Theme ☾", dark.theme.name()));
    }
    let mut add =
        |setting: &'static crate::theme::setting::Setting, side: Side, value: Value, resolved: &crate::theme::setting::Resolved| {
            let place = match setting.section {
                Some(section) => format!("Appearance ▸ Edit… ▸ {}", section.title),
                None => "Appearance ▸ Edit…".to_owned(),
            };
            let name = match side {
                Side::Both => setting.label.to_owned(),
                Side::Only(a) => format!("{} {}", setting.label, half_mark(a)),
            };
            let shown = value.shown(setting.key, resolved);
            let text = match (&setting.kind, shown) {
                (ThemeKind::Choice { choices, .. }, Value::Choice(id)) => choices.iter().find(|c| c.id == id).map(|c| c.label.to_owned()),
                (ThemeKind::Toggle { .. }, Value::Toggle(on)) => Some(on_off(on)),
                (ThemeKind::Color { .. }, Value::Color(Some(rgb))) => Some(rgb.hex()),
                _ => None,
            };
            let mut entry = setting_entry(name, Kind::Setting(SettingRef::Theme { key: setting.key, side }), place, text, setting.synonyms);
            entry.has_choices = matches!(setting.kind, ThemeKind::Choice { .. });
            out.push(in_look(entry));
        };
    if light.theme.id() == dark.theme.id() {
        for ((setting, l), (_, d)) in light.settings.iter().zip(dark.settings.iter()) {
            if l.shown(setting.key, &lr) == d.shown(setting.key, &dr) {
                add(setting, Side::Both, l, &lr);
            } else {
                add(setting, Side::Only(Appearance::Light), l, &lr);
                add(setting, Side::Only(Appearance::Dark), d, &dr);
            }
        }
    } else {
        for (setting, value) in light.settings.iter() {
            add(setting, Side::Only(Appearance::Light), value, &lr);
        }
        for (setting, value) in dark.settings.iter() {
            add(setting, Side::Only(Appearance::Dark), value, &dr);
        }
    }
    out
}

/// ☀ or ☾, as the editor sheet names a half.
fn half_mark(a: Appearance) -> &'static str {
    match a {
        Appearance::Light => "☀",
        Appearance::Dark => "☾",
    }
}

/// What picking a setting does, by what it is.
enum How {
    /// On or off: flipped at once.
    Flip,
    /// One of a few: its choices are listed.
    Choose,
    /// A typed value, a color: shown in Settings.
    Show,
}

/// The theme setting `key` as the Look declares it, in `side`'s half.
fn theme_setting<H: Host>(s: &H, key: &str, side: Side) -> Option<(&'static crate::theme::setting::Setting, Value)> {
    let preset = s.editing().1;
    let half = preset.get(side.appearances()[0]);
    half.settings.iter().find(|(setting, _)| setting.key == key)
}

fn how<H: Host>(s: &H, of: SettingRef) -> How {
    match of {
        SettingRef::Service(key) => match st::service_setting(key).map(|setting| setting.control) {
            Some(Control::Switch(_)) => How::Flip,
            _ => How::Show,
        },
        SettingRef::Presets => How::Choose,
        SettingRef::Theme { key, .. } if key == st::THEME => How::Choose,
        SettingRef::Theme { key, side } => match theme_setting(s, key, side).map(|(setting, _)| &setting.kind) {
            Some(ThemeKind::Toggle { .. }) => How::Flip,
            Some(ThemeKind::Choice { .. }) => How::Choose,
            _ => How::Show,
        },
    }
}

/// The choices of the setting listed, the current one ticked, then Show in Settings.
fn choices<H: Host>(s: &H, sub: &Sub) -> Vec<Entry> {
    let entry = |kind: Kind, label: &str, hint: Option<&str>, selected: bool| Entry {
        kind,
        group: Group::Choices,
        label: label.to_owned(),
        synonyms: &[],
        hint: hint.map(str::to_owned),
        icon: selected.then_some(Icon::Check),
        to: None,
        place: None,
        value: None,
        has_choices: false,
        selected,
        applies: Applies::EVERYWHERE,
        tab: None,
    };
    let mut out: Vec<Entry> = match sub.of {
        SettingRef::Presets => s
            .saved_presets()
            .iter()
            .flat_map(|p| [(None, ""), (Some(Appearance::Light), " ☀"), (Some(Appearance::Dark), " ☾")].map(|(half, mark)| (p, half, mark)))
            .map(|(p, half, mark)| {
                let label = format!("{}{}", p.name.as_str(), if half.is_none() { " ◐" } else { mark });
                entry(Kind::Apply(p.name.clone(), half), &label, None, false)
            })
            .collect(),
        SettingRef::Service(_) => return Vec::new(),
        SettingRef::Theme { key, side } if key == st::THEME => {
            let current = s.editing().1.get(side.appearances()[0]).theme.id();
            crate::theme::themes().iter().map(|t| entry(Kind::Choice(sub.of, t.id()), t.name(), None, t.id() == current)).collect()
        }
        SettingRef::Theme { key, side } => {
            let Some((setting, value)) = theme_setting(s, key, side) else { return Vec::new() };
            let ThemeKind::Choice { choices, .. } = &setting.kind else { return Vec::new() };
            choices.iter().map(|c| entry(Kind::Choice(sub.of, c.id), c.label, None, value == Value::Choice(c.id))).collect()
        }
    };
    out.push(entry(Kind::Show(sub.of), "Show in Settings", Some("⌘↩"), false));
    out
}

/// How many choices `sub`'s setting has, whatever's typed (Show in Settings isn't one).
pub fn choices_count<H: Host>(s: &H, sub: &Sub) -> usize {
    choices(s, sub).iter().filter(|e| matches!(e.kind, Kind::Choice(..) | Kind::Apply(..))).count()
}

/// What the palette lists for its text: the [`entries`] of this context whose name or synonyms match
/// it, group by group (a global ranking would scatter a group's heading over the list), best first
/// within a group, in the list's order among equals; while typing, the group with the best match
/// first. With nothing typed: Recent, then the window's own groups (the selection's commands, or in
/// Settings the sheet's, or the tab's settings), then every setting and place. A group with no match
/// has no heading. In a setting's choices ([`Sub`]): its choices that match, best first.
pub fn listed<H: Host>(s: &H) -> Vec<Entry> {
    let query = s.palette().query.text.trim();
    if let Some(sub) = &s.palette().sub {
        let mut scored: Vec<(i32, usize, Entry)> =
            choices(s, sub).into_iter().enumerate().filter_map(|(i, e)| score(query, &e.label).map(|sc| (sc, i, e))).collect();
        scored.sort_by_key(|(sc, i, _)| (-sc, *i));
        return scored.into_iter().map(|(_, _, e)| e).collect();
    }
    let all = entries(s);
    // A command matches by its letters in order, a setting or place more strictly.
    let matches = |e: &Entry| match e.kind {
        Kind::Action(_) => score(query, &e.label),
        _ => setting_score(query, e),
    };
    if query.is_empty() {
        let recent: Vec<Entry> = s
            .palette()
            .recent
            .iter()
            .filter_map(|k| all.iter().find(|e| e.kind == *k))
            .map(|e| Entry { group: Group::Recent, ..e.clone() })
            .collect();
        // What Recent lists isn't listed again: one entry, one row.
        let mut scored: Vec<(i32, usize, Entry)> =
            all.into_iter().enumerate().filter(|(_, e)| !recent.iter().any(|r| r.kind == e.kind)).map(|(i, e)| (0, i, e)).collect();
        scored.splice(0..0, recent.into_iter().map(|e| (0, 0, e)));
        scored.sort_by_key(|(sc, i, e)| (e.group, -sc, *i));
        return scored.into_iter().map(|(_, _, e)| e).collect();
    }
    let mut scored: Vec<(i32, usize, Entry)> = all.into_iter().enumerate().filter_map(|(i, e)| matches(&e).map(|sc| (sc, i, e))).collect();
    // The group with the best match first.
    let best = |g: Group| scored.iter().filter(|(_, _, e)| e.group == g).map(|(sc, ..)| *sc).max().unwrap_or(0);
    let ranks: Vec<(Group, i32)> =
        [Group::Sheet, Group::Torrents, Group::Files, Group::App, Group::Here, Group::Settings].into_iter().map(|g| (g, best(g))).collect();
    let rank = |g: Group| -ranks.iter().find(|(x, _)| *x == g).map_or(0, |(_, b)| *b);
    scored.sort_by_key(|(sc, i, e)| (rank(e.group), e.group, -sc, *i));
    scored.into_iter().map(|(_, _, e)| e).collect()
}

/// How well a setting or place matches `query`: its name, strictly ([`strict`]), or a synonym
/// a little under that.
fn setting_score(query: &str, e: &Entry) -> Option<i32> {
    let name = strict(query, &e.label);
    let synonym = e.synonyms.iter().filter_map(|syn| strict(query, syn)).max().map(|sc| sc - SYNONYM_PENALTY);
    name.max(synonym)
}

/// A synonym's match ranks this much under a name's.
const SYNONYM_PENALTY: i32 = 3;

/// How well `label` matches `query`, if it does: each of the query's letters (any case, spaces
/// ignored) in order in it. A letter starting a word scores most, then one right after the last;
/// a match from the label's first letter more still. `Some(0)` for an empty query.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    let mut from = 0;
    let mut last: Option<usize> = None;
    let mut score = 0;
    for q in query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase) {
        // A letter starting a word is taken before an earlier one inside a word.
        let starts_word = |i: usize| i == 0 || !label[i - 1].is_alphanumeric();
        let found = (from..label.len()).filter(|i| label[*i] == q);
        let at = found
            .clone()
            .find(|i| last.is_some_and(|l| *i == l + 1))
            .or_else(|| found.clone().find(|i| starts_word(*i)))
            .or_else(|| found.clone().next())?;
        score += 1;
        if starts_word(at) {
            score += 4;
        }
        if last.is_some_and(|l| at == l + 1) {
            score += 3;
        }
        if at == 0 {
            score += 4;
        }
        last = Some(at);
        from = at + 1;
    }
    Some(score)
}

/// [`score`], but stricter, as settings match: each letter must start a word or follow the one
/// before it, so "tone" finds Tone and not "Stop seeding after".
pub fn strict(query: &str, label: &str) -> Option<i32> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    let label: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    // The best way for `query` to be found in `label[from..]`, the letter before it at `last`.
    fn best(query: &[char], label: &[char], from: usize, last: Option<usize>) -> Option<i32> {
        let Some((&q, rest)) = query.split_first() else { return Some(0) };
        let mut best: Option<i32> = None;
        for at in from..label.len() {
            if label[at] != q {
                continue;
            }
            let starts_word = at == 0 || !label[at - 1].is_alphanumeric();
            let follows = last.is_some_and(|l| at == l + 1);
            if !(starts_word || follows) {
                continue;
            }
            let here = 1 + if starts_word { 4 } else { 0 } + if follows { 3 } else { 0 } + if at == 0 { 4 } else { 0 };
            if let Some(more) = best_after(rest, label, at) {
                best = best.max(Some(here + more));
            }
        }
        best
    }
    fn best_after(rest: &[char], label: &[char], at: usize) -> Option<i32> {
        best(rest, label, at + 1, Some(at))
    }
    if query.len() > label.len() {
        return None;
    }
    best(&query, &label, 0, None)
}

/// ⌘P: opens over whatever else was open, the field empty and focused, the first command
/// highlighted; the window takes a look at what the list rests on ([`Host::opening`]) and asks for
/// what its values show ([`Host::opened`]).
pub fn open<H: Host>(s: &mut H) {
    s.opening();
    let p = s.palette_mut();
    p.sub = None;
    p.query = TextEdit::new("");
    p.query.focused = true;
    p.then(Event::Open);
    p.hover = None;
    let first = listed(s).first().map(Entry::pin);
    s.palette_mut().on = first;
    s.opened();
}

/// Escape or a click outside.
pub fn dismiss<H: Host>(s: &mut H, tx: &mut Tx) {
    // The focus goes back to what had it, or nowhere: without this, after a Tab in the palette,
    // Rune hands it to a neighbour of the field as the palette fades.
    tx.blur();
    close(s);
}

/// [`dismiss`]ed, a command picked, or another overlay opening.
pub fn close<H: Host>(s: &mut H) {
    let p = s.palette_mut();
    p.sub = None;
    p.hover = None;
    sync_preview(s);
    let p = s.palette_mut();
    p.then(Event::Close);
    p.query = TextEdit::default();
}

/// Escape: back one level. From a setting's choices, to the list, with the text typed before it
/// back; from the list, it closes.
pub fn back<H: Host>(s: &mut H, tx: &mut Tx) {
    match s.palette_mut().sub.take() {
        Some(sub) => {
            let p = s.palette_mut();
            p.hover = None;
            p.query = TextEdit::new(sub.before);
            p.query.focused = true;
            p.state = State::Open { at: 0 };
            p.seen = String::new();
            p.on = None;
            let list = listed(s);
            let p = s.palette_mut();
            p.state = State::Open { at: 0 };
            p.seen = p.query.text.clone();
            p.on = list.first().map(Entry::pin);
            sync_preview(s);
        }
        None => dismiss(s, tx),
    }
}

/// Which of the `listed` commands is highlighted: the one the highlight is pinned to, wherever it
/// has moved to since; if that one is gone (or none is pinned), the row at the position it had,
/// or the last row when the list has shrunk past it, so the highlight lands on a neighbour.
pub fn highlighted<H: Host>(s: &H) -> usize {
    let list = listed(s);
    let pinned = s.palette().pinned().and_then(|c| list.iter().position(|e| e.pin() == *c));
    pinned.unwrap_or_else(|| s.palette().at().min(list.len().saturating_sub(1)))
}

/// Pins the highlight to what is highlighted now, before an arrow moves it from there.
fn settle<H: Host>(s: &mut H) -> Vec<Entry> {
    let (list, at) = (listed(s), highlighted(s));
    let p = s.palette_mut();
    p.state = State::Open { at };
    p.seen = p.query.text.clone();
    p.on = list.get(at).map(Entry::pin);
    list
}

fn arrow<H: Host>(s: &mut H, tx: &mut Tx, event: impl FnOnce(usize) -> Event) {
    let list = settle(s);
    let p = s.palette_mut();
    p.then(event(list.len()));
    let at = p.at();
    p.on = list.get(at).map(Entry::pin);
    p.hover = None;
    sync_preview(s);
    tx.scroll_into_view(row_key(at));
}

/// The Look as picking `entry` would leave it, for the preview: a choice of a theme setting or of
/// the Theme, a saved Look Preset applied.
fn candidate<H: Host>(s: &H, entry: &Entry) -> Option<LookPreset> {
    let active = s.editing().1;
    let preset = match &entry.kind {
        Kind::Choice(SettingRef::Theme { key, side }, id) if *key == st::THEME => st::with_theme(active.clone(), *side, id),
        Kind::Choice(SettingRef::Theme { key, side }, id) => st::with_theme_value(active.clone(), *side, key, Value::Choice(id)),
        Kind::Apply(name, half) => {
            let saved = s.saved_presets().into_iter().find(|p| p.name == *name)?;
            let mut preset = active.clone();
            for a in half.map_or(Appearance::BOTH.to_vec(), |h| vec![h]) {
                *preset.get_mut(a) = saved.preset.get(a).clone();
            }
            preset
        }
        _ => return None,
    };
    (preset != active).then_some(preset)
}

/// Shows in both windows what the choice under the pointer, or else the highlighted one, would
/// leave the Look as, nothing kept; with no choices listed, or the palette closed, the Look as it is.
/// Called wherever the highlight or the pointer moves.
pub fn sync_preview<H: Host>(s: &H) {
    let shown = match &s.palette().sub {
        Some(_) if s.palette().is_open() => {
            let list = listed(s);
            let at = s.palette().hover.filter(|i| *i < list.len()).unwrap_or_else(|| highlighted(s));
            list.get(at).and_then(|e| candidate(s, e))
        }
        _ => None,
    };
    s.show_preview(shown.as_ref());
}

/// The pointer came onto (`on`) or left the `i`-th entry listed: previews that choice, or, leaving it,
/// the highlighted one again.
pub fn hover<H: Host>(s: &mut H, i: usize, on: bool) {
    let now = if on { Some(i) } else { s.palette().hover.filter(|h| *h != i) };
    if s.palette().hover != now {
        s.palette_mut().hover = now;
        sync_preview(s);
    }
}

/// The text changed: the highlight is on the first match again, previewed.
pub fn edited<H: Host>(s: &mut H) {
    s.palette_mut().hover = None;
    sync_preview(s);
}

/// ↑: the highlight up one, the first going to the last.
pub fn up<H: Host>(s: &mut H, tx: &mut Tx) {
    arrow(s, tx, |n| Event::Up { n });
}

/// ↓: the highlight down one, the last going to the first.
pub fn down<H: Host>(s: &mut H, tx: &mut Tx) {
    arrow(s, tx, |n| Event::Down { n });
}

/// How an entry is picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    /// Return: it runs, and the palette closes.
    Return,
    /// ⌘Return: a setting shows in Settings (in Settings, its control focused where it is); any
    /// other entry as [`Pick::Return`].
    Show,
    /// ⇧Return: what runs without leaving the palette (an action, a flip, a choice) runs and leaves
    /// it open, its field empty, at the top level; any other entry as [`Pick::Return`].
    Keep,
}

/// Return: picks the highlighted entry ([`pick_entry`]). With nothing listed, nothing. It's the
/// entry highlighted that is picked, found by identity ([`highlighted`]), never whatever the
/// position now holds.
pub fn run<H: Host>(s: &mut H, tx: &mut Tx) {
    pick_highlighted(s, Pick::Return, tx);
}

/// ⌘Return: the highlighted setting in Settings, its control focused; any other entry as Return.
pub fn show_highlighted<H: Host>(s: &mut H, tx: &mut Tx) {
    pick_highlighted(s, Pick::Show, tx);
}

/// ⇧Return: runs the highlighted entry and keeps the palette open for the next, its field empty and
/// the list back at the top level, Recent first. An entry that opens something (Rename…, a place, a
/// setting's choices or its control in Settings) leaves the palette as Return does.
pub fn run_and_stay<H: Host>(s: &mut H, tx: &mut Tx) {
    pick_highlighted(s, Pick::Keep, tx);
}

fn pick_highlighted<H: Host>(s: &mut H, pick: Pick, tx: &mut Tx) {
    let at = highlighted(s);
    let Some(entry) = listed(s).into_iter().nth(at) else { return };
    pick_entry(s, entry, pick, tx);
}

/// The palette is done with a pick: closed, or, for [`Pick::Keep`], open again with its field empty
/// and the list at the top level.
fn done<H: Host>(s: &mut H, pick: Pick, tx: &mut Tx) {
    if pick == Pick::Keep {
        let p = s.palette_mut();
        p.sub = None;
        p.hover = None;
        sync_preview(s);
        let p = s.palette_mut();
        p.query = TextEdit::new("");
        p.query.focused = true;
        p.state = State::Open { at: 0 };
        p.seen = String::new();
        let first = listed(s).first().map(Entry::pin);
        s.palette_mut().on = first;
        tx.scroll_into_view(row_key(0));
    } else {
        dismiss(s, tx);
    }
}

/// What picking `entry` does: the same in every window ([`Host`] says how it's done there).
fn pick_entry<H: Host>(s: &mut H, entry: Entry, pick: Pick, tx: &mut Tx) {
    let show = pick == Pick::Show;
    match entry.kind.clone() {
        Kind::Action(command) => {
            // One that asks for more (its name ends with "…") opens its flow: the palette leaves.
            if command.spec().name.ends_with('…') || entry.to.is_some() {
                dismiss(s, tx);
            } else {
                done(s, pick, tx);
            }
            s.run_command(command, entry.to, tx);
        }
        Kind::Place(place) => {
            s.palette_mut().remember(&entry.kind);
            dismiss(s, tx);
            s.open_place(place, tx);
        }
        Kind::Setting(of) => {
            s.palette_mut().remember(&entry.kind);
            match (show, how(s, of)) {
                (false, How::Flip) => {
                    done(s, pick, tx);
                    flip(s, of, &entry.label);
                }
                (false, How::Choose) => enter(s, of, entry.label, tx),
                (true, _) | (false, How::Show) => show_in_settings(s, of, tx),
            }
        }
        Kind::Choice(of, id) if !show => {
            let name = s.palette().sub.as_ref().map(|sub| sub.name.clone()).unwrap_or_default();
            done(s, pick, tx);
            choose(s, of, id, &name, &entry.label);
        }
        Kind::Apply(name, half) if !show => {
            done(s, pick, tx);
            s.apply_preset(name, half);
            s.tell(format!("Look Presets: {}", entry.label));
        }
        Kind::Choice(of, _) | Kind::Show(of) => show_in_settings(s, of, tx),
        Kind::Apply(..) => show_in_settings(s, SettingRef::Presets, tx),
    }
}

/// A click on the `i`-th entry listed: picks it.
pub fn pick<H: Host>(s: &mut H, i: usize, tx: &mut Tx) {
    let list = listed(s);
    let p = s.palette_mut();
    p.seen = p.query.text.clone();
    p.state = State::Open { at: i };
    p.on = list.get(i).map(Entry::pin);
    run(s, tx);
}

/// The palette lists the choices of the setting `of`, named `name`, in place of everything else;
/// the highlight is on the current one.
fn enter<H: Host>(s: &mut H, of: SettingRef, name: String, tx: &mut Tx) {
    let p = s.palette_mut();
    let before = std::mem::take(&mut p.query.text);
    p.query = TextEdit::new("");
    p.query.focused = true;
    p.sub = Some(Sub { of, name: name.clone(), before });
    let list = listed(s);
    let at = list.iter().position(|e| e.selected).unwrap_or(0);
    let p = s.palette_mut();
    p.state = State::Open { at };
    p.seen = String::new();
    p.on = list.get(at).map(Entry::pin);
    let n = s.palette().sub.as_ref().map_or(0, |sub| choices_count(s, sub));
    tx.announce(client::display::choices_of(&name, n), rune::LiveRegion::Polite);
    tx.scroll_into_view(row_key(at));
    s.palette_mut().hover = None;
    sync_preview(s);
}

/// Flips an on or off setting, and tells so.
fn flip<H: Host>(s: &mut H, of: SettingRef, name: &str) {
    match of {
        SettingRef::Service(key) => {
            let Some(Control::Switch(switch)) = st::service_setting(key).map(|setting| setting.control) else { return };
            s.flip_switch(switch, name);
        }
        SettingRef::Presets => {}
        SettingRef::Theme { key, side } => {
            let Some((_, Value::Toggle(on))) = theme_setting(s, key, side) else { return };
            s.set_theme(side, key, Value::Toggle(!on));
            s.tell(format!("{name}: {}", on_off(!on)));
        }
    }
}

/// Sets the choice `id` of a setting, and tells so.
fn choose<H: Host>(s: &mut H, of: SettingRef, id: &'static str, name: &str, label: &str) {
    let SettingRef::Theme { key, side } = of else { return };
    if key == st::THEME {
        s.choose_theme(side, id);
    } else {
        s.set_theme(side, key, Value::Choice(id));
    }
    s.tell(format!("{name}: {label}"));
}

/// Show in Settings: the palette closes, the setting's control is focused in Settings (opened at its
/// tab, and the Look's sheet, for a theme setting, if it isn't there).
fn show_in_settings<H: Host>(s: &mut H, of: SettingRef, tx: &mut Tx) {
    dismiss(s, tx);
    s.show(of, tx);
}

/// The key of the `i`-th command's row, as the palette draws it.
pub fn row_key(i: usize) -> String {
    format!("palette-row-{i}")
}

// ---- The main window ----

impl Host for Phantom {
    fn palette(&self) -> &Palette {
        &self.palette
    }

    fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    fn context(&self) -> Context {
        Context { window: Which::Main, tab: Tab::default(), sheet: Sheet::Closed }
    }

    fn commands(&self) -> Vec<Entry> {
        offered(self)
    }

    fn service_settings(&self) -> Option<&Settings> {
        self.service_settings.as_ref()
    }

    fn saved_presets(&self) -> Vec<SavedPreset> {
        self.palette.saved.clone()
    }

    fn editing(&self) -> (Target, LookPreset) {
        (Target::Active, self.looks.borrow().preset().clone())
    }

    fn heading_of(&self, group: Group) -> String {
        main_heading(self, group)
    }

    fn show_preview(&self, shown: Option<&LookPreset>) {
        self.looks.borrow_mut().preview(shown);
    }

    /// Opens over whatever else was open; looks at the disk for Open and Reveal, of the torrent and
    /// of the file.
    fn opening(&mut self) {
        crate::overlay::opening(self, crate::overlay::Overlay::Palette);
        let target = details_torrent(&self.table, &self.torrents).and_then(|t| reveal_target(t).map(|p| p.to_owned()));
        look_at_disk(self, target);
        look_at_file(self);
        // One palette at a time: it previews in both windows.
        self.settings.post(|w, _| {
            if w.palette().is_open() {
                close(w);
            }
        });
    }

    /// Asks Settings for the saved presets and the service for its settings.
    fn opened(&mut self) {
        self.settings.post(|w, _| {
            let saved = w.saved.list().to_vec();
            w.main.post(move |m, _| m.palette.saved = saved);
        });
        self.service.settings(Box::new(|s: &mut Phantom, result, _| {
            if let Ok(settings) = result {
                s.service_settings = Some(settings);
            }
        }));
    }

    fn run_command(&mut self, command: Command, to: Option<proto::AbsolutePath>, tx: &mut Tx) {
        if let Some(to) = to {
            return super::moving::move_selection_to(self, to);
        }
        match command {
            Command::Torrents(c) => crate::commands::run_on_torrents(self, c, tx),
            Command::App(c) => crate::commands::run_in_app(self, c, tx),
            Command::Files(c) => crate::views::file_menu::run(self, c),
            Command::Sheet(_) => unreachable!("the main window has no sheet"),
        }
    }

    fn open_place(&mut self, place: Place, _: &mut Tx) {
        self.settings.post(move |w, tx| {
            st::open(w);
            match place {
                Place::Settings => {}
                Place::Tab(tab) => {
                    st::close_over(w);
                    st::show_tab(w, tab);
                }
                Place::EditLook => {
                    st::show_tab(w, Tab::Appearance);
                    st::presets::edit_active(w);
                }
                Place::AppIcon => st::show_icons(w, tx),
            }
        });
    }

    fn show(&mut self, of: SettingRef, _: &mut Tx) {
        self.settings.post(move |w, tx| st::palette::show_setting_ref(w, of, Target::Active, tx));
    }

    fn flip_switch(&mut self, switch: Switch, name: &str) {
        let Some(on) = self.service_settings.as_ref().map(|settings| switch.stored(settings)) else { return };
        let words = format!("{name}: {}", on_off(!on));
        st::send(self, st::Row::Switch(switch), switch.set(!on), move |s| s.tell(words));
    }

    fn set_theme(&mut self, side: Side, key: &'static str, value: Value) {
        self.settings.post(move |w, _| st::set_active_theme(w, side, key, value));
    }

    fn choose_theme(&mut self, side: Side, id: &'static str) {
        self.settings.post(move |w, _| st::choose_active_theme(w, side, id));
    }

    fn apply_preset(&mut self, name: Name, half: Option<Appearance>) {
        self.settings.post(move |w, _| st::presets::apply_named(w, &name, half));
    }

    /// The status bar tells what was changed, with Undo, which is ⌘Z's too.
    fn tell(&mut self, words: String) {
        use std::rc::Rc;

        use crate::notice::{Notice, NoticeAction};
        let mut undo = NoticeAction::new(client::display::undo(), crate::changes::press);
        undo.hint = Some(Rc::new(|_: &Phantom| Some(crate::shortcuts::hint(InApp::Undo))));
        undo.undo = true;
        self.bar.tell(Notice::new(words).long().with_actions(vec![undo]));
    }

    fn undo(&mut self) {
        crate::changes::press(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use machine::every;
    use machine::{Row, table};

    every!(State {
        State::Closed => [State::Closed],
        State::Open { .. } => [State::Open { at: 0 }, State::Open { at: 2 }],
    });

    every!(Event {
        Event::Open => [Event::Open],
        Event::Up { .. } => [Event::Up { n: 0 }, Event::Up { n: 1 }, Event::Up { n: 3 }, Event::Up { n: 5 }],
        Event::Down { .. } => [Event::Down { n: 0 }, Event::Down { n: 1 }, Event::Down { n: 3 }, Event::Down { n: 5 }],
        Event::Close => [Event::Close],
    });

    /// Every state × event pair, by the module's table.
    #[test]
    fn every_pair_is_decided() {
        let rows: Vec<Row<State, Event, State>> = vec![
            Row { says: "opening highlights the first", when: |_, e| *e == Event::Open, then: |_, _, out| *out == State::Open { at: 0 } },
            Row { says: "closing closes", when: |_, e| *e == Event::Close, then: |_, _, out| *out == State::Closed },
            Row {
                says: "an arrow while closed does nothing",
                when: |s, e| *s == State::Closed && matches!(e, Event::Up { .. } | Event::Down { .. }),
                then: |_, _, out| *out == State::Closed,
            },
            Row {
                says: "↑ moves up one, the first going to the last",
                when: |s, e| matches!(s, State::Open { .. }) && matches!(e, Event::Up { .. }),
                then: |s, e, out| match (s, e, out) {
                    (State::Open { at }, Event::Up { n }, State::Open { at: to }) => *to == if *n == 0 { 0 } else { (at + n - 1) % n },
                    _ => false,
                },
            },
            Row {
                says: "↓ moves down one, the last going to the first",
                when: |s, e| matches!(s, State::Open { .. }) && matches!(e, Event::Down { .. }),
                then: |s, e, out| match (s, e, out) {
                    (State::Open { at }, Event::Down { n }, State::Open { at: to }) => *to == if *n == 0 { 0 } else { (at + 1) % n },
                    _ => false,
                },
            },
        ];
        table(|s: &State, e: &Event| decide(*s, *e), &rows, &[]);
    }

    /// The letters in order match, any case; a prefix and word starts rank first.
    #[test]
    fn a_query_matches_its_letters_in_order_and_ranks_word_starts_first() {
        assert_eq!(score("", "Rename…"), Some(0));
        assert!(score("ren", "Rename…").is_some());
        assert!(score("REN", "Rename…").is_some());
        assert!(score("rnm", "Rename…").is_some());
        assert_eq!(score("nre", "Rename…"), None);
        assert_eq!(score("ren", "Resume"), None);
        assert!(score("ren", "Rename…") > score("ren", "Remove torrent (keep files)"));
        assert!(score("mt", "Move to…") > score("mt", "Copy magnet link"));
        assert!(score("del", "Delete torrent (files to the Trash)") > score("del", "Cancel move"));
    }
}
