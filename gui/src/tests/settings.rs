//! Generated sequences on the Settings sheet against `MemoryService`: a person opening it with ⌘,
//! and closing it, switching tabs, typing ports (good ones, a taken one, and text that isn't a
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
//!   refusal until its text changes;
//! - ⌘Z takes back the newest change, whoever made it: its setting is as it was, its tab shows
//!   with its control focused, and the status bar tells what it went back to ("Port is back to
//!   40000"), or "Couldn't be undone: <why>"; another client's change shows in the open sheet at
//!   once, a field not typed over following it.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::num::NonZeroU16;
use std::path::PathBuf;

use client::setting;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{Direction, ListenPort, SeedingLimit, SeedingLimits, SettingName, SpeedLimits, Unavailable};
use rune::{Key, KeyEvent, Mods, Runtime, Shell};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::{FRAME, app, on_screen, settle};
use crate::model::Phantom;
use crate::model::settings::{AppIcon, IconChange, Side, Switch, Tab};
use crate::platform::app_icon::{Icons, Manifest, manifest_dir, resolve};
use crate::service::{DiskChange, MemoryService};
use crate::theme::Look;
use crate::theme::preset::{Appearance, LookPreset, Looks};
use crate::theme::setting::{Kind, Rgb, Value};
use crate::views::settings::{SETTINGS, tab_of};

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

/// What's typed into the port field.
fn port_texts() -> Vec<String> {
    vec!["40000".into(), "40001".into(), "40002".into(), "70000".into(), "0".into(), "port".into(), String::new()]
}

#[derive(Debug, Clone)]
enum Step {
    /// ⌘,
    Open,
    /// Escape, or the close button.
    Close {
        escape: bool,
    },
    /// ⌘W, which File ▸ Close Window is the key of: closes the sheet when it's open, else the window.
    CmdW,
    Tab(Tab),
    TypePort(String),
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
    /// ⌘Z with the tabs focused (a click on the tab shown), as after switching tabs; with the sheet
    /// closed, the main window's, which takes no settings change.
    Undo,
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

/// A change ⌘Z takes back, with what its setting was before.
#[derive(Debug, Clone)]
enum Change {
    Service(SettingName, Stored),
    Icon(Option<Icons>),
    Theme(LookPreset, Side, &'static str),
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

#[derive(Debug, Clone)]
struct Model {
    stored: Stored,
    open: bool,
    /// The step asked the shell to close the window.
    closes_window: bool,
    tab: Tab,
    field: String,
    /// The field's text a refusal of the port is about; a folder refused.
    port_refused: Option<String>,
    folder_refused: bool,
    /// The folders that can't be reached now, each with why, as [`reasons`] has it.
    gone: BTreeMap<PathBuf, usize>,
    /// Each limit's field, and the text a refusal of it is about.
    limit_fields: [String; 2],
    limits_refused: [Option<String>; 2],
    /// Each seeding limit's field, and the text a refusal of it is about.
    seeding_fields: [String; 2],
    seeding_refused: [Option<String>; 2],
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
    colours: BTreeMap<(Side, &'static str), (String, Option<String>)>,
    /// The changes ⌘Z takes back, newest last, and what each undo told, in order.
    changes: Vec<Change>,
    told: Vec<String>,
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
    rt: Runtime<Phantom>,
    disk: crate::service::Disk,
    kept: Rc<RefCell<Option<LookPreset>>>,
    dock: Rc<RefCell<Icons>>,
    /// What the last step asked of the shell, taken as it ended.
    asked: Option<Shell>,
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
        let preset = prop_oneof![1 => Just(LookPreset::fresh()), 1 => same, 2 => crate::theme::tests::preset()];
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
                closes_window: false,
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
                changes: Vec::new(),
                told: Vec::new(),
            })
            .boxed()
    }

