from lib import *

# 02c's red card + white ghost, with the card's empty space filled
rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 300, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
n = 13
for i in range(1, n):
    x = x1 - (x1 - x0) * i / n
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -12)


def ghost(at, scale):
    return f"""
<g transform="{at} rotate(8) scale({scale}) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/>
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK3, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {poly(MASK3_EYE_L, fill="#fff")}{poly(MASK3_EYE_R, fill="#fff")}
</g>"""


def icon(name, card_fill, ghost_):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(900, 900, 15, 700, step=30, color="#3a3a3a")}
{halftone(110, 120, 11, 420, step=30, color="#3a3a3a")}
<g transform="translate(500 500) scale(0.84) translate(-512 -512)">
  {poly([(x + 22, y + 22) for x, y in card], fill="#fff")}
  {poly(card, fill=RED)}
  <clipPath id="cc">{poly(card)}</clipPath>
  <g clip-path="url(#cc)">{card_fill}</g>
</g>
{ghost_}
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


# bigger ghost, centred on the card and spilling past its edges
icon("02d-card-big-ghost", "", ghost("translate(556 468)", 0.8))
# card printed with a tonal star pattern, ghost breaking out at the top right as in 02c
icon("02e-card-star-print", stars(470, 640, 5, 900, 80, [RED, "#a8000c"], phase=-math.pi / 2 - 0.2),
     ghost("translate(560 440)", 0.74))
