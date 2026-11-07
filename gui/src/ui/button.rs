//! Button: `ui.button("add", "Add").primary().on_press(…).build()`. The key names it for tests,
//! scripts and anchoring menus.

use rune::{El, Tx};
use rune_kit::ButtonVariant;

use super::{Ui, keyed};

pub struct Button<'a> {
    ui: &'a Ui,
    key: &'static str,
    inner: rune_kit::Button,
}

impl Ui {
    pub fn button(&self, key: &'static str, label: &str) -> Button<'_> {
        Button { ui: self, key, inner: rune_kit::Button::new(label) }
    }
}

impl Button<'_> {
    /// The action a group of buttons leads with.
    pub fn primary(mut self) -> Self {
        self.inner = self.inner.variant(ButtonVariant::Contrast);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.inner = self.inner.disabled(disabled);
        self
    }

    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.inner = self.inner.on_press(f);
        self
    }

    pub fn on_press_tx<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.inner = self.inner.on_press_tx(f);
        self
    }

    pub fn build(self) -> El {
        keyed(self.key, self.inner.build(&self.ui.k))
    }
}
