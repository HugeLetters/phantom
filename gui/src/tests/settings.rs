//! Generated sequences on the Settings window (#257) beside the main window against `MemoryService`:
//! a person opening it with ⌘, (in either window) and closing it (Escape, its close button, ⌘W with
//! it in front), closing the main window with ⌘W and getting it back from the Dock, switching tabs, typing ports (good ones, a taken one, and text that isn't a
//! port) and sending them, typing speed limits (good ones, empty for unlimited, too low, too high
//! and text that isn't a speed) and sending them, letting the port be picked, flipping UPnP and keeping the Mac awake, choosing a default folder
//! (or one on a drive that isn't connected, or cancelling) and clearing it, and picking App icons
//! and custom pairs, and, in the Theme section, setting each theme setting for both Appearances or, split,
//! for one (Tone, and the tests' second theme's toggle, choice and colour), splitting and joining
//! fields, while the system's Appearance flips; another client (`phantom settings`) changing a
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
//! - the Theme section shows each field joined or split as the model says, each control showing its
//!   value, Split and Join where they apply, and a colour's field what's typed in it and its
//!   refusal until its text changes; a choice is a radio group named for its setting, its segments
//!   radios; Tab reaches it from the tabs and an arrow key picks the next or previous choice
//!   (#247), and Open settings and ⌘Z focus it like any other control;
//! - the windows that show and the one in front are the model's: ⌘W closes the one in front, the
//!   other staying; the Dock brings the main window back;
//! - Settings losing the front (a click on the main window, the Dock, ⌘Z there) leaves the field
//!   being edited, which applies it as a change of its own, a port, a limit or a colour (#254);
//! - ⌘Z takes back the newest change, whoever made it, in Settings or in the main window (#255): a
//!   setting's is as it was, Settings opens (if it was closed) in front at its tab with its control focused,
//!   and the status bar tells what it went back to ("Port is back to 40000"), or "Couldn't be
//!   undone: <why>"; a torrent removed (from the main window, or by another client, Settings open
//!   or not) is back in the list, Settings staying in front if it is, and told ("Zero is back in the
//!   list"); another client's change shows in the open sheet at once, a field not typed over
//!   following it; `phantom undo` takes the newest torrent action, `phantom settings undo` the
//!   newest service setting's change; Edit ▸ Undo names whichever ⌘Z takes next, in either window;
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
use crate::model::settings::SettingsWindow;
use crate::model::settings::{AppIcon, IconChange, Side, Switch, Tab};
use crate::platform::app_icon::{Icons, Manifest, manifest_dir, resolve};
use crate::service::{DiskChange, MemoryService};
use crate::shortcuts;
use crate::theme::Look;
use crate::theme::preset::{Appearance, LookPreset, Looks};
use crate::theme::setting::{Kind, Rgb, Value};
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
    /// A tile of the picker, by key.
    Icon(String),
    ToggleCustomPair,
    /// A side of the custom pair: dark or light, an icon's id.
    Side {
        dark: bool,
        id: String,
    },
    /// A choice theme setting's segment, by index, on a side.
    Choose(Side, &'static str, usize),
    /// From the tab shown, Tab to a choice theme setting's control on a side and press the arrow key
    /// toward the next segment (`true`: right), wrapping at the ends.
    Arrow(Side, &'static str, bool),
    /// A toggle theme setting's switch, on a side.
    Flip(Side, &'static str),
    /// Typed over a colour theme setting's field, and Return in it.
    TypeColour(Side, &'static str, String),
    SendColour(Side, &'static str),
    /// A field's Split or Join.
    Split(&'static str),
    Join(&'static str),
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
    Theme(LookPreset, Side, &'static str),
    Removed(u64),
}

/// What's typed into a colour's field: colours, either case, and not colours.
fn colour_texts() -> Vec<String> {
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
    /// A colour theme setting's field, on a side.
    Colour(Side, &'static str),
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
    custom_pair: bool,
    /// Every tile key with what it chooses, and every icon id.
    tiles: Vec<(String, Option<Icons>)>,
    ids: Vec<String>,
    /// The fake fails a new port or UPnP, as when the torrents don't start again with them.
    restarts_fail: bool,
    /// Error notices the status bar got.
    errors: usize,
    /// The control focused, by key, right after Open settings.
    focus: Option<String>,
    /// The Active Look Preset, and the system's Appearance.
    preset: LookPreset,
    dark: bool,
    /// The theme settings split by hand or set on one side since the sheet opened.
    split: BTreeSet<&'static str>,
    /// Each colour field shown: its text, and the text a refusal is about.
    colours: BTreeMap<(Side, &'static str), (String, Refusal)>,
    /// The text field with the focus: from a click in it until another step takes the focus away.
    focused: Option<Fld>,
    /// The changes ⌘Z takes back, newest last, and what each undo told, in order.
    changes: Vec<Change>,
    told: Vec<String>,
    /// The torrents in the list, by id.
    rows: BTreeSet<u64>,
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

    /// `name`'s value, as an undo tells it.
    fn value(&self, name: SettingName) -> String {
        let on = |on: bool| if on { "on" } else { "off" }.to_owned();
        match name {
            SettingName::DefaultDestination => self.folder.as_ref().map_or("none".into(), |f| f.display().to_string()),
            SettingName::ListenPort => match self.port {
                ListenPort::Pinned(port) => port.to_string(),
                ListenPort::Picked(port) => format!("automatic (now {port})"),
            },
            SettingName::Upnp => on(self.upnp),
            SettingName::KeepAwake => on(self.keep_awake),
            SettingName::DownloadLimit => self.speed_limits.download.map_or("unlimited".into(), client::display::speed_limit),
            SettingName::UploadLimit => self.speed_limits.upload.map_or("unlimited".into(), client::display::speed_limit),
            SettingName::SeedRatio => self.seeding.ratio.map_or("never".into(), client::display::ratio),
            SettingName::SeedTime => self.seeding.time.map_or("never".into(), client::display::seed_time),
        }
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

/// A theme setting the Theme section shows: its key, its kind, the sides it has a control for and
/// whether it's one theme's in both halves.
struct Shown {
    key: &'static str,
    kind: &'static Kind,
    sides: Vec<Side>,
    joinable: bool,
}

impl Model {
    fn fld_text(&self, f: Fld) -> &String {
        match f {
            Fld::Port => &self.field,
            Fld::Limit(d) => &self.limit_fields[d as usize],
            Fld::Seeding(at) => &self.seeding_fields[at as usize],
            Fld::Colour(side, key) => &self.colours[&(side, key)].0,
        }
    }

    fn fld_text_mut(&mut self, f: Fld) -> &mut String {
        match f {
            Fld::Port => &mut self.field,
            Fld::Limit(d) => &mut self.limit_fields[d as usize],
            Fld::Seeding(at) => &mut self.seeding_fields[at as usize],
            Fld::Colour(side, key) => &mut self.colours.get_mut(&(side, key)).expect("its field").0,
        }
    }

    fn fld_refused_mut(&mut self, f: Fld) -> &mut Refusal {
        match f {
            Fld::Port => &mut self.port_refused,
            Fld::Limit(d) => &mut self.limits_refused[d as usize],
            Fld::Seeding(at) => &mut self.seeding_refused[at as usize],
            Fld::Colour(side, key) => &mut self.colours.get_mut(&(side, key)).expect("its field").1,
        }
    }

    /// What the field shows for what's stored.
    fn fld_stored(&self, f: Fld) -> String {
        match f {
            Fld::Port => self.port_text(),
            Fld::Limit(d) => self.limit_text(d),
            Fld::Seeding(at) => self.seeding_text(at),
            Fld::Colour(side, key) => match self.value(side, key) {
                Value::Colour(rgb) => rgb.hex(),
                Value::Toggle(_) | Value::Choice(_) => unreachable!(),
            },
        }
    }

    /// Whether the field is shown.
    fn fld_shown(&self, f: Fld) -> bool {
        match f {
            Fld::Colour(side, key) => self.colours.contains_key(&(side, key)),
            Fld::Port | Fld::Limit(_) | Fld::Seeding(_) => true,
        }
    }

    /// Why `text` isn't a value of the field, as the app words it.
    fn fld_problem(f: Fld, text: &str) -> Option<String> {
        if let Fld::Colour(..) = f {
            return Rgb::parse(text).is_none().then(|| crate::model::settings::NOT_A_COLOUR.to_owned());
        }
        let why = match f {
            Fld::Port => setting::port(text).err(),
            Fld::Limit(_) if text.trim().is_empty() => None,
            Fld::Limit(_) => setting::speed(text).err(),
            Fld::Seeding(at) => seeding_value(at, text).err(),
            Fld::Colour(..) => unreachable!(),
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
            Fld::Colour(side, key) => {
                let rgb = Rgb::parse(&text).unwrap();
                self.set(side, key, Value::Colour(rgb));
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

    /// The sides showing now, as the app resolves them.
    fn sides(&self) -> [String; 2] {
        let manifest = manifest();
        [true, false].map(|dark| resolve(self.choice.as_ref(), &manifest, &self.theme_icons(), dark).id.clone())
    }

    /// What following the theme shows: each half's theme's App icon.
    fn theme_icons(&self) -> Icons {
        let [light, dark] = Appearance::BOTH.map(|a| Look::build(self.preset.get(a)).app_icon.to_owned());
        if light == dark { Icons::One { icon: light } } else { Icons::Pair { dark, light } }
    }

    /// The theme settings shown, in order: one theme's in both halves joined while their values
    /// agree and it isn't split, else one control each; two themes', each half's alone.
    fn shown(&self) -> Vec<Shown> {
        let (light, dark) = (&self.preset.light, &self.preset.dark);
        if light.theme.id() != dark.theme.id() {
            return Appearance::BOTH
                .into_iter()
                .flat_map(|a| {
                    self.preset.get(a).theme.settings().iter().map(move |s| Shown {
                        key: s.key,
                        kind: &s.kind,
                        sides: vec![Side::Only(a)],
                        joinable: false,
                    })
                })
                .collect();
        }
        light
            .theme
            .settings()
            .iter()
            .map(|s| {
                let agree = light.settings.get(s.key) == dark.settings.get(s.key);
                let sides = if agree && !self.split.contains(s.key) { vec![Side::Both] } else { Appearance::BOTH.map(Side::Only).to_vec() };
                Shown { key: s.key, kind: &s.kind, sides, joinable: true }
            })
            .collect()
    }

    /// The value `key` holds on `side` (Light's, for both).
    fn value(&self, side: Side, key: &str) -> Value {
        let appearance = match side {
            Side::Both | Side::Only(Appearance::Light) => Appearance::Light,
            Side::Only(Appearance::Dark) => Appearance::Dark,
        };
        self.preset.get(appearance).settings.get(key).expect("its theme has it")
    }

    /// Sets `key` on `side`'s halves; set on one, the field stays split.
    fn set(&mut self, side: Side, key: &'static str, value: Value) {
        // Spelt out here, not asked of the code under test.
        let halves: &[Appearance] = match side {
            Side::Both => &[Appearance::Light, Appearance::Dark],
            Side::Only(Appearance::Light) => &[Appearance::Light],
            Side::Only(Appearance::Dark) => &[Appearance::Dark],
        };
        for appearance in halves {
            let mut look = self.preset.get(*appearance).clone();
            if look.settings.set(key, value) {
                match appearance {
                    Appearance::Light => self.preset.light = look,
                    Appearance::Dark => self.preset.dark = look,
                }
            }
        }
        if side != Side::Both {
            self.split.insert(key);
        }
        self.show_colours();
    }

    /// A field for each colour shown: one still shown keeps its text, a new one shows the colour.
    fn show_colours(&mut self) {
        let mut fields = BTreeMap::new();
        for shown in self.shown().into_iter().filter(|s| matches!(s.kind, Kind::Colour { .. })) {
            for side in shown.sides {
                let Value::Colour(rgb) = self.value(side, shown.key) else { unreachable!() };
                let field = self.colours.remove(&(side, shown.key)).unwrap_or((rgb.hex(), None));
                fields.insert((side, shown.key), field);
            }
        }
        self.colours = fields;
    }
}

struct Sheet;

/// The app, a handle on its fake's disk, the Look Preset it last kept, and what Follow theme
/// last sent to the Dock and the bundle.
struct Sut {
    d: Desk,
    disk: crate::service::Disk,
    kept: Rc<RefCell<Option<LookPreset>>>,
    dock: Rc<RefCell<Icons>>,
}

impl ReferenceStateMachine for Sheet {
    type State = Model;
    type Transition = Step;

    fn init_state() -> BoxedStrategy<Model> {
        let manifest = manifest();
        let mut tiles = vec![("icon-follow".to_owned(), None)];
        tiles.extend(manifest.pairs().iter().enumerate().map(|(i, p)| (format!("icon-pair-{i}"), Some(p.icons()))));
        tiles.extend(manifest.icons().iter().map(|i| (format!("icon-{}", i.id), Some(Icons::One { icon: i.id.clone() }))));
        let ids = manifest.icons().iter().map(|i| i.id.clone()).collect();
        let home = Some(PathBuf::from(crate::service::DOWNLOADS));
        // A fresh install's; one theme in both halves, its values the same, so its fields start
        // joined; or any.
        let same = crate::theme::tests::theme_look().prop_map(|look| LookPreset { light: look.clone(), dark: look });
        // One with a colour setting, so the colour fields' steps are met often (#254).
        let coloured = || {
            crate::theme::tests::theme_look().prop_filter("a colour setting", |look| {
                look.settings.iter().any(|(setting, _)| matches!(setting.kind, Kind::Colour { .. }))
            })
        };
        let coloured = (coloured(), coloured()).prop_map(|(light, dark)| LookPreset { light, dark });
        let preset = prop_oneof![1 => Just(LookPreset::fresh()), 1 => same, 2 => crate::theme::tests::preset(), 3 => coloured];
        (0..4u8, preset)
            .prop_map(move |(n, preset)| Model {
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
                custom_pair: false,
                tiles: tiles.clone(),
                ids: Vec::clone(&ids),
                restarts_fail: n == 0,
                errors: 0,
                focus: None,
                preset,
                dark: true,
                split: BTreeSet::new(),
                colours: BTreeMap::new(),
                focused: None,
                changes: Vec::new(),
                told: Vec::new(),
                rows: TORRENTS.iter().map(|(id, _)| *id).collect(),
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
        // Every control the Theme section shows now, as a step on it.
        let mut theme: Vec<BoxedStrategy<Step>> = Vec::new();
        for shown in m.shown() {
            let key = shown.key;
            for side in shown.sides.clone() {
                theme.push(match shown.kind {
                    Kind::Choice { choices, .. } => prop_oneof![
                        3 => (0..choices.len()).prop_map(move |i| Step::Choose(side, key, i)),
                        2 => any::<bool>().prop_map(move |right| Step::Arrow(side, key, right)),
                    ]
                    .boxed(),
                    Kind::Toggle { .. } => Just(Step::Flip(side, key)).boxed(),
                    Kind::Colour { .. } => prop_oneof![
                        proptest::sample::select(colour_texts()).prop_map(move |text| Step::TypeColour(side, key, text)),
                        Just(Step::SendColour(side, key)),
                    ]
                    .boxed(),
                });
            }
            if shown.joinable {
                theme.push(prop_oneof![Just(Step::Split(key)), Just(Step::Join(key))].boxed());
            }
        }
        let theme = proptest::strategy::Union::new(theme);
        let tiles = proptest::sample::select(m.tiles.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>());
        let ids = proptest::sample::select(m.ids.clone());
        prop_oneof![
            1 => Just(Step::Open),
            1 => any::<bool>().prop_map(|escape| Step::Close { escape }),
            1 => Just(Step::CmdW),
            1 => prop_oneof![Just(Which::Main), Just(Which::Settings)].prop_map(Step::Front),
            1 => Just(Step::Reopen),
            4 => proptest::sample::select(Tab::ALL.to_vec()).prop_map(Step::Tab),
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
            3 => tiles.prop_map(Step::Icon),
            1 => Just(Step::ToggleCustomPair),
            2 => (any::<bool>(), ids).prop_map(|(dark, id)| Step::Side { dark, id }),
            12 => theme,
            1 => any::<bool>().prop_map(Step::Appearance),
            4 => Just(Step::Undo(Which::Settings)),
            2 => Just(Step::Undo(Which::Main)),
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
        let on = |tab| m.open && m.tab == tab;
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
            Step::Icon(_) | Step::ToggleCustomPair => on(Tab::Appearance),
            Step::Side { .. } => on(Tab::Appearance) && m.custom_pair,
            Step::Choose(side, key, _)
            | Step::Arrow(side, key, _)
            | Step::Flip(side, key)
            | Step::TypeColour(side, key, _)
            | Step::SendColour(side, key) => on(Tab::Appearance) && m.shown().iter().any(|s| s.key == *key && s.sides.contains(side)),
            // Split while one control sets both; Join while split, once both agree.
            Step::Split(key) => on(Tab::Appearance) && m.shown().iter().any(|s| s.key == *key && s.joinable && s.sides == [Side::Both]),
            Step::Join(key) => {
                on(Tab::Appearance)
                    && m.shown().iter().any(|s| s.key == *key && s.joinable && s.sides.len() == 2)
                    && m.value(Side::Only(Appearance::Light), key) == m.value(Side::Only(Appearance::Dark), key)
            }
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
        let (before, was) = (m.stored.clone(), m.preset.clone());
        let keeps = match step {
            Step::TypePort(_) | Step::SendPort => Some(Fld::Port),
            Step::TypeLimit(d, _) | Step::SendLimit(d) => Some(Fld::Limit(*d)),
            Step::TypeSeeding(at, _) | Step::SendSeeding(at) => Some(Fld::Seeding(*at)),
            Step::TypeColour(side, key, _) | Step::SendColour(side, key) => Some(Fld::Colour(*side, key)),
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
            if let Some(f) = left {
                m.leave(f);
            }
            m.focused = keeps;
            let changed: Vec<SettingName> = SettingName::ALL.into_iter().filter(|name| !m.stored.same(&before, *name)).collect();
            for name in changed {
                m.push(Change::Service(name, before.clone()));
            }
            if let Some(Fld::Colour(side, key)) = left.filter(|_| m.preset != was) {
                m.push(Change::Theme(was, side, key));
            }
        }
        let (stored, choice, preset) = (m.stored.clone(), m.choice.clone(), m.preset.clone());
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
                }
                let theme = match step {
                    Step::Choose(side, key, _) | Step::Arrow(side, key, _) | Step::Flip(side, key) | Step::SendColour(side, key) => {
                        Some((*side, *key))
                    }
                    _ => None,
                };
                if let Some((side, key)) = theme.filter(|_| m.preset != preset) {
                    m.push(Change::Theme(preset, side, key));
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
        (self.port_refused, self.folder_refused, self.custom_pair, self.limits_refused) = (None, false, false, Default::default());
        self.focused = None;
        (self.split, self.colours) = (BTreeSet::new(), BTreeMap::new());
        self.show_colours();
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
        let Some(change) = self.changes.pop() else { return };
        let failure = client::Failure::Failed(proto::ServiceError::NotRestarted { detail: "no room".into() });
        let refused = matches!(change, Change::Service(name, _) if self.restarts_fail && restarts(name));
        // A setting's undo shows Settings in front, opened if it was closed.
        if !refused && !matches!(change, Change::Removed(_)) {
            if !self.open {
                self.opened();
            }
            self.front = Some(Which::Settings);
        }
        match change {
            Change::Removed(id) => {
                self.rows.insert(id);
                self.told.push(format!("{} is back in the list", name_of(id)));
            }
            Change::Service(..) if refused => {
                let why = client::display::sentence(&format!("{failure:#}")).to_lowercase();
                self.told.push(client::display::couldnt_undo(&why));
            }
            Change::Service(name, was) => {
                self.back(name, &was);
                self.tab = tab_of(name.key()).unwrap();
                self.focus = Some(control_of(name).into());
                self.told.push(client::display::back_to(client::display::setting_short(name.key()), &self.stored.value(name)));
            }
            Change::Icon(was) => {
                self.choice = was;
                self.tab = Tab::Appearance;
                self.focus = Some(self.tiles.iter().find(|(_, c)| *c == self.choice).map_or("icon-custom".into(), |(k, _)| k.clone()));
                self.told.push(client::display::back_to("App icon", &self.icon_name()));
            }
            Change::Theme(preset, side, key) => {
                self.preset = preset;
                // The fields show the values they went back to, whatever was typed in them.
                self.colours.clear();
                self.show_colours();
                self.tab = Tab::Appearance;
                let shown = self.shown().into_iter().find(|s| s.key == key);
                self.focus = shown.map(|s| format!("theme-{key}-{}", s.sides[0].key()));
                let appearance = match side {
                    Side::Both | Side::Only(Appearance::Light) => Appearance::Light,
                    Side::Only(Appearance::Dark) => Appearance::Dark,
                };
                let (setting, value) = self.preset.get(appearance).settings.iter().find(|(s, _)| s.key == key).unwrap();
                let value = match (&setting.kind, value) {
                    (Kind::Choice { choices, .. }, Value::Choice(id)) => choices.iter().find(|c| c.id == id).unwrap().label.to_owned(),
                    (_, Value::Toggle(on)) => if on { "on" } else { "off" }.to_owned(),
                    (_, Value::Colour(rgb)) => rgb.hex(),
                    (_, Value::Choice(id)) => id.to_owned(),
                };
                let named = match side {
                    Side::Both => setting.label.to_owned(),
                    Side::Only(a) => format!("{} {}", a.label(), setting.label),
                };
                self.told.push(client::display::back_to(&named, &value));
            }
        }
    }

    /// What the App icon chosen is called.
    fn icon_name(&self) -> String {
        let manifest = manifest();
        let name = |id: &str| manifest.icon(id).unwrap().name.clone();
        match &self.choice {
            None => "Follow theme".into(),
            Some(icons) if let Some(pair) = manifest.pairs().iter().find(|p| p.icons() == *icons) => pair.name.clone(),
            Some(Icons::One { icon }) => name(icon),
            Some(Icons::Pair { dark, light }) => format!("{} / {}", name(dark), name(light)),
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
    }
    let main = if m.main { Some(Which::Main) } else { None };
    match step {
        Step::Open if !m.open => m.opened(),
        Step::OpenReset(reset) => {
            m.opened();
            m.tab = tab_of(reset[0]).unwrap();
            let first = SETTINGS.into_iter().find(|name| tab_of(name) == Some(m.tab) && reset.contains(name)).unwrap();
            m.focus = Some(match first {
                "default_destination" => "choose-folder".into(),
                "listen_port" => "port-field".into(),
                "upnp" => "upnp-switch".into(),
                "keep_awake" => "keep-awake-switch".into(),
                "download_limit" => "download-limit-field".into(),
                "upload_limit" => "upload-limit-field".into(),
                "seed_ratio" => "seed-ratio-field".into(),
                "seed_time" => "seed-time-field".into(),
                "look_preset" => {
                    let first = &m.shown()[0];
                    format!("theme-{}-{}", first.key, first.sides[0].key())
                }
                _ => m.tiles.iter().find(|(_, c)| *c == m.choice).map_or("icon-custom".into(), |(k, _)| k.clone()),
            });
        }
        Step::Open => m.front = Some(Which::Settings),
        Step::Front(which) => m.front = Some(*which),
        Step::Reopen => (m.main, m.front) = (true, Some(Which::Main)),
        // Escape in a field being edited puts the stored value back, and the sheet stays.
        Step::Close { escape: true } if let Some(f) = m.edited() => {
            *m.fld_text_mut(f) = m.fld_stored(f);
            *m.fld_refused_mut(f) = None;
        }
        Step::Close { .. } => (m.open, m.focused, m.front) = (false, None, main),
        // The window in front closes; the other stays.
        Step::CmdW if m.front == Some(Which::Settings) => (m.open, m.focused, m.front) = (false, None, main),
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
        Step::Icon(key) => m.choice = m.tiles.iter().find(|(k, _)| k == key).and_then(|(_, c)| c.clone()),
        Step::ToggleCustomPair => m.custom_pair = !m.custom_pair,
        Step::Side { dark, id } => {
            let [d, l] = m.sides();
            let (d, l) = if *dark { (id.clone(), l) } else { (d, id.clone()) };
            m.choice = Some(Icons::Pair { dark: d, light: l });
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
            m.focus = Some(format!("theme-{key}-{}", side.key()));
        }
        Step::Flip(side, key) => {
            let Value::Toggle(on) = m.value(*side, key) else { unreachable!() };
            m.set(*side, key, Value::Toggle(!on));
        }
        Step::TypeColour(side, key, text) => *m.fld_text_mut(Fld::Colour(*side, key)) = text.clone(),
        Step::SendColour(side, key) => m.send(Fld::Colour(*side, key)),
        Step::Split(key) => {
            m.split.insert(key);
            m.show_colours();
        }
        Step::Join(key) => {
            m.split.remove(key);
            m.show_colours();
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
        d.main.set_viewport((960.0, 900.0), 2.0);
        d.settings.set_viewport((600.0, 900.0), 2.0);
        d.settle();
        Sut { d, disk, kept, dock }
    }

    fn apply(mut sut: Sut, _: &Model, step: Step) -> Sut {
        let d = &mut sut.d;
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
            Step::Icon(key) => press(d, key),
            Step::ToggleCustomPair => press(d, "custom-pair"),
            Step::Side { dark, id } => press(d, &format!("{}-{id}", if *dark { "dark" } else { "light" })),
            Step::Choose(side, key, i) => press(d, &format!("theme-{key}-{}/#0/segment{i}", side.key())),
            Step::Arrow(side, key, right) => {
                let tab = Tab::ALL.iter().position(|t| *t == Tab::Appearance).unwrap();
                press(d, &format!("settings-tabs/#0/segment{tab}"));
                let r = &mut d.settings;
                let control = format!("/theme-{key}-{}/", side.key());
                // Tab reaches the control, wherever the focus starts.
                let mut tabs = 0;
                while !super::focus_path(r).is_some_and(|p| p.contains(&control)) {
                    assert!(tabs < 60, "Tab never reaches {control}");
                    r.key(KeyEvent { key: Key::Tab, mods: Mods::default() });
                    super::settle(r);
                    tabs += 1;
                }
                r.key(KeyEvent { key: if *right { Key::Right } else { Key::Left }, mods: Mods::default() });
            }
            Step::Flip(side, key) => press(d, &format!("theme-{key}-{}", side.key())),
            Step::TypeColour(side, key, text) => {
                let field = format!("theme-{key}-{}", side.key());
                press(d, &field);
                let r = &mut d.settings;
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendColour(side, key) => {
                press(d, &format!("theme-{key}-{}", side.key()));
                d.settings.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::Split(key) => press(d, &format!("theme-{key}-split")),
            Step::Join(key) => press(d, &format!("theme-{key}-join")),
            Step::Appearance(dark) => {
                d.main.set_system_dark(*dark);
                d.settings.set_system_dark(*dark);
            }
            Step::Undo(which) => {
                match (which, &d.settings.state.sheet) {
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
                d.main.key(shortcuts::keys(shortcuts::Command::Remove)[0].event());
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
        let listed = s
            .bar
            .entries()
            .iter()
            .filter(|e| e.notice.short.contains("didn't start again") && !e.notice.short.starts_with("Couldn't be undone"))
            .count();
        assert_eq!(listed, m.errors, "each failed restart is listed");
        let told: Vec<&str> = s
            .bar
            .entries()
            .iter()
            .map(|e| e.notice.short.as_str())
            .filter(|short| {
                short.contains(" is back to ") || short.ends_with(" is back in the list") || short.starts_with("Couldn't be undone")
            })
            .collect();
        assert_eq!(told, m.told, "each undo tells what it went back to, or what came back, or why it couldn't be undone");
        let listed: BTreeSet<u64> = s.torrents.iter().map(|t| t.id).collect();
        assert_eq!(listed, m.rows, "the torrents in the list, behind the sheet too");
        let newest = m.changes.last().map(|change| match change {
            Change::Service(name, _) => client::display::undo_change(client::display::setting_short(name.key())),
            Change::Icon(_) => client::display::undo_change("App icon"),
            Change::Theme(preset, _, key) => client::display::undo_change(
                preset.light.settings.iter().chain(preset.dark.settings.iter()).find(|(s, _)| s.key == *key).unwrap().0.label,
            ),
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
        for ((side, key), (text, _)) in &m.colours {
            let Value::Colour(rgb) = m.value(*side, key) else { unreachable!() };
            fields.push((format!("theme-{key}-{}", side.key()), text.clone(), rgb.hex()));
        }
        let editing = m.open
            && m.front == Some(Which::Settings)
            && fields.iter().any(|(key, text, stored)| focused.contains(&format!("/{key}/")) && text != stored);
        let title = newest.filter(|_| !editing).unwrap_or_else(|| "Undo".to_owned());
        assert_eq!(crate::changes::menu_title(s), title, "Edit ▸ Undo names what ⌘Z takes back, whichever window is in front");
        let looks = s.looks.borrow();
        assert_eq!(looks.preset(), &m.preset, "the Active Look Preset");
        assert_eq!(sut.kept.borrow().as_ref().unwrap_or(&looks.preset().clone()), &m.preset, "kept as it is");
        let shown = Look::build(m.preset.get(Appearance::of(m.dark)));
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
        if m.tab == Tab::Appearance {
            check_theme_tab(rt, m);
            let selected = checked(&rt.accessibility_json());
            let expected = match m.tiles.iter().find(|(_, c)| *c == m.choice) {
                Some((key, _)) => key.clone(),
                None => "icon-custom".into(),
            };
            assert!(selected.contains(&expected), "{expected} is selected: {selected:?}");
        }
    }
}

/// Each field joined or split as the model says, each control showing its value, Split and Join
/// where they apply, and each colour field's text and refusal.
fn check_theme_tab(rt: &Runtime<SettingsWindow>, m: &Model) {
    for shown in m.shown() {
        let key = shown.key;
        for side in [Side::Both, Side::Only(Appearance::Light), Side::Only(Appearance::Dark)] {
            let control = format!("theme-{key}-{}", side.key());
            assert_eq!(rt.find(&control).is_some(), shown.sides.contains(&side), "{control} drawn");
        }
        let agree = shown.joinable && m.value(Side::Only(Appearance::Light), key) == m.value(Side::Only(Appearance::Dark), key);
        let split = shown.sides.len() == 2;
        assert_eq!(rt.find(&format!("theme-{key}-split")).is_some(), shown.joinable && !split, "{key}'s Split");
        assert_eq!(rt.find(&format!("theme-{key}-join")).is_some(), shown.joinable && split && agree, "{key}'s Join");
        for side in &shown.sides {
            let control = format!("theme-{key}-{}", side.key());
            match (shown.kind, m.value(*side, key)) {
                (Kind::Choice { choices, .. }, Value::Choice(id)) => {
                    let i = choices.iter().position(|c| c.id == id).unwrap();
                    let at = |path: String| rt.elements().into_iter().find(|e| e.path.ends_with(&path) && !e.exiting).map(|e| e.rect[0]);
                    let indicator = at(format!("{control}/#0/indicator"));
                    assert_eq!(indicator, at(format!("{control}/#0/segment{i}")), "{control} shows {id}");
                    // Accessible: a radio group named for the setting, a radio for each choice.
                    let group =
                        rt.elements().into_iter().find(|e| e.path.ends_with(&format!("{control}/#0")) && !e.exiting).expect("its group");
                    assert_eq!(
                        (group.role, group.label.as_deref()),
                        ("radiogroup", Some(shown_label(&m, key))),
                        "{control}'s role and name"
                    );
                    for (j, choice) in choices.iter().enumerate() {
                        let radio = rt
                            .elements()
                            .into_iter()
                            .find(|e| e.path.ends_with(&format!("{control}/#0/segment{j}")) && !e.exiting)
                            .expect("a radio");
                        assert_eq!(
                            (radio.role, radio.label.as_deref(), radio.focusable),
                            ("radio", Some(choice.label), true),
                            "{control}'s {j}th choice"
                        );
                    }
                }
                (Kind::Colour { .. }, _) => {
                    let (typed, _) = &m.colours[&(*side, key)];
                    let field = rt.state.sheet.as_ref().and_then(|sheet| sheet.colour(*side, key)).expect("its field");
                    assert_eq!(&field.text.text, typed, "{control}'s text");
                }
                (Kind::Toggle { .. } | Kind::Choice { .. }, _) => {}
            }
        }
        let refused: Vec<_> = shown
            .sides
            .iter()
            .filter(|side| m.colours.get(&(**side, key)).is_some_and(|(text, why)| why.as_ref().is_some_and(|(t, _)| t == text)))
            .collect();
        assert_eq!(rt.find(&format!("theme-{key}-refused")).is_some(), !refused.is_empty(), "{key}'s refusal shows until its text changes");
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
    let rt = &mut d.settings;
    scroll_to(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
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
        let (Some((_, y)), Some((bx, by))) = (rt.find(key), rt.find("sheet-body")) else { break };
        rt.wheel(bx, by, -(y - by).clamp(-200.0, 200.0));
        rt.frame(FRAME);
    }
    assert!(on_screen(rt, key), "{key:?} can't be scrolled to");
}

/// The keys of the accessibility tree's checked nodes: the selected tiles.
fn checked(node: &serde_json::Value) -> Vec<String> {
    let here = node["states"].as_array().is_some_and(|s| s.iter().any(|s| s == "checked"));
    let key = node["key"].as_str().filter(|_| here).map(str::to_owned);
    let children = node["children"].as_array().into_iter().flatten().flat_map(checked);
    key.into_iter().chain(children).collect()
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
    assert!(w.sheet.as_ref().unwrap().editing(&w.looks()), "the port field is being edited");
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
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some("Port is back to automatic (now 51413)".into()));
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
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some("Port is back to 40001".into()));
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
    let told = client::display::back_to(client::display::setting_short("upnp"), "on");
    assert_eq!(d.main.state.bar.newest().map(|n| n.short.clone()), Some(told), "the app's own change undone");
    let next = client::display::undo_change(client::display::setting_short("keep_awake"));
    assert_eq!(crate::changes::menu_title(&d.main.state), next, "the other client's, newer to the main window than the press, stays");
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
    // A fresh install's Tone is split: Dark for the Light Appearance.
    let dark = Look::build(&crate::theme::preset::LookPreset::fresh().dark).ui.tokens().c.clone();
    assert_ne!(fill(&d.main, "app"), Some(dark.page), "light to start with");
    // Frames only, as time passes, no springs settled by hand, which would build the main window
    // again anyway.
    let (x, y) = d.settings.find("theme-tone-light/#0/segment1").expect("Tone for Light");
    d.settings.pointer_down(x, y);
    d.settings.pointer_up(x, y);
    d.wait(1.0);
    assert_eq!(fill(&d.main, "app"), Some(dark.page), "the main window, dark");
    assert_eq!(fill(&d.settings, "settings-sheet"), Some(dark.layer_01), "and Settings");
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
    match m.shown().into_iter().find(|s| s.key == key).map(|s| s.kind) {
        Some(Kind::Choice { .. }) => {
            m.preset.light.settings.iter().chain(m.preset.dark.settings.iter()).find(|(s, _)| s.key == key).unwrap().0.label
        }
        _ => unreachable!(),
    }
}
