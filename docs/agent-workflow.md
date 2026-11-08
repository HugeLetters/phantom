# Working on an issue

1. **Worktree:** `git worktree add -b <issue>-<slug> ../phantom-<slug> master`. Do everything
   there; the main checkout belongs to the coordinator.
2. **Isolation:** a real service only ever runs with `PHANTOM_DATA_DIR` and
   `PHANTOM_DOWNLOAD_DIR` in a temp dir. The GUI runs headless (`--snap`, `--tree`, `--script`;
   `PHANTOM_HEADLESS_SERVICE=1` to show a real service); never open windows, never run
   `scripts/install-mac.sh`.
3. **Decisions:** user-visible choices the issue doesn't settle (wording, placement, behaviour)
   go to the user: take the most conservative option and ask on the desk (`desk_ask`, label
   `pr:<n>` once the PR exists), one entry per question, with your recommendation.
4. **Checks:** `cargo build` without warnings and `cargo test` green.
5. **Artifacts:** for every user-visible change, render the real app headlessly:
   - stills: `target/debug/phantom-gui --snap out.png --scene <scene> --size 960x600 --scale 2`
     (both `--theme dark` and `light` when colours change), plus the 640x320 minimum size;
   - interactions: a `--script` run written to frames (`--out frames/`), turned into a GIF with
     `ffmpeg -framerate 30 -i frames/frame_%04d.png -vf "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" out.gif`.

   Publish them with `scripts/pr-artifacts.sh <branch> <files>…`, which prints the Markdown.
6. **Pull request:** push the branch and open it as a draft (`gh pr create --draft`). The body
   shows rather than tells: short, visual, focused on interfaces and architecture. In order:
   1. *Open questions*: one line, "On the desk (N)", or "None"; the questions themselves live
      only on the desk.
   2. *Artifacts*: the stills and GIFs.
   3. *Decisions*: choices you made that the user may want to veto, one line each.
   4. *Shape*: what changed in the code's structure, as compact visuals (see *Show, don't
      tell* below) rather than prose.
   5. *Tests*: one line each for what's covered, plus pass counts.

   End with `Closes #<issue>`. Mark it ready (`gh pr ready`) only once it's complete. While
   addressing review feedback it goes back to draft (`gh pr ready --undo`) until the fixes are
   pushed. The coordinator reviews and merges. PRs wait for the user's approval, asked on the
   desk, when they change what the user sees or does (UI, UX, CLI output, behaviour) or are
   sensitive (a new dependency, data formats on disk, anything hard to undo); the rest merge
   once the coordinator's review passes.

   **Show, don't tell** (PR *Shape* and desk questions alike; after humanlayer's `show-me`): pick
   the smallest view that makes the point, put it beside the one line it supports, and cut the
   preamble. Keep only the calls, types, files and states the reader needs to judge this change
   or choose between the options. Usually one or two views, never all of them.
   - logic: pseudocode; runtime flow: a call tree or stack (indented names, no prose);
   - types and protocol: the new or changed signatures and messages, as code;
   - interaction or state: Mermaid (`sequenceDiagram`, `stateDiagram-v2`);
   - responsibilities: a shallow file tree, one `# comment` per entry;
   - a change to an existing shape: a `diff` of that shape (`+`/`-` on the tree, stack, pseudocode
     or signature), with unchanged context only where it shows order or ownership;
   - a mostly new shape, or a copyable target: the whole block, no diff;
   - a desk question with options: the same view per option (before/after, or A/B side by side)
     so the user compares shapes instead of reading descriptions.

7. **After merge:** GitHub deletes the branch; `scripts/prune-merged.sh` in the main checkout
   removes the local branch and its worktree.
