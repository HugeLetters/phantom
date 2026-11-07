//! Empty state: what an area says when it has nothing to show, and how to fill it.

use rune::{El, col, text};

use super::Ui;

impl Ui {
    /// `title` centred in the area, with `hint` below it.
    pub fn empty_state(&self, title: &str, hint: &str) -> El {
        col()
            .w_full()
            .h_full()
            .center()
            .gap(6.0)
            .child(text(title.to_owned()).font(self.k.ty.body_medium).color(self.k.c.text_base))
            .child(text(hint.to_owned()).font(self.k.ty.text_small).color(self.k.c.text_muted))
    }
}
