# #417 Colors (branch 417-colors)

Headless renders (re-rendered after review round 1: no subtitle under Colors; after round 2: the picker placed for its tallest and drawn at its own height, the hex field short; the popup, two-theme and drag-after artifacts); round 3: no hairline around a section's or group's header in the two-theme lists, and every popup, two-theme and Every color still rendered again from it (`--at 0.5`, 2x unless noted) of the real build, Light and Dark system Appearance (the sheet shows both halves either way). Data and downloads in temp dirs.

- [sheet-light.png](sheet-light.png), [-dark](sheet-dark.png): the Active look's sheet: Look (Corners, Motion), Colors and the eight bases. 960x600.
- [narrow-sheet-light.png](narrow-sheet-light.png), [-dark](narrow-sheet-dark.png): the same at 640x600.
- [every-light.png](every-light.png), [-dark](every-dark.png): "Every color" open, 960x1500 at 1x. Faint text set by hand in Light, Muted text in Dark: each ringed in its half's accent.
- [narrow-every-light.png](narrow-every-light.png), [-dark](narrow-every-dark.png): the same at 640 wide ("Sheet and dialog" wraps).
- [popup-faint-light.png](popup-faint-light.png), [-dark](popup-faint-dark.png), [narrow 640 light](narrow-popup-faint-light.png), [dark](narrow-popup-faint-dark.png): the picker on a hand-set Faint text (#a3a6ab): two warnings, Follow Text.
- [popup-accent-light.png](popup-accent-light.png), [-dark](popup-accent-dark.png): the picker on a changed Accent (#6526d4): Reset, no warning (it passes 3:1): the popup as tall as what it shows.
- [tooltip-light.png](tooltip-light.png), [-dark](tooltip-dark.png): resting on Muted text's swatch: "#606369 · follows Text".
- [main-accent-light.png](main-accent-light.png), [-dark](main-accent-dark.png): the main window with the Accent changed (#c2410c Light, #fb923c Dark): bars, selection, tab mark.
- [themes-every-light.png](themes-every-light.png), [-dark](themes-every-dark.png): a different theme in each half (Phantom ☀, the stand-in ☾): Phantom's "Every color" opened from its chevron row, as with one theme.
- [drag-before-light.gif](drag-before-light.gif), [drag-after-light.gif](drag-after-light.gif): the same script, before round 1 and now: opening the Accent's picker, dragging in the square, then the saturation bar; the color applies while dragging (1x, pointer shown). Before, the popup grows and jumps above its swatch when the warnings appear; now its top stays level with the swatch and it grows downward.
- [drag-square-light.gif](drag-square-light.gif): the first version of the drag (the popup jumping), superseded by the pair above.
