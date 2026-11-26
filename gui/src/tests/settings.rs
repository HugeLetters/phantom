//! Generated sequences on the Settings window (#257) beside the main window against `MemoryService`:
//! a person opening it with ⌘, (in either window) and closing it (Escape, its close button, ⌘W with
//! it in front), closing the main window with ⌘W and getting it back from the Dock, switching tabs, typing ports (good ones, a taken one, and text that isn't a
//! port) and sending them, typing speed limits (good ones, empty for unlimited, too low, too high
//! and text that isn't a speed) and sending them, letting the port be picked, flipping UPnP and keeping the Mac awake, choosing a default folder
//! (or one on a drive that isn't connected, or cancelling) and clearing it, and, in the App icon's
//! sheet, a tile's ◐, ☀ or ☾ chip, adding an image (chosen with "+ Add image…", or dropped on the
//! sheet) and removing one, and, among the Look Presets (#228), applying a saved one to the Active
//! look (its ◐ chip, or a half's ☀ or ☾), Edit… on the Active look (or its ☀ or ☾ chip) or a saved
//! preset (the one sheet, in two columns: setting each theme setting for both Appearances, joined,
//! or in one half's column (Tone, and the tests' second theme's toggle, choice and color), joining
//! and splitting rows and the Theme row, choosing a half's theme in its pop-up, Done, Escape, ⌘W),
//! Delete, and
//! Save as… (a name typed, Return or Save; a blank name refused under the field; a taken one asking
//! Replace or Cancel), while the system's Appearance flips; another client (`phantom settings`) changing a
//! setting or undoing the newest change, restarts starting or stopping to fail, and ⌘Z in the sheet
//! (#221). The reference model knows what's stored, what the port field holds, which refusal shows,
//! the Active Look Preset and the changes ⌘Z takes back. After every step:
//! - the sheet shows what's stored, and a refused change stored nothing;
//! - a refusal's caption shows exactly until its field's text changes (the port's, a limit's);
//! - a limit's field shows what's stored exactly, or nothing when unlimited;
//! - the tab shows its own settings and no others, and every setting is on exactly one tab;
//! - a switch toggled twice before the first answer ends where two toggles put it;
//! - with restarts failing, a new port or UPnP stores nothing and goes to the status bar as an error;
//! - Open settings from the settings-reset notice focuses the first reset setting's control on
//!   its tab;
//! - the Active Look Preset is the model's and kept as it is, the window is drawn in the look of
//!   the system's Appearance, and Follow theme shows each look's App icon;
//! - the sheet shows each row joined or split as the model says (with a theme in each half, each
//!   column its own theme's settings, and no setting row joining), each control with its value,
//!   Join or Split where it applies, each Theme pop-up its half's theme, and a color's field what's typed in it and its
//!   refusal until its text changes; a choice is a dropdown (a combobox named for its setting, showing
//!   the chosen label whole), Tone's two a radio group, its segments radios (Tab reaches it and an
//!   arrow key picks the next or previous, #247); and ⌘Z focuses it like any other control;
//! - the windows that show and the one in front are the model's: ⌘W closes the one in front, the
//!   other staying; the Dock brings the main window back;
//! - Settings losing the front (a click on the main window, the Dock, ⌘Z there) leaves the field
//!   being edited, which applies it as a change of its own, a port, a limit or a color (#254);
//! - ⌘Z takes back the newest change, whoever made it, in Settings or in the main window (#255): a
//!   setting's is as it was, Settings opens (if it was closed) in front at its tab with its control focused,
//!   and the status bar tells it was undone ("Port undone"; the popover, "Port back from 40001 to 40000"), or "Couldn't be
//!   undone: <why>"; a torrent removed (from the main window, or by another client, Settings open
//!   or not) is back in the list, Settings staying in front if it is, and told ("Zero is back in the
//!   list"); another client's change shows in the open sheet at once, a field not typed over
//!   following it; `phantom undo` takes the newest torrent action, `phantom settings undo` the
//!   newest service setting's change; Edit ▸ Undo names whichever ⌘Z takes next, in either window;
//! - the saved Look Presets are the model's and kept as they are, each listed in order with its
//!   chips (◐ ☀ ☾, each drawn as its look and named for what a press does; outlined, and named so,
//!   when in use, by value alone) and buttons, after the Active look's row; while a saved preset's
//!   sheet is open both windows show it, never kept, and the Active look is untouched, while the
//!   Active look's sheet edits the Active look itself; Save as… shows what's typed and its
//!   refusal until its text changes, or its question; Escape reverts a field, closes the sheet, or
//!   cancels Save as… (its question first); ⌘Z takes back an Apply (the chip that was used focused:
//!   ◐, ☀ or ☾), a save, a replace or a delete (that preset's ◐ focused), "Apply Evening undone",
//!   and an edit of a Look Preset's setting in its sheet, opened again if it was closed, at the
//!   setting, "Tone (Dark) of Evening undone";
//! - both windows are drawn in the look of the system's Appearance and the Active Look Preset.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::num::NonZeroU16;
use std::path::PathBuf;

use client::setting;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{Direction, ListenPort, SeedingLimit, SeedingLimits, SettingName, SpeedLimits, Unavailable};
use rune::{Key, KeyEvent, Mods, Runtime};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::{Desk, FRAME, Showing, desk, on_screen, torrent};
use crate::changes::{Presets, Target};
use crate::model::settings::SettingsWindow;
use crate::model::settings::{AppIcon, IconChange, Side, Switch, Tab};
use crate::platform::app_icon::{FOLLOW, Icons, Manifest, manifest_dir};
use crate::service::{DiskChange, MemoryService};
use crate::shortcuts;
use crate::theme::Look;
use crate::theme::preset::{Appearance, LookPreset, Looks};
use crate::theme::saved::{Name, Saved, SavedPreset};
use crate::theme::setting::{Kind, Rgb, Show, Value};
use crate::views::settings::{SETTINGS, tab_of};
use crate::windows::Which;

/// Folders the picker can give: two that can be the default, and one on a drive that's gone.
const FOLDERS: [&str; 3] = ["/srv/films", "/srv/music", "/Volumes/MyDisk/torrents"];
const GONE: usize = 2;
/// Why the default folder can't be reached.
fn reasons() -> [Unavailable; 3] {
    [
        Unavailable::DriveNotConnected { name: "MyDisk".into() },
        Unavailable::FolderMissing { path: proto::AbsolutePath::new("/srv/films").unwrap() },
        Unavailable::NoPermission { path: proto::AbsolutePath::new("/srv/films").unwrap() },
    ]
}
/// A port another program has.
const TAKEN: u16 = 40_000;

fn manifest() -> Manifest {
    Manifest::load(&manifest_dir()).expect("the shipped icons")
}

/// Image files that can be added as icons: some of the shipped icons, as files anywhere would be.
fn images() -> [PathBuf; 3] {
    ["full-moon.icns", "dark-hour.icns", "black-mask.icns"].map(|f| manifest_dir().join(f))
}

/// The id of an image file added as an icon.
fn file_id(image: usize) -> String {
    format!("{}{}", crate::platform::app_icon::FILE, images()[image].display())
}

const THEME: &str = crate::model::settings::THEME;

/// What a change of a theme setting of a Look Preset is called: its label; a half's theme, "Theme".
fn theme_label(presets: &Presets, target: &Target, key: &str) -> &'static str {
    match key {
        THEME => crate::model::settings::THEME_LABEL,
        _ => {
            let preset = presets.get(target).unwrap();
            preset.light.settings.iter().chain(preset.dark.settings.iter()).find(|(s, _)| s.key == key).unwrap().0.label
        }
    }
}

/// What's typed into a limit's field: speeds, nothing (unlimited), too low, too high, not a speed.
fn limit_texts() -> Vec<String> {
    ["500 KiB/s", "2M", "1.5 mb/s", "64", "", "8", "5 GiB", "fast"].map(String::from).to_vec()
}

/// What's typed into a seeding limit's field: good ones, nothing (never), out of range, neither.
fn seeding_texts() -> Vec<String> {
    ["2", "1.5", "24", "90 min", "2 d", "", "0", "20000 d", "5000", "lots"].map(String::from).to_vec()
}

const SEEDING: [SeedingLimit; 2] = [SeedingLimit::Ratio, SeedingLimit::Time];

fn at() -> impl Strategy<Value = SeedingLimit> {
    proptest::sample::select(SEEDING.to_vec())
}

/// A seeding limit's field's text as a value for the service, or why it isn't one.
fn seeding_value(at: SeedingLimit, text: &str) -> Result<Option<u64>, setting::ValueProblem> {
    match (text.trim().is_empty(), at) {
        (true, _) => Ok(None),
        (false, SeedingLimit::Ratio) => setting::ratio(text).map(|r| Some(r.hundredths())),
        (false, SeedingLimit::Time) => setting::seed_time(text).map(|t| Some(t.minutes())),
    }
}

fn direction() -> impl Strategy<Value = Direction> {
    prop_oneof![Just(Direction::Download), Just(Direction::Upload)]
}

/// The torrents in the list at the start, by id.
const TORRENTS: [(u64, &str); 3] = [(0, "Zero"), (1, "One"), (2, "Two")];

fn name_of(id: u64) -> &'static str {
    TORRENTS.iter().find(|(i, _)| *i == id).unwrap().1
}

/// What's typed into the port field.
fn port_texts() -> Vec<String> {
    vec!["40000".into(), "40001".into(), "40002".into(), "70000".into(), "0".into(), "port".into(), String::new()]
}

