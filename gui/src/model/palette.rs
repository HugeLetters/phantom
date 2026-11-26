//! The command palette (#33): ⌘P opens a panel over the window listing the commands available now,
//! from the command list (`commands`): those for the selected torrents, those for the selected files
//! (while the details pane's Files list shows some selected), then the app's (Add file…,
//! and Undo, named after what it takes back, while there's something to). Each group is under a
//! muted heading naming what it acts on ([`heading`]); the headings aren't rows: ↑ and ↓ and
//! Return only know the commands. Its field has the focus;
//! typing filters by name, fuzzily (the letters in order, best matches first), ↑ and ↓ move (round the ends), Return
//! runs the one highlighted and closes it, Escape or a click outside closes it. A command that asks
//! for more starts its usual flow: Rename… opens the Name cell's field, Move to… the folder picker.
//! With nothing matching, Return runs nothing. It's one of the overlays (`overlay`): opening it
//! closes the others, and theirs close it.
//!
//! What it offers isn't kept: it's read from the selection and the service's actions every frame
//! it's drawn, so it can't go stale. Open and Reveal rest on a look at the disk, taken as it opens,
//! as the menus' do (#307).
//!
//! | state ↓ / event → | `Open`      | `Up`             | `Down { n }`           | `Close`  |
//! |-------------------|-------------|------------------|------------------------|----------|
//! | `Closed`          | `Open { 0 }`| `Closed`         | `Closed`               | `Closed` |
//! | `Open { at }`     | `Open { 0 }`| `Open { at − 1 }`, the first goes to the last (n − 1) | `Open { at + 1 }`, the last goes to the first | `Closed` |
//!
//! Typing leaves the state as it is: the highlight is on the first match again whenever the text
//! changed since an arrow last moved it ([`Palette::at`]).

use rune::{TextEdit, Tx};

use super::{Phantom, details_torrent, look_at_disk, look_at_file, reveal_target, selected_in_pane};
use crate::commands::{Command, InApp, OnFiles, OnTorrents, Selected, on_files, on_torrents};
use crate::ui::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Closed,
    /// Open, the `at`-th command highlighted.
    Open { at: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    /// Up a list of `n` commands.
    Up {
        n: usize,
    },
    /// Down a list of `n` commands.
    Down {
        n: usize,
    },
    /// Return ran a command, Escape, or a click outside.
    Close,
}

pub fn decide(state: State, event: Event) -> State {
    match (state, event) {
        (_, Event::Open) => State::Open { at: 0 },
        (State::Closed, Event::Up { .. } | Event::Down { .. } | Event::Close) | (State::Open { .. }, Event::Close) => State::Closed,
        (State::Open { at }, Event::Up { n }) => State::Open { at: if n == 0 { 0 } else { (at + n - 1) % n } },
        (State::Open { at }, Event::Down { n }) => State::Open { at: if n == 0 { 0 } else { (at + 1) % n } },
    }
}

/// The palette: its state, and its field's text.
#[derive(Default)]
pub struct Palette {
    state: State,
    pub query: TextEdit,
    /// The text as it was when an arrow last moved the highlight: once it changes, the highlight is
    /// on the first match again.
    seen: String,
    /// The command highlighted, as last arrowed to, clicked or opened on: the highlight follows it
    /// when rows come and go under it ([`highlighted`]). Meaningful only while `seen` is the text.
    on: Option<Pin>,
    // PROTOTYPE (#434): the Choice setting whose choices the palette lists instead, and the text
    // typed before it did.
    pub sub: Option<&'static str>,
    pub before_sub: String,
    // PROTOTYPE (#434): Keep awake, faked here: the real one is the service's.
    pub keep_awake: bool,
}

/// What identifies a listed command: the command, and for "Move to Films" its folder.
// PROTOTYPE (#434): and a setting's or place's label.
type Pin = (Command, Option<proto::AbsolutePath>, Option<String>);

impl Palette {
    pub fn is_open(&self) -> bool {
        self.state != State::Closed
    }

