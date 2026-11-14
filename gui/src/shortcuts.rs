//! The app's keyboard shortcuts, from one table: [`on_key`] runs them, and the row menu and the
//! tooltips show their hints from it, so a hint and its binding can't disagree. A double-click on
//! a row opens it too, but that's a pointer gesture, not a shortcut.
//!
//! | Command    | Keys                           |
//! |------------|--------------------------------|
//! | Open       | ⌘O, ⌘↓, Return                 |
//! | Reveal     | ⇧⌘O                            |
//! | Add file…  | ⇧⌘A                            |
//! | Remove     | ⌫, with no confirmation        |
//! | Delete     | ⌘⌫, with no confirmation       |
//!
//! Keys a focused control takes (Return and ⌫ in the link field) are its own: Rune hands the app
//! only those the focused control leaves.

use rune::{Key, KeyEvent, Mods, Tx};

use crate::model::{Phantom, details_torrent, remove_selected, selected_torrents};
use crate::platform::{Show, show_torrent};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Open,
    Reveal,
    AddFile,
    /// Remove (keeping files), at once.
    Remove,
    Delete,
}

/// A key with the modifiers that must be held, and no others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: Named,
    pub cmd: bool,
    pub shift: bool,
}

/// The keys the table uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Letter(char),
    Down,
    Return,
    Backspace,
}

const fn chord(key: Named, cmd: bool, shift: bool) -> Chord {
    Chord { key, cmd, shift }
}

/// Every command with its keys; the first is the one shown.
pub const TABLE: [(Command, &[Chord]); 5] = [
    (Command::Open, &[chord(Named::Letter('o'), true, false), chord(Named::Down, true, false), chord(Named::Return, false, false)]),
    (Command::Reveal, &[chord(Named::Letter('o'), true, true)]),
    (Command::AddFile, &[chord(Named::Letter('a'), true, true)]),
    (Command::Remove, &[chord(Named::Backspace, false, false)]),
    (Command::Delete, &[chord(Named::Backspace, true, false)]),
];

impl Chord {
    /// As macOS menus show it: modifiers in their order (⇧ before ⌘), then the key.
    pub fn hint(self) -> String {
        let key = match self.key {
            Named::Letter(c) => c.to_ascii_uppercase().to_string(),
            Named::Down => "↓".into(),
            Named::Return => "↩".into(),
            Named::Backspace => "⌫".into(),
        };
        format!("{}{}{key}", if self.shift { "⇧" } else { "" }, if self.cmd { "⌘" } else { "" })
    }

    /// Whether `ev` is this chord. Option and Control mustn't be held.
    pub fn matches(self, ev: &KeyEvent) -> bool {
        let Mods { shift, cmd, alt, ctrl, .. } = ev.mods;
        let key = match (&ev.key, self.key) {
            (Key::Command(c), Named::Letter(l)) => c.eq_ignore_ascii_case(&l),
            (Key::Down, Named::Down) | (Key::Enter, Named::Return) | (Key::Backspace, Named::Backspace) => true,
            _ => false,
        };
        // A Cmd letter arrives as `Key::Command` whether or not `mods.cmd` says so.
        let cmd_held = cmd || matches!(ev.key, Key::Command(_));
        key && cmd_held == self.cmd && shift == self.shift && !alt && !ctrl
    }

    /// The key event that is this chord.
    #[cfg(test)]
    pub fn event(self) -> KeyEvent {
        let key = match self.key {
            Named::Letter(c) => Key::Command(c),
            Named::Down => Key::Down,
            Named::Return => Key::Enter,
            Named::Backspace => Key::Backspace,
        };
        KeyEvent { key, mods: Mods { shift: self.shift, cmd: self.cmd, ..Mods::default() } }
    }
}

/// The command's keys, the shown one first.
pub fn keys(command: Command) -> &'static [Chord] {
    TABLE.iter().find(|(c, _)| *c == command).map(|(_, keys)| *keys).expect("every command is in the table")
}

