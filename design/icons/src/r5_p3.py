from lib import *

# Round 5, Persona 3 set: both ghost shapes with the teal enemy-style mask, drawn with tapered ink
# lines, two-tone cel shading, blue sheen instead of outlines on dark bodies, and blue liquid flame
TEAL, TEAL_D, TEAL_L, HOLE = "#86e6ec", "#3fa9bd", "#d4fbff", "#06122e"
CEL = [(560, 150), (900, 150), (900, 960), (600, 960), (640, 560)]
CEL2 = [(660, 150), (900, 150), (900, 960), (700, 960), (730, 560)]


def mask():
    return f"""
<clipPath id="mk"><path d="{OVAL_MASK}"/></clipPath>
<path d="{OVAL_MASK}" fill="#000" transform="translate(8 8)"/>
<path d="{OVAL_MASK}" fill="{TEAL}"/>
<g clip-path="url(#mk)">{poly([(530, 280), (700, 280), (700, 600), (540, 600)], fill=TEAL_D)}
  {poly([(430, 330), (470, 312), (446, 420), (426, 430)], fill=TEAL_L)}</g>
<path d="{OVAL_MASK}" fill="none" stroke="#000" stroke-width="6"/>
<g fill="{HOLE}">{OVAL_EYES}</g><path d="{OVAL_MOUTH}" fill="{HOLE}"/>"""


def white_body(d, uid):
    """White body, blue-grey two-tone cel, ink line heavier on the shadow side (offset black copy)."""
    return f"""
<path d="{d}" fill="#000" transform="translate(14 10)"/>
<clipPath id="{uid}"><path d="{d}"/></clipPath>
<path d="{d}" fill="#fff"/>
<g clip-path="url(#{uid})">{poly(CEL, fill="#b9c6e2")}{poly(CEL2, fill="#8d9fcf")}</g>
<path d="{d}" fill="none" stroke="#000" stroke-width="6" stroke-linejoin="round"/>"""


def dark_body(d, uid, sheen="#3f7dff", mid="#16205a", base="#05071a"):
    return rim_body(uid, d, base, sheen, mid, CEL, dx=-24, dy=0)


def flames(spots, color="#1d3fd0", light="#58a8ff", seed=1):
    """Liquid flame tongues: (x, base_y, height, half_width, lean)."""
    def tongue(x, y, h, w, lean):
        # pointed at both ends: a curling liquid blade from (x, y) up to (x + lean, y - h)
        return (f"M{f(x)},{f(y)} C{f(x - w)},{f(y - h * 0.3)} {f(x + lean - w * 0.6)},{f(y - h * 0.7)} {f(x + lean)},{f(y - h)} "
                f"C{f(x + lean + w * 0.15)},{f(y - h * 0.6)} {f(x + w * 0.7)},{f(y - h * 0.3)} {f(x)},{f(y)} Z")
    out = ""
    for x, y, h, w, lean in spots:
        out += f'<path d="{tongue(x, y, h, w, lean)}" fill="{color}"/>'
        out += f'<path d="{tongue(x + w * 0.15, y, h * 0.55, w * 0.45, lean * 0.6)}" fill="{light}" opacity="0.8"/>'
    return out


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


SKY = grad("sky", [(0, "#22c4ff"), (0.55, "#1488ec"), (1, "#1b2bc4")])
SEA = grad("sea", [(0, "#36d8ff"), (0.3, "#138fea"), (1, "#1a1fb8")], x2=0)
WEDGE = poly([(0, 470), (620, 1024), (0, 1024)], fill="#000")
SPOTS = [(250, 230, 24), (860, 690, 20), (820, 860, 14), (880, 470, 12), (330, 160, 12), (660, 920, 12)]
SEA_SPOTS = [(200, 520, 18), (860, 640, 18), (820, 860, 12), (240, 840, 14), (620, 930, 10)]
G_PLACE = "translate(512 526) scale(0.8) rotate(-7) translate(-548 -524)"     # the 01 ghost
S_PLACE = "translate(512 530) scale(0.86) rotate(-4) translate(-515 -586)"    # the draped Shadow
HEM_FLAMES = [(430, 820, 430, 110, -300), (600, 820, 430, 110, 300), (380, 930, 300, 80, -230), (660, 930, 300, 80, 230), (520, 960, 240, 60, 60)]
G_FLAMES = [(440, 780, 420, 110, -300), (640, 780, 420, 110, 300), (390, 880, 300, 80, -230), (690, 880, 300, 80, 230), (540, 900, 240, 60, 60)]


