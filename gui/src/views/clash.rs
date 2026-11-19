//! The clash dialog (#24): a Move refused because files of the torrent's are in the new folder
//! already asks what to do with them. Use existing and Replace send the Move again with the
//! choice; Cancel and Escape drop it; Reveal in Finder shows them, the dialog staying open.

use client::display;
use rune::col;

use crate::model::Phantom;
use crate::model::moving::{choose, reveal};
use crate::ui::Ui;

/// The open clash dialog, in `layer`.
pub fn overlay(s: &Phantom, ui: &Ui, layer: &mut Vec<rune::El>) {
    let Some(clash) = &s.clash else { return };
    let [use_existing, replace, cancel, show] = display::clash_choices();
    let files = col().key("clash-files").gap(2.0).children(clash.files.iter().take(5).map(|f| ui.dialog_line(&f.display().to_string())));
    let body = col().gap(12.0).child(files).child(ui.dialog_note(display::clash_help()));
    let buttons = vec![
        ui.button("clash-reveal", show).on_press(reveal).build(),
        rune::el().grow(1.0),
        ui.button("clash-cancel", cancel).on_press(|s: &mut Phantom| choose(s, None)).build(),
        ui.button("clash-replace", replace).on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::Replace))).build(),
        ui.button("clash-use-existing", use_existing)
            .primary()
            .on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::UseExisting)))
            .build(),
    ];
    let question = display::clash_question(&clash.name, &clash.files, &clash.to);
    ui.dialog("clash", &display::clash_title(&clash.to), &question, Some(body), buttons, |s: &mut Phantom| choose(s, None), layer);
}
