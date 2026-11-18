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
(after a restart it is Missing files, and librqbit's own check goes on).
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
downloaded.
_Avoid_: wanted, skipped, selected, included

**Move to Trash**:
Set the chosen files of a torrent to Off, then move those on disk to the Trash (⌘⌫ with the file
list focused, or in the file menu). Off comes first, so the torrent never misses a file it gets;
nothing else moves, and nothing is ever erased. Files that couldn't move are still where they were,
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
Take back the last Remove, Delete or Move to Trash (⌘Z, Edit ▸ Undo, `phantom undo`, or the Undo in
the notice of that action while it's the last), whether it's done or still running: Undo never
waits. One still running stops after the file in flight, and what it moved goes back; one that
hasn't started is dropped. Either way the list shows it undone at once: a torrent is back in its
old place and status, and starts once its files are back. Of a Move to Trash: the files come back from the
Trash to where they were (those still there: one emptied from the Trash stays Off) and each file
asked about has the priority it had. Of a Remove or Delete: the torrent comes back as
it was, with its id, its destination, its priorities and what it had downloaded, and a Delete's
files come back from the Trash to where they were; of a selection's, every torrent in it, each on
its own. A partial Delete undoes what it moved. The last
20 can be undone, last first; nothing else is undone, and there's no redo. Refused, with why, when
it can't be done now (the Trash was emptied, something is where a file goes back, the drive isn't
connected, the torrent was added again); a refused undo is gone, the next one is under it.
_Avoid_: revert, restore, put back (for the action)

**Settings change**:
A change of one setting that took effect: Return in a field, a switch, a folder, an App icon or a
theme setting, from the app or from `phantom settings`. In Settings, ⌘Z (and Edit ▸ Undo, which
names it: "Undo Port Change") takes back the newest, whoever made it: its setting goes back to what
it was, its tab opens with its control focused, and the status bar says what it went back to
("Port back to 6881"). `phantom config undo` takes back the newest of a service setting. While a
field is being edited, ⌘Z is the field's own. The last 20, in memory only (the app's go when it
quits, the service's when it stops), no redo; a refused one is gone. A default folder that can't be
reached any more goes back all the same. Not an **Undo**: ⌘Z in the main window never takes one.

**Command**:
What a client asks of a torrent: Pause, Resume, Check, Cancel check, a priority, Move to Trash (of files), Remove or Delete. Refused,
with a reason, when it can't apply. A torrent's **actions** are its commands (and Open and Reveal
in Finder), each with its **answer**: it does something now, it does nothing (and why: already
paused, say), or it's refused (and why). Clients offer those that do something, and a disabled
button clicked says why it's off.
