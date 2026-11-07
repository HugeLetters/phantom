from lib import *

# 01 with flat straight-cut masks, then the halftone replaced by other P5-style backgrounds


def figure(mask, eye_l, eye_r, outline=22):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(mask, fill="#000", stroke="#fff", stroke_width=outline, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {poly(eye_l, fill="#fff")}{poly(eye_r, fill="#fff")}
</g>"""


BURST = poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")
DOTS = halftone(130, 920, 14, 640, step=30) + halftone(920, 110, 11, 430, step=30)
FIG3 = figure(MASK3, MASK3_EYE_L, MASK3_EYE_R)
FIG4 = figure(MASK4, MASK4_EYE_L, MASK4_EYE_R)


def icon(name, bg):
    write(name, svg(f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>' + bg), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("01f-flat-mask", DOTS + BURST + FIG3)
icon("01g-blindfold-mask", DOTS + BURST + FIG4)
icon("01h-star-pattern", stars(512, 560, 5, 1150, 95, [RED, "#a8000c"], phase=-math.pi / 2 - 0.12) + BURST + FIG3)
icon("01i-dark-stars", stars(512, 560, 5, 1150, 95, ["#5a0008", "#000"], phase=-math.pi / 2 - 0.12)
     + poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill=RED) + FIG3)
icon("01j-rings-splatter", rings(512, 540, 900, 70, [RED, "#a8000c"])
     + splatter([(170, 220, 22), (860, 830, 26), (880, 260, 12), (190, 840, 14)], seed=2)
     + BURST + FIG3)
icon("01k-ink-splatter", splatter([(170, 220, 70), (860, 830, 84), (870, 220, 40), (190, 830, 50), (530, 930, 30), (120, 520, 26), (920, 520, 30)], seed=5)
     + BURST + FIG3)
