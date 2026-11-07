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
downloads them again) or **Error**.
_Avoid_: state, resolving, initializing, live

**Size**:
The total size of the files you're getting from a torrent, leaving out files whose priority is
Off.
_Avoid_: wanted size, selected size

**Full size**:
The total size of all of a torrent's files, whatever their priority.

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

**Client**:
Something that shows and controls the service's torrents: the Phantom app or the `phantom`
command.

## Actions

**Add**:
Put a torrent into Phantom's list, from a magnet link, a .torrent file or a .torrent URL.

**Remove**:
Take a torrent off the list, keeping any files it downloaded.
_Avoid_: cancel

**Delete**:
Remove a torrent and also delete the files it downloaded.
