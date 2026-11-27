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
mod commands;
mod menu;
mod menu_bar;
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
use theme::icon::Tint;
use theme::preset::{self, LookPreset, Looks};
use theme::saved::{self, Saved};
use windows::{Inbox, Which, Window};

/// Rune calls this before every frame: drain what the service sent since the last one, in order:
/// live data is applied, answers run.
fn poll(s: &mut Phantom, now: f64, tx: &mut Tx) -> bool {
    // What Settings asked first: a change made there is older than anything the service sends now.
    let posted = Inbox::run(s, |s| &s.inbox, tx);
    let incoming = s.service.poll(now);
    let changed = posted || s.finder.news() || !incoming.is_empty();
    for incoming in incoming {
        match incoming {
            Incoming::Live(live) => s.apply(live),
            Incoming::Answer(answer) => answer(s, tx),
        }
    }
    // Also after selections the table doesn't report, such as Cmd-A or a removed row.
    model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
    model::close_empty_menu(s);
    model::renaming::follow_focus(s);
    platform::menu_bar::sync(s, tx);
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
        original_name: None,
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
            TorrentInfo {
                uploaded_bytes: 3277 * mib,
                ..t(1, "ubuntu-24.04.3-desktop-amd64.iso", Status::Seeding, 6100 * mib, 6100 * mib, 0, 3 * mib / 2, 7)
            },
            t(2, "archlinux-2026.11.01-x86_64.iso", Status::Paused, 300 * mib, 1200 * mib, 0, 0, 0),
            t(3, "Sintel (4K)", Status::FetchingInfo, 0, 0, 0, 0, 0),
            TorrentInfo { uploaded_bytes: 960 * mib, ..t(4, "Big Buck Bunny (2008)", Status::Downloading, 0, 0, 24 * mib, 0, 18) },
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

/// The demo with a torrent in each status that needs the user, and a check under way (#33).
fn states() -> MemoryService {
    let failure = proto::TorrentFailure::DiskFull;
    let ratio = proto::Ratio::new(200).expect("a ratio");
    let reason = Unavailable::DriveNotConnected { name: "MyDisk".into() };
    demo_service()
        .with_torrents(vec![
            t(5, "Tears of Steel (2012)", Status::Error { failure }, 210 * MIB, 3400 * MIB, 0, 0, 0),
            t(6, "Cosmos Laundromat (2015)", Status::Done { reached: Some(proto::Reached::Ratio(ratio)) }, 1900 * MIB, 1900 * MIB, 0, 0, 0),
            t(7, "Elephants Dream (2006)", Status::Downloading, 300 * MIB, 820 * MIB, 3 * MIB, 0, 4),
            t(8, "Spring (2019)", Status::Seeding, 1100 * MIB, 1100 * MIB, 0, 0, 0),
            t(9, "Agent 327 (2017)", Status::Checking, 0, 2600 * MIB, 0, 0, 0),
        ])
        .with_missing(7, &[0])
        .at(0.0, DiskChange::DestinationGone { id: 8, reason })
}

/// Two torrents, one of them of 24 files: the list needs little of the window (#33).
fn few_rows() -> MemoryService {
    MemoryService::new()
        .with_torrents(vec![
            t(0, "debian-13.7.0-amd64-netinst.iso", Status::Downloading, 412 * MIB, 756 * MIB, 68 * MIB, 0, 32),
            TorrentInfo { uploaded_bytes: 960 * MIB, ..t(4, "Big Buck Bunny (2008)", Status::Downloading, 0, 0, 24 * MIB, 0, 18) },
        ])
        .with_files(4, few_rows_files())
}

/// 24 files, every seventh Off.
fn few_rows_files() -> Vec<File> {
    let file = |index: usize| File {
        index,
        path: format!("Big Buck Bunny (2008)/Frames/{:03}-{:03}.png", index * 100, index * 100 + 99).into(),
        size_bytes: (40 + index as u64 * 3) * MIB,
        progress_bytes: (index as u64 % 3) * 20 * MIB,
        priority: if index % 7 == 6 { Priority::Off } else { Priority::Normal },
        missing: false,
        on_disk: false,
    };
    (0..24).map(file).collect()
}

/// Folders torrents were moved to, for Move to ▸ in scenes: Films last.
fn recent_demo(s: &mut Phantom) {
    for inside in ["/Clash", "/Isos", "/Films"] {
        s.recent.record(archive(inside));
    }
}

/// A name longer than the details header's room for it, in `rename-long`.
const LONG_ISO: &str = "ubuntu-24.04.3-desktop-amd64 (Noble Numbat, the point release with the long description of its build).iso";

/// A drive to move torrents to, in the `moves` scenes.
const ARCHIVE: &str = "/Volumes/Archive";

/// A folder of the drive moves go to.
fn archive(inside: &str) -> proto::AbsolutePath {
    proto::AbsolutePath::new(format!("{ARCHIVE}{inside}")).expect("absolute")
}

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

/// An app talking to `service`, its list in already: what a scene acts on at once.
fn listed(s: &mut Phantom, service: MemoryService) {
    s.service = Box::new(service);
    for incoming in s.service.poll(0.0) {
        if let Incoming::Live(live) = incoming {
            s.apply(live);
        }
    }
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
        records_set_aside: None,
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
    // An image file added as an icon that's been read.
    let loaded = model::settings::poll_icons(s);
    posted || loaded
}

/// Settings' keys: ⌘W closes it (a Look Preset's sheet first), as Escape does (its page's own, see `views::settings`); ⌘, raises
/// it; ⌘Z, once no field took it as its text's, is the main window's (#221, #255): the newest change,
/// whichever; ⌘Q quits; nothing else of the main window's shortcuts.
fn settings_on_key(s: &mut SettingsWindow, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command(',') => model::settings::open(s),
        Key::Command('w') => model::settings::close_front(s),
        Key::Command('q') => tx.quit(),
        _ if shortcuts::command_of(ev) == Some(commands::InApp::Undo.into()) => s.main.post(|s, _| changes::press(s)),
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
    let headless = rune::tooling::headless_run().is_some();
    // The image files added as icons are read off the main thread: the window opens at once. A
    // headless run reads them first, to draw them.
    if !headless {
        s.app_icon.load_in_background(waker.clone());
    }
    s.app_icon.load_images(Rc::new(ImageCache::new(waker)));
    s.app_icon.load();
    model::settings::poll_icons(s);
    s.app_icon.redraw(&s.looks.borrow());
    if headless {
        for image in s.app_icon.images.iter().map(|(_, image)| image).chain(s.app_icon.drawn_images()) {
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
    let added: Vec<String> = settings::file::read(platform::app_icon::ADDED).unwrap_or_default();
    let tints = settings::file::read(theme::icon::SETTING).unwrap_or_default();
    #[cfg(target_os = "macos")]
    return AppIcon::new(manifest, choice, theme, |change| match change {
        IconChange::Chose(choice) => platform::app_icon::appkit::choose(choice),
        IconChange::Theme(theme) => platform::app_icon::appkit::follow(theme),
        IconChange::Added(added) => settings::file::keep(platform::app_icon::ADDED, Some(&added)),
        IconChange::Loaded(icon) => platform::app_icon::appkit::loaded(icon),
        // The Dock and the bundle still show the shipped picture (#70, #71).
        IconChange::Tints(tints) => settings::file::keep(theme::icon::SETTING, Some(tints)),
    })
    .with_added(added)
    .with_tints(tints);
    #[cfg(not(target_os = "macos"))]
    AppIcon::new(manifest, choice, theme, |change| match change {
        IconChange::Added(added) => settings::file::keep(platform::app_icon::ADDED, Some(&added)),
        IconChange::Tints(tints) => settings::file::keep(theme::icon::SETTING, Some(tints)),
        _ => {}
    })
    .with_added(added)
    .with_tints(tints)
}

/// The Active Look Preset kept in `gui.json` (repaired already), each change kept there;
/// headless, the one in the run's own (temp) data dir's `gui.json`, a fresh install's when it has
/// none, so a still can show a setting; a change is kept nowhere.
fn looks(headless: bool) -> Looks {
    let stored: preset::Stored = settings::file::read(preset::SETTING).unwrap_or_default();
    let preset = LookPreset::read(&stored).0;
    if headless {
        return Looks::new(preset, |_| {});
    }
    Looks::new(preset, |preset: &LookPreset| settings::file::keep(preset::SETTING, Some(&preset.stored())))
}

/// The folders Move to ▸ offers, as `gui.json` keeps them (repaired already), each change kept there.
fn recent_folders() -> model::recent::RecentFolders {
    let kept: Vec<AbsolutePath> = settings::file::read(model::recent::SETTING).unwrap_or_default();
    model::recent::RecentFolders::new(kept, |list: &[AbsolutePath]| settings::file::keep(model::recent::SETTING, Some(list)))
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

/// The columns' order, those hidden and the widths dragged, the pane's height and its tab, kept in `gui.json` (#33),
/// each change kept there; headless, none, kept nowhere.
fn window_prefs(headless: bool) -> model::layout::WindowPrefs {
    use model::layout::{COLUMN_ORDER, COLUMN_WIDTHS, Column, HIDDEN_COLUMNS, Kept, PANE_HEIGHT, PANE_TAB, WindowPrefs};
    if headless {
        return WindowPrefs::default();
    }
    let hidden: Vec<Column> = settings::file::read(HIDDEN_COLUMNS).unwrap_or_default();
    let kept = Kept {
        order: settings::file::read(COLUMN_ORDER).unwrap_or_default(),
        hidden: hidden.into_iter().collect(),
        widths: settings::file::read(COLUMN_WIDTHS).unwrap_or_default(),
        pane_height: settings::file::read::<model::layout::PaneHeight>(PANE_HEIGHT).map(f32::from),
        tab: settings::file::read(PANE_TAB).unwrap_or_default(),
    };
    WindowPrefs::new(kept, |key, value| settings::file::keep(key, Some(&value)))
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
            // The window's layout (#33): a torrent in each status; the header's column menu open, Peers
            // hidden; a torrent of one file; the pane's divider dragged.
            ("states", |s| s.service = Box::new(states())),
            ("columns-menu", |s| {
                use model::layout::{Column, ColumnOrder};
                s.service = Box::new(states());
                let order =
                    [Column::Name, Column::Status, Column::Progress, Column::Size, Column::Eta, Column::Down, Column::Up, Column::Peers];
                s.prefs.kept.order = ColumnOrder::try_from(order.to_vec()).expect("every column");
                s.prefs.toggle(model::layout::Column::Peers, &model::layout::Column::ALL);
                s.menu = menu::Menu::Open(menu::Open::Columns { at: (520.0, 70.0) });
            }),
            ("single-file", |s| {
                s.service = Box::new(demo_service());
                s.table.select("0");
            }),
            // The window (#33): a torrent in an error, at a seeding limit; a list of two torrents, one of many
            // files, the pane taking the room; the Info tab; the columns in another order.
            ("pane-error", |s| {
                s.service = Box::new(states());
                s.table.select("5");
            }),
            ("pane-limit", |s| {
                s.service = Box::new(states());
                s.table.select("6");
            }),
            // The row toggles (a paused one hovered by a script; one in an error; and a disabled
            // one, selected, at the end: the service lists neither Pause nor Resume for a torrent
            // whose info couldn't be fetched); the Info tab of a torrent at a seeding limit.
            ("toggles", |s| {
                let failure = proto::TorrentFailure::FetchFailed { detail: "no peers".into() };
                s.service = Box::new(states().with_torrents(vec![t(10, "Sintel (2010)", Status::Error { failure }, 0, 0, 0, 0, 0)]));
                s.table.select("10");
            }),
            ("info-limit", |s| {
                let ratio = proto::Ratio::new(200).expect("a ratio");
                let done = TorrentInfo {
                    uploaded_bytes: 3800 * MIB,
                    ..t(
                        6,
                        "Cosmos Laundromat (2015)",
                        Status::Done { reached: Some(proto::Reached::Ratio(ratio)) },
                        1900 * MIB,
                        1900 * MIB,
                        0,
                        0,
                        0,
                    )
                };
                s.service = Box::new(few_rows().with_torrents(vec![done]));
                s.table.select("6");
                s.prefs.set_tab(model::layout::PaneTab::Info);
            }),
            // A long name and path: shortened in the middle, whole on hover.
            ("long-name", |s| {
                s.service = Box::new(
                    few_rows()
                        .with_torrents(vec![TorrentInfo {
                            uploaded_bytes: 960 * MIB,
                            ..t(
                                5,
                                "Big Buck Bunny (2008) 4K 60 fps, the Blender Open Movie Complete Edition, Director's Cut with Commentary and Behind the Scenes, Extended Edition.mkv",
                                Status::Downloading,
                                0,
                                0,
                                24 * MIB,
                                0,
                                18,
                            )
                        }])
                        .with_files(5, few_rows_files()),
                );
                s.table.select("5");
            }),
            ("few-rows", |s| {
                s.service = Box::new(few_rows());
                s.table.select("4");
            }),
            ("info-tab", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.prefs.set_tab(model::layout::PaneTab::Info);
            }),
            ("columns-reordered", |s| {
                use model::layout::{Column, ColumnOrder};
                s.service = Box::new(demo_service());
                let order =
                    [Column::Name, Column::Status, Column::Progress, Column::Size, Column::Eta, Column::Down, Column::Up, Column::Peers];
                s.prefs.kept.order = ColumnOrder::try_from(order.to_vec()).expect("every column");
            }),
            // A torrent's files, with the Accent changed in both Appearances (#417).
            ("colors-accent", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.pane.files.select("1");
                for (half, hex) in [(preset::Appearance::Light, "#c2410c"), (preset::Appearance::Dark, "#fb923c")] {
                    let mut look = s.looks.borrow().preset().get(half).clone();
                    look.settings.set("accent", theme::setting::Value::Color(theme::setting::Rgb::parse(hex)));
                    s.looks.borrow_mut().set(half, look);
                }
            }),
            ("pane-dragged", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
                s.prefs.kept.pane_height = Some(330.0);
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
            // Rename (#331) of Big Buck Bunny, a folder of files: `rename` has its field open, the
            // name selected; `renamed` shows it renamed, its details its original name;
            // `rename-refused` a name a file can't have for Ubuntu's (a row in sight at the smallest
            // window), said in a tooltip under the field. `renames` is the demo as it is, for a
            // script to rename one in.
            ("renames", |s| {
                s.service = Box::new(demo_service());
                s.table.select("4");
            }),
            ("rename", |s| {
                listed(s, demo_service());
                s.table.select("4");
                model::renaming::open(s, 4);
            }),
            // A torrent of one file: the part before its extension selected.
            ("rename-file", |s| {
                let iso = File {
                    index: 0,
                    path: "debian-13.7.0-amd64-netinst.iso".into(),
                    size_bytes: 756 * MIB,
                    progress_bytes: 412 * MIB,
                    priority: Priority::Normal,
                    missing: false,
                    on_disk: false,
                };
                listed(s, demo_service().with_files(0, vec![iso]));
                s.table.select("0");
                model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
                for incoming in s.service.poll(0.1) {
                    if let Incoming::Live(live) = incoming {
                        s.apply(live);
                    }
                }
                model::renaming::open(s, 0);
            }),
            ("renamed", |s| {
                listed(s, demo_service());
                s.table.select("4");
                let name = proto::FileName::new("Big Buck Bunny (Director's Cut)").expect("a name");
                model::rename(s, 4, name);
            }),
            ("rename-refused", |s| {
                listed(s, demo_service());
                s.table.select("1");
                model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
                model::renaming::open(s, 1);
                s.rename.field = rune::TextEdit::new("Ubuntu: Noble Numbat");
                s.rename.field.focused = true;
                model::renaming::submit(s);
            }),
            // Moves (#24): each takes 3 s; into Clash a file of each is already, into Full the disk fills
            // halfway. `moving` has one under way, `clash` its dialog open, `move-failed` the error.
            ("moves", |s| s.service = Box::new(moves())),
            ("moving", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::start(s, vec![1], archive(""));
            }),
            ("clash", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::start(s, vec![1], archive("/Clash"));
            }),
            ("move-failed", |s| {
                s.service = Box::new(moves().moves_take(0.0));
                s.table.select("1");
                model::moving::start(s, vec![1], archive("/Full"));
            }),
            // Moves of three torrents (#270): the first under way, the others Waiting to move.
            ("moves-several", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::start(s, vec![0, 1, 2], archive(""));
            }),
            // The same into a folder holding a file of each: the first one's dialog, asking to apply its
            // answer to the remaining two.
            ("clashes", |s| {
                s.service = Box::new(moves());
                s.table.select("1");
                model::moving::start(s, vec![0, 1, 2], archive("/Clash"));
            }),
            // A Move into a folder where another torrent of the same name keeps its files (#270): the
            // dialog offers only Show in list, Cancel and Rename…, and says where it keeps them.
            ("clash-another", |s| {
                let ubuntu = t(5, "ubuntu-24.04.3-desktop-amd64.iso", Status::Seeding, 6100 * MIB, 6100 * MIB, 0, 0, 2);
                s.service = Box::new(moves().with_torrents(vec![ubuntu]).kept_in(5, format!("{ARCHIVE}/Isos")));
                s.table.select("1");
                model::moving::start(s, vec![1], archive("/Isos"));
            }),
            // Three torrents, one of them still fetching its info: two move, and the notice lists the one that can't.
            ("moved-some", |s| {
                s.service = Box::new(moves().moves_take(0.0));
                s.table.select("1");
                model::moving::start(s, vec![0, 3, 4], archive(""));
            }),
            // Rename… on a name longer than the header's room for it (it scrolls inside the field,
            // as wide as the header's name may be), a clash of several files, the Add tooltip, a
            // refused add as a notice, and Move to ▸ with recent folders in the palette
            // (`moves-recent`, for a script to open the row menu's).
            ("rename-long", |s| {
                let iso = File {
                    index: 0,
                    path: LONG_ISO.into(),
                    size_bytes: 6100 * MIB,
                    progress_bytes: 6100 * MIB,
                    priority: Priority::Normal,
                    missing: false,
                    on_disk: false,
                };
                let long = t(5, LONG_ISO, Status::Seeding, 6100 * MIB, 6100 * MIB, 0, 0, 3);
                listed(s, demo_service().with_torrents(vec![long]).with_files(5, vec![iso]));
                s.table.select("5");
                model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
                for incoming in s.service.poll(0.1) {
                    if let Incoming::Live(live) = incoming {
                        s.apply(live);
                    }
                }
                model::renaming::open(s, 5);
            }),
            // The same field on a name shorter than the header's room.
            ("rename-short", |s| {
                listed(s, demo_service());
                s.table.select("4");
                model::follow_selection(&mut *s.service, &s.table, &s.torrents, &mut s.pane);
                model::renaming::open(s, 4);
            }),
            ("clash-files", |s| {
                s.service = Box::new(moves().holding_all(format!("{ARCHIVE}/Films")));
                s.table.select("4");
                model::moving::start(s, vec![4], archive("/Films"));
            }),
            // A clashing file whose path is longer than a line of the dialog (#420): it wraps, whole.
            ("clash-long-path", |s| {
                let folder = format!("{ARCHIVE}/Films and series/Documentaries about the history of the long and winding road/Season 01");
                s.service = Box::new(moves().holding_all(folder.clone()));
                s.table.select("4");
                model::moving::start(s, vec![4], archive(&folder[ARCHIVE.len()..]));
            }),
            ("add-note", |s| {
                s.service = Box::new(demo_service());
                s.magnet = rune::TextEdit::new("hello");
                s.add_note = Some(model::AddNote::about(&s.magnet, display::link_problem(client::LinkProblem::NotALink)));
            }),
            ("add-refused", |s| {
                s.service = Box::new(failing_adds().adds_after(0.0, 0.0));
                s.magnet = rune::TextEdit::new(NOBODY);
                model::submit_url(&mut *s.service, &s.magnet, &mut s.add_note, &mut s.refocus_link, true);
            }),
            // Nothing typed: every entry, the list scrolling.
            ("palette-empty", |s| {
                listed(s, moves());
                s.table.select("1");
                recent_demo(s);
                model::palette::open(s);
                s.palette.query.focused = true;
            }),
            ("move-palette", |s| {
                listed(s, moves());
                s.table.select("1");
                recent_demo(s);
                model::palette::open(s);
                s.palette.query = rune::TextEdit::new("move");
                s.palette.query.focused = true;
            }),
            // Moves, with recent folders: for a script to move one to a recent folder.
            ("moves-recent", |s| {
                s.service = Box::new(moves());
                recent_demo(s);
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
                    fn exists(&self, _: &std::path::Path) -> bool {
                        true
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
            ("records-set-aside", |s| {
                s.service =
                    Box::new(demo_service().with_records_set_aside("/Users/me/Library/Application Support/Phantom/records.invalid.db"));
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
                for notice in model::undo_notices(&Err(refused), &s.undos) {
                    s.bar.tell(notice);
                }
            }),
            // An undo refused because another torrent has the files now (#371).
            ("cant-undo-place", |s| {
                s.service = Box::new(demo_service());
                let undo = proto::Undoable {
                    seq: 1,
                    action: proto::UndoAction::Remove,
                    torrents: vec![proto::Named { id: 9, name: "Big Buck Bunny (2008)".into() }],
                };
                let why = proto::CantUndo::AnotherHasFiles {
                    other: proto::Named { id: 10, name: "Big Buck Bunny (2008) [1080p]".into() },
                    path: proto::AbsolutePath::new(
                        "/Users/me/Downloads/Big Buck Bunny (2008)/Big_Buck_Bunny_1080p_surround_FrostWire.com.avi",
                    )
                    .expect("absolute"),
                    magnet: Some("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big%20Buck%20Bunny".into()),
                };
                let refused = client::Failure::Refused(proto::Refusal::Undo { undo, why, stranded: Vec::new() });
                for notice in model::undo_notices(&Err(refused), &s.undos) {
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
            // The notices of #87: an action's, with its Undo in the bar and the list; notices of
            // each severity, the newest a receipt and the worst an error.
            ("removed", |s| {
                listed(s, demo_service());
                s.table.select("4");
                model::remove_selected(s, false);
            }),
            // Two Removes: Undo and the pill, both.
            ("removed-two", |s| {
                listed(s, demo_service());
                for row in ["4", "3"] {
                    s.table.select(row);
                    model::remove_selected(s, false);
                }
            }),
            // The same while the app was replaced by a newer build: Undo beside Restart.
            ("removed-outdated", |s| {
                listed(s, demo_service());
                s.table.select("4");
                model::remove_selected(s, false);
                s.connection = Connection::Outdated;
            }),
            // The list, for a script to remove a torrent in and undo it from the bar, then drop
            // files on it (`bad…` ones fail).
            ("removes", |s| {
                listed(s, failing_adds());
            }),
            ("mixed", |s| {
                s.service = Box::new(demo_service());
                let failed = vec![model::AddFailure { what: "bad-1.torrent".into(), why: "Not a torrent file.".into(), request: None }];
                s.bar.tell(model::FailedAdds { total: 3, failed }.notice());
                s.bar.tell(model::settings::reset_notice(&["app_icon".into()], "gui.json").expect("a reset"));
                s.bar.tell(Notice::new(display::copied_magnet()).brief().with_names("Big Buck Bunny (2008)"));
            }),
            // The buttons of a failure's notice (#367): Try again after a failed add.
            ("notice-add-failed", |s| {
                s.service = Box::new(demo_service());
                let request = AddRequest::link(NOBODY).expect("a magnet link");
                let failed = vec![model::AddFailure {
                    what: "Nobody".into(),
                    why: "No peers have this torrent".into(),
                    request: Some(model::Asked { request, to: None, by_field: false }),
                }];
                s.bar.tell(model::FailedAdds { total: 1, failed }.notice());
            }),
            ("notice-delete-failed", |s| {
                listed(s, demo_service());
                let list = notice::Listing::of_entries([("Big Buck Bunny (2008)", &"Some of its files are locked".into())]);
                s.bar.tell(
                    Notice::error(display::commands_failed(proto::Action::Delete, 1, 1))
                        .with_listing(list)
                        .with_actions(vec![model::retry::command(s, &[1], proto::Action::Delete)]),
                );
            }),
            ("notice-resume-failed", |s| {
                listed(s, demo_service());
                let list = notice::Listing::of_entries([("Sintel (2010)", &"The disk is full".into())]);
                s.bar.tell(
                    Notice::error(display::commands_failed(proto::Action::Resume, 1, 2))
                        .with_listing(list)
                        .with_actions(vec![model::retry::command(s, &[2], proto::Action::Resume)]),
                );
            }),
            ("notice-move-partial", |s| {
                listed(s, demo_service());
                let to = proto::AbsolutePath::new("/Users/me/Films").expect("absolute");
                let list = notice::Listing::of_entries([("Sintel (2010)", &"Cancelled".into())]);
                let actions = model::retry::moving(s, &[1], to);
                s.bar.tell(Notice::warning(display::moved_of(1, 2)).with_listing(list).with_actions(actions));
            }),
            ("notice-rename-clash", |s| {
                listed(s, demo_service());
                let path = proto::AbsolutePath::new("/Users/me/Downloads/Big Buck Bunny (2008) (1)").expect("absolute");
                let why = display::refused_why(&proto::TorrentRefusal::NameTaken { path: path.clone() });
                let list = notice::Listing::of_entries([("Big Buck Bunny (2008)", &why)]);
                let way = notice::NoticeAction::reveal(std::path::Path::new(&path.display().to_string()));
                s.bar.tell(
                    Notice::error(display::commands_failed(proto::Action::Rename, 1, 1))
                        .with_listing(list)
                        .with_actions(vec![model::retry::rename(s, 1), way]),
                );
            }),
            ("notice-restart-failed", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(
                    Notice::error(display::cant_restart())
                        .with_text(display::sentence("no such file or directory"))
                        .with_actions(vec![model::retry::restart()]),
                );
            }),
            ("notice-setting-failed", |s| {
                s.service = Box::new(demo_service());
                let list = display::Listing::new("Listen port", &display::sentence("the torrents didn't start again"));
                s.bar.tell(
                    Notice::error(display::setting_not_changed())
                        .with_listing(list)
                        .with_actions(vec![model::retry::open_settings("listen_port")]),
                );
            }),
            // A failed Open of a file of torrent 1: Show folder, then Check.
            ("notice-open-check", |s| {
                struct NotThere;
                impl platform::Finder for NotThere {
                    fn show(&mut self, _: &std::path::Path, _: platform::Show) -> Result<(), platform::ShowError> {
                        Err(platform::ShowError::NotOnDisk)
                    }
                    fn exists(&self, _: &std::path::Path) -> bool {
                        true
                    }
                }
                listed(s, demo_service());
                s.finder = Box::new(NotThere);
                platform::show_of(s, std::path::Path::new("/Users/me/Downloads/Big Buck Bunny.mkv"), platform::Show::Open, Some(1));
            }),
            // The popover's width follows the widest notice: a narrow one alone, a path, and a list
            // of both on two pages.
            ("notice-width-narrow", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(Notice::new(display::copied_magnet()).brief());
            }),
            ("notice-width-path", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(Notice::error("Can't open the file").with_listing(display::show_failed_listing(
                    std::path::Path::new("/Users/me/Downloads/Pack/a.mkv"),
                    "It isn't on disk",
                )));
            }),
            // A long path and a long reason in the list: both wrap whole, at any window size; the line is short.
            ("notice-long", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(Notice::error("Couldn't add the torrent").with_listing(display::show_failed_listing(
                    std::path::Path::new(
                        "/Volumes/An External Drive With a Long Name/Downloads/A Folder Deep Down/And Deeper/And Deeper Still/file.torrent",
                    ),
                    "The folder it was going into was moved, renamed or deleted while it waited",
                )));
            }),
            ("notice-width-mixed", |s| {
                s.service = Box::new(demo_service());
                s.bar.tell(Notice::error("Can't open the file").with_listing(display::show_failed_listing(
                    std::path::Path::new("/Users/me/Downloads/Pack/a.mkv"),
                    "It isn't on disk",
                )));
                s.bar.tell(Notice::new(display::copied_magnet()).brief());
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
        families: FAMILIES,
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
            ("settings-no-presets", |s| settings_scene(s, Tab::Appearance, demo_settings(false))),
            // Save as… open with a name typed, and with a name taken: its question, the row as tall.
            ("settings-save-as", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.saved.set(demo_presets());
                model::settings::presets::save_as(s);
                s.sheet.as_mut().unwrap().save_as.as_mut().unwrap().name = rune::TextEdit::new("Night");
            }),
            ("settings-replace", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.saved.set(demo_presets());
                model::settings::presets::save_as(s);
                let save_as = s.sheet.as_mut().unwrap().save_as.as_mut().unwrap();
                save_as.name = rune::TextEdit::new("Night");
                save_as.replace = Some(theme::saved::Name::parse("Night").expect("a name"));
            }),
            // The Active look's sheet: one theme, every row joined (Dark in both); a fresh install's,
            // its Tone split; the Dark half the stand-in theme (debug builds), the Theme row split and
            // each theme's rows in its own column; and that with ☾'s Theme pop-up open.
            ("settings-sheet-joined", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                let dark = LookPreset::fresh().dark;
                s.looks.borrow_mut().set(preset::Appearance::Light, dark);
                model::settings::presets::edit_active(s);
            }),
            ("settings-sheet-split", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                model::settings::presets::edit_active(s);
            }),
            ("settings-sheet-themes", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                stand_in_dark(s);
                model::settings::presets::edit_active(s);
            }),
            ("settings-theme-menu", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                stand_in_dark(s);
                model::settings::presets::edit_active(s);
                model::settings::toggle_menu(s, model::settings::THEME, model::settings::Side::Only(preset::Appearance::Dark));
            }),
            // The Active look's sheet: Look and Colors; "Every color" open, Faint text set by hand
            // in Light and Muted text in Dark; the Accent changed in Light (#417).
            ("settings-colors", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                model::settings::presets::edit_active(s);
            }),
            ("settings-every-color", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                set_colors(s, &[(preset::Appearance::Light, "faint_text", "#a3a6ab"), (preset::Appearance::Dark, "muted_text", "#c9b99a")]);
                model::settings::presets::edit_active(s);
                model::settings::toggle_section(s, "every-color");
            }),
            // The picker on a hand-set Faint text in Dark's half: two warnings under it (#417).
            ("settings-faint-picker", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                set_colors(s, &[(preset::Appearance::Light, "faint_text", "#a3a6ab"), (preset::Appearance::Dark, "faint_text", "#5f656f")]);
                model::settings::presets::edit_active(s);
                model::settings::toggle_section(s, "every-color");
                model::settings::open_picker(s, model::settings::Side::Only(preset::Appearance::Dark), "faint_text");
            }),
            // The picker on a pure blue Accent in Light's half: no seam at that hue (#454).
            ("settings-blue-picker", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                set_colors(s, &[(preset::Appearance::Light, "accent", "#0000ff")]);
                model::settings::presets::edit_active(s);
                model::settings::open_picker(s, model::settings::Side::Only(preset::Appearance::Light), "accent");
            }),
            // The same, the picker still closed: for a script to scroll to the swatch and open it.
            ("settings-faint-sheet", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                set_colors(s, &[(preset::Appearance::Light, "faint_text", "#a3a6ab"), (preset::Appearance::Dark, "faint_text", "#5f656f")]);
                model::settings::presets::edit_active(s);
                model::settings::toggle_section(s, "every-color");
            }),
            ("settings-accent-set", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                set_colors(s, &[(preset::Appearance::Light, "accent", "#6526d4")]);
                model::settings::presets::edit_active(s);
            }),
            // The Active look's sheet with Motion's dropdown open.
            ("settings-motion-menu", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                model::settings::presets::edit_active(s);
                model::settings::toggle_menu(s, theme::phantom::MOTION, model::settings::Side::Both);
            }),
            // The App icon's sheet: a designed pair chosen; an image added and used in Dark.
            ("settings-icons", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.app_icon.choice = s.app_icon.manifest.as_ref().and_then(|m| m.pairs().get(1)).map(|p| p.icons());
                model::settings::edit_icons(s);
            }),
            // The App icon's sheet: a pair's Light icon in Light, the Dark half following the theme.
            ("settings-icons-follow", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                let light = s.app_icon.manifest.as_ref().and_then(|m| m.pairs().get(1)).map(|p| p.light.clone()).unwrap_or_default();
                s.app_icon.choice = Some(platform::app_icon::Icons::Pair { dark: platform::app_icon::FOLLOW.into(), light });
                model::settings::edit_icons(s);
            }),
            ("settings-icons-added", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.app_icon.choice = s.app_icon.manifest.as_ref().and_then(|m| m.pairs().get(1)).map(|p| p.icons());
                model::settings::edit_icons(s);
                model::settings::add_image(s, &platform::app_icon::manifest_dir().join("../03-invisible-thief.png"));
                let id = s.app_icon.added.last().cloned().unwrap_or_default();
                model::settings::choose_chip(s, Some(&id), Some(preset::Appearance::Dark));
            }),
            ("settings-icon-row", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.app_icon.choice = s.app_icon.manifest.as_ref().and_then(|m| m.pairs().get(1)).map(|p| p.icons());
            }),
            // Two icons that aren't a designed pair: the row's caption names both.
            ("settings-icon-row-mixed", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.app_icon.choice = Some(platform::app_icon::Icons::Pair { dark: "phantom".into(), light: "full-moon".into() });
            }),
            // One icon in both halves: the row shows it once.
            ("settings-icon-row-one", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                s.app_icon.choice = Some(platform::app_icon::Icons::One { icon: "full-moon".into() });
            }),
            // Phantom's icon customized (#417): the row with its Customize…, Light following the
            // look, Dark a blue tile set by hand.
            ("settings-icon-row-tinted", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                tint(s, &[(preset::Appearance::Light, Tint::Follow), (preset::Appearance::Dark, Tint::Color(BLUE))]);
            }),
            // The Customize sheet as it first opens: Original in both halves.
            ("settings-customize", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                model::settings::customize::open(s);
            }),
            // Its picker open on Light's Tile.
            ("settings-customize-picker", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                model::settings::customize::open(s);
                model::settings::customize::open_picker(s, preset::Appearance::Light, "tile");
            }),
            // Light following the look, Dark a blue set by hand, its picker open.
            ("settings-customize-follow", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                tint(s, &[(preset::Appearance::Light, Tint::Follow), (preset::Appearance::Dark, Tint::Color(BLUE))]);
                model::settings::customize::open(s);
                model::settings::customize::open_picker(s, preset::Appearance::Dark, "tile");
            }),
            // The Dark look another theme's: Dark's Follow the look is off (why on hover).
            ("settings-customize-other", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                stand_in_dark(s);
                tint(s, &[(preset::Appearance::Light, Tint::Follow)]);
                model::settings::customize::open(s);
                model::settings::customize::open_picker(s, preset::Appearance::Dark, "tile");
            }),
            // The tile in a few colors set by hand, the same in both halves.
            ("settings-customize-orange", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                let c = Tint::Color(theme::setting::Rgb([0xa7, 0x58, 0x31]));
                tint(s, &[(preset::Appearance::Light, c), (preset::Appearance::Dark, c)]);
                model::settings::customize::open(s);
            }),
            ("settings-customize-purple", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                let c = Tint::Color(theme::setting::Rgb([0x69, 0x33, 0xc2]));
                tint(s, &[(preset::Appearance::Light, c), (preset::Appearance::Dark, c)]);
                model::settings::customize::open(s);
            }),
            ("settings-customize-green", |s| {
                settings_scene(s, Tab::Appearance, demo_settings(false));
                let c = Tint::Color(theme::setting::Rgb([0x2f, 0x9e, 0x6a]));
                tint(s, &[(preset::Appearance::Light, c), (preset::Appearance::Dark, c)]);
                model::settings::customize::open(s);
            }),
        ],
        families: FAMILIES,
        ..AppSpec::new("Settings", SETTINGS_SIZE, SettingsWindow::detached, views::settings::view)
    }
}

