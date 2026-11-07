from lib import *

# Round 6, masked ghost (25-30): the more triangular MASK6, flame-shaped eye holes, card with MASK6


def figure(mask, eyes):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(mask, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {eyes}
</g>"""


TRI = poly(MASK6_EYE_L, fill="#fff") + poly(MASK6_EYE_R, fill="#fff")
FLAME = (f'<path d="{FLAME_EYE_L}" fill="#fff" transform="translate(402 444) scale(0.86) translate(-392 -440)"/>'
         f'<path d="{FLAME_EYE_R}" fill="#fff" transform="translate(626 444) scale(0.86) translate(-636 -440)"/>')
BURST = poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")
DOTS = halftone(130, 920, 14, 640, step=30) + halftone(920, 110, 11, 430, step=30)
STARS = stars(512, 560, 5, 1150, 95, [RED, "#a8000c"], phase=-math.pi / 2 - 0.12)
RINGS = rings(512, 540, 900, 70, [RED, "#a8000c"]) + splatter([(170, 220, 22), (860, 830, 26), (880, 260, 12), (190, 840, 14)], seed=2)


def icon(name, body):
    write(name, svg(f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>' + body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r6-25-tri-mask-stars", STARS + BURST + figure(MASK6, TRI))
icon("r6-26-tri-mask-rings", RINGS + BURST + figure(MASK6, TRI))
icon("r6-27-tri-mask-dots", DOTS + BURST + figure(MASK6, TRI))
icon("r6-28-flame-eyes-stars", STARS + BURST + figure(MASK6, FLAME))
icon("r6-29-flame-eyes-rings", RINGS + BURST + figure(MASK6, FLAME))

# 30: round 5's card (09) with the triangular mask
rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 300, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
for i in range(1, 13):
    x = x1 - (x1 - x0) * i / 13
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -12)
body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(900, 900, 15, 700, step=30, color="#3a3a3a")}
{halftone(110, 120, 11, 420, step=30, color="#3a3a3a")}
<g transform="translate(500 500) scale(0.84) translate(-512 -512)">
  {poly([(x + 22, y + 22) for x, y in card], fill="#fff")}{poly(card, fill=RED)}
</g>
<g transform="translate(556 468) rotate(8) scale(0.8) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/>
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK6, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {TRI}
</g>"""
write("r6-30-card-tri-mask", svg(body), sizes=(1024, 64, 32))
icon_sizes("r6-30-card-tri-mask")
