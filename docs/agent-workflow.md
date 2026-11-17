# Working on an issue

1. **Worktree:** `scripts/new-worktree.sh <issue> <slug>` (from any checkout). It fetches, adds
   `../phantom-<slug>` on branch `<issue>-<slug>` from `origin/master` (an existing branch is
   checked out instead) and seeds its `target/` from the main checkout's with an APFS clone
   (`cp -cR`), so the first build recompiles only the workspace crates, not every dependency.
   Rerunning it is safe: it never overwrites a `target/`. Do everything in the worktree; the
   main checkout belongs to the coordinator, which keeps its `target/` built at master (the
   seed). Delete your `target/` when done: it frees only the blocks you changed.
2. **Isolation:** a real service only ever runs with `PHANTOM_DATA_DIR` and
   `PHANTOM_DOWNLOAD_DIR` in a temp dir. The GUI runs headless (`--snap`, `--tree`, `--script`;
   `PHANTOM_HEADLESS_SERVICE=1` to show a real service); never open windows, never run
   `scripts/install-mac.sh`. Still set both variables to a temp dir for every render and GIF
   run: a headless GUI falls back to a fresh temp dir by itself (`isolate_headless` in
   `gui/src/main.rs`), but the explicit variables are the rule, the fallback a safety net.
3. **Decisions:** user-visible choices the issue doesn't settle (wording, placement, behaviour)
   go to the user: take the most conservative option and ask on the desk (`desk_ask`, label
   `pr:<n>` once the PR exists), one entry per question, with your recommendation. A PR's
   questions hang under its review entry: find it with `desk_list` (title "Review PR #<n>…") and
   pass it as `parent`; if it doesn't exist yet, create it first.
4. **Checks:** `cargo fmt --check` clean, `cargo build` without warnings and `cargo test` green.
   Run them in the foreground with a long timeout: a background job can't wake you, so a turn
   that ends while one runs leaves the work unfinished. Wrap long runs in
   `scripts/with-timeout.sh <seconds> <command>` (macOS has no `timeout`): it kills the whole
   process group and leaves nothing behind. A home-made wrapper with a background `sleep` as its
   watchdog can orphan that sleep, which keeps a `| grep` hanging long after the tests finished.

   **How long a run, when** (long property runs were two thirds of agents' command time):
   - *While changing code:* the default `cargo test` (16–32 generated cases per property) plus
     the specific test you're working on.
   - *Once, when the PR goes up for review:* the long runs for the areas it touches,
     `PROPTEST_CASES=256` for the engine model, race and lifecycle tests, `500` for the GUI.
     Give each long run its own command, so its time isn't put down to another.
     A review-fix round reruns long only the area its fixes touched; a reviewer reuses the
     author's long results unless a finding calls them into question.
   - *After a rebase:* if it only brought in master's changes (no conflicts in logic), the
     default `cargo test` is enough. Real conflicts in logic rerun long the touched area only.
   - *Flake hunts* use heavy load on purpose (parallel runs plus CPU burners), but never while
     another agent has a check run going: that load makes its unrelated timing-sensitive tests
     fail falsely. Check `pgrep -fl 'cargo test'` for other worktrees first.

   **What to test:** what the app does, not what it doesn't. Don't add a test that a removed or
   never-built feature stays absent (such as "an error never opens the popover"): the model-based
   tests compare the whole state after every step, so an unwanted change already fails them. A
   removed feature leaves only its machine-table row, stating what happens now.
5. **Artifacts:** for every user-visible change, render the real app headlessly:
   - stills: `target/debug/phantom-gui --snap out.png --scene <scene> --size 960x600 --scale 2`
     (both `--theme dark` and `light` when colours change), plus the 640x320 minimum size;
   - interactions: a `--script` run written to frames (`--out frames/`) with `--keys --pointer`,
     so the keys pressed and the cursor show, turned into a GIF with
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
   - interaction or state: Mermaid (`sequenceDiagram`, `stateDiagram-v2`), parsed before posting
     (`mermaid.parse` with bun). In sequence diagrams, `actor` is a keyword and `;` ends a
     message: alias participants (`participant S as Service`);
   - responsibilities: a shallow file tree, one `# comment` per entry;
   - a change to an existing shape: a `diff` of that shape (`+`/`-` on the tree, stack, pseudocode
     or signature), with unchanged context only where it shows order or ownership;
   - a mostly new shape, or a copyable target: the whole block, no diff;
   - a desk question with options: the same view per option (before/after, or A/B side by side)
     so the user compares shapes instead of reading descriptions.

7. **After merge:** GitHub deletes the branch; `scripts/prune-merged.sh` in the main checkout
   removes the local branch and its worktree, and any copies a killed Chrome left behind. A
   merge that changes the app (anything outside `.opencode/` and docs) is followed by
   `scripts/install-mac.sh`, so the user's installed Phantom is always master.
   Headless Chrome (tests, artifacts) is always closed normally (`browser.close()`), never
   killed.