/// The Active look's Dark half the stand-in theme, in a debug build; unchanged in a release one,
/// which has no stand-in.
/// Each color `(half, key, hex)` set by hand in the Active look.
fn set_colors(s: &mut SettingsWindow, colors: &[(preset::Appearance, &str, &str)]) {
    for (half, key, hex) in colors {
        let mut look = s.looks.borrow().preset().get(*half).clone();
        look.settings.set(key, theme::setting::Value::Color(theme::setting::Rgb::parse(hex)));
        s.looks.borrow_mut().set(*half, look);
    }
}

/// Phantom's Tile as `tints` set it in each half, drawn (#417).
fn tint(s: &mut SettingsWindow, tints: &[(preset::Appearance, Tint)]) {
    for (half, tint) in tints {
        s.app_icon.set_tint("phantom", *half, theme::phantom::app_icon::TILE, *tint);
    }
    s.app_icon.redraw(&s.looks.borrow());
}

/// A blue for the scenes' Tile set by hand.
const BLUE: theme::setting::Rgb = theme::setting::Rgb([0x3f, 0x7e, 0xd0]);

fn stand_in_dark(s: &mut SettingsWindow) {
    if let Some(theme) = theme::find("stand-in") {
        let look = theme::ThemeLook { theme, settings: theme::setting::Values::defaults(theme.settings()) };
        s.looks.borrow_mut().set(preset::Appearance::Dark, look);
    }
}

