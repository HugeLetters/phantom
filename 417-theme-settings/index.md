# #417 prototype: Phantom theme settings, v3 (stills)

Headless renders (`--at 0.5`, 2x unless noted) of a throwaway build (branch `417-theme-settings`). v3 stills are prefixed `v3-`; v2 and v1 lines stay below. Each still is in Light and Dark (the system's Appearance; the sheet shows both halves either way).

## v3: Settings sheet
- [v3-sheet-look-colors-light.png](v3-sheet-look-colors-light.png), [-dark](v3-sheet-look-colors-dark.png): Look (Corners, Density, Motion as a dropdown "Follow Reduce Motion") and Colors ("The rest follow these."). No rule above or below a section header; rows keep theirs.
- [v3-sheet-every-color-light.png](v3-sheet-every-color-light.png), [-dark](v3-sheet-every-color-dark.png): "Every color" open (1x, tall window). Faint text is set by hand: a dot on its swatch. "Accent text" is gone (Phantom draws none).
- [v3-popup-faint-light.png](v3-popup-faint-light.png), [-dark](v3-popup-faint-dark.png): the popup next to Faint text's swatch (Light, then Dark): saturation/brightness square, hue strip, hex field, "Follow Text", and the warnings under them: "Hard to read on Surface: 3.8:1 (needs 4.5:1)." (Dark's hand-set faint reads 2.5:1.)
- [v3-popup-accent-light.png](v3-popup-accent-light.png), [-dark](v3-popup-accent-dark.png): the popup on a hand-set Accent: "Reset" instead of Follow; no warning, it passes 3:1.
- [v3-tooltip-light.png](v3-tooltip-light.png), [-dark](v3-tooltip-dark.png): the swatch tooltip, "#4392f7 · theme default" (derived colors say "· follows Text").

## v3: Smallest window (640 wide)
Before = the v2 build (segmented controls cut: "Sq…", "Ro…", "Comfor…", Motion radios cut). After = segmented controls whose labels don't fit become dropdowns; nothing is cut.
- 640x400: [before light](v3-narrow-640x400-before-light.png), [after light](v3-narrow-640x400-after-light.png), [before dark](v3-narrow-640x400-before-dark.png), [after dark](v3-narrow-640x400-after-dark.png)
- 640x600: [before light](v3-narrow-640x600-before-light.png), [after light](v3-narrow-640x600-after-light.png), [before dark](v3-narrow-640x600-before-dark.png), [after dark](v3-narrow-640x600-after-dark.png)

Faked: the square and hue strip are drawn with real gradient fills and markers placed from the colour's real hue/saturation/brightness, but they don't drag yet; the hex field is a static view. The popup opens from a still's setting, not a click (a click on a swatch does open it).

---

Headless renders of a throwaway build (branch `417-theme-settings`). v2 stills are prefixed `v2-`; the v1 stills and their notes stay below, unchanged.

## v2: Settings sheet
- [v2-sheet-look-colours.png](v2-sheet-look-colours.png): the sheet closed to Look (Corners, Density, Motion as three radios) and Colours (eight swatches, "The rest follow these."), "Every colour" closed. The Accent swatch is pressed: its editor shows the hex field and "Phantom's colour"; the Light Accent warning is one line.
- [v2-sheet-every-colour.png](v2-sheet-every-colour.png): "Every colour" open, five groups with the same header style. Faint text is set by hand in Light: a dot on its swatch, two warnings of one line each, and its editor open (hex, "Set by hand", Follow). Pointer resting on Window in Dark shows its tooltip ("#121314 · follows Background").
- [v2-sheet-swatch-tooltip.png](v2-sheet-swatch-tooltip.png): the sheet closed again, tooltip on the Dark Accent swatch (hex and what it follows).

## v2: Motion
- [v2-motion-settings-choices.gif](v2-motion-settings-choices.gif): Settings, then Appearance, then Edit… opens the sheet, four panels, each labelled: Off (lands at once), On (fades), Follow Reduced Motion with the system's Reduce Motion off (acts as On), Follow Reduced Motion with it on (acts as Off).

