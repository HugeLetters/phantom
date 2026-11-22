//! The Settings window (#257) and what its controls do: [`SettingsWindow`], its own state, with the
//! sheet while it's open. It reaches the service and the undo list through the main window, which
//! owns them ([`crate::windows`]). The service's settings (default destination,
//! keeping the Mac awake, listening port, UPnP, speed limits) are asked for when it opens and changed one at a time, each applied at
//! once: the service answers with all of them as they now are, which the sheet then shows. They
//! stream too, so another client's change shows at once ([`SettingsSheet::observe`]). The App
//! icon is the GUI's own, in [`AppIcon`], and so is the look, the Active Look Preset in
//! [`Looks`](crate::theme::preset::Looks), whose theme settings the editor sheet sets ([`set_theme`]).
//! A value that can't be used is refused with why, under its row, and nothing changes. Each change
//! that takes effect is one step of Undo ([`crate::changes`], #221, #255).
//!
//! A text setting (port, speed limits, seeding limits, theme colours; one path, [`Row`]) is applied by Return, or when its field
//! loses focus ([`follow_focus`], and by hand on closing the sheet or the window losing the front,
//! [`left_window`]) (#254): a value
//! that can't be used isn't kept then, the stored one comes back with the reason under the row.
//! Escape in a field being edited ([`escape`]) puts the stored value back and nothing else.

pub mod presets;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use client::{Failure, display, setting};
use proto::{Direction, SeedingLimit, Setting, SettingName, Settings};
use rune::{FileDialogOptions, TextEdit, Tx};

use super::Phantom;
use crate::changes::{Presets, Target};
use crate::notice::{Notice, NoticeAction};
use crate::platform::app_icon::{FOLLOW, Icons, Manifest};
use crate::service::Done;
use crate::theme::preset::Appearance;
use crate::theme::preset::LookPreset;
use crate::theme::preset::Looks;
use crate::theme::saved::{Name, Saved, SavedPreset};
use crate::theme::setting::{Rgb, Value};
use crate::windows::{Inbox, Link, Shared, Window};

/// The Settings window's state: the sheet while it's open, the App icon, and the way to the main
/// window, which has the service and the undo list.
pub struct SettingsWindow {
    /// While the window is open.
    pub sheet: Option<SettingsSheet>,
    pub app_icon: AppIcon,
    /// The Active Look Preset, and the look each Appearance shows: the main window's too.
    pub looks: Shared<Looks>,
    /// The saved Look Presets, which only Settings shows.
    pub saved: Saved,
    /// Why the default destination can't be reached, as the main window's latest snapshot says
    /// (#209): the folder's row shows it as it changes.
    pub default_unavailable: Option<proto::Unavailable>,
    pub main: Link<Phantom>,
    pub inbox: Inbox<SettingsWindow>,
    /// This window, to bring to the front or close.
    pub window: Window,
    /// Whether a field was being edited, as last told to the main window, for Edit ▸ Undo.
    told_editing: bool,
}

impl SettingsWindow {
    pub fn new(
        looks: Shared<Looks>,
        saved: Saved,
        app_icon: AppIcon,
        main: Link<Phantom>,
        inbox: Inbox<SettingsWindow>,
        window: Window,
    ) -> Self {
        Self { sheet: None, app_icon, looks, saved, default_unavailable: None, main, inbox, window, told_editing: false }
    }

    /// Settings alone, linked to no main window, as in a scene of it or a test of one window.
    pub fn detached() -> Self {
        let inbox = crate::windows::link(|| {}).1;
        Self::new(Shared::default(), Saved::default(), AppIcon::default(), crate::windows::detached(), inbox, Window::none())
    }

    /// The looks, to read.
    pub fn looks(&self) -> std::cell::Ref<'_, Looks> {
        self.looks.borrow()
    }
}

/// The sheet: what the Settings window shows while it's open.
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
    /// The App icon's sheet is open over the window: a tile for every icon, each with its chips.
    pub icons: bool,
    /// A Look Preset being edited, in a sheet over the window, while it's open.
    pub edit: Option<EditSheet>,
    /// The chip an undo of an Apply focuses, with `focus` naming the saved presets.
    pub chip: Option<Chip>,
    /// Changes sent and not answered yet, oldest first: a control shows what was last asked of it.
    pub waiting: Vec<Waiting>,
    /// The setting, by its name in the settings file, whose control takes focus as the sheet opens.
    pub focus: Option<&'static str>,
    /// The text fields that had the focus when the last frame began, to see one let go of it.
    pub had_focus: Vec<Row>,
    /// Save as…, while it's open.
    pub save_as: Option<SaveAs>,
    /// Where an edit goes that's for a colour field no longer shown (an Apply or an undo changed
    /// the theme under it, and the window still had it focused): nowhere.
    gone: TextEdit,
}

/// One editor's own state, over the Look Preset it edits.
#[derive(Default)]
pub struct Editor {
    /// The rows shown split, a control in each half's column, though both hold the same value
    /// ([`THEME`] for the Theme row): split by hand, or set in one column since it opened. A row
    /// whose values differ (themes, for the Theme row) shows split anyway.
    pub split: BTreeSet<&'static str>,
    /// The Theme pop-up open, and of which side.
    pub menu: Option<Side>,
    /// Each colour setting's field, by which side it sets and its key, or what's typed over it
    /// until it's applied (Return, or the field losing focus).
    pub colours: Vec<ColourField>,
}

/// The sheet over Settings that edits a Look Preset, the Active look or a saved one: its editor.
/// On a saved preset both windows show it while it's open (a preview, never kept); on the Active
/// look, it's the look itself. Cancel puts back what it opened on; Done keeps its changes, as one
/// step of Undo.
pub struct EditSheet {
    pub target: Target,
    pub editor: Editor,
    /// The Look Presets as they were when it opened.
    pub opened: Presets,
    /// The theme setting whose control takes the focus, as an undo shows it, and the side it set
    /// ([`focused_side`]).
    pub focus: Option<(&'static str, Side)>,
}

/// A chip of a saved Look Preset's row: ◐ (`half` is `None`), ☀ or ☾.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub name: Name,
    pub half: Option<Appearance>,
}

