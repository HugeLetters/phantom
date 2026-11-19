//! The app's two windows (#257): the main window, whose state is [`Phantom`](crate::model::Phantom),
//! and Settings, whose state is [`SettingsWindow`](crate::model::settings::SettingsWindow). Rune
//! gives each window its own state, and a text field or a table binds to its own window's, so
//! each owns what only it shows and edits: the main window the service connection, the list and
//! the undo list (every change ⌘Z takes, from either window); Settings its sheet and the App icon.
//! What both draw, the looks, is [`Shared`]: a Tone change redraws both.
//!
//! One window asks something of the other through a [`Link`]: what it posts runs there, with
//! that window's state, in the order posted, before that window's next frame. So a change made
//! in Settings reaches the undo list before anything the main window does after it, and ⌘Z in
//! Settings is the main window's ⌘Z. A [`Window`] brings a window to the front or closes it.
//!
//! ```text
//! Settings ──post──▶ main: a setting sent, an app setting changed, ⌘Z, the settings asked for
//! main ──post──▶ Settings: an answer, the settings a snapshot carries, an undo shown, Open settings
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use rune::Tx;

/// What both windows read and either may change: the looks.
pub type Shared<T> = Rc<RefCell<T>>;

/// Which window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Which {
    #[default]
    Main,
    Settings,
}

type Task<S> = Box<dyn FnOnce(&mut S, &mut Tx)>;

/// What the app asks of one of its windows: to come to the front (shown if it was closed, restored
/// if minimized, the app active with it) or to close. Nothing for a window that isn't there, as in
/// a test of the other one alone.
#[derive(Clone)]
pub struct Window {
    raise: Rc<dyn Fn()>,
    close: Rc<dyn Fn()>,
}

impl Window {
    pub fn new(raise: impl Fn() + 'static, close: impl Fn() + 'static) -> Self {
        Self { raise: Rc::new(raise), close: Rc::new(close) }
    }

    pub fn none() -> Self {
        Self::new(|| {}, || {})
    }

    pub fn raise(&self) {
        (self.raise)()
    }

    pub fn close(&self) {
        (self.close)()
    }
}

/// The way to another window, whose state is `S`.
pub struct Link<S> {
    queue: Rc<RefCell<Vec<Task<S>>>>,
    wake: Rc<dyn Fn()>,
}

impl<S> Clone for Link<S> {
    fn clone(&self) -> Self {
        Self { queue: self.queue.clone(), wake: self.wake.clone() }
    }
}

/// What other windows posted to a window, for it to run ([`Inbox::run`]).
pub struct Inbox<S> {
    queue: Rc<RefCell<Vec<Task<S>>>>,
}

/// A link to a window and its inbox. `wake` makes that window run its `poll` (where it takes
/// what's posted) even while it's hidden.
pub fn link<S>(wake: impl Fn() + 'static) -> (Link<S>, Inbox<S>) {
    let queue = Rc::new(RefCell::new(Vec::new()));
    (Link { queue: queue.clone(), wake: Rc::new(wake) }, Inbox { queue })
}

/// A link to nowhere: what's posted is never run. For a window alone, as in a test of one.
pub fn detached<S>() -> Link<S> {
    link(|| {}).0
}

impl<S> Link<S> {
    /// `task` runs in the window, with its state, before its next frame, after what was posted
    /// before it.
    pub fn post(&self, task: impl FnOnce(&mut S, &mut Tx) + 'static) {
        self.queue.borrow_mut().push(Box::new(task));
        (self.wake)();
    }
}

impl<S> Inbox<S> {
    /// Runs what was posted, in order, also what that posts to this window meanwhile; whether
    /// there was anything. `inbox` finds the inbox in the state.
    pub fn run(s: &mut S, inbox: fn(&S) -> &Inbox<S>, tx: &mut Tx) -> bool {
        let mut ran = false;
        loop {
            let tasks = std::mem::take(&mut *inbox(s).queue.borrow_mut());
            if tasks.is_empty() {
                return ran;
            }
            ran = true;
            for task in tasks {
                task(s, tx);
            }
        }
    }
}
