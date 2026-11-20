//! Phantom: the GUI client for the Phantom service. This file only wires the app together: the
//! Rune spec and its scenes, and which service the app talks to.
//!
//! `cargo build && target/debug/phantom-gui` opens the window (build the whole workspace so
//! `phantom-service` is up to date next to it). `scripts/install-mac.sh` installs Phantom.app.
//!
//! Headless runs get an in-memory service: `target/debug/phantom-gui --snap out.png --scene demo`.
//! To snapshot a real service instead, opt in and point it at a throwaway data dir:
//! `PHANTOM_HEADLESS_SERVICE=1 PHANTOM_DATA_DIR=/tmp/x target/debug/phantom-gui --snap out.png`.

mod bar;
mod changes;
mod menu;
mod model;
mod notice;
mod overlay;
mod pane;
mod platform;
mod service;
mod settings;
mod shortcuts;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod views;
mod windows;

use std::num::NonZeroU16;
use std::time::Instant;

use client::display;
use client::machine::{Step, Why};
use proto::{AbsolutePath, Content, File, ListenPort, Priority, Settings, Status, TorrentInfo, Unavailable};
use std::cell::RefCell;
use std::rc::Rc;

use rune::{App, AppSpec, ImageCache, Key, KeyEvent, Tx, Waker, WindowOptions, WindowSpec};

use model::settings::{AppIcon, IconChange, SettingsSheet, SettingsWindow, Tab};
use model::{Phantom, add};
use notice::Notice;
use platform::app_icon::{Icons, Manifest};
use service::{AddRequest, Connection, DiskChange, Incoming, MemoryService, Service, SocketService};
use theme::preset::{self, LookPreset, Looks};
use theme::saved::{self, Saved};
use windows::{Inbox, Which, Window};

/// Rune calls this before every frame: drain what the service sent since the last one, in order:
/// live data is applied, answers run.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    // What Settings asked first: a change made there is older than anything the service sends now.
    let posted = Inbox::run(s, |s| &s.inbox, tx);
    let incoming = s.service.poll(now);
    let changed = posted || !incoming.is_empty() || s.add_note.as_ref().is_some_and(|n| n.alerting);
    for incoming in incoming {
        match incoming {
            Incoming::Live(live) => s.apply(live),
            Incoming::Answer(answer) => answer(s, tx),
        }
    }
    // Also after selections the table doesn't report, such as Cmd-A or a removed row.
    model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
    model::close_empty_menu(s);
    close_note_when_on(s);
    platform::menu_bar::name_undo(&changes::menu_title(s));
    model::age_add_note(&mut s.add_note, now, tx);
    scripted_drag(now, tx);
    // After the answers, so a notice they tell starts its clock in this frame.
    let changed = s.bar.passed(now) || changed;
    if let Some(due) = s.bar.next_due() {
        tx.wake_after(std::time::Duration::from_secs_f64((due - now).max(0.0)));
    }
    if let Some(after) = s.service.next_wake() {
        tx.wake_after(after);
    }
    changed
}

thread_local! {
    /// The main window, in a headless run only: for [`scripted_drag`].
    static MAIN: RefCell<Option<rune::AppHandle<Phantom>>> = const { RefCell::new(None) };
}

/// How long a scripted drag of the main window takes.
const DRAG: f64 = 0.6;

/// Headless GIFs only: Rune draws no title bar to drag there, so a script's mark `drag-main DX DY`
/// moves the main window by DX, DY points over [`DRAG`] seconds from the mark, eased, as a person
/// dragging it by its title bar would.
fn scripted_drag(now: f64, tx: &mut Tx) {
    let Some(run) = rune::tooling::headless_run() else { return };
    for (name, at) in run.marks() {
        let Some(by) = name.strip_prefix("drag-main ") else { continue };
        let by: Vec<f32> = by.split(' ').filter_map(|v| v.parse().ok()).collect();
        let (&[dx, dy], Some(start)) = (by.as_slice(), run.windows_at(at).into_iter().find(|w| w.key == "main")) else { continue };
        if now < at {
            tx.wake_after(std::time::Duration::from_secs_f64(at - now));
            continue;
        }
        let f = ((now - at) / DRAG).min(1.0) as f32;
        let f = f * f * (3.0 - 2.0 * f);
        let (x, y) = ((start.frame[0] + dx * f) * run.scale(), (start.frame[1] + dy * f) * run.scale());
        MAIN.with_borrow(|main| main.iter().for_each(|main| main.set_position((x.round() as i32, y.round() as i32))));
        if f < 1.0 {
            tx.wake_after(std::time::Duration::from_millis(16));
        }
    }
}

