---
id: 4
parent: 3
asker: main
answerer: user
status: open
labels: [glossary, service]
created: -59m
---

# Should Check also re-verify pieces that were complete?

Following up on your answer. The two options as a state machine:

```mermaid
stateDiagram-v2
    [*] --> Missing: files deleted
    Missing --> Checking: Check
    Checking --> Seeding: all pieces verify
    Checking --> Downloading: some pieces bad
```

Re-verifying everything costs a full read of the data:

```rust
fn check(torrent: &Torrent) -> Verdict {
    torrent.pieces().all(|piece| piece.verify())
}
```
