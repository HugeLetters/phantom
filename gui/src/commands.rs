//! Every command the app offers, each declared once (#33, #486): its name, the icon a theme draws
//! for it, its keys, its synonyms, where it can appear at all ([`Scope`]), when it can be done there
//! (`when`) and what it does (`run`), in one [`Decl`]. The menu bar (`menu_bar`), the row menu, the
//! file menu, the command palette, the empty list's hint, the tooltips and the keys (`shortcuts`)
//! all read it, so a name, an icon, a key or a rule can't differ between two places. A new command
//! is a variant and its arm in its kind's `decl`, in this file alone.
//!
//! There are four kinds, each its own type, so a place offers only its own kind (the row menu holds
//! [`OnTorrents`], the file menu [`OnFiles`]):
//!
//! | Kind           | Acts on                                    | Offered by                      |
//! |----------------|--------------------------------------------|---------------------------------|
//! | [`OnTorrents`] | the selected torrents                      | the row menu, the palette, keys |
//! | [`OnFiles`]    | the selected files of the pane's torrent   | the file menu, keys in the list |
//! | [`InApp`]      | nothing selected: the app                  | the palette, the toolbar, keys  |
//! | [`OnSheet`]    | the Look sheet open over Settings          | the palette, in Settings        |
//!
//! Where a command is offered is graded ([`Command::offer`], an [`Offer`]) from the [`Context`] a
//! surface is in:
//!
//! | `scope` holds | `when` holds | Offer      | Palette | Menu bar | Row and file menus |
//! |---------------|--------------|------------|---------|----------|--------------------|
//! | no            |              | `Hidden`   | –       | –        | –                  |
//! | yes           | no           | `Disabled` | –       | greyed   | –                  |
//! | yes           | yes, or none | `Enabled`  | listed  | enabled  | listed             |
//!
//! `when` rests on three things: the part of the screen that offers it being shown ([`Mounted`]:
//! each view registers its commands as it's built, so a theme's screen that leaves one out leaves it
//! offered nowhere), the selection, and what the service lists as each torrent's actions
//! (`proto::Actions`), never on a guess: Open and Reveal also on the disk, looked at once as the
//! menu or the palette opens (`Phantom::target_there`, #307). What a command does is Phantom's
//! (`model`), never a view's.

use proto::{Action, Priority, TorrentCommand, TorrentInfo};
use rune::Tx;
use rune_kit::TableState;

use crate::listed::listed_enum;
use crate::model::settings::{SettingsWindow, Side, Tab};
use crate::model::{Phantom, details_torrent, selected_in_pane, selected_torrents};
use crate::shortcuts::{Chord, Named, chord};
use crate::theme::preset::{Appearance, LookPreset};
use crate::ui::Icon;
use crate::windows::Which;

/// A command, declared once: `C` is its kind, `H` the window it acts in.
pub struct Decl<C: 'static, H: 'static> {
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
    /// Other names the palette finds it by, never shown.
    pub synonyms: &'static [&'static str],
    /// Where it can appear at all.
    pub scope: Scope,
    /// Whether it can be done now, where it appears; none: always.
    pub when: Option<fn(&Context, C) -> bool>,
    /// What it does.
    pub run: fn(&mut H, C, &mut Tx),
}

impl<C, H> Decl<C, H> {
    pub fn spec(&self) -> Spec {
        Spec { id: self.id, name: self.name, icon: self.icon, keys: self.keys, synonyms: self.synonyms }
    }
}

/// What every surface shows of a command, whatever its kind: the window-free part of its [`Decl`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: Option<Icon>,
    pub keys: &'static [Chord],
    pub synonyms: &'static [&'static str],
}

impl Spec {
    /// The key shown beside it: its first.
    pub fn hint(&self) -> Option<String> {
        self.keys.first().map(|k| k.hint())
    }
}

listed_enum! {
    /// What's open over the Settings window, the topmost; the main window has none.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Sheet {
        Closed,
        /// A Look Preset's editor sheet.
        Look,
        /// The App icon's sheet.
        AppIcon,
        /// The App icon's Customize, over its sheet.
        Customize,
        /// Save as…, naming the Active look.
        SaveAs,
    }
}

