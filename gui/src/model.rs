//! The app's state, [`Phantom`], and what actions do to it: applying the service's live data and
//! answering its commands, reading
//! the selection, adding (into the default destination, or asking for one when there's none), removing and
//! deleting, sorting, and the details pane of the one selected torrent with its files' priorities.
//! No drawing; views call these. Each helper takes only the pieces it uses.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use client::{LinkProblem, OpenRefusal, display};
use proto::{Action, Added, Details, File, Priority, TorrentCommand, TorrentId, TorrentInfo};
use rune::{FileDialogOptions, TextEdit, Tx};
use rune_kit::{SortDir, TableState};

use crate::bar::Bar;
use crate::menu::{Event, Menu, Open as MenuOpen};
use crate::notice::Notice;
use crate::pane::{self, DetailsPane};
use crate::platform::{Finder, Shown};
use crate::service::{AddError, AddRequest, Connection, Done, Live, Service};
use crate::theme::preset::Looks;
use crate::ui::{Failure, Ui};
use crate::windows::{Inbox, Link, Shared, Which, Window};
use client::display::UndoOff;
use proto::Leave;
use undone::Undos;

pub mod settings;
pub mod undone;

use settings::SettingsWindow;

pub struct Phantom {
    pub service: Box<dyn Service>,
    /// Brings the main window back: for a link opened while it's closed, the Dock icon, Window ▸
    /// Phantom.
    pub window: Window,
    pub torrents: Vec<TorrentInfo>,
    /// Rows are keyed by torrent id.
    pub table: TableState,
    pub magnet: TextEdit,
    /// An add sent by a click on Add is on its way. Once it's added the toolbar stops drawing its
    /// marker (`focus_return`), which gives the link field the focus only if it's still on Add
    /// or nowhere; wherever the person put it meanwhile, it stays.
    pub refocus_link: bool,
    /// What Add's popover says, until the field's text changes: why the text can't be added, or
    /// why the add failed.
    pub add_note: Option<AddNote>,
    /// Where Open and Reveal in Finder go.
    pub finder: Box<dyn Finder>,
    /// Which menu is open.
    pub menu: Menu,
    pub connection: Connection,
    /// What the status bar tells.
    pub bar: Bar,
    pub pane: DetailsPane,
    pub adds: Adds,
    /// The disabled button whose note says why it's off, until a click elsewhere (#89).
    pub why_off: Option<Action>,
    /// The Settings window (#257), which has the sheet and the App icon.
    pub settings: Link<SettingsWindow>,
    /// What Settings posted here.
    pub inbox: Inbox<Phantom>,
    /// The window in front, which keys go to.
    pub front: Which,
    /// A field of Settings is being edited, as Settings last said: ⌘Z there is the field's own.
    pub settings_editing: bool,
    /// The Active Look Preset, and the look each Appearance shows: Settings' too.
    pub looks: Shared<Looks>,
    /// The `(epoch, version)` of the snapshot the list and the pane are from: an older one, from
    /// another connection or from before the service restarted, is dropped.
    pub latest: (u64, u64),
    /// The service's settings reset at its start, as last told, so it's told of once.
    pub service_reset: Vec<String>,
    /// The disks torrents still need space on, as the latest snapshot has them.
    pub volumes: Vec<proto::Volume>,
    /// Why the default destination can't be reached, as the latest snapshot says (#209): Settings'
    /// row shows it as it changes, told each time it does.
    pub default_unavailable: Option<proto::Unavailable>,
    /// The service's undo stack, as the latest snapshot has it: whether a notice's Undo is on.
    pub undo: Vec<proto::Undoable>,
    /// What became of the entries that left it (#200).
    pub undos: undone::Undos,
    /// Why a notice's action is off, once it was pressed while off: shown under it.
    pub why_off_note: Option<String>,
    /// The changes ⌘Z takes back, torrent actions and settings changes in one list (#221, #255).
    pub changes: crate::changes::Changes,
}

/// One batch of adds made with no control in the window (files dropped on it or opened from
/// Finder, a magnet clicked in a browser) in which some failed: "Couldn't add 2 of 5". The
/// batch stays one error notice, its list as the view.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedAdds {
    /// How many were in the batch.
    pub total: usize,
    pub failed: Vec<AddFailure>,
}

impl FailedAdds {
    /// The entry as a [`Notice`]: "Couldn't add 2 of 5", and each failure as its view.
    pub fn notice(&self) -> Notice {
        let failed = self.failed.clone();
        let list = move |_: &Phantom, ui: &Ui| {
            let failures: Vec<_> = failed.iter().map(|f| Failure { what: &f.what, why: &f.why }).collect();
            ui.failure_list(&failures)
        };
        Notice { more: Some(Rc::new(list)), ..Notice::error(display::adds_failed(self.failed.len(), self.total)) }
    }
}

/// An add that failed: the file or link, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct AddFailure {
    pub what: String,
    pub why: String,
}

impl AddFailure {
    pub fn refused(request: &AddRequest, why: OpenRefusal) -> Self {
        Self { what: what(request), why: display::open_refusal(why).into() }
    }

    /// The link `text`, handed to the app from outside, refused for `why`.
    pub fn refused_link(text: &str, why: OpenRefusal) -> Self {
        Self { what: text.trim().to_owned(), why: display::open_refusal(why).into() }
    }

    /// `text` isn't a link Add accepts.
    pub fn not_a_link(text: &str, problem: LinkProblem) -> Self {
        Self { what: text.trim().to_owned(), why: display::link_problem(problem).into() }
    }
}

/// The file's name, or the link, as a note names an add.
fn what(request: &AddRequest) -> String {
    match request {
        AddRequest::File(path) => path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned(),
        AddRequest::Link { text, .. } => text.clone(),
    }
}

/// A message for the Add button, about the field's text as it was when it arose.
#[derive(Clone)]
pub struct AddNote {
    pub text: String,
    pub message: String,
    /// A failed add: the note is in the error colour, and Add stays red for [`ALERT_SECS`].
    pub error: bool,
    /// The app clock when it was first drawn; `None` until the next poll sees it.
    since: Option<f64>,
    pub alerting: bool,
}