#[derive(Debug, Clone)]
enum Step {
    /// ⌘, in the window in front.
    Open,
    /// Escape in Settings, or its close button.
    Close {
        escape: bool,
    },
    /// ⌘W, which File ▸ Close Window is the key of: closes the window in front, the other staying.
    CmdW,
    /// A click on a window: it's in front.
    Front(Which),
    /// The Dock icon: the main window back, in front.
    Reopen,
    Tab(Tab),
    TypePort(String),
    /// The focus leaves whatever field has it, with no control taking it (#254).
    Blur,
    /// Return in the port field.
    SendPort,
    TypeLimit(Direction, String),
    TypeSeeding(SeedingLimit, String),
    /// Return in the seeding limit's field.
    SendSeeding(SeedingLimit),
    /// Return in the limit's field.
    SendLimit(Direction),
    PickPort,
    /// Once, or twice before the first is answered.
    Toggle {
        switch: Switch,
        twice: bool,
    },
    /// The settings-reset notice's Open settings, for these settings reset.
    OpenReset(Vec<&'static str>),
    /// Choose…, then a folder by index into [`FOLDERS`], or cancel.
    ChooseFolder(Option<usize>),
    ClearFolder,
    /// The default folder can't be reached (by index into [`reasons`]), or can again, as when its
    /// drive is unplugged and plugged in, the sheet open or not (#209); an open sheet's row follows
    /// the service's next snapshot.
    FolderGone(usize),
    FolderBack,
    /// The App icon's Edit…, and its sheet's Done.
    EditIcons,
    IconsDone,
    /// A tile's chip in the App icon's sheet: ◐ (`None`), ☀ or ☾; a designed pair's tile.
    IconChip(IconTile, Option<Appearance>),
    PairTile(usize),
    /// "+ Add image…", and the image file chosen, by index into [`images`], or cancelled; an image
    /// dropped on the sheet; an added image's ×, by its place.
    AddImage(Option<usize>),
    DropImage(usize),
    RemoveImage(usize),
    /// In the sheet, a choice theme setting's option, by index, on a side: a segment pressed, or
    /// its dropdown opened and the option pressed.
    Choose(Side, &'static str, usize),
    /// In the sheet, Tab to a segmented choice's control on a side and press the arrow key
    /// toward the next segment (`true`: right), wrapping at the ends.
    Arrow(Side, &'static str, bool),
    /// A toggle theme setting's switch, on a side.
    Flip(Side, &'static str),
    /// Typed over a color theme setting's field, and Return in it.
    TypeColor(Side, &'static str, String),
    SendColor(Side, &'static str),
    /// A row's Split or Join (the Theme row's is [`THEME`]'s), splitting it, or joining it.
    Split(&'static str),
    Join(&'static str),
    /// The Theme pop-up of a side, and the theme by index into `themes()` chosen in it.
    Theme(Side, usize),
    /// The Active look's ☀ or ☾ chip: its sheet.
    ActiveChip(Appearance),
    /// A saved preset's ◐ chip (both halves) or its ☀ or ☾ chip (that half), by index.
    Apply(usize, Option<Appearance>),
    /// A saved preset's Edit… or Delete, by index; the Active look's Edit….
    Edit(usize),
    EditActive,
    DeletePreset(usize),
    /// The sheet's Done, and its Cancel.
    Done,
    Cancel,
    /// Save as…; typed over its field; Return in it, or its Save; the question's Replace or Cancel.
    SaveAs,
    TypeName(String),
    SaveName {
        by_return: bool,
    },
    Replace,
    CancelReplace,
    /// The system's Appearance changes, the sheet open or not.
    Appearance(bool),
    /// ⌘Z in a window: in Settings with the tabs focused (a click on the tab shown), as after
    /// switching tabs; in the main window, brought to the front first, which leaves the Settings
    /// field being edited and so applies it (#254). Either takes the newest change, a setting's or
    /// a torrent action.
    Undo(Which),
    /// In the main window, a row selected and ⌫: the torrent removed.
    Remove(u64),
    /// Another client removes a torrent (`phantom remove`), or undoes the newest torrent action
    /// (`phantom undo`); the sheet open or not.
    OtherRemove(u64),
    OtherUndoAction,
    /// Another client changes a setting, or undoes the newest settings change (`phantom settings
    /// undo`); the sheet open or not.
    Other(Other),
    OtherUndo,
    /// From now, a new port or UPnP fails (the torrents don't start again), or doesn't.
    Restarts {
        fail: bool,
    },
}

/// A tile of the App icon's sheet: Follow theme, a shipped icon by id, an added image by its place.
#[derive(Debug, Clone, PartialEq)]
enum IconTile {
    Follow,
    Icon(String),
    Added(usize),
}

/// A change another client makes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Other {
    Port(u16),
    Upnp(bool),
    KeepAwake(bool),
    DownloadLimit(Option<u64>),
    Folder(usize),
}

impl Other {
    fn setting(self) -> proto::Setting {
        use proto::Setting;
        match self {
            Other::Port(port) => Setting::ListenPort(NonZeroU16::new(port)),
            Other::Upnp(on) => Setting::Upnp(Some(on)),
            Other::KeepAwake(on) => Setting::KeepAwake(Some(on)),
            Other::DownloadLimit(limit) => Setting::SpeedLimit { direction: Direction::Download, bytes_per_sec: limit },
            Other::Folder(i) => Setting::DefaultDestination(Some(proto::AbsolutePath::new(FOLDERS[i]).unwrap())),
        }
    }
}

fn others() -> Vec<Other> {
    vec![
        Other::Port(40_001),
        Other::Port(40_002),
        Other::Upnp(false),
        Other::Upnp(true),
        Other::KeepAwake(true),
        Other::DownloadLimit(Some(512 * 1024)),
        Other::DownloadLimit(None),
        Other::Folder(0),
        Other::Folder(1),
    ]
}

/// A change ⌘Z takes back, with what its setting was before; or a torrent removed, by id.
#[derive(Debug, Clone)]
enum Change {
    Service(SettingName, Stored),
    Icon(Option<Icons>),
    /// A theme setting set on a side of a Look Preset: the presets before, and which it was.
    Theme(Presets, Target, Side, &'static str),
    /// The presets before, what the change is called, and the chip an undo focuses: the saved
    /// preset's name and its half (◐ when `None`).
    Presets(Presets, String, String, Option<Appearance>),
    /// A Look Preset's editor's Done: the presets when it opened, and what it edited. While it's
    /// open, its own changes are the `Theme` ones.
    Edit(Presets, Target),
    Removed(u64),
}

/// Whether the newest change is a Look Preset's editor's Done.
fn theme_edit_last(m: &Model) -> bool {
    matches!(m.changes.last(), Some(Change::Edit(..)))
}

/// Whether something's typed into Save as…'s field.
fn text_typed(m: &Model) -> bool {
    m.save_as.as_ref().is_some_and(|(text, _)| !text.is_empty())
}

/// What's typed into the name field: names, with spaces around one, blank.
fn name_texts(m: &Model) -> Vec<String> {
    let mut texts: Vec<String> = ["Calm", "Night 2", " Calm ", "", "   "].map(String::from).to_vec();
    // A name taken, as often as a new one.
    for _ in 0..4 {
        texts.extend(m.saved.iter().take(3).map(|s| s.name.as_str().to_owned()));
    }
    texts
}

/// What's typed into a color's field: colors, either case, and not colors.
fn color_texts() -> Vec<String> {
    ["#3366ff", "#FF0000", "#00ff7f", "#123", "red", ""].map(String::from).to_vec()
}

#[derive(Debug, Clone, PartialEq)]
struct Stored {
    folder: Option<PathBuf>,
    port: ListenPort,
    upnp: bool,
    keep_awake: bool,
    speed_limits: SpeedLimits,
    seeding: SeedingLimits,
}

/// A text field of the sheet that applies when it loses focus.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fld {
    Port,
    Limit(Direction),
    Seeding(SeedingLimit),
    /// A color theme setting's field, in the sheet, on a side.
    Color(Side, &'static str),
}

/// The text a refusal is about, and why when the app words it (a service's refusal is shown, not checked).
type Refusal = Option<(String, Option<String>)>;

#[derive(Debug, Clone)]
struct Model {
    stored: Stored,
    /// Settings shows, its sheet open.
    open: bool,
    /// The main window shows.
    main: bool,
    /// The window keys go to.
    front: Option<Which>,
    tab: Tab,
    field: String,
    /// The field's text a refusal of the port is about; a folder refused.
    port_refused: Refusal,
    folder_refused: bool,
    /// The folders that can't be reached now, each with why, as [`reasons`] has it.
    gone: BTreeMap<PathBuf, usize>,
    /// Each limit's field, and the text a refusal of it is about.
    limit_fields: [String; 2],
    limits_refused: [Refusal; 2],
    /// Each seeding limit's field, and the text a refusal of it is about.
    seeding_fields: [String; 2],
    seeding_refused: [Refusal; 2],
    choice: Option<Icons>,
    /// The App icon's sheet is open; the images added as icons, by id, in order.
    icons_open: bool,
    /// The picks made since it opened that ⌘Z hasn't taken: with it open, the only changes ⌘Z takes.
    visit: usize,
    /// The App icon when its sheet opened: what the visit's one step undoes to.
    icons_was: Option<Icons>,
    added: Vec<String>,
    /// The shipped icons' ids, and the designed pairs.
    ids: Vec<String>,
    pairs: Vec<Icons>,
    /// The fake fails a new port or UPnP, as when the torrents don't start again with them.
    restarts_fail: bool,
    /// Error notices the status bar got.
    errors: usize,
    /// The control focused, by key, right after Open settings.
    focus: Option<String>,
    /// The Active Look Preset, and the system's Appearance.
    preset: LookPreset,
    dark: bool,
    /// The Look Preset whose sheet is open (the Active look, or a saved one), and its editor.
    sheet: Option<(Target, Ed)>,
    /// The Look Presets as they were when it opened.
    opened: Option<Presets>,
    /// The text field with the focus: from a click in it until another step takes the focus away.
    focused: Option<Fld>,
    /// The changes ⌘Z takes back, newest last, and what each undo told, in order.
    changes: Vec<Change>,
    told: Vec<String>,
    /// The torrents in the list, by id.
    rows: BTreeSet<u64>,
    /// The saved Look Presets.
    saved: Vec<SavedPreset>,
    /// Save as…, while it's open: its field's text, and the name it asks whether to replace.
    save_as: Option<(String, Option<String>)>,
    /// The text a refusal of Save as…'s name is about and why, and whether its field has the focus
    /// (its focus applies nothing, so it's apart from `focused`).
    name_refused: Option<(String, String)>,
    name_focused: bool,
}

/// An editor's own state: the rows split by hand or set in one column since it opened ([`THEME`]
/// for the Theme row), and each color field shown, its text and the text a refusal is about.
#[derive(Debug, Clone, Default)]
struct Ed {
    split: BTreeSet<&'static str>,
    colors: BTreeMap<(Side, &'static str), (String, Refusal)>,
}

impl Stored {
    /// Whether `name` is the same in both.
    fn same(&self, other: &Stored, name: SettingName) -> bool {
        self.with(name, other) == *self
    }

    /// These, with `name` as `was` has it.
    fn with(&self, name: SettingName, was: &Stored) -> Stored {
        let mut back = self.clone();
        match name {
            SettingName::DefaultDestination => back.folder = was.folder.clone(),
            SettingName::ListenPort => back.port = was.port,
            SettingName::Upnp => back.upnp = was.upnp,
            SettingName::KeepAwake => back.keep_awake = was.keep_awake,
            SettingName::DownloadLimit => back.speed_limits.download = was.speed_limits.download,
            SettingName::UploadLimit => back.speed_limits.upload = was.speed_limits.upload,
            SettingName::SeedRatio => back.seeding.ratio = was.seeding.ratio,
            SettingName::SeedTime => back.seeding.time = was.seeding.time,
        }
        back
    }
}

/// A new port or UPnP restarts the torrents.
fn restarts(name: SettingName) -> bool {
    matches!(name, SettingName::ListenPort | SettingName::Upnp)
}

/// The key of the control an undo of `name` focuses.
fn control_of(name: SettingName) -> &'static str {
    match name {
        SettingName::DefaultDestination => "choose-folder",
        SettingName::ListenPort => "port-field",
        SettingName::Upnp => "upnp-switch",
        SettingName::KeepAwake => "keep-awake-switch",
        SettingName::DownloadLimit => "download-limit-field",
        SettingName::UploadLimit => "upload-limit-field",
        SettingName::SeedRatio => "seed-ratio-field",
        SettingName::SeedTime => "seed-time-field",
    }
}

/// A control the sheet shows of a theme setting: its key, its kind, and the side it sets.
#[derive(Clone)]
struct Shown {
    key: &'static str,
    kind: &'static Kind,
    side: Side,
}

impl Model {
    fn fld_text(&self, f: Fld) -> &String {
        match f {
            Fld::Port => &self.field,
            Fld::Limit(d) => &self.limit_fields[d as usize],
            Fld::Seeding(at) => &self.seeding_fields[at as usize],
            Fld::Color(side, key) => &self.ed().colors[&(side, key)].0,
        }
    }

    fn fld_text_mut(&mut self, f: Fld) -> &mut String {
        match f {
            Fld::Port => &mut self.field,
            Fld::Limit(d) => &mut self.limit_fields[d as usize],
            Fld::Seeding(at) => &mut self.seeding_fields[at as usize],
            Fld::Color(side, key) => &mut self.ed_mut().colors.get_mut(&(side, key)).expect("its field").0,
        }
    }

    fn fld_refused_mut(&mut self, f: Fld) -> &mut Refusal {
        match f {
            Fld::Port => &mut self.port_refused,
            Fld::Limit(d) => &mut self.limits_refused[d as usize],
            Fld::Seeding(at) => &mut self.seeding_refused[at as usize],
            Fld::Color(side, key) => &mut self.ed_mut().colors.get_mut(&(side, key)).expect("its field").1,
        }
    }

    /// What the field shows for what's stored.
    fn fld_stored(&self, f: Fld) -> String {
        match f {
            Fld::Port => self.port_text(),
            Fld::Limit(d) => self.limit_text(d),
            Fld::Seeding(at) => self.seeding_text(at),
            Fld::Color(side, key) => self.color_hex(side, key),
        }
    }

    /// Whether the field is shown.
    fn fld_shown(&self, f: Fld) -> bool {
        match f {
            Fld::Color(side, key) => self.ed_opt().is_some_and(|ed| ed.colors.contains_key(&(side, key))),
            Fld::Port | Fld::Limit(_) | Fld::Seeding(_) => true,
        }
    }

    /// Why `text` isn't a value of the field, as the app words it.
    fn fld_problem(f: Fld, text: &str) -> Option<String> {
        if let Fld::Color(..) = f {
            return Rgb::parse(text).is_none().then(|| crate::model::settings::NOT_A_COLOR.to_owned());
        }
        let why = match f {
            Fld::Port => setting::port(text).err(),
            Fld::Limit(_) if text.trim().is_empty() => None,
            Fld::Limit(_) => setting::speed(text).err(),
            Fld::Seeding(at) => seeding_value(at, text).err(),
            Fld::Color(..) => unreachable!(),
        };
        why.map(|why| client::display::value_problem(why).to_owned())
    }

    /// The field being edited: focused, its text other than what it shows of what's stored.
    fn edited(&self) -> Option<Fld> {
        self.focused.filter(|f| self.fld_shown(*f) && *self.fld_text(*f) != self.fld_stored(*f))
    }

    /// Return in the field: its text sent, as the service takes it, or refused.
    fn send(&mut self, f: Fld) {
        let text = self.fld_text(f).clone();
        if let Some(why) = Model::fld_problem(f, &text) {
            *self.fld_refused_mut(f) = Some((text, Some(why)));
            return;
        }
        match f {
            // A seeding limit restarts nothing, so it never fails as one.
            Fld::Seeding(at) => {
                let value = seeding_value(at, &text).unwrap();
                match at {
                    SeedingLimit::Ratio => self.stored.seeding.ratio = value.map(|h| proto::Ratio::new(h).unwrap()),
                    SeedingLimit::Time => self.stored.seeding.time = value.map(|m| proto::SeedTime::new(m).unwrap()),
                }
            }
            // Empty is unlimited; a limit applies without a restart, so it never fails as one.
            Fld::Limit(d) => {
                let limit = if text.trim().is_empty() { None } else { setting::speed(&text).ok() };
                self.stored.speed_limits.set(d, limit);
            }
            Fld::Color(side, key) => {
                let rgb = Rgb::parse(&text).unwrap();
                self.set(side, key, Value::Color(Some(rgb)));
            }
            // A new port that fails stores nothing: it's refused under its row and listed. The port
            // it has: nothing to send.
            Fld::Port => {
                let port = setting::port(&text).unwrap();
                if self.stored.port != ListenPort::Pinned(port) {
                    if self.restarts_fail {
                        self.errors += 1;
                        self.port_refused = Some((text, None));
                        return;
                    }
                    self.stored.port = ListenPort::Pinned(port);
                }
            }
        }
        *self.fld_text_mut(f) = self.fld_stored(f);
        *self.fld_refused_mut(f) = None;
    }

    /// The focus leaves the field (#254): text that's what's stored does nothing; a text that was
    /// refused already, or can't be used, isn't kept, the stored one shows with why; else it's sent.
    fn leave(&mut self, f: Fld) {
        if !self.fld_shown(f) {
            return;
        }
        let text = self.fld_text(f).clone();
        let stored = self.fld_stored(f);
        if text == stored {
            return;
        }
        let refused = self.fld_refused_mut(f).clone().filter(|(t, _)| *t == text).map(|(_, why)| why);
        match refused.or_else(|| Model::fld_problem(f, &text).map(Some)) {
            Some(why) => {
                *self.fld_text_mut(f) = stored.clone();
                *self.fld_refused_mut(f) = Some((stored, why));
            }
            None => self.send(f),
        }
    }

    fn port_text(&self) -> String {
        self.stored.port.port().to_string()
    }

    /// What a seeding limit's field shows for what's stored.
    fn seeding_text(&self, at: SeedingLimit) -> String {
        match at {
            SeedingLimit::Ratio => self.stored.seeding.ratio.map(client::display::ratio),
            SeedingLimit::Time => self.stored.seeding.time.map(client::display::seed_time),
        }
        .unwrap_or_default()
    }

    /// What a limit's field shows for what's stored.
    fn limit_text(&self, direction: Direction) -> String {
        self.stored.speed_limits.get(direction).map(client::display::speed_limit).unwrap_or_default()
    }

    /// What following the theme shows: each half's theme's App icon.
    fn theme_icons(&self) -> Icons {
        let [light, dark] = Appearance::BOTH.map(|a| Look::build(self.preset.get(a)).app_icon.to_owned());
        if light == dark { Icons::One { icon: light } } else { Icons::Pair { dark, light } }
    }

    /// The sheet's editor state, while it's open.
    fn ed_opt(&self) -> Option<&Ed> {
        self.sheet.as_ref().map(|(_, ed)| ed)
    }

    fn ed(&self) -> &Ed {
        self.ed_opt().expect("the sheet is open")
    }

    fn ed_mut(&mut self) -> &mut Ed {
        &mut self.sheet.as_mut().expect("the sheet is open").1
    }

    /// What the sheet edits.
    fn target(&self) -> Target {
        self.sheet.as_ref().expect("the sheet is open").0.clone()
    }

    /// The Look Presets now.
    fn presets(&self) -> Presets {
        Presets { active: self.preset.clone(), saved: self.saved.clone() }
    }

    /// The preset the sheet edits.
    fn preset_edited(&self) -> LookPreset {
        self.presets().get(&self.target()).expect("there's one").clone()
    }

    /// Whether both halves of what the sheet edits have one theme: its rows join and split.
    fn same_theme(&self) -> bool {
        let preset = self.preset_edited();
        preset.light.theme.id() == preset.dark.theme.id()
    }

    /// Whether the Theme row shows one pop-up for both.
    fn theme_joined(&self) -> bool {
        self.same_theme() && !self.ed().split.contains(THEME)
    }

    /// The sides the Theme row's pop-ups set.
    fn theme_sides(&self) -> Vec<Side> {
        if self.theme_joined() { vec![Side::Both] } else { Appearance::BOTH.map(Side::Only).to_vec() }
    }

    /// Whether `key`'s row is joined: one theme, one value in both halves as they show (a color
    /// as it comes to), not split by hand.
    fn joined(&self, key: &str) -> bool {
        self.same_theme()
            && self.shown_value(Side::Only(Appearance::Light), key) == self.shown_value(Side::Only(Appearance::Dark), key)
            && !self.ed().split.contains(key)
    }

    /// Every control the sheet shows of a theme setting, in order: with one theme, a row's one for
    /// both, joined, or one for each half; with a theme in each half, each one's own settings.
    fn shown(&self) -> Vec<Shown> {
        let preset = self.preset_edited();
        let look = |half: Appearance| preset.get(half).theme.settings().iter();
        if !self.same_theme() {
            let mut shown = Vec::new();
            for half in Appearance::BOTH {
                shown.extend(look(half).map(|s| Shown { key: s.key, kind: &s.kind, side: Side::Only(half) }));
            }
            return shown;
        }
        look(Appearance::Light)
            .flat_map(|s| match self.joined(s.key) {
                true => vec![Shown { key: s.key, kind: &s.kind, side: Side::Both }],
                false => Appearance::BOTH.map(|half| Shown { key: s.key, kind: &s.kind, side: Side::Only(half) }).to_vec(),
            })
            .collect()
    }

    /// Each half of the choice, `[dark, light]`: an icon, or `None` following the theme.
    fn icon_halves(&self) -> [Option<String>; 2] {
        let id = |id: &String| (id != FOLLOW).then(|| id.clone());
        match &self.choice {
            None => [None, None],
            Some(Icons::One { icon }) => [id(icon), id(icon)],
            Some(Icons::Pair { dark, light }) => [id(dark), id(light)],
        }
    }

    /// The choice with these halves: none following the theme in both, one icon in both.
    fn choose_halves(&mut self, halves: [Option<String>; 2]) {
        let id = |half: &Option<String>| half.clone().unwrap_or_else(|| FOLLOW.to_owned());
        self.choice = match &halves {
            [None, None] => None,
            [dark, light] if dark == light => Some(Icons::One { icon: id(dark) }),
            [dark, light] => Some(Icons::Pair { dark: id(dark), light: id(light) }),
        };
    }

    /// The icon each half shows now, `[dark, light]`: the choice's, or, following, the theme's.
    fn icon_sides(&self) -> [String; 2] {
        let theme = match self.theme_icons() {
            Icons::One { icon } => [icon.clone(), icon],
            Icons::Pair { dark, light } => [dark, light],
        };
        let [dark, light] = self.icon_halves();
        let [theme_dark, theme_light] = theme;
        [dark.unwrap_or(theme_dark), light.unwrap_or(theme_light)]
    }

    /// What an icon's chip (`None`: Follow theme's) chooses: ◐ it in both, ☀ or ☾ that half, the
    /// other as it is.
    fn chip_choice(&mut self, id: Option<String>, half: Option<Appearance>) {
        let mut halves = self.icon_halves();
        match half {
            None => halves = [id.clone(), id],
            Some(Appearance::Dark) => halves[0] = id,
            Some(Appearance::Light) => halves[1] = id,
        }
        self.choose_halves(halves);
    }

    /// The id of an icon tile, and its key.
    fn tile(&self, tile: &IconTile) -> (Option<String>, String) {
        match tile {
            IconTile::Follow => (None, "icon-follow".into()),
            IconTile::Icon(id) => (Some(id.clone()), format!("icon-{id}")),
            IconTile::Added(i) => (Some(self.added[*i].clone()), format!("icon-image-{i}")),
        }
    }

    /// Every tile, in order.
    fn tiles(&self) -> Vec<IconTile> {
        let icons = self.ids.iter().map(|id| IconTile::Icon(id.clone()));
        [IconTile::Follow].into_iter().chain(icons).chain((0..self.added.len()).map(IconTile::Added)).collect()
    }

    /// An image added as an icon, unless it's there already.
    fn add(&mut self, image: usize) {
        let id = file_id(image);
        if !self.added.contains(&id) {
            self.added.push(id);
        }
    }

    /// The value `key` shows on `side`: a color as its theme works it out, else what it holds.
    fn shown_value(&self, side: Side, key: &str) -> Value {
        let appearance = side.appearances()[0];
        self.value(side, key).shown(key, &self.preset_edited().get(appearance).resolved())
    }

    /// The hex a color's field shows for what's stored.
    fn color_hex(&self, side: Side, key: &str) -> String {
        match self.shown_value(side, key) {
            Value::Color(Some(rgb)) => rgb.hex(),
            _ => unreachable!("a color shows one"),
        }
    }

    /// The value `key` holds on `side` (Light's, for both) of what the sheet edits.
    fn value(&self, side: Side, key: &str) -> Value {
        let appearance = match side {
            Side::Both | Side::Only(Appearance::Light) => Appearance::Light,
            Side::Only(Appearance::Dark) => Appearance::Dark,
        };
        self.preset_edited().get(appearance).settings.get(key).expect("its theme has it")
    }

    /// Sets `key` on `side`'s halves of what the sheet edits; set in one column, its row stays split.
    fn set(&mut self, side: Side, key: &'static str, value: Value) {
        // Spelt out here, not asked of the code under test.
        let halves: &[Appearance] = match side {
            Side::Both => &[Appearance::Light, Appearance::Dark],
            Side::Only(Appearance::Light) => &[Appearance::Light],
            Side::Only(Appearance::Dark) => &[Appearance::Dark],
        };
        let mut preset = self.preset_edited();
        for appearance in halves {
            let _ = preset.get_mut(*appearance).settings.set(key, value);
        }
        self.keep_edited(preset);
        if side != Side::Both {
            self.ed_mut().split.insert(key);
        }
        self.show_colors();
    }

    /// What the sheet edits is now `preset`.
    fn keep_edited(&mut self, preset: LookPreset) {
        match self.target() {
            Target::Active => self.preset = preset,
            Target::Saved(name) => self.saved.iter_mut().find(|s| s.name == name).unwrap().preset = preset,
        }
    }

    /// The theme `theme` chosen for `side`: each half showing another starts at its settings as a
    /// fresh install has them for that half, or its defaults; for one half alone, the Theme row
    /// stays split.
    fn choose_theme(&mut self, side: Side, theme: &'static dyn crate::theme::Theme) {
        let halves: &[Appearance] = match side {
            Side::Both => &[Appearance::Light, Appearance::Dark],
            Side::Only(Appearance::Light) => &[Appearance::Light],
            Side::Only(Appearance::Dark) => &[Appearance::Dark],
        };
        let (mut preset, fresh) = (self.preset_edited(), LookPreset::fresh());
        for half in halves {
            if preset.get(*half).theme.id() != theme.id() {
                let settings = match fresh.get(*half) {
                    look if look.theme.id() == theme.id() => look.settings.clone(),
                    _ => crate::theme::setting::Values::defaults(theme.settings()),
                };
                *preset.get_mut(*half) = crate::theme::ThemeLook { theme, settings };
            }
        }
        self.keep_edited(preset);
        if side != Side::Both {
            self.ed_mut().split.insert(THEME);
        }
        self.show_colors();
    }

    /// In the sheet, a field for each color shown: one still shown keeps its text, a new one shows
    /// the color.
    fn show_colors(&mut self) {
        if self.sheet.is_none() {
            return;
        }
        let mut fields = BTreeMap::new();
        for shown in self.shown().into_iter().filter(|s| matches!(s.kind, Kind::Color { .. })) {
            let side = shown.side;
            let hex = self.color_hex(side, shown.key);
            let field = self.ed_mut().colors.remove(&(side, shown.key)).unwrap_or((hex, None));
            fields.insert((side, shown.key), field);
        }
        self.ed_mut().colors = fields;
    }

    /// The sheet's color fields show what they hold again, whatever was typed in them.
    fn refresh_colors(&mut self) {
        if let Some((_, ed)) = &mut self.sheet {
            ed.colors.clear();
        }
        self.show_colors();
    }

    /// What's shown in both windows: the saved preset the sheet's open on, else the Active look.
    fn shown_preset(&self) -> LookPreset {
        match &self.sheet {
            Some((Target::Saved(name), _)) => self.saved.iter().find(|s| s.name == *name).unwrap().preset.clone(),
            Some((Target::Active, _)) | None => self.preset.clone(),
        }
    }

    /// Whether the sheet is open on a saved preset, which both windows then preview.
    fn previewing(&self) -> bool {
        matches!(self.sheet, Some((Target::Saved(_), _)))
    }
}

struct Sheet;

/// The app, a handle on its fake's disk, the Look Preset it last kept, and what Follow theme
/// last sent to the Dock and the bundle.
struct Sut {
    d: Desk,
    disk: crate::service::Disk,
    kept: Rc<RefCell<Option<LookPreset>>>,
    /// The saved presets last kept.
    kept_saved: Rc<RefCell<Option<Vec<SavedPreset>>>>,
    dock: Rc<RefCell<Icons>>,
}

impl ReferenceStateMachine for Sheet {
    type State = Model;
    type Transition = Step;

    fn init_state() -> BoxedStrategy<Model> {
        let manifest = manifest();
        let ids: Vec<String> = manifest.icons().iter().map(|i| i.id.clone()).collect();
        let pairs: Vec<Icons> = manifest.pairs().iter().map(|p| p.icons()).collect();
        let home = Some(PathBuf::from(crate::service::DOWNLOADS));
        // A fresh install's; one theme in both halves, its values the same, so its fields start
        // joined; or any.
        let same = crate::theme::tests::theme_look().prop_map(|look| LookPreset { light: look.clone(), dark: look });
        // One with a color setting, so the color fields' steps are met often (#254).
        let colored = || {
            crate::theme::tests::theme_look()
                .prop_filter("a color setting", |look| look.settings.iter().any(|(setting, _)| matches!(setting.kind, Kind::Color { .. })))
        };
        // Both halves alike sometimes, so a color's field is one for both, and its Join
        // button meets it (#332).
        let colored = prop_oneof![
            (colored(), colored()).prop_map(|(light, dark)| LookPreset { light, dark }),
            colored().prop_map(|look| LookPreset { light: look.clone(), dark: look }),
        ];
        let preset = prop_oneof![1 => Just(LookPreset::fresh()), 1 => same, 2 => crate::theme::tests::preset(), 3 => colored];
        // Some saved, one of them the active one sometimes.
        // Some start with Settings open on Appearance, as after ⌘, and a click on its tab, and some
        // of those with a sheet open, the first saved preset's or the Active look's, as after
        // their Edit…: the most controls, reached in fewer steps.
        (0..4u8, preset, crate::theme::saved::tests::saved(), any::<bool>(), 0..7u8)
            .prop_map(move |(n, preset, mut saved, current, start)| {
                let name = Name::parse("Current").unwrap();
                if current && saved.iter().all(|s| s.name != name) {
                    saved.push(SavedPreset { name, preset: preset.clone() });
                }
                let mut m = Model {
                    stored: Stored {
                        folder: home.clone(),
                        port: ListenPort::Picked(NonZeroU16::new(51413).unwrap()),
                        upnp: true,
                        keep_awake: false,
                        speed_limits: SpeedLimits::default(),
                        seeding: SeedingLimits::default(),
                    },
                    open: false,
                    main: true,
                    front: Some(Which::Main),
                    tab: Tab::General,
                    field: String::new(),
                    port_refused: None,
                    folder_refused: false,
                    gone: BTreeMap::new(),
                    limit_fields: Default::default(),
                    limits_refused: Default::default(),
                    seeding_fields: Default::default(),
                    seeding_refused: Default::default(),
                    choice: None,
                    icons_open: false,
                    visit: 0,
                    icons_was: None,
                    added: Vec::new(),
                    ids: ids.clone(),
                    pairs: pairs.clone(),
                    restarts_fail: n == 0,
                    errors: 0,
                    focus: None,
                    preset,
                    dark: true,
                    sheet: None,
                    opened: None,
                    focused: None,
                    changes: Vec::new(),
                    told: Vec::new(),
                    rows: TORRENTS.iter().map(|(id, _)| *id).collect(),
                    saved,
                    save_as: None,
                    name_refused: None,
                    name_focused: false,
                };
                if start < 5 {
                    m.opened();
                    m.tab = Tab::Appearance;
                }
                // The App icon's sheet open.
                if start == 2 {
                    m.open_icons();
                }
                if start == 3 && !m.saved.is_empty() {
                    m.open_editor(Target::Saved(m.saved[0].name.clone()));
                }
                if start == 4 {
                    m.open_editor(Target::Active);
                }
                m
            })
            .boxed()
    }

    fn transitions(m: &Model) -> BoxedStrategy<Step> {
        if !m.open {
            let reset = proptest::sample::subsequence(SETTINGS.to_vec(), 1..=SETTINGS.len()).prop_shuffle();
            return prop_oneof![
                2 => Just(Step::Open),
                1 => Just(Step::CmdW),
                1 => Just(Step::Reopen),
                1 => reset.prop_map(Step::OpenReset),
                2 => (0..3usize).prop_map(Step::FolderGone),
                1 => Just(Step::FolderBack),
                1 => any::<bool>().prop_map(Step::Appearance),
                1 => proptest::sample::select(others()).prop_map(Step::Other),
                1 => Just(Step::OtherUndo),
                3 => Just(Step::Undo(Which::Main)),
                2 => (0..TORRENTS.len() as u64).prop_map(Step::Remove),
                1 => (0..TORRENTS.len() as u64).prop_map(Step::OtherRemove),
                1 => Just(Step::OtherUndoAction),
            ]
            .boxed();
        }
        // Every control the sheet shows now, as a step on it.
        let editor = || {
            let mut steps: Vec<BoxedStrategy<Step>> = Vec::new();
            for shown in m.shown() {
                let (key, side) = (shown.key, shown.side);
                steps.push(match shown.kind {
                    Kind::Choice { choices, show: Show::Segments, .. } => prop_oneof![
                        3 => (0..choices.len()).prop_map(move |i| Step::Choose(side, key, i)),
                        2 => any::<bool>().prop_map(move |right| Step::Arrow(side, key, right)),
                    ]
                    .boxed(),
                    Kind::Choice { choices, .. } => (0..choices.len()).prop_map(move |i| Step::Choose(side, key, i)).boxed(),
                    Kind::Toggle { .. } => Just(Step::Flip(side, key)).boxed(),
                    Kind::Color { .. } => prop_oneof![
                        proptest::sample::select(color_texts()).prop_map(move |text| Step::TypeColor(side, key, text)),
                        Just(Step::SendColor(side, key)),
                    ]
                    .boxed(),
                });
                if m.same_theme() {
                    steps.push(prop_oneof![Just(Step::Split(key)), Just(Step::Join(key))].boxed());
                }
            }
            if m.same_theme() {
                steps.push(prop_oneof![Just(Step::Split(THEME)), Just(Step::Join(THEME))].boxed());
            }
            let sides = m.theme_sides();
            steps.push(
                (proptest::sample::select(sides), 0..crate::theme::themes().len()).prop_map(|(side, i)| Step::Theme(side, i)).boxed(),
            );
            proptest::strategy::Union::new(steps)
        };
        // The App icon's sheet: its chips, adding and removing an image, Done.
        if m.icons_open {
            let tiles = proptest::sample::select(m.tiles());
            let half = proptest::option::of(prop_oneof![Just(Appearance::Light), Just(Appearance::Dark)]);
            let mut steps: Vec<(u32, BoxedStrategy<Step>)> = vec![
                (8, (tiles, half).prop_map(|(tile, half)| Step::IconChip(tile, half)).boxed()),
                (3, (0..m.pairs.len()).prop_map(Step::PairTile).boxed()),
                (2, proptest::option::of(0..images().len()).prop_map(Step::AddImage).boxed()),
                (1, (0..images().len()).prop_map(Step::DropImage).boxed()),
                (3, Just(Step::IconsDone).boxed()),
                (2, any::<bool>().prop_map(|escape| Step::Close { escape }).boxed()),
                (1, Just(Step::CmdW).boxed()),
                (1, prop_oneof![Just(Which::Main), Just(Which::Settings)].prop_map(Step::Front).boxed()),
                (1, any::<bool>().prop_map(Step::Appearance).boxed()),
                (3, Just(Step::Undo(Which::Settings)).boxed()),
                (1, Just(Step::Undo(Which::Main)).boxed()),
                (1, proptest::sample::select(others()).prop_map(Step::Other).boxed()),
            ];
            if !m.added.is_empty() {
                steps.push((2, (0..m.added.len()).prop_map(Step::RemoveImage).boxed()));
            }
            return proptest::strategy::Union::new_weighted(steps).boxed();
        }
        // The sheet is modal: its controls, Done, Escape and ⌘W, and what comes from outside it.
        if m.sheet.is_some() {
            return prop_oneof![
                12 => editor(),
                3 => Just(Step::Done),
                2 => Just(Step::Cancel),
                3 => any::<bool>().prop_map(|escape| Step::Close { escape }),
                2 => Just(Step::CmdW),
                1 => Just(Step::Blur),
                1 => prop_oneof![Just(Which::Main), Just(Which::Settings)].prop_map(Step::Front),
                1 => any::<bool>().prop_map(Step::Appearance),
                3 => Just(Step::Undo(Which::Settings)),
                2 => Just(Step::Undo(Which::Main)),
                1 => (0..TORRENTS.len() as u64).prop_map(Step::Remove),
                1 => proptest::sample::select(others()).prop_map(Step::Other),
                1 => Just(Step::OtherUndo),
            ]
            .boxed();
        }
        // The Look Presets' controls, and Save as…'s.
        // Save as… often enough to reach its question and replace.
        let mut presets: Vec<(u32, BoxedStrategy<Step>)> = match &m.save_as {
            None => vec![(3, Just(Step::SaveAs).boxed())],
            Some((_, Some(_))) => vec![(6, Just(Step::Replace).boxed()), (1, Just(Step::CancelReplace).boxed())],
            Some((_, None)) => vec![
                // Saving what's typed matters once something is.
                (if text_typed(m) { 2 } else { 6 }, proptest::sample::select(name_texts(m)).prop_map(Step::TypeName).boxed()),
                (if text_typed(m) { 6 } else { 1 }, any::<bool>().prop_map(|by_return| Step::SaveName { by_return }).boxed()),
            ],
        };
        presets.push((3, Just(Step::EditActive).boxed()));
        presets.push((2, prop_oneof![Just(Appearance::Light), Just(Appearance::Dark)].prop_map(Step::ActiveChip).boxed()));
        if !m.saved.is_empty() {
            let n = m.saved.len();
            let half = proptest::option::of(prop_oneof![Just(Appearance::Light), Just(Appearance::Dark)]);
            presets.push((3, (0..n, half).prop_map(|(i, half)| Step::Apply(i, half)).boxed()));
            presets.push((2, (0..n).prop_map(Step::Edit).boxed()));
            presets.push((1, (0..n).prop_map(Step::DeletePreset).boxed()));
        }
        let presets = proptest::strategy::Union::new_weighted(presets);
        prop_oneof![
            1 => Just(Step::Open),
            1 => any::<bool>().prop_map(|escape| Step::Close { escape }),
            1 => Just(Step::CmdW),
            1 => prop_oneof![Just(Which::Main), Just(Which::Settings)].prop_map(Step::Front),
            1 => Just(Step::Reopen),
            // Appearance most: it has the most controls.
            4 => prop_oneof![1 => Just(Tab::General), 1 => Just(Tab::Network), 3 => Just(Tab::Appearance)].prop_map(Step::Tab),
            4 => proptest::sample::select(port_texts()).prop_map(Step::TypePort),
            3 => Just(Step::SendPort),
            3 => Just(Step::Blur),
            3 => (direction(), proptest::sample::select(limit_texts())).prop_map(|(d, text)| Step::TypeLimit(d, text)),
            3 => direction().prop_map(Step::SendLimit),
            3 => (at(), proptest::sample::select(seeding_texts())).prop_map(|(at, text)| Step::TypeSeeding(at, text)),
            3 => at().prop_map(Step::SendSeeding),
            2 => Just(Step::PickPort),
            3 => (prop_oneof![Just(Switch::Upnp), Just(Switch::KeepAwake)], any::<bool>()).prop_map(|(switch, twice)| Step::Toggle { switch, twice }),
            3 => proptest::option::of(0..FOLDERS.len()).prop_map(Step::ChooseFolder),
            1 => Just(Step::ClearFolder),
            2 => (0..3usize).prop_map(Step::FolderGone),
            1 => Just(Step::FolderBack),
            3 => Just(Step::EditIcons),
            // On their tab, often enough to save a name taken and undo what they did.
            if m.tab == Tab::Appearance { 40 } else { 1 } => presets,
            1 => any::<bool>().prop_map(Step::Appearance),
            // More often after a Look Preset's editor's Done: its undo puts the preset back whole.
            if theme_edit_last(m) { 20 } else { 6 } => Just(Step::Undo(Which::Settings)),
            3 => Just(Step::Undo(Which::Main)),
            1 => (0..TORRENTS.len() as u64).prop_map(Step::Remove),
            2 => proptest::sample::select(others()).prop_map(Step::Other),
            1 => Just(Step::OtherUndo),
            1 => (0..TORRENTS.len() as u64).prop_map(Step::OtherRemove),
            1 => Just(Step::OtherUndoAction),
            1 => any::<bool>().prop_map(|fail| Step::Restarts { fail }),
        ]
        .boxed()
    }

    /// Each control is only there on its own tab, and some only sometimes.
    fn preconditions(m: &Model, step: &Step) -> bool {
        // The sheet is modal: nothing of the tab behind it takes a press.
        let on = |tab| m.open && m.tab == tab && m.sheet.is_none() && !m.icons_open;
        let editor = m.open && m.sheet.is_some();
        let icons = m.open && m.icons_open;
        match step {
            Step::Open | Step::CmdW => m.front.is_some(),
            Step::Front(Which::Main) => m.main,
            Step::Front(Which::Settings) => m.open,
            Step::Reopen => !m.main,
            // The notice is in the main window's status bar.
            Step::OpenReset(_) => m.main && !m.open,
            Step::Close { .. } | Step::Tab(_) | Step::Blur => m.open,
            Step::TypePort(_) | Step::SendPort | Step::Toggle { switch: Switch::Upnp, .. } => on(Tab::Network),
            Step::TypeLimit(..) | Step::SendLimit(_) => on(Tab::Network),
            Step::TypeSeeding(..) | Step::SendSeeding(_) => on(Tab::General),
            Step::Toggle { switch: Switch::KeepAwake, .. } => on(Tab::General),
            Step::PickPort => on(Tab::Network) && matches!(m.stored.port, ListenPort::Pinned(_)),
            Step::FolderGone(_) | Step::FolderBack => m.stored.folder.is_some(),
            Step::ChooseFolder(_) => on(Tab::General),
            Step::ClearFolder => on(Tab::General) && m.stored.folder.is_some(),
            Step::EditIcons | Step::ActiveChip(_) => on(Tab::Appearance),
            Step::IconChip(tile, _) => icons && m.tiles().contains(tile),
            Step::PairTile(i) => icons && *i < m.pairs.len(),
            Step::AddImage(_) | Step::DropImage(_) | Step::IconsDone => icons,
            Step::RemoveImage(i) => icons && *i < m.added.len(),
            Step::SaveAs => on(Tab::Appearance) && m.save_as.is_none(),
            Step::TypeName(_) | Step::SaveName { .. } => on(Tab::Appearance) && matches!(m.save_as, Some((_, None))),
            Step::Replace | Step::CancelReplace => on(Tab::Appearance) && matches!(m.save_as, Some((_, Some(_)))),
            Step::Apply(i, _) | Step::Edit(i) | Step::DeletePreset(i) => on(Tab::Appearance) && *i < m.saved.len(),
            Step::EditActive => on(Tab::Appearance),
            Step::Done | Step::Cancel => m.open && m.sheet.is_some(),
            Step::Arrow(side, key, _)
            | Step::Choose(side, key, _)
            | Step::Flip(side, key)
            | Step::TypeColor(side, key, _)
            | Step::SendColor(side, key) => editor && m.shown().iter().any(|s| s.key == *key && s.side == *side),
            // A row joins and splits with one theme in both halves.
            Step::Split(THEME) => editor && m.theme_joined(),
            Step::Join(THEME) => editor && m.same_theme() && !m.theme_joined(),
            Step::Split(key) => editor && m.same_theme() && m.joined(key),
            Step::Join(key) => editor && m.same_theme() && m.shown().iter().any(|s| s.key == *key) && !m.joined(key),
            Step::Theme(side, i) => editor && m.theme_sides().contains(side) && *i < crate::theme::themes().len(),
            Step::Appearance(_) | Step::Other(_) | Step::OtherUndo | Step::Restarts { .. } | Step::OtherUndoAction => true,
            Step::Undo(Which::Settings) => m.open,
            Step::Undo(Which::Main) => m.main,
            // A row of the list, and ⌫, Settings open or not.
            Step::Remove(id) => m.main && m.rows.contains(id),
            Step::OtherRemove(id) => m.rows.contains(id),
        }
    }

    /// The step, and each change that took effect a change ⌘Z takes back.
    fn apply(m: Model, step: &Step) -> Model {
        let mut m = m;
        // What the step takes the focus from is applied first, as a change of its own (#254).
        let (before, was) = (m.stored.clone(), m.presets());
        let keeps = match step {
            Step::TypePort(_) | Step::SendPort => Some(Fld::Port),
            Step::TypeLimit(d, _) | Step::SendLimit(d) => Some(Fld::Limit(*d)),
            Step::TypeSeeding(at, _) | Step::SendSeeding(at) => Some(Fld::Seeding(*at)),
            Step::TypeColor(side, key, _) | Step::SendColor(side, key) => Some(Fld::Color(*side, key)),
            // The main window to the front: Settings losing it leaves its field (#254), so ⌘Z
            // there takes that change first.
            Step::Front(Which::Main) | Step::Reopen | Step::Remove(_) | Step::Undo(Which::Main) if m.front == Some(Which::Settings) => None,
            // No click, no key into another control of Settings: its focus stays.
            Step::Front(_) | Step::Reopen | Step::Remove(_) | Step::Undo(Which::Main) => m.focused,
            Step::Open | Step::CmdW if m.front == Some(Which::Main) => m.focused,
            Step::Other(_)
            | Step::OtherUndo
            | Step::OtherRemove(_)
            | Step::OtherUndoAction
            | Step::FolderGone(_)
            | Step::FolderBack
            | Step::Restarts { .. }
            | Step::Appearance(_) => m.focused,
            Step::Close { escape: true } if m.edited().is_some() => m.focused,
            _ => None,
        };
        if m.focused != keeps {
            let left = m.focused;
            let target = match left {
                Some(Fld::Color(..)) => m.sheet.as_ref().map(|(target, _)| target.clone()),
                Some(Fld::Port | Fld::Limit(_) | Fld::Seeding(_)) | None => None,
            };
            if let Some(f) = left {
                m.leave(f);
            }
            m.focused = keeps;
            let changed: Vec<SettingName> = SettingName::ALL.into_iter().filter(|name| !m.stored.same(&before, *name)).collect();
            for name in changed {
                m.push(Change::Service(name, before.clone()));
            }
            if let (Some(Fld::Color(side, key)), Some(target)) = (left, target)
                && m.presets() != was
            {
                m.push(Change::Theme(was, target, side, key));
            }
        }
        let (stored, choice, presets) = (m.stored.clone(), m.choice.clone(), m.presets());
        let sheet = m.sheet.as_ref().map(|(target, _)| target.clone());
        let replacing = m.save_as.as_ref().and_then(|(_, replace)| replace.clone());
        let mut m = act(m, step);
        match step {
            // Two changes, the second's setting as the first left it.
            Step::Toggle { switch, twice: true } if !(*switch == Switch::Upnp && m.restarts_fail) => {
                let (name, flipped) = match switch {
                    Switch::Upnp => (SettingName::Upnp, Stored { upnp: !stored.upnp, ..stored.clone() }),
                    Switch::KeepAwake => (SettingName::KeepAwake, Stored { keep_awake: !stored.keep_awake, ..stored.clone() }),
                };
                m.push(Change::Service(name, stored));
                m.push(Change::Service(name, flipped));
            }
            Step::Undo(_) | Step::OtherUndo | Step::OtherUndoAction => {}
            Step::Remove(id) | Step::OtherRemove(id) => m.push(Change::Removed(*id)),
            _ => {
                let changed: Vec<SettingName> = SettingName::ALL.into_iter().filter(|name| !m.stored.same(&stored, *name)).collect();
                for name in changed {
                    m.push(Change::Service(name, stored.clone()));
                }
                if m.choice != choice {
                    m.push(Change::Icon(choice));
                    if m.icons_open {
                        m.visit += 1;
                    }
                }
                let theme = match step {
                    Step::Arrow(side, key, _) | Step::Choose(side, key, _) | Step::Flip(side, key) | Step::SendColor(side, key) => {
                        Some((*side, *key))
                    }
                    Step::Theme(side, _) => Some((*side, THEME)),
                    // A join copies the Light half's value, or theme, into the Dark one.
                    Step::Join(key) => Some((Side::Only(Appearance::Dark), *key)),
                    _ => None,
                };
                if let Some((side, key)) = theme.filter(|_| m.presets() != presets) {
                    m.push(Change::Theme(presets.clone(), sheet.clone().expect("the sheet was open"), side, key));
                }
                // An Apply, a save, a replace or a delete that changed something.
                let named = |i: usize| presets.saved[i].name.as_str().to_owned();
                // What it's called, the saved preset it's of, and the chip an undo focuses.
                let words = match step {
                    Step::Apply(i, None) => Some((format!("Apply {}", named(*i)), named(*i), None)),
                    Step::Apply(i, Some(half)) => Some((format!("Apply {}'s {}", named(*i), half.label()), named(*i), Some(*half))),
                    Step::DeletePreset(i) => Some((format!("Delete {}", named(*i)), named(*i), None)),
                    Step::SaveName { .. } => {
                        m.saved.last().map(|s| (format!("Save {}", s.name.as_str()), s.name.as_str().to_owned(), None))
                    }
                    Step::Replace => replacing.map(|name| match presets.saved.iter().any(|s| s.name.as_str() == name) {
                        true => (format!("Replace {name}"), name, None),
                        false => (format!("Save {name}"), name, None),
                    }),
                    _ => None,
                };
                if let Some((words, name, half)) = words.filter(|_| m.presets() != presets) {
                    m.push(Change::Presets(presets, words, name, half));
                }
            }
        }
        m
    }
}

impl Model {
    /// A change ⌘Z takes back, the newest; past the last 20 settings changes, the oldest of them
    /// goes.
    fn push(&mut self, change: Change) {
        self.changes.push(change);
        if self.changes.iter().filter(|c| !matches!(c, Change::Removed(_))).count() > 20 {
            let oldest = self.changes.iter().position(|c| !matches!(c, Change::Removed(_))).unwrap();
            self.changes.remove(oldest);
        }
    }

    /// Settings, just opened: on General, every field showing what's stored.
    fn opened(&mut self) {
        self.open = true;
        self.front = Some(Which::Settings);
        self.tab = Tab::General;
        self.field = self.port_text();
        self.limit_fields = Direction::BOTH.map(|d| self.limit_text(d));
        self.seeding_fields = SEEDING.map(|at| self.seeding_text(at));
        self.seeding_refused = Default::default();
        (self.port_refused, self.folder_refused, self.limits_refused) = (None, false, Default::default());
        self.close_icons();
        self.focused = None;
        (self.sheet, self.opened) = (None, None);
        (self.save_as, self.name_refused, self.name_focused) = (None, None, false);
        self.show_colors();
    }

    /// The open sheet's fields that showed what was stored `before` show what's stored now; what's
    /// typed over one stays.
    fn follow(&mut self, before: &Stored) {
        if !self.open {
            return;
        }
        let was = Model { stored: before.clone(), ..self.clone() };
        if self.field == was.port_text() {
            self.field = self.port_text();
        }
        for d in Direction::BOTH {
            if self.limit_fields[d as usize] == was.limit_text(d) {
                self.limit_fields[d as usize] = self.limit_text(d);
            }
        }
        for at in SEEDING {
            if self.seeding_fields[at as usize] == was.seeding_text(at) {
                self.seeding_fields[at as usize] = self.seeding_text(at);
            }
        }
    }

    /// The service's setting `name` back as `was` has it, its row showing it.
    fn back(&mut self, name: SettingName, was: &Stored) {
        let before = self.stored.clone();
        self.stored = self.stored.with(name, was);
        self.follow(&before);
        if !self.open {
            return;
        }
        match name {
            SettingName::ListenPort => (self.field, self.port_refused) = (self.port_text(), None),
            SettingName::DownloadLimit | SettingName::UploadLimit => {
                let d = if name == SettingName::DownloadLimit { Direction::Download } else { Direction::Upload };
                (self.limit_fields[d as usize], self.limits_refused[d as usize]) = (self.limit_text(d), None);
            }
            SettingName::SeedRatio | SettingName::SeedTime => {
                let at = if name == SettingName::SeedRatio { SeedingLimit::Ratio } else { SeedingLimit::Time };
                (self.seeding_fields[at as usize], self.seeding_refused[at as usize]) = (self.seeding_text(at), None);
            }
            SettingName::DefaultDestination => self.folder_refused = false,
            SettingName::Upnp | SettingName::KeepAwake => {}
        }
    }

    /// ⌘Z: the newest change. A setting's in Settings, opened if it was closed, on its tab, its
    /// control focused, and told; a torrent removed back in the list, and told.
    fn undo(&mut self) {
        // In a Look Preset's editor, only its own changes, newest first; in the App icon's sheet,
        // only this visit's picks.
        let at = match self.sheet {
            Some(_) => self.changes.iter().rposition(|c| matches!(c, Change::Theme(..))),
            None if self.icons_open => self.changes.iter().rposition(|c| matches!(c, Change::Icon(..))).filter(|_| self.visit > 0),
            None => self.changes.len().checked_sub(1),
        };
        let Some(at) = at else { return };
        let change = self.changes.remove(at);
        let refused = matches!(change, Change::Service(name, _) if self.restarts_fail && restarts(name));
        // An App icon pick undone in its sheet leaves it open, as it is.
        let in_sheet = self.icons_open && matches!(change, Change::Icon(..));
        // A setting's undo shows Settings in front, opened if it was closed.
        if !refused && !matches!(change, Change::Removed(_)) {
            if !self.open {
                self.opened();
            }
            self.front = Some(Which::Settings);
            if !in_sheet {
                // Its control takes the focus; a sheet over the window closes, so it shows.
                self.name_focused = false;
                self.close_icons();
            }
        }
        match change {
            Change::Removed(id) => {
                self.rows.insert(id);
                self.told.push("Back in the list".to_owned());
            }
            Change::Service(..) if refused => {
                self.told.push(client::display::undo_failed().to_owned());
            }
            Change::Service(name, was) => {
                self.close_sheet();
                self.back(name, &was);
                self.tab = tab_of(name.key()).unwrap();
                self.focus = Some(control_of(name).into());
                self.told.push(client::display::setting_undone_short(client::display::setting_short(name.key())));
            }
            Change::Icon(was) if in_sheet => {
                self.visit -= 1;
                self.choice = was;
                self.told.push(client::display::setting_undone_short("App icon"));
            }
            Change::Icon(was) => {
                self.close_sheet();
                self.choice = was;
                self.tab = Tab::Appearance;
                self.focus = Some("app-icon-edit".into());
                self.told.push(client::display::setting_undone_short("App icon"));
            }
            Change::Presets(was, _, name, half) => {
                self.restore(was);
                self.close_sheet();
                self.refresh_colors();
                self.tab = Tab::Appearance;
                // The chip that did it (◐, ☀ or ☾ of the saved preset), or, with the preset gone,
                // the Active look's Edit….
                self.focus = Some(match self.saved.iter().position(|s| s.name.as_str() == name) {
                    Some(i) => format!("preset-{i}-{}", half.map_or("both", Appearance::key)),
                    None => "active-edit".to_owned(),
                });
                self.told.push(client::display::look_preset_undone().to_owned());
            }
            Change::Edit(was, target) => {
                self.restore(was);
                self.close_sheet();
                self.refresh_colors();
                self.tab = Tab::Appearance;
                // The Look Presets' first chip.
                self.focus = (!self.saved.is_empty()).then(|| "preset-0-both".to_owned());
                // A saved Look Preset's name is the user's: the list's, not the line's.
                let words = match &target {
                    Target::Active => "Edit of the Active look".to_owned(),
                    Target::Saved(_) => client::display::of_a_saved_preset("Edit"),
                };
                self.told.push(format!("{words} undone"));
            }
            Change::Theme(was, target, side, key) => {
                let label = theme_label(&was, &target, key);
                self.restore(was);
                // Its sheet, open on it: ⌘Z takes these only while it is.
                // Its row as it was set: joined, or split.
                match side {
                    Side::Both => self.ed_mut().split.remove(key),
                    Side::Only(_) => self.ed_mut().split.insert(key),
                };
                // The fields show the values they went back to, whatever was typed in them.
                self.refresh_colors();
                self.tab = Tab::Appearance;
                let sides: Vec<Side> = match key {
                    THEME => self.theme_sides(),
                    _ => self.shown().iter().filter(|s| s.key == key).map(|s| s.side).collect(),
                };
                self.focus = crate::model::settings::focused_side(&sides, side).map(|shown| format!("edit-{key}-{}", shown.key()));
                let named = match side {
                    Side::Both => label.to_owned(),
                    Side::Only(a) => format!("{label} ({})", a.label()),
                };
                let named = match &target {
                    Target::Active => named,
                    Target::Saved(_) => client::display::of_a_saved_preset(&named),
                };
                self.told.push(client::display::setting_undone_short(&named));
            }
        }
    }

    /// The Look Presets back to `was`; a sheet whose preset is gone closes.
    fn restore(&mut self, was: Presets) {
        (self.preset, self.saved) = (was.active, was.saved);
        if matches!(&self.sheet, Some((Target::Saved(name), _)) if self.saved.iter().all(|s| s.name != *name)) {
            self.close_sheet();
        }
    }

    /// The sheet over the window closes (a Look Preset's, taking its fields' focus with it, its
    /// changes folded into one as Done does; the App icon's).
    fn close_sheet(&mut self) {
        self.close_editor(true);
        self.close_icons();
    }

    /// The App icon's sheet opens.
    fn open_icons(&mut self) {
        if !self.icons_open {
            (self.icons_open, self.visit, self.icons_was) = (true, 0, self.choice.clone());
        }
    }

    /// The App icon's sheet closes: what the visit changed is one step, as the editor's Done is.
    fn close_icons(&mut self) {
        if !std::mem::replace(&mut self.icons_open, false) {
            return;
        }
        for _ in 0..std::mem::take(&mut self.visit) {
            let at = self.changes.iter().rposition(|c| matches!(c, Change::Icon(..))).expect("a pick of the visit");
            self.changes.remove(at);
        }
        if self.choice != self.icons_was {
            self.push(Change::Icon(self.icons_was.clone()));
        }
    }

    /// A Look Preset's sheet opens on `target` (one open on another is done first).
    fn open_editor(&mut self, target: Target) {
        if self.sheet.as_ref().is_some_and(|(open, _)| *open == target) {
            return;
        }
        self.close_editor(true);
        (self.sheet, self.opened) = (Some((target, Ed::default())), Some(self.presets()));
        self.show_colors();
    }

    /// A Look Preset's sheet closes: its changes leave the list; Done's become one, if the presets
    /// aren't as it opened on them; Cancel's put them back so.
    fn close_editor(&mut self, done: bool) {
        let (Some((target, _)), Some(opened)) = (self.sheet.take(), self.opened.take()) else { return };
        if matches!(self.focused, Some(Fld::Color(..))) {
            self.focused = None;
        }
        self.changes.retain(|c| !matches!(c, Change::Theme(..)));
        match done {
            true if self.presets() != opened => self.push(Change::Edit(opened, target)),
            true => {}
            false => (self.preset, self.saved) = (opened.active, opened.saved),
        }
    }

    /// Another client's change, as the service takes it: a new port or UPnP fails while restarts
    /// do, and a folder that can't be reached is refused.
    fn other(&mut self, other: Other) {
        let before = self.stored.clone();
        match other {
            Other::Port(_) | Other::Upnp(_) if self.restarts_fail => {}
            Other::Port(port) => self.stored.port = ListenPort::Pinned(NonZeroU16::new(port).unwrap()),
            Other::Upnp(on) => self.stored.upnp = on,
            Other::KeepAwake(on) => self.stored.keep_awake = on,
            Other::DownloadLimit(limit) => self.stored.speed_limits.download = limit.map(|b| proto::SpeedLimit::new(b).unwrap()),
            Other::Folder(i) if self.gone.contains_key(&PathBuf::from(FOLDERS[i])) => {}
            Other::Folder(i) => self.stored.folder = Some(FOLDERS[i].into()),
        }
        self.follow(&before);
    }

    /// Another client undoes the newest torrent action: it's back in the list.
    fn other_undo_action(&mut self) {
        let Some(at) = self.changes.iter().rposition(|c| matches!(c, Change::Removed(_))) else { return };
        let Change::Removed(id) = self.changes.remove(at) else { unreachable!() };
        self.rows.insert(id);
    }

    /// Another client undoes the service's newest change; one refused is dropped.
    fn other_undo(&mut self) {
        let Some(at) = self.changes.iter().rposition(|c| matches!(c, Change::Service(..))) else { return };
        let Change::Service(name, was) = self.changes.remove(at) else { unreachable!() };
        if !(self.restarts_fail && restarts(name)) {
            let before = self.stored.clone();
            self.stored = self.stored.with(name, &was);
            self.follow(&before);
        }
    }
}

/// What `step` does to the model, besides the changes it makes.
fn act(mut m: Model, step: &Step) -> Model {
    m.focus = None;
    let name_focused = m.name_focused;
    // A click in Settings brings it to the front.
    let in_settings = !matches!(
        step,
        Step::Open
            | Step::CmdW
            | Step::Front(_)
            | Step::Reopen
            | Step::OpenReset(_)
            | Step::Close { escape: false }
            | Step::FolderGone(_)
            | Step::FolderBack
            | Step::Appearance(_)
            | Step::Undo(Which::Main)
            | Step::Remove(_)
            | Step::OtherRemove(_)
            | Step::OtherUndoAction
            | Step::Other(_)
            | Step::OtherUndo
            | Step::Restarts { .. }
    );
    if in_settings {
        m.front = Some(Which::Settings);
        // A press in Settings takes the focus from the name field; into it, below.
        m.name_focused = false;
    }
    let main = if m.main { Some(Which::Main) } else { None };
    match step {
        Step::Open if !m.open => m.opened(),
        Step::OpenReset(reset) => {
            m.opened();
            m.tab = tab_of(reset[0]).unwrap();
            let first = SETTINGS.into_iter().find(|name| tab_of(name) == Some(m.tab) && reset.contains(name)).unwrap();
            m.focus = Some(match first {
                // The first chip, if there's one.
                "saved_presets" if m.saved.is_empty() => String::new(),
                "saved_presets" => "preset-0-both".into(),
                "default_destination" => "choose-folder".into(),
                "listen_port" => "port-field".into(),
                "upnp" => "upnp-switch".into(),
                "keep_awake" => "keep-awake-switch".into(),
                "download_limit" => "download-limit-field".into(),
                "upload_limit" => "upload-limit-field".into(),
                "seed_ratio" => "seed-ratio-field".into(),
                "seed_time" => "seed-time-field".into(),
                "look_preset" => "active-edit".into(),
                _ => "app-icon-edit".into(),
            })
            .filter(|key| !key.is_empty());
        }
        Step::Open => m.front = Some(Which::Settings),
        Step::Front(which) => m.front = Some(*which),
        Step::Reopen => (m.main, m.front) = (true, Some(Which::Main)),
        // Escape in a field being edited puts the stored value back, and the sheet stays.
        Step::Close { escape: true } if let Some(f) = m.edited() => {
            *m.fld_text_mut(f) = m.fld_stored(f);
            *m.fld_refused_mut(f) = None;
        }
        // In Save as…'s field with a name typed: Save as… is cancelled.
        Step::Close { escape: true } if name_focused && m.save_as.as_ref().is_some_and(|(text, _)| !text.is_empty()) => {
            (m.save_as, m.name_refused) = (None, None)
        }
        // Else the sheet closes; else Save as…'s question goes, back to its name; else Save as… is
        // cancelled.
        // Else a Look Preset's sheet is cancelled, the App icon's closes.
        Step::Close { escape: true } if m.sheet.is_some() => m.close_editor(false),
        Step::Close { escape: true } if m.icons_open => m.close_sheet(),
        // Save as… shows on its tab only.
        Step::Close { escape: true } if m.tab == Tab::Appearance && matches!(m.save_as, Some((_, Some(_)))) => {
            m.save_as.as_mut().unwrap().1 = None;
            m.name_focused = true;
        }
        Step::Close { escape: true } if m.tab == Tab::Appearance && m.save_as.is_some() => (m.save_as, m.name_refused) = (None, None),
        Step::Close { .. } => {
            (m.open, m.focused, m.front, m.name_focused) = (false, None, main, false);
            m.close_sheet();
        }
        // The window in front closes, a saved preset's sheet first; the other stays.
        Step::CmdW if m.front == Some(Which::Settings) && (m.sheet.is_some() || m.icons_open) => m.close_sheet(),
        Step::CmdW if m.front == Some(Which::Settings) => {
            (m.open, m.focused, m.front, m.name_focused) = (false, None, main, false);
            m.close_sheet();
        }
        Step::CmdW => (m.main, m.front) = (false, if m.open { Some(Which::Settings) } else { None }),
        Step::Tab(tab) => m.tab = *tab,
        Step::TypePort(text) => m.field = text.clone(),
        Step::TypeLimit(d, text) => m.limit_fields[*d as usize] = text.clone(),
        Step::TypeSeeding(at, text) => m.seeding_fields[*at as usize] = text.clone(),
        Step::SendSeeding(at) => m.send(Fld::Seeding(*at)),
        Step::SendLimit(d) => m.send(Fld::Limit(*d)),
        Step::SendPort => m.send(Fld::Port),
        Step::Blur => {}
        Step::PickPort if m.restarts_fail => {
            m.errors += 1;
            m.port_refused = Some((m.field.clone(), None));
        }
        Step::PickPort => {
            let port = m.stored.port.port();
            // The fake's fresh port is the highest one free.
            let port = if port.get() == TAKEN { NonZeroU16::new(u16::MAX).unwrap() } else { port };
            m.stored.port = ListenPort::Picked(port);
            m.field = m.port_text();
            m.port_refused = None;
        }
        Step::Toggle { switch: Switch::Upnp, twice } => {
            let times = if *twice { 2 } else { 1 };
            if m.restarts_fail {
                m.errors += times;
            } else if times == 1 {
                m.stored.upnp = !m.stored.upnp;
            }
        }
        // Nothing restarts for it.
        Step::Toggle { switch: Switch::KeepAwake, twice } => {
            if !*twice {
                m.stored.keep_awake = !m.stored.keep_awake;
            }
        }
        Step::ChooseFolder(None) => {}
        // A folder set is one that can be reached.
        Step::ChooseFolder(Some(i)) if *i == GONE || m.gone.contains_key(&PathBuf::from(FOLDERS[*i])) => m.folder_refused = true,
        Step::ChooseFolder(Some(i)) => {
            m.stored.folder = Some(FOLDERS[*i].into());
            m.folder_refused = false;
        }
        Step::ClearFolder => {
            m.stored.folder = None;
            m.folder_refused = false;
        }
        Step::FolderGone(i) => {
            let folder = m.stored.folder.clone().unwrap();
            m.gone.insert(folder, *i);
        }
        Step::FolderBack => {
            let folder = m.stored.folder.clone().unwrap();
            m.gone.remove(&folder);
        }
        Step::EditIcons => {
            m.open_icons();
        }
        Step::IconsDone => m.close_icons(),
        Step::IconChip(tile, half) => {
            let id = m.tile(tile).0;
            m.chip_choice(id, *half);
        }
        Step::PairTile(i) => m.choice = Some(m.pairs[*i].clone()),
        Step::AddImage(None) => {}
        Step::AddImage(Some(image)) | Step::DropImage(image) => m.add(*image),
        // A half showing it follows the theme, never the other half's icon.
        Step::RemoveImage(i) => {
            let id = m.added.remove(*i);
            let halves = m.icon_halves().map(|half| half.filter(|h| *h != id));
            m.choose_halves(halves);
        }
        Step::Choose(side, key, i) => {
            let Some(Kind::Choice { choices, .. }) = m.shown().into_iter().find(|s| s.key == *key).map(|s| s.kind) else { unreachable!() };
            m.set(*side, key, Value::Choice(choices[*i].id));
        }
        Step::Arrow(side, key, right) => {
            let Some(Kind::Choice { choices, .. }) = m.shown().into_iter().find(|s| s.key == *key).map(|s| s.kind) else { unreachable!() };
            let Value::Choice(now) = m.value(*side, key) else { unreachable!() };
            let at = choices.iter().position(|c| c.id == now).unwrap();
            let to = if *right { (at + 1) % choices.len() } else { (at + choices.len() - 1) % choices.len() };
            m.set(*side, key, Value::Choice(choices[to].id));
            m.focus = Some(format!("edit-{key}-{}", side.key()));
        }
        Step::Flip(side, key) => {
            let Value::Toggle(on) = m.value(*side, key) else { unreachable!() };
            m.set(*side, key, Value::Toggle(!on));
        }
        Step::TypeColor(side, key, text) => *m.fld_text_mut(Fld::Color(*side, key)) = text.clone(),
        Step::SendColor(side, key) => m.send(Fld::Color(*side, key)),
        Step::Split(key) => {
            m.ed_mut().split.insert(key);
            m.show_colors();
        }
        Step::Join(THEME) => {
            m.ed_mut().split.remove(THEME);
            m.show_colors();
        }
        // The Dark half takes the Light one's value where they differ.
        Step::Join(key) => {
            let value = m.shown_value(Side::Only(Appearance::Light), key);
            if m.shown_value(Side::Only(Appearance::Dark), key) != value {
                m.set(Side::Only(Appearance::Dark), key, value);
            }
            m.ed_mut().split.remove(key);
            m.show_colors();
        }
        Step::Theme(side, i) => m.choose_theme(*side, crate::theme::themes()[*i]),
        Step::ActiveChip(_) => m.open_editor(Target::Active),
        Step::Apply(i, half) => {
            let saved = m.saved[*i].preset.clone();
            let halves = half.map_or(vec![Appearance::Light, Appearance::Dark], |h| vec![h]);
            let mut applied = m.preset.clone();
            for half in halves {
                *applied.get_mut(half) = saved.get(half).clone();
            }
            m.preset = applied;
        }
        Step::Edit(i) => m.open_editor(Target::Saved(m.saved[*i].name.clone())),
        Step::EditActive => m.open_editor(Target::Active),
        Step::Done => m.close_sheet(),
        Step::Cancel => m.close_editor(false),
        Step::DeletePreset(i) => drop(m.saved.remove(*i)),
        Step::SaveAs => (m.save_as, m.name_refused, m.name_focused) = (Some((String::new(), None)), None, true),
        Step::TypeName(text) => {
            m.save_as.as_mut().unwrap().0 = text.clone();
            m.name_focused = true;
        }
        Step::SaveName { by_return } => {
            m.name_focused = *by_return;
            let text = m.save_as.as_ref().unwrap().0.clone();
            match Name::parse(&text) {
                None => m.name_refused = Some((text, "A Look Preset needs a name".into())),
                // A name taken: the question, its Replace focused.
                Some(name) if m.saved.iter().any(|s| s.name == name) => {
                    m.save_as.as_mut().unwrap().1 = Some(name.as_str().to_owned());
                    m.name_focused = false;
                    m.focus = Some("save-as-replace".into());
                }
                Some(name) => {
                    m.saved.push(SavedPreset { name, preset: m.preset.clone() });
                    (m.save_as, m.name_refused) = (None, None);
                }
            }
        }
        // Saved over the one with its name; gone meanwhile (an undo took it), saved under it, last.
        Step::Replace => {
            let name = m.save_as.take().unwrap().1.unwrap();
            let active = m.preset.clone();
            match m.saved.iter_mut().find(|s| s.name.as_str() == name) {
                Some(saved) => saved.preset = active,
                None => m.saved.push(SavedPreset { name: Name::parse(&name).unwrap(), preset: active }),
            }
        }
        Step::CancelReplace => {
            m.save_as.as_mut().unwrap().1 = None;
            m.name_focused = true;
        }
        Step::Appearance(dark) => m.dark = *dark,
        Step::Undo(which) => {
            m.front = Some(*which);
            m.undo();
        }
        Step::Remove(id) => {
            m.rows.remove(id);
            m.front = Some(Which::Main);
        }
        Step::OtherRemove(id) => drop(m.rows.remove(id)),
        Step::OtherUndoAction => m.other_undo_action(),
        Step::Other(other) => m.other(*other),
        Step::OtherUndo => m.other_undo(),
        Step::Restarts { fail } => m.restarts_fail = *fail,
    }
    m
}

impl StateMachineTest for Sheet {
    type SystemUnderTest = Sut;
    type Reference = Sheet;

    fn init_test(m: &Model) -> Sut {
        let service = MemoryService::new()
            .with_torrents(TORRENTS.iter().map(|(id, name)| torrent(*id, name)).collect())
            .unreachable(FOLDERS[GONE], Unavailable::DriveNotConnected { name: "MyDisk".into() })
            .port_taken(TAKEN);
        let service = if m.restarts_fail { service.failing_restarts() } else { service };
        let disk = service.disk();
        let mut d = desk(service);
        let kept = Rc::new(RefCell::new(None));
        let keep = kept.clone();
        *d.main.state.looks.borrow_mut() =
            Looks::new(m.preset.clone(), move |preset: &LookPreset| *keep.borrow_mut() = Some(preset.clone()));
        // What the Dock and the bundle were last told to follow, starting from what they're given.
        let dock = Rc::new(RefCell::new(m.theme_icons()));
        let sent = dock.clone();
        d.settings.state.app_icon = AppIcon::new(Some(manifest()), None, m.theme_icons(), move |change| {
            if let IconChange::Theme(icons) = change {
                *sent.borrow_mut() = icons.clone();
            }
        });
        let kept_saved = Rc::new(RefCell::new(None));
        let keep = kept_saved.clone();
        d.settings.state.saved = Saved::new(m.saved.clone(), move |list: &[SavedPreset]| *keep.borrow_mut() = Some(list.to_vec()));
        d.main.set_viewport((960.0, 900.0), 2.0);
        d.settings.set_viewport((600.0, 900.0), 2.0);
        d.settle();
        if m.open {
            open_with_shortcut(&mut d);
            press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap()));
        }
        match &m.sheet {
            Some((Target::Saved(_), _)) => press(&mut d, "preset-0-edit"),
            Some((Target::Active, _)) => press(&mut d, "active-edit"),
            None => {}
        }
        if m.icons_open {
            press(&mut d, "app-icon-edit");
        }
        Sut { d, disk, kept, kept_saved, dock }
    }

    fn apply(mut sut: Sut, _: &Model, step: Step) -> Sut {
        let d = &mut sut.d;
        close_other_picker(d, &step);
        let cmd = |c| KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, ..Default::default() } };
        match &step {
            Step::Open => {
                blur_front(d);
                d.key(cmd(','));
                // Presses land once the sheet has come in, as for a person.
                d.wait(0.5);
            }
            Step::Close { escape: true } => {
                d.front(Which::Settings);
                d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            }
            Step::Close { escape: false } => d.close_button(Which::Settings),
            Step::CmdW => d.key(cmd('w')),
            Step::Front(which) => d.front(*which),
            Step::Reopen => d.reopen(),
            Step::Tab(tab) => press(d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| t == tab).unwrap())),
            Step::TypePort(text) => {
                press(d, "port-field");
                let r = &mut d.settings;
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            // A click on Settings' background.
            Step::Blur => {
                d.front(Which::Settings);
                d.settings.update(|_, tx| tx.blur());
            }
            Step::SendPort => {
                press(d, "port-field");
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::TypeLimit(direction, text) => {
                press(d, limit_field(*direction));
                let r = &mut d.settings;
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::TypeSeeding(at, text) => {
                press(d, seeding_field(*at));
                let r = &mut d.settings;
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendSeeding(at) => {
                press(d, seeding_field(*at));
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::SendLimit(direction) => {
                press(d, limit_field(*direction));
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::PickPort => press(d, "pick-port"),
            Step::Toggle { switch, twice: false } => press(d, switch.key()),
            // Both presses before a frame: the second meets the first on its way.
            Step::Toggle { switch, twice: true } => {
                d.front(Which::Settings);
                let r = &mut d.settings;
                scroll_to(r, switch.key());
                let (x, y) = r.find(switch.key()).expect("the switch");
                for _ in 0..2 {
                    r.pointer_down(x, y);
                    r.pointer_up(x, y);
                }
            }
            Step::OpenReset(reset) => {
                // The notice's Open settings, in the main window's status bar.
                let reset: Vec<String> = reset.iter().map(|name| name.to_string()).collect();
                d.main.update(move |s, _| s.settings.post(move |w, _| crate::model::settings::open_reset(w, &reset)));
                d.wait(0.5);
            }
            Step::ChooseFolder(folder) => {
                press(d, "choose-folder");
                d.settings.resolve_file_dialog(folder.map(|i| FOLDERS[i].into())).expect("the picker asks");
            }
            Step::ClearFolder => press(d, "clear-folder"),
            Step::FolderGone(i) => {
                sut.disk.change(DiskChange::DefaultGone { reason: reasons()[*i].clone() });
            }
            Step::FolderBack => {
                sut.disk.change(DiskChange::DefaultBack);
            }
            Step::EditIcons => press(d, "app-icon-edit"),
            Step::IconsDone => press(d, "icons-done"),
            Step::PairTile(i) => press(d, &format!("icon-pair-{i}")),
            Step::IconChip(tile, half) => {
                let key = m_tile_key(&d.settings.state.app_icon, tile);
                press(d, &format!("{key}-{}", half.map_or("both", Appearance::key)));
            }
            Step::AddImage(image) => {
                press(d, "icon-add");
                d.settings.resolve_file_dialog(image.map(|i| images()[i].clone())).expect("the tile asks for a file");
                d.settle();
            }
            Step::DropImage(image) => drop_on(d, "app_icon_tiles", images()[*image].clone()),
            // Its × shows while the pointer is on its tile.
            Step::RemoveImage(i) => {
                d.front(Which::Settings);
                scroll_to_hovering(&mut d.settings, &format!("icon-image-{i}"));
                press(d, &format!("icon-image-{i}-remove"));
            }
            Step::Choose(side, key, i) => choose(d, &format!("edit-{key}-{}", side.key()), *i),
            Step::Arrow(side, key, right) => {
                d.front(Which::Settings);
                d.settings.update(|_, tx| tx.blur());
                d.settle();
                let r = &mut d.settings;
                let control = format!("/edit-{key}-{}/", side.key());
                // Tab reaches the control, wherever the focus starts: a whole cycle of the sheet's
                // controls, when it's first in the order (an undo focused it).
                let mut tabs = 0;
                while !super::focus_path(r).is_some_and(|p| p.contains(&control)) {
                    assert!(tabs < 200, "Tab never reaches {control}");
                    r.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
                    super::settle(r);
                    tabs += 1;
                }
                r.key(KeyEvent { key: if *right { Key::Right } else { Key::Left }, mods: Mods::default() });
            }
            Step::Flip(side, key) => press(d, &format!("edit-{key}-{}", side.key())),
            Step::TypeColor(side, key, text) => {
                let field = format!("edit-{key}-{}", side.key());
                open_picker_of(d, &field);
                press(d, &field);
                let r = &mut d.settings;
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendColor(side, key) => {
                open_picker_of(d, &format!("edit-{key}-{}", side.key()));
                press(d, &format!("edit-{key}-{}", side.key()));
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::Split(key) => press(d, &format!("edit-{key}-split")),
            Step::Join(key) => press(d, &format!("edit-{key}-join")),
            Step::Theme(side, i) => {
                press(d, &format!("edit-theme-{}-trigger", side.key()));
                press(d, &format!("option{i}"));
            }
            Step::ActiveChip(half) => press(d, &format!("active-{}", half.key())),
            Step::Apply(i, half) => press(d, &format!("preset-{i}-{}", half.map_or("both", Appearance::key))),
            Step::Edit(i) => press(d, &format!("preset-{i}-edit")),
            Step::EditActive => press(d, "active-edit"),
            Step::DeletePreset(i) => press(d, &format!("preset-{i}-delete")),
            Step::Done => press(d, "edit-done"),
            Step::Cancel => press(d, "edit-cancel"),
            Step::SaveAs => press(d, "save-as-open"),
            Step::TypeName(text) => {
                press(d, "save-as-field");
                let r = &mut d.settings;
                r.key(select_all());
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SaveName { by_return: true } => {
                press(d, "save-as-field");
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::SaveName { by_return: false } => press(d, "save-as-save"),
            Step::Replace => press(d, "save-as-replace"),
            Step::CancelReplace => press(d, "save-as-cancel"),
            Step::Appearance(dark) => {
                d.main.set_system_dark(*dark);
                d.settings.set_system_dark(*dark);
            }
            Step::Undo(which) => {
                match (which, &d.settings.state.sheet) {
                    // The sheet is modal: the focus off its controls, as after a press on its
                    // background.
                    (Which::Settings, Some(sheet)) if sheet.over() => {
                        d.front(Which::Settings);
                        d.settings.update(|_, tx| tx.blur());
                        // A blur lands with the next frame, which comes before a person's next key.
                        d.settle();
                    }
                    (Which::Settings, Some(sheet)) => {
                        let tab = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap();
                        press(d, &format!("settings-tabs/#0/segment{tab}"));
                    }
                    (Which::Settings, None) => unreachable!("Settings is open"),
                    (Which::Main, _) => {
                        d.front(Which::Main);
                        d.main.update(|_, tx| tx.blur());
                    }
                }
                let closed = d.settings.state.sheet.is_none();
                d.key(cmd('z'));
                // Settled twice, as a press is (`press`, then below): a look an undo rebuilt moves
                // the layout over a second frame, under the next step's click.
                d.settle();
                // A setting's undo from the main window opens Settings: presses land once it has
                // come in, as after ⌘,.
                if closed && d.settings.state.sheet.is_some() {
                    d.wait(0.5);
                }
            }
            Step::Remove(id) => {
                d.front(Which::Main);
                super::tap(&mut d.main, &format!("rows/{id}"));
                d.main.key(shortcuts::keys(crate::commands::OnTorrents::Remove)[0].event());
            }
            Step::OtherRemove(id) => sut.disk.change(DiskChange::RemovedByAnother(*id)),
            Step::OtherUndoAction => sut.disk.change(DiskChange::TorrentUndoneByAnother),
            Step::Other(other) => sut.disk.change(DiskChange::SetByAnother(other.setting())),
            Step::OtherUndo => sut.disk.change(DiskChange::UndoneByAnother),
            Step::Restarts { fail } => sut.disk.change(DiskChange::RestartsFailing(*fail)),
        }
        d.settle();
        sut
    }

    fn check_invariants(sut: &Sut, m: &Model) {
        let d = &sut.d;
        let (s, w, rt) = (&d.main.state, &d.settings.state, &d.settings);
        assert_eq!(d.shown(), Showing { main: m.main, settings: m.open, front: m.front }, "the windows that show, and the one in front");
        assert_eq!(w.sheet.is_some(), m.open, "Settings is open");
        assert_eq!(rt.find("settings-sheet").is_some(), m.open, "and drawn");
        assert_eq!(w.app_icon.choice, m.choice, "the App icon");
        let listed = s.bar.entries().iter().filter(|e| e.notice.short == client::display::setting_not_changed()).count();
        assert_eq!(listed, m.errors, "each failed restart is listed");
        let told: Vec<&str> = s
            .bar
            .entries()
            .iter()
            .map(|e| e.notice.short.as_str())
            .filter(|short| short.ends_with(" undone") || *short == "Back in the list" || short.starts_with("Couldn't undo"))
            .collect();
        assert_eq!(told, m.told, "each undo tells what it went back to, or what came back, or why it couldn't be undone");
        // One failure, one look: a setting's undo that couldn't be done is an error, as a torrent's is.
        for e in s.bar.entries().iter().filter(|e| e.notice.short.starts_with("Couldn't undo")) {
            assert_eq!(e.notice.severity, crate::notice::Severity::Error, "{:?}", e.notice.short);
        }
        let listed: BTreeSet<u64> = s.torrents.iter().map(|t| t.id).collect();
        assert_eq!(listed, m.rows, "the torrents in the list, behind the sheet too");
        // In a Look Preset's editor, ⌘Z takes its own changes only.
        let newest = match m.sheet {
            Some(_) => m.changes.iter().rev().find(|c| matches!(c, Change::Theme(..))),
            // In the App icon's sheet, this visit's picks only.
            None if m.icons_open => m.changes.iter().rev().find(|c| matches!(c, Change::Icon(..))).filter(|_| m.visit > 0),
            None => m.changes.last(),
        };
        let newest = newest.map(|change| match change {
            Change::Service(name, _) => client::display::undo_change(client::display::setting_short(name.key())),
            // The pick, or the visit, from `was` to what the icon is now: it's the newest change.
            Change::Icon(was) => {
                let name = |choice: Option<&Icons>| crate::model::settings::icon_full_name(&w.app_icon, choice);
                format!("Undo {}", client::display::app_icon_change(&name(was.as_ref()), &name(m.choice.as_ref())))
            }
            Change::Theme(presets, target, _, key) => {
                let label = theme_label(presets, target, key);
                match target {
                    Target::Active => client::display::undo_change(label),
                    Target::Saved(name) => format!("Undo {label} of {}", name.as_str()),
                }
            }
            Change::Presets(_, words, ..) => format!("Undo {words}"),
            Change::Edit(_, Target::Active) => "Undo Edit of the Active look".to_owned(),
            Change::Edit(_, Target::Saved(name)) => format!("Undo Edit of {}", name.as_str()),
            Change::Removed(id) => format!("Undo Remove of {}", name_of(*id)),
        });
        // While a field is being edited, ⌘Z is its own: Edit ▸ Undo doesn't name a setting's change.
        let focused = rt.elements().into_iter().find(|e| e.focused).map(|e| e.path).unwrap_or_default();
        let mut fields = vec![("port-field".to_owned(), m.field.clone(), m.port_text())];
        for d in Direction::BOTH {
            fields.push((limit_field(d).to_owned(), m.limit_fields[d as usize].clone(), m.limit_text(d)));
        }
        for at in SEEDING {
            fields.push((seeding_field(at).to_owned(), m.seeding_fields[at as usize].clone(), m.seeding_text(at)));
        }
        for ((side, key), (text, _)) in m.ed_opt().map(|ed| ed.colors.clone()).unwrap_or_default() {
            fields.push((format!("edit-{key}-{}", side.key()), text.clone(), m.color_hex(side, key)));
        }
        // Save as…'s field: being edited with any name typed.
        if let Some((text, _)) = &m.save_as {
            fields.push(("save-as-field".to_owned(), text.clone(), String::new()));
        }
        let editing = m.open
            && m.front == Some(Which::Settings)
            && fields.iter().any(|(key, text, stored)| focused.contains(&format!("/{key}/")) && text != stored);
        let title = newest.filter(|_| !editing).unwrap_or_else(|| "Undo".to_owned());
        assert_eq!(crate::changes::menu_title(s), title, "Edit ▸ Undo names what ⌘Z takes back, whichever window is in front");
        let looks = s.looks.borrow();
        assert_eq!(looks.preset(), &m.preset, "the Active Look Preset");
        assert_eq!(sut.kept.borrow().as_ref().unwrap_or(&looks.preset().clone()), &m.preset, "kept as it is");
        assert_eq!(w.saved.list(), m.saved.as_slice(), "the saved Look Presets");
        assert_eq!(sut.kept_saved.borrow().as_deref().unwrap_or(w.saved.list()), m.saved.as_slice(), "kept as they are");
        // Both windows show a saved preset's sheet's preset while it's open, never kept; else the
        // Active look, which the Active look's sheet edits as it is.
        assert_eq!(looks.previewed(), m.previewing().then(|| m.shown_preset()).as_ref(), "the preview");
        let shown = Look::build(m.shown_preset().get(Appearance::of(m.dark)));
        assert_eq!(fill(&d.main, "app"), Some(shown.ui.tokens().c.page), "the main window in the look of the Appearance");
        if m.open {
            assert_eq!(fill(rt, "settings-sheet"), Some(shown.ui.tokens().c.layer_01), "and Settings, both redrawn at once");
        }
        assert_eq!(w.app_icon.theme, m.theme_icons(), "Follow theme shows each look's App icon");
        assert_eq!(*sut.dock.borrow(), m.theme_icons(), "and the Dock and the bundle are told");
        if let Some(key) = &m.focus {
            let focused = rt.elements().into_iter().find(|e| e.focused).map(|e| e.path);
            assert!(focused.as_deref().is_some_and(|p| p.contains(key.as_str())), "{key} is focused, not {focused:?}");
        }
        let Some(sheet) = &w.sheet else { return };
        assert_eq!(sheet.tab, m.tab);
        let settings = sheet.service.as_ref().expect("answered");
        // What's stored, as the service answers it: a refusal stored nothing.
        let stored = Stored {
            folder: settings.default_destination.clone().map(|f| f.into_path_buf()),
            port: settings.listen_port,
            upnp: settings.upnp,
            keep_awake: settings.keep_awake,
            speed_limits: settings.speed_limits,
            seeding: settings.seeding,
        };
        assert_eq!(stored, m.stored, "what's stored");
        for at in SEEDING {
            assert_eq!(sheet.seeding(at).text, m.seeding_fields[at as usize], "the {at:?} seeding limit's field");
        }
        assert_eq!(sheet.port.text, m.field, "the port field");
        for d in Direction::BOTH {
            assert_eq!(sheet.limit(d).text, m.limit_fields[d as usize], "the {d:?} limit's field");
        }

        // The tab shows its settings and no others.
        for name in SETTINGS {
            assert_eq!(rt.find(name).is_some(), tab_of(name) == Some(m.tab), "{name} on {:?}", m.tab);
        }
        if m.tab == Tab::Network {
            let refused = m.port_refused.as_ref().is_some_and(|(text, _)| *text == m.field);
            assert_eq!(rt.find("listen_port-refused").is_some(), refused, "the port's refusal shows until its text changes");
            if let Some((_, Some(why))) = m.port_refused.as_ref().filter(|_| refused) {
                assert_eq!(&text(rt, "listen_port-refused"), why);
            }
            for d in Direction::BOTH {
                let field = &m.limit_fields[d as usize];
                let refused = m.limits_refused[d as usize].as_ref().is_some_and(|(text, _)| text == field);
                let key = format!("{}-refused", crate::model::settings::limit_name(d));
                assert_eq!(rt.find(&key).is_some(), refused, "the {d:?} limit's refusal shows until its text changes");
                if let Some((_, Some(why))) = m.limits_refused[d as usize].as_ref().filter(|_| refused) {
                    assert_eq!(&text(rt, &key), why);
                }
            }
        }
        if m.tab == Tab::General {
            assert_eq!(rt.find("default_destination-refused").is_some(), m.folder_refused, "the folder's refusal");
            // The folder row says why the folder can't be reached, as the torrent rows word it.
            let gone = m.stored.folder.as_ref().and_then(|folder| m.gone.get(folder)).copied();
            assert_eq!(
                rt.find("default_destination-problem").is_some(),
                gone.is_some(),
                "the folder row says why it's unreachable, while it is"
            );
            if let Some(i) = gone {
                assert_eq!(text(rt, "default_destination-problem"), client::display::unavailable(&reasons()[i]), "why, worded by display");
            }
            for at in SEEDING {
                let field = &m.seeding_fields[at as usize];
                let refused = m.seeding_refused[at as usize].as_ref().is_some_and(|(text, _)| text == field);
                let key = format!("{}-refused", crate::model::settings::seeding_name(at));
                assert_eq!(rt.find(&key).is_some(), refused, "the {at:?} seeding limit's refusal shows until its text changes");
                if let Some((_, Some(why))) = m.seeding_refused[at as usize].as_ref().filter(|_| refused) {
                    assert_eq!(&text(rt, &key), why);
                }
            }
        }
        assert_eq!(
            sheet.save_as.as_ref().map(|a| a.name.text.clone()),
            m.save_as.as_ref().map(|(text, _)| text.clone()),
            "Save as…'s field"
        );
        assert_eq!(sheet.target(), m.sheet.as_ref().map(|(target, _)| target), "the sheet open, on what it edits");
        assert_eq!(rt.find("icons-sheet").is_some(), m.icons_open, "the App icon's sheet");
        assert_eq!(w.app_icon.added, m.added, "the images added");
        assert_eq!(rt.find("edit-sheet").is_some(), m.sheet.is_some(), "and drawn");
        if m.sheet.is_some() {
            check_theme_tab(rt, m);
        }
        if m.tab == Tab::Appearance {
            check_presets(rt, m);
        }
        if m.icons_open {
            check_icon_chips(rt, m);
        }
    }
}

/// Each row joined or split as the model says, each control with its value, Join or Split where it applies
/// (with a theme in each half, on the Theme row only), each Theme pop-up showing its half's theme,
/// and each color field's text and refusal.
fn check_theme_tab(rt: &Runtime<SettingsWindow>, m: &Model) {
    let prefix = "edit";
    let preset = m.preset_edited();
    for side in [Side::Both, Side::Only(Appearance::Light), Side::Only(Appearance::Dark)] {
        let trigger = format!("edit-theme-{}-trigger/value", side.key());
        let shows = rt.elements().into_iter().find(|e| e.path.ends_with(&trigger) && !e.exiting).and_then(|e| e.text);
        let theme = m.theme_sides().contains(&side).then(|| preset.get(side.appearances()[0]).theme.name());
        assert_eq!(shows.as_deref(), theme, "the Theme pop-up of {side:?} shows its half's theme");
    }
    assert_eq!(rt.find("halves-header").is_some(), m.same_theme(), "two columns named ☀ Light and ☾ Dark, all joined too");
    for half in ["light", "dark"] {
        assert!(rt.find(&format!("halves-header-{half}")).is_some(), "{half}'s heading, one theme or two");
    }
    assert_eq!(rt.find("edit-theme-split").is_some(), m.theme_joined(), "the Theme row's Split splits it while it's joined");
    for (what, label) in [("split", "Split"), ("join", "Join")] {
        if rt.find(&format!("edit-theme-{what}")).is_some() {
            assert_eq!(texts_in(rt, &format!("edit-theme-{what}")), [label], "its button says {label}");
        }
    }
    assert_eq!(
        rt.find("edit-theme-join").is_some(),
        m.same_theme() && !m.theme_joined(),
        "and joins it while it's split, one theme in both"
    );
    let keys: BTreeSet<&str> = m.shown().iter().map(|s| s.key).collect();
    let preset = m.preset_edited();
    let opened = rt.state.sheet.as_ref().and_then(|s| s.editor()).map(|e| e.open.clone()).unwrap_or_default();
    for key in keys {
        let sides: Vec<Side> = m.shown().iter().filter(|s| s.key == key).map(|s| s.side).collect();
        let setting = Appearance::BOTH.iter().find_map(|a| preset.get(*a).theme.settings().iter().find(|s| s.key == key)).unwrap();
        // A setting in a section that folds shows once it's opened; a color's control is its swatch.
        let shows = !setting.section.is_some_and(|s| s.folds && !opened.contains(s.key));
        let swatch = if matches!(setting.kind, Kind::Color { .. }) { "-swatch" } else { "" };
        for side in [Side::Both, Side::Only(Appearance::Light), Side::Only(Appearance::Dark)] {
            let control = format!("{prefix}-{key}-{}{swatch}", side.key());
            assert_eq!(rt.find(&control).is_some(), shows && sides.contains(&side), "{control} drawn");
        }
        let joined = m.joined(key);
        assert_eq!(rt.find(&format!("{prefix}-{key}-split")).is_some(), shows && m.same_theme() && joined, "{key}'s Split splits it");
        assert_eq!(
            rt.find(&format!("{prefix}-{key}-join")).is_some(),
            shows && m.same_theme() && !joined,
            "{key}'s Join joins it, with one theme only"
        );
        if !shows {
            continue;
        }
        for side in &sides {
            let shown = m.shown().into_iter().find(|s| s.key == key && s.side == *side).unwrap();
            let control = format!("{prefix}-{key}-{}", side.key());
            match (shown.kind, m.value(*side, key)) {
                (Kind::Choice { choices, show: Show::Segments, .. }, Value::Choice(id)) => {
                    let i = choices.iter().position(|c| c.id == id).unwrap();
                    let x = |path: String| rt.elements().into_iter().find(|e| e.path.ends_with(&path) && !e.exiting).map(|e| e.rect[0]);
                    let indicator = x(format!("{control}/#0/indicator"));
                    assert_eq!(indicator, x(format!("{control}/#0/segment{i}")), "{control} shows {id}");
                    // Accessible: a radio group named for the setting, a radio for each choice.
                    let group =
                        rt.elements().into_iter().find(|e| e.path.ends_with(&format!("{control}/#0")) && !e.exiting).expect("its group");
                    assert_eq!(
                        (group.role, group.label.as_deref()),
                        ("radiogroup", Some(shown_label(m, key))),
                        "{control}'s role and name"
                    );
                    for (j, choice) in choices.iter().enumerate() {
                        let radio = rt
                            .elements()
                            .into_iter()
                            .find(|e| e.path.ends_with(&format!("{control}/#0/segment{j}")) && !e.exiting)
                            .expect("a radio");
                        // Under a color's open picker, which is modal, nothing else takes the focus.
                        let picking = rt.state.sheet.as_ref().and_then(|s| s.editor()).is_some_and(|e| e.picker.is_some());
                        assert_eq!(
                            (radio.role, radio.label.as_deref(), radio.focusable),
                            ("radio", Some(choice.label), !picking),
                            "{control}'s {j}th choice"
                        );
                    }
                }
                (Kind::Choice { choices, .. }, Value::Choice(id)) => {
                    let label = choices.iter().find(|c| c.id == id).unwrap().label;
                    // A combobox named for the setting, showing the chosen label.
                    let trigger = rt
                        .elements()
                        .into_iter()
                        .find(|e| e.path.ends_with(&format!("{control}-trigger")) && !e.exiting)
                        .expect("its dropdown");
                    assert_eq!(
                        (trigger.role, trigger.label.as_deref()),
                        ("combobox", Some(shown_label(m, key))),
                        "{control}'s role and name"
                    );
                    let value = format!("{control}-trigger/value");
                    let shows = rt.elements().into_iter().find(|e| e.path.ends_with(&value) && !e.exiting).and_then(|e| e.text);
                    assert_eq!(shows.as_deref(), Some(label), "{control} shows {id}");
                }
                (Kind::Color { .. }, _) => {
                    let (typed, _) = &m.ed().colors[&(*side, key)];
                    let field =
                        rt.state.sheet.as_ref().and_then(|sheet| sheet.editor()).and_then(|e| e.color(*side, key)).expect("its field");
                    assert_eq!(&field.text.text, typed, "{control}'s text");
                }
                (Kind::Toggle { .. } | Kind::Choice { .. }, _) => {}
            }
        }
        let refused = sides
            .iter()
            .any(|side| m.ed().colors.get(&(*side, key)).is_some_and(|(text, why)| why.as_ref().is_some_and(|(t, _)| t == text)));
        let drawn = rt
            .elements()
            .into_iter()
            .any(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with(&format!("{prefix}-{key}-")) && k.ends_with("-refused")));
        assert_eq!(drawn, refused, "{key}'s refusal shows until its text changes");
    }
}

/// The Active look's row, then each saved preset listed in order: its chips (◐, ☀, ☾), each drawn
/// as its look, named for what a press does or, in use (the Active look's half, value for value,
/// ◐ when both), why it does nothing, and outlined; Edit… and Delete named for it; and Save as…:
/// its button, or its field and refusal until its text changes, or its question.
fn check_presets(rt: &Runtime<SettingsWindow>, m: &Model) {
    // A button's name is its control's, inside the element keyed for it.
    let label = |key: &str| rt.elements().into_iter().find(|e| e.path.ends_with(&format!("{key}/#0")) && !e.exiting).and_then(|e| e.label);
    let fill_of = |path: &str| rt.elements().into_iter().find(|e| e.path.ends_with(path) && !e.exiting).map(|e| e.fill);
    let page = |half: Option<LookPreset>, appearance: Appearance| Look::build(half.expect("a preset").get(appearance)).ui.tokens().c.page;
    // The Active look: a chip for each half, which only shows it.
    for half in Appearance::BOTH {
        let chip = format!("active-{}", half.key());
        assert_eq!(label(&chip), Some(format!("Edit the active look's {}", half.label())), "{chip}");
        assert_eq!(fill_of(&format!("{chip}/#0/face")), Some(page(Some(m.preset.clone()), half)), "{chip} is drawn as the look");
    }
    assert_eq!(label("active-edit"), Some("Edit the active look".to_owned()));
    for (i, saved) in m.saved.iter().enumerate() {
        let name = saved.name.as_str();
        for half in [None, Some(Appearance::Light), Some(Appearance::Dark)] {
            let chip = format!("preset-{i}-{}", half.map_or("both", Appearance::key));
            let halves = half.map_or(Appearance::BOTH.to_vec(), |h| vec![h]);
            let in_use = halves.iter().all(|h| saved.preset.get(*h) == m.preset.get(*h));
            let says = match (half, in_use) {
                (None, false) => format!("Apply {name} (Light and Dark) to the active look"),
                (None, true) => format!("{name} is the active look already"),
                (Some(half), false) => format!("Apply {name}'s {} to the active look", half.label()),
                (Some(half), true) => format!("{name}'s {} is the active look's {} already", half.label(), half.label()),
            };
            assert_eq!(label(&chip), Some(says), "{chip}");
            assert_eq!(rt.find(&format!("{chip}/#0/in-use")).is_some(), in_use, "{chip} is outlined while it's in use");
            // Each chip is drawn as its look: ◐ as the two side by side.
            match half {
                Some(half) => {
                    assert_eq!(fill_of(&format!("{chip}/#0/face")), Some(page(Some(saved.preset.clone()), half)), "{chip}'s look")
                }
                None => {
                    for half in Appearance::BOTH {
                        let side = format!("{chip}/#0/face/face-{}", half.key());
                        assert_eq!(fill_of(&side), Some(page(Some(saved.preset.clone()), half)), "{side}");
                    }
                }
            }
        }
        assert_eq!(label(&format!("preset-{i}-edit")), Some(format!("Edit {name}")));
        assert_eq!(label(&format!("preset-{i}-delete")), Some(format!("Delete {name}")));
    }
    assert!(rt.find(&format!("preset-{}-edit", m.saved.len())).is_none(), "no more listed");
    // The App icon's row: both halves' icons while they differ, else one.
    let [dark, light] = m.icon_sides();
    let preview = |h: &str| rt.find(&format!("app-icon-preview-{h}")).is_some();
    assert_eq!([preview("both"), preview("light"), preview("dark")], [dark == light, dark != light, dark != light], "the App icon's row");
    let shown = |key: &str| rt.find(key).is_some();
    assert!(shown("save-as-open"), "Save as…");
    assert_eq!(shown("save-as-field"), matches!(m.save_as, Some((_, None))), "its field");
    assert_eq!(shown("save-as-replace"), matches!(m.save_as, Some((_, Some(_)))), "its question");
    if let Some((_, Some(name))) = &m.save_as {
        assert_eq!(text(rt, "save-as-replace-ask"), format!("Replace {name}?"));
    }
    let refused = m.name_refused.as_ref().filter(|(text, _)| matches!(&m.save_as, Some((typed, None)) if typed == text));
    assert_eq!(rt.find("save-as-refused").is_some(), refused.is_some(), "the name's refusal shows until its text changes");
    if let Some((_, why)) = refused {
        assert_eq!(&text(rt, "save-as-refused"), why);
    }
}

fn seeding_field(at: SeedingLimit) -> &'static str {
    match at {
        SeedingLimit::Ratio => "seed-ratio-field",
        SeedingLimit::Time => "seed-time-field",
    }
}

fn limit_field(direction: Direction) -> &'static str {
    match direction {
        Direction::Download => "download-limit-field",
        Direction::Upload => "upload-limit-field",
    }
}

/// Presses the element at `key` in Settings, scrolling to it first: Settings is in front.
fn press(d: &mut Desk, key: &str) {
    d.front(Which::Settings);
    // A color's picker covers the window: a press outside it closes it first.
    let open = d.settings.state.sheet.as_ref().and_then(|s| s.editor()).and_then(|e| e.picker.as_ref()).map(|p| (p.side, p.key));
    if let Some((side, k)) = open
        && !key.starts_with(&format!("edit-{k}-{}", side.key()))
    {
        press_outside_picker(d);
    }
    // A setting's control in a section that folds shows once it's opened.
    if d.settings.find(key).is_none() {
        open_folded(d);
    }
    let rt = &mut d.settings;
    scroll_to(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    d.settle();
}

/// The picker a color's field `field` (`edit-<key>-<side>`) is in, opened by its swatch, the
/// section it's in opened first if it folds.
fn open_picker_of(d: &mut Desk, field: &str) {
    if d.settings.find(field).is_some() {
        return;
    }
    press(d, &format!("{field}-swatch"));
}

/// The first section that folds, opened, if one is closed.
fn open_folded(d: &mut Desk) {
    let open = d.settings.state.sheet.as_ref().and_then(|s| s.editor()).map(|e| e.open.clone()).unwrap_or_default();
    let header = d
        .settings
        .elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key)
        .find(|k| k.ends_with("-section") && !open.iter().any(|o| k.contains(o)));
    if let Some(header) = header {
        press(d, &header);
    }
}

/// A picker open on a color other than the step's closes before it, as a press outside it closes
/// it (what's typed in its field applied, as the field losing the focus applies it); one whose
/// field Escape is for stays.
fn close_other_picker(d: &mut Desk, step: &Step) {
    let open = d.settings.state.sheet.as_ref().and_then(|s| s.editor()).and_then(|e| e.picker.as_ref()).map(|p| (p.side, p.key));
    let Some((side, key)) = open else { return };
    let keeps = match step {
        Step::TypeColor(s, k, _) | Step::SendColor(s, k) => *s == side && *k == key,
        // Escape in a field being edited is the field's: what's typed goes, the picker stays.
        Step::Close { escape: true } => {
            let w = &d.settings.state;
            let preset = w.sheet.as_ref().and_then(|s| s.target()).and_then(|t| Presets::of(&w.looks.borrow(), &w.saved).get(t).cloned());
            let text = w.sheet.as_ref().and_then(|s| s.editor()).and_then(|e| e.color(side, key)).map(|f| f.text.text.clone());
            let stored = preset.and_then(|p| crate::model::settings::shown_color(&p, side, key)).map(|rgb| rgb.hex());
            text != stored
        }
        // Steps that don't press in Settings: what happens to the picker is the app's.
        Step::Open
        | Step::CmdW
        | Step::Close { escape: false }
        | Step::Front(_)
        | Step::Reopen
        | Step::Remove(_)
        | Step::Undo(Which::Main)
        | Step::Other(_)
        | Step::OtherUndo
        | Step::OtherRemove(_)
        | Step::OtherUndoAction
        | Step::FolderGone(_)
        | Step::FolderBack
        | Step::Restarts { .. }
        | Step::Appearance(_) => true,
        _ => false,
    };
    // A press in Settings, while it's in front: one behind would come to the front first. Escape
    // and ⌘Z in Settings bring it to the front themselves; the model's Escape is the sheet's, and
    // its ⌘Z comes after a press on the sheet's background.
    if !keeps && matches!(step, Step::Close { escape: true } | Step::Undo(Which::Settings)) {
        d.front(Which::Settings);
    }
    if !keeps && d.shown().front == Some(Which::Settings) {
        press_outside_picker(d);
    }
}

/// A press in a corner of Settings, which a color's picker never covers: it closes.
fn press_outside_picker(d: &mut Desk) {
    d.settings.pointer_down(2.0, 2.0);
    d.settings.pointer_up(2.0, 2.0);
    d.settle();
}

/// The centre of the `i`th option of the open dropdown `control`'s list.
fn option_at(rt: &Runtime<SettingsWindow>, control: &str, i: usize) -> (f32, f32) {
    let list = format!("{control}-menu-menu");
    let item = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains(&list) && !e.exiting && e.role == "menuitemcheckbox")
        .nth(i)
        .unwrap_or_else(|| panic!("no option {i} in {control}'s list"));
    (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0)
}

/// Presses the `i`th segment of `control`, or opens its dropdown and presses its `i`th option.
fn choose(d: &mut Desk, control: &str, i: usize) {
    let segment = format!("{control}/#0/segment{i}");
    if d.settings.find(&segment).is_some() {
        return press(d, &segment);
    }
    press(d, &format!("{control}-trigger"));
    let (x, y) = option_at(&d.settings, control, i);
    d.settings.pointer_down(x, y);
    d.settings.pointer_up(x, y);
    d.settle();
}

/// The key of an App icon tile.
fn m_tile_key(icon: &AppIcon, tile: &IconTile) -> String {
    match tile {
        IconTile::Follow => "icon-follow".into(),
        IconTile::Icon(id) => format!("icon-{id}"),
        IconTile::Added(i) => {
            assert!(*i < icon.added.len(), "added image {i}");
            format!("icon-image-{i}")
        }
    }
}

/// Every designed pair's tile, outlined while both halves are its icons; every tile's chips in
/// the App icon's sheet (Follow theme's too), outlined while in use: ◐ while both halves are that
/// icon (or follow the theme), else ☀ while the Light half is and ☾ while the Dark half is.
fn check_icon_chips(rt: &Runtime<SettingsWindow>, m: &Model) {
    let in_use = |key: &str| rt.find(&format!("{key}/#0/in-use")).is_some();
    let [dark, light] = m.icon_halves();
    for (i, pair) in m.pairs.iter().enumerate() {
        let Icons::Pair { dark: d, light: l } = pair else { unreachable!("a pair") };
        let used = dark.as_ref() == Some(d) && light.as_ref() == Some(l);
        assert_eq!(rt.find(&format!("icon-pair-{i}/in-use")).is_some(), used, "pair {i} in use");
    }
    for tile in m.tiles() {
        let (id, key) = m.tile(&tile);
        assert!(rt.find(&key).is_some(), "{key} drawn");
        let both = dark == id && light == id;
        assert_eq!(in_use(&format!("{key}-both")), both, "{key}'s ◐");
        assert_eq!(in_use(&format!("{key}-light")), !both && light == id, "{key}'s ☀");
        assert_eq!(in_use(&format!("{key}-dark")), !both && dark == id, "{key}'s ☾");
    }
    assert!(rt.find(&format!("icon-image-{}", m.added.len())).is_none(), "no more added");
    assert!(rt.find("icon-add").is_some(), "+ Add image…");
}

/// Drops the file at `path` on the element at `key` in Settings, from Finder: Settings is in front.
fn drop_on(d: &mut Desk, key: &str, path: PathBuf) {
    d.front(Which::Settings);
    let rt = &mut d.settings;
    scroll_to(rt, key);
    let at = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.drop_files(vec![path], Some(at));
    d.settle();
}

/// The focus off any control of the window in front, as before a key meant for the window.
fn blur_front(d: &mut Desk) {
    match d.shown().front {
        Some(Which::Main) => d.main.update(|_, tx| tx.blur()),
        Some(Which::Settings) => d.settings.update(|_, tx| tx.blur()),
        None => {}
    }
}

/// Scrolls Settings to the element at `key`.
fn scroll_to(rt: &mut Runtime<SettingsWindow>, key: &str) {
    for _ in 0..40 {
        if on_screen(rt, key) {
            break;
        }
        // A sheet over the window scrolls its own body.
        let over = rt.find("icons-sheet").or(rt.find("edit-sheet")).is_some();
        let (Some((_, y)), Some((bx, by))) = (rt.find(key), rt.find(if over { "body" } else { "sheet-body" })) else { break };
        rt.wheel(bx, by, -(y - by).clamp(-200.0, 200.0));
        rt.frame(FRAME);
    }
    assert!(on_screen(rt, key), "{key:?} can't be scrolled to");
}

/// Scrolls Settings to the element at `key`, and puts the pointer on it.
fn scroll_to_hovering(rt: &mut Runtime<SettingsWindow>, key: &str) {
    scroll_to(rt, key);
    let (x, y) = rt.find(key).expect("scrolled to");
    rt.pointer_move(x, y);
    rt.frame(FRAME);
}

/// The fill of the element keyed `key`.
fn fill<S: 'static>(rt: &Runtime<S>, key: &str) -> Option<rune::Color> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.fill)
}

/// The text drawn in the element keyed `key`.
fn text<S: 'static>(rt: &Runtime<S>, key: &str) -> String {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text).unwrap_or_default()
}

proptest! {
    #![proptest_config(Config { cases: 16, ..Config::default() })]

    /// Every text field Settings draws, found in what's drawn rather than listed, is in
    /// `FIELDS`, on its tab, and as wide as its row gives it, whatever's typed in it: empty,
    /// short, or longer than it shows (#140). Every field listed is drawn.
    #[test]
    fn every_field_fills_the_width_its_row_gives(text in prop_oneof![Just(String::new()), "[0-9a-z ]{1,4}", "[0-9a-z ]{40,60}"]) {
        let mut d = opened(MemoryService::new());
        let mut found = Vec::new();
        for tab in Tab::ALL {
            press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap()));
            for key in drawn_fields(&d.settings) {
                let listed = crate::views::settings::FIELDS.iter().find(|(k, ..)| *k == key);
                prop_assert!(listed.is_some_and(|(_, _, t)| *t == tab), "{} is drawn on {:?} but not listed in FIELDS for it", key, tab);
                found.push((key, listed.unwrap().1));
            }
        }
        let listed: Vec<&str> = crate::views::settings::FIELDS.iter().map(|(k, ..)| *k).collect();
        prop_assert_eq!(found.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), listed, "every field listed is drawn, in order");
        for (key, width) in found {
            let key = key.as_str();
            let tab = crate::views::settings::FIELDS.iter().find(|(k, ..)| *k == key).unwrap().2;
            press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap()));
            press(&mut d, key);
            d.settings.key(select_all());
            if text.is_empty() {
                d.settings.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
            } else {
                d.settings.paste(&text);
            }
            d.settle();
            let drawn = super::generated::rect_of(&d.settings, key).expect("the field")[2];
            prop_assert!((drawn - width).abs() < 0.5, "{} is {} wide, its row gives {}", key, drawn, width);
        }
    }
}

fn select_all() -> KeyEvent {
    KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } }
}

