//! Move to… (#24, #270): the row menu's item asks for one folder (the system's picker, starting
//! where the first torrent that can move is) and moves the selected torrents that can into it, each
//! as it would be on its own; those that can't are skipped. Before anything moves the service says
//! which of them clash with files already in the folder (`Request::Clashes`), and the clash dialog
//! asks about each, one after another, in the list's order: Use existing or Replace is that
//! torrent's choice, Cancel skips only it, and "Apply to the remaining N" gives the same answer to
//! the rest (those it's offered to: another torrent's files are never replaced nor used). Rename…
//! moves that torrent in under a name typed in the dialog, a field with #335's rules and notes; a
//! name is one torrent's, so the box is hidden meanwhile. For a clash with another listed
//! torrent's files the dialog offers only Show in list (which skips it, as Cancel does, and selects
//! that torrent), Cancel and Rename…. Then one request moves them all (`Request::Moves`), which the service runs one after
//! another in that order, each saying Waiting to move till its turn.
//!
//! One torrent that fails is told as an error notice, as ever; a selection tells once all have
//! ended: "Moved 2 of 3", the torrents that didn't, and why, in its list.

use std::collections::{BTreeMap, VecDeque};

use client::{Failure, display};
use proto::{AbsolutePath, Action, FileName, Found, MoveOf, NameProblem, NotMoved, Refusal, TorrentId, TorrentRefusal};
use rune::{FileDialogOptions, TextEdit, Tx};

use super::{AddFailure, Phantom, retry};
use crate::bar::machine::NoticeId;
use crate::notice::{Listing, Notice, NoticeAction};
use crate::platform::Show;

/// The clash dialog, while it asks about torrent `id`, called `name`: its files, in `to`, are there
/// already, and `remaining` more of the selection clash.
#[derive(Debug, Clone, PartialEq)]
pub struct Clash {
    pub id: TorrentId,
    pub name: String,
    pub to: AbsolutePath,
    pub files: Vec<AbsolutePath>,
    pub remaining: usize,
    /// Where it is among the selection's clashes, 1 first, and how many there are: the title's
    /// counter ("· 1 of 3"), none for one alone.
    pub at: usize,
    pub of: usize,
    /// "Apply to the remaining N" is ticked: the choice goes to every torrent still to be asked.
    pub apply_to_rest: bool,
    /// The files are another listed torrent's, this one: only Show in list, Cancel and Rename… (#270).
    pub other: Option<TorrentId>,
    /// Rename… is open: the name it takes in `to`.
    pub renaming: Option<Naming>,
    /// The choice (its key) the pointer rests on, whose tooltip says what it does.
    pub explained: Option<&'static str>,
}

/// The clash dialog's name field, while Rename… is open: as #335's in a row.
#[derive(Clone, PartialEq)]
pub struct Naming {
    pub field: TextEdit,
    /// Why the name sent with Return isn't one, with that text: shown until the text changes.
    pub refused: Option<(String, NameProblem)>,
}

impl Naming {
    /// Why the name in the field isn't one, while it's the text Return was refused for.
    pub fn note(&self) -> Option<NameProblem> {
        self.refused.as_ref().filter(|(text, _)| *text == self.field.text).map(|(_, p)| *p)
    }
}

impl std::fmt::Debug for Naming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Naming").field("text", &self.field.text).field("refused", &self.refused).finish()
    }
}

/// What the person chose for a torrent that clashes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Choice {
    Use(proto::Clash),
    /// Rename…: it moves in under this name, nothing of what's there answered for.
    Rename(FileName),
    /// Cancel: it isn't moved, the others are.
    Skip,
}

/// A Move of a selection, while the dialogs ask what to do with each clash: nothing is sent till
/// they're answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Asking {
    to: AbsolutePath,
    /// The selection's torrents that can move, in the list's order.
    order: Vec<TorrentId>,
    /// The others, with why each can't, for the notice's list.
    skipped: Vec<Unmoved>,
    /// How many torrents were selected.
    selected: usize,
    /// Those not asked about yet, in order, with their files and whose they are, if another
    /// listed torrent's.
    clashing: VecDeque<(TorrentId, Vec<Found>, Option<TorrentId>)>,
    /// The choice for each that was asked about, with its files as they were found: the answer
    /// holds for those only (#270).
    chosen: BTreeMap<TorrentId, (Choice, Vec<Found>)>,
    /// How many clashed when the service answered: the dialogs' counter.
    of: usize,
    /// The notice whose Move to… asked: it goes once a Move is sent.
    leaves: Option<NoticeId>,
}

