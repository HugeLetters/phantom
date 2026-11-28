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
use crate::commands::{Command, Context, Focus, LookSheet, Offer, OnSheet, Sheet};
use crate::model::palette::{Entry, Group, Host, Kind, Palette, Place, SettingRef};
use crate::theme::preset::{Appearance, LookPreset};
use crate::theme::saved::{Name, SavedPreset};
use crate::theme::setting::Value;
use crate::windows::Which;

impl SettingsWindow {
    /// What's open over the window, the topmost.
    fn over(&self) -> Sheet {
        match self.sheet.as_ref() {
            Some(sheet) if sheet.save_as.is_some() => Sheet::SaveAs,
            Some(sheet) if sheet.customize.is_some() => Sheet::Customize,
            Some(sheet) if sheet.icons.is_some() => Sheet::AppIcon,
            Some(sheet) if sheet.edit.is_some() => Sheet::Look,
            Some(_) | None => Sheet::Closed,
        }
    }

    /// The Look sheet's preset and its rows, while it's open: what its commands' `when` reads.
    fn look_sheet(&self) -> Option<LookSheet> {
        let (_, preset) = self.sheet_preset()?;
        let editor = self.sheet.as_ref()?.editor()?;
        let rows = match super::theme_rows(&preset, editor) {
            ThemeRows::Same { theme_joined, rows } => {
                let theme = (super::THEME, super::THEME_LABEL, theme_joined);
                Some(std::iter::once(theme).chain(rows.iter().map(|r| (r.setting.key, r.setting.label, r.joined))).collect())
            }
            ThemeRows::Apart { .. } => None,
        };
        Some(LookSheet { preset, rows })
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

    fn shown(&self) -> bool {
        self.sheet.is_some()
    }

    fn context(&self) -> Context<'_> {
        Context {
            tab: self.sheet.as_ref().map_or(Tab::default(), |sheet| sheet.tab),
            sheet: self.over(),
            look: self.look_sheet(),
            ..Context::empty(Which::Settings, Focus::List)
        }
    }

    /// The Look sheet's commands Enabled now: Done, Cancel, Restore Defaults, and a Split and a
    /// Join for each row it shows, the one that applies.
    fn commands(&self) -> Vec<Entry> {
        let ctx = self.context();
        let entry = |command: OnSheet, row: &str| Entry {
            kind: Kind::Action(Command::Sheet(command)),
            group: Group::Sheet,
            label: command.wording(row),
            synonyms: command.spec().synonyms,
            hint: None,
            icon: command.spec().icon,
            to: None,
            place: None,
            value: None,
            has_choices: false,
            selected: false,
            scope: Command::Sheet(command).scope(),
            tab: None,
        };
        let rows = ctx.look.as_ref().and_then(|look| look.rows.clone()).unwrap_or_default();
        let sheet = OnSheet::ALL.into_iter().filter(|c| !matches!(c, OnSheet::Split(_) | OnSheet::Join(_))).map(|c| (c, ""));
        let rows = rows.into_iter().flat_map(|(key, label, _)| [(OnSheet::Split(key), label), (OnSheet::Join(key), label)]);
        sheet.chain(rows).filter(|(c, _)| Command::Sheet(*c).offer(&ctx) == Offer::Enabled).map(|(c, row)| entry(c, row)).collect()
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

    fn run_command(&mut self, command: Command, _: Option<proto::AbsolutePath>, tx: &mut Tx) {
        let Command::Sheet(command) = command else { unreachable!("Settings offers only the sheet's commands") };
        (command.decl().run)(self, command, tx)
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
