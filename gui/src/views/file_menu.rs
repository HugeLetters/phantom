//! The context menu of the selected files, in the details pane (#151): the file commands available
//! (`commands`): Open and Reveal in Finder for one file on disk, the priorities under their title
//! with the current one checked (none for a mixed selection), and Move to Trash, each with its icon
//! and key. What it offers is the list's call; this groups the items and runs the one picked.

use proto::Priority;
use rune::{El, PointerPress, Tx};

use crate::commands::{OnFiles, on_files};
use crate::menu::{Event, Menu, Open};
use crate::model::{Phantom, common_priority, file_target, look_at_disk, select_file_for_menu, set_selected_files, trash_selected_files};
use crate::platform::{Show, show_file};
use crate::ui::{Entry, Ui};

/// Opens the menu at `press` for a right-click on file `index`'s row.
pub fn open(s: &mut Phantom, index: usize, press: PointerPress, _: &mut Tx) {
    select_file_for_menu(&mut s.pane, index);
    look_at_disk(s, file_target(&s.table, &s.torrents, &s.pane));
    crate::overlay::open_menu(s, Open::File { at: (press.x, press.y) });
}

/// The open file menu, in `layer`.
/// `window` is the window's size, when known: the menu stays inside it.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    let Menu::Open(Open::File { at }) = s.menu else { return };
    let checked = common_priority(&s.table, &s.torrents, &s.pane);
    ui.context_menu(
        "file-menu",
        Some(at),
        window,
        entries(&on_files(s), checked),
        move |s: &mut Phantom, picked, _| {
            s.menu = s.menu.then(Event::Close);
            if let Some(item) = on_files(s).into_iter().find(|i| i.spec().id == picked) {
                run(s, item);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}

/// The menu's entries: Open and Reveal, then the priorities under their title, then Move to Trash,
/// a separator between the groups that are there. The current priority is checked: a check at the
/// item's right edge, as each item has its own icon.
pub fn entries(items: &[OnFiles], checked: Option<Priority>) -> Vec<Entry> {
    let mut entries = Vec::new();
    for group in 0..3u8 {
        let in_group: Vec<OnFiles> = items.iter().copied().filter(|i| i.group() == group).collect();
        if in_group.is_empty() {
            continue;
        }
        if !entries.is_empty() {
            entries.push(Entry::Separator);
        }
        if group == 1 {
            entries.push(Entry::Title(client::display::priority_title().into()));
        }
        for item in in_group {
            let spec = item.spec();
            entries.push(Entry::Choice {
                id: spec.id,
                label: spec.name.into(),
                hint: spec.hint(),
                icon: Some(spec.icon),
                checked: match item {
                    OnFiles::Priority(p) => Some(checked == Some(p)),
                    OnFiles::Open | OnFiles::Reveal | OnFiles::MoveToTrash => None,
                },
            });
        }
    }
    entries
}

/// Does what `item` says to the selected files.
pub fn run(s: &mut Phantom, item: OnFiles) {
    match item {
        OnFiles::Open => show_file(s, Show::Open),
        OnFiles::Reveal => show_file(s, Show::Reveal),
        OnFiles::Priority(priority) => set_selected_files(s, priority),
        OnFiles::MoveToTrash => trash_selected_files(s),
    }
}
