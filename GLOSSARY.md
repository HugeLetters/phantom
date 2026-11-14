# Phantom

A BitTorrent client for macOS. These are the words the app uses for what it shows and does;
code, docs and issues use the same words.

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
files already on disk), **Downloading**, **Seeding**, **Paused** (incomplete), **Done**
(complete and paused), **Missing files** (files it had downloaded are gone from disk; resuming
downloads them again), **Destination unavailable** (its destination can't be reached, e.g. the
drive isn't connected), **Moving** (its files are being moved to a new destination) or **Error**.
_Avoid_: state, resolving, initializing, live

**Size**:
The total size of the files you're getting from a torrent, leaving out files whose priority is
Off.
_Avoid_: wanted size, selected size

**Full size**:
The total size of all of a torrent's files, whatever their priority.

**Destination**:
The folder a torrent's files go into. A torrent of several files gets its own folder there,
named after it; a torrent of one file goes straight in.
_Avoid_: location, output folder, save path, download dir

**Default destination**:
The destination of every add, set in the settings. Without one, every add asks for a destination
first.

## Files

**File**:
One of the files inside a torrent.

**Priority**:
Whether and how eagerly a file is downloaded: **Normal** or **Off**. A file set to Off isn't
downloaded.
_Avoid_: wanted, skipped, selected, included

## The app

**Service**:
Phantom's background process that runs the torrents. It keeps going when the window closes and
stops shortly after the last client quits.
_Avoid_: daemon, engine, backend

**Theme**:
The whole look of the app: Phantom (the default), Persona 5 or Persona 3 Reload.
_Avoid_: skin, style

**Appearance**:
Light, Dark, or following the system's setting. Only the Phantom theme follows it.
_Avoid_: mode, dark mode, color scheme

**Palette**:
One of a theme's own color variants, like Persona 3 Reload's Dark Hour green or blue moon.
_Avoid_: variant, color theme

**App icon**:
The icon Phantom shows in the Dock, Finder and launchers. Either it follows the theme, or it's one
icon always, or a light icon and a dark icon that swap with the Appearance; any icon from any
theme can be picked. Following the theme is the default.

**Designed pair**:
A dark icon and a light icon made to go together, offered as one App icon choice; any two icons
can also be combined into a custom pair.
_Avoid_: icon set, icon variant

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

**Error list**:
Where errors with no better place show: the adds made outside the window (files dropped on it or
opened from Finder, a magnet clicked in a browser), Add file…, the folder picker and failed commands.
A panel under the toolbar, in the middle, while it has entries; each entry is a notice, and a batch
of adds stays one entry ("Couldn't add 2 of 5", each failure under it). A click removes an entry;
Escape or Clear all removes everything. Errors that have a place stay there: the link field's by
Add, a disabled button's on the button. An actor in the app: anything sends it an event.

**Remove**:
Take a torrent off the list, keeping any files it downloaded.
_Avoid_: cancel

**Check**:
Verify a torrent's files on disk against its info, keeping what's valid. A torrent that was
running downloads whatever is missing; a stopped one stays stopped.
_Avoid_: recheck, verify, force recheck

**Size check**:
Phantom's cheap, automatic look at a torrent's files: does each file it's getting still exist,
with a plausible size? Reads no data; the file watcher and a regular sweep run it, and it's how
Missing files is noticed. A Check (a hash check, reading every piece) is the expensive one.
_Avoid_: look, scan, quick check

**Delete**:
Remove a torrent and move the files it downloaded to the Trash, so they can still be put back.
Nothing else in its destination moves, and nothing is ever erased. The files go first; the torrent
leaves the list only once they all went. On a drive with no Trash nothing moves and Delete is
refused; if only some could go, the torrent stays, stopped, with an error naming what's left, and
Delete can be tried again.

## How a torrent runs

Words for the service's own model of a torrent ([ADR 0006](docs/adr/0006-torrent-lifecycle-as-an-actor.md)).

**Wants**:
Whether the user wants a torrent running or stopped. Set when it's added and by Pause and
Resume, and by nothing else: a torrent wanted running runs whenever it can, e.g. once its files
or its destination are back.

**Phase**:
What a torrent is doing, whatever is wanted of it: fetching info, checking (due, or under way in librqbit), ready (running or
stopped as wanted), missing files, destination unavailable or failed. Its
Status follows from its phase, what it wants, and whether it's complete.
_Avoid_: state

**Event**:
Something that happens to a torrent: a command or an observation.

**Command**:
What a client asks of a torrent: Pause, Resume, Check, a priority, Remove or Delete. Refused,
with a reason, when it can't apply. A torrent's **actions** are the commands that do something
to it now.

**Observation**:
What the service sees of a torrent, as a fact: what librqbit is doing with it, or what a size
check finds (the files it's getting and what's on disk at each, or its destination out of reach).
What it means (files missing, set Off or put back) is the torrent's rules' to decide. It changes
only the phase and is never refused.
