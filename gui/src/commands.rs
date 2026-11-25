//! Every command the app offers, in one list (#33): each with its name, the icon a theme draws for
//! it, its keys, and when it's available. The row menu, the file menu, the command palette, the menu
//! bar (`menu_bar`), the empty list's hint and the keys (`shortcuts`) all read from here, so a name,
//! an icon or a key can't differ between two places.
//!
//! There are three kinds, each its own type, so a place offers only its own kind (the row menu holds
//! [`OnTorrents`], the file menu [`OnFiles`]):
//!
//! | Kind           | Acts on                                    | Offered by                      |
//! |----------------|--------------------------------------------|---------------------------------|
//! | [`OnTorrents`] | the selected torrents                      | the row menu, the palette, keys |
//! | [`OnFiles`]    | the selected files of the pane's torrent   | the file menu, keys in the list |
//! | [`InApp`]      | nothing selected: the app                  | the palette, the toolbar, keys  |
//!
//! What's available rests on three things: the part of the screen that offers it being shown
//! ([`Mounted`]: each view registers its commands as it's built, so a theme's screen that leaves one
//! out leaves it offered nowhere), the selection, and what the service lists as each torrent's
//! actions (`proto::Actions`), never on a guess: Open and Reveal also on the disk, looked at once as
//! the menu or the palette opens (`Phantom::target_there`, #307). What a command does is Phantom's
//! (`model`, [`run_on_torrents`]), never a view's.

use proto::{Action, Priority, TorrentInfo};
use rune_kit::TableState;

use crate::model::{Phantom, details_torrent, selected_in_pane, selected_torrents};
use crate::shortcuts::{Chord, Named, chord};
use crate::ui::Icon;

/// What a command is, wherever it's offered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    /// What a test, a script or a menu picks it by.
    pub id: &'static str,
    /// Its name: what the palette filters by, and the menus show unless they word it for the
    /// selection ([`OnTorrents::wording`]).
    pub name: &'static str,
    /// What a theme draws beside it, by `ui::Icon`, never a drawing: each theme resolves it
    /// (`Ui::icon`). None where it has none: the priorities, which the file menu marks with a check.
    pub icon: Option<Icon>,
    /// Its keys, the one shown first; none for most.
    pub keys: &'static [Chord],
}

impl Spec {
    /// The key shown beside it: its first.
    pub fn hint(&self) -> Option<String> {
        self.keys.first().map(|k| k.hint())
    }
}

const OPEN_KEYS: &[Chord] = &[chord(Named::Letter('o'), true, false), chord(Named::Return, false, false)];
const COPY_KEYS: &[Chord] = &[chord(Named::Letter('c'), true, false)];
/// Pauses or resumes the selection by its focused torrent ([`toggle`]), in the list.
pub const TOGGLE_KEY: Chord = chord(Named::Space, false, false);
const REVEAL_KEYS: &[Chord] = &[chord(Named::Letter('o'), true, true)];
const DELETE_KEYS: &[Chord] = &[chord(Named::Backspace, true, false)];
const REMOVE_KEYS: &[Chord] = &[chord(Named::Backspace, false, false)];
const ADD_FILE_KEYS: &[Chord] = &[chord(Named::Letter('a'), true, true)];
const UNDO_KEYS: &[Chord] = &[chord(Named::Letter('z'), true, false)];
const PALETTE_KEYS: &[Chord] = &[chord(Named::Letter('p'), true, false)];

/// What can be done to the selected torrents, in the order the row menu and the palette list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTorrents {
    Resume,
    Pause,
    Check,
    CancelCheck,
    CancelMove,
    Open,
    Reveal,
    MoveTo,
    Rename,
    /// Not the service's to decide: the link is made from what the listing has.
    CopyMagnet,
    /// Remove (keeping files), at once.
    Remove,
    Delete,
}

impl OnTorrents {
    pub const ALL: [OnTorrents; 12] = [
        OnTorrents::Resume,
        OnTorrents::Pause,
        OnTorrents::Check,
        OnTorrents::CancelCheck,
        OnTorrents::CancelMove,
        OnTorrents::Open,
        OnTorrents::Reveal,
        OnTorrents::MoveTo,
        OnTorrents::Rename,
        OnTorrents::CopyMagnet,
        OnTorrents::Remove,
        OnTorrents::Delete,
    ];

