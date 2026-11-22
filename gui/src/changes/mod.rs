//! Undo (#221, #255): ⌘Z, in the main window or in Settings, takes back the newest change, whoever
//! made it, the app or another client: a torrent action (Remove, Delete, Move to Trash) or a
//! settings change (Return in a field, a switch, a folder, an App icon, a theme setting, or a pick
//! from, or a change of, the saved Look Presets), one per
//! step. [`machine`] keeps the order and says which change ⌘Z takes; this carries it out. An app
//! setting's change (the App icon, a theme setting, in `gui.json`) the app undoes itself, from what
//! it was before, kept here in memory; a service setting's it asks the service to undo, which keeps
//! its own changes and never the app's; a torrent action's it asks the service to undo, which keeps
//! its undo stack. A setting's undo opens Settings at its tab, focuses its control, and tells what it
//! went back to ("Port is back to 6881"); a refused one says "Couldn't undo the change" and is gone.
//! A torrent action's undo brings it back in the list, behind Settings if it's open, and tells
//! what came back. The list is the main window's; ⌘Z in Settings is posted to it, and what an undo
//! shows in Settings is posted there ([`crate::windows`]). Edit ▸ Undo names the change it would take ([`menu_title`]). No redo; quitting
//! the app forgets its settings changes (the service keeps the torrents' stack).

pub mod machine;
pub mod session;

use std::collections::BTreeMap;

use client::{Failure, display};
use proto::{Configured, Refusal, SettingName, SettingUndone};
use rune::Tx;

use crate::model::Phantom;
use crate::model::settings::{self, Side};
use crate::notice::Notice;
use crate::platform::app_icon::Icons;
use crate::theme::preset::{Appearance, LookPreset, Looks};
use crate::theme::saved::{Name, Saved, SavedPreset};
use crate::windows::Which;
use machine::{AppSetting, Command, Do, Entry, Event, Observed, Of};
use session::{Session, Sheet};

/// The app's side of the settings changes.
pub struct Changes {
    pub state: machine::State,
    /// What each app setting's change was before, by its id.
    was: BTreeMap<u64, Was>,
    /// The next id for a change the app makes: of its own setting, or sent to the service.
    next: u64,
    /// Each service setting's value as the app saw it before it last changed, and now, as an undo
    /// words them: what an undo tells it undid. The stream may bring the undone value back before
    /// the undo's own answer does, so the one before is kept too.
    seen: BTreeMap<proto::SettingName, (Option<String>, String)>,
    /// While a sheet over Settings that edits things is open: its steps ([`session`]).
    session: Option<Session>,
    /// The sessions of sheets that closed, whose folded step isn't known yet ([`sheet_closing`]),
    /// oldest first.
    closing: std::collections::VecDeque<Option<Session>>,
}

/// What an app setting was before a change.
#[derive(Clone)]
pub enum Was {
    /// The App icon as it was: before a pick, or when its sheet opened. With what Edit ▸ Undo
    /// says of it, "App icon: Dark Hour → Memento Mori".
    Icon(Option<Icons>, String),
    /// A theme setting set on `side` of the Active look or a saved preset (`Target`): the Look
    /// Presets as they were.
    Theme(Presets, Target, Side),
    /// The Look Presets as they were before a change of the saved ones, or an Apply.
    Presets(Presets, PresetChange),
    /// The Look Presets as they were when the editor of `Target` opened: what its Done kept.
    Edit(Presets, Target),
}

/// The Active Look Preset and the saved ones: what an undo of a change of them puts back whole.
/// Undo takes changes newest first, so they're as the change left them when it's undone.
#[derive(Clone, Debug, PartialEq)]
pub struct Presets {
    pub active: LookPreset,
    pub saved: Vec<SavedPreset>,
}

impl Presets {
    pub fn of(looks: &Looks, saved: &Saved) -> Presets {
        Presets { active: looks.preset().clone(), saved: saved.list().to_vec() }
    }

    /// The preset `target` names, if there's one.
    pub fn get(&self, target: &Target) -> Option<&LookPreset> {
        match target {
            Target::Active => Some(&self.active),
            Target::Saved(name) => self.saved.iter().find(|s| s.name == *name).map(|s| &s.preset),
        }
    }
}

/// The Look Preset a theme setting's change is of: the Active look, or a saved one by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Active,
    Saved(Name),
}

/// A change of the Look Presets: an Apply of a saved one into the Active look, or a change of the
/// saved ones.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetChange {
    pub verb: PresetVerb,
    /// The saved preset it's of.
    pub name: Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetVerb {
    /// Into the Active look: both halves, or one.
    Apply(Option<Appearance>),
    /// The Active look saved under a new name.
    Save,
    /// The Active look saved over the saved one of its name.
    Replace,
    Delete,
}