/// Where an entry (a command, a setting, a place) can appear at all: in these windows, with one of
/// these open over it. Plain data, so a test can list every place an entry can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub windows: &'static [Which],
    pub sheets: &'static [Sheet],
}

impl Scope {
    /// The main window: the selection's commands and the app's.
    pub const MAIN: Scope = Scope { windows: &[Which::Main], sheets: &[Sheet::Closed] };
    /// Both windows, with nothing over Settings: what's behind a sheet isn't offered, the sheet
    /// being modal.
    pub const CLOSED: Scope = Scope { windows: &Which::ALL, sheets: &[Sheet::Closed] };
    /// Both windows, and over the Look sheet, which edits it: a setting of the Look.
    pub const LOOK: Scope = Scope { windows: &Which::ALL, sheets: &[Sheet::Closed, Sheet::Look] };
    /// The Look sheet's own.
    pub const LOOK_SHEET: Scope = Scope { windows: &[Which::Settings], sheets: &[Sheet::Look] };
    /// Anywhere.
    pub const EVERYWHERE: Scope = Scope { windows: &Which::ALL, sheets: &Sheet::ALL };

    pub fn holds(&self, window: Which, sheet: Sheet) -> bool {
        self.windows.contains(&window) && self.sheets.contains(&sheet)
    }
}

/// How a surface offers a command in a context ([`Command::offer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Out of its scope: shown nowhere.
    Hidden,
    /// In its scope, but it can't be done now: the menu bar greys it, the rest leave it out.
    Disabled,
    Enabled,
}

/// What has the keyboard, as the menu bar sees it; the palette and the menus, which are what has
/// it while they're open, are in [`Focus::List`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The window is in front, nothing over it but the notices.
    List,
    /// As `List`, a text field of the window having the focus: ⌫ and ⌘⌫ are its own.
    Field,
    /// A dialog, a menu or the palette is over the window.
    Modal,
    /// A panel of macOS's (a file picker, About) has the keyboard.
    Panel,
    /// Another window is in front, or none.
    Away,
}

/// Where a surface is, and what's there: what every `when` reads.
pub struct Context<'a> {
    pub window: Which,
    /// Settings' tab.
    pub tab: Tab,
    pub sheet: Sheet,
    pub focus: Focus,
    /// The main window's selected torrents.
    pub selected: Selected<'a>,
    /// The selected files of the torrent the details pane shows, and whether the one Open and
    /// Reveal show was on disk at the last look.
    pub files: Option<(&'a TorrentInfo, Vec<&'a proto::File>)>,
    pub file_there: bool,
    /// The commands the screen shown offers; none: every one.
    pub mounted: Option<&'a Mounted>,
    /// There's a change to take back.
    pub undo: bool,
    /// The window's palette has an entry to offer.
    pub palette: bool,
    /// The Look sheet's preset and rows, while it's open.
    pub look: Option<LookSheet>,
}

/// What the Look sheet over Settings shows: its preset, and with one theme in both halves each row
/// by its key and name, joined or not (the Theme row first).
pub struct LookSheet {
    pub preset: LookPreset,
    pub rows: Option<Vec<(&'static str, &'static str, bool)>>,
}

