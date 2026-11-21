//! Rename… (#331): the row menu's item turns the torrent's Name cell into a text field holding its
//! name, the part before the extension selected as Finder does it (of a torrent of one file; a
//! folder's whole name), and the field has the focus. Return renames it, if the name is one a file
//! can have, and so does a click away (the field losing the focus), as in Finder; a name a file
//! can't have keeps the field open, and a tooltip under it says why until the next keystroke. Only
//! Escape cancels, or the torrent leaving the list. The list scrolls the row into view first. What
//! the service refuses (something at that name) is told in the status bar, as any command's
//! failure.

use proto::{FileName, NameProblem, TorrentId};
use rune::TextEdit;

use super::Phantom;

/// The field, while a torrent's name is being edited in its row.
#[derive(Default)]
pub struct Rename {
    /// The torrent whose Name cell is the field; none while no name is edited.
    pub id: Option<TorrentId>,
    pub field: TextEdit,
    /// Why the name sent with Return isn't one, with that text: shown until the text changes.
    pub refused: Option<(String, NameProblem)>,
    /// The field had the focus at the last frame: losing it renames.
    had_focus: bool,
}

impl Rename {
    /// Why the name in the field isn't one, while it's the text Return was refused for.
    pub fn note(&self) -> Option<NameProblem> {
        self.refused.as_ref().filter(|(text, _)| *text == self.field.text).map(|(_, p)| *p)
    }

    /// Whether torrent `id`'s name is the one being edited.
    pub fn editing(&self, id: TorrentId) -> bool {
        self.id == Some(id)
    }
}

/// Rename… on torrent `id`: its row scrolled into view, then [`open`].
pub fn start(s: &mut Phantom, id: TorrentId, tx: &mut rune::Tx) {
    s.table.reveal(id.to_string(), tx);
    open(s, id);
}

/// Opens the field in torrent `id`'s Name cell, focused, with its name and the part of it before
/// the extension selected: all of it for a torrent of several files, whose name is a folder's.
pub fn open(s: &mut Phantom, id: TorrentId) {
    let Some(t) = s.torrents.iter().find(|t| t.id == id) else { return };
    let name = t.display_name().to_owned();
    let one_file = s.pane.details.as_ref().filter(|d| d.id == id).is_some_and(|d| d.files.len() == 1);
    let mut field = TextEdit::new(name.clone());
    field.anchor = 0;
    field.focus = selected_end(&name, one_file);
    field.focused = true;
    s.rename = Rename { id: Some(id), field, refused: None, had_focus: false };
}

/// Where the selection of a name just opened for editing ends (it starts at 0), for the inline
/// field and the clash dialog's alike: the stem of a torrent of one file, as Finder selects a
/// file's name; all of a folder's.
pub fn selected_end(name: &str, one_file: bool) -> usize {
    if one_file { stem(name) } else { name.len() }
}

/// Where a file's name ends before its extension, as Finder selects it: before the last `.`, unless
/// that's its first character (a hidden file's) or its last; else the whole name.
pub fn stem(name: &str) -> usize {
    match name.rfind('.') {
        Some(0) | None => name.len(),
        Some(dot) if dot + 1 == name.len() => name.len(),
        Some(dot) => dot,
    }
}

/// Return in the field, or a click away: renames the torrent to the name typed and closes the
/// field; the same name closes it; a name a file can't have keeps it open, focused, saying why.
pub fn submit(s: &mut Phantom) {
    let Some(id) = s.rename.id else { return };
    let text = s.rename.field.text.clone();
    let name = match FileName::new(text.clone()) {
        Ok(name) => name,
        Err(problem) => {
            s.rename.refused = Some((text, problem));
            s.rename.field.focused = true;
            return;
        }
    };
    let same = s.torrents.iter().find(|t| t.id == id).is_some_and(|t| t.display_name() == name.as_str());
    close(s);
    if !same {
        super::rename(s, id, name);
    }
}

/// Escape in the field: nothing renames, the name shows again.
pub fn close(s: &mut Phantom) {
    s.rename = Rename::default();
}

/// Called before every frame: the field that had the focus and hasn't now (a click elsewhere, Tab)
/// renames, as Return does; its torrent leaving the list closes it.
pub fn follow_focus(s: &mut Phantom) {
    let Some(id) = s.rename.id else { return };
    if !s.torrents.iter().any(|t| t.id == id) {
        return close(s);
    }
    if s.rename.had_focus && !s.rename.field.focused {
        submit(s);
    }
    s.rename.had_focus = s.rename.field.focused;
}

#[cfg(test)]
mod tests {
    use super::stem;

    /// As Finder selects a file's name to rename it: what's before its extension; a hidden file's
    /// whole name, and one ending in a dot.
    #[test]
    fn the_stem_is_the_name_before_its_extension() {
        for (name, want) in
            [("Movie.mkv", "Movie"), ("Album.2024.flac", "Album.2024"), (".hidden", ".hidden"), ("plain", "plain"), ("dot.", "dot.")]
        {
            assert_eq!(&name[..stem(name)], want, "{name}");
        }
    }
}
