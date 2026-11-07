from lib import *

# sinking ghost: 01's ghost in cel-shaded white, under a water surface of caustic light
shade = [(600, 150), (900, 150), (900, 900), (640, 900), (700, 560)]
eyeL = [(336, 436), (478, 462), (466, 488), (382, 484)]
eyeR = mirror(eyeL, 514)
rnd = random.Random(4)
caustics = ""
for _ in range(26):
    cx, cy = rnd.uniform(100, 930), rnd.uniform(90, 330)
    pts_ = [(cx + rnd.uniform(-60, 60), cy + rnd.uniform(-18, 18)) for _ in range(5)]
    caustics += poly(sorted(pts_, key=lambda p: math.atan2(p[1] - cy, p[0] - cx)), fill="#b9fbff",
                     opacity=f"{rnd.uniform(0.35, 0.8):.2f}")
ribbon = "M1040,560 C900,600 780,700 700,860 C800,720 910,630 1040,600 Z"
spots = [(190, 520, 22), (860, 640, 20), (820, 860, 14), (230, 800, 16), (880, 470, 12), (420, 900, 12)]
defs = """
<linearGradient id="sea" x1="0" y1="0" x2="0" y2="1">
  <stop offset="0" stop-color="#2fd2ff"/><stop offset="0.35" stop-color="#1192ea"/><stop offset="1" stop-color="#1a1fb8"/>
</linearGradient>
"""
body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="url(#sea)"/>
{caustics}
<path d="{ribbon}" fill="#000"/>
{shards(spots, colors=("#7ff3ff", "#ffffff"))}
<g transform="translate(512 560) scale(0.8) rotate(-7) translate(-548 -524)">
  <clipPath id="gc"><path d="{GHOST}"/></clipPath>
  <path d="{GHOST}" fill="#fff"/>
  <g clip-path="url(#gc)">{poly(shade, fill="#9fb3cf")}</g>
  <path d="{GHOST}" fill="none" stroke="#000" stroke-width="14" stroke-linejoin="round"/>
  {poly(eyeL, fill="#000")}{poly(eyeR, fill="#000")}
</g>
"""
write("07e-sinking-ghost", svg(body, defs), sizes=(1024, 64, 32))
icon_sizes("07e-sinking-ghost")