impl<'a> Context<'a> {
    /// Nowhere in particular: nothing selected, nothing open, nothing to undo.
    pub fn empty(window: Which, focus: Focus) -> Context<'a> {
        Context {
            window,
            tab: Tab::default(),
            sheet: Sheet::Closed,
            focus,
            selected: Selected { torrents: Vec::new(), there: false },
            files: None,
            file_there: false,
            mounted: None,
            undo: false,
            palette: false,
            look: None,
        }
    }

    /// The main window's, with `focus`: the selection, the pane's files, what's mounted.
    pub fn main(s: &'a Phantom, focus: Focus) -> Context<'a> {
        Context {
            selected: Selected::of(&s.table, &s.torrents, s.target_there),
            files: selected_in_pane(&s.table, &s.torrents, &s.pane),
            file_there: s.file_there,
            mounted: Some(&s.mounted),
            undo: s.changes.state.top().is_some(),
            ..Context::empty(Which::Main, focus)
        }
    }

    /// The window has the keyboard and nothing's over it.
    pub fn live(&self) -> bool {
        matches!(self.focus, Focus::List | Focus::Field)
    }

    /// The screen shown offers `command`: a view registered it.
    pub fn offers(&self, command: impl Into<Command>) -> bool {
        self.mounted.is_none_or(|m| m.offers(command))
    }

    /// A torrent command, live and mounted, that some selected torrent does.
    pub fn any(&self, c: OnTorrents) -> bool {
        self.live() && self.offers(c) && self.selected.torrents.iter().any(|t| does(t, c))
    }

    /// One that the one torrent selected does.
    pub fn one(&self, c: OnTorrents) -> bool {
        self.selected.torrents.len() == 1 && self.all(c)
    }

    /// One that every selected torrent does, some being selected.
    pub fn all(&self, c: OnTorrents) -> bool {
        let torrents = &self.selected.torrents;
        self.live() && self.offers(c) && !torrents.is_empty() && torrents.iter().all(|t| does(t, c))
    }

    /// A file command, live and mounted, that the pane's torrent does with files selected.
    pub fn pane(&self, c: OnFiles) -> bool {
        self.live() && self.offers(c) && self.files.as_ref().is_some_and(|(t, _)| t.actions.does(c.action()))
    }

    /// Exactly one file selected, on disk (#276: what's at its path, not what it downloaded).
    pub fn one_file_on_disk(&self) -> bool {
        self.files.as_ref().is_some_and(|(_, files)| matches!(files.as_slice(), [f] if on_disk(f)))
    }

    pub fn some_file_on_disk(&self) -> bool {
        self.files.as_ref().is_some_and(|(_, files)| files.iter().any(|f| on_disk(f)))
    }

    /// The Look sheet's preset has something to restore in `side`.
    pub fn restorable(&self, side: Side) -> bool {
        self.look.as_ref().is_some_and(|l| crate::model::settings::restorable(&l.preset, side))
    }

    /// The Look sheet shows a theme in each half.
    pub fn two_themes(&self) -> bool {
        self.look.as_ref().is_some_and(|l| l.preset.light.theme.id() != l.preset.dark.theme.id())
    }

    /// Whether the Look sheet's row `key` is joined; none while it shows no such row.
    pub fn joined(&self, key: &str) -> Option<bool> {
        let rows = self.look.as_ref()?.rows.as_ref()?;
        rows.iter().find(|(k, ..)| *k == key).map(|(.., joined)| *joined)
    }
}

fn does(t: &TorrentInfo, c: OnTorrents) -> bool {
    c.action().is_none_or(|a| t.actions.does(a))
}

