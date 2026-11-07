import math, random, re, subprocess, os, sys

WORK = os.path.dirname(os.path.abspath(__file__))
RENDER = os.path.join(WORK, "render/target/release/render")
OUT = os.path.join(WORK, "iter")
os.makedirs(OUT, exist_ok=True)

RED = "#e60012"
SQ = 'x="100" y="100" width="824" height="824" rx="185" ry="185"'


def f(v):
    return f"{v:.1f}".rstrip("0").rstrip(".")


def pts(points):
    return " ".join(f"{f(x)},{f(y)}" for x, y in points)


def poly(points, **attrs):
    a = " ".join(f'{k.replace("_", "-")}="{v}"' for k, v in attrs.items())
    return f'<polygon points="{pts(points)}" {a}/>'


def rot(points, deg, cx=512, cy=512):
    r = math.radians(deg)
    c, s = math.cos(r), math.sin(r)
    return [(cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c) for x, y in points]


def burst(cx, cy, n, r_out, r_in, jitter=0.0, seed=1, phase=0.0):
    rnd = random.Random(seed)
    out = []
    for i in range(n * 2):
        a = phase + math.pi * i / n
        r = r_out if i % 2 == 0 else r_in
        r *= 1 + rnd.uniform(-jitter, jitter)
        a += rnd.uniform(-jitter, jitter) * math.pi / n
        out.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return out


def halftone(cx, cy, maxr, reach, step=28, color="#000", angle=45, opacity=1.0, minr=1.5):
    """Dot grid (rotated) whose dot radius falls off with distance from (cx,cy)."""
    dots = []
    r = math.radians(angle)
    c, s = math.cos(r), math.sin(r)
    n = int(1400 / step)
    for i in range(-n, n):
        for j in range(-n, n):
            u, v = i * step, j * step
            x = 512 + u * c - v * s
            y = 512 + u * s + v * c
            if not (60 < x < 964 and 60 < y < 964):
                continue
            d = math.hypot(x - cx, y - cy)
            rad = maxr * (1 - d / reach)
            if rad > minr:
                dots.append(f'<circle cx="{f(x)}" cy="{f(y)}" r="{f(rad)}"/>')
    return f'<g fill="{color}" opacity="{opacity}">' + "".join(dots) + "</g>"


def svg(body, defs=""):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
        'width="1024" height="1024" viewBox="0 0 1024 1024">'
        f'<defs><clipPath id="sq"><rect {SQ}/></clipPath>{defs}</defs>'
        f'<g clip-path="url(#sq)">{body}</g></svg>'
    )


def write(name, content, sizes=(1024,)):
    p = os.path.join(OUT, name + ".svg")
    open(p, "w").write(content)
    for s in sizes:
        suffix = "" if s == 1024 else f"-{s}"
        subprocess.run([RENDER, p, os.path.join(OUT, f"{name}{suffix}.png"), str(s)], check=True)
    return p


def preview(name):
    """Side-by-side: 1024 render at 512, 64px at 4x and 32px at 8x (nearest neighbour), plus true size."""
    s = (
        '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1150" height="540">'
        '<rect width="1150" height="540" fill="#8a8a8a"/>'
        f'<image x="10" y="14" width="512" height="512" xlink:href="{name}.png"/>'
        f'<image x="540" y="14" width="256" height="256" image-rendering="optimizeSpeed" xlink:href="{name}-64.png"/>'
        f'<image x="820" y="14" width="256" height="256" image-rendering="optimizeSpeed" xlink:href="{name}-32.png"/>'
        f'<image x="540" y="300" width="64" height="64" xlink:href="{name}-64.png"/>'
        f'<image x="640" y="316" width="32" height="32" xlink:href="{name}-32.png"/>'
        "</svg>"
    )
    p = os.path.join(OUT, name + "-preview.svg")
    open(p, "w").write(s)
    subprocess.run([RENDER, p, os.path.join(OUT, name + "-preview.png"), "1150"], check=True)


def mirror(points, cx=512):
    return [(2 * cx - x, y) for x, y in points]


