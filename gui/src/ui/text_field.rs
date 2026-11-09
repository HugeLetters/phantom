//! Single-line text field, as wide as its parent lets it be:
//! `ui.text_field("magnet-field", &s.magnet, "Magnet link").on_edit(…).on_submit(…).build()`.

use rune::{El, TextEdit, Tx, el, row, text};
use rune_kit::TextInput;

use super::Ui;

pub struct TextField<'a> {
    ui: &'a Ui,
    key: &'static str,
    edit: TextEdit,
    placeholder: String,
    inner: TextInput,
}

impl Ui {
    /// `label` names the field for accessibility; `edit` is its text in the app's state.
    pub fn text_field(&self, key: &'static str, edit: &TextEdit, label: &str) -> TextField<'_> {
        TextField { ui: self, key, edit: edit.clone(), placeholder: String::new(), inner: TextInput::new(edit).label(label).width(None) }
    }
}

impl TextField<'_> {
    /// Shown while the field is empty.
    pub fn placeholder(mut self, text: &str) -> Self {
        self.placeholder = text.to_owned();
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
        let field = el().key(self.key).child(self.inner.build(&self.ui.k));
        if !self.edit.text.is_empty() || self.placeholder.is_empty() {
            return field;
        }
        // Drawn here rather than by the kit's field, whose placeholder wraps onto a second line
        // and spills out of the field when it's narrower than the text. Lines up with the text
        // the field would draw (its 2-point spacer and 6-point gap), and lets presses through.
        let hint = text(self.placeholder).font(self.ui.k.ty.ui_compact).color(self.ui.k.c.text_faint).truncate();
        field.child(row().pin(Some(8.0), Some(0.0), Some(8.0), Some(0.0)).center_y().inert().child(hint))
    }
}
