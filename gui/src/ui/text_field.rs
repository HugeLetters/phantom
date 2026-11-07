//! Single-line text field, as wide as its parent lets it be:
//! `ui.text_field("magnet-field", &s.magnet, "Magnet link").on_edit(…).on_submit(…).build()`.

use rune::{El, TextEdit, Tx, el};
use rune_kit::TextInput;

use super::Ui;

pub struct TextField<'a> {
    ui: &'a Ui,
    key: &'static str,
    inner: TextInput,
}

impl Ui {
    /// `label` names the field for accessibility; `edit` is its text in the app's state.
    pub fn text_field(&self, key: &'static str, edit: &TextEdit, label: &str) -> TextField<'_> {
        TextField { ui: self, key, inner: TextInput::new(edit).label(label).width(None) }
    }
}

impl TextField<'_> {
    /// Shown while the field is empty.
    pub fn placeholder(mut self, text: &str) -> Self {
        self.inner = self.inner.placeholder(text);
        self
    }

    /// Where the field's text lives in the app's state.
    pub fn on_edit<S: 'static>(mut self, f: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static) -> Self {
        self.inner = self.inner.on_edit(f);
        self
    }

    /// Return was pressed in the field.
    pub fn on_submit<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.inner = self.inner.on_submit(f);
        self
    }

    pub fn build(self) -> El {
        el().key(self.key).child(self.inner.build(&self.ui.k))
    }
}
