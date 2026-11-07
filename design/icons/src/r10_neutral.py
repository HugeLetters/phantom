from lib import *

# Round 10 (65-67): default icon for the neutral rune-kit look (Kit::neutral, dark: #0a0a0a ground, #ededed
# ink). A plain arcade-style ghost: round dome, straight sides, a hem of three rounded scallops, two dark
# oval eyes. Deliberately not Pac-Man's: monochrome, no zigzag hem, no white eyes with pupils.
BG_TOP, BG_BOTTOM, INK = "#2a2a2c", "#0a0a0a", "#ededed"
CX, TOP, R, SIDE_BOTTOM, HEM = 512, 214, 236, 700, 790


def ghost_path(ctrl=SIDE_BOTTOM - 10):
    """`ctrl` is the y of each scallop's quadratic control point; the arch rises halfway to it
    (the default gives a 50 px rise; closer to HEM gives a shallower hem)."""
    L, Rx = CX - R, CX + R
    d = f"M{L},{TOP + R} A{R},{R} 0 0 1 {Rx},{TOP + R} L{Rx},{HEM} "
    w = 2 * R / 3
    for i in range(3):          # three scallops, right to left, each a half-circle bulging upward
        x1 = Rx - w * (i + 1)
        d += f"Q{f(Rx - w * i - w / 2)},{ctrl} {f(x1)},{HEM} "
    return d + "Z"


GHOST_N = ghost_path()
EYES = (f'<ellipse cx="{CX - 86}" cy="430" rx="36" ry="52" fill="{BG_BOTTOM}"/>'
        f'<ellipse cx="{CX + 86}" cy="430" rx="36" ry="52" fill="{BG_BOTTOM}"/>')
DEFS = f"""
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{BG_TOP}"/><stop offset="1" stop-color="{BG_BOTTOM}"/></linearGradient>
<radialGradient id="glow" cx="0.5" cy="0.46" r="0.55"><stop offset="0" stop-color="#3a3a3d"/><stop offset="1" stop-color="{BG_BOTTOM}"/></radialGradient>
<linearGradient id="body" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#c9c9cc"/></linearGradient>
<filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="22"/></filter>
"""
EDGE = '<rect x="101" y="101" width="822" height="822" rx="184" fill="none" stroke="#ffffff" stroke-opacity="0.10" stroke-width="2"/>'


def out(name, body):
    write(name, svg(body, DEFS) .replace("</svg>", EDGE + "</svg>"), sizes=(1024, 64, 32))
    icon_sizes(name)


out("r10-65-neutral-flat", f'<rect width="1024" height="1024" fill="url(#bg)"/><path d="{GHOST_N}" fill="{INK}"/>{EYES}')
out("r10-66-neutral-lit", f"""<rect width="1024" height="1024" fill="url(#glow)"/>
<ellipse cx="{CX}" cy="{HEM + 30}" rx="200" ry="26" fill="#000" opacity="0.55" filter="url(#soft)"/>
<path d="{GHOST_N}" fill="url(#body)"/>{EYES}""")
out("r10-67-neutral-outline", f"""<rect width="1024" height="1024" fill="url(#bg)"/>
<path d="{GHOST_N}" fill="none" stroke="{INK}" stroke-width="40" stroke-linejoin="round"/>
{EYES.replace(f'fill="{BG_BOTTOM}"', f'fill="{INK}"')}""")

# 68: 65 with round eyes
ROUND_EYES = (f'<circle cx="{CX - 86}" cy="436" r="44" fill="{BG_BOTTOM}"/>'
              f'<circle cx="{CX + 86}" cy="436" r="44" fill="{BG_BOTTOM}"/>')
out("r10-68-neutral-round-eyes", f'<rect width="1024" height="1024" fill="url(#bg)"/><path d="{GHOST_N}" fill="{INK}"/>{ROUND_EYES}')

# 69: 68 with rounded bottom corners and hem points, like 67's outline: the ghost is traced with a thick
# same-colour stroke with round joins, scaled down a touch so the extra stroke width doesn't grow it
SOFT = (f'<g transform="translate({CX} 500) scale(0.955) translate(-{CX} -500)">'
        f'<path d="{GHOST_N}" fill="{INK}" stroke="{INK}" stroke-width="40" stroke-linejoin="round"/></g>')
out("r10-69-neutral-soft", f'<rect width="1024" height="1024" fill="url(#bg)"/>{SOFT}{ROUND_EYES}')

# 70: 69 with 65's oval eyes
out("r10-70-neutral-soft-oval", f'<rect width="1024" height="1024" fill="url(#bg)"/>{SOFT}{EYES}')


# 71-72: 70 with a shallower hem (scallops rising about 30 px and 18 px instead of 50)
def soft(d):
    return (f'<g transform="translate({CX} 500) scale(0.955) translate(-{CX} -500)">'
            f'<path d="{d}" fill="{INK}" stroke="{INK}" stroke-width="40" stroke-linejoin="round"/></g>')