fn cmd(c: char) -> KeyEvent {
    KeyEvent { key: Key::Command(c), mods: Mods { cmd: true, ..Default::default() } }
}

/// The keys of the text fields Settings draws now: each field is a textbox with no key of its
/// own (its input inside is keyed `input`) under the element keyed for it.
fn drawn_fields(rt: &Runtime<SettingsWindow>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "textbox" && e.key.is_none() && e.path.contains("settings-sheet"))
        .filter_map(|e| e.path.rsplit('/').nth(1).map(str::to_owned))
        .collect()
}

/// Both windows against `service`, Settings opened with ⌘, in the main window, as a person does.
fn opened(service: MemoryService) -> Desk {
    let mut d = desk(service);
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    open_with_shortcut(&mut d);
    d
}

/// ⌘, in the window in front.
fn open_with_shortcut(d: &mut Desk) {
    blur_front(d);
    d.key(cmd(','));
    d.wait(0.5);
    d.settle();
}

/// The tabs have room above them: the window's top edge is at least `TABS_PAD` from them, they're
/// centred in the title bar's band on every tab, and the traffic lights are placed level with their
/// middle.
#[test]
fn the_tabs_have_room_above_them_and_the_lights_are_level_with_them() {
    use crate::ui::TRAFFIC_LIGHTS;
    // The lights are centred in the band, so it's twice their top, plus a light.
    let band = 2.0 * TRAFFIC_LIGHTS.1 + rune::Chrome::TRAFFIC_LIGHT;
    let mut d = opened(MemoryService::new());
    for tab in Tab::ALL {
        crate::model::settings::show_tab(&mut d.settings.state, tab);
        d.settle();
        let tabs = super::generated::rect_of(&d.settings, "settings-tabs").expect("the tabs");
        let header = super::generated::rect_of(&d.settings, "header").expect("the header");
        assert_eq!((header[1], header[3]), (0.0, band), "the band at the window's top");
        assert!(tabs[1] >= 8.0, "room above the {tab:?} tab: {tabs:?}");
        assert!((tabs[1] - (header[3] - tabs[1] - tabs[3])).abs() < 0.5, "centred in the band: {tabs:?}");
        let lights_middle = TRAFFIC_LIGHTS.1 + rune::Chrome::TRAFFIC_LIGHT / 2.0;
        assert!((lights_middle - (tabs[1] + tabs[3] / 2.0)).abs() < 0.5, "the lights level with the tabs: {tabs:?}");
    }
}