fn on_disk(f: &proto::File) -> bool {
    f.on_disk && !f.missing
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

listed_enum! {
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
}

/// Sends `what` for the selected torrents `action` applies to.
fn send(s: &mut Phantom, action: Action, what: TorrentCommand) {
    use crate::model::{command, selected_for};
    command(&mut *s.service, &selected_for(&s.table, &s.torrents, action), what)
}

/// The one torrent selected, for Open, Reveal and Rename….
fn single(s: &Phantom) -> Option<proto::TorrentId> {
    details_torrent(&s.table, &s.torrents).map(|t| t.id)
}

impl OnTorrents {
    /// Pause, Resume, Check, Cancel check, Cancel move and Move to… (#270: those that can't move
    /// are skipped) are offered for the selected torrents they apply to; Remove and Delete only
    /// where they apply to every one, as they act on all (and not while a field has ⌫ and ⌘⌫);
    /// Open, Reveal, Rename… and Copy magnet link for exactly one, Open and Reveal only while its
    /// location is on disk.
    pub fn decl(self) -> Decl<OnTorrents, Phantom> {
        use crate::platform::{Show, show_torrent};
        use client::display;
        let (scope, synonyms) = (Scope::MAIN, &[] as &[&str]);
        match self {
            OnTorrents::Resume => Decl {
                id: "resume",
                name: "Resume",
                icon: Some(Icon::Resume),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, _| send(s, Action::Resume, TorrentCommand::Resume),
            },
            OnTorrents::Pause => Decl {
                id: "pause",
                name: "Pause",
                icon: Some(Icon::Pause),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, _| send(s, Action::Pause, TorrentCommand::Pause),
            },
            OnTorrents::Check => Decl {
                id: "check",
                name: "Check",
                icon: Some(Icon::Verify),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, _| send(s, Action::Check, TorrentCommand::Check),
            },
            OnTorrents::CancelCheck => Decl {
                id: "cancel-check",
                name: display::cancel_check(),
                icon: Some(Icon::CancelCheck),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, _| send(s, Action::CancelCheck, TorrentCommand::CancelCheck),
            },
            OnTorrents::CancelMove => Decl {
                id: "cancel-move",
                name: display::cancel_move(),
                icon: Some(Icon::CancelMove),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, _| send(s, Action::CancelMove, TorrentCommand::CancelMove),
            },
            OnTorrents::Open => Decl {
                id: "open",
                name: "Open",
                icon: Some(Icon::Open),
                keys: OPEN_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.selected.there && ctx.one(c)),
                run: |s, _, _| single(s).into_iter().for_each(|id| show_torrent(s, id, Show::Open)),
            },
            OnTorrents::Reveal => Decl {
                id: "reveal",
                name: display::reveal_in_finder(),
                icon: Some(Icon::Reveal),
                keys: REVEAL_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.selected.there && ctx.one(c)),
                run: |s, _, _| single(s).into_iter().for_each(|id| show_torrent(s, id, Show::Reveal)),
            },
            OnTorrents::MoveTo => Decl {
                id: "move",
                name: display::move_to(),
                icon: Some(Icon::MoveTo),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.any(c)),
                run: |s, _, tx| crate::model::moving::move_to(s, tx),
            },
            OnTorrents::Rename => Decl {
                id: "rename",
                name: display::rename(),
                icon: Some(Icon::Rename),
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.one(c)),
                run: |s, _, _| single(s).into_iter().for_each(|id| crate::model::renaming::open(s, id)),
            },
            OnTorrents::CopyMagnet => Decl {
                id: "copy-magnet",
                name: "Copy magnet link",
                icon: Some(Icon::Copy),
                keys: COPY_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.one(c)),
                run: |s, _, tx| crate::model::copy_magnet(s, tx),
            },
            OnTorrents::Remove => Decl {
                id: "remove",
                name: "Remove",
                icon: Some(Icon::Remove),
                keys: REMOVE_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.focus != Focus::Field && ctx.all(c)),
                run: |s, _, _| crate::model::remove_selected(s, false),
            },
            OnTorrents::Delete => Decl {
                id: "delete",
                name: "Delete",
                icon: Some(Icon::Trash),
                keys: DELETE_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.focus != Focus::Field && ctx.all(c)),
                run: |s, _, _| crate::model::remove_selected(s, true),
            },
        }
    }

    pub fn spec(self) -> Spec {
        self.decl().spec()
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

/// The torrent commands available for the selection, in the list's order: those Enabled in the
/// window's list.
pub fn on_torrents(s: &Phantom) -> Vec<OnTorrents> {
    let ctx = Context::main(s, Focus::List);
    OnTorrents::ALL.into_iter().filter(|c| Command::from(*c).offer(&ctx) == Offer::Enabled).collect()
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

listed_enum! {
    /// What can be done to the selected files of the torrent the details pane shows (#151).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OnFiles {
        Open,
        Reveal,
        /// Sets every selected file to this priority; checked while they all have it.
        Priority(Priority) => [Priority::Normal, Priority::Off],
        /// Sets them Off, then moves those on disk to the Trash.
        MoveToTrash,
    }
}

impl OnFiles {
    /// The same keys as the torrent commands that do the same to a torrent: the file list keeps them
    /// for files while it has the focus. Offered only for what the service lists for the torrent
    /// (Set priority, and Move to Trash, which does what Set priority does, while a selected file is
    /// on disk: it moves only those), Open and Reveal for exactly one file that's on disk, as the
    /// service says and as the disk looked at when the menu opened says (#307).
    pub fn decl(self) -> Decl<OnFiles, Phantom> {
        use crate::model::{set_selected_files, trash_selected_files};
        use crate::platform::{Show, show_file};
        use client::display;
        let (scope, synonyms) = (Scope::MAIN, &[] as &[&str]);
        match self {
            OnFiles::Open => Decl {
                id: "open",
                name: "Open",
                icon: Some(Icon::Open),
                keys: OPEN_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.file_there && ctx.one_file_on_disk() && ctx.pane(c)),
                run: |s, _, _| show_file(s, Show::Open),
            },
            OnFiles::Reveal => Decl {
                id: "reveal",
                name: display::reveal_in_finder(),
                icon: Some(Icon::Reveal),
                keys: REVEAL_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.file_there && ctx.one_file_on_disk() && ctx.pane(c)),
                run: |s, _, _| show_file(s, Show::Reveal),
            },
            OnFiles::Priority(Priority::Normal) => Decl {
                id: "priority-normal",
                name: display::priority(Priority::Normal),
                icon: None,
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.pane(c)),
                run: |s, _, _| set_selected_files(s, Priority::Normal),
            },
            OnFiles::Priority(Priority::Off) => Decl {
                id: "priority-off",
                name: display::priority(Priority::Off),
                icon: None,
                keys: &[],
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.pane(c)),
                run: |s, _, _| set_selected_files(s, Priority::Off),
            },
            OnFiles::MoveToTrash => Decl {
                id: "trash",
                name: display::move_to_trash(),
                icon: Some(Icon::Trash),
                keys: DELETE_KEYS,
                synonyms,
                scope,
                when: Some(|ctx, c| ctx.some_file_on_disk() && ctx.pane(c)),
                run: |s, _, _| trash_selected_files(s),
            },
        }
    }

    pub fn spec(self) -> Spec {
        self.decl().spec()
    }

    /// The service's action it rests on.
    pub fn action(self) -> Action {
        match self {
            OnFiles::Open => Action::Open,
            OnFiles::Reveal => Action::Reveal,
            OnFiles::Priority(_) => Action::SetPriority,
            OnFiles::MoveToTrash => Action::TrashFiles,
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
    let ctx = Context::main(s, Focus::List);
    OnFiles::ALL.into_iter().filter(|c| Command::from(*c).offer(&ctx) == Offer::Enabled).collect()
}

listed_enum! {
    /// What isn't about a selection.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InApp {
        AddFile,
        /// The newest change, a torrent action's or a setting's (#255); the menu bar's Edit ▸ Undo too.
        Undo,
        /// Opens the command palette.
        Palette,
    }
}

