from lib import *

# red calling card tilted one way; the white masked ghost tilted the other, breaking out over its top edge
rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 230, 512 + 230, 512 - 280, 512 + 330
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
n = 13
for i in range(1, n):
    x = x1 - (x1 - x0) * i / n
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -14)


def icon(name, bg, dots, ghost_at):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{bg}"/>
{halftone(900, 900, 15, 700, step=30, color=dots)}
{halftone(110, 120, 11, 420, step=30, color=dots)}
<g transform="translate(497 496) scale(0.86) translate(-512 -512)">
  {poly([(x + 22, y + 22) for x, y in card], fill="#fff")}
  {poly(card, fill=RED)}
</g>
<g transform="{ghost_at} rotate(10) scale(0.66) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(18 18)"/>
  <path d="{GHOST}" fill="#fff"/>
  <path d="{MASK2}" fill="#000" stroke="#fff" stroke-width="22" stroke-linejoin="round" paint-order="stroke"/>
  <path d="{MASK2_EYE_L}" fill="#fff"/><path d="{MASK2_EYE_R}" fill="#fff"/>
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("02c-red-card-white-ghost", "#0d0d0d", "#3a3a3a", "translate(588 406)")
