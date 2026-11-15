#!/bin/bash
# Usage: scripts/new-worktree.sh <issue> <slug>
#
# Creates ../phantom-<slug> on branch <issue>-<slug> (from origin/master; an existing branch is
# checked out as it is), then seeds its target/ from the main checkout's with an APFS clone
# (`cp -cR`: instant, shares blocks until written), so the first build doesn't compile every
# dependency from scratch. Safe to rerun: an existing worktree is kept, an existing target/ is
# never overwritten.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <issue> <slug>" >&2
    exit 2
fi
issue=$1
slug=$2
branch="$issue-$slug"

# The main checkout is the first entry of `git worktree list`, wherever this runs from.
main=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
dest="$main/../phantom-$slug"

git -C "$main" fetch --quiet origin

if [[ -d "$dest" ]]; then
    echo "worktree $dest exists, keeping it"
elif git -C "$main" show-ref --verify --quiet "refs/heads/$branch"; then
    git -C "$main" worktree add "$dest" "$branch"
else
    git -C "$main" worktree add "$dest" -b "$branch" origin/master
fi

if [[ -e "$dest/target" ]]; then
    echo "$dest/target exists, not seeding"
elif [[ ! -d "$main/target" ]]; then
    echo "$main/target doesn't exist, nothing to seed from"
else
    # Probe first: a failing `cp -c` (not APFS, or another filesystem) must not leave a
    # half-copied target/ behind.
    probe=$(mktemp -d "$dest/.clone-probe.XXXXXX")
    echo x >"$probe/a"
    if ! cp -c "$probe/a" "$probe/b" 2>/dev/null; then
        rm -rf "$probe"
        echo "error: cp -c (APFS clone) isn't supported here; not seeding target/" >&2
        exit 1
    fi
    rm -rf "$probe"
    cp -cR "$main/target" "$dest/target.seeding" && mv "$dest/target.seeding" "$dest/target"
    echo "seeded $dest/target from $main/target"
fi
echo "$dest"
