# GUI

The words of the Phantom app's own look and messages: its themes, their Look Presets and the App
icon, and the Notices in its status bar. Global words are in the [root glossary](../GLOSSARY.md).

## Look

**Theme**:
A collection of elements that defines how the app looks, with the settings it declares: Phantom (the
default), Persona 5: Phantom Thieves, Persona 5: Jose's shop, or Persona 3 Reload. The two Persona 5
themes are Phantom Thieves and Jose's shop for short.
_Avoid_: skin, style, palette; All-Out (for Phantom Thieves); Jose palette (for Jose's shop)

**Theme setting**:
A value a theme declares and the user can change, like Phantom's Tone or a theme's animations on or
off. Any theme setting can be set differently for the Light and the Dark Appearance.
_Avoid_: option, variant

**Appearance**:
Light or Dark, as the system sets it. Every theme can be shown in either; a theme says nothing about
it. The Settings tab named Appearance holds the theme and the App icon; in the app's text,
Appearance alone means Light or Dark.
_Avoid_: mode, dark mode, color scheme, side

**Theme Look**:
A theme with its settings, as one Appearance shows it: "Persona 3 Reload with animations off".
_Avoid_: skin, configured theme

**Look Preset**:
A named pair of Theme Looks: the one the Light Appearance shows and the one the Dark Appearance
shows. Often both are the same Theme Look. One half of a Look Preset can be copied into the active
one without the other.
_Avoid_: theme preset, profile

**Saved Look Preset**:
A Look Preset kept by name, in the Appearance tab's Look Presets, a row each, in columns shared with
the Active look's: a **look chip** for both halves (◐), for Light (☀) and for Dark (☾), Edit… and Delete
(as wide as Save as…). A press on a chip **applies** that
(◐: both halves) to the Active look. A chip whose half is the Active look's, value for value, is
**in use** (◐: both are); nothing is remembered of where a half came from. Edit… opens it in the
editor sheet over Settings; while the sheet is open both windows show it (never kept), and closing
it shows the Active look again. Save as… keeps the Active look under a name; a name one has asks
whether to replace it. A name is never blank, and no two saved ones share one.
_Avoid_: profile, favourite; pick, load (for Apply)

**Active Look Preset**:
The working copy that defines how the app looks now; the app calls it the **Active look**. It is
the first row of the Look Presets: its ☀ and ☾ chips only show its halves, and it has Edit… and
Save as…, no Apply, Delete or ◐. Its Edit… opens the same editor sheet as a saved preset's, whose
edits are the real settings, each one undo step. Applying a saved Look Preset, or a half of one,
copies it in; tweaking changes only the active one, and the saved one stays until it's replaced
(Save as… under its name).
_Avoid_: current preset, current look

**Look chip**:
A small square button for a Look Preset's half (☀ Light, ☾ Dark) or both (◐, `circle.lefthalf.filled`),
drawn as the look it stands for: its glyph in that half's text colour on its page (◐: split along
the diagonal from bottom-left to top-right, the Light's page top-left and the Dark's bottom-right;
its glyph is one ring on the same diagonal, the top-left half an arc in the Light's text colour,
the bottom-right half an arc in the Dark's, nothing along the seam, for every preset alike). It carries no theme name; its hover says what a press does ("Apply Evening's
Light to the active look"). In use, it's outlined 2 pt in that half's accent.
_Avoid_: swatch, badge, pill

**App icon**:
The icon Phantom shows in the Dock, Finder and launchers. It is not part of a Look Preset. Either it
follows the active Theme Look, or it's one icon always, or a light icon and a dark icon that swap with
the Appearance; any icon from any theme can be picked. Following the theme is the default.

**Designed pair**:
A dark icon and a light icon made to go together, offered as one App icon choice; any two icons
can also be combined into a custom pair.
_Avoid_: icon set, icon variant

## Notices

**Notice**:
A short line in the status bar about something that happened: an add, the settings reset, a
failure. It stays until it's dismissed with ×, or all of them with "Dismiss n messages". A
condition, something that holds while it's the case (a disk short of space, a drive that isn't
connected, a default folder that can't be reached), is no notice: it has a natural place in the
window and is shown there while it holds, from the latest update, so there's nothing to dismiss.
The notice list is only for what has no such place.
A notice's **line** (in the status bar) holds only words Phantom writes, short and known in advance:
"Added the torrent", "Can't open the file", "Download limit undone" (a setting's name is Phantom's).
Anything from outside goes in the **list** behind it: file paths, file names, torrent names (and
names a user gave a Look Preset), errors from macOS or a peer, "back from X to Y". A notice whose
line leaves something out has a list, and a click on it opens the list even when it's the only
notice.
_Avoid_: alert, message (the button says "messages", the thing is a notice), live notice

**Timed notice**:
A notice that leaves by itself once the notice list (the bar's popover) has been closed long enough:
a **receipt** (Added the torrent, Already in the list, an undo's "Back in the list", a setting undone, "Copied the
magnet link") 5 s after, an **action's notice** (Removed, Deleted, Moved to the Trash, with its
Undo button) 1 minute after. Its clock runs only while the list is closed: opening the list stops
it, closing gives every timed notice in it its full time again, and one told while the list is open
starts no clock until it closes. It leaves whether or not it's the one the bar shows, as a dismissal
would. Warnings, errors, the settings-reset, torrent-list-set-aside and part-not-saved notices and a partly failed undo are until dismissed. A
notice's length is its `Lasts`.
_Avoid_: toast, transient notice, auto-dismiss

**Error notice**:
A notice of severity Error: where errors with no better place show: the adds made outside the window
(files dropped on it or opened from Finder, a magnet clicked in a browser), Add file…, the folder
picker and failed commands. In the status bar in the error colour with the count, explained in the
popover a click opens (an error never opens it by itself), and dismissed with ×; a batch of adds stays one notice ("Couldn't add 2 of
5", each failure under it as its view). Errors that have a place stay there: the link field's by
Add, a disabled button's on the button.
