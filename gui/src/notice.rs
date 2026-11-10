//! [`Notice`]: every message in the app, a short line and, optionally, more. Where one shows is
//! separate from the type: the status bar, or the middle of the window for add errors.

use std::rc::Rc;

use rune::{El, col, row};

use crate::model::Phantom;
use crate::ui::Ui;

/// Rune rebuilds the tree every frame, so the state holds a builder, not an element.
pub type View = Rc<dyn Fn(&Phantom, &Ui) -> El>;

/// How much a notice matters; the status bar and the error list colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    /// Something was changed or skipped that the user should know of.
    Warning,
    Error,
}

/// What to tell the user: `short`, and `more` to open for the rest. All wording comes from
/// `client::display`.
#[derive(Clone)]
pub struct Notice {
    pub short: String,
    pub severity: Severity,
    /// `None`: nothing to click.
    pub more: Option<View>,
}

impl Notice {
    /// An information.
    pub fn new(short: impl Into<String>) -> Self {
        Self { short: short.into(), severity: Severity::Info, more: None }
    }

    pub fn warning(short: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, ..Self::new(short) }
    }

    pub fn error(short: impl Into<String>) -> Self {
        Self { severity: Severity::Error, ..Self::new(short) }
    }
}

/// Prints the short line.
impl std::fmt::Debug for Notice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Notice({:?})", self.short)
    }
}

/// A button under an explanation, pressed like any button.
#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct NoticeAction {
    pub title: String,
    pub run: Rc<dyn Fn(&mut Phantom)>,
}

/// The preset view: `text` explaining, with a button for each of `actions`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn explained(text: String, actions: Vec<NoticeAction>) -> View {
    Rc::new(move |_, ui| {
        let buttons = actions.iter().enumerate().map(|(i, action)| {
            let run = action.run.clone();
            ui.button("notice-action", &action.title).on_press(move |s: &mut Phantom| run(s)).build().key(format!("notice-action/{i}"))
        });
        col().key("notice-explained").gap(8.0).child(ui.caption(text.clone()).wrap()).child(row().gap(6.0).children(buttons))
    })
}