/// Sans and mono are both Inter: a number cell is set in mono with tabular figures, and the columns
/// are as wide as Inter's. (Rune's default mono is the system's monospaced face, a third wider.)
const FAMILIES: (&str, &str) = ("Inter", "Inter");

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
    // The toolbar is in the titlebar band (#33): only its empty parts move the window.
    let main_app = App::new(spec()).chrome(rune::Chrome { drag_regions: true, ..rune::Chrome::DEFAULT });
    let mut app = main_app.standard_menus().keep_running().on_open_url(platform::open_url).on_focus(on_focus).on_reopen(on_reopen);
    let handle = app.handle();
    let looks = looks(headless);
    #[cfg(target_os = "macos")]
    if !headless {
        platform::menu_bar::install(handle.clone());
        let opened = handle.clone();
        platform::open_documents::listen(move |paths| opened.update(move |s, tx| platform::open_files(s, &paths, tx)));
        platform::app_icon::appkit::install(looks.app_icons());
    }
    // The two windows, each with its own state, and the ways from one to the other (`windows`).
    let settings_app = App::new(settings_spec())
        .chrome(rune::Chrome { traffic_lights: Some(ui::TRAFFIC_LIGHTS), ..rune::Chrome::DEFAULT })
        .on_close(settings_on_close)
        .on_focus(settings_on_focus);
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
        let wake_for_disk = handle.clone();
        let wake = move || handle.wake();
        let service: Box<dyn Service> = if rune::tooling::headless_run().is_none() {
            Box::new(SocketService::start(wake))
        } else if std::env::var_os("PHANTOM_HEADLESS_SERVICE").is_some() {
            Box::new(SocketService::start_loaded(wake))
        } else {
            Box::new(MemoryService::new())
        };
        let prefs = window_prefs(rune::tooling::headless_run().is_some());
        let mut app = Phantom { window, looks, settings: to_settings, inbox: main_inbox, prefs, ..Phantom::new(service) };
        // Headless runs keep the fake finder (nothing opens) and keep no recent folders.
        if rune::tooling::headless_run().is_none() {
            app.finder = Box::new(platform::SystemFinder::new(move || wake_for_disk.wake()));
            app.relauncher = Box::new(platform::SystemRelauncher);
            app.recent = recent_folders();
        }
        match &reset {
            Ok(reset) => {
                if let Some(notice) = model::settings::reset_notice(reset, "gui.json") {
                    app.bar.tell(notice);
                }
            }
            Err(e) => {
                let file = proto::data_dir().join("gui.json");
                let notice = Notice::error(display::cant_read_gui_json()).with_listing(display::gui_json_listing(&file, &e.to_string()));
                app.bar.tell(notice.with_actions(vec![notice::NoticeAction::reveal(&file)]));
            }
        }
        app
    })
    .run();
}
