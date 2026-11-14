//! The UI driven like a person would, against `MemoryService` on Rune's virtual clock.

mod generated;
mod settings;

use generated::{entry_keys, list_cells, rect_of};

use std::path::Path;

use proto::{Action, Content, File, Priority, Status, TorrentCommand, TorrentId, TorrentInfo, Unavailable};
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use rune_kit::{SortDir, TableState};

use crate::model::{Phantom, details_torrent, reveal_target, selected_for, selected_torrents, shown_torrent, sort};
use crate::platform::{open_files, open_link, open_torrent_files, open_url};
use crate::service::{DiskChange, MemoryService};
use crate::shortcuts::{self, Command};
use crate::{on_key, poll, views};

const MAGNET: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000aa&dn=Big+Buck+Bunny";
const FRAME: f64 = 1.0 / 60.0;

fn app(service: MemoryService) -> Runtime<Phantom> {
    let env = Env { system_dark: true, scale: 2.0 };
    let mut rt = Runtime::new(Phantom::new(Box::new(service)), views::view, (960.0, 540.0), env);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    rt.frame(FRAME);
    rt
}

/// Two frames: handlers run during a frame, after that frame's `poll`, so the service's answer
/// to an action lands on the next one (as with the real service, only sooner).
fn settle(rt: &mut Runtime<Phantom>) {
    rt.frame(FRAME);
    // Springs on layout (the error list's panel) land; the app clock moves only by the frames.
    rt.settle_all();
    rt.frame(FRAME);
}

fn tap(rt: &mut Runtime<Phantom>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// What the open menu keyed `menu` draws, in order: a choice's label, `None` for a separator. `None`
/// when no such menu is open.
fn drawn_menu(rt: &Runtime<Phantom>, menu: &str) -> Option<Vec<Option<String>>> {
    let elements = rt.elements();
    let surface = elements.iter().find(|e| !e.exiting && e.key.as_deref() == Some(&format!("{menu}-menu")))?;
    let inside = |e: &&rune::ElementInfo| !e.exiting && e.path.starts_with(&format!("{}/", surface.path));
    Some(
        elements
            .iter()
            .filter(inside)
            .filter_map(|e| match (e.role, e.key.as_deref()) {
                ("menuitem", _) => Some(e.label.clone()),
                (_, Some(key)) if key.starts_with("sep") => Some(None),
                _ => None,
            })
            .collect(),
    )
}

/// Clicks the choice of the open menu `menu` whose label starts with `label`.
fn pick_choice(rt: &mut Runtime<Phantom>, menu: &str, label: &str) {
    pick_choice_then(rt, menu, label, settle);
}

/// [`pick_choice`], with `after` for the frames after scrolling to it and after the click: the
/// generated test's take no time on the app clock.
fn pick_choice_then(rt: &mut Runtime<Phantom>, menu: &str, label: &str, after: fn(&mut Runtime<Phantom>)) {
    let elements = rt.elements();
    let item = elements
        .iter()
        .find(|e| !e.exiting && e.role == "menuitem" && e.path.contains(menu) && e.label.as_deref().is_some_and(|l| l.starts_with(label)))
        .unwrap_or_else(|| panic!("no choice {label:?} in {menu}: {:?}", drawn_menu(rt, menu)));
    // A menu that doesn't fit its window scrolls: scroll to the choice first.
    let key = item.key.clone().expect("a choice has a key");
    rt.update(|_, tx| tx.scroll_into_view(key));
    after(rt);
    let item = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "menuitem" && e.label.as_deref().is_some_and(|l| l.starts_with(label)))
        .unwrap();
    let (x, y) = (item.rect[0] + item.rect[2] / 2.0, item.rect[1] + item.rect[3] / 2.0);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    after(rt);
}

fn wait(rt: &mut Runtime<Phantom>, secs: f64) {
    for _ in 0..(secs / FRAME) as usize {
        rt.frame(FRAME);
    }
}

fn torrent(id: TorrentId, name: &str) -> TorrentInfo {
    TorrentInfo {
        id,
        info_hash: format!("{id:040x}"),
        name: Some(name.into()),
        status: Status::Downloading,
        content: Some(Content { size_bytes: 1 << 30, full_size_bytes: 1 << 30, progress_bytes: 0 }),
        uploaded_bytes: 0,
        download_bytes_per_sec: 1 << 20,
        upload_bytes_per_sec: 0,
        live_peers: 3,
        trackers: Vec::new(),
        location: None,
        actions: Vec::new(),
    }
}

const MIB: u64 = 1 << 20;

/// Torrent 0 ("a", one file) and torrent 1 ("Album", three files of 100, 200 and 300 MiB).
fn with_an_album() -> MemoryService {
    let file = |index, path: &str, mib: u64| File {
        index,
        path: path.into(),
        size_bytes: mib * MIB,
        progress_bytes: 0,
        priority: Priority::Normal,
        missing: false,
    };
    MemoryService::new()
        .with_torrents(vec![torrent(0, "a"), torrent(1, "Album")])
        .with_files(1, vec![file(0, "01 Intro.flac", 100), file(1, "02 Song.flac", 200), file(2, "Cover/front.jpg", 300)])
}

fn shift(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods { shift: true, ..Default::default() } }
}

fn files(rt: &Runtime<Phantom>) -> Vec<(String, Priority)> {
    let details = rt.state.pane.details.as_ref().expect("details");
    details.files.iter().map(|f| (f.path.to_string_lossy().into_owned(), f.priority)).collect()
}

fn find_torrent(rt: &Runtime<Phantom>, id: TorrentId) -> &TorrentInfo {
    rt.state.torrents.iter().find(|t| t.id == id).unwrap()
}

/// A button can be pressed: rune-kit takes a disabled one out of the focus order.
fn enabled(rt: &Runtime<Phantom>, key: &str) -> bool {
    let elements = rt.elements();
    let button = elements.iter().position(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key:?}"));
    elements.iter().any(|e| e.parent == Some(button) && e.focusable)
}

/// The element keyed `key` is drawn where a click would reach it, not scrolled out of the list.
fn on_screen(rt: &Runtime<Phantom>, key: &str) -> bool {
    let Some((x, y)) = rt.find(key) else { return false };
    rt.element_at(x, y).is_some_and(|picked| picked.path().contains(key))
}

/// What the error list draws, an entry at a time: its short line, then what its view adds.
fn listed(rt: &Runtime<Phantom>) -> Vec<Vec<String>> {
    generated::drawn_entries(rt)
}

fn notice(rt: &Runtime<Phantom>) -> Option<&str> {
    rt.state.notice.as_ref().map(|n| n.short.as_str())
}

/// Presses the shortcut's first key, as the table has it.
fn shortcut(rt: &mut Runtime<Phantom>, command: Command) {
    rt.key(shortcuts::keys(command)[0].event());
    settle(rt);
}

/// Clicks file row `row`'s priority in the pane.
fn click_priority(rt: &mut Runtime<Phantom>, row: usize) {
    tap(rt, &format!("files/viewport/rows/{row}/priority"));
}

/// Right-clicks torrent `id`'s row.
fn right_click_row(rt: &mut Runtime<Phantom>, id: TorrentId) {
    let (x, y) = rt.find(&format!("rows/{id}/name")).unwrap_or_else(|| panic!("no row {id}"));
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
}

/// What torrent `id`'s row menu offers, then closed again.
fn row_menu(rt: &mut Runtime<Phantom>, id: TorrentId) -> Vec<String> {
    right_click_row(rt, id);
    let menu = drawn_menu(rt, "row-menu").expect("a row menu").into_iter().flatten().collect();
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(rt);
    menu
}

/// Picks the item starting with `label` from torrent `id`'s row menu.
fn row_pick(rt: &mut Runtime<Phantom>, id: TorrentId, label: &str) {
    right_click_row(rt, id);
    pick_choice(rt, "row-menu", label);
}

#[test]
fn a_submitted_magnet_fetches_info_then_downloads() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);

    assert_eq!(rt.state.magnet.text, "", "the field empties once the add succeeded");
    assert_eq!(rt.state.torrents.len(), 1);
    let t = &rt.state.torrents[0];
    assert_eq!(t.status, Status::FetchingInfo);
    assert_eq!(t.name.as_deref(), Some("Big Buck Bunny"), "named from the magnet's dn=");
    assert_eq!(notice(&rt), Some("Added Big Buck Bunny"));
    let id = t.id;

    wait(&mut rt, 3.0);
    let t = &rt.state.torrents[0];
    assert_eq!(t.id, id, "the same torrent, once its info arrived");
    assert_eq!(t.status, Status::Downloading);
    assert!(t.content.unwrap().progress_bytes > 0, "and it's downloading");
}

