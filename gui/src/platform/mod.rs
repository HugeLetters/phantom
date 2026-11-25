//! macOS integration: what the system hands Phantom from outside its window, the magnet links
//! clicked anywhere and the .torrent files opened from Finder (the bundle declares both), and
//! what Phantom hands it: Open and Reveal in Finder, the App icon, the menu bar's own items, and
//! the Settings window's frame, kept from one run to the next ([`remember_frame`]).

pub mod app_icon;
pub mod menu_bar;
#[cfg(target_os = "macos")]
pub mod open_documents;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use proto::{Action, AddSource, TorrentId};
use rune::Tx;

use client::{OpenRefusal, display};

use crate::model::{AddFailure, Phantom, add, reveal_target};
use crate::notice::Notice;
use crate::service::AddRequest;

/// Rune's `on_open_url`: macOS hands us a link; bring the window back and add it (asking where,
/// without a default destination: the picker shows with the window).
pub fn open_url(s: &mut Phantom, url: &str, _: &mut Tx) {
    match open_link(url) {
        Ok(request) => {
            s.window.raise();
            add(s, vec![request], Vec::new());
        }
        Err(refused) => add(s, Vec::new(), vec![refused]),
    }
}

/// The window keeps its size and place under `name` from one run to the next, as AppKit keeps a
/// window's frame (`NSWindow.frameAutosaveName`, in the app's defaults): the one kept, if any, is
/// taken now, unless it's on no display connected now ([`placed`]). Rune calls this each time it
/// creates the window.
#[cfg(target_os = "macos")]
pub fn remember_frame(window: &rune::WindowHandle, name: &str) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSString};
    use rune::raw_window_handle::RawWindowHandle;
    let frame = |r: NSRect| [r.origin.x, r.origin.y, r.size.width, r.size.height];
    let _ = window.with_raw_handle(|handle| {
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
        // SAFETY: the handle is the live window's content view, borrowed on the main thread for
        // this call only.
        let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
        let Some(window) = view.window() else { return };
        let name = NSString::from_str(name);
        let restored = window.setFrameUsingName(&name);
        window.setFrameAutosaveName(&name);
        let Some(mtm) = MainThreadMarker::new().filter(|_| restored) else { return };
        let displays: Vec<Frame> = NSScreen::screens(mtm).iter().map(|s| frame(s.frame())).collect();
        let Some(main) = NSScreen::mainScreen(mtm).map(|s| frame(s.visibleFrame())) else { return };
        let (now, to) = (frame(window.frame()), placed(frame(window.frame()), &displays, main));
        if to != now {
            window.setFrameOrigin(NSPoint::new(to[0], to[1]));
        }
    });
}

/// A frame as AppKit has it, in points: x and y of its bottom-left corner, width, height.
pub type Frame = [f64; 4];

/// Where a window restored at `frame` goes: there while some of it is on one of `displays`, else
/// centred in `main` (the main display's area that the menu bar and the Dock leave), as when the
/// display it was left on is unplugged.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn placed(frame: Frame, displays: &[Frame], main: Frame) -> Frame {
    let [x, y, w, h] = frame;
    let overlaps = |&[dx, dy, dw, dh]: &Frame| x < dx + dw && dx < x + w && y < dy + dh && dy < y + h;
    if displays.iter().any(overlaps) {
        return frame;
    }
    let [mx, my, mw, mh] = main;
    [(mx + (mw - w) / 2.0).round(), (my + (mh - h) / 2.0).round(), w, h]
}

/// The URL comes from outside the app, so only magnets go through.
pub fn open_link(url: &str) -> Result<AddRequest, AddFailure> {
    match AddRequest::link(url) {
        Ok(request @ AddRequest::Link { source: AddSource::Magnet(_), .. }) if url.starts_with("magnet:") => Ok(request),
        Ok(_) => Err(AddFailure::refused_link(url, OpenRefusal::NotAMagnetLink)),
        // A magnet link, but not one Add takes.
        Err(problem) if url.starts_with("magnet:") => Err(AddFailure::not_a_link(url, problem)),
        Err(_) => Err(AddFailure::refused_link(url, OpenRefusal::NotAMagnetLink)),
    }
}

