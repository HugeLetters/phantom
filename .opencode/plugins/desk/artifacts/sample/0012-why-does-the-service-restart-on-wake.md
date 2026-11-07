---
id: 12
parent:
asker: user
answerer: main
status: answered
labels: [service]
created: -50m
---

# Why does the service restart on wake?

After sleep the menu bar icon blinks and torrents re-announce. Expected?

<!-- desk:answer -->

Yes: on wake the network interfaces change, so the service re-binds its listener and re-announces to trackers (`service/src/net.rs`). It doesn't restart the process; the blink is the reconnect indicator.