out("r10-71-neutral-soft-shallow", f'<rect width="1024" height="1024" fill="url(#bg)"/>{soft(ghost_path(HEM - 60))}{EYES}')
out("r10-72-neutral-soft-flatter", f'<rect width="1024" height="1024" fill="url(#bg)"/>{soft(ghost_path(HEM - 36))}{EYES}')
# 73: halfway between 70 and 71 (scallops rise about 40 px)
out("r10-73-neutral-soft-mid", f'<rect width="1024" height="1024" fill="url(#bg)"/>{soft(ghost_path(HEM - 80))}{EYES}')
# 74: halfway between 73 and 71 (scallops rise about 35 px)
out("r10-74-neutral-soft-35", f'<rect width="1024" height="1024" fill="url(#bg)"/>{soft(ghost_path(HEM - 70))}{EYES}')


# 75: 74's amplitude, but the hem is an even wave: the downward points are rounded exactly like the upward
# bumps (cubic half-waves with flat tangents at every crest and trough). No rounding stroke here, since a
# stroke fattens the outward curves and pinches the inward ones; only the two outer corners are rounded.
def wave_ghost(amp, waves=3, k=0.182, corner=22):
    """k=0.182 gives handles of 0.364 x half-wave, the cubic that best traces a sine."""
    L, Rx = CX - R, CX + R
    d = f"M{L},{TOP + R} A{R},{R} 0 0 1 {Rx},{TOP + R} L{Rx},{HEM - corner} Q{Rx},{HEM} {Rx - corner},{HEM} "
    h = 2 * R / waves / 2                       # half a wave, trough to crest
    x, y = Rx, HEM
    for i in range(waves * 2):
        ny = HEM - amp if i % 2 == 0 else HEM
        nx = x - h
        sx = x - corner if i == 0 else x          # first and last half-waves start/end at the corner arcs
        ex = nx + corner if i == waves * 2 - 1 else nx
        d += f"C{f(sx - h * k * 2)},{f(y)} {f(ex + h * k * 2)},{f(ny)} {f(ex)},{f(ny)} "
        x, y = nx, ny
    return d + f"Q{L},{HEM} {L},{HEM - corner} Z"


GHOST_WAVE = wave_ghost(40)   # a rounded wave reads shallower than cusps, so a touch more than 74's 35
out("r10-75-neutral-wave", f'<rect width="1024" height="1024" fill="url(#bg)"/>'
    f'<path d="{GHOST_WAVE}" fill="{INK}"/>{EYES}')


# 76: every point of the hem rounded with the SAME radius: a zigzag of three hanging lobes (outer corners at
# notch height, so all corners have similar angles), each vertex replaced by a circular fillet of radius rho
def fillet_path(pts, rho):
    """Closed polygon -> path with every vertex rounded by a circular arc of radius rho."""
    n = len(pts)
    d = ""
    for i in range(n):
        A, P, B = pts[i - 1], pts[i], pts[(i + 1) % n]
        u1 = ((A[0] - P[0]), (A[1] - P[1]))
        u2 = ((B[0] - P[0]), (B[1] - P[1]))
        l1, l2 = math.hypot(*u1), math.hypot(*u2)
        u1, u2 = (u1[0] / l1, u1[1] / l1), (u2[0] / l2, u2[1] / l2)
        ang = math.acos(max(-1, min(1, u1[0] * u2[0] + u1[1] * u2[1])))
        if rho == 0 or ang > math.radians(179):
            p1 = p2 = P
            arc = ""
        else:
            t = min(rho / math.tan(ang / 2), l1 * 0.49, l2 * 0.49)
            r = t * math.tan(ang / 2)
            p1 = (P[0] + u1[0] * t, P[1] + u1[1] * t)
            p2 = (P[0] + u2[0] * t, P[1] + u2[1] * t)
            sweep = 1 if (u1[0] * u2[1] - u1[1] * u2[0]) < 0 else 0
            arc = f"A{f(r)},{f(r)} 0 0 {sweep} {f(p2[0])},{f(p2[1])} "
        d += (f"M{f(p1[0])},{f(p1[1])} " if i == 0 else f"L{f(p1[0])},{f(p1[1])} ") + arc
    return d + "Z"


def even_ghost(amp=50, rho=44, lobes=3):
    L, Rx = CX - R, CX + R
    w = 2 * R / lobes
    hem = []
    for i in range(lobes * 2 + 1):           # right to left: corner/notch, bottom, notch, ... corner
        hem.append((Rx - w / 2 * i, HEM - amp if i % 2 == 0 else HEM))
    # dome as a polyline (fine steps, so fillets there are no-ops), then the hem
    dome = [(CX + R * math.cos(a), TOP + R - R * math.sin(a)) for a in [math.pi * k / 48 for k in range(48, -1, -1)]]
    return fillet_path(dome + [(Rx, HEM - amp)] + hem[1:-1] + [(L, HEM - amp)], rho)


out("r10-76-neutral-even", f'<rect width="1024" height="1024" fill="url(#bg)"/><path d="{even_ghost()}" fill="{INK}"/>{EYES}')
# 77: 76 with a bigger radius, so the fillets meet and the hem is one continuous, evenly rounded wave;
# the zigzag is taller (74 vs 50) because the bigger rounding eats into the wave
out("r10-77-neutral-even-round", f'<rect width="1024" height="1024" fill="url(#bg)"/><path d="{even_ghost(amp=74, rho=68)}" fill="{INK}"/>{EYES}')
