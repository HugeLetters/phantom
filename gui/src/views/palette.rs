//! The command palette (#33, #434) of either window, while it's open: its field, and the entries `model::palette`
//! lists for its text, each with its icon and key, a setting with its place and value. Its keys: ↑ and
//! ↓ move the highlight, Return picks it, ⇧Return picks it and stays, ⌘Return shows a setting in
//! Settings, Escape goes back one
//! level; the field keeps the rest for its text.

use rune::{El, Key, KeyEvent, Tx};

use crate::model::palette::{self, Host, heading, listed, row_key};
use crate::ui::{PaletteRow, Ui};

/// The palette of either window, over its page: the same panel, rows and keys. `window` is the
/// window's size, when known.
pub fn overlay<H: Host>(s: &H, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    if !s.palette().is_open() {
        return;
    }
    let field = ui
        .text_field("palette-field", &s.palette().query, "Commands and settings")
        .placeholder(if s.palette().sub.is_some() { "Choose" } else { "Type a command or a setting" })
        .on_edit(|s: &mut H| &mut s.palette_mut().query)
        .on_submit(palette::run::<H>)
        .on_change(|s: &mut H, _: &mut Tx| palette::edited(s))
        .on_key(on_key::<H>)
        .build();
    let mut group = None;
    let rows = listed(s)
        .into_iter()
        .map(|e| {
            let first = group != Some(e.group);
            group = Some(e.group);
            let read = e.read();
            PaletteRow {
                heading: first.then(|| heading(s, e.group)),
                label: e.label,
                hint: e.hint,
                icon: e.icon,
                place: e.place,
                value: e.value,
                selected: e.selected,
                read,
            }
        })
        .collect();
    let choices = s.palette().sub.as_ref().map(|sub| {
        let n = palette::choices_count(s, sub);
        client::display::choices_of(&sub.name, n)
    });
    ui.palette(
        "palette",
        field,
        choices,
        rows,
        palette::highlighted(s),
        "No command matches",
        window.map(|(w, _)| w),
        row_key,
        palette::pick::<H>,
        palette::hover::<H>,
        palette::dismiss::<H>,
        layer,
    );
}

/// The palette's own keys, before the field's.
fn on_key<H: Host>(s: &mut H, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Up => palette::up(s, tx),
        Key::Down => palette::down(s, tx),
        Key::Escape => palette::back(s, tx),
        Key::Enter if ev.mods.cmd => palette::show_highlighted(s, tx),
        Key::Enter if ev.mods.shift => palette::run_and_stay(s, tx),
        // With nothing typed, ⌘Z is the app's, not the field's own: it takes back what ⇧Return ran.
        Key::Command('z') if ev.mods.cmd && !ev.mods.shift && s.palette().query.text.is_empty() => s.undo(),
        _ => return false,
    }
    true
}
