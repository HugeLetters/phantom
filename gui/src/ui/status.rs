//! Small status pieces: a coloured dot and secondary text.

use rune::{Color, El, el, text};

use super::Ui;

/// What a piece of text or a status dot means, for the look to colour: screens pick the tone,
/// only the building blocks turn it into a colour, so a theme can restyle each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    /// Secondary: a status, a placeholder, a file set Off.
    Muted,
    Good,
    /// Something changed that needs a look.
    Warning,
    /// Something's wrong and needs the user: an error, missing files.
    Error,
}

impl Ui {
    /// The colour of text in `tone`.
    pub(crate) fn tone_color(&self, tone: Tone) -> Color {
        match tone {
            Tone::Normal => self.k.c.text_base,
            Tone::Muted => self.k.c.text_muted,
            Tone::Good => self.k.c.success.icon,
            Tone::Warning => self.k.c.warning.icon,
            Tone::Error => self.k.c.danger.icon,
        }
    }

    /// A dot in the colour of `tone`, as the look draws it.
    pub fn status_dot(&self, tone: Tone) -> El {
        (self.blocks.status_dot)(self, tone)
    }

    /// Small secondary text, e.g. a status line.
    pub fn caption(&self, s: impl Into<String>) -> El {
        text(s.into()).font(self.k.ty.text_small).color(self.k.c.text_muted)
    }

    /// [`caption`](Self::caption) in the error colour: something the user must see to.
    pub fn error_caption(&self, s: impl Into<String>) -> El {
        text(s.into()).font(self.k.ty.text_small).color(self.tone_color(Tone::Error))
    }

    /// [`caption`](Self::caption) with figures of equal width, so a number that changes (a speed)
    /// doesn't make its text grow and shrink.
    pub fn figures(&self, s: impl Into<String>) -> El {
        text(s.into()).font(self.k.ty.text_small.features("'tnum' 1")).color(self.k.c.text_muted)
    }
}

/// Phantom's status dot: a small disc.
pub(super) fn phantom_dot(ui: &Ui, tone: Tone) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(ui.tone_color(tone))
}
