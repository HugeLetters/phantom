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

## Outbound: agents and queue

| What | State |
|---|---|
| PR #27 Missing files | Agent applying review: error tone for Missing files, auto-clear when files return |
| #30 tracking tool | Priority; design first |
| #23 Destinations | Next after #27 merges (shares the file watcher) |
| #24 Moving | After #23 |
| #29 Property/model tests | After #27 |
| #31 Race tests | After #29 |
| #32 Suspected races | Confirm with #31 |
| #33 Layout review | Later |
| #20 Copy magnet, #25 Pause while fetching, #26 Progress lost on stop | Small; when there's room |
| #14 App icon | Waits on the icon session's pairs |