#[test]
fn pasting_with_no_field_focused_adds_the_magnet() {
    let mut rt = app(MemoryService::new());
    rt.paste(MAGNET);
    settle(&mut rt);
    assert_eq!(rt.state.torrents.len(), 1);
}

#[test]
fn something_that_is_not_a_link_shows_an_error() {
    let mut rt = app(MemoryService::new());
    rt.paste("hello");
    settle(&mut rt);
    assert!(rt.state.notice.is_none(), "not in the status bar");
    assert_eq!(listed(&rt), vec![["Couldn't add the torrent", "hello", "Not a magnet link or .torrent URL"]], "in the error list");
    assert!(rt.state.torrents.is_empty());
}

#[test]
fn adding_a_magnet_already_in_the_list_selects_that_torrent() {
    let torrents: Vec<_> = (0..30).map(|id| torrent(id, &format!("t{id}"))).collect();
    let hash = torrents[29].info_hash.clone();
    let mut rt = app(MemoryService::new().with_torrents(torrents));
    tap(&mut rt, "rows/0");
    assert!(!on_screen(&rt, "rows/29"), "the last row starts out of view");

    rt.paste(&format!("magnet:?xt=urn:btih:{hash}&dn=Another+name"));
    settle(&mut rt);
    wait(&mut rt, 1.0);
    assert_eq!(rt.state.torrents.len(), 30, "no second row");
    assert_eq!(notice(&rt), Some("t29 is already in the list"));
    assert_eq!(selected_torrents(&rt.state.table), [29], "the existing row is selected instead");
    assert!(on_screen(&rt, "rows/29"), "and scrolled into view");
}

#[test]
fn pause_and_resume_act_on_the_selected_rows_only() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/1");
    tap(&mut rt, "pause");
    let status = |rt: &Runtime<Phantom>, id| rt.state.torrents.iter().find(|t| t.id == id).unwrap().status.clone();
    assert_eq!(status(&rt, 1), Status::Paused);
    assert_eq!(status(&rt, 0), Status::Downloading, "the other row is untouched");

    tap(&mut rt, "resume");
    assert_eq!(status(&rt, 1), Status::Downloading);
}

#[test]
fn a_torrent_paused_while_fetching_info_lands_paused_and_resumes() {
    let mut rt = app(MemoryService::new());
    rt.paste(MAGNET);
    settle(&mut rt);
    let id = rt.state.torrents[0].id;
    tap(&mut rt, &format!("rows/{id}"));
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].status, Status::FetchingInfo, "still fetching");
    assert!(rt.state.torrents[0].actions.contains(&Action::Resume));

    wait(&mut rt, 3.0);
    assert_eq!(rt.state.torrents[0].status, Status::Paused, "its info arrived, and it didn't download");
    assert_eq!(rt.state.torrents[0].content.unwrap().progress_bytes, 0);
    tap(&mut rt, "resume");
    assert_eq!(rt.state.torrents[0].status, Status::Downloading);
}

#[test]
fn a_complete_torrent_paused_is_done() {
    let mut seeding = torrent(0, "a");
    seeding.status = Status::Seeding;
    seeding.content = seeding.content.map(|c| Content { progress_bytes: c.size_bytes, ..c });
    let mut rt = app(MemoryService::new().with_torrents(vec![seeding]));
    tap(&mut rt, "rows/0");
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].status, Status::Done);
}

#[test]
fn remove_offers_remove_and_delete_for_downloaded_torrents() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b")]));
    tap(&mut rt, "rows/0");
    shortcut(&mut rt, Command::Remove);
    let menu = drawn_menu(&rt, "remove-menu").expect("a menu asks remove or delete");
    assert_eq!(menu.len(), 2, "{menu:?}");
    pick_choice(&mut rt, "remove-menu", "Delete");
    let names: Vec<_> = rt.state.torrents.iter().map(|t| t.name.clone().unwrap()).collect();
    assert_eq!(names, ["b"]);
    assert_eq!(rt.state.table.selected_len(), 0, "the selection goes with the row");
}

#[test]
fn remove_on_a_torrent_fetching_info_stops_it() {
    let mut rt = app(MemoryService::new().info_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    wait(&mut rt, 5.0);
    assert_eq!(rt.state.torrents[0].status, Status::FetchingInfo, "still fetching");
    let id = rt.state.torrents[0].id;

    tap(&mut rt, &format!("rows/{id}"));
    shortcut(&mut rt, Command::Remove);
    let menu = drawn_menu(&rt, "remove-menu").expect("a menu");
    assert_eq!(menu, [Some("Remove torrent".to_owned())], "no files to delete yet");
    pick_choice(&mut rt, "remove-menu", "Remove");
    assert!(rt.state.torrents.is_empty());
}

#[test]
fn pause_and_resume_are_off_without_a_torrent_selected() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a")]));
    tap(&mut rt, "pause");
    assert_eq!(rt.state.torrents[0].status, Status::Downloading, "nothing selected, nothing paused");
}

#[test]
fn an_action_applies_to_the_selected_torrents_the_service_lists_it_for() {
    let mut running = torrent(0, "a");
    running.actions = vec![Action::Pause];
    let mut fetching = torrent(1, "b");
    fetching.actions = vec![Action::Remove, Action::Delete];
    let torrents = [running, fetching];
    let mut table = TableState::new().multi_select(true);
    table.select("1");
    assert_eq!(selected_for(&table, &torrents, Action::Pause), [] as [TorrentId; 0]);
    table.select("0");
    assert_eq!(selected_for(&table, &torrents, Action::Pause), [0]);
}

#[test]
fn sorting_by_size_puts_the_biggest_first_when_descending() {
    let mut torrents = vec![torrent(0, "small"), torrent(1, "big")];
    torrents[1].content.as_mut().unwrap().size_bytes *= 4;
    sort(&mut torrents, "size", SortDir::Descending);
    assert_eq!(torrents[0].name.as_deref(), Some("big"));
}

#[test]
fn a_magnet_link_opened_from_outside_is_added() {
    let mut rt = app(MemoryService::new());
    rt.update(|s, tx| open_url(s, MAGNET, tx));
    settle(&mut rt);
    assert_eq!(rt.state.torrents.len(), 1);
}

#[test]
fn other_links_opened_from_outside_are_refused() {
    let mut rt = app(MemoryService::new());
    rt.update(|s, tx| open_url(s, "file:///etc/passwd", tx));
    settle(&mut rt);
    settle(&mut rt);
    let lines = ["Couldn't add the torrent", "file:///etc/passwd", "Phantom only opens magnet links"];
    assert_eq!(listed(&rt), vec![lines]);
    assert!(rt.state.torrents.is_empty(), "nothing added");
    assert!(open_link("https://example.com/a.torrent").is_err(), "only magnets");
}

/// A fresh directory for files opened from outside, removed when dropped.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("phantom-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, b"d4:infode").unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Opens `paths` as Finder hands them over, and returns how many torrents are listed after.
fn open_from_finder(rt: &mut Runtime<Phantom>, paths: &[std::path::PathBuf]) -> usize {
    let paths = paths.to_vec();
    rt.update(move |s, tx| open_files(s, &paths, tx));
    settle(rt);
    rt.state.torrents.len()
}

#[test]
fn torrent_files_opened_from_outside_are_added() {
    let dir = Dir::new("open");
    let opened = open_torrent_files(&[dir.file("a.torrent"), dir.file("B.TORRENT")]);
    assert_eq!(opened.adds.len(), 2);
    assert!(opened.refused.is_empty());
    let mut rt = app(MemoryService::new());
    assert_eq!(open_from_finder(&mut rt, &[dir.file("a.torrent"), dir.file("B.TORRENT")]), 2);
}

