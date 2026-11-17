# Iterating on a theme's design

What made the Persona 5 rounds fast or slow, for the next theme. The ★ items cost the most time
when missed.

## Before round 1

- ★ **Set up the tools first:** a release-build render script, the pixel tool (`tools/px.ts`) and
  a clip script that cuts and encodes a crop. The first rounds lost hours to debug builds and
  hand-typed ffmpeg.
- **References local-only, from the start.** Game footage and crops go in a local-only git repo
  outside every worktree (see the README's "Game references"), never in an issue or the PR.
  Catalogue its patterns first (one desk entry per area, confirmed by the reviewer), so every
  proposal can sit beside its reference crop.
- **Sample the reference colours** before the first render, not after the reviewer notices the
  yellow is too red.
- **Write down what will be merged** (spec, final components, helpers, tooling, these notes), so
  exploration stays out of git from the start.
- **Keep a running list of what's settled** (the element, the option, the commit) and update it
  at every "Applied". Long sessions get compacted, and desk attachments on resolved entries are
  lost; copy any reference the reviewer sends into the reference repo.

## Interactive elements

- ★ **Draw state as eased amounts from the first component** ([ADR 0009](../../docs/adr/0009-component-state-as-eased-amounts.md)):
  hovered, pressed and disabled as 0–1 amounts on springs, and the element drawn from them. Hand-
  written transitions per state cost a round per missed pair (leave, release, pressed → idle,
  hovered → disabled), and swapping drawings made Rune cross-fade two copies.
- ★ **Show the full state walk, both ways,** every time an interactive element changes:
  - idle → hover → press (held about 1 s) → release → leave;
  - press, drag off and release outside;
  - becoming disabled while hovered, and back. A timed scene can do it: the sketch's
    `*-status-flip` pauses a torrent under a hovered Pause.
  - the secondary variant beside the primary.

  The reviewer asked for each of these in turn.
- **Check mid-transition frames, not just the end states.** Contrast dips (red letters on a face
  turning red), shapes that jump (corner rounding that differs between idle and hover) and parts
  left behind (the cyan sliver floating as the band drew back) only show in between.
- **Seed each element's random shapes from its own activation,** not a window-wide count. A row
  being left redrew its fading shard in a new shape the moment the next row was hovered.
- **Swaps in and out should match.** A press that snapped in but eased out read as a bug.

## Rune pitfalls

- **Children matched by position.** Dropping or adding a child (a shadow on hover) makes the next
  child a "new" element, and Rune cross-fades it over the old one. Keep every layer and fade it.
- **Text colour always tweens** for at least 0.1 s, even with `Spring::NONE`. Where text swaps
  colours, keep both labels and clip each to its side of a wipe.
- **Offsets spring.** A brief shake needs `instant_changes()`, and a look at the exact frames.
- **Concave polygons fill wrongly**, flickering into a fan, or a clipped chevron filling its
  notch. Draw and clip them as convex pieces, and check 30+ consecutive frames, not one still.
- **A part at zero size still leaves an anti-aliased rim.** Skip drawing it.
- **Check the docs before saying a feature doesn't exist.** `scroll_edge_fade` does work on
  vertical lists.

## Rendering

- **Release builds only.** A frame at 2× took about 10 s in debug and 0.4 s in release. A round
  of 5–7 options went from 30–50 minutes of rendering to 1–3. `scripts/render.sh` builds release.
- **Stills early in the timeline.** `--snap --at 11` simulates every frame up to 11 s first. Take
  stills at `--at 1.5` (after the appear motion), or pull a frame from a clip already rendered.
- **A few renders at a time.** Four in parallel on 8 cores; more just slow each other down.
- **Pixel checks in Bun, not Python** (`bun tools/px.ts sky|mean|diff`): about 10× less CPU.
- **Cut slow motion by input frames** (`trim=end_frame=N,setpts=4*PTS`), not `-frames:v`, which
  caps the output: a 4× clip held 1.5 s of the walk and missed the press. Check its last frame.
- **One temp folder per round, deleted at its end.** Frame folders reached tens of GB in `/tmp`.
- **Baseline before a refactor.** Rendering is byte-for-byte deterministic, so frames of every
  kept scene, rendered before and after, prove a cleanup changed nothing.

## Showing options

- **3–6 options per question, and the before.** The before is only what was liked last, not the
  whole history. When the reviewer may be wrong about a change, keep their current pick as a
  comparison scene too.
- **Combine what they liked.** When the reviewer likes parts of several options, offer the
  combinations next, keeping the parents to compare.
- **Numbers as steps:** for a size, speed or strength, offer steps around the guess (+20%, +40%;
  10%, 20%, 30% less) with the old one kept.
- **Label options by number and by what differs,** in the table and the clip names. Say whether
  each clip is normal speed, slow motion or a time-lapse.
- **Use words for what the reviewer sees,** not the code's ("veil", "morph 7" needed explaining).
- **Motion at normal speed, as MP4.** GIFs of the full window came to 29 MB and blurred faint
  detail; an MP4 is about 3 MB and stays sharp. Slow effects (a 90 s cross-fade) get a normal-
  speed clip *and* a time-lapse, labelled as such. Interactions get a 4× slow-motion clip
  beside the normal one.
- **Backgrounds in the whole window; components in a focused view.** A sky needs the whole
  window's context; a progress bar's animation is hard to judge zoomed out, so show the
  component's rows close up, still in motion. Start clips after the window's appear motion
  (the rows sliding in), so only the element's own motion shows.
- **The test data that stresses the element.** A short list hid that rows over the heap don't
  read; the 16-torrent list showed it. An error at 6% hid every error design; showing it at 60%
  (`ErrAt`) let them be compared. Show every status over its whole range (each bar 0 → 100 → 0),
  and look at the extremes: bars looked wrong at exactly 0 and 100.
- **Hover and press, not just rest,** for anything near interactive elements: a hovered button's
  sheet covered the logo, which no still at rest showed.

## Before posting

- ★ **After every "Applied", check the default is the approved look:** `px.ts diff` the default
  scene against the approved option's scene, expecting 0. Jose's "final" renders once showed an
  older round's buttons, because the approved option was set only in its comparison scene.
- ★ **Check that options differ, numerically** (`px.ts diff`). Three times options rendered
  identically: an edit went into an older code path the window no longer drew, and option
  numbers switched on extras by their bits.
- ★ **Say exactly what you checked.** "Checked pixel by pixel" or "every clip" only when that's
  what was done; name the clips you didn't watch. Three corrections had to be posted for this.
- **Look at the render yourself** at the size it will be judged. Several first attempts were too
  faint or too small to see (faint stars, nebula detail, error marks).
- **Measure claims before making them.** "Reads as well as the other columns" was posted before
  checking the stills, and was wrong for the last row.
- **Scope debug visuals to their own scene.** A magenta debug veil leaked into every later option
  by number.
- **The image viewer can show a stale file.** Copy to a new name, or check the pixels.
- **A pixel count isn't proof that motion shows.** The error twitch "changed" a few dozen pixels,
  which was the bar barely moving. Look at the frames.
- **Record brief motion at 60 fps.** A 0.1 s twitch is one frame or none at 12 fps.

## The option scaffolding

- **Each element's options are a numbered `Part`**, set per scene. A settled choice becomes `0`,
  and the old look moves to a free number, so the defaults are always the latest decisions and
  every past option still renders by its scene name.
- **Don't derive an option's behaviour from its number's bits or ranges** (`16.. => opt - 16`).
  New options inherited extras by accident. Match options explicitly.
- **When a new version replaces a code path, delete the old one** (or make it fail loudly), so
  an edit can't land in code nothing draws.
- **When the same code shape exists for an old and a new version,** anchor edits after the new
  function's name; a first-match replace hits the old one.

## Working with the reviewer

- **One thread per element,** one entry per question, each ending in a plain question; side
  notes get their own threads. Questions are high priority, "Applied" notes low. Post an
  "Applied" note only when there's something to check (a changed default, a side effect).
- **Quote the reviewer's words back** when a note could mean two things ("translucent": more
  transparent, or frosted?), and look at the reference they point to before round 1. Jose's
  button hover took about 12 rounds to reach the shop's morphing board.
- **When a method keeps missing, change the method.** After two rounds of "none of these", try a
  different technique, not more variants. Shapes and blurs never looked like the nebula
  references across about 60 options; a noise texture did on the first try. Errors went through
  three rounds of broken stars and moons before starting from scratch.
- **Before building a spike, ask what each outcome would change,** and cap it at one round. The
  angled window was built and dropped.
- **Say what you couldn't do,** and note the mapping when the reviewer corrects a pick
  (pills 4 → 5).
- **Leave sketch performance alone;** note the production costs in the spec instead.