impl PresetChange {
    /// What Edit ▸ Undo and the undo's notice name it by: "Apply Evening's Dark".
    pub fn words(&self) -> String {
        let (verb, half) = match self.verb {
            PresetVerb::Apply(half) => ("Apply", half.map(Appearance::label)),
            PresetVerb::Save => ("Save", None),
            PresetVerb::Replace => ("Replace", None),
            PresetVerb::Delete => ("Delete", None),
        };
        display::preset_change(verb, self.name.as_str(), half)
    }
}

/// What Edit ▸ Undo and the undo's notice name an editor's Done by: "Edit of Night", "Edit of
/// the Active look".
pub fn edit_words(target: &Target) -> String {
    match target {
        Target::Active => display::of_preset("Edit", "the Active look"),
        Target::Saved(name) => display::of_preset("Edit", name.as_str()),
    }
}

impl Default for Changes {
    fn default() -> Self {
        // From now in nanoseconds, so an id is never one an earlier run of the app gave a change
        // the service still lists.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Self {
            state: machine::State::default(),
            was: BTreeMap::new(),
            next: now,
            seen: BTreeMap::new(),
            session: None,
            closing: Default::default(),
        }
    }
}

impl Changes {
    /// Takes `settings` as the service's now.
    fn see(&mut self, settings: &proto::Settings) {
        for name in proto::SettingName::ALL {
            let value = display::setting_value(name, settings);
            match self.seen.get_mut(&name) {
                Some((before, now)) if *now != value => *before = Some(std::mem::replace(now, value)),
                Some(_) => {}
                None => {
                    self.seen.insert(name, (None, value));
                }
            }
        }
    }

    /// What `name` was before the undo that answered with `after`: what the app had, or, if the
    /// stream already brought the undone value back, the one before it.
    fn undone_value(&mut self, name: proto::SettingName, after: &proto::Settings) -> String {
        let now = display::setting_value(name, after);
        let undone = match self.seen.get(&name) {
            Some((before, seen)) if *seen == now => before.clone(),
            Some((_, seen)) => Some(seen.clone()),
            None => None,
        };
        self.see(after);
        undone.unwrap_or(now)
    }

