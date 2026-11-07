from lib import *

# Round 6, download ghost (31-32) and keyhole ghost (33-34): open triangle eyes / the mask instead of slits
SMALL_BURST = poly(burst(512, 540, 12, 330, 262, jitter=0.16, seed=3, phase=0.2), fill="#000")


def tri_eyes(cx, cy, s, color):
    """MASK6's triangle eyes as bare eyes, centred on (cx, cy) and scaled by s."""
    L = [(cx + (x - 514) * s, cy + (y - 446) * s) for x, y in MASK6_EYE_L]
    return poly(L, fill=color) + poly(mirror(L, cx), fill=color)


def red_bg():
    return (f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>'
            + halftone(120, 930, 13, 640, step=30) + halftone(920, 100, 10, 420, step=30))


def out(name, body):
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


arrow = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
out("r6-31-download-mask", red_bg() + SMALL_BURST + f"""
<g transform="translate(512 526) rotate(-10) scale(0.9) translate(-512 -526)">
<path d="{arrow}" fill="#fff"/>
<g transform="translate(-2 -6)">{poly(MASK6, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}{poly(MASK6_EYE_L, fill="#fff")}{poly(MASK6_EYE_R, fill="#fff")}</g>
</g>""")
# bigger head, short arrowhead so there is less empty white below the face
short = ("M262,540 L262,430 C262,262 376,170 512,170 C648,170 762,262 762,430 L762,540 "
         "L856,500 L790,610 L512,800 L234,610 L168,500 Z")
out("r6-32-download-big-head", red_bg() + SMALL_BURST + f"""
<g transform="translate(512 520) rotate(-10) scale(0.92) translate(-512 -486)">
<path d="{short}" fill="#fff"/>{tri_eyes(512, 410, 1.12, "#000")}
</g>""")

# keyhole (round 5's 14/15) with open triangle eyes; 33 also gets a bigger black shadow
R, NECK, LENGTH, FLARE, TEETH, DEPTH, TILT = 230, 160, 430, 210, 3, 60, -6
ny = math.sqrt(R * R - NECK * NECK)
HOLE = f"M{f(512 - NECK)},{f(ny)} A{R},{R} 0 1 1 {f(512 + NECK)},{f(ny)} L{f(512 + FLARE)},{LENGTH} "
seg = 2 * FLARE / TEETH
for i in range(TEETH):
    x = 512 + FLARE - seg * i
    HOLE += f"L{f(x - seg / 2)},{f(LENGTH - DEPTH)} L{f(x - seg)},{f(LENGTH + (DEPTH * 0.15 if i % 2 else 0))} "
HOLE += "Z"
S = 620 / (LENGTH + R)
PLACE = f"translate(492 462) rotate({TILT}) scale({f(S)}) translate(-512 0)"   # centred as in round 5


def keyhole(name, body, cut, eye, shadow_px):
    out(name, red_bg() + f"""
<g transform="{PLACE}">
  <path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(shadow_px * 2 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(shadow_px / S)} {f(shadow_px / S)})"/>
  <path d="{HOLE}" fill="{body}"/>
  {tri_eyes(512, 4, 0.86, eye)}
</g>""")


keyhole("r6-33-keyhole-white", "#fff", "#000", "#000", 26)
keyhole("r6-34-keyhole-black", "#000", "#fff", "#fff", 18)
# 35: the flame-shaped eyes (from 28) on the white keyhole ghost
k = 0.86
flame_eyes = (f'<path d="{FLAME_EYE_L}" fill="#000" transform="translate({f(512 - 112 * k)} 8) scale({k}) translate(-392 -440)"/>'
              f'<path d="{FLAME_EYE_R}" fill="#000" transform="translate({f(512 + 112 * k)} 8) scale({k}) translate(-636 -440)"/>')
out("r6-35-keyhole-flame-eyes", red_bg() + f"""
<g transform="{PLACE}">
  <path d="{HOLE}" fill="#000" stroke="#000" stroke-width="{f(52 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(26 / S)} {f(26 / S)})"/>
  <path d="{HOLE}" fill="#fff"/>{flame_eyes}
</g>""")