const ALERT_SECS: f64 = 2.0;

impl AddNote {
    /// Why the text can't be added.
    pub fn about(field: &TextEdit, message: impl Into<String>) -> Self {
        Self { text: field.text.clone(), message: message.into(), error: false, since: None, alerting: false }
    }

    /// Why the add failed.
    fn failed(field: &TextEdit, message: impl Into<String>) -> Self {
        Self { error: true, alerting: true, ..Self::about(field, message) }
    }
}

/// Ends Add's red after [`ALERT_SECS`].
pub fn age_add_note(note: &mut Option<AddNote>, now: f64, tx: &mut Tx) {
    let Some(note) = note.as_mut().filter(|n| n.alerting) else { return };
    let since = *note.since.get_or_insert(now);
    if now - since >= ALERT_SECS {
        note.alerting = false;
    } else {
        tx.wake_after(std::time::Duration::from_secs_f64(ALERT_SECS - (now - since)));
    }
}

/// Adds the service refused for want of a destination, waiting for the folder picker: one at a
/// time. Each entry is one action's torrents (several files dropped together), which get one
/// destination.
#[derive(Default)]
pub struct Adds {
    waiting: VecDeque<Waiting>,
    asking: bool,
}

/// An add waiting for a destination. Its batch goes on with it, so the whole batch is told of
/// once, when the last of it is answered.
struct Waiting {
    requests: Vec<AddRequest>,
    batch: Batch,
}

type OnAdded = Done<Vec<Result<Added, AddError>>>;

/// What one action's adds have come to so far. Its failures are told of together, once none of
/// it waits for a destination any more.
struct Batch {
    /// The link the field sent, if this is the field's add: the field answers for it.
    field: Option<String>,
    /// Answered so far, or refused before reaching the service.
    total: usize,
    failed: Vec<AddFailure>,
}

impl Batch {
    /// A batch of adds with `refused` already failed before the service is asked.
    fn new(field: Option<String>, refused: Vec<AddFailure>) -> Self {
        Self { field, total: refused.len(), failed: refused }
    }

    /// What the service's answer to `requests` does to the batch.
    fn answer(self, requests: &[AddRequest]) -> OnAdded {
        let labels: Vec<String> = requests.iter().map(what).collect();
        Box::new(move |s, results, tx| self.answered(s, &labels, results, tx))
    }

    /// Each add that was added is told of in the status bar, the last one showing; those refused
    /// for want of a destination wait for the picker, as one. The field's own add, while the
    /// field still holds its link, empties the field once added, and a failure shows by Add, with
    /// the link selected to be fixed; any other failure goes to the batch's note.
    fn answered(mut self, s: &mut Phantom, labels: &[String], results: Vec<Result<Added, AddError>>, tx: &mut Tx) {
        let holds = self.field.as_deref().is_some_and(|link| s.magnet.text.trim() == link);
        let mut requests = Vec::new();
        for (label, result) in labels.iter().zip(results) {
            match result {
                Ok(added) => {
                    self.total += 1;
                    if holds {
                        s.magnet.clear();
                        s.refocus_link = false;
                    }
                    let added = show_added(&mut s.table, &added, tx);
                    s.bar.tell(added);
                }
                Err(AddError::NoDestination(request)) => requests.push(request),
                Err(AddError::Failed(e)) if holds => {
                    s.magnet.set_selection(0, s.magnet.text.len());
                    s.add_note = Some(AddNote::failed(&s.magnet, display::add_failure(&e)));
                }
                Err(AddError::Failed(e)) => {
                    self.total += 1;
                    self.failed.push(AddFailure { what: label.clone(), why: display::add_failure(&e) });
                }
            }
        }
        if requests.is_empty() {
            self.finish(s);
        } else {
            s.adds.waiting.push_back(Waiting { requests, batch: self });
            ask_destination(s, tx);
        }
    }

    /// Nothing of the batch is left to answer: its failures, if any, are reported as one entry.
    fn finish(self, s: &mut Phantom) {
        if !self.failed.is_empty() {
            s.bar.tell(FailedAdds { total: self.total, failed: self.failed }.notice());
        }
    }
}

impl Phantom {
    pub fn new(service: Box<dyn Service>) -> Self {
        Self {
            service,
            window: Window::none(),
            torrents: Vec::new(),
            table: TableState::new().multi_select(true),
            magnet: TextEdit::new(""),
            refocus_link: false,
            add_note: None,
            // Opens nothing, so a test or a headless run never launches an app: the real app
            // brings the system's.
            finder: Box::new(Shown::default()),
            menu: Menu::Closed,
            connection: Connection::Connecting,
            bar: Bar::default(),
            pane: DetailsPane::new(),
            adds: Adds::default(),
            why_off: None,
            settings: crate::windows::detached(),
            inbox: crate::windows::link(|| {}).1,
            front: Which::Main,
            settings_editing: false,
            looks: Shared::default(),
            latest: (0, 0),
            service_reset: Vec::new(),
            volumes: Vec::new(),
            default_unavailable: None,
            undo: Vec::new(),
            undos: undone::Undos::default(),
            why_off_note: None,
            changes: crate::changes::Changes::default(),
        }
    }

