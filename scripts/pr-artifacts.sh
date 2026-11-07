#!/usr/bin/env bash
# Publishes screenshots/GIFs for a pull request to the `pr-artifacts` branch (kept out of the code
# history) and prints Markdown to paste into the PR body.
#
#   scripts/pr-artifacts.sh <pr-branch> <file>...
set -euo pipefail
[ $# -ge 2 ] || { echo "usage: $0 <pr-branch> <file>..." >&2; exit 2; }
branch=$1; shift
repo=$(git remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')

work=$(mktemp -d)
trap 'git worktree remove --force "$work" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
if git ls-remote --exit-code --heads origin pr-artifacts >/dev/null; then
  git fetch -q origin pr-artifacts
  git worktree add -q --detach "$work" origin/pr-artifacts
else
  git worktree add -q --detach "$work"
  git -C "$work" checkout -q --orphan pr-artifacts
  git -C "$work" rm -rq --cached . && git -C "$work" clean -fdxq
fi

mkdir -p "$work/$branch"
for f in "$@"; do cp "$f" "$work/$branch/"; done
git -C "$work" add "$branch"
# Republishing identical files changes nothing; only push when something did.
if ! git -C "$work" diff --cached --quiet; then
  git -C "$work" commit -qm "Artifacts for $branch"
  git -C "$work" push -q origin HEAD:refs/heads/pr-artifacts 2>/dev/null
fi

for f in "$@"; do
  name=$(basename "$f")
  echo "![$name](https://raw.githubusercontent.com/$repo/pr-artifacts/$branch/$name)"
done
