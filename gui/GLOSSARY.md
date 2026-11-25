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
**editor sheet** over Settings; while the sheet is open both windows show it (never kept), and closing
it shows the Active look again. The editor's body is split down the middle, the left half ☀ Light drawn
entirely in the preset's Light look and the right half ☾ Dark entirely in its Dark one (a **joined**
row, one control for both, left-aligned in the left column where a split row's ☀ control is; a
**split** row, one per column; its Join or Split button turns one into the other, and only while one
theme is in both halves; with a theme in each half the columns are two lists, each with its own Theme
pop-up first, nothing joined). Its **Cancel** (and Escape) puts the preset back
as it was when the sheet opened; its **Done** keeps what changed as one undo step ("Edit of Night"),
while inside the sheet ⌘Z steps back one change at a time. Save as… keeps the Active look under a name; a name one has asks
whether to replace it. A name is never blank, and no two saved ones share one.
_Avoid_: profile, favourite; pick, load (for Apply)

**Active Look Preset**:
The working copy that defines how the app looks now; the app calls it the **Active look**. It is
the first row of the Look Presets: its ☀ and ☾ chips show its halves and open its Edit…, and it has Edit… and
Save as…, no Apply, Delete or ◐. Its Edit… opens the same editor sheet as a saved preset's, whose
edits are the real settings, Done keeping them as one undo step and Cancel putting them back. Applying a saved Look Preset, or a half of one,
copies it in; tweaking changes only the active one, and the saved one stays until it's replaced
(Save as… under its name).
_Avoid_: current preset, current look

**Look chip**:
A small square button for a Look Preset's half (☀ Light, ☾ Dark) or both (◐, `circle.lefthalf.filled`),
drawn as the look it stands for: its glyph in that half's text colour on its page (◐: split along
the diagonal from bottom-left to top-right, the Light's page top-left and the Dark's bottom-right;
its glyph is one ring on the same diagonal, the top-left half an arc in the Light's text colour,
the bottom-right half an arc in the Dark's, nothing along the seam, for every preset alike). It carries no theme name; its hover says what a press does ("Apply Evening's
Light to the active look"). In use, it's outlined 3 pt in that half's accent, 2 pt out from it.
The App icon's sheet has the same chips under each icon, ☀ on a light page and ☾ on a dark one. Like the editor, the sheet is an **undo session**: ⌘Z in it takes back this visit's picks one at a time and it stays open; with none left ⌘Z does nothing; closing it makes the visit one undo step ("App icon: Dark Hour → Memento Mori").
_Avoid_: swatch, badge, pill

