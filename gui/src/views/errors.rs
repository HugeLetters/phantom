//! The error list: every entry, oldest first, under the toolbar.

use rune::El;

use crate::model::Phantom;
use crate::ui::Ui;

/// Rows of the page above the list: the toolbar, and the gap under it.
const TOOLBAR: f32 = 28.0 + 10.0;
/// How far down a message by Add reaches, so the list starts below it: three lines and padding.
const ADD_NOTE: f32 = 76.0;

/// The panel; `inset` is the titlebar's, as for the page.
pub fn error_layer(s: &Phantom, ui: &Ui, inset: f32) -> El {
    let mut top = ui.page_top(inset) + TOOLBAR;
    if s.add_note.as_ref().is_some_and(|n| n.text == s.magnet.text) {
        top += ADD_NOTE;
    }
    let entries = s.errors.entries().iter().map(|entry| {
        let id = entry.id;
        let more = entry.notice.more.as_ref().map(|view| view(s, ui));
        ui.error_entry(
            format!("error/{id}"),
            &entry.notice.short,
            super::status_bar::tone_of(entry.notice.severity),
            more,
            entry.leaving,
            move |s: &mut Phantom| s.errors.dismiss(id),
        )
    });
    let clear_all = ui.button("clear-all", client::display::clear_all()).on_press(|s: &mut Phantom| s.errors.clear_all()).build();
    ui.error_layer(top, client::display::errors_title(), clear_all, entries.collect())
}
