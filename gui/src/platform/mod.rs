//! macOS integration: what the system hands Phantom from outside its window, the magnet links
//! clicked anywhere and the .torrent files opened from Finder (the bundle declares both), and
//! what Phantom hands it: Reveal in Finder.

#[cfg(target_os = "macos")]
pub mod open_documents;

use std::path::{Path, PathBuf};
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

/// [`open_documents`]' hook: macOS hands us files; add the torrents and bring the window back.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_files(s: &mut Phantom, paths: &[PathBuf], _: &mut Tx) {
    let opened = open_torrent_files(&mut *s.service, paths);
    if opened.added > 0 {
        if let Some(window) = &s.window {
            window.open();
        }
    }
    if let Some(notice) = opened.refused {
        s.notice = Some(notice);
    }
}

pub struct Opened {
    pub added: usize,
    /// What's wrong with the first refused file, and how many more were refused.
    pub refused: Option<Notice>,
}

/// The paths come from outside the app, so only .torrent files on disk go through; the others
/// are refused, and the rest still added.
pub fn open_torrent_files(service: &mut dyn Service, paths: &[PathBuf]) -> Opened {
    let mut added = 0;
    let mut refused = Vec::new();
    for path in paths {
        match torrent_file(path) {
            Ok(()) => {
                service.add(AddRequest::File(path.clone()));
                added += 1;
            }
            Err(why) => refused.push(why),
        }
    }
    let refused = refused.first().map(|why| match refused.len() {
        1 => Notice::error(why),
        n => Notice::error(format!("{why} (and {} more)", n - 1)),
    });
    Opened { added, refused }
}

/// Why `path` isn't a .torrent file Phantom can add.
fn torrent_file(path: &Path) -> Result<(), String> {
    let is_torrent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent"));
    match std::fs::metadata(path) {
        Err(_) => Err(format!("{} isn't on disk", path.display())),
        Ok(meta) if !meta.is_file() || !is_torrent => {
            Err(format!("Phantom only opens .torrent files, not {}", path.display()))
        }
        Ok(_) => Ok(()),
    }
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
