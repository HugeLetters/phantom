//! The Settings sheet and what its controls do. The service's settings (default destination,
//! keeping the Mac awake, listening port, UPnP, speed limits) are asked for when it opens and changed one at a time, each applied at
//! once: the service answers with all of them as they now are, which the sheet then shows. They
//! stream too, so another client's change shows at once ([`SettingsSheet::observe`]). The App
//! icon is the GUI's own, in [`AppIcon`], and so is the look, the Active Look Preset in
//! [`Looks`](crate::theme::preset::Looks), whose theme settings the Theme section sets ([`set_theme`]).
//! A value that can't be used is refused with why, under its row, and nothing changes. Each change
//! that takes effect is one step of Undo in Settings ([`crate::changes`], #221).
//!
//! A text setting (port, speed limits, seeding limits, theme colours; one path, [`Row`]) is applied by Return, or when its field
//! loses focus ([`follow_focus`], and by hand on closing the sheet) (#254): a value
//! that can't be used isn't kept then, the stored one comes back with the reason under the row.
//! Escape in a field being edited ([`escape`]) puts the stored value back and nothing else.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use client::{Failure, display, setting};
use proto::{Direction, SeedingLimit, Setting, SettingName, Settings};
use rune::{FileDialogOptions, TextEdit, Tx};

use super::Phantom;
use crate::notice::{Notice, NoticeAction, explained};
use crate::platform::app_icon::{Icons, Manifest};
use crate::service::Done;
use crate::theme::preset::Appearance;
use crate::theme::setting::{Rgb, Value};

/// The sheet, while it's open.
pub struct SettingsSheet {
    pub tab: Tab,
    /// As the service last answered; `None` until it has.
    pub service: Option<Settings>,
    /// The port, or what's typed over it until it's sent.
    pub port: TextEdit,
    /// Each limit as [`display::speed_limit`] shows it, empty when unlimited, or what's typed over
    /// it until it's sent.
    pub download_limit: TextEdit,
    pub upload_limit: TextEdit,
    /// Each seeding limit as [`display::ratio`] or [`display::seed_time`] shows it, empty when
    /// never, or what's typed over it until it's sent.
    pub seed_ratio: TextEdit,
    pub seed_time: TextEdit,
    /// Why each row's last change was refused, until its next change.
    pub refused: Vec<Refused>,
    /// The App icon's custom pair rows show.
    pub custom_pair: bool,
    /// The theme settings shown split, a control for each Appearance, though both hold the same
    /// value: split by hand, or set on one side since the sheet opened. One whose values differ
    /// shows split anyway.
    pub split: BTreeSet<&'static str>,
    /// Each colour setting's field, by which side it sets and its key, or what's typed over it
    /// until it's applied (Return, or the field losing focus).
    pub colours: Vec<ColourField>,
    /// Changes sent and not answered yet, oldest first: a control shows what was last asked of it.
    pub waiting: Vec<Waiting>,
    /// The setting, by its name in the settings file, whose control takes focus as the sheet opens.
    pub focus: Option<&'static str>,
    /// The text fields that had the focus when the last frame began, to see one let go of it.
    pub had_focus: Vec<Row>,
}

/// A change on its way to the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    Folder,
    Port,
    /// The switch turned on or off.
    Switch(Switch, bool),
    Limit(Direction),
    Seeding(SeedingLimit),
}

impl Waiting {
    fn row(self) -> Row {
        match self {
            Waiting::Folder => Row::Folder,
            Waiting::Port => Row::Port,
            Waiting::Switch(switch, _) => Row::Switch(switch),
            Waiting::Limit(direction) => Row::Limit(direction),
            Waiting::Seeding(at) => Row::Seeding(at),
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
    /// The default destination, keeping the Mac awake, and when seeding stops.
    #[default]
    General,
    /// The listening port, UPnP and the speed limits.
    Network,
    /// The Theme section (the Active Look Preset's theme in each Appearance, and that theme's
    /// settings), then the App icon section.
    Appearance,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::General, Tab::Network, Tab::Appearance];

    pub fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Network => "Network",
            Tab::Appearance => "Appearance",
        }
    }
}

/// Which Theme Looks a theme setting's control sets: both, while they're the same theme and the
/// field isn't split, or one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Both,
    Only(Appearance),
}

impl Side {
    pub fn appearances(self) -> &'static [Appearance] {
        match self {
            Side::Both => &Appearance::BOTH,
            Side::Only(Appearance::Light) => &[Appearance::Light],
            Side::Only(Appearance::Dark) => &[Appearance::Dark],
        }
    }

    /// Its part of a control's key: `both`, `light`, `dark`.
    pub fn key(self) -> &'static str {
        match self {
            Side::Both => "both",
            Side::Only(appearance) => appearance.key(),
        }
    }
}

