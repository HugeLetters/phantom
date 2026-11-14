//! The Settings sheet and what its controls do. The service's settings (default destination,
//! keeping the Mac awake, listening port, UPnP) are asked for when it opens and changed one at a time, each applied at
//! once: the service answers with all of them as they now are, which the sheet then shows. The App
//! icon is the GUI's own, in [`AppIcon`]. A value that can't be used is refused with why, under its
//! row, and nothing changes.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use client::{Failure, display, setting};
use proto::{Setting, Settings};
use rune::{FileDialogOptions, TextEdit, Tx};

use super::Phantom;
use crate::notice::{Notice, NoticeAction, explained};
use crate::platform::app_icon::{Icons, Manifest};
use crate::service::Done;

/// The sheet, while it's open.
pub struct SettingsSheet {
    pub tab: Tab,
    /// As the service last answered; `None` until it has.
    pub service: Option<Settings>,
    /// The port, or what's typed over it until it's sent with Return.
    pub port: TextEdit,
    /// Why each row's last change was refused, until its next change.
    pub refused: Vec<Refused>,
    /// The App icon's custom pair rows show.
    pub custom_pair: bool,
    /// Changes sent and not answered yet, oldest first: a control shows what was last asked of it.
    pub waiting: Vec<Waiting>,
    /// The setting, by its name in the settings file, whose control takes focus as the sheet opens.
    pub focus: Option<&'static str>,
}

/// A change on its way to the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    Folder,
    Port,
    /// The switch turned on or off.
    Switch(Switch, bool),
}

impl Waiting {
    fn row(self) -> Row {
        match self {
            Waiting::Folder => Row::Folder,
            Waiting::Port => Row::Port,
            Waiting::Switch(switch, _) => Row::Switch(switch),
        }
    }
}

/// A service setting that's on or off, shown as a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Switch {
    KeepAwake,
    Upnp,
}

impl Switch {
    /// Its name in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            Switch::KeepAwake => "keep_awake",
            Switch::Upnp => "upnp",
        }
    }

    /// The switch's key.
    pub fn key(self) -> &'static str {
        match self {
            Switch::KeepAwake => "keep-awake-switch",
            Switch::Upnp => "upnp-switch",
        }
    }

    pub fn stored(self, settings: &Settings) -> bool {
        match self {
            Switch::KeepAwake => settings.keep_awake,
            Switch::Upnp => settings.upnp,
        }
    }

    fn set(self, on: bool) -> Setting {
        match self {
            Switch::KeepAwake => Setting::KeepAwake(Some(on)),
            Switch::Upnp => Setting::Upnp(Some(on)),
        }
    }
}

/// The sheet's tabs, in order; each setting is on one of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// The default destination and keeping the Mac awake.
    #[default]
    General,
    /// The listening port and UPnP.
    Network,
    AppIcon,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::General, Tab::Network, Tab::AppIcon];

    pub fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Network => "Network",
            Tab::AppIcon => "App icon",
        }
    }
}

/// What a refusal is about: the row it shows under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Row {
    /// Reading the settings: under the sheet's title.
    All,
    Folder,
    Port,
    Switch(Switch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub row: Row,
    /// The port field's text it's about; it goes once the text changes.
    pub text: String,
    pub message: String,
}

impl SettingsSheet {
    /// Open on `tab`, showing `settings` as already answered: for scenes.
    pub fn showing(tab: Tab, settings: Settings) -> Self {
        let mut sheet = Self { tab, ..Self::new() };
        sheet.answered(Row::All, settings);
        sheet
    }

    fn new() -> Self {
        Self {
            tab: Tab::default(),
            service: None,
            port: TextEdit::new(""),
            refused: Vec::new(),
            custom_pair: false,
            waiting: Vec::new(),
            focus: None,
        }
    }

    /// Why `row`'s value was refused, while it's still about what the row shows.
    pub fn refused(&self, row: Row) -> Option<&str> {
        let refused = self.refused.iter().find(|r| r.row == row)?;
        (row != Row::Port || refused.text == self.port.text).then_some(&refused.message)
    }

    /// The port field keeps its text, selected to be typed over.
    fn refuse(&mut self, row: Row, message: impl Into<String>) {
        if row == Row::Port {
            self.port.set_selection(0, self.port.text.len());
        }
        self.refused.retain(|r| r.row != row);
        self.refused.push(Refused { row, text: self.port.text.clone(), message: message.into() });
    }

    /// The setting as its switch shows it: as last asked while a change is on its way, else as
    /// stored.
    pub fn switch(&self, switch: Switch) -> Option<bool> {
        let asked = self.waiting.iter().rev().find_map(|w| match *w {
            Waiting::Switch(which, on) if which == switch => Some(on),
            Waiting::Switch(..) | Waiting::Folder | Waiting::Port => None,
        });
        asked.or(self.service.as_ref().map(|settings| switch.stored(settings)))
    }