**App icon**:
The icon Phantom shows in the Dock, Finder and launchers. It is not part of a Look Preset. Either it
follows the active Theme Look, or it's one icon always, or a light icon and a dark icon that swap with
the Appearance; any icon from any theme can be picked for either half (its ☀ or ☾ chip), and so can
an image a person adds, or either half can follow the theme alone (Follow theme's ☀ or ☾). Following
the theme is the default; a half whose image is gone (removed, or moved on disk) follows it. An added image, of any size, must decode as an image (not just have an image extension); otherwise it's refused with a line under the tiles and never becomes a tile. Added images are read off the main thread and kept scaled to 1024 px on the long side: until one is read, a half naming it shows the theme's icon and its tile is a placeholder. Follow theme's tile shows the theme's icon for the window's Appearance. A chip
is in use as the Look Presets' are: ◐ while both halves are that icon, else ☀ under the Light
half's and ☾ under the Dark half's.

**Designed pair**:
A dark icon and a light icon made to go together, with a short name of its own ("Memento Mori"):
its tile in the App icon's sheet's Pairs (as wide as an icon's tile but shorter, with no chips, on the same grid), its two icons
split on the diagonal, picks both, and is in use while both halves are its icons, when the ☀ chip
under its Light icon and the ☾ chip under its Dark one are in use too; any two icons can also be picked one per half.
_Avoid_: icon set, icon variant

## Notices

**Notice**:
A short line in the status bar about something that happened: an add, the settings reset, a
failure. It stays until it's dismissed with ×, or all of them with Dismiss all. A
condition, something that holds while it's the case (a disk short of space, a drive that isn't
connected, a default folder that can't be reached), is no notice: it has a natural place in the
window and is shown there while it holds, from the latest update, so there's nothing to dismiss.
The notice list is only for what has no such place.
A notice's **line** (in the status bar) holds only words Phantom writes, short and known in advance:
"Added the torrent", "Can't open the file", "Download limit undone" (a setting's name is Phantom's).
Anything from outside goes in the **list** behind it: file paths, file names, torrent names (and
names a user gave a Look Preset), errors from macOS or a peer, a change as "old → new" ("10 MB/s →
2 MB/s"; the setting's name stays in the line), the settings a reset reset ("Settings reset"). A
**path or a link** has a line of its own in the list, shortened in the middle (whole on hover),
never inside a sentence; a failed magnet add shows the link's name (`dn=`) if it has one, else the
link so. A **reason** is short words under the name that never repeat the line ("The new
destination's disk is full", not "Not moved: …"), with the path under it ("Something is already
at"). A notice tells what happened, never why Phantom works so: a Remove says no word of its files
(a Delete says "Its files are in the Trash"). A notice whose path is on disk has **Reveal in
Finder** (the torrent list kept aside, a part not saved, files a refused Delete's undo left in
place); a failed Open of a file that isn't there has **Show folder** while its folder is. A
warning or error offers what the person would do next, when Phantom can do it: at most three buttons,
the likeliest first, named by what happens, in sentence case. **Try again** (a failed add, Move,
Delete, Pause or Resume) runs the same command as before, on the failed torrents only, so every
safety rule, refusal and clash dialog applies again; pressing it takes its notice away, so it runs
once. A failed Move also has **Move to…** (the torrents left); a failed Open of a file that isn't on
disk has **Check** after Show folder. A button that no longer applies is not shown: its torrent is
gone (a torrent removed and added again is another), its command already went through, or the
service says it can't run now. A receipt has none. The bar shows the newest notice's line in its own colour (a receipt
is never amber), the Undo of the newest action's notice (the small button the connection's are), then
the **pill**, an oval with the count of every notice tinted by the worst severity among them (a
count, not a place: no "1/7"; it shows from two notices), then ›. A click
on the bar always opens the list, at the newest, even for a lone notice; only × (or Dismiss all)
dismisses. The list shows one notice at a time: with several, a top row (‹ 2 of 3 › and Dismiss
all) over a hairline, with none for a lone notice; then the notice, its mark (✓ done, ⓘ nothing was done ("Already in the list"), ⚠, ⛔), its line
and × on its own line, the names, why, and its buttons, the first the default. The popover is as wide as the widest notice in the list needs, the same
on every page (it changes when a notice arrives or leaves): a path counts for its real width and is
cut in the middle at 480 pt; never under about 240 pt, which fits the pager and Dismiss all. Its Undo is in the
bar too, and "Undo ⌘Z" in the list.
The connection's words beside it are short and fixed too ("Connecting…", "Service unavailable",
"Phantom was updated"), with no ›, red while the dot is, as an error notice's line is; while the
connection is wrong a button follows them (Show log; Restart, which starts the app again once this
one has quit).
_Avoid_: alert, message (the thing is a notice; its button says Dismiss all), live notice

**Timed notice**:
A notice that leaves by itself once the notice list (the bar's popover) has been closed long enough:
a **receipt** (Added the torrent, Already in the list, an undo's "Back in the list" or "Renamed back", a setting undone, "Copied the
magnet link") 5 s after, an **action's notice** (Removed, Deleted, Moved to the Trash, Renamed, with its
Undo button) 1 minute after. Its clock runs only while the list is closed: opening the list stops
it, closing gives every timed notice in it its full time again, and one told while the list is open
starts no clock until it closes. It leaves whether or not it's the one the bar shows, as a dismissal
would. Warnings, errors, the settings-reset, torrent-list-set-aside and part-not-saved notices and a partly failed undo are until dismissed. A
notice's length is its `Lasts`.
_Avoid_: toast, transient notice, auto-dismiss

**Error notice**:
A notice of severity Error, "Couldn't undo the change" (or "Couldn't undo the Delete", with the
names in the list) among them, for a setting as for a torrent: where errors with no better place show: the adds made outside the window
(files dropped on it or opened from Finder, a magnet clicked in a browser), Add file…, the folder
picker and failed commands. In the status bar in the error colour, the pill red, explained in the
popover a click opens (an error never opens it by itself), and dismissed with ×; a batch of adds stays one notice ("Couldn't add 2 of
5", each failure under it as its view). What the service refuses is a notice, the field's own add
too ("No peers have this torrent"). What's known before anything is sent stays on the control: a
**tooltip**.

**Tooltip**:
Why something can't be done, known before anything is sent, on the control clicked: a disabled
button's (Resume with nothing paused), Add's with text that isn't a link, a name the Rename field
can't take (under the field, until the next keystroke). Neutral, the same in Light and Dark: one
shape, a raised surface with a single hairline, the text's own colour, never the error colour. A
hover's explanation (what a clash's choice does, once the pointer rests on it for 0.5 s) is one too. It's Phantom's own block (`ui/popover.rs`), not the system's tooltip, so it looks the same everywhere.
_Avoid_: error note, red bubble, alert

## Wording

**What happens, not why**:
The window says what happens, never why Phantom is designed that way. A dialog asks its question in
its title and names what each choice does in that choice's tooltip; a rule behind it ("A torrent's
files are never replaced or used by another's") goes unsaid.

## Commands

**Command palette**:
A panel over the window (⌘P) listing the commands available for the selection, each with its icon
and key: those for the selected torrents, then for the files selected in the details pane, then the
app's own (Add file…, Undo named after what it takes back). Each group is under a muted heading
naming what it acts on (the torrent's or file's name, "3 torrents", "2 files", "Phantom"); a
heading shows only while one of its commands matches. Typing filters by name,
the letters in order; ↑ ↓ move, Return runs, Escape closes. A command that asks for more starts its
usual flow (Rename… the header's name field, Move to… the folder picker).
_Avoid_: launcher, quick open, command bar