    pub fn apply(&mut self, mut live: Live) {
        // The settings go by their own count, whatever the torrents' are.
        if let Live::Snapshot(snapshot) = &mut live
            && let Some(configured) = snapshot.settings.take()
        {
            crate::changes::observe(self, configured);
        }
        // Then the undo stack, from any snapshot (the machine keeps the newest), whatever it was
        // built for: of another client's settings change and torrent action in one snapshot, the
        // setting counts as the older (ADR 0008).
        if let Live::Snapshot(snapshot) = &live {
            crate::changes::observe_stack(self, (snapshot.epoch, snapshot.version), &snapshot.undo);
        }
        match live {
            // One the app has a later one of, from another connection or an earlier run.
            Live::Snapshot(snapshot) if (snapshot.epoch, snapshot.version) < self.latest => {}
            // Built for other torrents than the pane asked for: its silence on the pane's says
            // nothing of them. One for these comes at once.
            Live::Snapshot(snapshot) if !same(&snapshot.watching, &self.pane.watching) => {}
            Live::Snapshot(snapshot) => {
                self.latest = (snapshot.epoch, snapshot.version);
                if snapshot.reset != self.service_reset {
                    self.service_reset = snapshot.reset.clone();
                    if let Some(notice) = settings::reset_notice(&snapshot.reset, "settings.json") {
                        self.bar.tell(notice);
                    }
                }
                self.torrents = snapshot.torrents;
                self.volumes = snapshot.volumes;
                let left = self.undos.observe(&snapshot.left);
                if snapshot.default_destination_unavailable != self.default_unavailable {
                    let unavailable = snapshot.default_destination_unavailable.clone();
                    self.settings.post(move |w, _| w.default_unavailable = unavailable);
                }
                self.default_unavailable = snapshot.default_destination_unavailable;
                // Off the stack with no word yet why: an undo under way, whoever asked (#200, #235).
                // Its notices leave now, as for one pressed here, and come back if it's refused.
                let undoing: Vec<u64> = self
                    .undo
                    .iter()
                    .map(|u| u.seq)
                    .filter(|q| !snapshot.undo.iter().any(|u| u.seq == *q) && self.undos.why(*q).is_none())
                    .collect();
                self.undo = snapshot.undo;
                if !undoing.is_empty() {
                    self.bar.undoing(undoing);
                }
                // What of a selection didn't come back, whoever undid it: told once, as the service says.
                for l in &left {
                    if let Leave::Undone { refused } = &l.why {
                        for not in refused {
                            self.bar.tell(not_back_notice(not));
                        }
                    }
                }
                self.bar.left(left);
                if let Some((column, dir)) = self.table.sort() {
                    let column = column.to_owned();
                    sort(&mut self.torrents, &column, dir);
                }
                let listed = self.torrents.iter().map(|t| t.id).collect();
                let detailed = snapshot.details.iter().map(|d| d.id).collect();
                let seen = pane::machine::Event::Observed(pane::machine::Observed::Snapshot { listed, detailed });
                self.pane.state = pane::machine::decide(&self.pane.state, &seen);
                self.show_details(snapshot.details);
            }
            Live::Connection(connection) => self.connection = connection,
        }
    }

    /// The pane shows the details of the torrent its machine says, out of `details`, from the
    /// snapshot just applied; another torrent's files and their selection go.
    fn show_details(&mut self, details: Vec<Details>) {
        let shown = self.pane.state.shown();
        if self.pane.details.as_ref().map(|d| d.id) != shown {
            self.pane.files = DetailsPane::new().files;
        }
        self.pane.details = details.into_iter().find(|d| Some(d.id) == shown).map(|mut details| {
            if let Some((column, dir)) = self.pane.files.sort() {
                let column = column.to_owned();
                sort_files(&mut details, &column, dir);
            }
            details
        });
    }
}

/// The same torrents, in any order.
fn same(a: &[TorrentId], b: &[TorrentId]) -> bool {
    let set = |ids: &[TorrentId]| ids.iter().copied().collect::<std::collections::BTreeSet<_>>();
    set(a) == set(b)
}

/// What to tell the user about an add. A torrent that was already in the list gets selected and
/// scrolled to, instead of a second row.
fn show_added(table: &mut TableState, added: &Added, tx: &mut Tx) -> Notice {
    if added.already_existed {
        table.reveal(added.id.to_string(), tx);
    }
    Notice::new(display::added(added))
}

pub fn selected_torrents(table: &TableState) -> Vec<TorrentId> {
    table.selected().filter_map(|id| id.parse().ok()).collect()
}

/// The selected torrents that `action` does something to now, as the service says.
pub fn selected_for(table: &TableState, torrents: &[TorrentInfo], action: Action) -> Vec<TorrentId> {
    let selected = selected_torrents(table);
    torrents.iter().filter(|t| t.actions.does(action) && selected.contains(&t.id)).map(|t| t.id).collect()
}

/// What a row's context menu can offer, in the order it lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowItem {
    /// What the service lists for the torrent: Open, Reveal, Pause, Resume, Check, Cancel check, Remove or Delete.
    Do(Action),
    /// Not the service's to decide: the link is made from what the listing has.
    CopyMagnet,
}

/// What the menu offers for the selection: the actions the service lists, and no others (Pause,
/// Resume and Check for those they apply to; Remove and Delete only when they apply to every
/// selected torrent, as they act on all; Open and Reveal for exactly one torrent), and Copy
/// magnet link for exactly one.
pub fn row_items(table: &TableState, torrents: &[TorrentInfo]) -> Vec<RowItem> {
    let ids = selected_torrents(table);
    let selected: Vec<&TorrentInfo> = torrents.iter().filter(|t| ids.contains(&t.id)).collect();
    let any = |action| selected.iter().any(|t| t.actions.does(action));
    let all = |action| !selected.is_empty() && selected.iter().all(|t| t.actions.does(action));
    let single = selected.len() == 1;
    let mut items = Vec::new();
    items.extend([Action::Resume, Action::Pause, Action::Check, Action::CancelCheck].into_iter().filter(|a| any(*a)).map(RowItem::Do));
    items.extend([Action::Open, Action::Reveal].into_iter().filter(|a| single && any(*a)).map(RowItem::Do));
    items.extend(single.then_some(RowItem::CopyMagnet));
    items.extend([Action::Remove, Action::Delete].into_iter().filter(|a| all(*a)).map(RowItem::Do));
    items
}

/// Closes the open menu once there's nothing for it to act on: the selection is gone.
pub fn close_empty_menu(s: &mut Phantom) {
    let empty = match s.menu {
        Menu::Closed => return,
        Menu::Open(MenuOpen::Row { .. }) => row_items(&s.table, &s.torrents).is_empty(),
        Menu::Open(MenuOpen::File { .. }) => file_items(&s.table, &s.torrents, &s.pane).is_empty(),
    };
    if empty {
        s.menu = s.menu.then(Event::Close);
    }
}