impl InApp {
    pub fn decl(self) -> Decl<InApp, Phantom> {
        let synonyms = &[] as &[&str];
        match self {
            InApp::AddFile => Decl {
                id: "add-file",
                name: "Add file…",
                icon: Some(Icon::AddFile),
                keys: ADD_FILE_KEYS,
                synonyms,
                scope: Scope::MAIN,
                // Also with Settings in front, or no window: the main one comes back for it.
                when: Some(|ctx, _| !matches!(ctx.focus, Focus::Modal | Focus::Panel)),
                run: |s, _, tx| crate::views::toolbar::add_torrent_file(s, tx),
            },
            InApp::Undo => Decl {
                id: "undo",
                name: client::display::undo(),
                icon: Some(Icon::Undo),
                keys: UNDO_KEYS,
                synonyms,
                scope: Scope::MAIN,
                when: Some(|ctx, _| ctx.undo),
                run: |s, _, _| crate::changes::press(s),
            },
            // It opens wherever its window's palette has something to offer: never listed itself.
            InApp::Palette => Decl {
                id: "palette",
                name: "Command palette…",
                icon: Some(Icon::Search),
                keys: PALETTE_KEYS,
                synonyms,
                scope: Scope::EVERYWHERE,
                when: Some(|ctx, _| ctx.live() && ctx.palette),
                run: |s, _, _| crate::model::palette::open(s),
            },
        }
    }