## v2: Colour sets (main window, notices popover, clash dialog with tooltip)
Three panes each, left to right: list with files, popover open, dialog with its tooltip.
- [v2-colours-today-light.png](v2-colours-today-light.png), [v2-colours-today-dark.png](v2-colours-today-dark.png): today's bases.
- [v2-colours-warm-light.png](v2-colours-warm-light.png), [v2-colours-warm-dark.png](v2-colours-warm-dark.png): warm set.
- [v2-colours-bold-light.png](v2-colours-bold-light.png), [v2-colours-bold-dark.png](v2-colours-bold-dark.png): bold set (violet).

Corners and Density are unchanged from v1 (below).

---

# v1

Headless renders of a throwaway build (branch `417-theme-settings`). Each colour and corner still has three panes, left to right: the list (pointer over archlinux, Big Buck Bunny selected, its files), the notices popover, and the clash dialog with a tooltip.

## Colours
- [colours-today-light.png](colours-today-light.png): today's eight bases through the derivation, Light.
- [colours-today-dark.png](colours-today-dark.png): today's bases, Dark.
- [colours-warm-light.png](colours-warm-light.png): warm set (cream, brown text, burnt-orange accent), Light.
- [colours-warm-dark.png](colours-warm-dark.png): warm set, Dark.
- [colours-bold-light.png](colours-bold-light.png): bold accent set (violet), Light.
- [colours-bold-dark.png](colours-bold-dark.png): bold accent set, Dark.
- [sheet-bases.png](sheet-bases.png): the Active look's sheet with Corners, Density, Motion and the eight bases per half; a too-low pair shows as a warning under its row.
- [sheet-every-colour.png](sheet-every-colour.png): "Show every colour" on: 22 derived colours in groups, each with its value and base. Faint text is set by hand in Light (a field and Follow Text), and the sheet warns that it reads 3.8:1.

## Corners (Dark)
- [corners-square-dark.png](corners-square-dark.png): every radius 0.
- [corners-small-dark.png](corners-small-dark.png): today's radii (2/4/6/8/10).
- [corners-round-dark.png](corners-round-dark.png): 4/7/10/14/18.

## Density (Light)
- [density-comfortable-vs-compact.png](density-comfortable-vs-compact.png): Comfortable (today, left) and Compact (right), with the same rows. Compact has rows of 22 and a header of 24 (28/30 today), a toolbar of 32 (40), a margin of 8 (12).

## Motion
- [motion-notices-on-vs-off.gif](motion-notices-on-vs-off.gif): the notices popover opens, pages and closes. On is left, off is right. The popover has no motion today, so the two halves match.
- [motion-settings-on-vs-off.gif](motion-settings-on-vs-off.gif): Settings tabs, then Edit… opens the sheet. With motion on (left) the tab pill slides and the sheet fades in. With it off (right) both land at once.

## Text contrast (WCAG 2, derived)
Muted and faint text are derived so they stay at 4.5:1 or more on the window, the surface and a selected row. Text on the accent is black or white, whichever reads better. Ratios under 4.5 are in bold.

| Set | Tone | text/surface | muted/window | muted/selected row | faint/window | accent as text | text on accent | danger, warning, success on their fill |
|---|---|---|---|---|---|---|---|---|
| today | Light | 17.1 | 5.0 | 4.7 | 4.5 | **4.0** | black 5.3 | 5.4, 5.1, 4.7 |
| today | Dark | 14.8 | 8.0 | 5.8 | 5.9 | 5.4 | black 6.7 | 5.7, 5.9, 6.3 |
| warm | Light | 15.4 | 5.1 | 4.6 | 4.6 | 5.0 | white 5.1 | 5.3, 5.2, 5.0 |
| warm | Dark | 14.3 | 7.7 | 5.3 | 5.7 | 7.1 | black 8.8 | 6.1, 6.3, 6.6 |
| bold | Light | 18.3 | 5.5 | 4.7 | 4.5 | 7.6 | white 7.6 | 5.0, 5.2, 4.6 |
| bold | Dark | 16.0 | 8.2 | 5.9 | 6.0 | 6.8 | black 7.8 | 6.0, 7.7, 7.8 |

Today's palette misses 4.5 in two places, both in Light: faint text (muted at 80%) reads 3.8 on the surface and 3.4 on the window, and accent `#2f7fe3` reads 4.0 as text or under white text. The derivation fixes faint text. The accent is the user's pick, so the sheet warns about it instead of changing it.
