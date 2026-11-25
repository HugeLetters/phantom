//! The app's keys, read from the command list (`commands`): [`on_key`] runs them, and the menus,
//! the palette and the tooltips show their hints from the same [`Spec`](crate::commands::Spec), so
//! a hint and its binding can't disagree. A double-click on a row opens it too, but that's a
//! pointer gesture, not a shortcut.
//!
//! | Command    | Keys                           |
//! |------------|--------------------------------|
//! | Open       | ⌘O, Return                     |
//! | Reveal     | ⇧⌘O                            |
//! | Add file…  | ⇧⌘A                            |
//! | Remove     | ⌫, with no confirmation        |
//! | Delete     | ⌘⌫, with no confirmation       |
//! | Copy       | ⌘C: each selected one's link   |
//! | Resume or Pause | Space in the list ([`on_list_key`]) |
//! | Undo       | ⌘Z: the newest change          |
//! | Commands…  | ⌘P: the command palette        |
//!
//! With the file list focused, Open, Reveal and ⌘⌫ are the file commands with those keys, about the
//! selected files instead of the torrent (⌘⌫ is Move to Trash, which sets them Off and moves them).
//! The torrent list keeps them for torrents.
//!
//! Keys a focused control takes (Return and ⌫ in the link field) are its own: Rune hands the app
//! only those the focused control leaves.

use rune::{Key, KeyEvent, Mods, Tx};

use crate::commands::{Command, OnFiles, OnTorrents, TOGGLE_KEY, on_files, toggle};
use crate::model::{Phantom, details_torrent, look_at_file, remove_selected, selected_torrents};
use crate::platform::{Show, open_torrent, press_torrent};

/// A key with the modifiers that must be held, and no others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: Named,
    pub cmd: bool,
    pub shift: bool,
}

/// The keys the list uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Letter(char),
    Return,
    Space,
    Backspace,
}

pub const fn chord(key: Named, cmd: bool, shift: bool) -> Chord {
    Chord { key, cmd, shift }
}

impl Chord {
    /// As macOS menus show it: modifiers in their order (⇧ before ⌘), then the key.
    pub fn hint(self) -> String {
        let key = match self.key {
            Named::Letter(c) => c.to_ascii_uppercase().to_string(),
            Named::Space => "Space".into(),
            Named::Return => "↩".into(),
            Named::Backspace => "⌫".into(),
        };
        format!("{}{}{key}", if self.shift { "⇧" } else { "" }, if self.cmd { "⌘" } else { "" })
    }

    /// As Rune parses a menu's key: "cmd+shift+o".
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn accelerator(self) -> String {
        let key = match self.key {
            Named::Letter(',') => "comma".into(),
            Named::Letter(c) => c.to_string(),
            Named::Return => "enter".into(),
            Named::Space => "space".into(),
            Named::Backspace => "backspace".into(),
        };
        format!("{}{}{key}", if self.cmd { "cmd+" } else { "" }, if self.shift { "shift+" } else { "" })
    }

    /// Whether `ev` is this chord. Option and Control mustn't be held.
    pub fn matches(self, ev: &KeyEvent) -> bool {
        let Mods { shift, cmd, alt, ctrl, .. } = ev.mods;
        let key = match (&ev.key, self.key) {
            (Key::Command(c), Named::Letter(l)) => c.eq_ignore_ascii_case(&l),
            (Key::Enter, Named::Return) | (Key::Backspace, Named::Backspace) => true,
            (Key::Text(t), Named::Space) => t == " ",
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
            Named::Space => Key::Text(" ".into()),
            Named::Return => Key::Enter,
            Named::Backspace => Key::Backspace,
        };
        KeyEvent { key, mods: Mods { shift: self.shift, cmd: self.cmd, ..Mods::default() } }
    }
}

/// The command's keys, the shown one first.
#[cfg(test)]
pub fn keys(command: impl Into<Command>) -> &'static [Chord] {
    command.into().spec().keys
}

/// The command's hint: its first key.
pub fn hint(command: impl Into<Command>) -> String {
    let command = command.into();
    command.spec().hint().unwrap_or_else(|| panic!("{command:?} has no key"))
}

/// The command `ev` is in the torrent list (a torrent's or the app's), if any.
pub fn command_of(ev: &KeyEvent) -> Option<Command> {
    let pressed = |c: &Command| c.spec().keys.iter().any(|k| k.matches(ev));
    Command::all().filter(|c| !matches!(c, Command::Files(_))).find(pressed)
}

/// The command in the torrent list whose shown hint is `hint`, if any.
#[cfg(test)]
pub fn by_hint(shown: &str) -> Option<Command> {
    Command::all().filter(|c| !matches!(c, Command::Files(_))).find(|c| c.spec().hint().as_deref() == Some(shown))
}

/// Runs the shortcut `ev` is, while nothing modal is open (a menu, the palette, a dialog); whether
/// it was one.
pub fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.menu.is_open() || s.palette.is_open() || s.clash.is_some() {
        return false;
    }
    let Some(command) = command_of(ev).filter(|c| s.mounted.offers(*c)) else { return false };
    // Return is a direct request: an Open of a gone location says so; the ⌘ keys stay silent (#312).
    match (command, ev.key == Key::Enter) {
        (Command::Torrents(OnTorrents::Open), true) => {
            details_torrent(&s.table, &s.torrents).map(|t| t.id).into_iter().for_each(|id| open_torrent(s, id))
        }
        (Command::Torrents(c), _) => run(s, c, tx),
        (Command::App(c), _) => crate::commands::run_in_app(s, c, tx),
        (Command::Files(_), _) => unreachable!("the torrent list's keys are no file command's"),
    }
    true
}

