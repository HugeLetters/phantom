# Persona themes are drawn in code, with generated textures where shapes fall short

The Persona themes draw everything as vector shapes built in Rust each frame, not from image
assets. That covers slabs, flags, halftone, stars and the heap of moons. Three reasons:
- the game's art isn't ours to ship;
- every font the themes name ships with macOS;
- shapes let each activation and each frame differ, so a button never lands the same way twice.

Where shapes can't make a look, such as the billowing nebulae of Persona 5: Jose's shop with dust lanes and filaments,
the theme generates a texture when it starts from an algorithm and a seed (domain-warped fractal
noise) instead of shipping an image, so each session gets a new sky.

## Considered options

- **Images from the game**: not ours to ship.
- **Painted images of our own**: still, at one resolution, and one per window size.
- **Vector shapes only, for the nebulae too**: metaballs and layered discs read as blobs, not
  clouds.

## Consequences

- **Performance:** the sketch redraws 40,000–53,000 shapes a frame and runs below 60 fps even in a
  release build. A theme must draw the still parts once and cache them, redrawing only what
  moves. The still parts are the halftone, the screentone, the rims and the star field.
- **Morphing nebulae:** they would need a GPU shader through Rune's effect slots, evaluated per
  pixel each frame, rather than more textures.