/// A torrent of the selection that didn't move, and why: what the notice's list says.
#[derive(Debug, Clone, PartialEq)]
struct Unmoved {
    /// Try again and Move to… are for these.
    id: TorrentId,
    name: String,
    why: display::Reason,
}

/// The selected torrents, in the list's order.
fn selection(s: &Phantom) -> Vec<TorrentId> {
    let selected = super::selected_torrents(&s.table);
    s.torrents.iter().filter(|t| selected.contains(&t.id)).map(|t| t.id).collect()
}

/// Move to ▸ a folder it offers (the default, a recent one): moves the selected torrents that can
/// into `to`, with no picker.
pub fn move_selection_to(s: &mut Phantom, to: AbsolutePath) {
    let ids = selection(s);
    start(s, ids, to);
}

/// Other…: asks where the selected torrents go, starting at the folder of the first that can
/// move, and moves those that can.
pub fn move_to(s: &mut Phantom, tx: &mut Tx) {
    let ids = selection(s);
    pick_folder(s, ids, None, tx);
}

/// [`move_to`] for `ids`; the notice `leaves` goes once the Move is sent. While a picker is open
/// no second opens.
pub fn pick_folder(s: &mut Phantom, ids: Vec<TorrentId>, leaves: Option<NoticeId>, tx: &mut Tx) {
    if s.picking {
        return;
    }
    let Some(first) = s.torrents.iter().find(|t| ids.contains(&t.id) && t.actions.does(Action::Move)) else { return };
    let directory = first.location.as_ref().and_then(|l| l.parent()).map(|p| p.to_path_buf());
    let title = match ids.as_slice() {
        [_] => display::move_picker(first.display_name()),
        _ => display::move_picker_several(ids.len()),
    };
    let options = FileDialogOptions { title, directory, ..Default::default() };
    s.picking = true;
    tx.open_folder(options, move |s: &mut Phantom, chosen, _| {
        s.picking = false;
        match chosen {
            Ok(Some(folder)) => match client::absolute(&folder) {
                Ok(to) => start_leaving(s, ids, to, leaves),
                Err(e) => unasked(s, &ids, &format!("{e:#}")),
            },
            Ok(None) => {}
            Err(e) => unasked(s, &ids, &format!("can't ask for a folder: {e}")),
        }
    });
}

/// A Move of `ids` with no folder to go to: `why`, and Move to… to ask for one again.
fn unasked(s: &mut Phantom, ids: &[TorrentId], why: &str) {
    let actions = vec![retry::move_to(s, ids)];
    tell(s, ids.first().copied().unwrap_or_default(), why, actions);
}

/// Moves the torrents of `ids` (the selection, in the list's order) that can move into `to`: asks
/// the service which clash, then each dialog in turn, then sends the Moves. `to` is the newest of
/// the recent folders (Move to ▸) from now.
pub fn start(s: &mut Phantom, ids: Vec<TorrentId>, to: AbsolutePath) {
    start_leaving(s, ids, to, None);
}

/// [`start`], for the button of notice `leaves`: it goes once a Move is sent.
fn start_leaving(s: &mut Phantom, ids: Vec<TorrentId>, to: AbsolutePath, leaves: Option<NoticeId>) {
    let (order, skipped): (Vec<TorrentId>, Vec<Unmoved>) = {
        let mut order = Vec::new();
        let mut skipped = Vec::new();
        for id in &ids {
            match s.torrents.iter().find(|t| t.id == *id) {
                Some(t) if t.actions.does(Action::Move) => order.push(*id),
                Some(t) => skipped.push(Unmoved {
                    id: *id,
                    name: t.display_name().to_owned(),
                    why: display::why_off(&[t.actions.get(Action::Move)]).unwrap_or_default().into(),
                }),
                // Not listed yet (a scene before its first snapshot): the service says whether it moves.
                None => order.push(*id),
            }
        }
        (order, skipped)
    };
    if order.is_empty() {
        return;
    }
    s.recent.record(to.clone());
    let asking = Asking {
        to: to.clone(),
        selected: ids.len(),
        order: order.clone(),
        skipped,
        clashing: VecDeque::new(),
        chosen: BTreeMap::new(),
        of: 0,
        leaves,
    };
    s.service.clashes(
        order,
        to,
        Box::new(move |s: &mut Phantom, found: Result<Vec<proto::Clashing>, Failure>, _: &mut Tx| match found {
            Ok(found) => {
                let clashing: VecDeque<_> = found.into_iter().map(|c| (c.id, c.files, c.other)).collect();
                s.asking = Some(Asking { of: clashing.len(), clashing, ..asking });
                go_on(s);
            }
            Err(e) => {
                let id = asking.order.first().copied().unwrap_or_default();
                let actions = retry::moving(s, &asking.order, asking.to.clone());
                tell(s, id, &format!("{e:#}"), actions);
            }
        }),
    );
}

