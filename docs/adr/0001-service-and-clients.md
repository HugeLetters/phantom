# A background service runs the torrents; the app and CLI are clients

Torrents run in `phantom-service`, a separate process the GUI and CLI talk to over a Unix socket, rather than inside the app. A client starts the service when none is running, and the service exits 10 s after its last client disconnects. So closing the window keeps downloading while the app runs, quitting it stops everything, and a crash in the GUI never interrupts a download. More than one client can share the service.

## Considered options

- **Everything in the app process.** Simpler, with the same ⌘W/⌘Q behaviour, but no crash isolation and no second client (CLI) on the same torrents.
- **A service that runs until stopped**, like `transmission-daemon`. Rejected: quitting the app should stop downloads. A client that just stays connected (`phantom keepalive`) gives that mode when wanted.
