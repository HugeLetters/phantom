//! macOS integration: what the system hands Phantom from outside its window, the magnet links
//! clicked anywhere and the .torrent files opened from Finder (the bundle declares both), and
//! what Phantom hands it: Reveal in Finder and the App icon.

pub mod app_icon;
#[cfg(target_os = "macos")]
pub mod open_documents;

use std::path::{Path, PathBuf};
use std::process::Command;

use rune::Tx;

use crate::model::Notice;
use crate::model::{Phantom, add, reveal_target, shown_torrent};
use crate::service::AddRequest;

/// Rune's `on_open_url`: macOS hands us a link; bring the window back and add it (asking where,
/// without a default destination: the picker shows with the window).
pub fn open_url(s: &mut Phantom, url: &str, _: &mut Tx) {
    match open_link(url) {
        Ok(request) => {
            if let Some(window) = &s.window {
                window.open();
            }
            add(&mut *s.service, vec![request]);
        }
        Err(notice) => s.notice = Some(notice),
    }
}

/// The URL comes from outside the app, so only magnets go through.
pub fn open_link(url: &str) -> Result<AddRequest, Notice> {
    if !url.starts_with("magnet:") {
        return Err(Notice::error(format!("Phantom only opens magnet links, not {url}")));
    }
    Ok(AddRequest::Url(url.to_owned()))
}

/// [`open_documents`]' hook: macOS hands us files; bring the window back and add the torrents.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_files(s: &mut Phantom, paths: &[PathBuf], _: &mut Tx) {
    let opened = open_torrent_files(paths);
    if !opened.adds.is_empty() {
        if let Some(window) = &s.window {
            window.open();
        }
    }
    if let Some(notice) = opened.refused {
        s.notice = Some(notice);
    }
    add(&mut *s.service, opened.adds);
}

pub struct Opened {
    pub adds: Vec<AddRequest>,
    /// What's wrong with the first refused file, and how many more were refused.
    pub refused: Option<Notice>,
}

/// The paths come from outside the app, so only .torrent files on disk go through; the others
/// are refused, and the rest still added.
pub fn open_torrent_files(paths: &[PathBuf]) -> Opened {
    let mut adds = Vec::new();
    let mut refused = Vec::new();
    for path in paths {
        match torrent_file(path) {
            Ok(()) => adds.push(AddRequest::File(path.clone())),
            Err(why) => refused.push(why),
        }
    }
    let refused = refused.first().map(|why| match refused.len() {
        1 => Notice::error(why),
        n => Notice::error(format!("{why} (and {} more)", n - 1)),
    });
    Opened { adds, refused }
}

/// Why `path` isn't a .torrent file Phantom can add.
fn torrent_file(path: &Path) -> Result<(), String> {
    let is_torrent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent"));
    match std::fs::metadata(path) {
        Err(_) => Err(format!("{} isn't on disk", path.display())),
        Ok(meta) if !meta.is_file() || !is_torrent => Err(format!("Phantom only opens .torrent files, not {}", path.display())),
        Ok(_) => Ok(()),
    }
}

/// Reveal in Finder: selects the details pane's torrent in a Finder window. What to reveal is
/// [`reveal_target`]'s call; this only runs `open -R`.
pub fn reveal(s: &mut Phantom) {
    let Some(path) = shown_torrent(&s.table, &s.torrents, &s.pane).and_then(|t| reveal_target(t, &s.pane)) else { return };
    if !path.exists() {
        s.notice = Some(Notice::error(format!("{} isn't on disk", path.display())));
        return;
    }
    if let Err(e) = Command::new("open").arg("-R").arg(path).spawn() {
        s.notice = Some(Notice::error(format!("can't open Finder: {e}")));
    }
}
