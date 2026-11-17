# Look Presets: a theme and its settings per Appearance

The look of the app is a **Look Preset**: a **Theme Look** (a theme with its settings) for the Light Appearance and one for the Dark Appearance. The system's Appearance only picks which of the two is shown; a theme declares nothing about it, and there is no Palette layer. A variant such as Persona 3 Reload's Dark Hour is a theme setting or another theme.

Each Theme Look stores its own theme id and settings, so a Look Preset is two full values, often equal. Every theme setting can differ between the two Looks. When both use the same theme the settings screen shows one set of fields and writes both; splitting a field is only a client shortcut for setting it on each Look separately, and a field shows as split when the two values differ. Nothing is stored per theme outside a Look Preset, so switching a Look's theme starts at that theme's defaults.

The app shows an **Active Look Preset**, a working copy, kept in the GUI's `gui.json`. Saved Look Presets are named snapshots: picking one copies it into the active one, tweaking leaves it alone, and the user can update it or save a new one. A saved preset's half can be copied into just the Light or the Dark Look, so mixing halves is possible; that is why the active preset can't be a saved one edited in place. A fresh install has Phantom with the Tone setting, Light in Light and Dark in Dark.

A theme declares its settings as typed descriptors: a key, a label, a default and a kind from a small closed set (toggle, choice from a fixed list, colour); Phantom declares `tone` (Light, Dark). One generic settings screen shows them. Values are stored as JSON by key; an unknown key or a value of the wrong kind is reset to the default, with the reason shown. A new kind is added when a theme needs it.

Given a Theme Look's settings, a theme builds a resolved look: tokens, block overrides (a button, a table row, a context menu), optional screen overrides (a wholly different torrent info panel), and the app icon as image bytes. The app keeps two resolved looks, one per Appearance, and a settings change rebuilds only the one it touches. ADR 0004's parent fallback is unchanged: a block the theme doesn't draw resolves through theme, parent, Phantom.

There is no control to pin the Appearance. Using the same Theme Look for both Appearances is the pin; the cost is that a temporary "always Dark" overwrites the Light half unless a preset was saved first. An Appearance override (System, Always Light, Always Dark) on the Active Look Preset can be added later without changing the model.

The **App icon** is not part of a Look Preset. It is one global setting, one icon or a Light and a Dark icon, or follows the theme, which means the icon the Theme Look for the current Appearance gives, possibly rendered from its settings. A deliberate icon choice therefore survives changing preset or theme.

## Considered options

- **Palette layer under each theme**: a third axis next to theme and Appearance; everything it does a theme setting can do.
- **Settings stored once per theme id**: keeps values across switches but can't express a different theme in each Appearance with its own settings, and hides what is shown behind a lookup.
- **Flat themes only (Plain Light, Plain Dark…)**: no shared settings between a family's members; changing "animations off" means doing it twice.
- **Editing saved presets in place**: simplest, but can't represent a mix of halves taken from different presets.
