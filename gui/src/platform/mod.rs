//! macOS integration: what the system hands Phantom from outside its window, the magnet links
//! clicked anywhere (the bundle declares the scheme), and what Phantom hands it: Reveal in Finder.

use std::process::Command;

use rune::Tx;

use crate::model::{Phantom, details_torrent, reveal_target};
use crate::service::{AddRequest, Notice, Service};

/// Rune's `on_open_url`: macOS hands us a link; add it and bring the window back.
pub fn open_url(s: &mut Phantom, url: &str, _: &mut Tx) {
    match open_link(&mut *s.service, url) {
        Ok(()) => {
            if let Some(window) = &s.window {
                window.open();
            }
        }
        Err(notice) => s.notice = Some(notice),
    }
}

/// The URL comes from outside the app, so only magnets go through.
pub fn open_link(service: &mut dyn Service, url: &str) -> Result<(), Notice> {
    if !url.starts_with("magnet:") {
        return Err(Notice::error(format!("Phantom only opens magnet links, not {url}")));
    }
    service.add(AddRequest::Url(url.to_owned()));
    Ok(())
}

/// Reveal in Finder: selects the details pane's torrent in a Finder window. What to reveal is
/// [`reveal_target`]'s call; this only runs `open -R`.
pub fn reveal(s: &mut Phantom) {
    let Some(path) = details_torrent(&s.table, &s.torrents).and_then(|t| reveal_target(t, &s.pane)) else { return };
    if !path.exists() {
        s.notice = Some(Notice::error(format!("{} isn't on disk", path.display())));
        return;
    }
    if let Err(e) = Command::new("open").arg("-R").arg(path).spawn() {
        s.notice = Some(Notice::error(format!("can't open Finder: {e}")));
    }
}
