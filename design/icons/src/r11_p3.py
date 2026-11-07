from lib import *

# Round 11, Persona 3 dark partners (78-80): 40's key art and 55's water redrawn in 36's Dark Hour
# green, so each can pair with its blue original. Helpers copied from r7_p3.py (that script draws on import).
TEAL, TEAL_D, TEAL_L, HOLE = "#86e6ec", "#3fa9bd", "#d4fbff", "#06122e"
CEL = [(560, 150), (900, 150), (900, 960), (600, 960), (640, 560)]
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


def grad(id_, stops, x2=0.25, y2=1):
    s = "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in stops)
    return f'<linearGradient id="{id_}" x1="0" y1="0" x2="{x2}" y2="{y2}">{s}</linearGradient>'


def caustics(seed=4, y0=80, y1=300, n=30, color="#c4fcff", k0=0.2, k1=0.5):
    rnd = random.Random(seed)
    out_ = ""
    for _ in range(n):
        cy = rnd.uniform(y0, y1)
        k = 1 - (cy - y0) / (y1 - y0)
        w, h, cx = rnd.uniform(30, 80) * (0.5 + k), rnd.uniform(10, 22) * (0.5 + k), rnd.uniform(90, 940)
        p = [(cx + w * math.cos(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1),
              cy + h * math.sin(2 * math.pi * i / 7) * rnd.uniform(0.6, 1.1)) for i in range(7)]
        out_ += f'<path d="{smooth(p)}" fill="{color}" opacity="{k0 + k1 * k:.2f}"/>'
    return out_


def out(name, body, defs=""):
    write(name, svg(body, defs), sizes=(1024, 64, 32))
    icon_sizes(name)


# 36's Dark Hour colours: sky gradient, moon lime, green sheen on the body
DH_SKY = grad("dh", [(0, "#2c4a1c"), (1, "#081208")], x2=0)
LIME, LIME_L, LIME_D = "#c6e65a", "#f4ff9a", "#8fbf3a"
GREEN_SHARDS = ("#f4ff9a", "#c6e65a", "#e8ffd0")
GREEN = rim_body("a", SHADOW, "#040806", "#b9e85a", "#14301c", CEL, dx=-24, dy=0)
SHARD_SPOTS = [(250, 230, 26), (860, 690, 22), (820, 860, 16), (880, 470, 14), (330, 160, 14), (660, 920, 12), (150, 600, 12)]
WEDGE = [(0, 470), (620, 1024), (0, 1024)]

# 78: 40 at the Dark Hour: dark green sky, black wedge, lime shards
out("r11-78-keyart-dark-hour", f"""<rect width="1024" height="1024" fill="url(#dh)"/>{poly(WEDGE, fill="#000")}
{shards(SHARD_SPOTS, colors=GREEN_SHARDS)}<g transform="{PLACE}">{GREEN}{mask()}</g>""", DH_SKY)
# 79: the same with the wedge in the moon's lime, so the wedge still reads against the dark sky
out("r11-79-keyart-lime-wedge", f"""<rect width="1024" height="1024" fill="url(#dh)"/>{poly(WEDGE, fill=LIME)}
{shards(SHARD_SPOTS, colors=GREEN_SHARDS)}<g transform="{PLACE}">{GREEN}{mask()}</g>""", DH_SKY)
# 80: 55's water at the Dark Hour: green-black depths with lime caustics at the surface
DH_SEA = grad("dsea", [(0, "#4f7a22"), (0.3, "#1f3d14"), (1, "#050c05")], x2=0)
out("r11-80-water-dark-hour", f"""<rect width="1024" height="1024" fill="url(#dsea)"/>{caustics(color=LIME_L, k0=0.1, k1=0.45)}
{shards([(200, 520, 18), (860, 640, 18), (820, 860, 12), (240, 840, 14), (620, 930, 10)], colors=GREEN_SHARDS)}
<g transform="{PLACE}">{GREEN}{mask()}</g>""", DH_SEA)
