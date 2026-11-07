# Phantom app icons

The app icon design is done. What ships:
- **Picks**: the 14 final icons, with names and ids (below), in `picks/<id>.svg`.
- **Pairs and theme icons**: the designed light/dark pairs and each theme's own icon.
- **`app/`**: an `.icns` per pick and `app-icons.json`, the manifest the app reads.

`overview-picks.png` shows every pick and pair. The rest of this file is the design history:
rounds 1–11 of concepts and feedback, numbered so designs can be referred to by number
(`NN-name.svg` with its 1024 px render `NN-name.png`, in this folder for rounds 1–4 and in
`roundN/` from round 5). Overview sheets show each design large with its 64 px and 32 px
versions beneath; 32 px is roughly how the icon appears in Finder lists and Raycast.

## Picks

A **pick** is a final icon that definitely ships. Every pick is a choice in the app's icon picker,
on its own and as part of any pair it's in. Picks are copied into `picks/` under their id
(`picks/<id>.svg` and `.png`); `overview-picks.png` (rendered by `python3 src/picks_sheet.py`)
shows them all, then the designed pairs. Their sources stay in the round folders and scripts
under their design number, so never change a picked design in place: iterate on a copy under a
new number instead. Numbers stay design history only; the app sees ids and names.

| # | Name | id | Theme | Source | Notes |
|---|---|---|---|---|---|
| 74 | Phantom | `phantom` | Plain | `round10/74-neutral-soft-35.svg`, `src/r10_neutral.py` | **Default icon**: a flat `#ededed` arcade-style ghost with oval eyes, a softly rounded hem of three scallops (about 35 px) and rounded corners, on a `#2a2a2c`→`#0a0a0a` gradient |
| 36 | Dark Hour | `dark-hour` | Persona 3 Reload | `round6/36-hour-shadow.svg`, `src/r6_p3.py` | A dark draped Shadow with a green sheen and a teal oval mask in front of a green moon |
| 37 | Full Moon | `full-moon` | Persona 3 Reload | `round6/37-blue-moon-shadow.svg`, `src/r6_p3.py` | The same Shadow with a blue sheen in front of a pale blue moon on a night-blue sky |
| 79 | Memento Mori (Dark Hour) | `memento-mori-dark-hour` | Persona 3 Reload | `round11/79-keyart-lime-wedge.svg`, `src/r11_p3.py` | 40 at the Dark Hour: dark green sky, lime wedge and shards, green sheen |
| 40 | Memento Mori | `memento-mori` | Persona 3 Reload | `round6/40-keyart-shadow.svg`, `src/r6_p3.py` | Key art: the Shadow on flat azure with a black diagonal wedge and glass shards |
| 80 | Sea of Souls (Dark Hour) | `sea-of-souls-dark-hour` | Persona 3 Reload | `round11/80-water-dark-hour.svg`, `src/r11_p3.py` | 55 at the Dark Hour: green-black water with lime caustics |
| 55 | Sea of Souls | `sea-of-souls` | Persona 3 Reload | `round7/55-shadow-dark-water.svg`, `src/r7_p3.py` | The Shadow under water, with caustic light at the surface |
| 46 | Phantom Thief | `phantom-thief` | Persona 5 | `round7/46-mid-mask-stars.svg`, `src/r7_mask.py` | The white ghost in the black `MASK7` domino mask over a black starburst on the tonal red star pattern |
| 82 | Phantom Thief (Alt) | `phantom-thief-alt` | Persona 5 | `round11/82-mask-stars-deep-red.svg`, `src/r11_p5.py` | 46 in deep reds, black burst |
| 87 | Phantom Thief (Alt 2) | `phantom-thief-alt-2` | Persona 5 | `round11/87-mask-stars-black-mid-burst.svg`, `src/r11_p5.py` | 46 on near-black stars with a mid-red burst |
| 51 | Calling Card | `calling-card` | Persona 5 | `round7/51-card-mid-mask-stars.svg`, `src/r7_mask.py` | A red card printed with the star pattern, the masked white ghost breaking out of it, on black |
| 50 | Calling Card (Alt) | `calling-card-alt` | Persona 5 | `round7/50-card-mid-mask.svg`, `src/r7_mask.py` | 51 without the star print |
| 84 | Calling Card (Alt 2) | `calling-card-alt-2` | Persona 5 | `round11/84-card-deep-red.svg`, `src/r11_p5.py` | 51 with a deep-red card and darker print |
| 85 | Black Mask | `black-mask` | Persona 5 | `round11/85-card-black-on-red.svg`, `src/r11_p5.py` | 51's colours swapped: a black card with a white edge on red |

The ghost is never red in the Persona 5 icons.

## Pairs and theme icons

A **designed pair** is a dark icon and a light icon that swap with the system Appearance; the
picker shows it as one choice, named "‹dark› / ‹light›" (both names, since paired icons needn't
share one). It's stored exactly like a custom pair the user makes: just its two ids.