/// What the file menu, and the shortcuts while the file list has focus, can do to the selected
/// files of the torrent the pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileItem {
    Open,
    Reveal,
    /// Sets every selected file to this priority; checked while they all have it.
    Priority(Priority),
    /// Sets them Off, then moves them to the Trash.
    MoveToTrash,
}

/// The torrent the pane shows and its selected files, as the pane has them: nothing selected, or
/// no pane, is none.
fn selected_in_pane<'a>(
    table: &TableState,
    torrents: &'a [TorrentInfo],
    pane: &'a DetailsPane,
) -> Option<(&'a TorrentInfo, Vec<&'a File>)> {
    let details = pane.details.as_ref()?;
    let torrent = details_torrent(table, torrents).filter(|t| t.id == details.id)?;
    let selected = selected_files(pane);
    let files: Vec<&File> = details.files.iter().filter(|f| selected.contains(&f.index)).collect();
    (!files.is_empty()).then_some((torrent, files))
}

/// What the file menu offers for the selected files: only what the service lists for the torrent
/// (Set priority and Move to Trash, which does what Set priority does), and Open and Reveal for
/// exactly one file, that's on disk: not missing, and not one set Off that never downloaded. A file
/// it gets is on disk as soon as the torrent is added (librqbit makes it, empty), so Open works on
/// one with no progress yet; one set Off is there only if some of it downloaded before.
pub fn file_items(table: &TableState, torrents: &[TorrentInfo], pane: &DetailsPane) -> Vec<FileItem> {
    let Some((torrent, files)) = selected_in_pane(table, torrents, pane) else { return Vec::new() };
    let on_disk = |f: &File| !f.missing && (f.priority == Priority::Normal || f.progress_bytes > 0);
    let single = matches!(&files[..], [f] if on_disk(f));
    let mut items = Vec::new();
    items.extend((single && torrent.actions.does(Action::Open)).then_some(FileItem::Open));
    items.extend((single && torrent.actions.does(Action::Reveal)).then_some(FileItem::Reveal));
    if torrent.actions.does(Action::SetPriority) {
        items.extend([Priority::Normal, Priority::Off].map(FileItem::Priority));
    }
    items.extend(torrent.actions.does(Action::TrashFiles).then_some(FileItem::MoveToTrash));
    items
}

/// The priority every selected file has, if they all have the same: what's checked in the menu.
pub fn common_priority(table: &TableState, torrents: &[TorrentInfo], pane: &DetailsPane) -> Option<Priority> {
    let (_, files) = selected_in_pane(table, torrents, pane)?;
    let first = files.first()?.priority;
    files.iter().all(|f| f.priority == first).then_some(first)
}

/// Where the only selected file is on disk: the torrent's location for a single-file torrent,
/// else the file's path in its folder.
pub fn file_target(table: &TableState, torrents: &[TorrentInfo], pane: &DetailsPane) -> Option<PathBuf> {
    let details = pane.details.as_ref()?;
    let (_, files) = selected_in_pane(table, torrents, pane)?;
    let [file] = &files[..] else { return None };
    Some(if details.files.len() == 1 { details.location.to_path_buf() } else { details.location.join(&file.path) })
}

/// A right-click on file `index`'s row selects it alone first if it isn't selected; else the
/// selection stays.
pub fn select_file_for_menu(pane: &mut DetailsPane, index: usize) {
    let row = index.to_string();
    if !pane.files.is_selected(&row) {
        pane.files.select(row);
    }
}

/// A choice of the file menu applied to every selected file.
pub fn set_selected_files(s: &mut Phantom, priority: Priority) {
    let (Some(details), files) = (&s.pane.details, selected_files(&s.pane)) else { return };
    if !files.is_empty() {
        command(&mut *s.service, &[details.id], TorrentCommand::SetPriority { files, priority });
    }
}

/// Moves the selected files to the Trash, set Off first; what moved is told in the status bar,
/// its notice with an Undo of it.
pub fn trash_selected_files(s: &mut Phantom) {
    let Some((torrent, files)) = selected_in_pane(&s.table, &s.torrents, &s.pane) else { return };
    let (id, selected) = (torrent.id, files.iter().map(|f| f.index).collect::<Vec<_>>());
    // Named only when one is: how many moved is the service's to say.
    let names: Vec<String> = match &files[..] {
        [one] => vec![one.path.file_name().map_or_else(|| one.path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())],
        _ => Vec::new(),
    };
    let after = s.undo.iter().map(|u| u.seq).max().unwrap_or(0);
    let pending = crate::changes::acted(s, after, vec![id]);
    command_then(&mut *s.service, Some(after), &[id], TorrentCommand::TrashFiles { files: selected }, move |s, _| {
        // The snapshot with its effect comes before its answer: its entry is on the stack now.
        let entry = s
            .undo
            .iter()
            .rev()
            .find(|u| u.torrents.iter().any(|t| t.id == id) && u.seq > after && matches!(u.action, proto::UndoAction::TrashFiles { .. }))
            .map(|entry| (entry.seq, entry.action));
        crate::changes::made(s, pending, entry.map(|(seq, _)| seq));
        if let Some((seq, action)) = entry {
            tell_happened(s, Happened { seq }, action, &names);
        }
    });
}

/// A right-click on a row that isn't selected selects it alone first, as in Finder; on one that
/// is, the selection stays.
pub fn select_for_menu(service: &mut dyn Service, table: &mut TableState, torrents: &[TorrentInfo], pane: &mut DetailsPane, id: TorrentId) {
    let row = id.to_string();
    if !table.is_selected(&row) {
        table.select(row);
        follow_selection(service, table, torrents, pane);
    }
}

/// The torrent the details pane shows: the selection, when it's exactly one torrent.
pub fn details_torrent<'a>(table: &TableState, torrents: &'a [TorrentInfo]) -> Option<&'a TorrentInfo> {
    match selected_torrents(table)[..] {
        [id] => torrents.iter().find(|t| t.id == id),
        _ => None,
    }
}