    pub fn spec(self) -> Spec {
        use client::display;
        let (id, name, icon, keys): (_, _, Option<Icon>, &'static [Chord]) = match self {
            OnTorrents::Resume => ("resume", "Resume", Some(Icon::Resume), &[]),
            OnTorrents::Pause => ("pause", "Pause", Some(Icon::Pause), &[]),
            OnTorrents::Check => ("check", "Check", Some(Icon::Verify), &[]),
            OnTorrents::CancelCheck => ("cancel-check", display::cancel_check(), Some(Icon::CancelCheck), &[]),
            OnTorrents::CancelMove => ("cancel-move", display::cancel_move(), Some(Icon::CancelMove), &[]),
            OnTorrents::Open => ("open", "Open", Some(Icon::Open), OPEN_KEYS),
            OnTorrents::Reveal => ("reveal", display::reveal_in_finder(), Some(Icon::Reveal), REVEAL_KEYS),
            OnTorrents::MoveTo => ("move", display::move_to(), Some(Icon::MoveTo), &[]),
            OnTorrents::Rename => ("rename", display::rename(), Some(Icon::Rename), &[]),
            OnTorrents::CopyMagnet => ("copy-magnet", "Copy magnet link", Some(Icon::Copy), COPY_KEYS),
            OnTorrents::Remove => ("remove", "Remove", Some(Icon::Remove), REMOVE_KEYS),
            OnTorrents::Delete => ("delete", "Delete", Some(Icon::Trash), DELETE_KEYS),
        };
        Spec { id, name, icon, keys }
    }

    /// The service's action it rests on: the torrents it's offered for are those whose actions do it.
    pub fn action(self) -> Option<Action> {
        Some(match self {
            OnTorrents::Resume => Action::Resume,
            OnTorrents::Pause => Action::Pause,
            OnTorrents::Check => Action::Check,
            OnTorrents::CancelCheck => Action::CancelCheck,
            OnTorrents::CancelMove => Action::CancelMove,
            OnTorrents::Open => Action::Open,
            OnTorrents::Reveal => Action::Reveal,
            OnTorrents::MoveTo => Action::Move,
            OnTorrents::Rename => Action::Rename,
            OnTorrents::CopyMagnet => return None,
            OnTorrents::Remove => Action::Remove,
            OnTorrents::Delete => Action::Delete,
        })
    }

    /// Whether it's offered for `selected`: Pause, Resume, Check, Cancel check and Cancel move for
    /// those they apply to, Move to… too (#270: those that can't move are skipped); Remove and
    /// Delete only where they apply to every one, as they act on all; Open, Reveal, Rename… and Copy
    /// magnet link for exactly one, Open and Reveal only while its location is on disk.
    pub fn available(self, selected: &Selected) -> bool {
        let torrents = &selected.torrents;
        let does = |t: &&TorrentInfo| self.action().is_none_or(|a| t.actions.does(a));
        let single = torrents.len() == 1;
        match self {
            OnTorrents::Resume | OnTorrents::Pause | OnTorrents::Check | OnTorrents::CancelCheck | OnTorrents::CancelMove => {
                torrents.iter().any(does)
            }
            OnTorrents::MoveTo => torrents.iter().any(does),
            OnTorrents::Open | OnTorrents::Reveal => single && selected.there && torrents.iter().all(does),
            OnTorrents::Rename | OnTorrents::CopyMagnet => single && torrents.iter().all(does),
            OnTorrents::Remove | OnTorrents::Delete => !torrents.is_empty() && torrents.iter().all(does),
        }
    }

    /// As the menus and the palette word it for `n` torrents: Remove and Delete with the count, and
    /// Remove saying it keeps the files when Delete is beside it; the others by their name.
    pub fn wording(self, n: usize, can_delete: bool) -> String {
        match self {
            OnTorrents::Remove => client::display::remove_choice(n, can_delete),
            OnTorrents::Delete => client::display::delete_choice(n),
            other => other.spec().name.into(),
        }
    }

