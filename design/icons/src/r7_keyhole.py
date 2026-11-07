from lib import *

# Round 7, keyhole ghost (52-54): bigger eyes; wearing MASK7 like the 01 ghost
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


def tri_eyes(cy, s, color):
    L = [(512 + (x - 514) * s, cy + (y - 446) * s) for x, y in MASK7_EYE_L]
    return poly(L, fill=color) + poly(mirror(L, 512), fill=color)


def mask_on_head(outline):
    # MASK7 is drawn for the 01 ghost's head (centre about 514,430); move it onto this head (512,0)
    return (f'<g transform="translate(0 -420) translate(512 440) scale(1.02) translate(-514 -440)">'
            + poly(MASK7, fill="#000", stroke=outline, stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")
            + poly(MASK7_EYE_L, fill="#fff") + poly(MASK7_EYE_R, fill="#fff") + "</g>")


def icon(name, body, cut, face, shadow_px=26):
    content = (f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>'
               + halftone(120, 920, 14, 600, step=30) + halftone(920, 110, 12, 480, step=30) + f"""
<g transform="{PLACE}">
  <path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(shadow_px * 2 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(shadow_px / S)} {f(shadow_px / S)})"/>
  <path d="{HOLE}" fill="{body}"/>{face}
</g>""")
    write(name, svg(content), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r7-52-keyhole-big-eyes", "#fff", "#000", tri_eyes(10, 1.0, "#000"))
icon("r7-53-keyhole-mask-white", "#fff", "#000", mask_on_head("#fff"))
icon("r7-54-keyhole-mask-black", "#000", "#fff", mask_on_head("#fff"), 18)
