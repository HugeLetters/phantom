//! [`Notice`]: every message in the app, a short line and, optionally, more. Where one shows is
//! separate from the type: the status bar ([`crate::bar`], whose popover shows the more). A failure
//! is one of severity Error.
//!
//! # What goes in the line
//!
//! The line holds only words Phantom writes: short, and known in advance ("Added the torrent",
//! "Can't open the file", "Download limit undone"; a setting's name is Phantom's own). Anything
//! from outside goes in the list, as [`Notice::with_names`], [`Notice::with_text`] or a [`Listing`]: file paths, file names,
//! torrent names (and the names a user gave a Look Preset), errors from macOS or a peer, and a change as
//! "old → new". A notice whose line leaves something out has more to read, so it has a list, and a
//! click on the bar opens the list even when it's the only notice. Wording lives in `client::display`:
//! a notice's line and its list are separate functions there, and a test
//! (`a_bar_line_holds_only_fixed_words_and_the_names_are_in_the_list`) tells every kind with long
//! names.

use std::rc::Rc;

use client::display;

use crate::model::Phantom;

/// What a notice's list says under its line: the names the line leaves out, a reason of their own,
/// the paths each on a line of its own (as `client::display` words them).
pub use client::display::{Line, Listing};

/// How much a notice matters; the status bar colours it. Ordered from least to most: the worst of
/// several is their `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    /// Something was changed or skipped that the user should know of.
    Warning,
    Error,
}

/// How long a notice stays in the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lasts {
    /// Until the user dismisses it, or what it tells of is undone.
    #[default]
    UntilDismissed,
    /// A receipt: it leaves by itself [`BRIEF_SECS`] after the popover was last closed (a timed notice).
    Brief,
    /// An action's notice, with its Undo: [`LONG_SECS`] after the popover was last closed.
    Long,
}

impl Lasts {
    /// How long its clock runs, once the popover is closed; `None`: it has none.
    pub fn secs(self) -> Option<f64> {
        match self {
            Lasts::UntilDismissed => None,
            Lasts::Brief => Some(BRIEF_SECS),
            Lasts::Long => Some(LONG_SECS),
        }
    }
}

/// How long a [`Lasts::Brief`] notice stays once the popover is closed.
pub const BRIEF_SECS: f64 = 5.0;

/// How long a [`Lasts::Long`] notice stays once the popover is closed.
pub const LONG_SECS: f64 = 60.0;

/// What to tell the user: `short`, and `more` to open for the rest, with its buttons. All wording
/// comes from `client::display`.
#[derive(Clone)]
pub struct Notice {
    pub short: String,
    pub severity: Severity,
    /// What the list shows under the line: the names the line leaves out, and why. `None`: the
    /// line says all there is.
    pub more: Option<Listing>,
    /// Its buttons, under it in the list, right-aligned; the first is the default (an action's
    /// Undo).
    pub actions: Vec<NoticeAction>,
    /// The undo stack's entries (by `seq`) of the actions this notice tells of, while none of them
    /// is undone: a notice with some leaves with the last of them (#200). Empty: it tells of no
    /// action, so no undo takes it away.
    pub of: Vec<u64>,
    pub lasts: Lasts,
    /// A receipt of something that didn't happen ("Already in the list"): a neutral ⓘ instead of ✓.
    pub nothing: bool,
    /// The torrent whose files don't match (#358) it tells of: it goes once that's no longer so
    /// (lifted, resumed, removed).
    pub about: Option<proto::TorrentId>,
}

impl Notice {
    /// An information.
    pub fn new(short: impl Into<String>) -> Self {
        Self {
            short: short.into(),
            severity: Severity::Info,
            more: None,
            actions: Vec::new(),
            of: Vec::new(),
            lasts: Lasts::UntilDismissed,
            nothing: false,
            about: None,
        }
    }

    /// Why, as text in the list, muted: a reason from macOS or a peer, an explanation.
    pub fn with_text(self, why: impl Into<String>) -> Self {
        Self { more: Some(Listing::new("", why)), ..self }
    }

    /// A receipt that says nothing was done: it wears ⓘ, not the ✓ of a success.
    pub fn nothing_done(self) -> Self {
        Self { nothing: true, ..self }
    }

    /// `more` as the list.
    pub fn with_listing(self, more: Listing) -> Self {
        Self { more: Some(more), ..self }
    }

    /// What the line leaves out, in the list, a line each: torrent and file names, paths.
    pub fn with_names(self, names: impl Into<String>) -> Self {
        Self { more: Some(Listing::new(&names.into(), "")), ..self }
    }

    /// Its buttons, the first the default.
    pub fn with_actions(self, actions: Vec<NoticeAction>) -> Self {
        Self { actions, ..self }
    }

    /// Whether the list has more to show than the line: names, a reason, or buttons.
    #[cfg(test)]
    pub fn has_list(&self) -> bool {
        self.more.is_some() || !self.actions.is_empty()
    }

    /// Its Undo, if it's an action's notice.
    pub fn undo(&self) -> Option<&NoticeAction> {
        self.actions.iter().find(|a| a.undo)
    }

