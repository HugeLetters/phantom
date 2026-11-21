//! What the list shows with no torrents: how to add one, naming Add file… and its key as the
//! command list has them.

use rune::El;

use crate::commands::InApp;
use crate::ui::Ui;

pub fn empty(ui: &Ui) -> El {
    let add_file = InApp::AddFile.spec();
    let hint = add_file.hint().map_or_else(String::new, |k| format!(" ({k})"));
    ui.empty_state("No torrents", &format!("Paste a magnet link, drop a .torrent file here, or click {}{hint}", add_file.name)).key("empty")
}
