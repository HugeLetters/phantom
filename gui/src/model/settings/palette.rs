//! The Settings window's side of the command palette (#434): ⌘P there lists the entries that apply
//! in Settings, from the same registry as the main window's (`model::palette`), and what it does is
//! done in place. This is only what differs from the main window ([`Host`]): the context, the sheet's
//! commands, and acting directly on the window instead of asking it.
//!
//! | Entry | Main window | Settings |
//! |---|---|---|
//! | A setting's jump (⌘Return) | opens Settings, the control focused | focuses the control where it is |
//! | A place, a tab | opens Settings there | goes there |
//! | An on/off setting (Return) | flips it, the status bar telling so with Undo | flips it: the control shows it, ⌘Z takes it back |
//! | The Look sheet's | none | Done, Cancel, Restore Defaults, Split, Join |
//!
//! With the Look sheet open the palette lists the sheet's commands and the Look's settings, which it
//! edits as the sheet's own rows do: previewed, kept by Done, put back by Cancel. What's behind the
//! sheet (the tabs, the other settings, the Look Presets) isn't listed: the sheet is modal.

use proto::Settings;
use rune::Tx;

use super::{SettingsWindow, Side, Switch, Tab, ThemeRows};
use crate::changes::{Presets, Target};
use crate::commands::{Command, Context, OnSheet, Sheet};
use crate::model::palette::{Entry, Group, Host, Kind, Palette, Place, SettingRef};
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::saved::{Name, SavedPreset};
use crate::theme::setting::Value;
use crate::windows::Which;

impl SettingsWindow {
    /// Whether ⌘P opens the palette: Settings is open, with no sheet over it but the Look's (the
    /// App icon's, its Customize and Save as… are small flows of their own).
    pub fn can_open_palette(&self) -> bool {
        self.sheet.as_ref().is_some_and(|sheet| sheet.icons.is_none() && sheet.customize.is_none() && sheet.save_as.is_none())
    }

    /// The preset the Look sheet edits, while it's open, with what it edits.
    fn sheet_preset(&self) -> Option<(Target, LookPreset)> {
        let target = self.sheet.as_ref()?.target()?.clone();
        let preset = Presets::of(&self.looks.borrow(), &self.saved).get(&target)?.clone();
        Some((target, preset))
    }
}

/// Show in Settings: the control of `of` focused, in place. A theme setting is of the Look `target`
/// (the one the open sheet edits, else the Active look).
pub fn show_setting_ref(s: &mut SettingsWindow, of: SettingRef, target: Target, tx: &mut Tx) {
    match of {
        SettingRef::Service(key) => super::show_service(s, key, tx),
        SettingRef::Theme { key, side } => super::presets::show_setting(s, target, key, side, tx),
        SettingRef::Presets => super::show_presets(s, tx),
    }
}

impl Host for SettingsWindow {
    fn palette(&self) -> &Palette {
        &self.palette
    }

    fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    fn context(&self) -> Context {
        let sheet = self.sheet.as_ref();
        Context {
            window: Which::Settings,
            tab: sheet.map_or(Tab::default(), |sheet| sheet.tab),
            sheet: if sheet.is_some_and(|sheet| sheet.edit.is_some()) { Sheet::Look } else { Sheet::Closed },
        }
    }

    /// The Look sheet's commands, while it's open: Done, Cancel, Restore Defaults where there's
    /// something to restore, and each row's Split or Join (with one theme in both halves).
    fn commands(&self) -> Vec<Entry> {
        let (Some((_, preset)), Some(editor)) = (self.sheet_preset(), self.sheet.as_ref().and_then(|sheet| sheet.editor())) else {
            return Vec::new();
        };
        let entry = |command: OnSheet, row: &str| Entry {
            kind: Kind::Action(Command::Sheet(command)),
            group: Group::Sheet,
            label: command.wording(row),
            synonyms: &[],
            hint: None,
            icon: command.spec().icon,
            to: None,
            place: None,
            value: None,
            has_choices: false,
            selected: false,
            applies: Command::Sheet(command).applies(),
            tab: None,
        };
        let mut out = vec![entry(OnSheet::Done, ""), entry(OnSheet::Cancel, "")];
        let (light, dark) = (Side::Only(Appearance::Light), Side::Only(Appearance::Dark));
        let one_theme = preset.light.theme.id() == preset.dark.theme.id();
        let sides: &[Side] = if one_theme { &[Side::Both] } else { &[light, dark, Side::Both] };
        out.extend(sides.iter().filter(|side| super::restorable(&preset, **side)).map(|side| entry(OnSheet::Restore(*side), "")));
        if let ThemeRows::Same { theme_joined, rows } = super::theme_rows(&preset, editor) {
            let row = |joined: bool, key: &'static str| if joined { OnSheet::Split(key) } else { OnSheet::Join(key) };
            out.push(entry(row(theme_joined, super::THEME), super::THEME_LABEL));
            out.extend(rows.iter().map(|r| entry(row(r.joined, r.setting.key), r.setting.label)));
        }
        out
    }

