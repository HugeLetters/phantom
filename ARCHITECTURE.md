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
| `proto/` | The messages and the data they carry. No I/O: the full list of what a client can ask. |
| `client/` | Reaching the service: connect, start it, protocol version check, one method per request; paths and display formatting shared by clients. |
| `service/` | The background process: one per data dir, the socket, idle shutdown, settings. |
| `service/src/engine/` | Torrents: add, list, details, priorities, pause, remove. Owns torrent ids and de-duplication. |
| `cli/` | Parses arguments and prints. |
| `gui/src/service/` | The GUI's seam to the service: a trait with a socket implementation and an in-memory fake for tests and headless runs. |
| `gui/src/model.rs` | App state and what actions do to it. No Rune. |
| `gui/src/views/` | Screens: what's shown and which actions exist, drawn through `ui`. |
| `gui/src/ui/` | Building blocks every screen draws with; themes override them ([ADR 0004](docs/adr/0004-themes-override-building-blocks.md)). |
| `gui/src/platform/` | macOS integration: magnet links, files opened from Finder, Reveal in Finder. |
| `design/` | Icons and theme references. |

## Rules

- Each module gets the narrowest input it needs: a helper takes the table state, not the app.
- Tests go through interfaces: GUI behaviour through `model` and the views against the fake
  service, the engine against a real session in a temp dir, the protocol end to end through
  `client`.
- A change to `proto` bumps `PROTOCOL_VERSION`, so clients replace an older service.