/// [`open_documents`]' hook: macOS hands us files; bring the window back and add the torrents.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn open_files(s: &mut Phantom, paths: &[PathBuf], _: &mut Tx) {
    let opened = open_torrent_files(paths);
    if !opened.adds.is_empty() {
        s.window.raise();
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

/// Why the system wasn't handed a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowError {
    /// Nothing is at the path.
    NotOnDisk,
    /// `open` couldn't be started: the system's reason.
    NoFinder(String),
}

/// How Phantom hands a path to the system: [`SystemFinder`], or [`Shown`] where nothing may open.
pub trait Finder {
    /// `Err` says, as a sentence, why it couldn't.
    fn show(&mut self, path: &Path, how: Show) -> Result<(), ShowError>;
    /// Whether something is at `path` right now (a link counts, and isn't followed): the one look
    /// at the disk that Open and Reveal's offer rests on, taken when a menu holding them opens or
    /// their shortcut is pressed (#307).
    fn exists(&self, path: &Path) -> bool;
}

/// Runs `open`.
pub struct SystemFinder;

impl Finder for SystemFinder {
    fn show(&mut self, path: &Path, how: Show) -> Result<(), ShowError> {
        if !path.exists() {
            return Err(ShowError::NotOnDisk);
        }
        let mut open = Command::new("open");
        if how == Show::Reveal {
            open.arg("-R");
        }
        open.arg(path).spawn().map(drop).map_err(|e| ShowError::NoFinder(e.to_string()))
    }

    fn exists(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }
}

/// Opens nothing and remembers what it was asked, in order, for tests and headless runs. Clones
/// share the record.
#[derive(Clone, Default)]
pub struct Shown(Arc<Mutex<Vec<(PathBuf, Show)>>>, Arc<Mutex<Vec<PathBuf>>>);

impl Shown {
    /// The disk it stands for has nothing at `path` from now on; before, everything is there.
    #[cfg(test)]
    pub fn lose(&self, path: &Path) {
        self.1.lock().unwrap().push(path.to_owned());
    }

    /// The disk has `path` again.
    #[cfg(test)]
    pub fn restore(&self, path: &Path) {
        self.1.lock().unwrap().retain(|p| p != path);
    }

    #[cfg(test)]
    pub fn all(&self) -> Vec<(PathBuf, Show)> {
        self.0.lock().unwrap().clone()
    }
}

impl Finder for Shown {
    fn show(&mut self, path: &Path, how: Show) -> Result<(), ShowError> {
        self.0.lock().unwrap().push((path.to_owned(), how));
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        !self.1.lock().unwrap().iter().any(|p| p == path)
    }
}

/// Opens or reveals the one selected file of the torrent the pane shows, if the file menu offers it
/// (the service lists the action, and the file is on disk); a failure shows in the status bar.
pub fn show_file(s: &mut Phantom, how: Show) {
    let item = match how {
        Show::Open => crate::commands::OnFiles::Open,
        Show::Reveal => crate::commands::OnFiles::Reveal,
    };
    if !crate::commands::on_files(s).contains(&item) {
        return;
    }
    let Some(path) = crate::model::file_target(&s.table, &s.torrents, &s.pane) else { return };
    let of = s.pane.details.as_ref().map(|d| d.id);
    show_of(s, &path, how, of);
}

/// Restarts the app, as the status bar's "Phantom was updated" offers: a relaunch is started that
/// waits for this process to exit, then the app quits. A second press before it has quit starts
/// nothing more. If the relaunch can't start the app stays and says why.
pub fn restart(s: &mut Phantom, tx: &mut Tx) {
    if s.relaunching {
        return;
    }
    match s.relauncher.relaunch() {
        Ok(()) => {
            s.relaunching = true;
            tx.quit();
        }
        Err(why) => s.bar.tell(
            Notice::error(display::cant_restart()).with_text(display::sentence(&why)).with_actions(vec![crate::model::retry::restart()]),
        ),
    }
}

/// How Phantom starts itself again: [`SystemRelauncher`], or [`Relaunched`] where nothing may start.
pub trait Relauncher {
    /// Starts a relaunch that waits for this process to exit; `Err` says why it couldn't.
    fn relaunch(&mut self) -> Result<(), String>;
}

/// The folders a run is kept to (see AGENTS.md), with their values, those `get` finds set.
pub fn forwarded_env(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    ["PHANTOM_DATA_DIR", "PHANTOM_DOWNLOAD_DIR"].into_iter().filter_map(|name| Some((name.to_owned(), get(name)?))).collect()
}

