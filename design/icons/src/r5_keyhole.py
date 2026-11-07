from lib import *

# Round 5, keyhole ghost (from 08g/08h/08i): angular eyes, colour swap, reworked burst
R, NECK, LENGTH, FLARE, TEETH, DEPTH, TILT = 230, 160, 430, 210, 3, 60, -6


def keyhole():
    ny = math.sqrt(R * R - NECK * NECK)
    d = f"M{f(512 - NECK)},{f(ny)} A{R},{R} 0 1 1 {f(512 + NECK)},{f(ny)} L{f(512 + FLARE)},{LENGTH} "
    seg = 2 * FLARE / TEETH
    for i in range(TEETH):
        x = 512 + FLARE - seg * i
        d += f"L{f(x - seg / 2)},{f(LENGTH - DEPTH)} L{f(x - seg)},{f(LENGTH + (DEPTH * 0.15 if i % 2 else 0))} "
    return d + "Z"


def angular_eyes(k=1.25):
    """CALM_EYE_L re-centred on the head (head centre is 512,0 here) and scaled."""
    s = R / 180 * k * 0.62
    L = [(512 + (x - 512) * s, (y - 430) * s + R * 0.06) for x, y in CALM_EYE_L]
    return L, mirror(L)


def centred(height=620):
    s = height / (LENGTH + R)
    ny = math.sqrt(R * R - NECK * NECK)
    a0 = math.atan2(ny, -NECK)
    pts_ = [(512 + R * math.cos(a0 + t * (2 * math.pi - 2 * (a0 - math.pi / 2)) / 60),
             R * math.sin(a0 + t * (2 * math.pi - 2 * (a0 - math.pi / 2)) / 60)) for t in range(61)]
    pts_ += [(512 + FLARE, LENGTH), (512 - FLARE, LENGTH)]
    a = math.radians(TILT)
    rp = [((x - 512) * math.cos(a) - y * math.sin(a), (x - 512) * math.sin(a) + y * math.cos(a)) for x, y in pts_]
    xs, ys = [p[0] * s for p in rp], [p[1] * s for p in rp]
    cx, cy = (min(xs) + max(xs)) / 2 + 8, (min(ys) + max(ys)) / 2 + 8
    return f"translate({f(512 - cx)} {f(512 - cy)}) rotate({TILT}) scale({f(s)}) translate(-512 0)", s


HOLE = keyhole()
EL, ER = angular_eyes()


def icon(name, body="#000", cut="#fff", eye="#fff", burst_=""):
    t, s = centred()
    content = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 920, 14, 600, step=30)}
{halftone(920, 110, 12, 480, step=30)}
{burst_}
<g transform="{t}">
  <path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(36 / s)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(16 / s)} {f(16 / s)})"/>
  <path d="{HOLE}" fill="{body}"/>
  {poly(EL, fill=eye)}{poly(ER, fill=eye)}
</g>
"""
    write(name, svg(content), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r5-14-keyhole")
icon("r5-15-keyhole-white", body="#fff", cut="#000", eye="#000")
# the burst centred on the whole figure, regular points, no extra outline ring; ghost white so it doesn't merge
icon("r5-16-white-burst", body="#fff", cut="#000", eye="#000",
     burst_=poly(burst(512, 500, 14, 345, 292, jitter=0.06, seed=3, phase=0.1), fill="#000"))
