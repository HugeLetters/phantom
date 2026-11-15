#!/bin/bash
# Usage: scripts/with-timeout.sh SECONDS command [args...]
#
# Runs the command in its own process group and, if it's still running after SECONDS, kills the
# whole group (TERM, then KILL after 5 s). Exits with the command's status (143 or 137 when
# killed). macOS has no `timeout`.
#
# The watchdog is a polling loop in this script, not a background `sleep`: a background sleep
# outlives the script when killed out of order, keeps the script's stdout open, and so hangs any
# `| grep` or `| tail` reading it until the sleep ends.
set -m
secs=$1
shift
"$@" &
pid=$!
end=$((SECONDS + secs))
while kill -0 "$pid" 2>/dev/null; do
    if ((SECONDS >= end)); then
        echo "with-timeout: killed after ${secs}s: $*" >&2
        kill -TERM -"$pid" 2>/dev/null
        for _ in 1 2 3 4 5; do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
        kill -KILL -"$pid" 2>/dev/null
        break
    fi
    sleep 1
done
wait "$pid"