    /// The service's answer to a change of `row`: the settings now. The port field shows the port
    /// once it's the port that changed, so what's typed there stays through other changes.
    fn answered(&mut self, row: Row, settings: Settings) {
        if matches!(row, Row::All | Row::Port) {
            self.port = TextEdit::new(port_text(&settings));
            // A field asks for focus through its text.
            self.port.focused = row == Row::All && self.focus == Some("listen_port");
        }
        self.refused.retain(|r| r.row != row);
        self.service = Some(settings);
    }
}

/// The port a person set or the service picked; while a pinned one is taken, still that one.
pub fn port_text(settings: &Settings) -> String {
    settings.listen_port.port().to_string()
}

/// The App icon: what's chosen, and what can be. Choosing applies it at once and keeps it in
/// `gui.json`, through `keep`.
pub struct AppIcon {
    /// What the picker offers; `None` when the icons can't be read, and there's nothing to pick.
    pub manifest: Option<Manifest>,
    /// `None` follows the theme.
    pub choice: Option<Icons>,
    /// Each icon drawn, by id, once decoded; the picker names those not (yet) drawn.
    pub images: Vec<(String, rune::Image)>,
    keep: Box<dyn FnMut(Option<&Icons>)>,
}

impl AppIcon {
    /// `manifest`, with `choice` as kept, and `keep` to show and keep a new one.
    pub fn new(manifest: Option<Manifest>, choice: Option<Icons>, keep: impl FnMut(Option<&Icons>) + 'static) -> Self {
        Self { manifest, choice, images: Vec::new(), keep: Box::new(keep) }
    }

    /// Decodes each icon to draw, off the main thread: the picker shows them as they come.
    pub fn load_images(&mut self, cache: &rune::ImageCache) {
        let Some(manifest) = &self.manifest else { return };
        self.images = manifest.icons().iter().map(|icon| (icon.id.clone(), cache.load(icon.bytes.clone()))).collect();
    }

    pub fn image(&self, id: &str) -> Option<&rune::Image> {
        self.images.iter().find(|(i, _)| i == id).map(|(_, image)| image)
    }

    /// The icon each Appearance shows now: `[dark, light]`.
    pub fn sides(&self) -> Option<[String; 2]> {
        let manifest = self.manifest.as_ref()?;
        let theme = crate::platform::app_icon::Theme::default();
        Some([true, false].map(|dark| crate::platform::app_icon::resolve(self.choice.as_ref(), manifest, theme, dark).id.clone()))
    }
}

impl Default for AppIcon {
    /// Nothing to pick from, as without the icons.
    fn default() -> Self {
        Self::new(None, None, |_| {})
    }
}

/// What reading `file` (`gui.json`, or the service's `settings.json`, both in the data dir) reset,
/// if anything: a warning that explains, with Reveal in Finder for the copy of the file as it was,
/// and Open settings ([`open_reset`]) (#72).
pub fn reset_notice(reset: &[String], file: &str) -> Option<Notice> {
    let short = display::settings_reset(reset)?;
    let copy = proto::tolerant::invalid_copy(&proto::data_dir().join(file));
    let name = copy.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let reveal = NoticeAction {
        title: display::reveal_in_finder().into(),
        run: Rc::new(move |s: &mut Phantom| {
            if let Err(why) = s.finder.show(&copy, crate::platform::Show::Reveal) {
                s.errors.report(Notice::error(why));
            }
        }),
    };
    let reset = reset.to_vec();
    let open = NoticeAction { title: "Open settings".into(), run: Rc::new(move |s: &mut Phantom| open_reset(s, &reset)) };
    Some(Notice { more: Some(explained(display::settings_reset_why(&name), vec![reveal, open])), ..Notice::warning(short) })
}

/// Opens the sheet on the tab of the first setting in `reset`, focusing the control of the first
/// one reset there, in the order the tab shows them.
pub fn open_reset(s: &mut Phantom, reset: &[String]) {
    use crate::views::settings::{SETTINGS, tab_of};
    let tab = reset.iter().find_map(|name| tab_of(name));
    open(s);
    if let (Some(sheet), Some(tab)) = (&mut s.settings, tab) {
        sheet.tab = tab;
        sheet.focus = SETTINGS.into_iter().find(|name| tab_of(name) == Some(tab) && reset.iter().any(|r| r == name));
    }
}

/// Opens the sheet, asking the service for its settings; already open, nothing.
pub fn open(s: &mut Phantom) {
    if s.settings.is_some() {
        return;
    }
    s.settings = Some(SettingsSheet::new());
    s.service.settings(answer(Row::All));
}

