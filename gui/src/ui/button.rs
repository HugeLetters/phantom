//! Button: `ui.button("add", "Add").primary().on_press(…).build()`. The key names it for tests,
//! scripts and anchoring menus.

use rune::{El, Tx, icon, row, text};
use rune_kit::ButtonVariant;

use super::{Icon, Tone, Ui, keyed};

pub struct Button<'a> {
    ui: &'a Ui,
    key: &'static str,
    label: String,
    inner: rune_kit::Button,
    /// What a press on the button does while it's disabled.
    when_disabled: Option<Box<dyn FnOnce(El) -> El>>,
    icon: Option<Icon>,
    done: bool,
}

impl Ui {
    pub fn button(&self, key: &'static str, label: &str) -> Button<'_> {
        Button { ui: self, key, label: label.to_owned(), inner: rune_kit::Button::new(label), when_disabled: None, icon: None, done: false }
    }
}

impl Button<'_> {
    /// The action a group of buttons leads with.
    pub fn primary(mut self) -> Self {
        self.inner = self.inner.variant(ButtonVariant::Contrast);
        self
    }

    /// An icon to the left of the label, drawn as the theme draws it.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// While `done`, the icon is a check and the whole button in the success colour: the action
    /// just happened. The button keeps its size.
    pub fn done(mut self, done: bool) -> Self {
        self.done = done;
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

    pub fn build(self) -> El {
        let mut inner = self.inner;
        if self.done {
            let color = self.ui.tone_color(Tone::Good);
            let (check, label) = (self.ui.icon(Icon::Check), self.label);
            // As the kit lays out an icon and a label: 16 points, 6 apart.
            inner = inner.label_with(move |font, _| {
                row().gap(6.0).center_y().child(icon(check, 16.0).color(color)).child(text(label).font(font).color(color))
            });
        } else if let Some(icon) = self.icon {
            inner = inner.icon(self.ui.icon(icon));
        }
        let button = keyed(self.key, inner.build(&self.ui.k));
        match self.when_disabled {
            Some(attach) => attach(button),
            None => button,
        }
    }
}
