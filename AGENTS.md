# Phantom

A macOS BitTorrent client: a background service plus GUI and CLI clients, in Rust. Start from
[ARCHITECTURE.md](ARCHITECTURE.md); use the words in [GLOSSARY.md](GLOSSARY.md).

- `cargo test` runs everything. GUI tests run headless against an in-memory service.
- Anything that starts a real service sets `PHANTOM_DATA_DIR` and `PHANTOM_DOWNLOAD_DIR` to a
  temp dir, so the user's own torrents stay untouched.
- Other sessions may be editing the repo, especially `design/`: stage only the paths you changed.
- Working on an issue: follow [docs/agent-workflow.md](docs/agent-workflow.md).
