from lib import *

rnd = random.Random(11)
# card: 470x640 centred, top-right corner clipped, bottom edge torn
x0, x1, y0, y1 = 512 - 240, 512 + 240, 512 - 330, 512 + 300
card = [(x0, y0), (x1 - 70, y0), (x1, y0 + 70), (x1, y1)]
n = 13
for i in range(1, n):
    x = x1 - (x1 - x0) * i / n
    card.append((x, y1 + (rnd.uniform(-46, -6) if i % 2 else rnd.uniform(6, 30))))
card.append((x0, y1 - 20))
card = rot(card, -12)

# sly face on card (in card space, then rotated)
eyeL = [(345, 330), (488, 372), (470, 404), (360, 380)]
eyeR = [(679, 330), (536, 372), (554, 404), (664, 380)]
grin = [(330, 470), (372, 486), (394, 520), (420, 496), (466, 530), (512, 504), (558, 530),
        (604, 496), (630, 520), (652, 486), (694, 470), (640, 600), (512, 640), (384, 600)]
teeth = []
for i, (a, b) in enumerate([(380, 486), (446, 503), (512, 508), (578, 503), (644, 486)]):
    pass

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(900, 900, 15, 700, step=30, color=RED)}
{poly(rot([(80, 640), (1000, 330), (1000, 520), (80, 830)], 0), fill=RED)}
<g transform="translate(512 520) scale(0.9) translate(-512 -512)">
  {poly([(x + 18, y + 18) for x, y in card], fill=RED)}
  {poly(card, fill="#fff")}
  <clipPath id="cc">{poly(card)}</clipPath>

  {poly(rot(eyeL, -12), fill="#000")}{poly(rot(eyeR, -12), fill="#000")}
  {poly(rot(grin, -12), fill="#000")}
</g>
"""
write("02-calling-card", svg(body), sizes=(1024, 64, 32))
preview("02-calling-card")
