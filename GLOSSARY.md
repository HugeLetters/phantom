# Phantom

A BitTorrent client for macOS. These are the words the app uses for what it shows and does;
code, docs and issues use the same words.

This file holds the words more than one domain uses. A word only one domain uses is in that
domain's glossary, next to its code; read this one and the glossary of the domain you touch.

## Domains

- [GUI](gui/GLOSSARY.md): how the app looks (themes, Look Presets, the App icon) and its Notices
- [Service](service/GLOSSARY.md): how the service runs a torrent: Wants, Phase, Step, Observation

`client/`, `cli/`, `lifecycle/` and `proto/` have no words of their own: `client/` and `cli/` use the
words below, and `lifecycle/` and `proto/` carry the service's.

## Torrents

**Torrent**:
One thing in Phantom's list: a set of files shared over BitTorrent, which Phantom downloads and
then seeds. It's a torrent from the moment it's added, even before its info arrives.
"Downloading" is fine for the process.
_Avoid_: download (as a noun for the item), transfer, item, pending add

**Info**:
A torrent's description of its files: their names, sizes and how to verify them. A magnet link
doesn't contain it, so Phantom first fetches it from peers.
_Avoid_: metadata, metainfo

**Peer**:
Another BitTorrent client sharing the same torrent; Phantom downloads pieces from peers and
uploads to them.
_Avoid_: node, seed (as a noun for any peer)

**Tracker**:
A server that tells Phantom which peers have a torrent. Optional: without one, peers are found
through DHT, just more slowly. A magnet link can name trackers (`tr=`).

**Status**:
What a torrent is doing right now; exactly one of **Fetching info**, **Checking** (verifying
files already on disk), **Unchecked** (a check was cancelled), **Downloading**, **Seeding**, **Paused** (incomplete), **Done**
(complete and paused, or stopped at a seeding limit), **Missing files** (files it had downloaded are gone from disk; resuming
downloads them again), **Destination unavailable** (its destination can't be reached, e.g. the
drive isn't connected), **Moving** (its files are being moved to a new destination) or **Error**.
A torrent whose Delete is under way isn't listed: it has no status.
_Avoid_: state, resolving, initializing, live

**Unchecked**:
A torrent whose check was cancelled; its status is **Unchecked**. Stopped, with nothing known
of what's on disk (so no progress shown), and never Ready on its own: only Check, Resume, or its
destination coming back after it was unavailable, checks it again. Pause is remembered (it stays
Unchecked), a priority is held until the next check, and it stays so across a restart, and
through a Delete that moved only some of its files (no progress shown there either). Where
files were being put back, it says Missing files instead, with those files gone, until it's checked
(after a restart too, with nothing known of what it has).
_Avoid_: cancelled, unverified, stopped

**Size**:
The total size of the files you're getting from a torrent, leaving out files whose priority is
Off.
_Avoid_: wanted size, selected size

**Full size**:
The total size of all of a torrent's files, whatever their priority.

**Needs**:
What the files a torrent is getting still have to take on its disk: their length, less what's
already allocated to them (a preallocated file needs nothing more). Only while it's wanted running
and its destination can be reached; a check never changes it. A disk's torrents' needs together
are compared with what's free on it: more, and the disk is short, and every torrent that still
needs space on it is warned of in its row (a ⚠, with the disk's whole picture on hover; a torrent
that alone needs more than is free says that of itself instead). A warning, never a block; nothing
is short while what's free is unknown.
_Avoid_: required space, remaining (as a noun)

**Destination**:
The folder a torrent's files go into. A torrent of several files gets its own folder there,
named after it; a torrent of one file goes straight in.
_Avoid_: location, output folder, save path, download dir

**Default destination**:
The destination of every add, set in the settings. Without one, every add asks for a destination
first.

**Seeding limit**:
When a finished torrent stops seeding by itself: at a share ratio (what it uploaded ÷ its Size)
or after a time seeding, whichever comes first; set for all torrents. It's then Done, saying
which. Each counts from the add, and again from a Resume after a limit, so it seeds another
round; a Resume after a Pause goes on with the round.
_Avoid_: share limit, seed goal

## Files

**File**:
One of the files inside a torrent.

**Priority**:
Whether and how eagerly a file is downloaded: **Normal** or **Off**. A file set to Off isn't
downloaded. One set Off whose bytes aren't at its path (deleted or moved away by hand, or in the
Trash) shows none of it, and nothing of it is offered to peers; set Normal again, it's downloaded
from nothing, not Missing files, which is for a file it gets that vanished.
_Avoid_: wanted, skipped, selected, included

**Move to Trash**:
Set the chosen files of a torrent that are on disk to Off, then move them to the Trash (⌘⌫ with
the file list focused, or in the file menu, which offers it while a chosen file is on disk). Off
comes first, so the torrent never misses a file it gets; a chosen file that isn't on disk is left as
it is, nothing else moves, and nothing is ever erased. Files that couldn't move are still where they were,
Off all the same, and said so. It can be undone, as a Remove or a Delete is: one step of Undo per
Move to Trash, which puts back what's still in the Trash and each file's priority as it was.
_Avoid_: delete (a file), discard