def icon_sizes(name):
    """Real-size sheet: 128/64/32/16 px on a light and a dark strip (Finder / Dock), plus 32px zoomed 6x."""
    for s in (128, 16):
        subprocess.run([RENDER, os.path.join(OUT, name + ".svg"), os.path.join(OUT, f"{name}-{s}.png"), str(s)], check=True)
    row = ""
    for y, bg in ((0, "#ececec"), (220, "#1e1e1e")):
        row += f'<rect y="{y}" width="820" height="220" fill="{bg}"/>'
        x = 30
        for s in (128, 64, 32, 16):
            row += f'<image x="{x}" y="{y + 110 - s // 2}" width="{s}" height="{s}" xlink:href="{name}-{s}.png"/>'
            x += s + 40
    row += f'<image x="560" y="30" width="192" height="192" image-rendering="optimizeSpeed" xlink:href="{name}-32.png"/>'
    s = ('<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="820" height="440">'
         + row + "</svg>")
    p = os.path.join(OUT, name + "-sizes.svg")
    open(p, "w").write(s)
    subprocess.run([RENDER, p, os.path.join(OUT, name + "-sizes.png"), "820"], check=True)


# the 01 ghost and its faceted domino mask (01b), shared by later concepts
GHOST = (
    "M292,500 C292,330 392,226 516,226 C648,226 736,330 736,490 "
    "L742,640 L812,800 L700,738 L664,822 L594,730 L532,812 L480,722 L408,786 L384,700 L318,742 L300,620 Z"
)
_half = [(514, 424), (466, 390), (340, 392), (214, 328), (268, 452), (346, 516), (452, 506), (514, 470)]
MASK = _half + mirror(_half[1:-1], 514)[::-1]
MASK_EYE_L = [(308, 408), (478, 444), (458, 490), (364, 484)]
MASK_EYE_R = mirror(MASK_EYE_L, 514)


AZURE = "#14a5ec"
CYAN = "#7ff3ff"


def shards(spots, seed=7, colors=("#7ff3ff", "#ffffff", "#2fd6ff"), opacity=0.85):
    """Scattered glass fragments: (cx, cy, size) per shard."""
    rnd = random.Random(seed)
    out = ""
    for cx, cy, s in spots:
        p = [(cx - s, cy - s * 0.4), (cx + s * 0.3, cy - s * 0.8), (cx + s, cy + s * 0.3), (cx - s * 0.2, cy + s * 0.7)]
        out += poly(rot(p, rnd.uniform(0, 360), cx, cy), fill=rnd.choice(colors), opacity=opacity)
    return out


# round 3: simpler domino mask: one point per wing, soft dip at the bridge, rounded lens bottoms
MASK2 = ("M236,332 L466,394 Q514,408 562,394 L792,332 L746,450 Q704,504 602,498 L514,464 "
         "L426,498 Q324,504 282,450 Z")
MASK2_EYE_L = "M318,410 L470,438 Q452,480 398,478 Q346,474 318,410 Z"
MASK2_EYE_R = "M710,410 L558,438 Q576,480 630,478 Q682,474 710,410 Z"


# round 4: flat, straight-cut masks (few vertices, no curves); triangle eyes
_half3 = [(514, 420), (470, 398), (214, 344), (300, 488), (430, 502), (514, 464)]
MASK3 = _half3 + mirror(_half3[1:-1], 514)[::-1]
MASK3_EYE_L = [(306, 414), (476, 440), (414, 486)]
MASK3_EYE_R = mirror(MASK3_EYE_L, 514)
# a single slanted blindfold band with knife-point ends
MASK4 = [(196, 330), (840, 372), (800, 470), (240, 470)]
MASK4_EYE_L = [(306, 398), (472, 418), (420, 456)]
MASK4_EYE_R = [(722, 410), (556, 420), (606, 458)]


def stars(cx, cy, n, r0, step, colors, ratio=0.62, phase=-math.pi / 2, rotate_step=0.0):
    """Concentric star outlines, largest first, alternating colours (P5-style tonal pattern)."""
    out, r, i = "", r0, 0
    while r > step:
        pts_ = []
        for k in range(n * 2):
            a = phase + i * rotate_step + math.pi * k / n
            rr = r if k % 2 == 0 else r * ratio
            pts_.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
        out += poly(pts_, fill=colors[i % len(colors)])
        r -= step
        i += 1
    return out


def rings(cx, cy, r0, step, colors):
    out, r, i = "", r0, 0
    while r > 0:
        out += f'<circle cx="{f(cx)}" cy="{f(cy)}" r="{f(r)}" fill="{colors[i % len(colors)]}"/>'
        r -= step
        i += 1
    return out


def smooth(p):
    """Closed Catmull-Rom path through points."""
    n = len(p)
    d = f"M{f(p[0][0])},{f(p[0][1])} "
    for i in range(n):
        p0, p1, p2, p3 = p[i - 1], p[i], p[(i + 1) % n], p[(i + 2) % n]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)
        d += f"C{f(c1[0])},{f(c1[1])} {f(c2[0])},{f(c2[1])} {f(p2[0])},{f(p2[1])} "
    return d + "Z"


