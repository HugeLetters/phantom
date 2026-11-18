//! Small status pieces: a coloured dot, secondary text, and a one-line notice.

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

    /// A notice's short line, cut to one line, in the colour of its `tone`.
    pub fn notice(&self, s: impl Into<String>, tone: Tone) -> El {
        text(s.into()).font(self.k.ty.text_small).color(self.tone_color(tone)).truncate()
    }

    /// The status bar's notice, keyed `notice`: its short line (`notice-text`), which truncates,
    /// and what follows it (`notice-count`), which never does, both in `tone`. A control: Tab
    /// reaches it, with the look's focus ring, and the popover gives focus back to it.
    pub fn notice_line(&self, short: impl Into<String>, tail: Option<String>, tone: Tone) -> El {
        rune::row()
            .key("notice")
            .gap(0.0)
            .center_y()
            .role("button")
            .focusable(true)
            .focus_ring(2.0, 2.0, self.k.c.border_focus)
            .min_w(0.0)
            .shrink()
            .child(self.notice(short, tone).key("notice-text").min_w(0.0).shrink())
            .children(tail.map(|tail| self.notice_count(tail, tone).key("notice-count")))
    }

    /// What follows a notice's short line in the bar ("· 1/3 ›"): in its colour, never cut or
    /// shrunk, so the line truncates before it.
    pub fn notice_count(&self, s: impl Into<String>, tone: Tone) -> El {
        text(s.into()).font(self.k.ty.text_small).color(self.tone_color(tone))
    }
}

/// Phantom's status dot: a small disc.
pub(super) fn phantom_dot(ui: &Ui, tone: Tone) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(ui.tone_color(tone))
}
