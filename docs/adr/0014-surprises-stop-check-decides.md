# Surprises stop; Check decides

Status: accepted. Amends [ADR 0008](0008-undo-is-inverse-commands-in-the-service.md): its no-check undo, all-or-nothing undo, crash-finishing and trashed-file reading are superseded, as marked there.

## Context

Most of Phantom's file recovery was built before the gate (#374), when stopping wasn't safe: librqbit writes over whatever sits at a path, so a torrent left with an unexpected file there could destroy it. So each surprise got its own path that went on, or restored something, without a Check: undo with no rehash, all-or-nothing undo, undos and Deletes finished after a crash, trashed files read and seeded from the Trash, the Trash watched for emptied files, kept-aside parts of trashed files, Move special cases. ADR 0008 rejected "finish an undo with what's back" for exactly this reason.

That convenience recovery is about a third of the file code (~3,300 implementation and ~6,500 test lines) and caused 24 of the area's ~54 bug issues, most of its flaky tests, and much of the drift between the engine model and the engine (13 "Engine model: …" issues). Each path needs its own crash points, model step, oracle routes and race cases, and every gate PR has to integrate trashed files (`in_trash`, `put_back`, `follow`). Estimates from a reading of the code, ±30%.

The gate makes stopping safe: nothing writes into a file Phantom hasn't verified, so "stop and let Check decide" can't destroy data.

## Decision

**A surprise stops the torrent in a named state; Check or Resume reads the disk and continues.** The states:

- **Missing files**: something it gets is absent.
- **Files don't match**: something there isn't Phantom's own, or doesn't hash.
- **Destination unavailable**: its destination can't be reached.
- **Error**: anything else, such as a Delete that couldn't finish.

Two things still happen without a press, as neither can lose data: a destination that comes back is checked, and an Off file found gone loses its pieces.

**Every safety rule stays:** Phantom writes into no file it hasn't verified, erases nothing (it only moves files to the Trash), and the gate denies by default. Its decisions on #374 stand. So do Undo and Move to Trash as features, the undo stack and its rules (ADR 0008), Move's write-ahead rollback, `places.rs`, the oracle, and `parts.rs` for Off and ungranted files.

**What goes:**

- **Undo without a Check.** Every undo of a Remove, Delete or Move to Trash ends in a Check of the whole torrent: it rehashes instead of seeding at once.
- **All-or-nothing undo and crash-finishing.** An undo refused before anything moves still changes nothing. One that fails or is cut short by a crash after it began leaves what came back, and the Check shows the rest as Missing files. A Delete that can't finish, or is cut short by a crash, leaves its torrent listed in Error; Delete again finishes it. Absorbed entries go.
- **Reading trashed files.** Move to Trash closes the file and forgets its pieces at once (one let-go); nothing is read or seeded from the Trash, so the Trash isn't watched and trashed files have no kept-aside parts.
- **Move special cases.** A Move is refused while files are missing ("Check first"); any surprise mid-move rolls it back, and the torrent shows Missing files.
- **#372's crash record** of which files were missing: the gate's note decides after a crash.
- **#361 and gate PR 7** ("notes may skip checks") are parked, to revisit only if check time becomes a real complaint.

**Undo after re-downloading a trashed file is allowed.** A file the user trashed and then set Normal again is downloaded anew at its path. If the thing at the path is Phantom's own new download (the gate's file note says Phantom made it), ⌘Z moves that new copy to the Trash (never erased), puts the old copy back, restores the priorities and runs a Check. Anything else at the path (the user's own file, or one Phantom can't vouch for) refuses the undo, and the old copy stays in the Trash for Finder's Put Back.

## Consequences

What users see in each removed case:

- ⌘Z on a Delete or Remove: Checking before it seeds (about 1–2 GB/s on an SSD, 100–200 MB/s on a USB disk, so 1–15 min for 100 GB). Nothing downloads again.
- ⌘Z on a Move to Trash: the same Check.
- A trashed file stops seeding at once.
- An undo that fails partway or is cut short by a crash: the torrent comes back Missing files for what didn't return.
- A Delete that fails or is cut short by a crash: the torrent stays listed in Error ("Couldn't move 2 files to the Trash"); Delete again.
- A Move while files are missing: refused, "Check first".
- A file replaced by hand before a Move to Trash or a Move: it goes with the rest (as #374 decided), and once it's back or moved the torrent shows Files don't match.
- A destination that comes back unchanged: a full Check, as on master today (PR 7 would have skipped it).

**Risks.** Data loss: none by design, but only once the gate denies by default (PR 6), so the removals that depend on it come after it. Some code that looks like convenience also enforces a safety rule: `trashed.rs` keeps trashed files read-only, which closing the file at trash time replaces, and the oracle has to prove that. Slow checks are the real cost, for big torrents on slow disks; if users complain, the fix is one generic rule (PR 7: a file whose note matches keeps its pieces) for undo, restart and a returning drive alike.

**What it saves.** Roughly 2,500–3,500 implementation and 5,000–7,000 test lines, the bug class behind 24 issues, most of the flakes (Trash watching, background let-gos and kept parts race the actor and librqbit), engine model and race test runtime, and the review cost of every gate PR. It also avoids #361's ~5,000 lines and PR 7. Allowing the undo over a re-download keeps a small part of today's code (row 26 of the analysis, ~450 lines).

The no-migration rule applies: `undo.json` entries, `parts/` logs and the records' `letting_go` state change shape freely, and older ones are dropped.

## Considered options

- **Freeze:** finish the gate, add no new recovery case, keep the existing ones. Cheapest now, but keeps the code that causes most of the bugs.
- **Only the Trash simplification:** removes the worst-behaved code (#436, #383, #386, #396, #302) in one or two PRs, but keeps the no-check undo and crash-finishing, and their crash points.
- **Refuse an undo over a re-download**, as for any file at the path. Simpler, but the gate's note already tells Phantom's own copy apart, and the new copy goes to the Trash, so allowing it loses nothing.

## Plan

1. This ADR, with ADR 0008 marked.
2. Move to Trash forgets the file at once: close, let go of its pieces, trash; its undo puts back, restores priorities and runs a Check; Phantom's own new copy at the path goes to the Trash first (told as today until gate PR 5's note takes over), anything else refuses. Deletes most of `trashed.rs`, the Trash watching and trashed files' parts, and trims the oracle and model in the same PR.
3. Gate PRs 5 and 6 (notes, default deny, Files don't match), with no trashed files left to integrate; the new-copy check moves onto the note.
4. The old decision points come out, as #374 set the bar, with #372's crash record and the Move special cases (refused while files are missing), closing #404 and the #373 class.
5. Every undo ends in a Check, and nothing is finished after a crash: an undo or Delete cut short leaves what happened, shown as Missing files or Error, and Absorbed goes.
6. #361 and PR 7 stay parked.