def splatter(spots, seed=1, color="#000"):
    """Ink splats: a blob with satellite drops and a few streaks per (cx, cy, size)."""
    rnd = random.Random(seed)
    out = ""
    for cx, cy, s in spots:
        n = 9
        p = [(cx + s * math.cos(2 * math.pi * i / n) * rnd.uniform(0.55, 1.15),
              cy + s * math.sin(2 * math.pi * i / n) * rnd.uniform(0.55, 1.15)) for i in range(n)]
        out += f'<path d="{smooth(p)}" fill="{color}"/>'
        for _ in range(rnd.randint(7, 13)):
            a, d = rnd.uniform(0, 2 * math.pi), s * rnd.uniform(1.3, 2.6)
            out += f'<circle cx="{f(cx + d * math.cos(a))}" cy="{f(cy + d * math.sin(a))}" r="{f(s * rnd.uniform(0.06, 0.22))}" fill="{color}"/>'
        for _ in range(rnd.randint(1, 3)):
            a, L = rnd.uniform(0, 2 * math.pi), s * rnd.uniform(1.4, 2.4)
            x1, y1 = cx + L * math.cos(a), cy + L * math.sin(a)
            nx, ny = -math.sin(a) * s * 0.12, math.cos(a) * s * 0.12
            out += poly([(cx + nx, cy + ny), (x1, y1), (cx - nx, cy - ny)], fill=color)
    return out


# round 4, Persona 3 set: a draped "Shadow"-style ghost (hood-like head, cloak arms, trailing wisps)
# and an oval face mask with round hollow eyes
SHADOW = ("M512,250 C620,250 690,330 690,430 C700,520 760,560 846,604 C800,640 762,650 730,662 "
          "C770,722 800,792 830,852 C762,822 722,802 690,792 C690,842 670,882 640,912 "
          "C612,862 592,832 570,822 C550,872 520,902 480,922 C470,872 460,842 440,822 "
          "C400,862 360,882 300,892 C330,842 340,792 340,742 C300,722 240,692 184,662 "
          "C250,622 300,582 330,522 C334,470 334,430 334,430 C334,330 404,250 512,250 Z")
OVAL_MASK = "M512,300 C580,300 616,360 616,430 C616,510 572,560 512,560 C452,560 408,510 408,430 C408,360 444,300 512,300 Z"
OVAL_EYES = '<circle cx="466" cy="414" r="27"/><circle cx="558" cy="414" r="27"/>'
OVAL_MOUTH = "M478,508 Q512,494 546,508 L546,516 Q512,504 478,516 Z"


# ---- round 5 ----
# MASK3 with smaller triangle eyes that stay clear of the mask's bottom edge
MASK3_EYE_SM_L = [(314, 418), (470, 440), (420, 478)]
MASK3_EYE_SM_R = mirror(MASK3_EYE_SM_L, 514)
# flame-wing mask: MASK3 whose wings break into sharp straight-cut flame tongues
_half5 = [(514, 420), (470, 398), (380, 384), (262, 296), (318, 380), (196, 318), (262, 400), (150, 360),
          (232, 430), (300, 488), (430, 502), (514, 464)]
MASK5 = _half5 + mirror(_half5[1:-1], 514)[::-1]
# white flame inlays along each wing (the noisier version)
MASK5_INLAY_L = [(440, 410), (350, 396), (250, 318), (310, 394), (226, 352), (300, 408)]
MASK5_INLAY_R = mirror(MASK5_INLAY_L, 514)
# angular but calm eyes for the 04 and 08 ghosts (nearly level top edge, straight cuts)
CALM_EYE_L = [(350, 404), (486, 414), (474, 452), (366, 448)]


