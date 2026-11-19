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

**Active Look Preset**:
The working copy that defines how the app looks now. Picking a saved Look Preset copies it in;
tweaking changes only the active one, and the saved one stays until it's updated.
_Avoid_: current preset

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
_Avoid_: alert, message (the button says "messages", the thing is a notice), live notice

**Timed notice**:
A notice that leaves by itself once the notice list (the bar's popover) has been closed long enough:
a **receipt** (Added X, X is already in the list, an undo's "is back", a setting undone, "Copied the
magnet link of X") 5 s after, an **action's notice** (Removed, Deleted, Moved to the Trash, with its
Undo button) 1 minute after. Its clock runs only while the list is closed: opening the list stops
it, closing gives every timed notice in it its full time again, and one told while the list is open
starts no clock until it closes. It leaves whether or not it's the one the bar shows, as a dismissal
would. Warnings, errors, the settings-reset notice and a partly failed undo are until dismissed. A
notice's length is its `Lasts`.
_Avoid_: toast, transient notice, auto-dismiss

**Error notice**:
A notice of severity Error: where errors with no better place show: the adds made outside the window
(files dropped on it or opened from Finder, a magnet clicked in a browser), Add file…, the folder
picker and failed commands. In the status bar in the error colour with the count, explained in the
popover a click opens (an error never opens it by itself), and dismissed with ×; a batch of adds stays one notice ("Couldn't add 2 of
5", each failure under it as its view). Errors that have a place stay there: the link field's by
Add, a disabled button's on the button.