/// A note saying why Pause or Resume is off closes for good once its button is on: it opens
/// again only with another click while off (#89).
fn close_note_when_on(s: &mut Phantom) {
    if s.why_off.is_some_and(|action| views::toolbar::why_off(s, action).is_none()) {
        s.why_off = None;
    }
}

/// The main window's keys. ⌘W closes the window (File ▸ Close Window is the same key), Settings
/// staying as it is, and keeps downloading; ⌘Q quits (the service follows once no client is left);
/// ⌘, opens Settings, as the menu item does; ⌘Z undoes the newest change, a torrent action or a
/// setting's (through the shortcut table); Escape closes the status bar's popover, or else clears
/// the selection, which closes the details pane.
fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command(',') => s.settings.post(|w, _| model::settings::open(w)),
        // The popover first, as the bar's machine says: Escape closes it and nothing else.
        Key::Escape if s.bar.close() => {}
        // (← and → step the popover's notices while the focus is inside it: its own handler.)
        Key::Escape => {
            s.table.clear_selection();
            s.add_note = None;
            s.why_off = None;
        }
        Key::Command('w') => tx.close_window(),
        Key::Command('q') => tx.quit(),
        _ => return shortcuts::on_key(s, ev, tx),
    }
    true
}

const MIB: u64 = 1024 * 1024;

#[allow(clippy::too_many_arguments)]
fn t(id: u64, name: &str, status: Status, progress: u64, size: u64, down: u64, up: u64, peers: u32) -> TorrentInfo {
    // Nothing known until its info arrives.
    let content = (status != Status::FetchingInfo).then_some(Content { size_bytes: size, full_size_bytes: size, progress_bytes: progress });
    TorrentInfo {
        id,
        info_hash: format!("{id:040x}"),
        name: Some(name.into()),
        status,
        content,
        uploaded_bytes: 0,
        download_bytes_per_sec: down,
        upload_bytes_per_sec: up,
        live_peers: peers,
        trackers: Vec::new(),
        location: None,
        actions: proto::Actions::default(),
    }
}

/// The demo, with a 6 GiB torrent more and 5 GiB free.
fn short_of_space() -> MemoryService {
    let tears = t(5, "Tears of Steel (2012) 4K", Status::Downloading, 0, 6 << 30, 31 * MIB, 0, 9);
    demo_service().with_torrents(vec![tears]).with_free_space(5 << 30)
}

fn demo_service() -> MemoryService {
    let mib = MIB;
    MemoryService::new()
        .with_torrents(vec![
            t(0, "debian-13.7.0-amd64-netinst.iso", Status::Downloading, 412 * mib, 756 * mib, 68 * mib, 0, 32),
            t(1, "ubuntu-24.04.3-desktop-amd64.iso", Status::Seeding, 6100 * mib, 6100 * mib, 0, 3 * mib / 2, 7),
            t(2, "archlinux-2026.11.01-x86_64.iso", Status::Paused, 300 * mib, 1200 * mib, 0, 0, 0),
            t(3, "Sintel (4K)", Status::FetchingInfo, 0, 0, 0, 0, 0),
            t(4, "Big Buck Bunny (2008)", Status::Downloading, 0, 0, 24 * mib, 0, 18),
        ])
        .with_files(4, {
            let kib = 1024;
            let f = |index, path: &str, size, progress, priority| File {
                index,
                path: path.into(),
                size_bytes: size,
                progress_bytes: progress,
                priority,
                missing: false,
                on_disk: false,
            };
            vec![
                f(0, "Big Buck Bunny (2008) 1080p.mkv", 1430 * mib, 862 * mib, Priority::Normal),
                f(1, "Big Buck Bunny (2008) 2160p.mkv", 4210 * mib, 0, Priority::Off),
                f(2, "Making of/Interview with the director.mp4", 312 * mib, 0, Priority::Normal),
                f(3, "Making of/Storyboards.pdf", 38 * mib, 0, Priority::Normal),
                f(4, "Subtitles/Deutsch.srt", 51 * kib, 51 * kib, Priority::Normal),
                f(5, "Subtitles/English.srt", 48 * kib, 48 * kib, Priority::Normal),
                f(6, "Subtitles/Français.srt", 50 * kib, 50 * kib, Priority::Normal),
                f(7, "poster.jpg", 820 * kib, 820 * kib, Priority::Normal),
            ]
        })
}