    /// The row menu's groups, a separator between neighbours: what it does to the torrent, where its
    /// files are, its link, taking it off the list.
    pub fn group(self) -> u8 {
        match self {
            OnTorrents::Resume | OnTorrents::Pause | OnTorrents::Check | OnTorrents::CancelCheck | OnTorrents::CancelMove => 0,
            OnTorrents::Open | OnTorrents::Reveal | OnTorrents::MoveTo | OnTorrents::Rename => 1,
            OnTorrents::CopyMagnet => 2,
            OnTorrents::Remove | OnTorrents::Delete => 3,
        }
    }
}

/// The selected torrents as the service lists them, and whether the location Open and Reveal show
/// was on disk at the last look.
pub struct Selected<'a> {
    pub torrents: Vec<&'a TorrentInfo>,
    pub there: bool,
}

impl<'a> Selected<'a> {
    pub fn of(table: &TableState, torrents: &'a [TorrentInfo], there: bool) -> Selected<'a> {
        let ids = selected_torrents(table);
        Selected { torrents: torrents.iter().filter(|t| ids.contains(&t.id)).collect(), there }
    }
}

/// The torrent commands available for the selection, in the list's order: those the screen shown
/// offers.
pub fn on_torrents(s: &Phantom) -> Vec<OnTorrents> {
    let selected = Selected::of(&s.table, &s.torrents, s.target_there);
    OnTorrents::ALL.into_iter().filter(|c| s.mounted.offers(*c) && c.available(&selected)).collect()
}

/// What Space and Torrent ▸ Resume or Pause do to the selection, by its focused torrent (the
/// selected one the keyboard is on, else the first selected in the list): running, every selected
/// one pauses; paused, every one resumes. An Error counts as running. None with nothing selected.
pub fn toggle(s: &Phantom) -> Option<OnTorrents> {
    let selected = selected_torrents(&s.table);
    let cursor = s.table.cursor().and_then(|id| id.parse().ok()).filter(|id| selected.contains(id));
    let focused = s.torrents.iter().find(|t| cursor.is_none_or(|id| t.id == id) && selected.contains(&t.id))?;
    let running = matches!(focused.status, proto::Status::Error { .. }) || crate::model::toggle_icon(focused) == Action::Pause;
    Some(if running { OnTorrents::Pause } else { OnTorrents::Resume })
}

/// The commands the screen shown now offers, as its views registered them while they were built
/// (VS Code's `when` contexts, in effect): the torrent list registers the torrent commands, the
/// details pane's Files list the file commands. A command no view registered isn't offered anywhere:
/// not in a menu, the palette or by its keys. The app's own commands ([`InApp`]) need no view.
///
/// It's rebuilt with each frame's view ([`Mounted::begin`]); between frames, handlers read what
/// the last one registered.
#[derive(Default)]
pub struct Mounted(std::cell::RefCell<Vec<Command>>);

impl Mounted {
    /// A new frame's view begins: nothing is registered yet.
    pub fn begin(&self) {
        self.0.borrow_mut().clear();
    }

    /// A view that's being built offers `commands`.
    pub fn offer<C: Into<Command>>(&self, commands: impl IntoIterator<Item = C>) {
        self.0.borrow_mut().extend(commands.into_iter().map(Into::into));
    }

    /// Whether the screen shown offers `command`: a view registered it, or it's the app's.
    pub fn offers(&self, command: impl Into<Command>) -> bool {
        let command = command.into();
        matches!(command, Command::App(_)) || self.0.borrow().contains(&command)
    }
}

/// What can be done to the selected files of the torrent the details pane shows (#151).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFiles {
    Open,
    Reveal,
    /// Sets every selected file to this priority; checked while they all have it.
    Priority(Priority),
    /// Sets them Off, then moves those on disk to the Trash.
    MoveToTrash,
}

impl OnFiles {
    pub const ALL: [OnFiles; 5] =
        [OnFiles::Open, OnFiles::Reveal, OnFiles::Priority(Priority::Normal), OnFiles::Priority(Priority::Off), OnFiles::MoveToTrash];