#[test]
fn other_files_opened_from_outside_are_refused() {
    let dir = Dir::new("refuse");
    let folder = dir.0.join("folder.torrent");
    std::fs::create_dir(&folder).unwrap();
    for (path, why) in [
        (dir.file("notes.txt"), "Phantom only opens .torrent files"),
        (dir.file("torrent"), "Phantom only opens .torrent files"),
        (folder, "Phantom only opens .torrent files"),
        (dir.0.join("gone.torrent"), "It isn't on disk"),
    ] {
        let mut rt = app(MemoryService::new());
        assert_eq!(open_from_finder(&mut rt, std::slice::from_ref(&path)), 0, "nothing added for {path:?}");
        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(listed(&rt), vec![["Couldn't add the torrent", &*name, why]]);
    }
}

#[test]
fn of_several_files_opened_together_the_torrents_are_added_and_the_rest_counted() {
    let dir = Dir::new("several");
    let (txt, gone) = (dir.file("a.txt"), dir.0.join("gone.torrent"));
    let opened = open_torrent_files(&[txt.clone(), dir.file("b.torrent"), gone.clone()]);
    assert_eq!(opened.refused.len(), 2);
    let mut rt = app(MemoryService::new());
    assert_eq!(open_from_finder(&mut rt, &[txt, dir.file("b.torrent"), gone]), 1);
    assert_eq!(listed(&rt).len(), 1, "one entry for the batch");
    assert_eq!(listed(&rt)[0][0], "Couldn't add 2 of 3");
}

#[test]
fn selecting_a_torrent_shows_its_files() {
    let mut rt = app(with_an_album());
    assert!(rt.find("details").is_none(), "no pane without a selection");
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    assert!(on_screen(&rt, "details"));
    assert_eq!(
        files(&rt),
        [
            ("01 Intro.flac".into(), Priority::Normal),
            ("02 Song.flac".into(), Priority::Normal),
            ("Cover/front.jpg".into(), Priority::Normal),
        ]
    );
    assert!(on_screen(&rt, "files/viewport/rows/2"), "each file is a row");
    assert!(row_menu(&mut rt, 1).contains(&"Reveal in Finder".to_owned()));
}

#[test]
fn a_file_set_off_drops_out_of_the_size() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    click_priority(&mut rt, 2);
    settle(&mut rt);

    assert_eq!(files(&rt)[2], ("Cover/front.jpg".into(), Priority::Off));
    let t = find_torrent(&rt, 1);
    assert_eq!(
        (t.content.unwrap().size_bytes, t.content.unwrap().full_size_bytes),
        (300 * MIB, 600 * MIB),
        "Size leaves it out, Full size doesn't"
    );

    click_priority(&mut rt, 2);
    settle(&mut rt);
    assert_eq!(files(&rt)[2].1, Priority::Normal);
    assert_eq!(find_torrent(&rt, 1).content.unwrap().size_bytes, 600 * MIB);
}

#[test]
fn several_selected_files_change_priority_together() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    // The first of the two selected: both take its next priority.
    click_priority(&mut rt, 0);
    settle(&mut rt);
    let priorities: Vec<_> = files(&rt).into_iter().map(|(_, p)| p).collect();
    assert_eq!(priorities, [Priority::Off, Priority::Off, Priority::Normal]);
    assert_eq!(find_torrent(&rt, 1).content.unwrap().size_bytes, 300 * MIB);
}

#[test]
fn every_file_off_finishes_the_torrent() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    click_priority(&mut rt, 0);
    settle(&mut rt);
    assert_eq!(notice(&rt), None, "no refusal");
    assert!(files(&rt).iter().all(|(_, p)| *p == Priority::Off));
    let t = find_torrent(&rt, 1);
    assert_eq!((t.content.unwrap().size_bytes, t.content.unwrap().full_size_bytes), (0, 600 * MIB));
    assert_eq!(t.status, Status::Seeding, "nothing left to get");
    assert_eq!(t.content.unwrap().progress(), 1.0);

    click_priority(&mut rt, 0);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading, "Normal files have more to get");
}

#[test]
fn the_pane_shows_only_while_exactly_one_torrent_is_selected() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/0");
    assert!(on_screen(&rt, "details"));
    rt.key(shift(Key::Down));
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 2);
    assert!(rt.find("details").is_none(), "two torrents have no one pane");
    assert!(rt.state.pane.watching.is_empty(), "and the service stops sending details");

    rt.key(KeyEvent { key: Key::Up, mods: Mods::default() });
    settle(&mut rt);
    assert!(on_screen(&rt, "details"), "one again");
    shortcut(&mut rt, Command::Remove);
    pick_choice(&mut rt, "remove-menu", "Remove");
    assert!(rt.find("details").is_none(), "the removed torrent's pane goes with it");
}

#[test]
fn switching_torrents_with_the_arrows_shows_one_whole_torrent_every_frame() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    rt.key(KeyEvent { key: Key::Down, mods: Mods::default() });
    let mut shown = Vec::new();
    for _ in 0..4 {
        rt.frame(FRAME);
        // Not the new name over no files while its details are on the way: the last torrent,
        // until the new one's details arrive.
        let name = texts(&rt, "details-name").concat();
        let first_file = texts(&rt, "files/viewport/rows/0").into_iter().next().expect("a file");
        assert!(matches!((&*name, &*first_file), ("a", "a") | ("Album", "01 Intro.flac")), "{name} with {first_file}");
        shown.push(name);
    }
    shown.dedup();
    assert_eq!(shown.last().map(String::as_str), Some("Album"), "the new torrent, once its details are in");
}

#[test]
fn reveal_shows_the_file_of_a_single_file_torrent_and_the_folder_of_others() {
    let mut rt = app(with_an_album());
    let target = |rt: &Runtime<Phantom>| {
        let torrent = details_torrent(&rt.state.table, &rt.state.torrents).unwrap();
        reveal_target(torrent).map(Path::to_owned)
    };
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    assert!(target(&rt).unwrap().ends_with("Downloads/a"), "the file itself");
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let folder = target(&rt).unwrap();
    assert!(folder.ends_with("Downloads/Album"), "the folder: {folder:?}");
    assert!(!folder.ends_with("01 Intro.flac"));
}

#[test]
fn reveal_is_off_while_fetching_info() {
    let mut rt = app(MemoryService::new().info_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    let id = rt.state.torrents[0].id;
    tap(&mut rt, &format!("rows/{id}"));
    settle(&mut rt);
    assert!(on_screen(&rt, "details"), "the pane opens, with no files yet");
    assert!(on_screen(&rt, "no-files"));
    assert_eq!(reveal_target(&rt.state.torrents[0]), None);
    assert!(!row_menu(&mut rt, id).contains(&"Reveal in Finder".to_owned()));
}

#[test]
fn the_status_bar_speeds_stay_put_when_an_action_runs() {
    // The speeds hug the right edge; acting on a torrent mustn't move them, and the bar shows no
    // "Working…" next to them.
    let mut rt = app(with_an_album());
    let before = rt.find("totals").expect("the totals");
    let id = rt.state.torrents[0].id;
    tap(&mut rt, &format!("rows/{id}"));
    tap(&mut rt, "pause");
    assert_eq!(rt.find("totals").expect("the totals"), before);
}

/// The texts drawn inside the element at `key`, e.g. a row's cells.
fn texts(rt: &Runtime<Phantom>, key: &str) -> Vec<String> {
    rt.elements().into_iter().filter(|e| e.path.contains(key)).filter_map(|e| e.text).collect()
}

/// The album with its second file gone from disk, selected.
fn album_missing_a_song() -> Runtime<Phantom> {
    let mut rt = app(with_an_album().with_missing(1, &[1]));
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    rt
}

#[test]
fn a_torrent_missing_files_says_so_and_marks_each_missing_file() {
    let mut rt = album_missing_a_song();
    assert_eq!(find_torrent(&rt, 1).status, Status::MissingFiles);
    assert!(texts(&rt, "rows/1").contains(&"Missing files".to_owned()), "in the list");
    assert!(texts(&rt, "files/viewport/rows/1").contains(&"Missing".to_owned()), "the missing file is marked");
    for other in ["files/viewport/rows/0", "files/viewport/rows/2"] {
        assert!(!texts(&rt, other).contains(&"Missing".to_owned()), "{other} isn't");
    }
    assert!(row_menu(&mut rt, 1).contains(&"Check".to_owned()));
}

#[test]
fn setting_the_missing_file_off_clears_it() {
    let mut rt = album_missing_a_song();
    click_priority(&mut rt, 1);
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading, "nothing missing: running again, as it was");
    assert!(!texts(&rt, "files/viewport/rows/1").contains(&"Missing".to_owned()));
    assert_eq!(files(&rt)[1].1, Priority::Off);
}

