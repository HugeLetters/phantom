# Working on an issue

0. **Words:** read the root `GLOSSARY.md` and the `GLOSSARY.md` of the domain you touch
   (`gui/`, `service/`). A new term goes to the domain that alone uses it, else to the root.
1. **Worktree:** `scripts/new-worktree.sh <issue> <slug>` (from any checkout). It fetches, adds
   `../phantom-<slug>` on branch `<issue>-<slug>` from `origin/master` (an existing branch is
   checked out instead) and seeds its `target/` from the main checkout's with an APFS clone
   (`cp -cR`), so the first build recompiles only the workspace crates, not every dependency.
   Rerunning it is safe: it never overwrites a `target/`. Do everything in the worktree; the
   main checkout belongs to the coordinator, which keeps its `target/` built at master (the
   seed). `target` is a link to `target.noindex`, which Spotlight skips; use `target/...`
   paths as usual. Delete `target.noindex/` and the `target` link when done
   (`rm -rf target target.noindex`): it frees only the blocks you changed.
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

   **Small, reversible details don't wait.** A spacing, a label, a placement within an existing
   screen, or a default that's easy to change: decide it, build it, and file its own desk entry
   worded "Decided X; say if you object", so work goes on while the user reads. New screens, new
   behaviour, installs, data formats, dependencies and anything public still wait for an answer.

   **UI wording (Apple's rule):** "…" ends a command's label only when it needs more input before
   acting (Edit…, Save as…, Add…, Move to…), never on one that acts at once (Apply, Delete).
4. **Checks:** `cargo fmt --check` clean, `cargo build` without warnings and the default suite
   green: `cargo nextest run --workspace`, then `cargo test -p gui --test appkit` (its own
   harness, which nextest can't list; seconds).
   Run them in the foreground with a long timeout: a background job can't wake you, so a turn
   that ends while one runs leaves the work unfinished. Wrap long runs in
   `scripts/with-timeout.sh <seconds> <command>` (macOS has no `timeout`): it kills the whole
   process group and leaves nothing behind. A home-made wrapper with a background `sleep` as its
   watchdog can orphan that sleep, which keeps a `| grep` hanging long after the tests finished.

   **How long a run, when:**
   - *While changing code:* the default suite (16–32 generated cases per property) plus the
     specific test you're working on.
   - *Once, when the PR goes up for review:* the default suite, plus the long runs for the areas
     it touches: `PROPTEST_CASES=256` for the engine model, race, oracle and lifecycle tests,
     `500` for the GUI. Give each long run its own command, so its time isn't put down to
     another. A review-fix round reruns long only the area its fixes touched; a reviewer reuses
     the author's long results unless a finding calls them into question.
   - *After a rebase:* if it only brought in master's changes (no conflicts in logic), the
     default suite is enough. Real conflicts in logic rerun long the touched area only.
   - *After each merge to master* the coordinator runs every long run in the background, from
     the main checkout. It blocks nothing: a failure becomes an issue, with its seed or shrunk
     case.
   - *Flake hunts* use heavy load on purpose (parallel runs plus CPU burners), but never while
     another agent has a check run going: that load makes its unrelated timing-sensitive tests
     fail falsely. Check `pgrep -fl 'cargo|nextest'` for other worktrees first.

   **Keep tests fast.** A test's budget is 20 s on an idle machine: nextest flags one past it SLOW,
   and `cargo nextest run --workspace --profile budget` fails it there unless that profile lists it,
   with why (the default profile kills a test only at 300 s: agents' runs share a loaded machine). Most of a test's time is waiting:
   - Wait on what happens, with a timeout: a status (`until`, `wait_for`), a gate's `waiting()`,
     a command handled (`engine.sent(..)`, then `engine.inspect(|_| ())`). A `sleep` is only the
     poll interval inside such a wait; a fixed one "long enough for it to happen" is slower than
     needed on an idle machine and too short on a loaded one.
   - That something *doesn't* happen: wait for what proves its moment passed, then look. In the
     engine tests: `size_checks(engine, n)`, `polls(engine, n)`, both (`settled`), a file put back
     judged (`judged`), a look reported (`looks`), the watcher live (`watching`), a stop begun
     (`stop_begun`), an answer still pending (`unanswered`). A test-only counter or flag on the
     actor for a new moment is fine: small, and `cfg(test)` or harmless.
   - A fixture that stands for something slow (a trash, a disk, a lock another process holds)
     holds at a gate the test opens (`trash::hold`, `files::Gate`, `lock_until`).
   - Timers that are tokio's alone, with no real I/O between them, run on paused time:
     `#[tokio::test(start_paused = true)]` (or `tokio::time::pause()`), whose clock moves only
     when every task waits, so a 2 s timeout takes none (`engine::bitv::tests`).
   - A test's independent cases, each with its own temp dir and engine, run side by side
     (`side_by_side(width, cases, run)` in the engine tests), not in a loop: each engine's stop
     alone waits librqbit's 1 s.
   - A property test whose cases start an engine, and so mostly wait, uses `cases::proptest!` or
     `cases::prop_state_machine!` (same form as proptest's): its cases run on several runners. One
     whose cases compute (the GUI's, rendering) keeps proptest's own: nextest already runs as
     many tests as there are cores, and more runners only cost CPU (the GUI's took a third more).
   - Use the existing small fixtures (`complete_torrent`, `multi_file_torrent`, 16 KiB pieces)
     rather than new large files.

   **What to test:** what the app does, not what it doesn't. Don't add a test that a removed or
   never-built feature stays absent (such as "an error never opens the popover"): the model-based
   tests compare the whole state after every step, so an unwanted change already fails them. A
   removed feature leaves only its machine-table row, stating what happens now.

   **Proptest seeds:** a fix's shrunk case goes into a named test, and its seed line is deleted
   from `proptest-regressions/` in the same PR. Every property test in a source file replays every
   seed in that file's regressions file, on top of its cases, and a seed is an RNG seed: once the
   generator changes it draws another case, so it no longer pins the bug.
5. **Artifacts:** for every user-visible change, render the real app headlessly:
   - stills: `target/debug/phantom-gui --snap out.png --scene <scene> --size 960x600 --scale 2`
     (both `--theme dark` and `light` when colors change), plus the 640x320 minimum size. Add
     `--at 0.5` when the scene shows anything the service sends later, such as a torrent's files:
     the first frame has none yet;
   - interactions: a `--script` run written to frames (`--out frames/`) with `--keys --pointer`,
     so the keys pressed and the cursor show, turned into a GIF with
     `ffmpeg -framerate 30 -i frames/frame_%04d.png -vf "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" out.gif`.

   **Check your own stills before anyone else sees them.** Look at every still and GIF frame you
   rendered for: text cut off or shortened, anything spilling out of its box, misalignment, both
   Tones, and 960x600, 640x600 and 640x320. Then have a second, cheap agent look only at the
   stills and list problems; fix what either of you finds before the PR goes up. Most of the
   user's UI notes so far were things the author's own stills already showed.

   **Stills before code only for a new screen** (or a new sheet, popover or window). A change to
   an existing screen is built directly and its stills go in the PR.

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

   **Review depth follows risk.** Engine, gate and data-format PRs get an adversarial review. Every
   other PR gets one review, whose reviewer fixes small findings itself rather than reporting
   them back. A new mechanism (a new state, a new stored file, a new way the engine and the gate
   talk) gets a short design note on the desk before it's built, so a design the user would drop
   is dropped at the note.

   **A feature that keeps breeding bugs goes back to the user.** When fixes in one area keep
   turning up new edge cases (each fix reveals another ordering, another state, another
   special case), stop patching and put the feature itself on the desk: the cases seen so far,
   what they have in common, and options to simplify the feature or change the model under it.
   The user judges features by their UX and may well trade a nicety for a simpler, sturdier
   model, as with dropping the automatic recovery of files for "surprises stop, Check decides"
   (ADR 0014). The coordinator watches for this across issues, not only within one PR.

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
   removes the local branch and its worktree, and any copies a killed Chrome left behind.
   Installing (`scripts/install-mac.sh`) restarts the user's Phantom, so only the coordinator
   installs, and only after the user says go on the desk.
   Headless Chrome (tests, artifacts) is always closed normally (`browser.close()`), never
   killed.
