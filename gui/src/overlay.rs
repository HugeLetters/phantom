//! The overlays that open over the page: the status bar's notices popover, the menus (the row
//! menu, Remove…) and the clash dialog of a Move (#24). One at a time, by one rule: opening one
//! closes the others ([`opening`]). Each keeps its own machine; this only says which closes which.
//!
//! | opening ↓ / closes → | popover | menu | clash dialog |
//! |---|---|---|---|
//! | popover (a click) | | ✓ | ✓ |
//! | menu | ✓ | | ✓ |
//! | clash dialog (a Move's answer) | ✓ | ✓ | |

use crate::menu;
use crate::model::Phantom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Notices,
    Menu,
    Clash,
}

impl Overlay {
    pub const ALL: [Overlay; 3] = [Overlay::Notices, Overlay::Menu, Overlay::Clash];
}

/// What opening `which` closes: every other overlay.
pub fn closes(which: Overlay) -> impl Iterator<Item = Overlay> {
    Overlay::ALL.into_iter().filter(move |o| *o != which)
}

/// `which` is opening: closes every other one. Call it as it opens.
pub fn opening(s: &mut Phantom, which: Overlay) {
    for other in closes(which) {
        match other {
            Overlay::Notices => {
                s.bar.close();
            }
            Overlay::Menu => s.menu = s.menu.then(menu::Event::Close),
            // Its Move dropped, nothing sent.
            Overlay::Clash => crate::model::moving::abandon(s),
        }
    }
}

/// The overlay drawn now, if any: the menu, else the popover. At most one, whatever the machines
/// say.
pub fn up(s: &Phantom) -> Option<Overlay> {
    if s.clash.is_some() {
        Some(Overlay::Clash)
    } else if s.menu.is_open() {
        Some(Overlay::Menu)
    } else if s.bar.is_open() {
        Some(Overlay::Notices)
    } else {
        None
    }
}

/// A click on the status bar's notice: the popover opens over whatever else was open.
pub fn press_notices(s: &mut Phantom) {
    s.bar.press();
    if s.bar.is_open() {
        opening(s, Overlay::Notices);
    }
}

/// A menu opens, over whatever else was open.
pub fn open_menu(s: &mut Phantom, open: menu::Open) {
    opening(s, Overlay::Menu);
    s.menu = s.menu.then(menu::Event::Open(open));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notice::Notice;
    use crate::service::MemoryService;

    /// Every overlay opening, over every set of the other open: it's the one drawn after, and the
    /// other is closed.
    #[test]
    fn opening_one_closes_the_others() {
        let clash = || crate::model::moving::Clash {
            id: 1,
            name: "Album".into(),
            to: proto::AbsolutePath::new("/Films").unwrap(),
            files: vec![proto::AbsolutePath::new("/Films/Album/a").unwrap()],
            remaining: 0,
            apply_to_rest: false,
            other: None,
            renaming: None,
        };
        for which in Overlay::ALL {
            for open in 0..8u8 {
                let mut rt = crate::tests::app(MemoryService::new());
                rt.update(move |s, _| {
                    if open & 1 != 0 {
                        s.bar.tell(Notice::warning("a"));
                        s.bar.tell(Notice::warning("b"));
                        s.bar.press();
                    }
                    if open & 2 != 0 {
                        s.menu = s.menu.then(menu::Event::Open(menu::Open::Row { id: 1, at: (10.0, 10.0) }));
                    }
                    if open & 4 != 0 {
                        s.clash = Some(clash());
                    }
                    match which {
                        Overlay::Notices => {
                            s.bar.tell(Notice::warning("c"));
                            s.bar.tell(Notice::warning("d"));
                            press_notices(s);
                        }
                        Overlay::Menu => open_menu(s, menu::Open::Row { id: 1, at: (10.0, 10.0) }),
                        Overlay::Clash => {
                            opening(s, Overlay::Clash);
                            s.clash = Some(clash());
                        }
                    }
                });
                let s = &rt.state;
                assert_eq!(up(s), Some(which), "{which:?} over {open:03b}");
                assert!(!s.bar.is_open() || which == Overlay::Notices, "the popover still open after {which:?} over {open:03b}");
                assert!(!s.menu.is_open() || which == Overlay::Menu, "a menu still open after {which:?} over {open:03b}");
                assert!(s.clash.is_none() || which == Overlay::Clash, "the clash dialog still open after {which:?} over {open:03b}");
            }
        }
    }
}