## The app

**Service**:
Phantom's background process that runs the torrents. It keeps going when the window closes and
stops shortly after the last client quits.
_Avoid_: daemon, engine, backend

**Client**:
Something that shows and controls the service's torrents: the Phantom app or the `phantom`
command.

## Actions

**Add**:
Put a torrent into Phantom's list, from a magnet link, a .torrent file or a .torrent URL. Adding
is always "add", whatever the source: a link, a file or a drop.

**Add file…**:
Pick a .torrent file and add it (⇧⌘A).
_Avoid_: Open… (for adding)

**Open**:
Open a torrent's content: its file in its default app, or its folder in Finder for a torrent of
several files (⌘O, Return, a double-click).
_Avoid_: open (for adding a .torrent file)

**Reveal in Finder**:
Show a torrent's content in Finder, selected in its folder (⇧⌘O).
_Avoid_: show, locate

**Remove**:
Take a torrent off the list, keeping any files it downloaded.
_Avoid_: cancel

**Check**:
Verify a torrent's files on disk against its info, keeping what's valid. A torrent that was
running downloads whatever is missing; a stopped one stays stopped.
_Avoid_: recheck, verify, force recheck

**Cancel check**:
Stop a check that's running (in the row menu, only while the torrent is Checking). Its work is
lost: the torrent is **Unchecked** until it's checked again.
_Avoid_: stop, abort, pause (Pause only changes what's wanted)

**Delete**:
Remove a torrent and move the files it downloaded to the Trash, so they can still be put back.
Nothing else in its destination moves, and nothing is ever erased. The torrent leaves the list at
once, and its files go to the Trash meanwhile. On a drive with no Trash nothing moves, the Delete
is refused, and the torrent comes back; if only some could go, it comes back stopped, with an error
naming what's left, and Delete can be tried again, or the torrent removed; nothing else, as
resuming or checking it would make the trashed files again. It
stays so after a restart, and while its drive is away. A Delete under way when Phantom stops is
finished first; one cut short by a crash comes back as what's left of it.

**Undo**:
Take back the newest change: a Remove, Delete, Move to Trash or Rename, or a **Settings change**, whoever
made it, in the order the app learned of them, its own as it makes them (⌘Z and Edit ▸ Undo, in the main window or in
Settings; Edit ▸ Undo names it: "Undo Remove of Album", "Undo Port Change"). `phantom undo` takes
the last Remove, Delete, Move to Trash or Rename, `phantom settings undo` the newest change of a service
setting; the Undo in the notice of an action takes that action while it's the last of them (⌘Z
shows beside it while ⌘Z would take it too). A
setting's undo opens Settings at it (see **Settings change**); an action's undo brings it back in
the list, whichever window is in front. Of a Remove, Delete or Move to Trash, whether it's done or
still running: Undo never waits. One still running stops after the file in flight, and what it moved goes back; one that
hasn't started is dropped. Either way the list shows it undone at once: a torrent is back in its
old place and status, and starts once its files are back. Of a Move to Trash: the files come back from the
Trash to where they were (those still there: one emptied from the Trash stays Off) and each file
asked about has the priority it had; one set Normal since and downloaded again has that new copy
sent to the Trash first, and the torrent is checked. Of a Remove or Delete: the torrent comes back as
it was, with its id, its destination, its priorities and what it had downloaded, and a Delete's
files come back from the Trash to where they were; of a selection's, every torrent in it, each on
its own. A partial Delete undoes what it moved. Of a Rename: it's renamed back, as a Rename is (never
over something at its old name, nor while it moves to another folder). The last
20 Removes, Deletes, Moves to Trash and Renames can be undone, last first; Pause, Resume, a priority, Add
and Check aren't undone, and there's no redo. Refused, with why, when
it can't be done now (the Trash was emptied, something is where a file goes back, the drive isn't
connected, the torrent was added again); a refused undo is gone, the next one is under it.
_Avoid_: revert, restore, put back (for the action)

