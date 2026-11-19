//! A modal dialog over the window: `ui.dialog("clash", title, message, body, buttons, close, &mut layer)`.

use rune::{El, col, row, text};
use rune_kit::Dialog;

use super::Ui;

impl Ui {
    /// A dialog keyed `<key>-dialog`, in `layer` (which the page hands to [`Ui::root`]): a short
    /// `title`, `message` under it (wrapped), then `body`, and `buttons` along its foot, from the
    /// left. It's modal:
    /// the window under it takes nothing, and Escape or its × calls `on_close`.
    #[allow(clippy::too_many_arguments)]
    pub fn dialog<S: 'static>(
        &self,
        key: &str,
        title: &str,
        message: &str,
        body: Option<El>,
        buttons: Vec<El>,
        on_close: impl Fn(&mut S) + Clone + 'static,
        layer: &mut Vec<El>,
    ) {
        let k = &self.k;
        let message = text(message.to_owned()).key(format!("{key}-message")).font(k.ty.ui).color(k.c.text_base).wrap();
        let mut content = col().w_full().px(16.0).gap(8.0).child(message);
        if let Some(body) = body {
            content = content.child(body);
        }
        let foot = row().key("buttons").w_full().padding(16.0).gap(8.0).children(buttons);
        Dialog::new(key, title).fit().open(true).click_outside(false).child(content).child(foot).on_close(on_close).build(k, layer);
    }

    /// A line of text in a dialog's body, muted, one path or name; cut short at the start, where a
    /// path is least telling.
    pub fn dialog_line(&self, line: &str) -> El {
        let k = &self.k;
        text(line.to_owned()).font(k.ty.text_small).color(k.c.text_muted).max_lines(1)
    }

    /// A note in a dialog's body, muted, wrapped.
    pub fn dialog_note(&self, note: &str) -> El {
        let k = &self.k;
        text(note.to_owned()).font(k.ty.text_small).color(k.c.text_muted).wrap()
    }
}
