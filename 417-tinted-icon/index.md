# #417 tinted app icon: mockups

**Mockups drawn outside the app** (a throwaway Swift/AppKit script redraws the default Phantom ghost, `design/icons/round10/74-neutral-soft-35.svg`, with other colours). Nothing in the app or repo changed. Colour sets are those of the `417-theme-settings` prototype.

Mappings (base colours of the set, per tone):
- **A** tile = Surface (top) to Background (bottom) gradient; ghost = Text; eyes = Accent.
- **B** tile = Accent gradient (+10% white at top, darkened 25% Light / 45% Dark at bottom); ghost = Surface in Light, Text in Dark; eyes = Accent darkened (55% Light / 60% Dark).

- [dock-B.png](dock-B.png): mapping B, six icons at 128px on a light and a dark desktop strip, current fixed icon beside them.
- [dock-A.png](dock-A.png): the same for mapping A.
- `icon-B-<set>-<tone>.png`, `icon-A-<set>-<tone>.png` (set: today, warm, bold; tone: light, dark): the twelve 1024px icons. [icon-current.png](icon-current.png) is today's fixed icon.
- [look-B-light.png](look-B-light.png), [look-B-dark.png](look-B-dark.png): the sheet's "Look" section with an Icon row (Phantom / Tinted, Tinted selected), Warm set, mapping B.
- [look-A-light.png](look-A-light.png), [look-A-dark.png](look-A-dark.png): the same with mapping A.