/// ⌘W, Escape and the close button close Settings and nothing else; the main window, behind,
/// keeps its focus where it was. ⌘W with the main window in front closes it, Settings staying in
/// front; the Dock brings it back (#257).
#[test]
fn cmd_w_closes_the_window_in_front_and_the_dock_brings_the_main_one_back() {
    for close in [Some(cmd('w')), Some(KeyEvent { key: Key::Escape, mods: Mods::default() }), None] {
        let mut d = desk(MemoryService::new());
        super::tap(&mut d.main, "magnet-field");
        let before = super::focus_path(&d.main);
        assert!(before.is_some(), "the field is focused");
        d.key(cmd(','));
        d.wait(0.5);
        assert_eq!(d.shown(), Showing { main: true, settings: true, front: Some(Which::Settings) }, "Settings opens in front");
        match close {
            Some(key) => d.key(key),
            None => d.close_button(Which::Settings),
        }
        d.wait(0.5);
        d.settle();
        assert_eq!(d.shown(), Showing { main: true, settings: false, front: Some(Which::Main) }, "only Settings closed");
        assert!(d.settings.state.sheet.is_none());
        assert_eq!(super::focus_path(&d.main), before, "the focus is where it was");
    }
    let mut d = opened(MemoryService::new());
    d.front(Which::Main);
    d.key(cmd('w'));
    d.settle();
    assert_eq!(d.shown(), Showing { main: false, settings: true, front: Some(Which::Settings) }, "only the main window closed");
    assert!(d.settings.state.sheet.is_some(), "Settings stays as it was");
    d.reopen();
    d.settle();
    assert_eq!(d.shown(), Showing { main: true, settings: true, front: Some(Which::Main) }, "the Dock brings the main window back");
}

