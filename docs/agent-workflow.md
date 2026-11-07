# Working on an issue

1. **Worktree:** `git worktree add -b <issue>-<slug> ../phantom-<slug> master`. Do everything
   there; the main checkout belongs to the coordinator.
2. **Isolation:** a real service only ever runs with `PHANTOM_DATA_DIR` and
   `PHANTOM_DOWNLOAD_DIR` in a temp dir. The GUI runs headless (`--snap`, `--tree`, `--script`;
   `PHANTOM_HEADLESS_SERVICE=1` to show a real service); never open windows, never run
   `scripts/install-mac.sh`.
3. **Decisions:** user-visible choices the issue doesn't settle (wording, placement, behaviour)
   go to the user: take the most conservative option and list it under *Open questions*.
4. **Checks:** `cargo build` without warnings and `cargo test` green.
5. **Artifacts:** for every user-visible change, render the real app headlessly:
   - stills: `target/debug/phantom-gui --snap out.png --scene <scene> --size 960x600 --scale 2`
     (both `--theme dark` and `light` when colours change), plus the 640x320 minimum size;
   - interactions: a `--script` run written to frames (`--out frames/`), turned into a GIF with
     `ffmpeg -framerate 30 -i frames/frame_%04d.png -vf "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" out.gif`.

   Publish them with `scripts/pr-artifacts.sh <branch> <files>…`, which prints the Markdown.
6. **Pull request:** push the branch and open it as a draft (`gh pr create --draft`) with a body
   of: what changed, the artifacts, decisions made, *Open questions*, and test results; end with
   `Closes #<issue>`. Mark it ready (`gh pr ready`) only once it's complete; a ready PR is the
   user's cue to review. While addressing review feedback it goes back to draft
   (`gh pr ready --undo`) until the fixes are pushed. PRs that change what the user sees wait
   for the user's approval; the coordinator merges.
