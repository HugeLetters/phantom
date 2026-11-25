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
}

/// What identifies a listed command: the command, and for "Move to Films" its folder.
type Pin = (Command, Option<proto::AbsolutePath>);

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
    Torrents,
    Files,
    App,
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
}

impl Entry {
    fn pin(&self) -> Pin {
        (self.command, self.to.clone())
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
    };
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
    // Worded as the file menu words them.
    entries.extend(on_files(s).into_iter().map(|c| entry(Command::Files(c), Group::Files, label_of(c))));
    entries.extend(InApp::ALL.into_iter().filter(|c| c.available(s)).map(|c| entry(Command::App(c), Group::App, c.wording(s))));
    entries
}

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
    scored.sort_by_key(|(score, i, e)| (e.group, -score, *i));
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