    fn decide(&mut self, event: Event) -> Result<(), machine::Nothing> {
        self.state = machine::decide(&self.state, &event)?;
        Ok(())
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// What an app setting's `change` was before it.
    fn was_of(&self, change: &machine::Change) -> Option<&Was> {
        match change {
            machine::Change::App { id, .. } => self.was.get(id),
            machine::Change::Sent { .. }
            | machine::Change::Listed { .. }
            | machine::Change::Action { .. }
            | machine::Change::Pending { .. } => None,
        }
    }
}

/// The app setting `of` changed; it was `was`.
pub fn app_changed(s: &mut Phantom, of: AppSetting, was: Was) {
    let c = &mut s.changes;
    let id = c.id();
    c.was.insert(id, was);
    match &mut c.session {
        Some(session) if session.owns(&of) => {
            session.push(id);
            let _ = c.decide(Event::Command(Command::Edited { id, of }));
        }
        _ => {
            let _ = c.decide(Event::Command(Command::Changed { id, of }));
        }
    }
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
    run(s);
}

/// The settings a snapshot carries: newer than the app has (the machine says), they're its
/// settings, shown at once.
pub fn observe(s: &mut Phantom, configured: Configured) {
    let before = s.changes.state.at;
    let _ = s.changes.decide(Event::Observed(Observed::Listed { at: configured.at, changes: configured.changes }));
    if s.changes.state.at != before {
        let settings = configured.settings;
        s.changes.see(&settings);
        s.settings.post(move |w, _| {
            if let Some(sheet) = &mut w.sheet {
                sheet.observe(settings);
            }
        });
    }
    run(s);
}

/// The service's undo stack, as a snapshot newer than any the app took lists it (`at`, its epoch
/// and version): the torrent actions ⌘Z takes join the list as the app learns of them.
pub fn observe_stack(s: &mut Phantom, at: (u64, u64), stack: &[proto::Undoable]) {
    let entries = stack.iter().map(|u| Entry { seq: u.seq, torrents: u.torrents.iter().map(|t| t.id).collect() }).collect();
    let _ = s.changes.decide(Event::Observed(Observed::Stack { at, entries }));
    run(s);
}

/// A Remove, Delete or Move to Trash of `torrents` is being sent, the newest entry on the stack
/// the app knows being `after`: it's the newest change from now, before any snapshot lists it.
/// The id it's sent as, for [`made`].
pub fn acted(s: &mut Phantom, after: u64, torrents: Vec<u64>) -> u64 {
    let id = s.changes.id();
    let _ = s.changes.decide(Event::Command(Command::Acted { id, after, torrents }));
    id
}

/// The answer to the torrent action sent as `id` came: the entry it made on the stack, if any.
pub fn made(s: &mut Phantom, id: u64, seq: Option<u64>) {
    let _ = s.changes.decide(Event::Observed(Observed::Made { id, seq }));
    run(s);
}

/// ⌘Z, in the main window or in Settings: the newest change, whoever made it; with none, nothing.
/// While a sheet is open, the newest of its own steps, and only those ([`session`]): what came
/// before it stays until it's closed.
pub fn press(s: &mut Phantom) {
    let event = match &s.changes.session {
        Some(session) => match session.top(&s.changes.state) {
            Some(id) => Command::PressApp { id },
            None => return,
        },
        None => Command::Press,
    };
    if s.changes.decide(Event::Command(event)).is_ok() {
        run(s);
    }
}

/// A sheet opened: its steps are its own from now on, until it closes.
pub fn sheet_opened(s: &mut Phantom, sheet: Sheet) {
    s.changes.session = Some(Session::new(sheet));
}

/// The sheet's session ends at once, its steps gone and none folded: its Look Preset is gone.
pub fn sheet_dropped(s: &mut Phantom) {
    let session = s.changes.session.take();
    fold(&mut s.changes, session, None);
}

/// A sheet closed (Done, Cancel, Escape, ⌘W, the window closing): `finish` runs in Settings once
/// what was pressed before it has (a ⌘Z just before it takes its step first, and shows), settles
/// what's left of the visit and says the one step it folds into, if any: what it's a change of,
/// and what it was before the sheet opened ([`fold_closed`]).
pub fn sheet_closing(s: &mut Phantom, finish: Box<dyn FnOnce(&mut settings::SettingsWindow) -> Option<(AppSetting, Was)>>) {
    let session = s.changes.session.take();
    s.changes.closing.push_back(session);
    s.settings.post(move |w, _| {
        let into = finish(w);
        w.main.post(move |s, _| fold_closed(s, into));
    });
}

/// What the oldest sheet that closed folds into: its steps leave the list, and `into`, if any,
/// is the one step they become.
fn fold_closed(s: &mut Phantom, into: Option<(AppSetting, Was)>) {
    let session = s.changes.closing.pop_front().flatten();
    fold(&mut s.changes, session, into);
}

fn fold(c: &mut Changes, session: Option<Session>, into: Option<(AppSetting, Was)>) {
    let ids = session.map(Session::into_ids).unwrap_or_default();
    let into = into.map(|(of, was)| {
        let id = c.id();
        c.was.insert(id, was);
        (id, of)
    });
    for id in &ids {
        c.was.remove(id);
    }
    let _ = c.decide(Event::Command(Command::Fold { ids, into }));
    c.was.retain(|id, _| c.state.list.iter().any(|ch| matches!(ch, machine::Change::App { id: i, .. } if i == id)));
}

/// The Undo in the notice of the torrent action `seq`: that one, while ⌘Z hasn't taken it.
pub fn press_action(s: &mut Phantom, seq: u64) {
    if s.changes.decide(Event::Command(Command::PressAction { seq })).is_ok() {
        run(s);
    }
}

/// What Edit ▸ Undo says: the change ⌘Z would take back, "Undo Port Change", "Undo Remove of
/// Album"; else "Undo", as while a field of Settings is being edited with Settings in front, whose
/// own undo ⌘Z then is.
pub fn menu_title(s: &Phantom) -> String {
    let editing = s.front == Which::Settings && s.settings_editing;
    let top = match &s.changes.session {
        Some(session) => session
            .top(&s.changes.state)
            .and_then(|id| s.changes.state.list.iter().find(|c| matches!(c, machine::Change::App { id: i, .. } if *i == id))),
        None => s.changes.state.top(),
    };
    let named = match top.map(machine::Change::of) {
        _ if editing => None,
        Some(Of::App(AppSetting::Presets)) => match top.and_then(|c| s.changes.was_of(c)) {
            Some(Was::Presets(_, change)) => Some(display::undo_named(&change.words())),
            Some(Was::Icon(..) | Was::Theme(..) | Was::Edit(..)) | None => None,
        },
        Some(Of::App(AppSetting::Edit)) => match top.and_then(|c| s.changes.was_of(c)) {
            Some(Was::Edit(_, target)) => Some(display::undo_named(&edit_words(target))),
            Some(Was::Icon(..) | Was::Theme(..) | Was::Presets(..)) | None => None,
        },
        // A saved preset's: named with it, "Undo Tone of Evening".
        Some(Of::App(AppSetting::Theme { label, .. }))
            if let Some(Was::Theme(_, Target::Saved(name), _)) = top.and_then(|c| s.changes.was_of(c)) =>
        {
            Some(display::undo_named(&display::of_preset(label, name.as_str())))
        }
        Some(Of::Action(seq)) => s.undo.iter().find(|u| u.seq == seq).map(display::undo_entry),
        Some(Of::Service(name)) => Some(display::undo_change(display::setting_short(name.key()))),
        Some(Of::App(AppSetting::Icon)) => match top.and_then(|c| s.changes.was_of(c)) {
            Some(Was::Icon(_, words)) => Some(display::undo_named(words)),
            Some(Was::Theme(..) | Was::Presets(..) | Was::Edit(..)) | None => {
                Some(display::undo_change(display::setting_short("app_icon")))
            }
        },
        Some(Of::App(AppSetting::Theme { label, .. })) => Some(display::undo_change(label)),
        // Sent, not listed yet: for a moment.
        Some(Of::Pending) | None => None,
    };
    named.unwrap_or_else(|| display::undo().to_owned())
}

/// Carries out what the machine says, until it says nothing.
fn run(s: &mut Phantom) {
    while let Some(todo) = machine::next(&s.changes.state) {
        match todo {
            Do::Undo { id } => {
                let of = s.changes.state.list.iter().find_map(|c| match c {
                    machine::Change::App { id: i, of, .. } if *i == id => Some(*of),
                    machine::Change::App { .. }
                    | machine::Change::Sent { .. }
                    | machine::Change::Listed { .. }
                    | machine::Change::Action { .. }
                    | machine::Change::Pending { .. } => None,
                });
                // The app settings are Settings', which shows them.
                if let (Some(was), Some(of)) = (s.changes.was.remove(&id), of) {
                    s.settings.post(move |w, tx| settings::undo_app(w, of, was, tx));
                }
                let _ = s.changes.decide(Event::Observed(Observed::AppUndone { id }));
            }
            Do::Ask { seq } => {
                let _ = s.changes.decide(Event::Command(Command::Asked { seq }));
                s.service.undo_setting(Box::new(move |s: &mut Phantom, undone, _: &mut Tx| undid(s, seq, undone)));
            }
            Do::AskAction { seq } => {
                let _ = s.changes.decide(Event::Command(Command::AskedAction { seq }));
                crate::model::ask_undo(
                    s,
                    seq,
                    Box::new(move |s: &mut Phantom, took, _: &mut Tx| {
                        let _ = s.changes.decide(Event::Observed(Observed::ActionAnswered { asked: seq, took }));
                        run(s);
                    }),
                );
            }
        }
    }
}

/// The service's answer to the undo asked for `asked`.
fn undid(s: &mut Phantom, asked: u64, undone: Result<SettingUndone, Failure>) {
    match undone {
        Ok(SettingUndone { change, settings }) => {
            let _ = s.changes.decide(Event::Observed(Observed::Undid { asked, took: change.seq, by: change.by }));
            // From the main window too: Settings opens at it.
            let (name, shown) = (change.name, settings.clone());
            s.settings.post(move |w, tx| settings::undone(w, name, shown, tx));
            let short = display::setting_short(change.name.key());
            // What was set before the undo: the settings as the app last had them.
            let now = display::setting_value(change.name, &settings);
            let undone = s.changes.undone_value(change.name, &settings);
            s.bar.tell(Notice::setting_undone(short, &undone, &now));
        }
        Err(failure) => {
            // The setting it was of, as the list names it, before the entry goes.
            let of = s.changes.state.list.iter().find_map(|c| match c {
                machine::Change::Listed { seq, name, .. } if *seq == asked => Some(display::setting_short(name.key()).to_owned()),
                _ => None,
            });
            let _ = s.changes.decide(Event::Observed(Observed::Refused { asked }));
            // Nothing there: another client undid it first, and the list follows.
            if !matches!(failure, Failure::Refused(Refusal::NothingToUndo)) {
                let why = display::sentence(&format!("{failure:#}"));
                s.bar.tell(Notice::error(display::undo_failed()).with_listing(display::Listing::new(&of.unwrap_or_default(), &why)));
            }
        }
    }
    run(s);
}
