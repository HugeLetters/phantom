//! A small note anchored under an element: `ui.note("add-note", trigger, "Why", open, close, &mut layer)`.

use rune::{El, text};
use rune_kit::Popover;

use super::Ui;

impl Ui {
    /// `trigger`, with a popover of `message` under it while `open`. The popover goes into
    /// `layer`, which the page hands to [`Ui::root`]; pressing outside it calls `on_close`.
    pub fn note<S: 'static>(
        &self,
        key: &'static str,
        trigger: El,
        message: &str,
        open: bool,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) -> El {
        let message = text(message.to_owned()).font(self.k.ty.text_small).color(self.k.c.text_base);
        Popover::new(key).trigger(trigger).padding(8.0).open(open).on_close(on_close).child(message).build(&self.k, layer)
    }
}
