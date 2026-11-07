from lib import *

# 07 round 4: Shadow-style ghost with an oval teal mask, centred, on mixes of the 07f and 07h backgrounds
TEAL, TEAL_D = "#7fe3ea", "#43b3c4"
BODY_SHADE = [(560, 150), (900, 150), (900, 960), (600, 960), (640, 560)]
MASK_SHADE = [(530, 280), (700, 280), (700, 600), (540, 600)]


def figure(body, shade, rim, place):
    return f"""
<g transform="{place}">
  <clipPath id="sc"><path d="{SHADOW}"/></clipPath>
  <path d="{SHADOW}" fill="{body}" stroke="{rim}" stroke-width="12" stroke-linejoin="round"/>
  <g clip-path="url(#sc)">{poly(BODY_SHADE, fill=shade)}</g>
  <clipPath id="mc"><path d="{OVAL_MASK}"/></clipPath>
  <path d="{OVAL_MASK}" fill="{TEAL}" stroke="#000" stroke-width="10"/>
  <g clip-path="url(#mc)">{poly(MASK_SHADE, fill=TEAL_D)}</g>
  <g fill="#06122e">{OVAL_EYES}</g>
  <path d="{OVAL_MOUTH}" fill="#06122e"/>
</g>"""


def grad(id_, stops, x2=0.25, y2=1):
    s = "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in stops)
    return f'<linearGradient id="{id_}" x1="0" y1="0" x2="{x2}" y2="{y2}">{s}</linearGradient>'


def caustics(seed=4, y0=80, y1=300, n=30):
    rnd = random.Random(seed)
    out = ""
    for _ in range(n):
        cy = rnd.uniform(y0, y1)
        k = 1 - (cy - y0) / (y1 - y0)
        w, h, cx = rnd.uniform(30, 80) * (0.5 + k), rnd.uniform(10, 22) * (0.5 + k), rnd.uniform(90, 940)
        p = [(cx + w * math.cos(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1),
              cy + h * math.sin(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1)) for i in range(7)]
        out += f'<path d="{smooth(p)}" fill="#c4fcff" opacity="{0.2 + 0.5 * k:.2f}"/>'
    return out


def out(name, body, defs):
    write(name, svg(body, defs), sizes=(1024, 64, 32))
    icon_sizes(name)


SKY = grad("sky", [(0, "#22c4ff"), (0.55, "#1488ec"), (1, "#1b2bc4")])
SEA = grad("sea", [(0, "#36d8ff"), (0.3, "#138fea"), (1, "#1a1fb8")], x2=0)
CENTER = "translate(512 530) scale(0.86) rotate(-4) translate(-515 -586)"
WEDGE = poly([(0, 470), (620, 1024), (0, 1024)], fill="#000")
spots = [(250, 230, 24), (860, 690, 20), (820, 860, 14), (880, 470, 12), (330, 160, 12), (660, 920, 12)]
dark = [(170, 860, 22), (300, 950, 12), (110, 660, 10)]

# white cel-shaded Shadow with the teal mask, on 07f's gradient + wedge
out("07i-shadow-mask-white", f"""
<rect width="1024" height="1024" fill="url(#sky)"/>{WEDGE}
{shards(spots)}{shards(dark, seed=3, colors=("#3a3f9e", "#5a4fc0"), opacity=0.9)}
{figure("#fff", "#b4c2dc", "#000", CENTER)}
""", SKY)

# dark Shadow (black body, navy cel shade, blue rim) under water with the wedge
out("07j-shadow-dark-underwater", f"""
<rect width="1024" height="1024" fill="url(#sea)"/>
{caustics()}
{poly([(0, 560), (520, 1024), (0, 1024)], fill="#000")}
{shards([(200, 520, 18), (860, 640, 18), (820, 860, 12), (620, 930, 10)])}
{figure("#05081c", "#1d2f7a", "#7fe3ff", CENTER)}
""", SEA)

# white Shadow under water, centred (07h's scene with the new ghost)
out("07k-shadow-white-underwater", f"""
<rect width="1024" height="1024" fill="url(#sea)"/>
{caustics()}
{shards([(200, 520, 18), (860, 640, 18), (820, 860, 12), (240, 840, 14), (620, 930, 10)])}
{figure("#fff", "#b4c2dc", "#000", CENTER)}
""", SEA)

# Dark Hour: sickly green moon, dark Shadow silhouette with a cyan rim
DARK = (grad("dh", [(0, "#2c4a1c"), (1, "#081208")], x2=0)
        + '<radialGradient id="gm" cx="0.45" cy="0.4" r="0.6"><stop offset="0" stop-color="#f4ff9a"/>'
        '<stop offset="0.7" stop-color="#c6e65a"/><stop offset="1" stop-color="#8fbf3a"/></radialGradient>')
rnd = random.Random(5)
mottle = "".join(f'<circle cx="{f(rnd.uniform(300, 880))}" cy="{f(rnd.uniform(120, 560))}" r="{f(rnd.uniform(14, 50))}" '
                 f'fill="#9dc24a" opacity="{rnd.uniform(0.25, 0.55):.2f}"/>' for _ in range(18))
out("07l-shadow-dark-hour", f"""
<rect width="1024" height="1024" fill="url(#dh)"/>
<circle cx="600" cy="380" r="300" fill="url(#gm)"/>
<clipPath id="moon"><circle cx="600" cy="380" r="300"/></clipPath><g clip-path="url(#moon)">{mottle}</g>
{figure("#05080c", "#1b3a2a", "#7fe3ea", CENTER)}
""", DARK)
