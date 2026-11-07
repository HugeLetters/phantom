# Phantom

A BitTorrent client for macOS, written in Rust: a background service that runs the torrents,
the Phantom app and the `phantom` command line client. The torrent work is done by
[librqbit](https://github.com/ikatson/rqbit); the app is drawn with
[Rune](https://github.com/anomalyco/rune).

## Install

```sh
scripts/install-mac.sh
```

This builds a release and installs `~/Applications/Phantom.app`. Rerun it to update. Phantom
then opens magnet links clicked anywhere. Downloads go to `~/Downloads`; the service keeps its
state and log in `~/Library/Application Support/Phantom`.

## Command line

`cargo build` puts `phantom` in `target/debug/`. It starts the service if needed:

```sh
phantom add 'magnet:?xt=urn:btih:…'   # or a .torrent file or URL
phantom watch                          # the list, refreshed every second
phantom pause 0 / resume 0
phantom remove 0                       # keeps its files; `delete 0` deletes them too
phantom keepalive                      # keep the service running with no app open
phantom stop
```

## Developing

See [ARCHITECTURE.md](ARCHITECTURE.md) for the map of the code and [GLOSSARY.md](GLOSSARY.md)
for the words it uses. `cargo test` runs the tests.
