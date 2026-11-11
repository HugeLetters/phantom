//! Generated sequences on the Settings sheet against `MemoryService`: a person opening it with ⌘,
//! and closing it, switching tabs, typing ports (good ones, a taken one, and text that isn't a
//! port) and sending them, letting the port be picked, flipping UPnP, choosing a default folder
//! (or one on a drive that isn't connected, or cancelling) and clearing it, and picking App icons
//! and custom pairs. The reference model knows what's stored, what the port field holds and which
//! refusal shows. After every step:
//! - the sheet shows what's stored, and a refused change stored nothing;
//! - a refusal's caption shows exactly until the port field's text changes;
//! - the tab shows its own settings and no others, and every setting is on exactly one tab;
//! - a UPnP toggled twice before the first answer ends where two toggles put it;
//! - with restarts failing, a new port or UPnP stores nothing and goes to the error list;
//! - Open settings from the settings-reset notice focuses the first reset setting's control on
//!   its tab.
//!
//! 32 cases per `cargo test`; `PROPTEST_CASES` runs more.

use std::num::NonZeroU16;
use std::path::PathBuf;

use client::setting;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use proto::{ListenPort, Unavailable};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::{FRAME, app, on_screen, settle};
use crate::model::Phantom;
use crate::model::settings::{AppIcon, Tab};
use crate::platform::app_icon::{Icons, Manifest, Theme, manifest_dir, resolve};
use crate::service::MemoryService;
use crate::views::settings::{SETTINGS, tab_of};

/// Folders the picker can give: two that can be the default, and one on a drive that's gone.
const FOLDERS: [&str; 3] = ["/srv/films", "/srv/music", "/Volumes/MyDisk/torrents"];
const GONE: usize = 2;
/// A port another program has.
const TAKEN: u16 = 40_000;

