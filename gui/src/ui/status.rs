//! Small status pieces: a coloured dot, secondary text, and a one-line notice.

use rune::{El, el, text};

use super::Ui;

/// What a status dot says at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Good,
    Bad,
}

impl Ui {
    pub fn status_dot(&self, tone: Tone) -> El {
        let color = match tone {
            Tone::Neutral => self.k.c.text_muted,
            Tone::Good => self.k.c.success.icon,
            Tone::Bad => self.k.c.danger.icon,
        };
        el().size(7.0, 7.0).radius(3.5).fill(color)
    }

    /// Small secondary text, e.g. a status line.
    pub fn caption(&self, s: impl Into<String>) -> El {
        text(s.into()).font(self.k.ty.text_small).color(self.k.c.text_muted)
    }

    /// A message for the user, cut to one line; `error` shows it in the error colour.
    pub fn notice(&self, s: impl Into<String>, error: bool) -> El {
        text(s.into())
            .font(self.k.ty.text_small)
            .color(if error { self.k.c.danger.icon } else { self.k.c.text_base })
            .truncate()
    }
}