/// A port typed into the port field on the Network tab, not sent.
fn typing_port(text: &str) -> Desk {
    typing_port_in(MemoryService::new(), text)
}

fn typing_port_in(service: MemoryService, text: &str) -> Desk {
    let mut d = opened(service);
    press(&mut d, "settings-tabs/#0/segment1");
    press(&mut d, "port-field");
    d.settings.key(select_all());
    d.settings.paste(text);
    d.settle();
    let w = &d.settings.state;
    assert!(w.sheet.as_ref().unwrap().editing(&w.looks(), &w.saved), "the port field is being edited");
    d
}

fn stored_port(d: &Desk) -> u16 {
    d.settings.state.sheet.as_ref().and_then(|s| s.service.as_ref()).map_or(0, |s| s.listen_port.port().get())
}

/// Closing Settings with a field being edited applies it, as a step of Undo: ⌘W and the close
/// button alike (#254, #252).
#[test]
fn closing_settings_applies_the_field_being_edited() {
    for close in [Some(cmd('w')), None] {
        let mut d = typing_port("40001");
        match close {
            Some(key) => d.key(key),
            None => d.close_button(Which::Settings),
        }
        d.wait(0.5);
        d.settle();
        assert_eq!(d.shown(), Showing { main: true, settings: false, front: Some(Which::Main) }, "only Settings closed");
        assert_eq!(d.main.state.changes.state.list.len(), 1, "one step of Undo");
        open_with_shortcut(&mut d);
        assert_eq!(stored_port(&d), 40_001, "applied");
    }
}

/// A port the service refused (its torrents wouldn't start again) isn't sent a second time when the
/// field loses focus: the stored port comes back, under the refusal (#254).
#[test]
fn leaving_a_field_whose_text_was_refused_does_not_send_it_again() {
    let mut d = typing_port_in(MemoryService::new().failing_restarts(), "40001");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    let errors = d.main.state.bar.entries().len();
    assert_eq!(errors, 1, "Return's failure is listed");
    d.settings.update(|_, tx| tx.blur());
    d.settle();
    d.settle();
    assert_eq!(d.main.state.bar.entries().len(), errors, "not sent again");
    assert_eq!(d.settings.state.sheet.as_ref().unwrap().port.text, "51413");
    assert!(d.settings.find("listen_port-refused").is_some(), "the refusal still shows");
}

/// A setting the service couldn't change offers Open settings: Settings on that setting's tab, its
/// control focused.
#[test]
fn a_setting_that_could_not_be_changed_offers_open_settings_at_it() {
    let mut d = typing_port_in(MemoryService::new().failing_restarts(), "40001");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    let notice = d.main.state.bar.entries().last().expect("its notice").notice.clone();
    let titles: Vec<_> = notice.actions.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(titles, ["Open settings"]);
    d.main.update(|s, tx| (notice.actions[0].run)(s, tx));
    d.settle();
    assert_eq!(d.settings.state.sheet.as_ref().and_then(|s| s.focus), Some("listen_port"));
}

/// Escape in a field being edited puts the stored value back and keeps Settings; the next Escape
/// closes it, sending nothing (#254).
#[test]
fn escape_in_a_field_being_edited_reverts_it_and_the_next_one_closes() {
    let mut d = typing_port("40001");
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.settle();
    let sheet = d.settings.state.sheet.as_ref().expect("Settings stays");
    assert_eq!(sheet.port.text, "51413", "the stored port shows again");
    assert!(d.main.state.changes.state.list.is_empty());
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.wait(0.5);
    d.settle();
    assert!(d.settings.state.sheet.is_none() && !d.shown().settings, "the second Escape closes");
    assert!(d.main.state.changes.state.list.is_empty(), "nothing was sent");
}

/// Tab, and a click elsewhere, apply a good value; a value that isn't one comes back as it was,
/// with Return's words under the row (#254).
#[test]
fn leaving_a_field_applies_it_or_puts_the_stored_value_back_with_why() {
    for leave in ["tab", "click"] {
        let go = |d: &mut Desk| match leave {
            "tab" => {
                d.settings.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
                // The blur is seen on the next frame, and the service answers on the one after.
                d.settle();
                d.settle();
            }
            _ => {
                press(d, "upload-limit-field");
                d.settle();
            }
        };
        let mut d = typing_port("40001");
        go(&mut d);
        assert_eq!(stored_port(&d), 40_001, "{leave}: applied");
        assert_eq!(d.main.state.changes.state.list.len(), 1, "{leave}: one step");
        // Unchanged: nothing more.
        press(&mut d, "port-field");
        go(&mut d);
        assert_eq!(d.main.state.changes.state.list.len(), 1, "{leave}: unchanged text sends nothing");

        press(&mut d, "port-field");
        d.settings.key(select_all());
        d.settings.paste("70000");
        go(&mut d);
        let sheet = d.settings.state.sheet.as_ref().unwrap();
        assert_eq!(sheet.port.text, "40001", "{leave}: the stored port is back");
        let why = client::display::value_problem(setting::port("70000").unwrap_err());
        assert_eq!(text(&d.settings, "listen_port-refused"), why, "{leave}: Return's words");
        assert_eq!(d.main.state.changes.state.list.len(), 1, "{leave}: nothing sent");
    }
}

/// While a field is being edited with Settings in front, ⌘Z is its text's own undo, and Edit ▸
/// Undo says only "Undo"; the settings' once it's let go (#221). An example: what a field's own
/// undo takes back is Rune's, which the generated sequences don't model.
#[test]
fn undo_in_a_field_being_edited_is_the_fields_own() {
    let mut d = opened(MemoryService::new());
    press(&mut d, "settings-tabs/#0/segment1");
    press(&mut d, "port-field");
    d.settings.key(select_all());
    d.settings.paste("40001");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    let port = |d: &Desk| d.settings.state.sheet.as_ref().unwrap().service.as_ref().unwrap().listen_port.port().get();
    assert_eq!(port(&d), 40_001);
    press(&mut d, "port-field");
    d.settings.key(select_all());
    d.settings.paste("123");
    d.settle();
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "the field's own, in Settings");
    d.key(cmd('z'));
    d.settle();
    assert_ne!(text_of_field(&d), "123", "the field's own undo took the typing back");
    assert_eq!(port(&d), 40_001, "and no setting");
    // Let go (a tab clicked), ⌘Z is the settings'.
    press(&mut d, "settings-tabs/#0/segment0");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(port(&d), 51_413, "the port back");
    assert_eq!(d.settings.state.sheet.as_ref().unwrap().tab, Tab::Network, "on its tab");
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some("Port undone".into()));
}

fn text_of_field(d: &Desk) -> String {
    d.settings.state.sheet.as_ref().unwrap().port.text.clone()
}

/// The port set to 40001 with Return, on the Network tab: one change on the list.
fn port_set() -> Desk {
    let mut d = typing_port("40001");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    assert_eq!(stored_port(&d), 40_001);
    d
}

/// The stored settings, as Settings last heard them.
fn stored(d: &Desk) -> proto::Settings {
    d.settings.state.sheet.as_ref().and_then(|s| s.service.clone()).expect("answered")
}

/// The port typed over and the main window clicked: Settings losing the front leaves the field,
/// which applies it (#254), so ⌘Z there takes that change, the newest, not the one before it, and
/// nothing typed is lost (#257). Found in review, when the field stayed as it was and the undo
/// replaced its text.
#[test]
fn cmd_z_in_the_main_window_applies_the_port_being_typed_then_undoes_it() {
    let mut d = port_set();
    press(&mut d, "port-field");
    d.settings.key(select_all());
    d.settings.paste("40123");
    d.settle();
    d.front(Which::Main);
    d.settle();
    assert_eq!(stored_port(&d), 40_123, "applied as Settings lost the front");
    assert_eq!(d.main.state.changes.state.list.len(), 2, "a step of its own");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Port Change");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(stored_port(&d), 40_001, "⌘Z took the typed port back, to the one before");
    assert_eq!(text_of_field(&d), "40001");
    assert_eq!(d.main.state.changes.state.list.len(), 1, "the first change stays");
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some("Port undone".into()));
}

/// The port set, then another field typed over and the main window clicked: that field is applied
/// as Settings loses the front, and ⌘Z there takes it back, not the port (#254, #257). Found in
/// review, when the undo took the port and the field was applied after it, as the newest change.
#[test]
fn cmd_z_in_the_main_window_applies_another_field_being_typed_then_undoes_that() {
    let mut d = port_set();
    press(&mut d, limit_field(Direction::Upload));
    d.settings.key(select_all());
    d.settings.paste("500");
    d.settle();
    d.front(Which::Main);
    d.settle();
    assert_eq!(stored(&d).speed_limits.upload.map(u64::from), Some(500 * 1024), "applied as Settings lost the front");
    let upload = client::display::undo_change(client::display::setting_short("upload_limit"));
    assert_eq!(crate::changes::menu_title(&d.main.state), upload);
    d.key(cmd('z'));
    d.settle();
    assert_eq!(stored(&d).speed_limits.upload, None, "⌘Z took the limit back");
    assert_eq!(stored_port(&d), 40_001, "and left the port");
    assert_eq!(d.settings.state.sheet.as_ref().unwrap().upload_limit.text, "");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Port Change", "the port's change is next");
}

/// Settings comes to the front and the main window right after, both before the main window's next
/// frame: the main window knows it's in front, as the newest of the two says, though Settings'
/// word comes by its link (#257). Found in review.
#[test]
fn the_window_in_front_is_the_last_to_come_to_the_front_before_a_frame() {
    let mut d = opened(MemoryService::new());
    d.front(Which::Main);
    d.settle();
    assert_eq!(d.main.state.front, Which::Main);
    d.front(Which::Settings);
    d.tell_focus();
    d.front(Which::Main);
    d.tell_focus();
    d.frame(FRAME);
    assert_eq!(d.main.state.front, Which::Main, "the main window, the last to come to the front");
}

/// ⌘Z in Settings, and a snapshot listing another client's newer change, reach the main window in
/// the same frame: the press runs first, as it was made before the main window knew of that
/// change, so it takes the app's own UPnP change and the other client's stays (`poll`).
#[test]
fn a_cmd_z_from_settings_runs_before_what_the_service_sends_in_the_same_frame() {
    let service = MemoryService::new();
    let disk = service.disk();
    let mut d = opened(service);
    press(&mut d, "settings-tabs/#0/segment1");
    press(&mut d, "upnp-switch");
    d.settle();
    assert!(!stored(&d).upnp, "UPnP off");
    // A click on the tabs, so ⌘Z is the window's, not a field's.
    press(&mut d, "settings-tabs/#0/segment1");
    d.key(cmd('z'));
    // Settings' frame runs the key, posting the press; the main window's next poll gets both.
    d.settings.frame(FRAME);
    disk.change(DiskChange::SetByAnother(proto::Setting::KeepAwake(Some(true))));
    d.settle();
    d.wait(0.5);
    let told = client::display::setting_undone_short(client::display::setting_short("upnp"));
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some(told), "the app's own change undone");
    crate::tests::tap(&mut d.main, "notice");
    assert!(crate::tests::texts(&d.main, "notice-listing").contains(&"off → on".to_owned()), "and the popover says from what to what");
    let next = client::display::undo_change(client::display::setting_short("keep_awake"));
    assert_eq!(crate::changes::menu_title(&d.main.state), next, "the other client's, newer to the main window than the press, stays");
    let now = stored(&d);
    assert!(now.upnp, "the sheet shows UPnP back on");
    assert!(now.keep_awake, "and the other client's Keep awake");
}

