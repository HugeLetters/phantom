from lib import *

rnd = random.Random(11)
# card: tilted, top-right corner clipped, bottom edge torn
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 330, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
n = 15
for i in range(1, n):
    x = x1 - (x1 - x0) * i / n
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -12)

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(900, 900, 15, 700, step=30, color=RED)}
{poly([(80, 640), (1000, 330), (1000, 520), (80, 830)], fill=RED)}
<g transform="translate(512 520) scale(0.9) translate(-512 -512)">
  {poly([(x + 20, y + 20) for x, y in card], fill=RED)}
  {poly(card, fill="#fff")}
  <g transform="rotate(-12 512 512) translate(512 528) scale(0.74) translate(-548 -524) rotate(-6 512 512)">
    <path d="{GHOST}" fill="#000"/>
    {poly(MASK, fill=RED, stroke="#000", stroke_width=26, stroke_linejoin="miter", paint_order="stroke")}
    {poly(MASK_EYE_L, fill="#fff")}{poly(MASK_EYE_R, fill="#fff")}
  </g>
</g>
"""
write("02b-calling-card-ghost", svg(body), sizes=(1024, 64, 32))
icon_sizes("02b-calling-card-ghost")
