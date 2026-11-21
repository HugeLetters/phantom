//! The command palette (#33): ⌘P opens a panel over the window listing the commands available now,
//! from the command list (`commands`): those for the selected torrents, those for the selected files
//! (while the details pane's Files list shows some selected), then the app's (Add file…,
//! and Undo, named after what it takes back, while there's something to). Its field has the focus;
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

use super::{Phantom, details_torrent, look_at_disk, look_at_file, reveal_target};
use crate::commands::{Command, InApp, OnTorrents, Selected, on_files};
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
}

impl Palette {
    pub fn is_open(&self) -> bool {
        self.state != State::Closed
    }

    /// Which command is highlighted, of those listed: the first since the text last changed.
    pub fn at(&self) -> usize {
        match self.state {
            State::Open { at } if self.query.text == self.seen => at,
            State::Open { .. } | State::Closed => 0,
        }
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

/// One command as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub command: Command,
    /// As the selection words it: "Delete 3 torrents (files to the Trash)", "Undo Remove of Album".
    pub label: String,
    pub hint: Option<String>,
    pub icon: Option<Icon>,
}

/// Every command available now, in the list's order: the selected torrents', the selected files'
/// (while the Files list shows some selected), then the app's.
pub fn offered(s: &Phantom) -> Vec<Entry> {
    let selected = Selected::of(&s.table, &s.torrents, s.target_there);
    let can_delete = OnTorrents::Delete.available(&selected);
    let n = selected.torrents.len();
    let torrents = OnTorrents::ALL
        .into_iter()
        .filter(|c| s.mounted.offers(*c) && c.available(&selected))
        .map(|c| (Command::Torrents(c), c.wording(n, can_delete)));
    // Worded as the file menu words them.
    let files = on_files(s).into_iter().map(|c| (Command::Files(c), c.spec().name.to_owned()));
    let app = InApp::ALL.into_iter().filter(|c| c.available(s)).map(|c| (Command::App(c), c.wording(s)));
    torrents
        .chain(files)
        .chain(app)
        .map(|(command, label)| Entry { command, label, hint: command.spec().hint(), icon: command.spec().icon })
        .collect()
}

/// What the palette lists for its text: the commands offered whose label matches it, best first,
/// in the list's order among equals; all of them for no text.
pub fn listed(s: &Phantom) -> Vec<Entry> {
    let query = s.palette.query.text.trim();
    let mut scored: Vec<(i32, usize, Entry)> =
        offered(s).into_iter().enumerate().filter_map(|(i, e)| score(query, &e.label).map(|score| (score, i, e))).collect();
    scored.sort_by_key(|(score, i, _)| (-score, *i));
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
}

/// Escape, a click outside, or another overlay opening.
pub fn close(s: &mut Phantom) {
    s.palette.then(Event::Close);
    s.palette.query = TextEdit::default();
}

/// ↑: the highlight up one, the first going to the last.
pub fn up(s: &mut Phantom, tx: &mut Tx) {
    let n = listed(s).len();
    s.palette.then(Event::Up { n });
    tx.scroll_into_view(row_key(s.palette.at()));
}

/// ↓: the highlight down one, the last going to the first.
pub fn down(s: &mut Phantom, tx: &mut Tx) {
    let n = listed(s).len();
    s.palette.then(Event::Down { n });
    tx.scroll_into_view(row_key(s.palette.at()));
}

/// Return: runs the highlighted command, closing the palette first, so a command that opens
/// something (Rename…'s field, the folder picker) has the focus. With nothing listed, nothing.
pub fn run(s: &mut Phantom, tx: &mut Tx) {
    let Some(entry) = listed(s).into_iter().nth(s.palette.at()) else { return };
    close(s);
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
