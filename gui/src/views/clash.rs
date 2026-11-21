//! The clash dialog (#24, #270): a Move whose torrent has files in the new folder already asks what
//! to do with them, for each torrent of a selection that does, one after another. Use existing and
//! Replace are that torrent's choice; Cancel and Escape skip only it; Reveal in Finder, under the
//! files, shows them, the dialog staying open. With more to ask, "Apply to the remaining N" gives the choice to them too.
//! Rename… turns the dialog into a name field (#335's rules and notes) and Move: the torrent moves
//! in under that name; Escape goes back to the choices. Files of another listed torrent offer only
//! Rename… and Cancel, and the dialog names that torrent.

use client::display;
use rune::{col, row};

use crate::model::Phantom;
use crate::model::moving::{Clash, apply_to_rest, choose, close_name, rename, reveal, submit_name};
use crate::ui::Ui;

/// The open clash dialog, in `layer`.
pub fn overlay(s: &Phantom, ui: &Ui, layer: &mut Vec<rune::El>) {
    let Some(clash) = &s.clash else { return };
    let [use_existing, replace, cancel, show, rename_to, move_in] = display::clash_choices();
    let cancel = ui.button("clash-cancel", cancel).on_press(|s: &mut Phantom| choose(s, None)).build();
    let title = display::clash_title(&clash.to);
    let close = |s: &mut Phantom| choose(s, None);
    if clash.renaming.is_some() {
        let question = display::clash_new_name(&clash.name, &clash.to);
        let buttons = vec![rune::el().grow(1.0), cancel, ui.button("clash-move", move_in).primary().on_press(submit_name).build()];
        return ui.dialog("clash", &title, &question, Some(name_field(clash, ui)), buttons, close, layer);
    }
    let rest = (clash.remaining > 0).then(|| {
        ui.checkbox("clash-apply-to-rest", &display::clash_apply_to_remaining(clash.remaining), clash.apply_to_rest, apply_to_rest)
    });
    let rename_button = |primary: bool| {
        let button = ui.button("clash-rename", rename_to).on_press(rename);
        if primary { button.primary() } else { button }.build()
    };
    let (question, body, buttons) = match &clash.other {
        Some(other) => {
            let body = col().gap(12.0).child(ui.dialog_note(display::clash_help_another())).children(rest);
            (display::clash_question_another(&clash.name, other, &clash.to), body, vec![rune::el().grow(1.0), cancel, rename_button(true)])
        }
        None => {
            let lines = clash.files.iter().take(5).map(|f| ui.dialog_line(&f.display().to_string()));
            let files = col().key("clash-files").gap(2.0).children(lines);
            // Beside the files it shows, so the foot holds the four choices.
            let reveal = row().child(ui.button("clash-reveal", show).on_press(reveal).build());
            let body = col().gap(12.0).child(files).child(reveal).child(ui.dialog_note(display::clash_help())).children(rest);
            let buttons = vec![
                rune::el().grow(1.0),
                cancel,
                rename_button(false),
                ui.button("clash-replace", replace).on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::Replace))).build(),
                ui.button("clash-use-existing", use_existing)
                    .primary()
                    .on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::UseExisting)))
                    .build(),
            ];
            (display::clash_question(&clash.name, &clash.files, &clash.to), body, buttons)
        }
    };
    ui.dialog("clash", &title, &question, Some(body), buttons, close, layer);
}

/// Rename…'s field: Return moves the torrent in under the name, Escape goes back to the choices; a
/// name a file can't have is said under it, in #335's words, until the text changes.
fn name_field(clash: &Clash, ui: &Ui) -> rune::El {
    let Some(naming) = &clash.renaming else { return col() };
    let field = ui
        .text_field("clash-name-field", &naming.field, "Name")
        .on_edit(|s: &mut Phantom| &mut s.clash.as_mut().and_then(|c| c.renaming.as_mut()).expect("Rename… is open").field)
        .on_submit(|s: &mut Phantom, _| submit_name(s))
        .on_key(|s: &mut Phantom, ev: &rune::KeyEvent, _: &mut rune::Tx| match ev.key {
            rune::Key::Escape => {
                close_name(s);
                true
            }
            _ => false,
        })
        .build();
    let note = naming.note().map(|why| ui.error_caption(display::name_problem(why)).key("clash-name-note"));
    col().w_full().gap(4.0).child(field).children(note)
}