def splatter2(spots, seed=1, color="#000"):
    """Spikier ink splats: a smooth core with a few long sharp spikes, droplets and streak trails."""
    rnd = random.Random(seed)
    out = ""
    for cx, cy, s in spots:
        n = 11
        p = []
        for i in range(n):
            a = 2 * math.pi * i / n + rnd.uniform(-0.15, 0.15)
            r = s * rnd.uniform(0.6, 1.05)
            p.append((cx + r * math.cos(a), cy + r * math.sin(a)))
        out += f'<path d="{smooth(p)}" fill="{color}"/>'
        for _ in range(rnd.randint(3, 6)):
            a, L, w = rnd.uniform(0, 2 * math.pi), s * rnd.uniform(1.3, 2.3), s * rnd.uniform(0.12, 0.25)
            nx, ny = -math.sin(a) * w, math.cos(a) * w
            out += poly([(cx + nx, cy + ny), (cx + L * math.cos(a), cy + L * math.sin(a)), (cx - nx, cy - ny)], fill=color)
            if rnd.random() < 0.6:
                d = L + s * rnd.uniform(0.2, 0.6)
                out += f'<circle cx="{f(cx + d * math.cos(a))}" cy="{f(cy + d * math.sin(a))}" r="{f(w * 0.9)}" fill="{color}"/>'
        for _ in range(rnd.randint(6, 12)):
            a, d = rnd.uniform(0, 2 * math.pi), s * rnd.uniform(1.2, 2.8)
            out += f'<circle cx="{f(cx + d * math.cos(a))}" cy="{f(cy + d * math.sin(a))}" r="{f(s * rnd.uniform(0.04, 0.16))}" fill="{color}"/>'
    return out


def rim_body(uid, d, base, rim, shade=None, shade_pts=None, shade2=None, shade2_pts=None, dx=-20, dy=-8):
    """Fill path d with a rim of colour `rim` along its lower-right edge: the body is drawn in `rim`, then
    again in `base` clipped to a copy shifted by (dx, dy). Cel-shade polygons are clipped the same way."""
    shades = ""
    if shade:
        shades += poly(shade_pts, fill=shade)
    if shade2:
        shades += poly(shade2_pts, fill=shade2)
    return (f'<clipPath id="{uid}a"><path d="{d}"/></clipPath>'
            f'<clipPath id="{uid}b"><path d="{d}" transform="translate({dx} {dy})"/></clipPath>'
            f'<path d="{d}" fill="{rim}"/>'
            f'<g clip-path="url(#{uid}a)"><g clip-path="url(#{uid}b)">'
            f'<rect x="0" y="0" width="1024" height="1024" fill="{base}"/>{shades}</g></g>')


# ---- round 6 ----
# MASK3 made more triangular: the bottom corners of each wing raised towards the wing tip
_half6 = [(514, 420), (470, 398), (214, 344), (324, 458), (440, 512), (514, 478)]
MASK6 = _half6 + mirror(_half6[1:-1], 514)[::-1]
MASK6_EYE_L = [(322, 418), (472, 442), (424, 484)]
MASK6_EYE_R = mirror(MASK6_EYE_L, 514)
# flame-shaped eye holes: a rounded base by the nose, licking up and outward into a curled tip,
# with one small second tongue on the inner edge
FLAME_EYE_L = ("M468,474 C444,490 384,490 354,466 C330,446 318,422 290,392 C330,404 356,402 382,418 "
               "C390,402 402,394 422,390 C414,406 422,420 444,428 C460,436 474,450 468,474 Z")
FLAME_EYE_R = ("M560,474 C584,490 644,490 674,466 C698,446 710,422 738,392 C698,404 672,402 646,418 "
               "C638,402 626,394 606,390 C614,406 606,420 584,428 C568,436 554,450 560,474 Z")

GOLD_DEFS = """
<linearGradient id="gold" x1="0" y1="0" x2="1" y2="1">
  <stop offset="0" stop-color="#fff4cf"/><stop offset="0.22" stop-color="#e6b85c"/><stop offset="0.45" stop-color="#7a4a17"/>
  <stop offset="0.62" stop-color="#f3d27e"/><stop offset="0.8" stop-color="#a8692a"/><stop offset="1" stop-color="#ffe7a6"/>
</linearGradient>
<radialGradient id="glint" cx="0.5" cy="0.5" r="0.5">
  <stop offset="0" stop-color="#fff" stop-opacity="1"/><stop offset="0.35" stop-color="#fff6d8" stop-opacity="0.55"/>
  <stop offset="1" stop-color="#fff6d8" stop-opacity="0"/>
</radialGradient>
<radialGradient id="royalbg" cx="0.5" cy="0.45" r="0.75">
  <stop offset="0" stop-color="#ff2a1a"/><stop offset="0.6" stop-color="#d10010"/><stop offset="1" stop-color="#6e0008"/>
</radialGradient>
"""


