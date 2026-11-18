//! Progress bar with its percentage beside it.

use rune::{Align, El, el, row, text};
use rune_kit::Progress;

use super::Ui;

impl Ui {
    /// `fraction` from 0.0 to 1.0, as the look draws it.
    pub fn progress_bar(&self, fraction: f32) -> El {
        (self.blocks.progress_bar)(self, fraction)
    }
}

/// Phantom's: the kit's bar with the percentage beside it, rounded down, so 100% means done.
pub(super) fn phantom(ui: &Ui, fraction: f32) -> El {
    let k = &ui.k;
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(el().grow(1.0).child(Progress::new(Some(fraction)).build(k)))
        .child(text(format!("{:.0}%", (fraction * 100.0).floor())).font(k.ty.text_small).color(k.c.text_muted).w(34.0).align(Align::End))
}