    fn service_settings(&self) -> Option<&Settings> {
        self.sheet.as_ref().and_then(|sheet| sheet.service.as_ref())
    }

    fn saved_presets(&self) -> Vec<SavedPreset> {
        self.saved.list().to_vec()
    }

    /// The preset the sheet edits, else the Active look.
    fn editing(&self) -> (Target, LookPreset) {
        self.sheet_preset().unwrap_or_else(|| (Target::Active, self.looks.borrow().preset().clone()))
    }

    fn heading_of(&self, group: Group) -> String {
        match (group, self.sheet.as_ref()) {
            (Group::Sheet, Some(sheet)) => match sheet.target() {
                Some(Target::Saved(name)) => name.as_str().to_owned(),
                Some(Target::Active) | None => "Active look".to_owned(),
            },
            (Group::Here, Some(sheet)) => sheet.tab.label().to_owned(),
            _ => String::new(),
        }
    }

    /// Both windows show it; with nothing to preview, the saved preset the sheet is open on (which
    /// it previews while it's open), else the Active look.
    fn show_preview(&self, shown: Option<&LookPreset>) {
        let baseline = match self.sheet.as_ref().and_then(|sheet| sheet.target()) {
            Some(Target::Saved(name)) => self.saved.get(name).map(|p| p.preset.clone()),
            Some(Target::Active) | None => None,
        };
        let want = shown.cloned().or(baseline);
        if self.looks.borrow().previewed() != want.as_ref() {
            self.looks.borrow_mut().preview(want.as_ref());
            // The main window draws it once it runs what's posted.
            self.main.post(|_, _| {});
        }
    }

    /// One palette at a time: it previews in both windows.
    fn opening(&mut self) {
        self.main.post(|m, _| {
            if m.palette.is_open() {
                crate::model::palette::close(m);
            }
        });
    }

    fn opened(&mut self) {}

    fn run_command(&mut self, command: Command, _: Option<proto::AbsolutePath>, _: &mut Tx) {
        let Command::Sheet(command) = command else { unreachable!("Settings offers only the sheet's commands") };
        match command {
            OnSheet::Done => super::presets::close(self),
            OnSheet::Cancel => super::presets::cancel(self),
            OnSheet::Restore(side) => super::restore_defaults(self, side),
            OnSheet::Split(key) => super::split_row(self, key),
            OnSheet::Join(key) => super::join_row(self, key),
        }
    }

    fn open_place(&mut self, place: Place, tx: &mut Tx) {
        match place {
            Place::Settings => {}
            Place::Tab(tab) => {
                super::close_over(self);
                super::show_tab(self, tab);
            }
            Place::EditLook => {
                super::show_tab(self, Tab::Appearance);
                super::presets::edit_active(self);
            }
            Place::AppIcon => super::show_icons(self, tx),
        }
    }

    fn show(&mut self, of: SettingRef, tx: &mut Tx) {
        let (target, _) = self.editing();
        show_setting_ref(self, of, target, tx);
    }

    fn flip_switch(&mut self, switch: Switch, _: &str) {
        super::toggle(self, switch);
    }

    fn set_theme(&mut self, side: Side, key: &'static str, value: Value) {
        match self.sheet.as_ref().and_then(|sheet| sheet.target()) {
            Some(_) => super::set_theme(self, side, key, value),
            None => super::set_active_theme(self, side, key, value),
        }
    }

    fn choose_theme(&mut self, side: Side, id: &'static str) {
        match self.sheet.as_ref().and_then(|sheet| sheet.target()) {
            Some(_) => super::choose_theme(self, side, id),
            None => super::choose_active_theme(self, side, id),
        }
    }

    fn apply_preset(&mut self, name: Name, half: Option<Appearance>) {
        super::presets::apply_named(self, &name, half);
    }

    /// Settings has no status bar: the control that changed shows it, and ⌘Z takes it back.
    fn tell(&mut self, _: String) {}

    fn undo(&mut self) {
        self.main.post(|m, _| crate::changes::press(m));
    }
}
