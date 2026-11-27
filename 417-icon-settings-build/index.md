# #417 App icon settings: built (draft PR, branch `417-icon-settings-build`)

The real Settings window, headless, 960x600 @2x unless noted, `--at 0.5`, in Light and Dark system Appearance. Nothing faked: every tinted icon is drawn from `design/icons/picks/phantom.svg` with its Tile color and rendered by resvg (Rune's `ImageCache::svg`).

**App icon row.** Customize… shows beside Edit… while the icon shown has settings (Phantom's).

- Default (Original): ![](settings-icon-customize-row-light.png) ![](settings-icon-customize-row-dark.png)
- Light following the look, Dark a blue set by hand: ![](settings-icon-row-tinted-light.png) ![](settings-icon-row-tinted-dark.png)

**Customize sheet** (Light and Dark columns, the icon each half draws, then a Tile swatch per half).

- As it opens, Original in both: ![](settings-customize-light.png) ![](settings-customize-dark.png); at 640: ![](settings-customize-640-light.png)
- The picker open on Light's Tile, with Original (in use, off) and Follow the look: ![](settings-customize-picker-light.png) ![](settings-customize-picker-dark.png)
- Light following the look, Dark set by hand (accent ring), Dark's picker open: ![](settings-customize-follow-light.png) ![](settings-customize-follow-dark.png); at 640: ![](settings-customize-follow-640-light.png)
- Dark look another theme's (the debug stand-in): Dark's Follow the look is off, why on hover: ![](settings-customize-other-light.png) ![](settings-customize-other-dark.png); at 640: ![](settings-customize-other-640-dark.png)
- The tile at a few colors set by hand, both halves: orange ![](settings-customize-orange-light.png) ![](settings-customize-orange-dark.png), purple ![](settings-customize-purple-light.png) ![](settings-customize-purple-dark.png), green ![](settings-customize-green-light.png) ![](settings-customize-green-dark.png)

At 640 wide, where the picker's 360 pt fit on neither side of a swatch, it opens below the swatch, or above it when there is no room below, shifted to stay inside the window; nothing is cut. The two 640 stills with a picker were rendered again (?v=2).

**At the 640x320 minimum**, the picker open (Light and Dark system Appearance): Dark's Tile ([light](min-tile-picker-light.png), [dark](min-tile-picker-dark.png)), and a hand-set Faint text in Colors with two warnings ([light](min-faint-picker-light.png), [dark](min-faint-picker-dark.png)). With no room beside, below or above the swatch, the picker takes the window's middle.