/// The torrent the pane shows: the one its details are of, from the same snapshot, or while none
/// are (yet), the selected one, with no files. A switch lands whole once the new one's arrive,
/// rather than through a pane with no files and its buttons off.
pub fn shown_torrent<'a>(table: &TableState, torrents: &'a [TorrentInfo], pane: &DetailsPane) -> Option<&'a TorrentInfo> {
    let selected = details_torrent(table, torrents)?;
    let shown = pane.details.as_ref().and_then(|d| torrents.iter().find(|t| t.id == d.id));
    Some(shown.unwrap_or(selected))
}

/// Tells the pane's machine what's selected, and the service what it needs the details of. Runs
/// when the selection or the list changes.
pub fn follow_selection(service: &mut dyn Service, table: &TableState, torrents: &[TorrentInfo], pane: &mut DetailsPane) {
    let selected = details_torrent(table, torrents).map(|t| t.id);
    let select = pane::machine::Event::Command(pane::machine::Command::Select(selected));
    pane.state = pane::machine::decide(&pane.state, &select);
    if pane.state.shown().is_none() {
        pane.details = None;
        if pane.state == pane::machine::State::Empty {
            pane.files = DetailsPane::new().files;
        }
    }
    if let Some(pane::machine::Do::Watch(ids)) = pane::machine::next(&pane.state, &pane.watching) {
        pane.watching = ids.clone();
        service.watch_details(ids);
    }
}

/// What Open and Reveal in Finder show: where the torrent is on disk, the file itself for a
/// single-file torrent and its folder otherwise. Nothing until its info arrives.
pub fn reveal_target(torrent: &TorrentInfo) -> Option<&Path> {
    torrent.location.as_deref()
}

pub fn selected_files(pane: &DetailsPane) -> Vec<usize> {
    pane.files.selected().filter_map(|i| i.parse().ok()).collect()
}

/// The priority after `p` in the cycle a click on a file's priority steps through.
pub fn next_priority(p: Priority) -> Priority {
    match p {
        Priority::Normal => Priority::Off,
        Priority::Off => Priority::Normal,
    }
}

/// A click on file `index`'s priority: it's the baseline, so its next priority goes to every
/// selected file if it's one of them, else to it alone.
pub fn cycle_priority(service: &mut dyn Service, pane: &DetailsPane, index: usize) {
    let Some(details) = &pane.details else { return };
    let Some(clicked) = details.files.iter().find(|f| f.index == index) else { return };
    let selected = selected_files(pane);
    let files = if selected.contains(&index) { selected } else { vec![index] };
    command(service, &[details.id], TorrentCommand::SetPriority { files, priority: next_priority(clicked.priority) });
}

/// Copies the magnet link of the one selected torrent, and says so in the status bar.
pub fn copy_magnet(s: &mut Phantom, tx: &mut Tx) {
    let Some(t) = details_torrent(&s.table, &s.torrents) else { return };
    s.bar.tell(Notice::new(display::copied_magnet(t.display_name())));
    tx.copy(client::magnet_link(t));
}

/// "Size of Full size" when some files are Off, else just the Size.
/// Nothing until its info arrives.
pub fn size_label(t: &TorrentInfo) -> String {
    match t.content {
        None => String::new(),
        Some(c) if c.size_bytes == c.full_size_bytes => display::bytes(c.size_bytes),
        Some(c) => format!("{} of {}", display::bytes(c.size_bytes), display::bytes(c.full_size_bytes)),
    }
}

/// Whether the text is something Add accepts: a magnet link with an info hash, an info hash on
/// its own, or an http(s) URL. An info hash is 40 hex digits or 32 base32 letters and digits.
pub fn link_problem(text: &str) -> Option<LinkProblem> {
    client::parse_link(text).err()
}

/// Sends the field's link to the service, each time: one already in the list is answered so. If
/// it isn't a link, `note` says why instead. `button`: it was sent by a click on Add, which
/// leaves the focus on the button; `refocus` says the field takes the focus back once added, if
/// the focus is still there ([`Phantom::refocus_link`]). The answer is the field's: see [`Batch::answered`].
pub fn submit_url(service: &mut dyn Service, field: &TextEdit, note: &mut Option<AddNote>, refocus: &mut bool, button: bool) {
    match AddRequest::link(&field.text) {
        Err(problem) => *note = Some(AddNote::about(field, display::link_problem(problem))),
        Ok(request) => {
            *note = None;
            *refocus = button;
            let requests = vec![request];
            let batch = Batch::new(Some(field.text.trim().to_owned()), Vec::new());
            service.add(requests.clone(), None, batch.answer(&requests));
        }
    }
}

pub fn add_files(s: &mut Phantom, files: &[PathBuf]) {
    add(s, files.iter().cloned().map(AddRequest::File).collect(), Vec::new());
}

/// Adds the torrents into the default destination, for adds with no control in the window; the
/// `refused` ones never reach the service, and fail in the same note as the rest. Without a
/// default destination the service says so ([`AddError::NoDestination`]), and the user picks a
/// folder for them all first; cancelling adds none. Failures are told of in one note, once the
/// whole batch is answered.
pub fn add(s: &mut Phantom, requests: Vec<AddRequest>, refused: Vec<AddFailure>) {
    let batch = Batch::new(None, refused);
    if requests.is_empty() {
        batch.finish(s);
    } else {
        let answer = batch.answer(&requests);
        s.service.add(requests, None, answer);
    }
}

/// Asks the service to undo its top torrent action, for the entry `seq` ⌘Z or its notice's Undo
/// took (`crate::changes`): the service takes the top step at once, whatever runs (#190), and the
/// list shows it undone in the next snapshot, behind Settings if it's open. The notice of that
/// action leaves at once (#200), and comes back if the undo is refused; the status bar says what
/// came back, selected in the list, or why it couldn't. A selection's Remove or Delete is one entry:
/// all of it comes back (#235). `took` hears which entry the service took, undone or refused, if
/// it names one.
pub fn ask_undo(s: &mut Phantom, seq: u64, took: Box<dyn FnOnce(&mut Phantom, Option<crate::changes::machine::Entry>, &mut Tx) + Send>) {
    s.bar.undoing(vec![seq]);
    s.service.undo(Box::new(move |s: &mut Phantom, answer: Result<proto::Undone, client::Failure>, tx: &mut Tx| {
        let entry = taken_entry(&answer);
        match &answer {
            Ok(undone) => {
                for t in &undone.undo.torrents {
                    s.table.reveal(t.id.to_string(), tx);
                }
            }
            Err(e) => refused_asked(s, vec![seq], e),
        }
        for notice in undo_notices(&answer) {
            s.bar.tell(notice);
        }
        took(s, entry, tx);
    }));
}

