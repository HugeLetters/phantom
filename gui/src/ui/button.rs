//! Button: `ui.button("add", "Add").primary().on_press(…).build()`. The key names it for tests,
//! scripts and anchoring menus.

use rune::{El, Tx};
use rune_kit::ButtonVariant;

use super::{Ui, keyed};

pub struct Button<'a> {
    ui: &'a Ui,
    key: String,
    inner: rune_kit::Button,
    /// What a press on the button does while it's disabled.
    when_disabled: Option<Box<dyn FnOnce(El) -> El>>,
    first_focus: bool,
    /// What VoiceOver reads, when the label it shows doesn't say it (a glyph).
    accessible: Option<String>,
    fill: bool,
}

impl Ui {
    pub fn button(&self, key: impl Into<String>, label: &str) -> Button<'_> {
        Button {
            ui: self,
            key: key.into(),
            inner: rune_kit::Button::new(label),
            when_disabled: None,
            first_focus: false,
            accessible: None,
            fill: false,
        }
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

    /// A disabled button takes no presses itself; this one catches them on its key, to say why.
    pub fn on_press_disabled<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.when_disabled = Some(Box::new(move |e| e.on_press(f)));
        self
    }

    /// What VoiceOver reads for it, when what it shows is a glyph: "Dismiss" for ×.
    pub fn accessible(mut self, label: &str) -> Self {
        self.accessible = Some(label.to_owned());
        self
    }

    /// First in the Tab order, so a page of settings that opens focuses it.
    pub fn first_focus(mut self, first: bool) -> Self {
        self.first_focus = first;
        self
    }

    /// Smaller than a dialog's choices: for one that sits in a line of text (Reveal in Finder by a path).
    pub fn small(mut self) -> Self {
        self.inner = self.inner.control_size(rune_kit::ControlSize::Small);
        self
    }

    /// As wide as the space it's given, the label centred.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    pub fn build(self) -> El {
        let k = &self.ui.k;
        let mut built = self.inner.build(k);
        if self.fill {
            built = built.w_full();
        }
        if let Some(label) = self.accessible {
            built = built.label(label);
        }
        let button = keyed(self.key, if self.first_focus { built.tab_index(1) } else { built });
        match self.when_disabled {
            Some(attach) => attach(button),
            None => button,
        }
    }
}