/// A colour setting's text field.
pub struct ColourField {
    pub side: Side,
    pub key: &'static str,
    pub text: TextEdit,
}

/// What a refusal is about: the row it shows under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Row {
    /// Reading the settings: under the sheet's title.
    All,
    Folder,
    Port,
    Switch(Switch),
    Limit(Direction),
    Seeding(SeedingLimit),
    /// A colour theme setting's field, on a side.
    Colour(Side, &'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub row: Row,
    /// The row's field's text it's about, for a row with a field; it goes once the text changes.
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
            download_limit: TextEdit::new(""),
            upload_limit: TextEdit::new(""),
            seed_ratio: TextEdit::new(""),
            seed_time: TextEdit::new(""),
            refused: Vec::new(),
            custom_pair: false,
            split: BTreeSet::new(),
            colours: Vec::new(),
            waiting: Vec::new(),
            focus: None,
            had_focus: Vec::new(),
        }
    }

    /// Why `row`'s value was refused, while it's still about what the row shows: for a row with a
    /// field, until its text changes.
    pub fn refused(&self, row: Row) -> Option<&str> {
        let refused = self.refused.iter().find(|r| r.row == row)?;
        self.field(row).is_none_or(|field| refused.text == field.text).then_some(&refused.message)
    }

    /// The text field of a row that has one.
    fn field(&self, row: Row) -> Option<&TextEdit> {
        match row {
            Row::Port => Some(&self.port),
            Row::Limit(direction) => Some(self.limit(direction)),
            Row::Seeding(at) => Some(self.seeding(at)),
            Row::Colour(side, key) => self.colour(side, key).map(|f| &f.text),
            Row::All | Row::Folder | Row::Switch(_) => None,
        }
    }

    fn field_mut(&mut self, row: Row) -> Option<&mut TextEdit> {
        match row {
            Row::Port => Some(&mut self.port),
            Row::Limit(direction) => Some(self.limit_mut(direction)),
            Row::Seeding(at) => Some(self.seeding_mut(at)),
            Row::Colour(side, key) => self.colour_mut(side, key).map(|f| &mut f.text),
            Row::All | Row::Folder | Row::Switch(_) => None,
        }
    }

    /// The rows with a text field, in the order the tabs show them, then each colour's.
    fn text_rows(&self) -> Vec<Row> {
        TEXT_ROWS.into_iter().chain(self.colours.iter().map(|f| Row::Colour(f.side, f.key))).collect()
    }

    /// What a row's field shows for what's stored; `None` for a row without a field, or before
    /// the service has answered.
    fn stored_text(&self, row: Row, looks: &crate::theme::preset::Looks) -> Option<String> {
        if let Row::Colour(side, key) = row {
            return match looks.preset().get(side.appearances()[0]).settings.get(key) {
                Some(Value::Colour(rgb)) => Some(rgb.hex()),
                Some(Value::Toggle(_) | Value::Choice(_)) | None => None,
            };
        }
        let settings = self.service.as_ref()?;
        match row {
            Row::Port => Some(port_text(settings)),
            Row::Limit(direction) => Some(limit_text(settings, direction)),
            Row::Seeding(at) => Some(seeding_text(settings, at)),
            Row::All | Row::Folder | Row::Switch(_) | Row::Colour(..) => None,
        }
    }

    /// The text field being edited: focused, with text other than what it shows of what's stored.
    fn edited(&self, looks: &crate::theme::preset::Looks) -> Option<Row> {
        self.text_rows().into_iter().find(|&row| {
            let field = self.field(row).expect("a text row has a field");
            field.focused && self.stored_text(row, looks).is_some_and(|stored| field.text != stored)
        })
    }

    pub fn seeding(&self, at: SeedingLimit) -> &TextEdit {
        match at {
            SeedingLimit::Ratio => &self.seed_ratio,
            SeedingLimit::Time => &self.seed_time,
        }
    }

    pub fn seeding_mut(&mut self, at: SeedingLimit) -> &mut TextEdit {
        match at {
            SeedingLimit::Ratio => &mut self.seed_ratio,
            SeedingLimit::Time => &mut self.seed_time,
        }
    }

    pub fn limit(&self, direction: Direction) -> &TextEdit {
        match direction {
            Direction::Download => &self.download_limit,
            Direction::Upload => &self.upload_limit,
        }
    }

    pub fn limit_mut(&mut self, direction: Direction) -> &mut TextEdit {
        match direction {
            Direction::Download => &mut self.download_limit,
            Direction::Upload => &mut self.upload_limit,
        }
    }

    /// A row's field keeps its text, selected to be typed over.
    fn refuse(&mut self, row: Row, message: impl Into<String>) {
        let text = self.field(row).map(|field| field.text.clone()).unwrap_or_default();
        let len = text.len();
        if let Some(field) = self.field_mut(row) {
            field.set_selection(0, len);
        }
        self.refused.retain(|r| r.row != row);
        self.refused.push(Refused { row, text, message: message.into() });
    }

    /// The setting as its switch shows it: as last asked while a change is on its way, else as
    /// stored.
    pub fn switch(&self, switch: Switch) -> Option<bool> {
        let asked = self.waiting.iter().rev().find_map(|w| match *w {
            Waiting::Switch(which, on) if which == switch => Some(on),
            Waiting::Switch(..) | Waiting::Folder | Waiting::Port | Waiting::Limit(_) | Waiting::Seeding(_) => None,
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
        for at in [SeedingLimit::Ratio, SeedingLimit::Time] {
            if matches!(row, Row::All) || row == Row::Seeding(at) {
                let mut field = TextEdit::new(seeding_text(&settings, at));
                field.focused = row == Row::All && self.focus == Some(seeding_name(at));
                *self.seeding_mut(at) = field;
            }
        }
        for direction in Direction::BOTH {
            if matches!(row, Row::All) || row == Row::Limit(direction) {
                let mut field = TextEdit::new(limit_text(&settings, direction));
                field.focused = row == Row::All && self.focus == Some(limit_name(direction));
                *self.limit_mut(direction) = field;
            }
        }
        self.refused.retain(|r| r.row != row);
        self.service = Some(settings);
    }

    /// The settings as the service streams them (#221): a field still showing what was stored
    /// shows the new value; what's typed over one stays.
    pub fn observe(&mut self, settings: Settings) {
        let Some(old) = self.service.clone() else { return self.answered(Row::All, settings) };
        let follow = |field: &mut TextEdit, was: String, now: String| {
            if field.text == was && now != was {
                let focused = field.focused;
                *field = TextEdit::new(now);
                field.focused = focused;
            }
        };
        follow(&mut self.port, port_text(&old), port_text(&settings));
        for direction in Direction::BOTH {
            follow(self.limit_mut(direction), limit_text(&old, direction), limit_text(&settings, direction));
        }
        for at in [SeedingLimit::Ratio, SeedingLimit::Time] {
            follow(self.seeding_mut(at), seeding_text(&old, at), seeding_text(&settings, at));
        }
        self.service = Some(settings);
    }

    /// Whether a field is being edited: focused, with text other than what it shows of what's
    /// stored. ⌘Z is then its own.
    pub fn editing(&self, looks: &crate::theme::preset::Looks) -> bool {
        self.edited(looks).is_some()
    }

    /// The service's answer to an undo of `name`: its row shows the value it went back to, whatever
    /// was typed in its field.
    pub fn answered_undo(&mut self, name: SettingName, settings: Settings) {
        self.answered(row_of(name), settings);
    }
}

/// The rows with a text field that applies when it loses focus, in the order the tabs show them.
const TEXT_ROWS: [Row; 5] = [
    Row::Port,
    Row::Limit(Direction::Download),
    Row::Limit(Direction::Upload),
    Row::Seeding(SeedingLimit::Ratio),
    Row::Seeding(SeedingLimit::Time),
];

/// The row of the service setting `name`.
pub fn row_of(name: SettingName) -> Row {
    match name {
        SettingName::DefaultDestination => Row::Folder,
        SettingName::ListenPort => Row::Port,
        SettingName::Upnp => Row::Switch(Switch::Upnp),
        SettingName::KeepAwake => Row::Switch(Switch::KeepAwake),
        SettingName::DownloadLimit => Row::Limit(Direction::Download),
        SettingName::UploadLimit => Row::Limit(Direction::Upload),
        SettingName::SeedRatio => Row::Seeding(SeedingLimit::Ratio),
        SettingName::SeedTime => Row::Seeding(SeedingLimit::Time),
    }
}

/// After an undo, the setting `key` (a settings file's name, or a theme setting's key) on its tab,
/// its control focused: a field through its text, any other as the sheet's first in the Tab
/// order, which the sheet focuses once nothing has the focus. With the sheet closed, nothing.
pub fn focus_setting(s: &mut Phantom, key: &'static str, tx: Option<&mut Tx>) {
    use crate::views::settings::tab_of;
    let Some(sheet) = &mut s.settings else { return };
    sheet.tab = tab_of(key).unwrap_or(Tab::Appearance);
    let fields: Vec<&mut TextEdit> =
        [&mut sheet.port, &mut sheet.download_limit, &mut sheet.upload_limit, &mut sheet.seed_ratio, &mut sheet.seed_time]
            .into_iter()
            .chain(sheet.colours.iter_mut().map(|f| &mut f.text))
            .collect();
    for field in fields {
        field.focused = false;
    }
    let field = match key {
        "listen_port" => Some(&mut sheet.port),
        "download_limit" => Some(&mut sheet.download_limit),
        "upload_limit" => Some(&mut sheet.upload_limit),
        "seed_ratio" => Some(&mut sheet.seed_ratio),
        "seed_time" => Some(&mut sheet.seed_time),
        theme => sheet.colours.iter_mut().find(|f| f.key == theme).map(|f| &mut f.text),
    };
    match field {
        Some(field) => {
            field.focused = true;
            sheet.focus = None;
        }
        None => {
            sheet.focus = Some(key);
            if let Some(tx) = tx {
                tx.blur();
            }
        }
    }
}

/// A limit as its field shows it: exactly, or empty when unlimited.
pub fn limit_text(settings: &Settings, direction: Direction) -> String {
    settings.speed_limits.get(direction).map(display::speed_limit).unwrap_or_default()
}

/// A seeding limit as its field shows it: exactly, or empty when never.
pub fn seeding_text(settings: &Settings, at: SeedingLimit) -> String {
    match at {
        SeedingLimit::Ratio => settings.seeding.ratio.map(display::ratio),
        SeedingLimit::Time => settings.seeding.time.map(display::seed_time),
    }
    .unwrap_or_default()
}

/// A seeding limit's name in the settings file.
pub fn seeding_name(at: SeedingLimit) -> &'static str {
    match at {
        SeedingLimit::Ratio => "seed_ratio",
        SeedingLimit::Time => "seed_time",
    }
}