/// A drive to move torrents to, in the `moves` scenes.
const ARCHIVE: &str = "/Volumes/Archive";

/// [`demo_service`] whose moves take 3 s: into `Clash` a file of each torrent is already, into
/// `Full` the disk fills halfway.
fn moves() -> MemoryService {
    demo_service().moves_take(3.0).holding(format!("{ARCHIVE}/Clash")).move_failing(format!("{ARCHIVE}/Full"), proto::NotMoved::DiskFull)
}

fn slow_adds() -> MemoryService {
    let nobody = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";
    MemoryService::new()
        .with_torrents(vec![t(0, "debian-13.7.0-amd64-netinst.iso", Status::Seeding, 756 * MIB, 756 * MIB, 0, MIB, 4)])
        .adds_after(1.0, 3.0)
        .rejecting(nobody)
}

/// An app whose link is in `connection`: after what the fake service sent first, which says
/// it's connected.
fn connection(s: &mut Phantom, connection: Connection) {
    for incoming in s.service.poll(0.0) {
        if let Incoming::Live(live) = incoming {
            s.apply(live);
        }
    }
    s.connection = connection;
}

const NOBODY: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";

/// Files named `names` in Downloads, as dropped on the window; those starting with `bad` fail.
fn files(names: &[&str]) -> Vec<AddRequest> {
    names.iter().map(|name| AddRequest::File(format!("/Users/me/Downloads/{name}").into())).collect()
}

/// The demo list, adds answered after half a second, the magnet named Nobody and the files
/// named `bad…` failing.
fn failing_adds() -> MemoryService {
    let mut service = demo_service().adds_after(0.5, 0.5).rejecting(NOBODY).with_default_destination(Some("/srv/torrents".into()));
    for name in ["bad-1.torrent", "bad-2.torrent", "bad-3.torrent"] {
        service = service.rejecting(&format!("/Users/me/Downloads/{name}"));
    }
    service
}

/// The settings the fake service answers with, its port pinned and taken if `taken`.
fn demo_settings(taken: bool) -> Settings {
    let port = NonZeroU16::new(51413).unwrap();
    Settings {
        default_destination: Some(AbsolutePath::new("/Users/me/Downloads").unwrap()),
        default_destination_unavailable: None,
        listen_port: if taken { ListenPort::Pinned(port) } else { ListenPort::Picked(port) },
        listening_on: if taken { NonZeroU16::new(54321).unwrap() } else { port },
        upnp: true,
        keep_awake: false,
        speed_limits: proto::SpeedLimits { download: proto::SpeedLimit::new(2 << 20).ok(), upload: None },
        seeding: proto::SeedingLimits { ratio: proto::Ratio::new(200).ok(), time: None },
        reset: Vec::new(),
    }
}

/// Saved Look Presets for scenes: the fresh one, as "System", and Dark in both, as "Night".
fn demo_presets() -> Vec<saved::SavedPreset> {
    let fresh = LookPreset::fresh();
    let night = LookPreset { light: fresh.dark.clone(), dark: fresh.dark.clone() };
    [("System", fresh), ("Night", night)]
        .map(|(name, preset)| saved::SavedPreset { name: saved::Name::parse(name).expect("a name"), preset })
        .to_vec()
}

/// The window in front is the one keys go to: Edit ▸ Undo follows it. Queued behind what Settings
/// posted, which says when it came to the front, so the newest focus change wins.
fn on_focus(s: &mut Phantom, focused: bool, _: &mut Tx) {
    if focused {
        s.inbox.push(|s, _| s.front = Which::Main);
    }
}

/// The Dock icon (or opening Phantom again from Finder) brings the main window back, whatever else
/// shows, as Window ▸ Phantom does.
fn on_reopen(s: &mut Phantom, _: bool, _: &mut Tx) -> bool {
    s.window.raise();
    true
}