#[test]
fn resuming_a_torrent_missing_files_checks_it_then_downloads_them_again() {
    let mut rt = album_missing_a_song();
    tap(&mut rt, "resume");
    assert_eq!(find_torrent(&rt, 1).status, Status::Checking);
    assert!(!texts(&rt, "files/viewport/rows/1").contains(&"Missing".to_owned()), "to download, once checked");
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading);
    assert!(rt.state.pane.details.as_ref().unwrap().files.iter().all(|f| !f.missing), "nothing marked");
}

#[test]
fn check_checks_a_torrent_and_leaves_it_running_or_stopped() {
    let mut seeding = torrent(0, "a");
    seeding.status = Status::Seeding;
    seeding.content = seeding.content.map(|c| Content { progress_bytes: c.size_bytes, ..c });
    let mut rt = app(MemoryService::new().with_torrents(vec![seeding, torrent(1, "b")]));
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    row_pick(&mut rt, 0, "Check");
    assert_eq!(find_torrent(&rt, 0).status, Status::Checking);
    assert!(!row_menu(&mut rt, 0).contains(&"Check".to_owned()), "not again while checking");
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 0).status, Status::Seeding);

    tap(&mut rt, "pause");
    row_pick(&mut rt, 0, "Check");
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 0).status, Status::Done, "stopped stays stopped");
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading, "the other is untouched");
}

#[test]
fn check_is_off_while_fetching_info() {
    let mut rt = app(MemoryService::new().info_after(f64::INFINITY));
    rt.paste(MAGNET);
    settle(&mut rt);
    let id = rt.state.torrents[0].id;
    tap(&mut rt, &format!("rows/{id}"));
    settle(&mut rt);
    assert!(!row_menu(&mut rt, id).contains(&"Check".to_owned()));
}

/// The colour the text `text` is drawn in, inside the element at `key`.
fn text_color(rt: &Runtime<Phantom>, key: &str, text: &str) -> rune::Color {
    let elements = rt.elements();
    let e = elements.iter().find(|e| e.path.contains(key) && e.text.as_deref() == Some(text));
    e.and_then(|e| e.text_color).unwrap_or_else(|| panic!("no {text:?} in {key}"))
}

#[test]
fn missing_files_look_like_an_error_in_the_list_and_the_pane() {
    let mut failed = torrent(2, "c");
    failed.status = Status::Error { message: "disk full".into() };
    let mut rt = app(with_an_album().with_torrents(vec![failed]).with_missing(1, &[1]));
    tap(&mut rt, "rows/1");
    settle(&mut rt);

    let error = text_color(&rt, "rows/2", "Error");
    assert_eq!(text_color(&rt, "rows/1", "Missing files"), error, "the list shows it as an error");
    assert_ne!(text_color(&rt, "rows/0", "Downloading"), error, "other statuses aren't");
    assert_eq!(text_color(&rt, "files/viewport/rows/1", "Missing"), error, "and so the pane's file mark");
}

#[test]
fn escape_clears_the_selection_and_closes_the_details_pane() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    assert!(rt.find("details").is_some(), "the pane is open");

    rt.key(KeyEvent { key: Key::Escape, mods: Default::default() });
    settle(&mut rt);
    assert_eq!(rt.state.table.selected_len(), 0);
    assert!(rt.find("details").is_none(), "the pane is closed");
    assert!(rt.state.pane.watching.is_empty(), "and the service stops sending details");
}

#[test]
fn copy_magnet_link_copies_the_info_hash_and_name() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "Big Buck Bunny (2008)")]));
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    row_pick(&mut rt, 0, "Copy magnet link");
    let copied = rt.copy_request.take().expect("a copy");
    assert_eq!(&*copied, format!("magnet:?xt=urn:btih:{:040x}&dn=Big%20Buck%20Bunny%20%282008%29", 0));
}

#[test]
fn add_is_off_until_the_field_holds_a_link() {
    let mut rt = app(MemoryService::new());
    assert!(!enabled(&rt, "add"), "empty");
    tap(&mut rt, "magnet-field");
    rt.paste("hello");
    settle(&mut rt);
    assert!(!enabled(&rt, "add"), "not a link");

    rt.state.magnet.clear();
    rt.paste("magnet:?xt=urn:btih:abc&dn=x");
    settle(&mut rt);
    assert!(!enabled(&rt, "add"), "a magnet without a whole info hash");

    rt.state.magnet.clear();
    rt.paste(MAGNET);
    settle(&mut rt);
    assert!(enabled(&rt, "add"));
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);
    assert_eq!(rt.state.torrents.len(), 1);
}

#[test]
fn an_invalid_link_is_not_submitted_with_return() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste("hello");
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);
    assert_eq!(rt.state.magnet.text, "hello", "the text stays");
    assert!(rt.state.notice.is_none() && rt.state.torrents.is_empty());
}

#[test]
fn what_is_wrong_with_a_link() {
    use crate::model::link_problem;
    use client::LinkProblem::*;
    let hex = "0".repeat(40);
    assert_eq!(link_problem(&hex), None, "a bare info hash");
    assert_eq!(link_problem(&"A2".repeat(16)), None, "or a base32 one");
    assert_eq!(link_problem(" https://example.org/a.torrent "), None);
    assert_eq!(link_problem(&format!("magnet:?dn=x&xt=urn:btih:{hex}")), None);
    assert_eq!(link_problem("https://"), Some(NotALink));
    assert_eq!(link_problem("ftp://example.org/a.torrent"), Some(NotALink));
    assert_eq!(link_problem(&"0".repeat(39)), Some(NotALink), "a short info hash");
    assert_eq!(link_problem("magnet:?xt=urn:btih:abc"), Some(NoInfoHash));
    assert_eq!(link_problem("magnet:?dn=x"), Some(NoInfoHash));
}

#[test]
fn pressing_the_disabled_add_says_why_until_the_text_changes() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste("magnet:?xt=urn:btih:abc");
    settle(&mut rt);
    assert!(rt.find("add-note-content").is_none(), "not before it's pressed");

    tap(&mut rt, "add");
    assert!(rt.find("add-note-content").is_some(), "the reason shows");
    assert!(rt.state.torrents.is_empty() && rt.state.notice.is_none(), "and nothing was added");
    assert!(on_screen(&rt, "add-note-content"));

    tap(&mut rt, "magnet-field");
    rt.paste("0");
    wait(&mut rt, 1.0);
    assert!(rt.find("add-note-content").is_none(), "an edit closes it");
}

#[test]
fn return_in_an_invalid_field_says_why_too() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste("hello");
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);
    assert!(rt.find("add-note-content").is_some());
}

#[test]
fn a_bare_info_hash_is_added() {
    let mut rt = app(MemoryService::new());
    tap(&mut rt, "magnet-field");
    rt.paste(&"0".repeat(40));
    settle(&mut rt);
    assert!(enabled(&rt, "add"));
    tap(&mut rt, "add");
    assert_eq!(rt.state.torrents.len(), 1);
}

fn without_a_default_destination() -> MemoryService {
    MemoryService::new().with_default_destination(None)
}

/// The folder the picker showing asks for, and where it starts.
fn picker(rt: &Runtime<Phantom>) -> Option<(String, Option<std::path::PathBuf>)> {
    let (kind, options) = rt.file_dialog()?;
    assert_eq!(kind, rune::FileDialogKind::Folder);
    Some((options.title.clone(), options.directory.clone()))
}

fn choose(rt: &mut Runtime<Phantom>, folder: Option<&str>) {
    rt.resolve_file_dialog(folder.map(Into::into)).unwrap();
    settle(rt);
}

#[test]
fn without_a_default_destination_an_add_asks_where_first_suggesting_downloads() {
    let mut rt = app(without_a_default_destination().with_torrents(vec![torrent(0, "a")]));
    rt.paste(MAGNET);
    settle(&mut rt);
    let downloads = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join("Downloads");
    assert_eq!(picker(&rt), Some(("Choose where to download the torrent".into(), Some(downloads))));
    assert_eq!(rt.state.torrents.len(), 1, "not added yet");

    choose(&mut rt, Some("/Volumes/MyDisk/Films"));
    wait(&mut rt, 3.0);
    assert_eq!(rt.state.torrents.len(), 2);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    let location = &rt.state.pane.details.as_ref().unwrap().location;
    assert_eq!(location, Path::new("/Volumes/MyDisk/Films/Big Buck Bunny"), "into the chosen folder");
}