/// A limit's name in the settings file.
pub fn limit_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Download => "download_limit",
        Direction::Upload => "upload_limit",
    }
}

/// The port a person set or the service picked; while a pinned one is taken, still that one.
pub fn port_text(settings: &Settings) -> String {
    settings.listen_port.port().to_string()
}

/// The App icon: what's chosen, what following the theme shows, and what can be chosen. Choosing
/// applies it at once and keeps it in `gui.json`, and a theme's icon that changes shows at once,
/// both through `show`.
pub struct AppIcon {
    /// What the picker offers; `None` when the icons can't be read, and there's nothing to pick.
    pub manifest: Option<Manifest>,
    /// `None` follows the theme.
    pub choice: Option<Icons>,
    /// What following the theme shows: the App icon of the look each Appearance shows.
    pub theme: Icons,
    /// Each icon drawn, by id, once decoded; the picker names those not (yet) drawn.
    pub images: Vec<(String, rune::Image)>,
    show: Box<dyn FnMut(IconChange<'_>)>,
}

/// What changed about the App icon, for the Dock and the bundle.
pub enum IconChange<'a> {
    /// A new choice, to keep; `None` follows the theme.
    Chose(Option<&'a Icons>),
    /// What following the theme shows.
    Theme(&'a Icons),
}

impl AppIcon {
    /// `manifest`, with `choice` as kept and `theme` what following the theme shows, and `show` to
    /// show (and keep) each change.
    pub fn new(manifest: Option<Manifest>, choice: Option<Icons>, theme: Icons, show: impl FnMut(IconChange<'_>) + 'static) -> Self {
        Self { manifest, choice, theme, images: Vec::new(), show: Box::new(show) }
    }

    /// Follows the theme's icon `theme` from now on; shown at once when it's another.
    pub fn follow(&mut self, theme: Icons) {
        if theme != self.theme {
            self.theme = theme;
            (self.show)(IconChange::Theme(&self.theme));
        }
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
        Some([true, false].map(|dark| crate::platform::app_icon::resolve(self.choice.as_ref(), manifest, &self.theme, dark).id.clone()))
    }
}

impl Default for AppIcon {
    /// Nothing to pick from, as without the icons, following a fresh install's look.
    fn default() -> Self {
        Self::new(None, None, crate::theme::preset::Looks::default().app_icons(), |_| {})
    }
}

/// What reading `file` (`gui.json`, or the service's `settings.json`, both in the data dir) reset,
/// if anything: a warning that explains, with Reveal in Finder for the copy of the file as it was,
/// and Open settings ([`open_reset`]) (#72).
pub fn reset_notice(reset: &[String], file: &str) -> Option<Notice> {
    // A theme setting is worded here, by its theme; the rest by `display`.
    let labels: Vec<String> = reset.iter().map(|name| crate::theme::preset::reset_label(name).unwrap_or_else(|| name.clone())).collect();
    let short = display::settings_reset(&labels)?;
    let copy = proto::tolerant::invalid_copy(&proto::data_dir().join(file));
    let name = copy.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let reveal = NoticeAction::new(display::reveal_in_finder(), move |s: &mut Phantom| {
        if let Err(why) = s.finder.show(&copy, crate::platform::Show::Reveal) {
            s.bar.tell(Notice::error(why));
        }
    });
    let reset = reset.to_vec();
    let open = NoticeAction::new("Open settings", move |s: &mut Phantom| open_reset(s, &reset));
    Some(Notice { more: Some(explained(display::settings_reset_why(&name), vec![reveal, open])), ..Notice::warning(short) })
}

/// Opens the sheet on the tab of the first setting in `reset`, focusing the control of the first
/// one reset there, in the order the tab shows them.
pub fn open_reset(s: &mut Phantom, reset: &[String]) {
    use crate::views::settings::{SETTINGS, setting_of, tab_of};
    let tab = reset.iter().find_map(|name| tab_of(name));
    open(s);
    if let (Some(sheet), Some(tab)) = (&mut s.settings, tab) {
        sheet.tab = tab;
        sheet.focus = SETTINGS.into_iter().find(|name| tab_of(name) == Some(tab) && reset.iter().any(|r| setting_of(r) == *name));
    }
}

/// Opens the sheet, asking the service for its settings; already open, nothing.
pub fn open(s: &mut Phantom) {
    if s.settings.is_some() {
        return;
    }
    let mut sheet = SettingsSheet::new();
    sheet.show_colours(&s.looks);
    s.settings = Some(sheet);
    s.service.settings(answer(Row::All));
}

pub fn show_tab(s: &mut Phantom, tab: Tab) {
    if let Some(sheet) = &mut s.settings {
        sheet.tab = tab;
    }
}

pub fn close(s: &mut Phantom) {
    leave_all(s);
    s.settings = None;
}

/// What a text field's text is, once it's a value.
enum Change {
    Send(Waiting, Setting),
    Colour(Rgb),
}

/// A row's text field as a change: what's to be made of it, or why its text can't be.
fn change_of(sheet: &SettingsSheet, row: Row) -> Option<Result<Change, String>> {
    let text = &sheet.field(row)?.text;
    let problem = |problem| display::value_problem(problem).to_owned();
    let send = |waiting, setting| Change::Send(waiting, setting);
    Some(match row {
        Row::Port => setting::port(text).map(|port| send(Waiting::Port, Setting::ListenPort(Some(port)))).map_err(problem),
        Row::Limit(direction) => {
            let limit = if text.trim().is_empty() { Ok(None) } else { setting::speed(text).map(Some) };
            limit
                .map(|limit| send(Waiting::Limit(direction), Setting::SpeedLimit { direction, bytes_per_sec: limit.map(u64::from) }))
                .map_err(problem)
        }
        Row::Seeding(at) => {
            let value = match (text.trim().is_empty(), at) {
                (true, _) => Ok(None),
                (false, SeedingLimit::Ratio) => setting::ratio(text).map(|r| Some(r.hundredths())),
                (false, SeedingLimit::Time) => setting::seed_time(text).map(|t| Some(t.minutes())),
            };
            value.map(|value| send(Waiting::Seeding(at), Setting::StopSeeding { at, value })).map_err(problem)
        }
        Row::Colour(..) => Rgb::parse(text).map(Change::Colour).ok_or_else(|| NOT_A_COLOUR.to_owned()),
        Row::All | Row::Folder | Row::Switch(_) => return None,
    })
}

/// Makes the change: sent to the service, or set in the theme.
fn apply(s: &mut Phantom, row: Row, change: Change) {
    match (change, row) {
        (Change::Send(waiting, setting), _) => send_field(s, waiting, setting),
        (Change::Colour(rgb), Row::Colour(side, key)) => {
            if let Some(sheet) = &mut s.settings {
                sheet.refused.retain(|r| r.row != row);
                if let Some(field) = sheet.colour_mut(side, key) {
                    field.text = TextEdit::new(rgb.hex());
                }
            }
            set_theme(s, side, key, Value::Colour(rgb));
        }
        (Change::Colour(_), _) => {}
    }
}

/// Return in a text field: its value applied, or why it can't be, under its row, the text kept.
pub fn submit(s: &mut Phantom, row: Row) {
    let Some(sheet) = &mut s.settings else { return };
    match change_of(sheet, row) {
        Some(Ok(change)) => apply(s, row, change),
        Some(Err(why)) => sheet.refuse(row, why),
        None => {}
    }
}

/// Sends the port field's port, or says why its text isn't one.
pub fn submit_port(s: &mut Phantom) {
    submit(s, Row::Port);
}

/// Sends a limit's field: its speed, or unlimited when it's empty; or says why its text isn't one.
pub fn submit_limit(s: &mut Phantom, direction: Direction) {
    submit(s, Row::Limit(direction));
}

/// Sends a seeding limit's field: its ratio or time, or never when it's empty; or says why its
/// text isn't one.
pub fn submit_seeding(s: &mut Phantom, at: SeedingLimit) {
    submit(s, Row::Seeding(at));
}

/// Sets a colour field's colour, or says why its text isn't one.
pub fn submit_colour(s: &mut Phantom, side: Side, key: &'static str) {
    submit(s, Row::Colour(side, key));
}

/// Called before every frame: a text field that had the focus and hasn't now (Tab, a click
/// elsewhere) applies its text (#254).
pub fn follow_focus(s: &mut Phantom) {
    let Some(sheet) = &mut s.settings else { return };
    let now: Vec<Row> = sheet.text_rows().into_iter().filter(|&row| sheet.field(row).is_some_and(|f| f.focused)).collect();
    let left: Vec<Row> = sheet.had_focus.iter().copied().filter(|row| !now.contains(row)).collect();
    sheet.had_focus = now;
    for row in left {
        leave(s, row);
    }
}

/// The focus leaves every text field, as closing the sheet takes it (a tab switch or a click
/// elsewhere takes it first, and [`follow_focus`] sees that).
fn leave_all(s: &mut Phantom) {
    let Some(sheet) = &mut s.settings else { return };
    let rows: Vec<Row> = sheet
        .text_rows()
        .into_iter()
        .filter(|&row| sheet.had_focus.contains(&row) || sheet.field(row).is_some_and(|f| f.focused))
        .collect();
    sheet.had_focus.clear();
    for row in rows {
        if let Some(sheet) = &mut s.settings
            && let Some(field) = sheet.field_mut(row)
        {
            field.focused = false;
        }
        leave(s, row);
    }
}

/// `row`'s field lost the focus: its text is applied. Text that's what's stored, or sent and not
/// answered yet, does nothing. Text that can't be used isn't kept: the stored one comes back, with
/// why under the row, as Return words it.
fn leave(s: &mut Phantom, row: Row) {
    let Some(sheet) = &mut s.settings else { return };
    let (Some(stored), Some(field)) = (sheet.stored_text(row, &s.looks), sheet.field(row)) else { return };
    if field.text == stored || sheet.waiting.iter().any(|w| w.row() == row) {
        return;
    }
    // What's refused already (Return in it, or the service) isn't sent again.
    let refused = sheet.refused(row).map(str::to_owned);
    let change = match refused {
        Some(why) => Err(why),
        None => change_of(sheet, row).expect("a text row has a change"),
    };
    match change {
        Ok(change) => apply(s, row, change),
        Err(why) => sheet.put_back(row, stored, why),
    }
}

/// Escape in a field being edited: its text goes, the stored value shows again, and the field keeps
/// the focus. Whether there was one; Escape then does nothing more, and the next one closes the
/// sheet.
pub fn escape(s: &mut Phantom) -> bool {
    let Some(sheet) = &mut s.settings else { return false };
    let Some(row) = sheet.edited(&s.looks) else { return false };
    let stored = sheet.stored_text(row, &s.looks).expect("an edited field has what it's stored as");
    let mut field = TextEdit::new(stored);
    field.focused = true;
    *sheet.field_mut(row).expect("a text row has a field") = field;
    sheet.refused.retain(|r| r.row != row);
    true
}

impl SettingsSheet {
    /// `row`'s field shows `stored` again, with `why` the text wasn't used under it.
    fn put_back(&mut self, row: Row, stored: String, why: String) {
        if let Some(field) = self.field_mut(row) {
            *field = TextEdit::new(stored.clone());
        }
        self.refused.retain(|r| r.row != row);
        self.refused.push(Refused { row, text: stored, message: why });
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

/// Shows and keeps `choice`; `None` follows the theme. Another than chosen is a change to undo.
pub fn choose_icon(s: &mut Phantom, choice: Option<Icons>) {
    follow_focus(s);
    if choice != s.app_icon.choice {
        let was = crate::changes::Was::Icon(s.app_icon.choice.clone());
        crate::changes::app_changed(s, crate::changes::machine::AppSetting::Icon, was);
    }
    show_icon(s, choice);
}

/// Shows and keeps `choice`, as an undo does.
pub fn show_icon(s: &mut Phantom, choice: Option<Icons>) {
    (s.app_icon.show)(IconChange::Chose(choice.as_ref()));
    s.app_icon.choice = choice;
}

/// The Active Look Preset back to `preset`, as an undo does: each look it changes rebuilt and kept,
/// and the App icon the looks give shown.
pub fn show_preset(s: &mut Phantom, preset: crate::theme::preset::LookPreset) {
    for appearance in Appearance::BOTH {
        s.looks.set(appearance, preset.get(appearance).clone());
    }
    s.app_icon.follow(s.looks.app_icons());
    if let Some(sheet) = &mut s.settings {
        // The fields show the values they went back to, whatever was typed in them.
        sheet.colours.clear();
        sheet.refused.retain(|r| !matches!(r.row, Row::Colour(..)));
        sheet.show_colours(&s.looks);
    }
}

/// What the App icon `choice` is called: "Follow theme", a pair's or an icon's name, or a custom
/// pair's two.
pub fn icon_name(icon: &AppIcon, choice: Option<&Icons>) -> String {
    let manifest = icon.manifest.as_ref();
    let name = |id: &str| manifest.and_then(|m| m.icon(id)).map_or(id.to_owned(), |i| i.name.clone());
    match choice {
        None => "Follow theme".into(),
        Some(icons) if let Some(pair) = manifest.and_then(|m| m.pairs().iter().find(|p| p.icons() == *icons)) => pair.name.clone(),
        Some(Icons::One { icon }) => name(icon),
        Some(Icons::Pair { dark, light }) => format!("{} / {}", name(dark), name(light)),
    }
}

/// The theme setting `key`'s value on `side` (Light's, for both), as an undo tells it: a choice's
/// label, on or off, a colour's hex. Split, the side's in the setting's name ([`theme_named`]).
pub fn theme_value(looks: &crate::theme::preset::Looks, side: Side, key: &str) -> String {
    let appearance = side.appearances()[0];
    let look = looks.preset().get(appearance);
    let Some((setting, value)) = look.settings.iter().find(|(s, _)| s.key == key) else { return String::new() };
    let value = match (&setting.kind, value) {
        (crate::theme::setting::Kind::Choice { choices, .. }, Value::Choice(id)) => {
            choices.iter().find(|c| c.id == id).map_or(id, |c| c.label).to_owned()
        }
        (_, Value::Toggle(on)) => if on { "on" } else { "off" }.to_owned(),
        (_, Value::Colour(rgb)) => rgb.hex(),
        (_, Value::Choice(id)) => id.to_owned(),
    };
    value
}

/// A theme setting `label`, as an undo set on `side` tells it: "Tone"; split, "Light Tone".
pub fn theme_named(label: &str, side: Side) -> String {
    match side {
        Side::Both => label.to_owned(),
        Side::Only(appearance) => format!("{} {label}", appearance.label()),
    }
}

/// Sets the theme setting `key` to `value` in the Theme Looks of `side`, each of whose theme has
/// it: rebuilding the look of each it changes and keeping the preset, and showing the App icon
/// the looks now give. Set on one side, the field stays split.
pub fn set_theme(s: &mut Phantom, side: Side, key: &'static str, value: Value) {
    follow_focus(s);
    let was = s.looks.preset().clone();
    let label = side
        .appearances()
        .iter()
        .find_map(|a| s.looks.preset().get(*a).settings.iter().find(|(st, _)| st.key == key))
        .map(|(st, _)| st.label);
    for &appearance in side.appearances() {
        let mut look = s.looks.preset().get(appearance).clone();
        if look.settings.set(key, value) {
            s.looks.set(appearance, look);
        }
    }
    if let (Side::Only(_), Some(sheet)) = (side, &mut s.settings) {
        sheet.split.insert(key);
    }
    s.app_icon.follow(s.looks.app_icons());
    if let Some(sheet) = &mut s.settings {
        sheet.show_colours(&s.looks);
    }
    if let Some(label) = label.filter(|_| *s.looks.preset() != was) {
        crate::changes::app_changed(s, crate::changes::machine::AppSetting::Theme { key, label }, crate::changes::Was::Theme(was, side));
    }
}

/// Shows the theme setting `key` with a control for each Appearance, or one for both again.
pub fn split_theme(s: &mut Phantom, key: &'static str, split: bool) {
    let Some(sheet) = &mut s.settings else { return };
    match split {
        true => sheet.split.insert(key),
        false => sheet.split.remove(key),
    };
    sheet.show_colours(&s.looks);
}

/// Why a colour isn't one.
pub const NOT_A_COLOUR: &str = "A colour is # and six hex digits, like #3366ff";

impl SettingsSheet {
    /// The field of the colour setting `key` on `side`.
    pub fn colour(&self, side: Side, key: &str) -> Option<&ColourField> {
        self.colours.iter().find(|f| f.side == side && f.key == key)
    }

    pub fn colour_mut(&mut self, side: Side, key: &str) -> Option<&mut ColourField> {
        self.colours.iter_mut().find(|f| f.side == side && f.key == key)
    }

    /// A field for each colour setting each side shows: one still shown keeps what's typed in it
    /// and its refusal, a new one shows the colour it holds.
    pub fn show_colours(&mut self, looks: &crate::theme::preset::Looks) {
        let mut fields = Vec::new();
        let shown = theme_groups(looks.preset(), &self.split).into_iter().flat_map(|g| g.fields);
        for (setting, sides) in shown.map(|f| (f.setting, f.sides)) {
            for (side, value) in sides {
                let Value::Colour(rgb) = value else { continue };
                let field = match self.colours.iter().position(|f| f.side == side && f.key == setting.key) {
                    Some(i) => self.colours.swap_remove(i),
                    None => ColourField { side, key: setting.key, text: TextEdit::new(rgb.hex()) },
                };
                fields.push(field);
            }
        }
        self.colours = fields;
    }
}

/// What the Theme section shows of the Active Look Preset: one group with the same theme in both
/// halves, else one for each Appearance.
pub struct ThemeGroup {
    /// The Appearance it's for; `None` for both.
    pub appearance: Option<Appearance>,
    pub theme: &'static dyn crate::theme::Theme,
    pub fields: Vec<ThemeField>,
}

/// A theme setting as the Theme section shows it: one control for both Appearances, or one for each.
pub struct ThemeField {
    pub setting: &'static crate::theme::setting::Setting,
    /// `[(Both, value)]`, or a side and its value for each Appearance.
    pub sides: Vec<(Side, Value)>,
}

impl ThemeField {
    pub fn split(&self) -> bool {
        self.sides.len() > 1
    }
}

/// The groups and fields of `preset`: with the same theme in both halves a field is split when
/// its two values differ or it's in `split`, else one control sets both.
pub fn theme_groups(preset: &crate::theme::preset::LookPreset, split: &BTreeSet<&'static str>) -> Vec<ThemeGroup> {
    let alone = |appearance: Appearance| {
        let look = preset.get(appearance);
        let fields = look.settings.iter().map(|(setting, value)| ThemeField { setting, sides: vec![(Side::Only(appearance), value)] });
        ThemeGroup { appearance: Some(appearance), theme: look.theme, fields: fields.collect() }
    };
    if preset.light.theme.id() != preset.dark.theme.id() {
        return Appearance::BOTH.into_iter().map(alone).collect();
    }
    let fields = preset.light.settings.iter().zip(preset.dark.settings.iter()).map(|((setting, light), (_, dark))| {
        let sides = if light == dark && !split.contains(setting.key) {
            vec![(Side::Both, light)]
        } else {
            vec![(Side::Only(Appearance::Light), light), (Side::Only(Appearance::Dark), dark)]
        };
        ThemeField { setting, sides }
    });
    vec![ThemeGroup { appearance: None, theme: preset.light.theme, fields: fields.collect() }]
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

/// Return in a field: `setting` sent, unless it's what's stored and no change of the row is on its
/// way, when the field only shows it again: nothing changes, so there's no step for ⌘Z to take.
fn send_field(s: &mut Phantom, waiting: Waiting, setting: Setting) {
    let Some(sheet) = &mut s.settings else { return };
    let stored = sheet.service.clone().filter(|stored| holds(stored, &setting));
    match stored {
        Some(stored) if !sheet.waiting.iter().any(|w| w.row() == waiting.row()) => sheet.answered(waiting.row(), stored),
        Some(_) | None => ask(s, waiting, setting),
    }
}

/// Whether `settings` already hold what a field's `setting` sets.
fn holds(settings: &Settings, setting: &Setting) -> bool {
    match setting {
        Setting::ListenPort(Some(port)) => settings.listen_port == proto::ListenPort::Pinned(*port),
        Setting::SpeedLimit { direction, bytes_per_sec } => settings.speed_limits.get(*direction).map(u64::from) == *bytes_per_sec,
        Setting::StopSeeding { at: SeedingLimit::Ratio, value } => settings.seeding.ratio.map(|r| r.hundredths()) == *value,
        Setting::StopSeeding { at: SeedingLimit::Time, value } => settings.seeding.time.map(|t| t.minutes()) == *value,
        Setting::ListenPort(None) | Setting::DefaultDestination(_) | Setting::Upnp(_) | Setting::KeepAwake(_) => false,
    }
}

/// Sends `setting`, waiting for its answer as `waiting`: a change to undo, if it changes anything.
fn ask(s: &mut Phantom, waiting: Waiting, setting: Setting) {
    // A field this change took the focus from is applied first: the order a person made them in.
    follow_focus(s);
    if let Some(sheet) = &mut s.settings {
        sheet.waiting.push(waiting);
    }
    let by = crate::changes::sent(s, setting.name());
    let done = answer(waiting.row());
    s.service.set_setting(
        setting,
        Some(by),
        Box::new(move |s: &mut Phantom, result: Result<proto::SettingSet, Failure>, tx: &mut Tx| {
            crate::changes::answered(s, by, result.as_ref().ok());
            done(s, result.map(|set| set.settings), tx);
        }),
    );
}

/// The answer to a change of `row`, or to opening: shown while the sheet's open. A refusal shows
/// under its row; a failure (the torrents didn't start again with a new port, say) also goes to
/// the status bar as an error, which outlives the sheet.
fn answer(row: Row) -> Done<Result<Settings, Failure>> {
    Box::new(move |s: &mut Phantom, result, _| {
        if let Err(failure @ (Failure::Failed(_) | Failure::Error(_))) = &result {
            s.bar.tell(Notice::error(display::sentence(&format!("{failure:#}"))));
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