/// Rune calls this before every frame of Settings: what the main window posted, then a field that
/// lost the focus applies its text, and the main window hears whether one is being edited.
fn settings_poll(s: &mut SettingsWindow, _: f64, tx: &mut Tx) -> bool {
    let posted = Inbox::run(s, |s| &s.inbox, tx);
    model::settings::follow_focus(s);
    model::settings::tell_editing(s);
    posted
}

/// Settings' keys: ⌘W closes it (a Look Preset's sheet first), as Escape does (its page's own, see `views::settings`); ⌘, raises
/// it; ⌘Z, once no field took it as its text's, is the main window's (#221, #255): the newest change,
/// whichever; ⌘Q quits; nothing else of the main window's shortcuts.
fn settings_on_key(s: &mut SettingsWindow, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command(',') => model::settings::open(s),
        // A Look Preset's sheet first, then the window.
        Key::Command('w') if s.sheet.as_ref().is_some_and(|sheet| sheet.edit.is_some()) => model::settings::presets::close(s),
        Key::Command('w') => model::settings::close(s),
        Key::Command('q') => tx.quit(),
        _ if shortcuts::command_of(ev) == Some(shortcuts::Command::Undo) => s.main.post(|s, _| changes::press(s)),
        _ => return false,
    }
    true
}

/// Settings in front tells the main window, for Edit ▸ Undo. Losing the front, to the main window or
/// another app, leaves the field being edited, which applies it (#254): a ⌘Z in the main window
/// then takes it as the newest change.
fn settings_on_focus(s: &mut SettingsWindow, focused: bool, _: &mut Tx) {
    if focused {
        s.main.post(|s, _| s.front = Which::Settings);
    } else {
        model::settings::left_window(s);
    }
}

/// Settings' close button (and any close Rune runs it for): the field being edited is applied.
fn settings_on_close(s: &mut SettingsWindow, _: &mut Tx) {
    model::settings::closed(s);
}

/// Settings open on `tab`, as already answered.
fn settings_scene(s: &mut SettingsWindow, tab: Tab, settings: Settings) {
    s.sheet = Some(SettingsSheet::showing(tab, settings));
}

/// Decodes the App icons to draw; a headless run waits for them, so its frames show them.
fn setup(s: &mut SettingsWindow, waker: Waker) {
    s.app_icon.load_images(&ImageCache::new(waker));
    if rune::tooling::headless_run().is_some() {
        for (_, image) in &s.app_icon.images {
            let _ = image.wait();
        }
    }
}

/// The App icons and the choice kept in `gui.json`, following `theme`; headless, nothing is kept.
fn app_icon(theme: Icons) -> AppIcon {
    let manifest = Manifest::load(&platform::app_icon::manifest_dir());
    if rune::tooling::headless_run().is_some() {
        return AppIcon::new(manifest, None, theme, |_| {});
    }
    let choice = settings::file::read(platform::app_icon::SETTING);
    #[cfg(target_os = "macos")]
    return AppIcon::new(manifest, choice, theme, |change| match change {
        IconChange::Chose(choice) => platform::app_icon::appkit::choose(choice),
        IconChange::Theme(theme) => platform::app_icon::appkit::follow(theme),
    });
    #[cfg(not(target_os = "macos"))]
    AppIcon::new(manifest, choice, theme, |_| {})
}

/// The Active Look Preset kept in `gui.json` (repaired already), each change kept there;
/// headless, a fresh install's, kept nowhere.
fn looks(headless: bool) -> Looks {
    if headless {
        return Looks::default();
    }
    let stored: preset::Stored = settings::file::read(preset::SETTING).unwrap_or_default();
    Looks::new(LookPreset::read(&stored).0, |preset: &LookPreset| settings::file::keep(preset::SETTING, Some(&preset.stored())))
}

/// The saved Look Presets kept in `gui.json` (repaired already), each change kept there; headless,
/// none, kept nowhere.
fn saved(headless: bool) -> Saved {
    if headless {
        return Saved::default();
    }
    let entries: Vec<serde_json::Value> = settings::file::read(saved::SETTING).unwrap_or_default();
    Saved::new(saved::read(&entries).0, |list: &[saved::SavedPreset]| settings::file::keep(saved::SETTING, Some(&saved::stored(list))))
}