    fn transitions(m: &Model) -> BoxedStrategy<Step> {
        if !m.open {
            let reset = proptest::sample::subsequence(SETTINGS.to_vec(), 1..=SETTINGS.len()).prop_shuffle();
            return prop_oneof![
                2 => Just(Step::Open),
                1 => Just(Step::CmdW),
                1 => reset.prop_map(Step::OpenReset),
                2 => (0..3usize).prop_map(Step::FolderGone),
                1 => Just(Step::FolderBack),
                1 => any::<bool>().prop_map(Step::Appearance),
                1 => proptest::sample::select(others()).prop_map(Step::Other),
                1 => Just(Step::OtherUndo),
                1 => Just(Step::Undo),
            ]
            .boxed();
        }
        // Every control the Theme section shows now, as a step on it.
        let mut theme: Vec<BoxedStrategy<Step>> = Vec::new();
        for shown in m.shown() {
            let key = shown.key;
            for side in shown.sides.clone() {
                theme.push(match shown.kind {
                    Kind::Choice { choices, .. } => (0..choices.len()).prop_map(move |i| Step::Choose(side, key, i)).boxed(),
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
            4 => proptest::sample::select(Tab::ALL.to_vec()).prop_map(Step::Tab),
            4 => proptest::sample::select(port_texts()).prop_map(Step::TypePort),
            3 => Just(Step::SendPort),
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
            6 => Just(Step::Undo),
            2 => proptest::sample::select(others()).prop_map(Step::Other),
            1 => Just(Step::OtherUndo),
            1 => any::<bool>().prop_map(|fail| Step::Restarts { fail }),
        ]
        .boxed()
    }

    /// Each control is only there on its own tab, and some only sometimes.
    fn preconditions(m: &Model, step: &Step) -> bool {
        let on = |tab| m.open && m.tab == tab;
        match step {
            Step::Open | Step::CmdW => true,
            Step::OpenReset(_) => !m.open,
            Step::Close { .. } | Step::Tab(_) => m.open,
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
            Step::Choose(side, key, _) | Step::Flip(side, key) | Step::TypeColour(side, key, _) | Step::SendColour(side, key) => {
                on(Tab::Appearance) && m.shown().iter().any(|s| s.key == *key && s.sides.contains(side))
            }
            // Split while one control sets both; Join while split, once both agree.
            Step::Split(key) => on(Tab::Appearance) && m.shown().iter().any(|s| s.key == *key && s.joinable && s.sides == [Side::Both]),
            Step::Join(key) => {
                on(Tab::Appearance)
                    && m.shown().iter().any(|s| s.key == *key && s.joinable && s.sides.len() == 2)
                    && m.value(Side::Only(Appearance::Light), key) == m.value(Side::Only(Appearance::Dark), key)
            }
            Step::Appearance(_) | Step::Other(_) | Step::OtherUndo | Step::Restarts { .. } | Step::Undo => true,
        }
    }

    /// The step, and each change that took effect a change ⌘Z takes back.
    fn apply(m: Model, step: &Step) -> Model {
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
            Step::Undo | Step::OtherUndo => {}
            _ => {
                let changed: Vec<SettingName> = SettingName::ALL.into_iter().filter(|name| !m.stored.same(&stored, *name)).collect();
                for name in changed {
                    m.push(Change::Service(name, stored.clone()));
                }
                if m.choice != choice {
                    m.push(Change::Icon(choice));
                }
                let theme = match step {
                    Step::Choose(side, key, _) | Step::Flip(side, key) | Step::SendColour(side, key) => Some((*side, *key)),
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
    /// A change ⌘Z takes back, the newest; past the last 20, the oldest goes.
    fn push(&mut self, change: Change) {
        self.changes.push(change);
        if self.changes.len() > 20 {
            self.changes.remove(0);
        }
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

    /// ⌘Z: the newest change, on its tab, its control focused, and told.
    fn undo(&mut self) {
        let Some(change) = self.changes.pop() else { return };
        let failure = client::Failure::Failed(proto::ServiceError::NotRestarted { detail: "no room".into() });
        match change {
            Change::Service(name, _) if self.restarts_fail && restarts(name) => {
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
                self.show_colours();
                self.tab = Tab::Appearance;
                let shown = self.shown().into_iter().find(|s| s.key == key);
                self.focus = shown.filter(|s| !matches!(s.kind, Kind::Choice { .. })).map(|s| format!("theme-{key}-{}", s.sides[0].key()));
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
    m.closes_window = false;
    let opened = |m: &mut Model| {
        m.open = true;
        m.tab = Tab::General;
        m.field = m.port_text();
        m.limit_fields = Direction::BOTH.map(|d| m.limit_text(d));
        m.seeding_fields = SEEDING.map(|at| m.seeding_text(at));
        m.seeding_refused = Default::default();
        (m.port_refused, m.folder_refused, m.custom_pair, m.limits_refused) = (None, false, false, Default::default());
        (m.split, m.colours) = (BTreeSet::new(), BTreeMap::new());
        m.show_colours();
    };
    match step {
        Step::Open if !m.open => opened(&mut m),
        Step::OpenReset(reset) => {
            opened(&mut m);
            m.tab = tab_of(reset[0]).unwrap();
            let first = SETTINGS.into_iter().find(|name| tab_of(name) == Some(m.tab) && reset.contains(name)).unwrap();
            m.focus = (first != "look_preset" || !matches!(m.shown()[0].kind, Kind::Choice { .. })).then(|| match first {
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
            // A choice can't be first in the Tab order (`Ui::choice`): the sheet focuses as it
            // would without a reset.
        }
        Step::Open => {}
        Step::Close { .. } => m.open = false,
        // Settings first, as Escape; only with it closed, the window.
        Step::CmdW if m.open => m.open = false,
        Step::CmdW => m.closes_window = true,
        Step::Tab(tab) => m.tab = *tab,
        Step::TypePort(text) => m.field = text.clone(),
        Step::TypeLimit(d, text) => m.limit_fields[*d as usize] = text.clone(),
        Step::TypeSeeding(at, text) => m.seeding_fields[*at as usize] = text.clone(),
        // A seeding limit restarts nothing, so it never fails as one.
        Step::SendSeeding(at) => {
            let i = *at as usize;
            match seeding_value(*at, &m.seeding_fields[i]) {
                Ok(value) => {
                    match at {
                        SeedingLimit::Ratio => m.stored.seeding.ratio = value.map(|h| proto::Ratio::new(h).unwrap()),
                        SeedingLimit::Time => m.stored.seeding.time = value.map(|m| proto::SeedTime::new(m).unwrap()),
                    }
                    m.seeding_fields[i] = m.seeding_text(*at);
                    m.seeding_refused[i] = None;
                }
                Err(_) => m.seeding_refused[i] = Some(m.seeding_fields[i].clone()),
            }
        }
        // Empty is unlimited; a limit applies without a restart, so it never fails as one.
        Step::SendLimit(d) => {
            let i = *d as usize;
            let text = &m.limit_fields[i];
            let limit = if text.trim().is_empty() { Ok(None) } else { setting::speed(text).map(Some) };
            match limit {
                Ok(limit) => {
                    m.stored.speed_limits.set(*d, limit);
                    m.limit_fields[i] = m.limit_text(*d);
                    m.limits_refused[i] = None;
                }
                Err(_) => m.limits_refused[i] = Some(text.clone()),
            }
        }
        // A new port or UPnP that fails stores nothing: it's refused under its row and listed.
        Step::SendPort => match setting::port(&m.field) {
            // The port it has: nothing to send.
            Ok(port) if m.stored.port == ListenPort::Pinned(port) => {
                m.field = m.port_text();
                m.port_refused = None;
            }
            Ok(_) if m.restarts_fail => {
                m.errors += 1;
                m.port_refused = Some(m.field.clone());
            }
            Ok(port) => {
                m.stored.port = ListenPort::Pinned(port);
                m.field = m.port_text();
                m.port_refused = None;
            }
            Err(_) => m.port_refused = Some(m.field.clone()),
        },
        Step::PickPort if m.restarts_fail => {
            m.errors += 1;
            m.port_refused = Some(m.field.clone());
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
        Step::Flip(side, key) => {
            let Value::Toggle(on) = m.value(*side, key) else { unreachable!() };
            m.set(*side, key, Value::Toggle(!on));
        }
        Step::TypeColour(side, key, text) => {
            m.colours.insert((*side, key), (text.clone(), m.colours[&(*side, *key)].1.clone()));
        }
        Step::SendColour(side, key) => {
            let text = m.colours[&(*side, *key)].0.clone();
            match Rgb::parse(&text) {
                Some(rgb) => {
                    m.colours.insert((*side, key), (rgb.hex(), None));
                    m.set(*side, key, Value::Colour(rgb));
                }
                None => {
                    m.colours.insert((*side, key), (text.clone(), Some(text)));
                }
            }
        }
        Step::Split(key) => {
            m.split.insert(key);
            m.show_colours();
        }
        Step::Join(key) => {
            m.split.remove(key);
            m.show_colours();
        }
        Step::Appearance(dark) => m.dark = *dark,
        Step::Undo if m.open => m.undo(),
        Step::Undo => {}
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
        let service =
            MemoryService::new().unreachable(FOLDERS[GONE], Unavailable::DriveNotConnected { name: "MyDisk".into() }).port_taken(TAKEN);
        let service = if m.restarts_fail { service.failing_restarts() } else { service };
        let disk = service.disk();
        let mut rt = app(service);
        let kept = Rc::new(RefCell::new(None));
        let keep = kept.clone();
        rt.state.looks = Looks::new(m.preset.clone(), move |preset: &LookPreset| *keep.borrow_mut() = Some(preset.clone()));
        // What the Dock and the bundle were last told to follow, starting from what they're given.
        let dock = Rc::new(RefCell::new(m.theme_icons()));
        let sent = dock.clone();
        rt.state.app_icon = AppIcon::new(Some(manifest()), None, m.theme_icons(), move |change| {
            if let IconChange::Theme(icons) = change {
                *sent.borrow_mut() = icons.clone();
            }
        });
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(&mut rt);
        Sut { rt, disk, kept, dock, asked: None }
    }

    fn apply(mut sut: Sut, _: &Model, step: Step) -> Sut {
        let r = &mut sut.rt;
        match &step {
            Step::Open => {
                r.update(|_, tx| tx.blur());
                r.key(KeyEvent { key: Key::Command(','), mods: Mods { cmd: true, ..Default::default() } });
                // Presses land once the sheet has come in, as for a person.
                super::wait(r, 0.5);
            }
            Step::Close { escape: true } => r.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
            Step::Close { escape: false } => press(r, "header/close"),
            Step::CmdW => r.key(KeyEvent { key: Key::Command('w'), mods: Mods { cmd: true, ..Default::default() } }),
            Step::Tab(tab) => press(r, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| t == tab).unwrap())),
            Step::TypePort(text) => {
                press(r, "port-field");
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendPort => {
                press(r, "port-field");
                r.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::TypeLimit(d, text) => {
                press(r, limit_field(*d));
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::TypeSeeding(at, text) => {
                press(r, seeding_field(*at));
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendSeeding(at) => {
                press(r, seeding_field(*at));
                r.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::SendLimit(d) => {
                press(r, limit_field(*d));
                r.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::PickPort => press(r, "pick-port"),
            Step::Toggle { switch, twice: false } => press(r, switch.key()),
            // Both presses before a frame: the second meets the first on its way.
            Step::Toggle { switch, twice: true } => {
                scroll_to(r, switch.key());
                let (x, y) = r.find(switch.key()).expect("the switch");
                for _ in 0..2 {
                    r.pointer_down(x, y);
                    r.pointer_up(x, y);
                }
            }
            Step::OpenReset(reset) => {
                let reset: Vec<String> = reset.iter().map(|name| name.to_string()).collect();
                r.update(move |s, _| crate::model::settings::open_reset(s, &reset));
                super::wait(r, 0.5);
            }
            Step::ChooseFolder(folder) => {
                press(r, "choose-folder");
                r.resolve_file_dialog(folder.map(|i| FOLDERS[i].into())).expect("the picker asks");
            }
            Step::ClearFolder => press(r, "clear-folder"),
            Step::FolderGone(i) => {
                sut.disk.change(DiskChange::DefaultGone { reason: reasons()[*i].clone() });
            }
            Step::FolderBack => {
                sut.disk.change(DiskChange::DefaultBack);
            }
            Step::Icon(key) => press(r, key),
            Step::ToggleCustomPair => press(r, "custom-pair"),
            Step::Side { dark, id } => press(r, &format!("{}-{id}", if *dark { "dark" } else { "light" })),
            Step::Choose(side, key, i) => press(r, &format!("theme-{key}-{}/#0/segment{i}", side.key())),
            Step::Flip(side, key) => press(r, &format!("theme-{key}-{}", side.key())),
            Step::TypeColour(side, key, text) => {
                let field = format!("theme-{key}-{}", side.key());
                press(r, &field);
                r.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
                if text.is_empty() {
                    r.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
                } else {
                    r.paste(text);
                }
            }
            Step::SendColour(side, key) => {
                press(r, &format!("theme-{key}-{}", side.key()));
                r.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
            }
            Step::Split(key) => press(r, &format!("theme-{key}-split")),
            Step::Join(key) => press(r, &format!("theme-{key}-join")),
            Step::Appearance(dark) => r.set_system_dark(*dark),
            Step::Undo => {
                if let Some(sheet) = &r.state.settings {
                    let tab = Tab::ALL.iter().position(|t| *t == sheet.tab).unwrap();
                    press(r, &format!("settings-tabs/#0/segment{tab}"));
                }
                r.key(KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } });
                // Settled twice, as a press is (`press`, then below): a look an undo rebuilt moves
                // the layout over a second frame, under the next step's click.
                settle(r);
            }
            Step::Other(other) => sut.disk.change(DiskChange::SetByAnother(other.setting())),
            Step::OtherUndo => sut.disk.change(DiskChange::UndoneByAnother),
            Step::Restarts { fail } => sut.disk.change(DiskChange::RestartsFailing(*fail)),
        }
        settle(r);
        sut.asked = sut.rt.shell_request.take();
        sut
    }

    fn check_invariants(sut: &Sut, m: &Model) {
        let rt = &sut.rt;
        let s = &rt.state;
        assert_eq!(s.settings.is_some(), m.open, "the sheet is open");
        assert_eq!(rt.find("settings-sheet").is_some(), m.open, "and drawn");
        assert_eq!(sut.asked, m.closes_window.then_some(Shell::Close), "the window closes exactly when ⌘W meets no sheet");
        assert_eq!(s.app_icon.choice, m.choice, "the App icon");
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
            .filter(|short| short.contains(" is back to ") || short.starts_with("Couldn't be undone"))
            .collect();
        assert_eq!(told, m.told, "each undo tells what it went back to, or why it couldn't be undone");
        let newest = m.changes.last().filter(|_| m.open).map(|change| match change {
            Change::Service(name, _) => client::display::setting_short(name.key()).to_owned(),
            Change::Icon(_) => "App icon".to_owned(),
            Change::Theme(preset, _, key) => {
                preset.light.settings.iter().chain(preset.dark.settings.iter()).find(|(s, _)| s.key == *key).unwrap().0.label.to_owned()
            }
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
        let editing = m.open && fields.iter().any(|(key, text, stored)| focused.contains(&format!("/{key}/")) && text != stored);
        let title = newest.filter(|_| !editing).map_or("Undo".to_owned(), |short| client::display::undo_change(&short));
        assert_eq!(crate::changes::menu_title(s), title, "Edit ▸ Undo names what ⌘Z takes back in Settings");
        assert_eq!(s.looks.preset(), &m.preset, "the Active Look Preset");
        assert_eq!(sut.kept.borrow().as_ref().unwrap_or(&s.looks.preset().clone()), &m.preset, "kept as it is");
        let shown = Look::build(m.preset.get(Appearance::of(m.dark)));
        let page = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("app")).map(|e| e.fill);
        assert_eq!(page, Some(shown.ui.tokens().c.page), "the window in the look of the Appearance");
        assert_eq!(s.app_icon.theme, m.theme_icons(), "Follow theme shows each look's App icon");
        assert_eq!(*sut.dock.borrow(), m.theme_icons(), "and the Dock and the bundle are told");
        if let Some(key) = &m.focus {
            let focused = rt.elements().into_iter().find(|e| e.focused).map(|e| e.path);
            assert!(focused.as_deref().is_some_and(|p| p.contains(key.as_str())), "{key} is focused, not {focused:?}");
        }
        let Some(sheet) = &s.settings else { return };
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
            let refused = m.port_refused.as_ref().is_some_and(|text| *text == m.field);
            assert_eq!(rt.find("listen_port-refused").is_some(), refused, "the port's refusal shows until its text changes");
            if let Some(why) = setting::port(&m.field).err().filter(|_| refused) {
                assert_eq!(text(rt, "listen_port-refused"), client::display::value_problem(why));
            }
            for d in Direction::BOTH {
                let field = &m.limit_fields[d as usize];
                let refused = m.limits_refused[d as usize].as_ref().is_some_and(|text| text == field);
                let key = format!("{}-refused", crate::model::settings::limit_name(d));
                assert_eq!(rt.find(&key).is_some(), refused, "the {d:?} limit's refusal shows until its text changes");
                if let Some(why) = setting::speed(field).err().filter(|_| refused) {
                    assert_eq!(text(rt, &key), client::display::value_problem(why));
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
                let refused = m.seeding_refused[at as usize].as_ref().is_some_and(|text| text == field);
                let key = format!("{}-refused", crate::model::settings::seeding_name(at));
                assert_eq!(rt.find(&key).is_some(), refused, "the {at:?} seeding limit's refusal shows until its text changes");
                if let Some(why) = seeding_value(at, field).err().filter(|_| refused) {
                    assert_eq!(text(rt, &key), client::display::value_problem(why));
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
fn check_theme_tab(rt: &Runtime<Phantom>, m: &Model) {
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
                }
                (Kind::Colour { .. }, _) => {
                    let (typed, _) = &m.colours[&(*side, key)];
                    let field = rt.state.settings.as_ref().and_then(|sheet| sheet.colour(*side, key)).expect("its field");
                    assert_eq!(&field.text.text, typed, "{control}'s text");
                }
                (Kind::Toggle { .. } | Kind::Choice { .. }, _) => {}
            }
        }
        let refused: Vec<_> =
            shown.sides.iter().filter(|side| m.colours.get(&(**side, key)).is_some_and(|(text, why)| why.as_ref() == Some(text))).collect();
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

/// Presses the element at `key`, scrolling the sheet to it first.
fn press(rt: &mut Runtime<Phantom>, key: &str) {
    scroll_to(rt, key);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Scrolls the sheet to the element at `key`.
fn scroll_to(rt: &mut Runtime<Phantom>, key: &str) {
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

/// The text drawn in the element keyed `key`.
fn text(rt: &Runtime<Phantom>, key: &str) -> String {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text).unwrap_or_default()
}

proptest! {
    #![proptest_config(Config { cases: 16, ..Config::default() })]

    /// Every text field the sheet draws, found in what's drawn rather than listed, is in
    /// `FIELDS`, on its tab, and as wide as its row gives it, whatever's typed in it: empty,
    /// short, or longer than it shows (#140). Every field listed is drawn.
    #[test]
    fn every_field_fills_the_width_its_row_gives(text in prop_oneof![Just(String::new()), "[0-9a-z ]{1,4}", "[0-9a-z ]{40,60}"]) {
        let mut rt = app(MemoryService::new());
        rt.set_viewport((960.0, 900.0), 2.0);
        press_shortcut_open(&mut rt);
        let mut found = Vec::new();
        for tab in Tab::ALL {
            press(&mut rt, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap()));
            for key in drawn_fields(&rt) {
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
            press(&mut rt, &format!("settings-tabs/#0/segment{}", Tab::ALL.iter().position(|t| *t == tab).unwrap()));
            press(&mut rt, key);
            rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
            if text.is_empty() {
                rt.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
            } else {
                rt.paste(&text);
            }
            settle(&mut rt);
            let drawn = super::generated::rect_of(&rt, key).expect("the field")[2];
            prop_assert!((drawn - width).abs() < 0.5, "{} is {} wide, its row gives {}", key, drawn, width);
        }
    }
}

/// The keys of the text fields the sheet draws now: each field is a textbox with no key of its
/// own (its input inside is keyed `input`) under the element keyed for it.
fn drawn_fields(rt: &Runtime<Phantom>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.role == "textbox" && e.key.is_none() && e.path.contains("settings-sheet"))
        .filter_map(|e| e.path.rsplit('/').nth(1).map(str::to_owned))
        .collect()
}

/// Opens the sheet with ⌘,, as a person does.
fn press_shortcut_open(rt: &mut Runtime<Phantom>) {
    rt.update(|_, tx| tx.blur());
    rt.key(KeyEvent { key: Key::Command(','), mods: Mods { cmd: true, ..Default::default() } });
    super::wait(rt, 0.5);
    settle(rt);
}

/// ⌘W closes the sheet, and the focus returns to where it was in the main window, as with Escape.
#[test]
fn cmd_w_closes_the_sheet_and_returns_focus() {
    let after = |close: KeyEvent| {
        let mut rt = app(MemoryService::new());
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(&mut rt);
        press(&mut rt, "magnet-field");
        let before = super::focus_path(&rt);
        assert!(before.is_some(), "the field is focused");
        rt.key(KeyEvent { key: Key::Command(','), mods: Mods { cmd: true, ..Default::default() } });
        super::wait(&mut rt, 0.5);
        rt.key(close);
        super::wait(&mut rt, 0.5);
        settle(&mut rt);
        assert!(rt.state.settings.is_none() && rt.shell_request.is_none(), "only the sheet closed");
        assert_eq!(super::focus_path(&rt), before, "the focus is where it was");
    };
    after(KeyEvent { key: Key::Command('w'), mods: Mods { cmd: true, ..Default::default() } });
    after(KeyEvent { key: Key::Escape, mods: Mods::default() });
}

/// While a field is being edited, ⌘Z is its text's own undo, and the settings' only once it's let
/// go (#221). An example: what a field's own undo takes back is Rune's, which the generated
/// sequences don't model.
#[test]
fn undo_in_a_field_being_edited_is_the_fields_own() {
    let mut rt = app(MemoryService::new());
    rt.set_viewport((960.0, 900.0), 2.0);
    press_shortcut_open(&mut rt);
    press(&mut rt, "settings-tabs/#0/segment1");
    let z = KeyEvent { key: Key::Command('z'), mods: Mods { cmd: true, ..Default::default() } };
    let select_all = KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } };
    press(&mut rt, "port-field");
    rt.key(select_all.clone());
    rt.paste("40001");
    rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
    settle(&mut rt);
    let port = |rt: &Runtime<Phantom>| rt.state.settings.as_ref().unwrap().service.as_ref().unwrap().listen_port.port().get();
    assert_eq!(port(&rt), 40_001);
    press(&mut rt, "port-field");
    rt.key(select_all);
    rt.paste("123");
    rt.key(z.clone());
    settle(&mut rt);
    assert_ne!(text_of_field(&rt), "123", "the field's own undo took the typing back");
    assert_eq!(port(&rt), 40_001, "and no setting");
    // Let go (a tab clicked), ⌘Z is the settings'.
    press(&mut rt, "settings-tabs/#0/segment0");
    rt.key(z);
    settle(&mut rt);
    assert_eq!(port(&rt), 51_413, "the port back");
    assert_eq!(rt.state.settings.as_ref().unwrap().tab, Tab::Network, "on its tab");
    assert_eq!(rt.state.bar.newest().map(|n| n.short.clone()), Some("Port is back to automatic (now 51413)".into()));
}

fn text_of_field(rt: &Runtime<Phantom>) -> String {
    rt.state.settings.as_ref().unwrap().port.text.clone()
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