/// The next clash's dialog, or, with none left to ask, the Moves.
fn go_on(s: &mut Phantom) {
    let Some(asking) = &s.asking else { return };
    match asking.clashing.front() {
        Some((id, files, other)) => {
            let (id, files, to, remaining) = (*id, Found::paths(files), asking.to.clone(), asking.clashing.len() - 1);
            // Those answered by "Apply to the remaining" count as asked: it's the next one's place.
            let (of, at) = (asking.of, asking.of - asking.clashing.len() + 1);
            let (name, other) = (name(s, id), *other);
            crate::overlay::opening(s, crate::overlay::Overlay::Clash);
            s.clash = Some(Clash { id, name, to, files, remaining, at, of, apply_to_rest: false, other, renaming: None, explained: None });
        }
        None => {
            s.clash = None;
            if let Some(asking) = s.asking.take() {
                send(s, asking);
            }
        }
    }
}

/// The clash dialog's choice for the torrent it asks about (`None`, Cancel: skip it), and, with
/// "Apply to the remaining" ticked, for every one still to be asked.
/// While Rename… is open the box is hidden: Cancel skips only this one. Replace and Use existing
/// go to the rest whose files aren't another torrent's; those are still asked about.
pub fn choose(s: &mut Phantom, choice: Option<proto::Clash>) {
    let rest = s.clash.as_ref().is_some_and(|c| c.apply_to_rest && c.renaming.is_none());
    answer(s, choice.map_or(Choice::Skip, Choice::Use), rest);
}

/// `choice` for the torrent asked about, and with `rest`, for those still to be asked it's offered to.
fn answer(s: &mut Phantom, choice: Choice, rest: bool) {
    if s.clash.take().is_none() {
        return;
    }
    let Some(asking) = &mut s.asking else { return };
    let mut answered: Vec<_> = asking.clashing.pop_front().into_iter().collect();
    if rest {
        let offered = |other: &Option<TorrentId>| other.is_none() || choice == Choice::Skip;
        let (to, left): (VecDeque<_>, VecDeque<_>) = asking.clashing.drain(..).partition(|(_, _, other)| offered(other));
        answered.extend(to);
        asking.clashing = left;
    }
    asking.chosen.extend(answered.into_iter().map(|(id, found, _)| (id, (choice.clone(), found))));
    go_on(s);
}

/// Rename…: asks the service for a name that is free in the folder now, as the dialog's name field
/// opens (never with the name that clashes): see [`rename_opens`]. With none to suggest it opens with
/// the torrent's own.
pub fn rename(s: &mut Phantom) {
    let Some(clash) = &s.clash else { return };
    let (id, to) = (clash.id, clash.to.clone());
    s.service.free_name(
        id,
        to,
        Box::new(move |s: &mut Phantom, found: Result<proto::FileName, Failure>, _: &mut Tx| match found {
            Ok(name) => rename_opens(s, id, name.as_str()),
            // No free name to suggest: the field opens with the torrent's own, to change by hand.
            Err(_) => {
                let own = s.clash.as_ref().filter(|c| c.id == id).map(|c| c.name.clone());
                if let Some(own) = own {
                    rename_opens(s, id, &own);
                }
            }
        }),
    );
}

