# Themes override building blocks, not whole screens

Screens describe what is shown and which actions exist, and draw everything through Phantom's own `ui` building blocks (panel, button, row, progress bar…), never through rune-kit or raw styling directly. A theme works in three layers, each overriding the one below: **tokens** (palette, spacing, timing curves), **building blocks** drawn with tokens, and **screens** composed of blocks. Each theme declares a parent (normally Plain, which is complete). A building block resolves as a whole through theme → parent → Plain, so a block never mixes two themes' geometry and a partial theme still yields a working app.

A screen override is a last resort for a genuinely different layout (the torrent list as a grid of cards), still built from shared blocks; a theme that only wants a block to look different (a jagged progress bar) overrides the block. Every theme shows the same data: a screen override may show less, never something different. When a theme needs a hook a screen doesn't expose, the hook is added to the screen for every theme; themes never reach into a screen's internals. The `Theme` interface is designed when the first non-plain theme (Persona 5) is built.

Look and **App icon** are independent axes (GLOSSARY.md): the app icon follows the theme unless the user picks one, so switching themes never overrides a deliberate choice. Plain follows the system's Appearance; Persona themes ignore it and may offer their own Palettes.

Themes are built into the app (Rust code with their assets in their own folder). User-made themes loaded at runtime, as herdr-app does with layout files and scripted mods, are out of scope.

## Considered options

- **Colour tokens only**: can't express themes that differ in shape and motion, not just palette.
- **A separate set of views per theme**: duplicates behaviour and doubles every change.
- **User-made themes with a scripting layer**: what gives herdr-app its range, but it needs a stable scripting API and runs untrusted code; far more than Phantom needs.
