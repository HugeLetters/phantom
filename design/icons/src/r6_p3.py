from lib import *

# Round 6, Persona 3 set (36-40): the draped Shadow only, bigger mask, moons instead of water/flames
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


BLUR = '<filter id="vb" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="8"/></filter>'
DARK_HOUR = (grad("dh", [(0, "#2c4a1c"), (1, "#081208")], x2=0)
             + '<radialGradient id="gm" cx="0.45" cy="0.4" r="0.6"><stop offset="0" stop-color="#f4ff9a"/>'
             '<stop offset="0.7" stop-color="#c6e65a"/><stop offset="1" stop-color="#8fbf3a"/></radialGradient>')
NIGHT = (grad("night", [(0, "#2d64e0"), (1, "#0a1666")], x2=0)
         + '<radialGradient id="bm" cx="0.42" cy="0.38" r="0.65"><stop offset="0" stop-color="#f4f9ff"/>'
         '<stop offset="0.75" stop-color="#cfe0ff"/><stop offset="1" stop-color="#9dbbf0"/></radialGradient>')

# 36: round 5's 23 (Dark Hour) with the bigger mask
out("r6-36-hour-shadow", f"""<rect width="1024" height="1024" fill="url(#dh)"/>{moon(600, 380, 300, "gm", "#9dc24a")}
<g transform="{PLACE}">{dark_body('a', "#b9e85a", "#14301c", "#040806")}{mask()}</g>""", DARK_HOUR)
# 37: the same scene with a pale blue P3 moon on a night-blue sky, blue sheen
out("r6-37-blue-moon-shadow", f"""<rect width="1024" height="1024" fill="url(#night)"/>{moon(600, 380, 300, "bm", "#a9c2ee")}
<g transform="{PLACE}">{dark_body('b', "#7fb2ff", "#16205a", "#05071a")}{mask()}</g>""", NIGHT)
# 38: the white Shadow (round 5's 21) against the blue moon, so the white body has a dark sky around it
out("r6-38-blue-moon-white", f"""<rect width="1024" height="1024" fill="url(#night)"/>{moon(640, 360, 280, "bm", "#a9c2ee")}
<g transform="{PLACE}">{white_body('c')}{mask()}</g>""", NIGHT)
# 39: dark Shadow with glowing cyan veins under the blue moon
out("r6-39-vein-shadow", f"""<rect width="1024" height="1024" fill="url(#night)"/>{moon(600, 380, 300, "bm", "#a9c2ee")}
<g transform="{PLACE}">{dark_body('d', "#7fb2ff", "#16205a", "#05071a", veins())}{mask()}</g>""", NIGHT + BLUR)
# 40: key-art composition without water or flames: flat azure, black wedge, shards, dark Shadow
out("r6-40-keyart-shadow", f"""<rect width="1024" height="1024" fill="#14a5ec"/>
{poly([(0, 470), (620, 1024), (0, 1024)], fill="#000")}
{shards([(250, 230, 26), (860, 690, 22), (820, 860, 16), (880, 470, 14), (330, 160, 14), (660, 920, 12), (150, 600, 12)])}
<g transform="{PLACE}">{dark_body('e', "#7fb2ff", "#16205a", "#05071a")}{mask()}</g>""", "")
