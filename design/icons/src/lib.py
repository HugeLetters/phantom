import math, random, subprocess, os, sys

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