| Pair (dark / light) | # |
|---|---|
| Dark Hour / Full Moon | 36 / 37 |
| Memento Mori (Dark Hour) / Memento Mori | 79 / 40 |
| Sea of Souls (Dark Hour) / Sea of Souls | 80 / 55 |
| Phantom Thief (Alt) / Phantom Thief | 82 / 46 |
| Phantom Thief (Alt 2) / Phantom Thief | 87 / 46 |
| Calling Card / Black Mask | 51 / 85 |

Each theme's own icon, which "Follow theme" uses:

| Theme | Icon |
|---|---|
| Plain (`plain`) | Phantom |
| Persona 3 Reload (`persona-3-reload`) | Dark Hour / Full Moon |
| Persona 5 (`persona-5`) | Calling Card / Black Mask |

Phantom (74) is the only icon compiled into the bundle, so it's the only one macOS 26 restyles
for its dark and tinted looks; it ships as one icon with no light partner. Whether Persona themes
should follow the Appearance through their palettes (and then the icon with them) is open in
[#34](https://github.com/HugeLetters/phantom/issues/34).

## App icons (`app/`)

What the app ships, all generated by `python3 src/app_icons.py` from the table at its top (ids,
names, themes, pairs, theme icons):

- `app/<id>.icns`, one per pick: every iconset size (16, 32, 128, 256 and 512 px, each also at
  @2x, so up to 1024 px) rendered from `picks/<id>.svg`, packed with `iconutil -c icns`.
- `app/app-icons.json`, the manifest the app reads:

```json
{
  "default": "phantom",
  "icons": [
    { "id": "phantom", "name": "Phantom", "theme": "plain", "file": "phantom.icns" },
    …
  ],
  "pairs": [
    { "name": "Dark Hour / Full Moon", "dark": "dark-hour", "light": "full-moon" },
    …
  ],
  "themes": {
    "plain": { "icon": "phantom" },
    "persona-3-reload": { "dark": "dark-hour", "light": "full-moon" },
    "persona-5": { "dark": "calling-card", "light": "black-mask" }
  }
}
```

  `default` is the bundle's own icon (the app touches nothing when it's chosen). `icons` is in
  picker order; `file` is relative to `app/`. A theme maps to one `icon` or a `dark`/`light`
  pair.
- `src/iter/app-check.png` (not committed) unpacks each `.icns` again and shows its 16 and 32 px
  images on light and dark, so the check is of what ships. All fourteen hold at 32 px; at 16 px
  the Persona 3 masks shrink to a teal dot and 84's dark card nearly vanishes, but the figure and
  the colours still read.

To change the set, edit the table in `app_icons.py` (and copy any new pick into `picks/<id>.*`),
then rerun it and `picks_sheet.py`.

## Shortlist (closed)

Picking finished after round 9; the result is the Picks table above. Not picked, but kept in their
round folders: 09 card, 10 card-stars (50 and 51 replaced them), 63 keyhole-bigger-eyes, 64
keyhole-bigger-eyes-black. The one-burning-eye idea was dropped after scripted attempts (see
Round 9) and outside image generators both fell short.

## Brief

- The name comes from the "phantom thieves" idea. The UI will later take visual cues from
  Persona 5, and maybe Persona 3 Reload through a theme switch, so Persona 5's style is the main
  inspiration. Ghost and phantom themes in general also fit.
- Designs must be original. Take only general style cues from Persona 5: red (`#e60012`),
  black and white, high contrast, jagged cut-out and torn-paper shapes, halftone dots, slanted
  angles, thief motifs. Don't reproduce the Phantom Thieves emblem, the game's calling card,
  its characters, masks or title lettering.
- Follow the macOS icon grid: an 824 px rounded square with corners of about 185 px, centred
  on a 1024 px canvas and transparent outside it, with one simple symbol that still reads at
  32 px.

## Concepts

| # | Name | Idea | At 32 px | Verdict |
|---|---|---|---|---|
| 01 | masked-ghost | White ghost with a jagged hem in a black domino mask, over a black starburst on red with halftone corners | The ghost and mask band read; the starburst becomes a dark smudge | Lively large, but busy |
| 02 | calling-card | Tilted white card with a torn, ghost-like bottom edge and a toothy sly grin, a red slash and halftone on black | Very good: card and grin stay crisp | **Top pick**: most legible, most "phantom thieves" |
| 03 | invisible-thief | Top hat and domino mask floating over a body that dissolves into halftone | Fair: shrinks to "little hat on red" | **Rule out**: too close to the Phantom Thieves emblem, and weakest small |
| 04 | download-ghost | Red ghost whose lower half is a download arrowhead, white cut-out offset, halftone | Very good: bold shape, eyes still visible | The only one that hints at downloading; looks a bit like a red hood |
| 05 | shattered-p | Bold angular "P" sliced in two and offset, on torn red paper over black | Very good: the P survives | Readable but generic: a letter, not an idea |
| 06 | swarm-ghost | Ghost outline made of linked nodes (BitTorrent peers), neon red on black | Weak: a reddish blob | Better as an in-app illustration (for example the empty state) |
| 07 | moonlit-ghost | Persona 3-style variant: dark ghost with a cyan rim and glowing eyes against a pale moon, navy | Good: moon and silhouette contrast; eyes fade | Alternate icon for a future Persona 3 theme |
| 08 | keyhole-ghost | Black keyhole whose flared bottom ends in a ghost's ragged hem, sly eyes, white cut-out offset on red | Good: silhouette very recognisable; eyes barely there | Most original idea: ghost and thief in one shape |