/// A Tone change in Settings redraws both windows at once, though nothing else happens in the main
/// window (no torrents, so nothing streams): they share the looks (#257). An example: the
/// generated sequences have torrents downloading, whose snapshots redraw the main window anyway.
#[test]
fn a_tone_change_in_settings_redraws_both_windows_at_once() {
    let mut d = desk(MemoryService::new());
    d.main.set_system_dark(false);
    d.settings.set_system_dark(false);
    open_with_shortcut(&mut d);
    press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap()));
    // The Active look's Edit…, whose edits are the real settings. A fresh install's Tone is split:
    // Dark for the Light Appearance.
    press(&mut d, "active-edit");
    let dark = Look::build(&crate::theme::preset::LookPreset::fresh().dark).ui.tokens().c.clone();
    assert_ne!(fill(&d.main, "app"), Some(dark.page), "light to start with");
    // Frames only, as time passes, no springs settled by hand, which would build the main window
    // again anyway.
    let (x, y) = d.settings.find("edit-tone-light/#0/segment1").expect("Tone for Light");
    d.settings.pointer_down(x, y);
    d.settings.pointer_up(x, y);
    d.wait(1.0);
    assert_eq!(fill(&d.main, "app"), Some(dark.page), "the main window, dark");
    assert_eq!(fill(&d.settings, "settings-sheet"), Some(dark.layer_01), "and Settings");
}

/// The Look Presets are columns: ☀, ☾, Edit… and the last button sit at the same x in the Active
/// look's row and in each preset's, and Delete is as wide as Save as…, wide or at 480 pt, a long
/// name wrapping the buttons under it or not. Measured, whatever the font.
#[test]
fn the_look_presets_line_up_in_columns() {
    for width in [600.0, 480.0] {
        let mut d = desk(MemoryService::new());
        let fresh = LookPreset::fresh();
        let long = "A name long enough that its row has to put its buttons on a line of their own";
        let list: Vec<_> = ["System", long, "Night"].map(|n| SavedPreset { name: Name::parse(n).unwrap(), preset: fresh.clone() }).to_vec();
        d.settings.state.saved = Saved::new(list, |_: &[SavedPreset]| {});
        d.main.set_viewport((960.0, 900.0), 2.0);
        d.settings.set_viewport((width, 900.0), 2.0);
        d.settle();
        open_with_shortcut(&mut d);
        press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap()));
        let rect = |key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key} at {width} pt"));
        let rows = [
            ["active-light", "active-dark", "active-edit", "save-as-open"],
            ["preset-0-light", "preset-0-dark", "preset-0-edit", "preset-0-delete"],
            ["preset-1-light", "preset-1-dark", "preset-1-edit", "preset-1-delete"],
            ["preset-2-light", "preset-2-dark", "preset-2-edit", "preset-2-delete"],
        ];
        for column in 0..4 {
            let xs: Vec<f32> = rows.iter().map(|row| rect(row[column])[0]).collect();
            assert!(xs.iter().all(|x| (x - xs[0]).abs() < 0.5), "column {column} at {width} pt: {xs:?}");
        }
        // The buttons themselves (the control inside the keyed cell), not only their cells.
        let button = |key: &str| {
            let at = format!("{key}/#0");
            d.settings
                .elements()
                .into_iter()
                .find(|e| e.path.ends_with(&at) && !e.exiting)
                .map(|e| e.rect)
                .unwrap_or_else(|| panic!("{at}"))
        };
        let last: Vec<[f32; 4]> = [rows[0][3], rows[1][3], rows[2][3], rows[3][3]].iter().map(|k| button(k)).collect();
        for r in &last {
            assert!(
                (r[0] - last[0][0]).abs() < 0.5 && (r[2] - last[0][2]).abs() < 0.5,
                "Save as… and Delete share left and width at {width} pt: {last:?}"
            );
            assert!(((r[0] + r[2]) - (last[0][0] + last[0][2])).abs() < 0.5, "and right edge at {width} pt: {last:?}");
        }
        // The saved ones' ◐ is the column before ☀, the Active look's left empty.
        let both: Vec<f32> = (0..3).map(|i| rect(&format!("preset-{i}-both"))[0]).collect();
        assert!(both.iter().all(|x| (x - both[0]).abs() < 0.5), "◐ at {width} pt: {both:?}");
        assert!(both[0] < rect("active-light")[0], "the Active look's ☀ is under ◐'s column's right");
        if width < 500.0 {
            assert!(
                rect("preset-1-light")[1] > rect("preset-0-light")[1] + 20.0 && rect("preset-1")[3] > rect("preset-0")[3],
                "the long name's buttons wrap under it"
            );
        }
    }
}

/// Every setting is on exactly one tab.
#[test]
fn every_setting_is_on_exactly_one_tab() {
    for name in SETTINGS {
        assert_eq!(Tab::ALL.iter().filter(|t| tab_of(name) == Some(**t)).count(), 1, "{name}");
    }
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|c| c.parse().ok()).unwrap_or(32);
    Config { cases, max_shrink_iters: 1024, ..Config::default() }
}

prop_state_machine! {
    #![proptest_config(config())]

    #[test]
    fn the_settings_sheet_shows_what_is_stored_and_refusals_change_nothing(sequential 1..24 => Sheet);
}

/// The label a theme setting is shown under.
fn shown_label(m: &Model, key: &str) -> &'static str {
    let preset = m.preset_edited();
    match m.shown().into_iter().find(|s| s.key == key).map(|s| s.kind) {
        Some(Kind::Choice { .. }) => {
            preset.light.settings.iter().chain(preset.dark.settings.iter()).find(|(s, _)| s.key == key).unwrap().0.label
        }
        _ => unreachable!(),
    }
}

/// Settings on the Active look's sheet, its theme the sample one with Accent black in both halves,
/// so its field is one for both.
fn accent_sheet() -> Desk {
    let mut look = crate::theme::ThemeLook::defaults(crate::theme::find("sample").unwrap());
    let _ = look.settings.set("accent", Value::Color(Some(Rgb([0, 0, 0]))));
    let mut d = desk(MemoryService::new());
    *d.main.state.looks.borrow_mut() = Looks::new(LookPreset { light: look.clone(), dark: look }, |_: &LookPreset| {});
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    open_with_shortcut(&mut d);
    press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap()));
    press(&mut d, "active-edit");
    d
}

/// Types `text` into the field keyed `field`, leaving it focused.
fn type_into(d: &mut Desk, field: &str, text: &str) {
    if field.starts_with("edit-") {
        open_picker_of(d, field);
    }
    press(d, field);
    d.settings.key(select_all());
    d.settings.paste(text);
}

/// The Accent of each half of the Active look.
fn accents(d: &Desk) -> [Option<Value>; 2] {
    let looks = d.main.state.looks.borrow();
    Appearance::BOTH.map(|a| looks.preset().get(a).settings.get("accent"))
}

/// Split, pressed while a color is typed into the field for both, applies the color first, as
/// leaving the field does, and ⌘Z then takes it back (#332). The field went with the split, and
/// the color with it: Edit ▸ Undo named the change before.
#[test]
fn split_applies_the_color_being_typed_first() {
    let mut d = accent_sheet();
    type_into(&mut d, "edit-accent-both", "#3366ff");
    press(&mut d, "edit-accent-split");
    let blue = Some(Value::Color(Some(Rgb([0x33, 0x66, 0xff]))));
    assert_eq!(accents(&d), [blue, blue], "both halves");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Accent Change");
}

/// So does Join, the color typed into the Light column applied, then copied into the Dark one.
#[test]
fn join_applies_the_color_being_typed_first() {
    let mut d = accent_sheet();
    press(&mut d, "edit-accent-split");
    type_into(&mut d, "edit-accent-light", "#3366ff");
    press(&mut d, "edit-accent-join");
    let blue = Some(Value::Color(Some(Rgb([0x33, 0x66, 0xff]))));
    assert_eq!(accents(&d), [blue, blue], "the Light half's, in both");
    d.settings.update(|_, tx| tx.blur());
    d.key(cmd('z'));
    d.settle();
    assert_eq!(accents(&d), [blue, Some(Value::Color(Some(Rgb([0, 0, 0]))))], "⌘Z: the join's copy undone, the color typed kept");
}

/// A refusal goes with its field: Split, pressed while text that's no color is typed into the field
/// for both, then Join, gives a new field for both, which shows the color and no refusal (#408).
/// The old field's came back under it.
#[test]
fn a_refusal_goes_with_the_field_a_split_takes_away() {
    let refused = |d: &Desk| {
        let of_accent = |key: &str| key.starts_with("edit-accent") && key.ends_with("-refused");
        d.settings.elements().into_iter().any(|e| !e.exiting && e.key.as_deref().is_some_and(of_accent))
    };
    let mut d = accent_sheet();
    type_into(&mut d, "edit-accent-both", "#123");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    assert!(refused(&d), "Return says why it's no color");
    press(&mut d, "edit-accent-split");
    assert!(!refused(&d), "the two new fields were refused nothing");
    press(&mut d, "edit-accent-join");
    assert!(!refused(&d), "nor was the new field for both");
    let black = Some(Value::Color(Some(Rgb([0, 0, 0]))));
    assert_eq!(accents(&d), [black, black], "nothing changed");
}

/// Settings on Appearance, two saved presets, the window 600 pt wide.
pub(super) fn appearance_tab() -> Desk {
    let mut d = desk(MemoryService::new());
    let fresh = LookPreset::fresh();
    let list = ["System", "Night"].map(|n| SavedPreset { name: Name::parse(n).unwrap(), preset: fresh.clone() }).to_vec();
    d.settings.state.saved = Saved::new(list, |_: &[SavedPreset]| {});
    d.settings.state.app_icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), |_| {});
    d.main.set_viewport((960.0, 900.0), 2.0);
    d.settings.set_viewport((600.0, 900.0), 2.0);
    d.settle();
    open_with_shortcut(&mut d);
    press(&mut d, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap()));
    d
}

/// What a frame shows of an element: where it's drawn, how opaque, and its colors (fill, text).
type Drawn = (String, [i32; 4], i32, [i32; 4], Option<[i32; 4]>);

/// What a frame shows of each element of the tab shown.
fn tab_frame(rt: &Runtime<SettingsWindow>) -> Vec<Drawn> {
    let round = |r: [f32; 4]| r.map(|v| (v * 2.0).round() as i32);
    let color = |c: rune::Color| [c.r, c.g, c.b, c.a].map(|v| (v * 255.0).round() as i32);
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains("tab-page"))
        .map(|e| (e.path, round(e.rect), (e.opacity * 100.0).round() as i32, color(e.fill), e.text_color.map(color)))
        .collect()
}

/// A tab switch lands at once (#342): the first frame after the press shows the new tab as it
/// settles, its rows and their dividers where they end up, nothing fading in and no color easing
/// in (the App icon's row among them), for every pair of tabs.
#[test]
fn a_tab_switch_lands_at_once() {
    for from in Tab::ALL {
        for to in Tab::ALL.into_iter().filter(|t| *t != from) {
            let mut d = appearance_tab();
            let segment = |tab: Tab| format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap());
            press(&mut d, &segment(from));
            d.wait(1.0);
            let (x, y) = d.settings.find(&segment(to)).expect("the tab");
            d.settings.pointer_down(x, y);
            d.settings.pointer_up(x, y);
            d.settings.frame(FRAME);
            let first = tab_frame(&d.settings);
            d.wait(1.0);
            d.settle();
            assert_eq!(first, tab_frame(&d.settings), "{from:?} to {to:?}: the first frame is the settled one");
        }
    }
}

/// Save as…'s row is as tall asking Replace? as it was with its field, so the list under it stays
/// where it is (#343).
#[test]
fn the_replace_question_keeps_the_save_as_rows_height() {
    let mut d = appearance_tab();
    press(&mut d, "save-as-open");
    type_into(&mut d, "save-as-field", "Night");
    d.settle();
    let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    let (field, below) = (rect(&d, "save-as"), rect(&d, "preset-0"));
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.wait(1.0);
    d.settle();
    assert!(d.settings.find("save-as-replace").is_some(), "the question");
    assert_eq!(rect(&d, "save-as")[3], field[3], "as tall");
    assert_eq!(rect(&d, "preset-0")[1], below[1], "the list under it stays");
}

/// The Active look's ☀ and ☾ chips open its sheet (#87).
#[test]
fn the_active_looks_chips_open_its_sheet() {
    for half in Appearance::BOTH {
        let mut d = appearance_tab();
        press(&mut d, &format!("active-{}", half.key()));
        assert_eq!(d.settings.state.sheet.as_ref().unwrap().target(), Some(&Target::Active), "the Active look's sheet");
    }
}

/// The Active look's preset, `key`'s value in each half.
fn halves(d: &Desk, key: &str) -> [Option<Value>; 2] {
    let looks = d.main.state.looks.borrow();
    Appearance::BOTH.map(|a| looks.preset().get(a).settings.get(key))
}

/// The texts drawn in the element keyed `key` and what it holds.
fn texts_in<S: 'static>(rt: &Runtime<S>, key: &str) -> Vec<String> {
    let Some(path) = rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.path) else { return Vec::new() };
    rt.elements().into_iter().filter(|e| e.path == path || e.path.starts_with(&format!("{path}/"))).filter_map(|e| e.text).collect()
}

/// A row's Join and Split, small buttons named so: joined, one control across both columns, the
/// Light half's value copied into the Dark one, a change ⌘Z takes back; split, one in each column,
/// even with one value in both. The columns are named ☀ Light and ☾ Dark either way, all rows
/// joined too.
#[test]
fn a_rows_join_and_split() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    let (light, dark) = (Some(Value::Choice("light")), Some(Value::Choice("dark")));
    assert_eq!(halves(&d, "tone"), [light, dark], "a fresh install's Tone differs: split");
    assert!(d.settings.find("edit-tone-light").is_some() && d.settings.find("edit-tone-dark").is_some());
    assert!(d.settings.find("halves-header").is_some(), "the columns named");
    assert_eq!(texts_in(&d.settings, "edit-tone-join"), ["Join"], "a split row's Join");
    assert_eq!(texts_in(&d.settings, "edit-theme-split"), ["Split"], "a joined row's Split");
    press(&mut d, "edit-tone-join");
    assert_eq!(halves(&d, "tone"), [light, light], "joined: the Light half's Tone in both");
    assert!(d.settings.find("edit-tone-both").is_some() && d.settings.find("edit-tone-light").is_none(), "one control");
    assert!(d.settings.find("halves-header").is_some(), "all joined: still two columns, named");
    assert_eq!(texts_in(&d.settings, "halves-header").concat(), "LightDark", "☀ Light and ☾ Dark");
    assert_eq!(texts_in(&d.settings, "edit-tone-split"), ["Split"]);
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Tone Change");
    press(&mut d, "edit-tone-split");
    assert!(d.settings.find("edit-tone-light").is_some() && d.settings.find("edit-tone-dark").is_some(), "split, one value in both");
    choose(&mut d, "edit-tone-dark", 1);
    assert_eq!(halves(&d, "tone"), [light, dark], "the Dark half alone");
}

/// The sheet's body is split down the middle: the left half is drawn entirely in the preset's
/// Light look and the right half entirely in its Dark one, backgrounds included, every row and
/// the headings; the rest of the sheet in the look the system's Appearance shows (#87).
#[test]
fn the_editors_halves_are_drawn_each_in_its_halfs_look() {
    let mut d = appearance_tab();
    d.settings.set_system_dark(false);
    press(&mut d, "active-edit");
    let preset = d.main.state.looks.borrow().preset().clone();
    let [light, dark] = Appearance::BOTH.map(|a| Look::build(preset.get(a)).ui.tokens().c.clone());
    assert_ne!(light.bg_base, dark.bg_base, "a fresh install's halves look apart");
    let texts = |d: &Desk, key: &str| -> Vec<rune::Color> {
        let Some(path) = d.settings.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).map(|e| e.path) else { return vec![] };
        d.settings
            .elements()
            .into_iter()
            .filter(|e| e.path.starts_with(&format!("{path}/")) && e.text.is_some())
            .filter_map(|e| e.text_color)
            .collect()
    };
    // Every row joined, and then one split.
    for split in [false, true] {
        if split {
            press(&mut d, "edit-theme-split");
        }
        for row in ["halves-header", "edit-theme", "edit-tone"] {
            assert_eq!(fill(&d.settings, &format!("{row}-left")), Some(light.bg_base), "{row}: the left half in the Light look");
            assert_eq!(fill(&d.settings, &format!("{row}-right")), Some(dark.bg_base), "{row}: the right half in the Dark look");
        }
        assert!(texts(&d, "edit-theme-left").contains(&light.text_base), "the Light half's text in its look's color");
        assert!(texts(&d, "edit-tone-right").contains(&dark.text_base), "the Dark half's in its");
    }
    let rect = |key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    let (card, right) = (rect("halves-card"), rect("edit-tone-right"));
    assert!((right[0] - (card[0] + card[2] / 2.0)).abs() <= 1.0, "the seam in the middle: {right:?} of {card:?}");
    assert!((right[0] + right[2] - (card[0] + card[2])).abs() <= 2.0, "the Dark half to the end");
    let system = Look::build(preset.get(Appearance::Light)).ui.tokens().c.clone();
    assert_eq!(fill(&d.settings, "edit-sheet"), Some(system.layer_01), "the sheet itself in the system's");
}

/// A joined setting's control sits left-aligned in the left column, where the ☀ control of a split
/// row sits, as wide as it, so joined and split rows line up (#87).
#[test]
fn a_joined_control_is_in_the_left_column() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    // A fresh install: Theme joined, Tone split.
    let (joined, split, dark) = (rect(&d, "edit-theme-both-cell"), rect(&d, "edit-tone-light-cell"), rect(&d, "edit-tone-dark-cell"));
    assert_eq!(joined[0], split[0], "the joined control starts where the ☀ control does");
    assert_eq!(joined[2], split[2], "and is as wide: not across the middle");
    assert!(joined[0] + joined[2] <= dark[0], "wholly left of the ☾ column ({joined:?}, {dark:?})");
    press(&mut d, "edit-tone-join");
    assert_eq!(rect(&d, "edit-tone-both-cell")[0], joined[0], "a row just joined: the same");
}

/// Night's Tone: each half's.
fn night_tone(d: &Desk) -> [Option<Value>; 2] {
    let saved = d.settings.state.saved.list()[1].preset.clone();
    Appearance::BOTH.map(|a| saved.get(a).settings.get("tone"))
}

/// The editor's Done folds every change made in it into one undo step, "Undo Edit of Night",
/// which puts the preset back as it was when the sheet opened; inside the sheet, ⌘Z steps back
/// one change at a time. Cancel, and Escape, put it back as it was then and leave none of its
/// steps (#87).
#[test]
fn done_is_one_undo_step_and_cancel_puts_the_preset_back() {
    let (light, dark) = (Some(Value::Choice("light")), Some(Value::Choice("dark")));
    let edit = |d: &mut Desk| {
        press(d, "preset-1-edit");
        assert_eq!(night_tone(d), [light, dark], "as it opens");
        press(d, "edit-tone-join");
        choose(d, "edit-tone-both", 1);
        assert_eq!(night_tone(d), [dark, dark], "two changes");
    };
    let mut d = appearance_tab();
    edit(&mut d);
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Tone of Night");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(night_tone(&d), [light, light], "⌘Z in the sheet: one change back");
    choose(&mut d, "edit-tone-both", 1);
    press(&mut d, "edit-done");
    assert!(d.settings.find("edit-sheet").is_none(), "Done closes it");
    assert_eq!(night_tone(&d), [dark, dark], "kept");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Edit of Night", "one step");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(night_tone(&d), [light, dark], "one ⌘Z: as it was when the sheet opened");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "nothing older");
    // The bar's line holds no name the user chose; the list has it.
    let told = d.main.state.bar.entries().last().expect("the undo is told").notice.clone();
    assert_eq!(told.short, "Edit of a saved Look Preset undone");
    assert_eq!(told.more.as_ref().map(|list| list.plain()), Some("Night".to_owned()), "the name is the list's");

    for cancel in ["edit-cancel", "escape"] {
        let mut d = appearance_tab();
        edit(&mut d);
        match cancel {
            "escape" => d.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
            key => press(&mut d, key),
        }
        d.wait(0.5);
        d.settle();
        assert!(d.settings.find("edit-sheet").is_none(), "{cancel} closes it");
        assert_eq!(night_tone(&d), [light, dark], "{cancel}: as it was when the sheet opened");
        assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "{cancel}: none of its steps left");
        d.key(cmd('z'));
        d.settle();
        assert_eq!(night_tone(&d), [light, dark], "⌘Z then changes nothing");
    }
}

/// The editor's own steps don't count against the 20 changes ⌘Z can take back while it's open:
/// an App icon picked before it, then 23 steps in it, and Done: one ⌘Z for the Edit, the next the
/// icon (#87).
#[test]
fn a_long_edit_keeps_the_older_changes() {
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-0");
    press(&mut d, "icons-done");
    let picked = d.settings.state.app_icon.choice.clone();
    assert!(picked.is_some(), "a pair picked");
    press(&mut d, "preset-1-edit");
    press(&mut d, "edit-tone-join");
    for i in 0..22 {
        choose(&mut d, "edit-tone-both", (i + 1) % 2);
    }
    press(&mut d, "edit-done");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Edit of Night", "one step for the sheet");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(night_tone(&d), [Some(Value::Choice("light")), Some(Value::Choice("dark"))], "the Edit back");
    assert_ne!(crate::changes::menu_title(&d.main.state), "Undo", "the icon change is still there");
    d.key(cmd('z'));
    d.settle();
    assert_ne!(d.settings.state.app_icon.choice, picked, "the second ⌘Z undoes the icon");
}

/// The Theme row splits; a theme chosen for the Dark half makes each column its own list, its
/// Theme pop-up then its own theme's settings (Tone only under ☀, Sample's three only under ☾),
/// with no Join or Split anywhere; Phantom's "Every color" opens from its row there too. The same
/// theme chosen again brings the shared rows back, each split where its values differ (#87).
#[test]
fn a_theme_in_each_half_has_its_own_column() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    press(&mut d, "edit-theme-split");
    let sample = crate::theme::themes().iter().position(|t| t.id() == "sample").expect("the tests' second theme");
    press(&mut d, "edit-theme-dark-trigger");
    press(&mut d, &format!("option{sample}"));
    let preset = d.main.state.looks.borrow().preset().clone();
    assert_eq!(preset.dark.theme.id(), "sample");
    assert!(d.settings.find("edit-tone-light").is_some() && d.settings.find("edit-tone-dark").is_none(), "Tone under ☀ only");
    for (key, control) in [("pulse", ""), ("shape", ""), ("accent", "-swatch")] {
        assert!(d.settings.find(&format!("edit-{key}-dark{control}")).is_some(), "{key} under ☾");
    }
    // Phantom has an Accent of its own (a color), but neither Pulse nor Shape.
    for key in ["pulse", "shape"] {
        assert!(d.settings.find(&format!("edit-{key}-light")).is_none(), "{key} not under ☀");
    }
    for key in ["theme", "tone", "pulse", "shape", "accent"] {
        assert!(d.settings.find(&format!("edit-{key}-join")).is_none(), "no Join or Split on {key}'s row");
        assert!(d.settings.find(&format!("edit-{key}-split")).is_none(), "no Join or Split on {key}'s row");
    }
    let top = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).expect(key)[1];
    assert_eq!(top(&d, "edit-pulse-dark-row"), top(&d, "edit-tone-light-row"), "each list packed from the top");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Theme Change");
    // Phantom's sections in its own list: "Every color" opens from its chevron row, as with one
    // theme.
    assert!(d.settings.find("edit-every-color-light-section").is_some(), "Every color's row under ☀");
    assert!(d.settings.find("edit-faint_text-light-swatch").is_none(), "closed at first");
    press(&mut d, "edit-every-color-light-section");
    assert!(d.settings.find("edit-faint_text-light-swatch").is_some(), "open, its colors under ☀");
    // Its headers have no hairline above or below them; its rows keep theirs.
    let hairlines: Vec<[f32; 4]> = d.settings.elements().into_iter().filter(|e| !e.exiting && e.rect[3] == 1.0).map(|e| e.rect).collect();
    let touching = |key: &str| {
        let r = super::generated::rect_of(&d.settings, key).expect(key);
        hairlines
            .iter()
            .filter(|h| h[0] < r[0] + r[2] && r[0] < h[0] + h[2])
            .any(|h| (h[1] + 1.0 - r[1]).abs() < 0.5 || (h[1] - (r[1] + r[3])).abs() < 0.5)
    };
    for header in ["edit-every-color-light-section", "edit-group-text-light", "edit-group-surfaces-light"] {
        assert!(!touching(header), "no hairline next to {header}");
    }
    assert!(touching("edit-muted_text-light-row"), "rows keep their hairlines");
    press(&mut d, "edit-every-color-light-section");
    assert!(d.settings.find("edit-faint_text-light-swatch").is_none(), "closed again");
    // Phantom in the Dark half's pop-up: one theme, Tone split as its values differ.
    press(&mut d, "edit-theme-dark-trigger");
    press(&mut d, "option0");
    assert_eq!(halves(&d, "tone"), [Some(Value::Choice("light")), Some(Value::Choice("dark"))], "a fresh install's Dark half");
    assert!(d.settings.find("edit-tone-join").is_some(), "Tone joins again");
}

/// The App icon's sheet (#87): first Pairs, a tile for each designed pair, which one click
/// applies (both halves), outlined while both halves are its icons; then Icons: Follow theme and
/// every icon, each with ◐ ☀ ☾ (Follow theme too, so one half can follow it), ◐ that icon (or
/// following) in both, ☀ in Light and ☾ in Dark, the other half as it is. In use means one thing:
/// ◐ outlined only while both halves are that icon, else ☀ under the Light half's and ☾ under the
/// Dark half's. ☀ is drawn light and ☾ dark, as the Look Presets' chips are, far enough apart that
/// their outlines don't meet. Done closes it, and so does Escape.
#[test]
fn the_app_icons_sheet() {
    for close in ["icons-done", "escape"] {
        let mut d = appearance_tab();
        assert!(d.settings.find("icon-follow").is_none(), "no tiles on the tab");
        press(&mut d, "app-icon-edit");
        assert!(d.settings.find("icons-sheet").is_some(), "its sheet");
        let choice = |d: &Desk| d.settings.state.app_icon.choice.clone();
        let in_use = |d: &Desk, key: &str| d.settings.find(&format!("{key}/#0/in-use")).is_some();
        let marked = |d: &Desk, tile: &str| ["both", "light", "dark"].map(|h| in_use(d, &format!("{tile}-{h}")));
        assert_eq!(marked(&d, "icon-follow"), [true, false, false], "Follow theme in both: its ◐ alone");
        let pairs = manifest().pairs().to_vec();
        for i in 0..pairs.len() {
            assert!(d.settings.find(&format!("icon-pair-{i}")).is_some(), "a tile for each pair");
        }
        press(&mut d, "icon-pair-1");
        assert_eq!(choice(&d), Some(pairs[1].icons()), "a pair's tile: both halves");
        assert!(d.settings.find("icon-pair-1/in-use").is_some() && d.settings.find("icon-pair-0/in-use").is_none(), "it in use");
        assert_eq!(marked(&d, &format!("icon-{}", pairs[1].light)), [false, true, false], "its Light icon's ☀");
        assert_eq!(marked(&d, &format!("icon-{}", pairs[1].dark)), [false, false, true], "its Dark icon's ☾");
        press(&mut d, "icon-follow-dark");
        assert_eq!(
            choice(&d),
            Some(Icons::Pair { dark: FOLLOW.into(), light: pairs[1].light.clone() }),
            "☾ of Follow theme: the Dark half follows it"
        );
        assert_eq!(marked(&d, "icon-follow"), [false, false, true]);
        assert!(d.settings.find("icon-pair-1/in-use").is_none(), "the pair no longer in use");
        press(&mut d, "icon-dark-hour-both");
        assert_eq!(choice(&d), Some(Icons::One { icon: "dark-hour".into() }), "◐ of an icon of a pair: it alone, in both");
        assert_eq!(marked(&d, "icon-dark-hour"), [true, false, false], "◐ alone in use");
        press(&mut d, "icon-full-moon-light");
        assert_eq!(choice(&d), Some(Icons::Pair { dark: "dark-hour".into(), light: "full-moon".into() }), "☀: the Light half alone");
        assert!(d.settings.find("icon-pair-0/in-use").is_some(), "the designed pair it makes, in use");
        // ☀ drawn light and ☾ dark, apart.
        let fresh = LookPreset::fresh();
        let [light, dark] = Appearance::BOTH.map(|a| Look::build(fresh.get(a)).ui.tokens().c.page);
        assert_eq!(face(&d, "icon-phantom-light"), Some(light), "☀ light");
        assert_eq!(face(&d, "icon-phantom-dark"), Some(dark), "☾ dark");
        let rect = |key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
        for (left, right) in [("both", "light"), ("light", "dark")] {
            let (a, b) = (rect(&format!("icon-phantom-{left}")), rect(&format!("icon-phantom-{right}")));
            assert!(b[0] - (a[0] + a[2]) >= 2.0 * 5.0, "{left} and {right}: outlines 5 pt out never meet ({a:?}, {b:?})");
        }
        match close {
            "escape" => d.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
            key => press(&mut d, key),
        }
        d.wait(0.5);
        d.settle();
        assert!(d.settings.find("icons-sheet").is_none(), "{close} closes it");
        assert!(d.settings.state.sheet.is_some(), "and only it");
    }
}

