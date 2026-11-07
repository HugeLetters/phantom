# The torrent lifecycle is a plain-enum state machine inside one actor

librqbit derives a torrent's state from its own internals and misses things Phantom must model (files deleted from disk, an unavailable destination, moving), so the engine keeps each torrent's Status as an explicit state machine: a synchronous `decide(state, event) -> (next state, reply)` table plus async entry actions that drive librqbit. One tokio task (an actor) owns every torrent's machine and handles events one at a time from a queue (client commands with a reply channel, file-system events, librqbit progress), so there are no locks and no races between, say, a pause and a deletion. Every (state, event) pair is decided explicitly: transition, ignore (a harmless no-op such as Pause while Paused) or reject with a reason that goes back to the client.

## Considered options

- **The `statig` crate** (hierarchical state machines with superstates and entry/exit actions). A spike showed its async machine can't be used inside `tokio::spawn` (rustc can't prove its futures `Send` because events are passed by higher-ranked reference), and it has no notion of replying, so an unhandled command silently drops its reply. The one advantage, a superstate for Downloading and Seeding, is a one-line `is_active()` guard in the plain table.