/// The name field opens, focused, with `name`, the free one: the same selection as #335's field in
/// a row ([`super::renaming::selected_end`]). Nothing opens if the dialog is another torrent's by now.
///
/// The dialog doesn't have the torrent's files, whoever's the clash is with; the service does, and
/// numbers a name before its extension only for a torrent of one file. So it's one file when the
/// name is the torrent's own extension after a ` (n)`.
fn rename_opens(s: &mut Phantom, id: TorrentId, name: &str) {
    let Some(clash) = s.clash.as_mut().filter(|c| c.id == id) else { return };
    let extension = &clash.name[super::renaming::stem(&clash.name)..];
    let one_file = !extension.is_empty() && name.strip_suffix(extension).is_some_and(|numbered| numbered.ends_with(')'));
    let mut field = TextEdit::new(name.to_owned());
    field.anchor = 0;
    field.focus = super::renaming::selected_end(name, one_file);
    field.focused = true;
    clash.renaming = Some(Naming { field, refused: None });
}

/// Return in the name field, or Move: the torrent moves in under the name typed, if it's one a
/// file can have; else the field stays, saying why. It answers only this torrent.
pub fn submit_name(s: &mut Phantom) {
    let Some(naming) = s.clash.as_mut().and_then(|c| c.renaming.as_mut()) else { return };
    let text = naming.field.text.clone();
    match FileName::new(text.clone()) {
        Ok(name) => answer(s, Choice::Rename(name), false),
        Err(problem) => {
            naming.refused = Some((text, problem));
            naming.field.focused = true;
        }
    }
}

/// Escape in the name field: back to the dialog's choices, the box as it was.
pub fn close_name(s: &mut Phantom) {
    if let Some(clash) = &mut s.clash {
        clash.renaming = None;
    }
}

/// Ticks or unticks "Apply to the remaining N".
pub fn apply_to_rest(s: &mut Phantom) {
    if let Some(clash) = &mut s.clash {
        clash.apply_to_rest = !clash.apply_to_rest;
    }
}

/// The pointer came to rest on choice `key` (`Some`), or left it: its tooltip shows while it rests.
pub fn explain(s: &mut Phantom, key: &'static str, resting: bool) {
    if let Some(clash) = &mut s.clash {
        match resting {
            true => clash.explained = Some(key),
            false if clash.explained == Some(key) => clash.explained = None,
            false => {}
        }
    }
}

/// Show in list, in a clash with another listed torrent's files: this torrent's question ends as
/// Cancel's does (it isn't moved, only it: the box doesn't count), then the list selects that
/// other torrent alone and scrolls it into view, its details in the pane. With more to ask, the
/// next dialog follows.
pub fn show_in_list(s: &mut Phantom, tx: &mut Tx) {
    let Some(other) = s.clash.as_ref().and_then(|c| c.other) else { return };
    answer(s, Choice::Skip, false);
    s.table.reveal(other.to_string(), tx);
    super::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
}

/// Drops the Move that's being asked about, with nothing sent: the dialog was closed some other way.
pub fn abandon(s: &mut Phantom) {
    s.clash = None;
    s.asking = None;
}

/// Shows the `i`th clashing file in Finder (another torrent's place, where none was found); the
/// dialog stays open, for the choice.
pub fn reveal(s: &mut Phantom, i: usize) {
    let Some(clash) = &s.clash else { return };
    let path = clash.files.get(i).map_or_else(|| clash.to.join(&clash.name), |f| f.to_path_buf());
    crate::platform::show(s, &path, Show::Reveal);
}

/// Sends the Moves of those that weren't skipped, in the list's order, and tells how they went.
fn send(s: &mut Phantom, asking: Asking) {
    let Asking { to, order, mut skipped, selected, chosen, leaves, .. } = asking;
    let mut moves = Vec::new();
    for id in order {
        match chosen.get(&id) {
            Some((Choice::Skip, _)) => {
                skipped.push(Unmoved { id, name: name(s, id), why: display::not_moved(&NotMoved::Cancelled).into() })
            }
            Some((Choice::Use(choice), found)) => {
                moves.push(MoveOf { id, clash: Some(proto::Answered { choice: *choice, found: found.clone() }), name: None })
            }
            Some((Choice::Rename(name), _)) => moves.push(MoveOf { id, clash: None, name: Some(name.clone()) }),
            None => moves.push(MoveOf { id, clash: None, name: None }),
        }
    }
    if moves.is_empty() {
        // The only torrent, its Move cancelled in its dialog: as asked, nothing to tell.
        return if selected == 1 { () } else { tell_selection(s, selected, 0, skipped, &to) };
    }
    // Sent, even if only some: the notice's buttons were for the whole, and what's not sent was
    // answered for in a dialog.
    if let Some(notice) = leaves {
        s.bar.take(notice);
    }
    let sent: Vec<TorrentId> = moves.iter().map(|m| m.id).collect();
    s.service.moves(
        to.clone(),
        moves,
        Box::new(move |s: &mut Phantom, failed: Vec<(TorrentId, Failure)>, _: &mut Tx| {
            // Taken back by an Undo before it ended: not a failure, and the Undo tells it.
            let failed: Vec<(TorrentId, Failure)> =
                failed.into_iter().filter(|(_, e)| !matches!(e, Failure::Refused(Refusal::Undone))).collect();
            match (selected, sent.as_slice()) {
                (1, [id]) => one(s, *id, failed, &to),
                _ => {
                    let moved = sent.iter().filter(|id| failed.iter().all(|(f, _)| f != *id)).count();
                    let mut unmoved = skipped;
                    unmoved.extend(failed.iter().map(|(id, failure)| unmoved_by(s, *id, failure)));
                    tell_selection(s, selected, moved, unmoved, &to);
                }
            }
        }),
    );
}