#[test]
fn cancelling_the_folder_picker_adds_nothing() {
    let mut rt = app(without_a_default_destination());
    rt.paste(MAGNET);
    settle(&mut rt);
    choose(&mut rt, None);
    wait(&mut rt, 1.0);
    assert!(rt.state.torrents.is_empty());
    assert!(rt.state.notice.is_none(), "nothing to say");
}

#[test]
fn links_and_files_opened_from_outside_ask_too_one_picker_at_a_time() {
    let dir = Dir::new("ask");
    let mut rt = app(without_a_default_destination());
    let files = vec![dir.file("a.torrent"), dir.file("b.torrent")];
    rt.update(move |s, tx| open_files(s, &files, tx));
    rt.update(|s, tx| open_url(s, MAGNET, tx));
    settle(&mut rt);
    assert_eq!(picker(&rt).unwrap().0, "Choose where to download 2 torrents", "files opened together: one question");

    choose(&mut rt, Some("/tmp/one"));
    assert_eq!(rt.state.torrents.len(), 2);
    assert_eq!(picker(&rt).unwrap().0, "Choose where to download the torrent", "then the link's turn");
    choose(&mut rt, Some("/tmp/two"));
    assert_eq!(rt.state.torrents.len(), 3);
    assert!(rt.file_dialog().is_none());
}

#[test]
fn with_a_default_destination_an_add_goes_straight_there() {
    let mut rt = app(MemoryService::new().with_default_destination(Some("/srv/torrents".into())));
    rt.paste(MAGNET);
    settle(&mut rt);
    assert!(rt.file_dialog().is_none());
    assert_eq!(rt.state.torrents.len(), 1);
}

#[test]
fn a_link_that_opens_the_app_asks_only_once_the_service_says_there_is_no_default() {
    for default in [Some("/srv/torrents".into()), None] {
        let env = Env { system_dark: true, scale: 2.0 };
        let service = MemoryService::new().with_default_destination(default.clone());
        let mut rt = Runtime::new(Phantom::new(Box::new(service)), views::view, (960.0, 540.0), env);
        rt.poll = Some(poll);
        rt.update(|s, tx| open_url(s, MAGNET, tx));
        assert!(rt.file_dialog().is_none(), "not asked before the service answers");
        rt.frame(0.0);
        settle(&mut rt);
        assert_eq!(rt.file_dialog().is_some(), default.is_none(), "asked only without a default");
        assert_eq!(rt.state.torrents.len(), if default.is_some() { 1 } else { 0 });
    }
}

fn drive_gone() -> Unavailable {
    Unavailable::DriveNotConnected { name: "MyDisk".into() }
}

/// The album, whose destination is away, selected.
fn album_away(service: MemoryService) -> Runtime<Phantom> {
    let mut rt = app(service.at(0.0, DiskChange::DestinationGone { id: 1, reason: drive_gone() }));
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    rt
}

#[test]
fn a_destination_unavailable_says_why_on_hover_in_the_list_and_in_the_pane() {
    let mut rt = album_away(with_an_album());
    let elements = rt.elements();

    let status = elements.iter().find(|e| e.path.contains("rows/1") && e.text.as_deref() == Some("Destination unavailable"));
    assert_eq!(status.and_then(|e| e.help.as_deref()), Some("Drive MyDisk isn't connected"), "on hover in the list");
    assert!(texts(&rt, "details-header").contains(&"Drive MyDisk isn't connected".to_owned()), "in the pane's header");
    let error = text_color(&rt, "details-header", "Drive MyDisk isn't connected");
    assert_eq!(text_color(&rt, "rows/1", "Destination unavailable"), error, "both as an error");
    let menu = row_menu(&mut rt, 1);
    assert!(!menu.contains(&"Check".to_owned()) && !menu.contains(&"Reveal in Finder".to_owned()), "nothing to check or show: {menu:?}");
}

#[test]
fn resume_is_off_while_the_destination_is_away_and_pause_keeps_it_stopped_once_back() {
    let mut rt = album_away(with_an_album());
    assert!(!enabled(&rt, "resume"), "it's wanted running already");
    tap(&mut rt, "pause");
    assert!(!enabled(&rt, "resume"), "it can't start while away");
    assert!(!enabled(&rt, "pause"));
    assert_eq!(find_torrent(&rt, 1).status, Status::DestinationUnavailable { reason: drive_gone() });
}

#[test]
fn remove_keeps_the_files_of_a_torrent_whose_destination_is_away() {
    let mut rt = album_away(with_an_album());
    shortcut(&mut rt, Command::Remove);
    let menu = drawn_menu(&rt, "remove-menu").expect("a menu");
    assert_eq!(menu, [Some("Remove torrent (keep files)".to_owned())], "its files can't be deleted");
}

#[test]
fn a_torrent_running_when_its_destination_went_checks_and_runs_again_once_it_is_back() {
    let mut rt = album_away(with_an_album().at(2.0, DiskChange::DestinationBack { id: 1 }));
    wait(&mut rt, 2.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::Checking);
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading);
}

#[test]
fn a_torrent_paused_before_or_while_its_destination_is_away_stays_paused_once_it_is_back() {
    let mut paused = torrent(2, "c");
    paused.status = Status::Paused;
    let service = with_an_album().with_torrents(vec![paused]).at(0.0, DiskChange::DestinationGone { id: 2, reason: drive_gone() });
    let back = |id| DiskChange::DestinationBack { id };
    let mut rt = album_away(service.at(2.0, back(1)).at(2.0, back(2)));
    tap(&mut rt, "pause");
    wait(&mut rt, 4.0);
    assert_eq!(find_torrent(&rt, 1).status, Status::Paused, "paused while away");
    assert_eq!(find_torrent(&rt, 2).status, Status::Paused, "paused before");
}

#[test]
fn files_deleted_outside_phantom_stop_the_torrent_and_putting_them_back_checks_it() {
    let service = with_an_album();
    let disk = service.disk();
    let mut rt = app(service);
    tap(&mut rt, "rows/1");
    wait(&mut rt, 1.0);
    let before = find_torrent(&rt, 1).content.unwrap().progress_bytes;
    assert!(before > 0, "it has downloaded some");

    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![0, 1] });
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::MissingFiles);
    assert!(texts(&rt, "files/viewport/rows/0").contains(&"Missing".to_owned()));

    disk.change(DiskChange::FilesPutBack { id: 1, files: vec![0] });
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::MissingFiles, "one is still missing");

    disk.change(DiskChange::FilesPutBack { id: 1, files: vec![1] });
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::Checking, "checked once they're all back");
    wait(&mut rt, 1.5);
    let t = find_torrent(&rt, 1);
    assert_eq!(t.status, Status::Downloading, "and runs again, as it was");
    assert!(t.content.unwrap().progress_bytes >= before, "with what they held");
    assert!(rt.state.pane.details.as_ref().unwrap().files.iter().all(|f| !f.missing));
}

#[test]
fn a_change_on_disk_can_wait_for_its_time_on_the_app_clock() {
    let back = DiskChange::FilesPutBack { id: 1, files: vec![1] };
    let mut rt = app(with_an_album().with_missing(1, &[1]).at(2.0, back));
    wait(&mut rt, 1.0);
    assert_eq!(find_torrent(&rt, 1).status, Status::MissingFiles);
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::Checking, "put back at 2 s");
}

#[test]
fn copying_says_so_in_the_status_bar() {
    let mut rt = app(MemoryService::new().with_torrents(vec![torrent(0, "a")]));
    tap(&mut rt, "rows/0");
    settle(&mut rt);
    row_pick(&mut rt, 0, "Copy magnet link");
    assert!(rt.copy_request.take().is_some(), "a copy");
    assert_eq!(notice(&rt), Some(client::display::copied_magnet("a").as_str()));
}