def out(name, body, defs):
    write(name, svg(body, defs), sizes=(1024, 64, 32))
    icon_sizes(name)


def ghost(body):
    return f'<g transform="{G_PLACE}">{body}{mask()}</g>'


def shadow(body, behind=""):
    return f'<g transform="{S_PLACE}">{behind}{body}{mask()}</g>'


out("r5-17-ghost-sky", f"""<rect width="1024" height="1024" fill="url(#sky)"/>{WEDGE}{shards(SPOTS)}
{ghost(white_body(GHOST, 'g1'))}""", SKY)
out("r5-18-ghost-water", f"""<rect width="1024" height="1024" fill="url(#sea)"/>{caustics()}{shards(SEA_SPOTS)}
{ghost(white_body(GHOST, 'g2'))}""", SEA)
out("r5-19-ghost-flame", f"""<rect width="1024" height="1024" fill="url(#sea)"/>{caustics()}{shards(SEA_SPOTS)}
<g transform="{G_PLACE}">{flames(G_FLAMES)}{dark_body(GHOST, 'g3')}{mask()}</g>""", SEA)
out("r5-20-shadow-flame", f"""<rect width="1024" height="1024" fill="url(#sea)"/>{caustics()}{shards(SEA_SPOTS)}
{shadow(dark_body(SHADOW, 's1'), flames(HEM_FLAMES))}""", SEA)
out("r5-21-shadow-white", f"""<rect width="1024" height="1024" fill="url(#sea)"/>{caustics()}{shards(SEA_SPOTS)}
{shadow(white_body(SHADOW, 's2'))}""", SEA)
out("r5-22-shadow-sky", f"""<rect width="1024" height="1024" fill="url(#sky)"/>{WEDGE}{shards(SPOTS)}
{shadow(dark_body(SHADOW, 's3'), flames(HEM_FLAMES))}""", SKY)

# Dark Hour: green moon; sheen in sickly green instead of blue
DARK = (grad("dh", [(0, "#2c4a1c"), (1, "#081208")], x2=0)
        + '<radialGradient id="gm" cx="0.45" cy="0.4" r="0.6"><stop offset="0" stop-color="#f4ff9a"/>'
        '<stop offset="0.7" stop-color="#c6e65a"/><stop offset="1" stop-color="#8fbf3a"/></radialGradient>')
rnd = random.Random(5)
MOTTLE = "".join(f'<circle cx="{f(rnd.uniform(300, 880))}" cy="{f(rnd.uniform(120, 560))}" r="{f(rnd.uniform(14, 50))}" '
                 f'fill="#9dc24a" opacity="{rnd.uniform(0.25, 0.55):.2f}"/>' for _ in range(18))
MOON = f'<circle cx="600" cy="380" r="300" fill="url(#gm)"/><clipPath id="moon"><circle cx="600" cy="380" r="300"/></clipPath><g clip-path="url(#moon)">{MOTTLE}</g>'
out("r5-23-hour-shadow", f"""<rect width="1024" height="1024" fill="url(#dh)"/>{MOON}
{shadow(dark_body(SHADOW, 's4', sheen="#b9e85a", mid="#14301c", base="#040806"))}""", DARK)
out("r5-24-hour-ghost", f"""<rect width="1024" height="1024" fill="url(#dh)"/>{MOON}
<g transform="{G_PLACE}">{dark_body(GHOST, 'g5', sheen="#b9e85a", mid="#14301c", base="#040806")}{mask()}</g>""", DARK)