/// The failure of a Move of the only torrent selected, as it's told: an error notice, but not one a
/// Remove, a Delete or Cancel move asked for.
fn one(s: &mut Phantom, id: TorrentId, failed: Vec<(TorrentId, Failure)>, to: &AbsolutePath) {
    let Some((_, failure)) = failed.into_iter().next() else { return };
    if cancelled(&failure) {
        return;
    }
    let name = name(s, id);
    let (what, why) = display::command_failure(&name, &failure);
    told(s, AddFailure { what, why, request: None }, retry::moving(s, &[id], to.clone()));
}

/// A Move that failed, as a line of the notice's list: the torrent and why, which names it no
/// more ("Its new folder changed while it waited: …").
fn unmoved_by(s: &Phantom, id: TorrentId, failure: &Failure) -> Unmoved {
    match failure {
        _ if cancelled(failure) => Unmoved { id, name: name(s, id), why: display::not_moved(&NotMoved::Cancelled).into() },
        Failure::Refused(Refusal::Torrent { why, .. }) => Unmoved { id, name: name(s, id), why: display::refused_why(why) },
        _ => {
            let (what, why) = display::command_failure(&name(s, id), failure);
            Unmoved { id, name: what, why }
        }
    }
}

/// What the person asked for meanwhile: a Remove, a Delete or Cancel move.
fn cancelled(failure: &Failure) -> bool {
    matches!(failure, Failure::Refused(Refusal::Torrent { why: TorrentRefusal::NotMoved { why: NotMoved::Cancelled, .. }, .. }))
}

/// A selection's Move once it ended: "Moved 2 of 3", with those that didn't, and why, in the list.
/// Nothing is told when every torrent moved.
fn tell_selection(s: &mut Phantom, selected: usize, moved: usize, unmoved: Vec<Unmoved>, to: &AbsolutePath) {
    if unmoved.is_empty() {
        return;
    }
    let list = Listing::of_entries(unmoved.iter().map(|u| (u.name.as_str(), &u.why)));
    let left: Vec<TorrentId> = unmoved.iter().map(|u| u.id).collect();
    let actions = retry::moving(s, &left, to.clone());
    s.bar.tell(Notice { more: Some(list), ..Notice::warning(display::moved_of(moved, selected)) }.with_actions(actions));
}

fn name(s: &Phantom, id: TorrentId) -> String {
    s.torrents.iter().find(|t| t.id == id).map_or_else(|| format!("torrent {id}"), |t| t.display_name().to_owned())
}

/// A Move that couldn't be asked, `id` its first torrent: `why`, and the buttons that ask again.
fn tell(s: &mut Phantom, id: TorrentId, why: &str, actions: Vec<NoticeAction>) {
    told(s, AddFailure { what: name(s, id), why: display::sentence(why).into(), request: None }, actions);
}

/// A Move that failed, as any failed command is told: "Couldn't move the torrent", the torrent and
/// why under it.
fn told(s: &mut Phantom, failed: AddFailure, actions: Vec<NoticeAction>) {
    let title = display::commands_failed(Action::Move, 1, 1);
    let list = Listing::of_entries([(failed.what.as_str(), &failed.why)]);
    s.bar.tell(Notice { more: Some(list), ..Notice::error(title) }.with_actions(actions));
}
