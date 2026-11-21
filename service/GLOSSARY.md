# Service

The words of the service's own model of a torrent ([ADR 0006](../docs/adr/0006-torrent-lifecycle-as-an-actor.md)),
which the `lifecycle` crate holds as pure rules and the engine runs; the GUI's in-memory fake runs
the same. Global words are in the [root glossary](../GLOSSARY.md).

## How a torrent runs

**Record**:
What Phantom keeps of a torrent across restarts: its .torrent (once its info arrived), its
destination, which files it gets, what's wanted and what's under way on it. It's the torrent: the
service adds it to librqbit from its record, at every start too, and librqbit only runs it and
keeps which pieces it has.
_Avoid_: librqbit's session (for what Phantom keeps), saved state

**Wants**:
Whether the user wants a torrent running or stopped. Set when it's added and by Pause and
Resume, and by a seeding limit reached while it seeds, and by nothing else: a torrent wanted
running runs whenever it can, e.g. once its files or its destination are back.

**Phase**:
What a torrent is doing, whatever is wanted of it: fetching info, checking (due, or under way in librqbit), unchecked (its check
was cancelled), ready (running or stopped as wanted), missing files, destination unavailable or failed. Its
Status follows from its phase, what it wants, and whether it's complete.
_Avoid_: state

**Event**:
Something that happens to a torrent: a command or an observation.

**Step**:
Something done to torrents, in the order it was asked: a Remove, a Delete or a Move to Trash, the
undo of one, or any other command. One press is one step: a Remove or Delete of a selection is one,
on every torrent in it, and one Undo brings all of it back. Each torrent's steps run one at a time,
in that order; different torrents' run side by side. Remove, Delete and Move to Trash are on
Undo's stack from the moment they're asked, in that order too. librqbit reading the pieces it saved
of a torrent added back (at a start, by an undo) counts as running on its lane: a step meanwhile
shows done at once and runs right after. A Move is a step that runs until its files are moved, or
back where they were; what comes meanwhile waits behind it, but a Remove or a Delete cancels it first.
Moves of any torrents run one at a time, in the order they came: a Move not started waits for
every older one, whichever torrent's, and shows Waiting to move. It joins its torrent's lane only at
its turn, so it holds none meanwhile: the torrent's other commands run as they come. Cancel move, a
Remove or a Delete takes it out of the queue; a stop drops it with the rest that wait, and a restart
for new settings hands it to the engine that starts next.
A Rename is a Move within its destination under a new name, on Undo's stack as a Move to Trash is
(#331).
_Avoid_: job, task, transaction (for the thing itself)

**Observation**:
What the service sees of a torrent, as a fact: what librqbit is doing with it, or what a size
check finds (the files it's getting and what's on disk at each, or its destination out of reach).
What it means (files missing, set Off or put back) is the torrent's rules' to decide. It changes
only the phase and is never refused.

**Size check**:
Phantom's cheap, automatic look at a torrent's files: does each file it's getting still exist,
with a plausible size? Reads no data; the file watcher and a regular sweep run it, and it's how
Missing files is noticed. A Check (a hash check, reading every piece) is the expensive one.
_Avoid_: look, scan, quick check
