//! Progress bar with its percentage beside it.

use rune::{Align, El, el, row, text};
use rune_kit::Progress;

use super::Ui;

impl Ui {
    /// `fraction` from 0.0 to 1.0; the percentage rounds down, so 100% means done.
    pub fn progress_bar(&self, fraction: f32) -> El {
        row().w_full().gap(8.0).center_y().child(el().grow(1.0).child(Progress::new(Some(fraction)).build(&self.k))).child(
            text(format!("{:.0}%", (fraction * 100.0).floor()))
                .font(self.k.ty.text_small)
                .color(self.k.c.text_muted)
                .w(34.0)
                .align(Align::End),
        )
    }
}
