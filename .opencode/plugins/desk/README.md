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

## Inside

Effect services and layers; `app.ts` composes them:

| Layer | Provides / does | Needs |
|---|---|---|
| `local-fs.ts` | `FileSystem` over `node:fs` (incl. `watch`) | — |
| `config.ts` | `DeskConfig` (checkout, `coordination/`, `mainSession` option) | — |
| `sessions.ts` | `Sessions`: root, get, list, prompt (the OpenCode boundary) | `PluginContext` |
| `store.ts` | `Store`: entry files, state, drafts; snapshot as a `SubscriptionRef` | `FileSystem`, `Path`, `DeskConfig` |
| `main-session.ts` | `MainSession`: claim, select, require | `Store`, `Sessions` |
| `desk.ts` | `Desk`: ask, answer, resolve, describe; drafts, submit, send | `Store`, `Sessions`, `MainSession` |
| `watcher.ts` | outside edits → `Store.reload` (re-attaches if `coordination/` is replaced) | `FileSystem`, `Store` |
| `pruning.ts` | `Store.changes` → delete prunable entries | `Store` |
| `http/` | `HttpRouter` routes on Effect's `HttpServer` over `Bun.serve`; `PageUrl` | the services above |
| `tools.ts`, `status.ts` | the `desk_*` tools; the `desk` RPC for the TUI | `PluginContext`, `Desk`, `PageUrl` |

Promises remain only at the edges: the TUI badge (`tui.tsx`, Solid), the page (`page/`, Solid), and Bun/Node callbacks
inside `local-fs.ts` and `http/server.ts`.

## Develop

```sh
cd .opencode
bun install
bun test                           # unit tests, the page bundle check, and a throwaway `opencode serve`
bunx tsc --noEmit
bun plugins/desk/page/build.ts     # after editing page/: rebuilds the committed page/dist/page.js
```

At runtime the plugin imports only what OpenCode provides (`effect`, including `effect/http`; `@opencode/plugin`;
`solid-js`) and Bun/Node built-ins, so it loads without `bun install`. For the same reason the page is a committed
bundle: Solid compiled with `babel-preset-solid`, with `marked`, `DOMPurify` and `highlight.js` inside. Only `mermaid`
loads from jsDelivr, the first time a diagram appears.
