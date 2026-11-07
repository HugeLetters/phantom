from lib import *

# Round 7, Persona 3 set (55-58): helpers copied from r6_p3.py (that script draws on import); new scenes below
TEAL, TEAL_D, TEAL_L, HOLE = "#86e6ec", "#3fa9bd", "#d4fbff", "#06122e"
CEL = [(560, 150), (900, 150), (900, 960), (600, 960), (640, 560)]
CEL2 = [(660, 150), (900, 150), (900, 960), (700, 960), (730, 560)]
PLACE = "translate(512 540) scale(0.86) rotate(-4) translate(-515 -586)"
MASK_SCALE = "translate(512 430) scale(1.12) translate(-512 -430)"


def mask():
    return f"""<g transform="{MASK_SCALE}">
<clipPath id="mk"><path d="{OVAL_MASK}"/></clipPath>
<path d="{OVAL_MASK}" fill="#000" transform="translate(7 7)"/>
<path d="{OVAL_MASK}" fill="{TEAL}"/>
<g clip-path="url(#mk)">{poly([(530, 280), (700, 280), (700, 600), (540, 600)], fill=TEAL_D)}
  {poly([(430, 330), (470, 312), (446, 420), (426, 430)], fill=TEAL_L)}</g>
<path d="{OVAL_MASK}" fill="none" stroke="#000" stroke-width="5"/>
<g fill="{HOLE}">{OVAL_EYES}</g><path d="{OVAL_MOUTH}" fill="{HOLE}"/></g>"""


def white_body(uid):
    return f"""
<path d="{SHADOW}" fill="#000" transform="translate(14 10)"/>
<clipPath id="{uid}"><path d="{SHADOW}"/></clipPath>
<path d="{SHADOW}" fill="#fff"/>
<g clip-path="url(#{uid})">{poly(CEL, fill="#b9c6e2")}{poly(CEL2, fill="#8d9fcf")}</g>
<path d="{SHADOW}" fill="none" stroke="#000" stroke-width="6" stroke-linejoin="round"/>"""


def dark_body(uid, sheen, mid, base, extra=""):
    return rim_body(uid, SHADOW, base, sheen, mid, CEL, dx=-24, dy=0) + extra


def veins(seed=3, color="#6ff3ff"):
    """Branching glowing lines clipped to the body (from the boss reference)."""
    rnd = random.Random(seed)
    paths = []
    for sx, sy in [(400, 640), (560, 700), (470, 820), (650, 600), (380, 760)]:
        x, y, a = sx, sy, rnd.uniform(0, 2 * math.pi)
        d = f"M{f(x)},{f(y)}"
        for _ in range(9):
            a += rnd.uniform(-0.9, 0.9)
            x, y = x + 34 * math.cos(a), y + 34 * math.sin(a)
            d += f" L{f(x)},{f(y)}"
        paths.append(d)
    lines = "".join(f'<path d="{p}"/>' for p in paths)
    return (f'<clipPath id="vc"><path d="{SHADOW}"/></clipPath><g clip-path="url(#vc)" fill="none" stroke-linecap="round" stroke-linejoin="round">'
            f'<g stroke="{color}" stroke-width="16" opacity="0.35" filter="url(#vb)">{lines}</g>'
            f'<g stroke="{color}" stroke-width="6">{lines}</g></g>')


def moon(cx, cy, r, fill_id, mottle_color, seed=5):
    rnd = random.Random(seed)
    m = "".join(f'<circle cx="{f(rnd.uniform(cx - r * 0.9, cx + r * 0.9))}" cy="{f(rnd.uniform(cy - r * 0.9, cy + r * 0.9))}" '
                f'r="{f(rnd.uniform(r * 0.05, r * 0.17))}" fill="{mottle_color}" opacity="{rnd.uniform(0.25, 0.55):.2f}"/>'
                for _ in range(18))
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#{fill_id})"/>'
            f'<clipPath id="moonc"><circle cx="{cx}" cy="{cy}" r="{r}"/></clipPath><g clip-path="url(#moonc)">{m}</g>')


def grad(id_, stops, x2=0.25, y2=1):
    s = "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in stops)
    return f'<linearGradient id="{id_}" x1="0" y1="0" x2="{x2}" y2="{y2}">{s}</linearGradient>'


def out(name, body, defs):
    write(name, svg(body, defs), sizes=(1024, 64, 32))
    icon_sizes(name)




def caustics(seed=4, y0=80, y1=300, n=30):
    rnd = random.Random(seed)
    out_ = ""
    for _ in range(n):
        cy = rnd.uniform(y0, y1)
        k = 1 - (cy - y0) / (y1 - y0)
        w, h, cx = rnd.uniform(30, 80) * (0.5 + k), rnd.uniform(10, 22) * (0.5 + k), rnd.uniform(90, 940)
        p = [(cx + w * math.cos(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1),
              cy + h * math.sin(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1)) for i in range(7)]
        out_ += f'<path d="{smooth(p)}" fill="#c4fcff" opacity="{0.2 + 0.5 * k:.2f}"/>'
    return out_


SEA = grad("sea", [(0, "#36d8ff"), (0.3, "#138fea"), (1, "#1a1fb8")], x2=0)
WATER = '<rect width="1024" height="1024" fill="url(#sea)"/>' + caustics() + shards(
    [(200, 520, 18), (860, 640, 18), (820, 860, 12), (240, 840, 14), (620, 930, 10)])
KEYART = ('<rect width="1024" height="1024" fill="#14a5ec"/>' + poly([(0, 470), (620, 1024), (0, 1024)], fill="#000")
          + shards([(250, 230, 26), (860, 690, 22), (820, 860, 16), (880, 470, 14), (330, 160, 14), (660, 920, 12), (150, 600, 12)]))
WATER_WEDGE = ('<rect width="1024" height="1024" fill="url(#sea)"/>' + caustics()
               + poly([(0, 560), (520, 1024), (0, 1024)], fill="#000")
               + shards([(200, 520, 18), (860, 640, 18), (820, 860, 12), (620, 930, 10)]))
DARK = dark_body('a', "#7fb2ff", "#16205a", "#05071a")

# 55: round 6's 37 (dark Shadow, blue sheen) on round 5's 21 underwater background
out("r7-55-shadow-dark-water", WATER + f'<g transform="{PLACE}">{DARK}{mask()}</g>', SEA)
# 56: round 6's 40 with the white Shadow
out("r7-56-shadow-white-keyart", KEYART + f'<g transform="{PLACE}">{white_body("b")}{mask()}</g>', "")
# 57: 40's composition (wedge, shards) under 21's water, dark Shadow
out("r7-57-dark-water-wedge", WATER_WEDGE + f'<g transform="{PLACE}">{dark_body("c", "#7fb2ff", "#16205a", "#05071a")}{mask()}</g>', SEA)
# 58: the same with the white Shadow
out("r7-58-white-water-wedge", WATER_WEDGE + f'<g transform="{PLACE}">{white_body("d")}{mask()}</g>', SEA)