fn spec() -> AppSpec<Phantom> {
    AppSpec {
        on_key: Some(on_key),
        poll: Some(poll),
        min_size: Some((640.0, 320.0)),
        scenes: &[
            ("demo", |s| {
                s.service = Box::new(demo_service());
                s.table.select("0");
            }),
            // Nothing selected; a selected torrent's details come 2 s after it's selected.
            ("slow-details", |s| s.service = Box::new(demo_service().details_after(2.0))),
            ("details", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.pane.files.select("1");
            }),
            // A downloaded file set Off, then moved to the Trash (#276): it shows none of it, and its
            // menu (right-click it) offers no Move to Trash.
            ("trashed-off", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.pane.files.select("5");
                let off = proto::TorrentCommand::SetPriority { files: vec![5], priority: Priority::Off };
                model::command(&mut *s.service, &[4], off);
                model::command(&mut *s.service, &[4], proto::TorrentCommand::TrashFiles { files: vec![5] });
            }),
            ("fetching", |s| {
                s.service = Box::new(demo_service());
                s.table.select("3");
            }),
            ("missing", |s| {
                s.service = Box::new(demo_service().with_missing(4, &[0]));
                s.table.select("4");
            }),
            ("unavailable", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                s.service = Box::new(demo_service().at(0.0, DiskChange::DestinationGone { id: 4, reason }));
                s.table.select("4");
            }),
            ("destination-back", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                let back = DiskChange::DestinationBack { id: 4 };
                s.service = Box::new(demo_service().at(0.0, DiskChange::DestinationGone { id: 4, reason }).at(2.0, back));
                s.table.select("4");
            }),
            // The missing file is put back 2 s in: checked, then running again, as with the real service.
            ("put-back", |s| {
                let back = DiskChange::FilesPutBack { id: 4, files: vec![0] };
                s.service = Box::new(demo_service().with_missing(4, &[0]).at(2.0, back));
                s.table.select("4");
            }),
            // A check under way (it takes a second): right-click it for Cancel check.
            ("checking", |s| {
                let checking = t(5, "Sintel (2010) 4K", Status::Checking, 0, 3400 * MIB, 0, 0, 0);
                s.service = Box::new(demo_service().with_torrents(vec![checking]));
                s.table.select("5");
            }),
            // Its check was cancelled.
            ("unchecked", |s| {
                let unchecked = t(5, "Sintel (2010) 4K", Status::Unchecked, 0, 3400 * MIB, 0, 0, 0);
                s.service = Box::new(demo_service().with_torrents(vec![unchecked]));
                s.table.select("5");
            }),
            ("error", |s| {
                let failure = proto::TorrentFailure::DiskFull;
                let failed = t(5, "Tears of Steel (2012)", Status::Error { failure }, 210 * MIB, 3400 * MIB, 0, 0, 0);
                s.service = Box::new(demo_service().with_torrents(vec![failed]));
                s.table.select("5");
            }),
            // Moves (#24): each takes 3 s; into Clash a file of each is already, into Full the disk fills
            // halfway. `moving` has one under way, `clash` its dialog open, `move-failed` the error.
            ("moves", |s| s.service = Box::new(moves())),
            ("moving", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::send(s, 1, proto::AbsolutePath::new(ARCHIVE).expect("absolute"), None);
            }),
            ("clash", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::send(s, 1, proto::AbsolutePath::new(format!("{ARCHIVE}/Clash")).expect("absolute"), None);
            }),
            ("move-failed", |s| {
                s.service = Box::new(moves().moves_take(0.0));
                s.table.select("1");
                model::moving::send(s, 1, proto::AbsolutePath::new(format!("{ARCHIVE}/Full")).expect("absolute"), None);
            }),
            ("notice", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(Notice::error("Not a magnet link or .torrent URL"));
            }),
            // An Open that couldn't find its file, the path long: the line holds Phantom's words only.
            ("open-failed", |s| {
                struct NotThere;
                impl platform::Finder for NotThere {
                    fn show(&mut self, _: &std::path::Path, _: platform::Show) -> Result<(), platform::ShowError> {
                        Err(platform::ShowError::NotOnDisk)
                    }
                }
                s.service = Box::new(demo_service());
                s.finder = Box::new(NotThere);
                let path = "/Volumes/An External Drive With a Long Name/Downloads/Big Buck Bunny (2008) [1080p x265 10bit]/Big Buck Bunny (2008) 1080p.mkv";
                platform::show(s, std::path::Path::new(path), platform::Show::Open);
            }),
            // A receipt: an add, its torrent's name in the list.
            ("receipt", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(
                    Notice::new(display::added_line(&proto::Added {
                        id: 1,
                        name: Some("Big Buck Bunny (2008)".into()),
                        already_existed: false,
                    }))
                    .brief()
                    .with_text("Big Buck Bunny (2008)"),
                );
            }),
            ("settings-reset", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(model::settings::reset_notice(&["app_icon".into()], "gui.json").expect("a reset"));
            }),
            ("rejecting", |s| s.service = Box::new(MemoryService::new().rejecting_links())),
            // An undo refused: what Delete moved left the Trash.
            // Deletes that take 3 s to move their files and undos that take 1.5 s to put them back
            // (#190): the list changes at once all the same.
            ("slow-deletes", |s| s.service = Box::new(demo_service().trash_takes(3.0).undo_takes(1.5))),
            ("cant-undo", |s| {
                s.service = Box::new(demo_service());
                let path = proto::AbsolutePath::new("/Users/me/Downloads/archlinux-2026.11.01-x86_64.iso").expect("absolute");
                let undo = proto::Undoable {
                    seq: 1,
                    action: proto::UndoAction::Delete,
                    torrents: vec![proto::Named { id: 9, name: "archlinux-2026.11.01-x86_64.iso".into() }],
                };
                let refused = client::Failure::Refused(proto::Refusal::Undo {
                    undo,
                    why: proto::CantUndo::NotInTrash { path },
                    stranded: Vec::new(),
                });
                for notice in model::undo_notices(&Err(refused)) {
                    s.bar.tell(notice);
                }
            }),
            // Adds take 1 s, a .torrent URL 3 s, as from a busy service; the magnet named Nobody fails.
            ("adds", |s| s.service = Box::new(slow_adds())),
            ("no-default", |s| s.service = Box::new(slow_adds().with_default_destination(None))),
            // The connection's states, as the status bar words them.
            ("connecting", |s| connection(s, Connection::Connecting)),
            ("replacing", |s| connection(s, Connection::Replacing { deadline: Instant::now(), step: Step::Act })),
            ("spawning", |s| connection(s, Connection::Spawning { deadline: Instant::now(), step: Step::Act })),
            ("lost", |s| connection(s, Connection::Lost { why: Why::DidNotStart, retry_at: Instant::now() })),
            ("outdated", |s| connection(s, Connection::Outdated)),
            // The list, ready for files to be dropped (scripts); `bad…` ones fail.
            ("outside-adds", |s| s.service = Box::new(failing_adds())),
            // Adds made outside the window, failing: one link, 2 of 5 files, two batches together.
            ("add-failed", |s| {
                s.service = Box::new(failing_adds());
                add(s, vec![AddRequest::link(NOBODY).expect("a magnet link")], Vec::new());
            }),
            ("add-failed-some", |s| {
                s.service = Box::new(failing_adds());
                add(
                    s,
                    files(&["Sintel.torrent", "bad-1.torrent", "Tears of Steel.torrent", "bad-2.torrent", "Elephants Dream.torrent"]),
                    Vec::new(),
                );
            }),
            ("add-failed-two", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                add(s, vec![AddRequest::link(NOBODY).expect("a magnet link")], Vec::new());
            }),
            // The error notice with the message by Add open.
            ("add-failed-and-add-note", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                s.add_note = Some(model::AddNote::about(&s.magnet, display::link_problem(client::LinkProblem::NotALink)));
            }),
            // Several entries of different shapes, enough to scroll.
            ("several-entries", |s| {
                s.service = Box::new(failing_adds());
                add(s, files(&["bad-1.torrent", "Sintel.torrent", "bad-2.torrent"]), Vec::new());
                add(s, vec![AddRequest::link(NOBODY).expect("a magnet link")], Vec::new());
                s.bar.tell(Notice::error(display::cant_start_finder()).with_text("No such application"));
                add(s, files(&["bad-1.torrent", "bad-2.torrent", "bad-3.torrent", "Tears of Steel.torrent"]), Vec::new());
                add(s, vec![AddRequest::link(NOBODY).expect("a magnet link")], Vec::new());
            }),
            // 5 GiB free, and a 6 GiB torrent on top of the others: its own warning, and the disk's.
            ("free-space", |s| s.service = Box::new(short_of_space())),
            // Several notices at once: the settings reset, an add, and a disk short of space (which
            // comes with the first snapshot, so it's the newest).
            ("notices", |s| {
                s.service = Box::new(short_of_space());
                s.bar.tell(model::settings::reset_notice(&["app_icon".into()], "gui.json").expect("a reset"));
                s.bar.tell(Notice::new("Added the torrent").with_text("Big Buck Bunny (2008)"));
            }),
            ("many-errors", |s| {
                s.service = Box::new(failing_adds());
                for n in 0..12 {
                    add(s, files(&["bad-1.torrent", "bad-2.torrent"]), Vec::new());
                    s.bar.tell(Notice::error(display::cant_start_finder()).with_text(format!("Attempt {n}")));
                }
            }),
            // The list, for scripts that open Settings; a folder on a drive that isn't connected
            // can't be the default.
            ("settings-script", |s| {
                let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
                s.service = Box::new(
                    demo_service()
                        .with_default_destination(Some("/Users/me/Downloads".into()))
                        .unreachable("/Volumes/MyDisk/Films", reason),
                );
            }),
            // A small torrent seeding at 1 MiB/s, with a ratio of 1.0 set: Done in three seconds.
            ("seeding-limit", |s| {
                let short = t(5, "Cosmos Laundromat (2015)", Status::Seeding, 3 * MIB, 3 * MIB, 0, MIB, 4);
                let ratio = proto::Ratio::new(100).ok();
                s.service = Box::new(demo_service().with_torrents(vec![short]).with_seeding(proto::SeedingLimits { ratio, time: None }));
            }),
        ],
        ..AppSpec::new("Phantom", (960.0, 540.0), || Phantom::new(Box::new(MemoryService::new())), views::view)
    }
}

