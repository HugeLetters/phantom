//! macOS integration: what the system hands Phantom from outside its window, the magnet links
//! clicked anywhere and the .torrent files opened from Finder (the bundle declares both), and
//! what Phantom hands it: Open and Reveal in Finder and the App icon.

pub mod app_icon;
#[cfg(target_os = "macos")]
pub mod open_documents;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use proto::{Action, TorrentId};
use rune::Tx;

use client::OpenRefusal;

use crate::model::{AddFailure, Phantom, add, reveal_target};
use crate::notice::Notice;
use crate::service::AddRequest;

/// Rune's `on_open_url`: macOS hands us a link; bring the window back and add it (asking where,
/// without a default destination: the picker shows with the window).
pub fn open_url(s: &mut Phantom, url: &str, _: &mut Tx) {
    match open_link(url) {
        Ok(request) => {
            if let Some(window) = &s.window {
                window.open();
            }
            add(s, vec![request], Vec::new());
        }
        Err(refused) => add(s, Vec::new(), vec![refused]),
    }
}

/// The URL comes from outside the app, so only magnets go through.
pub fn open_link(url: &str) -> Result<AddRequest, AddFailure> {
    if !url.starts_with("magnet:") {
        return Err(AddFailure::refused(&AddRequest::Url(url.to_owned()), OpenRefusal::NotAMagnetLink));
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
    add(s, opened.adds, opened.refused);
}

pub struct Opened {
    pub adds: Vec<AddRequest>,
    /// The files that aren't .torrent files on disk, and why.
    pub refused: Vec<AddFailure>,
}

/// The paths come from outside the app, so only .torrent files on disk go through; the others
/// are refused, and the rest still added.
pub fn open_torrent_files(paths: &[PathBuf]) -> Opened {
    let mut adds = Vec::new();
    let mut refused = Vec::new();
    for path in paths {
        match torrent_file(path) {
            Ok(()) => adds.push(AddRequest::File(path.clone())),
            Err(why) => refused.push(AddFailure::refused(&AddRequest::File(path.clone()), why)),
        }
    }
    Opened { adds, refused }
}

/// Why `path` isn't a .torrent file Phantom can add.
fn torrent_file(path: &Path) -> Result<(), OpenRefusal> {
    let is_torrent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent"));
    match std::fs::metadata(path) {
        Err(_) => Err(OpenRefusal::NotOnDisk),
        Ok(meta) if !meta.is_file() || !is_torrent => Err(OpenRefusal::NotATorrentFile),
        Ok(_) => Ok(()),
    }
}

/// What a torrent's location is shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// A file in its default app, a folder in Finder, showing its contents.
    Open,
    /// In a Finder window, selected in its parent folder.
    Reveal,
}

impl Show {
    /// The action the service lists when this can be done.
    pub fn action(self) -> Action {
        match self {
            Show::Open => Action::Open,
            Show::Reveal => Action::Reveal,
        }
    }
}

/// How Phantom hands a path to the system: [`SystemFinder`], or [`Shown`] where nothing may open.
pub trait Finder {
    /// `Err` says, as a sentence, why it couldn't.
    fn show(&mut self, path: &Path, how: Show) -> Result<(), String>;
}

/// Runs `open`.
pub struct SystemFinder;

impl Finder for SystemFinder {
    fn show(&mut self, path: &Path, how: Show) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("{} isn't on disk", path.display()));
        }
        let mut open = Command::new("open");
        if how == Show::Reveal {
            open.arg("-R");
        }
        open.arg(path).spawn().map(drop).map_err(|e| format!("Can't open Finder: {e}"))
    }
}

/// Opens nothing and remembers what it was asked, in order, for tests and headless runs. Clones
/// share the record.
#[derive(Clone, Default)]
pub struct Shown(Arc<Mutex<Vec<(PathBuf, Show)>>>);

impl Shown {
    #[cfg(test)]
    pub fn all(&self) -> Vec<(PathBuf, Show)> {
        self.0.lock().unwrap().clone()
    }
}

impl Finder for Shown {
    fn show(&mut self, path: &Path, how: Show) -> Result<(), String> {
        self.0.lock().unwrap().push((path.to_owned(), how));
        Ok(())
    }
}

/// Opens or reveals torrent `id`'s location, if the service lists that action for it; a failure
/// shows in the status bar.
pub fn show_torrent(s: &mut Phantom, id: TorrentId, how: Show) {
    let Some(t) = s.torrents.iter().find(|t| t.id == id).filter(|t| t.actions.contains(&how.action())) else { return };
    let Some(path) = reveal_target(t) else { return };
    if let Err(e) = s.finder.show(path, how) {
        s.errors.report(Notice::error(e));
    }
}