/// A sheet over Settings that edits things, as the rules of a sheet's undo session see it
/// ([`crate::changes::session`]): how to make a change before it opens, open it, take a step in it,
/// close it with Done, and read what it edits and what the change before it was of.
struct SheetKind {
    /// A change made before the sheet opens, and what that one reads as.
    before: fn(&mut Desk),
    before_value: fn(&Desk) -> String,
    open: fn(&mut Desk),
    /// Step `i` (from 0) in the open sheet; every step changes what the sheet reads.
    step: fn(&mut Desk, usize),
    done: fn(&mut Desk),
    value: fn(&Desk) -> String,
    /// The key that is only on screen while the sheet is open.
    key: &'static str,
}

/// The Look Preset editor, on Night: its steps are its Tone's joining and settings.
fn the_editor() -> SheetKind {
    SheetKind {
        before: |d| {
            press(d, "app-icon-edit");
            press(d, "icon-pair-3");
            press(d, "icons-done");
        },
        before_value: |d| format!("{:?}", d.settings.state.app_icon.choice),
        open: |d| press(d, "preset-1-edit"),
        step: |d, i| match i {
            0 => press(d, "edit-tone-join"),
            n => choose(d, "edit-tone-both", n % 2),
        },
        done: |d| press(d, "edit-done"),
        value: |d| format!("{:?}", night_tone(d)),
        key: "edit-sheet",
    }
}

/// The App icon's sheet: its steps are picks of a designed pair.
fn the_icons_sheet() -> SheetKind {
    SheetKind {
        before: |d| {
            press(d, "preset-1-edit");
            press(d, "edit-tone-join");
            press(d, "edit-done");
        },
        before_value: |d| format!("{:?}", night_tone(d)),
        open: |d| press(d, "app-icon-edit"),
        step: |d, i| press(d, &format!("icon-pair-{}", [2, 1, 0][i % 3])),
        done: |d| press(d, "icons-done"),
        value: |d| format!("{:?}", d.settings.state.app_icon.choice),
        key: "icons-sheet",
    }
}

/// One set of rules for every sheet's undo (#87): ⌘Z takes only the sheet's own steps, newest
/// first, and it stays open; with none left ⌘Z does nothing; once it's closed the visit is one
/// step, and the change before it is the next.
fn a_sheets_undo_follows_the_rules(sheet: SheetKind) {
    let mut d = appearance_tab();
    (sheet.before)(&mut d);
    let (before, opened) = ((sheet.before_value)(&d), (sheet.value)(&d));
    (sheet.open)(&mut d);
    let mut seen = vec![(sheet.value)(&d)];
    for i in 0..3 {
        (sheet.step)(&mut d, i);
        seen.push((sheet.value)(&d));
    }
    assert!(seen.windows(2).all(|w| w[0] != w[1]), "every step changes it: {seen:?}");
    let undo = |d: &mut Desk| {
        d.key(cmd('z'));
        d.settle();
    };
    for back in [2, 1, 0] {
        undo(&mut d);
        assert!(d.settings.find(sheet.key).is_some(), "⌘Z leaves the sheet open");
        assert_eq!((sheet.value)(&d), seen[back], "one step back");
        assert_eq!((sheet.before_value)(&d), before, "nothing older is taken");
    }
    undo(&mut d);
    assert!(d.settings.find(sheet.key).is_some(), "still open with no step left");
    assert_eq!((sheet.value)(&d), opened, "⌘Z does nothing with no step left");
    assert_eq!((sheet.before_value)(&d), before, "and takes nothing older");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "Edit ▸ Undo names nothing");
    for i in 0..3 {
        (sheet.step)(&mut d, i);
    }
    let after = (sheet.value)(&d);
    (sheet.done)(&mut d);
    assert!(d.settings.find(sheet.key).is_none(), "Done closes it");
    assert_eq!((sheet.value)(&d), after, "and keeps what was done");
    undo(&mut d);
    assert_eq!((sheet.value)(&d), opened, "one ⌘Z takes the whole visit");
    assert_eq!((sheet.before_value)(&d), before, "and no more");
    undo(&mut d);
    assert_ne!((sheet.before_value)(&d), before, "the next ⌘Z takes the change before it");
}

#[test]
fn the_editors_undo_follows_a_sheets_rules() {
    a_sheets_undo_follows_the_rules(the_editor());
}

#[test]
fn the_app_icons_sheets_undo_follows_a_sheets_rules() {
    a_sheets_undo_follows_the_rules(the_icons_sheet());
}

/// A click that doesn't wait for the windows to settle: what came before it is still on its way.
fn press_now(d: &mut Desk, key: &str) {
    d.front(Which::Settings);
    let rt = &mut d.settings;
    scroll_to(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
}

/// ⌘Z and a close in the same breath, before the windows have settled: the one step a visit folds
/// into says what's left of it, and none if nothing is (#87).
#[test]
fn cmd_z_then_close_at_once_folds_what_is_left_of_the_icon_sheets_visit() {
    let pairs = manifest().pairs().to_vec();
    let [b, c] = [pairs[2].clone(), pairs[1].clone()];
    // A → B → C, ⌘Z, Escape: one step, A → B.
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-2");
    press(&mut d, "icon-pair-1");
    d.key(cmd('z'));
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.settle();
    assert_eq!(d.settings.state.app_icon.choice, Some(b.icons()));
    assert_eq!(crate::changes::menu_title(&d.main.state), format!("Undo App icon: Follow theme → {}", b.name));
    // A → B, ⌘Z, Escape: nothing left of the visit, no step.
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-2");
    d.key(cmd('z'));
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.settle();
    assert_eq!(d.settings.state.app_icon.choice, None);
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "no step");
    let _ = c;
}

/// The same for the Look Preset editor: Done after a ⌘Z folds what's left (none, when the ⌘Z took
/// the only change), and Escape, its Cancel, leaves the preset as it opened (#87).
#[test]
fn cmd_z_then_close_at_once_folds_what_is_left_of_the_editors_visit() {
    let (light, dark) = (Some(Value::Choice("light")), Some(Value::Choice("dark")));
    let mut d = appearance_tab();
    press(&mut d, "preset-1-edit");
    press(&mut d, "edit-tone-join");
    d.key(cmd('z'));
    press_now(&mut d, "edit-done");
    d.settle();
    assert_eq!(night_tone(&d), [light, dark], "the join undone");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "no step");
    let mut d = appearance_tab();
    press(&mut d, "preset-1-edit");
    press(&mut d, "edit-tone-join");
    choose(&mut d, "edit-tone-both", 1);
    d.key(cmd('z'));
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.settle();
    assert_eq!(night_tone(&d), [light, dark], "Cancel: as it opened");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "none of its steps left");
}

/// A visit that ends where it began (A → B → A) leaves no step (#87).
#[test]
fn a_visit_to_the_app_icons_sheet_that_ends_where_it_began_leaves_no_step() {
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-2");
    press(&mut d, "icon-follow-both");
    assert_eq!(d.settings.state.app_icon.choice, None);
    press(&mut d, "icons-done");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "no step");
}

/// An image that fails to load while the sheet is open drops its half to the theme's icon: that's
/// no pick of the user's, so Done leaves no step for it (#87).
#[test]
fn an_image_failing_to_load_during_a_visit_is_no_step_of_it() {
    let gone = format!("{}/nowhere/moon.png", crate::platform::app_icon::FILE);
    let choice = Icons::Pair { dark: gone, light: "full-moon".into() };
    let mut d = appearance_tab();
    let mut icon = AppIcon::new(Some(manifest()), Some(choice), Looks::default().app_icons(), |_| {});
    icon.hold_loads();
    icon.load();
    d.settings.state.app_icon = icon;
    press(&mut d, "app-icon-edit");
    d.settings.state.app_icon.release_loads();
    d.settle();
    assert_eq!(
        d.settings.state.app_icon.choice,
        Some(Icons::Pair { dark: FOLLOW.into(), light: "full-moon".into() }),
        "its half follows the theme"
    );
    press(&mut d, "icons-done");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo", "no step");
}

/// A load that finishes after its image's × applies nothing: the tile stays gone, and the Dock
/// isn't told (#87).
#[test]
fn a_late_load_never_brings_back_a_removed_image() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let mut d = appearance_tab();
    let mut icon = AppIcon::new(Some(manifest()), None, Looks::default().app_icons(), move |change| {
        if let crate::model::settings::IconChange::Loaded(icon) = change {
            log.borrow_mut().push(icon.id.clone());
        }
    })
    .with_added(vec![file_id(0)]);
    icon.hold_loads();
    icon.load();
    d.settings.state.app_icon = icon;
    press(&mut d, "app-icon-edit");
    assert!(d.settings.find("icon-image-0").is_some(), "a placeholder tile while it loads");
    d.settings.update(|s, _| crate::model::settings::remove_image(s, &file_id(0)));
    d.settle();
    assert!(d.settings.state.app_icon.added.is_empty(), "removed");
    d.settings.state.app_icon.release_loads();
    d.settle();
    assert!(d.settings.state.app_icon.added.is_empty() && d.settings.find("icon-image-0").is_none(), "the tile stays gone");
    assert!(heard.borrow().is_empty(), "nothing applied");
    assert!(d.settings.state.app_icon.manifest.as_ref().unwrap().icon(&file_id(0)).is_none());
}

/// An image both halves name is read once (#87).
#[test]
fn an_image_in_both_halves_is_read_once() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let both = Icons::One { icon: file_id(0) };
    let mut icon = AppIcon::new(Some(manifest()), Some(both), Icons::One { icon: "phantom".into() }, move |change| {
        if let crate::model::settings::IconChange::Loaded(icon) = change {
            log.borrow_mut().push(icon.id.clone());
        }
    })
    .with_added(vec![file_id(0)]);
    icon.load();
    assert_eq!(icon.poll(), [crate::model::settings::Arrival::Loaded]);
    assert_eq!(*heard.borrow(), [file_id(0)], "one read, one Loaded");
}

/// An image with more than 8192 px a side (more than the sheet's decoder takes) is scaled to icon
/// size as it's read: a tile that draws, and only the scaled image is kept (#87).
#[cfg(target_os = "macos")]
#[test]
fn a_huge_image_is_kept_at_icon_size_and_its_tile_draws() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("huge.png");
    std::fs::write(&file, blank_png(9000)).unwrap();
    let id = format!("{}{}", crate::platform::app_icon::FILE, file.display());
    let mut icon = AppIcon::new(Some(manifest()), None, Icons::One { icon: "phantom".into() }, |_| {}).with_added(vec![id.clone()]);
    icon.load_images(Rc::new(rune::ImageCache::new(std::sync::Arc::new(|| {}))));
    icon.load();
    assert_eq!(icon.poll(), [crate::model::settings::Arrival::Loaded]);
    let kept = icon.manifest.as_ref().unwrap().icon(&id).expect("read");
    let side = |at: usize| u32::from_be_bytes(kept.bytes[at..at + 4].try_into().unwrap());
    assert_eq!([side(16), side(20)], [crate::platform::app_icon::ICON_SIZE; 2], "kept at icon size");
    assert!(kept.bytes.len() < 4_000_000, "not the 9000 px file: {} bytes", kept.bytes.len());
    icon.image(&id).expect("an image to draw").wait().expect("it draws");
}

/// An image smaller than the icon size is kept as it is, never scaled up (#87).
#[cfg(target_os = "macos")]
#[test]
fn a_small_image_is_not_scaled_up() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("small.png");
    std::fs::write(&file, blank_png(100)).unwrap();
    let icon = crate::platform::app_icon::read_image(&format!("{}{}", crate::platform::app_icon::FILE, file.display())).unwrap();
    assert_eq!(u32::from_be_bytes(icon.bytes[16..20].try_into().unwrap()), 100);
}

/// A square 1-bit PNG, `side` px a side, all black (stored, not compressed).
#[cfg(target_os = "macos")]
fn blank_png(side: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let body: Vec<u8> = kind.iter().chain(data).copied().collect();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let row = (side as usize).div_ceil(8) + 1;
    let raw = vec![0u8; row * side as usize];
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(*block);
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(side.to_be_bytes());
    header.extend(side.to_be_bytes());
    header.extend([1, 0, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// A visit to the App icon's sheet is one step: three picks and a ⌘Z leave the sheet open on the
/// pick before; closing it, one ⌘Z puts the icon back to what it was when it opened, and Edit ▸
/// Undo names the step "App icon: <old> → <new>" (#87).
#[test]
fn a_visit_to_the_app_icons_sheet_is_one_undo_step() {
    let mut d = appearance_tab();
    let pairs = manifest().pairs().to_vec();
    let choice = |d: &Desk| d.settings.state.app_icon.choice.clone();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-3");
    press(&mut d, "icons-done");
    assert_eq!(choice(&d), Some(pairs[3].icons()));
    press(&mut d, "app-icon-edit");
    for pair in [2, 1, 0] {
        press(&mut d, &format!("icon-pair-{pair}"));
    }
    d.key(cmd('z'));
    d.settle();
    assert!(d.settings.find("icons-sheet").is_some(), "⌘Z leaves the sheet open");
    assert_eq!(choice(&d), Some(pairs[1].icons()), "on the pick before");
    press(&mut d, "icons-done");
    let title = crate::changes::menu_title(&d.main.state);
    assert_eq!(title, "Undo App icon: Phantom Thief (Alt) → Memento Mori");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(choice(&d), Some(pairs[3].icons()), "the whole visit undone: Phantom Thief (Alt) again");
    let told = d.main.state.bar.entries().iter().map(|e| e.notice.short.clone()).collect::<Vec<_>>();
    assert!(told.iter().any(|t| t == "App icon undone"), "no icon's name in the line: {told:?}");
    d.key(cmd('z'));
    d.settle();
    assert_eq!(choice(&d), None, "the earlier visit is the next step");
}

/// The page color a chip keyed `key` is drawn on.
fn face(d: &Desk, key: &str) -> Option<rune::Color> {
    d.settings.elements().into_iter().find(|e| e.path.ends_with(&format!("{key}/#0/face")) && !e.exiting).map(|e| e.fill)
}

/// "+ Add image…" adds the file chosen as an icon: a tile like the others, with its chips; its ×
/// removes it, and a half showing it follows the theme then, never the other half's icon (#87).
#[test]
fn an_image_is_added_and_removed() {
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-add");
    d.settings.resolve_file_dialog(Some(images()[0].clone())).expect("a file dialog");
    d.settle();
    assert_eq!(d.settings.state.app_icon.added, [file_id(0)], "added");
    press(&mut d, "icon-dark-hour-light");
    press(&mut d, "icon-image-0-dark");
    let choice = |d: &Desk| d.settings.state.app_icon.choice.clone();
    assert_eq!(choice(&d), Some(Icons::Pair { dark: file_id(0), light: "dark-hour".into() }), "the Dark half");
    drop_on(&mut d, "app_icon_tiles", images()[1].clone());
    assert_eq!(d.settings.state.app_icon.added, [file_id(0), file_id(1)], "a drop adds one too");
    scroll_to_hovering(&mut d.settings, "icon-image-0");
    press(&mut d, "icon-image-0-remove");
    assert_eq!(d.settings.state.app_icon.added, [file_id(1)], "removed");
    assert_eq!(choice(&d), Some(Icons::Pair { dark: FOLLOW.into(), light: "dark-hour".into() }), "the Dark half follows the theme");
    assert!(d.settings.find("icon-image-1").is_none() && d.settings.find("icon-image-0").is_some(), "its tile gone");
}

/// A file that isn't an image (whatever its extension says) is refused with a line under the
/// tiles, and never becomes a tile; an image of any size is added (#87).
#[test]
fn a_file_that_is_no_image_is_refused_and_a_big_image_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("fake.png");
    std::fs::write(&fake, "this is text").unwrap();
    let big = dir.path().join("big.icns");
    let mut bytes = std::fs::read(&images()[0]).unwrap();
    bytes.resize(20_000_000, 0);
    std::fs::write(&big, bytes).unwrap();
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-add");
    d.settings.resolve_file_dialog(Some(fake.clone())).expect("a file dialog");
    d.settle();
    assert!(d.settings.state.app_icon.added.is_empty(), "no tile");
    assert!(d.settings.find("image-refused").is_some(), "a line under the tiles");
    let shown = text(&d.settings, "image-refused");
    assert!(shown.to_lowercase().contains("fake.png isn't an image"), "{shown:?}");
    drop_on(&mut d, "app_icon_tiles", fake);
    assert!(d.settings.state.app_icon.added.is_empty(), "a drop is refused too");
    press(&mut d, "icon-add");
    d.settings.resolve_file_dialog(Some(big.clone())).expect("a file dialog");
    d.settle();
    assert_eq!(d.settings.state.app_icon.added.len(), 1, "a 20 MB image is a tile");
    assert!(d.settings.find("image-refused").is_none(), "and the line is gone");
}

/// The images kept as icons are read off the main thread: until one is, a half naming it shows the
/// theme's icon and its tile is there, a placeholder; once it's read the half shows it, and the
/// Dock hears of it (#87).
#[test]
fn a_kept_image_shows_the_themes_icon_until_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.icns");
    let mut bytes = std::fs::read(&images()[0]).unwrap();
    bytes.resize(20_000_000, 0);
    std::fs::write(&big, bytes).unwrap();
    let id = format!("{}{}", crate::platform::app_icon::FILE, big.display());
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let choice = Icons::Pair { dark: id.clone(), light: "full-moon".into() };
    let mut icon = AppIcon::new(Some(manifest()), Some(choice.clone()), Icons::One { icon: "phantom".into() }, move |change| {
        if let crate::model::settings::IconChange::Loaded(icon) = change {
            log.borrow_mut().push(icon.id.clone());
        }
    })
    .with_added(vec![id.clone()]);
    icon.hold_loads();
    icon.load();
    assert!(icon.poll().is_empty(), "nothing read yet: the first frame doesn't wait for it");
    assert_eq!(icon.added, [id.clone()], "its tile is there, a placeholder");
    assert!(icon.image(&id).is_none() && icon.manifest.as_ref().unwrap().icon(&id).is_none(), "no image yet");
    assert_eq!(icon.choice, Some(choice), "the choice is kept");
    assert_eq!(icon.sides(), Some(["phantom".to_owned(), "full-moon".to_owned()]), "the theme's icon in the half naming it");
    icon.release_loads();
    assert_eq!(icon.poll(), [crate::model::settings::Arrival::Loaded]);
    assert_eq!(icon.sides(), Some([id.clone(), "full-moon".to_owned()]), "the image shows once it's read");
    assert_eq!(*heard.borrow(), [id], "and the Dock was told");
}

/// The thread that reads an image answers through the window's waker, and the app takes it as it
/// comes (#87).
#[test]
fn an_image_is_read_on_a_thread_of_its_own() {
    let (tx, rx) = std::sync::mpsc::channel();
    let waker: rune::Waker = std::sync::Arc::new(move || {
        let _ = tx.send(());
    });
    let mut icon = AppIcon::new(Some(manifest()), None, Icons::One { icon: "phantom".into() }, |_| {}).with_added(vec![file_id(0)]);
    icon.load_in_background(waker);
    icon.load();
    rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the thread woke the window");
    assert_eq!(icon.poll(), [crate::model::settings::Arrival::Loaded]);
    assert!(icon.manifest.as_ref().unwrap().icon(&file_id(0)).is_some(), "read");
}

/// A shipped icon's id among the added images is no added image: no tile of its own, no ×; a
/// real image beside it stays (#87).
#[test]
fn a_shipped_icons_id_is_not_an_added_image() {
    let icon = AppIcon::new(Some(manifest()), None, Icons::One { icon: "phantom".into() }, |_| {}).with_added(vec![
        "phantom".into(),
        file_id(0),
        "phantom".into(),
        format!("{}", crate::platform::app_icon::FILE),
    ]);
    assert_eq!(icon.added, [file_id(0)]);
}

/// Follow theme's tile shows the icon the theme gives each Appearance, not the default's (#87).
#[test]
fn the_follow_theme_tile_shows_the_themes_icon() {
    let theme = Icons::Pair { dark: "dark-hour".into(), light: "full-moon".into() };
    let icon = AppIcon::new(Some(manifest()), None, theme, |_| {});
    assert_eq!(icon.theme_icon(true).as_deref(), Some("dark-hour"));
    assert_eq!(icon.theme_icon(false).as_deref(), Some("full-moon"));
}

/// An image kept as a half's icon that's gone (moved, or deleted) when the app starts: that half
/// follows the theme, the other keeps its icon (#87).
#[test]
fn an_image_gone_makes_its_half_follow_the_theme() {
    let gone = format!("{}/nowhere/moon.png", crate::platform::app_icon::FILE);
    let choice = Icons::Pair { dark: gone, light: "full-moon".into() };
    let mut icon = AppIcon::new(Some(manifest()), Some(choice), Icons::One { icon: "phantom".into() }, |_| {});
    icon.load();
    assert_eq!(icon.poll(), [crate::model::settings::Arrival::Gone]);
    assert_eq!(icon.choice, Some(Icons::Pair { dark: FOLLOW.into(), light: "full-moon".into() }));
    assert_eq!(icon.sides(), Some(["phantom".to_owned(), "full-moon".to_owned()]), "the theme's in Dark");
}

/// The tab's App icon row shows the icon each half shows: both, each with a small ☀ or ☾ under
/// it, while they differ; one while they're the same (#87).
#[test]
fn the_app_icon_row_shows_both_halves() {
    let mut d = appearance_tab();
    let shows = |d: &Desk| ["both", "light", "dark"].map(|h| d.settings.find(&format!("app-icon-preview-{h}")).is_some());
    assert_eq!(shows(&d), [true, false, false], "a fresh install's: Phantom in both");
    press(&mut d, "app-icon-edit");
    press(&mut d, "icon-pair-0");
    press(&mut d, "icons-done");
    assert_eq!(shows(&d), [false, true, true], "a pair: both, each with its half's mark");
    assert!(d.settings.find("app-icon-preview-light/mark").is_some() && d.settings.find("app-icon-preview-dark/mark").is_some());
    // ☀ light and ☾ dark, as the chips are: the Light page with its ink, the Dark page with its.
    let fresh = LookPreset::fresh();
    let [light, dark] = Appearance::BOTH.map(|a| Look::build(fresh.get(a)).ui.tokens().c.clone());
    let drawn = |half: &str, part: &str| {
        let end = format!("app-icon-preview-{half}/mark{part}");
        d.settings.elements().into_iter().find(|e| e.path.ends_with(&end) && !e.exiting).unwrap_or_else(|| panic!("{end}"))
    };
    assert_eq!(drawn("light", "").fill, light.page, "☀ on a light page");
    assert_eq!(drawn("dark", "").fill, dark.page, "☾ on a dark page, not drawn light on the window");
    assert_eq!(drawn("light", "/glyph").text_color, Some(light.text_base));
    assert_eq!(drawn("dark", "/glyph").text_color, Some(dark.text_base));
}

/// A manifest of seven designed pairs over as many icons, all the same image.
fn seven_pairs() -> Manifest {
    let bytes = std::fs::read(manifest_dir().join("full-moon.icns")).expect("a shipped icon");
    let icons: Vec<String> = (0..8).map(|i| format!(r#"{{"id": "i{i}", "file": "i{i}.icns"}}"#)).collect();
    let pairs: Vec<String> = (0..7).map(|i| format!(r#"{{"name": "Pair {i}", "dark": "i{i}", "light": "i{}"}}"#, i + 1)).collect();
    let json = format!(r#"{{"default": "i0", "icons": [{}], "pairs": [{}]}}"#, icons.join(","), pairs.join(","));
    Manifest::parse(&json, |_| Ok(bytes.clone())).expect("a manifest")
}

/// The App icon sheet's Pairs are tiles of the Icons' width, but shorter (no chips), on the Icons' grid: pair `i` has the
/// column of icon `i`, and a seventh wraps to another row, as the number of pairs isn't fixed
/// (#87).
#[test]
fn pair_tiles_are_on_the_icons_grid() {
    let mut d = appearance_tab();
    d.settings.state.app_icon = AppIcon::new(Some(seven_pairs()), None, Looks::default().app_icons(), |_| {});
    d.settle();
    press(&mut d, "app-icon-edit");
    let rect = |key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    let icons: Vec<[f32; 4]> =
        std::iter::once("icon-follow".to_owned()).chain((0..8).map(|i| format!("icon-i{i}"))).map(|k| rect(&k)).collect();
    let columns = icons.iter().take_while(|r| r[1] == icons[0][1]).count();
    assert!(columns >= 2 && columns < 7, "the icons wrap too, in {columns} columns");
    let pairs: Vec<[f32; 4]> = (0..7).map(|i| rect(&format!("icon-pair-{i}"))).collect();
    for (i, pair) in pairs.iter().enumerate() {
        assert_eq!(pair[2], icons[0][2], "pair {i} is as wide as an icon's tile");
        assert!(pair[3] < icons[0][3], "pair {i} is shorter: no chips, no empty room for them");
        assert_eq!(pair[0], icons[i % columns][0], "pair {i} in the column of icon {}", i % columns);
        assert_eq!(pair[1], pairs[i - i % columns][1], "pair {i} on its row");
    }
    assert!(pairs[6][1] > pairs[0][1], "seven pairs wrap to a second row");
    assert!(icons[0][1] > pairs[6][1] + pairs[6][3], "the Icons below the last row of pairs");
}

/// A designed pair in use marks the ☀ chip under its Light icon and the ☾ chip under its Dark one
/// in the Icons below, and no other chip but those (#87).
#[test]
fn a_pair_in_use_marks_both_chips_below() {
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    let pairs = manifest().pairs().to_vec();
    for (i, pair) in pairs.iter().enumerate() {
        press(&mut d, &format!("icon-pair-{i}"));
        let marked: Vec<String> = d
            .settings
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.path.ends_with("/in-use") && e.path.contains("icon-") && !e.path.contains("icon-pair-"))
            .filter_map(|e| e.path.strip_suffix("/#0/in-use")?.rsplit('/').next().map(str::to_owned))
            .collect();
        let mut want = vec![format!("icon-{}-light", pair.light), format!("icon-{}-dark", pair.dark)];
        want.sort();
        let mut got = marked;
        got.sort();
        assert_eq!(got, want, "pair {i} in use: its two chips");
    }
}

/// Same theme, a hairline between the editor's rows is one line across both halves, in one color,
/// whether the rows are joined or split (#87).
#[test]
fn a_rows_hairline_runs_unbroken_across_both_halves() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    for split in [false, true] {
        if split {
            press(&mut d, "edit-theme-split");
        }
        let card = rect(&d, "halves-card");
        let seam = rect(&d, "edit-tone-right")[0];
        let mut colors = Vec::new();
        for i in 1..=2 {
            let key = format!("halves-rule-{i}-line");
            let lines: Vec<_> = d.settings.elements().into_iter().filter(|e| e.key.as_deref() == Some(&key) && !e.exiting).collect();
            assert_eq!(lines.len(), 1, "rule {i}: one hairline, not one per half");
            let line = lines[0].rect;
            assert!(line[0] < seam - 8.0 && line[0] + line[2] > seam + 8.0, "rule {i} crosses the seam: {line:?}, seam {seam}");
            assert!(line[0] - card[0] <= 12.0 && (card[0] + card[2]) - (line[0] + line[2]) <= 12.0, "and runs the card's width: {line:?}");
            colors.push(lines[0].fill);
        }
        assert_eq!(colors[0], colors[1], "one color for every rule");
    }
}

/// The tab's App icon row: the previews (an icon, its ☀ or ☾ mark at its corner) are centred on
/// Edit…'s line, and the row stays as tall as the others (#87).
#[test]
fn the_app_icon_rows_previews_are_centred_with_edit() {
    let mut d = appearance_tab();
    let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    let middle = |r: [f32; 4]| r[1] + r[3] / 2.0;
    let other = rect(&d, "look_preset")[3];
    for pair in [false, true] {
        if pair {
            press(&mut d, "app-icon-edit");
            press(&mut d, "icon-pair-0");
            press(&mut d, "icons-done");
        }
        let edit = rect(&d, "app-icon-edit");
        let previews: Vec<[f32; 4]> = ["both", "light", "dark"]
            .iter()
            .filter_map(|h| super::generated::rect_of(&d.settings, &format!("app-icon-preview-{h}")))
            .collect();
        assert_eq!(previews.len(), if pair { 2 } else { 1 });
        for p in previews {
            assert!((middle(p) - middle(edit)).abs() <= 0.5, "pair {pair}: preview {p:?} centred on Edit… {edit:?}");
            assert!(p[3] <= other, "pair {pair}: no taller than another row ({other})");
        }
        for h in ["both", "light", "dark"] {
            if let Some(icon) = super::generated::rect_of(&d.settings, &format!("app-icon-preview-{h}/icon")) {
                assert!((middle(icon) - middle(edit)).abs() <= 0.5, "pair {pair}: the {h} icon {icon:?} centred on Edit… {edit:?}");
            }
        }
    }
}

/// What the App icon row's caption says: a designed pair's name; an icon's, when both halves have
/// it; else both, the Light's first, each shortened in the middle when long (#87).
#[test]
fn the_app_icon_rows_caption_names_what_the_halves_show() {
    let caption = |choice: Option<Icons>| {
        let mut d = appearance_tab();
        d.settings.state.app_icon = AppIcon::new(Some(manifest()), choice, Looks::default().app_icons(), |_| {});
        d.settle();
        texts_in(&d.settings, "app_icon-note").concat()
    };
    let pair = |dark: &str, light: &str| Some(Icons::Pair { dark: dark.into(), light: light.into() });
    assert_eq!(caption(None), "Follow theme");
    assert_eq!(caption(pair("memento-mori-dark-hour", "memento-mori")), "Memento Mori", "a designed pair: its name");
    assert_eq!(caption(Some(Icons::One { icon: "phantom".into() })), "Phantom", "one icon");
    assert_eq!(caption(pair("phantom", "phantom")), "Phantom", "the same icon in both halves");
    assert_eq!(caption(pair(FOLLOW, FOLLOW)), "Follow theme", "Follow theme is a name like any");
    assert_eq!(caption(pair("phantom", "full-moon")), "Full Moon · Phantom", "unrelated: Light first");
    assert_eq!(caption(pair(FOLLOW, "full-moon")), "Full Moon · Follow theme");
    let bytes = std::fs::read(manifest_dir().join("full-moon.icns")).expect("a shipped icon");
    let json = r#"{"default": "a", "icons": [{"id": "a", "name": "An icon with a name far too long to show", "file": "a.icns"},
        {"id": "b", "name": "Full Moon", "file": "b.icns"}], "pairs": []}"#;
    let long = Manifest::parse(json, |_| Ok(bytes.clone())).expect("a manifest");
    let mut d = appearance_tab();
    d.settings.state.app_icon = AppIcon::new(Some(long), pair("a", "b"), Looks::default().app_icons(), |_| {});
    d.settle();
    let both = texts_in(&d.settings, "app_icon-note").concat();
    let (light, dark) = both.split_once(" · ").expect("both names");
    assert_eq!(light, "Full Moon");
    assert!(dark.contains('…') && dark.chars().count() <= 24, "a long one shortened in the middle: {dark}");
}

/// The name drawn in the tile keyed `tile`: its text and its box.
fn tile_name_of(d: &Desk, tile: &str) -> (String, [f32; 4]) {
    let end = format!("{tile}/name");
    let e = d.settings.elements().into_iter().find(|e| e.path.ends_with(&end) && !e.exiting).unwrap_or_else(|| panic!("{end}"));
    (e.text.unwrap_or_default(), e.rect)
}

/// No icon's or pair's name in the sheet is shortened: each is drawn whole, in the two lines its box
/// holds (#87).
#[test]
fn no_name_in_the_icon_sheet_is_shortened() {
    let mut d = appearance_tab();
    press(&mut d, "app-icon-edit");
    let m = manifest();
    let tiles: Vec<(String, String)> = m
        .pairs()
        .iter()
        .enumerate()
        .map(|(i, p)| (format!("icon-pair-{i}"), p.name.clone()))
        .chain(m.icons().iter().map(|i| (format!("icon-{}", i.id), i.name.clone())))
        .collect();
    assert!(tiles.len() >= 20);
    let font = Look::build(LookPreset::fresh().get(Appearance::Light)).ui.tokens().ty.text_xsmall.lh(crate::ui::NAME_LINE);
    for (tile, name) in tiles {
        let (shown, rect) = tile_name_of(&d, &tile);
        assert_eq!(shown, name, "{tile}: the name whole");
        let (_, wrapped) = rune::text::measure(&name, &[], font, Some(rect[2]));
        assert!(wrapped <= rect[3] + 0.5, "{tile}: {name:?} wraps to {wrapped} pt, past its box {rect:?}");
    }
}

/// An in-use chip's outline (3 pt, 2 pt out) never reaches the name above it, whatever the name's
/// length (#87).
#[test]
fn a_chips_outline_keeps_clear_of_the_name() {
    let mut d = appearance_tab();
    let bytes = std::fs::read(manifest_dir().join("full-moon.icns")).expect("a shipped icon");
    let json = r#"{"default": "a", "icons": [{"id": "a", "name": "An icon with a name far too long to show in two lines at all", "file": "a.icns"},
        {"id": "b", "name": "Moon", "file": "b.icns"}], "pairs": []}"#;
    let long = Manifest::parse(json, |_| Ok(bytes.clone())).expect("a manifest");
    d.settings.state.app_icon = AppIcon::new(Some(long), None, Looks::default().app_icons(), |_| {});
    d.settle();
    press(&mut d, "app-icon-edit");
    for id in ["a", "b"] {
        let (_, name) = tile_name_of(&d, &format!("icon-{id}"));
        for half in ["both", "light", "dark"] {
            let chip = super::generated::rect_of(&d.settings, &format!("icon-{id}-{half}")).expect("a chip");
            assert!(chip[1] - 5.0 >= name[1] + name[3], "icon {id}'s {half} chip {chip:?}: its outline reaches the name {name:?}");
        }
    }
}

