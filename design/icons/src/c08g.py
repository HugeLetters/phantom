from lib import *

# 08e iterated: centred on its real bounding box; smaller burst with a thinner outline; colour swap
R, NECK, LENGTH, FLARE, TEETH, DEPTH = 230, 160, 430, 210, 3, 60
TILT = -6


def outline_points():
    """Points on the keyhole outline (head circle + body polygon) in local coordinates."""
    ny = math.sqrt(R * R - NECK * NECK)
    a0 = math.atan2(ny, -NECK)
    pts_ = [(512 + R * math.cos(a0 + t * (2 * math.pi - 2 * (a0 - math.pi / 2)) / 60),
             R * math.sin(a0 + t * (2 * math.pi - 2 * (a0 - math.pi / 2)) / 60)) for t in range(61)]
    return pts_ + [(512 + FLARE, LENGTH + DEPTH * 0.15), (512 - FLARE, LENGTH + DEPTH * 0.15)]


def keyhole():
    ny = math.sqrt(R * R - NECK * NECK)
    d = f"M{f(512 - NECK)},{f(ny)} A{R},{R} 0 1 1 {f(512 + NECK)},{f(ny)} L{f(512 + FLARE)},{LENGTH} "
    seg = 2 * FLARE / TEETH
    for i in range(TEETH):
        x = 512 + FLARE - seg * i
        d += f"L{f(x - seg / 2)},{f(LENGTH - DEPTH)} L{f(x - seg)},{f(LENGTH + (DEPTH * 0.15 if i % 2 else 0))} "
    return d + "Z"


def eyes(k=1.15):
    s = R / 180 * k
    def eye(sign):
        p = lambda x, y: f"{f(512 + sign * x * s)},{f(y * s)}"
        return f"M{p(-112, -18)} L{p(-14, -4)} Q{p(-20, 40)} {p(-62, 40)} Q{p(-104, 36)} {p(-112, -18)} Z"
    return eye(1), eye(-1)


def centred(height=620):
    """Transform that scales the figure to `height` and centres its rotated bounding box (with the cut-out offset)."""
    s = height / (LENGTH + R)
    a = math.radians(TILT)
    pts_ = [((x - 512) * math.cos(a) - y * math.sin(a), (x - 512) * math.sin(a) + y * math.cos(a)) for x, y in outline_points()]
    xs, ys = [p[0] * s for p in pts_], [p[1] * s for p in pts_]
    cx, cy = (min(xs) + max(xs)) / 2 + 8, (min(ys) + max(ys)) / 2 + 8
    return f"translate({f(512 - cx)} {f(512 - cy)}) rotate({TILT}) scale({f(s)}) translate(-512 0)", s


HOLE = keyhole()
EL, ER = eyes()


def icon(name, burst_r=None, body="#000", cut="#fff", eye="#fff", outline=0, height=620):
    t, s = centred(height)
    burst_ = (poly(burst(512, 470, 13, burst_r, burst_r * 0.78, jitter=0.16, seed=3, phase=0.2), fill="#000")
              if burst_r else "")
    ring = (f'<path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(outline / s)}" stroke-linejoin="round"/>'
            if outline else "")
    body_ = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 920, 14, 600, step=30)}
{halftone(920, 110, 12, 480, step=30)}
{burst_}
<g transform="{t}">
  {ring}
  <path d="{HOLE}" fill="{cut}" stroke="{cut}" stroke-width="{f(36 / s)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(16 / s)} {f(16 / s)})"/>
  <path d="{HOLE}" fill="{body}"/>
  <path d="{EL}" fill="{eye}"/><path d="{ER}" fill="{eye}"/>
</g>
"""
    write(name, svg(body_), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("08g-bighead-centred")
icon("08h-bighead-small-burst", burst_r=365, outline=24)
icon("08i-bighead-white-burst", burst_r=365, body="#fff", cut="#000", eye="#000")
