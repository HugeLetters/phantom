# Iterating on a theme's design

What made the Persona 5 rounds fast or slow, for the next theme.

## Rendering

- **Release builds only.** A frame at 2× took about 10 s in debug and 0.4 s in release. A round
  of 5–7 options went from 30–50 minutes of rendering to 1–3. `scripts/render.sh` builds release.
- **Stills early in the timeline.** `--snap --at 11` simulates every frame up to 11 s first. Take
  stills at `--at 1.5` (after the appear motion), or pull a frame from a clip already rendered.
- **A few renders at a time.** Four in parallel on 8 cores; more just slow each other down.
- **Pixel checks in Bun, not Python** (`bun tools/px.ts sky|mean|diff`): about 10× less CPU.

## Showing options

- **3–6 options per question, and the before.** The before is only what was liked last, not the
  whole history.
- **Motion at normal speed, as MP4.** GIFs of the full window came to 29 MB and blurred faint
  detail; an MP4 is about 3 MB and stays sharp. Slow effects (a 90 s cross-fade) get a normal-
  speed clip *and* a time-lapse, labelled as such.
- **The whole window, not zoomed crops,** unless the detail is too small to see. Context changes
  how an element reads.
- **The test data that stresses the element.** A short list hid that rows over the heap don't
  read; the 16-torrent list showed it. An error at 6% hid every error design; showing it at 60%
  (`ErrAt`) let them be compared.
- **Hover and press, not just rest,** for anything near interactive elements: a hovered button's
  sheet covered the logo, which no still at rest showed.

## Before posting

- **Check that options differ, numerically** (`px.ts diff`). Twice in this round options rendered
  identically: an edit went into an older code path the window no longer drew, and option
  numbers switched on extras by their bits.
- **Look at the render yourself** at the size it will be judged. Several first attempts were too
  faint or too small to see (faint stars, nebula detail, error marks).
- **Measure claims before making them.** "Reads as well as the other columns" was posted before
  checking the stills, and was wrong for the last row.
- **Sample reference colours** instead of eyeballing them (the logo's green).
- **The image viewer can show a stale file.** Copy to a new name, or check the pixels.

## The option scaffolding

- **Each element's options are a numbered `Part`**, set per scene. A settled choice becomes `0`,
  and the old look moves to a free number, so the defaults are always the latest decisions and
  every past option still renders by its scene name.
- **Don't derive an option's behaviour from its number's bits or ranges** (`16.. => opt - 16`).
  New options inherited extras by accident. Match options explicitly.
- **When the same code shape exists for an old and a new version,** anchor edits after the new
  function's name; a first-match replace hits the old one.

## Working with the reviewer

- **One thread per element,** one entry per question; side notes get their own threads.
- **Say what you couldn't do,** and when an idea isn't working, start over instead of tweaking
  it. Errors went through three rounds of broken stars and moons before starting from scratch.
- **When a method keeps missing, change the method.** Shapes and blurs never looked like the
  nebula references; a noise texture did on the first try.
- **Ask what the reviewer means when a note is ambiguous,** and note the mapping when they
  correct a pick (pills 4 → 5).