/// The tab's App icon row is one line: the label, then in a row the caption, the previews and Edit…,
/// no taller than another row (#87).
#[test]
fn the_app_icon_row_is_one_line() {
    let mut d = appearance_tab();
    let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}"));
    // A setting's row: a 36 pt line (`min_h`) in 4 pt padding.
    let single = 36.0 + 2.0 * 4.0;
    for pair in [false, true] {
        if pair {
            press(&mut d, "app-icon-edit");
            press(&mut d, "icon-pair-0");
            press(&mut d, "icons-done");
        }
        let (row, edit, caption) = (rect(&d, "app_icon"), rect(&d, "app-icon-edit"), rect(&d, "app_icon-note"));
        assert!(row[3] <= single, "pair {pair}: the row {row:?} is one line: as tall as another setting's ({single})");
        let middle = |r: [f32; 4]| r[1] + r[3] / 2.0;
        assert!((middle(caption) - middle(edit)).abs() <= 0.5, "pair {pair}: caption {caption:?} on Edit…'s line {edit:?}");
        let previews: Vec<[f32; 4]> = ["both", "light", "dark"]
            .iter()
            .filter_map(|h| super::generated::rect_of(&d.settings, &format!("app-icon-preview-{h}")))
            .collect();
        assert!(!previews.is_empty());
        let first = previews.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        let last = previews.iter().map(|p| p[0] + p[2]).fold(0.0, f32::max);
        assert!(caption[0] + caption[2] <= first + 0.5, "pair {pair}: the caption before the previews");
        assert!(last <= edit[0] + 0.5, "pair {pair}: the previews before Edit…");
    }
}

/// Corners and Motion are dropdowns: the button shows the whole chosen label and opening it lists
/// every option whole, nothing cut; Tone's Light | Dark are segments, each label whole; at the
/// sheet's widest and at the 640 pt minimum.
#[test]
fn the_look_sheets_choices_show_their_labels_whole() {
    let font = Look::build(LookPreset::fresh().get(Appearance::Light)).ui.tokens().ty.ui;
    let rows: [(&str, &str, &[&str]); 2] = [
        ("edit-corners-both", "Small", &["Square", "Small", "Round"]),
        ("edit-motion-both", "Follow Reduce Motion", &["Follow Reduce Motion", "On", "Off"]),
    ];
    for width in [960.0, 640.0] {
        let mut d = appearance_tab();
        d.settings.set_viewport((width, 900.0), 2.0);
        d.settle();
        press(&mut d, "active-edit");
        for control in ["edit-tone-light", "edit-tone-dark"] {
            let elements = d.settings.elements();
            for (j, name) in ["Light", "Dark"].into_iter().enumerate() {
                let segment = format!("{control}/#0/segment{j}/");
                let text = elements.iter().find(|e| e.path.contains(&segment) && e.text.as_deref() == Some(name)).expect("its label");
                let (needs, _) = rune::text::measure(name, &[], font, None);
                assert!(needs <= text.rect[2] + 0.5, "at {width} pt {name:?} needs {needs} but is drawn {} wide", text.rect[2]);
            }
        }
        for (control, shown, options) in rows {
            let elements = d.settings.elements();
            let value = elements.iter().find(|e| e.path.ends_with(&format!("{control}-trigger/value")) && !e.exiting);
            let value = value.unwrap_or_else(|| panic!("no dropdown {control}"));
            assert_eq!(value.text.as_deref(), Some(shown), "{control}");
            let (needs, _) = rune::text::measure(shown, &[], font, None);
            assert!(needs <= value.rect[2] + 0.5, "at {width} pt {shown:?} needs {needs} but its box is {} wide", value.rect[2]);
            press(&mut d, &format!("{control}-trigger"));
            let list = format!("{control}-menu-menu");
            let elements = d.settings.elements();
            let items: Vec<_> = elements.iter().filter(|e| e.path.contains(&list) && !e.exiting && e.role == "menuitemcheckbox").collect();
            let labels: Vec<_> = items.iter().filter_map(|e| e.label.as_deref()).collect();
            assert_eq!(labels, options, "at {width} pt {control}'s list, in order");
            for (item, name) in items.iter().zip(options) {
                let below = format!("{}/", item.path);
                let text = elements.iter().find(|e| e.path.starts_with(&below) && e.text.as_deref() == Some(*name)).expect("its text");
                let (needs, _) = rune::text::measure(name, &[], font, None);
                assert!(needs <= text.rect[2] + 0.5, "at {width} pt {name:?} needs {needs} but is drawn {} wide", text.rect[2]);
            }
            d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
            d.settle();
        }
    }
}

/// A choice's dropdown works from the keyboard: Tab focuses it, Space or Return opens its list,
/// the arrow keys move in it, Return picks and Escape closes without changing the setting.
#[test]
fn a_choices_dropdown_works_from_the_keyboard() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    let key = |d: &mut Desk, key: Key| {
        d.settings.key(KeyEvent { key, mods: Mods::default() });
        d.settle();
    };
    let tab_to = |d: &mut Desk, control: &str| {
        d.front(Which::Settings);
        d.settings.update(|_, tx| tx.blur());
        d.settle();
        let mut tabs = 0;
        while !super::focus_path(&d.settings).is_some_and(|p| p.contains(&format!("/{control}-trigger"))) {
            assert!(tabs < 200, "Tab never reaches {control}");
            key(d, Key::Tab);
            tabs += 1;
        }
    };
    let open =
        |d: &Desk, control: &str| d.settings.elements().iter().any(|e| e.path.contains(&format!("{control}-menu-menu")) && !e.exiting);
    let shown = |d: &Desk, control: &str| {
        let trigger = format!("{control}-trigger/value");
        d.settings.elements().into_iter().find(|e| e.path.ends_with(&trigger) && !e.exiting).and_then(|e| e.text)
    };
    let control = "edit-corners-both";
    for opener in [Key::Text(" ".into()), Key::Enter] {
        tab_to(&mut d, control);
        assert!(!open(&d, control), "closed to start with");
        key(&mut d, opener.clone());
        assert!(open(&d, control), "{opener:?} opens the list");
        key(&mut d, Key::Escape);
        assert!(!open(&d, control), "Escape closes it");
        assert_eq!(shown(&d, control).as_deref(), Some("Small"), "and changes nothing");
    }
    // Down moves to the next option, Return picks it.
    tab_to(&mut d, control);
    key(&mut d, Key::Enter);
    key(&mut d, Key::Down);
    key(&mut d, Key::Enter);
    assert!(!open(&d, control), "a pick closes the list");
    assert_eq!(shown(&d, control).as_deref(), Some("Round"), "Down then Return picks the next option");
    // Up twice from the opened list goes past the start of it: Square.
    key(&mut d, Key::Enter);
    key(&mut d, Key::Up);
    key(&mut d, Key::Up);
    key(&mut d, Key::Enter);
    assert_eq!(shown(&d, control).as_deref(), Some("Square"), "Up moves back");
    // Moving, then Escape: closed, nothing picked.
    key(&mut d, Key::Enter);
    key(&mut d, Key::Down);
    key(&mut d, Key::Escape);
    assert!(!open(&d, control), "Escape closes it after moving");
    assert_eq!(shown(&d, control).as_deref(), Some("Square"), "and picks nothing");
}

/// The Active look's sheet with "Every color" open, Settings `width` wide.
fn colors_sheet(width: f32) -> Desk {
    let mut d = appearance_tab();
    d.settings.set_viewport((width, 900.0), 2.0);
    d.settle();
    press(&mut d, "active-edit");
    press(&mut d, "edit-every-color-section");
    d
}

/// The color the setting `key` holds in the Active look's `half`.
fn color_of(d: &Desk, half: Appearance, key: &str) -> Option<Value> {
    d.main.state.looks.borrow().preset().get(half).settings.get(key)
}

/// Every name in the Colors sections shows whole, wrapped if need be, never cut, at the sheet's
/// widest and at the 640 pt minimum; and each header and group is there.
#[test]
fn the_colors_names_are_never_cut() {
    for width in [960.0, 640.0] {
        let mut d = colors_sheet(width);
        for key in ["edit-look-title", "edit-colors-title", "edit-every-color-section", "edit-group-text", "edit-group-status"] {
            assert!(d.settings.find(key).is_some(), "at {width} pt {key} is drawn");
        }
        let names: BTreeSet<&str> =
            crate::theme::phantom::palette::DERIVED.iter().map(|c| c.label).chain(["Background", "Warning"]).collect();
        // The sheet scrolls: each name is checked once it's in view.
        for name in names {
            let key = d
                .settings
                .elements()
                .into_iter()
                .find(|e| !e.exiting && e.text.as_deref() == Some(name) && e.path.contains("halves-card"))
                .map(|e| e.path)
                .unwrap_or_else(|| panic!("at {width} pt {name:?} is drawn"));
            let row = key.split('/').find(|p| p.starts_with("edit-") && !p.ends_with("-left")).unwrap().to_owned();
            scroll_to(&mut d.settings, &row);
            let text = d.settings.elements().into_iter().find(|e| !e.exiting && e.text.as_deref() == Some(name)).unwrap();
            assert!(!text.text_cut, "at {width} pt {name:?} is cut: {:?}", text.rect);
        }
    }
}

/// A swatch's picker opens beside it, inside the window, at both widths, wherever its swatch is:
/// the square, the bar, the hex field and the button, each named for screen readers.
#[test]
fn the_picker_opens_inside_the_window() {
    for width in [960.0, 640.0] {
        // Light and Dark's colors differ: each row is split, a swatch in each half.
        for (swatch, field) in
            [("edit-accent-dark-swatch", "edit-accent-dark"), ("edit-success_edge-dark-swatch", "edit-success_edge-dark")]
        {
            let mut d = colors_sheet(width);
            press(&mut d, swatch);
            let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key} at {width}"));
            let popup = rect(&d, &format!("{field}-popup"));
            assert!(popup[0] >= 0.0 && popup[1] >= 0.0, "at {width} pt {popup:?} inside");
            assert!(popup[0] + popup[2] <= width + 0.5 && popup[1] + popup[3] <= 900.5, "at {width} pt {popup:?} inside");
            let label = |key: &str| d.settings.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.label);
            assert_eq!(label(&format!("{field}-square")).as_deref(), Some("Hue and lightness"));
            assert_eq!(label(&format!("{field}-bar")).as_deref(), Some("Saturation"));
            assert!(d.settings.find(&format!("{field}-follow")).is_some(), "its Follow or Reset");
            assert!(super::focus_path(&d.settings).is_some_and(|p| p.contains(&format!("/{field}/"))), "its hex field has the focus");
        }
    }
}

/// A swatch says what it shows: its hex and what it follows, or that it's the theme's own; set by
/// hand, it's ringed in the accent and says so.
#[test]
fn a_swatch_says_what_it_follows() {
    let mut d = colors_sheet(960.0);
    let description =
        |d: &Desk, key: &str| d.settings.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.description);
    assert_eq!(description(&d, "edit-muted_text-light-swatch").as_deref(), Some("#606369 · follows Text"));
    assert_eq!(description(&d, "edit-accent-light-swatch").as_deref(), Some("#2f7fe3 · theme default"));
    type_into(&mut d, "edit-faint_text-light", "#b0b0b0");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    assert_eq!(description(&d, "edit-faint_text-light-swatch").as_deref(), Some("#b0b0b0 · set by hand"));
}

/// Dragging in the square picks as it goes, set by hand, and is one step of Undo; a faint text
/// that's hard to read warns under the picker; Follow Text puts it back to the rule.
#[test]
fn a_drag_picks_and_follow_puts_it_back() {
    let mut d = colors_sheet(960.0);
    let light = Appearance::Light;
    let before = color_of(&d, light, "faint_text");
    assert_eq!(before, Some(Value::Color(None)));
    press(&mut d, "edit-faint_text-light-swatch");
    let square = super::generated::rect_of(&d.settings, "edit-faint_text-light-square").expect("the square");
    let (x, y) = (square[0] + square[2] * 0.6, square[1] + square[3] * 0.3);
    d.settings.pointer_down(x, y);
    d.settings.pointer_move(x + 10.0, y + 5.0);
    d.settle();
    let Some(Value::Color(Some(moving))) = color_of(&d, light, "faint_text") else { panic!("set by hand as it moves") };
    d.settings.pointer_move(x + 20.0, y + 10.0);
    d.settings.pointer_up(x + 20.0, y + 10.0);
    d.settle();
    let Some(Value::Color(Some(picked))) = color_of(&d, light, "faint_text") else { panic!("set by hand") };
    assert_ne!(moving, picked, "it followed the pointer");
    assert_eq!(crate::changes::menu_title(&d.main.state), "Undo Faint Text Change");
    let warned = d
        .settings
        .elements()
        .into_iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("edit-faint_text-light-warning-")))
        .count();
    let resolved = d.main.state.looks.borrow().preset().get(light).resolved();
    assert_eq!(warned, resolved.warnings("faint_text").len(), "one line per warning");
    // Light at 30% from the top: hard to read on white.
    assert!(warned > 0);
    press(&mut d, "edit-faint_text-light-follow");
    assert_eq!(color_of(&d, light, "faint_text"), Some(Value::Color(None)), "Follow Text");
    d.settings.update(|_, tx| tx.blur());
    d.key(KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } });
    d.settle();
    assert_eq!(color_of(&d, light, "faint_text"), Some(Value::Color(Some(picked))), "⌘Z: back to the pick");
    // The undo reopened the picker on its field, whose ⌘Z is its own.
    d.settings.update(|_, tx| tx.blur());
    d.key(KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } });
    d.settle();
    assert_eq!(color_of(&d, light, "faint_text"), before, "⌘Z again: the drag was one step");
}

/// Where the picker goes is worked out for every warning its color could have: beside its swatch,
/// top edges level when the tallest it could get fits below, else bottom edges level. A drag that
/// brings warnings up grows it away from that edge, never moving the edge or the swatch, in a
/// window tall enough for it to fit below and in one too short; the color applies all the while.
#[test]
fn a_drag_that_brings_warnings_never_moves_the_picker() {
    let mut placed = Vec::new();
    for (width, height) in [(960.0, 900.0), (640.0, 900.0), (960.0, 760.0)] {
        let at = format!("at {width}x{height}");
        let mut d = appearance_tab();
        d.settings.set_viewport((width, height), 2.0);
        d.settle();
        press(&mut d, "active-edit");
        press(&mut d, "edit-accent-light-swatch");
        let rect = |d: &Desk, key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key} {at}"));
        let warned = |d: &Desk| {
            d.settings
                .elements()
                .into_iter()
                .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("edit-accent-light-warning-")))
                .count()
        };
        let swatch = rect(&d, "edit-accent-light-swatch");
        let popup = rect(&d, "edit-accent-light-popup");
        assert_eq!(warned(&d), 0, "{at} the default Accent has no warning");
        let below = (popup[1] - swatch[1]).abs() < 0.5;
        let bottom = |r: [f32; 4]| r[1] + r[3];
        assert!(below || (bottom(popup) - bottom(swatch)).abs() < 0.5, "{at} beside its swatch, an edge level: {popup:?} {swatch:?}");
        placed.push(below);
        // The edge it keeps.
        let edge = |r: [f32; 4]| if below { r[1] } else { bottom(r) };
        let square = rect(&d, "edit-accent-light-square");
        let (x, y) = (square[0] + square[2] / 2.0, square[1] + square[3] / 2.0);
        d.settings.pointer_down(x, y);
        let mut colors = Vec::new();
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            d.settings.pointer_move(x - 110.0 * t, y - 40.0 * t);
            d.settle();
            let now = rect(&d, "edit-accent-light-popup");
            assert_eq!(edge(now), edge(popup), "{at}, step {i}: the popup keeps its edge");
            assert_eq!((now[0], now[2]), (popup[0], popup[2]), "{at}, step {i}: and its side");
            assert_eq!(rect(&d, "edit-accent-light-swatch"), swatch, "{at}, step {i}: the swatch stays");
            colors.push(color_of(&d, Appearance::Light, "accent"));
        }
        assert!(warned(&d) > 0, "{at} the drag ends on a light Accent, hard to see");
        assert!(rect(&d, "edit-accent-light-popup")[3] > popup[3], "{at} grown for its warnings");
        d.settings.pointer_up(x - 110.0, y - 40.0);
        d.settle();
        assert_eq!(edge(rect(&d, "edit-accent-light-popup")), edge(popup), "{at} the popup keeps its edge as the drag ends");
        colors.dedup();
        assert!(colors.len() > 5, "{at} the color applies as it moves: {colors:?}");
    }
    assert_eq!(placed, [true, true, false], "below where it fits, else above");
}

/// The picker draws where it goes from its first frame: placed for the tallest it could get in the
/// same pass that opens it, never below for a frame before it moves up, in a window where it goes
/// above (960x760) and one where it goes below (960x900).
#[test]
fn the_picker_is_in_place_from_its_first_frame() {
    for height in [760.0, 900.0] {
        let mut d = appearance_tab();
        d.settings.set_viewport((960.0, height), 2.0);
        d.settle();
        press(&mut d, "active-edit");
        press_now(&mut d, "edit-accent-light-swatch");
        d.settings.frame(super::FRAME);
        let first = super::generated::rect_of(&d.settings, "edit-accent-light-popup").expect("drawn on its first frame");
        d.settle();
        let settled = super::generated::rect_of(&d.settings, "edit-accent-light-popup").expect("the popup");
        assert_eq!(first, settled, "at 960x{height} the first frame is where it stays");
    }
}

/// The picker is as tall as what it shows: its frame ends its padding under the last thing in it,
/// the hex field and its button with no warning, the last warning with one or two, in either half
/// and at 640 pt. The tallest it could get only places it, unseen.
#[test]
fn the_picker_is_as_tall_as_what_it_shows() {
    // (width, field, hex typed, warnings)
    let cases = [
        (960.0, "edit-accent-light", None, 0),
        (960.0, "edit-accent-dark", None, 0),
        (640.0, "edit-accent-light", None, 0),
        (960.0, "edit-danger-light", Some("#e06060"), 1),
        (960.0, "edit-faint_text-light", Some("#a3a6ab"), 2),
        (960.0, "edit-faint_text-dark", Some("#5a5c60"), 2),
        (640.0, "edit-faint_text-light", Some("#a3a6ab"), 2),
    ];
    for (width, field, hex, warnings) in cases {
        let at = format!("{field} at {width}");
        let mut d = colors_sheet(width);
        match hex {
            Some(hex) => {
                type_into(&mut d, field, hex);
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
                d.settle();
            }
            None => press(&mut d, &format!("{field}-swatch")),
        }
        let rect = |key: &str| super::generated::rect_of(&d.settings, key).unwrap_or_else(|| panic!("{key}, {at}"));
        let lines: Vec<[f32; 4]> = (0..).map_while(|i| super::generated::rect_of(&d.settings, &format!("{field}-warning-{i}"))).collect();
        assert_eq!(lines.len(), warnings, "{at}: its warnings");
        let last = lines.last().copied().unwrap_or_else(|| rect(&format!("{field}-follow")));
        let popup = rect(&format!("{field}-popup"));
        let under = popup[1] + popup[3] - (last[1] + last[3]);
        assert!((under - 12.0).abs() < 0.5, "{at}: the frame ends its padding under its last line, not {under} pt");
    }
}

/// Reset on a changed base: the theme's own again, and Escape closes the picker, the sheet staying.
#[test]
fn reset_and_escape() {
    let mut d = colors_sheet(960.0);
    type_into(&mut d, "edit-accent-dark", "#ff8000");
    d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    d.settle();
    let orange = Some(Value::Color(Some(Rgb([0xff, 0x80, 0x00]))));
    assert_eq!(color_of(&d, Appearance::Dark, "accent"), orange);
    let says = d.settings.elements().into_iter().filter(|e| e.path.contains("edit-accent-dark-follow")).find_map(|e| e.text);
    assert_eq!(says.as_deref(), Some("Reset"));
    press(&mut d, "edit-accent-dark-follow");
    assert_eq!(color_of(&d, Appearance::Dark, "accent"), Some(Value::Color(None)));
    d.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    d.settle();
    let sheet = d.settings.state.sheet.as_ref().unwrap();
    assert!(sheet.editor().is_some_and(|e| e.picker.is_none()), "the picker closed, the sheet stays");
}

/// An undo from the main window, a color's picker open with text that's no color: the text goes
/// back, the picker closes, and the control the undo shows has the focus, not the picker's swatch.
#[test]
fn an_undo_that_closes_the_picker_focuses_what_it_shows() {
    let mut d = appearance_tab();
    press(&mut d, "active-edit");
    choose(&mut d, "edit-corners-both", 0);
    type_into(&mut d, "edit-faint_text-light", "#123");
    d.front(Which::Main);
    d.settle();
    d.key(KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } });
    d.settle();
    let sheet = d.settings.state.sheet.as_ref().unwrap();
    assert!(sheet.editor().is_some_and(|e| e.picker.is_none()), "the picker closed");
    let focused = super::focus_path(&d.settings);
    assert!(focused.as_deref().is_some_and(|p| p.contains("edit-corners-both")), "Corners is focused, not {focused:?}");
}
