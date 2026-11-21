//! Button: `ui.button("add", "Add").primary().on_press(…).build()`. The key names it for tests,
//! scripts and anchoring menus.

use rune::{El, Tx, text};
use rune_kit::ButtonVariant;

use super::{Tone, Ui, keyed};

pub struct Button<'a> {
    ui: &'a Ui,
    key: String,
    label: String,
    inner: rune_kit::Button,
    alert: bool,
    /// What a press on the button does while it's disabled.
    when_disabled: Option<Box<dyn FnOnce(El) -> El>>,
    first_focus: bool,
    /// What VoiceOver reads, when the label it shows doesn't say it (a glyph).
    accessible: Option<String>,
    fill: bool,
    /// A shortcut shown inside it after the label, muted.
    hint: Option<String>,
}

impl Ui {
    pub fn button(&self, key: impl Into<String>, label: &str) -> Button<'_> {
        Button {
            ui: self,
            key: key.into(),
            label: label.to_owned(),
            inner: rune_kit::Button::new(label),
            alert: false,
            when_disabled: None,
            first_focus: false,
            accessible: None,
            fill: false,
            hint: None,
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

    /// As wide as the space it's given, the label centred.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Small, as AppKit's small controls: for buttons inside a notice, a popover or the status bar.
    pub fn small(mut self) -> Self {
        self.inner = self.inner.control_size(rune_kit::ControlSize::Small);
        self
    }

    /// No fill or border until it's hovered: a text button.
    pub fn borderless(mut self) -> Self {
        self.inner = self.inner.variant(ButtonVariant::Ghost);
        self
    }

    /// Its shortcut ("⌘Z") inside it after the label, muted; nothing for `None`.
    pub fn hint(mut self, hint: Option<String>) -> Self {
        self.hint = hint;
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
        if let Some(hint) = self.hint {
            let label = self.label.clone();
            let muted = k.ty.text_xsmall;
            inner = inner.label_with(move |font, ink| {
                rune::row()
                    .gap(6.0)
                    .center_y()
                    .child(text(label).font(font).color(ink))
                    .child(text(hint).key("notice-action-hint").font(muted).color(ink.alpha(0.6)))
            });
        }
        if self.alert {
            let color = self.ui.tone_color(Tone::Error);
            let label = self.label;
            inner = inner.variant(ButtonVariant::Ghost).label_with(move |font, _| text(label).font(font).color(color));
            wash = Some(k.alpha(color, 0.2));
        }
        let mut built = inner.build(k);
        if self.fill {
            built = built.w_full();
        }
        if let Some(label) = self.accessible {
            built = built.label(label);
        }
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
