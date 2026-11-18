//! Undo (#221, #255): ⌘Z, in the main window or in Settings, takes back the newest change, whoever
//! made it, the app or another client: a torrent action (Remove, Delete, Move to Trash) or a
//! settings change (Return in a field, a switch, a folder, an App icon or a theme setting), one per
//! step. [`machine`] keeps the order and says which change ⌘Z takes; this carries it out. An app
//! setting's change (the App icon, a theme setting, in `gui.json`) the app undoes itself, from what
//! it was before, kept here in memory; a service setting's it asks the service to undo, which keeps
//! its own changes and never the app's; a torrent action's it asks the service to undo, which keeps
//! its undo stack. A setting's undo opens Settings at its tab, focuses its control, and tells what it
//! went back to ("Port is back to 6881"); a refused one says "Couldn't be undone: <why>" and is gone.
//! A torrent action's undo brings it back in the list, behind the sheet if it's open, and tells
//! what came back. Edit ▸ Undo names the change it would take ([`menu_title`]). No redo; quitting
//! the app forgets its settings changes (the service keeps the torrents' stack).

pub mod machine;

use std::collections::BTreeMap;

use client::{Failure, display};
use proto::{Configured, Refusal, SettingName, SettingUndone};
use rune::Tx;

use crate::model::Phantom;
use crate::model::settings::{Side, icon_name, show_setting, theme_named, theme_value};
use crate::notice::Notice;
use crate::platform::app_icon::Icons;
use crate::theme::preset::LookPreset;
use machine::{AppSetting, Command, Do, Event, Observed, Of};

/// The app's side of the settings changes.
pub struct Changes {
    pub state: machine::State,
    /// What each app setting's change was before, by its id.
    was: BTreeMap<u64, Was>,
    /// The next id for a change the app makes: of its own setting, or sent to the service.
    next: u64,
}

/// What an app setting was before a change.
#[derive(Clone)]
pub enum Was {
    Icon(Option<Icons>),
    /// The Active Look Preset, and the side the theme setting was set on.
    Theme(LookPreset, Side),
}

impl Default for Changes {
    fn default() -> Self {
        // From now in nanoseconds, so an id is never one an earlier run of the app gave a change
        // the service still lists.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Self { state: machine::State::default(), was: BTreeMap::new(), next: now }
    }
}