## Round 2

Feedback on round 1: keep 01 but make the mask more angular, and show it at icon size. Give 02
the ghost from 01. Try 04 in one palette. Push 07 toward the Persona 3 Reload key art: flat
azure, black, white and grey cel shading, black ribbon strands, cyan glass shards. Vary 08's
proportions. 03, 05 and 06 are dropped.

`overview-round2.png` shows all of round 2 the same way as `overview.png`. Variants keep their
parent's number plus a letter. The 01 ghost and the faceted mask from 01b live in `lib.py`
(`GHOST`, `MASK`, `MASK_EYE_L`, `MASK_EYE_R`), so 01b, 02b and 07d stay in sync.

| # | Name | Change | At 32 px |
|---|---|---|---|
| 01b | masked-ghost-angular | Mask redrawn from straight cuts: wings swept up to points, a notch at the bridge, slanted trapezoid eye holes. Mask band thickened so it survives small sizes | The mask reads as pointed cat-eye glasses; the starburst turns to noise |
| 01b small | masked-ghost-angular-small | Same figure without starburst or halftone, scaled up, for the 16 and 32 px slots of the `.icns` | Cleaner than 01b at 16 and 32 px |
| 02b | calling-card-ghost | Card now carries the 01 ghost in black with the angular mask in red, outlined in black so it joins the body | Very good: white card, black ghost, red mask band |
| 04b | download-ghost-red | White arrow ghost on red, black offset shadow and black angular eyes (01's palette) | Very good |
| 04c | download-ghost-black | The same on black, with a red shadow and red eyes | Very good |
| 07d | masked-ghost-p3 | 01b redrawn in the P3R style: azure background, grey cel shade, black ribbon strands and glass shards instead of the starburst and halftone | Good. The natural theme-switch twin of 01b |
| 07e | sinking-ghost | 01's ghost (without the mask) in cel-shaded white sinking through water: cyan surface with caustic light, deepening to ultramarine | Good: a white ghost on blue; the caustics fade out |
| 08b | keyhole-ghost-bighead | Oversized head with big eyes, short stubby skirt | Best of the 08s at 32 px: the eyes stay visible |
| 08c | keyhole-ghost-tall | Small head, long slender body | Weak: thin, and the eyes vanish |
| 08d | keyhole-ghost-bell | Medium head, wide bell-shaped hem like a cape | Good: a bold triangle with a head |

Tried and dropped for 07: an upside-down "falling" ghost (it stops reading as a ghost and looks
like a comet), and a ghost with a black fringe over one eye (the fringe read as a penguin cap or
claw marks, and it leans too close to the Persona 3 protagonist).

## Round 3

Feedback on round 2:
- 01b: the mask has too many angles. Keep the halftone dots, which read as shading at small sizes.
- 01b-small: rejected, it looks empty without the dots. Try other dot scales instead.
- 02b: disliked. The card and ghost sit square to each other; try a red card with a white ghost.
- 04b: drop the black offset shadow and make the eyes less angry. 04c is worse than 04b.
- 07d: promising, but needs the same simpler mask, and something other than the black strands.
  A gradient background is worth trying. 07e: the ghost doesn't look Persona 3 at all.
- 08b: liked. Calmer eyes, a wider neck, maybe the 01 burst behind it. 08c and 08d are rejected;
  08 itself is better than either.

From round 3 on, every variant gets its own letter (round 2's "01b small" broke that rule).

`overview-round3.png` shows round 3.

The simpler mask is `MASK2` / `MASK2_EYE_L` / `MASK2_EYE_R` in `lib.py`: one point per wing, a
soft dip at the bridge, rounded lens bottoms. The calmer eyes have an almost level top lid and a
rounded bottom.

| # | Name | Change | At 32 px |
|---|---|---|---|
| 01c | masked-ghost-simple-glasses | 01b with the simpler mask, original dot scale | Mask reads; dots blur into shading |
| 01d | masked-ghost-medium-dots | 01c with dots about 1.5× bigger and further apart | Dots start to show as dots |
| 01e | masked-ghost-big-dots | 01c with dots about 2× bigger | Dots still read as dots |
| 02c | red-card-white-ghost | Red card with a white offset edge, tilted left. The white masked ghost tilts right and breaks out over the card's top edge, on black with grey halftone | Red card and white ghost stay separate; the mask is small |
| 04d | download-ghost-burst | White arrow ghost on red, no offset shadow, 01's black starburst behind, calmer eyes | Very good |
| 04e | download-ghost-plain | The same without the starburst | Very good, quieter |
| 07f | p3-gradient-wedge | 07d's cel-shaded ghost with the simpler mask, on an azure-to-ultramarine gradient. A black diagonal wedge (from the key art) replaces the strands | Good: the white hem against the black wedge stands out |
| 07g | p3-moon-window | Ghost as a flat cyan silhouette with a navy mask, in front of a big mottled moon behind dark window bars, on a night-blue gradient | Good: moon and bars read; the ghost is lower contrast than the white ones |
| 07h | p3-underwater | The white ghost sinking under a water surface: soft caustic blobs, faint light shafts, cyan-to-ultramarine | Good; the caustics become a light band |
| 08e | bighead-wide-neck | 08b with a much wider neck (less keyhole, more ghost) and calmer eyes | Very good |
| 08f | bighead-wide-neck-burst | 08e over 01's starburst; a full white cut-out outline keeps the black ghost from merging with the black burst | Good, busier |

## Round 4

Feedback on round 3:
- 01c: the rounded mask is out. It should be angular (straight cuts, no curves) but simple.
- 01d/01e: bigger dots look odd. Try P5-style backgrounds instead: concentric star outlines,
  rings, ink splatter.
- 02c: more fun, but the card has too much empty space; fill it or make the ghost bigger.
- 04d: smaller burst, tilted ghost, angular (not rounded) eyes, and fill the empty arrowhead.
  Iterate 04d over 04e.
- 07f: the background and the cel shadow are good, but the ghost and especially the mask look
  like Persona 5. Make the ghost more Persona 3, perhaps borrowing from the Shadow enemies.
- 07g: looks like Flash-game art; dropped. 07h: nice, but off-centre; mix 07f and 07h.
- 08e: very nice but off-centre. 08f: burst too big and outline too wide; maybe swap colours.

Reference images are saved in `refs/` (gitignored, see `refs/README.md`). From this round on,
the overview shows the 64 and 32 px renders at 2× with nearest-neighbour scaling, so the
pixels are visible.

New shapes in `lib.py`:
- `MASK3`: a flat domino mask, six straight cuts per side, with triangle eyes.
- `MASK4`: a slanted blindfold band.
- `SHADOW`: a draped, hooded ghost with cloak arms and trailing wisps.
- `OVAL_MASK`, `OVAL_EYES`, `OVAL_MOUTH`: the Persona 3 face mask.
- Background helpers `stars`, `rings`, `splatter` and `smooth`.

| # | Name | Change | At 32 px |
|---|---|---|---|
| 01f | flat-mask | 01 with `MASK3`: straight cuts only, triangle eyes, on the original fine dots | Mask reads as a sharp black band |
| 01g | blindfold-mask | 01 with `MASK4`, one slanted band | Reads as a censor bar; weakest |
| 01h | star-pattern | 01f with a tonal red concentric-star pattern instead of dots | Pattern becomes soft tonal shading |
| 01i | dark-stars | Star pattern in black and dark red, with the burst turned red | High contrast; busier |
| 01j | rings-splatter | Tonal red rings plus a few ink splats | Rings disappear, so it reads as plain red |
| 01k | ink-splatter | Ink splats instead of dots | Splats become dark specks |
| 02d | card-big-ghost | Bigger ghost covering most of the card, card visible down the left and bottom | Ghost dominates; card is a red edge |
| 02e | card-star-print | Card printed with the tonal star pattern, ghost breaking out at the top right | Similar to 02d |
| 04g | download-ghost-shaded | Smaller burst, ghost tilted -10°, angular calm eyes, hard grey cel shadow on half the arrowhead | Good; the shade gives it volume |
| 07i | shadow-mask-white | New Shadow-style ghost in cel-shaded white with a teal oval mask, round eyes and a flat mouth, centred on 07f's gradient and wedge | Teal face pops |
| 07j | shadow-dark-underwater | The same in black with a navy cel shade and cyan rim, under water with a smaller wedge | Good: dark figure, teal face |
| 07k | shadow-white-underwater | White Shadow centred in 07h's water | Good |
| 07l | shadow-dark-hour | Dark Shadow against a green "Dark Hour" moon | Moon and face read |
| 08g | bighead-centred | 08e centred on its real rotated bounding box | Very good |
| 08h | bighead-small-burst | Smaller burst behind the head, thinner white outline | Good |
| 08i | bighead-white-burst | Colours swapped: white ghost, black eyes, black cut-out, over the smaller burst | Very good |

Dropped this round: a ragged-edged arrowhead for 04 (it read as stair steps), and a white bar
under 04's arrow tip (clutter).

## Round 5: names reset

From this round on, designs live in `round5/` with fresh numbers, `01`–`24`; old names stay valid
for the old files. Earlier files are never deleted, so you can go back to them.

There are two sheets: `overview-round5-red.png` (masked ghost, card, download, keyhole) and
`overview-round5-p3.png` (the Persona 3 set). Each family row opens with the earlier versions it
builds on, on a darker panel labelled "was 01f" and so on, followed by the new designs. Main
previews are 30% bigger than round 4 (390 px), the 64/32 px renders are shown at 2×, and the type
is twice the size.

Feedback on round 4:
- 01f: nice, but the eyes are a bit big; they nearly touch the bottom of the mask.
- 01g: the blocky band doesn't feel Persona. Try flame shapes like the reference mask, if they
  aren't too noisy at small sizes.
- 01h and 01j: liked a lot. 01i needs more colour. 01k: liked, but tweak the randomness.
- 02d and 02e: nice.
- 04g: looks like a knight. Other ways to fill the arrowhead?
- 07i: good idea, but too flat, like Flash-game art. Keep the earlier ghost ideas and give the
  01 ghost the enemy-style mask.
- 07j: less flat than 07i, but the outline looks out of place and it still isn't P3 enough.
- 07k: here the underwater background works better than 07i's sky.
- 07l: keep the green Dark Hour idea; it needs the same work on the outline.
- 08g: nice and simple; try angular eyes. 08h: the burst looks off. 08i: try without the burst,
  with a black shadow like 08g's, and with angular eyes.

| # | Name | From | What's new |
|---|---|---|---|
| 01 | flat-mask | 01f | Smaller triangle eyes (`MASK3_EYE_SM_*`) that stay clear of the mask's bottom edge |
| 02 | stars | 01h | 01 on the tonal star pattern |
| 03 | rings | 01j | 01 on rings and splatter |
| 04 | stars-black | 01i | More red: bright red, dark red, bright red, black, so black is one band in four |
| 05 | splatter | 01k | Spikier splats (`splatter2`), new placement |
| 06 | splatter-big | 01k | Bigger, heavier splats |
| 07 | flame-mask | 01g | `MASK5`: the flat mask with wings that break into swept-back flame tongues |
| 08 | flame-inlay | 01g | 07 on stars, with white flame inlays on the wings. Shows how noisy inlays get |
| 09 | card | 02d | Smaller eyes |
| 10 | card-stars | 02e | Smaller eyes |
| 11 | grin | 04g | Jagged sly grin across the arrowhead |
| 12 | chevrons | 04g | Two download chevrons cut through the arrowhead |
| 13 | halftone | 04g | Comic halftone shading in the arrowhead, densest at the tip |
| 14 | keyhole | 08g | Angular calm eyes |
| 15 | keyhole-white | 08i | White ghost, black cut-out shadow, no burst, angular eyes |
| 16 | white-burst | 08h/08i | 15 over a smaller, more regular burst centred on the figure |
| 17 | ghost-sky | 07i | The 01 ghost with the teal enemy-style mask, two-tone blue-grey cel shading and a tapered ink line, on the sky and wedge |
| 18 | ghost-water | 07k | The same under water |
| 19 | ghost-flame | 07j | The 01 ghost in black with a blue sheen instead of an outline, in blue liquid flame (from the key art), under water |
| 20 | shadow-flame | 07j | The same for the draped Shadow |
| 21 | shadow-white | 07k | The draped Shadow in white with the new shading and ink line |
| 22 | shadow-sky | 07i/07j | 20 on the sky and wedge |
| 23 | hour-shadow | 07l | Dark Hour with a green sheen instead of the outline |
| 24 | hour-ghost | 07l | The same with the 01 ghost |

Dropped while drawing: a black keyhole ghost on a black burst (it merged).

Scripts: `r5_mask.py` (01–10), `r5_download.py` (11–13), `r5_keyhole.py` (14–16), `r5_p3.py`
(17–24) and `overview5.py`. They write `iter/r5-NN-name.*`; the finals are copied to
`round5/NN-name.*`.

## Round 6

Numbering continues from round 5, at 25–45, with files in `round6/`. The sheets are
`overview-round6-red.png` (masked ghost, card, download, keyhole, royal) and
`overview-round6-p3.png`. Same layout as round 5; the "was NN" panels are round 5 designs.

Feedback on round 5:
- 01 is better; make the mask more triangular by raising the bottom corners.
- Backgrounds: 02 (stars) is the best, then 03 (rings), then 01 (dots). 04 and 05: no. 06: meh.
- 07 and 08: the mask doesn't work. The "flames" idea meant eye holes that look like flames, like
  the white flames on the emblem's band.
- 09 and 10: loved.
- 11–13: no. The slit eyes still look like a knight's visor.
- 14 looks like he's squinting. 15 is better, but its shadow looks small. 16 is worse.
- 17, 18 and 24: the 01 ghost shape looks odd with the teal mask; the draped Shadow fits it
  better. At small sizes the mask looks too small.
- 19, 20 and 22: flame under water makes no sense. 21: slightly better, needs something more.
  23: nice.
- New request: "royal" designs like the P5R logo.

New in `lib.py`:
- `MASK6`: more triangular, with the bottom corners raised.
- `FLAME_EYE_L` / `FLAME_EYE_R`: flame-shaped eye holes, an original flame with a rounded base
  and a curled tip.
- `GOLD_DEFS` with `gold_ribbed()` and `sparkle()` for the royal set.

| # | Name | From | What's new |
|---|---|---|---|
| 25 | tri-mask-stars | 01/02 | `MASK6` on the star background |
| 26 | tri-mask-rings | 03 | `MASK6` on rings |
| 27 | tri-mask-dots | 01 | `MASK6` on dots |
| 28 | flame-eyes-stars | 07 | `MASK6` with flame-shaped eye holes, on stars |
| 29 | flame-eyes-rings | 07 | The same on rings |
| 30 | card-tri-mask | 09 | The card with `MASK6` (09 and 10 are kept as they are) |
| 31 | download-mask | 11–13 | Download ghost wearing `MASK6` instead of slit eyes |
| 32 | download-big-head | 11–13 | Bigger head, shorter arrowhead, open triangle eyes |
| 33 | keyhole-white | 15 | Bigger black shadow, open triangle eyes |
| 34 | keyhole-black | 14 | Open triangle eyes instead of the squint |
| 35 | keyhole-flame-eyes | 15 | Flame-shaped eyes on the white keyhole ghost |
| 36 | hour-shadow | 23 | Mask 12% bigger |
| 37 | blue-moon-shadow | 23 | The Dark Hour scene in P3 blue: a pale moon on a night-blue sky, blue sheen |
| 38 | blue-moon-white | 21 | The white Shadow against the blue moon (21's "something more") |
| 39 | vein-shadow | 37 | Glowing cyan veins on the dark body (from the boss reference) |
| 40 | keyart-shadow | 22 | Key-art composition without water or flame: flat azure, black wedge, shards |
| 41 | royal-gold-mask | royal | White ghost with a black drop shadow and a ribbed gold `MASK6`, star glints, red radial glow |
| 42 | royal-gold-burst | royal | Black-mask ghost over a ribbed gold burst |
| 43 | royal-flame-eyes | royal | Gold mask with flame-shaped eye holes |
| 44 | royal-gold-card | royal | Ribbed gold calling card with the white masked ghost |
| 45 | royal-keyhole | royal | 35 with a gold cut-out shadow |

Scripts: `r6_mask.py` (25–30), `r6_ghosts.py` (31–35), `r6_p3.py` (36–40), `r6_royal.py`
(41–45) and `overview6.py`.

## Round 7

Numbers 46–58, with files in `round7/`. The sheets are `overview-round7-red.png` and
`overview-round7-p3.png`; the "was NN" panels are round 5–6 designs.

Feedback on round 6:
- 25: try a middle ground between it and 02. 26 and 27 are worse than 25.
- 28 and 29: the flame is hard to see. It should look like the band art, flowing out of the eye.
- 30: same mask note as 25. Still likes the card fills in 09 and 10.
- 31 and 32: no. 33 and 34: eyes too small; also try the mask from 01–03/25. 35: no.
- 41–45: the royal set doesn't land. 44 is somewhat interesting, but worse than 30.
- 36 is better than 23. 37 is nice; try it with 21's background. 38: 37 is better. 39: no.
- 40 is nice; try a white version and one with 21's background.

New in `lib.py`:
- `MASK7` / `MASK7_EYE_*`: halfway between `MASK3` (02) and `MASK6` (25).
- `FLAME_OUT_L` / `FLAME_OUT_R`: an eye whose flame streams outward past the wing in hooked
  licks. It's drawn at 78% from the inner eye corner, with a black edge.
- `FLAME_UP_*`: an upward variant, unused because it buried the mask.

| # | Name | From | What's new |
|---|---|---|---|
| 46 | mid-mask-stars | 02/25 | `MASK7` on stars |
| 47 | mid-mask-rings | 03/25 | `MASK7` on rings |
| 48 | flame-eyes-out | 28 | White flames streaming out of the eyes and past the wings |
| 49 | flame-eyes-red | 28 | The same in red: burning eyes |
| 50 | card-mid-mask | 09 | 09 with `MASK7` |
| 51 | card-mid-mask-stars | 10 | 10 with `MASK7` |
| 52 | keyhole-big-eyes | 33 | Bigger triangle eyes |
| 53 | keyhole-mask-white | 33 | White keyhole ghost wearing `MASK7` |
| 54 | keyhole-mask-black | 34 | Black keyhole ghost wearing `MASK7` with a white outline |
| 55 | shadow-dark-water | 37 + 21 | 37's dark Shadow on 21's underwater background |
| 56 | shadow-white-keyart | 40 | 40 with the white Shadow |
| 57 | dark-water-wedge | 40 + 21 | 40's wedge and shards under 21's water, dark Shadow |
| 58 | white-water-wedge | 40 + 21 | The same with the white Shadow |

Scripts: `r7_mask.py` (46–51), `r7_keyhole.py` (52–54), `r7_p3.py` (55–58; its P3 helpers are
copied from `r6_p3.py`) and `overview7.py`.

## Round 8

Numbers 59, 61 and 62, with files in `round8/` and the sheet in `overview-round8-all.png`. (60, a
bolder flame with wisps escaping past the wing tips, was dropped while drawing: the wisps read as
glasses temples.)

Feedback on round 7:
- 46: great; it beats 47.
- 48: the flame shape is too simple; follow the original art more closely. 49: no to red flames.
- 51: beats 50 and looks nice.
- 52: the eyes look the same size, just further apart, which is worse.
- 53: OK; try eyes the size of the mask's, without the mask. 54: the same, but not liked.
- 55: nice, better than 37, as good as 40. 56: 40 is better. 57 and 58: 55 is better.
- **36 is a pick** (see Picks at the top).

| # | Name | From | What's new |
|---|---|---|---|
| 59 | burning-eyes | 46 + 48 | 46 with flame eyes built from `tongue()`: three curved tongues per eye with hooked, curling tips stream from the eye towards the wing tip. Clipped to `MASK7` so the band stays black around them, like a flame printed on a band, and the gaps between tongues act as the dark pockets |
| 61 | keyhole-mask-eyes | 53 | Exactly 53's eye holes, same size and position, without the mask band |
| 62 | keyhole-lens-eyes | 53 | Each half of the mask as one big angular eye, shrunk to fit the head |

Tried while drawing 59 and dropped: one big hand-drawn flame outline with comma-like pockets (it
buried the mask and the pockets read as pupils), and the same flame clipped to the mask (the wing
turned all white).

New in `lib.py`: `mirror_path()`; `tongue()`, which builds a flame tongue from a bezier spine, a
taper and a hook curl; and `FLAME_TONGUES_L` / `FLAME_BASE_L` for 59. Scripts: `r8_mask.py` (59),
`r8_keyhole.py` (61–62; helpers copied from `r7_keyhole.py`) and `overview8.py`.

## Round 9

Feedback on round 8:
- 59: wrong idea. The reference has **one** burning eye, and its flame is **big**: a large white
  mass that erupts from the eye and rises up and outward past the face, with hooked tongues,
  dark bays curling into it, and wisps breaking off at the top.
- 61 looks the same as 52: the eyes still look small, while 53's look bigger. 62: no.
- Asked for a black version of 61.

| # | Name | What's new |
|---|---|---|
| 63 | keyhole-bigger-eyes | 53's eye positions without the mask, each eye 1.4× about its own centre. The mask's black surround made 53's holes look bigger than bare eyes of the same size |
| 64 | keyhole-bigger-eyes-black | The same on the black keyhole ghost |

**The one burning eye wasn't solved**, and the idea was later dropped. The scripted attempts (since
deleted) were:
1. Hooked `tongue()`s fanning from the eye: read as a hand with red fingernails.
2. One wide body with tongues splitting off its edge: read as a glove.
3. A Catmull-Rom silhouette drawn like a pen-tool path, with bay-shaped pockets: closer to fire,
   but still reads as a fin with little "C" ears.

The emblem's flame gets its life from hand-drawn, slightly irregular curves, which are hard to
place blind from code. Tracing a hand sketch would be the way to revive the idea.

Scripts: `r9_keyhole.py` (63–64), `overview9.py`; `shortlist.py` (since replaced by `picks_sheet.py`) rendered the picks sheet.

## Round 10: default icon for the neutral theme

The app's current look is rune-kit's neutral kit (`Kit::neutral` in `gui/src/ui/mod.rs`): a
near-black `#0a0a0a` ground with `#ededed` ink. Brief from the user: a simple Pac-Man-like ghost
on black, maybe with a bit of gradient. It's a plain arcade-style ghost (round dome, straight
sides, a hem of three scallops, two dark oval eyes), deliberately not Pac-Man's: monochrome, no
zigzag hem, no white eyes with pupils. Files are in `round10/`; the sheet is
`overview-round10.png`.

| # | Name | What it is |
|---|---|---|
| 65 | neutral-flat | Flat `#ededed` ghost on a top-to-bottom gradient from `#2a2a2c` to `#0a0a0a` |
| 66 | neutral-lit | Softly lit: a radial glow behind the ghost, a white-to-grey body gradient and a blurred floor shadow |
| 67 | neutral-outline | Line-art ghost (40 px stroke) with light eyes, like an SF Symbol |
| 68 | neutral-round-eyes | 65 with round eyes (r = 44), after the user liked 65 |
| 69 | neutral-soft | 68 with the bottom corners and hem points rounded like 67's outline: the ghost traced with a 40 px same-colour stroke with round joins, scaled to 95.5% to compensate |
| 70 | neutral-soft-oval | 69 with 65's oval eyes |
| 71 | neutral-soft-shallow | 70 with a shallower hem: scallops rise about 30 px instead of 50 |
| 72 | neutral-soft-flatter | 70 with an even shallower hem, about 18 px |
| 73 | neutral-soft-mid | Halfway between 70 and 71: scallops rise about 40 px |
| 74 | neutral-soft-35 | Halfway between 73 and 71: scallops rise about 35 px |
| 75 | neutral-wave | The hem as an even sine wave (cubic half-waves with flat tangents, handles 0.364 × half-wave), so the downward points are as round as the upward bumps; 40 px tall. No rounding stroke, since it fattens outward curves and pinches inward ones; only the two outer corners get a 22 px round |
| 76 | neutral-even | Every point of the hem rounded with the same 44 px radius (`fillet_path`): a zigzag of three hanging lobes with the outer corners at notch height, so the corners, lobe bottoms and notches all curve alike. 75 wasn't even: tight 22 px corners against wide wave curves, and squeezed end lobes |
| 77 | neutral-even-round | 76 with a 68 px radius and a taller zigzag (74 px instead of 50), so the hem is rounder overall and still evenly curved |

Outcome: **74 is picked as the default icon.** The user preferred its hem over the evenly rounded
76 and 77.

All three get a faint white hairline on the squircle edge, as dark macOS icons have, so they
don't dissolve into a dark Dock. Scripts: `r10_neutral.py` and `overview10.py`.

## Round 11: light/dark pairs

Pairing the picks for the Appearance swap. Feedback: pair 36 (dark) with 37 (light), and give 40
and 55 dark versions in 36's Dark Hour green; try dark colour swaps of 46, 50 and 51, never
painting the ghost red. Then: mute 81's and 83's bright red burst towards 82's deep red. The
candidate pairs were compared on `src/iter/overview-pairs.png` (`src/pairs.py`): the light icon on
a light Dock-like panel, the dark icon on a dark one.

| # | Name | From | What's new | Outcome |
|---|---|---|---|---|
| 78 | keyart-dark-hour | 40 | Dark green sky, black wedge, lime shards, green sheen | The black wedge vanishes into the dark sky |
| 79 | keyart-lime-wedge | 78 | The wedge in the moon's lime | **Pick**, pairs with 40 |
| 80 | water-dark-hour | 55 | Green-black water with lime caustics | **Pick**, pairs with 55 |
| 81 | mask-stars-black | 46 | Near-black tonal stars, bright red burst | Burst too loud |
| 82 | mask-stars-deep-red | 46 | Stars in deep reds, black burst | **Pick**, pairs with 46 |
| 83 | mask-stars-ember | 46 | Black-red stars, bright red burst | Burst too loud |
| 84 | card-deep-red | 51 | Deep-red card with darker print, on black | **Pick** on its own; as 46's partner, no |
| 85 | card-black-on-red | 51 | Colours swapped: black card, white edge, red ground | **Pick**, the light partner of 51 |
| 86 | mask-stars-black-deep-burst | 81 | Burst in 82's deep red | Burst disappears at 32 px |
| 87 | mask-stars-black-mid-burst | 81 | Burst in a mid red (`#7a000b`) | **Pick**, pairs with 46 |
| 88 | mask-stars-ember-mid-burst | 83 | 83 with a mid-red burst | Too close to 82 |

46 + 51 was dropped as a pair. Picks also got names and ids (see Picks); `picks/` is now named by
id. Scripts: `r11_p3.py` (78–80), `r11_p5.py` (81–88), `pairs.py`, `picks_sheet.py` (replacing
the deleted `shortlist.py`) and `app_icons.py`.

## Regenerating and editing

The concepts are generated by the Python scripts in `src/`. `lib.py` holds the shared helpers,
and `cNN.py` draws concept NN. The SVGs are rendered by a small Rust CLI around the `resvg`
crate, in `src/render/`, which is standalone and not part of the Phantom workspace.

```sh
cd design/icons/src
(cd render && cargo build --release)   # once
python3 c02.py                         # writes iter/02-calling-card{.svg,.png,-64.png,-32.png,-preview.png}
```

Scripts from round 2 on also write `iter/NAME-sizes.png`, which shows the icon at its real 128,
64, 32 and 16 px sizes on light and dark backgrounds, plus 32 px zoomed in. `python3 overview.py 2`
(or `3`, `4`) rebuilds `iter/overview-roundN.png` after that round's scripts have run.

| Script | Writes |
|---|---|
| `c01b.py` | 01b, 01b-small |
| `c02b.py`, `c04b.py`, `c07d.py`, `c07e.py` | 02b; 04b and 04c; 07d; 07e |
| `c08b.py` | 08b, 08c, 08d |
| `c01c.py` | 01c, 01d, 01e |
| `c02c.py`, `c04d.py` | 02c; 04d and 04e |
| `c07f.py` | 07f, 07g, 07h |
| `c08e.py` | 08e, 08f |
| `c01f.py` | 01f–01k |
| `c02d.py`, `c04f.py` | 02d, 02e; 04g |
| `c07i.py` | 07i–07l |
| `c08g.py` | 08g, 08h, 08i |

From round 5 on, scripts are named `rN_*.py` and write `iter/rN-NN-name.*`; the finals live in
`roundN/NN-name.*`, and `overviewN.py` (`overview5.py` to `overview9.py`) rebuilds that round's
sheets. The round sections above list which script draws which number.

Output goes to `src/iter/`, which git ignores. To change a concept, edit its script, rerun it,
look at the renders, then copy the final `.svg` and `.png` into this folder (rounds 1–4) or
`roundN/` (rounds 5 on). Running the scripts unchanged reproduces every committed SVG exactly.

## Next steps

Ship them: `app/phantom.icns` as the bundle icon in `scripts/install-mac.sh`, then the in-app icon
picker reading `app/app-icons.json` ([#14](https://github.com/HugeLetters/phantom/issues/14)).
