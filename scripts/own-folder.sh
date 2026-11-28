#!/bin/bash
# nextest run-wrapper (.config/nextest.toml): runs a test binary from a folder of its own.
# Usage: scripts/own-folder.sh <test-binary> [args...]
#
# Why: at start, CoreText (the GUI) and Foundation (the service) read the directory their
# executable sits in. target/debug/deps holds tens of thousands of files, so each test process
# start cost 0.3 s there against 0.04 s from an empty folder.
#
# The binary is hard-linked (APFS, same volume) to target/<profile>/run/<name>/<name>, relinked
# through an atomic rename whenever it was rebuilt, then run from there. If anything about the
# link fails, the binary runs where it is.
bin=$1
case $bin in
*/deps/*)
    name=$(basename "$bin")
    dir="${bin%/deps/*}/run/$name"
    if mkdir -p "$dir" 2>/dev/null; then
        if [[ "$bin" -ef "$dir/$name" ]] ||
            { ln "$bin" "$dir/.$name.$$" 2>/dev/null && mv -f "$dir/.$name.$$" "$dir/$name" 2>/dev/null; }; then
            shift
            exec "$dir/$name" "$@"
        fi
        rm -f "$dir/.$name.$$"
    fi
    ;;
esac
exec "$@"