    /// Which command is highlighted, of those listed: the first since the text last changed.
    fn at(&self) -> usize {
        match self.state {
            State::Open { at } if self.query.text == self.seen => at,
            State::Open { .. } | State::Closed => 0,
        }
    }

    /// The command the highlight is pinned to, unless the text changed since.
    fn pinned(&self) -> Option<&Pin> {
        if self.query.text == self.seen { self.on.as_ref() } else { None }
    }

    fn then(&mut self, event: Event) {
        let state = match self.state {
            State::Open { .. } => State::Open { at: self.at() },
            State::Closed => State::Closed,
        };
        self.state = decide(state, event);
        self.seen = self.query.text.clone();
    }
}

/// What a command acts on: the palette lists them in this order, each under a heading naming it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    // PROTOTYPE (#434)
    Recent,
    Choices,
    Torrents,
    Files,
    App,
    Settings,
}

/// The heading over `group`'s commands, naming the actual selection: the torrent's name or "3
/// torrents", the file's name or "2 files", "Phantom" for the app's.
pub fn heading(s: &Phantom, group: Group) -> String {
    match group {
        Group::Torrents => {
            let selected = Selected::of(&s.table, &s.torrents, s.target_there);
            match selected.torrents.as_slice() {
                [t] => t.display_name().to_owned(),
                many => format!("{} torrents", many.len()),
            }
        }
        Group::Files => match selected_in_pane(&s.table, &s.torrents, &s.pane).map(|(_, files)| files) {
            Some(files) if files.len() == 1 => {
                files[0].path.file_name().unwrap_or(files[0].path.as_os_str()).to_string_lossy().into_owned()
            }
            Some(files) => format!("{} files", files.len()),
            None => String::new(),
        },
        Group::App => "Phantom".to_owned(),
        Group::Recent => "Recent".to_owned(),
        Group::Settings => "Settings".to_owned(),
        Group::Choices => s.palette.sub.and_then(|k| theme_setting(s, k)).map_or(String::new(), |(st, _)| st.label.to_owned()),
    }
}

/// One command as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub command: Command,
    pub group: Group,
    /// As the selection words it: "Delete 3 torrents (files to the Trash)", "Undo Remove of Album".
    pub label: String,
    pub hint: Option<String>,
    pub icon: Option<Icon>,
    /// A folder Move to ▸ offers (#87), for "Move to Films": it moves there with no picker.
    pub to: Option<proto::AbsolutePath>,
    // PROTOTYPE (#434): where it lives ("Appearance ▸ Edit… ▸ Look"), and what it is.
    pub place: Option<String>,
    pub proto: Option<Proto>,
}

impl Entry {
    fn pin(&self) -> Pin {
        (self.command, self.to.clone(), self.proto.as_ref().map(|_| self.label.clone()))
    }
}

/// Every command available now, in the list's order: the selected torrents', the selected files'
/// (while the Files list shows some selected), then the app's. Move to… (which asks for a folder)
/// is followed by "Move to <folder>" for each folder Move to ▸ offers.
pub fn offered(s: &Phantom) -> Vec<Entry> {
    let selected = Selected::of(&s.table, &s.torrents, s.target_there);
    let offered = on_torrents(s);
    let can_delete = offered.contains(&OnTorrents::Delete);
    let n = selected.torrents.len();
    let entry = |command: Command, group: Group, label: String| Entry {
        command,
        group,
        label,
        hint: command.spec().hint(),
        icon: command.spec().icon,
        to: None,
        place: None,
        proto: None,
    };
    if s.palette.sub.is_some() {
        return choices(s);
    }
    let mut entries = Vec::new();
    for c in offered {
        entries.push(entry(Command::Torrents(c), Group::Torrents, c.wording(n, can_delete)));
        if c == OnTorrents::MoveTo {
            let folders = super::recent::offered(s);
            let names = super::recent::names(&folders);
            entries.extend(folders.into_iter().zip(names).map(|(f, name)| Entry {
                label: client::display::move_to_folder(&name),
                hint: None,
                to: Some(f.folder),
                ..entry(Command::Torrents(c), Group::Torrents, String::new())
            }));
        }
    }
    // Worded as the file menu words them, but for the priorities ([`label_of`]).
    entries.extend(on_files(s).into_iter().map(|c| entry(Command::Files(c), Group::Files, label_of(c))));
    entries.extend(InApp::ALL.into_iter().filter(|c| c.available(s)).map(|c| entry(Command::App(c), Group::App, c.wording(s))));
    let settings = settings_entries(s);
    if s.palette.query.text.trim().is_empty() {
        // PROTOTYPE (#434): a fixed "recent" list, where the real one keeps what was last picked.
        let recent: Vec<Entry> = ["Corners", "Keep awake", "Tone"]
            .iter()
            .filter_map(|l| settings.iter().find(|e| e.label == *l).cloned())
            .map(|e| Entry { group: Group::Recent, ..e })
            .collect();
        entries.splice(0..0, recent);
    } else {
        entries.extend(settings);
    }
    entries
}

