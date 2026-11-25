//! The buttons of a failure's notice that do again what failed (#367): Try again, and the Move to…
//! and Check that follow a failed Move or Open. Each runs the ordinary command, so its refusals and
//! dialogs are the list's own. A button knows the torrents it is for by id and info hash ([`Torrent`]), and acts on those still
//! listed that the service says the command does something to now: a torrent removed and added
//! again has another id, and a list begun anew (the old one unreadable) gives ids out again: neither
//! is one of them, and one whose command has since gone through (or
//! can't run: its destination went) is left out. With none left the button isn't shown. Try again
//! takes its notice away when pressed, so a second press can't run it again.

use proto::{Action, TorrentCommand, TorrentId};

use super::moving;
use crate::model::Asked;
use crate::model::{Phantom, remove_ids};
use crate::notice::NoticeAction;

/// A torrent a button is for. Not its id alone: the service counts ids anew with a new list, so an id
/// may come to mean another torrent while the notice stays.
#[derive(Clone)]
struct Torrent {
    id: TorrentId,
    info_hash: String,
}

/// The torrents of `ids`, as listed now, when their command's failure is told.
fn torrents(s: &Phantom, ids: &[TorrentId]) -> Vec<Torrent> {
    s.torrents.iter().filter(|t| ids.contains(&t.id)).map(|t| Torrent { id: t.id, info_hash: t.info_hash.clone() }).collect()
}

/// Those of `of` still listed that the service says `action` does something to now.
fn applicable(s: &Phantom, of: &[Torrent], action: Action) -> Vec<TorrentId> {
    let is_one = |t: &proto::TorrentInfo| of.iter().any(|o| o.id == t.id && o.info_hash == t.info_hash);
    s.torrents.iter().filter(|t| is_one(t) && t.actions.does(action)).map(|t| t.id).collect()
}

/// Try again for a failed Pause, Resume or Delete of `ids`.
pub fn command(s: &Phantom, ids: &[TorrentId], action: Action) -> NoticeAction {
    let run = torrents(s, ids);
    let listed = run.clone();
    NoticeAction::new(client::display::try_again(), move |s: &mut Phantom| {
        let ids = applicable(s, &run, action);
        match action {
            Action::Pause => crate::model::command(&mut *s.service, &ids, TorrentCommand::Pause),
            Action::Resume => crate::model::command(&mut *s.service, &ids, TorrentCommand::Resume),
            Action::Delete => remove_ids(s, ids, true),
            _ => {}
        }
    })
    .once()
    .shown_while(move |s| !applicable(s, &listed, action).is_empty())
}

/// Try again for the adds that failed: each goes to the service as it went before, into the folder
/// chosen for it, if one was.
pub fn add(adds: Vec<Asked>) -> NoticeAction {
    NoticeAction::new(client::display::try_again(), move |s: &mut Phantom| crate::model::add_again(s, adds.clone())).once()
}

/// Try again and Move to… for a Move of `ids` into `to`.
pub fn moving(s: &Phantom, ids: &[TorrentId], to: proto::AbsolutePath) -> Vec<NoticeAction> {
    vec![move_again(s, ids, to), move_to(s, ids)]
}

/// Try again for a Move of `ids` into `to`: asked and answered as any Move is, clash dialogs included.
/// Gone with its folder: a Move into a folder that isn't on disk any more isn't offered again.
fn move_again(s: &Phantom, ids: &[TorrentId], to: proto::AbsolutePath) -> NoticeAction {
    let of = torrents(s, ids);
    let (listed, there) = (of.clone(), to.to_path_buf());
    NoticeAction::new(client::display::try_again(), move |s: &mut Phantom| {
        let ids = applicable(s, &of, Action::Move);
        moving::start(s, ids, to.clone());
    })
    .once()
    .shown_while(move |s| s.finder.seen(&there) && !applicable(s, &listed, Action::Move).is_empty())
}

/// Move to… for the torrents of `ids` that are left: the folder picker, then a Move.
pub fn move_to(s: &Phantom, ids: &[TorrentId]) -> NoticeAction {
    let of = torrents(s, ids);
    let listed = of.clone();
    NoticeAction::with_tx(client::display::move_to(), move |s: &mut Phantom, tx: &mut rune::Tx| {
        let ids = applicable(s, &of, Action::Move);
        moving::pick_folder(s, ids, tx);
    })
    .once()
    .shown_while(move |s| !applicable(s, &listed, Action::Move).is_empty())
}

/// Check for torrent `id`, whose file wasn't on disk: it then shows Missing files. Offered while
/// the service lists Check for it.
pub fn check(s: &Phantom, id: TorrentId) -> NoticeAction {
    let of = torrents(s, &[id]);
    let listed = of.clone();
    NoticeAction::new(client::display::check_files(), move |s: &mut Phantom| {
        let ids = applicable(s, &of, Action::Check);
        crate::model::command(&mut *s.service, &ids, TorrentCommand::Check);
    })
    .shown_while(move |s| !applicable(s, &listed, Action::Check).is_empty())
}

/// Rename… for torrent `id`, after a rename that couldn't be done: selects it and opens its name
/// field, as the Rename command does. Offered while the service lists Rename for it.
pub fn rename(s: &Phantom, id: TorrentId) -> NoticeAction {
    let of = torrents(s, &[id]);
    let listed = of.clone();
    NoticeAction::with_tx(client::display::rename(), move |s: &mut Phantom, tx: &mut rune::Tx| {
        if applicable(s, &of, Action::Rename).is_empty() {
            return;
        }
        s.table.clear_selection();
        s.table.select(&id.to_string());
        s.table.reveal(id.to_string(), tx);
        crate::model::renaming::open(s, id);
        s.bar.close();
    })
    .once()
    .shown_while(move |s| !applicable(s, &listed, Action::Rename).is_empty())
}

/// Try again for a restart that couldn't start: the same restart.
pub fn restart() -> NoticeAction {
    NoticeAction::with_tx(client::display::try_again(), |s: &mut Phantom, tx: &mut rune::Tx| crate::platform::restart(s, tx)).once()
}

/// Open settings on the tab of `setting` (its key in the settings file), its control focused.
pub fn open_settings(setting: &'static str) -> NoticeAction {
    NoticeAction::new(client::display::open_settings(), move |s: &mut Phantom| {
        s.settings.post(move |w, _| crate::model::settings::open_reset(w, &[setting.to_owned()]));
        s.bar.close();
    })
}