    /// The same keys as the torrent commands that do the same to a torrent: the file list keeps them
    /// for files while it has the focus.
    pub fn spec(self) -> Spec {
        use client::display;
        let (id, name, icon, keys): (_, _, Option<Icon>, &'static [Chord]) = match self {
            OnFiles::Open => ("open", "Open", Some(Icon::Open), OPEN_KEYS),
            OnFiles::Reveal => ("reveal", display::reveal_in_finder(), Some(Icon::Reveal), REVEAL_KEYS),
            OnFiles::Priority(Priority::Normal) => ("priority-normal", display::priority(Priority::Normal), None, &[]),
            OnFiles::Priority(Priority::Off) => ("priority-off", display::priority(Priority::Off), None, &[]),
            OnFiles::MoveToTrash => ("trash", display::move_to_trash(), Some(Icon::Trash), DELETE_KEYS),
        };
        Spec { id, name, icon, keys }
    }

    /// Whether it's offered for the selected files of `torrent`: only what the service lists for
    /// the torrent (Set priority, and Move to Trash, which does what Set priority does, while a
    /// selected file is on disk: it moves only those), and Open and Reveal for exactly one file
    /// that's on disk, as the service says and as `there` says: the disk looked at when the menu
    /// opened (#307). On disk is the service's word for it (#276): what's at the file's path, not
    /// what it downloaded.
    pub fn available(self, torrent: &TorrentInfo, files: &[&proto::File], there: bool) -> bool {
        let on_disk = |f: &&proto::File| f.on_disk && !f.missing;
        match self {
            OnFiles::Open => there && matches!(files, [f] if on_disk(f)) && torrent.actions.does(Action::Open),
            OnFiles::Reveal => there && matches!(files, [f] if on_disk(f)) && torrent.actions.does(Action::Reveal),
            OnFiles::Priority(_) => torrent.actions.does(Action::SetPriority),
            OnFiles::MoveToTrash => files.iter().any(on_disk) && torrent.actions.does(Action::TrashFiles),
        }
    }

    /// The file menu's groups: where it is, the priorities under their title, the Trash.
    pub fn group(self) -> u8 {
        match self {
            OnFiles::Open | OnFiles::Reveal => 0,
            OnFiles::Priority(_) => 1,
            OnFiles::MoveToTrash => 2,
        }
    }
}

/// The file commands available for the selected files of the torrent the pane shows, in the list's
/// order; none with no file selected.
pub fn on_files(s: &Phantom) -> Vec<OnFiles> {
    let Some((torrent, files)) = selected_in_pane(&s.table, &s.torrents, &s.pane) else { return Vec::new() };
    OnFiles::ALL.into_iter().filter(|c| s.mounted.offers(*c) && c.available(torrent, &files, s.file_there)).collect()
}

/// What isn't about a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InApp {
    AddFile,
    /// The newest change, a torrent action's or a setting's (#255); the menu bar's Edit ▸ Undo too.
    Undo,
    /// Opens the command palette.
    Palette,
}

impl InApp {
    pub const ALL: [InApp; 3] = [InApp::AddFile, InApp::Undo, InApp::Palette];

    pub fn spec(self) -> Spec {
        let (id, name, icon, keys): (_, _, Option<Icon>, &'static [Chord]) = match self {
            InApp::AddFile => ("add-file", "Add file…", Some(Icon::AddFile), ADD_FILE_KEYS),
            InApp::Undo => ("undo", client::display::undo(), Some(Icon::Undo), UNDO_KEYS),
            InApp::Palette => ("palette", "Command palette…", Some(Icon::Search), PALETTE_KEYS),
        };
        Spec { id, name, icon, keys }
    }

    /// Whether the palette offers it now: Undo while there's a change to take back; the palette
    /// never offers itself.
    pub fn available(self, s: &Phantom) -> bool {
        match self {
            InApp::AddFile => true,
            InApp::Undo => s.changes.state.top().is_some(),
            InApp::Palette => false,
        }
    }

