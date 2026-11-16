# Persona 5 themes: the spec

What each element of Phantom's two Persona 5 themes looks like and how it moves, as settled on
PR #128 (issue #11):
- **Persona 5: Phantom Thieves**, the red, black and white theme after the game's menus;
- **Persona 5: Jose's shop**, the blue and yellow theme after the shop in the game.

In this text they're Phantom Thieves and Jose's shop.

The sketch in this folder draws both and is the reference: when this text and a render disagree,
the render wins. Run `scripts/render.sh` for the stills and clips, and see
[reference/](reference/) for the settled set.

Sizes are in points, at the sketch's 960 × 600 window: the toolbar is 34 high, a list row 36, the
status bar 26, the window padding 12. Times are in seconds.

## Shared by both themes

### State as eased amounts

Every interactive element is drawn from a few amounts, 0–1, each easing on its own spring toward
what the current state wants. It is never drawn per state. Any change of state then animates
from wherever the element is, and no transition is written for a pair of states.

| element | amounts | springs (ω rad/s, ζ) |
|---|---|---|
| button | hovered, pressed, disabled | hover in 17, 0.62 (a slight overshoot); hover out 16, 1; press 24, 1; disabled 13, 1 |
| list row | hovered (pressed counts), selected | hover 22, 1; selecting 20, 0.55 (overshoots); deselecting 30, 1 |

- **Never swap drawings:** an element keeps the same parts at every state, faded or moved by the
  amounts. Swapping one drawing for another shows as a cross-fade, a double image.
- **Never blend through low contrast:** where text and its ground swap colours (red letters on
  paper becoming paper letters on red), the change moves as a wipe across the element, each side
  in its own colours.

### Progress

- **The pill shows the report.** The percentage shows the client's latest number, unsmoothed.
- **The bar follows the reports smoothly:**
  - **Speed:** it estimates the speed as the median of the last three steps.
  - **Target:** it carries on at that speed for at most one report interval past the last report.
  - **Following:** it follows that target on a critically damped spring (ω 6), so a big jump or
    drop closes in about half a second by itself.
  - **Bound:** it never moves against the latest report's direction; when it gets ahead, it waits.
- **Any status, any value:** every status's bar works as a general bar from 0 to 100. At 0 a bar
  shows its smallest stub. Fetching is the exception: at 0 it means "no value yet", with "??" and
  no fill.

### The list

- **On open** the list is simply there: no cascade.
- **A torrent added** slides its row in while the rows below make room (0.25 s).
- **A torrent removed** slides its row out (0.3 s), then the rows below close the gap (0.25 s).
- **Scrolling:** rows fade out over their last 30 points at the list's top and bottom edges
  (Rune: `scroll_edge_fade(30.0)` with `scroll_margin`).
- **Multi-select:** each selected row gets its own band, the same as a single selection.

### Not yet designed

- A progress bar changing status (downloading to seeding, to error, to paused): today the pattern
  swaps at once.
- The footer's connection states beyond Jose's shop's moon.
- Errors: Phantom Thieves' errors panel is kept as a reference only; the main theme is being
  redesigned.

## Persona 5: Phantom Thieves

### Ground and window

- **Ground:** red (`#e60012`) with a slowly turning starburst of deeper red rays, and black
  halftone dots in the corners.
- **Window:** a rectangle with two cut corners (top-left and bottom-right); a fully slanted window
  was dropped because clicks on its clear corners don't pass through.
- **List sheet:** black (`#0b0b0b`) on a torn white paper panel with a fat black outline.

### Colours

| token | value |
|---|---|
| red | `#e60012` |
| deep red | `#a8000c` |
| black | `#0b0b0b` |
| paper | `#f4f2ec` |
| cyan | `#3fe0f0` |
| magenta | `#ff2bd6` |

### Type

- **Ransom lettering** for the title, buttons, column headings and the connection state:
  - **Capital:** the first letter in Superclarendon, larger (18).
  - **Lowercase:** at least one letter and at most half the word go lowercase, in Superclarendon
    14.5.
  - **The rest:** Arial Rounded MT Bold 14.
  - **Boxed:** one letter (never the first) is set inverted in a box.
- **List text:** torrent names and statuses in Optima, extra bold. Optima Nova Black is the game's
  list face.
- **Figures:** Avenir Next Condensed, tabular.

### Buttons

- **Rest:**
  - **Primary (Add):** a paper slab with red letters.
  - **Secondary:** a black slab with paper letters.
  - **Both:** cut on the slant, over a black shadow.
- **Hover:**
  - **Face:** the slab morphs into a living slab whose corners drift, 4 points in from the box.
    It keeps its own colours.
  - **Copies:** cyan and red copies slide out from behind it.
  - **Pose:** it swells to 108% and tilts, fresh each activation.
  - **Shadow:** fades out.
- **Press:** a red slab wipes in from the right over the face, the letters turning paper on its
  side of the wipe. The button shrinks to 92%. Release wipes it back out to the right.
- **Disabled:** the face and rim fade to deep red, a hatch fades in and the shadow fades out. The
  ransom letters stay, dimmed to `#d2505a`.

### List

- **Rows:** step right at random per row (7 points, never close to the row above).
- **Hover:** a red shard points at the row, a red zigzag runs under it, and the row steps in 8
  points. All fade with the hover.
