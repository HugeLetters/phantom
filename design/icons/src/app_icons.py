# Shippable app icons: renders every iconset size of each pick from picks/<id>.svg, packs it into
# ../app/<id>.icns with iconutil, and writes ../app/app-icons.json, the manifest the app reads.
# Also writes iter/app-check.png: each .icns's own 16 and 32 px images (and 32 at 6x) on light and dark.
import json
import shutil
import tempfile
from lib import *

ICONS_DIR = os.path.join(WORK, "..")
APP = os.path.join(ICONS_DIR, "app")

# (id, name, theme, design number) in picker order; the source is picks/<id>.svg
ICONS = [
    ("phantom", "Phantom", "phantom", 74),
    ("dark-hour", "Dark Hour", "persona-3-reload", 36),
    ("full-moon", "Full Moon", "persona-3-reload", 37),
    ("memento-mori-dark-hour", "Memento Mori (Dark Hour)", "persona-3-reload", 79),
    ("memento-mori", "Memento Mori", "persona-3-reload", 40),
    ("sea-of-souls-dark-hour", "Sea of Souls (Dark Hour)", "persona-3-reload", 80),
    ("sea-of-souls", "Sea of Souls", "persona-3-reload", 55),
    ("phantom-thief", "Phantom Thief", "persona-5", 46),
    ("phantom-thief-alt", "Phantom Thief (Alt)", "persona-5", 82),
    ("phantom-thief-alt-2", "Phantom Thief (Alt 2)", "persona-5", 87),
    ("calling-card", "Calling Card", "persona-5", 51),
    ("calling-card-alt", "Calling Card (Alt)", "persona-5", 50),
    ("calling-card-alt-2", "Calling Card (Alt 2)", "persona-5", 84),
    ("black-mask", "Black Mask", "persona-5", 85),
]
DEFAULT = "phantom"  # compiled into the bundle; the only one macOS 26 restyles for dark and tinted
# designed light/dark pairs as (dark, light)
PAIRS = [
    ("dark-hour", "full-moon"),
    ("memento-mori-dark-hour", "memento-mori"),
    ("sea-of-souls-dark-hour", "sea-of-souls"),
    ("phantom-thief-alt", "phantom-thief"),
    ("phantom-thief-alt-2", "phantom-thief"),
    ("calling-card", "black-mask"),
]
# each theme's own icon, used by "Follow theme": one icon, or a (dark, light) pair
THEMES = {
    "phantom": "phantom",
    "persona-3-reload": ("dark-hour", "full-moon"),
    "persona-5": ("calling-card", "black-mask"),
}
# iconset file name -> pixel size
ICONSET = [(f"icon_{s}x{s}{'@2x' if x == 2 else ''}.png", s * x) for s in (16, 32, 128, 256, 512) for x in (1, 2)]

NAME = {i: n for i, n, _, _ in ICONS}


def side(v):
    return {"icon": v} if isinstance(v, str) else {"dark": v[0], "light": v[1]}


def manifest():
    return {
        "default": DEFAULT,
        "icons": [{"id": i, "name": n, "theme": t, "file": f"{i}.icns"} for i, n, t, _ in ICONS],
        "pairs": [{"name": f"{NAME[d]} / {NAME[l]}", "dark": d, "light": l} for d, l in PAIRS],
        "themes": {t: side(v) for t, v in THEMES.items()},
    }


def build_icns(id_):
    src = os.path.join(ICONS_DIR, "picks", id_ + ".svg")
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, id_ + ".iconset")
        os.mkdir(iconset)
        for fname, px in ICONSET:
            subprocess.run([RENDER, src, os.path.join(iconset, fname), str(px)], check=True)
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(APP, id_ + ".icns")], check=True)


def check_sheet():
    """Unpack each .icns again and lay out its 16 and 32 px images, so the check is of what ships."""
    tmp = tempfile.mkdtemp()
    cw, rh = 330, 130
    s = ""
    for k, (id_, name, _, num) in enumerate(ICONS):
        iconset = os.path.join(tmp, id_ + ".iconset")
        subprocess.run(["iconutil", "-c", "iconset", os.path.join(APP, id_ + ".icns"), "-o", iconset], check=True)
        x, y = (k % 4) * cw, (k // 4) * (2 * rh + 50)
        s += (f'<text x="{x + 10}" y="{y + 34}" font-family="Helvetica Neue, Arial" font-size="22" fill="#111">'
              f'<tspan font-weight="bold">{num}</tspan> {name}</text>')
        for j, bg in enumerate(("#ececec", "#1e1e1e")):
            yy = y + 50 + j * rh
            s += f'<rect x="{x}" y="{yy}" width="{cw - 10}" height="{rh}" fill="{bg}"/>'
            s += f'<image x="{x + 20}" y="{yy + 57}" width="16" height="16" xlink:href="{iconset}/icon_16x16.png"/>'
            s += f'<image x="{x + 56}" y="{yy + 49}" width="32" height="32" xlink:href="{iconset}/icon_32x32.png"/>'
            s += f'<image x="{x + 110}" y="{yy + 49}" width="32" height="32" xlink:href="{iconset}/icon_16x16@2x.png"/>'
            if j == 0:
                s += (f'<image x="{x + 170}" y="{yy + 5}" width="120" height="120" image-rendering="optimizeSpeed" '
                      f'xlink:href="{iconset}/icon_16x16.png"/>')
            else:
                s += (f'<image x="{x + 170}" y="{yy + 5}" width="120" height="120" image-rendering="optimizeSpeed" '
                      f'xlink:href="{iconset}/icon_32x32.png"/>')
    rows = math.ceil(len(ICONS) / 4)
    w, h = 4 * cw, rows * (2 * rh + 50)
    s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}">'
         f'<rect width="{w}" height="{h}" fill="#8e8f93"/>{s}</svg>')
    p = os.path.join(OUT, "app-check.svg")
    open(p, "w").write(s)
    subprocess.run([RENDER, p, os.path.join(OUT, "app-check.png"), str(w)], check=True)
    shutil.rmtree(tmp)


if __name__ == "__main__":
    os.makedirs(APP, exist_ok=True)
    for id_, *_ in ICONS:
        build_icns(id_)
    with open(os.path.join(APP, "app-icons.json"), "w") as fh:
        json.dump(manifest(), fh, indent=2)
        fh.write("\n")
    check_sheet()
