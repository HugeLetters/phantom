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
    /// The context menu of the selected files of the torrent the details pane shows (#151), opened
    /// by a right-click on a file's row at the point `at`.
    File { at: (f32, f32) },
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

    use lifecycle::every;
    use lifecycle::machine::{Row, table};

    // A new menu, state or event needs samples (`every!`) and a row before this passes.

    every!(Open {
        Open::Row { .. } => [Open::Row { id: 1, at: (3.0, 4.0) }, Open::Row { id: 2, at: (5.0, 6.0) }],
        Open::File { .. } => [Open::File { at: (7.0, 8.0) }],
    });

    every!(Menu {
        Menu::Closed => [Menu::Closed],
        Menu::Open(_) => <Open as lifecycle::machine::Every>::every().into_iter().map(Menu::Open),
    });

    every!(Event {
        Event::Open(_) => <Open as lifecycle::machine::Every>::every().into_iter().map(Event::Open),
        Event::Close => [Event::Close],
    });

    /// Every state × event pair: the event's own result, whatever was open.
    #[test]
    fn every_pair_is_decided() {
        let rows: Vec<Row<Menu, Event, Menu>> = vec![
            Row {
                says: "opening a menu opens it, replacing any open",
                when: |_, e| matches!(e, Event::Open(_)),
                then: |_, e, out| matches!(e, Event::Open(open) if *out == Menu::Open(*open)),
            },
            Row { says: "closing closes", when: |_, e| matches!(e, Event::Close), then: |_, _, out| *out == Menu::Closed },
        ];
        table(|s: &Menu, e: &Event| decide(*s, *e), &rows, &[]);
    }
}
