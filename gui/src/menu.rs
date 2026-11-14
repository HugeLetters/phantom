//! Which menu is open: a machine, since what a press or a pick means depends on it. At most one
//! menu is open; opening another replaces it, and a pick or a dismissal (Escape, a press outside)
//! closes it. What a menu offers isn't kept here: it's read from the selection and the service's
//! `actions` every frame it's drawn, so it can't go stale. Rune does the keyboard focus (the open
//! menu holds it and gives it back on closing).
//!
//! | state ↓ / event → | `Open(menu)` | `Close`  |
//! |-------------------|--------------|----------|
//! | `Closed`          | `Open(menu)` | `Closed` |
//! | `Open(_)`         | `Open(menu)` | `Closed` |

use proto::TorrentId;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Menu {
    #[default]
    Closed,
    Open(Open),
}

/// An open menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Open {
    /// The context menu of the selection, opened by a right-click on torrent `id`'s row at the
    /// point `at`.
    Row { id: TorrentId, at: (f32, f32) },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Open(Open),
    /// A pick or a dismissal.
    Close,
}

pub fn decide(_: Menu, event: Event) -> Menu {
    match event {
        Event::Open(menu) => Menu::Open(menu),
        Event::Close => Menu::Closed,
    }
}

impl Menu {
    pub fn then(self, event: Event) -> Menu {
        decide(self, event)
    }

    pub fn is_open(self) -> bool {
        self != Menu::Closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every state × event pair: the event's own result, whatever was open.
    #[test]
    fn every_pair_is_decided() {
        let menus = [Open::Row { id: 1, at: (3.0, 4.0) }, Open::Row { id: 2, at: (5.0, 6.0) }];
        let states: Vec<Menu> = [Menu::Closed].into_iter().chain(menus.map(Menu::Open)).collect();
        for state in states {
            for open in menus {
                assert_eq!(decide(state, Event::Open(open)), Menu::Open(open), "{state:?} opening {open:?}");
            }
            assert_eq!(decide(state, Event::Close), Menu::Closed, "{state:?} closing");
        }
    }
}
