//! Button: `ui.button("add", "Add").primary().on_press(…).build()`. The key names it for tests,
//! scripts and anchoring menus.

use rune::{El, Tx, text};
use rune_kit::ButtonVariant;

use super::{Tone, Ui, keyed};

pub struct Button<'a> {
    ui: &'a Ui,
    key: &'static str,
    label: String,
    inner: rune_kit::Button,
    alert: bool,
    /// What a press on the button does while it's disabled.
    when_disabled: Option<Box<dyn FnOnce(El) -> El>>,
    first_focus: bool,
}

impl Ui {
    pub fn button(&self, key: &'static str, label: &str) -> Button<'_> {
        Button {
            ui: self,
            key,
            label: label.to_owned(),
            inner: rune_kit::Button::new(label),
            alert: false,
            when_disabled: None,
            first_focus: false,
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

    /// First in the Tab order, so a sheet that opens focuses it.
    pub fn first_focus(mut self, first: bool) -> Self {
        self.first_focus = first;
        self
    }

    /// Draws the label in the error colour on a wash of it, to draw the eye for a moment.
    pub fn alert(mut self, alert: bool) -> Self {
        self.alert = alert;
        self
    }

    pub fn build(self) -> El {
        let k = &self.ui.k;
        let mut inner = self.inner;
        let mut wash = None;
        if self.alert {
            let color = self.ui.tone_color(Tone::Error);
            let label = self.label;
            inner = inner.variant(ButtonVariant::Ghost).label_with(move |font, _| text(label).font(font).color(color));
            wash = Some(k.alpha(color, 0.2));
        }
        let built = inner.build(k);
        let mut button = keyed(self.key, if self.first_focus { built.tab_index(1) } else { built });
        if let Some(wash) = wash {
            button = button.radius(k.radius.md).fill(wash);
        }
        match self.when_disabled {
            Some(attach) => attach(button),
            None => button,
        }
    }
}
