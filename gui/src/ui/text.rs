//! Text that isn't part of another block: a heading over an area.

use rune::{El, text};

use super::Ui;

impl Ui {
    /// The name of what an area shows, cut to one line.
    pub fn heading(&self, s: impl Into<String>) -> El {
        text(s.into()).font(self.k.ty.ui_medium).color(self.k.c.text_base).truncate()
    }
}
