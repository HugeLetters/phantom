//! The context menu of the selected files, in the details pane (#151): Open and Reveal in Finder
//! for one file on disk, the priorities under their title with the current one checked (none for a
//! mixed selection), and Move to Trash. What it offers is [`file_items`]'s call; this words the
//! items, shows each one's shortcut (from the one table), and runs the one picked.

use proto::Priority;
use rune::{El, PointerPress, Tx};

use crate::menu::{Event, Menu, Open};
use crate::model::{FileItem, Phantom, common_priority, file_items, select_file_for_menu, set_selected_files, trash_selected_files};
use crate::platform::{Show, show_file};
use crate::shortcuts::{self, Command};
use crate::ui::{Entry, Icon, Ui};

/// Opens the menu at `press` for a right-click on file `index`'s row.
pub fn open(s: &mut Phantom, index: usize, press: PointerPress, _: &mut Tx) {
    select_file_for_menu(&mut s.pane, index);
    crate::overlay::open_menu(s, Open::File { at: (press.x, press.y) });
}

/// The open file menu, in `layer`.
pub fn overlay(s: &Phantom, ui: &Ui, layer: &mut Vec<El>) {
    let Menu::Open(Open::File { at }) = s.menu else { return };
    let items = file_items(&s.table, &s.torrents, &s.pane);
    let checked = common_priority(&s.table, &s.torrents, &s.pane);
    ui.context_menu(
        "file-menu",
        Some(at),
        entries(&items, checked),
        move |s: &mut Phantom, picked, _| {
            s.menu = s.menu.then(Event::Close);
            if let Some(item) = file_items(&s.table, &s.torrents, &s.pane).into_iter().find(|i| item_id(*i) == picked) {
                run(s, item);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}

/// The menu's entries: Open and Reveal, then the priorities under their title, then Move to Trash,
/// a separator between the groups that are there.
pub fn entries(items: &[FileItem], checked: Option<Priority>) -> Vec<Entry> {
    let show = |items: &[FileItem], group: u8| -> Vec<FileItem> { items.iter().copied().filter(|i| group_of(*i) == group).collect() };
    let mut entries = Vec::new();
    for group in 0..3u8 {
        let in_group = show(items, group);
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
            entries.push(Entry::Choice {
                id: item_id(item),
                label: label(item),
                hint: shortcut(item).map(shortcuts::hint),
                // The current priority has the check in the icon gutter, as the row menu has its Copy
                // icon; none of a mixed selection has.
                icon: matches!(item, FileItem::Priority(p) if checked == Some(p)).then_some(Icon::Check),
                checked: match item {
                    FileItem::Priority(p) => Some(checked == Some(p)),
                    FileItem::Open | FileItem::Reveal | FileItem::MoveToTrash => None,
                },
            });
        }
    }
    entries
}

/// The shortcut that does what the item does, if one does: the same keys as for torrents, acting on
/// files while the file list has focus.
pub fn shortcut(item: FileItem) -> Option<Command> {
    shortcuts::TABLE.iter().map(|(command, _)| *command).find(|command| shortcuts::file_item(*command) == Some(item))
}

fn group_of(item: FileItem) -> u8 {
    match item {
        FileItem::Open | FileItem::Reveal => 0,
        FileItem::Priority(Priority::Normal | Priority::Off) => 1,
        FileItem::MoveToTrash => 2,
    }
}

/// The menu item's id: what a test or script picks.
pub fn item_id(item: FileItem) -> &'static str {
    match item {
        FileItem::Open => "open",
        FileItem::Reveal => "reveal",
        FileItem::Priority(Priority::Normal) => "priority-normal",
        FileItem::Priority(Priority::Off) => "priority-off",
        FileItem::MoveToTrash => "trash",
    }
}

fn label(item: FileItem) -> String {
    match item {
        FileItem::Open => "Open".into(),
        FileItem::Reveal => client::display::reveal_in_finder().into(),
        FileItem::Priority(p) => client::display::priority(p).into(),
        FileItem::MoveToTrash => client::display::move_to_trash().into(),
    }
}

/// Does what `item` says to the selected files.
pub fn run(s: &mut Phantom, item: FileItem) {
    match item {
        FileItem::Open => show_file(s, Show::Open),
        FileItem::Reveal => show_file(s, Show::Reveal),
        FileItem::Priority(priority) => set_selected_files(s, priority),
        FileItem::MoveToTrash => trash_selected_files(s),
    }
}
