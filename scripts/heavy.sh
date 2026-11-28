#!/bin/bash
# Usage: scripts/heavy.sh SECONDS command [args...]
#
# Runs a heavy command (a whole test suite, a long property run, a release build) in one of the
# machine's SLOTS (default 2), waiting for a free one, then under scripts/with-timeout.sh SECONDS.
# Several agents share this Mac (8 cores): each nextest run starts up to 24 test processes, so three
# at once drive the load past 300 and every timing-sensitive test fails. The timeout starts once a
# slot is held. Slots are lockf(1) locks on /tmp/phantom-heavy.N, released when the command ends,
# however it ends.
slots=${PHANTOM_HEAVY_SLOTS:-2}
here=$(cd "$(dirname "$0")" && pwd)
waited=0
while :; do
    for ((s = 1; s <= slots; s++)); do
        # -t 0: don't wait for this slot; 75 (EX_TEMPFAIL) means it's taken.
        lockf -s -t 0 "/tmp/phantom-heavy.$s" "$here/with-timeout.sh" "$@"
        rc=$?
        [ $rc -ne 75 ] && exit $rc
    done
    if ((waited % 60 == 0)); then echo "heavy: waiting for a free slot (${waited}s)" >&2; fi
    sleep 5
    waited=$((waited + 5))
done
