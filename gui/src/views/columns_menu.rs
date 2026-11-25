//! The list header's menu (#33): a right-click on the header lists every column, in the list's
//! order, each ticked while it's wanted; a pick shows or hides it, kept in `gui.json`. A wanted
//! column the window has no room for says so. The list always keeps one column: a pick of the last
//! one showing leaves it, and its entry is dimmed. The menu stays open as columns are ticked, so
//! several can be shown or hidden in one go (#352); Escape or a press outside closes it.

use rune::{El, PointerEvent, Tx};

use crate::menu::{Event, Menu, Open};
use crate::model::Phantom;
use crate::model::layout::{Column, TABLE_HEADER};
use crate::ui::{Entry, Icon, Ui};

/// A right-click on the list at `press`, in its own points: on its header, the menu opens there.
pub fn open(s: &mut Phantom, press: PointerEvent, _: &mut Tx) {
    if press.y <= TABLE_HEADER {
        crate::overlay::open_menu(s, Open::Columns { at: (press.window_x, press.window_y) });
    }
}

/// Every column in the list's order, and whether it's ticked: while wanted; the one column left when
/// every other is hidden shows, so it's ticked too. `shown` is what the list draws.
pub fn wanted(s: &Phantom, shown: &[Column]) -> Vec<(Column, bool)> {
    s.prefs.order().columns().iter().map(|&c| (c, !s.prefs.hidden().contains(&c) || shown == [c])).collect()
}

/// The open menu, in `layer`; `window` is the window's size, when known, which the menu stays inside;
/// `shown` is what the list draws now.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, shown: &[Column], layer: &mut Vec<El>) {
    let Menu::Open(Open::Columns { at }) = s.menu else { return };
    let shown_now = shown.to_vec();
    let entries: Vec<Entry> = wanted(s, shown)
        .into_iter()
        .map(|(c, wanted)| {
            let (id, label, icon, checked) = (c.id().into(), c.title().into(), wanted.then_some(Icon::Check), Some(wanted));
            if shown == [c] {
                return Entry::Disabled { id, label, icon, checked };
            }
            Entry::Choice { id, label, hint: (wanted && !shown.contains(&c)).then(|| client::display::no_room().into()), icon, checked }
        })
        .collect();
    ui.context_menu(
        "columns-menu",
        Some(at),
        window,
        entries,
        move |s: &mut Phantom, picked, _| {
            if let Some(column) = Column::from_id(picked) {
                s.prefs.toggle(column, &shown_now);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}