/// The command's hint: its first key.
pub fn hint(command: Command) -> String {
    keys(command)[0].hint()
}

/// The command `ev` is, if any.
pub fn command_of(ev: &KeyEvent) -> Option<Command> {
    TABLE.iter().find(|(_, keys)| keys.iter().any(|k| k.matches(ev))).map(|(c, _)| *c)
}

/// The command whose shown hint is `hint`, if any.
#[cfg(test)]
pub fn by_hint(shown: &str) -> Option<Command> {
    TABLE.iter().find(|(c, _)| hint(*c) == shown).map(|(c, _)| *c)
}

/// Runs the shortcut `ev` is, while nothing modal is open; whether it was one.
pub fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.settings.is_some() || s.menu.is_open() {
        return false;
    }
    let Some(command) = command_of(ev) else { return false };
    run(s, command, tx);
    true
}

/// What the command does to the selection: Open and Reveal for exactly one torrent, Remove and
/// Delete for every one selected (only where it applies to them all, as the menu offers it).
pub fn run(s: &mut Phantom, command: Command, tx: &mut Tx) {
    let single = details_torrent(&s.table, &s.torrents).map(|t| t.id);
    match command {
        Command::Open => single.into_iter().for_each(|id| show_torrent(s, id, Show::Open)),
        Command::Reveal => single.into_iter().for_each(|id| show_torrent(s, id, Show::Reveal)),
        Command::AddFile => crate::views::toolbar::add_torrent_file(s, tx),
        Command::Remove => remove_all(s, proto::Action::Remove),
        Command::Delete => remove_all(s, proto::Action::Delete),
    }
}

/// Removes (or deletes) the selection at once, only where `action` applies to every one selected,
/// as the row menu offers it.
fn remove_all(s: &mut Phantom, action: proto::Action) {
    let selected = selected_torrents(&s.table);
    let all = !selected.is_empty() && s.torrents.iter().filter(|t| selected.contains(&t.id)).all(|t| t.actions.contains(&action));
    if all {
        remove_selected(&mut *s.service, &mut s.table, action == proto::Action::Delete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMANDS: [Command; 5] = [Command::Open, Command::Reveal, Command::AddFile, Command::Remove, Command::Delete];

    /// Every key of every command is that command's and no other's, and every hint names its own.
    #[test]
    fn each_key_and_hint_is_its_commands_alone() {
        for (command, keys) in TABLE {
            for key in keys {
                assert_eq!(command_of(&key.event()), Some(command), "{key:?}");
            }
            assert_eq!(by_hint(&hint(command)), Some(command), "{command:?}");
        }
        assert!(COMMANDS.iter().all(|c| TABLE.iter().any(|(t, _)| t == c)), "every command has keys");
    }

    #[test]
    fn hints_read_as_macos_menus_show_them() {
        let hints: Vec<String> = COMMANDS.map(hint).into();
        assert_eq!(hints, ["⌘O", "⇧⌘O", "⇧⌘A", "⌫", "⌘⌫"]);
        assert_eq!(keys(Command::Open).iter().map(|k| k.hint()).collect::<Vec<_>>(), ["⌘O", "⌘↓", "↩"]);
    }

    /// Option, Control or a missing modifier make it another key.
    #[test]
    fn a_chord_needs_exactly_its_modifiers() {
        let backspace = |mods| KeyEvent { key: Key::Backspace, mods };
        assert_eq!(command_of(&backspace(Mods::default())), Some(Command::Remove));
        assert_eq!(command_of(&backspace(Mods { cmd: true, ..Mods::default() })), Some(Command::Delete));
        assert_eq!(command_of(&backspace(Mods { alt: true, ..Mods::default() })), None);
        assert_eq!(command_of(&KeyEvent { key: Key::Command('o'), mods: Mods { shift: true, ..Mods::default() } }), Some(Command::Reveal));
        assert_eq!(command_of(&KeyEvent { key: Key::Down, mods: Mods::default() }), None, "↓ alone moves the selection");
    }
}
