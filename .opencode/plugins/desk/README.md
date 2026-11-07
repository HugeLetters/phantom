# desk

Questions, answers and reviews between the user and the agents, kept out of the chat (#30).

- **Agents** use the tools `desk_ask`, `desk_answer`, `desk_resolve` and `desk_list`.
- **The user** uses the desk page. The plugin serves it on `http://127.0.0.1:47xxx/`, a stable port per checkout.
- **The TUI footer** shows `desk N`, the number of entries waiting on the user. Clicking it opens the page.

## Entries

One Markdown file per entry, in the checkout's gitignored `coordination/` directory:

```md
---
id: 7
parent: 3
asker: main
answerer: user
status: open
labels: [design, icons]
created: 2026-11-07T16:30:00.000Z
---

# Which icon?

The question.

<!-- desk:answer -->

The answer.
```

`asker` and `answerer` are `user` or `main` (the main session, speaking for the agents). An entry moves through three
statuses:

- `open`: waiting on the answerer.
- `answered`: waiting on the asker to read it, follow up or mark it done.
- `resolved`: the asker marked it done.

Resolved entries with nothing unresolved below them are deleted automatically, bottom-up. Files that don't look like
entries (`NNNN-slug.md` with this header) are ignored. The desk's own state (next id, main session, drafts) lives in
`coordination/.desk/`.

## Main session

Answers go to the main session. It's the top-level session of the first session to call a desk tool. The user can
switch it from the page, or fix it with the plugin option `mainSession`.

On the page:

- **Submit** sends every draft in one message with `delivery: "queue"`, after the current turn.
- **Send now** sends one draft with `delivery: "steer"`.

## Install

Once per checkout, like the global package in `~/.config/opencode`:

```sh
cd .opencode
bun install          # node_modules/ is gitignored; bun.lock is committed
```

Until then the desk stays off: OpenCode's log shows one line, ``desk: dependencies are not installed. Run `bun install`
in <checkout>/.opencode, then restart OpenCode's service (`opencode service restart`); the desk stays off until
then.``, and everything else works as usual.

The restart is needed once, when installing while OpenCode is running: Bun caches module resolution per process, so
after it has seen `.opencode/` without `node_modules`, reloading the plugin still can't find the packages. (The
integration test checks this; the desk then logs a line saying to restart.)

## Inside

Effect services and layers; `app.ts` composes them:

| Layer | Provides / does | Needs |
|---|---|---|
| `BunServices` (`@effect/platform-bun`) | `FileSystem` (incl. `watch`), `Path`, … | — |
| `config.ts` | `DeskConfig` (checkout, `coordination/`, `mainSession` option), `PluginContext` | — |
| `sessions.ts` | `Sessions`: root, get, list, prompt (the OpenCode boundary) | `PluginContext` |
| `store.ts` | `Store`: entry files, state, drafts; snapshot as a `SubscriptionRef` | `FileSystem`, `Path`, `DeskConfig` |
| `main-session.ts` | `MainSession`: claim, select, require | `Store`, `Sessions` |
| `desk.ts` | `Desk`: ask, answer, resolve, describe; drafts, submit, send | `Store`, `Sessions`, `MainSession` |
| `watcher.ts` | outside edits → `Store.reload` (re-attaches if `coordination/` is replaced) | `FileSystem`, `Store` |
| `pruning.ts` | `Store.changes` → delete prunable entries | `Store` |
| `page/bundle.ts` | `PageBundle`: the Solid page compiled in memory by `Bun.build` when the plugin loads | — |
| `http/` | `HttpRouter` routes on `BunHttpServer`; `PageUrl` | the services above |
| `tools.ts`, `status.ts` | the `desk_*` tools; the `desk` RPC for the TUI | `PluginContext`, `Desk`, `PageUrl` |

Promises remain only at the edges: the TUI badge (`tui.tsx`, Solid) and the page itself (`page/`, Solid). `index.ts`
imports only what OpenCode provides; it checks the install (`install-check.ts`, `node:fs` only) and then imports
`app.ts` dynamically, so a missing install becomes the log line above rather than a failed plugin.

## Develop

```sh
cd .opencode
bun test           # unit tests and a throwaway `opencode serve` (needs `opencode` on PATH)
bunx tsc --noEmit
```

## Artifacts

Screenshots and the walk-through GIF for pull requests come from a script, not by hand:

```sh
cd .opencode
bun run artifacts [out-dir]                  # default: $TMPDIR/desk-artifacts
scripts/pr-artifacts.sh <branch> <out-dir>/*  # from the repo root: publish, prints the Markdown
```

It runs the page on the plugin's own layers (demo sessions instead of OpenCode) in a temporary checkout whose
`coordination/` holds only the committed sample (`artifacts/sample/`), plus a GIF and a video it generates with ffmpeg.
It never reads a real `coordination/`. Through headless Google Chrome (`playwright-core`) it writes:

- `desk-list.png`, `desk-entry-images.png`, `desk-entry-mermaid.png`, `desk-entry-video.png`: 1000 px wide at 2x, light theme;
- `desk-entry-dark.png`: the same, dark theme, showing an entry with a parent and a follow-up;
- `desk-walk.gif`: list → #11 → type an answer → a follow-up file arrives in `coordination/` → Next → Mark done → Next →
  Submit, taken frame by frame and encoded with ffmpeg at 12 fps.

It needs Google Chrome and ffmpeg installed. Mermaid loads from jsDelivr, so it also needs network access.
