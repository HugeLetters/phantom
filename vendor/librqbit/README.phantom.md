# librqbit 9.1.0, patched

The crate as published on crates.io (`librqbit-9.1.0`, upstream commit `4309d387`), used through
`[patch.crates-io]` in the root `Cargo.toml`. `LICENSE` is the upstream repository's, at `v9.1.0`.

One change on top, in its own commit: `Session::stop` waits, up to 5 s, for the tasks spawned with
the session's cancellation token to drop their futures, instead of sleeping 1 s. They're tracked
with a `tokio_util::task::TaskTracker` (`Session::tasks`, shared through
`ManagedTorrentShared::tasks`), which needs tokio-util's `rt` feature.

To drop the patch once a librqbit release has the fix: bump `librqbit` in `service/Cargo.toml`,
delete this folder, and remove the `librqbit` line from `[patch.crates-io]` and `exclude =
["vendor"]` from the root `Cargo.toml` (unless something else is vendored by then).
