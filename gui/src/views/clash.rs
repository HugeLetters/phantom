//! The clash dialog (#24, #270): a Move whose torrent has files in the new folder already asks what
//! to do with them, for each torrent of a selection that does, one after another. Its title is the
//! question, with where it is among several ("· 1 of 3"); under it up to three of the paths, cut in
//! the middle, then "and 2 more", Reveal in Finder small at the end of the first; then a row per
//! choice, Use existing (Return), Replace and Rename…, each saying what it does only in its
//! tooltip. Along the foot, with more to ask, "Apply to the remaining N", and Cancel, which skips
//! only this torrent, as Escape does. At the smallest window the paths give way first, to one, so
//! the choices always fit.
//!
//! Rename… turns the dialog into a name field (#335's rules) and Move: the torrent moves in under
//! that name; Escape goes back to the choices. Files of another listed torrent: "Album can't go
//! into Films", that torrent's path, and only Rename… and Cancel.

use client::display;
use rune::{El, Key, KeyEvent, Tx, col, el, row};

use crate::model::Phantom;
use crate::model::moving::{Clash, apply_to_rest, choose, close_name, rename, reveal, submit_name};
use crate::ui::Ui;

/// The most paths it shows, and the most at a window shorter than [`ROOMY`].
const PATHS: usize = 3;
const ROOMY: f32 = 420.0;

/// The open clash dialog, in `layer`. `window` is the window's size, when known.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    let Some(clash) = &s.clash else { return };
    let [use_existing, replace, cancel, show, rename_to, move_in] = display::clash_choices();
    let [use_help, replace_help, rename_help] = display::clash_choice_help();
    let cancel = || ui.button("clash-cancel", cancel).on_press(|s: &mut Phantom| choose(s, None)).build();
    let close = |s: &mut Phantom| choose(s, None);
    if clash.renaming.is_some() {
        let title = display::clash_title(&display::clash_new_name(&clash.name, &clash.to), clash.at, clash.of);
        let foot = row()
            .key("buttons")
            .w_full()
            .gap(8.0)
            .child(el().grow(1.0))
            .child(cancel())
            .child(ui.button("clash-move", move_in).primary().on_press(submit_name).build());
        let body = name_field(clash, ui);
        ui.dialog("clash", &title, body, foot, close, |_: &mut Phantom, _: &KeyEvent, _: &mut Tx| false, layer);
        // Over the dialog, under its field.
        if let Some(why) = clash.renaming.as_ref().and_then(|n| n.note()) {
            ui.tooltip_under("clash-name-note", "clash-name-field", &display::name_problem(why), layer);
        }
        return;
    }
    let rest = (clash.remaining > 0).then(|| {
        ui.checkbox("clash-apply-to-rest", &display::clash_apply_to_remaining(clash.remaining), clash.apply_to_rest, apply_to_rest)
    });
    let rename_button = |primary: bool| {
        let button = ui.button("clash-rename", rename_to).on_press(rename);
        let button = if primary { button.primary() } else { button.fill() };
        button.build().help(rename_help)
    };
    let room = window.is_none_or(|(_, h)| h >= ROOMY);
    let paths = paths(clash, ui, show, if room { PATHS } else { 1 });
    match &clash.other {
        Some(_) => {
            let title = display::clash_title(&display::clash_question_another(&clash.name, &clash.to), clash.at, clash.of);
            let body = col().key("clash-body").gap(6.0).child(ui.dialog_line(display::clash_another_keeps())).child(paths);
            let foot = row()
                .key("buttons")
                .w_full()
                .gap(8.0)
                .center_y()
                .children(rest)
                .child(el().grow(1.0))
                .child(rename_button(true))
                .child(cancel());
            let on_key = |s: &mut Phantom, ev: &KeyEvent, _: &mut Tx| return_does(ev, || rename(s));
            ui.dialog("clash", &title, body, foot, close, on_key, layer);
        }
        None => {
            let title = display::clash_title(&display::clash_question(&clash.name, &clash.files, &clash.to), clash.at, clash.of);
            let choices = col()
                .key("clash-choices")
                .w_full()
                .gap(6.0)
                .child(
                    ui.button("clash-use-existing", use_existing)
                        .primary()
                        .fill()
                        .on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::UseExisting)))
                        .build()
                        .help(use_help),
                )
                .child(
                    ui.button("clash-replace", replace)
                        .fill()
                        .on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::Replace)))
                        .build()
                        .help(replace_help),
                )
                .child(rename_button(false));
            let body = col().key("clash-body").w_full().gap(14.0).child(paths).child(choices);
            let foot = row().key("buttons").w_full().gap(8.0).center_y().children(rest).child(el().grow(1.0)).child(cancel());
            let on_key = |s: &mut Phantom, ev: &KeyEvent, _: &mut Tx| return_does(ev, || choose(s, Some(proto::Clash::UseExisting)));
            ui.dialog("clash", &title, body, foot, close, on_key, layer);
        }
    }
}

/// Return, wherever the focus is in the dialog, is its first choice: `act`.
fn return_does(ev: &KeyEvent, act: impl FnOnce()) -> bool {
    if matches!(ev.key, Key::Enter) && ev.mods == rune::Mods::default() {
        act();
        return true;
    }
    false
}

/// Up to `most` of the clashing paths, one a line, cut in the middle; "and N more" for the rest;
/// Reveal in Finder, small, at the end of the first. Another torrent's place with no file there yet
/// is where this one would go.
fn paths(clash: &Clash, ui: &Ui, show: &str, most: usize) -> El {
    let place = clash.to.join(&clash.name);
    let files: Vec<&std::path::Path> = match clash.files.is_empty() {
        true => vec![place.as_path()],
        false => clash.files.iter().map(|f| f.as_ref()).collect(),
    };
    let lines = files.iter().take(most).enumerate().map(|(i, f)| {
        let path = ui.dialog_path(&f.display().to_string());
        let line = row().w_full().gap(8.0).center_y().child(path);
        if i == 0 { line.child(el().grow(1.0)).child(ui.button("clash-reveal", show).small().on_press(reveal).build()) } else { line }
    });
    let first: Vec<El> = lines.collect();
    let more = files.len().saturating_sub(most);
    let more = (more > 0).then(|| ui.dialog_line(&display::clash_more(more)));
    col().key("clash-files").w_full().gap(2.0).children(first).children(more)
}

/// Rename…'s field: Return moves the torrent in under the name, Escape goes back to the choices; a
/// name a file can't have is said in a tooltip under it ([`overlay`]), until the text changes.
fn name_field(clash: &Clash, ui: &Ui) -> El {
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
    col().w_full().child(field)
}
