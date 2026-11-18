//! [`Notice`]: every message in the app, a short line and, optionally, more. Where one shows is
//! separate from the type: the status bar ([`crate::bar`], whose popover shows the more). A failure
//! is one of severity Error.

use std::rc::Rc;

use rune::{El, col, row};

use crate::model::Phantom;
use crate::ui::Ui;

/// Rune rebuilds the tree every frame, so the state holds a builder, not an element.
pub type View = Rc<dyn Fn(&Phantom, &Ui) -> El>;

/// How much a notice matters; the status bar colours it.
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
    /// The undo stack's entries (by `seq`) of the actions this notice tells of, while none of them
    /// is undone: a notice with some leaves with the last of them (#200). Empty: it tells of no
    /// action, so no undo takes it away.
    pub of: Vec<u64>,
}

impl Notice {
    /// An information.
    pub fn new(short: impl Into<String>) -> Self {
        Self { short: short.into(), severity: Severity::Info, more: None, of: Vec::new() }
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
pub struct NoticeAction {
    pub title: String,
    pub run: Rc<dyn Fn(&mut Phantom)>,
    /// Its shortcut, from the table, shown beside it while it's that key's.
    pub hint: Option<Rc<dyn Fn(&Phantom) -> Option<String>>>,
    /// Why it's off now, if it is: then it's drawn disabled, and a press on it says why, as a
    /// disabled button does (#89).
    pub off: Option<Rc<dyn Fn(&Phantom) -> Option<String>>>,
}

impl NoticeAction {
    pub fn new(title: impl Into<String>, run: impl Fn(&mut Phantom) + 'static) -> Self {
        Self { title: title.into(), run: Rc::new(run), hint: None, off: None }
    }
}

/// The preset view: `text` explaining, with a button for each of `actions`, its hint beside it;
/// one that's off says why under them once pressed.
pub fn explained(text: String, actions: Vec<NoticeAction>) -> View {
    Rc::new(move |s, ui| {
        let mut why_shown = None;
        let buttons = actions.iter().enumerate().map(|(i, action)| {
            let run = action.run.clone();
            let off = action.off.as_ref().and_then(|off| off(s));
            if off.is_some() && s.why_off_note == off {
                why_shown = off.clone();
            }
            let note = off.clone();
            let button = ui
                .button("notice-action", &action.title)
                .disabled(off.is_some())
                .on_press(move |s: &mut Phantom| run(s))
                .on_press_disabled(move |s: &mut Phantom| s.why_off_note = note.clone())
                .build();
            row()
                .gap(6.0)
                .center_y()
                .child(button)
                .children(action.hint.as_ref().and_then(|hint| hint(s)).map(|hint| ui.caption(hint).key("notice-action-hint")))
                .key(format!("notice-action/{i}"))
        });
        let buttons: Vec<El> = buttons.collect();
        col()
            .key("notice-explained")
            .gap(8.0)
            .child(ui.caption(text.clone()).wrap())
            .child(row().gap(12.0).children(buttons))
            .children(why_shown.map(|why| ui.caption(why).wrap().key("notice-action-why")))
    })
}