def gold_ribbed(points, ribs=3, step=14):
    """Gold-filled polygon with ribbed bevel lines inset from its edge (like a stamped metal emblem)."""
    cx = sum(p[0] for p in points) / len(points)
    cy = sum(p[1] for p in points) / len(points)
    out = poly(points, fill="url(#gold)", stroke="#3a1f06", stroke_width=6, stroke_linejoin="miter")
    for i in range(1, ribs + 1):
        k = 1 - i * step / 180
        inset = [(cx + (x - cx) * k, cy + (y - cy) * k) for x, y in points]
        out += poly(inset, fill="none", stroke="#5c3510" if i % 2 else "#fff1c2", stroke_width=3, opacity="0.7")
    return out


def sparkle(cx, cy, r, angle=0):
    """Four-point star glint with a soft glow."""
    t = r * 0.07
    pts_ = []
    for k in range(8):
        a = math.radians(angle) + math.pi * k / 4
        rr = r if k % 2 == 0 else t
        pts_.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
    return (f'<circle cx="{f(cx)}" cy="{f(cy)}" r="{f(r * 0.45)}" fill="url(#glint)"/>'
            + poly(pts_, fill="#fff"))


# ---- round 7 ----
# halfway between MASK3 (round 5's 01/02) and MASK6 (round 6's 25)
_half7 = [(514, 420), (470, 398), (214, 344), (312, 473), (435, 507), (514, 471)]
MASK7 = _half7 + mirror(_half7[1:-1], 514)[::-1]
MASK7_EYE_L = [(318, 418), (471, 441), (422, 481)]
MASK7_EYE_R = mirror(MASK7_EYE_L, 514)
# eyes burning: an eye hole whose flame streams out past the outer corner of the mask in licking tongues
FLAME_OUT_L = "M470,442 C452,474 410,484 366,474 C300,458 230,400 176,336 C222,360 252,374 276,380 C262,366 258,352 262,334 C276,356 296,370 320,384 C322,370 330,358 344,350 C346,372 360,392 392,408 C420,418 450,426 470,442 Z"
FLAME_OUT_R = "M558,442 C576,474 618,484 662,474 C728,458 798,400 852,336 C806,360 776,374 752,380 C766,366 770,352 766,334 C752,356 732,370 708,384 C706,370 698,358 684,350 C682,372 668,392 636,408 C608,418 578,426 558,442 Z"


# ---- round 8 ----
def mirror_path(d, cx=514):
    """Mirror an absolute-coordinate path (M/L/C/Z with x,y pairs) across x = cx."""
    return re.sub(r"(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)", lambda m: f"{f(2 * cx - float(m.group(1)))},{m.group(2)}", d)


def tongue(p0, p1, p2, p3, w0, curl=150, curl_len=36, n=40):
    """A flame tongue as a closed polygon: a cubic-bezier spine whose width tapers from w0 to 0, ending in a
    hook that curls by `curl` degrees (positive turns clockwise on screen) over curl_len px."""
    def bez(t):
        u = 1 - t
        return (u ** 3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
                u ** 3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1])
    spine = [bez(i / n) for i in range(n + 1)]
    # hook: keep walking from the tip while turning
    (ax, ay), (bx, by) = spine[-2], spine[-1]
    ang = math.atan2(by - ay, bx - ax)
    steps = 14
    x, y = bx, by
    for i in range(1, steps + 1):
        ang += math.radians(curl) / steps
        x, y = x + curl_len / steps * math.cos(ang), y + curl_len / steps * math.sin(ang)
        spine.append((x, y))
    total = len(spine) - 1
    left, right = [], []
    for i, (px, py) in enumerate(spine):
        qx, qy = spine[min(i + 1, total)]
        rx, ry = spine[max(i - 1, 0)]
        dx, dy = qx - rx, qy - ry
        L = math.hypot(dx, dy) or 1
        nx, ny = -dy / L, dx / L
        w = w0 * (1 - i / total) ** 0.8 / 2
        left.append((px + nx * w, py + ny * w))
        right.append((px - nx * w, py - ny * w))
    return left + right[::-1]


# burning eye for MASK7's left wing, built from tongues streaming towards the wing tip (mirror for the right)
FLAME_TONGUES_L = [
    tongue((452, 452), (394, 448), (322, 420), (270, 388), 44, curl=210, curl_len=52),
    tongue((440, 474), (392, 482), (346, 468), (322, 442), 28, curl=200, curl_len=34),
    tongue((462, 434), (434, 420), (404, 412), (380, 404), 20, curl=190, curl_len=24),
]
FLAME_BASE_L = "M472,442 C458,474 430,488 398,484 C368,480 348,464 338,446 C372,436 420,430 472,442 Z"
