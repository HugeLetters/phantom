# The torrent lifecycle is what's wanted plus a phase, decided inside one actor

librqbit derives a torrent's state from its own internals and misses things Phantom must model (files deleted from disk, an unavailable destination, moving), so the engine keeps each torrent's lifecycle itself, as two things: what the user **wants** (run or stop, set by the add, Pause and Resume, and nothing else) and its **phase** (what it's doing: fetching info, checking, ready, missing files, destination unavailable, error; #24 adds moving). Data that belongs to a phase lives in it (the missing files, the files put back being checked). Its Status follows from the two and whether it's complete, which is read from librqbit, not stored.

Two pure functions:

- `decide(torrent, event)`. An **event** is a **command** (from a client: it may set what's wanted and start a check, or is refused with a typed reason that goes back to the client, which words it) or an **observation** (librqbit, the file system: it changes only the phase, and is never refused). The split is in the types, so an observation can't change what's wanted and a client can't send an observation. Every phase and event is a written decision, one match with no catch-all, so a new one doesn't compile until it's decided.
- `next(torrent, librqbit)` says what makes librqbit match: start it, stop it, or take it out and add it again, which is how a check is entered (librqbit can't check a torrent it has). librqbit only runs a torrent when `next` starts it.

One tokio task (an actor) owns every torrent and handles events one at a time from a queue (client commands with a reply channel, file-system events, librqbit progress), so there are no locks and no races between, say, a pause and a deletion. What's wanted is saved whenever it changes, so a restart, after a clean stop or a crash, makes librqbit match it again.

The engine also lists, per torrent, the commands that do something now (`TorrentInfo.actions`), so clients offer those instead of repeating the rules.

## History

The first version (#22) kept one enum of states (Downloading, Paused, `Checking { paused }`, `DestinationUnavailable { running }`, …) and a table of every (state, event) pair. Run or stop was then stored in four places and lost by states that didn't carry it, and many rows only corrected librqbit; #44 split it into what's wanted and a phase. #99 made `decide` one exhaustive match over every phase and event, with the actor reporting only raw facts (what's on disk, not "files missing" or "files back"), split checking into a check due and one under way, so a check librqbit just ended is never taken for one not started and added again, and named it as every machine is named (#110): the module `machine`, `State`, `Event::{Command, Observed}`, `Do`, and `next` for `reconcile`.

## Considered options

- **The `statig` crate** (hierarchical state machines with superstates and entry/exit actions). A spike showed its async machine can't be used inside `tokio::spawn` (rustc can't prove its futures `Send` because events are passed by higher-ranked reference), and it has no notion of replying, so an unhandled command silently drops its reply.
