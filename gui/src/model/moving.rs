//! Move to… (#24): the row menu's item asks for a folder (the system's picker, starting where the
//! torrent is) and sends the Move. The service answers once it's moved, or back where it was: a
//! failure is told as an error notice, but one a Remove or a Delete cancelled, which the person
//! asked for. Refused for a clash (files of the torrent's are in the folder already), the clash
//! dialog asks what to do with them: Use existing or Replace sends the Move again with the choice,
//! Cancel (or Escape) drops it, Reveal in Finder shows them and keeps the dialog open.

use std::rc::Rc;

use client::{Failure, display};
use proto::{AbsolutePath, Action, Refusal, TorrentCommand, TorrentId, TorrentRefusal};
use rune::{FileDialogOptions, Tx};

use super::{AddFailure, Phantom};
use crate::notice::Notice;
use crate::platform::Show;
use crate::ui::Ui;

/// A Move refused for a clash, while its dialog asks what to do: torrent `id`, called `name`, into
/// `to`, where `files` are already.
#[derive(Debug, Clone, PartialEq)]
pub struct Clash {
    pub id: TorrentId,
    pub name: String,
    pub to: AbsolutePath,
    pub files: Vec<AbsolutePath>,
}

/// Asks where torrent `id` goes, starting at the folder it's in now, and moves it there.
pub fn move_to(s: &mut Phantom, id: TorrentId, tx: &mut Tx) {
    let Some(t) = s.torrents.iter().find(|t| t.id == id && t.actions.does(Action::Move)) else { return };
    let directory = t.location.as_ref().and_then(|l| l.parent()).map(|p| p.to_path_buf());
    let options = FileDialogOptions { title: display::move_picker(t.display_name()), directory, ..Default::default() };
    tx.open_folder(options, move |s: &mut Phantom, chosen, _| match chosen {
        Ok(Some(folder)) => match client::absolute(&folder) {
            Ok(to) => send(s, id, to, None),
            Err(e) => tell(s, id, &format!("{e:#}")),
        },
        Ok(None) => {}
        Err(e) => tell(s, id, &format!("can't ask for a folder: {e}")),
    });
}

/// Sends the Move of torrent `id` into `to`, with what to do with a clash, if the person chose.
pub fn send(s: &mut Phantom, id: TorrentId, to: AbsolutePath, clash: Option<proto::Clash>) {
    let command = TorrentCommand::Move { to: to.clone(), clash };
    s.service.command(
        &[id],
        command,
        Box::new(move |s: &mut Phantom, failed: Vec<(TorrentId, Failure)>, _: &mut Tx| {
            let Some((_, failure)) = failed.into_iter().next() else { return };
            let name = name(s, id);
            match failure {
                Failure::Refused(Refusal::Torrent { why: TorrentRefusal::Clash { files }, .. }) => {
                    crate::overlay::opening(s, crate::overlay::Overlay::Clash);
                    s.clash = Some(Clash { id, name, to, files });
                }
                // What the person asked for meanwhile: a Remove or a Delete, or an Undo.
                Failure::Refused(Refusal::Torrent { why: TorrentRefusal::NotMoved { why: proto::NotMoved::Cancelled, .. }, .. })
                | Failure::Refused(Refusal::Undone) => {}
                failure => {
                    let (what, why) = display::command_failure(&name, &failure);
                    told(s, AddFailure { what, why });
                }
            }
        }),
    );
}

/// The clash dialog's choice: the Move sent again with it, or (`None`, Cancel) dropped.
pub fn choose(s: &mut Phantom, choice: Option<proto::Clash>) {
    let Some(clash) = s.clash.take() else { return };
    if let Some(choice) = choice {
        send(s, clash.id, clash.to, Some(choice));
    }
}

/// Shows the clashing files in Finder; the dialog stays open, for the choice.
pub fn reveal(s: &mut Phantom) {
    let Some(path) = s.clash.as_ref().and_then(|c| c.files.first()).cloned() else { return };
    if let Err(e) = s.finder.show(&path, Show::Reveal) {
        s.bar.tell(Notice::error(e));
    }
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