    pub fn spec(self) -> Spec {
        self.decl().spec()
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

listed_enum! {
    /// What the Look sheet over Settings offers (#434): leaving it, resetting it, and each row's Split
    /// or Join. Each acts in the Settings window, on what the sheet edits.
    ///
    /// `ALL` has one of each, for what is the same in every use (the name, the icon, the keys): the
    /// Split and Join of a row are made for each row the sheet shows, the key here being none.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OnSheet {
        /// Done: keeps what was edited.
        Done,
        /// Cancel: puts the Look Presets back as they were when the sheet opened.
        Cancel,
        /// Restore Defaults in this half (or both).
        Restore(Side) => [Side::Only(Appearance::Light), Side::Only(Appearance::Dark), Side::Both],
        /// A row's Split, by the row's key ([`THEME`](crate::model::settings::THEME) for the Theme row).
        Split(&'static str) => [""],
        /// A row's Join.
        Join(&'static str) => [""],
    }
}

impl OnSheet {
    /// Restore Defaults is offered only where there's something to restore, in one half while a
    /// theme is in each; Split and Join for each row the sheet shows, with one theme in both halves.
    pub fn decl(self) -> Decl<OnSheet, SettingsWindow> {
        use crate::model::settings::{self as st, presets};
        let (scope, synonyms, keys, icon) = (Scope::LOOK_SHEET, &[] as &[&str], &[] as &[Chord], None);
        match self {
            OnSheet::Done => Decl {
                id: "done",
                name: "Done",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, _| ctx.look.is_some()),
                run: |w, _, _| presets::close(w),
            },
            OnSheet::Cancel => Decl {
                id: "cancel",
                name: "Cancel",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, _| ctx.look.is_some()),
                run: |w, _, _| presets::cancel(w),
            },
            OnSheet::Restore(Side::Both) => Decl {
                id: "restore-both",
                name: "Restore Defaults",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, _| ctx.restorable(Side::Both)),
                run: |w, _, _| st::restore_defaults(w, Side::Both),
            },
            OnSheet::Restore(Side::Only(Appearance::Light)) => Decl {
                id: "restore-light",
                name: "Restore Defaults ☀",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, _| ctx.two_themes() && ctx.restorable(Side::Only(Appearance::Light))),
                run: |w, _, _| st::restore_defaults(w, Side::Only(Appearance::Light)),
            },
            OnSheet::Restore(Side::Only(Appearance::Dark)) => Decl {
                id: "restore-dark",
                name: "Restore Defaults ☾",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, _| ctx.two_themes() && ctx.restorable(Side::Only(Appearance::Dark))),
                run: |w, _, _| st::restore_defaults(w, Side::Only(Appearance::Dark)),
            },
            OnSheet::Split(_) => Decl {
                id: "split",
                name: "Split",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, c| matches!(c, OnSheet::Split(key) if ctx.joined(key) == Some(true))),
                run: |w, c, _| {
                    if let OnSheet::Split(key) = c {
                        st::split_row(w, key)
                    }
                },
            },
            OnSheet::Join(_) => Decl {
                id: "join",
                name: "Join",
                icon,
                keys,
                synonyms,
                scope,
                when: Some(|ctx, c| matches!(c, OnSheet::Join(key) if ctx.joined(key) == Some(false))),
                run: |w, c, _| {
                    if let OnSheet::Join(key) = c {
                        st::join_row(w, key)
                    }
                },
            },
        }
    }

    pub fn spec(self) -> Spec {
        self.decl().spec()
    }

    /// As the palette words it: a row's Split and Join name the row ("Split Corners"); Restore
    /// Defaults marks a half (the sheet's pull-down says Light, Dark and Both; the palette's marks
    /// are those a setting's halves have).
    pub fn wording(self, row: &str) -> String {
        match self {
            OnSheet::Split(_) | OnSheet::Join(_) => format!("{} {row}", self.spec().name),
            other => other.spec().name.to_owned(),
        }
    }
}

/// Any command, of any kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Torrents(OnTorrents),
    Files(OnFiles),
    App(InApp),
    Sheet(OnSheet),
}

/// A declaration's scope, and whether its `when` holds in `ctx`.
fn graded<C: Copy, H>(d: Decl<C, H>, c: C, ctx: &Context) -> (Scope, bool) {
    (d.scope, d.when.is_none_or(|when| when(ctx, c)))
}

impl Command {
    pub fn spec(self) -> Spec {
        match self {
            Command::Torrents(c) => c.spec(),
            Command::Files(c) => c.spec(),
            Command::App(c) => c.spec(),
            Command::Sheet(c) => c.spec(),
        }
    }

    /// Where it can appear at all.
    pub fn scope(self) -> Scope {
        match self {
            Command::Torrents(c) => c.decl().scope,
            Command::Files(c) => c.decl().scope,
            Command::App(c) => c.decl().scope,
            Command::Sheet(c) => c.decl().scope,
        }
    }