/// The Settings window: hidden until it opens, its own size; scenes show it open on a tab, as
/// already answered (`--window settings --scene settings-network`).
fn settings_spec() -> AppSpec<SettingsWindow> {
    AppSpec {
        setup: Some(setup),
        on_key: Some(settings_on_key),
        poll: Some(settings_poll),
        min_size: Some(SETTINGS_MIN),
        scenes: &[
            ("settings", |s| settings_scene(s, Tab::General, demo_settings(false))),
            // The default folder is on a drive that isn't connected: the row says so.
            ("settings-folder-gone", |s| {
                let settings =
                    Settings { default_destination: Some(AbsolutePath::new("/Volumes/MyDisk/Torrents").unwrap()), ..demo_settings(false) };
                settings_scene(s, Tab::General, settings);
                s.default_unavailable = Some(Unavailable::DriveNotConnected { name: "MyDisk".into() });
            }),
            ("settings-network", |s| settings_scene(s, Tab::Network, demo_settings(false))),
            // A pinned port another program has: a temporary one for now.
            ("settings-taken", |s| settings_scene(s, Tab::Network, demo_settings(true))),
            ("settings-refused", |s| {
                settings_scene(s, Tab::Network, demo_settings(false));
                s.sheet.as_mut().unwrap().port = rune::TextEdit::new("70000");
                model::settings::submit_port(s);
            }),
            ("settings-no-default", |s| {
                settings_scene(s, Tab::General, Settings { default_destination: None, ..demo_settings(false) });
            }),
            // A fresh install's look: Tone split, Light in Light and Dark in Dark; the App icons under it.
            ("settings-appearance", |s| settings_scene(s, Tab::Appearance, demo_settings(false))),
            // Two saved Look Presets, the active one one of them.
            ("settings-presets", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.saved.set(demo_presets());
            }),
            // Night's sheet open over Settings: both windows show Night meanwhile.
            ("settings-preset-sheet", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.saved.set(demo_presets());
                model::settings::presets::edit(s, 1);
            }),
            ("settings-custom", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.sheet.as_mut().unwrap().custom_pair = true;
                s.app_icon.choice = Some(Icons::Pair { dark: "calling-card".into(), light: "full-moon".into() });
            }),
        ],
        ..AppSpec::new("Settings", SETTINGS_SIZE, SettingsWindow::detached, views::settings::view)
    }
}