- **Selection:**
  - **Band:** a red band with a wedge past the list's left edge, cut fresh for each selection.
    It slams in from the left with an overshoot and, deselected, draws back to the left.
  - **Sliver:** a cyan sliver under its right part stretches with the band, so it comes and goes
    with it.
  - **Name:** one letter of the selected name is inverted.
- **Pills:** percentages in slanted paper pills; on the selection, deep red with a cyan sliver.
- **Speeds:** black figures on a living slab, cyan for down, magenta for up.
- **Rows added:** slam in from the right with an overshoot (0.38 s).

### Progress bars

The track is black, level at the top and bottom with leaning sides, and its patterns run straight.
A paper shadow sits 2.5 points down and right: red for an error, none while seeding. Every fill
runs past the end at 100 and is cut flush by the track's edge, so 100 lines up with the shadow.

| status | fill |
|---|---|
| Downloading | a red arrowhead with paper stripes racing through it (26 pt/s); never drawn smaller than at 15%, so a stripe always shows |
| Seeding | paper with red chevrons marching out (10 pt/s); a chevron at the fill's end is cut by it |
| Checking | paper halftone, the dots growing towards the front, a red slit scanning across; a slanted front edge |
| Paused | frozen grey blocks (`#9a9a9a`); the pill's number grey |
| Fetching | a paper shard sliding to and fro over the whole bar; with progress, a pale paper fill (55%) under it |
| Error | a torn red edge with a bolt just past it, the bar twitching now and then; near the end the edge stops 20 points short and the bolt itself stretches right until the red fills the bar |

### Status bar

The ghost mask sits by "Connected" on a black strip, with the totals on the right.

## Persona 5: Jose's shop

### Ground

- **Sky:**
  - **Base:** deep blue bands.
  - **Nebula:** a generated texture (domain-warped fractal noise with dark dust lanes and bright
    filaments, in the sky's blues) at 55%. It cross-fades between two seeds over 90 s, over soft
    metaball clouds. A new session takes a new seed.
- **Stars:** 2,400 soft stars, most faint, twinkling slowly; a few bright ones in wide halos.
- **Comets:** six arcs sweeping from the lower left.
- **The heap:** 140 lemon moons and stars drifting right along the bottom at 21 pt/s.
  - **Placement:** evenly spaced, each in its own shade within half the spread.
  - **Glow:** a tight, faint glow.
  - **Glints:** 90 white glints twinkling among them, any of which flashes a four-point sparkle
    every 6–10 s.

### Colours

| token | value |
|---|---|
| lemon | `#fbf25a` |
| lemon glow | `#efe02a` |
| pale lemon | `#fbf7a6` |
| sizes and secondary text | `#f3eeb6` |
| cream (tape, footer) | `#fff6c8` |
| deep navy (glass) | `#020a2c` |
| navy | `#0b1f5c` |
| seeding cyan | `#7fe3ff` |
| error | `#ff5a6a` |
| logo green core | `#e6f8d4` |

### Type

SF Compact Rounded throughout. The column headings are hand-set capitals.

### Toolbar

- **Paste field:** a strip of torn tape in cream (`#fff6c8`) at 93%, soft shadow, navy hint text.
- **Buttons:** a board, title case.
  - **Primary (Add):** a cream rim (3.2 points) round deep-navy glass at 58%.
  - **Secondary (the rest):** the glass alone; the rim comes in with the hover.
  - **Hover:** a lemon sheet morphs in over the board, its corners wandering. The glass fades out,
    the rim turns black where it crosses the sheet, the label goes black, and the board lifts and
    swells to 106%.
  - **Press:** the board shrinks to 94% and presses in.
  - **Disabled:** the board, primary or secondary, at 40%.
- **Logo:** "Phantom" in bouncing, glowing green letters, drawn under the toolbar so a hovered
  button passes over it.

### List

- **Card:** fading smoke (about 78% at the bottom) with a lemon rim.
- **Header:** a lemon strip.
- **Rows:** sizes in pale lemon; percentages as bare numbers.
- **Selection:** an opaque lemon flag with a swallowtail at the left and swaying edges, glowing.
  - **Inset:** the row's text moves in 8 points.
  - **Name:** plain black.
  - **Pale sliver:** a pale-lemon sliver sits under its right part.
- **Hover:** the selection's own flag at a quarter strength, without the sliver; it stays through
  the press.
- **Select:** the flag fades in over the hover flag.
- **Deselect:** the flag fades out. The name returns to its pale ink once the flag is half gone.
- **Rows added:** fade in with a short drift (28 points).

### Progress bars

A rounded, glowing track: navy with a pale-lemon rim; on the selection, pale cream with a navy
rim.

| status | fill |
|---|---|
| Downloading | lemon (navy on the selection) with small stars |
| Seeding | cyan (`#7fe3ff`) with stars rising through it |
| Checking | pale lemon with small stars growing along it, a brighter wave passing; a star grows in as the fill reaches it |
| Paused | dim lemon crescents, each growing in as the fill reaches it; tiny z's rising from every other moon, or one pair at the start before any moon |
| Fetching | small stars along the track lighting one after another; with progress, a soft lemon fill (40%) under them with a gentler glow |
| Error | red (`#ff5a6a`) with navy hazard stripes; every 2.5 s the bar twitches up and down for about 0.1 s |

### Footer

- **Flag:** a cream frosted flag (`#fff6c8`, 78%) with a gentle sway.
- **Moon:** the connection state, waxing from new to full over 2.5 s while connecting and dropping
  straight to new (0.52 s) when the connection drops.
