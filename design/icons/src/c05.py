from lib import *

P = ("M330,210 L640,210 L730,300 L730,480 L640,570 L480,570 L480,820 L330,820 Z "
     "M480,330 L570,330 L590,350 L590,430 L570,450 L480,450 Z")
# torn paper scrap behind
rnd = random.Random(7)
scrap = []
for i in range(36):
    a = 2 * math.pi * i / 36
    r = 1 + rnd.uniform(-0.04, 0.04)
    x = 512 + 380 * r * math.cos(a) * (1.0 if math.cos(a) > 0 else 0.95)
    y = 512 + 395 * r * math.sin(a)
    scrap.append((x, y))
# squarer scrap: clamp to rounded box
scrap = [(max(190, min(834, x)), max(170, min(860, y))) for x, y in scrap]
scrap = []
edge = lambda a, b, n, amp: [(a[0] + (b[0] - a[0]) * t / n + rnd.uniform(-amp, amp),
                              a[1] + (b[1] - a[1]) * t / n + rnd.uniform(-amp, amp)) for t in range(n)]
corners = [(250, 200), (800, 175), (830, 830), (215, 860)]
for k in range(4):
    scrap += edge(corners[k], corners[(k + 1) % 4], 16, 18)
scrap = rot(scrap, -7)

cut = "M0,0 L1024,0 L1024,520 L0,760 Z"
cut2 = "M0,760 L1024,520 L1024,1024 L0,1024 Z"

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(512, 512, 9, 700, step=26, color="#3a3a3a")}
{poly(scrap, fill=RED)}
<clipPath id="sc">{poly(scrap)}</clipPath>
<g clip-path="url(#sc)">{halftone(780, 820, 15, 520, step=30, color="#000", opacity=0.55)}</g>
<clipPath id="c1"><path d="{cut}"/></clipPath>
<clipPath id="c2"><path d="{cut2}"/></clipPath>
<g transform="translate(512 512) scale(0.9) translate(-512 -512) skewX(-8) translate(85 0)">
  <g clip-path="url(#c1)" transform="translate(-10 -22) rotate(-2 512 400)">
    <path d="{P}" fill="#000" fill-rule="evenodd" transform="translate(18 18)"/>
    <path d="{P}" fill="#fff" fill-rule="evenodd"/>
  </g>
  <g clip-path="url(#c2)" transform="translate(4 30) rotate(4 400 680)">
    <path d="{P}" fill="#000" fill-rule="evenodd" transform="translate(18 18)"/>
    <path d="{P}" fill="#fff" fill-rule="evenodd"/>
  </g>
</g>
"""
write("05-shattered-p", svg(body), sizes=(1024, 64, 32))
preview("05-shattered-p")