/// The command that waits for process `pid` to exit, then opens the app: the bundle (`open -n`, a
/// new instance, since the old one may still be quitting) when `exe` is inside one, else `exe`
/// itself with `args`. `open` doesn't pass this process's environment on, so `env` goes to it as
/// `--env`; a bare binary keeps it. Everything reaches the shell as an argument, never in its script.
pub fn relaunch_command(pid: u32, exe: &Path, args: &[String], env: &[(String, String)]) -> Command {
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"));
    let mut sh = Command::new("/bin/sh");
    let launch = match bundle {
        Some(_) => "exec open -n \"$@\"",
        None => "exec \"$@\"",
    };
    sh.arg("-c").arg(format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; {launch}")).arg("phantom-relaunch");
    match bundle {
        Some(app) => {
            for (name, value) in env {
                sh.arg("--env").arg(format!("{name}={value}"));
            }
            sh.arg(app)
        }
        None => sh.arg(exe).args(args),
    };
    sh
}

/// Relaunches this app once it has exited.
pub struct SystemRelauncher;

impl Relauncher for SystemRelauncher {
    fn relaunch(&mut self) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let args: Vec<String> = std::env::args().skip(1).collect();
        relaunch_command(std::process::id(), &exe, &args, &forwarded_env(|n| std::env::var(n).ok()))
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

/// Starts nothing and counts what it was asked, for tests and headless runs. Clones share the count.
#[derive(Clone, Default)]
pub struct Relaunched(Arc<Mutex<u32>>);

impl Relaunched {
    #[cfg(test)]
    pub fn times(&self) -> u32 {
        *self.0.lock().unwrap()
    }
}

impl Relauncher for Relaunched {
    fn relaunch(&mut self) -> Result<(), String> {
        *self.0.lock().unwrap() += 1;
        Ok(())
    }
}

/// Hands `path` to the system as `how` says; a failure is an error notice: Phantom's few words
/// in the bar, the path and the system's reason in the list.
pub fn show(s: &mut Phantom, path: &Path, how: Show) {
    show_of(s, path, how, None);
}

/// [`show`] for a file of torrent `of`: a failure for want of the file offers to Check it.
pub fn show_of(s: &mut Phantom, path: &Path, how: Show, of: Option<TorrentId>) {
    let Err(e) = s.finder.show(path, how) else { return };
    cant_show(s, path, how, e, of);
}

/// Tells why `path` couldn't be shown: Phantom's few words in the bar, the path and the reason in
/// the list.
fn cant_show(s: &mut Phantom, path: &Path, how: Show, e: ShowError, of: Option<TorrentId>) {
    let missing = matches!(e, ShowError::NotOnDisk);
    let (short, reason, folder) = match e {
        // The file's folder, if that is there: Show folder.
        ShowError::NotOnDisk => (display::cant_show(how.action()), display::not_on_disk().to_owned(), path.parent()),
        ShowError::NoFinder(reason) => (display::cant_start_finder(), reason, None),
    };
    let notice = Notice::error(short).with_listing(display::show_failed_listing(path, &reason));
    let check = of.filter(|_| missing).map(crate::model::retry::check);
    s.bar.tell(notice.with_actions(folder.map(crate::notice::NoticeAction::folder).into_iter().chain(check).collect()));
}

/// A command key's Open or Reveal for torrent `id` (⌘O, ⌘↓, ⇧⌘O): done only if its location is on
/// disk now, as for any item not offered nothing happens otherwise (#307).
pub fn press_torrent(s: &mut Phantom, id: TorrentId, how: Show) {
    if located(s, id, how).is_some_and(|p| s.finder.exists(&p)) {
        show_torrent(s, id, how);
    }
}

/// Return's or a double-click's Open for torrent `id`: a direct request, so a location that's gone
/// is said so, as a failed Open says it (#312).
pub fn open_torrent(s: &mut Phantom, id: TorrentId) {
    let Some(path) = located(s, id, Show::Open) else { return };
    if s.finder.exists(&path) {
        show_torrent(s, id, Show::Open);
    } else {
        cant_show(s, &path, Show::Open, ShowError::NotOnDisk, Some(id));
    }
}

/// Where torrent `id` is, if the service lists `how` for it.
fn located(s: &Phantom, id: TorrentId, how: Show) -> Option<PathBuf> {
    s.torrents.iter().find(|t| t.id == id).filter(|t| t.actions.does(how.action())).and_then(reveal_target).map(Path::to_owned)
}

/// Opens or reveals torrent `id`'s location, if the service lists that action for it; a failure
/// shows in the status bar.
pub fn show_torrent(s: &mut Phantom, id: TorrentId, how: Show) {
    let Some(t) = s.torrents.iter().find(|t| t.id == id).filter(|t| t.actions.does(how.action())) else { return };
    let Some(path) = reveal_target(t).map(Path::to_owned) else { return };
    show_of(s, &path, how, Some(id));
}

#[cfg(test)]
mod tests {
    use super::{placed, relaunch_command};
    use std::path::Path;

    /// A laptop's display, and an external one to its right; the main display's area under the
    /// menu bar, above the Dock.
    const LAPTOP: [f64; 4] = [0.0, 0.0, 1512.0, 982.0];
    const EXTERNAL: [f64; 4] = [1512.0, 0.0, 2560.0, 1440.0];
    const MAIN: [f64; 4] = [0.0, 80.0, 1512.0, 870.0];

    /// Settings left on a display that's still there stays where it was, also partly off it; left
    /// on one unplugged since, it opens centred on the main display (#257).
    #[test]
    fn a_window_left_on_a_display_that_is_gone_opens_centred_on_the_main_one() {
        let on_external = [2000.0, 400.0, 600.0, 460.0];
        assert_eq!(placed(on_external, &[LAPTOP, EXTERNAL], MAIN), on_external, "its display is there");
        let straddling = [1300.0, 400.0, 600.0, 460.0];
        assert_eq!(placed(straddling, &[LAPTOP], MAIN), straddling, "partly on the laptop's");
        assert_eq!(placed(on_external, &[LAPTOP], MAIN), [456.0, 285.0, 600.0, 460.0], "unplugged: centred, its size kept");
    }

    /// A bundled app is relaunched through `open -n` of the bundle once the process is gone; a bare
    /// binary (a development run) is run again with its arguments.
    #[test]
    fn a_restart_waits_for_the_exit_then_opens_the_bundle_or_runs_the_binary_again() {
        let args = |c: &std::process::Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let app = Path::new("/Applications/Phantom.app/Contents/MacOS/phantom-gui");
        let bundled = relaunch_command(42, app, &["x".into()], &[]);
        let a = args(&bundled);
        assert!(a[1].starts_with("while kill -0 42 ") && a[1].ends_with("exec open -n \"$@\""), "{a:?}");
        assert_eq!(a[3..], ["/Applications/Phantom.app"], "the bundle, not the binary, and no environment");
        // LaunchServices doesn't pass this process's environment on: the data and download folders go as `--env`,
        // each one argument, whatever the value holds.
        let env =
            [("PHANTOM_DATA_DIR".to_owned(), "/tmp/a b/'q\" $x".to_owned()), ("PHANTOM_DOWNLOAD_DIR".to_owned(), "/tmp/d".to_owned())];
        let a = args(&relaunch_command(42, app, &[], &env));
        assert_eq!(
            a[3..],
            ["--env", "PHANTOM_DATA_DIR=/tmp/a b/'q\" $x", "--env", "PHANTOM_DOWNLOAD_DIR=/tmp/d", "/Applications/Phantom.app"],
            "{a:?}"
        );
        assert!(!a[1].contains("$x") && !a[1].contains("/tmp"), "values reach the shell as arguments, never in its script");
        let bare = relaunch_command(7, Path::new("/tmp/target/debug/phantom-gui"), &["--flag".into()], &env);
        let a = args(&bare);
        assert!(a[1].starts_with("while kill -0 7 ") && a[1].ends_with("exec \"$@\""), "{a:?}");
        assert_eq!(a[3..], ["/tmp/target/debug/phantom-gui", "--flag"], "a bare binary keeps the environment by itself");
    }

    /// The environment forwarded is the two folders, when set.
    #[test]
    fn only_the_data_and_download_folders_are_forwarded_when_set() {
        let got = super::forwarded_env(|name| match name {
            "PHANTOM_DATA_DIR" => Some("/tmp/a b".into()),
            _ => None,
        });
        assert_eq!(got, [("PHANTOM_DATA_DIR".to_owned(), "/tmp/a b".to_owned())]);
        assert!(super::forwarded_env(|_| None).is_empty());
    }
}