/// Save as…: the name typed, and, for a name a saved preset has, the question whether to replace
/// it.
pub struct SaveAs {
    pub name: TextEdit,
    pub replace: Option<Name>,
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
    /// The Look Presets (the Active look and the saved ones, whose editor sheet sets a theme in each
    /// Appearance and its settings), then the App icon section.
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
    /// A colour theme setting's field, in the sheet's editor, on a side.
    Colour(Side, &'static str),
    /// The name to save the active Look Preset as.
    PresetName,
    /// The App icon's sheet: an image added that can't be read.
    Image,
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
            icons: false,
            edit: None,
            chip: None,
            waiting: Vec::new(),
            focus: None,
            had_focus: Vec::new(),
            save_as: None,
            gone: TextEdit::new(""),
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
            Row::Colour(side, key) => self.editor()?.colour(side, key).map(|f| &f.text),
            Row::PresetName => self.save_as.as_ref().map(|a| &a.name),
            Row::All | Row::Folder | Row::Switch(_) | Row::Image => None,
        }
    }

    fn field_mut(&mut self, row: Row) -> Option<&mut TextEdit> {
        match row {
            Row::Port => Some(&mut self.port),
            Row::Limit(direction) => Some(self.limit_mut(direction)),
            Row::Seeding(at) => Some(self.seeding_mut(at)),
            Row::Colour(side, key) => self.editor_mut()?.colour_mut(side, key).map(|f| &mut f.text),
            Row::PresetName => self.save_as.as_mut().map(|a| &mut a.name),
            Row::All | Row::Folder | Row::Switch(_) | Row::Image => None,
        }
    }

    /// The rows with a text field, in the order the tabs show them, then each colour's.
    fn text_rows(&self) -> Vec<Row> {
        let colours = self.editor().into_iter().flat_map(|e| e.colours.iter().map(|f| Row::Colour(f.side, f.key)));
        TEXT_ROWS.into_iter().chain(colours).collect()
    }

    /// The text of the colour field of `key` on `side`, for its edits; one no longer shown edits
    /// nothing.
    pub fn colour_edit(&mut self, side: Side, key: &str) -> &mut TextEdit {
        let Some(i) = self.editor().and_then(|e| e.colours.iter().position(|f| f.side == side && f.key == key)) else {
            self.gone = TextEdit::new("");
            return &mut self.gone;
        };
        &mut self.editor_mut().expect("it's shown").colours[i].text
    }

    /// Whether a sheet is open over the window: a Look Preset's, or the App icon's.
    pub fn over(&self) -> bool {
        self.edit.is_some() || self.icons
    }

    /// The sheet's editor, while it's open.
    pub fn editor(&self) -> Option<&Editor> {
        self.edit.as_ref().map(|e| &e.editor)
    }

    pub fn editor_mut(&mut self) -> Option<&mut Editor> {
        self.edit.as_mut().map(|e| &mut e.editor)
    }

    /// What the sheet edits, while it's open.
    pub fn target(&self) -> Option<&Target> {
        self.edit.as_ref().map(|e| &e.target)
    }

    /// What a row's field shows for what's stored; `None` for a row without a field, or before
    /// the service has answered.
    fn stored_text(&self, row: Row, looks: &Looks, saved: &Saved) -> Option<String> {
        if let Row::Colour(side, key) = row {
            let presets = Presets::of(looks, saved);
            return match presets.get(self.target()?)?.get(side.appearances()[0]).settings.get(key) {
                Some(Value::Colour(rgb)) => Some(rgb.hex()),
                Some(Value::Toggle(_) | Value::Choice(_)) | None => None,
            };
        }
        let settings = self.service.as_ref()?;
        match row {
            Row::Port => Some(port_text(settings)),
            Row::Limit(direction) => Some(limit_text(settings, direction)),
            Row::Seeding(at) => Some(seeding_text(settings, at)),
            Row::All | Row::Folder | Row::Switch(_) | Row::Colour(..) | Row::PresetName | Row::Image => None,
        }
    }

    /// The text field being edited: focused, with text other than what it shows of what's stored;
    /// the preset's name, with any.
    fn edited(&self, looks: &Looks, saved: &Saved) -> Option<Row> {
        let name = self.save_as.as_ref().filter(|a| a.name.focused && !a.name.text.is_empty()).map(|_| Row::PresetName);
        let stored = self.text_rows().into_iter().find(|&row| {
            let field = self.field(row).expect("a text row has a field");
            field.focused && self.stored_text(row, looks, saved).is_some_and(|stored| field.text != stored)
        });
        stored.or(name)
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
    pub fn editing(&self, looks: &Looks, saved: &Saved) -> bool {
        self.edited(looks, saved).is_some()
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

/// After an undo, the setting `key` in Settings (#255): an open sheet shows it at once
/// ([`focus_setting`]); a closed one opens at its tab, its control focused once the settings are
/// read, as Open settings does.
pub fn show_setting(s: &mut SettingsWindow, key: &'static str, tx: Option<&mut Tx>) {
    show(s, key, None, tx);
}

/// [`show_setting`], with `chip` the one of the saved Look Presets that takes the focus, if the
/// setting is them.
fn show(s: &mut SettingsWindow, key: &'static str, chip: Option<Chip>, tx: Option<&mut Tx>) {
    use crate::views::settings::tab_of;
    // It's not in a sheet over the window: that closes, so it shows.
    let sheet_open = s.sheet.as_ref().is_some_and(SettingsSheet::over);
    close_over(s);
    if s.sheet.is_some() {
        s.window.raise();
        focus_setting(s, key, chip.clone(), tx);
        // As the sheet goes, Rune gives the focus back to what opened it: the control asked for
        // takes it once the sheet's gone.
        if sheet_open {
            s.inbox.push(move |s, tx| focus_setting(s, key, chip, Some(tx)));
        }
        return;
    }
    open(s);
    if let Some(sheet) = &mut s.sheet {
        sheet.tab = tab_of(key).unwrap_or(Tab::Appearance);
        sheet.focus = Some(key);
        sheet.chip = chip;
    }
}

/// After an undo, the setting `key` (a settings file's name) on its tab, its control focused: a
/// field through its text, any other as the sheet's first in the Tab order, which the sheet
/// focuses once nothing has the focus; `chip`, the saved Look Presets' one that takes it. With the
/// sheet closed, nothing.
fn focus_setting(s: &mut SettingsWindow, key: &'static str, chip: Option<Chip>, tx: Option<&mut Tx>) {
    use crate::views::settings::tab_of;
    let Some(sheet) = &mut s.sheet else { return };
    sheet.tab = tab_of(key).unwrap_or(Tab::Appearance);
    let fields: Vec<&mut TextEdit> =
        [&mut sheet.port, &mut sheet.download_limit, &mut sheet.upload_limit, &mut sheet.seed_ratio, &mut sheet.seed_time]
            .into_iter()
            .chain(sheet.save_as.as_mut().map(|a| &mut a.name))
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
        _ => None,
    };
    sheet.chip = chip;
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
    /// The image files added as icons of their own, by id ([`crate::platform::app_icon::FILE`]),
    /// in the order they were added; kept in `gui.json`.
    pub added: Vec<String>,
    /// What decodes them, an image added too; `None` where nothing is drawn (tests).
    cache: Option<Rc<rune::ImageCache>>,
    show: Box<dyn FnMut(IconChange<'_>)>,
}

/// What changed about the App icon, for the Dock and the bundle.
pub enum IconChange<'a> {
    /// A new choice, to keep; `None` follows the theme.
    Chose(Option<&'a Icons>),
    /// What following the theme shows.
    Theme(&'a Icons),
    /// The images added as icons, to keep.
    Added(&'a [String]),
}

impl AppIcon {
    /// `manifest`, with `choice` as kept and `theme` what following the theme shows, and `show` to
    /// show (and keep) each change.
    pub fn new(manifest: Option<Manifest>, choice: Option<Icons>, theme: Icons, show: impl FnMut(IconChange<'_>) + 'static) -> Self {
        let mut icon = Self { manifest, choice, theme, images: Vec::new(), added: Vec::new(), cache: None, show: Box::new(show) };
        // The image files the choice names, read so they can show; a half whose icon isn't there
        // (an image moved or deleted) follows the theme.
        if let Some(choice) = icon.choice.clone() {
            for half in halves(Some(&choice)).into_iter().flatten() {
                let _ = icon.adopt(&Icons::One { icon: half });
            }
            if let Some(manifest) = &icon.manifest {
                let known = halves(Some(&choice)).map(|half| half.filter(|id| manifest.icon(id).is_some()));
                icon.choice = choice_of(known);
            }
        }
        icon
    }

    /// With the images added as icons `added` (those that can be read), as `gui.json` keeps them.
    pub fn with_added(mut self, added: Vec<String>) -> Self {
        // Only image files: a shipped icon's id here is no image added.
        for id in added.into_iter().filter(|id| id.starts_with(crate::platform::app_icon::FILE)) {
            if self.adopt(&Icons::One { icon: id.clone() }).is_ok() && !self.added.contains(&id) {
                self.added.push(id);
            }
        }
        self
    }

    /// Reads the image files `icons` names that aren't known yet, and decodes them to draw, or
    /// says why one can't be read.
    pub fn adopt(&mut self, icons: &Icons) -> Result<(), String> {
        let Some(manifest) = &mut self.manifest else { return Err("the icons can't be read".into()) };
        let added = manifest.adopt(icons)?;
        if let Some(cache) = &self.cache {
            for id in added {
                let bytes = manifest.icon(&id).map(|i| i.bytes.clone()).unwrap_or_default();
                self.images.push((id, cache.load(bytes)));
            }
        }
        Ok(())
    }

    /// Follows the theme's icon `theme` from now on; shown at once when it's another.
    pub fn follow(&mut self, theme: Icons) {
        if theme != self.theme {
            self.theme = theme;
            (self.show)(IconChange::Theme(&self.theme));
        }
    }

    /// Decodes each icon to draw, off the main thread: the picker shows them as they come. The
    /// cache stays, for the images a well takes later.
    pub fn load_images(&mut self, cache: Rc<rune::ImageCache>) {
        if let Some(manifest) = &self.manifest {
            self.images = manifest.everyone().map(|icon| (icon.id.clone(), cache.load(icon.bytes.clone()))).collect();
        }
        self.cache = Some(cache);
    }

    pub fn image(&self, id: &str) -> Option<&rune::Image> {
        self.images.iter().find(|(i, _)| i == id).map(|(_, image)| image)
    }

    /// The id of the icon Follow theme shows in the Appearance `dark` says: the theme's, else the
    /// default's.
    pub fn theme_icon(&self, dark: bool) -> Option<String> {
        let manifest = self.manifest.as_ref()?;
        Some(crate::platform::app_icon::resolve(None, manifest, &self.theme, dark).id.clone())
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
    // The status bar's line holds no name the user chose (a saved Look Preset's): those are the list's.
    let lines: Vec<String> = reset.iter().map(|name| crate::theme::preset::reset_line(name).unwrap_or_else(|| name.clone())).collect();
    if lines.is_empty() {
        return None;
    }
    // The list names every setting reset, as Settings labels it, a saved Look Preset by its name.
    let names: Vec<String> = reset
        .iter()
        .zip(&lines)
        .map(|(name, line)| crate::theme::preset::reset_label(name).unwrap_or_else(|| display::setting_label(line).to_owned()))
        .collect();
    let copy = proto::tolerant::invalid_copy(&proto::data_dir().join(file));
    let name = copy.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let reveal = NoticeAction::reveal(&copy);
    let reset = reset.to_vec();
    let open = NoticeAction::new("Open settings", move |s: &mut Phantom| {
        let reset = reset.clone();
        s.settings.post(move |w, _| open_reset(w, &reset));
    });
    Some(
        Notice::warning(display::settings_reset_line())
            .with_listing(display::settings_reset_listing(&names, &name))
            .with_actions(vec![open, reveal]),
    )
}

/// Opens Settings, or raises it, on the tab of the first setting in `reset`, focusing the control of
/// the first one reset there, in the order the tab shows them.
pub fn open_reset(s: &mut SettingsWindow, reset: &[String]) {
    use crate::views::settings::{SETTINGS, setting_of, tab_of};
    let tab = reset.iter().find_map(|name| tab_of(name));
    open(s);
    if let (Some(sheet), Some(tab)) = (&mut s.sheet, tab) {
        sheet.tab = tab;
        sheet.chip = None;
        sheet.focus = SETTINGS.into_iter().find(|name| tab_of(name) == Some(tab) && reset.iter().any(|r| setting_of(r) == *name));
    }
}

/// Brings Settings to the front; closed, it opens, asking the service for its settings.
pub fn open(s: &mut SettingsWindow) {
    s.window.raise();
    if s.sheet.is_some() {
        return;
    }
    s.sheet = Some(SettingsSheet::new());
    s.main.post(|s, _| s.service.settings(answer(Row::All)));
}

pub fn show_tab(s: &mut SettingsWindow, tab: Tab) {
    if let Some(sheet) = &mut s.sheet {
        sheet.tab = tab;
    }
}

/// The sheet over the window closes (Escape, ⌘W): a Look Preset's, or the App icon's.
pub fn close_over(s: &mut SettingsWindow) {
    presets::close(s);
    close_icons(s);
}

/// ⌘W or Escape: the window closes ([`closed`]).
pub fn close(s: &mut SettingsWindow) {
    closed(s);
    s.window.close();
}

/// The window closed, also by its close button: the field being edited is applied first.
pub fn closed(s: &mut SettingsWindow) {
    // A Look Preset's sheet keeps what was done in it, as Done does.
    presets::close(s);
    leave_all(s);
    s.sheet = None;
    // A saved preset's sheet goes with it, and so does its preview.
    presets::preview(s);
}

/// The window lost the front, to the main window or another app: that leaves the field being edited,
/// which applies it (#254), so whatever the other window does next comes after it.
pub fn left_window(s: &mut SettingsWindow) {
    leave_all(s);
}

/// Called before every frame: tells the main window when a field starts or stops being edited, so
/// Edit ▸ Undo is the field's own then ([`crate::changes::menu_title`]).
pub fn tell_editing(s: &mut SettingsWindow) {
    let editing = s.sheet.as_ref().is_some_and(|sheet| sheet.editing(&s.looks.borrow(), &s.saved));
    if editing != s.told_editing {
        s.told_editing = editing;
        s.main.post(move |s, _| s.settings_editing = editing);
    }
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
        Row::All | Row::Folder | Row::Switch(_) | Row::PresetName | Row::Image => return None,
    })
}

/// Makes the change: sent to the service, or set in the theme.
fn apply(s: &mut SettingsWindow, row: Row, change: Change) {
    match (change, row) {
        (Change::Send(waiting, setting), _) => send_field(s, waiting, setting),
        (Change::Colour(rgb), Row::Colour(side, key)) => {
            if let Some(sheet) = &mut s.sheet {
                sheet.refused.retain(|r| r.row != row);
                if let Some(field) = sheet.editor_mut().and_then(|e| e.colour_mut(side, key)) {
                    field.text = TextEdit::new(rgb.hex());
                }
            }
            set_theme(s, side, key, Value::Colour(rgb));
        }
        (Change::Colour(_), _) => {}
    }
}

/// Return in a text field: its value applied, or why it can't be, under its row, the text kept.
pub fn submit(s: &mut SettingsWindow, row: Row) {
    let Some(sheet) = &mut s.sheet else { return };
    match change_of(sheet, row) {
        Some(Ok(change)) => apply(s, row, change),
        Some(Err(why)) => sheet.refuse(row, why),
        None => {}
    }
}

/// Sends the port field's port, or says why its text isn't one.
pub fn submit_port(s: &mut SettingsWindow) {
    submit(s, Row::Port);
}

/// Sends a limit's field: its speed, or unlimited when it's empty; or says why its text isn't one.
pub fn submit_limit(s: &mut SettingsWindow, direction: Direction) {
    submit(s, Row::Limit(direction));
}

/// Sends a seeding limit's field: its ratio or time, or never when it's empty; or says why its
/// text isn't one.
pub fn submit_seeding(s: &mut SettingsWindow, at: SeedingLimit) {
    submit(s, Row::Seeding(at));
}

/// Sets a colour field's colour, or says why its text isn't one.
pub fn submit_colour(s: &mut SettingsWindow, side: Side, key: &'static str) {
    submit(s, Row::Colour(side, key));
}

/// Called before every frame: a text field that had the focus and hasn't now (Tab, a click
/// elsewhere) applies its text (#254).
pub fn follow_focus(s: &mut SettingsWindow) {
    let Some(sheet) = &mut s.sheet else { return };
    let now: Vec<Row> = sheet.text_rows().into_iter().filter(|&row| sheet.field(row).is_some_and(|f| f.focused)).collect();
    let left: Vec<Row> = sheet.had_focus.iter().copied().filter(|row| !now.contains(row)).collect();
    sheet.had_focus = now;
    for row in left {
        leave(s, row);
    }
}

/// The focus leaves every text field, as closing the sheet or another window coming to the front
/// takes it (a tab switch or a click elsewhere takes it first, and [`follow_focus`] sees that).
fn leave_all(s: &mut SettingsWindow) {
    let Some(sheet) = &mut s.sheet else { return };
    let rows: Vec<Row> = sheet
        .text_rows()
        .into_iter()
        .filter(|&row| sheet.had_focus.contains(&row) || sheet.field(row).is_some_and(|f| f.focused))
        .collect();
    sheet.had_focus.clear();
    for row in rows {
        if let Some(sheet) = &mut s.sheet
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
fn leave(s: &mut SettingsWindow, row: Row) {
    let Some(sheet) = &mut s.sheet else { return };
    let (Some(stored), Some(field)) = (sheet.stored_text(row, &s.looks.borrow(), &s.saved), sheet.field(row)) else { return };
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

/// Escape: in a field being edited, its text goes, the stored value shows again, and the field
/// keeps the focus; else the Look Preset's sheet closes; else Save as…, on its tab, is cancelled (its
/// question first, back to the name). Whether it did one; else Escape closes the window.
pub fn escape(s: &mut SettingsWindow) -> bool {
    let Some(sheet) = &mut s.sheet else { return false };
    if let Some(row) = sheet.edited(&s.looks.borrow(), &s.saved) {
        // Save as…'s name has nothing stored: it's cancelled.
        if row == Row::PresetName {
            sheet.save_as = None;
            sheet.refused.retain(|r| r.row != row);
            return true;
        }
        let stored = sheet.stored_text(row, &s.looks.borrow(), &s.saved).unwrap_or_default();
        let mut field = TextEdit::new(stored);
        field.focused = true;
        *sheet.field_mut(row).expect("a text row has a field") = field;
        sheet.refused.retain(|r| r.row != row);
        return true;
    }
    // A Look Preset's sheet is cancelled; the App icon's closes.
    if sheet.edit.is_some() {
        presets::cancel(s);
        return true;
    }
    if sheet.icons {
        close_icons(s);
        return true;
    }
    // Save as… shows on its tab only.
    match sheet.save_as.as_mut().filter(|_| sheet.tab == Tab::Appearance) {
        Some(save_as) if save_as.replace.is_some() => {
            save_as.replace = None;
            save_as.name.focused = true;
        }
        Some(_) => {
            sheet.save_as = None;
            sheet.refused.retain(|r| r.row != Row::PresetName);
        }
        None => return false,
    }
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
pub fn pick_port_automatically(s: &mut SettingsWindow) {
    ask(s, Waiting::Port, Setting::ListenPort(None));
}

/// Flips the setting from what its switch shows, so a second toggle before the first is answered
/// asks the opposite of the first.
pub fn toggle(s: &mut SettingsWindow, switch: Switch) {
    let Some(on) = s.sheet.as_ref().and_then(|sheet| sheet.switch(switch)) else { return };
    ask(s, Waiting::Switch(switch, !on), switch.set(!on));
}

/// Asks for a folder, starting at the current one, and sets it as the default destination.
pub fn choose_folder(s: &mut SettingsWindow, tx: &mut Tx) {
    let Some(sheet) = &s.sheet else { return };
    let current = sheet.service.as_ref().and_then(|settings| settings.default_destination.clone());
    let directory = current.map(|folder| folder.into_path_buf()).or_else(downloads);
    let options = FileDialogOptions { title: "Choose the default download folder".into(), directory, ..Default::default() };
    tx.open_folder(options, |s: &mut SettingsWindow, chosen, _| {
        let folder = match chosen {
            Ok(Some(folder)) => client::absolute(&folder).map_err(|e| format!("{e:#}")),
            Ok(None) => return,
            Err(e) => Err(format!("can't ask for a folder: {e}")),
        };
        match folder {
            Ok(folder) => ask(s, Waiting::Folder, Setting::DefaultDestination(Some(folder))),
            Err(why) => {
                if let Some(sheet) = &mut s.sheet {
                    sheet.refuse(Row::Folder, display::sentence(&why));
                }
            }
        }
    });
}

/// No default destination: every add asks where.
pub fn clear_folder(s: &mut SettingsWindow) {
    ask(s, Waiting::Folder, Setting::DefaultDestination(None));
}

/// Shows and keeps `choice`; `None` follows the theme. Another than chosen is a change to undo.
pub fn choose_icon(s: &mut SettingsWindow, choice: Option<Icons>) {
    follow_focus(s);
    if choice != s.app_icon.choice {
        let was = crate::changes::Was::Icon(s.app_icon.choice.clone());
        app_changed(s, crate::changes::machine::AppSetting::Icon, was);
    }
    show_icon(s, choice);
}

/// The app setting `of` changed, from `was`: a change ⌘Z takes back, on the main window's list.
pub(crate) fn app_changed(s: &SettingsWindow, of: crate::changes::machine::AppSetting, was: crate::changes::Was) {
    s.main.post(move |s, _| crate::changes::app_changed(s, of, was));
}

/// ⌘Z undid the app setting `of`, which was `was`: it's back, shown here at its control, opened if
/// Settings was closed, and the status bar tells what it went back to. A theme setting shows in
/// the sheet of the Look Preset it's of, opened if it was closed.
pub fn undo_app(s: &mut SettingsWindow, of: crate::changes::machine::AppSetting, was: crate::changes::Was, tx: &mut Tx) {
    use crate::changes::Was;
    use crate::changes::machine::AppSetting;
    let told = match (of, was) {
        (AppSetting::Icon, Was::Icon(choice)) => {
            let undone = icon_name(&s.app_icon, s.app_icon.choice.as_ref());
            show_icon(s, choice.clone());
            show_setting(s, "app_icon", Some(tx));
            Notice::setting_undone(display::setting_short("app_icon"), &undone, &icon_name(&s.app_icon, choice.as_ref()))
        }
        (AppSetting::Theme { key, label }, Was::Theme(was, target, side)) => {
            let value = |presets: &Presets| presets.get(&target).map(|p| theme_value(p, side, key)).unwrap_or_default();
            let undone = value(&Presets::of(&s.looks.borrow(), &s.saved));
            let back = value(&was);
            restore(s, was);
            // A saved Look Preset's name is the user's: the list's, not the line's.
            let (named, of) = match &target {
                Target::Active => (theme_named(label, side), None),
                Target::Saved(name) => (display::of_a_saved_preset(&theme_named(label, side)), Some(name.as_str().to_owned())),
            };
            presets::show_setting(s, target, key, side, tx);
            let told = Notice::setting_undone(&named, &undone, &back);
            match of {
                Some(preset) => {
                    Notice { more: told.more.map(|list| list.then(vec![crate::notice::Line::Muted(format!("In {preset}"))])), ..told }
                }
                None => told,
            }
        }
        (AppSetting::Presets, Was::Presets(was, change)) => {
            restore(s, was);
            presets::show_change(s, &change, tx);
            Notice::new(display::look_preset_undone()).brief().with_names(change.words())
        }
        // An editor's Done: the Look Presets as it opened on them, shown at the Look Presets.
        (AppSetting::Edit, Was::Edit(was, target)) => {
            restore(s, was);
            show_setting(s, presets::SETTING, Some(tx));
            // A saved Look Preset's name is the user's: the list's, not the line's.
            match &target {
                Target::Active => Notice::new(display::setting_undone_short(&crate::changes::edit_words(&target))).brief(),
                Target::Saved(name) => {
                    Notice::new(display::setting_undone_short(&display::of_a_saved_preset("Edit"))).brief().with_names(name.as_str())
                }
            }
        }
        // A change's `was` is always of its setting (`app_changed`'s callers).
        (AppSetting::Icon, Was::Theme(..) | Was::Presets(..) | Was::Edit(..))
        | (AppSetting::Theme { .. }, Was::Icon(_) | Was::Presets(..) | Was::Edit(..))
        | (AppSetting::Presets, Was::Icon(_) | Was::Theme(..) | Was::Edit(..))
        | (AppSetting::Edit, Was::Icon(_) | Was::Theme(..) | Was::Presets(..)) => return,
    };
    s.main.post(move |s, _| s.bar.tell(told));
}

/// The service undid a change of `name`: the settings it answered with, its row showing the value
/// it went back to, and Settings at it, opened if it was closed.
pub fn undone(s: &mut SettingsWindow, name: SettingName, settings: Settings, tx: &mut Tx) {
    if let Some(sheet) = &mut s.sheet {
        sheet.observe(settings.clone());
        sheet.answered_undo(name, settings);
    }
    show_setting(s, name.key(), Some(tx));
}

/// Shows and keeps `choice`, as an undo does.
pub fn show_icon(s: &mut SettingsWindow, choice: Option<Icons>) {
    (s.app_icon.show)(IconChange::Chose(choice.as_ref()));
    s.app_icon.choice = choice;
}

/// The Active Look Preset and the saved ones back to `was`, as an undo does: each look it changes
/// rebuilt and kept, the App icon the looks give shown, and the fields showing the values they went
/// back to, whatever was typed in them. A saved preset's sheet whose preset is gone closes.
pub fn restore(s: &mut SettingsWindow, was: Presets) {
    for appearance in Appearance::BOTH {
        s.looks.borrow_mut().set(appearance, was.active.get(appearance).clone());
    }
    s.saved.set(was.saved);
    if let Some(sheet) = &mut s.sheet {
        if let Some(edit) = &mut sheet.edit {
            edit.editor.colours.clear();
        }
        sheet.refused.retain(|r| !matches!(r.row, Row::Colour(..)));
        // A saved preset that's gone has its sheet closed.
        if let Some(Target::Saved(name)) = sheet.target()
            && s.saved.get(name).is_none()
        {
            sheet.edit = None;
            s.main.post(|s, _| crate::changes::editor_closed(s, None));
        }
    }
    looks_changed(s);
}

/// How many characters of an icon's name [`icon_name`] shows when it names two.
const NAME_CHARS: usize = 24;

/// What the App icon `choice` is called: a designed pair's name; an icon's, when both halves show
/// it; else both, "Light · Dark" (the Light's first), each shortened in the middle when long.
/// "Follow theme" is a name like any other.
pub fn icon_name(icon: &AppIcon, choice: Option<&Icons>) -> String {
    named(icon, choice, NAME_CHARS)
}

/// [`icon_name`] with nothing shortened: what the row's caption says on hover.
pub fn icon_full_name(icon: &AppIcon, choice: Option<&Icons>) -> String {
    named(icon, choice, usize::MAX)
}

fn named(icon: &AppIcon, choice: Option<&Icons>, chars: usize) -> String {
    let manifest = icon.manifest.as_ref();
    let name = |id: &str| match id {
        FOLLOW => "Follow theme".to_owned(),
        id => manifest.and_then(|m| m.icon(id)).map_or(id.to_owned(), |i| i.name.clone()),
    };
    match choice {
        None => "Follow theme".into(),
        Some(icons) if let Some(pair) = manifest.and_then(|m| m.pairs().iter().find(|p| p.icons() == *icons)) => pair.name.clone(),
        Some(Icons::One { icon }) => name(icon),
        Some(Icons::Pair { dark, light }) if dark == light => name(light),
        Some(Icons::Pair { dark, light }) => {
            let short = |id: &str| crate::views::details::shorten_middle(&name(id), |t| t.chars().count() <= chars);
            format!("{} · {}", short(light), short(dark))
        }
    }
}

/// The theme setting `key`'s value in `preset` on `side` (Light's, for both), as an undo tells it:
/// a choice's label, on or off, a colour's hex. Split, the side's in the setting's name
/// ([`theme_named`]).
pub fn theme_value(preset: &LookPreset, side: Side, key: &str) -> String {
    let look = preset.get(side.appearances()[0]);
    match key {
        THEME => return look.theme.name().to_owned(),
        _ => {}
    }
    let Some((setting, value)) = look.settings.iter().find(|(s, _)| s.key == key) else { return String::new() };
    match (&setting.kind, value) {
        (crate::theme::setting::Kind::Choice { choices, .. }, Value::Choice(id)) => {
            choices.iter().find(|c| c.id == id).map_or(id, |c| c.label).to_owned()
        }
        (_, Value::Toggle(on)) => if on { "on" } else { "off" }.to_owned(),
        (_, Value::Colour(rgb)) => rgb.hex(),
        (_, Value::Choice(id)) => id.to_owned(),
    }
}

/// A theme setting `label`, as an undo set on `side` tells it: "Tone"; split, "Tone (Light)".
pub fn theme_named(label: &str, side: Side) -> String {
    match side {
        Side::Both => label.to_owned(),
        Side::Only(appearance) => format!("{label} ({})", appearance.label()),
    }
}

/// Sets the theme setting `key` to `value` in the Theme Looks of `side` of what the sheet edits,
/// each of whose theme has it: the Active look (rebuilding the look of each half it changes, kept,
/// and showing the App icon the looks now give) or a saved preset (kept, and shown in its preview).
/// Set in one half's column, its row stays split.
pub fn set_theme(s: &mut SettingsWindow, side: Side, key: &'static str, value: Value) {
    follow_focus(s);
    let Some(target) = s.sheet.as_ref().and_then(|sheet| sheet.target().cloned()) else { return };
    let was = Presets::of(&s.looks.borrow(), &s.saved);
    let Some(mut preset) = was.get(&target).cloned() else { return };
    let label =
        side.appearances().iter().find_map(|a| preset.get(*a).settings.iter().find(|(st, _)| st.key == key)).map(|(st, _)| st.label);
    for &appearance in side.appearances() {
        preset.get_mut(appearance).settings.set(key, value);
    }
    keep_edited(s, &target, preset);
    if let (Side::Only(_), Some(editor)) = (side, s.sheet.as_mut().and_then(|sheet| sheet.editor_mut())) {
        editor.split.insert(key);
    }
    if let Some(label) = label {
        edited(s, was, target, side, key, label);
    }
}

/// The looks may have changed: the App icon follows the Active look, the colour fields show what
/// they hold, and a saved preset's sheet shows it over both windows. The main window draws in them
/// once it runs what's posted with the change: the change itself, for the undo list, or what its
/// undo went back to, for the status bar.
fn looks_changed(s: &mut SettingsWindow) {
    let icons = s.looks.borrow().app_icons();
    s.app_icon.follow(icons);
    let presets = Presets::of(&s.looks.borrow(), &s.saved);
    if let Some(sheet) = &mut s.sheet {
        let preset = sheet.target().and_then(|t| presets.get(t).cloned());
        if let (Some(editor), Some(preset)) = (sheet.editor_mut(), preset) {
            editor.show_colours(&preset);
        }
    }
    presets::preview(s);
}

/// The key a change of a half's theme is undone by, and its name.
pub const THEME: &str = "theme";
pub const THEME_LABEL: &str = "Theme";

/// `preset` is now what the sheet's `target` holds: the Active look's halves (each look it changes
/// rebuilt and kept), or the saved preset's (kept).
fn keep_edited(s: &mut SettingsWindow, target: &Target, preset: LookPreset) {
    match target {
        Target::Active => {
            for appearance in Appearance::BOTH {
                s.looks.borrow_mut().set(appearance, preset.get(appearance).clone());
            }
        }
        Target::Saved(name) => {
            let list = s
                .saved
                .list()
                .iter()
                .map(|p| if p.name == *name { SavedPreset { preset: preset.clone(), ..p.clone() } } else { p.clone() });
            s.saved.set(list.collect());
        }
    }
}

/// After an edit in the sheet: the looks shown again, and, if the Look Presets aren't `was` any
/// more, a change ⌘Z takes back, named `label`.
fn edited(s: &mut SettingsWindow, was: Presets, target: Target, side: Side, key: &'static str, label: &'static str) {
    let changed = Presets::of(&s.looks.borrow(), &s.saved) != was;
    looks_changed(s);
    if changed {
        app_changed(s, crate::changes::machine::AppSetting::Theme { key, label }, crate::changes::Was::Theme(was, target, side));
    }
}

/// A row's Split: a control in each half's column from now on.
pub fn split_row(s: &mut SettingsWindow, key: &'static str) {
    follow_focus(s);
    if let Some(editor) = s.sheet.as_mut().and_then(|sheet| sheet.editor_mut()) {
        editor.split.insert(key);
        editor.menu = None;
    }
    looks_changed(s);
}

/// A row's Join: one control for both halves, the Light one's value (theme, for the Theme
/// row, one theme in both halves) copied into the Dark one where they differ, a change ⌘Z takes
/// back. With a different theme in each half no row has a Join: nothing joins.
pub fn join_row(s: &mut SettingsWindow, key: &'static str) {
    follow_focus(s);
    let Some(target) = s.sheet.as_ref().and_then(|sheet| sheet.target().cloned()) else { return };
    let Some(preset) = Presets::of(&s.looks.borrow(), &s.saved).get(&target).cloned() else { return };
    let light = preset.light.theme.id();
    match (key, light == preset.dark.theme.id()) {
        (_, false) => return,
        (THEME, true) => {}
        (_, true) => {
            let Some(value) = preset.light.settings.get(key) else { return };
            if preset.dark.settings.get(key) != Some(value) {
                set_theme(s, Side::Only(Appearance::Dark), key, value);
            }
        }
    }
    if let Some(editor) = s.sheet.as_mut().and_then(|sheet| sheet.editor_mut()) {
        editor.split.remove(key);
    }
    looks_changed(s);
}

/// The Theme pop-up of `side`, opened or closed.
pub fn toggle_theme_menu(s: &mut SettingsWindow, side: Side) {
    follow_focus(s);
    if let Some(editor) = s.sheet.as_mut().and_then(|sheet| sheet.editor_mut()) {
        editor.menu = if editor.menu == Some(side) { None } else { Some(side) };
    }
}

pub fn close_theme_menu(s: &mut SettingsWindow) {
    if let Some(editor) = s.sheet.as_mut().and_then(|sheet| sheet.editor_mut()) {
        editor.menu = None;
    }
}

/// The theme `id` chosen in the Theme pop-up of `side`: each of its halves that shows another
/// theme starts at that theme's settings as a fresh install has them for that half (its defaults
/// where a fresh install's half is another theme), a change ⌘Z takes back. Chosen for one half
/// alone, the Theme row stays split.
pub fn choose_theme(s: &mut SettingsWindow, side: Side, id: &str) {
    follow_focus(s);
    close_theme_menu(s);
    let Some(theme) = crate::theme::find(id) else { return };
    let Some(target) = s.sheet.as_ref().and_then(|sheet| sheet.target().cloned()) else { return };
    let was = Presets::of(&s.looks.borrow(), &s.saved);
    let Some(mut preset) = was.get(&target).cloned() else { return };
    let fresh = LookPreset::fresh();
    for &appearance in side.appearances() {
        if preset.get(appearance).theme.id() == theme.id() {
            continue;
        }
        let settings = match fresh.get(appearance) {
            look if look.theme.id() == theme.id() => look.settings.clone(),
            _ => crate::theme::setting::Values::defaults(theme.settings()),
        };
        *preset.get_mut(appearance) = crate::theme::ThemeLook { theme, settings };
    }
    keep_edited(s, &target, preset);
    if let (Side::Only(_), Some(editor)) = (side, s.sheet.as_mut().and_then(|sheet| sheet.editor_mut())) {
        editor.split.insert(THEME);
    }
    edited(s, was, target, side, THEME, THEME_LABEL);
}

/// Why a colour isn't one.
pub const NOT_A_COLOUR: &str = "A colour is # and six hex digits, like #3366ff";

impl Editor {
    /// The field of the colour setting `key` on `side`.
    pub fn colour(&self, side: Side, key: &str) -> Option<&ColourField> {
        self.colours.iter().find(|f| f.side == side && f.key == key)
    }

    pub fn colour_mut(&mut self, side: Side, key: &str) -> Option<&mut ColourField> {
        self.colours.iter_mut().find(|f| f.side == side && f.key == key)
    }

    /// A field for each colour the sheet shows of `preset`, by its side: one still shown keeps
    /// what's typed in it and its refusal, a new one shows the colour it holds.
    pub fn show_colours(&mut self, preset: &LookPreset) {
        let mut fields = Vec::new();
        for (side, setting, value) in theme_rows(preset, self).controls() {
            let Value::Colour(rgb) = value else { continue };
            let field = match self.colours.iter().position(|f| f.side == side && f.key == setting.key) {
                Some(i) => self.colours.swap_remove(i),
                None => ColourField { side, key: setting.key, text: TextEdit::new(rgb.hex()) },
            };
            fields.push(field);
        }
        self.colours = fields;
    }
}

/// A theme setting with its value, in one half.
pub type Held = (&'static crate::theme::setting::Setting, Value);

/// What the editor sheet shows of a Look Preset, in two columns, ☀ Light and ☾ Dark.
pub enum ThemeRows {
    /// One theme in both halves: its Theme row joined (one pop-up) unless split by hand, and a
    /// row for each of its settings, each joined (one control for both) while its two values
    /// agree and it isn't split by hand, else split (one in each column).
    Same { theme_joined: bool, rows: Vec<ThemeRow> },
    /// A theme in each half: two lists that share no rows, each its half's Theme then its own
    /// theme's settings.
    Apart { light: Vec<Held>, dark: Vec<Held> },
}

/// A setting's row, with one theme in both halves.
pub struct ThemeRow {
    pub setting: &'static crate::theme::setting::Setting,
    pub light: Value,
    pub dark: Value,
    pub joined: bool,
}

impl ThemeRows {
    /// Every control it shows, by the side it sets: one for both of a joined row, one for each
    /// half of a split one, and each column's own.
    pub fn controls(&self) -> Vec<(Side, &'static crate::theme::setting::Setting, Value)> {
        let (light, dark) = (Side::Only(Appearance::Light), Side::Only(Appearance::Dark));
        match self {
            ThemeRows::Same { rows, .. } => rows
                .iter()
                .flat_map(|r| match r.joined {
                    true => vec![(Side::Both, r.setting, r.light)],
                    false => vec![(light, r.setting, r.light), (dark, r.setting, r.dark)],
                })
                .collect(),
            ThemeRows::Apart { light: l, dark: d } => {
                l.iter().map(|(s, v)| (light, *s, *v)).chain(d.iter().map(|(s, v)| (dark, *s, *v))).collect()
            }
        }
    }
}

/// Which of a setting's controls, on `sides`, takes the focus an undo of a change on `wanted`
/// gives: the one for both, if it's joined; else the one of that side, or, a change of both, the
/// first; none where only the other side has the setting.
pub fn focused_side(sides: &[Side], wanted: Side) -> Option<Side> {
    match (sides.contains(&Side::Both), wanted) {
        (true, _) => Some(Side::Both),
        (false, Side::Only(_)) => sides.contains(&wanted).then_some(wanted),
        (false, Side::Both) => sides.first().copied(),
    }
}

/// What the editor sheet shows of `preset` ([`ThemeRows`]).
pub fn theme_rows(preset: &LookPreset, editor: &Editor) -> ThemeRows {
    let (light, dark) = (&preset.light, &preset.dark);
    if light.theme.id() != dark.theme.id() {
        return ThemeRows::Apart { light: light.settings.iter().collect(), dark: dark.settings.iter().collect() };
    }
    let rows = light.settings.iter().zip(dark.settings.iter()).map(|((setting, l), (_, d))| ThemeRow {
        setting,
        light: l,
        dark: d,
        joined: l == d && !editor.split.contains(setting.key),
    });
    ThemeRows::Same { theme_joined: !editor.split.contains(THEME), rows: rows.collect() }
}

/// The App icon's Edit…: its sheet over Settings.
pub fn edit_icons(s: &mut SettingsWindow) {
    follow_focus(s);
    if let Some(sheet) = &mut s.sheet {
        sheet.icons = true;
    }
}

/// Done in the App icon's sheet, Escape or ⌘W: it closes.
pub fn close_icons(s: &mut SettingsWindow) {
    if let Some(sheet) = &mut s.sheet {
        sheet.icons = false;
        sheet.refused.retain(|r| r.row != Row::Image);
    }
}

/// Each half of the App icon `choice`, `[dark, light]`: an icon's id, or `None` following the
/// theme.
pub fn halves(choice: Option<&Icons>) -> [Option<String>; 2] {
    let id = |id: &String| (id != FOLLOW).then(|| id.clone());
    match choice {
        None => [None, None],
        Some(Icons::One { icon }) => [id(icon), id(icon)],
        Some(Icons::Pair { dark, light }) => [id(dark), id(light)],
    }
}

/// The App icon choice with these halves, `[dark, light]`: both following the theme is no
/// choice, the same in both one icon.
pub fn choice_of(halves: [Option<String>; 2]) -> Option<Icons> {
    let id = |half: Option<String>| half.unwrap_or_else(|| FOLLOW.to_owned());
    match halves {
        [None, None] => None,
        [dark, light] if dark == light => Some(Icons::One { icon: id(dark) }),
        [dark, light] => Some(Icons::Pair { dark: id(dark), light: id(light) }),
    }
}

/// What a chip under an icon's tile (`id`; `None`, Follow theme's) chooses: ◐ (`half` `None`) it in
/// both halves, ☀ in Light and ☾ in Dark, the other half as it is.
pub fn chip_choice(icon: &AppIcon, id: Option<&str>, half: Option<Appearance>) -> Option<Icons> {
    let mut halves = halves(icon.choice.as_ref());
    let id = id.map(str::to_owned);
    match half {
        None => halves = [id.clone(), id],
        Some(Appearance::Dark) => halves[0] = id,
        Some(Appearance::Light) => halves[1] = id,
    }
    choice_of(halves)
}

/// Whether a chip under an icon's tile (`id`; `None`, Follow theme's) is in use: ◐ while both
/// halves are it, else ☀ while the Light half is and ☾ while the Dark half is.
pub fn chip_in_use(icon: &AppIcon, id: Option<&str>, half: Option<Appearance>) -> bool {
    let [dark, light] = halves(icon.choice.as_ref());
    let is = |h: &Option<String>| h.as_deref() == id;
    let both = is(&dark) && is(&light);
    match half {
        None => both,
        Some(Appearance::Light) => !both && is(&light),
        Some(Appearance::Dark) => !both && is(&dark),
    }
}

/// An icon's ◐, ☀ or ☾ chip (`id`; `None`, Follow theme's): the App icon [`chip_choice`] gives, a
/// change ⌘Z takes back.
pub fn choose_chip(s: &mut SettingsWindow, id: Option<&str>, half: Option<Appearance>) {
    let choice = chip_choice(&s.app_icon, id, half);
    choose_icon(s, choice);
}

/// A designed pair's tile: both its icons, a change ⌘Z takes back.
pub fn choose_pair(s: &mut SettingsWindow, i: usize) {
    let Some(pair) = s.app_icon.manifest.as_ref().and_then(|m| m.pairs().get(i)).cloned() else { return };
    choose_icon(s, Some(pair.icons()));
}

/// Whether the designed pair `i` is in use: both halves are its icons.
pub fn pair_in_use(icon: &AppIcon, i: usize) -> bool {
    let pair = icon.manifest.as_ref().and_then(|m| m.pairs().get(i));
    pair.is_some_and(|p| halves(icon.choice.as_ref()) == [Some(p.dark.clone()), Some(p.light.clone())])
}

/// "+ Add image…": a file dialog for an image, which becomes an icon of its own ([`add_image`]).
pub fn choose_image(s: &mut SettingsWindow, tx: &mut Tx) {
    follow_focus(s);
    let options =
        FileDialogOptions { title: "Add an image as an App icon".into(), ..Default::default() }.filter("Images", &crate::ui::IMAGES);
    tx.open_file(options, move |s: &mut SettingsWindow, chosen, _| match chosen {
        Ok(Some(path)) => add_image(s, &path),
        Ok(None) => {}
        Err(e) => {
            if let Some(sheet) = &mut s.sheet {
                sheet.refuse(Row::Image, display::sentence(&format!("can't ask for a file: {e}")));
            }
        }
    });
}

/// The image file at `path` (chosen, or dropped on the sheet) becomes an icon of its own, a tile
/// after the others, kept; added already, nothing. One that can't be read: why, under the tiles.
pub fn add_image(s: &mut SettingsWindow, path: &Path) {
    follow_focus(s);
    let id = format!("{}{}", crate::platform::app_icon::FILE, path.display());
    if let Err(why) = s.app_icon.adopt(&Icons::One { icon: id.clone() }) {
        if let Some(sheet) = &mut s.sheet {
            sheet.refuse(Row::Image, display::sentence(&why));
        }
        return;
    }
    if let Some(sheet) = &mut s.sheet {
        sheet.refused.retain(|r| r.row != Row::Image);
    }
    if !s.app_icon.added.contains(&id) {
        s.app_icon.added.push(id);
        let icon = &mut s.app_icon;
        (icon.show)(IconChange::Added(&icon.added));
    }
}

/// An added image's ×: its tile goes. A half showing it follows the theme from then, never the
/// other half's icon: a change ⌘Z takes back (the tile stays gone).
pub fn remove_image(s: &mut SettingsWindow, id: &str) {
    follow_focus(s);
    s.app_icon.added.retain(|added| added != id);
    let icon = &mut s.app_icon;
    (icon.show)(IconChange::Added(&icon.added));
    let choice = choice_of(halves(s.app_icon.choice.as_ref()).map(|half| half.filter(|h| h != id)));
    if choice != s.app_icon.choice {
        choose_icon(s, choice);
    }
}

/// Return in a field: `setting` sent, unless it's what's stored and no change of the row is on its
/// way, when the field only shows it again: nothing changes, so there's no step for ⌘Z to take.
fn send_field(s: &mut SettingsWindow, waiting: Waiting, setting: Setting) {
    let Some(sheet) = &mut s.sheet else { return };
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
fn ask(s: &mut SettingsWindow, waiting: Waiting, setting: Setting) {
    // A field this change took the focus from is applied first: the order a person made them in.
    follow_focus(s);
    if let Some(sheet) = &mut s.sheet {
        sheet.waiting.push(waiting);
    }
    s.main.post(move |s, _| {
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
    });
}

/// The setting a row of the sheet changes, by its key in the settings file; none for a row that
/// isn't one setting's (reading them all, a Look Preset's field).
fn row_setting(row: Row) -> Option<&'static str> {
    Some(match row {
        Row::Folder => "default_destination",
        Row::Port => "listen_port",
        Row::Switch(switch) => switch.name(),
        Row::Limit(Direction::Download) => "download_limit",
        Row::Limit(Direction::Upload) => "upload_limit",
        Row::Seeding(SeedingLimit::Ratio) => "seed_ratio",
        Row::Seeding(SeedingLimit::Time) => "seed_time",
        Row::All | Row::Colour(..) | Row::PresetName | Row::Image => return None,
    })
}

/// The answer to a change of `row`, or to opening: shown while the sheet's open. A refusal shows
/// under its row; a failure (the torrents didn't start again with a new port, say) also goes to
/// the status bar as an error, which outlives the sheet.
fn answer(row: Row) -> Done<Result<Settings, Failure>> {
    Box::new(move |s: &mut Phantom, result, _| {
        if let Err(failure @ (Failure::Failed(_) | Failure::Error(_))) = &result {
            let of = row_setting(row).map(|key| display::setting_short(key).to_owned()).unwrap_or_default();
            let why = display::sentence(&format!("{failure:#}"));
            s.bar.tell(Notice::error(display::setting_not_changed()).with_names_and_why(&format!("{of}\n{why}")));
        }
        s.settings.post(move |s, _| {
            let Some(sheet) = &mut s.sheet else { return };
            if let Some(at) = sheet.waiting.iter().position(|w| w.row() == row) {
                sheet.waiting.remove(at);
            }
            match result {
                Ok(settings) => sheet.answered(row, settings),
                Err(e) => sheet.refuse(row, display::sentence(&format!("{e:#}"))),
            }
        });
    })
}

/// Where the folder picker starts without a default destination: `~/Downloads`.
fn downloads() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join("Downloads"))
}