    /// A timed notice: it leaves by itself, [`BRIEF_SECS`] after the popover is closed.
    pub fn brief(self) -> Self {
        Self { lasts: Lasts::Brief, ..self }
    }

    /// A timed notice for an action, with its Undo: [`LONG_SECS`] after the popover is closed.
    pub fn long(self) -> Self {
        Self { lasts: Lasts::Long, ..self }
    }

    /// A setting's undo that worked: "Download limit undone", brief, and in the popover "10 MB/s →
    /// 2 MB/s".
    pub fn setting_undone(short: &str, undone: &str, now: &str) -> Self {
        Self::new(display::setting_undone_short(short)).brief().with_names(display::changed(undone, now))
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

impl Notice {
    /// The service couldn't read the torrents' database and kept it aside at `kept`: a warning
    /// that stays until dismissed (#308).
    pub fn records_set_aside(kept: &std::path::Path) -> Self {
        Self::warning(display::records_set_aside())
            .with_listing(display::records_set_aside_listing(kept))
            .with_actions(vec![NoticeAction::reveal(kept)])
    }
}

impl Notice {
    /// Torrent `id` found where it is now (#358), `name` at `at`, whose check found no piece with
    /// real content there: it's stopped and its files untouched. A warning that stays until it's
    /// dismissed or the torrent isn't held so any more. Its buttons act on this torrent alone, each
    /// offered while it would do something to it: Check again, Download over them (Resume), and
    /// Reveal in Finder.
    pub fn files_dont_match(id: proto::TorrentId, name: &str, at: &std::path::Path) -> Self {
        use proto::Action;
        let does = move |action: Action| move |s: &Phantom| s.torrents.iter().find(|t| t.id == id).is_some_and(|t| t.actions.does(action));
        let on_it = move |action: Action, command: proto::TorrentCommand| {
            move |s: &mut Phantom| {
                // Only this torrent, and only while it would do something: the notice may be old.
                if s.torrents.iter().find(|t| t.id == id).is_some_and(|t| t.actions.does(action)) {
                    crate::model::command(&mut *s.service, &[id], command.clone());
                }
            }
        };
        let mut check = NoticeAction::new(display::check_again(), on_it(Action::Check, proto::TorrentCommand::Check));
        check.shown = Some(Rc::new(does(Action::Check)));
        let mut resume = NoticeAction::new(display::download_over_them(), on_it(Action::Resume, proto::TorrentCommand::Resume));
        resume.shown = Some(Rc::new(does(Action::Resume)));
        Self {
            about: Some(id),
            ..Self::warning(display::files_dont_match())
                .with_listing(display::found_listing(&[(name.to_owned(), at.to_owned())]))
                .with_actions(vec![check, resume, NoticeAction::reveal(at)])
        }
    }
}

impl Notice {
    /// A part of a file in the Trash couldn't be kept on disk, even at a retry: a warning that
    /// stays until dismissed (#304).
    pub fn part_not_saved(not_saved: &proto::PartNotSaved) -> Self {
        Self::warning(display::part_not_saved())
            .with_listing(display::part_not_saved_listing(&not_saved.file, &not_saved.reason))
            .with_actions(vec![NoticeAction::reveal(&not_saved.file)])
    }
}

/// A button under a notice in the list, pressed like any button.
#[derive(Clone)]
pub struct NoticeAction {
    pub title: String,
    pub run: Rc<dyn Fn(&mut Phantom)>,
    /// Its shortcut, from the table, shown inside it, muted, while it's that key's.
    pub hint: Option<Rc<dyn Fn(&Phantom) -> Option<String>>>,
    /// Why it's off now, if it is: then it's drawn disabled, and a press on it says why, as a
    /// disabled button does (#89).
    pub off: Option<Rc<dyn Fn(&Phantom) -> Option<String>>>,
    /// It's an action's Undo, which the bar offers too, after its line.
    pub undo: bool,
    /// Whether it's offered now: `None`, always. Reveal in Finder is, while what it shows is on disk.
    pub shown: Option<Rc<dyn Fn(&Phantom) -> bool>>,
}

impl NoticeAction {
    pub fn new(title: impl Into<String>, run: impl Fn(&mut Phantom) + 'static) -> Self {
        Self { title: title.into(), run: Rc::new(run), hint: None, off: None, undo: false, shown: None }
    }

    /// Reveal in Finder for `path`, offered while it's on disk.
    pub fn reveal(path: &std::path::Path) -> Self {
        Self::on_disk(display::reveal_in_finder(), path)
    }

    /// Show folder: reveals `folder` in Finder (never opens it: a package would launch), offered
    /// while it's on disk (a failed Open's, the file's folder).
    pub fn folder(folder: &std::path::Path) -> Self {
        Self::on_disk(display::show_folder(), folder)
    }

    /// A button that reveals `path` in Finder, offered while it's on disk.
    fn on_disk(title: &str, path: &std::path::Path) -> Self {
        let (run, there) = (path.to_path_buf(), path.to_path_buf());
        let mut action = Self::new(title, move |s: &mut Phantom| crate::platform::show(s, &run, crate::platform::Show::Reveal));
        action.shown = Some(Rc::new(move |s: &Phantom| s.finder.exists(&there)));
        action
    }
}
