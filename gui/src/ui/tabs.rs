//! Tabs over an area (the details pane's Files · Info, #33), and what a theme puts in the titlebar
//! band's free room (a logo). Both are blocks a theme can draw ([`super::blocks`]); the screen
//! gives each tab its key and its press.

use rune::{El, el, row, text};

use super::Ui;

/// Which free room of the titlebar band, either side of the link field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandSide {
    /// Between the traffic lights and the field.
    Left,
    /// Between Add file… and Resume.
    Right,
}

/// Phantom's tab: its label, the one shown in the text colour over a line in the accent.
pub(super) fn phantom_tab(ui: &Ui, label: &str, selected: bool) -> El {
    let k = &ui.k;
    let color = if selected { k.c.text_base } else { k.c.text_muted };
    let mark = el().h(2.0).w_full().radius(1.0).fill(if selected { k.c.text_accent } else { rune::Color::TRANSPARENT });
    rune::col()
        .h_full()
        .justify_end()
        .gap(5.0)
        .cursor(rune::Cursor::Pointer)
        .role("tab")
        .label(label.to_owned())
        .checked(selected)
        .child(text(label.to_owned()).font(k.ty.text_small_medium).color(color))
        .child(mark)
}

/// Phantom's tab bar: the tabs in a row, a hairline under them.
pub(super) fn phantom_tab_bar(ui: &Ui, tabs: Vec<El>) -> El {
    let rule = el().key("rule").pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(ui.k.c.border_weak);
    row().role("tablist").px(10.0).gap(18.0).child(rule).children(tabs)
}

/// Phantom puts nothing in the band's free room: it's only there to drag the window by.
pub(super) fn phantom_band(_: &Ui, _: BandSide) -> El {
    el()
}

impl Ui {
    /// One tab, `selected` or not, as the look draws it: the screen keys it and gives it its press.
    pub fn tab(&self, label: &str, selected: bool) -> El {
        (self.blocks.tab)(self, label, selected)
    }

    /// The row holding `tabs` ([`Ui::tab`]), as the look draws it.
    pub fn tab_bar(&self, tabs: Vec<El>) -> El {
        (self.blocks.tab_bar)(self, tabs)
    }

    /// What the look draws in the titlebar band's free room on `side` (a theme's logo); the screen
    /// keeps the room a place to drag the window by.
    pub fn band(&self, side: BandSide) -> El {
        (self.blocks.band)(self, side)
    }
}
