# Themes override building blocks, not whole screens

Screens describe what is shown and which actions exist, and draw everything through Phantom's own `ui` building blocks (panel, button, row, progress bar…), never through rune-kit or raw styling directly. A theme overrides the building blocks it cares about; anything it leaves out falls back to the plain theme, so a partial theme still yields a working app. A theme may override a whole screen where its layout genuinely differs, but behaviour stays shared. The `Theme` interface is designed when the first non-plain theme (Persona 5) is built, not before.

## Considered options

- **Colour tokens only**: can't express themes that differ in shape and motion, not just palette.
- **A separate set of views per theme**: duplicates behaviour and doubles every change.
