//! The command palette (#33), while it's open: its field, and the commands `model::palette` lists
//! for its text, each with its icon and key. Its keys: ↑ and ↓ move the highlight, Return runs it,
//! Escape closes; the field keeps the rest for its text.

use rune::{El, Key, KeyEvent, Tx};

use crate::model::Phantom;
use crate::model::palette::{self, listed, row_key};
use crate::ui::{PaletteRow, Ui};

/// `window` is the window's size, when known.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    if !s.palette.is_open() {
        return;
    }
    let field = ui
        .text_field("palette-field", &s.palette.query, "Command")
        .placeholder("Type a command")
        .on_edit(|s: &mut Phantom| &mut s.palette.query)
        .on_submit(palette::run)
        .on_key(on_key)
        .build();
    let rows = listed(s).into_iter().map(|e| PaletteRow { label: e.label, hint: e.hint, icon: e.icon }).collect();
    ui.palette(
        "palette",
        field,
        rows,
        s.palette.at(),
        "No command matches",
        window.map(|(w, _)| w),
        row_key,
        palette::pick,
        palette::close,
        layer,
    );
}

/// The palette's own keys, before the field's.
fn on_key(s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Up => palette::up(s, tx),
        Key::Down => palette::down(s, tx),
        Key::Escape => palette::close(s),
        _ => return false,
    }
    true
}
