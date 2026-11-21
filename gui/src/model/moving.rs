//! Move to… (#24, #270): the row menu's item asks for one folder (the system's picker, starting
//! where the first torrent that can move is) and moves the selected torrents that can into it, each
//! as it would be on its own; those that can't are skipped. Before anything moves the service says
//! which of them clash with files already in the folder (`Request::Clashes`), and the clash dialog
//! asks about each, one after another, in the list's order: Use existing or Replace is that
//! torrent's choice, Cancel skips only it, and "Apply to the remaining N" gives the same answer to
//! the rest. Then one request moves them all (`Request::Moves`), which the service runs one after
//! another in that order, each saying Waiting to move till its turn.
//!
//! One torrent that fails is told as an error notice, as ever; a selection tells once all have
//! ended: "Moved 2 of 3", the torrents that didn't, and why, in its list.

use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use client::{Failure, display};
use proto::{AbsolutePath, Action, Found, MoveOf, NotMoved, Refusal, TorrentId, TorrentRefusal};
use rune::{FileDialogOptions, Tx};

use super::{AddFailure, Phantom};
use crate::notice::Notice;
use crate::platform::Show;
use crate::ui::Ui;

/// The clash dialog, while it asks about torrent `id`, called `name`: its files, in `to`, are there
/// already, and `remaining` more of the selection clash.
#[derive(Debug, Clone, PartialEq)]
pub struct Clash {
    pub id: TorrentId,
    pub name: String,
    pub to: AbsolutePath,
    pub files: Vec<AbsolutePath>,
    pub remaining: usize,
    /// "Apply to the remaining N" is ticked: the choice goes to every torrent still to be asked.
    pub apply_to_rest: bool,
}

/// What the person chose for a torrent that clashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Use(proto::Clash),
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
    /// Those not asked about yet, in order, with their files.
    clashing: VecDeque<(TorrentId, Vec<Found>)>,
    /// The choice for each that was asked about, with its files as they were found: the answer
    /// holds for those only (#270).
    chosen: BTreeMap<TorrentId, (Choice, Vec<Found>)>,
}

/// A torrent of the selection that didn't move, and why: what the notice's list says.
#[derive(Debug, Clone, PartialEq)]
struct Unmoved {
    name: String,
    why: String,
}

/// Asks where the selected torrents go, starting at the folder of the first that can move, and
/// moves those that can.
pub fn move_to(s: &mut Phantom, tx: &mut Tx) {
    let selected = super::selected_torrents(&s.table);
    let ids: Vec<TorrentId> = s.torrents.iter().filter(|t| selected.contains(&t.id)).map(|t| t.id).collect();
    let Some(first) = s.torrents.iter().find(|t| ids.contains(&t.id) && t.actions.does(Action::Move)) else { return };
    let directory = first.location.as_ref().and_then(|l| l.parent()).map(|p| p.to_path_buf());
    let title = match ids.as_slice() {
        [_] => display::move_picker(first.display_name()),
        _ => display::move_picker_several(ids.len()),
    };
    let options = FileDialogOptions { title, directory, ..Default::default() };
    tx.open_folder(options, move |s: &mut Phantom, chosen, _| match chosen {
        Ok(Some(folder)) => match client::absolute(&folder) {
            Ok(to) => start(s, ids, to),
            Err(e) => tell(s, ids.first().copied().unwrap_or_default(), &format!("{e:#}")),
        },
        Ok(None) => {}
        Err(e) => tell(s, ids.first().copied().unwrap_or_default(), &format!("can't ask for a folder: {e}")),
    });
}

