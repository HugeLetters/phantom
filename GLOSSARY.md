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
The folder a torrent's files go into.
_Avoid_: location, output folder, save path, download dir

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
The whole look of the app: Plain, Persona 5 or Persona 3 Reload.
_Avoid_: skin, style

**Appearance**:
Light, Dark, or following the system's setting. Only the Plain theme follows it.
_Avoid_: mode, dark mode, color scheme

**Palette**:
One of a theme's own color variants, like Persona 3 Reload's Dark Hour green or blue moon.
_Avoid_: variant, color theme

**App icon**:
The icon Phantom shows in the Dock, Finder and launchers. Either it follows the theme, or it's one
icon always, or a light icon and a dark icon that swap with the Appearance; any icon from any
theme can be picked.

**Client**:
Something that shows and controls the service's torrents: the Phantom app or the `phantom`
command.

## Actions

**Add**:
Put a torrent into Phantom's list, from a magnet link, a .torrent file or a .torrent URL.

**Remove**:
Take a torrent off the list, keeping any files it downloaded.
_Avoid_: cancel

**Check**:
Verify a torrent's files on disk against its info, keeping what's valid and downloading the rest.
_Avoid_: recheck, verify, force recheck

**Delete**:
Remove a torrent and also delete the files it downloaded.
