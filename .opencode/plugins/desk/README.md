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

## Develop

```sh
cd .opencode
bun install
bun test           # unit tests, plus an integration test against a throwaway `opencode serve`
bunx tsc --noEmit
```

At runtime the plugin imports only what OpenCode provides (`effect`, `@opencode/plugin`, `solid-js`) and Bun/Node
built-ins, so it needs no `bun install` to load. The page loads `marked`, `DOMPurify`, `highlight.js` and `mermaid` from
jsDelivr.
