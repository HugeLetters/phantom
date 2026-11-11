# Persona 5 theme sketches (#11)

A visual exploration of the Persona 5 theme on **throwaway components**. Nothing here is app code
or the `Theme` interface ([ADR 0004](../../docs/adr/0004-themes-override-building-blocks.md)):
it's a standalone Rune crate, outside the workspace, pinned to the same Rune rev as
`gui/Cargo.toml`, that draws Phantom's current layout (toolbar, torrent list, status bar). Style
cues only: no copied assets, logos or fonts. Every face named here ships with macOS, so a theme can
name it without bundling it.

The sketch has a window still, a component sheet (buttons in every state, progress bars, an error
list, rows idle/selected/hovered) and a GIF of a fixed script: hover and press Add, hover rows,
select a row, press Pause, select a paused torrent, press Resume.

## The direction: All-Out

A red ground with a slowly turning starburst and black halftone. The list sits on a tilted sheet of
white paper with a fat black outline. The buttons and the selection are black cut-out slabs, and
the edges are jagged rather than straight. Everything moves: rows slam in when they appear, buttons
jump off their shadow on hover and slam into it on press, each progress bar moves by status, and
the ghost bobs.

- **Slanted slabs.** Buttons, the field, the header strip and the status ribbon are cut on the
  slant: no square corners.
- **Active shapes.** A hovered or pressed button and the selected row throw out a *blade*: a spiky,
  non-rectangular shape behind them, at a random tilt. Each activation lands differently.
- **One logo.** The ransom "PHANTOM" is text only, in the titlebar. The ghost sits by "Connected",
  where it shows the connection state.
- **Lettering.** Ransom letters (five faces swapped letter by letter, a few on scraps, the first
  letter a little larger, only slightly tilted, so labels stay readable) for the title, buttons,
  column headings and the connection state. Torrent names and statuses are in
  Optima: Persona 5 Royal sets its English lists in Optima Nova Black.
- **The list, as the game's item lists.** A black tilted sheet with a white outline and thin
  rules. Rows step right a few points each, every eight rows over. The selection is a red band
  with a wedge out past the left edge, and one letter of the selected name is inverted. Percents
  sit in slanted pills (dark red with a cyan sliver on the selection). Speeds have a coloured
  underline: cyan down, pink up.
- **Progress by status.** Downloading is an arrowhead with racing stripes. Seeding is full, with
  chevrons marching out. Checking is halftone with a scanning slit. Paused is frozen grey blocks.
  Error is torn, cracked and twitching. Fetching is an indeterminate shard sliding back and forth.

![window](out/attack.png)
![parts](out/attack-parts.png)
![motion](out/attack.gif)

`out/` is gitignored: run `scripts/render.sh attack` to see these. The first pass is published on
the PR's artifacts branch.

## Next

- Active shapes from reference footage of the game's menus, once it's captured.
- An alternate palette after Jose's shop: blue and yellow in place of red, black and white (soft
  pale-yellow flags, torn paper tape, cards that sway). See
  `out/reference/patterns/battle/jose-*`.
- Motion is always on. In the real theme it becomes a setting.
- Light / dark is set aside: design one good-looking theme first. Whether it gets a light and a dark
  variant, or neither, is decided later.
- A spike: an angled four-sided window (transparent, borderless, with the corners cut away). It has
  to prove that clicks on the cut corners reach the app behind, that resizing and dragging work, and
  where the traffic lights go, before any design depends on that shape.

**Decided so far** (on the desk): C as the base; one logo (ransom title, ghost by Connected);
the black list sheet, the red selection band and the inverted letter; percent pills (condensed
face, 46 pt); header names; speeds as **F**, black figures on a living cyan / pink slab; row steps as **B2** at 7 pt
(random per row, never close to the row above); buttons lettered **P2 + one box**, with at least one
lowercase letter and at most half; progress tracks as **B**, level top and bottom with leaning
sides, patterns straight, a red shadow on error and none while seeding. Still open:
the active button (round 2), and the Jose palette later. The sheets that compare options are the
`attack-letters`, `attack-hovers`, `attack-steps`, `attack-speeds` and `attack-tracks` scenes;
`attack-many*` shows a 16-torrent list. Footage pattern catalogues are in
`out/reference/patterns/` (gitignored).

Open questions live on the desk. Every proposal there offers **3–6 rendered options** and shows
the **before** next to them. In a follow-up round, the before is only what the user liked last
round, each beside its tweak, not the whole previous sheet. Renders go to `out/v/<commit>-…` as well as `out/`, so an older
desk entry keeps showing what it showed.

## Fonts

Every face is a macOS system font, named rather than bundled. Rune doesn't look system fonts up by
name, though: a face it hasn't loaded silently falls back to Inter. So `main.rs` reads the font
files from `/System/Library/Fonts` at start and hands them to Rune. A real theme needs the same.
The fan-made Persona 5 fonts have no licence or forbid apps, so none is bundled.

The ransom letters swap between five faces, as a fan's guide to Persona 5 Royal's fonts suggests
for the main menus: Futura, Times New Roman, Superclarendon (a Clarendon), Helvetica Neue and Avenir
Next (in place of ITC Avant Garde).

**To try later**, to see whether they look nicer:

| Role | Font | Licence |
|---|---|---|
| list and body text | Optima Nova Black (what the game uses) | paid, about $90 for that style |
| list and body text | Arsenal, 15% wider | OFL, Google Fonts |
| condensed headings | Anton, Bebas Neue | OFL |
| serif letters | Playfair Display | OFL |
| tarot-card lettering | Bubblegum Superstar | $20 commercial, no middle dot |
| time, deadline, actions | Helvetica Now Display, or Franklin Gothic | the free weight is for graphics only |
| main-menu letter mix | Cooper Black, ITC Avant Garde / TeX Gyre Adventor, Clarendon, Noto Sans / Serif (accents) | mixed |

## Files

```
Cargo.toml          # standalone crate, the same Rune rev as gui/; [workspace] keeps it out of the app's
src/main.rs         # the layout (window and component sheet), the Parts a direction fills in, scenes
src/common.rs       # palette, demo data, state, hover/press tracking; shapes: slant, jagged, inflate
                    #   (offset outlines), halftone, hatch, burst; the masked ghost; ransom()
src/attack.rs       # All-Out
src/card.rs         # an earlier direction, kept for reference until the theme is final
src/accent.rs       # an earlier direction, kept for reference until the theme is final
scripts/interact.json  # the GIF's pointer script
scripts/render.sh   # renders into out/ (gitignored)
```

Scenes: `attack` (window), `attack-parts` (component sheet) and `attack-letters` (nine ways to letter the buttons) and `attack-hovers` (ways to show an active button). The GIF's script also hovers Open
three times, so the random tilt shows.

Halftone, screentone and bursts are vector shapes (`canvas`), not shaders or images: crisp at any
scale, and no compositor is needed. A shader would only pay off for whole-window textures that
move.

## Rendering

```sh
design/persona5/scripts/render.sh attack         # into design/persona5/out/
# one still by hand (from design/persona5):
cargo build && target/debug/persona5-sketches --snap out/attack.png --scene attack --size 960x600 --scale 2 --at 1.5
```

A GIF takes about 4 minutes (315 frames at 2×). `--at 1.5` lets the appear motion finish
before the still. Delete `design/persona5/target/` when done: it grows to about 2 GB.