impl Changes {
    fn decide(&mut self, event: Event) -> Result<(), machine::Nothing> {
        self.state = machine::decide(&self.state, &event)?;
        Ok(())
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

/// The app setting `of` changed; it was `was`.
pub fn app_changed(s: &mut Phantom, of: AppSetting, was: Was) {
    let c = &mut s.changes;
    let id = c.id();
    c.was.insert(id, was);
    let _ = c.decide(Event::Command(Command::Changed { id, of }));
    // Past the last 20, an app change's `was` goes with it.
    c.was.retain(|id, _| c.state.list.iter().any(|ch| matches!(ch, machine::Change::App { id: i, .. } if i == id)));
}

/// A change of the service setting `name` is being sent: the id it's sent by.
pub fn sent(s: &mut Phantom, name: SettingName) -> u64 {
    let by = s.changes.id();
    let _ = s.changes.decide(Event::Command(Command::Sent { by, name }));
    by
}

/// The answer to the change sent `by` came: the change it made, if it made one (refused, or the
/// value was already that, none).
pub fn answered(s: &mut Phantom, by: u64, set: Option<&proto::SettingSet>) {
    let (seq, at) = set.map_or((None, 0), |set| (set.change.as_ref().map(|c| c.seq), set.at));
    let _ = s.changes.decide(Event::Observed(Observed::Answered { by, seq, at }));
    run(s, None);
}

/// The settings a snapshot carries: newer than the app has (the machine says), they're its
/// settings, shown at once.
pub fn observe(s: &mut Phantom, configured: Configured) {
    let before = s.changes.state.at;
    let _ = s.changes.decide(Event::Observed(Observed::Listed { at: configured.at, changes: configured.changes }));
    if s.changes.state.at != before
        && let Some(sheet) = &mut s.settings
    {
        sheet.observe(configured.settings);
    }
    run(s, None);
}

/// The service's undo stack, as a snapshot newer than any the app took lists it (`at`, its epoch
/// and version): the torrent actions ⌘Z takes join the list as the app learns of them.
pub fn observe_stack(s: &mut Phantom, at: (u64, u64), stack: &[proto::Undoable]) {
    let entries = stack.iter().map(|u| u.seq).collect();
    let _ = s.changes.decide(Event::Observed(Observed::Stack { at, entries }));
}

/// ⌘Z, in the main window or in Settings: the newest change, whoever made it; with none, nothing.
pub fn press(s: &mut Phantom, tx: &mut Tx) {
    if s.changes.decide(Event::Command(Command::Press)).is_ok() {
        run(s, Some(tx));
    }
}

/// The Undo in the notice of the torrent action `seq`: that one, while ⌘Z hasn't taken it.
pub fn press_action(s: &mut Phantom, seq: u64) {
    if s.changes.decide(Event::Command(Command::PressAction { seq })).is_ok() {
        run(s, None);
    }
}

/// What Edit ▸ Undo says: the change ⌘Z would take back, "Undo Port Change", "Undo Remove of
/// Album"; else "Undo", as while a field of Settings is being edited, whose own undo ⌘Z then is.
pub fn menu_title(s: &Phantom) -> String {
    let editing = s.settings.as_ref().is_some_and(|sheet| sheet.editing(&s.looks));
    let named = match s.changes.state.top().map(machine::Change::of) {
        _ if editing => None,
        Some(Of::Action(seq)) => s.undo.iter().find(|u| u.seq == seq).map(display::undo_entry),
        Some(Of::Service(name)) => Some(display::undo_change(display::setting_short(name.key()))),
        Some(Of::App(AppSetting::Icon)) => Some(display::undo_change(display::setting_short("app_icon"))),
        Some(Of::App(AppSetting::Theme { label, .. })) => Some(display::undo_change(label)),
        None => None,
    };
    named.unwrap_or_else(|| display::undo().to_owned())
}

/// Carries out what the machine says, until it says nothing.
fn run(s: &mut Phantom, mut tx: Option<&mut Tx>) {
    while let Some(todo) = machine::next(&s.changes.state) {
        match todo {
            Do::Undo { id } => {
                let of = s.changes.state.list.iter().find_map(|c| match c {
                    machine::Change::App { id: i, of, .. } if *i == id => Some(*of),
                    machine::Change::App { .. }
                    | machine::Change::Sent { .. }
                    | machine::Change::Listed { .. }
                    | machine::Change::Action { .. } => None,
                });
                if let (Some(was), Some(of)) = (s.changes.was.remove(&id), of) {
                    undo_app(s, of, was, tx.as_deref_mut());
                }
                let _ = s.changes.decide(Event::Observed(Observed::AppUndone { id }));
            }
            Do::Ask { seq } => {
                let _ = s.changes.decide(Event::Command(Command::Asked { seq }));
                s.service.undo_setting(Box::new(move |s: &mut Phantom, undone, tx: &mut Tx| undid(s, seq, undone, tx)));
            }
            Do::AskAction { seq } => {
                let _ = s.changes.decide(Event::Command(Command::AskedAction { seq }));
                crate::model::ask_undo(
                    s,
                    seq,
                    Box::new(move |s: &mut Phantom, took, tx: &mut Tx| {
                        let _ = s.changes.decide(Event::Observed(Observed::ActionAnswered { asked: seq, took }));
                        run(s, Some(tx));
                    }),
                );
            }
        }
    }
}

/// The app setting `of` back to what it `was`: shown in Settings, opened if it was closed, kept,
/// and told.
fn undo_app(s: &mut Phantom, of: AppSetting, was: Was, tx: Option<&mut Tx>) {
    let told = match (of, was) {
        (AppSetting::Icon, Was::Icon(choice)) => {
            crate::model::settings::show_icon(s, choice.clone());
            display::back_to(display::setting_short("app_icon"), &icon_name(&s.app_icon, choice.as_ref()))
        }
        (AppSetting::Theme { key, label }, Was::Theme(preset, side)) => {
            crate::model::settings::show_preset(s, preset);
            let value = theme_value(&s.looks, side, key);
            display::back_to(&theme_named(label, side), &value)
        }
        // A change's `was` is always of its setting (`app_changed`'s callers).
        (AppSetting::Icon, Was::Theme(..)) | (AppSetting::Theme { .. }, Was::Icon(_)) => return,
    };
    let key = match of {
        AppSetting::Icon => "app_icon",
        AppSetting::Theme { key, .. } => key,
    };
    show_setting(s, key, tx);
    s.bar.tell(Notice::new(told));
}

/// The service's answer to the undo asked for `asked`.
fn undid(s: &mut Phantom, asked: u64, undone: Result<SettingUndone, Failure>, tx: &mut Tx) {
    match undone {
        Ok(SettingUndone { change, settings }) => {
            let _ = s.changes.decide(Event::Observed(Observed::Undid { asked, took: change.seq }));
            if let Some(sheet) = &mut s.settings {
                sheet.observe(settings.clone());
                sheet.answered_undo(change.name, settings.clone());
            }
            // From the main window too: Settings opens at it.
            show_setting(s, change.name.key(), Some(tx));
            s.bar.tell(Notice::new(display::setting_undone(change.name, &settings)));
        }
        Err(failure) => {
            let _ = s.changes.decide(Event::Observed(Observed::Refused { asked }));
            // Nothing there: another client undid it first, and the list follows.
            if !matches!(failure, Failure::Refused(Refusal::NothingToUndo)) {
                s.bar.tell(Notice::error(display::couldnt_undo(&display::sentence(&format!("{failure:#}")).to_lowercase())));
            }
        }
    }
    run(s, Some(tx));
}
