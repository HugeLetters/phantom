//! Free space (#127): a warning for each disk whose torrents need more than it has free, in the
//! status bar, which opens to the list of them with what each needs and Pause; and a torrent's own
//! warning in its row when it alone needs more. Warnings only: nothing is blocked.

use std::rc::Rc;

use client::display;
use proto::{TorrentCommand, TorrentId, Volume};
use rune::{El, col, row};

use crate::model::{Phantom, command, space};
use crate::notice::Notice;
use crate::ui::{Tone, Ui};

/// Each short disk's warning, its list above it while open.
pub fn warnings(s: &Phantom, ui: &Ui, layer: &mut Vec<El>) -> Vec<El> {
    let short = s.volumes.iter().filter(|v| v.short());
    short
        .enumerate()
        .map(|(i, volume)| {
            let notice = notice(volume);
            let mount = volume.mount.clone();
            let open = s.space == space::State::Open(mount.clone());
            let trigger = ui
                .notice(format!("{} ›", notice.short), super::status_bar::tone_of(notice.severity))
                .key(format!("space/{i}"))
                .min_w(0.0)
                .help("Show them")
                .on_press(move |s: &mut Phantom| s.disk_warning(space::Event::Command(space::Command::Open(mount.clone()))));
            let list = notice.more.as_ref().map(|view| view(s, ui)).unwrap_or_else(rune::el);
            let close = |s: &mut Phantom| s.disk_warning(space::Event::Command(space::Command::Close));
            ui.panel_above(&format!("space/{i}"), trigger, list, open, close, layer).shrink().min_w(0.0)
        })
        .collect()
}

/// A disk's warning, as a [`Notice`]: the short line, and its torrents with Pause.
fn notice(volume: &Volume) -> Notice {
    let short = display::disk_short(volume);
    let volume = volume.clone();
    let list = move |s: &Phantom, ui: &Ui| {
        let rows = volume.needs.iter().map(|needs| {
            let id = needs.id;
            let name =
                s.torrents.iter().find(|t| t.id == id).map_or_else(|| display::unlisted_torrent(id), |t| t.display_name().to_owned());
            row()
                .key(format!("space-torrent/{id}"))
                .w_full()
                .gap(10.0)
                .center_y()
                .child(ui.notice(name, Tone::Normal).grow(1.0).min_w(0.0))
                .child(ui.caption(display::bytes(needs.bytes)))
                .child(
                    ui.button("space-pause", "Pause")
                        .on_press(move |s: &mut Phantom| pause(s, id))
                        .build()
                        .key(format!("space-pause/{id}")),
                )
        });
        col()
            .key("space-list")
            .w(380.0)
            .gap(6.0)
            .child(ui.caption(display::disk_short(&volume)))
            .children(rows)
            .child(ui.caption(display::disk_short_why()).wrap())
    };
    Notice { more: Some(Rc::new(list)), ..Notice::warning(short) }
}

fn pause(s: &mut Phantom, id: TorrentId) {
    command(&mut *s.service, &[id], TorrentCommand::Pause);
}

/// Torrent `id`'s own warning, when it alone needs more than its disk has free: the line for its
/// row, and why.
pub fn too_big(s: &Phantom, id: TorrentId) -> Option<(String, String)> {
    s.volumes.iter().find_map(|v| {
        let (needs, free) = (v.too_big(id)?, v.free_bytes?);
        Some((display::too_big(free), display::too_big_why(needs, free, &v.mount)))
    })
}