fn manifest() -> Manifest {
    Manifest::load(&manifest_dir()).expect("the shipped icons")
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
    Tab(Tab),
    TypePort(String),
    /// Return in the port field.
    SendPort,
    PickPort,
    /// Once, or twice before the first is answered.
    ToggleUpnp {
        twice: bool,
    },
    /// The settings-reset notice's Open settings, for these settings reset.
    OpenReset(Vec<&'static str>),
    /// Choose…, then a folder by index into [`FOLDERS`], or cancel.
    ChooseFolder(Option<usize>),
    ClearFolder,
    /// A tile of the picker, by key.
    Icon(String),
    ToggleCustomPair,
    /// A side of the custom pair: dark or light, an icon's id.
    Side {
        dark: bool,
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Stored {
    folder: Option<PathBuf>,
    port: ListenPort,
    upnp: bool,
}

#[derive(Debug, Clone)]
struct Model {
    stored: Stored,
    open: bool,
    tab: Tab,
    field: String,
    /// The field's text a refusal of the port is about; a folder refused.
    port_refused: Option<String>,
    folder_refused: bool,
    choice: Option<Icons>,
    custom_pair: bool,
    /// Every tile key with what it chooses, and every icon id.
    tiles: Vec<(String, Option<Icons>)>,
    ids: Vec<String>,
    /// The fake fails a new port or UPnP, as when the torrents don't start again with them.
    restarts_fail: bool,
    /// Entries the error list got.
    errors: usize,
    /// The control focused, by key, right after Open settings.
    focus: Option<String>,
}

impl Model {
    fn port_text(&self) -> String {
        self.stored.port.port().to_string()
    }

    /// The sides showing now, as the app resolves them.
    fn sides(&self) -> [String; 2] {
        let manifest = manifest();
        [true, false].map(|dark| resolve(self.choice.as_ref(), &manifest, Theme::default(), dark).id.clone())
    }
}

struct Sheet;

impl ReferenceStateMachine for Sheet {
    type State = Model;
    type Transition = Step;

    fn init_state() -> BoxedStrategy<Model> {
        let manifest = manifest();
        let mut tiles = vec![("icon-follow".to_owned(), None)];
        tiles.extend(manifest.pairs().iter().enumerate().map(|(i, p)| (format!("icon-pair-{i}"), Some(p.icons()))));
        tiles.extend(manifest.icons().iter().map(|i| (format!("icon-{}", i.id), Some(Icons::One { icon: i.id.clone() }))));
        let ids = manifest.icons().iter().map(|i| i.id.clone()).collect();
        let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"));
        (0..4u8)
            .prop_map(move |n| Model {
                stored: Stored { folder: home.clone(), port: ListenPort::Picked(NonZeroU16::new(51413).unwrap()), upnp: true },
                open: false,
                tab: Tab::General,
                field: String::new(),
                port_refused: None,
                folder_refused: false,
                choice: None,
                custom_pair: false,
                tiles: tiles.clone(),
                ids: Vec::clone(&ids),
                restarts_fail: n == 0,
                errors: 0,
                focus: None,
            })
            .boxed()
    }

    fn transitions(m: &Model) -> BoxedStrategy<Step> {
        if !m.open {
            let reset = proptest::sample::subsequence(SETTINGS.to_vec(), 1..=SETTINGS.len()).prop_shuffle();
            return prop_oneof![2 => Just(Step::Open), 1 => reset.prop_map(Step::OpenReset)].boxed();
        }
        let tiles = proptest::sample::select(m.tiles.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>());
        let ids = proptest::sample::select(m.ids.clone());
        prop_oneof![
            1 => Just(Step::Open),
            1 => any::<bool>().prop_map(|escape| Step::Close { escape }),
            4 => proptest::sample::select(Tab::ALL.to_vec()).prop_map(Step::Tab),
            4 => proptest::sample::select(port_texts()).prop_map(Step::TypePort),
            3 => Just(Step::SendPort),
            2 => Just(Step::PickPort),
            2 => any::<bool>().prop_map(|twice| Step::ToggleUpnp { twice }),
            3 => proptest::option::of(0..FOLDERS.len()).prop_map(Step::ChooseFolder),
            1 => Just(Step::ClearFolder),
            3 => tiles.prop_map(Step::Icon),
            1 => Just(Step::ToggleCustomPair),
            2 => (any::<bool>(), ids).prop_map(|(dark, id)| Step::Side { dark, id }),
        ]
        .boxed()
    }

    /// Each control is only there on its own tab, and some only sometimes.
    fn preconditions(m: &Model, step: &Step) -> bool {
        let on = |tab| m.open && m.tab == tab;
        match step {
            Step::Open => true,
            Step::OpenReset(_) => !m.open,
            Step::Close { .. } | Step::Tab(_) => m.open,
            Step::TypePort(_) | Step::SendPort | Step::ToggleUpnp { .. } => on(Tab::Network),
            Step::PickPort => on(Tab::Network) && matches!(m.stored.port, ListenPort::Pinned(_)),
            Step::ChooseFolder(_) => on(Tab::General),
            Step::ClearFolder => on(Tab::General) && m.stored.folder.is_some(),
            Step::Icon(_) | Step::ToggleCustomPair => on(Tab::AppIcon),
            Step::Side { .. } => on(Tab::AppIcon) && m.custom_pair,
        }
    }

    fn apply(mut m: Model, step: &Step) -> Model {
        m.focus = None;
        let opened = |m: &mut Model| {
            m.open = true;
            m.tab = Tab::General;
            m.field = m.port_text();
            (m.port_refused, m.folder_refused, m.custom_pair) = (None, false, false);
        };
        match step {
            Step::Open if !m.open => opened(&mut m),
            Step::OpenReset(reset) => {
                opened(&mut m);
                m.tab = tab_of(reset[0]).unwrap();
                let first = SETTINGS.into_iter().find(|name| tab_of(name) == Some(m.tab) && reset.contains(name)).unwrap();
                m.focus = Some(match first {
                    "default_destination" => "choose-folder".into(),
                    "listen_port" => "port-field".into(),
                    "upnp" => "upnp-switch".into(),
                    _ => m.tiles.iter().find(|(_, c)| *c == m.choice).map_or("icon-custom".into(), |(k, _)| k.clone()),
                });
            }
            Step::Open => {}
            Step::Close { .. } => m.open = false,
            Step::Tab(tab) => m.tab = *tab,
            Step::TypePort(text) => m.field = text.clone(),
            // A new port or UPnP that fails stores nothing: it's refused under its row and listed.
            Step::SendPort => match setting::port(&m.field) {
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
            Step::ToggleUpnp { twice } => {
                let times = if *twice { 2 } else { 1 };
                if m.restarts_fail {
                    m.errors += times;
                } else if times == 1 {
                    m.stored.upnp = !m.stored.upnp;
                }
            }
            Step::ChooseFolder(None) => {}
            Step::ChooseFolder(Some(GONE)) => m.folder_refused = true,
            Step::ChooseFolder(Some(i)) => {
                m.stored.folder = Some(FOLDERS[*i].into());
                m.folder_refused = false;
            }
            Step::ClearFolder => {
                m.stored.folder = None;
                m.folder_refused = false;
            }
            Step::Icon(key) => m.choice = m.tiles.iter().find(|(k, _)| k == key).and_then(|(_, c)| c.clone()),
            Step::ToggleCustomPair => m.custom_pair = !m.custom_pair,
            Step::Side { dark, id } => {
                let [d, l] = m.sides();
                let (d, l) = if *dark { (id.clone(), l) } else { (d, id.clone()) };
                m.choice = Some(Icons::Pair { dark: d, light: l });
            }
        }
        m
    }
}

impl StateMachineTest for Sheet {
    type SystemUnderTest = Runtime<Phantom>;
    type Reference = Sheet;

    fn init_test(m: &Model) -> Runtime<Phantom> {
        let service =
            MemoryService::new().unreachable(FOLDERS[GONE], Unavailable::DriveNotConnected { name: "MyDisk".into() }).port_taken(TAKEN);
        let service = if m.restarts_fail { service.failing_restarts() } else { service };
        let mut rt = app(service);
        rt.state.app_icon = AppIcon::new(Some(manifest()), None, |_| {});
        rt.set_viewport((960.0, 900.0), 2.0);
        settle(&mut rt);
        rt
    }

    fn apply(mut rt: Runtime<Phantom>, _: &Model, step: Step) -> Runtime<Phantom> {
        let r = &mut rt;
        match &step {
            Step::Open => {
                r.update(|_, tx| tx.blur());
                r.key(KeyEvent { key: Key::Command(','), mods: Mods { cmd: true, ..Default::default() } });
                // Presses land once the sheet has come in, as for a person.
                super::wait(r, 0.5);
            }
            Step::Close { escape: true } => r.key(KeyEvent { key: Key::Escape, mods: Mods::default() }),
            Step::Close { escape: false } => press(r, "header/close"),
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
            Step::PickPort => press(r, "pick-port"),
            Step::ToggleUpnp { twice: false } => press(r, "upnp-switch"),
            // Both presses before a frame: the second meets the first on its way.
            Step::ToggleUpnp { twice: true } => {
                let (x, y) = r.find("upnp-switch").expect("the switch");
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
            Step::Icon(key) => press(r, key),
            Step::ToggleCustomPair => press(r, "custom-pair"),
            Step::Side { dark, id } => press(r, &format!("{}-{id}", if *dark { "dark" } else { "light" })),
        }
        settle(r);
        rt
    }

    fn check_invariants(rt: &Runtime<Phantom>, m: &Model) {
        let s = &rt.state;
        assert_eq!(s.settings.is_some(), m.open, "the sheet is open");
        assert_eq!(rt.find("settings-sheet").is_some(), m.open, "and drawn");
        assert_eq!(s.app_icon.choice, m.choice, "the App icon");
        let listed = s.errors.entries().iter().filter(|e| e.notice.short.contains("didn't start again")).count();
        assert_eq!(listed, m.errors, "each failed restart is listed");
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
        };
        assert_eq!(stored, m.stored, "what's stored");
        assert_eq!(sheet.port.text, m.field, "the port field");

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
        }
        if m.tab == Tab::General {
            assert_eq!(rt.find("default_destination-refused").is_some(), m.folder_refused, "the folder's refusal");
        }
        if m.tab == Tab::AppIcon {
            let selected = checked(&rt.accessibility_json());
            let expected = match m.tiles.iter().find(|(_, c)| *c == m.choice) {
                Some((key, _)) => key.clone(),
                None => "icon-custom".into(),
            };
            assert!(selected.contains(&expected), "{expected} is selected: {selected:?}");
        }
    }
}

/// Presses the element at `key`, scrolling the sheet to it first.
fn press(rt: &mut Runtime<Phantom>, key: &str) {
    for _ in 0..40 {
        if on_screen(rt, key) {
            break;
        }
        let (Some((_, y)), Some((bx, by))) = (rt.find(key), rt.find("sheet-body")) else { break };
        rt.wheel(bx, by, -(y - by).clamp(-200.0, 200.0));
        rt.frame(FRAME);
    }
    assert!(on_screen(rt, key), "{key:?} can't be scrolled to");
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
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
