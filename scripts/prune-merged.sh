#!/bin/sh
# Removes local branches whose GitHub branch is gone (GitHub deletes a branch when its PR merges),
# together with their worktrees. Run from the main checkout.
set -eu
git fetch --prune --quiet
git for-each-ref --format='%(refname:short) %(upstream:track)' refs/heads |
	awk '$2 == "[gone]" { print $1 }' |
	while read -r branch; do
		worktree=$(git worktree list --porcelain |
			awk -v ref="branch refs/heads/$branch" '/^worktree /{w=substr($0,10)} $0==ref{print w}')
		[ -n "$worktree" ] && git worktree remove --force "$worktree" && echo "removed $worktree"
		git branch -D --quiet "$branch" && echo "deleted $branch"
	done
