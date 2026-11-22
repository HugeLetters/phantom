//! Move to ▸ (#87): the row menu's Move to opens a submenu of the folders a move most likely goes
//! to: the default folder, then the last [`MOST`] folders torrents were moved to, newest first,
//! then Other…, which asks with macOS's folder picker. The palette lists the same folders, each as
//! "Move to <folder>". The recent ones are kept in `gui.json` ([`SETTING`]); a headless run keeps
//! them nowhere.

use proto::AbsolutePath;

use super::Phantom;

/// The key in `gui.json`.
pub const SETTING: &str = "recent_folders";

/// The most recent folders kept, and offered.
pub const MOST: usize = 5;

/// The folders torrents were last moved to, newest first, at most [`MOST`], each once.
#[derive(Default)]
pub struct RecentFolders {
    list: Vec<AbsolutePath>,
    /// Keeps the list where it lasts, after each change.
    keep: Option<Box<dyn Fn(&[AbsolutePath])>>,
}

impl RecentFolders {
    /// `list` as kept (the first [`MOST`], each once), each change kept by `keep`.
    pub fn new(list: Vec<AbsolutePath>, keep: impl Fn(&[AbsolutePath]) + 'static) -> Self {
        let mut recent = RecentFolders { list: Vec::new(), keep: None };
        for folder in list.into_iter().rev() {
            recent.push(folder);
        }
        RecentFolders { keep: Some(Box::new(keep)), ..recent }
    }

    pub fn list(&self) -> &[AbsolutePath] {
        &self.list
    }

    /// Torrents are being moved to `folder`: it's the newest, once, and the oldest past [`MOST`] goes.
    pub fn record(&mut self, folder: AbsolutePath) {
        self.push(folder);
        if let Some(keep) = &self.keep {
            keep(&self.list);
        }
    }

    fn push(&mut self, folder: AbsolutePath) {
        self.list.retain(|f| *f != folder);
        self.list.insert(0, folder);
        self.list.truncate(MOST);
    }
}

/// A folder Move to ▸ offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Offered {
    pub folder: AbsolutePath,
    /// The default folder, which comes first.
    pub default: bool,
}

/// The folders Move to ▸ offers, in order: the default folder, if the service has one, then the
/// recent ones that aren't it.
pub fn offered(s: &Phantom) -> Vec<Offered> {
    let default = s.default_folder.iter().map(|folder| Offered { folder: folder.clone(), default: true });
    // Up to [`MOST`] besides the default folder, which isn't one of them.
    let recent = s
        .recent
        .list()
        .iter()
        .filter(|f| s.default_folder.as_ref() != Some(*f))
        .take(MOST)
        .map(|f| Offered { folder: f.clone(), default: false });
    default.chain(recent).collect()
}

/// How a menu names each of `folders`: its own name, and for a name another has too, the rest of
/// its path after a dash ("Films — Volumes/Archive"), so two Films tell apart.
pub fn names(folders: &[Offered]) -> Vec<String> {
    folders
        .iter()
        .map(|f| {
            let own = name(&f.folder);
            if folders.iter().filter(|o| name(&o.folder) == own).count() < 2 {
                return own;
            }
            let parent = f.folder.parent().map(|p| p.display().to_string()).unwrap_or_default();
            format!("{own} — {}", parent.strip_prefix('/').filter(|p| !p.is_empty()).unwrap_or(&parent))
        })
        .collect()
}

/// How a menu names a folder: its own name ("/" for the root).
pub fn name(folder: &AbsolutePath) -> String {
    folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(p: &str) -> AbsolutePath {
        AbsolutePath::new(p).expect("absolute")
    }

    /// Newest first, each once, the oldest past five gone; each change kept.
    #[test]
    fn a_folder_moved_to_goes_first_once_and_the_list_keeps_five() {
        let kept = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut recent = RecentFolders::new(Vec::new(), {
            let kept = kept.clone();
            move |list: &[AbsolutePath]| *kept.borrow_mut() = list.to_vec()
        });
        for f in ["/a", "/b", "/c", "/b", "/d", "/e", "/f"] {
            recent.record(at(f));
        }
        let want: Vec<AbsolutePath> = ["/f", "/e", "/d", "/b", "/c"].map(at).to_vec();
        assert_eq!(recent.list(), want.as_slice());
        assert_eq!(*kept.borrow(), want, "kept as it is");
    }

    /// What `gui.json` held comes back as it was, cut to five and each once.
    #[test]
    fn the_kept_list_is_read_back_in_its_order() {
        let list = ["/a", "/b", "/a", "/c", "/d", "/e", "/f"].map(at).to_vec();
        let recent = RecentFolders::new(list, |_: &[AbsolutePath]| {});
        assert_eq!(recent.list(), ["/a", "/b", "/c", "/d", "/e"].map(at).as_slice());
    }
}
