from lib import *

# Persona 3 set, round 3: 07d's cel-shaded masked ghost (with the simpler glasses) on new backgrounds,
# plus a flat cyan silhouette version
SHADE = [(600, 150), (900, 150), (900, 900), (640, 900), (700, 560)]


def ghost(fill="#fff", shade="#b4c2dc", mask="#000", eyes="#fff", outline="#fff", place="rotate(-9 512 512)"):
    return f"""
<g transform="{place}">
  <clipPath id="gc"><path d="{GHOST}"/></clipPath>
  <path d="{GHOST}" fill="{fill}"/>
  <g clip-path="url(#gc)">{poly(SHADE, fill=shade)}</g>
  <path d="{MASK2}" fill="{mask}" stroke="{outline}" stroke-width="22" stroke-linejoin="round" paint-order="stroke"/>
  <path d="{MASK2_EYE_L}" fill="{eyes}"/><path d="{MASK2_EYE_R}" fill="{eyes}"/>
</g>"""


def grad(id_, stops, x2=0.25, y2=1):
    s = "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in stops)
    return f'<linearGradient id="{id_}" x1="0" y1="0" x2="{x2}" y2="{y2}">{s}</linearGradient>'


def out(name, body, defs):
    write(name, svg(body, defs), sizes=(1024, 64, 32))
    icon_sizes(name)


SKY = grad("sky", [(0, "#22c4ff"), (0.55, "#1488ec"), (1, "#1b2bc4")])

# 07f: gradient + the black diagonal wedge from the key art, shards
spots = [(250, 230, 26), (860, 690, 22), (820, 860, 14), (880, 470, 12), (330, 160, 12), (620, 900, 12)]
dark = [(170, 840, 24), (300, 930, 14), (130, 640, 12), (420, 960, 10)]
out("07f-p3-gradient-wedge", f"""
<rect x="0" y="0" width="1024" height="1024" fill="url(#sky)"/>
{poly([(0, 400), (700, 1024), (0, 1024)], fill="#000")}
{shards(spots)}
{shards(dark, seed=3, colors=("#3a3f9e", "#5a4fc0"), opacity=0.9)}
{ghost()}
""", SKY)

# 07g: big moon behind dark window bars, ghost as a flat cyan silhouette with a navy mask
rnd = random.Random(5)
mottle = "".join(f'<circle cx="{f(rnd.uniform(380, 900))}" cy="{f(rnd.uniform(120, 640))}" r="{f(rnd.uniform(14, 60))}" '
                 f'fill="#a9c2ee" opacity="{rnd.uniform(0.25, 0.6):.2f}"/>' for _ in range(22))
NIGHT = grad("night", [(0, "#2d64e0"), (1, "#0a1666")]) + '<clipPath id="moon"><circle cx="640" cy="380" r="290"/></clipPath>'
out("07g-p3-moon-window", f"""
<rect x="0" y="0" width="1024" height="1024" fill="url(#night)"/>
<circle cx="640" cy="380" r="290" fill="#e6f0ff"/>
<g clip-path="url(#moon)">{mottle}</g>
<g fill="#0b1650"><rect x="560" y="0" width="40" height="1024"/><rect x="0" y="470" width="1024" height="40"/>
  <rect x="140" y="0" width="34" height="1024"/></g>
{ghost(fill="#4cc4f4", shade="#2c9fe0", mask="#0b1650", eyes="#e6f0ff", outline="#4cc4f4",
       place="translate(500 560) scale(0.86) rotate(-6) translate(-548 -524)")}
""", NIGHT)

# 07h: underwater, cyan surface with caustic light and light shafts, ghost sinking
rnd = random.Random(4)
caustics = ""


def blob(cx, cy, w, h, n=7):
    """Smooth irregular closed shape (Catmull-Rom through jittered points on an ellipse)."""
    p = [(cx + w * math.cos(2 * math.pi * i / n) * rnd.uniform(0.6, 1.1),
          cy + h * math.sin(2 * math.pi * i / n) * rnd.uniform(0.6, 1.1)) for i in range(n)]
    d = f"M{f(p[0][0])},{f(p[0][1])} "
    for i in range(n):
        p0, p1, p2, p3 = p[i - 1], p[i], p[(i + 1) % n], p[(i + 2) % n]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)
        d += f"C{f(c1[0])},{f(c1[1])} {f(c2[0])},{f(c2[1])} {f(p2[0])},{f(p2[1])} "
    return d + "Z"


for _ in range(34):
    cy = rnd.uniform(80, 320)
    k = 1 - (cy - 80) / 300
    caustics += (f'<path d="{blob(rnd.uniform(90, 940), cy, rnd.uniform(30, 80) * (0.5 + k), rnd.uniform(10, 22) * (0.5 + k))}" '
                 f'fill="#c4fcff" opacity="{0.25 + 0.5 * k:.2f}"/>')
rays = "".join(poly([(x, 100), (x + w, 100), (x + w + 120, 1024), (x + 60, 1024)], fill="#bff6ff", opacity="0.12")
               for x, w in [(300, 40), (520, 70), (700, 30)])
SEA = grad("sea", [(0, "#36d8ff"), (0.3, "#138fea"), (1, "#1a1fb8")], x2=0)
out("07h-p3-underwater", f"""
<rect x="0" y="0" width="1024" height="1024" fill="url(#sea)"/>
{rays}
{caustics}
{shards([(200, 560, 20), (860, 640, 18), (820, 860, 12), (240, 820, 14), (600, 920, 10)])}
{ghost(place="translate(512 580) scale(0.82) rotate(-9) translate(-548 -524)")}
""", SEA)