/// The size Settings first opens at, then as it was left; and the smallest it gets.
const SETTINGS_SIZE: (f32, f32) = (600.0, 460.0);
const SETTINGS_MIN: (f32, f32) = (480.0, 320.0);

/// A headless run (a still, a GIF, a script) never touches the user's own data dir, `gui.json` or
/// Downloads: unless the caller already points `PHANTOM_DATA_DIR` and `PHANTOM_DOWNLOAD_DIR`
/// somewhere, they go to a fresh temp dir.
fn isolate_headless() {
    if std::env::var_os("PHANTOM_DATA_DIR").is_some() && std::env::var_os("PHANTOM_DOWNLOAD_DIR").is_some() {
        return;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let root = std::env::temp_dir().join(format!("phantom-headless-{}-{nanos}", std::process::id()));
    for (var, dir) in [("PHANTOM_DATA_DIR", "data"), ("PHANTOM_DOWNLOAD_DIR", "downloads")] {
        if std::env::var_os(var).is_none() {
            // SAFETY: first thing in `main`, before any other thread exists.
            unsafe { std::env::set_var(var, root.join(dir)) };
        }
    }
}

fn main() {
    // Before `run`, where `headless_run()` is still `None`: only the arguments say whether this
    // run is headless, and it must never read or write the user's own data.
    let headless = rune::is_headless();
    if headless {
        isolate_headless();
    }
    // Before anything reads `gui.json`; the notice shows once the window does.
    let reset = if !headless { settings::repair(&proto::data_dir().join("gui.json")) } else { Ok(vec![]) };
    let app = platform::menu_bar::items().into_iter().fold(App::new(spec()), App::menu_item);
    let mut app = app.standard_menus().keep_running().on_open_url(platform::open_url).on_focus(on_focus).on_reopen(on_reopen);
    let handle = app.handle();
    let looks = looks(headless);
    #[cfg(target_os = "macos")]
    if !headless {
        platform::menu_bar::install();
        platform::open_documents::install(handle.clone());
        platform::app_icon::appkit::install(looks.app_icons());
    }
    // The two windows, each with its own state, and the ways from one to the other (`windows`).
    let settings_app = App::new(settings_spec()).on_close(settings_on_close).on_focus(settings_on_focus);
    #[cfg(target_os = "macos")]
    let settings_app = settings_app.on_window(|window| platform::remember_frame(window, "Settings"));
    let settings_handle = settings_app.handle();
    let main_wake = handle.clone();
    let (to_main, main_inbox) = windows::link(move || main_wake.wake());
    let (settings_wake, settings_open) = (settings_handle.clone(), settings_handle.clone());
    let (to_settings, settings_inbox) = windows::link(move || settings_wake.wake());
    let settings_window = Window::new(move || settings_open.open(), move || settings_handle.close());
    let looks = Rc::new(RefCell::new(looks));
    let shared = looks.clone();
    let settings_app = settings_app.init(move || {
        let icons = shared.borrow().app_icons();
        SettingsWindow::new(shared, saved(headless), app_icon(icons), to_main, settings_inbox, settings_window)
    });
    let options = WindowOptions { visible: false, min_size: Some(SETTINGS_MIN), ..Default::default() };
    app.window(WindowSpec::from_app("settings", settings_app).options(options));
    app.init(move || {
        let raise = handle.clone();
        let window = Window::new(move || raise.open(), || {});
        if rune::tooling::headless_run().is_some() {
            MAIN.set(Some(handle.clone()));
        }
        let wake = move || handle.wake();
        let service: Box<dyn Service> = if rune::tooling::headless_run().is_none() {
            Box::new(SocketService::start(wake))
        } else if std::env::var_os("PHANTOM_HEADLESS_SERVICE").is_some() {
            Box::new(SocketService::start_loaded(wake))
        } else {
            Box::new(MemoryService::new())
        };
        let mut app = Phantom { window, looks, settings: to_settings, inbox: main_inbox, ..Phantom::new(service) };
        // Headless runs keep the fake finder: nothing opens.
        if rune::tooling::headless_run().is_none() {
            app.finder = Box::new(platform::SystemFinder);
        }
        match &reset {
            Ok(reset) => {
                if let Some(notice) = model::settings::reset_notice(reset, "gui.json") {
                    app.bar.tell(notice);
                }
            }
            Err(e) => app.bar.tell(Notice::error(display::cant_read_gui_json()).with_text(display::sentence(&e.to_string()))),
        }
        app
    })
    .run();
}
