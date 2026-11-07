from lib import *

# Round 9, keyhole ghost (63-64): 53's eye positions without the mask, each eye enlarged about its own centre,
# because the mask's black surround made 53's holes look bigger than bare eyes of the same size (52, 61)
R, NECK, LENGTH, FLARE, TEETH, DEPTH, TILT = 230, 160, 430, 210, 3, 60, -6
ny = math.sqrt(R * R - NECK * NECK)
HOLE = f"M{f(512 - NECK)},{f(ny)} A{R},{R} 0 1 1 {f(512 + NECK)},{f(ny)} L{f(512 + FLARE)},{LENGTH} "
seg = 2 * FLARE / TEETH
for i in range(TEETH):
    x = 512 + FLARE - seg * i
    HOLE += f"L{f(x - seg / 2)},{f(LENGTH - DEPTH)} L{f(x - seg)},{f(LENGTH + (DEPTH * 0.15 if i % 2 else 0))} "
HOLE += "Z"
S = 620 / (LENGTH + R)
PLACE = f"translate(492 462) rotate({TILT}) scale({f(S)}) translate(-512 0)"
MASK_T = "translate(0 -420) translate(512 440) scale(1.02) translate(-514 -440)"


def big(eye, k):
    cx = sum(p[0] for p in eye) / 3
    cy = sum(p[1] for p in eye) / 3
    return [(cx + (x - cx) * k, cy + (y - cy) * k) for x, y in eye]


def icon(name, body, cut, eye, shadow_px, k=1.4):
    eyes = f'<g transform="{MASK_T}">{poly(big(MASK7_EYE_L, k), fill=eye)}{poly(big(MASK7_EYE_R, k), fill=eye)}</g>'
    content = (f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>'
               + halftone(120, 920, 14, 600, step=30) + halftone(920, 110, 12, 480, step=30) + f"""
<g transform="{PLACE}">
  <path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(shadow_px * 2 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(shadow_px / S)} {f(shadow_px / S)})"/>
  <path d="{HOLE}" fill="{body}"/>{eyes}
</g>""")
    write(name, svg(content), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r9-63-keyhole-bigger-eyes", "#fff", "#000", "#000", 26)
icon("r9-64-keyhole-bigger-eyes-black", "#000", "#fff", "#fff", 18)