/// The entry an undo's answer says the service took off its stack: undone, or refused (a refused
/// undo is gone too, ADR 0008); none when it says nothing of one (nothing to undo, the connection
/// lost).
pub(crate) fn taken_entry(answer: &Result<proto::Undone, client::Failure>) -> Option<crate::changes::machine::Entry> {
    let entry =
        |undo: &proto::Undoable| crate::changes::machine::Entry { seq: undo.seq, torrents: undo.torrents.iter().map(|t| t.id).collect() };
    match answer {
        Ok(undone) => Some(entry(&undone.undo)),
        Err(client::Failure::Refused(proto::Refusal::Undo { undo, .. })) => Some(entry(undo)),
        Err(_) => None,
    }
}

/// Undos asked for `seqs` weren't done, for the reason `failure` is: each such entry is told refused,
/// so its notice comes back saying why. (The service says it too, if it refused; the same word
/// is taken once.)
pub(crate) fn refused_asked(s: &mut Phantom, mut seqs: Vec<u64>, failure: &client::Failure) {
    let (why, stranded) = match failure {
        client::Failure::Refused(proto::Refusal::Undo { undo, why, stranded }) => {
            seqs.push(undo.seq);
            (why.clone(), stranded.clone())
        }
        client::Failure::Refused(proto::Refusal::NothingToUndo) => {
            (proto::CantUndo::Failed { detail: "there is nothing to undo".into() }, Vec::new())
        }
        other => (proto::CantUndo::Failed { detail: display::sentence(&format!("{other:#}")).to_lowercase() }, Vec::new()),
    };
    seqs.sort();
    seqs.dedup();
    let left: Vec<proto::Left> = seqs
        .into_iter()
        .map(|seq| proto::Left { seq, why: proto::Leave::Refused { why: why.clone(), stranded: stranded.clone() } })
        .collect();
    let new = s.undos.observe(&left);
    s.bar.left(new);
}

/// A torrent of a selection's undo that didn't come back (#235): one added again since says so, short
/// (nothing was touched); another, "Couldn't undo Delete of X", and why (more, in the popover).
fn not_back_notice(not: &proto::NotBack) -> Notice {
    match display::not_back(not) {
        (short, why) if why.is_empty() => Notice::new(short),
        (short, why) => Notice { more: Some(crate::notice::explained(why, Vec::new())), ..Notice::warning(short) },
    }
}

/// What the status bar tells of an undo's answer: what came back (what of a selection didn't, the
/// snapshot before it told), or why none could (more, in the popover); nothing when there was
/// nothing to undo.
pub fn undo_notices(undone: &Result<proto::Undone, client::Failure>) -> Vec<Notice> {
    let refused =
        |(short, why): (String, String)| Notice { more: Some(crate::notice::explained(why, Vec::new())), ..Notice::warning(short) };
    match undone {
        // What didn't come back the snapshot told already, as it comes first (`Leave::Undone`).
        Ok(undone) => vec![Notice::new(display::undone(&undone.undo))],
        Err(client::Failure::Refused(proto::Refusal::NothingToUndo)) => Vec::new(),
        // Added again by hand: nothing to bring back, and nothing touched.
        Err(client::Failure::Refused(proto::Refusal::Undo { why: proto::CantUndo::InList, .. })) => {
            vec![Notice::new(display::already_in_list())]
        }
        Err(client::Failure::Refused(proto::Refusal::Undo { undo, why, stranded })) => {
            vec![refused(display::cant_undo(undo, why, stranded))]
        }
        Err(e) => vec![Notice::warning(display::sentence(&format!("{e:#}")))],
    }
}

/// Sends `command` to each of `ids`; those that fail show together, as one entry in the error
/// list: "Couldn't delete 2 of 5", each torrent with why.
pub fn command(service: &mut dyn Service, ids: &[TorrentId], command: TorrentCommand) {
    command_then(service, None, ids, command, |_, _| {});
}

/// [`command`], then `done` with the torrents it did as asked. A Remove, Delete or Move to Trash
/// that failed in part tells of the entries it left on the undo stack past `after` (#200), so the
/// notice goes with them once they're undone.
fn command_then(
    service: &mut dyn Service,
    after: Option<u64>,
    ids: &[TorrentId],
    command: TorrentCommand,
    done: impl FnOnce(&mut Phantom, Vec<TorrentId>) + Send + 'static,
) {
    let (action, total, all) = (command.action(), ids.len(), ids.to_vec());
    service.command(
        ids,
        command,
        Box::new(move |s: &mut Phantom, failed: Vec<(TorrentId, client::Failure)>, _: &mut Tx| {
            let succeeded = all.into_iter().filter(|id| failed.iter().all(|(f, _)| f != id)).collect();
            let all_failed = failed.len();
            done(s, succeeded);
            // Taken back by an Undo before it ended (#190): not a failure, and the Undo tells it.
            let failed: Vec<(TorrentId, client::Failure)> =
                failed.into_iter().filter(|(_, e)| !matches!(e, client::Failure::Refused(proto::Refusal::Undone))).collect();
            if failed.is_empty() {
                return;
            }
            let named = |id: TorrentId| {
                s.torrents.iter().find(|t| t.id == id).map_or_else(|| format!("torrent {id}"), |t| t.display_name().to_owned())
            };
            let failed_ids: Vec<TorrentId> = failed.iter().map(|(id, _)| *id).collect();
            let failed: Vec<AddFailure> = failed
                .iter()
                .map(|(id, e)| {
                    let (what, why) = display::command_failure(&named(*id), e);
                    AddFailure { what, why }
                })
                .collect();
            let total = total - (all_failed - failed.len());
            let title = display::commands_failed(action, failed.len(), total);
            let list = move |_: &Phantom, ui: &Ui| {
                let failures: Vec<_> = failed.iter().map(|f| Failure { what: &f.what, why: &f.why }).collect();
                ui.failure_list(&failures)
            };
            // Tells of the entries its failures left, if each left one: else a notice that went with
            // one's undo would take another's failure with it.
            let entries = |id: TorrentId| -> Vec<u64> {
                s.undo.iter().filter(|u| after.is_some_and(|a| u.seq > a) && u.torrents.iter().any(|t| t.id == id)).map(|u| u.seq).collect()
            };
            let of: Vec<u64> = match failed_ids.iter().all(|id| !entries(*id).is_empty()) {
                true => failed_ids.iter().flat_map(|id| entries(*id)).collect(),
                false => Vec::new(),
            };
            s.bar.tell(Notice { more: Some(Rc::new(list)), of, ..Notice::error(title) });
        }),
    );
}

