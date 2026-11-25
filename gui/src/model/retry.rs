//! The buttons of a failure's notice that do again what failed (#367): Try again, and the Move to…
//! and Check that follow a failed Move or Open. Each runs the ordinary command, so its refusals and
//! dialogs are the list's own. A button names the torrents it is for by id, and acts on those still
//! listed that the service says the command does something to now: a torrent removed and added
//! again has another id and is not one of them, and one whose command has since gone through (or
//! can't run: its destination went) is left out. With none left the button isn't shown. Try again
//! takes its notice away when pressed, so a second press can't run it again.

use proto::{Action, TorrentCommand, TorrentId};

use super::moving;
use crate::model::{Phantom, remove_ids};
use crate::notice::NoticeAction;
use crate::service::AddRequest;

/// The torrents of `ids` still listed that the service says `action` does something to now.
fn applicable(s: &Phantom, ids: &[TorrentId], action: Action) -> Vec<TorrentId> {
    s.torrents.iter().filter(|t| ids.contains(&t.id) && t.actions.does(action)).map(|t| t.id).collect()
}

/// Try again for a failed Pause, Resume or Delete of `ids`.
pub fn command(ids: Vec<TorrentId>, action: Action) -> NoticeAction {
    let (listed, run) = (ids.clone(), ids);
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

/// Try again for the adds that failed: `requests` go to the service as they went before.
pub fn add(requests: Vec<AddRequest>) -> NoticeAction {
    NoticeAction::new(client::display::try_again(), move |s: &mut Phantom| crate::model::add(s, requests.clone(), Vec::new())).once()
}

/// Try again for a Move of `ids` into `to`: asked and answered as any Move is, clash dialogs included.
/// Gone with its folder: a Move into a folder that isn't on disk any more isn't offered again.
pub fn move_again(ids: Vec<TorrentId>, to: proto::AbsolutePath) -> NoticeAction {
    let (listed, there) = (ids.clone(), std::path::PathBuf::from(to.display().to_string()));
    NoticeAction::new(client::display::try_again(), move |s: &mut Phantom| {
        let ids = applicable(s, &ids, Action::Move);
        moving::start(s, ids, to.clone());
    })
    .once()
    .shown_while(move |s| s.finder.exists(&there) && !applicable(s, &listed, Action::Move).is_empty())
}

/// Move to… for the torrents of `ids` that are left: the folder picker, then a Move.
pub fn move_to(ids: Vec<TorrentId>) -> NoticeAction {
    let listed = ids.clone();
    NoticeAction::with_tx(client::display::move_to(), move |s: &mut Phantom, tx: &mut rune::Tx| {
        let ids = applicable(s, &ids, Action::Move);
        moving::pick_folder(s, ids, tx);
    })
    .once()
    .shown_while(move |s| !applicable(s, &listed, Action::Move).is_empty())
}

/// Check for torrent `id`, whose file wasn't on disk: it then shows Missing files. Offered while
/// the service lists Check for it.
pub fn check(id: TorrentId) -> NoticeAction {
    NoticeAction::new(client::display::check_files(), move |s: &mut Phantom| {
        let ids = applicable(s, &[id], Action::Check);
        crate::model::command(&mut *s.service, &ids, TorrentCommand::Check);
    })
    .shown_while(move |s| !applicable(s, &[id], Action::Check).is_empty())
}

/// Rename… for torrent `id`, after a rename that couldn't be done: selects it and opens its name
/// field, as the Rename command does. Offered while the service lists Rename for it.
pub fn rename(id: TorrentId) -> NoticeAction {
    NoticeAction::with_tx(client::display::rename(), move |s: &mut Phantom, tx: &mut rune::Tx| {
        if applicable(s, &[id], Action::Rename).is_empty() {
            return;
        }
        s.table.clear_selection();
        s.table.select(&id.to_string());
        s.table.reveal(id.to_string(), tx);
        crate::model::renaming::open(s, id);
        s.bar.close();
    })
    .once()
    .shown_while(move |s| !applicable(s, &[id], Action::Rename).is_empty())
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