#[test]
fn a_theme_without_overrides_draws_phantoms_icons() {
    use crate::ui::{Icon, Theme, Ui};
    let phantom = Ui::new(true);
    let bare = Ui::new(true).themed(Theme::overriding(|_| None));
    assert!(std::ptr::eq(bare.icon(Icon::Copy), phantom.icon(Icon::Copy)), "nothing overridden falls back");
    static MINE: rune::Icon = rune::Icon { name: "mine", view: 24.0, body: "" };
    let own = Ui::new(true).themed(Theme::overriding(|i| (i == Icon::Copy).then_some(&MINE)));
    assert!(std::ptr::eq(own.icon(Icon::Copy), &MINE), "an override wins");
}

#[test]
fn a_failed_add_keeps_the_text_selected_next_to_the_error() {
    let mut rt = app(MemoryService::new().rejecting_links());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);

    assert_eq!(rt.state.magnet.text, MAGNET);
    assert_eq!(rt.state.magnet.selected(), MAGNET, "selected, to be typed over");
    assert!(rt.state.notice.is_none(), "not in the status bar");
    assert!(rt.find("add-note-content").is_some(), "but by Add");
    assert_eq!(rt.state.add_note.as_ref().map(|n| n.message.as_str()), Some("No peers have this torrent"), "as a sentence");
    assert!(enabled(&rt, "add"), "which can be tried again");
    assert!(rt.state.add_note.as_ref().is_some_and(|n| n.error && n.alerting), "and is red for a while");
    wait(&mut rt, 1.0);
    assert!(rt.state.add_note.as_ref().is_some_and(|n| n.alerting), "still red at 1 s");
    wait(&mut rt, 1.5);
    assert!(rt.state.add_note.as_ref().is_some_and(|n| !n.alerting), "back to normal after 2 s");
    assert!(rt.find("add-note-content").is_some(), "the message stays");

    tap(&mut rt, "magnet-field");
    rt.paste("x");
    wait(&mut rt, 1.0);
    assert!(rt.find("add-note-content").is_none(), "an edit closes it");

    rt.key(KeyEvent { key: Key::Escape, mods: Default::default() });
    settle(&mut rt);
    assert!(rt.state.add_note.is_none());
}

#[test]
fn the_next_add_closes_the_error() {
    let mut rt = app(MemoryService::new().rejecting_links());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);
    assert!(rt.find("add-note-content").is_some());
    rt.frame(FRAME);
    tap(&mut rt, "add");
    rt.frame(FRAME);
    assert!(rt.state.add_note.is_none(), "closed while the next add is out");
}

#[test]
fn a_click_outside_closes_the_error() {
    let mut rt = app(MemoryService::new().rejecting_links());
    tap(&mut rt, "magnet-field");
    rt.paste(MAGNET);
    rt.key(KeyEvent { key: Key::Enter, mods: Default::default() });
    settle(&mut rt);
    tap(&mut rt, "list");
    assert!(rt.state.add_note.is_none());
}

#[test]
fn files_deleted_during_a_check_are_missing_once_it_ends_unless_put_back_meanwhile() {
    let service = with_an_album();
    let disk = service.disk();
    let mut rt = app(service);
    tap(&mut rt, "rows/1");
    row_pick(&mut rt, 1, "Check");
    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![2] });
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::Checking, "the check goes on");
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::MissingFiles);

    row_pick(&mut rt, 1, "Check");
    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![2] });
    disk.change(DiskChange::FilesPutBack { id: 1, files: vec![2] });
    wait(&mut rt, 1.5);
    assert_eq!(find_torrent(&rt, 1).status, Status::Downloading, "back before the check ended");
}

#[test]
fn files_deleted_while_the_destination_is_away_change_nothing_visible() {
    // Found by the generated test (#85): [DestinationGone, Select, FilesDeleted, Remove{delete}].
    let service = with_an_album();
    let disk = service.disk();
    let mut rt = app(service);
    disk.change(DiskChange::DestinationGone { id: 1, reason: drive_gone() });
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![0] });
    settle(&mut rt);
    assert_eq!(find_torrent(&rt, 1).status, Status::DestinationUnavailable { reason: drive_gone() }, "out of reach, not Missing files");
    shortcut(&mut rt, Command::Remove);
    let menu = drawn_menu(&rt, "remove-menu").expect("a menu");
    assert_eq!(menu.len(), 1, "Delete is refused while the destination is away: {menu:?}");
    pick_choice(&mut rt, "remove-menu", "Remove");
    assert!(rt.state.torrents.iter().all(|t| t.id != 1), "Remove still removes it");
}

#[test]
fn a_paused_torrent_whose_files_went_while_the_destination_was_away_stays_stopped_once_it_is_back() {
    // Seen on #81 and master (#85): "started stopped torrent" after DestinationGone + FilesDeleted.
    let mut paused = torrent(2, "c");
    paused.status = Status::Paused;
    let service = with_an_album().with_torrents(vec![paused]);
    let disk = service.disk();
    let mut rt = app(service);
    disk.change(DiskChange::DestinationGone { id: 2, reason: drive_gone() });
    settle(&mut rt);
    disk.change(DiskChange::FilesDeleted { id: 2, files: vec![0] });
    disk.change(DiskChange::DestinationBack { id: 2 });
    wait(&mut rt, 2.5);
    let status = &find_torrent(&rt, 2).status;
    assert!(!matches!(status, Status::Downloading | Status::Seeding), "stopped, not {status:?}");
}

/// An example, not a property: the sequence the generated test shrank to on #107, pinned because
/// #86 promised it passes. A failure and another answer on one frame used to leave the failure
/// shown to no one.
#[test]
fn a_failure_landing_with_another_answer_is_still_shown() {
    const NOBODY: &str = "magnet:?xt=urn:btih:00000000000000000000000000000000000000ff&dn=Nobody";
    let mut rt = app(MemoryService::new().rejecting(NOBODY).adds_after(0.5, 0.5));
    tap(&mut rt, "magnet-field");
    rt.paste(NOBODY);
    settle(&mut rt);
    tap(&mut rt, "add");
    rt.update(|s, _| s.magnet = rune::TextEdit::new(MAGNET));
    rt.update(|_, tx| tx.blur());
    rt.paste(MAGNET);
    wait(&mut rt, 1.5);
    assert_eq!(listed(&rt), vec![["Couldn't add the torrent", NOBODY, "No peers have this torrent"]]);
}

/// An example, not a property: where the panel sits and what lies around it is geometry. It starts
/// under the toolbar, in the middle; the window stays usable around it; it never covers the
/// message by Add, even at the smallest window; and it scrolls past about half the window.
#[test]
fn the_error_list_sits_under_the_toolbar_without_covering_the_add_note_or_the_window() {
    for size in [(960.0, 900.0), (640.0, 320.0)] {
        let bad: Vec<std::path::PathBuf> = (0..12).map(|n| format!("/a/bad-{n}.torrent").into()).collect();
        let mut service = with_an_album().with_default_destination(Some("/srv".into()));
        for path in &bad {
            service = service.rejecting(&path.to_string_lossy());
        }
        let mut rt = app(service);
        rt.set_viewport(size, 2.0);
        settle(&mut rt);
        rt.drop_files(bad[..1].to_vec(), None);
        settle(&mut rt);
        settle(&mut rt);
        let panel = rect_of(&rt, "error-list").expect("the panel is up");
        let toolbar = rect_of(&rt, "toolbar").unwrap();
        assert!(panel[1] >= toolbar[1] + toolbar[3], "under the toolbar");
        assert!(((panel[0] + panel[2] / 2.0) - size.0 / 2.0).abs() < 1.0, "in the middle");

        // The message by Add, while the list is up: Add is off for an empty field.
        tap(&mut rt, "add");
        let (panel, add) = (rect_of(&rt, "error-list").unwrap(), rect_of(&rt, "add-note-content").unwrap());
        assert!(
            panel[1] >= add[1] + add[3] || panel[0] + panel[2] <= add[0] || add[0] + add[2] <= panel[0],
            "{panel:?} covers {add:?} at {size:?}"
        );
        rt.key(KeyEvent { key: Key::Escape, mods: Default::default() });
        settle(&mut rt);

        // Many entries: the panel grows down to the status bar's room, or to its cap, and the
        // rest scrolls.
        rt.drop_files(bad[1..].to_vec(), None);
        settle(&mut rt);
        rt.update(|s, _| {
            for n in 0..30 {
                s.errors.report(crate::notice::Notice::new(format!("Error number {n}")));
            }
        });
        settle(&mut rt);
        let panel = rect_of(&rt, "error-list").unwrap();
        let (_, expected_bottom) = (0, (size.1 - crate::ui::BOTTOM_ROOM).min(panel[1] + crate::ui::MAX_HEIGHT));
        assert!((panel[1] + panel[3] - expected_bottom).abs() < 1.0, "reaches {expected_bottom}: {panel:?} in {size:?}");
        assert!(panel[3] <= crate::ui::MAX_HEIGHT + 0.5, "never taller than its cap");

        generated::scroll_through(&mut rt);
        assert!(rect_of(&rt, "error-scroll").is_some(), "the list scrolls in its own viewport");

        // The window works around it: a row is selected by a click, and the list stays.
        let before = entry_keys(&rt).len();
        let (_, y) = rt.find("rows/1").unwrap();
        let x = 20.0;
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(&mut rt);
        assert_eq!(selected_torrents(&rt.state.table), vec![1], "the list takes clicks beside the panel");
        assert_eq!(entry_keys(&rt).len(), before, "and the errors stay");
        tap(&mut rt, "clear-all");
        assert!(entry_keys(&rt).is_empty(), "Clear all empties it");
        wait(&mut rt, 1.0);
        assert!(rect_of(&rt, "error-list").is_none(), "and the panel goes once they have faded");
    }
}