    /// As the palette words it: Undo names what it takes back, as Edit ▸ Undo does ("Undo Remove of
    /// Album").
    pub fn wording(self, s: &Phantom) -> String {
        match self {
            InApp::Undo => crate::changes::menu_title(s),
            other => other.spec().name.into(),
        }
    }
}

/// Any command, of any kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Torrents(OnTorrents),
    Files(OnFiles),
    App(InApp),
}

impl Command {
    pub fn spec(self) -> Spec {
        match self {
            Command::Torrents(c) => c.spec(),
            Command::Files(c) => c.spec(),
            Command::App(c) => c.spec(),
        }
    }

    /// Every command, each kind in its list's order.
    pub fn all() -> impl Iterator<Item = Command> {
        let torrents = OnTorrents::ALL.into_iter().map(Command::Torrents);
        let files = OnFiles::ALL.into_iter().map(Command::Files);
        torrents.chain(files).chain(InApp::ALL.into_iter().map(Command::App))
    }
}

impl From<OnTorrents> for Command {
    fn from(c: OnTorrents) -> Command {
        Command::Torrents(c)
    }
}

impl From<OnFiles> for Command {
    fn from(c: OnFiles) -> Command {
        Command::Files(c)
    }
}

impl From<InApp> for Command {
    fn from(c: InApp) -> Command {
        Command::App(c)
    }
}

/// Does what `command` says to the selected torrents, as the row menu and the palette run it: Open,
/// Reveal and Rename… on the one selected, the others on every one selected they do something to.
pub fn run_on_torrents(s: &mut Phantom, command: OnTorrents, tx: &mut rune::Tx) {
    use crate::model::{command as send_command, copy_magnet, remove_selected, selected_for};
    use crate::platform::{Show, show_torrent};
    use proto::TorrentCommand;
    let send = |s: &mut Phantom, action: Action, what: TorrentCommand| {
        send_command(&mut *s.service, &selected_for(&s.table, &s.torrents, action), what)
    };
    let single = details_torrent(&s.table, &s.torrents).map(|t| t.id);
    match command {
        OnTorrents::Resume => send(s, Action::Resume, TorrentCommand::Resume),
        OnTorrents::Pause => send(s, Action::Pause, TorrentCommand::Pause),
        OnTorrents::Check => send(s, Action::Check, TorrentCommand::Check),
        OnTorrents::CancelCheck => send(s, Action::CancelCheck, TorrentCommand::CancelCheck),
        OnTorrents::CancelMove => send(s, Action::CancelMove, TorrentCommand::CancelMove),
        OnTorrents::Remove => remove_selected(s, false),
        OnTorrents::Delete => remove_selected(s, true),
        OnTorrents::MoveTo => crate::model::moving::move_to(s, tx),
        OnTorrents::Rename => single.into_iter().for_each(|id| crate::model::renaming::open(s, id)),
        OnTorrents::Open => single.into_iter().for_each(|id| show_torrent(s, id, Show::Open)),
        OnTorrents::Reveal => single.into_iter().for_each(|id| show_torrent(s, id, Show::Reveal)),
        OnTorrents::CopyMagnet => copy_magnet(s, tx),
    }
}

