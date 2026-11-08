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
| `proto/` | The messages and the data they carry, and where the service's socket is (the data dir). No I/O: the full list of what a client can ask. |
| `client/` | Reaching the service: connect, start it (`phantom-service` next to the client), protocol version check, one method per request; display formatting shared by clients. |
| `service/` | The background process: one per data dir, the socket, idle shutdown, settings. |
| `service/src/engine/` | Torrents: add, list, details, priorities, pause, check, remove. One actor task owns every torrent's Status as a state machine ([ADR 0006](docs/adr/0006-torrent-lifecycle-as-an-actor.md)); owns torrent ids and de-duplication; watches destinations for files deleted outside Phantom; keeps `engine.json` in the data dir. |
| `cli/` | Parses arguments and prints. |
| `gui/src/service/` | The GUI's seam to the service: a trait with a socket implementation and an in-memory fake for tests and headless runs. |
| `gui/src/model.rs` | App state and what actions do to it. No drawing. |
| `gui/src/views/` | Screens: what's shown and which actions exist, drawn through `ui`. |
| `gui/src/ui/` | Building blocks every screen draws with; themes override them ([ADR 0004](docs/adr/0004-themes-override-building-blocks.md)). |
| `gui/src/platform/` | macOS integration: magnet links, files opened from Finder, Reveal in Finder. |
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
- These are heuristics and taste, not rules.

## Rules

- Each module gets the narrowest input it needs: a helper takes the table state, not the app.
- Tests go through interfaces: GUI behaviour through `model` and the views against the fake
  service, the engine against a real session in a temp dir, the protocol end to end through
  `client`.
- A change to `proto` bumps `PROTOCOL_VERSION`, so clients replace an older service.
- Hard parts go to dedicated, well-maintained crates rather than our own code; agree on each new
  dependency with the user before adding it.