/// Asks where the next refused torrents go: one picker at a time, so adds refused while it asks
/// wait their turn.
fn ask_destination(s: &mut Phantom, tx: &mut Tx) {
    if s.adds.asking {
        return;
    }
    let Some(Waiting { requests, mut batch }) = s.adds.waiting.pop_front() else { return };
    s.adds.asking = true;
    let options = FileDialogOptions { title: destination_title(&requests), directory: downloads(), ..Default::default() };
    tx.open_folder(options, move |s: &mut Phantom, chosen, tx| {
        s.adds.asking = false;
        let folder = match chosen {
            Ok(Some(folder)) => client::absolute(&folder).map_err(|e| Some(format!("{e:#}"))),
            Ok(None) => Err(None),
            Err(e) => Err(Some(format!("can't ask where to download: {e}"))),
        };
        match folder {
            Ok(folder) => {
                let answer = batch.answer(&requests);
                s.service.add(requests, Some(folder), answer);
            }
            // None added: the batch fails for them, with the reason, unless cancelled.
            Err(why) => {
                if let Some(why) = why {
                    batch.total += requests.len();
                    batch.failed.extend(requests.iter().map(|r| AddFailure { what: what(r), why: display::sentence(&why) }));
                }
                batch.finish(s);
            }
        }
        ask_destination(s, tx);
    });
}

/// What the folder picker asks.
fn destination_title(requests: &[AddRequest]) -> String {
    match requests {
        [AddRequest::File(path)] => match path.file_stem() {
            Some(name) => format!("Choose where to download {}", name.to_string_lossy()),
            None => "Choose where to download the torrent".into(),
        },
        [_] => "Choose where to download the torrent".into(),
        _ => format!("Choose where to download {} torrents", requests.len()),
    }
}

/// Where the folder picker starts: `~/Downloads`.
fn downloads() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"))
}

/// Removes the selected torrents, or deletes them with their files if `delete`: one command, one
/// step on the undo stack (#235). Torrents fetching info have no files yet and stop either way.
/// What went is told in the status bar, its notice with an Undo of it.
pub fn remove_selected(s: &mut Phantom, delete: bool) {
    let ids = selected_torrents(&s.table);
    let (command, action) = match delete {
        true => (TorrentCommand::Delete, proto::UndoAction::Delete),
        false => (TorrentCommand::Remove, proto::UndoAction::Remove),
    };
    let names: Vec<(TorrentId, String)> =
        s.torrents.iter().filter(|t| ids.contains(&t.id)).map(|t| (t.id, t.display_name().to_owned())).collect();
    // Its entry on the undo stack comes after every one there now.
    let after = s.undo.iter().map(|u| u.seq).max().unwrap_or(0);
    // The newest change from now: ⌘Z takes it even before a snapshot lists it (#255).
    let pending = crate::changes::acted(s, after, ids.clone());
    command_then(&mut *s.service, Some(after), &ids, command, move |s, went| {
        let names: Vec<String> = names.into_iter().filter(|(id, _)| went.contains(id)).map(|(_, name)| name).collect();
        // The snapshot with its effect comes before its answer: its entry is on the stack now.
        let entry = s.undo.iter().find(|u| u.seq > after && u.torrents.iter().any(|t| went.contains(&t.id))).map(|u| u.seq);
        crate::changes::made(s, pending, entry);
        if let (false, Some(seq)) = (names.is_empty(), entry) {
            tell_happened(s, Happened { seq }, action, &names);
        }
    });
    s.table.clear_selection();
}

/// A Remove, Delete or Move to Trash that was done, as its notice knows it: its entry on the undo
/// stack, by seq (one press, one entry: #235).
#[derive(Debug, Clone, PartialEq)]
pub struct Happened {
    pub seq: u64,
}

impl Happened {
    /// Why it can't be undone now (#200), as the service says why its entry left the stack: its undo
    /// was refused, a newer Delete took its place, or it was pushed past the last 20. Or something
    /// newer is above it. `None`: its entry is the top of the stack.
    pub fn off(&self, stack: &[proto::Undoable], undos: &Undos) -> Option<UndoOff> {
        match stack.iter().rposition(|u| u.seq == self.seq) {
            Some(at) if at + 1 == stack.len() => None,
            Some(_) => Some(UndoOff::Newer),
            None => Some(match undos.why(self.seq) {
                Some(Leave::Refused { why, stranded }) => UndoOff::Refused(why.clone(), stranded.clone()),
                Some(Leave::Absorbed { .. }) => UndoOff::Newer,
                // Pushed off the stack, or undone (its notice leaves with it): the stack moved on.
                // (No word yet: an undo of it under way elsewhere, which took its notice out of the list.)
                Some(Leave::Aged | Leave::Dropped | Leave::Undone { .. }) | None => UndoOff::PastLimit,
            }),
        }
    }
}

