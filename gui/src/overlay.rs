//! The overlays that open over the page: the error list, the status bar's notices popover, and the
//! menus (the row menu, Remove…). One at a time, by one rule: opening one closes the others
//! ([`opening`]). Each keeps its own machine; this only says which closes which.
//!
//! The error list can't be closed without losing its entries: it's there while it has some. So
//! the popover or a menu opened over it hides it until they close ([`up`]), and an error reported
//! closes them, the list showing at once.
//!
//! | opening ↓ / closes → | error list | popover | menus |
//! |---|---|---|---|
//! | error list (an error reported) | | ✓ | ✓ |
//! | popover | hidden while it's open | | ✓ |
//! | menu | hidden while it's open | ✓ | |

use crate::menu;
use crate::model::Phantom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Errors,
    Notices,
    Menu,
}

impl Overlay {
    pub const ALL: [Overlay; 3] = [Overlay::Errors, Overlay::Notices, Overlay::Menu];
}

/// What opening `which` closes: every other overlay.
pub fn closes(which: Overlay) -> impl Iterator<Item = Overlay> {
    Overlay::ALL.into_iter().filter(move |o| *o != which)
}

/// `which` is opening: closes every other one. Call it as it opens.
pub fn opening(s: &mut Phantom, which: Overlay) {
    for other in closes(which) {
        match other {
            // Hidden while another is up, as `up` says: its entries stay.
            Overlay::Errors => {}
            Overlay::Notices => {
                s.bar.close();
            }
            Overlay::Menu => s.menu = s.menu.then(menu::Event::Close),
        }
    }
}

/// The overlay drawn now, if any: the menu or the popover, else the error list while it has
/// entries. At most one, whatever the machines say.
pub fn up(s: &Phantom) -> Option<Overlay> {
    if s.menu.is_open() {
        Some(Overlay::Menu)
    } else if s.bar.is_open() {
        Some(Overlay::Notices)
    } else if s.errors.is_open() {
        Some(Overlay::Errors)
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

    /// Every overlay opening, over every set of the others open: it's the one drawn after, and
    /// each other is closed (the error list hidden, its entries kept) unless it's the error list
    /// opening, which closes the rest.
    #[test]
    fn opening_one_closes_the_others() {
        for which in Overlay::ALL {
            for open in 0..8u8 {
                let mut rt = crate::tests::app(MemoryService::new());
                rt.update(move |s, tx| {
                    if open & 1 != 0 {
                        s.errors.report(Notice::error("failed"));
                        s.errors.pump(0.0, tx);
                    }
                    if open & 2 != 0 {
                        s.bar.tell(Notice::warning("a"));
                        s.bar.tell(Notice::warning("b"));
                        s.bar.press();
                    }
                    if open & 4 != 0 {
                        s.menu = s.menu.then(menu::Event::Open(menu::Open::Remove));
                    }
                    match which {
                        Overlay::Errors => {
                            s.errors.report(Notice::error("failed again"));
                            s.errors.pump(0.0, tx);
                            opening(s, Overlay::Errors);
                        }
                        Overlay::Notices => {
                            s.bar.tell(Notice::warning("c"));
                            s.bar.tell(Notice::warning("d"));
                            press_notices(s);
                        }
                        Overlay::Menu => open_menu(s, menu::Open::Remove),
                    }
                });
                let s = &rt.state;
                assert_eq!(up(s), Some(which), "{which:?} over {open:03b}");
                assert!(!s.bar.is_open() || which == Overlay::Notices, "the popover still open after {which:?} over {open:03b}");
                assert!(!s.menu.is_open() || which == Overlay::Menu, "a menu still open after {which:?} over {open:03b}");
                if open & 1 != 0 || which == Overlay::Errors {
                    assert!(s.errors.is_open(), "the error list keeps its entries under {which:?}");
                }
            }
        }
    }
}
