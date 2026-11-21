//! The list header's menu (#33): a right-click on the header lists every column, in the list's
//! order, each ticked while it's wanted; a pick shows or hides it, kept in `gui.json`. A wanted
//! column the window has no room for says so. The list always keeps one column: a pick of the last
//! one showing leaves it.

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

/// The open menu, in `layer`; `window` is the window's size, when known, which the menu stays inside;
/// `shown` is what the list draws now.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, shown: &[Column], layer: &mut Vec<El>) {
    let shown_now = shown.to_vec();
    let Menu::Open(Open::Columns { at }) = s.menu else { return };
    let entries: Vec<Entry> = s
        .prefs
        .order()
        .columns()
        .iter()
        .copied()
        .map(|c| {
            let wanted = !s.prefs.hidden().contains(&c);
            Entry::Choice {
                id: c.id(),
                label: c.title().into(),
                hint: (wanted && !shown.contains(&c)).then(|| client::display::no_room().into()),
                icon: wanted.then_some(Icon::Check),
                checked: Some(wanted),
            }
        })
        .collect();
    ui.context_menu(
        "columns-menu",
        Some(at),
        window,
        entries,
        move |s: &mut Phantom, picked, _| {
            s.menu = s.menu.then(Event::Close);
            if let Some(column) = Column::from_id(picked) {
                s.prefs.toggle(column, &shown_now);
            }
        },
        |s: &mut Phantom| s.menu = s.menu.then(Event::Close),
        layer,
    );
}
