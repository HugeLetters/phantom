# Pairs sheet (iter/overview-pairs.png), used in round 11 to choose pairs: candidate light/dark pairs, each row showing the light icon on a light
# panel and the dark icon on a dark panel (as the Dock would in each Appearance), at 256 px plus
# 64 and 32 px renders at 2x (nearest neighbour).
import re
from lib import *

# Icons by design number, from the round folders (round 5 on, so numbers are unique)
ROOT = os.path.join(WORK, "..")
SRC = {x.split("-")[0]: os.path.join(ROOT, d, x[:-4]) for d in os.listdir(ROOT) if re.fullmatch(r"round\d+", d)
       for x in os.listdir(os.path.join(ROOT, d)) if x.endswith(".svg") and int(d[5:]) >= 5}
# (label, light, dark)
PAIRS = [
    ("46 + 82  (chosen)", "46", "82"),
    ("46 + 86  stars on black, deep-red burst", "46", "86"),
    ("46 + 87  stars on black, mid-red burst", "46", "87"),
    ("46 + 88  ember stars, mid-red burst", "46", "88"),
]
LIGHT_BG, DARK_BG = "#e8e8ea", "#1c1c1e"
ROW, PANEL, BIG = 400, 640, 256

for name in {p for _, l, d in PAIRS for p in (l, d)}:
    for size in (BIG, 64, 32):
        subprocess.run([RENDER, SRC[name] + ".svg",
                        os.path.join(OUT, f"pair-{name}-{size}.png"), str(size)], check=True)

w, h = 2 * PANEL + 40, len(PAIRS) * ROW
s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}">'
     f'<rect width="{w}" height="{h}" fill="#8e8f93"/>')
for i, (label, light, dark) in enumerate(PAIRS):
    y = i * ROW
    s += (f'<text x="20" y="{y + 50}" font-family="Helvetica Neue, Arial" font-weight="bold" '
          f'font-size="40" fill="#111">{label}</text>')
    for j, (name, bg, tag) in enumerate(((light, LIGHT_BG, "light"), (dark, DARK_BG, "dark"))):
        x = 20 + j * PANEL
        fg = "#111" if j == 0 else "#eee"
        s += f'<rect x="{x}" y="{y + 70}" width="{PANEL - 20}" height="{ROW - 90}" rx="20" fill="{bg}"/>'
        s += f'<image x="{x + 20}" y="{y + 90}" width="{BIG}" height="{BIG}" xlink:href="pair-{name}-{BIG}.png"/>'
        s += f'<image x="{x + 300}" y="{y + 110}" width="128" height="128" image-rendering="optimizeSpeed" xlink:href="pair-{name}-64.png"/>'
        s += f'<image x="{x + 460}" y="{y + 142}" width="64" height="64" image-rendering="optimizeSpeed" xlink:href="pair-{name}-32.png"/>'
        s += (f'<text x="{x + 300}" y="{y + 300}" font-family="Helvetica Neue, Arial" font-size="32" '
              f'fill="{fg}">{tag}: {name}</text>')
s += "</svg>"
p = os.path.join(OUT, "overview-pairs.svg")
open(p, "w").write(s)
subprocess.run([RENDER, p, os.path.join(OUT, "overview-pairs.png"), str(w)], check=True)