// ---- PROTOTYPE (#434) from here to the end of `settings_entries`: not for merge. ----

/// What a settings entry is, and so what picking it does.
#[derive(Clone, Debug, PartialEq)]
pub enum Proto {
    /// A place: Settings, a tab, a sheet. Picking opens it.
    Place(Option<crate::model::settings::Tab>, bool),
    /// A service setting shown as a switch: picking flips it.
    Switch(crate::model::settings::Switch),
    /// A service setting with a field or a button: picking shows it in Settings.
    Service(&'static str),
    /// A theme setting, by key: a Choice opens its choices, a Toggle flips, a Color shows it.
    Theme(&'static str),
    /// One of the choices of a Choice setting.
    Choice(&'static str, &'static str),
    /// The setting in Settings, its control focused.
    Show(&'static str),
}

/// The theme setting `key` as the Active look declares it, and its value in Light.
fn theme_setting(s: &Phantom, key: &str) -> Option<(&'static crate::theme::setting::Setting, crate::theme::setting::Value)> {
    s.looks.borrow().preset().get(crate::theme::preset::Appearance::Light).settings.iter().find(|(st, _)| st.key == key)
}

fn plain(label: impl Into<String>, group: Group, place: Option<String>, hint: Option<String>, proto: Proto) -> Entry {
    Entry { command: Command::App(InApp::Palette), group, label: label.into(), hint, icon: None, to: None, place, proto: Some(proto) }
}

/// The choices of the setting the palette is in, the current one ticked, then Show in Settings.
fn choices(s: &Phantom) -> Vec<Entry> {
    use crate::theme::setting::{Kind, Value};
    let Some(key) = s.palette.sub else { return Vec::new() };
    let Some((setting, value)) = theme_setting(s, key) else { return Vec::new() };
    let Kind::Choice { choices, .. } = &setting.kind else { return Vec::new() };
    let mut out: Vec<Entry> = choices
        .iter()
        .map(|c| {
            let tick = (value == Value::Choice(c.id)).then(|| "✓".to_owned());
            plain(c.label, Group::Choices, None, tick, Proto::Choice(key, c.id))
        })
        .collect();
    out.push(plain("Show in Settings", Group::Choices, None, Some("⌘↩".into()), Proto::Show(key)));
    out
}

/// Settings' places and every setting: the service's, then the theme's, read from what the
/// Active look's theme declares.
fn settings_entries(s: &Phantom) -> Vec<Entry> {
    use crate::model::settings::{Switch, Tab};
    use crate::theme::setting::{Kind, Value};
    let g = Group::Settings;
    let on_off = |on: bool| Some(if on { "On" } else { "Off" }.to_owned());
    let mut out = vec![
        plain("Settings…", g, None, Some("⌘,".into()), Proto::Place(None, false)),
        plain("General", g, Some("Settings".into()), None, Proto::Place(Some(Tab::General), false)),
        plain("Network", g, Some("Settings".into()), None, Proto::Place(Some(Tab::Network), false)),
        plain("Appearance", g, Some("Settings".into()), None, Proto::Place(Some(Tab::Appearance), false)),
        plain("Edit…", g, Some("Settings ▸ Appearance ▸ Look".into()), None, Proto::Place(Some(Tab::Appearance), true)),
        plain("Default download folder", g, Some("General".into()), None, Proto::Service("default_destination")),
        plain("Keep awake", g, Some("General".into()), on_off(s.palette.keep_awake), Proto::Switch(Switch::KeepAwake)),
        plain("Stop seeding at ratio", g, Some("General".into()), None, Proto::Service("seed_ratio")),
        plain("Stop seeding after", g, Some("General".into()), None, Proto::Service("seed_time")),
        plain("Listening port", g, Some("Network".into()), None, Proto::Service("listen_port")),
        plain("UPnP", g, Some("Network".into()), on_off(true), Proto::Switch(Switch::Upnp)),
        plain("Download limit", g, Some("Network".into()), None, Proto::Service("download_limit")),
        plain("Upload limit", g, Some("Network".into()), None, Proto::Service("upload_limit")),
    ];
    let declared = s.looks.borrow().preset().get(crate::theme::preset::Appearance::Light).settings.iter().collect::<Vec<_>>();
    for (setting, value) in declared {
        let place = match setting.section {
            Some(section) => format!("Appearance ▸ Edit… ▸ {}", section.title),
            None => "Appearance ▸ Edit…".to_owned(),
        };
        let hint = match (&setting.kind, value) {
            (Kind::Choice { choices, .. }, Value::Choice(id)) => choices.iter().find(|c| c.id == id).map(|c| format!("{} ›", c.label)),
            (Kind::Toggle { .. }, Value::Toggle(on)) => on_off(on),
            _ => None,
        };
        out.push(plain(setting.label, g, Some(place), hint, Proto::Theme(setting.key)));
    }
    out
}

/// What picking a settings entry does.
fn run_proto(s: &mut Phantom, entry: Entry, proto: Proto, tx: &mut Tx) {
    use crate::model::settings::{self as st, Side, Tab};
    use crate::theme::setting::{Kind, Value};
    match proto {
        Proto::Theme(key) => {
            let Some((setting, value)) = theme_setting(s, key) else { return };
            match (&setting.kind, value) {
                (Kind::Choice { .. }, _) => {
                    s.palette.before_sub = s.palette.query.text.clone();
                    s.palette.sub = Some(key);
                    s.palette.query = TextEdit::new("");
                    s.palette.query.focused = true;
                    // The highlight starts on the current choice.
                    let list = listed(s);
                    let at = list.iter().position(|e| e.hint.as_deref() == Some("✓")).unwrap_or(0);
                    s.palette.state = State::Open { at };
                    s.palette.seen = String::new();
                    s.palette.on = list.get(at).map(Entry::pin);
                    return;
                }
                (Kind::Toggle { .. }, Value::Toggle(on)) => {
                    dismiss(s, tx);
                    set_theme(s, key, Value::Toggle(!on));
                }
                _ => return show(s, key, tx),
            }
        }
        Proto::Choice(key, id) => {
            dismiss(s, tx);
            set_theme(s, key, Value::Choice(id));
            let label = theme_setting(s, key).map_or("", |(st, _)| st.label);
            s.bar.tell(crate::notice::Notice::new(format!("{label}: {}", entry.label)).brief());
        }
        Proto::Show(key) => show(s, key, tx),
        Proto::Switch(switch) => {
            dismiss(s, tx);
            s.palette.keep_awake = !s.palette.keep_awake;
            let _ = switch;
            let word = if s.palette.keep_awake { "On" } else { "Off" };
            s.bar.tell(crate::notice::Notice::new(format!("Keep awake: {word}")).brief());
        }
        Proto::Service(key) => {
            dismiss(s, tx);
            s.settings.post(move |w, tx| st::show_setting(w, key, Some(tx)));
        }
        Proto::Place(tab, edit) => {
            dismiss(s, tx);
            s.settings.post(move |w, _| {
                st::open(w);
                st::show_tab(w, tab.unwrap_or(Tab::General));
                if edit {
                    st::presets::edit_active(w);
                }
            });
        }
    }
    let _ = Side::Both;
}

/// The theme setting `key` in Settings: Appearance, the Active look's sheet open, its control focused.
fn show(s: &mut Phantom, key: &'static str, tx: &mut Tx) {
    use crate::model::settings::{self as st, Side, Tab};
    dismiss(s, tx);
    s.settings.post(move |w, _| {
        st::open(w);
        st::show_tab(w, Tab::Appearance);
        st::presets::edit_active(w);
        if let Some(edit) = w.sheet.as_mut().and_then(|sheet| sheet.edit.as_mut()) {
            edit.focus = Some((key, Side::Both));
        }
    });
}

/// Sets a theme setting in both halves of the Active look. The real one is one step of Undo.
fn set_theme(s: &mut Phantom, key: &str, value: crate::theme::setting::Value) {
    use crate::theme::preset::Appearance;
    for a in Appearance::BOTH {
        let mut look = s.looks.borrow().preset().get(a).clone();
        look.settings.set(key, value);
        s.looks.borrow_mut().set(a, look);
    }
}

// ---- end of PROTOTYPE ----

/// A file command's label in the palette: the priorities say what they set, as the file menu's
/// title does over them.
fn label_of(c: OnFiles) -> String {
    match c {
        OnFiles::Priority(p) => client::display::priority_command(p),
        _ => c.spec().name.to_owned(),
    }
}

/// What the palette lists for its text: the commands offered whose label matches it, group by
/// group (torrents', files', the app's), best first within a group (a global ranking would
/// scatter a group's heading over the list), in the list's order among equals; all of them for no
/// text. A group with no match has no heading.
pub fn listed(s: &Phantom) -> Vec<Entry> {
    let query = s.palette.query.text.trim();
    let mut scored: Vec<(i32, usize, Entry)> =
        offered(s).into_iter().enumerate().filter_map(|(i, e)| score(query, &e.label).map(|score| (score, i, e))).collect();
    // PROTOTYPE (#434): while typing, the group with the best match first.
    let best = |g: Group| scored.iter().filter(|(_, _, e)| e.group == g).map(|(sc, _, _)| *sc).max().unwrap_or(0);
    let bests: Vec<(Group, i32)> = [Group::Recent, Group::Choices, Group::Torrents, Group::Files, Group::App, Group::Settings]
        .into_iter()
        .map(|g| (g, best(g)))
        .collect();
    let rank = |g: Group| if query.is_empty() { 0 } else { -bests.iter().find(|(x, _)| *x == g).map_or(0, |(_, b)| *b) };
    scored.sort_by_key(|(score, i, e)| (rank(e.group), e.group, -score, *i));
    scored.into_iter().map(|(_, _, e)| e).collect()
}

/// How well `label` matches `query`, if it does: each of the query's letters (any case, spaces
/// ignored) in order in it. A letter starting a word scores most, then one right after the last;
/// a match from the label's first letter more still. `Some(0)` for an empty query.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    let mut from = 0;
    let mut last: Option<usize> = None;
    let mut score = 0;
    for q in query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase) {
        // A letter starting a word is taken before an earlier one inside a word.
        let starts_word = |i: usize| i == 0 || !label[i - 1].is_alphanumeric();
        let found = (from..label.len()).filter(|i| label[*i] == q);
        let at = found
            .clone()
            .find(|i| last.is_some_and(|l| *i == l + 1))
            .or_else(|| found.clone().find(|i| starts_word(*i)))
            .or_else(|| found.clone().next())?;
        score += 1;
        if starts_word(at) {
            score += 4;
        }
        if last.is_some_and(|l| at == l + 1) {
            score += 3;
        }
        if at == 0 {
            score += 4;
        }
        last = Some(at);
        from = at + 1;
    }
    Some(score)
}

/// ⌘P: opens over whatever else was open, the field empty and focused, the first command
/// highlighted; looks at the disk for Open and Reveal, of the torrent and of the file.
pub fn open(s: &mut Phantom) {
    crate::overlay::opening(s, crate::overlay::Overlay::Palette);
    let target = details_torrent(&s.table, &s.torrents).and_then(|t| reveal_target(t).map(|p| p.to_owned()));
    look_at_disk(s, target);
    look_at_file(s);
    s.palette.query = TextEdit::new("");
    s.palette.query.focused = true;
    s.palette.then(Event::Open);
    s.palette.on = listed(s).first().map(Entry::pin);
}

/// Escape or a click outside.
pub fn dismiss(s: &mut Phantom, tx: &mut Tx) {
    // The focus goes back to what had it, or nowhere: without this, after a Tab in the palette,
    // Rune hands it to a neighbour of the field as the palette fades.
    tx.blur();
    close(s);
}

/// [`dismiss`]ed, a command picked, or another overlay opening.
pub fn close(s: &mut Phantom) {
    s.palette.sub = None;
    s.palette.then(Event::Close);
    s.palette.query = TextEdit::default();
}

/// Which of the `listed` commands is highlighted: the one the highlight is pinned to, wherever it
/// has moved to since; if that one is gone (or none is pinned), the row at the position it had,
/// or the last row when the list has shrunk past it, so the highlight lands on a neighbour.
pub fn highlighted(s: &Phantom) -> usize {
    let list = listed(s);
    let pinned = s.palette.pinned().and_then(|c| list.iter().position(|e| e.pin() == *c));
    pinned.unwrap_or_else(|| s.palette.at().min(list.len().saturating_sub(1)))
}

/// Pins the highlight to what is highlighted now, before an arrow moves it from there.
fn settle(s: &mut Phantom) -> Vec<Entry> {
    let (list, at) = (listed(s), highlighted(s));
    s.palette.state = State::Open { at };
    s.palette.seen = s.palette.query.text.clone();
    s.palette.on = list.get(at).map(Entry::pin);
    list
}

fn arrow(s: &mut Phantom, tx: &mut Tx, event: impl FnOnce(usize) -> Event) {
    let list = settle(s);
    s.palette.then(event(list.len()));
    let at = s.palette.at();
    s.palette.on = list.get(at).map(Entry::pin);
    tx.scroll_into_view(row_key(at));
}

/// ↑: the highlight up one, the first going to the last.
pub fn up(s: &mut Phantom, tx: &mut Tx) {
    arrow(s, tx, |n| Event::Up { n });
}

/// ↓: the highlight down one, the last going to the first.
pub fn down(s: &mut Phantom, tx: &mut Tx) {
    arrow(s, tx, |n| Event::Down { n });
}

/// Return: runs the highlighted command, closing the palette first, so a command that opens
/// something (Rename…'s field, the folder picker) has the focus. With nothing listed, nothing.
/// It's the command highlighted that runs, found by identity ([`highlighted`]), never whatever
/// the position now holds.
pub fn run(s: &mut Phantom, tx: &mut Tx) {
    let at = highlighted(s);
    let Some(entry) = listed(s).into_iter().nth(at) else { return };
    if let Some(proto) = entry.proto.clone() {
        return run_proto(s, entry, proto, tx);
    }
    dismiss(s, tx);
    if let Some(to) = entry.to {
        return super::moving::move_selection_to(s, to);
    }
    match entry.command {
        Command::Torrents(c) => crate::commands::run_on_torrents(s, c, tx),
        Command::App(c) => crate::commands::run_in_app(s, c, tx),
        Command::Files(c) => crate::views::file_menu::run(s, c),
    }
}

// PROTOTYPE (#434): ⌘Return shows the highlighted setting in Settings; Escape in a setting's
// choices goes back to the list.
pub fn show_highlighted(s: &mut Phantom, tx: &mut Tx) {
    let at = highlighted(s);
    let Some(entry) = listed(s).into_iter().nth(at) else { return };
    match entry.proto.clone() {
        Some(Proto::Theme(key) | Proto::Show(key) | Proto::Choice(key, _)) => show(s, key, tx),
        Some(p @ (Proto::Service(_) | Proto::Place(..) | Proto::Switch(_))) => run_proto(s, entry, p, tx),
        _ => {}
    }
}

pub fn back(s: &mut Phantom, tx: &mut Tx) {
    match s.palette.sub.take() {
        Some(_) => {
            s.palette.query = TextEdit::new(std::mem::take(&mut s.palette.before_sub));
            s.palette.query.focused = true;
            s.palette.state = State::Open { at: 0 };
            s.palette.seen = String::new();
        }
        None => dismiss(s, tx),
    }
}

/// A click on the `i`-th command listed: runs it.
pub fn pick(s: &mut Phantom, i: usize, tx: &mut Tx) {
    s.palette.seen = s.palette.query.text.clone();
    s.palette.state = State::Open { at: i };
    s.palette.on = listed(s).get(i).map(Entry::pin);
    run(s, tx);
}

/// The key of the `i`-th command's row, as the palette draws it.
pub fn row_key(i: usize) -> String {
    format!("palette-row-{i}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use machine::every;
    use machine::{Row, table};

    every!(State {
        State::Closed => [State::Closed],
        State::Open { .. } => [State::Open { at: 0 }, State::Open { at: 2 }],
    });

    every!(Event {
        Event::Open => [Event::Open],
        Event::Up { .. } => [Event::Up { n: 0 }, Event::Up { n: 1 }, Event::Up { n: 3 }, Event::Up { n: 5 }],
        Event::Down { .. } => [Event::Down { n: 0 }, Event::Down { n: 1 }, Event::Down { n: 3 }, Event::Down { n: 5 }],
        Event::Close => [Event::Close],
    });

    /// Every state × event pair, by the module's table.
    #[test]
    fn every_pair_is_decided() {
        let rows: Vec<Row<State, Event, State>> = vec![
            Row { says: "opening highlights the first", when: |_, e| *e == Event::Open, then: |_, _, out| *out == State::Open { at: 0 } },
            Row { says: "closing closes", when: |_, e| *e == Event::Close, then: |_, _, out| *out == State::Closed },
            Row {
                says: "an arrow while closed does nothing",
                when: |s, e| *s == State::Closed && matches!(e, Event::Up { .. } | Event::Down { .. }),
                then: |_, _, out| *out == State::Closed,
            },
            Row {
                says: "↑ moves up one, the first going to the last",
                when: |s, e| matches!(s, State::Open { .. }) && matches!(e, Event::Up { .. }),
                then: |s, e, out| match (s, e, out) {
                    (State::Open { at }, Event::Up { n }, State::Open { at: to }) => *to == if *n == 0 { 0 } else { (at + n - 1) % n },
                    _ => false,
                },
            },
            Row {
                says: "↓ moves down one, the last going to the first",
                when: |s, e| matches!(s, State::Open { .. }) && matches!(e, Event::Down { .. }),
                then: |s, e, out| match (s, e, out) {
                    (State::Open { at }, Event::Down { n }, State::Open { at: to }) => *to == if *n == 0 { 0 } else { (at + 1) % n },
                    _ => false,
                },
            },
        ];
        table(|s: &State, e: &Event| decide(*s, *e), &rows, &[]);
    }

    /// The letters in order match, any case; a prefix and word starts rank first.
    #[test]
    fn a_query_matches_its_letters_in_order_and_ranks_word_starts_first() {
        assert_eq!(score("", "Rename…"), Some(0));
        assert!(score("ren", "Rename…").is_some());
        assert!(score("REN", "Rename…").is_some());
        assert!(score("rnm", "Rename…").is_some());
        assert_eq!(score("nre", "Rename…"), None);
        assert_eq!(score("ren", "Resume"), None);
        assert!(score("ren", "Rename…") > score("ren", "Remove torrent (keep files)"));
        assert!(score("mt", "Move to…") > score("mt", "Copy magnet link"));
        assert!(score("del", "Delete torrent (files to the Trash)") > score("del", "Cancel move"));
    }
}
