//! The undo stack's machine is the shared one ([`lifecycle::undo`], #150), which the GUI's fake
//! service runs too, over the engine's own [`Entry`].

pub use lifecycle::undo::{Command, Do, Refusal, World, decide, next};

use super::Entry;

pub type State = lifecycle::undo::State<Entry>;
pub type Event = lifecycle::undo::Event<Entry>;
pub type Observed = lifecycle::undo::Observed<Entry>;

impl lifecycle::undo::Undoes for Entry {
    fn absorbs(&self, older: &Entry) -> bool {
        Entry::absorbs(self, older)
    }
}

/// `n` entries, each of its own torrent, Removes and Deletes alternating, a partial Delete among
/// them.
#[cfg(test)]
pub fn samples(n: usize) -> Vec<Entry> {
    use super::{Back, Source, Stash};
    (0..n)
        .map(|i| {
            let stash = Stash { dir: format!("/stash/{i}").into(), output_folder: "/d".into(), only_files: None, librqbit_id: i };
            let back = match i % 3 {
                2 => Back::InList { stash },
                _ => Back::Gone { saved: Default::default(), source: Source::Stash(stash) },
            };
            let action = if i % 3 == 0 { proto::UndoAction::Remove } else { proto::UndoAction::Delete };
            Entry { action, name: format!("t{i}"), info_hash: format!("{i:040x}"), id: i as u64, back, put_back: Vec::new() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Delete takes the place of its torrent's partial Delete before it, and of nothing else.
    #[test]
    fn a_delete_absorbs_only_its_own_torrents_partial_delete() {
        let entries = samples(6);
        let partial = &entries[2];
        let mut whole = partial.clone();
        whole.back = super::super::Back::Gone { saved: Default::default(), source: super::super::Source::Magnet(String::new()) };
        assert!(whole.absorbs(partial));
        assert!(entries.iter().filter(|e| *e != partial).all(|e| !whole.absorbs(e)));
        let mut again = whole.clone();
        again.id += 100;
        assert!(!again.absorbs(partial), "the same info hash, added again since: another torrent");
    }
}