/// Does what `command` says.
pub fn run_in_app(s: &mut Phantom, command: InApp, tx: &mut rune::Tx) {
    match command {
        InApp::AddFile => crate::views::toolbar::add_torrent_file(s, tx),
        InApp::Undo => crate::changes::press(s),
        InApp::Palette => crate::model::palette::open(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proto::{Answer, Status};

    fn torrent(id: u64, does: &[Action]) -> TorrentInfo {
        let actions =
            proto::Actions::new(|a| if does.contains(&a) { Answer::Does } else { Answer::DoesNothing { why: proto::Why::AlreadyStopped } });
        TorrentInfo {
            id,
            info_hash: format!("{id:040x}"),
            name: Some(format!("t{id}")),
            status: Status::Downloading,
            content: None,
            uploaded_bytes: 0,
            download_bytes_per_sec: 0,
            upload_bytes_per_sec: 0,
            live_peers: 0,
            original_name: None,
            trackers: Vec::new(),
            location: None,
            actions,
        }
    }

    const EVERY: [Action; 9] = [
        Action::Resume,
        Action::Check,
        Action::Open,
        Action::Reveal,
        Action::Move,
        Action::Rename,
        Action::Remove,
        Action::Delete,
        Action::CancelMove,
    ];

    /// Every torrent command, for every selection of up to three torrents that each do some of
    /// the actions, on disk or not: offered exactly as its rule says.
    #[test]
    fn a_torrent_command_is_offered_by_its_rule_for_every_selection() {
        let sets: [&[Action]; 4] =
            [&[], &EVERY, &[Action::Pause, Action::Remove], &[Action::Resume, Action::Delete, Action::Remove, Action::Open]];
        for n in 0..=3usize {
            for pick in 0..sets.len().pow(n as u32) {
                let torrents: Vec<TorrentInfo> =
                    (0..n).map(|i| torrent(i as u64, sets[pick / sets.len().pow(i as u32) % sets.len()])).collect();
                for there in [false, true] {
                    let selected = Selected { torrents: torrents.iter().collect(), there };
                    for c in OnTorrents::ALL {
                        let does = |t: &TorrentInfo| c.action().is_none_or(|a| t.actions.does(a));
                        let (any, all) = (torrents.iter().any(does), n > 0 && torrents.iter().all(does));
                        let want = match c {
                            OnTorrents::Remove | OnTorrents::Delete => all,
                            OnTorrents::Open | OnTorrents::Reveal => n == 1 && there && all,
                            OnTorrents::Rename | OnTorrents::CopyMagnet => n == 1 && all,
                            _ => any,
                        };
                        assert_eq!(c.available(&selected), want, "{c:?} for {n} torrents ({pick}), there: {there}");
                    }
                }
            }
        }
    }

    /// With nothing selected no torrent command is offered.
    #[test]
    fn nothing_selected_offers_no_torrent_command() {
        let selected = Selected { torrents: Vec::new(), there: true };
        assert!(OnTorrents::ALL.iter().all(|c| !c.available(&selected)));
    }

    /// Every command has its own id within its kind, a name, and keys no other command of the places
    /// they're pressed in has: the torrent list's (torrent and app commands) and the file list's
    /// (file and app commands).
    #[test]
    fn ids_and_keys_are_each_commands_own() {
        let torrent_list: Vec<Command> = Command::all().filter(|c| !matches!(c, Command::Files(_))).collect();
        let file_list: Vec<Command> = Command::all().filter(|c| !matches!(c, Command::Torrents(_))).collect();
        for place in [torrent_list, file_list] {
            for (i, a) in place.iter().enumerate() {
                assert!(!a.spec().name.is_empty());
                for b in &place[i + 1..] {
                    assert_ne!(a.spec().id, b.spec().id, "{a:?} and {b:?}");
                    for key in a.spec().keys {
                        assert!(!b.spec().keys.contains(key), "{key:?} is both {a:?}'s and {b:?}'s");
                    }
                }
            }
        }
    }

    /// A command acting at once has no "…"; one that asks for more first (a folder, a name, a file)
    /// ends with it (Apple's rule).
    #[test]
    fn an_ellipsis_ends_only_a_command_that_asks_first() {
        for c in Command::all() {
            let asks =
                matches!(c, Command::Torrents(OnTorrents::MoveTo | OnTorrents::Rename) | Command::App(InApp::AddFile | InApp::Palette));
            assert_eq!(c.spec().name.ends_with('…'), asks, "{c:?}");
        }
    }

    /// The same deed has the same icon and keys, whether on torrents or on files.
    #[test]
    fn a_file_command_looks_like_the_torrent_command_it_mirrors() {
        for (file, torrent) in
            [(OnFiles::Open, OnTorrents::Open), (OnFiles::Reveal, OnTorrents::Reveal), (OnFiles::MoveToTrash, OnTorrents::Delete)]
        {
            assert_eq!((file.spec().icon, file.spec().keys), (torrent.spec().icon, torrent.spec().keys), "{file:?}");
        }
    }
}
