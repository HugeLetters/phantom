# Phantom architecture

A map of the code. Words follow [GLOSSARY.md](GLOSSARY.md); the reasons behind the big choices
are in [docs/adr/](docs/adr/). Each module's interface is documented at the top of its own
source file.

```
        gui ──┐       ┌── cli          clients
              client                   reaching the service, typed calls
                 │
               proto                   the messages, shared by both sides
                 │
              service                  the background process
                 └── engine            the torrents (the only code that knows librqbit)
```

At runtime, requests flow from a client through `client` and the socket to `service` and its
`engine`, and answers flow back. At compile time `proto` is the only crate both sides share;
clients never depend on `service`, and nothing but `engine` uses librqbit.

## Crates and modules

| Where | Job |
|---|---|
| `proto/` | The messages and the data they carry, and where the service's socket is (the data dir); `tolerant`, the one decoder of a settings file that resets what's invalid. No I/O but the files named: the full list of what a client can ask. |
| `client/` | Reaching the service: the connection machine (`client::machine`: connect, replace a service from another build or one that's stopping, start one, lose it), which the CLI and the app's link thread share; one method per request; `display`, the one module for text people see (sizes, durations, the words for the service's typed reasons), shared by clients. |
| `service/` | The background process: one per data dir, the socket, its lifetime as a machine (`service::machine`: idle shutdown, refusing every request while it stops), settings (`settings.json`: the default destination, the listening port, UPnP). |
| `service/src/engine/` | Torrents: add (each into its destination), list, details, priorities, pause, check, remove. One actor task owns every torrent's lifecycle, what's wanted plus a phase, and lists what can be done to each ([ADR 0006](docs/adr/0006-torrent-lifecycle-as-an-actor.md)); after every event it publishes a snapshot (the list and the watched torrents' details) when it changed, which `List` and `Details` read; owns torrent ids and de-duplication; watches destinations for files deleted outside Phantom and for destinations that can't be reached (never creating a missing one); keeps `engine.json` in the data dir. |
| `cli/` | Parses arguments and prints. |
| `gui/src/service/` | The GUI's seam to the service: a trait with a socket implementation (one link thread owns the connection; commands are messages to it) and an in-memory fake for tests and headless runs. |
| `gui/src/model.rs` | App state and what actions do to it. No drawing. |
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
- These are heuristics and taste, not rules.

## Rules

- Each module gets the narrowest input it needs: a helper takes the table state, not the app.
- Tests go through interfaces: GUI behaviour through `model` and the views against the fake
  service, the engine against a real session in a temp dir, the protocol end to end through
  `client`.
- Tests assert properties rather than hand-written outcomes: the lifecycle table over every
  state × event pair, the engine and the GUI over generated sequences against a reference model
  (`engine/model_tests.rs`, `gui/src/tests/generated.rs`; `PROPTEST_CASES` runs more), and the
  engine's add tasks, held at gates, against generated commands and restarts
  (`engine/race_tests.rs`). A new behaviour or a bug starts there, as a property or a step of the
  generated sequences; an example test is the exception, for what truly can't be generated, and
  the PR says why. A behaviour that's hard to state as an always-true property, or a bug the
  generated tests can't reach, usually means the model, or the fake, is wrong or missing something
  (e.g. answers that are never late).
- The service reports its build id (`proto::exe_id`: a hash of its own executable); a client replaces a service whose id isn't that of the `phantom-service` it would start. Nothing is bumped by hand.
- Hard parts go to dedicated, well-maintained crates rather than our own code; agree on each new
  dependency with the user before adding it.
