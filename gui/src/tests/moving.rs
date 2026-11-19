//! Move to… (#24) in the row menu: off, with why, while files of the torrent are in the Trash for
//! an undo, and on again once that's undone.

use proto::TorrentCommand;

use super::*;

/// The row menu's Move to… item: whether it takes presses, and what's written under it.
fn move_item(rt: &Runtime<Phantom>) -> Option<(bool, Option<String>)> {
    let elements = rt.elements();
    let item = elements.iter().find(|e| {
        !e.exiting && e.role == "menuitem" && e.path.contains("row-menu") && e.label.as_deref().is_some_and(|l| l.starts_with("Move to"))
    })?;
    let under = elements.iter().find(|e| e.path.starts_with(&format!("{}/", item.path)) && e.key.as_deref() == Some("description"));
    Some((item.focusable, under.and_then(|e| e.text.clone())))
}

#[test]
fn move_to_is_off_with_why_while_files_of_the_torrent_are_in_the_trash_for_an_undo() {
    let mut rt = app(with_an_album());
    settle(&mut rt);
    rt.state.service.command(&[1], TorrentCommand::TrashFiles { files: vec![0] }, Box::new(|_, _, _| {}));
    wait(&mut rt, 1.0);
    settle(&mut rt);
    right_click_row(&mut rt, 1);
    let why = client::display::why_off(&[&proto::Answer::Refused { why: proto::TorrentRefusal::TrashedFiles }]);
    assert_eq!(move_item(&rt), Some((false, why)), "off, why written under it");
    pick_choice(&mut rt, "row-menu", "Move to");
    assert!(picker(&rt).is_none(), "a press asks for no folder");
    rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
    settle(&mut rt);
    rt.state.service.undo(Box::new(|_, _, _| {}));
    wait(&mut rt, 1.0);
    settle(&mut rt);
    right_click_row(&mut rt, 1);
    assert_eq!(move_item(&rt), Some((true, None)), "on again once it's undone");
}
