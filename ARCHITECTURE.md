# Phantom architecture

A map of the code. Words follow [GLOSSARY.md](GLOSSARY.md); the reasons behind the big choices
are in [docs/adr/](docs/adr/). Each module's interface is documented at the top of its own
source file.

```
        gui ──┐       ┌── cli          clients
              client                   reaching the service, typed calls
                 │
             lifecycle                 the torrent's lifecycle and what a machine is, shared
                 │
               proto                   the messages, shared by both sides
                 │
              service                  the background process
                 └── engine            the torrents (the only code that knows librqbit)
```

At runtime, requests flow from a client through `client` and the socket to `service` and its
`engine`, and answers flow back. At compile time `proto` and `lifecycle` are the only crates both
sides share; clients never depend on `service`, and nothing but `engine` uses librqbit. The GUI's
fake service runs the engine's own lifecycle, so the GUI's tests meet the engine's rules.

## Crates and modules

| Where | Job |
|---|---|
| `proto/` | The messages and the data they carry, and where the service's socket is (the data dir); `tolerant`, the one decoder of a settings file that resets what's invalid. No I/O but the files named: the full list of what a client can ask. |
| `lifecycle/` | A torrent's lifecycle, the engine's and the GUI's fake's: what's wanted, its phase, `decide`, `next` and `answer` (what each action would do now, #89) ([ADR 0006](docs/adr/0006-torrent-lifecycle-as-an-actor.md)); `seeding`: seeding limits, a round's count of what it uploaded and how long it seeded, and when it reached a limit; what a size check finds as plain data (`disk`). `machine`: the `Machine` trait every machine implements, `Every` and `every!` (samples of every variant, which a new one can't skip), and, for tests (feature `harness`), the table and sequence harness. |
| `client/` | Reaching the service: the connection machine (`client::machine`: connect, replace a service from another build or one that's stopping, start one, lose it), which the CLI and the app's link thread share; one method per request; `display`, the one module for text people see (sizes, durations, the words for the service's typed reasons), shared by clients. |
| `service/` | The background process: one per data dir, the socket, its lifetime as a machine (`service::machine`: idle shutdown, restarting the engine when the listening port or UPnP changes, during which requests that need the engine wait and the rest are answered, refusing every request while it stops), settings (`settings.json`: the default destination, the listening port as its own machine, `settings::machine`, UPnP, keeping the Mac awake, the speed limits, which the engine sets on the running session: nothing restarts); a .torrent URL is fetched before its add reaches the engine. `awake`: an actor that holds macOS's power assertion exactly while the setting is on and the engine's latest snapshot has a torrent Downloading, Fetching info or Checking, released when the service stops (`awake::machine`). |
| `service/src/engine/` | Torrents: add (each into its destination), list, details, priorities, pause, check, remove. One actor task owns every torrent's lifecycle, what's wanted plus a phase, decided by `machine/` from the facts the actor reports, and its librqbit side (adding, never added, in librqbit or taken out: `machine/inlib.rs`), and lists what can be done to each ([ADR 0006](docs/adr/0006-torrent-lifecycle-as-an-actor.md)); after every event it publishes a snapshot (the list and the torrents' details) when it changed, numbered within an epoch (each start of the engine is a later one), which the service pushes to each subscribed connection (`Subscribe { watching }`, at most every 100 ms, the latest only, on across an engine restart) with the details it watches, the list it was built for and the settings reset at start; clients keep the latest by `(epoch, version)`; owns torrent ids and de-duplication; watches destinations for files deleted outside Phantom and for destinations that can't be reached (never creating a missing one); has each destination's disk looked at off the actor (`statfs`, and what's allocated to each file, in a thread per look with a timeout, readings back as messages; a stalled disk's free space is unknown) and puts in each snapshot the disks torrents still need space on, with what each needs and what's free (`space.rs`: a warning, never a block); keeps `engine.json` in the data dir. |
| `cli/` | Parses arguments and prints. |
| `gui/src/service/` | The GUI's seam to the service: a trait with a socket implementation (one link thread owns the connection and subscribes on it, nothing polls; commands are messages to it) and an in-memory fake for tests and headless runs: an actor without a thread, whose mailbox the app clock drains, running each torrent's shared lifecycle against a pretend librqbit and disk. |
| `gui/src/pane/` | The details pane as a machine (Empty, Waiting, Showing, Switching): it draws only from one snapshot, and keeps the shown torrent, live, until the selected one's details come. |
| `gui/src/model.rs` | App state and what actions do to it. No drawing. `model/space.rs`: each disk short of space as a live notice for the status bar. |
| `gui/src/errors/` | The error list: an actor that owns the errors with no better place; anyone with the app state sends it events (`report(notice)`), its `machine` decides them, a mailbox is drained once per frame. |
| `gui/src/notice.rs` | `Notice`, a short line and optionally a view, with the `explained` preset. |
| `gui/src/bar/` | The status bar's notices: told once (an add, the settings reset) or live while a condition holds (a disk short of space); its `machine` decides them, what a click on the bar does, and whether their popover is open (Escape goes to it first). The bar shows the newest with which of how many it is (1/3); a click opens a popover above it showing one at a time with its view, and ‹ › or ← → step through them, newest first, stopping at the ends; what's shown follows the notice, so one told or ceasing elsewhere never moves it, and when it goes the next older one is shown. The popover is modal (focus in it while open, back after) and scrolls when the window is short. |
| `gui/src/overlay.rs` | One overlay at a time: the error list, the notices popover and the menus. Opening one closes the others; the error list, which can't close without losing its errors, hides under the others until they close. |
| `gui/src/views/` | Screens: what's shown and which actions exist, drawn through `ui`. |
| `gui/src/ui/` | Building blocks every screen draws with; themes override them ([ADR 0004](docs/adr/0004-themes-override-building-blocks.md)). |
| `gui/src/platform/` | macOS integration: magnet links, files opened from Finder, Reveal in Finder, the App icon (choice and manifest resolved purely; AppKit sets the Dock and bundle icons). |
| `design/` | Icons and theme references. |

## Principles

- **A right model makes edge cases disappear.** Many special cases mean the model is wrong or too
  loose. Fix the model (its states, who owns what, the types) so the case can't happen or follows
  from the general rule; don't add another branch.
- **Odd scenarios test the model; they aren't features.** "Delete while moving" or "the drive
  vanishes mid-check" ask whether the model already covers them. The answer should be "yes, by
  the general rule", not new code.
- **Less logic, not fewer lines.** A better model often adds lines (a struct, a service, an
  interface) and removes branches; that's the trade we want. Dense one-liners aren't simpler.
  Leave out what nothing needs yet.
- **One thought-out shape over many cases.** Code that keeps asking what kind of thing it holds
  (`if list.is_empty()`, `if first.kind == A`, `if any(kind in [A, B])`) usually means the model
  is missing a concept. Prefer one shape whose behaviour follows from it, but not a struct full of
  optional fields, which is the same `A | B | C` hidden. A real sum type (a `Result`, a phase) is
  fine when the cases genuinely differ.
- **Parse, don't validate** ([lexi-lambda](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)):
  check input once at the edge and turn it into a type that can only hold valid values (a
  `Destination`, not a path checked in three places); code past the edge takes that type.
- **The service states typed reasons; clients word them.** Refusals and errors cross the protocol
  as data (an enum with its facts), never as sentences. Wording lives in `client`, shared by the
  CLI and the GUI, like an HTTP status the server sends without writing the page.
- **State with rules is a state machine; long-lived concurrent state is an actor.** A flag or an
  `Option` that decides what may happen next is a machine's state in hiding: write the state ×
  event table out, every pair decided (#110). An actor owns the state no one else touches.
  Reach for this pair first, in the GUI too: a part others report to (the error list) is an actor
  they send events to, so how it's shown can change while delivery stays the same.
  Every machine uses the same names, in a `machine` module: `State`, `Event` (`Command(..)` or
  `Observed(..)`, raw facts), `Do` (what the actor carries out), `decide(&State, &Event) ->
  Result<State, Refusal>` (one match, no `_` arm) and `next(&State, ..) -> Option<Do>`, and
  implements `lifecycle::machine::Machine`; the torrent's lifecycle (`lifecycle/`) is the
  reference.
- **Everything on screen is ours to theme.** No native-looking controls (menus, popups, alerts):
  a theme restyles all of it ([ADR 0004](docs/adr/0004-themes-override-building-blocks.md)).
  Native stays only where macOS draws it outside the window: the menu bar, the Dock, file pickers.
- These are heuristics and taste, not rules.

## Rules

- Each module gets the narrowest input it needs: a helper takes the table state, not the app.
- Tests go through interfaces: GUI behaviour through `model` and the views against the fake
  service, the engine against a real session in a temp dir, the protocol end to end through
  `client`.
- Tests assert properties rather than hand-written outcomes: a machine's table over every
  state × event pair, each in exactly one row with its property, and generated event sequences
  checked against the same rows (`lifecycle::machine`'s harness, from `every!` samples), the engine and the GUI over generated sequences against a reference model
  (`engine/model_tests.rs`, `gui/src/tests/generated.rs`; `PROPTEST_CASES` runs more), and the
  engine's add tasks, held at gates, against generated commands and restarts, downloading from a
  local seeder that has half of each torrent, so progress never goes down across a restart
  (`engine/race_tests.rs`). A new behaviour or a bug starts there, as a property or a step of the
  generated sequences; an example test is the exception, for what truly can't be generated, and
  the PR says why. A behaviour that's hard to state as an always-true property, or a bug the
  generated tests can't reach, usually means the model, or the fake, is wrong or missing something
  (e.g. answers that are never late).
- The service reports its build id (`proto::exe_id`: a hash of its own executable); a client replaces a service whose id isn't that of the `phantom-service` it would start. Nothing is bumped by hand.
- Hard parts go to dedicated, well-maintained crates rather than our own code; agree on each new
  dependency with the user before adding it.
