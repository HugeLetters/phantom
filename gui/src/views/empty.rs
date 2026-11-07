//! What the list shows with no torrents: how to add one.

use rune::El;

use crate::ui::Ui;

pub fn empty(ui: &Ui) -> El {
    ui.empty_state("No torrents", "Paste a magnet link, drop a .torrent file here, or click Open…").key("empty")
}