pub fn show_tab(s: &mut Phantom, tab: Tab) {
    if let Some(sheet) = &mut s.settings {
        sheet.tab = tab;
    }
}

pub fn close(s: &mut Phantom) {
    s.settings = None;
}

/// Sends the port field's port, or says why its text isn't one.
pub fn submit_port(s: &mut Phantom) {
    let Some(sheet) = &mut s.settings else { return };
    match setting::port(&sheet.port.text) {
        Ok(port) => ask(s, Waiting::Port, Setting::ListenPort(Some(port))),
        Err(problem) => sheet.refuse(Row::Port, display::value_problem(problem)),
    }
}

/// Lets the service pick the port: it keeps the one it has until another program takes it.
pub fn pick_port_automatically(s: &mut Phantom) {
    ask(s, Waiting::Port, Setting::ListenPort(None));
}

/// Flips the setting from what its switch shows, so a second toggle before the first is answered
/// asks the opposite of the first.
pub fn toggle(s: &mut Phantom, switch: Switch) {
    let Some(on) = s.settings.as_ref().and_then(|sheet| sheet.switch(switch)) else { return };
    ask(s, Waiting::Switch(switch, !on), switch.set(!on));
}

/// Asks for a folder, starting at the current one, and sets it as the default destination.
pub fn choose_folder(s: &mut Phantom, tx: &mut Tx) {
    let Some(sheet) = &s.settings else { return };
    let current = sheet.service.as_ref().and_then(|settings| settings.default_destination.clone());
    let directory = current.map(|folder| folder.into_path_buf()).or_else(downloads);
    let options = FileDialogOptions { title: "Choose the default download folder".into(), directory, ..Default::default() };
    tx.open_folder(options, |s: &mut Phantom, chosen, _| {
        let folder = match chosen {
            Ok(Some(folder)) => client::absolute(&folder).map_err(|e| format!("{e:#}")),
            Ok(None) => return,
            Err(e) => Err(format!("can't ask for a folder: {e}")),
        };
        match folder {
            Ok(folder) => ask(s, Waiting::Folder, Setting::DefaultDestination(Some(folder))),
            Err(why) => {
                if let Some(sheet) = &mut s.settings {
                    sheet.refuse(Row::Folder, display::sentence(&why));
                }
            }
        }
    });
}

/// No default destination: every add asks where.
pub fn clear_folder(s: &mut Phantom) {
    ask(s, Waiting::Folder, Setting::DefaultDestination(None));
}

/// Shows and keeps `choice`; `None` follows the theme.
pub fn choose_icon(s: &mut Phantom, choice: Option<Icons>) {
    (s.app_icon.keep)(choice.as_ref());
    s.app_icon.choice = choice;
}

/// Shows the custom pair's rows, or hides them.
pub fn toggle_custom_pair(s: &mut Phantom) {
    if let Some(sheet) = &mut s.settings {
        sheet.custom_pair = !sheet.custom_pair;
    }
}

/// Makes `id` one side of a custom pair, the other side staying as it shows now.
pub fn choose_side(s: &mut Phantom, dark: bool, id: &str) {
    let Some([d, l]) = s.app_icon.sides() else { return };
    let (dark, light) = if dark { (id.to_owned(), l) } else { (d, id.to_owned()) };
    choose_icon(s, Some(Icons::Pair { dark, light }));
}

/// Sends `setting`, waiting for its answer as `waiting`.
fn ask(s: &mut Phantom, waiting: Waiting, setting: Setting) {
    if let Some(sheet) = &mut s.settings {
        sheet.waiting.push(waiting);
    }
    s.service.set_setting(setting, answer(waiting.row()));
}

/// The answer to a change of `row`, or to opening: shown while the sheet's open. A refusal shows
/// under its row; a failure (the torrents didn't start again with a new port, say) also goes to
/// the error list, as it outlives the sheet.
fn answer(row: Row) -> Done<Result<Settings, Failure>> {
    Box::new(move |s: &mut Phantom, result, _| {
        if let Err(Failure::Error(e)) = &result {
            s.errors.report(Notice::error(display::sentence(&format!("{e:#}"))));
        }
        let Some(sheet) = &mut s.settings else { return };
        if let Some(at) = sheet.waiting.iter().position(|w| w.row() == row) {
            sheet.waiting.remove(at);
        }
        match result {
            Ok(settings) => sheet.answered(row, settings),
            Err(e) => sheet.refuse(row, display::sentence(&format!("{e:#}"))),
        }
    })
}

/// Where the folder picker starts without a default destination: `~/Downloads`.
fn downloads() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join("Downloads"))
}
