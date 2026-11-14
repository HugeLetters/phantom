//! Free space (#127): each disk whose torrents need more than it has free is a live notice in the
//! status bar ([`crate::bar`]), there while it's short, which opens to its torrents, what each
//! needs and Pause.

use std::rc::Rc;

use client::display;
use proto::{TorrentCommand, TorrentId, Volume};
use rune::{col, row};

use super::{Phantom, command};
use crate::notice::Notice;
use crate::ui::{Tone, Ui};

/// The disks short in `volumes`, each as its live notice, keyed by its mount.
pub fn warnings(volumes: &[Volume]) -> Vec<(String, Notice)> {
    volumes.iter().filter(|v| v.short()).map(|v| (v.mount.display().to_string(), notice(v))).collect()
}

/// A disk's warning: the short line, and its torrents with what each needs and Pause.
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
        col().key("space-list").w_full().gap(6.0).children(rows).child(ui.caption(display::disk_short_why()).wrap())
    };
    Notice { more: Some(Rc::new(list)), ..Notice::warning(short) }
}

fn pause(s: &mut Phantom, id: TorrentId) {
    command(&mut *s.service, &[id], TorrentCommand::Pause);
}
