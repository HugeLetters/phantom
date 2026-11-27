# Phantom

A macOS BitTorrent client: a background service plus GUI and CLI clients, in Rust. Start from
[ARCHITECTURE.md](ARCHITECTURE.md); use the words in [GLOSSARY.md](GLOSSARY.md)
and in the `GLOSSARY.md` of the domain you touch ([gui](gui/GLOSSARY.md), [service](service/GLOSSARY.md)).

- `cargo nextest run --workspace` plus `cargo test -p gui --test appkit` runs everything
  ([docs/agent-workflow.md](docs/agent-workflow.md) step 4). GUI tests run headless against an
  in-memory service.
- Anything that starts a real service sets `PHANTOM_DATA_DIR` and `PHANTOM_DOWNLOAD_DIR` to a
  temp dir, so the user's own torrents stay untouched.
- No migration code: a change to stored data (the record, `undo.json`, `gui.json`, settings)
  may break what an older build wrote. Don't read old formats or take old files over. Repairing a
  damaged file (reset what's invalid, with a notice) is not migration and stays.
- Other sessions may be editing the repo, especially `design/`: stage only the paths you changed.
- Working on an issue: follow [docs/agent-workflow.md](docs/agent-workflow.md).
- Questions for the user (open questions, review requests) go to the desk via `desk_ask`, not the
  chat. The desk is a global OpenCode plugin, not part of this repo.
