//! The clash dialog (#24, #270): a Move whose torrent has files in the new folder already asks what
//! to do with them, for each torrent of a selection that does, one after another. Use existing and
//! Replace are that torrent's choice; Cancel and Escape skip only it; Reveal in Finder shows them,
//! the dialog staying open. With more to ask, "Apply to the remaining N" gives the choice to them too.

use client::display;
use rune::col;

use crate::model::Phantom;
use crate::model::moving::{apply_to_rest, choose, reveal};
use crate::ui::Ui;

/// The open clash dialog, in `layer`.
pub fn overlay(s: &Phantom, ui: &Ui, layer: &mut Vec<rune::El>) {
    let Some(clash) = &s.clash else { return };
    let [use_existing, replace, cancel, show] = display::clash_choices();
    let files = col().key("clash-files").gap(2.0).children(clash.files.iter().take(5).map(|f| ui.dialog_line(&f.display().to_string())));
    let rest = (clash.remaining > 0).then(|| {
        ui.checkbox("clash-apply-to-rest", &display::clash_apply_to_remaining(clash.remaining), clash.apply_to_rest, apply_to_rest)
    });
    let body = col().gap(12.0).child(files).child(ui.dialog_note(display::clash_help())).children(rest);
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
