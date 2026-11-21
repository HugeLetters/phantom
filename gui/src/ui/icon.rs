//! The icons screens can ask for. A screen names an [`Icon`]; the theme decides how it's drawn
//! (each icon is a block of its own, see `blocks`), so a theme can swap any icon without touching
//! a screen. Add a variant when a screen first needs it, with its drawing in the Phantom theme.

use machine::every;

use super::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Two overlapping sheets: copy something.
    Copy,
    /// A triangle with an exclamation mark: a warning.
    Warning,
    /// A check mark: the current one of several values, in a menu's icon gutter.
    Check,
    /// A sun: the Light Appearance.
    Light,
    /// A moon: the Dark Appearance.
    Dark,
    /// A circle with its left half filled: both Appearances.
    Both,
    // The commands' (`commands`), one each; a file command shares its torrent twin's.
    /// A play triangle: Resume.
    Resume,
    /// Two bars: Pause.
    Pause,
    /// A page with a check: Check (verify the files).
    Verify,
    /// A circle with a cross: Cancel check.
    CancelCheck,
    /// A folder with a cross: Cancel move.
    CancelMove,
    /// A box with an arrow out of it: Open.
    Open,
    /// A folder with a magnifier: Reveal in Finder.
    Reveal,
    /// A folder with an arrow into it: Move to….
    MoveTo,
    /// A pencil: Rename….
    Rename,
    /// A list with a line taken off it: Remove (keep files).
    Remove,
    /// A bin: Delete, and Move to Trash of files.
    Trash,
    /// A page with a plus: Add file….
    AddFile,
    /// An arrow turning back: Undo.
    Undo,
    /// A magnifier: the command palette's field.
    Search,
    /// A check in a circle: a notice of something done (a receipt, an action's notice).
    Done,
    /// A stop sign with a cross: an error notice.
    Error,
    /// ×: dismiss.
    Close,
    /// ‹: a step back, toward the newer notices.
    Previous,
    /// ›: a step on, toward the older notices.
    Next,
}

every!(Icon {
    Icon::Copy => [Icon::Copy],
    Icon::Warning => [Icon::Warning],
    Icon::Check => [Icon::Check],
    Icon::Light => [Icon::Light],
    Icon::Dark => [Icon::Dark],
    Icon::Both => [Icon::Both],
    Icon::Resume => [Icon::Resume],
    Icon::Pause => [Icon::Pause],
    Icon::Verify => [Icon::Verify],
    Icon::CancelCheck => [Icon::CancelCheck],
    Icon::CancelMove => [Icon::CancelMove],
    Icon::Open => [Icon::Open],
    Icon::Reveal => [Icon::Reveal],
    Icon::MoveTo => [Icon::MoveTo],
    Icon::Rename => [Icon::Rename],
    Icon::Remove => [Icon::Remove],
    Icon::Trash => [Icon::Trash],
    Icon::AddFile => [Icon::AddFile],
    Icon::Undo => [Icon::Undo],
    Icon::Search => [Icon::Search],
    Icon::Done => [Icon::Done],
    Icon::Error => [Icon::Error],
    Icon::Close => [Icon::Close],
    Icon::Previous => [Icon::Previous],
    Icon::Next => [Icon::Next],
});

impl Ui {
    /// How the theme draws `icon`; Rune's icon, for anything that draws one.
    pub fn icon(&self, icon: Icon) -> &'static rune::Icon {
        self.blocks.icon(icon)
    }
}
