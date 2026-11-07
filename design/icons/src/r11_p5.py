from lib import *

# Round 11, Persona 5 colour swaps (81-85): darker versions of 46 and the cards 50/51 for a dark
# Appearance, plus a red-ground card. The ghost always stays white. Shapes copied from r7_mask.py.
TRI = poly(MASK7_EYE_L, fill="#fff") + poly(MASK7_EYE_R, fill="#fff")
DARK_RED, DEEP_RED = "#a8000c", "#5c0007"


def figure():
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK7, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {TRI}
</g>"""


def burst_(fill):
    return poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill=fill)


def stars_(colors):
    return stars(512, 560, 5, 1150, 95, colors, phase=-math.pi / 2 - 0.12)


def icon(name, body):
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


# 81: 46 on black: tonal near-black stars, red burst
icon("r11-81-mask-stars-black", stars_(["#1e1e1e", "#0a0a0a"]) + burst_(RED) + figure())
# 82: 46 in deep reds: the stars darkened to blood red, black burst
icon("r11-82-mask-stars-deep-red", stars_([DEEP_RED, "#2a0004"]) + burst_("#000") + figure())
# 83: 82 with a red burst instead, black-red stars
icon("r11-83-mask-stars-ember", stars_(["#3a0005", "#140002"]) + burst_(RED) + figure())

rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 300, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
for i in range(1, 13):
    x = x1 - (x1 - x0) * i / 13
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -12)
CARD_STARS = (470, 640, 5, 900, 80)


def card_icon(name, ground, dots, edge, card_fill, card_print=""):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{ground}"/>
{halftone(900, 900, 15, 700, step=30, color=dots)}
{halftone(110, 120, 11, 420, step=30, color=dots)}
<g transform="translate(500 500) scale(0.84) translate(-512 -512)">
  {poly([(x + 22, y + 22) for x, y in card], fill=edge)}{poly(card, fill=card_fill)}
  <clipPath id="cc">{poly(card)}</clipPath><g clip-path="url(#cc)">{card_print}</g>
</g>
<g transform="translate(556 468) rotate(8) scale(0.8) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/>
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK7, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {TRI}
</g>"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


# 84: 51 with a deep-red card printed with darker stars, red edge, on black
card_icon("r11-84-card-deep-red", "#0d0d0d", "#2e2e2e", RED, DEEP_RED,
          stars(*CARD_STARS, [DEEP_RED, "#2a0004"], phase=-math.pi / 2 - 0.2))
# 85: colours swapped: a black card with a red edge and dark star print on red, grey-red halftone
card_icon("r11-85-card-black-on-red", RED, DARK_RED, "#fff", "#0d0d0d",
          stars(*CARD_STARS, ["#0d0d0d", "#262626"], phase=-math.pi / 2 - 0.2))

# 86-88: 81 and 83 with the burst muted towards 82's deep red, after feedback that bright red was too loud
icon("r11-86-mask-stars-black-deep-burst", stars_(["#1e1e1e", "#0a0a0a"]) + burst_(DEEP_RED) + figure())
icon("r11-87-mask-stars-black-mid-burst", stars_(["#1e1e1e", "#0a0a0a"]) + burst_("#7a000b") + figure())
icon("r11-88-mask-stars-ember-mid-burst", stars_(["#3a0005", "#140002"]) + burst_("#80000d") + figure())