    /// How it's offered in `ctx`: Hidden out of its scope, Disabled where its `when` doesn't hold,
    /// else Enabled.
    pub fn offer(self, ctx: &Context) -> Offer {
        let (scope, when) = match self {
            Command::Torrents(c) => graded(c.decl(), c, ctx),
            Command::Files(c) => graded(c.decl(), c, ctx),
            Command::App(c) => graded(c.decl(), c, ctx),
            Command::Sheet(c) => graded(c.decl(), c, ctx),
        };
        match (scope.holds(ctx.window, ctx.sheet), when) {
            (false, _) => Offer::Hidden,
            (true, false) => Offer::Disabled,
            (true, true) => Offer::Enabled,
        }
    }

    /// Every command, each kind in its list's order.
    pub fn all() -> impl Iterator<Item = Command> {
        let torrents = OnTorrents::ALL.into_iter().map(Command::Torrents);
        let files = OnFiles::ALL.into_iter().map(Command::Files);
        let app = InApp::ALL.into_iter().map(Command::App);
        torrents.chain(files).chain(app).chain(OnSheet::ALL.into_iter().map(Command::Sheet))
    }
}

impl From<OnSheet> for Command {
    fn from(c: OnSheet) -> Command {
        Command::Sheet(c)
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

/// Does what `command` says to the selected torrents, as the row menu and the palette run it.
pub fn run_on_torrents(s: &mut Phantom, command: OnTorrents, tx: &mut Tx) {
    (command.decl().run)(s, command, tx)
}

/// Does what `command` says.
pub fn run_in_app(s: &mut Phantom, command: InApp, tx: &mut Tx) {
    (command.decl().run)(s, command, tx)
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
                    let ctx = Context { selected, ..Context::empty(Which::Main, Focus::List) };
                    for c in OnTorrents::ALL {
                        let does = |t: &TorrentInfo| c.action().is_none_or(|a| t.actions.does(a));
                        let (any, all) = (torrents.iter().any(does), n > 0 && torrents.iter().all(does));
                        let want = match c {
                            OnTorrents::Remove | OnTorrents::Delete => all,
                            OnTorrents::Open | OnTorrents::Reveal => n == 1 && there && all,
                            OnTorrents::Rename | OnTorrents::CopyMagnet => n == 1 && all,
                            _ => any,
                        };
                        let enabled = Command::from(c).offer(&ctx) == Offer::Enabled;
                        assert_eq!(enabled, want, "{c:?} for {n} torrents ({pick}), there: {there}");
                    }
                }
            }
        }
    }

    /// With nothing selected no torrent command is offered.
    #[test]
    fn nothing_selected_offers_no_torrent_command() {
        let ctx = Context { selected: Selected { torrents: Vec::new(), there: true }, ..Context::empty(Which::Main, Focus::List) };
        assert!(OnTorrents::ALL.iter().all(|c| Command::from(*c).offer(&ctx) == Offer::Disabled));
    }

    /// Every command has a name, and no two that can be offered in the same window with the same
    /// sheet open (their scopes) share an id or a key; but a torrent's and a file's, whose keys are
    /// pressed in two lists: the file list keeps its own for files while it has the focus
    /// (`shortcuts::on_file_key`).
    #[test]
    fn no_two_commands_in_one_scope_share_an_id_or_a_key() {
        for window in Which::ALL {
            for sheet in Sheet::ALL {
                let here: Vec<Command> = Command::all().filter(|c| c.scope().holds(window, sheet)).collect();
                for (i, a) in here.iter().enumerate() {
                    assert!(!a.spec().name.is_empty());
                    for b in &here[i + 1..] {
                        if matches!((a, b), (Command::Torrents(_), Command::Files(_)) | (Command::Files(_), Command::Torrents(_))) {
                            continue;
                        }
                        assert_ne!(a.spec().id, b.spec().id, "{a:?} and {b:?} in {window:?}, {sheet:?}");
                        for key in a.spec().keys {
                            assert!(!b.spec().keys.contains(key), "{key:?} is both {a:?}'s and {b:?}'s in {window:?}, {sheet:?}");
                        }
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