/// Tells that `happened`, to the torrents `names`: what its files are now, and Undo, which takes
/// back that Remove or Delete and then tells what came back in its place.
fn tell_happened(s: &mut Phantom, happened: Happened, action: proto::UndoAction, names: &[String]) {
    let off = happened.clone();
    let mut undo = crate::notice::NoticeAction::new(display::undo(), {
        let happened = happened.clone();
        move |s: &mut Phantom| {
            if happened.off(&s.undo, &s.undos).is_none() {
                crate::changes::press_action(s, happened.seq);
            }
        }
    });
    // ⌘Z's, while ⌘Z would take this action; the button takes it all the same.
    let seq = happened.seq;
    undo.hint = Some(Rc::new(move |s: &Phantom| {
        let next = s.changes.state.top().and_then(crate::changes::machine::Change::entry);
        (next == Some(seq)).then(|| crate::shortcuts::hint(crate::shortcuts::Command::Undo))
    }));
    undo.off = Some(Rc::new(move |s: &Phantom| off.off(&s.undo, &s.undos).map(|why| display::undo_off(&why))));
    let count = match action {
        proto::UndoAction::TrashFiles { count } => count,
        proto::UndoAction::Remove | proto::UndoAction::Delete => names.len(),
    };
    let more = crate::notice::explained(display::happened_more(action, count), vec![undo]);
    s.bar.tell(Notice { more: Some(more), of: vec![happened.seq], ..Notice::new(display::happened(action, names)) });
}

pub fn sort(torrents: &mut [TorrentInfo], column: &str, dir: SortDir) {
    torrents.sort_by(|a, b| {
        dir.apply(match column {
            "name" => rune_kit::natural_cmp(a.name.as_deref().unwrap_or(""), b.name.as_deref().unwrap_or("")),
            "progress" => {
                a.content.map(|c| c.progress()).partial_cmp(&b.content.map(|c| c.progress())).unwrap_or(std::cmp::Ordering::Equal)
            }
            "status" => a.status.cmp(&b.status),
            "size" => a.content.map(|c| c.size_bytes).cmp(&b.content.map(|c| c.size_bytes)),
            "down" => a.download_bytes_per_sec.cmp(&b.download_bytes_per_sec),
            "up" => a.upload_bytes_per_sec.cmp(&b.upload_bytes_per_sec),
            "peers" => a.live_peers.cmp(&b.live_peers),
            "eta" => a.eta().cmp(&b.eta()),
            _ => a.id.cmp(&b.id),
        })
    });
}

pub fn sort_files(details: &mut Details, column: &str, dir: SortDir) {
    details.files.sort_by(|a, b| {
        dir.apply(match column {
            "path" => rune_kit::natural_cmp(&a.path.to_string_lossy(), &b.path.to_string_lossy()),
            "progress" => a.progress().total_cmp(&b.progress()),
            "size" => a.size_bytes.cmp(&b.size_bytes),
            "priority" => a.priority.cmp(&b.priority),
            _ => a.index.cmp(&b.index),
        })
    });
}

#[cfg(test)]
mod link_problem_tests {
    //! What Add accepts, over generated text rather than examples.

    use proptest::prelude::*;

    use super::link_problem;
    use client::LinkProblem;

    /// An info hash: 40 hex digits, or 32 base32 letters and digits, in either case.
    fn info_hash() -> impl Strategy<Value = String> {
        prop_oneof!["[0-9a-fA-F]{40}", "[a-zA-Z2-7]{32}"]
    }

    /// Other magnet parameters, never `xt=`: their keys have no `x`.
    fn params() -> impl Strategy<Value = Vec<String>> {
        proptest::collection::vec("[a-w]{1,4}=[A-Za-z0-9%+.:/-]{0,12}", 0..4)
    }

    fn whitespace() -> impl Strategy<Value = String> {
        "[ \t\n\r]{0,3}"
    }

    proptest! {
        #[test]
        fn whitespace_around_never_changes_the_answer(text in "\\PC{0,60}", before in whitespace(), after in whitespace()) {
            prop_assert_eq!(link_problem(&format!("{before}{text}{after}")), link_problem(&text));
        }

        #[test]
        fn a_magnet_with_an_info_hash_is_a_link(hash in info_hash(), params in params(), at in 0..4usize) {
            let mut params = params;
            params.insert(at.min(params.len()), format!("xt=urn:btih:{hash}"));
            let link = format!("magnet:?{}", params.join("&"));
            prop_assert_eq!(link_problem(&link), None, "{}", link);
        }

        #[test]
        fn a_magnet_without_xt_has_no_info_hash(params in params()) {
            let link = format!("magnet:?{}", params.join("&"));
            prop_assert_eq!(link_problem(&link), Some(LinkProblem::NoInfoHash), "{}", link);
        }

        #[test]
        fn a_bare_info_hash_is_a_link(hash in info_hash()) {
            prop_assert_eq!(link_problem(&hash), None);
        }

        #[test]
        fn a_torrent_url_is_a_link(scheme in "https?", host in "[a-z0-9-]{1,12}(\\.[a-z]{2,6}){1,2}", path in "(/[A-Za-z0-9._%-]{1,10}){0,3}", name in "[A-Za-z0-9._%-]{1,16}") {
            let url = format!("{scheme}://{host}{path}/{name}.torrent");
            prop_assert_eq!(link_problem(&url), None, "{}", url);
        }

        /// Text with a character no info hash has, and no scheme in front.
        #[test]
        fn other_text_is_not_a_link(before in "\\PC{0,20}", odd in "[!_./ #?=-]", after in "\\PC{0,20}") {
            let text = format!("{before}{odd}{after}");
            let trimmed = text.trim();
            prop_assume!(!["magnet:?", "http://", "https://"].iter().any(|scheme| trimmed.starts_with(scheme)));
            prop_assert_eq!(link_problem(&text), Some(LinkProblem::NotALink), "{:?}", text);
        }

        #[test]
        fn nothing_or_only_whitespace_is_not_a_link(text in whitespace()) {
            prop_assert_eq!(link_problem(&text), Some(LinkProblem::NotALink));
        }
    }
}