**Settings change**:
A change of one setting that took effect: Return in a field, a switch, a folder, an App icon, a
theme setting (of the Active look, or of a Saved Look Preset in its sheet: "Accent colour (Dark) of
Evening undone", its undo opening the sheet again), or an Apply, a save, a replace or a delete of a
Saved Look Preset (Edit ▸ Undo "Undo Apply Evening", and the status bar "Apply Evening undone"),
from the app or from `phantom settings`. ⌘Z (and Edit ▸ Undo, which names it: "Undo
Port Change") takes back the newest, whoever made it, when it's the newest change of all (see
**Undo**), in Settings or in the main window: its setting goes back to what it was, Settings opens
(if it was closed) in front, at its tab with its control focused, and the status bar says what it went back to
("Port is back to 6881"; "Light Tone is back to Dark" for a theme setting split by Appearance;
"Port is back to automatic (now 51413)" for a port the service picks). `phantom settings undo`
takes back the newest of a service setting. While a field is being edited, ⌘Z is the field's
own. The last 20, in memory only (the app's go when it quits, the service's when it stops), no
redo; a refused one opens nothing, the status bar says why, and it's gone. The app's 20 are its own and the service's changes together: a
service change pushed out of the app's 20 by app changes is still the service's, which `phantom
settings undo` takes. A default folder that can't be reached any more goes back all the same.

**Move to…**:
Change the destination of the selected torrents (in the row menu, or `phantom move`), downloading or after: its
files go into the new folder, renamed on the same drive or copied to another, then the originals
go; it keeps what it had, without a full check, its id and its row. It's **Moving** meanwhile,
with how much is copied. Where files of the torrent's are already, it asks first: **Use existing**
(they're kept and checked; the torrent's own copies of them go to the Trash), **Replace** (those go
to the Trash and the torrent's take their place; other files there stay), **Cancel**, or **Reveal
in Finder**. A folder is the folder on disk, however it's spelled: into the one it's in already,
nothing moves, and it says so. One that fails (the disk is full, a drive goes, something put where
a file goes while it copied) is back where it was, always, with nothing left of what was copied,
and says why; a file of it deleted by hand meanwhile stays deleted (what was copied of it goes to
the Trash), and the torrent is Missing files. A Remove or Delete meanwhile cancels it first; Pause
and Resume, and a priority, apply once it's moved. With files of the torrent in the Trash for an
undo it moves all the same: those Moves to Trash can't be undone any more (their undo would put
the files back in the old folder), and the files stay in the Trash. Quitting puts back a move
under way; a restart for new settings waits for it. It isn't undone.

Of several torrents it's one folder for all, each torrent in it as it would be on its own. It's
offered when at least one selected torrent can move; those that can't are skipped, and the notice
says how many moved and lists each that didn't, with why. The moves run **one after another**, in the
list's order, and a torrent waiting its turn is **Waiting to move**: its Move is refused, **Cancel
move** takes it out of the queue (a Remove or a Delete does too); meanwhile it runs as before, and
what's asked of it (Pause, a priority) is done at once. Quitting puts back only the move under way:
those not started don't happen; a restart for new settings waits only for the move under way, and
the others still wait. A clash is asked about for each torrent that has one, before anything moves,
one dialog after another in the list's order: Use existing, Replace, Rename… or Cancel, which skips
only that torrent, with **Apply to the remaining N** giving the answer to the rest. Rename… moves the
torrent in under a name typed in the dialog (a Rename's rules: anything at that name when its turn
comes refuses it); the field opens with a free name (`name (2)`, `(3)`…, before the extension for a
file), asked of the service as it opens (`Request::FreeName`); a name is one torrent's, so the box
is hidden meanwhile. A clash with another listed torrent's files offers only Rename… and Cancel,
and names that torrent, which can be an earlier torrent of the same selection with the same paths; Replace or Use
existing given to the rest passes over those, which are still asked about. An answer holds for the
files the dialog showed, as they were: a torrent whose new folder changed by its turn (a file put
there, an earlier move's files at its paths) isn't moved, and the notice lists it: "Its new folder
changed while it waited", with the path. A file another torrent in the list has (where it is, or
where its Move takes it) is never replaced nor used by another's Move, whatever the answer, even
one taken over unchanged: that torrent isn't moved, and the notice says "Another torrent has its
files there", with the path; only a new name moves it there. What Use existing keeps must still be as the dialog showed once the
rest is copied, or the Move goes back.
_Avoid_: relocate, change folder, set location

**Rename…**:
Give a torrent a new name on disk where it is (in the row menu, for one torrent; `phantom rename`):
a torrent of one file its file's, a torrent of several its folder's (the files in it keep theirs).
The Name cell becomes a field with its name, the part before the extension selected; Return
renames it, Escape or a click away doesn't. A name a file can't have (empty, with `/` or `:`, `.`
or `..`, too long) is said under the field. Its files are renamed as a Move on the same drive does,
and it keeps what it had, without a check. Something at the new name (a file, a folder, a link) or
another torrent in the list keeping its files there refuses it: nothing is written over or merged.
The list, search and sort go by the new name; its details show its info's own as **Original
name**. One step of Undo. Cut short by quitting or a crash, it's finished or put back at the next
start.
_Avoid_: move (for a rename), retitle

**Command**:
What a client asks of a torrent: Pause, Resume, Check, Cancel check, a priority, Move to Trash (of files), Move to…, Cancel move, Rename…, Remove or Delete. Refused,
with a reason, when it can't apply. A torrent's **actions** are its commands (and Open and Reveal
in Finder), each with its **answer**: it does something now, it does nothing (and why: already
paused, say), or it's refused (and why). Clients offer those that do something, and a disabled
button clicked says why it's off.