/// Moves the torrents of `ids` (the selection, in the list's order) that can move into `to`: asks
/// the service which clash, then each dialog in turn, then sends the Moves.
pub fn start(s: &mut Phantom, ids: Vec<TorrentId>, to: AbsolutePath) {
    let (order, skipped): (Vec<TorrentId>, Vec<Unmoved>) = {
        let mut order = Vec::new();
        let mut skipped = Vec::new();
        for id in &ids {
            match s.torrents.iter().find(|t| t.id == *id) {
                Some(t) if t.actions.does(Action::Move) => order.push(*id),
                Some(t) => skipped.push(Unmoved {
                    name: t.display_name().to_owned(),
                    why: display::why_off(&[t.actions.get(Action::Move)]).unwrap_or_default(),
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
    let asking =
        Asking { to: to.clone(), selected: ids.len(), order: order.clone(), skipped, clashing: VecDeque::new(), chosen: BTreeMap::new() };
    s.service.clashes(
        order,
        to,
        Box::new(move |s: &mut Phantom, found: Result<Vec<proto::Clashing>, Failure>, _: &mut Tx| match found {
            Ok(found) => {
                let clashing = found.into_iter().map(|c| (c.id, c.files)).collect();
                s.asking = Some(Asking { clashing, ..asking });
                go_on(s);
            }
            Err(e) => {
                let id = asking.order.first().copied().unwrap_or_default();
                tell(s, id, &format!("{e:#}"));
            }
        }),
    );
}

/// The next clash's dialog, or, with none left to ask, the Moves.
fn go_on(s: &mut Phantom) {
    let Some(asking) = &s.asking else { return };
    match asking.clashing.front() {
        Some((id, files)) => {
            let (id, files, to, remaining) = (*id, Found::paths(files), asking.to.clone(), asking.clashing.len() - 1);
            let name = name(s, id);
            crate::overlay::opening(s, crate::overlay::Overlay::Clash);
            s.clash = Some(Clash { id, name, to, files, remaining, apply_to_rest: false });
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
pub fn choose(s: &mut Phantom, choice: Option<proto::Clash>) {
    let Some(clash) = s.clash.take() else { return };
    let Some(asking) = &mut s.asking else { return };
    let choice = choice.map_or(Choice::Skip, Choice::Use);
    let answered = match clash.apply_to_rest {
        true => asking.clashing.drain(..).collect::<Vec<_>>(),
        false => asking.clashing.pop_front().into_iter().collect(),
    };
    asking.chosen.extend(answered.into_iter().map(|(id, found)| (id, (choice, found))));
    go_on(s);
}

/// Ticks or unticks "Apply to the remaining N".
pub fn apply_to_rest(s: &mut Phantom) {
    if let Some(clash) = &mut s.clash {
        clash.apply_to_rest = !clash.apply_to_rest;
    }
}

/// Drops the Move that's being asked about, with nothing sent: the dialog was closed some other way.
pub fn abandon(s: &mut Phantom) {
    s.clash = None;
    s.asking = None;
}

/// Shows the clashing files in Finder; the dialog stays open, for the choice.
pub fn reveal(s: &mut Phantom) {
    let Some(path) = s.clash.as_ref().and_then(|c| c.files.first()).cloned() else { return };
    crate::platform::show(s, &path, Show::Reveal);
}

/// Sends the Moves of those that weren't skipped, in the list's order, and tells how they went.
fn send(s: &mut Phantom, asking: Asking) {
    let Asking { to, order, mut skipped, selected, chosen, .. } = asking;
    let mut moves = Vec::new();
    for id in order {
        match chosen.get(&id) {
            Some((Choice::Skip, _)) => skipped.push(Unmoved { name: name(s, id), why: display::not_moved(&NotMoved::Cancelled) }),
            Some((Choice::Use(choice), found)) => {
                moves.push(MoveOf { id, clash: Some(proto::Answered { choice: *choice, found: found.clone() }) })
            }
            None => moves.push(MoveOf { id, clash: None }),
        }
    }
    if moves.is_empty() {
        // The only torrent, its Move cancelled in its dialog: as asked, nothing to tell.
        return if selected == 1 { () } else { tell_selection(s, selected, 0, skipped) };
    }
    let sent: Vec<TorrentId> = moves.iter().map(|m| m.id).collect();
    s.service.moves(
        to,
        moves,
        Box::new(move |s: &mut Phantom, failed: Vec<(TorrentId, Failure)>, _: &mut Tx| {
            // Taken back by an Undo before it ended: not a failure, and the Undo tells it.
            let failed: Vec<(TorrentId, Failure)> =
                failed.into_iter().filter(|(_, e)| !matches!(e, Failure::Refused(Refusal::Undone))).collect();
            match (selected, sent.as_slice()) {
                (1, [id]) => one(s, *id, failed),
                _ => {
                    let moved = sent.iter().filter(|id| failed.iter().all(|(f, _)| f != *id)).count();
                    let mut unmoved = skipped;
                    unmoved.extend(failed.iter().map(|(id, failure)| unmoved_by(s, *id, failure)));
                    tell_selection(s, selected, moved, unmoved);
                }
            }
        }),
    );
}

/// The failure of a Move of the only torrent selected, as it's told: an error notice, but not one a
/// Remove, a Delete or Cancel move asked for.
fn one(s: &mut Phantom, id: TorrentId, failed: Vec<(TorrentId, Failure)>) {
    let Some((_, failure)) = failed.into_iter().next() else { return };
    if cancelled(&failure) {
        return;
    }
    let name = name(s, id);
    let (what, why) = display::command_failure(&name, &failure);
    told(s, AddFailure { what, why });
}

/// A Move that failed, as a line of the notice's list: the torrent and why, which names it no
/// more ("Its new folder changed while it waited: …").
fn unmoved_by(s: &Phantom, id: TorrentId, failure: &Failure) -> Unmoved {
    match failure {
        _ if cancelled(failure) => Unmoved { name: name(s, id), why: display::not_moved(&NotMoved::Cancelled) },
        Failure::Refused(Refusal::Torrent { why, .. }) => Unmoved { name: name(s, id), why: display::refused_why(why) },
        _ => {
            let (what, why) = display::command_failure(&name(s, id), failure);
            Unmoved { name: what, why }
        }
    }
}

/// What the person asked for meanwhile: a Remove, a Delete or Cancel move.
fn cancelled(failure: &Failure) -> bool {
    matches!(failure, Failure::Refused(Refusal::Torrent { why: TorrentRefusal::NotMoved { why: NotMoved::Cancelled, .. }, .. }))
}

/// A selection's Move once it ended: "Moved 2 of 3", with those that didn't, and why, in the list.
/// Nothing is told when every torrent moved.
fn tell_selection(s: &mut Phantom, selected: usize, moved: usize, unmoved: Vec<Unmoved>) {
    if unmoved.is_empty() {
        return;
    }
    let list = move |_: &Phantom, ui: &Ui| {
        let failures: Vec<_> = unmoved.iter().map(|u| crate::ui::Failure { what: &u.name, why: &u.why }).collect();
        ui.failure_list(&failures)
    };
    s.bar.tell(Notice { more: Some(Rc::new(list)), ..Notice::warning(display::moved_of(moved, selected)) });
}

fn name(s: &Phantom, id: TorrentId) -> String {
    s.torrents.iter().find(|t| t.id == id).map_or_else(|| format!("torrent {id}"), |t| t.display_name().to_owned())
}

/// A Move of torrent `id` that couldn't be asked: `why`.
fn tell(s: &mut Phantom, id: TorrentId, why: &str) {
    told(s, AddFailure { what: name(s, id), why: display::sentence(why) });
}

/// A Move that failed, as any failed command is told: "Couldn't move the torrent", the torrent and
/// why under it.
fn told(s: &mut Phantom, failed: AddFailure) {
    let title = display::commands_failed(Action::Move, 1, 1);
    let list = move |_: &Phantom, ui: &Ui| ui.failure_list(&[crate::ui::Failure { what: &failed.what, why: &failed.why }]);
    s.bar.tell(Notice { more: Some(Rc::new(list)), ..Notice::error(title) });
}
