# desk

Questions, answers and reviews between the user and the agents, kept out of the chat (#30).

- **Agents** use the tools `desk_ask`, `desk_answer`, `desk_resolve` and `desk_list`.
- **The user** uses the desk page. The plugin serves it on `http://127.0.0.1:47xxx/`, a fixed port per project.
- **The TUI footer** shows `desk N`, the number of entries waiting on the user. Clicking it opens the page.

Run `bun install` in `.opencode/` once per checkout. It also builds the page (Vite, `postinstall`). Without it, the
plugin fails to load.

## Entries

One Markdown file per entry, `coordination/<id>.md`. Ids are random UUIDs (`crypto.randomUUID()`), never reused.
Nothing depends on their order.

```md
---
parent: 3f2b8c1e-6b1e-4c2a-9d4e-1f2a3b4c5d6e
asker: agent
status: open
session: ses_…
labels: [design, icons]
created: "2026-11-07T16:30:00.000Z"
---

# Which icon?

The question.

<!-- desk:answer -->

The answer.
```

- **Asker and answerer:** `asker` is `agent` or `user`; the answerer is always the other one.
- **Status:** `open` (waiting on the answerer) → `answered` (waiting on the asker) → `resolved` (the asker marked it done). A resolved entry leaves the desk once it has no follow-ups; then its parent gets the same check, and so on up the chain.
- **Session:** the top-level OpenCode session the entry belongs to. For an agent's question, that's the session above whichever (sub)agent asked. Everything the user says about the entry goes there.
- **Parents:** an entry's `parent` must exist when the entry is created, and never changes, so there are no cycles and no orphans.
- **Order:** entries are ordered by `created`: the list, `desk_list` and Next all use it.
- **Showing ids:** the page shows a short form, `#` plus the last six hex digits. An entry's
  **Copy id** button copies the full id for pasting into chat. The tools and `desk_list` take and show full ids.
- **Other files:** files in `coordination/` that aren't `<uuid>.md` are ignored.

The plugin reads `coordination/` when it loads and writes every change itself; nobody edits the files by hand.

## The page

- **The list:** open entries at any depth, in two groups, *Waiting on you* and *Waiting on the agents*.
- **An entry:** its parent, its follow-ups, a draft box, and **Next**, which goes to the next entry waiting on you, by `created`, wrapping round.
- **Drafts:** kept in the browser. On an entry that waits for your answer, the draft is your answer. On any other unresolved entry, it becomes a follow-up question from you, filed under that entry in the same session.
- **Submit:** sends every draft, all or nothing, as one queued message per session.
- **Send now:** sends one draft right away, steering its session's current turn.

## Inside

| File | What it is |
|---|---|
| `entry.ts` | the entry and its file format (YAML header via `Bun.YAML` + Schema) |
| `api.ts` | the page's API: an Effect `RpcGroup` (`view` stream, `submit`, `send`, `resolve`), shared by page and plugin |
| `sessions.ts` | `Sessions`: the seam to OpenCode (or recorded, in demos and tests) |
| `desk.ts` | `Desk`: the entries in a `SubscriptionRef`, written to their files inside each update |
| `http.ts` | `BunHttpServer`: the built page (`page/dist/`) and repo files via `HttpStaticServer`, the RPC API over HTTP (NDJSON), and one guard (loopback Host; same-origin writes) |
| `opencode.ts` | the `desk_*` tools and the `desk` RPC the TUI badge reads (`tui-rpc.ts`, `tui.tsx`) |
| `app.ts` | `desk(root, port)` and `layer(ctx, root)` |
| `page/` | built by Vite (`vite.config.ts`, `vite-plugin-solid`) into the gitignored `dist/`. `index.html` is the shell, `main.tsx` the app (Solid, `@solidjs/router`'s hash router, `@solidjs/meta` for the title), `markdown.ts` renders Markdown (marked, DOMPurify, highlight.js; `mermaid` in its own chunk, loaded on first use), and `style.css` holds the styles |

## Develop

```sh
cd .opencode
bun install        # dependencies, and builds the page
bun run page       # while editing page/: rebuilds on save; reload the browser, no OpenCode restart needed
bun run test       # builds the page, then unit tests and a throwaway `opencode serve` (needs `opencode` on PATH)
bunx tsc --noEmit
```

A bare `bun test` skips the `pretest` build, so the page tests then run against whatever `page/dist/` holds.

## Artifacts

Screenshots and the walk-through GIF for pull requests come from a script:

```sh
cd .opencode
bun run artifacts [out-dir]                  # default: $TMPDIR/desk-artifacts
scripts/pr-artifacts.sh <branch> <out-dir>/*  # from the repo root: publish, prints the Markdown
```

The script runs `App.desk` with recorded sessions in a temporary checkout. That checkout's `coordination/` holds only a
sample desk generated from a fixed seed (`artifacts/sample.ts`: entries, ids, the follow-up tree and the images), plus a
GIF and a video the script generates with ffmpeg. Only the seed is committed, and it never reads a real `coordination/`.
It drives headless Google Chrome (`playwright-core`) and writes:

- `desk-list.png`, `desk-entry-images.png`, `desk-entry-mermaid.png`, `desk-entry-video.png`: 1000 px wide at 2x, light theme.
- `desk-entry-dark.png`: an entry with a parent and a follow-up, dark theme.
- `desk-walk.gif`: list → an entry → type an answer → an agent's follow-up arrives → Next → Mark done → Next → Submit, encoded with ffmpeg at 12 fps.

It needs Google Chrome and ffmpeg installed.