/// Space while the torrent list has the focus, before the list itself sees it: the selection pauses
/// or resumes, once per press. Whether it was that.
pub fn on_list_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !TOGGLE_KEY.matches(ev) || s.menu.is_open() || s.palette.is_open() || s.clash.is_some() {
        return false;
    }
    if let Some(command) = toggle(s).filter(|c| s.mounted.offers(*c) && !tx.key_repeat()) {
        crate::commands::run_on_torrents(s, command, tx);
    }
    true
}

/// The shortcuts while the file list has focus: Open, Reveal and ⌘⌫ act on the selected files, where
/// the file menu offers them, and are the list's even where it doesn't (nothing is selected, or the
/// file is off disk), so a key meant for a file never reaches the torrent. Whether it was one.
pub fn on_file_key(s: &mut Phantom, ev: &KeyEvent) -> bool {
    if s.menu.is_open() || s.palette.is_open() || s.clash.is_some() {
        return false;
    }
    let pressed = OnFiles::ALL.into_iter().find(|c| c.spec().keys.iter().any(|k| k.matches(ev)));
    let Some(command) = pressed.filter(|c| s.mounted.offers(*c)) else { return false };
    look_at_file(s);
    if on_files(s).contains(&command) {
        crate::views::file_menu::run(s, command);
    }
    true
}

/// What a torrent command's key does to the selection: Open and Reveal for exactly one torrent,
/// looking at the disk first, Remove and Delete for every one selected (only where it applies to
/// them all, as the menu offers it). The others have no key.
fn run(s: &mut Phantom, command: OnTorrents, tx: &mut Tx) {
    let single = details_torrent(&s.table, &s.torrents).map(|t| t.id);
    match command {
        OnTorrents::Open => single.into_iter().for_each(|id| press_torrent(s, id, Show::Open)),
        OnTorrents::Reveal => single.into_iter().for_each(|id| press_torrent(s, id, Show::Reveal)),
        OnTorrents::Remove => remove_all(s, proto::Action::Remove),
        OnTorrents::Delete => remove_all(s, proto::Action::Delete),
        OnTorrents::CopyMagnet => crate::model::copy_magnet(s, tx),
        other => unreachable!("{other:?} has no key"),
    }
}

/// Removes (or deletes) the selection at once, only where `action` applies to every one selected,
/// as the row menu offers it.
fn remove_all(s: &mut Phantom, action: proto::Action) {
    let selected = selected_torrents(&s.table);
    let all = !selected.is_empty() && s.torrents.iter().filter(|t| selected.contains(&t.id)).all(|t| t.actions.does(action));
    if all {
        remove_selected(s, action == proto::Action::Delete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::InApp;

    /// Every key of every command of the torrent list is that command's and no other's, and every
    /// hint names its own.
    #[test]
    fn each_key_and_hint_is_its_commands_alone() {
        for command in Command::all().filter(|c| !matches!(c, Command::Files(_))) {
            for key in command.spec().keys {
                assert_eq!(command_of(&key.event()), Some(command), "{key:?}");
            }
            if let Some(shown) = command.spec().hint() {
                assert_eq!(by_hint(&shown), Some(command), "{command:?}");
            }
        }
    }

    #[test]
    fn hints_read_as_macos_menus_show_them() {
        let hints: Vec<String> = [
            hint(OnTorrents::Open),
            hint(OnTorrents::Reveal),
            hint(InApp::AddFile),
            hint(OnTorrents::Remove),
            hint(OnTorrents::Delete),
            hint(InApp::Undo),
            hint(InApp::Palette),
        ]
        .into();
        assert_eq!(hints, ["⌘O", "⇧⌘O", "⇧⌘A", "⌫", "⌘⌫", "⌘Z", "⌘P"]);
        assert_eq!(keys(OnTorrents::Open).iter().map(|k| k.hint()).collect::<Vec<_>>(), ["⌘O", "↩"]);
    }

    /// Option, Control or a missing modifier make it another key.
    #[test]
    fn a_chord_needs_exactly_its_modifiers() {
        let backspace = |mods| KeyEvent { key: Key::Backspace, mods };
        assert_eq!(command_of(&backspace(Mods::default())), Some(OnTorrents::Remove.into()));
        assert_eq!(command_of(&backspace(Mods { cmd: true, ..Mods::default() })), Some(OnTorrents::Delete.into()));
        assert_eq!(command_of(&backspace(Mods { alt: true, ..Mods::default() })), None);
        assert_eq!(
            command_of(&KeyEvent { key: Key::Command('o'), mods: Mods { shift: true, ..Mods::default() } }),
            Some(OnTorrents::Reveal.into())
        );
        assert_eq!(command_of(&KeyEvent { key: Key::Down, mods: Mods::default() }), None, "↓ alone moves the selection");
        let z = |shift| KeyEvent { key: Key::Command('z'), mods: Mods { shift, ..Mods::default() } };
        assert_eq!((command_of(&z(false)), command_of(&z(true))), (Some(InApp::Undo.into()), None), "no redo");
    }
}
