# Component state is drawn from eased amounts, not per state

A themed component's state is a few amounts, from 0 to 1. A button has three (hovered, pressed,
disabled) and a list row two (hovered, selected). Each amount eases on its own spring toward what
the current state wants, and the component is drawn from those amounts alone, with the same parts
at every state. Any change of state, in any order, then animates from wherever the component is,
with no transition written per pair of states. Pressed to idle, hovered to disabled and a
selection moving on are all covered.

## Considered options

- **A drawing per state, swapped on change**: what the Persona 5 sketch tried first. Rune shows a
  swapped element as a cross-fade, so buttons showed two faces on hover and labels vanished on
  press. Each new pair of states also needed its own code, and pressed to idle was simply missed.

## Consequences

- **Low-contrast color swaps:** where a label and its ground would pass through low contrast
  (red letters on a face turning red), the change moves as a wipe across the component instead of
  a color blend.
- **Two copies of the label:** Rune still tweens a text color change for at least 0.1 s, even
  with `Spring::NONE`, so a label that swaps colors is kept twice, each copy clipped to its side
  of the wipe.