/// An example, not a property: the sequence the generated test shrank to on #111 (slow details,
/// then back to a torrent shown before, whose file goes). Pinned because its saved seed replays to
/// something else once the generated test's setup grew (late command snapshots, #133). The pane
/// said Missing files with no file marked missing.
#[test]
fn a_torrent_selected_again_while_its_details_are_slow_never_shows_a_status_its_files_contradict() {
    let service = with_an_album().details_after(48.5 * FRAME);
    let disk = service.disk();
    let mut rt = app(service);
    let agree = |rt: &Runtime<Phantom>| {
        let (Some(t), Some(details)) = (shown_torrent(&rt.state.table, &rt.state.torrents, &rt.state.pane), &rt.state.pane.details) else {
            return;
        };
        assert_eq!(details.id, t.id, "the files are the shown torrent's");
        let missing = details.files.iter().any(|f| f.missing);
        assert_eq!(t.status == Status::MissingFiles, missing && t.status != Status::Checking, "{t:?}: {details:?}");
    };
    tap(&mut rt, "rows/1");
    wait(&mut rt, 2.5);
    tap(&mut rt, "rows/0");
    tap(&mut rt, "rows/1");
    disk.change(DiskChange::FilesDeleted { id: 1, files: vec![0] });
    for _ in 0..120 {
        rt.frame(FRAME);
        agree(&rt);
    }
    assert_eq!(rt.state.pane.details.as_ref().map(|d| d.id), Some(1), "and it shows the torrent once its details come");
}

/// An example, not a property: the case #133's review found, which the generated test reaches
/// only rarely (it needs torrents that don't change, a command whose own snapshot lands late, and
/// another torrent selected meanwhile). That snapshot is as new as the latest, but built for the
/// torrent watched before: taken for the pane's, it blanked the pane for good.
#[test]
fn a_late_snapshot_built_for_another_torrent_never_blanks_the_pane() {
    let mut rt = app(with_an_album().paused().stale_answers(1.0));
    tap(&mut rt, "rows/1");
    wait(&mut rt, 0.5);
    click_priority(&mut rt, 0);
    tap(&mut rt, "rows/0");
    for _ in 0..(3.0 / FRAME) as usize {
        rt.frame(FRAME);
        if rt.state.pane.watching == [0] {
            assert_ne!(rt.state.pane.details.as_ref().map(|d| d.id), None, "{:?}", rt.state.pane.state);
        }
    }
    assert_eq!(rt.state.pane.details.as_ref().map(|d| d.id), Some(0), "it shows the torrent selected");
}

/// A click on a file's priority steps it Normal → Off → Normal: for the selected files when it's
/// one of them, else for it alone, whatever the others are.
#[test]
fn a_click_on_a_files_priority_cycles_it_for_the_selection_it_belongs_to() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    tap(&mut rt, "files/viewport/rows/0");
    rt.key(shift(Key::Down));
    settle(&mut rt);
    click_priority(&mut rt, 2);
    let priorities = |rt: &Runtime<Phantom>| files(rt).into_iter().map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!(priorities(&rt), [Priority::Normal, Priority::Normal, Priority::Off], "not selected: it alone");
    click_priority(&mut rt, 0);
    assert_eq!(priorities(&rt), [Priority::Off, Priority::Off, Priority::Off], "selected: the selection, from it");
    click_priority(&mut rt, 1);
    assert_eq!(priorities(&rt), [Priority::Normal, Priority::Normal, Priority::Off], "and back");
}

/// What's open and revealed in Finder, recorded instead of done.
fn finder(rt: &mut Runtime<Phantom>) -> crate::platform::Shown {
    let shown = crate::platform::Shown::default();
    rt.state.finder = Box::new(shown.clone());
    shown
}

#[test]
fn each_open_key_opens_the_selected_torrent_and_shift_cmd_o_reveals_it() {
    use crate::platform::Show;
    let mut rt = app(with_an_album());
    let shown = finder(&mut rt);
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    for key in shortcuts::keys(Command::Open) {
        rt.key(key.event());
        settle(&mut rt);
    }
    shortcut(&mut rt, Command::Reveal);
    let hows: Vec<Show> = shown
        .all()
        .into_iter()
        .map(|(path, how)| {
            assert!(path.ends_with("Downloads/Album"), "the torrent's folder: {path:?}");
            how
        })
        .collect();
    assert_eq!(hows, [Show::Open, Show::Open, Show::Open, Show::Reveal], "⌘O, ⌘↓ and Return open; ⇧⌘O reveals");
}

#[test]
fn cmd_backspace_deletes_at_once_and_backspace_asks() {
    let service = with_an_album();
    let probe = service.probe();
    let mut rt = app(service);
    tap(&mut rt, "rows/0");
    shortcut(&mut rt, Command::Delete);
    assert!(drawn_menu(&rt, "remove-menu").is_none(), "no confirmation");
    assert_eq!(probe.commands(), [(vec![0], TorrentCommand::Delete)], "a Delete, not a Remove");
    assert_eq!(rt.state.torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [1], "deleted");
    tap(&mut rt, "rows/1");
    shortcut(&mut rt, Command::Remove);
    let menu = drawn_menu(&rt, "remove-menu").expect("⌫ asks");
    assert_eq!(menu, [Some("Remove torrent (keep files)".to_owned()), Some("Delete torrent (files to the Trash)".to_owned())]);
}

/// A Delete that takes its time shows Deleting, with nothing else to do to it, until it's done.
#[test]
fn a_slow_delete_shows_deleting_until_it_is_done() {
    let mut rt = app(with_an_album().trash_takes(1.0));
    tap(&mut rt, "rows/0");
    shortcut(&mut rt, Command::Delete);
    assert_eq!(find_torrent(&rt, 0).status, Status::Deleting);
    assert_eq!(list_cells(&rt)[&0].get("status").map(String::as_str), Some("Deleting"));
    assert!(!enabled(&rt, "pause") && !enabled(&rt, "resume"), "nothing to do to it meanwhile");
    for _ in 0..70 {
        rt.frame(FRAME);
    }
    settle(&mut rt);
    assert_eq!(rt.state.torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [1], "deleted");
}

/// A Delete of several goes to every one; those the Trash refused show together, as one entry.
#[test]
fn a_delete_of_several_reports_its_failures_together() {
    let service =
        MemoryService::new().with_torrents(vec![torrent(0, "a"), torrent(1, "b"), torrent(2, "c")]).trash_refusing("a").trash_refusing("c");
    let mut rt = app(service);
    tap(&mut rt, "rows/0");
    rt.key(KeyEvent { key: Key::Command('a'), mods: Mods { cmd: true, ..Default::default() } });
    settle(&mut rt);
    shortcut(&mut rt, Command::Delete);
    settle(&mut rt);
    assert_eq!(rt.state.torrents.iter().map(|t| t.id).collect::<Vec<_>>(), [0, 2], "b went; a and c stay as they were");
    let entries = listed(&rt);
    assert_eq!(entries.len(), 1, "one entry: {entries:?}");
    assert_eq!(entries[0][0], "Couldn't delete 2 of 3");
    let mut named = [entries[0][1].as_str(), entries[0][3].as_str()];
    named.sort();
    assert_eq!(named, ["a", "c"], "each by name");
    assert!(entries[0][2].starts_with("Couldn't move everything to the Trash: it's locked"), "{entries:?}");
}

