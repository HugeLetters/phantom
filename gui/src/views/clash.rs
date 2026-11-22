//! The clash dialog (#24, #270): a Move whose torrent has files in the new folder already asks what
//! to do with them, for each torrent of a selection that does, one after another. Its title is the
//! question, with where it is among several ("· 1 of 3"); under it up to three of the paths, cut in
//! the middle, then "and 2 more", Reveal in Finder small at the end of the first; with more to
//! ask, "Apply to the remaining N"; then one row of buttons at the bottom right: Cancel, which skips
//! only this torrent, as Escape does, Rename…, Replace and Use existing (the default, Return), each
//! saying what it does only in its tooltip. Return presses the button with the focus, and the
//! default when none has it. At the smallest window the paths give way first, to
//! one, so the buttons always fit.
//!
//! Rename… turns the dialog into a name field (#335's rules) and Move: the torrent moves in under
//! that name; Escape goes back to the choices. Files of another listed torrent: "Album can't go
//! into Films", "Another torrent in the list keeps its files there:" and its path, and only Show
//! in list (which skips this torrent, as Cancel does, and selects that one in the list), Cancel and
//! Rename… (the default).

use client::display;
use rune::{El, Key, KeyEvent, Tx, col, el, row};

use crate::model::Phantom;
use crate::model::moving::{Clash, apply_to_rest, choose, close_name, explain, rename, reveal, show_in_list, submit_name};
use crate::ui::Ui;

/// The most paths it shows, and the most at a window shorter than [`ROOMY`].
const PATHS: usize = 3;
const ROOMY: f32 = 420.0;

/// The open clash dialog, in `layer`. `window` is the window's size, when known.
pub fn overlay(s: &Phantom, ui: &Ui, window: Option<(f32, f32)>, layer: &mut Vec<El>) {
    let Some(clash) = &s.clash else { return };
    let [use_existing, replace, cancel, show, rename_to, move_in] = display::clash_choices();
    let [use_help, replace_help, rename_help] = display::clash_choice_help();
    // A button with the focus takes Return itself and presses, rather than the dialog's first
    // choice, which sends this torrent's own files to the Trash.
    let presses = |button: El, act: fn(&mut Phantom, &mut Tx)| {
        button.on_key(move |s: &mut Phantom, ev: &KeyEvent, tx: &mut Tx| return_does(ev, || act(s, tx)))
    };
    let cancel = || presses(ui.button("clash-cancel", cancel).on_press(|s: &mut Phantom| choose(s, None)).build(), |s, _| choose(s, None));
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
    // Over the row of buttons, at the left: with four buttons beside it, it wouldn't fit.
    let rest = (clash.remaining > 0).then(|| {
        ui.checkbox("clash-apply-to-rest", &display::clash_apply_to_remaining(clash.remaining), clash.apply_to_rest, apply_to_rest)
    });
    // What a choice does, in a tooltip under it while the pointer rests on it.
    let explained = |key: &'static str, button: El, message: &str| {
        ui.explained(button, message, move |s: &mut Phantom, resting| explain(s, key, resting))
    };
    let rename_button = |primary: bool| {
        let button = ui.button("clash-rename", rename_to).on_press(rename);
        let button = if primary { button.primary() } else { button };
        explained("clash-rename", presses(button.build(), |s, _| rename(s)), rename_help)
    };
    let room = window.is_none_or(|(_, h)| h >= ROOMY);
    let paths = paths(clash, ui, show, if room { PATHS } else { 1 });
    // The buttons along the bottom, at the right, the default last, as macOS orders them.
    let buttons = |choices: Vec<El>| row().key("clash-choices").w_full().gap(8.0).center_y().child(el().grow(1.0)).children(choices);
    let foot = |choices: Vec<El>| col().key("buttons").w_full().gap(10.0).children(rest).child(buttons(choices));
    match &clash.other {
        Some(_) => {
            let title = display::clash_title(&display::clash_question_another(&clash.name, &clash.to), clash.at, clash.of);
            let keeps = ui.dialog_line(display::clash_another_keeps());
            let body = col().key("clash-body").w_full().gap(6.0).child(keeps).child(paths);
            let show = ui.button("clash-show-in-list", display::clash_show_in_list()).on_press_tx(show_in_list).build();
            let foot = foot(vec![presses(show, show_in_list), cancel(), rename_button(true)]);
            let on_key = |s: &mut Phantom, ev: &KeyEvent, _: &mut Tx| return_does(ev, || rename(s));
            ui.dialog("clash", &title, body, foot, close, on_key, layer);
        }
        None => {
            let title = display::clash_title(&display::clash_question(&clash.name, &clash.files, &clash.to), clash.at, clash.of);
            let replace = ui.button("clash-replace", replace).on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::Replace))).build();
            let replace = presses(replace, |s, _| choose(s, Some(proto::Clash::Replace)));
            let replace = explained("clash-replace", replace, replace_help);
            let use_existing = ui
                .button("clash-use-existing", use_existing)
                .primary()
                .on_press(|s: &mut Phantom| choose(s, Some(proto::Clash::UseExisting)));
            let use_existing = presses(use_existing.build(), |s, _| choose(s, Some(proto::Clash::UseExisting)));
            let use_existing = explained("clash-use-existing", use_existing, use_help);
            let body = col().key("clash-body").w_full().child(paths);
            let foot = foot(vec![cancel(), rename_button(false), replace, use_existing]);
            let on_key = |s: &mut Phantom, ev: &KeyEvent, _: &mut Tx| return_does(ev, || choose(s, Some(proto::Clash::UseExisting)));
            ui.dialog("clash", &title, body, foot, close, on_key, layer);
        }
    }
    // Over the dialog, under the choice the pointer rests on.
    let help = [("clash-use-existing", use_help), ("clash-replace", replace_help), ("clash-rename", rename_help)];
    if let Some((key, message)) = help.into_iter().find(|(key, _)| clash.explained == Some(*key)) {
        ui.tooltip_below("clash-tip", key, message, layer);
    }
}

/// Return, with no button of the dialog focused, is its first choice: `act`.
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
        if i == 0 {
            let button = ui.button("clash-reveal", show).small().on_press(reveal).build();
            // With the focus, Return presses it, as on any button of the dialog.
            let button = button.on_key(|s: &mut Phantom, ev: &KeyEvent, _: &mut Tx| return_does(ev, || reveal(s)));
            line.child(el().grow(1.0)).child(button)
        } else {
            line
        }
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
