# Open questions and work in flight

Kept by the coordinating session until the tracking tool (#30) replaces it. **Inbound** waits on
the user; each item has a recommendation (➡️), so "yes" accepts it. **Outbound** waits on agents
or is queued.

## Inbound: waiting on you

### S1: Storage for the engine's state
Today: `engine.json` (ids, held priorities, running-at-shutdown, destinations, files at shutdown),
rewritten whole on every change, next to `settings.json` in the data dir. Fine for hundreds of
torrents; thousands mean rewriting a large file often. SQLite instead: atomic per-row updates,
scales, queryable.
➡️ SQLite via `rusqlite` (bundled SQLite, synchronous, the mature standard) rather than `libsql`
(Turso's fork: async, replication features we don't need, bigger and younger). JSON stays for
settings. New dependency to approve.

### S2: Settings that are partly invalid
Where: `~/Library/Application Support/Phantom/settings.json`, read by the service. Serde can
default a *missing* field (`#[serde(default)]`), but one *invalid* field fails the whole file.
➡️ Decode field by field: keep every valid field, reset invalid or unknown ones to defaults,
show a notice naming them, and keep a copy of the bad file (`settings.invalid.json`) instead of
overwriting it. Never lose a valid value.

### S3: Theme settings
Reverses "user tweaks: later" for one case: a theme declares its own settings (e.g. turn off a
theme's animation), shown by a generic settings screen.
➡️ Yes; part of the `Theme` interface when Persona 5 is built. ADR 0004 updated.

### R2: Race-testing tools: which ones
Answered in chat: fake engine + gates + proptest (primary), shuttle (secondary).
➡️ Not madsim (it would need the engine split into a librqbit-free crate and still can't run
librqbit), not loom, not tokio's seed.

### T-tool: the tracking tool (#30)
Grill round pending: what it shows, how answers flow back, where it runs, what the main session
does.

### Icon session
Which picks form designed light/dark pairs and each theme's own icon (handoff given; #14 waits).

## Your questions to me (to answer once the tool exists)

### A1: State machine vocabulary (PR #27)
Context: the engine's lifecycle table (`service/src/engine/lifecycle.rs`) has *commands* (from
clients), *observations* (from librqbit, the watcher, the sweep) and *decisions* (what the table
returns: go to a state, allow, ignore, reject, or `Recheck { paused }` = take the torrent out of
librqbit, re-add it with overwrite, then go to Checking). Your read: commands and observations are
both events (from different sources), decisions are the internal mechanism. Why is Recheck a
decision rather than plain "go to Checking" with the re-add as Checking's entry action? If so,
update the glossary and code to say it that way, and find a nicer way to express the
commands/observations split (clients mustn't be able to send librqbit's events, but conceptually
they're the same thing).

### A2: Why does "no files missing" go to Paused?
Context: PR #27's `NoFilesMissing { complete }` observation moves a Missing-files torrent to
Paused or Done when the sweep finds nothing missing (e.g. after setting missing files Off). Why not
resume if it was downloading before? What happens if the sweep finds nothing missing while a
download is ongoing?

### A3: The double check after a restart
Context: a file deleted while the service was stopped makes librqbit's restore run a full hash
check; Phantom shows Missing files; Resume re-adds with overwrite, which hashes again. Why does a
re-add hash again (to find which existing data is valid instead of starting from zero?). Could
Phantom check files *before* starting librqbit and skip librqbit's own check, or do we only know
files are missing because librqbit's check reported it? (PR #27 says the startup check happens
before librqbit restores torrents.)

### A4: A term for the cheap checks
Context: the watcher and the 5 s sweep only look at names and sizes (no hashing); Check hashes.
The cheap look needs its own glossary term.

### A5: Why a fake engine for race tests
Context: race tests need to control when each wait finishes; librqbit's internal waits can't be
controlled. Your read: with the real engine and a controlled scheduler we could simulate "our A
runs after librqbit's B", but not reliably "before", since we can't make B wait longer. Is that the
reason?

### A6: Rune issue for opening files and links
Context: Rune already handles links (`on_open_url`); for files we filed anomalyco/rune#157 and
built our own handler in the meantime. Confirm that's what was meant.

### S4: Install the `show-me` skill?
Context: humanlayer's `show-me` skill makes agents explain with compact visuals instead of prose;
the PR format now asks for that (docs/agent-workflow.md). Installing it (`npx skills add
humanlayer/skills --skill show-me`) changes your agent setup.
➡️ Your call; the workflow doc already covers PRs without it.

## Outbound: agents and queue

| What | State |
|---|---|
| PR #27 Missing files | Ready for review: error tone, files put back clear Missing files |
| Shuttle debug-build crash | Agent investigating |
| #30 tracking tool | Priority; design first |
| #23 Destinations | Next after #27 merges (shares the file watcher) |
| #24 Moving | After #23 |
| #29 Property/model tests | After #27 |
| #31 Race tests | After #29 |
| #32 Suspected races | Confirm with #31 |
| #33 Layout review | Later |
| #20 Copy magnet, #25 Pause while fetching, #26 Progress lost on stop | Small; when there's room |
| #14 App icon | Waits on the icon session's pairs |