#[test]
fn backspace_in_the_link_field_edits_the_text() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/0");
    tap(&mut rt, "magnet-field");
    rt.paste("ab");
    shortcut(&mut rt, Command::Remove);
    assert_eq!(rt.state.magnet.text, "a");
    assert!(drawn_menu(&rt, "remove-menu").is_none(), "the field's, not Remove…");
}

#[test]
fn the_row_menu_shows_each_items_shortcut() {
    let mut rt = app(with_an_album());
    tap(&mut rt, "rows/1");
    settle(&mut rt);
    right_click_row(&mut rt, 1);
    let elements = rt.elements();
    let hint = |label: &str| {
        let item =
            elements.iter().find(|e| e.role == "menuitem" && e.label.as_deref() == Some(label)).unwrap_or_else(|| panic!("no {label}"));
        let inside = format!("{}/", item.path);
        elements.iter().filter(|e| e.path.starts_with(&inside)).filter_map(|e| e.text.clone()).find(|t| t != label)
    };
    assert_eq!(hint("Open").as_deref(), Some("⌘O"));
    assert_eq!(hint("Reveal in Finder").as_deref(), Some("⇧⌘O"));
    assert_eq!(hint("Remove torrent (keep files)").as_deref(), Some("⌫"));
    assert_eq!(hint("Delete torrent (files to the Trash)").as_deref(), Some("⌘⌫"));
    assert_eq!(hint("Pause"), None);
}

#[test]
fn add_file_picks_a_torrent_file_and_adds_it_from_its_button_or_shift_cmd_a() {
    let mut rt = app(MemoryService::new());
    let help = rt.elements().into_iter().find(|e| e.path.ends_with("add-file")).and_then(|e| e.help);
    assert_eq!(help.as_deref(), Some("Add a .torrent file (⇧⌘A)"));
    tap(&mut rt, "add-file");
    let (kind, options) = rt.file_dialog().expect("a picker");
    assert_eq!((kind, options.title.as_str()), (rune::FileDialogKind::Open, "Add torrent file"));
    rt.resolve_file_dialog(Some("/tmp/Sintel.torrent".into())).unwrap();
    settle(&mut rt);
    assert_eq!(rt.state.torrents.iter().map(|t| t.display_name()).collect::<Vec<_>>(), ["Sintel"]);
    shortcut(&mut rt, Command::AddFile);
    assert!(rt.file_dialog().is_some(), "⇧⌘A opens it too");
    rt.resolve_file_dialog(None).unwrap();
}

/// The service's settings reset at its start come with its snapshots, and show as a warning once.
#[test]
fn settings_the_service_reset_show_from_its_snapshot() {
    let mut rt = app(MemoryService::new().with_reset(&["upnp"]));
    settle(&mut rt);
    let notice = rt.state.notice.clone().expect("a notice");
    assert_eq!(Some(notice.short.clone()), client::display::settings_reset(&["upnp".into()]));
    rt.state.notice = None;
    wait(&mut rt, 1.0);
    assert!(rt.state.notice.is_none(), "told of once");
}

/// Examples, not properties: `Notice` holds closures, which can't be generated or compared, so
/// its short line is checked by value and its view through what's drawn (#120).
#[test]
fn a_notice_prints_its_short_line_and_the_explained_preset_draws_its_buttons() {
    use crate::notice::{Notice, NoticeAction, explained};
    use crate::ui::Ui;

    assert_eq!(format!("{:?}", Notice::new("3 settings were reset")), "Notice(\"3 settings were reset\")");
    assert!(Notice::new("x").more.is_none());

    let view = |s: &Phantom, cx: &mut rune::Cx<Phantom>| {
        let ui = Ui::new(cx.system_dark());
        let run: std::rc::Rc<dyn Fn(&mut Phantom)> = std::rc::Rc::new(|s| s.notice = Some(Notice::new("Revealed")));
        let actions = vec![NoticeAction { title: "Reveal in Finder".into(), run }];
        ui.page(0.0).child(explained("Your settings file was unreadable.".into(), actions)(s, &ui))
    };
    let mut rt = Runtime::new(Phantom::new(Box::new(MemoryService::new())), view, (960.0, 540.0), Env { system_dark: true, scale: 2.0 });
    rt.frame(0.0);
    rt.frame(FRAME);
    assert_eq!(texts(&rt, "notice-explained"), vec!["Your settings file was unreadable.", "Reveal in Finder"]);
    tap(&mut rt, "notice-action/0");
    assert_eq!(notice(&rt), Some("Revealed"), "the action runs on the state");
}

/// An example, not a property: colours are what's drawn, which only a comparison of two draws
/// shows. The status bar reads a notice's severity in its colour again.
#[test]
fn the_status_bar_shows_a_notice_in_the_colour_of_its_severity() {
    use crate::notice::Notice;
    let mut colours = Vec::new();
    for notice in [Notice::new("Same words"), Notice::warning("Same words"), Notice::error("Same words")] {
        let mut rt = app(MemoryService::new());
        rt.update(move |s, _| s.notice = Some(notice.clone()));
        settle(&mut rt);
        colours.push(text_color(&rt, "notice", "Same words"));
    }
    assert!(colours[0] != colours[1] && colours[1] != colours[2] && colours[0] != colours[2], "{colours:?}");
}

/// An example, not a property: an overlap is a matter of every frame of an animation, which the
/// generated steps (that land springs between steps) don't draw. While an entry fades out and the
/// panel closes over it, no two entries, the leaving one included, overlap on any frame.
#[test]
fn a_leaving_entry_never_overlaps_the_next_one() {
    let mut rt = app(with_an_album().rejecting("/a/bad.torrent").with_default_destination(Some("/srv".into())));
    rt.set_viewport((960.0, 900.0), 2.0);
    for _ in 0..3 {
        rt.drop_files(vec!["/a/bad.torrent".into()], None);
        wait(&mut rt, 1.0);
    }
    assert_eq!(entry_keys(&rt).len(), 3);
    let (x, y) = rt.find("error/1").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    let mut frames = 0;
    while frames < 90 {
        rt.frame(FRAME);
        frames += 1;
        // What's drawn: a leaving entry that has faded out is nothing, wherever its box is.
        let mut rects: Vec<(String, [f32; 4])> = rt
            .elements()
            .into_iter()
            .filter(|e| e.opacity > 0.02 && e.key.as_deref().is_some_and(|k| k.starts_with("error/")))
            .map(|e| (e.key.unwrap(), e.rect))
            .collect();
        rects.sort_by(|a, b| a.1[1].total_cmp(&b.1[1]));
        for pair in rects.windows(2) {
            assert!(pair[0].1[1] + pair[0].1[3] <= pair[1].1[1] + 0.5, "frame {frames}: {pair:?} overlap");
        }
    }
    assert_eq!(entry_keys(&rt).len(), 2, "and it's gone in the end");
}

/// An example, not a property, for the reason above: the settings-reset notice explains, and its
/// Open settings opens the sheet on the tab of the setting reset.
#[test]
fn the_settings_reset_notice_opens_settings_on_the_tab_of_what_was_reset() {
    use crate::model::settings::{Tab, reset_notice};
    use crate::ui::Ui;

    fn reset() -> crate::notice::Notice {
        reset_notice(&["app_icon".into()]).expect("one was reset")
    }
    assert!(reset_notice(&[]).is_none());
    assert_eq!(reset().short, "1 setting was reset to its default: App icon");
    let view = |s: &Phantom, cx: &mut rune::Cx<Phantom>| {
        let ui = Ui::new(cx.system_dark());
        ui.page(0.0).child(reset().more.expect("it explains")(s, &ui))
    };
    let mut rt = Runtime::new(Phantom::new(Box::new(MemoryService::new())), view, (960.0, 540.0), Env { system_dark: true, scale: 2.0 });
    rt.frame(0.0);
    rt.frame(FRAME);
    assert_eq!(texts(&rt, "notice-explained")[1], "Open settings");
    tap(&mut rt, "notice-action/0");
    assert_eq!(rt.state.settings.as_ref().map(|sheet| sheet.tab), Some(Tab::AppIcon));
}
